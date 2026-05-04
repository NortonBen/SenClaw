//! Schedule MCP server. Port target: src-old/mcp/schedule-server.ts
//!
//! Tools: schedule_task, list_tasks, pause_task, cancel_task.
//! Operates directly on the SQLite `scheduled_tasks` table through [`Db`].

use anyhow::{bail, Context, Result};
use chrono::Utc;
use cron::Schedule;
use rmcp::ServiceExt;
use std::str::FromStr;
use std::sync::Arc;
use uuid::Uuid;

use crate::db::Db;
use crate::types::{ContextMode, ScheduleType, ScheduledTask, TaskStatus};

// ===== MCP stdio server =====

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct ScheduleTaskParams {
    group_folder: String,
    chat_jid: String,
    prompt: String,
    schedule_type: String,
    schedule_value: String,
    #[serde(default)]
    context_mode: Option<String>,
    #[serde(default)]
    script_command: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct ListTasksParams {
    group_folder: String,
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct TaskActionParams {
    task_id: String,
    group_folder: String,
}

#[derive(Clone)]
struct McpScheduleServer {
    db: Arc<Db>,
    group_folder: String,
    chat_jid: String,
}

#[rmcp::tool_router(server_handler)]
impl McpScheduleServer {
    #[rmcp::tool(description = "Schedule a new recurring or one-off task")]
    async fn schedule_task(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            ScheduleTaskParams,
        >,
    ) -> String {
        let srv = ScheduleServer::new();
        let result = srv
            .schedule_task(
                &self.db,
                &p.group_folder,
                &p.chat_jid,
                &p.prompt,
                &p.schedule_type,
                &p.schedule_value,
                p.context_mode.as_deref(),
                p.script_command.as_deref(),
            )
            .await;
        if result.is_error {
            return result.content;
        }
        result.content
    }

    #[rmcp::tool(description = "List all scheduled tasks for a group")]
    fn list_tasks(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            ListTasksParams,
        >,
    ) -> String {
        let srv = ScheduleServer::new();
        let result = srv.list_tasks(&self.db, &p.group_folder);
        result.content
    }

    #[rmcp::tool(description = "Pause a scheduled task")]
    fn pause_task(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            TaskActionParams,
        >,
    ) -> String {
        let srv = ScheduleServer::new();
        let result = srv.pause_task(&self.db, &p.task_id, &p.group_folder);
        result.content
    }

    #[rmcp::tool(description = "Cancel a scheduled task")]
    fn cancel_task(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            TaskActionParams,
        >,
    ) -> String {
        let srv = ScheduleServer::new();
        let result = srv.cancel_task(&self.db, &p.task_id, &p.group_folder);
        result.content
    }
}

/// Start the schedule MCP server over stdio. Reads config from environment
/// variables set by [`crate::mcp::helper::schedule_mcp_config`].
pub async fn run_stdio_server() -> Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();

    let db_path = std::env::var("SENCLAW_DB_PATH").context("SENCLAW_DB_PATH not set")?;
    let group_folder =
        std::env::var("SENCLAW_GROUP_FOLDER").context("SENCLAW_GROUP_FOLDER not set")?;
    let chat_jid = std::env::var("SENCLAW_CHAT_JID").context("SENCLAW_CHAT_JID not set")?;

    let mut config = crate::config::Config::from_env();
    config.paths.db_path = std::path::PathBuf::from(&db_path);
    let db = Arc::new(Db::open(&config).context("open schedule DB")?);

    let server = McpScheduleServer {
        db,
        group_folder,
        chat_jid,
    };

    let service = server.serve(rmcp::transport::io::stdio()).await?;
    service.waiting().await?;
    Ok(())
}

pub struct ScheduleServer;

/// Result returned by each tool call, compatible with MCP content format.
#[derive(Debug)]
pub struct ToolResult {
    pub content: String,
    pub is_error: bool,
}

impl ToolResult {
    pub fn ok(text: String) -> Self {
        Self {
            content: text,
            is_error: false,
        }
    }

    pub fn err(text: String) -> Self {
        Self {
            content: text,
            is_error: true,
        }
    }
}

impl ScheduleServer {
    pub fn new() -> Self {
        Self
    }

    // ===== schedule_task =====

    /// Create a scheduled task. Returns JSON with `{success, taskId, nextRun}`.
    pub async fn schedule_task(
        &self,
        db: &Db,
        group_folder: &str,
        chat_jid: &str,
        prompt: &str,
        schedule_type: &str,
        schedule_value: &str,
        context_mode: Option<&str>,
        script_command: Option<&str>,
    ) -> ToolResult {
        let schedule_type = ScheduleType::parse(schedule_type);
        let resolved_mode = context_mode
            .map(|s| ContextMode::parse(s))
            .unwrap_or(ContextMode::Notify);

        if matches!(
            resolved_mode,
            ContextMode::Script | ContextMode::ScriptAgent
        ) && script_command.is_none()
        {
            return ToolResult::err(
                "Error: script_command is required for script and script-agent modes".into(),
            );
        }

        match compute_next_run(&schedule_type, schedule_value) {
            Ok(next_run) => {
                let task = ScheduledTask {
                    id: Uuid::new_v4().to_string(),
                    group_folder: group_folder.to_owned(),
                    chat_jid: chat_jid.to_owned(),
                    prompt: prompt.to_owned(),
                    schedule_type,
                    schedule_value: schedule_value.to_owned(),
                    context_mode: resolved_mode,
                    script_command: script_command.map(|s| s.to_owned()),
                    next_run: Some(next_run.clone()),
                    last_run: None,
                    last_result: None,
                    status: TaskStatus::Active,
                    created_at: Utc::now().to_rfc3339(),
                };
                if let Err(e) = db.insert_task(&task) {
                    return ToolResult::err(format!("Error: {e}"));
                }
                let json = serde_json::json!({
                    "success": true,
                    "taskId": task.id,
                    "nextRun": next_run,
                });
                ToolResult::ok(json.to_string())
            }
            Err(e) => ToolResult::err(format!("Error: {e}")),
        }
    }

    // ===== list_tasks =====

    /// List all scheduled tasks for the owning group folder.
    pub fn list_tasks(&self, db: &Db, group_folder: &str) -> ToolResult {
        match db.get_tasks_by_group(group_folder) {
            Ok(tasks) => {
                let json = serde_json::to_string_pretty(&tasks).unwrap_or_default();
                ToolResult::ok(json)
            }
            Err(e) => ToolResult::err(format!("Error: {e}")),
        }
    }

    // ===== pause_task =====

    pub fn pause_task(&self, db: &Db, task_id: &str, group_folder: &str) -> ToolResult {
        // validate ownership: only pause tasks belonging to this group
        match db.get_tasks_by_group(group_folder) {
            Ok(tasks) => {
                if !tasks.iter().any(|t| t.id == task_id) {
                    return ToolResult::err(format!(
                        "Task not found or not in this group: {task_id}"
                    ));
                }
            }
            Err(e) => return ToolResult::err(format!("Error: {e}")),
        }
        match db.update_task_status(task_id, TaskStatus::Paused) {
            Ok(_) => {
                let json = serde_json::json!({
                    "success": true,
                    "taskId": task_id,
                    "status": "paused",
                });
                ToolResult::ok(json.to_string())
            }
            Err(e) => ToolResult::err(format!("Error: {e}")),
        }
    }

    // ===== cancel_task =====

    pub fn cancel_task(&self, db: &Db, task_id: &str, group_folder: &str) -> ToolResult {
        match db.get_tasks_by_group(group_folder) {
            Ok(tasks) => {
                if !tasks.iter().any(|t| t.id == task_id) {
                    return ToolResult::err(format!(
                        "Task not found or not in this group: {task_id}"
                    ));
                }
            }
            Err(e) => return ToolResult::err(format!("Error: {e}")),
        }
        match db.update_task_status(task_id, TaskStatus::Completed) {
            Ok(_) => {
                let json = serde_json::json!({
                    "success": true,
                    "taskId": task_id,
                    "status": "completed",
                });
                ToolResult::ok(json.to_string())
            }
            Err(e) => ToolResult::err(format!("Error: {e}")),
        }
    }
}

impl Default for ScheduleServer {
    fn default() -> Self {
        Self::new()
    }
}

// ===== next_run calculation =====

fn compute_next_run(schedule_type: &ScheduleType, value: &str) -> Result<String> {
    match schedule_type {
        ScheduleType::Cron => {
            let expr = if value.trim().split_whitespace().count() == 5 {
                format!("0 {value}")
            } else {
                value.to_owned()
            };
            let schedule = Schedule::from_str(&expr)?;
            schedule
                .upcoming(Utc)
                .next()
                .map(|t| t.to_rfc3339())
                .ok_or_else(|| anyhow::anyhow!("Cron schedule has no upcoming occurrence"))
        }
        ScheduleType::Interval => {
            let ms: i64 = value
                .parse()
                .map_err(|_| anyhow::anyhow!("Invalid interval value: {value}"))?;
            if ms <= 0 {
                bail!("Interval value must be positive: {value}");
            }
            Ok((Utc::now() + chrono::Duration::milliseconds(ms)).to_rfc3339())
        }
        ScheduleType::Once => {
            // value is used directly as ISO time
            let _ = chrono::DateTime::parse_from_rfc3339(value)?;
            Ok(value.to_owned())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn test_config() -> Config {
        Config::from_env()
    }

    #[test]
    fn compute_next_run_interval() {
        let result = compute_next_run(&ScheduleType::Interval, "3600000").unwrap();
        // should be ~1 hour from now
        assert!(result.len() > 10);
    }

    #[test]
    fn compute_next_run_once() {
        let result = compute_next_run(&ScheduleType::Once, "2026-12-25T00:00:00+00:00").unwrap();
        assert!(result.contains("2026-12-25"));
    }

    #[test]
    fn compute_next_run_cron() {
        let result = compute_next_run(&ScheduleType::Cron, "0 9 * * *").unwrap();
        assert!(result.len() > 10);
    }

    #[test]
    fn compute_next_run_invalid_interval() {
        assert!(compute_next_run(&ScheduleType::Interval, "bad").is_err());
    }

    #[test]
    fn schedule_task_pause_cancel_flow() {
        let cfg = test_config();
        let db = Db::open_in_memory(&cfg).unwrap();
        let srv = ScheduleServer::new();

        let create = tokio_test::block_on(srv.schedule_task(
            &db,
            "team-a",
            "tg:group:1",
            "do thing",
            "once",
            "2026-12-25T00:00:00+00:00",
            Some("isolated"),
            None,
        ));
        assert!(!create.is_error);
        let task_id: String = serde_json::from_str::<serde_json::Value>(&create.content)
            .unwrap()
            .get("taskId")
            .unwrap()
            .as_str()
            .unwrap()
            .to_owned();

        let list = srv.list_tasks(&db, "team-a");
        assert!(!list.is_error);

        let pause = srv.pause_task(&db, &task_id, "team-a");
        assert!(!pause.is_error);

        let cancel = srv.cancel_task(&db, &task_id, "team-a");
        assert!(!cancel.is_error);
    }
}
