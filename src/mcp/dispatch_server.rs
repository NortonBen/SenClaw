//! Dispatch MCP server. Port target: src-old/mcp/dispatch-server.ts
//!
//! Tools: list_agents, create_parent, create_parent_and_run, dispatch_task, dispatch_all_tasks.
//! Manages DAG task orchestration via a shared state file (`dispatch-state.json`)
//! with file-based locking — identical semantics to the TS DispatchBridge.

use anyhow::Context;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::mcp::schedule_server::ToolResult;

use rmcp::ServiceExt;

// State types are owned by the daemon-side `DispatchBridge` so the wire format
// stays in lock-step with what the Web Agent Console consumes.
pub use crate::agent::dispatch_bridge::{
    DispatchAgent, DispatchParent, DispatchState, DispatchTask, DispatchTaskStatus,
};

// ===== Persona type (lightweight, shadows PersonaRegistry) =====

#[derive(Debug, Clone)]
pub struct PersonaInfo {
    pub name: String,
    pub description: String,
}

pub trait PersonaResolver: Send + Sync {
    fn list(&self) -> Vec<PersonaInfo>;
    fn get(&self, name: &str) -> Option<PersonaInfo>;
}

/// Default persona resolver that scans .md files from a directory.
pub struct FsPersonaResolver {
    personas: Vec<PersonaInfo>,
}

impl FsPersonaResolver {
    pub fn from_dir(dir: &Path) -> Self {
        let mut personas = Vec::new();
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |e| e == "md") {
                    let mut name = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("unknown")
                        .to_owned();
                    let mut description = String::from("(no description)");

                    if let Ok(content) = fs::read_to_string(&path) {
                        let mut in_frontmatter = false;
                        let mut found_fm = false;
                        for line in content.lines() {
                            let trimmed = line.trim();
                            if trimmed == "---" {
                                if in_frontmatter {
                                    break;
                                } else if !found_fm {
                                    in_frontmatter = true;
                                    found_fm = true;
                                    continue;
                                }
                            }
                            if in_frontmatter {
                                if trimmed.starts_with("name:") {
                                    name = trimmed["name:".len()..]
                                        .trim()
                                        .trim_matches(&['"', '\''][..])
                                        .to_owned();
                                } else if trimmed.starts_with("description:") {
                                    description = trimmed["description:".len()..]
                                        .trim()
                                        .trim_matches(&['"', '\''][..])
                                        .to_owned();
                                }
                            } else if !found_fm && !trimmed.is_empty() && !trimmed.starts_with('#')
                            {
                                description = trimmed.trim_matches(&['"', '\''][..]).to_owned();
                                break;
                            }
                        }
                    }
                    personas.push(PersonaInfo { name, description });
                }
            }
        }
        Self { personas }
    }
}

impl PersonaResolver for FsPersonaResolver {
    fn list(&self) -> Vec<PersonaInfo> {
        self.personas.clone()
    }
    fn get(&self, name: &str) -> Option<PersonaInfo> {
        self.personas.iter().find(|p| p.name == name).cloned()
    }
}

// ===== DispatchServer =====

pub struct DispatchServer {
    state_path: PathBuf,
    admin_folder: String,
    persona_resolver: Option<Box<dyn PersonaResolver>>,
}

impl DispatchServer {
    pub fn new(
        state_path: &Path,
        admin_folder: &str,
        persona_resolver: Option<Box<dyn PersonaResolver>>,
    ) -> Self {
        Self {
            state_path: state_path.to_path_buf(),
            admin_folder: admin_folder.to_owned(),
            persona_resolver,
        }
    }

    // ===== File helpers — share the daemon-side lock implementation =====

    fn read_state(&self) -> DispatchState {
        crate::agent::dispatch_bridge::read_state_file(&self.state_path).unwrap_or_default()
    }

    fn modify_state(&self, f: impl FnOnce(&mut DispatchState)) {
        match crate::agent::dispatch_bridge::modify_state_file(&self.state_path, f) {
            Ok(state) => {
                let task_count = state.parents.iter().map(|p| p.tasks.len()).sum::<usize>();
                tracing::info!(
                    "[McpDispatchServer] wrote dispatch state path={} parents={} tasks={} \
                     (daemon DispatchBridge will broadcast on next poll)",
                    self.state_path.display(),
                    state.parents.len(),
                    task_count
                );
            }
            Err(e) => {
                tracing::warn!(
                    "[McpDispatchServer] failed to write dispatch state path={}: {e}",
                    self.state_path.display()
                );
            }
        }
    }

    fn next_id(state: &mut DispatchState, prefix: char) -> String {
        state.seq += 1;
        let date = chrono::Utc::now().format("%Y%m%d");
        format!("{}-{}-{:04}", prefix, date, state.seq)
    }

    fn read_admin_workspace(&self) -> Option<String> {
        let state_file = dirs::home_dir()?
            .join(".senclaw")
            .join(format!("workspace-state-{}.json", self.admin_folder));
        let raw = fs::read_to_string(&state_file).ok()?;
        let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
        v.get("currentDir")
            .and_then(|d| d.as_str())
            .map(|s| s.to_owned())
    }

    fn resolve_agent(&self, state: &DispatchState, agent_name: &str) -> Option<ResolvedAgent> {
        let trimmed = agent_name.trim();
        if trimmed.is_empty() {
            return None;
        }

        // Explicit virtual: persona:{name}
        if let Some(persona_name) = trimmed.strip_prefix("persona:") {
            let persona_name = persona_name.trim();
            if persona_name.is_empty() {
                return None;
            }
            if self.persona_resolver.as_ref()?.get(persona_name).is_some() {
                return Some(ResolvedAgent {
                    id: format!("persona:{persona_name}"),
                    jid: String::new(),
                    is_virtual: true,
                    persona_name: Some(persona_name.to_owned()),
                });
            }
            return None;
        }

        // Persistent agent from daemon-synced dispatch state
        let lower = trimmed.to_lowercase();
        if let Some(a) = state.agents.iter().find(|a| {
            a.name.to_lowercase() == lower || a.id.to_lowercase() == lower
        }) {
            return Some(ResolvedAgent {
                id: a.id.clone(),
                jid: a.jid.clone(),
                is_virtual: false,
                persona_name: None,
            });
        }

        // Bare name matches a virtual persona (LLMs often omit the persona: prefix)
        if let Some(resolver) = &self.persona_resolver {
            if let Some(p) = resolver
                .list()
                .iter()
                .find(|p| p.name.eq_ignore_ascii_case(trimmed))
            {
                return Some(ResolvedAgent {
                    id: format!("persona:{}", p.name),
                    jid: String::new(),
                    is_virtual: true,
                    persona_name: Some(p.name.clone()),
                });
            }
        }

        None
    }

    // ===== DAG cycle detection (DFS) =====

    fn detect_cycle(tasks: &[(String, Vec<String>)]) -> Option<String> {
        let mut visited = HashSet::new();
        let mut in_stack = HashSet::new();

        fn dfs(
            label: &str,
            tasks: &[(String, Vec<String>)],
            visited: &mut HashSet<String>,
            in_stack: &mut HashSet<String>,
        ) -> bool {
            if in_stack.contains(label) {
                return true;
            }
            if visited.contains(label) {
                return false;
            }
            visited.insert(label.to_string());
            in_stack.insert(label.to_string());
            if let Some((_, deps)) = tasks.iter().find(|(l, _)| l == label) {
                for dep in deps {
                    if dfs(dep, tasks, visited, in_stack) {
                        return true;
                    }
                }
            }
            in_stack.remove(label);
            false
        }

        for (label, _) in tasks {
            if !visited.contains(label.as_str()) && dfs(label, tasks, &mut visited, &mut in_stack) {
                return Some(label.clone());
            }
        }
        None
    }

    /// Topological order of task labels (deps before dependents). DAG must be acyclic.
    pub(crate) fn topo_task_order(tasks: &[DispatchTask]) -> Result<Vec<String>, String> {
        let labels: HashSet<&str> = tasks.iter().map(|t| t.label.as_str()).collect();
        for t in tasks {
            for d in &t.depends_on {
                if !labels.contains(d.as_str()) {
                    return Err(format!(
                        "Task \"{}\" depends on unknown label \"{d}\"",
                        t.label
                    ));
                }
            }
        }
        let mut remaining: HashSet<String> = tasks.iter().map(|t| t.label.clone()).collect();
        let mut order = Vec::new();
        while !remaining.is_empty() {
            let mut pick: Option<String> = None;
            for t in tasks {
                if !remaining.contains(&t.label) {
                    continue;
                }
                if t.depends_on.iter().all(|d| !remaining.contains(d)) {
                    pick = Some(t.label.clone());
                    break;
                }
            }
            let Some(label) = pick else {
                return Err(
                    "Dependency cycle or unsatisfiable dependsOn — fix the DAG".to_string(),
                );
            };
            remaining.remove(&label);
            order.push(label);
        }
        Ok(order)
    }

    // ===== list_agents =====

    pub fn list_agents(&self) -> ToolResult {
        let state = self.read_state();
        let mut lines: Vec<String> = Vec::new();

        if !state.agents.is_empty() {
            lines.push("**Persistent Agents:**".into());
            for a in &state.agents {
                let channel = if a.channel.is_empty() {
                    "web-only"
                } else {
                    a.channel.as_str()
                };
                lines.push(format!("- {} (id: {}, channel: {})", a.name, a.id, channel));
            }
        }

        if let Some(resolver) = &self.persona_resolver {
            let personas = resolver.list();
            if !personas.is_empty() {
                if !lines.is_empty() {
                    lines.push(String::new());
                }
                lines.push("**Virtual Personas:**".into());
                for p in &personas {
                    let desc = p.description.trim_matches(&['"', '\''][..]);
                    lines.push(format!("- persona:{} — {}", p.name, desc));
                }
            }
        }

        if lines.is_empty() {
            return ToolResult::ok("No agents or personas registered.".into());
        }
        ToolResult::ok(lines.join("\n"))
    }

    // ===== create_parent =====

    pub fn create_parent(
        &self,
        goal: &str,
        tasks: Vec<DispatchTaskInput>,
        timeout_seconds: Option<u64>,
    ) -> ToolResult {
        let timeout = timeout_seconds.unwrap_or(900);

        // Fill and validate labels
        let mut normalized: Vec<(String, String, String, Vec<String>)> = Vec::new();
        let mut errors: Vec<String> = Vec::new();
        let mut label_set = HashSet::new();

        for (i, t) in tasks.iter().enumerate() {
            let label = if t.label.trim().is_empty() {
                format!("task-{i}")
            } else {
                t.label.clone()
            };
            if label_set.contains(&label) {
                errors.push(format!("Duplicate label: \"{label}\""));
            } else {
                label_set.insert(label.clone());
            }
            normalized.push((
                label,
                t.agent_name.clone(),
                t.prompt.clone(),
                t.depends_on.clone(),
            ));
        }

        // Validate dependsOn references
        for (label, _, _, deps) in &normalized {
            for dep in deps {
                if !label_set.contains(dep) {
                    errors.push(format!(
                        "Task \"{label}\" depends on unknown label: \"{dep}\""
                    ));
                }
            }
        }

        if !errors.is_empty() {
            return ToolResult::err(format!(
                "Error:\n{}",
                errors
                    .iter()
                    .map(|e| format!("  - {e}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }

        // DAG cycle detection
        let dag_input: Vec<(String, Vec<String>)> = normalized
            .iter()
            .map(|(l, _, _, d)| (l.clone(), d.clone()))
            .collect();
        if let Some(cycle) = Self::detect_cycle(&dag_input) {
            return ToolResult::err(format!(
                "Error: Circular dependency detected involving task \"{cycle}\""
            ));
        }

        let mut parent_id = String::new();
        let mut is_queued = false;

        self.modify_state(|s| {
            // Resolve agents
            let mut resolved: Vec<(ResolvedAgent, String, String, Vec<String>)> = Vec::new();
            for (label, agent_name, prompt, deps) in &normalized {
                match self.resolve_agent(s, agent_name) {
                    Some(agent) => {
                        resolved.push((agent, label.clone(), prompt.clone(), deps.clone()));
                    }
                    None => {
                        errors.push(format!(
                            "Unknown agent: \"{agent_name}\" (for task \"{label}\")"
                        ));
                    }
                }
            }
            if !errors.is_empty() {
                return;
            }

            parent_id = Self::next_id(s, 'p');
            let now = chrono::Utc::now().to_rfc3339();

            let has_active = s
                .parents
                .iter()
                .any(|p| p.admin_folder == self.admin_folder && p.status == "active");
            is_queued = has_active;

            let parent = DispatchParent {
                id: parent_id.clone(),
                admin_folder: self.admin_folder.clone(),
                shared_workspace: if has_active {
                    None
                } else {
                    self.read_admin_workspace()
                },
                goal: goal.to_owned(),
                status: if has_active {
                    "queued".into()
                } else {
                    "active".into()
                },
                created_at: now.clone(),
                completed_at: None,
                tasks: resolved
                    .iter()
                    .map(|(agent, label, prompt, deps)| DispatchTask {
                        id: Self::next_id(s, 'd'),
                        label: label.clone(),
                        agent_id: agent.id.clone(),
                        agent_jid: agent.jid.clone(),
                        depends_on: deps.clone(),
                        status: DispatchTaskStatus::Registered,
                        prompt: prompt.clone(),
                        result: None,
                        timeout_seconds: timeout,
                        timeout_at: None,
                        created_at: now.clone(),
                        started_at: None,
                        completed_at: None,
                        is_virtual: agent.is_virtual,
                        persona_name: agent.persona_name.clone(),
                    })
                    .collect(),
            };
            s.parents.push(parent);
        });

        if !errors.is_empty() {
            return ToolResult::err(format!(
                "Error:\n{}",
                errors
                    .iter()
                    .map(|e| format!("  - {e}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }

        let task_lines = normalized
            .iter()
            .map(|(label, agent_name, _, deps)| {
                let deps_str = if deps.is_empty() {
                    ", no deps".into()
                } else {
                    format!(", depends on: [{}]", deps.join(", "))
                };
                format!("  - \"{label}\" (agent: {agent_name}{deps_str})")
            })
            .collect::<Vec<_>>()
            .join("\n");

        let status_note = if is_queued {
            "Status: QUEUED (another dispatch is active; this will start automatically when it completes)"
        } else {
            "Status: ACTIVE (starting immediately)"
        };
        tracing::info!(
            "[McpDispatchServer] create_parent parent_id={parent_id} status={} tasks={} goal_len={}",
            if is_queued { "queued" } else { "active" },
            normalized.len(),
            goal.len()
        );

        let body = if is_queued {
            format!(
                "Parent task created: {parent_id}\n{status_note}\nTasks:\n{task_lines}\n\n\
                 ⚠️ **QUEUED workflow** — another dispatch is still active. **Do not** call \
                 `mcp__dispatch__all_tasks` or `mcp__dispatch__task` for this parent on this turn; \
                 that would block until subtasks finish, but queued parents only start after you finish — deadlock.\n\n\
                 **What to do:** end this turn now. When this workflow becomes ACTIVE, use **mcp__dispatch__all_tasks** \
                 with `{{\"parentId\":\"{parent_id}\"}}` (or **create_parent_and_run** next time for one-shot create+wait).\n\
                 Do **not** invent task results from the goal alone."
            )
        } else {
            format!(
                "Parent task created: {parent_id}\n{status_note}\nTasks:\n{task_lines}\n\n\
                 IMPORTANT — Subtasks are running in the daemon; you do **not** have their results until you call a wait tool.\n\
                 • **Preferred:** call **mcp__dispatch__all_tasks** now with JSON `{{\"parentId\":\"{parent_id}\"}}` (optional timeoutSeconds).\n\
                 • **One-shot next time:** use **mcp__dispatch__create_parent_and_run** to create this parent and wait for every task in one call.\n\
                 • Or call **mcp__dispatch__task** per task label if you only need some outputs.\n\
                 Do **not** invent task results from the goal alone."
            )
        };
        ToolResult::ok(body)
    }

    pub(crate) fn parse_parent_id_from_create_output(content: &str) -> Option<String> {
        for line in content.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("Parent task created:") {
                let id = rest.trim();
                if !id.is_empty() {
                    return Some(id.to_string());
                }
            }
        }
        None
    }

    /// [`create_parent`](Self::create_parent) then [`dispatch_all_tasks`](Self::dispatch_all_tasks) — one MCP round-trip.
    pub async fn create_parent_and_run(
        &self,
        goal: &str,
        tasks: Vec<DispatchTaskInput>,
        timeout_seconds: Option<u64>,
    ) -> ToolResult {
        let created = self.create_parent(goal, tasks, timeout_seconds);
        if created.is_error {
            return created;
        }
        let Some(pid) = Self::parse_parent_id_from_create_output(&created.content) else {
            return ToolResult::err(format!(
                "Internal error: could not parse parent id from create_parent output:\n{}",
                created.content
            ));
        };
        let mut out = created.content;
        out.push_str("\n\n---\nWaiting for all tasks (mcp__dispatch__all_tasks)…\n\n");
        let run = self.dispatch_all_tasks(&pid, timeout_seconds).await;
        if run.is_error {
            return ToolResult::err(format!("{out}\n{}", run.content));
        }
        out.push_str(&run.content);
        ToolResult::ok(out)
    }

    // ===== dispatch_task =====

    /// Waiting on a **queued** parent deadlocks when the same admin agent holds an active
    /// dispatch turn (e.g. Cowork lead): subtasks never start until the current turn ends,
    /// but this tool blocks the turn until subtasks complete.
    fn reject_wait_if_parent_queued(&self, parent_id: &str) -> Option<ToolResult> {
        let state = self.read_state();
        let p = state.parents.iter().find(|p| p.id == parent_id)?;
        if p.status != "queued" {
            return None;
        }
        Some(ToolResult::err(format!(
            "Parent `{parent_id}` is QUEUED — another dispatch is still active for admin `{}`.\n\n\
             **Deadlock:** subtasks under a queued parent are not scheduled until that active workflow finishes, \
             but `dispatch_task` / `dispatch_all_tasks` blocks this turn until those subtasks complete.\n\n\
             **What to do:** End this turn after `create_parent` only (no `all_tasks`). \
             Or finish/Cancel the other workflow first. \
             Or use `create_parent_and_run` only when no other dispatch is active for this admin.\n\
             (Cowork/DAG will pick up the queued parent automatically when the pipeline is free.)",
            self.admin_folder
        )))
    }

    pub async fn dispatch_task(
        &self,
        parent_id: &str,
        task_label: &str,
        timeout_seconds: Option<u64>,
    ) -> ToolResult {
        if let Some(e) = self.reject_wait_if_parent_queued(parent_id) {
            return e;
        }

        let start_task = {
            let state = self.read_state();
            let Some(p) = state.parents.iter().find(|p| p.id == parent_id) else {
                return ToolResult::err(format!("Parent not found: {parent_id}"));
            };
            p.tasks
                .iter()
                .find(|t| t.label == task_label)
                .cloned()
        };

        let start_task = match start_task {
            Some(t) => t,
            None => {
                return ToolResult::err(format!(
                    "Task not found: parent={parent_id} label=\"{task_label}\""
                ));
            }
        };

        let mut deadline = if let Some(ts) = timeout_seconds {
            Instant::now() + Duration::from_secs(ts)
        } else {
            Instant::now() + Duration::from_secs(start_task.timeout_seconds)
        };

        loop {
            if Instant::now() > deadline {
                return ToolResult::err(format!(
                    "Task \"{task_label}\" timed out waiting for result"
                ));
            }

            let state = self.read_state();
            let current = state
                .parents
                .iter()
                .find(|p| p.id == parent_id)
                .and_then(|p| p.tasks.iter().find(|t| t.label == task_label))
                .cloned();

            let current = match current {
                Some(t) => t,
                None => {
                    return ToolResult::err(format!(
                        "Task \"{task_label}\" disappeared from state file"
                    ));
                }
            };

            // After task starts, switch to task timeoutAt as deadline
            if timeout_seconds.is_none() {
                if let Some(ref timeout_at) = current.timeout_at {
                    if let Ok(t) = chrono::DateTime::parse_from_rfc3339(timeout_at) {
                        let task_deadline = t.timestamp_millis() as u64;
                        let now_ms = chrono::Utc::now().timestamp_millis() as u64;
                        if task_deadline > now_ms + 5000 {
                            // Only extend, never shorten
                            deadline = Instant::now()
                                + Duration::from_millis(task_deadline.saturating_sub(now_ms));
                        }
                    }
                }
            }

            match current.status {
                DispatchTaskStatus::Done => {
                    return ToolResult::ok(current.result.unwrap_or_default());
                }
                DispatchTaskStatus::Error => {
                    return ToolResult::err(format!(
                        "Task \"{task_label}\" failed (agent: {})",
                        current.agent_id
                    ));
                }
                DispatchTaskStatus::Timeout => {
                    return ToolResult::err(format!(
                        "Task \"{task_label}\" timed out (agent: {})",
                        current.agent_id
                    ));
                }
                _ => {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        }
    }

    /// Wait for every task under `parent_id` in dependency order (same wait semantics as
    /// [`dispatch_task`] per label). Returns one combined report or stops at the first failure.
    pub async fn dispatch_all_tasks(
        &self,
        parent_id: &str,
        timeout_seconds: Option<u64>,
    ) -> ToolResult {
        if let Some(e) = self.reject_wait_if_parent_queued(parent_id) {
            return e;
        }

        let parent_tasks = {
            let state = self.read_state();
            let Some(p) = state.parents.iter().find(|p| p.id == parent_id) else {
                return ToolResult::err(format!("Parent not found: {parent_id}"));
            };
            if p.tasks.is_empty() {
                return ToolResult::err(format!("Parent {parent_id} has no tasks"));
            }
            p.tasks.clone()
        };

        let order = match Self::topo_task_order(&parent_tasks) {
            Ok(o) => o,
            Err(e) => return ToolResult::err(e),
        };

        let mut sections: Vec<String> = vec![format!(
            "Parent `{parent_id}` — running {} task(s) in order: [{}]",
            order.len(),
            order.join(", ")
        )];

        for label in &order {
            let r = self.dispatch_task(parent_id, label, timeout_seconds).await;
            if r.is_error {
                let trail = sections.join("\n\n");
                return ToolResult::err(format!(
                    "{trail}\n\n---\nStopped on task `{label}`:\n{}",
                    r.content
                ));
            }
            sections.push(format!("### `{label}`\n{}", r.content));
        }

        sections.push("**All dispatch tasks completed.**".into());
        ToolResult::ok(sections.join("\n\n"))
    }
}

// ===== Helper types =====

struct ResolvedAgent {
    id: String,
    jid: String,
    is_virtual: bool,
    persona_name: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
pub struct DispatchTaskInput {
    #[serde(default)]
    pub label: String,
    #[serde(rename = "agentName")]
    pub agent_name: String,
    pub prompt: String,
    #[serde(default)]
    #[serde(rename = "dependsOn")]
    pub depends_on: Vec<String>,
}

// ===== MCP stdio server =====

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct CreateParentParams {
    goal: String,
    tasks: Vec<DispatchTaskInput>,
    #[serde(default)]
    #[serde(rename = "timeoutSeconds")]
    timeout_seconds: Option<u64>,
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct DispatchTaskParams {
    #[serde(rename = "parentId")]
    parent_id: String,
    #[serde(rename = "taskLabel")]
    task_label: String,
    #[serde(default)]
    #[serde(rename = "timeoutSeconds")]
    timeout_seconds: Option<u64>,
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct DispatchAllTasksParams {
    #[serde(rename = "parentId")]
    parent_id: String,
    #[serde(default)]
    #[serde(rename = "timeoutSeconds")]
    timeout_seconds: Option<u64>,
}

#[derive(Clone)]
struct McpDispatchServer {
    state_path: std::path::PathBuf,
    admin_folder: String,
    agents_config_dir: Option<String>,
}

impl McpDispatchServer {
    fn inner(&self) -> DispatchServer {
        let persona_resolver: Option<Box<dyn PersonaResolver>> =
            self.agents_config_dir.as_ref().map(|dir| {
                Box::new(FsPersonaResolver::from_dir(std::path::Path::new(dir)))
                    as Box<dyn PersonaResolver>
            });
        DispatchServer::new(&self.state_path, &self.admin_folder, persona_resolver)
    }
}

#[rmcp::tool_router(server_handler)]
impl McpDispatchServer {
    #[rmcp::tool(description = "List all registered agents and personas")]
    fn list_agents(&self) -> String {
        self.inner().list_agents().content
    }

    #[rmcp::tool(
        description = "Create a parent dispatch with multiple tasks. Subtasks start in the daemon; you MUST then call mcp__dispatch__all_tasks (or use create_parent_and_run) — never invent task results."
    )]
    fn create_parent(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            CreateParentParams,
        >,
    ) -> String {
        self.inner()
            .create_parent(&p.goal, p.tasks, p.timeout_seconds)
            .content
    }

    #[rmcp::tool(
        description = "Create a dispatch parent and block until every subtask finishes (dependency order). Same args as create_parent. Prefer this when the user wants the full pipeline without a second tool call."
    )]
    async fn create_parent_and_run(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            CreateParentParams,
        >,
    ) -> String {
        self.inner()
            .create_parent_and_run(&p.goal, p.tasks, p.timeout_seconds)
            .await
            .content
    }

    #[rmcp::tool(description = "Dispatch a task within a parent and wait for its result")]
    async fn dispatch_task(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            DispatchTaskParams,
        >,
    ) -> String {
        self.inner()
            .dispatch_task(&p.parent_id, &p.task_label, p.timeout_seconds)
            .await
            .content
    }

    #[rmcp::tool(
        description = "Run every task under a parent in dependency order and return combined results. Stops on first error. Prefer this over calling dispatch_task repeatedly."
    )]
    async fn dispatch_all_tasks(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            DispatchAllTasksParams,
        >,
    ) -> String {
        self.inner()
            .dispatch_all_tasks(&p.parent_id, p.timeout_seconds)
            .await
            .content
    }
}

/// Start the dispatch MCP server over stdio.
pub async fn run_stdio_server() -> anyhow::Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();

    let state_path = std::env::var("SENCLAW_DISPATCH_STATE_PATH")
        .context("SENCLAW_DISPATCH_STATE_PATH not set")?;
    let admin_folder =
        std::env::var("SENCLAW_ADMIN_FOLDER").context("SENCLAW_ADMIN_FOLDER not set")?;
    let agents_config_dir = std::env::var("SENCLAW_AGENTS_CONFIG_DIR").ok();

    let server = McpDispatchServer {
        state_path: std::path::PathBuf::from(state_path),
        admin_folder,
        agents_config_dir,
    };

    let service = server.serve(rmcp::transport::io::stdio()).await?;
    service.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::dispatch_bridge::DispatchTaskStatus;

    fn dummy_task(label: &str, depends_on: &[&str]) -> DispatchTask {
        DispatchTask {
            id: format!("id-{label}"),
            label: label.into(),
            agent_id: "agent".into(),
            agent_jid: "web:test".into(),
            depends_on: depends_on.iter().map(|s| (*s).to_string()).collect(),
            prompt: String::new(),
            status: DispatchTaskStatus::Registered,
            result: None,
            created_at: String::new(),
            started_at: None,
            timeout_seconds: 60,
            timeout_at: None,
            completed_at: None,
            is_virtual: false,
            persona_name: None,
        }
    }

    #[test]
    fn topo_task_order_linear_chain() {
        let tasks = vec![
            dummy_task("research", &[]),
            dummy_task("implement", &["research"]),
            dummy_task("review", &["implement"]),
        ];
        let order = DispatchServer::topo_task_order(&tasks).unwrap();
        assert_eq!(order, vec!["research", "implement", "review"]);
    }

    #[test]
    fn topo_task_order_unknown_dep_errors() {
        let tasks = vec![dummy_task("a", &["missing"])];
        assert!(DispatchServer::topo_task_order(&tasks).is_err());
    }

    #[test]
    fn parse_parent_id_from_create_output_works() {
        let s = "Parent task created: p-test-0001\nStatus: ACTIVE\n";
        assert_eq!(
            DispatchServer::parse_parent_id_from_create_output(s).as_deref(),
            Some("p-test-0001")
        );
    }

    #[test]
    fn detect_cycle_acyclic() {
        let tasks = vec![
            ("t1".into(), vec![]),
            ("t2".into(), vec!["t1".into()]),
            ("t3".into(), vec!["t1".into()]),
        ];
        assert!(DispatchServer::detect_cycle(&tasks).is_none());
    }

    #[test]
    fn detect_cycle_finds_cycle() {
        let tasks = vec![
            ("t1".into(), vec!["t3".into()]),
            ("t2".into(), vec!["t1".into()]),
            ("t3".into(), vec!["t2".into()]),
        ];
        assert!(DispatchServer::detect_cycle(&tasks).is_some());
    }

    #[test]
    fn detect_cycle_self_loop() {
        let tasks = vec![("t1".into(), vec!["t1".into()])];
        assert!(DispatchServer::detect_cycle(&tasks).is_some());
    }

    #[test]
    fn persona_resolver_from_dir() {
        let tmp = tempfile::TempDir::new().unwrap();
        fs::write(tmp.path().join("coder.md"), "# Coder\nWrites code").unwrap();
        fs::write(tmp.path().join("reviewer.md"), "# Reviewer\nReviews PRs").unwrap();
        let resolver = FsPersonaResolver::from_dir(tmp.path());
        let list = resolver.list();
        assert_eq!(list.len(), 2);
    }
}
