//! Dispatch bridge — DAG task orchestration for the main agent.
//! Port target: src-old/agent/DispatchBridge.ts (777 lines).
//!
//! Phases ported so far:
//! * Phase 1 — types, trait surface, JSON state file persistence with PID lock,
//!   parent/task CRUD via [`DispatchBridge::modify_state`], stale-state recovery
//!   on startup, agent list sync, pause/resume/cancel/has-active-dispatch,
//!   notify_task_done/notify_task_error (state mutation only — no scheduler yet).
//! * Phase 2 — WS notify callback fired on every state mutation; admin-activity
//!   heartbeat task that pings active-parent admins every 2 minutes so their
//!   inactivity timer doesn't fire mid-dispatch.
//!
//! Phase 3+ (DAG scheduler / timeout watcher / virtual workers) lands later.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use crate::agent::persona_registry::PersonaRegistry;
use crate::agent::virtual_worker_pool::VirtualWorkerPool;
use crate::types::GroupBinding;

// ===== Public types =====

/// Subtask status — mirrors TS `DispatchTask.status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DispatchTaskStatus {
    Registered,
    Processing,
    Done,
    Error,
    Timeout,
}

impl DispatchTaskStatus {
    /// Terminal statuses — DAG dependants may proceed once a task hits one of these.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Done | Self::Error | Self::Timeout)
    }
}

impl DispatchTaskStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Registered => "registered",
            Self::Processing => "processing",
            Self::Done => "done",
            Self::Error => "error",
            Self::Timeout => "timeout",
        }
    }
}

/// One subtask inside a dispatch parent group.
/// Wire format mirrors TS `DispatchTask` so the Web Agent Console can render
/// agent names (incl. virtual/persona tasks) without a translation layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchTask {
    pub id: String,
    pub label: String,
    /// Persisted agents: folder. Virtual agents: `"persona:<personaName>"`.
    pub agent_id: String,
    /// Persisted agents: jid. Virtual agents: empty string.
    pub agent_jid: String,
    pub depends_on: Vec<String>,
    pub prompt: String,
    pub status: DispatchTaskStatus,
    pub result: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    /// Timeout budget supplied at creation (seconds); preserved across restarts.
    #[serde(default)]
    pub timeout_seconds: u64,
    pub timeout_at: Option<String>,
    pub completed_at: Option<String>,
    /// True when this task targets a virtual (persona-backed) worker.
    #[serde(default)]
    pub is_virtual: bool,
    /// Persona name when `is_virtual` is true.
    #[serde(default)]
    pub persona_name: Option<String>,
}

/// Parent dispatch (one `dispatch_task` MCP call → N subtasks).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchParent {
    pub id: String,
    pub goal: String,
    pub admin_folder: String,
    /// Workspace path shared by child tasks under this parent.
    pub shared_workspace: Option<String>,
    /// "queued" / "active" / "done" — matches Web `DispatchParent.status`.
    pub status: String,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub tasks: Vec<DispatchTask>,
}

/// Persisted reference to a registered (persistent) agent. Mirrors the
/// `agents[]` array in the TS state file so external dispatch tooling
/// (CLI, MCP) can resolve `name → jid` without re-querying the daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchAgent {
    pub name: String,
    /// Folder identifier (matches `GroupBinding.folder`).
    pub id: String,
    pub jid: String,
    pub channel: String,
}

/// Top-level state file shape (`~/.senclaw/dispatch-state.json`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DispatchState {
    /// Monotonic sequence used to generate `p-…` and `d-…` IDs.
    #[serde(rename = "_seq", default)]
    pub seq: u64,
    #[serde(default)]
    pub agents: Vec<DispatchAgent>,
    #[serde(default)]
    pub parents: Vec<DispatchParent>,
}

/// Callback fired when subtask activity (start/complete/error) should reset
/// the admin agent's inactivity timer.
pub type AdminActivityCallback = Arc<dyn Fn(&str) + Send + Sync>;

// ===== API trait =====

/// Operations AgentPool calls on DispatchBridge.
///
/// Default no-op implementations let partial wiring compile; a concrete
/// `DispatchBridge` will replace them in a later phase.
#[allow(unused_variables)]
pub trait DispatchBridgeApi: Send + Sync {
    /// Notify that a dispatch task completed successfully (with optional final reply).
    fn notify_task_done(&self, task_id: &str, content: &str) {}

    /// Compatibility path when the taskId is unknown — match by agent JID.
    fn notify_reply(&self, agent_jid: &str, content: &str) {}

    /// Notify that the agent for a dispatch task errored / timed out.
    fn notify_error(&self, agent_jid: &str, error: &str) {}

    /// Snapshot of all parent dispatches (used by AgentPool to build resume hints).
    fn get_parents(&self) -> Vec<DispatchParent> {
        Vec::new()
    }

    /// Inject a callback fired on subtask activity, used by AgentPool to reset
    /// the admin agent's inactivity timer.
    fn set_admin_activity_callback(&self, cb: AdminActivityCallback) {}

    /// Whether there are active dispatch tasks under `folder`.
    fn has_active_dispatch(&self, _folder: &str) -> bool {
        false
    }

    /// Pause dispatch scheduling for `folder`; returns child JIDs to pause.
    fn pause_admin(&self, _folder: &str) -> Vec<String> {
        Vec::new()
    }

    /// Resume dispatch scheduling for `folder`.
    fn resume_admin(&self, _folder: &str) {}

    /// Cancel all active/queued parents for `folder`; returns child JIDs to stop.
    fn cancel_admin_parents(&self, _folder: &str) -> Vec<String> {
        Vec::new()
    }

    /// Build a resume hint for the admin agent listing active dispatches.
    fn build_dispatch_resume_hint(&self, folder: &str) -> String
    where
        Self: Sized,
    {
        build_dispatch_resume_hint(Some(self), folder).unwrap_or_default()
    }
}

/// No-op stub used until a real DispatchBridge ships.
pub struct NoopDispatchBridge;

impl DispatchBridgeApi for NoopDispatchBridge {}

// ===== Resume hint builder (free function — used by AgentPool getOrCreate) =====

/// Build a `[System Note]` reminder listing in-flight dispatches under
/// `admin_folder`. Returns `None` when nothing is active. Mirrors
/// `buildDispatchResumeHint` in `src-old/agent/AgentPool.ts:76`.
pub fn build_dispatch_resume_hint(
    bridge: Option<&dyn DispatchBridgeApi>,
    admin_folder: &str,
) -> Option<String> {
    let bridge = bridge?;
    let parents: Vec<_> = bridge
        .get_parents()
        .into_iter()
        .filter(|p| p.admin_folder == admin_folder && p.status == "active")
        .collect();
    if parents.is_empty() {
        return None;
    }
    let mut lines = vec![
        "[System Note] You have previously dispatched the following tasks via dispatch_task. They are still running; do not recreate or redispatch them:".to_string(),
    ];
    for parent in &parents {
        lines.push(format!("- Task group {} (goal: {})", parent.id, parent.goal));
        for task in &parent.tasks {
            let preview: String = task.prompt.chars().take(80).collect();
            lines.push(format!(
                "  • [{}] → {}：{}（{}）",
                task.label,
                task.agent_id,
                preview,
                task.status.label()
            ));
        }
    }
    lines.push("You will be notified when these tasks complete. Please wait for results.".to_string());
    Some(lines.join("\n"))
}

// ===== Real DispatchBridge =====

/// Callback invoked after every state mutation. Receives the full parents
/// snapshot serialized as JSON in the wire format the Web Agent Console
/// consumes (`dispatch:update.parents`).
pub type WsNotifyCallback = Arc<dyn Fn(&serde_json::Value) + Send + Sync>;

/// Callback invoked when the scheduler decides a task is ready to run.
/// Arguments: `(jid, task_id, augmented_prompt, workspace_dir)`. An empty
/// `workspace_dir` means "do not switch the sub-agent's working directory".
pub type SendToAgentCallback =
    Arc<dyn Fn(&str, &str, &str, &str) + Send + Sync>;

/// Callback invoked when the last in-flight dispatch task on `jid` finishes,
/// so the sub-agent can be restored to its own working directory.
pub type RevertWorkspaceCallback = Arc<dyn Fn(&str) + Send + Sync>;

/// Polling cadence for the scheduler tick (timeouts + ready-task launch).
const POLL_INTERVAL: Duration = Duration::from_millis(300);
/// Cleanup cadence — drops `done` parents older than [`CLEANUP_RETENTION`].
const CLEANUP_INTERVAL: Duration = Duration::from_secs(10 * 60);
/// How long completed parents are kept before being garbage-collected.
const CLEANUP_RETENTION_SECONDS: i64 = 60 * 60;

/// Concrete `DispatchBridgeApi` implementation backed by a JSON state file.
///
/// Phase 1+2 scope: state persistence, parent/task tracking, WS notify, admin
/// activity heartbeat, pause/resume/cancel. The DAG scheduler that actually
/// dispatches `registered` tasks to sub-agents lands in a later phase — until
/// then the bridge serves as the single source of truth visible to the Web UI
/// and lets `notify_*` calls correctly mark state.
pub struct DispatchBridge {
    state_path: PathBuf,
    inner: Mutex<Inner>,
    /// Optional callback fired post-write with the parents snapshot.
    ws_notify: Mutex<Option<WsNotifyCallback>>,
    /// Optional callback fired when sub-task activity should reset the admin
    /// agent's inactivity timer.
    on_admin_activity: Mutex<Option<AdminActivityCallback>>,
    /// Scheduler hand-off — invoked when a `registered` task is ready to run.
    send_to_agent: Mutex<Option<SendToAgentCallback>>,
    /// Workspace restore hand-off — invoked after the last in-flight task on
    /// a jid finishes.
    revert_workspace: Mutex<Option<RevertWorkspaceCallback>>,
    /// Persona registry — required for virtual-agent dispatch (Phase 5).
    persona_registry: Mutex<Option<Arc<Mutex<PersonaRegistry>>>>,
    /// Virtual worker pool — required for virtual-agent dispatch (Phase 5).
    virtual_worker_pool: Mutex<Option<Arc<VirtualWorkerPool>>>,
    /// Self-reference populated in `start()` so spawned futures can call back
    /// into the bridge for `notify_task_done` / `notify_task_error`.
    self_weak: Mutex<Option<Weak<Self>>>,
    /// JSON snapshot of the parents array last sent to admin clients via
    /// `ws_notify`. Used in `process_pending` to detect external state-file
    /// mutations (e.g. from the MCP dispatch stdio process) and push a
    /// `dispatch:update` without waiting for a task transition.
    last_notified_parents_json: Mutex<String>,
}

#[derive(Default)]
struct Inner {
    /// taskId → jid (primary index for in-flight persistent-agent tasks).
    active_tasks: HashMap<String, String>,
    /// jid → set of taskIds (secondary index for fast per-agent lookup).
    active_agent_tasks: HashMap<String, HashSet<String>>,
    /// Admin folders with scheduling currently paused.
    paused_admins: HashSet<String>,
    /// Handle to the spawned heartbeat task — dropped on `stop()` to cancel it.
    heartbeat_handle: Option<tokio::task::JoinHandle<()>>,
    /// Handle to the scheduler poll task.
    poll_handle: Option<tokio::task::JoinHandle<()>>,
    /// Handle to the cleanup task.
    cleanup_handle: Option<tokio::task::JoinHandle<()>>,
}

impl DispatchBridge {
    /// Create a new bridge bound to `state_path` (typically
    /// `Config::paths::dispatch_state_path`). Does not start the heartbeat —
    /// call [`DispatchBridge::start`] for that.
    pub fn new(state_path: impl Into<PathBuf>) -> Self {
        Self {
            state_path: state_path.into(),
            inner: Mutex::new(Inner::default()),
            ws_notify: Mutex::new(None),
            on_admin_activity: Mutex::new(None),
            send_to_agent: Mutex::new(None),
            revert_workspace: Mutex::new(None),
            persona_registry: Mutex::new(None),
            virtual_worker_pool: Mutex::new(None),
            self_weak: Mutex::new(None),
            last_notified_parents_json: Mutex::new(String::new()),
        }
    }

    /// Inject the persona registry + virtual worker pool used by the virtual
    /// dispatch path. Without these, virtual tasks remain `registered` forever
    /// (and `can_start_task` returns false).
    pub fn set_virtual_workers(
        &self,
        registry: Arc<Mutex<PersonaRegistry>>,
        pool: Arc<VirtualWorkerPool>,
    ) {
        *self.persona_registry.lock().unwrap() = Some(registry);
        *self.virtual_worker_pool.lock().unwrap() = Some(pool);
    }

    /// Inject the WebSocket notify callback. Called after every state mutation
    /// with the parents array serialized as JSON (camelCase wire format).
    pub fn set_ws_notify(&self, cb: WsNotifyCallback) {
        *self.ws_notify.lock().unwrap() = Some(cb);
    }

    /// Inject the scheduler hand-off used to actually deliver augmented prompts
    /// to sub-agents. Without it the bridge still tracks state but no task
    /// will ever transition out of `registered`.
    pub fn set_send_to_agent(&self, cb: SendToAgentCallback) {
        *self.send_to_agent.lock().unwrap() = Some(cb);
    }

    /// Inject the workspace-restore hand-off used after the last in-flight
    /// task on a sub-agent finishes.
    pub fn set_revert_workspace(&self, cb: RevertWorkspaceCallback) {
        *self.revert_workspace.lock().unwrap() = Some(cb);
    }

    /// Boot-time recovery + heartbeat startup. Mirrors TS `start()`:
    /// 1. Mark any leftover `active`/`queued` parents from a previous run as
    ///    `done` with their in-flight tasks marked `error: "Interrupted: …"`.
    /// 2. Spawn a 2-minute heartbeat that fires `on_admin_activity` for each
    ///    admin with an active parent — keeps the admin agent's `processAndWait`
    ///    timer from firing while it waits for dispatch results.
    pub fn start(self: &Arc<Self>) {
        *self.self_weak.lock().unwrap() = Some(Arc::downgrade(self));
        self.recover_stale_state();

        // Heartbeat — every 2 min, ping admins with active parents.
        let weak = Arc::downgrade(self);
        let heartbeat = tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(2 * 60));
            tick.tick().await; // skip the immediate first tick
            loop {
                tick.tick().await;
                let Some(this) = weak.upgrade() else { break };
                this.heartbeat_active_admins();
            }
        });

        // Scheduler poll — every 300ms, check timeouts then launch ready tasks.
        let weak = Arc::downgrade(self);
        let poll = tokio::spawn(async move {
            let mut tick = tokio::time::interval(POLL_INTERVAL);
            tick.tick().await;
            loop {
                tick.tick().await;
                let Some(this) = weak.upgrade() else { break };
                this.process_pending();
            }
        });

        // Cleanup — every 10 min, drop done parents older than 1h.
        let weak = Arc::downgrade(self);
        let cleanup = tokio::spawn(async move {
            let mut tick = tokio::time::interval(CLEANUP_INTERVAL);
            tick.tick().await;
            loop {
                tick.tick().await;
                let Some(this) = weak.upgrade() else { break };
                this.cleanup();
            }
        });

        let mut inner = self.inner.lock().unwrap();
        inner.heartbeat_handle = Some(heartbeat);
        inner.poll_handle = Some(poll);
        inner.cleanup_handle = Some(cleanup);
        drop(inner);

        // First-tick: drop already-stale parents immediately.
        self.cleanup();
        tracing::info!(
            "[DispatchBridge] Started, state: {}",
            self.state_path.display()
        );
    }

    /// Cancel all background tasks. Called on graceful shutdown.
    pub fn stop(&self) {
        let mut inner = self.inner.lock().unwrap();
        for h in [
            inner.heartbeat_handle.take(),
            inner.poll_handle.take(),
            inner.cleanup_handle.take(),
        ]
        .into_iter()
        .flatten()
        {
            h.abort();
        }
    }

    fn recover_stale_state(&self) {
        let now = chrono::Utc::now().to_rfc3339();
        let _ = self.modify_state(|state| {
            for parent in &mut state.parents {
                if parent.status == "active" || parent.status == "queued" {
                    for task in &mut parent.tasks {
                        if matches!(
                            task.status,
                            DispatchTaskStatus::Processing | DispatchTaskStatus::Registered
                        ) {
                            task.status = DispatchTaskStatus::Error;
                            task.result = Some("Interrupted: service restarted".into());
                            task.completed_at = Some(now.clone());
                        }
                    }
                    parent.status = "done".into();
                    parent.completed_at = Some(now.clone());
                }
            }
        });
    }

    fn heartbeat_active_admins(&self) {
        let state = match self.read_state() {
            Ok(s) => s,
            Err(_) => return,
        };
        let mut seen: HashSet<&str> = HashSet::new();
        for parent in &state.parents {
            if parent.status == "active" && seen.insert(parent.admin_folder.as_str()) {
                if let Some(cb) = self.on_admin_activity.lock().unwrap().as_ref() {
                    cb(&parent.admin_folder);
                }
            }
        }
    }

    // ---- Active-task in-memory tracking ----

    fn add_active_task(&self, task_id: &str, jid: &str) {
        let mut inner = self.inner.lock().unwrap();
        inner.active_tasks.insert(task_id.to_string(), jid.to_string());
        inner
            .active_agent_tasks
            .entry(jid.to_string())
            .or_default()
            .insert(task_id.to_string());
    }

    fn remove_active_task(&self, task_id: &str) -> Option<String> {
        let mut inner = self.inner.lock().unwrap();
        let jid = inner.active_tasks.remove(task_id)?;
        if let Some(set) = inner.active_agent_tasks.get_mut(&jid) {
            set.remove(task_id);
            if set.is_empty() {
                inner.active_agent_tasks.remove(&jid);
            }
        }
        Some(jid)
    }

    fn has_active_jid_tasks(&self, jid: &str) -> bool {
        self.inner
            .lock()
            .unwrap()
            .active_agent_tasks
            .get(jid)
            .map(|s| !s.is_empty())
            .unwrap_or(false)
    }

    // ---- Public-ish helpers used by AgentPool / MCP layer ----

    /// Sync the persisted `agents[]` list from current group bindings. Called
    /// whenever GroupManager mutates its set of registered groups.
    pub fn update_agents(&self, groups: &[GroupBinding]) {
        let agents: Vec<DispatchAgent> = groups
            .iter()
            .filter(|g| !g.is_admin)
            .map(|g| DispatchAgent {
                name: g.name.clone(),
                id: g.folder.clone(),
                jid: g.jid.clone(),
                channel: g.channel.clone(),
            })
            .collect();
        let _ = self.modify_state(|state| {
            state.agents = agents;
        });
    }

    /// Mark `task_id` as `done`. Mirrors TS `notifyTaskDone` minus the
    /// scheduler kick (Phase 3) and workspace revert (Phase 4).
    fn mark_task_done(&self, task_id: &str, text: &str) {
        let jid = self.remove_active_task(task_id);
        let now = chrono::Utc::now().to_rfc3339();
        let mut task_admin: Option<String> = None;
        let mut completed_admin: Option<String> = None;
        let _ = self.modify_state(|state| {
            for parent in &mut state.parents {
                if let Some(task) = parent.tasks.iter_mut().find(|t| t.id == task_id) {
                    if task.status.is_terminal() {
                        return;
                    }
                    task_admin = Some(parent.admin_folder.clone());
                    task.status = DispatchTaskStatus::Done;
                    task.result = Some(text.to_string());
                    task.completed_at = Some(now.clone());
                    if parent.tasks.iter().all(|t| t.status.is_terminal()) {
                        parent.status = "done".into();
                        parent.completed_at = Some(now.clone());
                        completed_admin = Some(parent.admin_folder.clone());
                    }
                    return;
                }
            }
        });
        tracing::info!(
            "[DispatchBridge] Task {task_id} done{}",
            if let Some(j) = &jid {
                format!(" for {j}")
            } else {
                " (virtual)".into()
            }
        );
        if let Some(folder) = task_admin {
            self.fire_admin_activity(&folder);
        }
        if let Some(folder) = completed_admin {
            self.activate_next_queued(&folder);
        }
        self.process_next_pending();
        if let Some(j) = jid.as_ref() {
            if !self.has_active_jid_tasks(j) {
                self.fire_revert_workspace(j);
            }
        }
    }

    /// Mark `task_id` as `error` with `error_message`. Same caveats as
    /// `mark_task_done`.
    fn mark_task_error(&self, task_id: &str, error_message: &str) {
        let jid = self.remove_active_task(task_id);
        let now = chrono::Utc::now().to_rfc3339();
        let mut task_admin: Option<String> = None;
        let mut completed_admin: Option<String> = None;
        let _ = self.modify_state(|state| {
            for parent in &mut state.parents {
                if let Some(task) = parent.tasks.iter_mut().find(|t| t.id == task_id) {
                    if task.status.is_terminal() {
                        return;
                    }
                    task_admin = Some(parent.admin_folder.clone());
                    task.status = DispatchTaskStatus::Error;
                    task.result = Some(error_message.to_string());
                    task.completed_at = Some(now.clone());
                    if parent.tasks.iter().all(|t| t.status.is_terminal()) {
                        parent.status = "done".into();
                        parent.completed_at = Some(now.clone());
                        completed_admin = Some(parent.admin_folder.clone());
                    }
                    return;
                }
            }
        });
        tracing::warn!(
            "[DispatchBridge] Task {task_id} error{}: {error_message}",
            if let Some(j) = &jid {
                format!(" for {j}")
            } else {
                " (virtual)".into()
            }
        );
        if let Some(folder) = task_admin {
            self.fire_admin_activity(&folder);
        }
        if let Some(folder) = completed_admin {
            self.activate_next_queued(&folder);
        }
        self.process_next_pending();
        if let Some(j) = jid.as_ref() {
            if !self.has_active_jid_tasks(j) {
                self.fire_revert_workspace(j);
            }
        }
    }

    /// Find the earliest `processing` task owned by `jid` (oldest `started_at`)
    /// — used by the by-jid fallback notify paths.
    fn earliest_processing_for_jid(&self, jid: &str) -> Option<String> {
        let set = self
            .inner
            .lock()
            .unwrap()
            .active_agent_tasks
            .get(jid)?
            .clone();
        if set.is_empty() {
            return None;
        }
        let state = self.read_state().ok()?;
        let mut best: Option<(String, String)> = None;
        for parent in &state.parents {
            for task in &parent.tasks {
                if !set.contains(&task.id) || task.status != DispatchTaskStatus::Processing {
                    continue;
                }
                let started = task.started_at.clone().unwrap_or_default();
                match &best {
                    Some((_, ts)) if started >= *ts => {}
                    _ => best = Some((task.id.clone(), started)),
                }
            }
        }
        best.map(|(id, _)| id)
    }

    /// Promote the oldest `queued` parent under `admin_folder` to `active`.
    /// On promotion, snapshot the admin's current working dir from its
    /// workspace-state file into `shared_workspace` so child tasks inherit it.
    fn activate_next_queued(&self, admin_folder: &str) {
        let workspace = self.read_admin_workspace(admin_folder);
        let mut activated_id: Option<String> = None;
        let _ = self.modify_state(|state| {
            let mut candidates: Vec<&mut DispatchParent> = state
                .parents
                .iter_mut()
                .filter(|p| p.status == "queued" && p.admin_folder == admin_folder)
                .collect();
            candidates.sort_by(|a, b| a.created_at.cmp(&b.created_at));
            if let Some(next) = candidates.into_iter().next() {
                next.status = "active".into();
                if !workspace.is_empty() {
                    next.shared_workspace = Some(workspace.clone());
                }
                activated_id = Some(next.id.clone());
            }
        });
        if let Some(id) = activated_id {
            tracing::info!(
                "[DispatchBridge] Activated next queued parent {id} for admin: {admin_folder}"
            );
        }
    }

    /// Read the admin agent's current working dir from
    /// `<senclaw_home>/workspace-state-<folder>.json`. Returns empty string
    /// when the file doesn't exist or can't be parsed (matches TS behavior:
    /// child agent then keeps its own workdir).
    fn read_admin_workspace(&self, admin_folder: &str) -> String {
        let Some(home) = self.state_path.parent() else {
            return String::new();
        };
        let path = home.join(format!("workspace-state-{admin_folder}.json"));
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return String::new();
        };
        #[derive(Deserialize)]
        struct WorkspaceState {
            #[serde(rename = "currentDir", default)]
            current_dir: String,
        }
        serde_json::from_str::<WorkspaceState>(&raw)
            .map(|w| w.current_dir)
            .unwrap_or_default()
    }

    fn fire_admin_activity(&self, folder: &str) {
        if let Some(cb) = self.on_admin_activity.lock().unwrap().as_ref() {
            cb(folder);
        }
    }

    fn fire_revert_workspace(&self, jid: &str) {
        if let Some(cb) = self.revert_workspace.lock().unwrap().as_ref() {
            cb(jid);
        }
    }

    // ---- Scheduler ====

    /// Polling tick: timeout-check active tasks, then launch ready tasks.
    /// Mirrors TS `processPending`. Virtual-agent scheduling is deferred to
    /// Phase 5; for now `start_task` only fires for persistent agents.
    fn process_pending(&self) {
        let Ok(state) = self.read_state() else { return };

        // Detect external state-file mutations (e.g. MCP dispatch stdio
        // process wrote parents via `modify_state_file`). Fire ws_notify
        // immediately so admin clients see the change without waiting for a
        // task transition inside this bridge.
        {
            let parents_json = serde_json::to_string(&state.parents).unwrap_or_default();
            let last = self.last_notified_parents_json.lock().unwrap();
            if parents_json != *last {
                drop(last);
                tracing::info!(
                    "[DispatchBridge] External state change detected ({} parent(s))",
                    state.parents.len()
                );
                if let Some(cb) = self.ws_notify.lock().unwrap().as_ref() {
                    let v = serde_json::to_value(&state.parents).unwrap_or(serde_json::Value::Null);
                    cb(&v);
                }
                *self.last_notified_parents_json.lock().unwrap() = parents_json;
            }
        }

        let now = chrono::Utc::now();

        // 1. Timeout sweep — only persistent-agent tasks; virtual tasks have
        //    their own timeout inside the worker pool.
        let mut timed_out: Vec<(String, String)> = Vec::new(); // (task_id, jid)
        for parent in &state.parents {
            if parent.status != "active" {
                continue;
            }
            for task in &parent.tasks {
                if task.status != DispatchTaskStatus::Processing || task.is_virtual {
                    continue;
                }
                let Some(deadline_str) = &task.timeout_at else { continue };
                let Ok(deadline) = chrono::DateTime::parse_from_rfc3339(deadline_str) else {
                    continue;
                };
                if deadline.with_timezone(&chrono::Utc) < now {
                    timed_out.push((task.id.clone(), task.agent_jid.clone()));
                }
            }
        }
        for (task_id, jid) in &timed_out {
            self.mark_task_timeout(task_id, jid);
        }

        // 2. Launch ready tasks. Re-read state since timeout sweep may have
        //    mutated it (and freed up jid concurrency slots).
        let Ok(state) = self.read_state() else { return };
        let paused = self.inner.lock().unwrap().paused_admins.clone();
        for parent in &state.parents {
            if parent.status != "active" || paused.contains(&parent.admin_folder) {
                continue;
            }
            for task in &parent.tasks {
                if task.status == DispatchTaskStatus::Registered
                    && self.can_start_task(task, &parent.tasks)
                {
                    self.start_task(parent, task);
                }
            }
        }
    }

    /// After a task completes, scan all active parents for newly-unblocked
    /// `registered` tasks and start them. Cheaper than waiting for the next
    /// poll tick. Also detects external state changes like `process_pending`.
    fn process_next_pending(&self) {
        let Ok(state) = self.read_state() else { return };

        // Detect external state-file mutations (same logic as process_pending).
        {
            let parents_json = serde_json::to_string(&state.parents).unwrap_or_default();
            let last = self.last_notified_parents_json.lock().unwrap();
            if parents_json != *last {
                drop(last);
                if let Some(cb) = self.ws_notify.lock().unwrap().as_ref() {
                    let v = serde_json::to_value(&state.parents).unwrap_or(serde_json::Value::Null);
                    cb(&v);
                }
                *self.last_notified_parents_json.lock().unwrap() = parents_json;
            }
        }

        let paused = self.inner.lock().unwrap().paused_admins.clone();
        for parent in &state.parents {
            if parent.status != "active" || paused.contains(&parent.admin_folder) {
                continue;
            }
            for task in &parent.tasks {
                if task.status == DispatchTaskStatus::Registered
                    && self.can_start_task(task, &parent.tasks)
                {
                    self.start_task(parent, task);
                }
            }
        }
    }

    /// Dependency satisfied + concurrency slot free.
    /// * Persistent agents — same jid limited to one in-flight task.
    /// * Virtual agents — `VirtualWorkerPool::get_active_count(persona) <
    ///   persona.max_concurrent`. If persona / pool aren't wired the task is
    ///   un-startable (prevents the scheduler from trying to fire something
    ///   it can't deliver).
    fn can_start_task(&self, task: &DispatchTask, all_tasks: &[DispatchTask]) -> bool {
        if !is_ready(task, all_tasks) {
            return false;
        }
        if task.is_virtual {
            let Some(persona_name) = task.persona_name.as_deref() else {
                return false;
            };
            let pool = self.virtual_worker_pool.lock().unwrap().clone();
            let registry = self.persona_registry.lock().unwrap().clone();
            let (Some(pool), Some(registry)) = (pool, registry) else {
                return false;
            };
            let max = {
                let reg = registry.lock().unwrap();
                let Some(p) = reg.get(persona_name) else { return false };
                p.max_concurrent
            };
            return pool.get_active_count(persona_name) < max;
        }
        !self.has_active_jid_tasks(&task.agent_jid)
    }

    /// Build the augmented prompt and hand it off to `send_to_agent`.
    /// Transitions the task to `processing` with `started_at` / `timeout_at`
    /// stamped in the same `modify_state` write.
    fn start_task(&self, parent: &DispatchParent, task: &DispatchTask) {
        let augmented = build_augmented_prompt(parent, task);
        let started_at = chrono::Utc::now();
        let started_at_iso = started_at.to_rfc3339();
        let timeout_at_iso = (started_at
            + chrono::Duration::seconds(task.timeout_seconds as i64))
        .to_rfc3339();

        let task_id = task.id.clone();
        let started_clone = started_at_iso.clone();
        let timeout_clone = timeout_at_iso.clone();
        let _ = self.modify_state(|state| {
            for p in &mut state.parents {
                if let Some(t) = p.tasks.iter_mut().find(|x| x.id == task_id) {
                    t.status = DispatchTaskStatus::Processing;
                    t.started_at = Some(started_clone.clone());
                    t.timeout_at = Some(timeout_clone.clone());
                }
            }
        });

        let target = if task.is_virtual {
            format!("persona:{}", task.persona_name.as_deref().unwrap_or(""))
        } else {
            task.agent_jid.clone()
        };
        let preview: String = task.prompt.chars().take(50).collect();
        tracing::info!(
            "[DispatchBridge] Starting {}({}) → {target}: \"{preview}\"",
            task.id,
            task.label
        );

        let workspace = parent.shared_workspace.clone().unwrap_or_default();

        if task.is_virtual {
            // Virtual path: spawn an async run; concurrency is governed by the
            // VirtualWorkerPool (we already gated on `get_active_count` in
            // `can_start_task`). No active_tasks tracking — no jid to track.
            let pool = self.virtual_worker_pool.lock().unwrap().clone();
            let registry = self.persona_registry.lock().unwrap().clone();
            let persona_name = match task.persona_name.clone() {
                Some(n) => n,
                None => {
                    self.mark_task_error(&task.id, "Virtual task missing persona name");
                    return;
                }
            };
            let (Some(pool), Some(registry)) = (pool, registry) else {
                self.mark_task_error(
                    &task.id,
                    &format!(
                        "Virtual agent setup error: persona \"{persona_name}\" not available"
                    ),
                );
                return;
            };
            let persona = {
                let reg = registry.lock().unwrap();
                match reg.get(&persona_name) {
                    Some(p) => p.clone(),
                    None => {
                        drop(reg);
                        self.mark_task_error(
                            &task.id,
                            &format!(
                                "Virtual agent setup error: persona \"{persona_name}\" not available"
                            ),
                        );
                        return;
                    }
                }
            };
            let timeout = if task.timeout_seconds > 0 {
                Some(Duration::from_secs(task.timeout_seconds))
            } else {
                None
            };
            let weak = self.self_weak.lock().unwrap().clone();
            let task_id = task.id.clone();
            let cwd = if workspace.is_empty() {
                std::env::current_dir()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default()
            } else {
                workspace
            };
            tokio::spawn(async move {
                let outcome = pool
                    .run(&persona, &augmented, &cwd, Some(&task_id), timeout)
                    .await;
                let Some(arc) = weak.and_then(|w| w.upgrade()) else { return };
                match outcome {
                    Ok(r) => arc.mark_task_done(&task_id, &r.result),
                    Err(e) => arc.mark_task_error(&task_id, &e.to_string()),
                }
            });
            return;
        }

        // Persistent-agent path.
        self.add_active_task(&task.id, &task.agent_jid);
        let cb = self.send_to_agent.lock().unwrap().clone();
        match cb {
            Some(cb) => {
                cb(&task.agent_jid, &task.id, &augmented, &workspace);
            }
            None => {
                self.remove_active_task(&task.id);
                self.mark_task_error(
                    &task.id,
                    "send_to_agent callback not wired — DispatchBridge cannot dispatch",
                );
                if !self.has_active_jid_tasks(&task.agent_jid) {
                    self.fire_revert_workspace(&task.agent_jid);
                }
            }
        }
    }

    fn mark_task_timeout(&self, task_id: &str, jid: &str) {
        let now = chrono::Utc::now().to_rfc3339();
        let mut completed_admin: Option<String> = None;
        let _ = self.modify_state(|state| {
            for parent in &mut state.parents {
                if let Some(task) = parent.tasks.iter_mut().find(|t| t.id == task_id) {
                    task.status = DispatchTaskStatus::Timeout;
                    task.completed_at = Some(now.clone());
                    if parent.tasks.iter().all(|t| t.status.is_terminal()) {
                        parent.status = "done".into();
                        parent.completed_at = Some(now.clone());
                        completed_admin = Some(parent.admin_folder.clone());
                    }
                    return;
                }
            }
        });
        self.remove_active_task(task_id);
        tracing::warn!("[DispatchBridge] Task {task_id} timed out");
        if let Some(folder) = completed_admin {
            self.activate_next_queued(&folder);
        }
        if !jid.is_empty() && !self.has_active_jid_tasks(jid) {
            self.fire_revert_workspace(jid);
        }
    }

    /// Drop `done` parents whose `completed_at` is older than
    /// [`CLEANUP_RETENTION_SECONDS`]. Mirrors TS `cleanup`.
    fn cleanup(&self) {
        let cutoff = chrono::Utc::now() - chrono::Duration::seconds(CLEANUP_RETENTION_SECONDS);
        let _ = self.modify_state(|state| {
            state.parents.retain(|p| {
                if p.status != "done" {
                    return true;
                }
                let Some(ts) = &p.completed_at else { return true };
                let Ok(completed) = chrono::DateTime::parse_from_rfc3339(ts) else {
                    return true;
                };
                completed.with_timezone(&chrono::Utc) >= cutoff
            });
        });
    }

    // ---- File I/O ----

    fn read_state(&self) -> std::io::Result<DispatchState> {
        match std::fs::read_to_string(&self.state_path) {
            Ok(raw) => serde_json::from_str(&raw)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(DispatchState::default()),
            Err(e) => Err(e),
        }
    }

    fn write_state(&self, state: &DispatchState) -> std::io::Result<()> {
        if let Some(parent) = self.state_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(state)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(&self.state_path, json)
    }

    /// Atomic read-modify-write under a PID-stamped lock file (`<state>.lock`).
    /// On success, fires the WS notify callback with the new parents snapshot.
    /// Mirrors TS `modifyState` including stale-lock recovery.
    pub fn modify_state<F>(&self, fn_mut: F) -> std::io::Result<()>
    where
        F: FnOnce(&mut DispatchState),
    {
        let lock_path = lock_path_for(&self.state_path);
        if !acquire_lock(&lock_path) {
            tracing::warn!("[DispatchBridge] Failed to acquire state lock, skipping modification");
            return Err(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "lock contention",
            ));
        }
        let result = (|| -> std::io::Result<DispatchState> {
            let mut state = self.read_state()?;
            fn_mut(&mut state);
            self.write_state(&state)?;
            Ok(state)
        })();
        let _ = std::fs::remove_file(&lock_path);

        let state = result?;
        // Only fire ws_notify when parents actually changed. This avoids
        // noise from update_agents() periodic syncs that only touch agents[].
        let parents_json = serde_json::to_string(&state.parents).unwrap_or_default();
        let mut last = self.last_notified_parents_json.lock().unwrap();
        if parents_json != *last {
            *last = parents_json.clone();
            drop(last);
            if let Some(cb) = self.ws_notify.lock().unwrap().as_ref() {
                let parents =
                    serde_json::to_value(&state.parents).unwrap_or(serde_json::Value::Null);
                cb(&parents);
            }
        }
        Ok(())
    }
}

impl DispatchBridgeApi for DispatchBridge {
    fn notify_task_done(&self, task_id: &str, content: &str) {
        self.mark_task_done(task_id, content);
    }

    fn notify_reply(&self, agent_jid: &str, content: &str) {
        if let Some(task_id) = self.earliest_processing_for_jid(agent_jid) {
            self.mark_task_done(&task_id, content);
        }
    }

    fn notify_error(&self, agent_jid: &str, error: &str) {
        if let Some(task_id) = self.earliest_processing_for_jid(agent_jid) {
            self.mark_task_error(&task_id, error);
        }
    }

    fn get_parents(&self) -> Vec<DispatchParent> {
        self.read_state().map(|s| s.parents).unwrap_or_default()
    }

    fn set_admin_activity_callback(&self, cb: AdminActivityCallback) {
        *self.on_admin_activity.lock().unwrap() = Some(cb);
    }

    fn has_active_dispatch(&self, folder: &str) -> bool {
        self.read_state()
            .map(|s| {
                s.parents.iter().any(|p| {
                    p.admin_folder == folder && (p.status == "active" || p.status == "queued")
                })
            })
            .unwrap_or(false)
    }

    fn pause_admin(&self, folder: &str) -> Vec<String> {
        self.inner
            .lock()
            .unwrap()
            .paused_admins
            .insert(folder.to_string());
        let mut child_jids: Vec<String> = Vec::new();
        if let Ok(state) = self.read_state() {
            let active = self.inner.lock().unwrap().active_tasks.clone();
            for parent in &state.parents {
                if parent.admin_folder != folder || parent.status != "active" {
                    continue;
                }
                for task in &parent.tasks {
                    if task.status == DispatchTaskStatus::Processing
                        && !task.agent_jid.is_empty()
                        && active.contains_key(&task.id)
                    {
                        child_jids.push(task.agent_jid.clone());
                    }
                }
            }
        }
        tracing::info!(
            "[DispatchBridge] pauseAdmin({folder}): blocked scheduling, child jids: [{}]",
            child_jids.join(", ")
        );
        child_jids
    }

    fn resume_admin(&self, folder: &str) {
        self.inner
            .lock()
            .unwrap()
            .paused_admins
            .remove(folder);
        tracing::info!("[DispatchBridge] resumeAdmin({folder}): scheduling unblocked");
    }

    fn cancel_admin_parents(&self, folder: &str) -> Vec<String> {
        let mut affected: Vec<String> = Vec::new();
        let mut to_remove: Vec<String> = Vec::new();
        let mut virtual_to_cancel: Vec<String> = Vec::new();
        let now = chrono::Utc::now().to_rfc3339();
        let _ = self.modify_state(|state| {
            for parent in &mut state.parents {
                if parent.admin_folder != folder {
                    continue;
                }
                if parent.status != "active" && parent.status != "queued" {
                    continue;
                }
                for task in &mut parent.tasks {
                    match task.status {
                        DispatchTaskStatus::Processing => {
                            if task.is_virtual {
                                virtual_to_cancel.push(task.id.clone());
                            } else if !task.agent_jid.is_empty() {
                                affected.push(task.agent_jid.clone());
                            }
                            to_remove.push(task.id.clone());
                            task.status = DispatchTaskStatus::Error;
                            task.result = Some("Cancelled: admin agent stopped".into());
                            task.completed_at = Some(now.clone());
                        }
                        DispatchTaskStatus::Registered => {
                            task.status = DispatchTaskStatus::Error;
                            task.result = Some("Cancelled: admin agent stopped".into());
                            task.completed_at = Some(now.clone());
                        }
                        _ => {}
                    }
                }
                parent.status = "done".into();
                parent.completed_at = Some(now.clone());
            }
        });
        for id in &to_remove {
            self.remove_active_task(id);
        }
        if !virtual_to_cancel.is_empty() {
            if let Some(pool) = self.virtual_worker_pool.lock().unwrap().clone() {
                for id in &virtual_to_cancel {
                    pool.cancel_task(id);
                }
            }
        }
        self.inner.lock().unwrap().paused_admins.remove(folder);
        if !affected.is_empty() {
            tracing::info!(
                "[DispatchBridge] cancelAdminParents({folder}): cancelled tasks for jids: {}",
                affected.join(", ")
            );
        }
        affected
    }
}

// ===== DAG helpers =====

/// True when every task referenced in `task.depends_on` has reached a terminal
/// status. Continue-on-error semantics: error/timeout still unblock dependants.
fn is_ready(task: &DispatchTask, all_tasks: &[DispatchTask]) -> bool {
    task.depends_on.iter().all(|dep_label| {
        all_tasks
            .iter()
            .find(|t| &t.label == dep_label)
            .map(|t| t.status.is_terminal())
            .unwrap_or(false)
    })
}

/// Build the prompt actually delivered to the sub-agent:
/// `<parent_goal>` + `<prerequisites>` (results of dependsOn tasks) +
/// `<other_tasks>` (situational awareness of siblings) + the original prompt.
/// Mirrors TS `startTask` context construction verbatim.
fn build_augmented_prompt(parent: &DispatchParent, task: &DispatchTask) -> String {
    let mut ctx = format!("<parent_goal>{}</parent_goal>", parent.goal);

    if !task.depends_on.is_empty() {
        ctx.push_str("\n\n<prerequisites>");
        for dep_label in &task.depends_on {
            let Some(dep) = parent.tasks.iter().find(|t| &t.label == dep_label) else {
                continue;
            };
            ctx.push_str(&format!(
                "\n  <task label=\"{}\" agent=\"{}\" status=\"{}\">",
                dep.label,
                dep.agent_id,
                dep.status.label()
            ));
            ctx.push_str(&format!("\n    <prompt>{}</prompt>", dep.prompt));
            if dep.status == DispatchTaskStatus::Done {
                let result = match &dep.result {
                    Some(r) if !r.is_empty() => format!("\n    <result>{r}</result>"),
                    _ => "\n    <result>(task completed but produced no text output — the agent may have only used tools; check workspace for artifacts)</result>".into(),
                };
                ctx.push_str(&result);
            }
            ctx.push_str("\n  </task>");
        }
        ctx.push_str("\n</prerequisites>");
    }

    let others: Vec<&DispatchTask> = parent
        .tasks
        .iter()
        .filter(|t| t.id != task.id && !task.depends_on.contains(&t.label))
        .collect();
    if !others.is_empty() {
        ctx.push_str("\n\n<other_tasks>");
        for o in others {
            if o.status == DispatchTaskStatus::Done {
                let result_tag = match &o.result {
                    Some(r) if !r.is_empty() => format!("\n    <result>{r}</result>"),
                    _ => "\n    <result>(completed, no text output)</result>".into(),
                };
                ctx.push_str(&format!(
                    "\n  <task label=\"{}\" agent=\"{}\" status=\"done\">{}{}\n  </task>",
                    o.label, o.agent_id, o.prompt, result_tag
                ));
            } else {
                ctx.push_str(&format!(
                    "\n  <task label=\"{}\" agent=\"{}\" status=\"{}\">{}</task>",
                    o.label,
                    o.agent_id,
                    o.status.label(),
                    o.prompt
                ));
            }
        }
        ctx.push_str("\n</other_tasks>");
    }

    format!("{ctx}\n\n{}", task.prompt)
}

// ===== File-lock helpers =====

pub(crate) fn lock_path_for(state_path: &Path) -> PathBuf {
    let mut p = state_path.as_os_str().to_owned();
    p.push(".lock");
    PathBuf::from(p)
}

/// Read the dispatch state file at `state_path`, returning the default empty
/// state when the file is missing.
pub(crate) fn read_state_file(state_path: &Path) -> std::io::Result<DispatchState> {
    match std::fs::read_to_string(state_path) {
        Ok(raw) => serde_json::from_str(&raw)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(DispatchState::default()),
        Err(e) => Err(e),
    }
}

/// Acquire the lock, read-modify-write the state file. Returns the new state
/// on success. Does not fire WS notify — callers wanting that behavior must
/// go through `DispatchBridge::modify_state` instead. Used by the MCP
/// dispatch server (which runs in a separate stdio process and can't reach
/// the bridge in-memory).
pub(crate) fn modify_state_file<F: FnOnce(&mut DispatchState)>(
    state_path: &Path,
    f: F,
) -> std::io::Result<DispatchState> {
    let lock_path = lock_path_for(state_path);
    if !acquire_lock(&lock_path) {
        tracing::warn!("[dispatch] Failed to acquire state lock, skipping modification");
        return Err(std::io::Error::new(
            std::io::ErrorKind::WouldBlock,
            "lock contention",
        ));
    }
    let result = (|| -> std::io::Result<DispatchState> {
        let mut state = read_state_file(state_path)?;
        f(&mut state);
        if let Some(parent) = state_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&state)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(state_path, json)?;
        Ok(state)
    })();
    let _ = std::fs::remove_file(&lock_path);
    result
}

/// Acquire a PID-stamped advisory lock by `O_CREAT|O_EXCL`-creating the lock
/// file. Retries up to 50× with ~10 ms backoff; on persistent failure checks
/// whether the holder's PID is still alive and clears the lock if not.
pub(crate) fn acquire_lock(lock_path: &Path) -> bool {
    use std::fs::OpenOptions;
    use std::io::Write;
    let pid = std::process::id();
    for _ in 0..50 {
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(lock_path)
        {
            Ok(mut f) => {
                let _ = write!(f, "{pid}");
                return true;
            }
            Err(_) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    // Stale-lock recovery: if the recorded PID is gone, clear and retry once.
    if let Ok(raw) = std::fs::read_to_string(lock_path) {
        if let Ok(holder) = raw.trim().parse::<i32>() {
            let alive = unsafe { libc::kill(holder, 0) } == 0;
            if !alive {
                let _ = std::fs::remove_file(lock_path);
                if let Ok(mut f) = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(lock_path)
                {
                    let _ = write!(f, "{pid}");
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_bridge_returns_no_parents() {
        let b = NoopDispatchBridge;
        assert!(b.get_parents().is_empty());
        assert!(build_dispatch_resume_hint(Some(&b), "main").is_none());
    }

    #[test]
    fn resume_hint_handles_no_bridge() {
        assert!(build_dispatch_resume_hint(None, "main").is_none());
    }

    struct FakeBridge {
        parents: Vec<DispatchParent>,
    }
    impl DispatchBridgeApi for FakeBridge {
        fn get_parents(&self) -> Vec<DispatchParent> {
            self.parents.clone()
        }
    }

    #[test]
    fn resume_hint_renders_active_parents_only() {
        let now = "2025-01-01T00:00:00Z".to_string();
        let parents = vec![
            DispatchParent {
                id: "p1".into(),
                goal: "goal-1".into(),
                admin_folder: "main".into(),
                shared_workspace: None,
                status: "active".into(),
                created_at: now.clone(),
                completed_at: None,
                tasks: vec![DispatchTask {
                    id: "t1".into(),
                    label: "writer".into(),
                    agent_id: "writer-agent".into(),
                    agent_jid: String::new(),
                    depends_on: vec![],
                    prompt: "do thing".into(),
                    status: DispatchTaskStatus::Processing,
                    result: None,
                    created_at: now.clone(),
                    started_at: None,
                    timeout_seconds: 0,
                    timeout_at: None,
                    completed_at: None,
                    is_virtual: false,
                    persona_name: None,
                }],
            },
            DispatchParent {
                id: "p2".into(),
                goal: "goal-2".into(),
                admin_folder: "main".into(),
                shared_workspace: None,
                status: "completed".into(),
                created_at: now.clone(),
                completed_at: None,
                tasks: vec![],
            },
            DispatchParent {
                id: "p3".into(),
                goal: "goal-3".into(),
                admin_folder: "other".into(),
                shared_workspace: None,
                status: "active".into(),
                created_at: now,
                completed_at: None,
                tasks: vec![],
            },
        ];
        let hint = build_dispatch_resume_hint(Some(&FakeBridge { parents }), "main").unwrap();
        assert!(hint.contains("Task group p1"));
        assert!(hint.contains("processing"));
        assert!(!hint.contains("p2"));
        assert!(!hint.contains("p3"));
    }

    fn tmp_state_path(suffix: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "senclaw-dispatch-{}-{}.json",
            suffix,
            std::process::id()
        ));
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_file(lock_path_for(&p));
        p
    }

    fn make_task(id: &str, label: &str, jid: &str) -> DispatchTask {
        DispatchTask {
            id: id.into(),
            label: label.into(),
            agent_id: "writer".into(),
            agent_jid: jid.into(),
            depends_on: vec![],
            prompt: "do".into(),
            status: DispatchTaskStatus::Processing,
            result: None,
            created_at: "2025-01-01T00:00:00Z".into(),
            started_at: Some("2025-01-01T00:00:01Z".into()),
            timeout_seconds: 60,
            timeout_at: None,
            completed_at: None,
            is_virtual: false,
            persona_name: None,
        }
    }

    #[test]
    fn modify_state_round_trips_through_disk() {
        let path = tmp_state_path("roundtrip");
        let bridge = DispatchBridge::new(&path);
        bridge
            .modify_state(|s| {
                s.parents.push(DispatchParent {
                    id: "p1".into(),
                    goal: "g".into(),
                    admin_folder: "main".into(),
                    shared_workspace: None,
                    status: "active".into(),
                    created_at: "2025-01-01T00:00:00Z".into(),
                    completed_at: None,
                    tasks: vec![make_task("d1", "writer", "jid-a")],
                });
            })
            .unwrap();

        // Re-open and confirm the state survives a fresh bridge instance.
        let bridge2 = DispatchBridge::new(&path);
        let parents = bridge2.get_parents();
        assert_eq!(parents.len(), 1);
        assert_eq!(parents[0].tasks[0].agent_jid, "jid-a");
        assert_eq!(parents[0].tasks[0].status, DispatchTaskStatus::Processing);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn notify_task_done_marks_terminal_and_completes_parent() {
        let path = tmp_state_path("done");
        let bridge = DispatchBridge::new(&path);
        bridge
            .modify_state(|s| {
                s.parents.push(DispatchParent {
                    id: "p1".into(),
                    goal: "g".into(),
                    admin_folder: "main".into(),
                    shared_workspace: None,
                    status: "active".into(),
                    created_at: "2025-01-01T00:00:00Z".into(),
                    completed_at: None,
                    tasks: vec![make_task("d1", "only", "jid-a")],
                });
            })
            .unwrap();

        bridge.notify_task_done("d1", "result-text");

        let parents = bridge.get_parents();
        assert_eq!(parents[0].tasks[0].status, DispatchTaskStatus::Done);
        assert_eq!(parents[0].tasks[0].result.as_deref(), Some("result-text"));
        assert_eq!(parents[0].status, "done");
        assert!(parents[0].completed_at.is_some());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn notify_reply_resolves_earliest_processing_task() {
        let path = tmp_state_path("reply");
        let bridge = DispatchBridge::new(&path);
        let mut t_old = make_task("d_old", "old", "jid-a");
        t_old.started_at = Some("2025-01-01T00:00:01Z".into());
        let mut t_new = make_task("d_new", "new", "jid-a");
        t_new.started_at = Some("2025-01-01T00:00:09Z".into());
        bridge
            .modify_state(|s| {
                s.parents.push(DispatchParent {
                    id: "p1".into(),
                    goal: "g".into(),
                    admin_folder: "main".into(),
                    shared_workspace: None,
                    status: "active".into(),
                    created_at: "2025-01-01T00:00:00Z".into(),
                    completed_at: None,
                    tasks: vec![t_old, t_new],
                });
            })
            .unwrap();
        // Both are tracked as in-flight against the same jid.
        bridge.add_active_task("d_old", "jid-a");
        bridge.add_active_task("d_new", "jid-a");

        bridge.notify_reply("jid-a", "old-result");

        let parents = bridge.get_parents();
        let by_id: HashMap<_, _> = parents[0].tasks.iter().map(|t| (t.id.as_str(), t)).collect();
        assert_eq!(by_id["d_old"].status, DispatchTaskStatus::Done);
        assert_eq!(by_id["d_new"].status, DispatchTaskStatus::Processing);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn is_ready_with_terminal_deps_returns_true() {
        let mut a = make_task("a", "a", "j");
        a.status = DispatchTaskStatus::Done;
        let mut b = make_task("b", "b", "j");
        b.status = DispatchTaskStatus::Error; // continue-on-error
        let mut c = make_task("c", "c", "j");
        c.depends_on = vec!["a".into(), "b".into()];
        c.status = DispatchTaskStatus::Registered;
        let all = vec![a, b, c.clone()];
        assert!(is_ready(&c, &all));

        // Flip one dep back to processing → not ready.
        let mut all2 = all.clone();
        all2[0].status = DispatchTaskStatus::Processing;
        assert!(!is_ready(&c, &all2));
    }

    #[test]
    fn build_augmented_prompt_includes_parent_goal_and_prereq_results() {
        let mut dep = make_task("d_dep", "writer", "j");
        dep.status = DispatchTaskStatus::Done;
        dep.result = Some("dep-result".into());
        dep.prompt = "draft a thing".into();

        let mut other = make_task("d_other", "reviewer", "j");
        other.status = DispatchTaskStatus::Processing;
        other.prompt = "review later".into();

        let mut me = make_task("d_me", "publisher", "j");
        me.depends_on = vec!["writer".into()];
        me.prompt = "publish it".into();

        let parent = DispatchParent {
            id: "p1".into(),
            goal: "ship the thing".into(),
            admin_folder: "main".into(),
            shared_workspace: None,
            status: "active".into(),
            created_at: "2025-01-01T00:00:00Z".into(),
            completed_at: None,
            tasks: vec![dep, other, me.clone()],
        };
        let augmented = build_augmented_prompt(&parent, &me);
        assert!(augmented.contains("<parent_goal>ship the thing</parent_goal>"));
        assert!(augmented.contains("<prerequisites>"));
        assert!(augmented.contains("<result>dep-result</result>"));
        assert!(augmented.contains("<other_tasks>"));
        assert!(augmented.contains("review later"));
        assert!(augmented.ends_with("\n\npublish it"));
    }

    #[test]
    fn process_pending_launches_ready_task_via_callback() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let path = tmp_state_path("scheduler");
        let bridge = DispatchBridge::new(&path);
        let fired = Arc::new(AtomicBool::new(false));
        {
            let f = Arc::clone(&fired);
            bridge.set_send_to_agent(Arc::new(
                move |jid: &str, task_id: &str, prompt: &str, _ws: &str| {
                    assert_eq!(jid, "jid-x");
                    assert_eq!(task_id, "d1");
                    assert!(prompt.contains("<parent_goal>g</parent_goal>"));
                    f.store(true, Ordering::SeqCst);
                },
            ));
        }
        let mut t = make_task("d1", "only", "jid-x");
        t.status = DispatchTaskStatus::Registered;
        bridge
            .modify_state(|s| {
                s.parents.push(DispatchParent {
                    id: "p1".into(),
                    goal: "g".into(),
                    admin_folder: "main".into(),
                    shared_workspace: None,
                    status: "active".into(),
                    created_at: "2025-01-01T00:00:00Z".into(),
                    completed_at: None,
                    tasks: vec![t],
                });
            })
            .unwrap();
        bridge.process_pending();
        assert!(fired.load(Ordering::SeqCst));
        let parents = bridge.get_parents();
        assert_eq!(parents[0].tasks[0].status, DispatchTaskStatus::Processing);
        assert!(parents[0].tasks[0].started_at.is_some());
        assert!(parents[0].tasks[0].timeout_at.is_some());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn activate_next_queued_promotes_oldest_and_picks_up_admin_workspace() {
        // state file lives under a tmp dir so the workspace-state file we
        // write next to it is found via state_path.parent().
        let dir = std::env::temp_dir().join(format!("senclaw-q-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let state_path = dir.join("dispatch-state.json");
        let _ = std::fs::remove_file(&state_path);
        let _ = std::fs::remove_file(lock_path_for(&state_path));
        std::fs::write(
            dir.join("workspace-state-main.json"),
            r#"{"currentDir":"/tmp/admin-workspace"}"#,
        )
        .unwrap();

        let bridge = DispatchBridge::new(&state_path);
        let now = chrono::Utc::now();
        let older = (now - chrono::Duration::seconds(10)).to_rfc3339();
        let newer = now.to_rfc3339();
        bridge
            .modify_state(|s| {
                s.parents.push(DispatchParent {
                    id: "p_old".into(),
                    goal: "g".into(),
                    admin_folder: "main".into(),
                    shared_workspace: None,
                    status: "queued".into(),
                    created_at: older,
                    completed_at: None,
                    tasks: vec![],
                });
                s.parents.push(DispatchParent {
                    id: "p_new".into(),
                    goal: "g".into(),
                    admin_folder: "main".into(),
                    shared_workspace: None,
                    status: "queued".into(),
                    created_at: newer,
                    completed_at: None,
                    tasks: vec![],
                });
            })
            .unwrap();

        bridge.activate_next_queued("main");
        let parents = bridge.get_parents();
        let by_id: HashMap<_, _> = parents.iter().map(|p| (p.id.as_str(), p)).collect();
        assert_eq!(by_id["p_old"].status, "active");
        assert_eq!(
            by_id["p_old"].shared_workspace.as_deref(),
            Some("/tmp/admin-workspace")
        );
        assert_eq!(by_id["p_new"].status, "queued");

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn cleanup_drops_old_done_parents() {
        let path = tmp_state_path("cleanup");
        let bridge = DispatchBridge::new(&path);
        let stale = (chrono::Utc::now() - chrono::Duration::hours(2)).to_rfc3339();
        let fresh = chrono::Utc::now().to_rfc3339();
        bridge
            .modify_state(|s| {
                s.parents.push(DispatchParent {
                    id: "old".into(),
                    goal: "g".into(),
                    admin_folder: "main".into(),
                    shared_workspace: None,
                    status: "done".into(),
                    created_at: stale.clone(),
                    completed_at: Some(stale),
                    tasks: vec![],
                });
                s.parents.push(DispatchParent {
                    id: "new".into(),
                    goal: "g".into(),
                    admin_folder: "main".into(),
                    shared_workspace: None,
                    status: "done".into(),
                    created_at: fresh.clone(),
                    completed_at: Some(fresh),
                    tasks: vec![],
                });
            })
            .unwrap();
        bridge.cleanup();
        let ids: Vec<_> = bridge.get_parents().iter().map(|p| p.id.clone()).collect();
        assert_eq!(ids, vec!["new".to_string()]);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn cancel_admin_parents_marks_active_jids_and_clears_tasks() {
        let path = tmp_state_path("cancel");
        let bridge = DispatchBridge::new(&path);
        bridge
            .modify_state(|s| {
                s.parents.push(DispatchParent {
                    id: "p1".into(),
                    goal: "g".into(),
                    admin_folder: "main".into(),
                    shared_workspace: None,
                    status: "active".into(),
                    created_at: "2025-01-01T00:00:00Z".into(),
                    completed_at: None,
                    tasks: vec![make_task("d1", "x", "jid-a")],
                });
            })
            .unwrap();
        bridge.add_active_task("d1", "jid-a");

        let affected = bridge.cancel_admin_parents("main");
        assert_eq!(affected, vec!["jid-a".to_string()]);
        let parents = bridge.get_parents();
        assert_eq!(parents[0].status, "done");
        assert_eq!(parents[0].tasks[0].status, DispatchTaskStatus::Error);
        assert!(!bridge.has_active_jid_tasks("jid-a"));
        let _ = std::fs::remove_file(path);
    }
}
