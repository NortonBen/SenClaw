//! DispatchBridge struct and its core implementation.

use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use super::dag::{build_augmented_prompt, is_ready};
use super::locks::{acquire_lock, lock_path_for};
use super::types::{
    AdminActivityCallback, DispatchAgent, DispatchParent, DispatchState, DispatchTask,
    DispatchTaskStatus,
};
use crate::agent::persona_registry::PersonaRegistry;
use crate::agent::virtual_worker_pool::VirtualWorkerPool;
use crate::types::GroupBinding;

// ===== Real DispatchBridge =====

/// Callback invoked after every state mutation. Receives the full parents
/// snapshot serialized as JSON in the wire format the Web Agent Console
/// consumes (`dispatch:update.parents`).
pub type WsNotifyCallback = Arc<dyn Fn(&serde_json::Value) + Send + Sync>;

/// Callback invoked when the scheduler decides a task is ready to run.
/// Arguments: `(jid, task_id, augmented_prompt, workspace_dir)`. An empty
/// `workspace_dir` means "do not switch the sub-agent's working directory".
pub type SendToAgentCallback = Arc<dyn Fn(&str, &str, &str, &str) + Send + Sync>;

/// Callback invoked when the last in-flight dispatch task on `jid` finishes,
/// so the sub-agent can be restored to its own working directory.
pub type RevertWorkspaceCallback = Arc<dyn Fn(&str) + Send + Sync>;

/// Callback invoked when a persistent sub-agent must be stopped, for example
/// after its dispatch task reaches the DispatchBridge timeout.
pub type AbortAgentCallback = Arc<dyn Fn(&str, &str) + Send + Sync>;

/// Callback invoked when a dispatch task changes status. Arguments:
/// `(task_id, new_status, task_label, parent_goal)` — used by CoworkManager
/// to keep CoworkTask status in sync with DispatchTask lifecycle.
pub type TaskLifecycleCallback = Arc<dyn Fn(&str, &str, &str, &str) + Send + Sync>;

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
    pub(super) inner: Mutex<Inner>,
    /// Optional callback fired post-write with the parents snapshot.
    ws_notify: Mutex<Option<WsNotifyCallback>>,
    /// Optional callback fired when sub-task activity should reset the admin
    /// agent's inactivity timer.
    pub(super) on_admin_activity: Mutex<Option<AdminActivityCallback>>,
    /// Scheduler hand-off — invoked when a `registered` task is ready to run.
    send_to_agent: Mutex<Option<SendToAgentCallback>>,
    /// Workspace restore hand-off — invoked after the last in-flight task on
    /// a jid finishes.
    revert_workspace: Mutex<Option<RevertWorkspaceCallback>>,
    /// Agent abort hand-off — invoked when a timed-out persistent task should
    /// stop its underlying AgentPool process_and_wait loop.
    abort_agent: Mutex<Option<AbortAgentCallback>>,
    /// Persona registry — required for virtual-agent dispatch (Phase 5).
    persona_registry: Mutex<Option<Arc<Mutex<PersonaRegistry>>>>,
    /// Virtual worker pool — required for virtual-agent dispatch (Phase 5).
    pub(super) virtual_worker_pool: Mutex<Option<Arc<VirtualWorkerPool>>>,
    /// Optional callback fired on every task status transition (start/done/error/timeout).
    /// Used by CoworkManager to keep CoworkTask ↔ DispatchTask status in sync.
    on_task_lifecycle: Mutex<Option<TaskLifecycleCallback>>,
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
pub(super) struct Inner {
    /// taskId → jid (primary index for in-flight persistent-agent tasks).
    pub(super) active_tasks: HashMap<String, String>,
    /// jid → set of taskIds (secondary index for fast per-agent lookup).
    pub(super) active_agent_tasks: HashMap<String, HashSet<String>>,
    /// Admin folders with scheduling currently paused.
    pub(super) paused_admins: HashSet<String>,
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
            abort_agent: Mutex::new(None),
            persona_registry: Mutex::new(None),
            virtual_worker_pool: Mutex::new(None),
            on_task_lifecycle: Mutex::new(None),
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

    /// Inject the sub-agent abort hand-off used when DispatchBridge times out
    /// a persistent task before AgentPool's longer process_and_wait watchdog.
    pub fn set_abort_agent(&self, cb: AbortAgentCallback) {
        *self.abort_agent.lock().unwrap() = Some(cb);
    }

    /// Inject a callback fired on every task status transition (start/done/error/timeout).
    /// Used by CoworkManager to keep CoworkTask ↔ DispatchTask status in sync.
    pub fn set_task_lifecycle_callback(&self, cb: TaskLifecycleCallback) {
        *self.on_task_lifecycle.lock().unwrap() = Some(cb);
    }

    fn fire_task_lifecycle(&self, task_id: &str, status: &str, label: &str, parent_goal: &str) {
        if let Some(cb) = self.on_task_lifecycle.lock().unwrap().as_ref() {
            cb(task_id, status, label, parent_goal);
        }
    }

    /// Enqueue a new parent dispatch into the bridge state. Generates `p-*` and
    /// `d-*` IDs from the monotonic sequence. Returns `(parent_id, task_ids)`.
    /// All tasks start as `registered` and the parent starts as `queued`.
    pub fn enqueue_parent(
        &self,
        goal: String,
        admin_folder: String,
        shared_workspace: Option<String>,
        tasks: Vec<DispatchTask>,
    ) -> std::io::Result<(String, Vec<String>)> {
        let admin_folder_display = admin_folder.clone(); // clone before move into closure
        let mut parent_id = String::new();
        let mut task_ids: Vec<String> = Vec::new();
        let now = chrono::Utc::now().to_rfc3339();
        self.modify_state(|state| {
            state.seq += 1;
            parent_id = format!("p-{}", state.seq);
            let mut resolved_tasks = Vec::with_capacity(tasks.len());
            for mut t in tasks {
                state.seq += 1;
                let tid = format!("d-{}", state.seq);
                t.id = tid.clone();
                t.created_at = now.clone();
                task_ids.push(tid);
                resolved_tasks.push(t);
            }
            let parent = DispatchParent {
                id: parent_id.clone(),
                goal,
                admin_folder,
                shared_workspace,
                status: "queued".into(),
                created_at: now,
                completed_at: None,
                tasks: resolved_tasks,
            };
            state.parents.push(parent);
        })?;
        // After adding a queued parent, try to activate it immediately if the
        // admin has no other active parent.
        if !parent_id.is_empty() {
            self.activate_next_queued(&admin_folder_display);
        }
        tracing::info!(
            "[DispatchBridge] Enqueued parent {parent_id} with {} task(s) → admin: {admin_folder_display}",
            task_ids.len()
        );
        Ok((parent_id, task_ids))
    }

    /// Compare cowork workspace roots ignoring trailing path separators.
    fn cowork_root_paths_match(stored: Option<&str>, root: &str) -> bool {
        let Some(s) = stored else {
            return false;
        };
        if root.trim().is_empty() {
            return false;
        }
        let a = s.trim().trim_end_matches(['/', '\\']);
        let b = root.trim().trim_end_matches(['/', '\\']);
        !a.is_empty() && a == b
    }

    /// Cancel active/queued parents matching `pred`. Used by admin-folder cancel
    /// and by Cowork workspace teardown.
    pub(super) fn cancel_active_parents_where(
        &self,
        mut pred: impl FnMut(&DispatchParent) -> bool,
        cancel_note: &str,
    ) -> Vec<String> {
        let mut affected: Vec<String> = Vec::new();
        let mut to_remove: Vec<String> = Vec::new();
        let mut virtual_to_cancel: Vec<String> = Vec::new();
        let mut admins_to_unpause: HashSet<String> = HashSet::new();
        let now = chrono::Utc::now().to_rfc3339();
        let note = cancel_note.to_string();
        let _ = self.modify_state(|state| {
            for parent in &mut state.parents {
                if !pred(parent) {
                    continue;
                }
                if parent.status != "active" && parent.status != "queued" {
                    continue;
                }
                admins_to_unpause.insert(parent.admin_folder.clone());
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
                            task.result = Some(note.clone());
                            task.completed_at = Some(now.clone());
                        }
                        DispatchTaskStatus::Registered => {
                            task.status = DispatchTaskStatus::Error;
                            task.result = Some(note.clone());
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
        {
            let mut inner = self.inner.lock().unwrap();
            for folder in &admins_to_unpause {
                inner.paused_admins.remove(folder);
            }
        }
        if !affected.is_empty() {
            tracing::info!(
                "[DispatchBridge] cancel_active_parents_where: cancelled tasks for jids: {}",
                affected.join(", ")
            );
        }
        affected
    }

    /// Cancel DAG parents created for a Cowork workspace (`shared_workspace` path).
    pub fn cancel_parents_for_shared_workspace(&self, root_dir: &str) -> Vec<String> {
        self.cancel_active_parents_where(
            |p| Self::cowork_root_paths_match(p.shared_workspace.as_deref(), root_dir),
            "Cancelled: cowork workspace deleted",
        )
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

    pub(super) fn add_active_task(&self, task_id: &str, jid: &str) {
        let mut inner = self.inner.lock().unwrap();
        inner
            .active_tasks
            .insert(task_id.to_string(), jid.to_string());
        inner
            .active_agent_tasks
            .entry(jid.to_string())
            .or_default()
            .insert(task_id.to_string());
    }

    pub(super) fn remove_active_task(&self, task_id: &str) -> Option<String> {
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

    pub(super) fn has_active_jid_tasks(&self, jid: &str) -> bool {
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
    pub(super) fn mark_task_done(&self, task_id: &str, text: &str) {
        let jid = self.remove_active_task(task_id);
        let now = chrono::Utc::now().to_rfc3339();
        let mut task_admin: Option<String> = None;
        let mut completed_admin: Option<String> = None;
        let mut task_label = String::new();
        let mut parent_goal = String::new();
        let _ = self.modify_state(|state| {
            for parent in &mut state.parents {
                if let Some(task) = parent.tasks.iter_mut().find(|t| t.id == task_id) {
                    if task.status.is_terminal() {
                        return;
                    }
                    task_admin = Some(parent.admin_folder.clone());
                    task_label = task.label.clone();
                    parent_goal = parent.goal.clone();
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
        self.fire_task_lifecycle(task_id, "done", &task_label, &parent_goal);
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
    pub(super) fn mark_task_error(&self, task_id: &str, error_message: &str) {
        let jid = self.remove_active_task(task_id);
        let now = chrono::Utc::now().to_rfc3339();
        let mut task_admin: Option<String> = None;
        let mut completed_admin: Option<String> = None;
        let mut task_label = String::new();
        let mut parent_goal = String::new();
        let _ = self.modify_state(|state| {
            for parent in &mut state.parents {
                if let Some(task) = parent.tasks.iter_mut().find(|t| t.id == task_id) {
                    if task.status.is_terminal() {
                        return;
                    }
                    task_admin = Some(parent.admin_folder.clone());
                    task_label = task.label.clone();
                    parent_goal = parent.goal.clone();
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
        self.fire_task_lifecycle(task_id, "error", &task_label, &parent_goal);
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
    pub(super) fn earliest_processing_for_jid(&self, jid: &str) -> Option<String> {
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
    pub(super) fn activate_next_queued(&self, admin_folder: &str) {
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

    fn fire_abort_agent(&self, jid: &str, reason: &str) {
        if let Some(cb) = self.abort_agent.lock().unwrap().as_ref() {
            cb(jid, reason);
        }
    }

    // ---- Scheduler ====

    /// Polling tick: timeout-check active tasks, then launch ready tasks.
    /// Mirrors TS `processPending`. Virtual-agent scheduling is deferred to
    /// Phase 5; for now `start_task` only fires for persistent agents.
    pub(super) fn process_pending(&self) {
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
                let task_count = state.parents.iter().map(|p| p.tasks.len()).sum::<usize>();
                tracing::info!(
                    "[DispatchBridge] External state change detected parents={} tasks={} — \
                     broadcasting dispatch:update from daemon",
                    state.parents.len(),
                    task_count
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
                let Some(deadline_str) = &task.timeout_at else {
                    continue;
                };
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
                let Some(p) = reg.get(persona_name) else {
                    return false;
                };
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
        let timeout_at_iso =
            (started_at + chrono::Duration::seconds(task.timeout_seconds as i64)).to_rfc3339();

        let task_id = task.id.clone();
        let task_label = task.label.clone();
        let parent_goal = parent.goal.clone();
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
        self.fire_task_lifecycle(&task_id, "processing", &task_label, &parent_goal);

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
                    &format!("Virtual agent setup error: persona \"{persona_name}\" not available"),
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
                let Some(arc) = weak.and_then(|w| w.upgrade()) else {
                    return;
                };
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
        let mut task_label = String::new();
        let mut parent_goal = String::new();
        let _ = self.modify_state(|state| {
            for parent in &mut state.parents {
                if let Some(task) = parent.tasks.iter_mut().find(|t| t.id == task_id) {
                    task_label = task.label.clone();
                    parent_goal = parent.goal.clone();
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
        self.fire_task_lifecycle(task_id, "timeout", &task_label, &parent_goal);
        self.remove_active_task(task_id);
        tracing::warn!("[DispatchBridge] Task {task_id} timed out");
        if !jid.is_empty() {
            self.fire_abort_agent(jid, &format!("Dispatch task {task_id} timed out"));
        }
        if let Some(folder) = completed_admin {
            self.activate_next_queued(&folder);
        }
        if !jid.is_empty() && !self.has_active_jid_tasks(jid) {
            self.fire_revert_workspace(jid);
        }
    }

    /// Drop `done` parents whose `completed_at` is older than
    /// [`CLEANUP_RETENTION_SECONDS`]. Mirrors TS `cleanup`.
    pub(super) fn cleanup(&self) {
        let cutoff = chrono::Utc::now() - chrono::Duration::seconds(CLEANUP_RETENTION_SECONDS);
        let _ = self.modify_state(|state| {
            state.parents.retain(|p| {
                if p.status != "done" {
                    return true;
                }
                let Some(ts) = &p.completed_at else {
                    return true;
                };
                let Ok(completed) = chrono::DateTime::parse_from_rfc3339(ts) else {
                    return true;
                };
                completed.with_timezone(&chrono::Utc) >= cutoff
            });
        });
    }

    // ---- File I/O ----

    pub(super) fn read_state(&self) -> std::io::Result<DispatchState> {
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
