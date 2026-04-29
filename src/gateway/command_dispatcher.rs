//! Parse and execute admin commands (is_admin groups only).
//! Mirrors `src-old/gateway/CommandDispatcher.ts`.

use std::sync::LazyLock;

use regex::Regex;

use crate::db::Db;
use crate::types::{ScheduledTask, TaskRunLog, TaskStatus};

pub const COMMANDS_HELP: &str = "\
📋 Available commands:
  list_tasks [folder]       — list tasks (optionally filter by group folder)
  task_logs <taskId> [n]    — show latest n execution logs (default 20)
  pause_task <taskId>       — pause task
  resume_task <taskId>      — resume task
  cancel_task <taskId>      — cancel task (mark completed, keep record)
  del_task <taskId>         — delete task (remove permanently)
  history                   — show conversation history stats
  reset                     — reset session (clear chat history)
  help                      — show this help";

/// Try parsing text as an admin command and execute it.
/// Returns command output text, or None (not a command — handle via agent).
/// `chat_jid` is required for `reset` and `history` commands.
pub fn dispatch_command(db: &Db, text: &str, chat_jid: Option<&str>) -> Option<String> {
    let t = text.trim();
    if re_help().is_match(t) {
        return Some(COMMANDS_HELP.to_string());
    }

    if let Some(caps) = re_list_tasks().captures(t) {
        let folder = caps.get(1).map(|m| m.as_str().to_string());
        let tasks = match &folder {
            Some(f) => db.get_tasks_by_group(f).unwrap_or_default(),
            None => db.list_all_tasks().unwrap_or_default(),
        };
        return Some(format_task_list(&tasks, folder.as_deref()));
    }

    if let Some(caps) = re_task_logs().captures(t) {
        let task_id = caps.get(1)?.as_str();
        let limit: u32 = caps.get(2).map(|m| m.as_str().parse().unwrap_or(20)).unwrap_or(20);
        let logs = db.get_task_run_logs(task_id, limit).unwrap_or_default();
        return Some(format_task_logs(task_id, &logs));
    }

    if let Some(caps) = re_manage_task().captures(t) {
        let action = caps.get(1)?.as_str().to_lowercase();
        let task_id = caps.get(2)?.as_str();
        let new_status = match action.as_str() {
            "pause" => TaskStatus::Paused,
            "resume" => TaskStatus::Active,
            "cancel" => TaskStatus::Completed,
            _ => return None,
        };
        if db.update_task_status(task_id, new_status).is_err() {
            return Some(format!("❌ Failed to update task {task_id}"));
        }
        let label = match action.as_str() {
            "pause" => "paused", "resume" => "resumed", _ => "cancelled",
        };
        return Some(format!("✅ Task {task_id} {label}"));
    }

    if let Some(caps) = re_del_task().captures(t) {
        let task_id = caps.get(1)?.as_str();
        match db.delete_task(task_id) {
            Ok(true) => return Some(format!("🗑️ Task {task_id} deleted")),
            Ok(false) => return Some(format!("❌ Task not found: {task_id}")),
            Err(_) => return Some(format!("❌ Failed to delete task {task_id}")),
        }
    }

    if re_history().is_match(t) {
        let jid = chat_jid?;
        let count = db.count_messages(jid).unwrap_or(0);
        let last_ts = db.get_last_agent_timestamp(jid).ok().flatten();
        let cursor = last_ts.as_deref().unwrap_or("(none)");
        return Some(format!(
            "📊 Conversation history — {jid}\n  Messages: {count}\n  Last agent cursor: {cursor}\n\n\
             Send `/reset` to clear all chat history."
        ));
    }

    if re_reset().is_match(t) {
        let jid = chat_jid?;
        let count = db.delete_messages_for_jid(jid).unwrap_or(0);
        let _ = db.delete_agent_timestamp(jid);
        return Some(format!("🗑️ Session reset — cleared {count} messages for {jid}"));
    }

    None
}

// ===== Regex =====

fn re_help() -> &'static Regex {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^/?help$").unwrap());
    &RE
}
fn re_list_tasks() -> &'static Regex {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^/?list[_\s]tasks?(?:\s+(\S+))?$").unwrap());
    &RE
}
fn re_task_logs() -> &'static Regex {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^/?task[_\s]logs?\s+(\S+)(?:\s+(\d+))?$").unwrap());
    &RE
}
fn re_manage_task() -> &'static Regex {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^/?(pause|resume|cancel)[_\s]task\s+(\S+)$").unwrap());
    &RE
}
fn re_del_task() -> &'static Regex {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^/?del[_\s]task\s+(\S+)$").unwrap());
    &RE
}
fn re_history() -> &'static Regex {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^/?history$").unwrap());
    &RE
}
fn re_reset() -> &'static Regex {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^/?reset[_\s]?(session)?$").unwrap());
    &RE
}

// ===== Formatting =====

fn format_task_list(tasks: &[ScheduledTask], folder: Option<&str>) -> String {
    let title = match folder {
        Some(f) => format!("📋 Task List - {f} ({} items)", tasks.len()),
        None => format!("📋 All Tasks ({} items)", tasks.len()),
    };
    if tasks.is_empty() {
        return format!("{title}\nNo tasks");
    }
    let status_icon = |s: TaskStatus| match s {
        TaskStatus::Active => "🟢",
        TaskStatus::Paused => "⏸",
        _ => "⏹",
    };
    let mut lines = vec![title, String::new()];
    for t in tasks {
        lines.push(format!(
            "{} {} · {}",
            status_icon(t.status),
            t.group_folder,
            t.context_mode.as_str()
        ));
        lines.push(format!("   ID: {}", t.id));
        lines.push(format!(
            "   Schedule: {} ({})",
            t.schedule_value,
            t.schedule_type.as_str()
        ));
        let preview: String = if t.prompt.chars().count() > 60 {
            format!("{}…", t.prompt.chars().take(60).collect::<String>())
        } else {
            t.prompt.clone()
        };
        lines.push(format!("   Content: {preview}"));
        if let Some(ref nr) = t.next_run {
            lines.push(format!("   Next: {nr}"));
        }
        if let Some(ref lr) = t.last_run {
            lines.push(format!("   Last: {lr}"));
        }
        lines.push(String::new());
    }
    lines.join("\n").trim_end().to_string()
}

fn format_task_logs(task_id: &str, logs: &[TaskRunLog]) -> String {
    if logs.is_empty() {
        return format!("📜 Task {task_id} No execution records yet");
    }
    let mut lines = vec![
        format!("📜 Execution Logs — {task_id} (latest {} entries)", logs.len()),
        String::new(),
    ];
    for log in logs {
        let icon = match log.status {
            crate::types::RunStatus::Success => "✅",
            crate::types::RunStatus::Error => "❌",
        };
        let dur = log
            .duration_ms
            .map(|d| format!("  ({d}ms)"))
            .unwrap_or_default();
        lines.push(format!("{icon} {}{dur}", log.run_at));
        if let Some(ref result) = log.result {
            let preview: String = if result.chars().count() > 120 {
                format!("{}…", result.chars().take(120).collect::<String>())
            } else {
                result.clone()
            };
            lines.push(format!("   {preview}"));
        }
        if let Some(ref err) = log.error {
            let preview: String = if err.chars().count() > 120 {
                format!("{}…", err.chars().take(120).collect::<String>())
            } else {
                err.clone()
            };
            lines.push(format!("   Error: {preview}"));
        }
        lines.push(String::new());
    }
    lines.join("\n").trim_end().to_string()
}
