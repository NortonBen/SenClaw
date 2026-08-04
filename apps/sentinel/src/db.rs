//! Kho riêng của Sentinel: `~/.senclaw/apps/sentinel/sentinel.db`.
//!
//! Đây là bản sao **chỉ-thêm** của dấu vết hoạt động agent, cộng với kết quả
//! phân tích. Ba tính chất quan trọng:
//!
//! 1. **Bảo toàn** — daemon FIFO-xoá `tool_executions` theo `groups.max_messages`,
//!    nên lịch sử biến mất dần. Bản chép ở đây sống sót qua việc đó.
//! 2. **Chống sửa vết** — mỗi sự kiện mang `prev_hash`/`hash`; sửa hay xoá một
//!    dòng quá khứ làm gãy chuỗi và `verify_chain()` chỉ ra đúng chỗ gãy. Đây là
//!    tamper-*evident*, không phải tamper-proof: người có quyền ghi file vẫn dựng
//!    lại được chuỗi. Mục tiêu là phát hiện sửa lặng lẽ.
//! 3. **Không giữ bí mật** — mọi thứ vào `detail_json` đã qua [`crate::redact`].

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Mutex;

pub struct Db {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS events (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  ts          TEXT NOT NULL,
  source      TEXT NOT NULL,
  kind        TEXT NOT NULL,
  actor       TEXT NOT NULL,
  agent_id    TEXT NOT NULL DEFAULT 'main',
  tool_name   TEXT,
  ok          INTEGER,
  summary     TEXT NOT NULL DEFAULT '',
  detail_json TEXT NOT NULL DEFAULT '{}',
  src_key     TEXT,
  prev_hash   TEXT NOT NULL DEFAULT '',
  hash        TEXT NOT NULL DEFAULT ''
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_events_srckey ON events(src_key) WHERE src_key IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_events_ts    ON events(ts);
CREATE INDEX IF NOT EXISTS idx_events_actor ON events(actor, ts);
CREATE INDEX IF NOT EXISTS idx_events_kind  ON events(kind, ts);
CREATE INDEX IF NOT EXISTS idx_events_tool  ON events(tool_name, ts);

CREATE TABLE IF NOT EXISTS ingest_cursor (
  source   TEXT PRIMARY KEY,
  last_key TEXT NOT NULL DEFAULT '0',
  last_run TEXT NOT NULL DEFAULT '',
  ok       INTEGER NOT NULL DEFAULT 1,
  error    TEXT,
  copied   INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS snapshots (
  id        INTEGER PRIMARY KEY AUTOINCREMENT,
  taken_at  TEXT NOT NULL,
  kind      TEXT NOT NULL,
  body_json TEXT NOT NULL,
  body_hash TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_snapshots_kind ON snapshots(kind, taken_at);

CREATE TABLE IF NOT EXISTS snapshot_diffs (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  kind        TEXT NOT NULL,
  from_id     INTEGER NOT NULL,
  to_id       INTEGER NOT NULL,
  added       TEXT NOT NULL DEFAULT '[]',
  removed     TEXT NOT NULL DEFAULT '[]',
  changed     TEXT NOT NULL DEFAULT '[]',
  detected_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_diffs_kind ON snapshot_diffs(kind, detected_at);

CREATE TABLE IF NOT EXISTS findings (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  rule_id    TEXT NOT NULL,
  severity   TEXT NOT NULL,
  score      INTEGER NOT NULL DEFAULT 0,
  title      TEXT NOT NULL,
  detail     TEXT NOT NULL DEFAULT '',
  actor      TEXT,
  first_ts   TEXT NOT NULL,
  last_ts    TEXT NOT NULL,
  evidence   TEXT NOT NULL DEFAULT '[]',
  standards  TEXT NOT NULL DEFAULT '[]',
  status     TEXT NOT NULL DEFAULT 'open',
  dedupe_key TEXT NOT NULL,
  case_id    INTEGER,
  note       TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_findings_dedupe ON findings(dedupe_key);
CREATE INDEX IF NOT EXISTS idx_findings_status ON findings(status, score);

CREATE TABLE IF NOT EXISTS cases (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  title      TEXT NOT NULL,
  summary    TEXT NOT NULL DEFAULT '',
  status     TEXT NOT NULL DEFAULT 'open',
  severity   TEXT NOT NULL DEFAULT 'medium',
  hypothesis TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  closed_at  TEXT
);

CREATE TABLE IF NOT EXISTS case_notes (
  id      INTEGER PRIMARY KEY AUTOINCREMENT,
  case_id INTEGER NOT NULL,
  author  TEXT NOT NULL DEFAULT 'user',
  body    TEXT NOT NULL,
  ts      TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_case_notes ON case_notes(case_id, id);

CREATE TABLE IF NOT EXISTS rule_config (
  rule_id    TEXT PRIMARY KEY,
  enabled    INTEGER NOT NULL DEFAULT 1,
  severity   TEXT,
  params     TEXT NOT NULL DEFAULT '{}',
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS suppressions (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  rule_id    TEXT NOT NULL,
  match_json TEXT NOT NULL DEFAULT '{}',
  reason     TEXT NOT NULL,
  until      TEXT,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS settings (k TEXT PRIMARY KEY, v TEXT NOT NULL);
"#;

pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

pub fn sha256_hex(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    format!("{:x}", h.finalize())
}

/// Một sự kiện sắp được ghi. `src_key` là `<nguồn>:<id gốc>` để chạy ingest lại
/// không sinh bản trùng.
#[derive(Debug, Clone)]
pub struct NewEvent {
    pub ts: String,
    pub source: String,
    pub kind: String,
    pub actor: String,
    pub agent_id: String,
    pub tool_name: Option<String>,
    pub ok: Option<bool>,
    pub summary: String,
    pub detail: Value,
    pub src_key: Option<String>,
}

impl NewEvent {
    pub fn new(source: &str, kind: &str, actor: &str, ts: &str) -> Self {
        Self {
            ts: ts.to_string(),
            source: source.to_string(),
            kind: kind.to_string(),
            actor: actor.to_string(),
            agent_id: "main".into(),
            tool_name: None,
            ok: None,
            summary: String::new(),
            detail: json!({}),
            src_key: None,
        }
    }
    pub fn tool(mut self, t: &str) -> Self {
        self.tool_name = Some(t.to_string());
        self
    }
    pub fn agent(mut self, a: &str) -> Self {
        self.agent_id = a.to_string();
        self
    }
    pub fn ok(mut self, v: bool) -> Self {
        self.ok = Some(v);
        self
    }
    pub fn summary(mut self, s: impl Into<String>) -> Self {
        self.summary = s.into();
        self
    }
    pub fn detail(mut self, v: Value) -> Self {
        self.detail = v;
        self
    }
    pub fn key(mut self, k: String) -> Self {
        self.src_key = Some(k);
        self
    }
}

/// Trường đưa vào hàm băm. Cố định và có thứ tự — đổi công thức này là làm gãy
/// mọi chuỗi đã ghi, nên nếu buộc phải đổi thì phải kèm migration đánh dấu.
fn event_digest(prev_hash: &str, e: &NewEvent, detail_json: &str) -> String {
    let material = format!(
        "{prev}\u{1f}{ts}\u{1f}{src}\u{1f}{kind}\u{1f}{actor}\u{1f}{agent}\u{1f}{tool}\u{1f}{ok}\u{1f}{sum}\u{1f}{det}",
        prev = prev_hash,
        ts = e.ts,
        src = e.source,
        kind = e.kind,
        actor = e.actor,
        agent = e.agent_id,
        tool = e.tool_name.clone().unwrap_or_default(),
        ok = e.ok.map(|b| if b { "1" } else { "0" }).unwrap_or("-"),
        sum = e.summary,
        det = detail_json,
    );
    sha256_hex(&material)
}

impl Db {
    pub fn open_default() -> Result<Self> {
        let dir = std::env::var("SENCLAW_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                PathBuf::from(home)
                    .join(".senclaw")
                    .join("apps")
                    .join("sentinel")
            });
        std::fs::create_dir_all(&dir).ok();
        Self::open(dir.join("sentinel.db"))
    }

    pub fn open(path: PathBuf) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    #[cfg(test)]
    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    // ---------------- events ----------------

    /// Ghi một sự kiện, nối vào chuỗi băm. Trả `Ok(None)` khi `src_key` đã tồn
    /// tại (ingest chạy lại) — đó là đường bình thường, không phải lỗi.
    pub fn append_event(&self, e: &NewEvent) -> Result<Option<i64>> {
        let conn = self.conn.lock().unwrap();
        if let Some(k) = &e.src_key {
            let dup: Option<i64> = conn
                .query_row("SELECT id FROM events WHERE src_key = ?1", params![k], |r| {
                    r.get(0)
                })
                .optional()?;
            if dup.is_some() {
                return Ok(None);
            }
        }
        let prev: String = conn
            .query_row(
                "SELECT hash FROM events ORDER BY id DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or_default();

        let detail = crate::redact::redact_value(&e.detail);
        let detail_json = detail.to_string();
        let summary = crate::redact::redact(&e.summary);
        let mut ev = e.clone();
        ev.summary = summary;
        let hash = event_digest(&prev, &ev, &detail_json);

        conn.execute(
            r#"INSERT INTO events
               (ts, source, kind, actor, agent_id, tool_name, ok, summary, detail_json, src_key, prev_hash, hash)
               VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)"#,
            params![
                ev.ts,
                ev.source,
                ev.kind,
                ev.actor,
                ev.agent_id,
                ev.tool_name,
                ev.ok.map(|b| b as i64),
                ev.summary,
                detail_json,
                ev.src_key,
                prev,
                hash
            ],
        )?;
        Ok(Some(conn.last_insert_rowid()))
    }

    /// Duyệt lại toàn bộ chuỗi. Trả `(số dòng kiểm tra, id dòng gãy đầu tiên)`.
    pub fn verify_chain(&self) -> Result<(i64, Option<i64>)> {
        let conn = self.conn.lock().unwrap();
        let mut st = conn.prepare(
            "SELECT id, ts, source, kind, actor, agent_id, tool_name, ok, summary, detail_json, src_key, prev_hash, hash
             FROM events ORDER BY id ASC",
        )?;
        let mut rows = st.query([])?;
        let mut prev = String::new();
        let mut n = 0i64;
        while let Some(r) = rows.next()? {
            n += 1;
            let id: i64 = r.get(0)?;
            let stored_prev: String = r.get(11)?;
            let stored_hash: String = r.get(12)?;
            let detail_json: String = r.get(9)?;
            let ok_i: Option<i64> = r.get(7)?;
            let e = NewEvent {
                ts: r.get(1)?,
                source: r.get(2)?,
                kind: r.get(3)?,
                actor: r.get(4)?,
                agent_id: r.get(5)?,
                tool_name: r.get(6)?,
                ok: ok_i.map(|v| v != 0),
                summary: r.get(8)?,
                detail: json!({}),
                src_key: r.get(10)?,
            };
            if stored_prev != prev {
                return Ok((n, Some(id)));
            }
            if event_digest(&prev, &e, &detail_json) != stored_hash {
                return Ok((n, Some(id)));
            }
            prev = stored_hash;
        }
        Ok((n, None))
    }

    /// Truy vấn dòng thời gian. Mọi tham số đều tuỳ chọn; `q` tìm trong summary.
    #[allow(clippy::too_many_arguments)]
    pub fn events(
        &self,
        from: Option<&str>,
        to: Option<&str>,
        actor: Option<&str>,
        kind: Option<&str>,
        tool: Option<&str>,
        q: Option<&str>,
        limit: i64,
        before_id: Option<i64>,
    ) -> Result<Vec<Value>> {
        let conn = self.conn.lock().unwrap();
        let mut sql = String::from(
            "SELECT id, ts, source, kind, actor, agent_id, tool_name, ok, summary, detail_json
             FROM events WHERE 1=1",
        );
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(v) = from {
            sql.push_str(" AND ts >= ?");
            args.push(Box::new(v.to_string()));
        }
        if let Some(v) = to {
            sql.push_str(" AND ts <= ?");
            args.push(Box::new(v.to_string()));
        }
        if let Some(v) = actor {
            sql.push_str(" AND actor = ?");
            args.push(Box::new(v.to_string()));
        }
        if let Some(v) = kind {
            sql.push_str(" AND kind = ?");
            args.push(Box::new(v.to_string()));
        }
        if let Some(v) = tool {
            sql.push_str(" AND tool_name LIKE ?");
            args.push(Box::new(format!("%{v}%")));
        }
        if let Some(v) = q {
            sql.push_str(" AND (summary LIKE ? OR detail_json LIKE ?)");
            args.push(Box::new(format!("%{v}%")));
            args.push(Box::new(format!("%{v}%")));
        }
        if let Some(v) = before_id {
            sql.push_str(" AND id < ?");
            args.push(Box::new(v));
        }
        sql.push_str(" ORDER BY id DESC LIMIT ?");
        // Trần cao vì rule engine nạp cả kho để tương quan; kẹp thấp ở đây từng
        // làm luật "khoảng cách phê duyệt" im lặng dù dữ liệu có đủ — nó xin
        // 20.000 sự kiện nhưng chỉ nhận 2.000 mà không có dấu hiệu gì báo là đã
        // bị cắt. Giới hạn cho người dùng được đặt ở tầng API, không ở đây.
        args.push(Box::new(limit.clamp(1, 200_000)));

        let mut st = conn.prepare(&sql)?;
        let refs: Vec<&dyn rusqlite::ToSql> = args.iter().map(|b| b.as_ref()).collect();
        let mut rows = st.query(refs.as_slice())?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            out.push(row_to_event(r)?);
        }
        Ok(out)
    }

    pub fn event(&self, id: i64) -> Result<Option<Value>> {
        let conn = self.conn.lock().unwrap();
        let v = conn
            .query_row(
                "SELECT id, ts, source, kind, actor, agent_id, tool_name, ok, summary, detail_json
                 FROM events WHERE id = ?1",
                params![id],
                row_to_event,
            )
            .optional()?;
        Ok(v)
    }

    pub fn events_by_ids(&self, ids: &[i64]) -> Result<Vec<Value>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let conn = self.conn.lock().unwrap();
        let ph = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT id, ts, source, kind, actor, agent_id, tool_name, ok, summary, detail_json
             FROM events WHERE id IN ({ph}) ORDER BY ts ASC"
        );
        let mut st = conn.prepare(&sql)?;
        let refs: Vec<&dyn rusqlite::ToSql> =
            ids.iter().map(|i| i as &dyn rusqlite::ToSql).collect();
        let mut rows = st.query(refs.as_slice())?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            out.push(row_to_event(r)?);
        }
        Ok(out)
    }

    /// Sự kiện quanh một mốc thời gian của cùng actor — nguyên hàm pivot.
    pub fn events_near(&self, actor: &str, ts: &str, minutes: i64) -> Result<Vec<Value>> {
        let center = chrono::DateTime::parse_from_rfc3339(ts)
            .map_err(|e| anyhow!("mốc thời gian không hợp lệ: {e}"))?;
        let from = (center - chrono::Duration::minutes(minutes)).to_rfc3339();
        let to = (center + chrono::Duration::minutes(minutes)).to_rfc3339();
        self.events(Some(&from), Some(&to), Some(actor), None, None, None, 500, None)
    }

    pub fn event_count(&self) -> i64 {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
            .unwrap_or(0)
    }

    pub fn event_span(&self) -> (Option<String>, Option<String>) {
        let conn = self.conn.lock().unwrap();
        let lo = conn
            .query_row("SELECT MIN(ts) FROM events", [], |r| r.get(0))
            .unwrap_or(None);
        let hi = conn
            .query_row("SELECT MAX(ts) FROM events", [], |r| r.get(0))
            .unwrap_or(None);
        (lo, hi)
    }

    /// Đếm sự kiện theo ngày cho biểu đồ hoạt động.
    pub fn activity_by_day(&self, days: i64) -> Result<Vec<Value>> {
        let conn = self.conn.lock().unwrap();
        let mut st = conn.prepare(
            "SELECT substr(ts,1,10) AS d, COUNT(*), SUM(CASE WHEN ok = 0 THEN 1 ELSE 0 END)
             FROM events GROUP BY d ORDER BY d DESC LIMIT ?1",
        )?;
        let mut rows = st.query(params![days])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            out.push(json!({
                "day": r.get::<_, String>(0)?,
                "count": r.get::<_, i64>(1)?,
                "failed": r.get::<_, Option<i64>>(2)?.unwrap_or(0),
            }));
        }
        out.reverse();
        Ok(out)
    }

    // ---------------- cursor ----------------

    pub fn cursor(&self, source: &str) -> String {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT last_key FROM ingest_cursor WHERE source = ?1",
            params![source],
            |r| r.get(0),
        )
        .optional()
        .ok()
        .flatten()
        .unwrap_or_else(|| "0".to_string())
    }

    pub fn set_cursor(&self, source: &str, last_key: &str, copied: i64, err: Option<&str>) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            r#"INSERT INTO ingest_cursor (source, last_key, last_run, ok, error, copied)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6)
               ON CONFLICT(source) DO UPDATE SET
                 last_key = excluded.last_key,
                 last_run = excluded.last_run,
                 ok       = excluded.ok,
                 error    = excluded.error,
                 copied   = ingest_cursor.copied + excluded.copied"#,
            params![
                source,
                last_key,
                now_rfc3339(),
                err.is_none() as i64,
                err,
                copied
            ],
        );
    }

    pub fn cursors(&self) -> Result<Vec<Value>> {
        let conn = self.conn.lock().unwrap();
        let mut st = conn.prepare(
            "SELECT source, last_key, last_run, ok, error, copied FROM ingest_cursor ORDER BY source",
        )?;
        let mut rows = st.query([])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            out.push(json!({
                "source":  r.get::<_, String>(0)?,
                "last_key": r.get::<_, String>(1)?,
                "last_run": r.get::<_, String>(2)?,
                "ok":      r.get::<_, i64>(3)? != 0,
                "error":   r.get::<_, Option<String>>(4)?,
                "copied":  r.get::<_, i64>(5)?,
            }));
        }
        Ok(out)
    }

    // ---------------- snapshots ----------------

    /// Lưu ảnh chụp nếu nội dung khác lần trước. Trả `Some((from_id, to_id))`
    /// khi có thay đổi thật sự — người gọi dùng để sinh diff.
    pub fn put_snapshot(&self, kind: &str, body: &Value) -> Result<Option<(i64, i64)>> {
        let body_json = serde_json::to_string(body)?;
        let body_hash = sha256_hex(&body_json);
        let conn = self.conn.lock().unwrap();
        let last: Option<(i64, String)> = conn
            .query_row(
                "SELECT id, body_hash FROM snapshots WHERE kind = ?1 ORDER BY id DESC LIMIT 1",
                params![kind],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((id, h)) = &last {
            if *h == body_hash {
                let _ = conn.execute(
                    "UPDATE snapshots SET taken_at = ?1 WHERE id = ?2",
                    params![now_rfc3339(), id],
                );
                return Ok(None);
            }
        }
        conn.execute(
            "INSERT INTO snapshots (taken_at, kind, body_json, body_hash) VALUES (?1,?2,?3,?4)",
            params![now_rfc3339(), kind, body_json, body_hash],
        )?;
        let new_id = conn.last_insert_rowid();
        Ok(Some((last.map(|(i, _)| i).unwrap_or(0), new_id)))
    }

    pub fn snapshot_body(&self, id: i64) -> Result<Option<Value>> {
        let conn = self.conn.lock().unwrap();
        let s: Option<String> = conn
            .query_row(
                "SELECT body_json FROM snapshots WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(s.and_then(|t| serde_json::from_str(&t).ok()))
    }

    pub fn latest_snapshot(&self, kind: &str) -> Result<Option<(i64, Value)>> {
        let conn = self.conn.lock().unwrap();
        let row: Option<(i64, String)> = conn
            .query_row(
                "SELECT id, body_json FROM snapshots WHERE kind = ?1 ORDER BY id DESC LIMIT 1",
                params![kind],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        Ok(row.and_then(|(i, s)| serde_json::from_str(&s).ok().map(|v| (i, v))))
    }

    pub fn snapshots(&self, kind: Option<&str>, limit: i64) -> Result<Vec<Value>> {
        let conn = self.conn.lock().unwrap();
        let mut out = Vec::new();
        if let Some(k) = kind {
            let mut st = conn.prepare(
                "SELECT id, taken_at, kind, body_hash, length(body_json)
                 FROM snapshots WHERE kind = ?1 ORDER BY id DESC LIMIT ?2",
            )?;
            let mut rows = st.query(params![k, limit])?;
            while let Some(r) = rows.next()? {
                out.push(snapshot_row(r)?);
            }
        } else {
            let mut st = conn.prepare(
                "SELECT id, taken_at, kind, body_hash, length(body_json)
                 FROM snapshots ORDER BY id DESC LIMIT ?1",
            )?;
            let mut rows = st.query(params![limit])?;
            while let Some(r) = rows.next()? {
                out.push(snapshot_row(r)?);
            }
        }
        Ok(out)
    }

    pub fn put_diff(
        &self,
        kind: &str,
        from_id: i64,
        to_id: i64,
        added: &Value,
        removed: &Value,
        changed: &Value,
    ) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"INSERT INTO snapshot_diffs (kind, from_id, to_id, added, removed, changed, detected_at)
               VALUES (?1,?2,?3,?4,?5,?6,?7)"#,
            params![
                kind,
                from_id,
                to_id,
                added.to_string(),
                removed.to_string(),
                changed.to_string(),
                now_rfc3339()
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn diffs(&self, kind: Option<&str>, limit: i64) -> Result<Vec<Value>> {
        let conn = self.conn.lock().unwrap();
        let sql = match kind {
            Some(_) => "SELECT id, kind, from_id, to_id, added, removed, changed, detected_at
                        FROM snapshot_diffs WHERE kind = ?1 ORDER BY id DESC LIMIT ?2",
            None => "SELECT id, kind, from_id, to_id, added, removed, changed, detected_at
                     FROM snapshot_diffs WHERE ?1 IS NULL ORDER BY id DESC LIMIT ?2",
        };
        let mut st = conn.prepare(sql)?;
        let mut rows = st.query(params![kind, limit])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            out.push(json!({
                "id": r.get::<_, i64>(0)?,
                "kind": r.get::<_, String>(1)?,
                "from_id": r.get::<_, i64>(2)?,
                "to_id": r.get::<_, i64>(3)?,
                "added": serde_json::from_str::<Value>(&r.get::<_, String>(4)?).unwrap_or(json!([])),
                "removed": serde_json::from_str::<Value>(&r.get::<_, String>(5)?).unwrap_or(json!([])),
                "changed": serde_json::from_str::<Value>(&r.get::<_, String>(6)?).unwrap_or(json!([])),
                "detected_at": r.get::<_, String>(7)?,
            }));
        }
        Ok(out)
    }

    // ---------------- findings ----------------

    /// Ghi phát hiện. Trùng `dedupe_key` thì cập nhật mốc cuối + chứng cứ thay
    /// vì tạo dòng mới — nếu không, mỗi lần quét lại sẽ nhân bản hàng đợi.
    pub fn upsert_finding(&self, f: &Value) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let key = f["dedupe_key"].as_str().unwrap_or_default();
        let existing: Option<(i64, String)> = conn
            .query_row(
                "SELECT id, status FROM findings WHERE dedupe_key = ?1",
                params![key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;

        if let Some((id, _status)) = existing {
            conn.execute(
                r#"UPDATE findings SET
                     last_ts = ?1, evidence = ?2, score = ?3, detail = ?4, severity = ?5, updated_at = ?6
                   WHERE id = ?7"#,
                params![
                    f["last_ts"].as_str().unwrap_or_default(),
                    f["evidence"].to_string(),
                    f["score"].as_i64().unwrap_or(0),
                    f["detail"].as_str().unwrap_or_default(),
                    f["severity"].as_str().unwrap_or("medium"),
                    now_rfc3339(),
                    id
                ],
            )?;
            return Ok(id);
        }

        conn.execute(
            r#"INSERT INTO findings
               (rule_id, severity, score, title, detail, actor, first_ts, last_ts,
                evidence, standards, status, dedupe_key, note, created_at, updated_at)
               VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'open',?11,'',?12,?12)"#,
            params![
                f["rule_id"].as_str().unwrap_or_default(),
                f["severity"].as_str().unwrap_or("medium"),
                f["score"].as_i64().unwrap_or(0),
                f["title"].as_str().unwrap_or_default(),
                f["detail"].as_str().unwrap_or_default(),
                f["actor"].as_str(),
                f["first_ts"].as_str().unwrap_or_default(),
                f["last_ts"].as_str().unwrap_or_default(),
                f["evidence"].to_string(),
                f["standards"].to_string(),
                key,
                now_rfc3339()
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn findings(
        &self,
        status: Option<&str>,
        severity: Option<&str>,
        rule: Option<&str>,
        limit: i64,
    ) -> Result<Vec<Value>> {
        let conn = self.conn.lock().unwrap();
        let mut sql = String::from(
            "SELECT id, rule_id, severity, score, title, detail, actor, first_ts, last_ts,
                    evidence, standards, status, note, case_id, created_at
             FROM findings WHERE 1=1",
        );
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(v) = status {
            sql.push_str(" AND status = ?");
            args.push(Box::new(v.to_string()));
        }
        if let Some(v) = severity {
            sql.push_str(" AND severity = ?");
            args.push(Box::new(v.to_string()));
        }
        if let Some(v) = rule {
            sql.push_str(" AND rule_id = ?");
            args.push(Box::new(v.to_string()));
        }
        sql.push_str(" ORDER BY score DESC, last_ts DESC LIMIT ?");
        args.push(Box::new(limit.clamp(1, 500)));
        let mut st = conn.prepare(&sql)?;
        let refs: Vec<&dyn rusqlite::ToSql> = args.iter().map(|b| b.as_ref()).collect();
        let mut rows = st.query(refs.as_slice())?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            out.push(finding_row(r)?);
        }
        Ok(out)
    }

    pub fn finding(&self, id: i64) -> Result<Option<Value>> {
        let conn = self.conn.lock().unwrap();
        let v = conn
            .query_row(
                "SELECT id, rule_id, severity, score, title, detail, actor, first_ts, last_ts,
                        evidence, standards, status, note, case_id, created_at
                 FROM findings WHERE id = ?1",
                params![id],
                finding_row,
            )
            .optional()?;
        Ok(v)
    }

    pub fn set_finding_status(&self, id: i64, status: &str, note: Option<&str>) -> Result<()> {
        const OK: &[&str] = &[
            "open",
            "triaged",
            "accepted_risk",
            "false_positive",
            "resolved",
        ];
        if !OK.contains(&status) {
            return Err(anyhow!("trạng thái không hợp lệ: {status}"));
        }
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "UPDATE findings SET status = ?1, note = COALESCE(?2, note), updated_at = ?3 WHERE id = ?4",
            params![status, note, now_rfc3339(), id],
        )?;
        if n == 0 {
            return Err(anyhow!("không có phát hiện id={id}"));
        }
        Ok(())
    }

    pub fn attach_finding_to_case(&self, finding_id: i64, case_id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE findings SET case_id = ?1, updated_at = ?2 WHERE id = ?3",
            params![case_id, now_rfc3339(), finding_id],
        )?;
        Ok(())
    }

    pub fn finding_counts(&self) -> Result<Value> {
        let conn = self.conn.lock().unwrap();
        let mut st = conn.prepare(
            "SELECT severity, COUNT(*) FROM findings WHERE status IN ('open','triaged') GROUP BY severity",
        )?;
        let mut rows = st.query([])?;
        let mut m = json!({"critical":0,"high":0,"medium":0,"low":0,"info":0});
        while let Some(r) = rows.next()? {
            let s: String = r.get(0)?;
            let c: i64 = r.get(1)?;
            m[s] = json!(c);
        }
        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM findings", [], |r| r.get(0))
            .unwrap_or(0);
        let open: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM findings WHERE status IN ('open','triaged')",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        Ok(json!({ "by_severity": m, "total": total, "open": open }))
    }

    // ---------------- cases ----------------

    pub fn create_case(&self, title: &str, summary: &str, severity: &str) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO cases (title, summary, severity, created_at, updated_at) VALUES (?1,?2,?3,?4,?4)",
            params![title, summary, severity, now_rfc3339()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn cases(&self, status: Option<&str>) -> Result<Vec<Value>> {
        let conn = self.conn.lock().unwrap();
        let mut st = conn.prepare(
            "SELECT c.id, c.title, c.summary, c.status, c.severity, c.hypothesis, c.created_at,
                    (SELECT COUNT(*) FROM findings f WHERE f.case_id = c.id)
             FROM cases c
             WHERE (?1 IS NULL OR c.status = ?1)
             ORDER BY c.id DESC",
        )?;
        let mut rows = st.query(params![status])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            out.push(json!({
                "id": r.get::<_, i64>(0)?,
                "title": r.get::<_, String>(1)?,
                "summary": r.get::<_, String>(2)?,
                "status": r.get::<_, String>(3)?,
                "severity": r.get::<_, String>(4)?,
                "hypothesis": r.get::<_, String>(5)?,
                "created_at": r.get::<_, String>(6)?,
                "finding_count": r.get::<_, i64>(7)?,
            }));
        }
        Ok(out)
    }

    pub fn case_detail(&self, id: i64) -> Result<Option<Value>> {
        let conn = self.conn.lock().unwrap();
        let base: Option<Value> = conn
            .query_row(
                "SELECT id, title, summary, status, severity, hypothesis, created_at, updated_at, closed_at
                 FROM cases WHERE id = ?1",
                params![id],
                |r| {
                    Ok(json!({
                        "id": r.get::<_, i64>(0)?,
                        "title": r.get::<_, String>(1)?,
                        "summary": r.get::<_, String>(2)?,
                        "status": r.get::<_, String>(3)?,
                        "severity": r.get::<_, String>(4)?,
                        "hypothesis": r.get::<_, String>(5)?,
                        "created_at": r.get::<_, String>(6)?,
                        "updated_at": r.get::<_, String>(7)?,
                        "closed_at": r.get::<_, Option<String>>(8)?,
                    }))
                },
            )
            .optional()?;
        let Some(mut c) = base else {
            return Ok(None);
        };
        drop(conn);

        c["findings"] = json!(self.findings_of_case(id)?);
        let conn = self.conn.lock().unwrap();
        let mut st =
            conn.prepare("SELECT id, author, body, ts FROM case_notes WHERE case_id = ?1 ORDER BY id")?;
        let mut rows = st.query(params![id])?;
        let mut notes = Vec::new();
        while let Some(r) = rows.next()? {
            notes.push(json!({
                "id": r.get::<_, i64>(0)?,
                "author": r.get::<_, String>(1)?,
                "body": r.get::<_, String>(2)?,
                "ts": r.get::<_, String>(3)?,
            }));
        }
        c["notes"] = json!(notes);
        Ok(Some(c))
    }

    pub fn findings_of_case(&self, case_id: i64) -> Result<Vec<Value>> {
        let conn = self.conn.lock().unwrap();
        let mut st = conn.prepare(
            "SELECT id, rule_id, severity, score, title, detail, actor, first_ts, last_ts,
                    evidence, standards, status, note, case_id, created_at
             FROM findings WHERE case_id = ?1 ORDER BY score DESC",
        )?;
        let mut rows = st.query(params![case_id])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            out.push(finding_row(r)?);
        }
        Ok(out)
    }

    pub fn update_case(&self, id: i64, patch: &Value) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        for (col, key) in [
            ("title", "title"),
            ("summary", "summary"),
            ("status", "status"),
            ("severity", "severity"),
            ("hypothesis", "hypothesis"),
        ] {
            if let Some(v) = patch[key].as_str() {
                conn.execute(
                    &format!("UPDATE cases SET {col} = ?1, updated_at = ?2 WHERE id = ?3"),
                    params![v, now_rfc3339(), id],
                )?;
            }
        }
        if patch["status"].as_str() == Some("closed") {
            conn.execute(
                "UPDATE cases SET closed_at = ?1 WHERE id = ?2 AND closed_at IS NULL",
                params![now_rfc3339(), id],
            )?;
        }
        Ok(())
    }

    pub fn add_case_note(&self, case_id: i64, author: &str, body: &str) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO case_notes (case_id, author, body, ts) VALUES (?1,?2,?3,?4)",
            params![case_id, author, crate::redact::redact(body), now_rfc3339()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    // ---------------- rule config & suppressions ----------------

    pub fn rule_config(&self, rule_id: &str) -> (bool, Option<String>, Value) {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT enabled, severity, params FROM rule_config WHERE rule_id = ?1",
            params![rule_id],
            |r| {
                let enabled: i64 = r.get(0)?;
                let sev: Option<String> = r.get(1)?;
                let p: String = r.get(2)?;
                Ok((
                    enabled != 0,
                    sev,
                    serde_json::from_str(&p).unwrap_or(json!({})),
                ))
            },
        )
        .optional()
        .ok()
        .flatten()
        .unwrap_or((true, None, json!({})))
    }

    pub fn set_rule_config(
        &self,
        rule_id: &str,
        enabled: Option<bool>,
        severity: Option<&str>,
        params_v: Option<&Value>,
    ) -> Result<()> {
        let (cur_en, cur_sev, cur_params) = self.rule_config(rule_id);
        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"INSERT INTO rule_config (rule_id, enabled, severity, params, updated_at)
               VALUES (?1,?2,?3,?4,?5)
               ON CONFLICT(rule_id) DO UPDATE SET
                 enabled = excluded.enabled,
                 severity = excluded.severity,
                 params = excluded.params,
                 updated_at = excluded.updated_at"#,
            params![
                rule_id,
                enabled.unwrap_or(cur_en) as i64,
                severity.map(|s| s.to_string()).or(cur_sev),
                params_v.cloned().unwrap_or(cur_params).to_string(),
                now_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn suppressions(&self) -> Result<Vec<Value>> {
        let conn = self.conn.lock().unwrap();
        let mut st = conn.prepare(
            "SELECT id, rule_id, match_json, reason, until, created_at FROM suppressions ORDER BY id DESC",
        )?;
        let mut rows = st.query([])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            out.push(json!({
                "id": r.get::<_, i64>(0)?,
                "rule_id": r.get::<_, String>(1)?,
                "match": serde_json::from_str::<Value>(&r.get::<_, String>(2)?).unwrap_or(json!({})),
                "reason": r.get::<_, String>(3)?,
                "until": r.get::<_, Option<String>>(4)?,
                "created_at": r.get::<_, String>(5)?,
            }));
        }
        Ok(out)
    }

    pub fn add_suppression(
        &self,
        rule_id: &str,
        m: &Value,
        reason: &str,
        until: Option<&str>,
    ) -> Result<i64> {
        if reason.trim().is_empty() {
            // Bắt buộc có lý do: sáu tháng sau còn phải biết vì sao đã tắt.
            return Err(anyhow!("phải nêu lý do khi bỏ qua một luật"));
        }
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO suppressions (rule_id, match_json, reason, until, created_at) VALUES (?1,?2,?3,?4,?5)",
            params![rule_id, m.to_string(), reason, until, now_rfc3339()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn delete_suppression(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM suppressions WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Suppression còn hiệu lực (chưa hết hạn).
    pub fn active_suppressions(&self) -> Vec<(String, Value)> {
        let now = now_rfc3339();
        self.suppressions()
            .unwrap_or_default()
            .into_iter()
            .filter(|s| match s["until"].as_str() {
                Some(u) => u > now.as_str(),
                None => true,
            })
            .map(|s| (s["rule_id"].as_str().unwrap_or("").to_string(), s["match"].clone()))
            .collect()
    }

    // ---------------- settings ----------------

    pub fn get_setting(&self, k: &str) -> Option<String> {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT v FROM settings WHERE k = ?1", params![k], |r| {
            r.get(0)
        })
        .optional()
        .ok()
        .flatten()
    }

    pub fn set_setting(&self, k: &str, v: &str) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO settings (k,v) VALUES (?1,?2) ON CONFLICT(k) DO UPDATE SET v = excluded.v",
            params![k, v],
        );
    }
}

fn row_to_event(r: &rusqlite::Row) -> rusqlite::Result<Value> {
    Ok(json!({
        "id": r.get::<_, i64>(0)?,
        "ts": r.get::<_, String>(1)?,
        "source": r.get::<_, String>(2)?,
        "kind": r.get::<_, String>(3)?,
        "actor": r.get::<_, String>(4)?,
        "agent_id": r.get::<_, String>(5)?,
        "tool_name": r.get::<_, Option<String>>(6)?,
        "ok": r.get::<_, Option<i64>>(7)?.map(|v| v != 0),
        "summary": r.get::<_, String>(8)?,
        "detail": serde_json::from_str::<Value>(&r.get::<_, String>(9)?).unwrap_or(json!({})),
    }))
}

fn snapshot_row(r: &rusqlite::Row) -> rusqlite::Result<Value> {
    Ok(json!({
        "id": r.get::<_, i64>(0)?,
        "taken_at": r.get::<_, String>(1)?,
        "kind": r.get::<_, String>(2)?,
        "body_hash": r.get::<_, String>(3)?,
        "bytes": r.get::<_, i64>(4)?,
    }))
}

fn finding_row(r: &rusqlite::Row) -> rusqlite::Result<Value> {
    Ok(json!({
        "id": r.get::<_, i64>(0)?,
        "rule_id": r.get::<_, String>(1)?,
        "severity": r.get::<_, String>(2)?,
        "score": r.get::<_, i64>(3)?,
        "title": r.get::<_, String>(4)?,
        "detail": r.get::<_, String>(5)?,
        "actor": r.get::<_, Option<String>>(6)?,
        "first_ts": r.get::<_, String>(7)?,
        "last_ts": r.get::<_, String>(8)?,
        "evidence": serde_json::from_str::<Value>(&r.get::<_, String>(9)?).unwrap_or(json!([])),
        "standards": serde_json::from_str::<Value>(&r.get::<_, String>(10)?).unwrap_or(json!([])),
        "status": r.get::<_, String>(11)?,
        "note": r.get::<_, String>(12)?,
        "case_id": r.get::<_, Option<i64>>(13)?,
        "created_at": r.get::<_, String>(14)?,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(kind: &str, actor: &str, ts: &str) -> NewEvent {
        NewEvent::new("test", kind, actor, ts).summary(format!("{kind} của {actor}"))
    }

    #[test]
    fn append_and_read_back() {
        let db = Db::open_memory().unwrap();
        db.append_event(&ev("tool_call", "chat:a", "2026-07-01T00:00:00Z"))
            .unwrap();
        db.append_event(&ev("tool_call", "chat:b", "2026-07-01T00:01:00Z"))
            .unwrap();
        assert_eq!(db.event_count(), 2);
        let rows = db
            .events(None, None, Some("chat:a"), None, None, None, 10, None)
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["actor"], "chat:a");
    }

    #[test]
    fn src_key_blocks_duplicates() {
        let db = Db::open_memory().unwrap();
        let e = ev("tool_call", "chat:a", "2026-07-01T00:00:00Z").key("tool_executions:7".into());
        assert!(db.append_event(&e).unwrap().is_some());
        assert!(
            db.append_event(&e).unwrap().is_none(),
            "chạy ingest lại không được nhân bản"
        );
        assert_eq!(db.event_count(), 1);
    }

    #[test]
    fn hash_chain_is_intact_when_untouched() {
        let db = Db::open_memory().unwrap();
        for i in 0..5 {
            db.append_event(&ev("tool_call", "chat:a", &format!("2026-07-01T00:0{i}:00Z")))
                .unwrap();
        }
        let (n, broken) = db.verify_chain().unwrap();
        assert_eq!(n, 5);
        assert!(broken.is_none(), "chuỗi phải nguyên vẹn");
    }

    #[test]
    fn hash_chain_detects_silent_edit() {
        let db = Db::open_memory().unwrap();
        for i in 0..5 {
            db.append_event(&ev("tool_call", "chat:a", &format!("2026-07-01T00:0{i}:00Z")))
                .unwrap();
        }
        {
            let conn = db.conn.lock().unwrap();
            conn.execute("UPDATE events SET summary = 'đã bị sửa' WHERE id = 3", [])
                .unwrap();
        }
        let (_, broken) = db.verify_chain().unwrap();
        assert_eq!(broken, Some(3), "phải chỉ ra đúng dòng bị sửa");
    }

    #[test]
    fn hash_chain_detects_deletion() {
        let db = Db::open_memory().unwrap();
        for i in 0..5 {
            db.append_event(&ev("tool_call", "chat:a", &format!("2026-07-01T00:0{i}:00Z")))
                .unwrap();
        }
        {
            let conn = db.conn.lock().unwrap();
            conn.execute("DELETE FROM events WHERE id = 3", []).unwrap();
        }
        let (_, broken) = db.verify_chain().unwrap();
        assert_eq!(broken, Some(4), "dòng kế tiếp phải lộ ra chỗ gãy");
    }

    #[test]
    fn secrets_never_land_in_store() {
        let db = Db::open_memory().unwrap();
        let e = NewEvent::new("test", "tool_call", "chat:a", "2026-07-01T00:00:00Z")
            .summary("chạy với ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ012345")
            .detail(json!({"headers": {"Authorization": "Bearer sk-abcdefghijklmnopqrstuvwx1234"}}));
        db.append_event(&e).unwrap();
        let rows = db.events(None, None, None, None, None, None, 10, None).unwrap();
        let s = rows[0].to_string();
        assert!(!s.contains("ghp_ABCDEF"), "{s}");
        assert!(!s.contains("sk-abcdefghij"), "{s}");
    }

    #[test]
    fn snapshot_skips_identical_body() {
        let db = Db::open_memory().unwrap();
        let a = json!({"servers": ["x", "y"]});
        assert!(db.put_snapshot("mcp_servers", &a).unwrap().is_some());
        assert!(
            db.put_snapshot("mcp_servers", &a).unwrap().is_none(),
            "nội dung không đổi thì không lưu thêm"
        );
        let b = json!({"servers": ["x", "y", "z"]});
        let r = db.put_snapshot("mcp_servers", &b).unwrap();
        assert!(r.is_some(), "nội dung đổi thì phải lưu");
    }

    #[test]
    fn finding_dedupes_by_key() {
        let db = Db::open_memory().unwrap();
        let f = json!({
            "rule_id": "SEN-CTRL-01", "severity": "critical", "score": 90,
            "title": "HITL tắt", "detail": "x", "actor": null,
            "first_ts": "2026-07-01T00:00:00Z", "last_ts": "2026-07-01T00:00:00Z",
            "evidence": [1], "standards": ["LLM06"], "dedupe_key": "SEN-CTRL-01:global"
        });
        let id1 = db.upsert_finding(&f).unwrap();
        let mut f2 = f.clone();
        f2["last_ts"] = json!("2026-07-02T00:00:00Z");
        let id2 = db.upsert_finding(&f2).unwrap();
        assert_eq!(id1, id2, "cùng dedupe_key phải cập nhật chứ không thêm mới");
        let all = db.findings(None, None, None, 100).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0]["last_ts"], "2026-07-02T00:00:00Z");
    }

    #[test]
    fn finding_status_rejects_unknown_value() {
        let db = Db::open_memory().unwrap();
        let f = json!({
            "rule_id": "R", "severity": "low", "score": 1, "title": "t", "detail": "",
            "actor": null, "first_ts": "t", "last_ts": "t", "evidence": [], "standards": [],
            "dedupe_key": "R:1"
        });
        let id = db.upsert_finding(&f).unwrap();
        assert!(db.set_finding_status(id, "khong-ton-tai", None).is_err());
        assert!(db.set_finding_status(id, "false_positive", Some("dev tự chạy")).is_ok());
        assert_eq!(db.finding(id).unwrap().unwrap()["status"], "false_positive");
    }

    #[test]
    fn suppression_requires_reason() {
        let db = Db::open_memory().unwrap();
        assert!(db.add_suppression("SEN-ANOM-01", &json!({}), "  ", None).is_err());
        assert!(db
            .add_suppression("SEN-ANOM-01", &json!({}), "làm đêm là bình thường", None)
            .is_ok());
    }

    #[test]
    fn expired_suppression_stops_applying() {
        let db = Db::open_memory().unwrap();
        db.add_suppression("R1", &json!({}), "tạm", Some("2000-01-01T00:00:00Z"))
            .unwrap();
        db.add_suppression("R2", &json!({}), "còn hạn", Some("2999-01-01T00:00:00Z"))
            .unwrap();
        let active: Vec<String> = db.active_suppressions().into_iter().map(|(r, _)| r).collect();
        assert_eq!(active, vec!["R2".to_string()]);
    }

    #[test]
    fn case_roundtrip_with_findings_and_notes() {
        let db = Db::open_memory().unwrap();
        let cid = db.create_case("Nghi vấn persistence", "", "high").unwrap();
        let f = json!({
            "rule_id": "SEN-PERSIST-02", "severity": "critical", "score": 90,
            "title": "lịch script", "detail": "", "actor": "main",
            "first_ts": "t", "last_ts": "t", "evidence": [], "standards": [],
            "dedupe_key": "SEN-PERSIST-02:abc"
        });
        let fid = db.upsert_finding(&f).unwrap();
        db.attach_finding_to_case(fid, cid).unwrap();
        db.add_case_note(cid, "user", "đã hỏi chủ máy").unwrap();
        let d = db.case_detail(cid).unwrap().unwrap();
        assert_eq!(d["findings"].as_array().unwrap().len(), 1);
        assert_eq!(d["notes"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn cursor_advances_and_accumulates() {
        let db = Db::open_memory().unwrap();
        assert_eq!(db.cursor("tool_executions"), "0");
        db.set_cursor("tool_executions", "120", 120, None);
        db.set_cursor("tool_executions", "140", 20, None);
        assert_eq!(db.cursor("tool_executions"), "140");
        let c = db.cursors().unwrap();
        assert_eq!(c[0]["copied"], 140);
    }
}
