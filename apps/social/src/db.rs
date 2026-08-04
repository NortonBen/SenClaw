//! SQLite access — a single serialized connection behind a mutex.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

const SCHEMA: &str = include_str!("schema.sql");

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Monotonic-ish opaque id for extension RPC correlation. Timestamp nanos + a
/// process-wide counter — unique within a run without pulling in a uuid crate.
pub fn new_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let t = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);
    format!("{t:x}-{n:x}")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: i64,
    pub platform: String,
    pub handle: String,
    pub display_name: String,
    #[serde(default)]
    pub official_config: Value,
    pub session_present: bool,
    pub token_expiry: String,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

fn account_from_row(r: &Row) -> rusqlite::Result<Account> {
    let cfg_txt: String = r.get("official_config")?;
    Ok(Account {
        id: r.get("id")?,
        platform: r.get("platform")?,
        handle: r.get("handle")?,
        display_name: r.get("display_name")?,
        official_config: serde_json::from_str(&cfg_txt)
            .unwrap_or(Value::Object(Default::default())),
        session_present: r.get::<_, i64>("session_present")? != 0,
        token_expiry: r.get("token_expiry")?,
        enabled: r.get::<_, i64>("enabled")? != 0,
        created_at: r.get("created_at")?,
        updated_at: r.get("updated_at")?,
    })
}

fn has_column(conn: &Connection, table: &str, column: &str) -> rusqlite::Result<bool> {
    let mut st = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let cols = st.query_map([], |r| r.get::<_, String>(1))?;
    for c in cols {
        if c? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Additive migrations for databases created before a column existed.
fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    if !has_column(conn, "inbox", "sender")? {
        conn.execute(
            "ALTER TABLE inbox ADD COLUMN sender TEXT NOT NULL DEFAULT ''",
            [],
        )?;
    }
    if !has_column(conn, "drafts", "media")? {
        conn.execute(
            "ALTER TABLE drafts ADD COLUMN media TEXT NOT NULL DEFAULT '[]'",
            [],
        )?;
    }
    Ok(())
}

fn draft_row(r: &Row) -> rusqlite::Result<Value> {
    Ok(serde_json::json!({
        "id": r.get::<_, i64>(0)?,
        "platform": r.get::<_, String>(1)?,
        "handle": r.get::<_, String>(2)?,
        "kind": r.get::<_, String>(3)?,
        "text": r.get::<_, String>(4)?,
        "thread_id": r.get::<_, String>(5)?,
        "status": r.get::<_, String>(6)?,
        "ref_id": r.get::<_, String>(7)?,
        "detail": r.get::<_, String>(8)?,
        "media": serde_json::from_str::<Value>(&r.get::<_, String>(9)?).unwrap_or_else(|_| serde_json::json!([])),
        "created_at": r.get::<_, String>(10)?,
    }))
}

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    pub fn open(path: &str) -> Result<Self> {
        Self::init(Connection::open(path)?)
    }

    #[allow(dead_code)] // used by unit tests
    pub fn open_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "busy_timeout", 5000)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA)?;
        migrate(&conn)?;
        Ok(Db {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> Result<T> {
        let conn = self.conn.lock().unwrap();
        Ok(f(&conn)?)
    }

    // ---- settings ----

    #[allow(dead_code)] // settings read path, kept for symmetry with set_setting
    pub fn setting(&self, key: &str, default: &str) -> String {
        self.with_conn(|c| {
            c.query_row(
                "SELECT value FROM app_settings WHERE key = ?1",
                params![key],
                |r| r.get::<_, String>(0),
            )
            .optional()
        })
        .ok()
        .flatten()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| default.to_string())
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
        })?;
        Ok(())
    }

    pub fn all_settings(&self) -> Result<Vec<(String, String)>> {
        self.with_conn(|c| {
            let mut st = c.prepare("SELECT key, value FROM app_settings ORDER BY key")?;
            let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect()
        })
    }

    // ---- accounts ----

    pub fn list_accounts(&self) -> Result<Vec<Account>> {
        self.with_conn(|c| {
            let mut st = c.prepare("SELECT * FROM accounts ORDER BY platform, handle")?;
            let rows = st.query_map([], account_from_row)?;
            rows.collect()
        })
    }

    #[allow(dead_code)] // single-account lookup, used by future REST detail route
    pub fn get_account(&self, id: i64) -> Result<Option<Account>> {
        self.with_conn(|c| {
            c.query_row(
                "SELECT * FROM accounts WHERE id = ?1",
                params![id],
                account_from_row,
            )
            .optional()
        })
    }

    pub fn upsert_account(
        &self,
        platform: &str,
        handle: &str,
        display_name: &str,
        official_config: &Value,
    ) -> Result<i64> {
        let cfg = serde_json::to_string(official_config).unwrap_or_else(|_| "{}".into());
        let ts = now();
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO accounts (platform, handle, display_name, official_config, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5)
                 ON CONFLICT(platform, handle) DO UPDATE SET
                   display_name = excluded.display_name,
                   official_config = excluded.official_config,
                   updated_at = excluded.updated_at",
                params![platform, handle, display_name, cfg, ts],
            )?;
            Ok(c.last_insert_rowid())
        })
    }

    pub fn delete_account(&self, id: i64) -> Result<()> {
        self.with_conn(|c| c.execute("DELETE FROM accounts WHERE id = ?1", params![id]))?;
        Ok(())
    }

    /// The official_config JSON stored for (platform, handle), or `{}`.
    pub fn official_config(&self, platform: &str, handle: &str) -> Value {
        self.list_accounts()
            .unwrap_or_default()
            .into_iter()
            .find(|a| a.platform == platform && a.handle == handle)
            .map(|a| a.official_config)
            .unwrap_or(Value::Object(Default::default()))
    }

    pub fn account_count(&self) -> i64 {
        self.with_conn(|c| c.query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get(0)))
            .unwrap_or(0)
    }

    // ---- logs ----

    pub fn log_action(&self, platform: &str, action: &str, status: &str, detail: &str) {
        let _ = self.with_conn(|c| {
            c.execute(
                "INSERT INTO action_log (platform, action, status, detail, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![platform, action, status, detail, now()],
            )
        });
    }

    pub fn recent_actions(&self, limit: i64) -> Result<Vec<Value>> {
        self.with_conn(|c| {
            let mut st = c.prepare(
                "SELECT platform, action, status, detail, created_at
                 FROM action_log ORDER BY id DESC LIMIT ?1",
            )?;
            let rows = st.query_map(params![limit], |r| {
                Ok(serde_json::json!({
                    "platform": r.get::<_, String>(0)?,
                    "action": r.get::<_, String>(1)?,
                    "status": r.get::<_, String>(2)?,
                    "detail": r.get::<_, String>(3)?,
                    "created_at": r.get::<_, String>(4)?,
                }))
            })?;
            rows.collect()
        })
    }

    pub fn log_session_event(&self, platform: &str, event: &str) {
        let _ = self.with_conn(|c| {
            c.execute(
                "INSERT INTO session_log (platform, event, created_at) VALUES (?1, ?2, ?3)",
                params![platform, event, now()],
            )
        });
    }

    pub fn recent_sessions(&self, limit: i64) -> Result<Vec<Value>> {
        self.with_conn(|c| {
            let mut st = c.prepare(
                "SELECT platform, event, created_at FROM session_log ORDER BY id DESC LIMIT ?1",
            )?;
            let rows = st.query_map(params![limit], |r| {
                Ok(serde_json::json!({
                    "platform": r.get::<_, String>(0)?,
                    "event": r.get::<_, String>(1)?,
                    "created_at": r.get::<_, String>(2)?,
                }))
            })?;
            rows.collect()
        })
    }

    pub fn log_post(&self, platform: &str, kind: &str, ref_id: &str, status: &str, detail: &str) {
        let _ = self.with_conn(|c| {
            c.execute(
                "INSERT INTO post_log (platform, kind, ref_id, status, detail, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![platform, kind, ref_id, status, detail, now()],
            )
        });
    }

    pub fn recent_posts(&self, limit: i64) -> Result<Vec<Value>> {
        self.with_conn(|c| {
            let mut st = c.prepare(
                "SELECT platform, kind, ref_id, status, detail, created_at
                 FROM post_log ORDER BY id DESC LIMIT ?1",
            )?;
            let rows = st.query_map(params![limit], |r| {
                Ok(serde_json::json!({
                    "platform": r.get::<_, String>(0)?,
                    "kind": r.get::<_, String>(1)?,
                    "ref_id": r.get::<_, String>(2)?,
                    "status": r.get::<_, String>(3)?,
                    "detail": r.get::<_, String>(4)?,
                    "created_at": r.get::<_, String>(5)?,
                }))
            })?;
            rows.collect()
        })
    }

    /// Persist one message. `external_id` is the platform thread/chat id (the
    /// reply target); `sender` is the counterpart's display name (inbound only).
    /// Returns the new row id.
    pub fn insert_inbox(
        &self,
        platform: &str,
        external_id: &str,
        sender: &str,
        direction: &str,
        text: &str,
    ) -> Result<i64> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO inbox (platform, thread_id, external_id, sender, direction, text, created_at)
                 VALUES (?1, ?2, ?2, ?3, ?4, ?5, ?6)",
                params![platform, external_id, sender, direction, text, now()],
            )?;
            Ok(c.last_insert_rowid())
        })
    }

    // ---- autonomy / drafts ----

    /// Autonomy mode: observe | draft | live. Default "draft" (safest).
    pub fn autonomy(&self) -> String {
        let m = self.setting("autonomy", "draft");
        match m.as_str() {
            "observe" | "draft" | "live" => m,
            _ => "draft".to_string(),
        }
    }

    pub fn create_draft(
        &self,
        platform: &str,
        handle: &str,
        kind: &str,
        text: &str,
        thread_id: &str,
        media: &Value,
    ) -> Result<i64> {
        let media_txt = serde_json::to_string(media).unwrap_or_else(|_| "[]".into());
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO drafts (platform, handle, kind, text, thread_id, media, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![platform, handle, kind, text, thread_id, media_txt, now()],
            )?;
            Ok(c.last_insert_rowid())
        })
    }

    pub fn get_draft(&self, id: i64) -> Result<Option<Value>> {
        self.with_conn(|c| {
            c.query_row(
                "SELECT id, platform, handle, kind, text, thread_id, status, ref_id, detail, media, created_at
                 FROM drafts WHERE id = ?1",
                params![id],
                draft_row,
            )
            .optional()
        })
    }

    pub fn list_drafts(&self, status: Option<&str>, limit: i64) -> Result<Vec<Value>> {
        self.with_conn(|c| {
            let mut st = c.prepare(
                "SELECT id, platform, handle, kind, text, thread_id, status, ref_id, detail, media, created_at
                 FROM drafts WHERE (?1 IS NULL OR status = ?1) ORDER BY id DESC LIMIT ?2",
            )?;
            let rows = st.query_map(params![status, limit], draft_row)?;
            rows.collect()
        })
    }

    pub fn set_draft_status(
        &self,
        id: i64,
        status: &str,
        ref_id: &str,
        detail: &str,
    ) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "UPDATE drafts SET status = ?2, ref_id = ?3, detail = ?4, decided_at = ?5 WHERE id = ?1",
                params![id, status, ref_id, detail, now()],
            )
        })?;
        Ok(())
    }

    fn inbox_row(r: &Row) -> rusqlite::Result<Value> {
        Ok(serde_json::json!({
            "id": r.get::<_, i64>(0)?,
            "platform": r.get::<_, String>(1)?,
            "external_id": r.get::<_, String>(2)?,
            "sender": r.get::<_, String>(3)?,
            "direction": r.get::<_, String>(4)?,
            "text": r.get::<_, String>(5)?,
            "created_at": r.get::<_, String>(6)?,
        }))
    }

    pub fn list_inbox(&self, platform: Option<&str>, limit: i64) -> Result<Vec<Value>> {
        self.with_conn(|c| {
            let mut st = c.prepare(
                "SELECT id, platform, external_id, sender, direction, text, created_at FROM inbox
                 WHERE (?1 IS NULL OR platform = ?1) ORDER BY id DESC LIMIT ?2",
            )?;
            let rows = st.query_map(params![platform, limit], Self::inbox_row)?;
            rows.collect()
        })
    }

    /// Cheap dedup: has this exact inbound already been stored? Keyed on
    /// (platform, external_id, text) since the extension gives no per-message id
    /// — good enough to make repeated polls idempotent for distinct messages.
    pub fn inbox_contains(&self, platform: &str, external_id: &str, text: &str) -> bool {
        self.with_conn(|c| {
            c.query_row(
                "SELECT 1 FROM inbox WHERE platform=?1 AND external_id=?2 AND text=?3 AND direction='in' LIMIT 1",
                params![platform, external_id, text],
                |_| Ok(()),
            )
            .optional()
        })
        .map(|o| o.is_some())
        .unwrap_or(false)
    }

    /// Cursor feed for external pullers (e.g. CRM): inbound messages with
    /// `id > since`, oldest first, so the caller advances its cursor by the last
    /// `id` it saw. Only `direction='in'` — operators pull what came IN.
    pub fn inbox_since(&self, since: i64, limit: i64) -> Result<Vec<Value>> {
        self.with_conn(|c| {
            let mut st = c.prepare(
                "SELECT id, platform, external_id, sender, direction, text, created_at FROM inbox
                 WHERE id > ?1 AND direction = 'in' ORDER BY id ASC LIMIT ?2",
            )?;
            let rows = st.query_map(params![since, limit], Self::inbox_row)?;
            rows.collect()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn accounts_upsert_and_list() {
        let db = Db::open_memory().unwrap();
        let id = db
            .upsert_account("tiktok", "@shop", "My Shop", &json!({"app_key": "K"}))
            .unwrap();
        assert!(id > 0);
        // Upsert on the same (platform, handle) updates, not duplicates.
        db.upsert_account("tiktok", "@shop", "Renamed", &json!({}))
            .unwrap();
        let all = db.list_accounts().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].display_name, "Renamed");
    }

    #[test]
    fn draft_persists_and_returns_media() {
        let db = Db::open_memory().unwrap();
        let media = json!(["data:image/png;base64,AAAA", "data:image/png;base64,BBBB"]);
        let id = db
            .create_draft("facebook", "bacnd.120", "post", "hi", "", &media)
            .unwrap();
        let d = db.get_draft(id).unwrap().unwrap();
        assert_eq!(d["media"], media);
        // …and via the list path too.
        let listed = db.list_drafts(Some("pending"), 10).unwrap();
        assert_eq!(listed[0]["media"], media);
        // A media-less draft comes back as an empty array, not null.
        let id2 = db
            .create_draft("x", "@me", "post", "hi", "", &json!([]))
            .unwrap();
        assert_eq!(db.get_draft(id2).unwrap().unwrap()["media"], json!([]));
    }

    #[test]
    fn settings_roundtrip_with_default() {
        let db = Db::open_memory().unwrap();
        assert_eq!(db.setting("k", "fallback"), "fallback");
        db.set_setting("k", "v").unwrap();
        assert_eq!(db.setting("k", "fallback"), "v");
    }

    #[test]
    fn new_id_is_unique() {
        let a = new_id();
        let b = new_id();
        assert_ne!(a, b);
    }

    #[test]
    fn post_log_records_and_reads_back_newest_first() {
        let db = Db::open_memory().unwrap();
        db.log_post("facebook", "post", "111", "ok", "hi");
        db.log_post("x", "post", "222", "ok", "yo");
        let posts = db.recent_posts(10).unwrap();
        assert_eq!(posts.len(), 2);
        assert_eq!(posts[0]["ref_id"], "222"); // newest first
    }

    #[test]
    fn action_and_session_logs_read_back_newest_first() {
        let db = Db::open_memory().unwrap();
        db.log_action("tiktok", "search", "reserved", "q");
        db.log_action("tiktok", "search", "blocked", "hạn mức");
        let acts = db.recent_actions(10).unwrap();
        assert_eq!(acts.len(), 2);
        assert_eq!(acts[0]["status"], "blocked");

        db.log_session_event("facebook", "online");
        db.log_session_event("facebook", "offline");
        let sess = db.recent_sessions(10).unwrap();
        assert_eq!(sess.len(), 2);
        assert_eq!(sess[0]["event"], "offline");
    }

    #[test]
    fn inbox_stores_and_filters_by_platform() {
        let db = Db::open_memory().unwrap();
        db.insert_inbox("facebook", "t1", "Khách A", "in", "khách hỏi giá")
            .unwrap();
        db.insert_inbox("tiktok", "t2", "", "out", "đã trả lời")
            .unwrap();
        assert_eq!(db.list_inbox(None, 10).unwrap().len(), 2);
        let fb = db.list_inbox(Some("facebook"), 10).unwrap();
        assert_eq!(fb.len(), 1);
        assert_eq!(fb[0]["direction"], "in");
        assert_eq!(fb[0]["sender"], "Khách A");
    }

    #[test]
    fn inbox_since_is_an_inbound_only_ascending_cursor_feed() {
        let db = Db::open_memory().unwrap();
        let a = db.insert_inbox("x", "t1", "Alice", "in", "m1").unwrap();
        db.insert_inbox("x", "t1", "", "out", "reply").unwrap(); // outbound excluded
        let c = db.insert_inbox("x", "t2", "Bob", "in", "m2").unwrap();

        let all = db.inbox_since(0, 100).unwrap();
        assert_eq!(all.len(), 2, "only inbound rows");
        assert_eq!(all[0]["id"], a); // ascending
        assert_eq!(all[1]["id"], c);

        // Advancing the cursor past the first inbound yields only the later one.
        let after = db.inbox_since(a, 100).unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0]["text"], "m2");
    }

    #[test]
    fn inbox_contains_dedups_inbound() {
        let db = Db::open_memory().unwrap();
        db.insert_inbox("tiktok", "t9", "X", "in", "chào").unwrap();
        assert!(db.inbox_contains("tiktok", "t9", "chào"));
        assert!(!db.inbox_contains("tiktok", "t9", "khác"));
        assert!(!db.inbox_contains("x", "t9", "chào"));
    }
}
