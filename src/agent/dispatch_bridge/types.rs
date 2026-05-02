//! Public types for the dispatch bridge.

use serde::{Deserialize, Serialize};
use std::sync::Arc;

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
