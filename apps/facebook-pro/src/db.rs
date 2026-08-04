//! Local SQLite store for the Facebook Pro app. Facebook is the source of truth
//! for posts/comments/insights; we keep only *local* state:
//!   * `settings` — app_id / app_secret / user_token / active_page_id / autonomy / version (kv)
//!   * `pages`    — the admin's Pages + their Page Access Tokens
//!   * `drafts`   — the human-approval queue (draft-first posts/comments/replies)
//!   * `triggers` — rule → auto-reply/notify rules evaluated by the heartbeat
//!   * `seen_comments` — heartbeat dedup (comments already processed)
//!   * `activity` — a local log of what the app/engine did
//!
//! `app_secret` and tokens never leave this DB except on requests to
//! `graph.facebook.com`.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Db {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS pages (
  page_id      TEXT PRIMARY KEY,
  name         TEXT NOT NULL DEFAULT '',
  access_token TEXT NOT NULL DEFAULT '',
  category     TEXT NOT NULL DEFAULT '',
  updated_at   INTEGER NOT NULL DEFAULT 0
);
-- One row per queued write. `kind`: post | photo | comment | reply | edit.
CREATE TABLE IF NOT EXISTS drafts (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  kind        TEXT NOT NULL DEFAULT 'post',
  status      TEXT NOT NULL DEFAULT 'pending',
  page_id     TEXT NOT NULL DEFAULT '',
  target_id   TEXT NOT NULL DEFAULT '',
  message     TEXT NOT NULL DEFAULT '',
  link        TEXT NOT NULL DEFAULT '',
  image_url   TEXT NOT NULL DEFAULT '',
  source      TEXT NOT NULL DEFAULT 'user',
  model       TEXT NOT NULL DEFAULT '',
  result_id   TEXT NOT NULL DEFAULT '',
  error       TEXT NOT NULL DEFAULT '',
  created_at  INTEGER NOT NULL,
  decided_at  INTEGER
);
CREATE INDEX IF NOT EXISTS idx_drafts_status ON drafts(status);
CREATE TABLE IF NOT EXISTS triggers (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  name        TEXT NOT NULL DEFAULT '',
  page_id     TEXT NOT NULL DEFAULT '',
  event       TEXT NOT NULL DEFAULT 'new_comment',
  match_type  TEXT NOT NULL DEFAULT 'all',
  match_value TEXT NOT NULL DEFAULT '',
  action      TEXT NOT NULL DEFAULT 'draft_reply',
  reply_hint  TEXT NOT NULL DEFAULT '',
  enabled     INTEGER NOT NULL DEFAULT 1,
  created_at  INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS seen_comments (
  comment_id TEXT PRIMARY KEY,
  created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS activity (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  kind       TEXT NOT NULL,
  text       TEXT NOT NULL DEFAULT '',
  ref        TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL
);
"#;

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
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
                    .join("facebook-pro")
            });
        std::fs::create_dir_all(&dir).ok();
        Self::open(dir.join("facebook-pro.db"))
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

    // ---- settings (kv) ----

    pub fn get_setting(&self, key: &str) -> Option<String> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT value FROM settings WHERE key=?1",
            params![key],
            |r| r.get(0),
        )
        .optional()
        .ok()
        .flatten()
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO settings(key,value) VALUES(?1,?2)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn autonomy(&self) -> String {
        self.get_setting("autonomy")
            .unwrap_or_else(|| "draft".into())
    }

    pub fn version(&self) -> String {
        self.get_setting("version")
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| crate::fb::DEFAULT_VERSION.into())
    }

    pub fn active_page_id(&self) -> Option<String> {
        self.get_setting("active_page_id").filter(|s| !s.is_empty())
    }

    /// Non-secret settings for the UI (never returns `app_secret` or tokens).
    pub fn settings_public(&self) -> Value {
        json!({
            "app_id": self.get_setting("app_id").unwrap_or_default(),
            "version": self.version(),
            "autonomy": self.autonomy(),
            "active_page_id": self.active_page_id().unwrap_or_default(),
            "active_ad_account": self.get_setting("active_ad_account").unwrap_or_default(),
            "app_secret_set": self.get_setting("app_secret").map(|k| !k.is_empty()).unwrap_or(false),
            "user_token_set": self.get_setting("user_token").map(|k| !k.is_empty()).unwrap_or(false),
        })
    }

    // ---- pages ----

    pub fn save_page(&self, page_id: &str, name: &str, token: &str, category: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO pages(page_id,name,access_token,category,updated_at)
             VALUES(?1,?2,?3,?4,?5)
             ON CONFLICT(page_id) DO UPDATE SET
               name=excluded.name, access_token=excluded.access_token,
               category=excluded.category, updated_at=excluded.updated_at",
            params![page_id, name, token, category, now()],
        )?;
        Ok(())
    }

    pub fn page_token(&self, page_id: &str) -> Option<String> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT access_token FROM pages WHERE page_id=?1",
            params![page_id],
            |r| r.get(0),
        )
        .optional()
        .ok()
        .flatten()
    }

    pub fn list_pages(&self) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT page_id,name,category FROM pages ORDER BY name")
            .unwrap();
        let rows = stmt
            .query_map([], |r| {
                Ok(json!({
                    "page_id": r.get::<_, String>(0)?,
                    "name": r.get::<_, String>(1)?,
                    "category": r.get::<_, String>(2)?,
                }))
            })
            .unwrap();
        rows.filter_map(|r| r.ok()).collect()
    }

    // ---- drafts ----

    pub fn add_draft(&self, d: &DraftInput) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO drafts(kind,page_id,target_id,message,link,image_url,source,model,created_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![d.kind, d.page_id, d.target_id, d.message, d.link, d.image_url, d.source, d.model, now()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_draft(&self, id: i64) -> Option<Draft> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id,kind,status,page_id,target_id,message,link,image_url,source,model,result_id,error FROM drafts WHERE id=?1",
            params![id],
            Draft::from_row,
        )
        .optional()
        .ok()
        .flatten()
    }

    pub fn list_drafts(&self, status: &str) -> Vec<Draft> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id,kind,status,page_id,target_id,message,link,image_url,source,model,result_id,error FROM drafts WHERE status=?1 ORDER BY created_at DESC LIMIT 100")
            .unwrap();
        let rows = stmt.query_map(params![status], Draft::from_row).unwrap();
        rows.filter_map(|r| r.ok()).collect()
    }

    /// The set of `target_id`s that already have a pending draft — used by the
    /// heartbeat to avoid double-drafting a reply to the same comment.
    pub fn pending_targets(&self) -> std::collections::HashSet<String> {
        self.list_drafts("pending")
            .into_iter()
            .map(|d| d.target_id)
            .filter(|t| !t.is_empty())
            .collect()
    }

    pub fn decide_draft(&self, id: i64, status: &str, result_id: &str, error: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE drafts SET status=?2, result_id=?3, error=?4, decided_at=?5 WHERE id=?1",
            params![id, status, result_id, error, now()],
        )?;
        Ok(())
    }

    // ---- triggers ----

    pub fn add_trigger(&self, t: &TriggerInput) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO triggers(name,page_id,event,match_type,match_value,action,reply_hint,enabled,created_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![t.name, t.page_id, t.event, t.match_type, t.match_value, t.action, t.reply_hint, t.enabled as i64, now()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_triggers(&self, page_id: Option<&str>) -> Vec<Trigger> {
        let conn = self.conn.lock().unwrap();
        let (sql, page) = match page_id {
            Some(p) => ("SELECT id,name,page_id,event,match_type,match_value,action,reply_hint,enabled FROM triggers WHERE page_id=?1 OR page_id='' ORDER BY id", p.to_string()),
            None => ("SELECT id,name,page_id,event,match_type,match_value,action,reply_hint,enabled FROM triggers ORDER BY id", String::new()),
        };
        let mut stmt = conn.prepare(sql).unwrap();
        let map = |r: &rusqlite::Row| -> rusqlite::Result<Trigger> {
            Ok(Trigger {
                id: r.get(0)?,
                name: r.get(1)?,
                page_id: r.get(2)?,
                event: r.get(3)?,
                match_type: r.get(4)?,
                match_value: r.get(5)?,
                action: r.get(6)?,
                reply_hint: r.get(7)?,
                enabled: r.get::<_, i64>(8)? != 0,
            })
        };
        let rows = if page_id.is_some() {
            stmt.query_map(params![page], map)
                .unwrap()
                .filter_map(|r| r.ok())
                .collect()
        } else {
            stmt.query_map([], map)
                .unwrap()
                .filter_map(|r| r.ok())
                .collect()
        };
        rows
    }

    pub fn delete_trigger(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM triggers WHERE id=?1", params![id])?;
        Ok(())
    }

    // ---- seen comments (heartbeat dedup) ----

    pub fn is_comment_seen(&self, comment_id: &str) -> bool {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT 1 FROM seen_comments WHERE comment_id=?1",
            params![comment_id],
            |_| Ok(()),
        )
        .optional()
        .ok()
        .flatten()
        .is_some()
    }

    pub fn mark_comment_seen(&self, comment_id: &str) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT OR IGNORE INTO seen_comments(comment_id,created_at) VALUES(?1,?2)",
            params![comment_id, now()],
        );
    }

    // ---- activity ----

    pub fn log(&self, kind: &str, text: &str, r#ref: &str) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO activity(kind,text,ref,created_at) VALUES(?1,?2,?3,?4)",
            params![kind, text, r#ref, now()],
        );
    }

    pub fn recent_activity(&self, limit: i64) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT kind,text,ref,created_at FROM activity ORDER BY id DESC LIMIT ?1")
            .unwrap();
        let rows = stmt
            .query_map(params![limit], |r| {
                Ok(json!({
                    "kind": r.get::<_, String>(0)?,
                    "text": r.get::<_, String>(1)?,
                    "ref": r.get::<_, String>(2)?,
                    "created_at": r.get::<_, i64>(3)?,
                }))
            })
            .unwrap();
        rows.filter_map(|r| r.ok()).collect()
    }
}

/// Fields needed to enqueue a write. `target_id` is the object a comment/reply/
/// edit acts on (post id for a comment, comment id for a reply, post id for edit).
#[derive(Debug, Clone, Default)]
pub struct DraftInput {
    pub kind: String,
    pub page_id: String,
    pub target_id: String,
    pub message: String,
    pub link: String,
    pub image_url: String,
    pub source: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Draft {
    pub id: i64,
    pub kind: String,
    pub status: String,
    pub page_id: String,
    pub target_id: String,
    pub message: String,
    pub link: String,
    pub image_url: String,
    pub source: String,
    pub model: String,
    pub result_id: String,
    pub error: String,
}

impl Draft {
    fn from_row(r: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Self {
            id: r.get(0)?,
            kind: r.get(1)?,
            status: r.get(2)?,
            page_id: r.get(3)?,
            target_id: r.get(4)?,
            message: r.get(5)?,
            link: r.get(6)?,
            image_url: r.get(7)?,
            source: r.get(8)?,
            model: r.get(9)?,
            result_id: r.get(10)?,
            error: r.get(11)?,
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct TriggerInput {
    pub name: String,
    pub page_id: String,
    pub event: String,
    pub match_type: String,
    pub match_value: String,
    pub action: String,
    pub reply_hint: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Trigger {
    pub id: i64,
    pub name: String,
    pub page_id: String,
    pub event: String,
    pub match_type: String,
    pub match_value: String,
    pub action: String,
    pub reply_hint: String,
    pub enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_never_leak_secret() {
        let db = Db::open_memory().unwrap();
        db.set_setting("app_secret", "s3cr3t").unwrap();
        db.set_setting("user_token", "EAAtoken").unwrap();
        let pub_view = db.settings_public();
        assert_eq!(pub_view["app_secret_set"], json!(true));
        assert_eq!(pub_view["user_token_set"], json!(true));
        assert!(pub_view.get("app_secret").is_none());
        assert!(pub_view.get("user_token").is_none());
    }

    #[test]
    fn draft_lifecycle() {
        let db = Db::open_memory().unwrap();
        let id = db
            .add_draft(&DraftInput {
                kind: "reply".into(),
                page_id: "P1".into(),
                target_id: "C1".into(),
                message: "Cảm ơn anh/chị ạ".into(),
                source: "heartbeat".into(),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(db.list_drafts("pending").len(), 1);
        assert!(db.pending_targets().contains("C1"));
        db.decide_draft(id, "published", "P1_777", "").unwrap();
        assert_eq!(db.list_drafts("pending").len(), 0);
        let d = db.get_draft(id).unwrap();
        assert_eq!(d.status, "published");
        assert_eq!(d.result_id, "P1_777");
    }

    #[test]
    fn seen_comment_dedup() {
        let db = Db::open_memory().unwrap();
        assert!(!db.is_comment_seen("C9"));
        db.mark_comment_seen("C9");
        assert!(db.is_comment_seen("C9"));
    }

    #[test]
    fn trigger_page_scoping() {
        let db = Db::open_memory().unwrap();
        db.add_trigger(&TriggerInput {
            name: "global".into(),
            page_id: "".into(),
            match_type: "all".into(),
            action: "notify".into(),
            enabled: true,
            event: "new_comment".into(),
            ..Default::default()
        })
        .unwrap();
        db.add_trigger(&TriggerInput {
            name: "p2".into(),
            page_id: "P2".into(),
            match_type: "keyword".into(),
            match_value: "giá".into(),
            action: "draft_reply".into(),
            enabled: true,
            event: "new_comment".into(),
            ..Default::default()
        })
        .unwrap();
        // Page P1 sees only the global trigger.
        assert_eq!(db.list_triggers(Some("P1")).len(), 1);
        // Page P2 sees the global + its own.
        assert_eq!(db.list_triggers(Some("P2")).len(), 2);
    }
}
