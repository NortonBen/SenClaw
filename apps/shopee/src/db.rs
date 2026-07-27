//! Local SQLite store for the Shopee app. Holds only *local* state — Shopee is
//! the source of truth for orders/chat. We keep:
//!   * `settings` — partner_id / partner_key / shop_id / host + autonomy (kv, JSON)
//!   * `tokens`   — the current access/refresh token + expiry for a shop
//!   * `drafts`   — the human-approval queue (draft-first CSKH replies)
//!   * `activity` — a local log of what the app/engine did
//!
//! `partner_key` and tokens never leave this DB except on requests to the
//! configured Shopee host.

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
-- One row per authorized shop. `shop_id` 0 is the "not connected" placeholder.
CREATE TABLE IF NOT EXISTS tokens (
  shop_id       INTEGER PRIMARY KEY,
  access_token  TEXT NOT NULL DEFAULT '',
  refresh_token TEXT NOT NULL DEFAULT '',
  expires_at    INTEGER NOT NULL DEFAULT 0,
  updated_at    INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS drafts (
  id              INTEGER PRIMARY KEY AUTOINCREMENT,
  kind            TEXT NOT NULL DEFAULT 'chat_reply',
  status          TEXT NOT NULL DEFAULT 'pending',
  conversation_id TEXT NOT NULL DEFAULT '',
  to_id           INTEGER NOT NULL DEFAULT 0,
  to_name         TEXT NOT NULL DEFAULT '',
  content         TEXT NOT NULL DEFAULT '',
  source          TEXT NOT NULL DEFAULT 'user',
  model           TEXT NOT NULL DEFAULT '',
  error           TEXT NOT NULL DEFAULT '',
  created_at      INTEGER NOT NULL,
  decided_at      INTEGER
);
CREATE INDEX IF NOT EXISTS idx_drafts_status ON drafts(status);
CREATE TABLE IF NOT EXISTS activity (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  kind       TEXT NOT NULL,
  text       TEXT NOT NULL DEFAULT '',
  ref        TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL
);
"#;

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

impl Db {
    pub fn open_default() -> Result<Self> {
        let dir = std::env::var("SENCLAW_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                PathBuf::from(home).join(".senclaw").join("apps").join("shopee")
            });
        std::fs::create_dir_all(&dir).ok();
        Self::open(dir.join("shopee.db"))
    }

    pub fn open(path: PathBuf) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    #[cfg(test)]
    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    // ---- settings (kv) ----

    pub fn get_setting(&self, key: &str) -> Option<String> {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT value FROM settings WHERE key=?1", params![key], |r| r.get(0))
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

    /// Non-secret settings for the UI (never returns `partner_key`).
    pub fn settings_public(&self) -> Value {
        json!({
            "partner_id": self.get_setting("partner_id").unwrap_or_default(),
            "shop_id": self.get_setting("shop_id").unwrap_or_default(),
            "host": self.get_setting("host").unwrap_or_default(),
            "autonomy": self.get_setting("autonomy").unwrap_or_else(|| "draft".into()),
            "partner_key_set": self.get_setting("partner_key").map(|k| !k.is_empty()).unwrap_or(false),
        })
    }

    // ---- tokens ----

    pub fn save_token(&self, shop_id: i64, access: &str, refresh: &str, expire_in: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let expires_at = now() + expire_in;
        conn.execute(
            "INSERT INTO tokens(shop_id,access_token,refresh_token,expires_at,updated_at)
             VALUES(?1,?2,?3,?4,?5)
             ON CONFLICT(shop_id) DO UPDATE SET
               access_token=excluded.access_token,
               refresh_token=excluded.refresh_token,
               expires_at=excluded.expires_at,
               updated_at=excluded.updated_at",
            params![shop_id, access, refresh, expires_at, now()],
        )?;
        Ok(())
    }

    pub fn get_token(&self, shop_id: i64) -> Option<Token> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT shop_id,access_token,refresh_token,expires_at FROM tokens WHERE shop_id=?1",
            params![shop_id],
            |r| Ok(Token {
                shop_id: r.get(0)?,
                access_token: r.get(1)?,
                refresh_token: r.get(2)?,
                expires_at: r.get(3)?,
            }),
        )
        .optional()
        .ok()
        .flatten()
    }

    // ---- drafts ----

    pub fn add_draft(&self, conversation_id: &str, to_id: i64, to_name: &str, content: &str, source: &str, model: &str) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO drafts(kind,conversation_id,to_id,to_name,content,source,model,created_at)
             VALUES('chat_reply',?1,?2,?3,?4,?5,?6,?7)",
            params![conversation_id, to_id, to_name, content, source, model, now()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_draft(&self, id: i64) -> Option<Draft> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id,status,conversation_id,to_id,to_name,content,source,model,error FROM drafts WHERE id=?1",
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
            .prepare("SELECT id,status,conversation_id,to_id,to_name,content,source,model,error FROM drafts WHERE status=?1 ORDER BY created_at DESC LIMIT 100")
            .unwrap();
        let rows = stmt.query_map(params![status], Draft::from_row).unwrap();
        rows.filter_map(|r| r.ok()).collect()
    }

    pub fn decide_draft(&self, id: i64, status: &str, error: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE drafts SET status=?2, error=?3, decided_at=?4 WHERE id=?1",
            params![id, status, error, now()],
        )?;
        Ok(())
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

#[derive(Debug, Clone, Serialize)]
pub struct Token {
    pub shop_id: i64,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
}

impl Token {
    /// True when the access token is expired or within 5 minutes of it.
    pub fn is_stale(&self) -> bool {
        now() >= self.expires_at - 300
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Draft {
    pub id: i64,
    pub status: String,
    pub conversation_id: String,
    pub to_id: i64,
    pub to_name: String,
    pub content: String,
    pub source: String,
    pub model: String,
    pub error: String,
}

impl Draft {
    fn from_row(r: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Self {
            id: r.get(0)?,
            status: r.get(1)?,
            conversation_id: r.get(2)?,
            to_id: r.get(3)?,
            to_name: r.get(4)?,
            content: r.get(5)?,
            source: r.get(6)?,
            model: r.get(7)?,
            error: r.get(8)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_never_leak_partner_key() {
        let db = Db::open_memory().unwrap();
        db.set_setting("partner_key", "shpk_secret").unwrap();
        let pub_view = db.settings_public();
        assert_eq!(pub_view["partner_key_set"], json!(true));
        assert!(pub_view.get("partner_key").is_none());
    }

    #[test]
    fn draft_lifecycle() {
        let db = Db::open_memory().unwrap();
        let id = db.add_draft("conv1", 42, "Khách A", "Dạ em gửi anh ạ", "user", "").unwrap();
        assert_eq!(db.list_drafts("pending").len(), 1);
        db.decide_draft(id, "approved", "").unwrap();
        assert_eq!(db.list_drafts("pending").len(), 0);
        assert_eq!(db.get_draft(id).unwrap().status, "approved");
    }

    #[test]
    fn token_staleness() {
        let db = Db::open_memory().unwrap();
        db.save_token(1, "a", "r", 14400).unwrap();
        assert!(!db.get_token(1).unwrap().is_stale());
        db.save_token(2, "a", "r", 60).unwrap(); // within the 5-min window
        assert!(db.get_token(2).unwrap().is_stale());
    }
}
