//! Dispatch bridge — DAG task orchestration for the main agent.
//! Port target: src-old/agent/DispatchBridge.ts (777 lines).
//!
//! Phase 1 ports only the trait surface that AgentPool calls into:
//! `notify_task_done`, `notify_reply`, `notify_error`, `set_admin_activity_callback`,
//! and the `get_parents` accessor used to build the resume hint.
//!
//! Concrete implementation (state file persistence, sub-agent scheduling, MCP
//! `dispatch_task` tool wiring) lands in a later phase.

use std::sync::Arc;

// ===== Public types =====

/// Subtask status — mirrors TS `DispatchTask.status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchTaskStatus {
    Registered,
    Processing,
    Done,
    Error,
}

impl DispatchTaskStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Registered => "pending",
            Self::Processing => "running",
            Self::Done => "done",
            Self::Error => "error",
        }
    }
}

/// One subtask inside a dispatch parent group.
#[derive(Debug, Clone)]
pub struct DispatchTask {
    pub id: String,
    pub label: String,
    pub agent_id: String,
    pub prompt: String,
    pub status: DispatchTaskStatus,
}

/// Parent dispatch (one `dispatch_task` MCP call → N subtasks).
#[derive(Debug, Clone)]
pub struct DispatchParent {
    pub id: String,
    pub goal: String,
    pub admin_folder: String,
    /// "active" / "completed" / "error" / "cancelled".
    pub status: String,
    pub tasks: Vec<DispatchTask>,
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
        let parents = vec![
            DispatchParent {
                id: "p1".into(),
                goal: "goal-1".into(),
                admin_folder: "main".into(),
                status: "active".into(),
                tasks: vec![DispatchTask {
                    id: "t1".into(),
                    label: "writer".into(),
                    agent_id: "writer-agent".into(),
                    prompt: "do thing".into(),
                    status: DispatchTaskStatus::Processing,
                }],
            },
            DispatchParent {
                id: "p2".into(),
                goal: "goal-2".into(),
                admin_folder: "main".into(),
                status: "completed".into(),
                tasks: vec![],
            },
            DispatchParent {
                id: "p3".into(),
                goal: "goal-3".into(),
                admin_folder: "other".into(),
                status: "active".into(),
                tasks: vec![],
            },
        ];
        let hint = build_dispatch_resume_hint(Some(&FakeBridge { parents }), "main").unwrap();
        assert!(hint.contains("Task group p1"));
        assert!(hint.contains("running"));
        assert!(!hint.contains("p2"));
        assert!(!hint.contains("p3"));
    }
}
