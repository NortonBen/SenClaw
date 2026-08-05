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

/// Run a scheduled shell command, honouring Settings → Sandbox → "script".
///
/// Enforcement ON: the command runs inside a throwaway OS sandbox (write-jail,
/// kill-enforced 10-minute ceiling; network per the policy switch). When the
/// switch is on but the engine cannot run, the task FAILS rather than silently
/// dropping to a raw shell. Enforcement OFF: raw `bash -c`, the historical
/// behaviour.
///
/// Returns `(stdout, stderr, exit_code, isolation)` — `isolation` is `"none"`
/// on the legacy path so run logs always say what confined the script.
async fn run_script_command(cmd: &str) -> anyhow::Result<(String, String, Option<i32>, String)> {
    let policy = crate::sandbox::policy::current();
    if policy.scheduler_script {
        let run = crate::sandbox::policy::run_once_sandboxed(
            "bash",
            cmd,
            policy.scheduler_network,
            Some(crate::sandbox::backend::MAX_TIMEOUT_MS as i64),
        )
        .await?;
        let mut stderr = run.stderr;
        if run.timed_out {
            if !stderr.is_empty() {
                stderr.push('\n');
            }
            stderr.push_str("(killed: sandbox deadline reached)");
        }
        return Ok((
            run.stdout,
            stderr,
            run.exit_code.map(|c| c as i32),
            run.isolation,
        ));
    }
    let output = Command::new("bash").arg("-c").arg(cmd).output()?;
    Ok((
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.code(),
        "none".to_string(),
    ))
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

        let (stdout, stderr, exit_code, _isolation) = run_script_command(cmd).await?;

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

        if exit_code != Some(0) {
            result.push_str(&format!("\nExit code: {}", exit_code.unwrap_or(-1)));
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

        let (stdout, stderr, exit_code, _isolation) = run_script_command(cmd).await?;

        let mut result = format!("Script output:\n{stdout}");
        if !stderr.is_empty() {
            result.push_str(&format!("\n\nStderr:\n{stderr}"));
        }
        if exit_code != Some(0) {
            result.push_str(&format!("\n\nExit code: {}", exit_code.unwrap_or(-1)));
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
            Ok(None) => {
                // Self-heal: the schedule's chat session can go missing (e.g. an
                // older build's config reconciliation wiped it). Recreate a
                // minimal binding from the task so the recurring schedule keeps
                // running instead of failing forever with "chat session not found".
                warn!(
                    task_id = %task.id,
                    chat_jid = %task.chat_jid,
                    "[TaskScheduler] group task: chat session missing — recreating from task"
                );
                let now = chrono::Utc::now().to_rfc3339();
                let binding = crate::types::GroupBinding {
                    jid: task.chat_jid.clone(),
                    folder: task.group_folder.clone(),
                    name: task.prompt.chars().take(60).collect::<String>(),
                    channel: String::new(),
                    group_type: "chat".into(),
                    requires_trigger: false,
                    allowed_tools: None,
                    allowed_paths: None,
                    allowed_work_dirs: None,
                    bot_token: None,
                    max_messages: None,
                    llm_config_id: None,
                    last_active: Some(now.clone()),
                    added_at: now,
                };
                self.db.upsert_group(&binding)?;
                binding
            }
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
