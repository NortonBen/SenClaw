//! Task executor implementations.
//!
//! Handles the 5 context modes: notify / script / script-agent / isolated / group.
//! Mirrors `src-old/scheduler/TaskScheduler.ts` executor behaviour.

use std::process::Command;
use std::sync::Arc;

use async_trait::async_trait;
use tracing::{debug, info, warn};

use crate::db::Db;
use crate::scheduler::task_scheduler::TaskExecutor;
use crate::types::{AgentApi, ContextMode, RunStatus, ScheduledTask, TaskRunLogInsert};

/// Executor that handles each context mode appropriately.
pub struct DefaultTaskExecutor {
    db: Arc<Db>,
    /// Used by `ContextMode::Group` to dispatch the scheduled prompt into the
    /// owning chat session. `None` falls back to a stub log (useful in tests).
    agent_api: Option<Arc<dyn AgentApi>>,
}

impl DefaultTaskExecutor {
    pub fn new(db: Arc<Db>) -> Self {
        Self {
            db,
            agent_api: None,
        }
    }

    pub fn with_agent_api(mut self, api: Arc<dyn AgentApi>) -> Self {
        self.agent_api = Some(api);
        self
    }
}

#[async_trait]
impl TaskExecutor for DefaultTaskExecutor {
    async fn execute(&self, task: ScheduledTask) {
        let task_id = task.id.clone();

        let result = match task.context_mode {
            ContextMode::Notify => self.execute_notify(&task).await,
            ContextMode::Script => self.execute_script(&task).await,
            ContextMode::ScriptAgent => self.execute_script_agent(&task).await,
            ContextMode::Isolated => {
                info!(
                    task_id = %task.id,
                    group_folder = %task.group_folder,
                    "[TaskScheduler] isolated task (will be dispatched as a fresh session when agent pool is wired)"
                );
                Ok(format!("[isolated] task queued: {}", task.prompt))
            }
            ContextMode::Group => self.execute_group(&task).await,
        };

        let now = chrono::Utc::now().to_rfc3339();
        match result {
            Ok(output) => {
                debug!(task_id = %task_id, "[TaskScheduler] completed");
                if let Err(e) = self.db.insert_task_run_log(&TaskRunLogInsert {
                    task_id: task_id.clone(),
                    run_at: now,
                    duration_ms: None,
                    status: RunStatus::Success,
                    result: Some(output),
                    error: None,
                }) {
                    warn!(task_id = %task_id, error = %e, "[TaskScheduler] failed to record result");
                }
            }
            Err(e) => {
                warn!(task_id = %task_id, error = %e, "[TaskScheduler] failed");
                let err_msg = format!("{e:#}");
                if let Err(e2) = self.db.insert_task_run_log(&TaskRunLogInsert {
                    task_id: task_id.clone(),
                    run_at: now,
                    duration_ms: None,
                    status: RunStatus::Error,
                    result: None,
                    error: Some(err_msg),
                }) {
                    warn!(task_id = %task_id, error = %e2, "[TaskScheduler] failed to record error");
                }
            }
        }
    }
}

impl DefaultTaskExecutor {
    /// Notify mode: just record the task result.
    async fn execute_notify(&self, task: &ScheduledTask) -> anyhow::Result<String> {
        info!(
            task_id = %task.id,
            "[TaskScheduler] notify: {}",
            task.prompt
        );
        Ok(format!("[notify] {}", task.prompt))
    }

    /// Script mode: execute a shell command.
    async fn execute_script(&self, task: &ScheduledTask) -> anyhow::Result<String> {
        let cmd = task.script_command.as_deref().unwrap_or(&task.prompt);
        info!(
            task_id = %task.id,
            command = %cmd,
            "[TaskScheduler] script"
        );

        let output = Command::new("bash").arg("-c").arg(cmd).output()?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        let mut result = String::new();
        if !stdout.is_empty() {
            result.push_str(&stdout);
        }
        if !stderr.is_empty() {
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str(&stderr);
        }

        if !output.status.success() {
            result.push_str(&format!(
                "\nExit code: {}",
                output.status.code().unwrap_or(-1)
            ));
        }

        Ok(result)
    }

    /// Script-agent mode: shell output is fed back to the agent (stub).
    async fn execute_script_agent(&self, task: &ScheduledTask) -> anyhow::Result<String> {
        let cmd = task.script_command.as_deref().unwrap_or(&task.prompt);
        info!(
            task_id = %task.id,
            command = %cmd,
            "[TaskScheduler] script-agent"
        );

        let output = Command::new("bash").arg("-c").arg(cmd).output()?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        let mut result = format!("Script output:\n{stdout}");
        if !stderr.is_empty() {
            result.push_str(&format!("\n\nStderr:\n{stderr}"));
        }
        if !output.status.success() {
            result.push_str(&format!(
                "\n\nExit code: {}",
                output.status.code().unwrap_or(-1)
            ));
        }

        // In full implementation: feed this output to the agent for interpretation.
        info!(
            task_id = %task.id,
            "[TaskScheduler] script-agent output ready (agent feed-back will be wired when agent pool is integrated)"
        );

        Ok(result)
    }

    /// Group mode: dispatch the prompt as an agent run on the schedule's chat
    /// session. Agent replies stream through `broadcast_reply` and land in the
    /// existing chat history (channel_messages + WS push), so the recurring
    /// schedule's chat view shows live output.
    async fn execute_group(&self, task: &ScheduledTask) -> anyhow::Result<String> {
        let api = match &self.agent_api {
            Some(a) => a,
            None => {
                info!(
                    task_id = %task.id,
                    chat_jid = %task.chat_jid,
                    "[TaskScheduler] group task: agent api not wired, logging only"
                );
                return Ok(format!("[group:stub] {}", task.prompt));
            }
        };
        let group = match self.db.get_group(&task.chat_jid) {
            Ok(Some(g)) => g,
            Ok(None) => anyhow::bail!("chat session not found: {}", task.chat_jid),
            Err(e) => anyhow::bail!("db error: {e}"),
        };
        info!(
            task_id = %task.id,
            chat_jid = %task.chat_jid,
            "[TaskScheduler] group task: dispatching to agent"
        );
        api.process_and_wait(&task.chat_jid, &group, &task.prompt)
            .await?;
        let reply = api
            .get_last_reply_text(&task.chat_jid)
            .unwrap_or_else(|| "(no reply)".into());
        Ok(reply)
    }
}
