//! Cowork workspace lifecycle management.
//! Workspaces are multi-agent collaborative environments with task boards,
//! shared knowledge boards, inter-agent messaging, and recording.

pub mod prompt;

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use uuid::Uuid;

use crate::agent::dispatch_bridge::{DispatchBridge, DispatchTask, DispatchTaskStatus};
use crate::config::Config;
use crate::db::Db;
use crate::types::{
    AgentApi, CoworkBoardEntry, CoworkMember, CoworkMessage, CoworkTask, CoworkTemplate,
    CoworkWorkspace, GroupBinding, TemplateBoard, TemplateBoardSection, TemplateMember,
};

/// Payload emitted when a CoworkTask transitions to "done".
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskResultEvent {
    pub task_id: String,
    pub workspace_id: String,
    pub title: String,
    pub input_summary: Option<String>,
    pub result_output: Option<String>,
    pub references: Option<String>,
    pub artifacts: Option<String>,
    pub completed_at: Option<String>,
    pub output_validation: Option<OutputValidation>,
}

/// Validation result for task output against member's output format requirements.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputValidation {
    pub format_valid: bool,
    pub expected_format: Option<String>,
    pub required_sections_present: Vec<String>,
    pub required_sections_missing: Vec<String>,
    pub overall_compliant: bool,
}

#[inline]
fn cowork_handoff_result_has(haystack: &str, needle: &str) -> bool {
    let h = haystack.to_lowercase();
    let n = needle.to_lowercase();
    h.contains(&n)
}

/// Validate task output against member's output format requirements.
fn validate_output_format(
    output: &str,
    output_format_json: Option<&str>,
) -> Option<OutputValidation> {
    let output_format = output_format_json?;
    let fmt: serde_json::Value = serde_json::from_str(output_format).ok()?;

    let expected_format = fmt.get("format").and_then(|v| v.as_str()).map(|s| s.to_string());
    let required_sections: Vec<String> = fmt
        .get("requiredSections")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_default();

    // Validate format
    let format_valid = if let Some(ref expected) = expected_format {
        match expected.as_str() {
            "json" => {
                // Check if output is valid JSON
                serde_json::from_str::<serde_json::Value>(output).is_ok()
            }
            "markdown" | "plain" => true, // Text is always valid for markdown/plain
            _ => true, // Unknown format, assume valid
        }
    } else {
        true // No format specified, assume valid
    };

    // Check required sections
    let output_lower = output.to_lowercase();
    let required_sections_present: Vec<String> = required_sections
        .iter()
        .filter(|section| {
            let section_lower = section.to_lowercase();
            // Check if section header exists (e.g., "## Summary" or "Summary")
            output_lower.contains(&section_lower)
                || output_lower.contains(&format!("## {}", section_lower))
                || output_lower.contains(&format!("### {}", section_lower))
        })
        .cloned()
        .collect();

    let required_sections_missing: Vec<String> = required_sections
        .iter()
        .filter(|section| !required_sections_present.contains(section))
        .cloned()
        .collect();

    let overall_compliant = format_valid && required_sections_missing.is_empty();

    Some(OutputValidation {
        format_valid,
        expected_format,
        required_sections_present,
        required_sections_missing,
        overall_compliant,
    })
}

/// Parse `cowork:{workspace_id}:{member_id}` → `workspace_id`.
pub fn workspace_id_from_cowork_jid(jid: &str) -> Option<&str> {
    let mut parts = jid.splitn(3, ':');
    match (parts.next(), parts.next(), parts.next()) {
        (Some("cowork"), Some(ws), Some(_member)) if !ws.is_empty() => Some(ws),
        _ => None,
    }
}

/// Serialize workspace members for `SENCLAW_DISPATCH_COWORK_AGENTS_JSON` so the dispatch MCP
/// subprocess lists / resolves subtask assignees **only** from Cowork (not the global agent board or `persona:` files).
pub fn dispatch_cowork_agents_json_for_mcp(db: &Db, workspace_id: &str) -> Result<String> {
    let members = db.list_cowork_members(workspace_id)?;
    let rows: Vec<serde_json::Value> = members
        .iter()
        .map(|m| {
            let jid = m
                .jid
                .clone()
                .unwrap_or_else(|| format!("cowork:{workspace_id}:{}", m.member_id));
            serde_json::json!({
                "memberId": m.member_id,
                "role": m.role,
                "jid": jid,
            })
        })
        .collect();
    Ok(serde_json::Value::Array(rows).to_string())
}

pub struct CoworkManager {
    on_changed: Mutex<Option<Box<dyn Fn() + Send + 'static>>>,
    /// Fired when a task reaches "done" — carries the full result payload.
    on_task_result: Mutex<Option<Box<dyn Fn(TaskResultEvent) + Send + 'static>>>,
    /// Fired when workspace resources change (upsert/delete).
    on_resource_changed: Mutex<Option<Box<dyn Fn(String) + Send + 'static>>>,
    /// Optional dispatch bridge for DAG-based task orchestration.
    dispatch_bridge: Mutex<Option<Arc<DispatchBridge>>>,
    /// Maps DispatchTask IDs → (CoworkTask ID, workspace ID) for status sync.
    dispatch_task_map: Mutex<HashMap<String, (String, String)>>,
}

impl CoworkManager {
    pub fn new() -> Self {
        Self {
            on_changed: Mutex::new(None),
            on_task_result: Mutex::new(None),
            on_resource_changed: Mutex::new(None),
            dispatch_bridge: Mutex::new(None),
            dispatch_task_map: Mutex::new(HashMap::new()),
        }
    }

    pub fn set_on_changed(&self, cb: Box<dyn Fn() + Send + 'static>) {
        if let Ok(mut guard) = self.on_changed.lock() {
            *guard = Some(cb);
        }
    }

    pub fn set_on_task_result(&self, cb: Box<dyn Fn(TaskResultEvent) + Send + 'static>) {
        if let Ok(mut guard) = self.on_task_result.lock() {
            *guard = Some(cb);
        }
    }

    pub fn set_on_resource_changed(&self, cb: Box<dyn Fn(String) + Send + 'static>) {
        if let Ok(mut guard) = self.on_resource_changed.lock() {
            *guard = Some(cb);
        }
    }

    /// Inject the dispatch bridge for DAG-based task orchestration.
    /// When set, `process_user_message` routes tasks through the dispatch
    /// bridge instead of calling `send_to_cowork_agent` directly.
    pub fn set_dispatch_bridge(&self, bridge: Arc<DispatchBridge>) {
        *self.dispatch_bridge.lock().unwrap() = Some(bridge);
        self.fire_changed();
    }

    /// Get the dispatch bridge if set.
    pub fn get_dispatch_bridge(&self) -> Option<Arc<DispatchBridge>> {
        self.dispatch_bridge.lock().unwrap().clone()
    }

    fn fire_changed(&self) {
        if let Ok(guard) = self.on_changed.lock() {
            if let Some(ref cb) = *guard {
                cb();
            }
        }
    }

    fn fire_task_result(&self, evt: TaskResultEvent) {
        if let Ok(guard) = self.on_task_result.lock() {
            if let Some(ref cb) = *guard {
                cb(evt);
            }
        }
    }

    pub fn fire_resource_changed(&self, workspace_id: String) {
        if let Ok(guard) = self.on_resource_changed.lock() {
            if let Some(ref cb) = *guard {
                cb(workspace_id);
            }
        }
        self.fire_changed();
    }

    // ============================================================
    // Dispatch lifecycle — keep CoworkTask ↔ DispatchTask in sync
    // ============================================================

    /// Called when a dispatch task transitions status. Updates the corresponding
    /// CoworkTask status and fires cowork:changed.
    pub fn on_dispatch_task_lifecycle(
        &self,
        db: &Arc<Db>,
        dispatch_task_id: &str,
        new_status: &str,
        task_label: &str,
        _parent_goal: &str,
        dispatch_result: Option<&str>,
        agent_api: Option<Arc<dyn AgentApi>>,
        self_arc: Arc<CoworkManager>,
    ) {
        let (cowork_task_id, workspace_id) = {
            let map = self.dispatch_task_map.lock().unwrap();
            let Some((tid, wid)) = map.get(dispatch_task_id) else {
                return;
            };
            (tid.clone(), wid.clone())
        };

        let cowork_status = match new_status {
            "processing" => "in_progress",
            "done" => "done",
            "error" | "timeout" => "blocked",
            _ => return,
        };
        let now = chrono::Utc::now().to_rfc3339();

        // Treat empty result string same as None.
        let result_opt = dispatch_result.filter(|s| !s.trim().is_empty());

        let db_err = if cowork_status == "done" {
            db.update_cowork_task_result(
                &cowork_task_id,
                Some(task_label),
                result_opt,
                None,
                None,
                &now,
            )
            .err()
        } else {
            db.update_cowork_task(
                &cowork_task_id,
                None,
                None,
                Some(cowork_status),
                None,
                None,
                None,
                None,
                None,
                &now,
            )
            .err()
        };

        if let Some(e) = db_err {
            tracing::warn!(
                "[Cowork] Failed to update task {cowork_task_id} → {cowork_status}: {e}"
            );
        } else {
            tracing::info!(
                "[Cowork] Task {cowork_task_id} → {cowork_status} (dispatch: {dispatch_task_id})"
            );
            if cowork_status == "done" {
                // Validate output against member's output format requirements
                let output_validation = if let (Some(result), Ok(task_opt)) = (
                    result_opt,
                    db.get_cowork_task(&cowork_task_id),
                ) {
                    if let Some(task) = task_opt {
                        if let (Some(assignee), Ok(members)) = (
                            &task.assignee,
                            db.list_cowork_members(&workspace_id),
                        ) {
                            if let Some(member) = members.iter().find(|m| m.member_id == *assignee) {
                                validate_output_format(result, member.output_format.as_deref())
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                };

                self.fire_task_result(TaskResultEvent {
                    task_id: cowork_task_id.clone(),
                    workspace_id: workspace_id.clone(),
                    title: task_label.to_string(),
                    input_summary: Some(task_label.to_string()),
                    result_output: result_opt.map(|s| s.to_string()),
                    references: None,
                    artifacts: None,
                    completed_at: Some(now.clone()),
                    output_validation,
                });

                // Process handoff rules for the completed task's assignee.
                let agent_api_clone = agent_api.clone();
                if let Some(api) = agent_api_clone {
                    let self_arc_clone = Arc::clone(&self_arc);
                    self.process_handoff_rules(
                        db,
                        &workspace_id,
                        &cowork_task_id,
                        result_opt.unwrap_or(task_label),
                        &now,
                        api,
                        self_arc_clone,
                    );
                }
            }

            // Process task status triggers for all status changes
            let result_str = result_opt.map(|s| s.as_ref());
            let agent_api_for_triggers = agent_api.clone();
            let self_arc_for_triggers = Arc::clone(&self_arc);
            self.process_task_status_triggers(
                db,
                &workspace_id,
                &cowork_task_id,
                cowork_status,
                result_str,
                &now,
                agent_api_for_triggers,
                self_arc_for_triggers,
            );
        }

        self.fire_changed();
    }

    /// After a task completes, check the assignee's handoff_rules and create +
    /// dispatch follow-up tasks for the specified target members.
    ///
    /// Rule shape (JSON per item): `when` (e.g. `task_complete`), `to`, `type`,
    /// optional `only_if_result_contains` / `unless_result_contains` — substring
    /// gates on the completed task result (case-insensitive).
    fn process_handoff_rules(
        &self,
        db: &Arc<Db>,
        workspace_id: &str,
        completed_task_id: &str,
        result_content: &str,
        now: &str,
        agent_api: Arc<dyn AgentApi>,
        self_arc: Arc<CoworkManager>,
    ) {
        let completed_task = match db.get_cowork_task(completed_task_id) {
            Ok(Some(t)) => t,
            _ => return,
        };
        let assignee_id = match completed_task.assignee.as_deref() {
            Some(a) => a.to_string(),
            None => return,
        };
        let members = match db.list_cowork_members(workspace_id) {
            Ok(m) => m,
            Err(_) => return,
        };
        let assignee = match members.iter().find(|m| m.member_id == assignee_id) {
            Some(m) => m,
            None => return,
        };
        let handoff_rules_json = match assignee.handoff_rules.as_deref() {
            Some(r) => r.to_string(),
            None => return,
        };
        let rules: Vec<serde_json::Value> = match serde_json::from_str(&handoff_rules_json) {
            Ok(r) => r,
            Err(_) => return,
        };

        let mut followup_tasks = Vec::new();
        for rule in &rules {
            let when = rule["when"].as_str().unwrap_or("");
            if when != "task_complete" {
                continue;
            }
            let to = match rule["to"].as_str() {
                Some(t) => t,
                None => continue,
            };
            // Optional gates (case-insensitive substring match on task result):
            // - `unless_result_contains`: skip rule if this text appears (e.g. workstream done).
            // - `only_if_result_contains`: run rule only if this text appears (e.g. explicit handoff).
            if let Some(u) = rule["unless_result_contains"].as_str() {
                if !u.is_empty() && cowork_handoff_result_has(result_content, u) {
                    tracing::info!(
                        "[Cowork] Handoff rule → {to} skipped (unless_result_contains matched)"
                    );
                    continue;
                }
            }
            if let Some(o) = rule["only_if_result_contains"].as_str() {
                if !o.is_empty() && !cowork_handoff_result_has(result_content, o) {
                    tracing::info!(
                        "[Cowork] Handoff rule → {to} skipped (only_if_result_contains not in result)"
                    );
                    continue;
                }
            }
            let handoff_type = rule["type"].as_str().unwrap_or("handoff");
            // Find target member
            if !members.iter().any(|m| m.member_id == to) {
                tracing::warn!("[Cowork] Handoff target '{to}' not found in workspace {workspace_id}");
                continue;
            }
            let task_title = format!(
                "[handoff from {}] {}",
                assignee_id,
                if completed_task.title.len() > 60 { &completed_task.title[..60] } else { &completed_task.title }
            );
            let description = format!(
                "Handoff from {assignee_id}.\n\nOriginal task: {}\n\nResult:\n{result_content}",
                completed_task.title
            );
            match self.create_task(
                db,
                workspace_id,
                &task_title,
                Some(&description),
                Some(to),
                None,
                Some("high"),
                None,
                &assignee_id,
                None,
                now,
            ) {
                Ok(task) => {
                    tracing::info!(
                        "[Cowork] Handoff rule: created task '{}' → {to} (type={handoff_type})",
                        task.title
                    );
                    // Post a handoff message
                    let _ = db.insert_cowork_message(&CoworkMessage {
                        id: format!("cwmsg-{}", uuid::Uuid::new_v4().to_string().split('-').next().unwrap_or("0")),
                        workspace_id: workspace_id.to_string(),
                        from_member: assignee_id.clone(),
                        to_member: Some(to.to_string()),
                        message_type: handoff_type.to_string(),
                        content: format!("{assignee_id} → {to}: {}", completed_task.title),
                        attachments: None,
                        task_id: Some(task.id.clone()),
                        is_read: false,
                        created_at: now.to_string(),
                    });
                    followup_tasks.push(task);
                }
                Err(e) => {
                    tracing::error!("[Cowork] Failed to create handoff task: {e}");
                }
            }
        }

        if !followup_tasks.is_empty() {
            let _ = self.dispatch_cowork_tasks_batch(
                db,
                workspace_id,
                &members,
                &followup_tasks,
                &completed_task.title,
                Some((agent_api, Arc::clone(db))),
                self_arc,
            );
        }
    }

    // ============================================================
    // Orchestration — message → task decomposition
    // ============================================================

    /// Dispatch a cowork task to a workspace member agent for execution.
    /// Spawns a background task that calls process_and_wait and updates status.
    pub fn send_to_cowork_agent(
        &self,
        db: Arc<Db>,
        agent_api: Arc<dyn AgentApi>,
        workspace_id: &str,
        task: &CoworkTask,
        member: &CoworkMember,
        manager: Arc<CoworkManager>,
    ) {
        let task_id = task.id.clone();
        let member_id = member.member_id.clone();
        let workspace_id_owned = workspace_id.to_string();
        let jid = member
            .jid
            .clone()
            .unwrap_or_else(|| format!("cowork:{}:{}", workspace_id, member.member_id));
        let folder = member.member_id.clone();
        let _allowed_dirs = member.subdir.clone().map(|d| vec![d]);

        tracing::info!(
            "[Cowork] Dispatching task {} to agent {} (jid={})",
            task_id,
            member_id,
            jid
        );

        // Resolve context for prompt
        let workspace = match db.get_cowork_workspace(&workspace_id_owned) {
            Ok(Some(ws)) => ws,
            _ => return,
        };
        let board = db
            .get_cowork_board_entries(&workspace_id_owned, None)
            .unwrap_or_default();
        let all_tasks = db
            .list_cowork_tasks(&workspace_id_owned, None)
            .unwrap_or_default();

        // Find completed dependency tasks
        let dependent_results: Vec<CoworkTask> = if let Some(ref deps_json) = task.depends_on {
            if let Ok(dep_ids) = serde_json::from_str::<Vec<String>>(deps_json) {
                all_tasks
                    .into_iter()
                    .filter(|t| dep_ids.contains(&t.id) && t.status == "done")
                    .collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        let prompt = self::prompt::build_cowork_task_prompt(
            task,
            member,
            &workspace,
            &board,
            &dependent_results,
        );

        // Use workspace working_dir if set, otherwise root_dir for code/folder operations
        let workspace_dir = workspace
            .working_dir
            .as_ref()
            .filter(|d| !d.is_empty())
            .unwrap_or(&workspace.root_dir);

        let group = GroupBinding {
            jid: jid.clone(),
            folder: folder.clone(),
            name: format!("cowork-{member_id}"),
            channel: "web".to_string(),
            group_type: "cowork".to_string(),
            is_admin: false,
            requires_trigger: false,
            allowed_tools: None,
            allowed_paths: None,
            allowed_work_dirs: Some(vec![workspace_dir.clone()]),
            bot_token: None,
            max_messages: None,
            last_active: None,
            added_at: chrono::Utc::now().to_rfc3339(),
        };

        let task_title = task.title.clone();
        let db_clone = Arc::clone(&db);
        let agent_api_followup = Arc::clone(&agent_api);

        tokio::spawn(async move {
            // Mark in_progress
            let now = chrono::Utc::now().to_rfc3339();
            let _ = db_clone.update_cowork_task(
                &task_id,
                None, // title
                None, // description
                Some("in_progress"),
                None, // assignee
                None, // reviewer
                None, // priority
                None, // depends_on
                None, // attachments
                &now,
            );

            // Insert a status message
            let dispatch_msg_id = format!(
                "cwmsg-{}",
                Uuid::new_v4()
                    .to_string()
                    .split('-')
                    .next()
                    .unwrap_or("0000")
            );
            let _ = db_clone.insert_cowork_message(&CoworkMessage {
                id: dispatch_msg_id,
                workspace_id: workspace_id_owned.clone(),
                from_member: "system".into(),
                to_member: Some(member_id.clone()),
                message_type: "status".into(),
                content: format!("Dispatched task to {member_id}"),
                attachments: None,
                task_id: Some(task_id.clone()),
                is_read: false,
                created_at: now.clone(),
            });

            tracing::info!(
                "[Cowork] Task {task_id} → in_progress, calling process_and_wait for {jid}"
            );
            let result = agent_api.process_and_wait(&jid, &group, &prompt).await;

            // Capture last reply text before any cleanup.
            let reply_text = agent_api.get_last_reply_text(&jid);

            // Update status based on result
            let now2 = chrono::Utc::now().to_rfc3339();
            let new_status = if result.is_ok() { "done" } else { "blocked" };
            tracing::info!(
                "[Cowork] Task {task_id} → {new_status} (process_and_wait: {:?})",
                result.is_ok()
            );

            if new_status == "done" {
                // Persist result output — input_summary from task description/prompt,
                // result_output from the agent's last reply.
                let _ = db_clone.update_cowork_task_result(
                    &task_id,
                    Some(prompt.as_str()),
                    reply_text.as_deref(),
                    None,
                    None,
                    &now2,
                );
                // Broadcast task result event so UI can display without polling.
                // Validate output against member's output format requirements
                let output_validation = if let (Some(result), Ok(members)) = (
                    reply_text.as_deref(),
                    db_clone.list_cowork_members(&workspace_id_owned),
                ) {
                    if let Some(member) = members.iter().find(|m| m.member_id == member_id) {
                        validate_output_format(result, member.output_format.as_deref())
                    } else {
                        None
                    }
                } else {
                    None
                };

                manager.fire_task_result(TaskResultEvent {
                    task_id: task_id.clone(),
                    workspace_id: workspace_id_owned.clone(),
                    title: task_title.clone(),
                    input_summary: Some(prompt.clone()),
                    result_output: reply_text.clone(),
                    references: None,
                    artifacts: None,
                    completed_at: Some(now2.clone()),
                    output_validation,
                });
            } else {
                let _ = db_clone.update_cowork_task(
                    &task_id,
                    None,
                    None,
                    Some(new_status),
                    None,
                    None,
                    None,
                    None,
                    None,
                    &now2,
                );
            }

            // Insert a completion message
            let done_msg_id = format!(
                "cwmsg-{}",
                Uuid::new_v4()
                    .to_string()
                    .split('-')
                    .next()
                    .unwrap_or("0000")
            );
            let content = if new_status == "done" {
                format!("{member_id} completed task: {task_title}")
            } else {
                format!("{member_id} blocked on task: {task_title}")
            };
            let _ = db_clone.insert_cowork_message(&CoworkMessage {
                id: done_msg_id,
                workspace_id: workspace_id_owned.clone(),
                from_member: member_id.clone(),
                to_member: Some("user".into()),
                message_type: if new_status == "done" {
                    "result".into()
                } else {
                    "alert".into()
                },
                content: content.clone(),
                attachments: None,
                task_id: Some(task_id.clone()),
                is_read: false,
                created_at: now2.clone(),
            });

            // Process task status triggers for both done and blocked status
            manager.process_task_status_triggers(
                &db_clone,
                &workspace_id_owned,
                &task_id,
                new_status,
                reply_text.as_deref(),
                &now2,
                Some(Arc::clone(&agent_api_followup)),
                Arc::clone(&manager),
            );

            if new_status == "done" {
                if let Ok(members) = db_clone.list_cowork_members(&workspace_id_owned) {
                    let mut followup = Vec::new();
                    let msg_type = "result";
                    if manager
                        .collect_triggered_tasks(
                            &db_clone,
                            &workspace_id_owned,
                            &members,
                            &member_id,
                            msg_type,
                            &content,
                            &[],
                            &now2,
                            &mut followup,
                        )
                        .is_ok()
                        && !followup.is_empty()
                    {
                        let _ = manager.dispatch_cowork_tasks_batch(
                            &db_clone,
                            &workspace_id_owned,
                            &members,
                            &followup,
                            &content,
                            Some((Arc::clone(&agent_api_followup), Arc::clone(&db_clone))),
                            Arc::clone(&manager),
                        );
                    }
                }
            }

            manager.fire_changed();
        });
    }

    /// Match member triggers against an incoming message and append new tasks to `out`.
    fn collect_triggered_tasks(
        &self,
        db: &Db,
        workspace_id: &str,
        members: &[CoworkMember],
        from_user: &str,
        resolved_message_type: &str,
        content: &str,
        already_created: &[CoworkTask],
        now: &str,
        out: &mut Vec<CoworkTask>,
    ) -> Result<()> {
        for member in members {
            if let Some(ref triggers_json) = member.triggers {
                if let Ok(triggers) = serde_json::from_str::<Vec<serde_json::Value>>(triggers_json)
                {
                    for trigger in &triggers {
                        let trigger_type = trigger["type"].as_str().unwrap_or("");
                        let pool: Vec<&CoworkTask> =
                            already_created.iter().chain(out.iter()).collect();
                        let should_fire = match trigger_type {
                            "message_received" => {
                                let from_filter = trigger["from"].as_str();
                                let from_ok = from_filter.map_or(true, |f| f == from_user);
                                let type_filter = trigger["messageType"].as_str();
                                let type_ok = match type_filter {
                                    None => true,
                                    Some(mt) => resolved_message_type == mt,
                                };
                                from_ok && type_ok
                            }
                            "on_mention" => {
                                let from_filter = trigger["from"].as_str();
                                from_filter.map_or(false, |f| f == from_user)
                            }
                            "task_assigned" => pool
                                .iter()
                                .any(|t| t.assignee.as_deref() == Some(member.member_id.as_str())),
                            "task_status_changed" => false,
                            _ => false,
                        };
                        let duplicate_assignee = pool
                            .iter()
                            .any(|t| t.assignee.as_deref() == Some(member.member_id.as_str()));
                        if should_fire && !duplicate_assignee {
                            let task = self.create_task(
                                db,
                                workspace_id,
                                &format!(
                                    "[from {}] {}",
                                    from_user,
                                    if content.len() > 60 {
                                        &content[..60]
                                    } else {
                                        content
                                    }
                                ),
                                Some(content),
                                Some(&member.member_id),
                                None,
                                Some("medium"),
                                None,
                                from_user,
                                None,
                                now,
                            )?;
                            out.push(task);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Match member triggers against a task status change and append new tasks to `out`.
    fn collect_task_status_triggers(
        &self,
        db: &Db,
        workspace_id: &str,
        members: &[CoworkMember],
        task: &CoworkTask,
        new_status: &str,
        task_result: Option<&str>,
        already_created: &[CoworkTask],
        now: &str,
        out: &mut Vec<CoworkTask>,
    ) -> Result<()> {
        tracing::info!(
            "[Cowork] collect_task_status_triggers: checking {} members for task_status_changed triggers, new_status={}",
            members.len(),
            new_status
        );

        for member in members {
            tracing::debug!(
                "[Cowork] Checking member {} for triggers (has_triggers={})",
                member.member_id,
                member.triggers.is_some()
            );

            if let Some(ref triggers_json) = member.triggers {
                if let Ok(triggers) = serde_json::from_str::<Vec<serde_json::Value>>(triggers_json)
                {
                    tracing::debug!(
                        "[Cowork] Member {} has {} trigger(s)",
                        member.member_id,
                        triggers.len()
                    );

                    for trigger in &triggers {
                        let trigger_type = trigger["type"].as_str().unwrap_or("");
                        tracing::debug!(
                            "[Cowork] Evaluating trigger: type={}, status={:?}, to={:?}",
                            trigger_type,
                            trigger["status"].as_str(),
                            trigger["to"].as_str()
                        );

                        if trigger_type != "task_status_changed" {
                            continue;
                        }

                        let status_filter = trigger["status"].as_str();
                        let status_ok = status_filter.map_or(false, |s| s == new_status);
                        if !status_ok {
                            tracing::debug!(
                                "[Cowork] Trigger skipped: status filter {:?} doesn't match new_status {}",
                                status_filter,
                                new_status
                            );
                            continue;
                        }

                        let assignee_filter = trigger["assignee"].as_str();
                        let assignee_ok = assignee_filter.map_or(true, |a| {
                            task.assignee.as_deref().map_or(false, |ta| ta == a)
                        });
                        if !assignee_ok {
                            tracing::debug!(
                                "[Cowork] Trigger skipped: assignee filter {:?} doesn't match task assignee {:?}",
                                assignee_filter,
                                task.assignee
                            );
                            continue;
                        }

                        let to = match trigger["to"].as_str() {
                            Some(t) => t,
                            None => {
                                tracing::warn!("[Cowork] Trigger skipped: missing 'to' field");
                                continue;
                            }
                        };

                        if !members.iter().any(|m| m.member_id == to) {
                            tracing::warn!(
                                "[Cowork] Task status trigger target '{to}' not found in workspace {workspace_id}"
                            );
                            continue;
                        }

                        if let Some(result) = task_result {
                            if let Some(u) = trigger["unless_result_contains"].as_str() {
                                if !u.is_empty() && cowork_handoff_result_has(result, u) {
                                    tracing::info!(
                                        "[Cowork] Task status trigger → {to} skipped (unless_result_contains matched)"
                                    );
                                    continue;
                                }
                            }
                            if let Some(o) = trigger["only_if_result_contains"].as_str() {
                                if !o.is_empty() && !cowork_handoff_result_has(result, o) {
                                    tracing::info!(
                                        "[Cowork] Task status trigger → {to} skipped (only_if_result_contains not in result)"
                                    );
                                    continue;
                                }
                            }
                        }

                        let pool: Vec<&CoworkTask> =
                            already_created.iter().chain(out.iter()).collect();
                        let duplicate_assignee = pool
                            .iter()
                            .any(|t| t.assignee.as_deref() == Some(to));

                        if duplicate_assignee {
                            tracing::info!(
                                "[Cowork] Task status trigger → {to} skipped (duplicate assignee)"
                            );
                            continue;
                        }

                        let task_title = format!(
                            "[status: {} from {}] {}",
                            new_status,
                            task.assignee.as_deref().unwrap_or("unknown"),
                            if task.title.len() > 60 {
                                &task.title[..60]
                            } else {
                                &task.title
                            }
                        );

                        let description = format!(
                            "Triggered by task status change.\n\nOriginal task: {}\nNew status: {}\nAssignee: {}\n\nResult:\n{}",
                            task.title,
                            new_status,
                            task.assignee.as_deref().unwrap_or("unknown"),
                            task_result.unwrap_or("(no result)")
                        );

                        let task = self.create_task(
                            db,
                            workspace_id,
                            &task_title,
                            Some(&description),
                            Some(to),
                            None,
                            Some("medium"),
                            None,
                            task.assignee.as_deref().unwrap_or("system"),
                            None,
                            now,
                        )?;

                        tracing::info!(
                            "[Cowork] Task status trigger: created task '{}' → {to}",
                            task.title
                        );

                        out.push(task);
                    }
                } else {
                    tracing::warn!(
                        "[Cowork] Failed to parse triggers JSON for member {}",
                        member.member_id
                    );
                }
            }
        }
        Ok(())
    }

    /// Process task status change triggers and dispatch created tasks.
    /// Called after a CoworkTask status changes to check for matching triggers
    /// and create follow-up tasks via DAG dispatch.
    fn process_task_status_triggers(
        &self,
        db: &Arc<Db>,
        workspace_id: &str,
        task_id: &str,
        new_status: &str,
        task_result: Option<&str>,
        now: &str,
        agent_api: Option<Arc<dyn AgentApi>>,
        self_arc: Arc<CoworkManager>,
    ) {
        tracing::info!(
            "[Cowork] process_task_status_triggers: workspace_id={}, task_id={}, new_status={}, has_agent_api={}",
            workspace_id,
            task_id,
            new_status,
            agent_api.is_some()
        );

        let task = match db.get_cowork_task(task_id) {
            Ok(Some(t)) => t,
            Ok(None) => {
                tracing::warn!("[Cowork] Task {task_id} not found for status trigger processing");
                return;
            }
            Err(e) => {
                tracing::error!("[Cowork] Failed to fetch task {task_id}: {e}");
                return;
            }
        };

        let members = match db.list_cowork_members(workspace_id) {
            Ok(m) => m,
            Err(e) => {
                tracing::error!("[Cowork] Failed to list members for {workspace_id}: {e}");
                return;
            }
        };

        tracing::info!(
            "[Cowork] Found {} members in workspace {}, checking for task_status_changed triggers",
            members.len(),
            workspace_id
        );

        let mut followup_tasks = Vec::new();
        if let Err(e) = self.collect_task_status_triggers(
            db,
            workspace_id,
            &members,
            &task,
            new_status,
            task_result,
            &[],
            now,
            &mut followup_tasks,
        ) {
            tracing::error!("[Cowork] Failed to collect task status triggers: {e}");
            return;
        }

        if followup_tasks.is_empty() {
            tracing::info!(
                "[Cowork] No task status triggers matched for status {}",
                new_status
            );
            return;
        }

        tracing::info!(
            "[Cowork] Task status change created {} follow-up task(s)",
            followup_tasks.len()
        );

        let _ = self.dispatch_cowork_tasks_batch(
            db,
            workspace_id,
            &members,
            &followup_tasks,
            &format!("Task status: {}", new_status),
            agent_api.map(|api| (api, Arc::clone(db))),
            self_arc,
        );
    }

    /// DAG or direct dispatch for a set of cowork tasks (already persisted).
    fn dispatch_cowork_tasks_batch(
        &self,
        db: &Db,
        workspace_id: &str,
        members: &[CoworkMember],
        created_tasks: &[CoworkTask],
        goal: &str,
        agent_api: Option<(Arc<dyn AgentApi>, Arc<Db>)>,
        self_arc: Arc<CoworkManager>,
    ) -> Result<()> {
        if created_tasks.is_empty() {
            return Ok(());
        }
        let dag_bridge = self.dispatch_bridge.lock().unwrap().clone();
        if let Some(ref bridge) = dag_bridge {
            tracing::info!(
                "[Cowork] DAG bridge — routing {} task(s) through dispatch",
                created_tasks.len()
            );
            let workspace = match db.get_cowork_workspace(workspace_id) {
                Ok(Some(ws)) => ws,
                _ => {
                    self.fire_changed();
                    return Ok(());
                }
            };
            let board = db
                .get_cowork_board_entries(workspace_id, None)
                .unwrap_or_default();

            let dispatch_tasks: Vec<DispatchTask> = created_tasks
                .iter()
                .map(|task| {
                    let assignee_id = task.assignee.as_deref().unwrap_or("");
                    let member = members.iter().find(|m| m.member_id == assignee_id);
                    let agent_id = assignee_id.to_string();
                    let agent_jid = member
                        .and_then(|m| m.jid.clone())
                        .unwrap_or_else(|| format!("cowork:{}:{}", workspace_id, agent_id));

                    let depends_on: Vec<String> = task
                        .depends_on
                        .as_ref()
                        .and_then(|d| serde_json::from_str::<Vec<String>>(d).ok())
                        .unwrap_or_default()
                        .iter()
                        .filter_map(|dep_id| {
                            created_tasks
                                .iter()
                                .find(|t| t.id == *dep_id)
                                .map(|t| t.title.clone())
                        })
                        .collect();

                    let deps: Vec<CoworkTask> = created_tasks
                        .iter()
                        .filter(|t| {
                            task.depends_on
                                .as_ref()
                                .and_then(|d| serde_json::from_str::<Vec<String>>(d).ok())
                                .map(|ids| ids.contains(&t.id))
                                .unwrap_or(false)
                        })
                        .cloned()
                        .collect();

                    let prompt = self::prompt::build_cowork_task_prompt(
                        task,
                        member.unwrap_or(&CoworkMember {
                            workspace_id: workspace_id.to_string(),
                            member_id: agent_id.clone(),
                            role: "worker".into(),
                            jid: Some(agent_jid.clone()),
                            subdir: None,
                            persona: None,
                            responsibilities: None,
                            triggers: None,
                            handoff_rules: None,
                            acceptance_criteria: None,
                            output_format: None,
                            sla: None,
                            limits: None,
                            joined_at: String::new(),
                            updated_at: String::new(),
                        }),
                        &workspace,
                        &board,
                        &deps,
                    );

                    let timeout_seconds: u64 = member
                        .and_then(|m| m.sla.as_ref())
                        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                        .and_then(|v| v.get("maxDurationPerTaskMinutes").and_then(|t| t.as_i64()))
                        .map(|mins| (mins * 60) as u64)
                        .unwrap_or(1800);

                    DispatchTask {
                        id: String::new(),
                        label: task.title.clone(),
                        agent_id,
                        agent_jid,
                        depends_on,
                        prompt,
                        status: DispatchTaskStatus::Registered,
                        result: None,
                        created_at: String::new(),
                        started_at: None,
                        timeout_seconds,
                        timeout_at: None,
                        completed_at: None,
                        is_virtual: false,
                        persona_name: None,
                    }
                })
                .collect();

            let admin_folder = members
                .iter()
                .find(|m| m.role == "lead")
                .or_else(|| members.first())
                .map(|m| m.member_id.clone())
                .unwrap_or_else(|| "cowork".into());

            match bridge.enqueue_parent(
                goal.to_string(),
                admin_folder,
                Some(workspace.root_dir.clone()),
                dispatch_tasks,
            ) {
                Ok((parent_id, task_ids)) => {
                    tracing::info!(
                        "[Cowork] Enqueued DAG parent {parent_id} with {} task(s)",
                        task_ids.len()
                    );
                    {
                        let mut map = self.dispatch_task_map.lock().unwrap();
                        for (i, dispatch_id) in task_ids.iter().enumerate() {
                            if let Some(cowork_task) = created_tasks.get(i) {
                                map.insert(
                                    dispatch_id.clone(),
                                    (cowork_task.id.clone(), workspace_id.to_string()),
                                );
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("[Cowork] Failed to enqueue DAG parent: {e}");
                }
            }
        } else if let Some((ref api, ref db_arc)) = agent_api {
            tracing::info!(
                "[Cowork] No DAG bridge — dispatching {} task(s) via direct process_and_wait",
                created_tasks.len()
            );
            for task in created_tasks {
                let assignee_id = task.assignee.as_deref().unwrap_or("");
                if let Some(member) = members.iter().find(|m| m.member_id == assignee_id) {
                    self.send_to_cowork_agent(
                        Arc::clone(db_arc),
                        Arc::clone(api),
                        workspace_id,
                        task,
                        member,
                        Arc::clone(&self_arc),
                    );
                }
            }
        } else {
            tracing::warn!(
                "[Cowork] No DAG bridge AND no agent_api — {} task(s) not dispatched",
                created_tasks.len()
            );
        }

        self.fire_changed();
        Ok(())
    }

    /// Process a user message in the workspace: save it, create tasks for agents,
    /// and optionally dispatch tasks to agents for execution.
    /// Returns (message, created_tasks).
    pub fn process_user_message(
        &self,
        db: &Db,
        workspace_id: &str,
        from_user: &str,
        content: &str,
        incoming_message_type: Option<&str>,
        now: &str,
        agent_api: Option<(Arc<dyn AgentApi>, Arc<Db>)>,
        self_arc: Arc<CoworkManager>,
    ) -> Result<(CoworkMessage, Vec<CoworkTask>)> {
        // 1. Save the message
        let msg_id = format!(
            "cwmsg-{}",
            Uuid::new_v4()
                .to_string()
                .split('-')
                .next()
                .unwrap_or("0000")
        );
        let resolved_type = incoming_message_type
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                if from_user == "user" {
                    "status"
                } else {
                    "handoff"
                }
            })
            .to_string();
        let msg = CoworkMessage {
            id: msg_id,
            workspace_id: workspace_id.to_string(),
            from_member: from_user.to_string(),
            to_member: None,
            message_type: resolved_type.clone(),
            content: content.to_string(),
            attachments: None,
            task_id: None,
            is_read: false,
            created_at: now.to_string(),
        };
        db.insert_cowork_message(&msg)?;

        // 2. Get workspace members (agents)
        let members = db.list_cowork_members(workspace_id)?;
        let mut created_tasks = Vec::new();

        if members.is_empty() {
            self.fire_changed();
            return Ok((msg, created_tasks));
        }

        // 3. Assign the primary planning task to the lead (if any), not the first worker.
        let lead = members
            .iter()
            .find(|m| m.role == "lead")
            .or_else(|| members.first());

        if let Some(agent) = lead {
            // Create a planning/execution task for the lead agent
            let task_title = if content.len() > 80 {
                format!("{}...", &content[..80])
            } else {
                content.to_string()
            };
            let task = self.create_task(
                db,
                workspace_id,
                &task_title,
                Some(content),          // full message as description
                Some(&agent.member_id), // assignee
                None,                   // reviewer
                Some("high"),
                None, // depends_on
                from_user,
                None, // attachments
                now,
            )?;
            created_tasks.push(task);
        }

        // 4–5. Triggers → extra tasks → dispatch (shared with agent-result follow-ups)
        let mut triggered = Vec::new();
        self.collect_triggered_tasks(
            db,
            workspace_id,
            &members,
            from_user,
            &resolved_type,
            content,
            &created_tasks,
            now,
            &mut triggered,
        )?;
        created_tasks.extend(triggered);

        self.dispatch_cowork_tasks_batch(
            db,
            workspace_id,
            &members,
            &created_tasks,
            content,
            agent_api,
            self_arc,
        )?;

        self.fire_changed();
        Ok((msg, created_tasks))
    }

    // ============================================================
    // Workspaces
    // ============================================================

    pub fn create_workspace(
        &self,
        db: &Db,
        config: &Config,
        name: &str,
        description: Option<&str>,
        working_dir: Option<&str>,
        now: &str,
    ) -> Result<CoworkWorkspace> {
        let id = format!(
            "ws-{}",
            &name
                .to_lowercase()
                .replace(|c: char| !c.is_alphanumeric() && c != '-', "-")
        );
        let root_dir = config.paths.workspace_dir.join("cowork").join(&id);

        // Create workspace directory structure
        fs::create_dir_all(root_dir.join("board")).ok();
        fs::create_dir_all(root_dir.join("tasks")).ok();
        fs::create_dir_all(root_dir.join("memory")).ok();
        fs::create_dir_all(root_dir.join("shared")).ok();
        fs::create_dir_all(root_dir.join("agents")).ok();
        fs::create_dir_all(root_dir.join("recordings")).ok();

        // Resource directories: raw, wiki, reference, workdir
        let raw_dir = root_dir.join("raw");
        let wiki_dir = root_dir.join("wiki");
        let reference_dir = root_dir.join("reference");
        let workdir = working_dir
            .map(PathBuf::from)
            .unwrap_or_else(|| root_dir.join("workdir"));
        fs::create_dir_all(&raw_dir).ok();
        fs::create_dir_all(&wiki_dir).ok();
        fs::create_dir_all(&reference_dir).ok();
        fs::create_dir_all(&workdir).ok();

        let ws = CoworkWorkspace {
            id: id.clone(),
            name: name.to_string(),
            description: description.map(|s| s.to_string()),
            status: "active".to_string(),
            root_dir: root_dir.to_string_lossy().into_owned(),
            working_dir: Some(workdir.to_string_lossy().into_owned()),
            created_at: now.to_string(),
            updated_at: now.to_string(),
        };

        db.insert_cowork_workspace(&ws)?;

        // Persist the 4 typed resource paths
        for (kind, path) in &[
            ("raw", raw_dir.as_path()),
            ("wiki", wiki_dir.as_path()),
            ("reference", reference_dir.as_path()),
            ("workdir", workdir.as_path()),
        ] {
            db.upsert_workspace_resource(&crate::types::WorkspaceResource {
                workspace_id: id.clone(),
                kind: kind.to_string(),
                path: path.to_string_lossy().into_owned(),
            })
            .ok();
        }

        self.fire_changed();
        Ok(ws)
    }

    pub fn get_workspace(&self, db: &Db, id: &str) -> Result<Option<CoworkWorkspace>> {
        db.get_cowork_workspace(id)
    }

    pub fn list_workspaces(&self, db: &Db) -> Result<Vec<CoworkWorkspace>> {
        db.list_cowork_workspaces()
    }

    pub fn update_workspace(
        &self,
        db: &Db,
        id: &str,
        name: Option<&str>,
        description: Option<&str>,
        status: Option<&str>,
        working_dir: Option<&str>,
        now: &str,
    ) -> Result<()> {
        db.update_cowork_workspace(id, name, description, status, working_dir, now)?;
        self.fire_changed();
        Ok(())
    }

    pub fn delete_workspace(&self, db: &Db, id: &str) -> Result<()> {
        let ws = db.get_cowork_workspace(id)?;
        let root_dir = ws.as_ref().map(|w| w.root_dir.clone());

        {
            let mut map = self.dispatch_task_map.lock().unwrap();
            map.retain(|_, (_, wid)| wid != id);
        }

        if let (Some(ref dir), Some(bridge)) = (root_dir.as_ref(), self.get_dispatch_bridge()) {
            let _ = bridge.cancel_parents_for_shared_workspace(dir);
        }

        db.delete_cowork_workspace(id)?;

        // Remove the workspace folder from disk
        if let Some(dir) = root_dir {
            let path = PathBuf::from(&dir);
            if path.exists() {
                if let Err(e) = fs::remove_dir_all(&path) {
                    tracing::warn!(
                        path = %path.display(),
                        error = %e,
                        "failed to remove workspace directory during delete",
                    );
                }
            }
        }

        self.fire_changed();
        Ok(())
    }

    // ============================================================
    // Resources
    // ============================================================

    pub fn upsert_resource(
        &self,
        db: &Db,
        workspace_id: &str,
        kind: &str,
        path: &str,
    ) -> Result<()> {
        db.upsert_workspace_resource(&crate::types::WorkspaceResource {
            workspace_id: workspace_id.to_string(),
            kind: kind.to_string(),
            path: path.to_string(),
        })?;
        self.fire_resource_changed(workspace_id.to_string());
        Ok(())
    }

    pub fn delete_resource(&self, db: &Db, workspace_id: &str, kind: &str) -> Result<()> {
        db.delete_workspace_resource(workspace_id, kind)?;
        self.fire_resource_changed(workspace_id.to_string());
        Ok(())
    }

    // ============================================================
    // Members
    // ============================================================

    pub fn add_member(
        &self,
        db: &Db,
        config: &Config,
        workspace_id: &str,
        member_id: &str,
        role: &str,
        jid: Option<&str>,
        subdir: Option<&str>,
        now: &str,
    ) -> Result<CoworkMember> {
        let ws = db
            .get_cowork_workspace(workspace_id)?
            .ok_or_else(|| anyhow::anyhow!("Workspace {workspace_id} not found"))?;

        // Create agent subdir
        let agent_dir = PathBuf::from(&ws.root_dir).join("agents").join(member_id);
        fs::create_dir_all(&agent_dir).ok();
        let workspace_link = config.paths.workspace_dir.join(member_id);
        if !workspace_link.exists() {
            // Point agent's workspace to cowork shared dir
            std::os::unix::fs::symlink(&ws.root_dir, &workspace_link).ok();
        }

        let member = CoworkMember {
            workspace_id: workspace_id.to_string(),
            member_id: member_id.to_string(),
            role: role.to_string(),
            jid: jid.map(|s| s.to_string()),
            subdir: subdir.map(|s| s.to_string()),
            persona: None,
            responsibilities: None,
            triggers: None,
            handoff_rules: None,
            acceptance_criteria: None,
            output_format: None,
            sla: None,
            limits: None,
            joined_at: now.to_string(),
            updated_at: now.to_string(),
        };

        db.insert_cowork_member(&member)?;
        self.fire_changed();
        Ok(member)
    }

    pub fn get_member(
        &self,
        db: &Db,
        workspace_id: &str,
        member_id: &str,
    ) -> Result<Option<CoworkMember>> {
        db.get_cowork_member(workspace_id, member_id)
    }

    pub fn list_members(&self, db: &Db, workspace_id: &str) -> Result<Vec<CoworkMember>> {
        db.list_cowork_members(workspace_id)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_member_spec(
        &self,
        db: &Db,
        workspace_id: &str,
        member_id: &str,
        role: Option<&str>,
        persona: Option<&str>,
        responsibilities: Option<&str>,
        triggers: Option<&str>,
        handoff_rules: Option<&str>,
        acceptance_criteria: Option<&str>,
        output_format: Option<&str>,
        sla: Option<&str>,
        limits: Option<&str>,
        now: &str,
    ) -> Result<()> {
        db.update_cowork_member(
            workspace_id,
            member_id,
            role,
            persona,
            responsibilities,
            triggers,
            handoff_rules,
            acceptance_criteria,
            output_format,
            sla,
            limits,
            now,
        )?;
        self.fire_changed();
        Ok(())
    }

    pub fn remove_member(&self, db: &Db, workspace_id: &str, member_id: &str) -> Result<()> {
        db.delete_cowork_member(workspace_id, member_id)?;
        self.fire_changed();
        Ok(())
    }

    // ============================================================
    // Board entries
    // ============================================================

    pub fn upsert_board_entry(
        &self,
        db: &Db,
        workspace_id: &str,
        section: &str,
        title: Option<&str>,
        content: &str,
        author: &str,
        now: &str,
    ) -> Result<CoworkBoardEntry> {
        // Check if a section entry already exists for this workspace+section
        let existing = db.get_cowork_board_entries(workspace_id, Some(section))?;
        let entry = if let Some(e) = existing.first() {
            // Update existing
            let id = e.id.clone();
            db.update_cowork_board_entry(&id, title, Some(content), None, None, now)?;
            db.get_cowork_board_entries(workspace_id, Some(section))?
                .into_iter()
                .next()
                .ok_or_else(|| anyhow::anyhow!("Board entry disappeared after update"))?
        } else {
            let id = format!(
                "be-{}",
                Uuid::new_v4()
                    .to_string()
                    .split('-')
                    .next()
                    .unwrap_or("0000")
            );
            let e = CoworkBoardEntry {
                id,
                workspace_id: workspace_id.to_string(),
                section: section.to_string(),
                title: title.map(|s| s.to_string()),
                content: content.to_string(),
                author: author.to_string(),
                pinned: false,
                tags: None,
                created_at: now.to_string(),
                updated_at: now.to_string(),
            };
            db.insert_cowork_board_entry(&e)?;
            e
        };
        self.fire_changed();
        Ok(entry)
    }

    pub fn get_board(
        &self,
        db: &Db,
        workspace_id: &str,
        section: Option<&str>,
    ) -> Result<Vec<CoworkBoardEntry>> {
        db.get_cowork_board_entries(workspace_id, section)
    }

    pub fn update_board_entry(
        &self,
        db: &Db,
        id: &str,
        title: Option<&str>,
        content: Option<&str>,
        pinned: Option<bool>,
        tags: Option<&str>,
        now: &str,
    ) -> Result<()> {
        db.update_cowork_board_entry(id, title, content, pinned, tags, now)?;
        self.fire_changed();
        Ok(())
    }

    pub fn delete_board_entry(&self, db: &Db, id: &str) -> Result<()> {
        db.delete_cowork_board_entry(id)?;
        self.fire_changed();
        Ok(())
    }

    // ============================================================
    // Tasks
    // ============================================================

    pub fn create_task(
        &self,
        db: &Db,
        workspace_id: &str,
        title: &str,
        description: Option<&str>,
        assignee: Option<&str>,
        reviewer: Option<&str>,
        priority: Option<&str>,
        depends_on: Option<&str>,
        created_by: &str,
        attachments: Option<&str>,
        now: &str,
    ) -> Result<CoworkTask> {
        let id = format!(
            "cwt-{}",
            Uuid::new_v4()
                .to_string()
                .split('-')
                .next()
                .unwrap_or("0000")
        );
        let task = CoworkTask {
            id,
            workspace_id: workspace_id.to_string(),
            title: title.to_string(),
            description: description.map(|s| s.to_string()),
            status: "todo".to_string(),
            assignee: assignee.map(|s| s.to_string()),
            reviewer: reviewer.map(|s| s.to_string()),
            priority: priority.unwrap_or("medium").to_string(),
            depends_on: depends_on.map(|s| s.to_string()),
            attachments: attachments.map(|s| s.to_string()),
            created_by: created_by.to_string(),
            created_at: now.to_string(),
            updated_at: now.to_string(),
            due_at: None,
            completed_at: None,
            input_summary: None,
            result_output: None,
            references: None,
            artifacts: None,
        };
        db.insert_cowork_task(&task)?;
        self.fire_changed();
        Ok(task)
    }

    pub fn get_task(&self, db: &Db, id: &str) -> Result<Option<CoworkTask>> {
        db.get_cowork_task(id)
    }

    pub fn list_tasks(
        &self,
        db: &Db,
        workspace_id: &str,
        status: Option<&str>,
    ) -> Result<Vec<CoworkTask>> {
        db.list_cowork_tasks(workspace_id, status)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_task(
        &self,
        db: &Db,
        id: &str,
        title: Option<&str>,
        description: Option<&str>,
        status: Option<&str>,
        assignee: Option<&str>,
        reviewer: Option<&str>,
        priority: Option<&str>,
        depends_on: Option<&str>,
        attachments: Option<&str>,
        now: &str,
    ) -> Result<()> {
        db.update_cowork_task(
            id,
            title,
            description,
            status,
            assignee,
            reviewer,
            priority,
            depends_on,
            attachments,
            now,
        )?;
        self.fire_changed();
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_task_with_triggers(
        &self,
        db: &Arc<Db>,
        id: &str,
        title: Option<&str>,
        description: Option<&str>,
        status: Option<&str>,
        assignee: Option<&str>,
        reviewer: Option<&str>,
        priority: Option<&str>,
        depends_on: Option<&str>,
        attachments: Option<&str>,
        now: &str,
        agent_api: Option<Arc<dyn AgentApi>>,
        self_arc: Arc<CoworkManager>,
    ) -> Result<()> {
        let old_task = db.get_cowork_task(id)?.ok_or_else(|| anyhow::anyhow!("Task not found"))?;
        let old_status = old_task.status.clone();

        tracing::info!(
            "[Cowork] update_task_with_triggers: task_id={}, old_status={}, new_status={:?}, agent_api_available={}",
            id,
            old_status,
            status,
            agent_api.is_some()
        );

        db.update_cowork_task(
            id,
            title,
            description,
            status,
            assignee,
            reviewer,
            priority,
            depends_on,
            attachments,
            now,
        )?;

        // Process task status triggers if status changed and agent_api is available
        if let Some(new_status) = status {
            if new_status != old_status {
                tracing::info!(
                    "[Cowork] Status changed: {} -> {}, checking triggers (agent_api={})",
                    old_status,
                    new_status,
                    agent_api.is_some()
                );
                if let Some(api) = agent_api {
                    let task_result = if new_status == "done" {
                        old_task.result_output.as_deref()
                    } else {
                        None
                    };
                    self.process_task_status_triggers(
                        db,
                        &old_task.workspace_id,
                        id,
                        new_status,
                        task_result,
                        now,
                        Some(api),
                        Arc::clone(&self_arc),
                    );
                } else {
                    tracing::warn!(
                        "[Cowork] Status changed but no agent_api available for trigger processing"
                    );
                }
            }
        }

        self.fire_changed();
        Ok(())
    }

    pub fn delete_task(&self, db: &Db, id: &str) -> Result<()> {
        db.delete_cowork_task(id)?;
        self.fire_changed();
        Ok(())
    }

    // ============================================================
    // Task comments
    // ============================================================

    pub fn add_task_comment(
        &self,
        db: &Db,
        task_id: &str,
        author: &str,
        content: &str,
        now: &str,
    ) -> Result<i64> {
        let id = db.insert_cowork_task_comment(task_id, author, content, now)?;
        self.fire_changed();
        Ok(id)
    }

    pub fn list_task_comments(
        &self,
        db: &Db,
        task_id: &str,
    ) -> Result<Vec<crate::types::CoworkTaskComment>> {
        db.list_cowork_task_comments(task_id)
    }

    // ============================================================
    // Messages
    // ============================================================

    pub fn send_message(
        &self,
        db: &Db,
        workspace_id: &str,
        from_member: &str,
        to_member: Option<&str>,
        message_type: &str,
        content: &str,
        task_id: Option<&str>,
        attachments: Option<&str>,
        now: &str,
    ) -> Result<CoworkMessage> {
        let id = format!(
            "cwm-{}",
            Uuid::new_v4()
                .to_string()
                .split('-')
                .next()
                .unwrap_or("0000")
        );
        let msg = CoworkMessage {
            id,
            workspace_id: workspace_id.to_string(),
            from_member: from_member.to_string(),
            to_member: to_member.map(|s| s.to_string()),
            message_type: message_type.to_string(),
            content: content.to_string(),
            attachments: attachments.map(|s| s.to_string()),
            task_id: task_id.map(|s| s.to_string()),
            is_read: false,
            created_at: now.to_string(),
        };
        db.insert_cowork_message(&msg)?;
        self.fire_changed();
        Ok(msg)
    }

    pub fn list_messages(
        &self,
        db: &Db,
        workspace_id: &str,
        limit: u32,
        since: Option<&str>,
    ) -> Result<Vec<CoworkMessage>> {
        db.list_cowork_messages(workspace_id, limit, since)
    }

    pub fn mark_message_read(&self, db: &Db, id: &str) -> Result<()> {
        db.mark_cowork_message_read(id)
    }

    // ============================================================
    // Templates
    // ============================================================

    /// Ensure built-in templates exist in the templates directory
    pub fn ensure_builtin_templates(&self, config: &Config) {
        let dir = &config.paths.workspace_templates_dir;
        fs::create_dir_all(dir).ok();

        let builtins: Vec<CoworkTemplate> = vec![
            CoworkTemplate {
                name: "Software Development".into(),
                description: "Only the lead plans and coordinates (Board plan_checklist, task order, handoff tokens). Code and test agents execute assigned work — no replanning or checklist ownership.".into(),
                icon: Some("CodeOutlined".into()),
                members: vec![
                    TemplateMember {
                        agent_folder: "lead-agent".into(),
                        role: "lead".into(),
                        subdir: Some("lead".into()),
                        persona: Some(
                            "You are the **only** role that plans and coordinates this workspace. Workers do not own the plan — they execute tasks you assign. Read user goals and Board, maintain plan_checklist, produce a short plan, delegate concrete tasks with assignees and depends_on (implement → test), record decisions on the Board. \
Plan gate (switch-style): (1) First execution on a goal: output Plan and create/update Board section plan_checklist with `- [ ]`; omit HANDOFF_TO_CODE_AGENT so implementation is not auto-dispatched. \
(2) Later: re-read plan_checklist; if all `- [x]` and nothing remains, put WORKSTREAM_COMPLETE in your task result. \
(3) To start implementation, put HANDOFF_TO_CODE_AGENT in your task result."
                                .into(),
                        ),
                        responsibilities: Some(vec![
                            "Sole owner of plan_checklist and coordination: edit checklist and Board progress; workers must not change plan order or add parallel PM-style plans".into(),
                            "Break work into ordered tasks for code-agent and test-agent; set depends_on so tests run only after implementation completes".into(),
                            "Unblock the team when dependencies or scope change; workers escalate replanning requests through their task results for you to adjust the plan".into(),
                            "When finishing a lead task: HANDOFF_TO_CODE_AGENT, WORKSTREAM_COMPLETE, or neither (plan-only), as documented in output rules".into(),
                        ]),
                        triggers: Some(vec![
                            serde_json::from_str(
                                r#"{"type":"task_assigned","condition":"assignee == me"}"#,
                            )
                            .unwrap(),
                            serde_json::from_str(r#"{"type":"message_received","from":"user"}"#)
                                .unwrap(),
                            serde_json::from_str(r#"{"type":"task_status_changed","status":"done","assignee":"code-agent","to":"test-agent"}"#).unwrap(),
                            serde_json::from_str(r#"{"type":"task_status_changed","status":"done","assignee":"test-agent","to":"lead-agent"}"#).unwrap(),
                        ]),
                        handoff: Some(vec![
                            serde_json::from_str(
                                r#"{"when":"task_complete","to":"code-agent","type":"handoff","only_if_result_contains":"HANDOFF_TO_CODE_AGENT","unless_result_contains":"WORKSTREAM_COMPLETE"}"#,
                            )
                            .unwrap(),
                        ]),
                        acceptance_criteria: Some(vec![
                            "Every active task has a clear owner and acceptance criteria".into(),
                            "Board progress section updated when milestones move".into(),
                            "plan_checklist reflects current scope; closure uses WORKSTREAM_COMPLETE or HANDOFF_TO_CODE_AGENT as appropriate".into(),
                        ]),
                        output: Some(
                            serde_json::from_str(
                                r#"{"format":"markdown","requiredSections":["Plan","Plan Checklist","Assignments","Risks","Board Updates"],"description":"Mirror Plan Checklist with the Board section plan_checklist. Use the literal tokens HANDOFF_TO_CODE_AGENT to queue code-agent, or WORKSTREAM_COMPLETE when all checklist items are done and the workstream should stop without further implementation handoff."}"#,
                            )
                            .unwrap(),
                        ),
                        sla: Some(
                            serde_json::from_str(
                                r#"{"maxDurationPerTaskMinutes":45,"maxTokenPerTask":40000}"#,
                            )
                            .unwrap(),
                        ),
                        limits: Some(
                            serde_json::from_str(r#"{"deniedTools":["Bash","Write","Edit"]}"#)
                                .unwrap(),
                        ),
                    },
                    TemplateMember {
                        agent_folder: "code-agent".into(),
                        role: "worker".into(),
                        subdir: Some("impl".into()),
                        persona: Some("Senior engineer — **not** a coordinator. Follow the task and lead's plan only; do not edit plan_checklist, reorder the workstream, or assign work to others. Implement features, own code quality (diff, clippy/tests), document risks in the task outcome. Production-ready before handoff to QA.".into()),
                        responsibilities: Some(vec![
                            "Execute only what lead assigned; if scope is unclear or conflicts with plan, ask in your task outcome for lead to update plan_checklist — do not act as PM".into(),
                            "Run project checks (e.g. cargo test / clippy) and fix issues before marking done".into(),
                            "Summarize files and shared/ paths in your result; lead updates Board progress from your outcome".into(),
                        ]),
                        triggers: Some(vec![
                            serde_json::from_str(r#"{"type":"task_assigned","condition":"assignee == me"}"#).unwrap(),
                            serde_json::from_str(r#"{"type":"task_status_changed","status":"done","assignee":"test-agent","to":"code-agent"}"#).unwrap(),
                        ]),
                        handoff: Some(vec![
                            serde_json::from_str(
                                r#"{"when":"task_complete","to":"test-agent","type":"handoff"}"#,
                            )
                            .unwrap(),
                        ]),
                        acceptance_criteria: Some(vec![
                            "Required tests pass".into(),
                            "Static checks clean (e.g. clippy) where applicable".into(),
                            "Summary lists files touched and follow-ups".into(),
                        ]),
                        output: Some(serde_json::from_str(r#"{"format":"markdown","requiredSections":["Summary","Files Changed","Test Results","Notes"],"attachDiff":true}"#).unwrap()),
                        sla: Some(serde_json::from_str(r#"{"maxDurationPerTaskMinutes":60,"maxTokenPerTask":50000}"#).unwrap()),
                        limits: Some(serde_json::from_str(r#"{"allowedBashCommands":["cargo build","cargo test","cargo clippy","git diff"]}"#).unwrap()),
                    },
                    TemplateMember {
                        agent_folder: "test-agent".into(),
                        role: "worker".into(),
                        subdir: Some("tests".into()),
                        persona: Some("QA engineer — **not** a coordinator. Follow assigned tasks and lead's plan only; do not edit plan_checklist or replan. After implementation (or handoff from code-agent), extend coverage, run the suite, report failures with repro steps.".into()),
                        responsibilities: Some(vec![
                            "Execute verification per task; escalate plan or scope gaps to lead in your result — do not change coordination on the Board".into(),
                            "Add or extend tests per acceptance criteria; run full suite when assigned or handed off from code-agent".into(),
                            "Report coverage, flakiness, and regressions in your result; lead may copy summaries to Board if needed".into(),
                        ]),
                        triggers: Some(vec![
                            serde_json::from_str(r#"{"type":"task_assigned","condition":"assignee == me"}"#).unwrap(),
                            serde_json::from_str(r#"{"type":"message_received","from":"code-agent","messageType":"result"}"#).unwrap(),
                            serde_json::from_str(r#"{"type":"task_status_changed","status":"in_progress","assignee":"code-agent","to":"test-agent"}"#).unwrap(),
                        ]),
                        handoff: Some(vec![
                            serde_json::from_str(r#"{"when":"task_complete","to":"lead-agent","type":"status"}"#).unwrap(),
                        ]),
                        acceptance_criteria: Some(vec![
                            "Test coverage targets met for new code".into(),
                            "No flaky tests introduced".into(),
                        ]),
                        output: None,
                        sla: None,
                        limits: Some(serde_json::from_str(r#"{"allowedBashCommands":["cargo test","cargo tarpaulin"]}"#).unwrap()),
                    },
                ],
                board: Some(TemplateBoard {
                    sections: vec![
                        TemplateBoardSection { section_type: "brief".into(), title: "Project Brief".into(), template: Some("Describe the project and its goals...".into()) },
                        TemplateBoardSection { section_type: "plan_checklist".into(), title: "Plan checklist".into(), template: Some("**(Lead only — do not edit as code-agent / test-agent)** — ordered steps for this workstream.\n\n- Lead: first visit add `- [ ]` from Plan; later mark `- [x]`.\n- Workers: read-only here; put updates in task results for lead to fold in.\n- When all checked: lead ends with `WORKSTREAM_COMPLETE`.\n\n- [ ] ...".into()) },
                        TemplateBoardSection { section_type: "guidelines".into(), title: "Development Guidelines".into(), template: Some("- Language/framework: ...\n- Coding conventions: ...\n- Testing requirements: ...".into()) },
                        TemplateBoardSection { section_type: "decisions".into(), title: "Architecture Decisions".into(), template: Some("(lead records notable decisions after each milestone)".into()) },
                    ],
                }),
            },
            CoworkTemplate {
                name: "Research".into(),
                description: "Research workflow with a research lead plus researcher, synthesizer, and critic agents".into(),
                icon: Some("SearchOutlined".into()),
                members: vec![
                    TemplateMember {
                        agent_folder: "research-lead".into(),
                        role: "lead".into(),
                        subdir: Some("lead".into()),
                        persona: Some(
                            "Research lead. Frames questions, scope, and success criteria; routes work between research, synthesis, and critique.".into(),
                        ),
                        responsibilities: Some(vec![
                            "Maintain research brief and Board reference expectations".into(),
                            "Assign or sequence tasks for researcher, synthesizer, and critic".into(),
                            "Close the loop when findings are ready for stakeholders".into(),
                        ]),
                        triggers: Some(vec![
                            serde_json::from_str(
                                r#"{"type":"task_assigned","condition":"assignee == me"}"#,
                            )
                            .unwrap(),
                            serde_json::from_str(r#"{"type":"message_received","from":"user"}"#)
                                .unwrap(),
                            serde_json::from_str(r#"{"type":"task_status_changed","status":"done","assignee":"researcher","to":"synthesizer"}"#).unwrap(),
                            serde_json::from_str(r#"{"type":"task_status_changed","status":"done","assignee":"synthesizer","to":"critic"}"#).unwrap(),
                            serde_json::from_str(r#"{"type":"task_status_changed","status":"done","assignee":"critic","to":"research-lead"}"#).unwrap(),
                        ]),
                        handoff: Some(vec![
                            serde_json::from_str(
                                r#"{"when":"task_complete","to":"researcher","type":"handoff"}"#,
                            )
                            .unwrap(),
                        ]),
                        acceptance_criteria: Some(vec![
                            "Research scope and deliverables are explicit".into(),
                            "Board brief matches stakeholder intent".into(),
                        ]),
                        output: Some(
                            serde_json::from_str(
                                r#"{"format":"markdown","requiredSections":["Scope","Work Plan","Board"]}"#,
                            )
                            .unwrap(),
                        ),
                        sla: Some(
                            serde_json::from_str(
                                r#"{"maxDurationPerTaskMinutes":60,"maxTokenPerTask":50000}"#,
                            )
                            .unwrap(),
                        ),
                        limits: Some(
                            serde_json::from_str(r#"{"deniedTools":["Bash","Write","Edit"]}"#)
                                .unwrap(),
                        ),
                    },
                    TemplateMember {
                        agent_folder: "researcher".into(),
                        role: "worker".into(),
                        subdir: Some("research".into()),
                        persona: Some("Research analyst. Gather information thoroughly from multiple sources. Cite all sources clearly.".into()),
                        responsibilities: Some(vec![
                            "Research topics assigned via tasks".into(),
                            "Compile findings with full citations".into(),
                            "Update Board reference section with key sources".into(),
                        ]),
                        triggers: Some(vec![
                            serde_json::from_str(r#"{"type":"task_assigned","condition":"assignee == me"}"#).unwrap(),
                            serde_json::from_str(r#"{"type":"task_status_changed","status":"done","to":"synthesizer"}"#).unwrap(),
                        ]),
                        handoff: Some(vec![
                            serde_json::from_str(r#"{"when":"task_complete","to":"synthesizer","type":"handoff"}"#).unwrap(),
                        ]),
                        acceptance_criteria: Some(vec![
                            "All claims have citations".into(),
                            "Multiple sources cross-referenced".into(),
                        ]),
                        output: Some(serde_json::from_str(r#"{"format":"markdown","requiredSections":["Summary","Findings","Sources","Further Reading"]}"#).unwrap()),
                        sla: Some(serde_json::from_str(r#"{"maxDurationPerTaskMinutes":120,"maxTokenPerTask":100000}"#).unwrap()),
                        limits: None,
                    },
                    TemplateMember {
                        agent_folder: "synthesizer".into(),
                        role: "worker".into(),
                        subdir: Some("synthesis".into()),
                        persona: Some("Synthesis specialist. Combine research findings into coherent, well-structured documents. Identify patterns and connections.".into()),
                        responsibilities: Some(vec![
                            "Synthesize research findings into structured documents".into(),
                            "Identify gaps in research and request clarification".into(),
                        ]),
                        triggers: Some(vec![
                            serde_json::from_str(r#"{"type":"message_received","from":"researcher","messageType":"handoff"}"#).unwrap(),
                        ]),
                        handoff: Some(vec![
                            serde_json::from_str(r#"{"when":"task_complete","to":"critic","type":"review_request"}"#).unwrap(),
                        ]),
                        acceptance_criteria: Some(vec![
                            "Document is well-structured".into(),
                            "All researcher findings are incorporated".into(),
                        ]),
                        output: None, sla: None, limits: None,
                    },
                    TemplateMember {
                        agent_folder: "critic".into(),
                        role: "reviewer".into(),
                        subdir: Some("critique".into()),
                        persona: Some("Critical thinker. Challenge assumptions, identify logical fallacies, and verify factual claims. Be constructive, not destructive.".into()),
                        responsibilities: Some(vec![
                            "Critique synthesized documents for logic and accuracy".into(),
                            "Fact-check key claims".into(),
                            "Suggest improvements".into(),
                        ]),
                        triggers: Some(vec![
                            serde_json::from_str(r#"{"type":"message_received","from":"synthesizer","messageType":"review_request"}"#).unwrap(),
                        ]),
                        handoff: Some(vec![
                            serde_json::from_str(r#"{"when":"task_complete","to":"synthesizer","type":"result"}"#).unwrap(),
                        ]),
                        acceptance_criteria: Some(vec![
                            "All key claims verified".into(),
                            "Actionable feedback provided".into(),
                        ]),
                        output: None, sla: None, limits: None,
                    },
                ],
                board: Some(TemplateBoard {
                    sections: vec![
                        TemplateBoardSection { section_type: "brief".into(), title: "Research Brief".into(), template: Some("Research topic and scope...".into()) },
                        TemplateBoardSection { section_type: "guidelines".into(), title: "Research Guidelines".into(), template: Some("- Use primary sources when possible\n- Cross-reference claims\n- Cite in APA format".into()) },
                        TemplateBoardSection { section_type: "reference".into(), title: "Key References".into(), template: Some("(references will be added by researcher)".into()) },
                    ],
                }),
            },
            CoworkTemplate {
                name: "Content Pipeline".into(),
                description: "Content creation with a content lead plus writer, editor, and fact-checker agents".into(),
                icon: Some("EditOutlined".into()),
                members: vec![
                    TemplateMember {
                        agent_folder: "content-lead".into(),
                        role: "lead".into(),
                        subdir: Some("lead".into()),
                        persona: Some(
                            "Content lead / editor-in-chief. Aligns brief, tone, and deadlines across writer, editor, and fact-checker.".into(),
                        ),
                        responsibilities: Some(vec![
                            "Own Board brief and style expectations".into(),
                            "Sequence drafts → editorial → fact-check before publish".into(),
                            "Resolve conflicting feedback between roles".into(),
                        ]),
                        triggers: Some(vec![
                            serde_json::from_str(
                                r#"{"type":"task_assigned","condition":"assignee == me"}"#,
                            )
                            .unwrap(),
                            serde_json::from_str(r#"{"type":"message_received","from":"user"}"#)
                                .unwrap(),
                        ]),
                        handoff: Some(vec![
                            serde_json::from_str(
                                r#"{"when":"task_complete","to":"writer","type":"handoff"}"#,
                            )
                            .unwrap(),
                        ]),
                        acceptance_criteria: Some(vec![
                            "Writer has a complete brief (audience, length, tone, deadline)".into(),
                            "Fact-check happens after structural edit sign-off".into(),
                        ]),
                        output: Some(
                            serde_json::from_str(
                                r#"{"format":"markdown","requiredSections":["Brief Summary","Pipeline Status","Decisions"]}"#,
                            )
                            .unwrap(),
                        ),
                        sla: Some(
                            serde_json::from_str(
                                r#"{"maxDurationPerTaskMinutes":45,"maxTokenPerTask":40000}"#,
                            )
                            .unwrap(),
                        ),
                        limits: Some(
                            serde_json::from_str(r#"{"deniedTools":["Bash"]}"#).unwrap(),
                        ),
                    },
                    TemplateMember {
                        agent_folder: "writer".into(),
                        role: "worker".into(),
                        subdir: Some("drafts".into()),
                        persona: Some("Professional content writer. Write clear, engaging, and well-structured content for the target audience.".into()),
                        responsibilities: Some(vec![
                            "Write content based on brief and guidelines".into(),
                            "Incorporate editor feedback".into(),
                            "Track word count and tone requirements".into(),
                        ]),
                        triggers: Some(vec![
                            serde_json::from_str(r#"{"type":"task_assigned","condition":"assignee == me"}"#).unwrap(),
                        ]),
                        handoff: Some(vec![
                            serde_json::from_str(r#"{"when":"task_complete","to":"editor","type":"review_request"}"#).unwrap(),
                        ]),
                        acceptance_criteria: Some(vec![
                            "Meets word count target".into(),
                            "Matches tone requirements".into(),
                            "Grammar and spelling correct".into(),
                        ]),
                        output: Some(serde_json::from_str(r#"{"format":"markdown","attachDiff":false}"#).unwrap()),
                        sla: Some(serde_json::from_str(r#"{"maxDurationPerTaskMinutes":90,"maxTokenPerTask":80000}"#).unwrap()),
                        limits: None,
                    },
                    TemplateMember {
                        agent_folder: "editor".into(),
                        role: "reviewer".into(),
                        subdir: Some("edits".into()),
                        persona: Some("Senior editor. Improve clarity, flow, and impact. Focus on structure and audience engagement, not just grammar.".into()),
                        responsibilities: Some(vec![
                            "Review content for structure and flow".into(),
                            "Check tone and audience alignment".into(),
                            "Return with specific revision notes".into(),
                        ]),
                        triggers: Some(vec![
                            serde_json::from_str(r#"{"type":"message_received","from":"writer","messageType":"review_request"}"#).unwrap(),
                        ]),
                        handoff: Some(vec![
                            serde_json::from_str(r#"{"when":"task_complete","to":"fact-checker","type":"review_request"}"#).unwrap(),
                        ]),
                        acceptance_criteria: Some(vec!["Structural issues addressed".into(), "Tone is consistent".into()]),
                        output: None, sla: None, limits: None,
                    },
                    TemplateMember {
                        agent_folder: "fact-checker".into(),
                        role: "reviewer".into(),
                        subdir: Some("facts".into()),
                        persona: Some("Fact-checker. Verify every factual claim. Flag any unverified statements. Accuracy over speed.".into()),
                        responsibilities: Some(vec!["Verify all factual claims".into(), "Flag unverified or dubious claims".into()]),
                        triggers: Some(vec![
                            serde_json::from_str(r#"{"type":"message_received","from":"editor","messageType":"review_request"}"#).unwrap(),
                        ]),
                        handoff: Some(vec![
                            serde_json::from_str(r#"{"when":"task_complete","to":"writer","type":"result"}"#).unwrap(),
                        ]),
                        acceptance_criteria: Some(vec!["Every claim verified or flagged".into()]),
                        output: None, sla: None, limits: None,
                    },
                ],
                board: Some(TemplateBoard {
                    sections: vec![
                        TemplateBoardSection { section_type: "brief".into(), title: "Content Brief".into(), template: Some("Topic, target audience, word count, tone...".into()) },
                        TemplateBoardSection { section_type: "guidelines".into(), title: "Style Guide".into(), template: Some("- Tone: ...\n- Word count: ...\n- Format: ...".into()) },
                    ],
                }),
            },
            CoworkTemplate {
                name: "Data Analysis".into(),
                description: "Analytics lead coordinating analyst and visualizer for statistics and chart generation".into(),
                icon: Some("BarChartOutlined".into()),
                members: vec![
                    TemplateMember {
                        agent_folder: "analysis-lead".into(),
                        role: "lead".into(),
                        subdir: Some("lead".into()),
                        persona: Some(
                            "Analytics lead. Clarifies questions, data contracts, and outputs; coordinates analyst and visualizer.".into(),
                        ),
                        responsibilities: Some(vec![
                            "Lock analysis brief and success metrics on the Board".into(),
                            "Decide when results move from analysis to visualization".into(),
                            "Ensure caveats and data lineage are documented".into(),
                        ]),
                        triggers: Some(vec![
                            serde_json::from_str(
                                r#"{"type":"task_assigned","condition":"assignee == me"}"#,
                            )
                            .unwrap(),
                            serde_json::from_str(r#"{"type":"message_received","from":"user"}"#)
                                .unwrap(),
                        ]),
                        handoff: Some(vec![
                            serde_json::from_str(
                                r#"{"when":"task_complete","to":"analyst","type":"handoff"}"#,
                            )
                            .unwrap(),
                        ]),
                        acceptance_criteria: Some(vec![
                            "Data sources and freshness are stated before deep analysis".into(),
                            "Final artifact list (tables, charts, narrative) is agreed".into(),
                        ]),
                        output: Some(
                            serde_json::from_str(
                                r#"{"format":"markdown","requiredSections":["Questions","Data","Deliverables"]}"#,
                            )
                            .unwrap(),
                        ),
                        sla: Some(
                            serde_json::from_str(
                                r#"{"maxDurationPerTaskMinutes":45,"maxTokenPerTask":45000}"#,
                            )
                            .unwrap(),
                        ),
                        limits: Some(
                            serde_json::from_str(r#"{"deniedTools":["Bash"]}"#).unwrap(),
                        ),
                    },
                    TemplateMember {
                        agent_folder: "analyst".into(),
                        role: "worker".into(),
                        subdir: Some("analysis".into()),
                        persona: Some("Data analyst. Process large datasets, compute statistics, identify trends. Write clean analysis code.".into()),
                        responsibilities: Some(vec![
                            "Load and clean data from CSV, JSON, or Parquet files".into(),
                            "Compute descriptive and inferential statistics".into(),
                            "Identify patterns, anomalies, and trends".into(),
                            "Document methodology and assumptions".into(),
                        ]),
                        triggers: Some(vec![
                            serde_json::from_str(r#"{"type":"task_assigned","condition":"assignee == me"}"#).unwrap(),
                        ]),
                        handoff: Some(vec![
                            serde_json::from_str(r#"{"when":"task_complete","to":"visualizer","type":"handoff"}"#).unwrap(),
                        ]),
                        acceptance_criteria: Some(vec![
                            "All statistics are correctly computed".into(),
                            "Edge cases and nulls are handled".into(),
                            "Methodology is documented".into(),
                        ]),
                        output: Some(serde_json::from_str(r#"{"format":"markdown","requiredSections":["Summary","Methodology","Results","Caveats"]}"#).unwrap()),
                        sla: Some(serde_json::from_str(r#"{"maxDurationPerTaskMinutes":120,"maxTokenPerTask":150000}"#).unwrap()),
                        limits: Some(serde_json::from_str(r#"{"allowedBashCommands":["python","python3","node","jq","csvkit","pandas"]}"#).unwrap()),
                    },
                    TemplateMember {
                        agent_folder: "visualizer".into(),
                        role: "worker".into(),
                        subdir: Some("charts".into()),
                        persona: Some("Data visualization specialist. Create clear, informative charts and graphs. Use matplotlib, plotly, or eCharts.".into()),
                        responsibilities: Some(vec![
                            "Create charts from analyst's results".into(),
                            "Generate interactive visualizations when useful".into(),
                            "Export charts as PNG/SVG for reports".into(),
                        ]),
                        triggers: Some(vec![
                            serde_json::from_str(r#"{"type":"message_received","from":"analyst","messageType":"handoff"}"#).unwrap(),
                        ]),
                        handoff: Some(vec![
                            serde_json::from_str(r#"{"when":"task_complete","to":"analyst","type":"result"}"#).unwrap(),
                        ]),
                        acceptance_criteria: Some(vec![
                            "Charts are correctly labeled".into(),
                            "Colors are accessible".into(),
                            "Data matches analyst output".into(),
                        ]),
                        output: None, sla: None, limits: None,
                    },
                ],
                board: Some(TemplateBoard {
                    sections: vec![
                        TemplateBoardSection { section_type: "brief".into(), title: "Analysis Brief".into(), template: Some("Data source, questions to answer, output format...".into()) },
                        TemplateBoardSection { section_type: "guidelines".into(), title: "Analysis Guidelines".into(), template: Some("- Handle missing values explicitly\n- Report confidence intervals\n- Use appropriate statistical tests".into()) },
                        TemplateBoardSection { section_type: "reference".into(), title: "Data Sources".into(), template: Some("(links and descriptions of data files)".into()) },
                    ],
                }),
            },
            CoworkTemplate {
                name: "API Backend".into(),
                description: "API/backend lead plus engineer and test agent for REST APIs with OpenAPI specs".into(),
                icon: Some("ApiOutlined".into()),
                members: vec![
                    TemplateMember {
                        agent_folder: "backend-lead".into(),
                        role: "lead".into(),
                        subdir: Some("lead".into()),
                        persona: Some(
                            "Backend/API lead. Aligns OpenAPI contracts, release readiness, and test strategy between implementation and API testing.".into(),
                        ),
                        responsibilities: Some(vec![
                            "Maintain API brief and non-functional requirements on the Board".into(),
                            "Gate handoffs from implementation to formal API testing".into(),
                            "Track breaking changes and versioning decisions".into(),
                        ]),
                        triggers: Some(vec![
                            serde_json::from_str(
                                r#"{"type":"task_assigned","condition":"assignee == me"}"#,
                            )
                            .unwrap(),
                            serde_json::from_str(r#"{"type":"message_received","from":"user"}"#)
                                .unwrap(),
                        ]),
                        handoff: Some(vec![
                            serde_json::from_str(
                                r#"{"when":"task_complete","to":"backend-dev","type":"handoff"}"#,
                            )
                            .unwrap(),
                        ]),
                        acceptance_criteria: Some(vec![
                            "OpenAPI reflects agreed contract before wide endpoint work".into(),
                            "Exit criteria for api-tester are explicit per milestone".into(),
                        ]),
                        output: Some(
                            serde_json::from_str(
                                r#"{"format":"markdown","requiredSections":["Contract Notes","Milestones","Board"]}"#,
                            )
                            .unwrap(),
                        ),
                        sla: Some(
                            serde_json::from_str(
                                r#"{"maxDurationPerTaskMinutes":45,"maxTokenPerTask":40000}"#,
                            )
                            .unwrap(),
                        ),
                        limits: Some(
                            serde_json::from_str(r#"{"deniedTools":["Bash"]}"#).unwrap(),
                        ),
                    },
                    TemplateMember {
                        agent_folder: "backend-dev".into(),
                        role: "worker".into(),
                        subdir: Some("src".into()),
                        persona: Some("Backend engineer. Build robust REST APIs. Follow OpenAPI spec, handle errors properly, write integration tests.".into()),
                        responsibilities: Some(vec![
                            "Implement API endpoints per spec".into(),
                            "Write OpenAPI/Swagger documentation".into(),
                            "Handle error cases with proper status codes".into(),
                            "Write integration tests for all endpoints".into(),
                        ]),
                        triggers: Some(vec![
                            serde_json::from_str(r#"{"type":"task_assigned","condition":"assignee == me"}"#).unwrap(),
                        ]),
                        handoff: Some(vec![
                            serde_json::from_str(r#"{"when":"task_complete","to":"api-tester","type":"review_request"}"#).unwrap(),
                        ]),
                        acceptance_criteria: Some(vec![
                            "All endpoints return correct status codes".into(),
                            "OpenAPI spec is valid".into(),
                            "Error responses follow RFC 7807".into(),
                        ]),
                        output: Some(serde_json::from_str(r#"{"format":"markdown","requiredSections":["Summary","Endpoints","Error Handling","Test Results"]}"#).unwrap()),
                        sla: Some(serde_json::from_str(r#"{"maxDurationPerTaskMinutes":90,"maxTokenPerTask":80000}"#).unwrap()),
                        limits: None,
                    },
                    TemplateMember {
                        agent_folder: "api-tester".into(),
                        role: "reviewer".into(),
                        subdir: Some("tests".into()),
                        persona: Some("API test engineer. Test every endpoint thoroughly — happy path, edge cases, error conditions, and load characteristics.".into()),
                        responsibilities: Some(vec![
                            "Test all API endpoints from OpenAPI spec".into(),
                            "Test edge cases: nulls, empty arrays, large payloads".into(),
                            "Report performance and error rates".into(),
                        ]),
                        triggers: Some(vec![
                            serde_json::from_str(r#"{"type":"message_received","from":"backend-dev","messageType":"review_request"}"#).unwrap(),
                        ]),
                        handoff: Some(vec![
                            serde_json::from_str(r#"{"when":"task_complete","to":"backend-dev","type":"result"}"#).unwrap(),
                        ]),
                        acceptance_criteria: Some(vec![
                            "Every endpoint tested".into(),
                            "Edge cases covered".into(),
                            "Performance within SLA".into(),
                        ]),
                        output: None, sla: None, limits: None,
                    },
                ],
                board: Some(TemplateBoard {
                    sections: vec![
                        TemplateBoardSection { section_type: "brief".into(), title: "API Brief".into(), template: Some("API purpose, base URL, auth method...".into()) },
                        TemplateBoardSection { section_type: "guidelines".into(), title: "API Guidelines".into(), template: Some("- OpenAPI 3.0+\n- RESTful conventions\n- Error format: RFC 7807\n- Versioning: URL path".into()) },
                        TemplateBoardSection { section_type: "reference".into(), title: "Endpoints Reference".into(), template: Some("(generated OpenAPI spec)".into()) },
                    ],
                }),
            },
            CoworkTemplate {
                name: "Blank".into(),
                description: "Empty workspace — start from scratch and configure everything manually".into(),
                icon: Some("FileOutlined".into()),
                members: vec![],
                board: None,
            },
        ];

        // Write each built-in JSON only if missing (do not overwrite user edits).
        for tmpl in &builtins {
            let filename = tmpl.name.to_lowercase().replace(' ', "-") + ".json";
            let path = dir.join(&filename);
            if path.exists() {
                continue;
            }
            if let Ok(json) = serde_json::to_string_pretty(tmpl) {
                fs::write(&path, json).ok();
            }
        }
    }

    /// List all available templates
    pub fn list_templates(&self, config: &Config) -> Result<Vec<CoworkTemplate>> {
        let dir = &config.paths.workspace_templates_dir;
        if !dir.exists() {
            return Ok(vec![]);
        }

        let mut templates = Vec::new();
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |e| e == "json") {
                    if let Ok(content) = fs::read_to_string(&path) {
                        if let Ok(tmpl) = serde_json::from_str::<CoworkTemplate>(&content) {
                            templates.push(tmpl);
                        }
                    }
                }
            }
        }
        templates.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(templates)
    }

    /// Load a specific template by name
    pub fn get_template(&self, config: &Config, name: &str) -> Result<Option<CoworkTemplate>> {
        let filename = name.to_lowercase().replace(' ', "-") + ".json";
        let path = config.paths.workspace_templates_dir.join(&filename);
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&content).ok())
    }

    /// Create a workspace and optionally apply a template
    pub fn create_workspace_with_template(
        &self,
        db: &Db,
        config: &Config,
        name: &str,
        description: Option<&str>,
        working_dir: Option<&str>,
        template_name: Option<&str>,
        now: &str,
    ) -> Result<CoworkWorkspace> {
        let ws = self.create_workspace(db, config, name, description, working_dir, now)?;

        if let Some(tmpl_name) = template_name {
            if let Some(tmpl) = self.get_template(config, tmpl_name)? {
                // Apply template members
                for m in &tmpl.members {
                    let member = CoworkMember {
                        workspace_id: ws.id.clone(),
                        member_id: m.agent_folder.clone(),
                        role: m.role.clone(),
                        jid: None,
                        subdir: m.subdir.clone(),
                        persona: m.persona.clone(),
                        responsibilities: m
                            .responsibilities
                            .as_ref()
                            .map(|r| serde_json::to_string(r).unwrap_or_default()),
                        triggers: m
                            .triggers
                            .as_ref()
                            .map(|t| serde_json::to_string(t).unwrap_or_default()),
                        handoff_rules: m
                            .handoff
                            .as_ref()
                            .map(|h| serde_json::to_string(h).unwrap_or_default()),
                        acceptance_criteria: m
                            .acceptance_criteria
                            .as_ref()
                            .map(|a| serde_json::to_string(a).unwrap_or_default()),
                        output_format: m
                            .output
                            .as_ref()
                            .map(|o| serde_json::to_string(o).unwrap_or_default()),
                        sla: m
                            .sla
                            .as_ref()
                            .map(|s| serde_json::to_string(s).unwrap_or_default()),
                        limits: m
                            .limits
                            .as_ref()
                            .map(|l| serde_json::to_string(l).unwrap_or_default()),
                        joined_at: now.to_string(),
                        updated_at: now.to_string(),
                    };

                    // Create agent subdir
                    let agent_dir = PathBuf::from(&ws.root_dir)
                        .join("agents")
                        .join(&m.agent_folder);
                    fs::create_dir_all(&agent_dir).ok();

                    db.insert_cowork_member(&member).ok();
                }

                // Apply template board sections
                if let Some(ref board) = tmpl.board {
                    for section in &board.sections {
                        let content = section.template.as_deref().unwrap_or("");
                        self.upsert_board_entry(
                            db,
                            &ws.id,
                            &section.section_type,
                            Some(&section.title),
                            content,
                            "system",
                            now,
                        )
                        .ok();
                    }
                }
            }
        }

        self.fire_changed();
        Ok(ws)
    }
}

impl Default for CoworkManager {
    fn default() -> Self {
        Self::new()
    }
}
