//! Local SQLite store. Google is the source of truth for mail/events/files —
//! we keep only what must survive a restart:
//!   * `settings`  — kv (client_id, client_secret, days, services, tokens JSON)
//!   * `sync_runs` — local activity log shown in the UI
//!
//! `client_secret` and `tokens` never leave this DB except on requests to
//! Google endpoints; the REST/MCP surfaces always return them masked.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
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
CREATE TABLE IF NOT EXISTS sync_runs (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  service    TEXT NOT NULL,
  status     TEXT NOT NULL,
  detail     TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL
);
"#;

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// OAuth token bundle as stored in the `tokens` setting.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Tokens {
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: String,
    /// Unix seconds when `access_token` expires; 0 = unknown.
    #[serde(default)]
    pub expires_at: i64,
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
                    .join("google-workspace")
            });
        std::fs::create_dir_all(&dir).ok();
        Self::open(dir.join("gworkspace.db"))
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

    pub fn delete_setting(&self, key: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM settings WHERE key=?1", params![key])?;
        Ok(())
    }

    // ---- typed settings ----

    pub fn client_id(&self) -> String {
        self.get_setting("client_id").unwrap_or_default()
    }

    pub fn client_secret(&self) -> String {
        self.get_setting("client_secret").unwrap_or_default()
    }

    pub fn days(&self) -> u32 {
        self.get_setting("days")
            .and_then(|v| v.parse().ok())
            .unwrap_or(7)
    }

    pub fn services(&self) -> Vec<String> {
        self.get_setting("services")
            .and_then(|v| serde_json::from_str::<Vec<String>>(&v).ok())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| vec!["gmail".into(), "calendar".into(), "drive".into()])
    }

    pub fn tokens(&self) -> Tokens {
        self.get_setting("tokens")
            .and_then(|v| serde_json::from_str(&v).ok())
            .unwrap_or_default()
    }

    pub fn save_tokens(&self, tokens: &Tokens) -> Result<()> {
        self.set_setting("tokens", &serde_json::to_string(tokens)?)
    }

    pub fn clear_tokens(&self) -> Result<()> {
        self.delete_setting("tokens")
    }

    pub fn connected(&self) -> bool {
        !self.tokens().access_token.is_empty()
    }

    /// Settings snapshot with secrets masked — the only shape REST/MCP return.
    pub fn masked_settings(&self) -> Value {
        let tokens = self.tokens();
        json!({
            "clientId": self.client_id(),
            "clientSecret": if self.client_secret().is_empty() { "" } else { "***" },
            "days": self.days(),
            "services": self.services(),
            "connected": !tokens.access_token.is_empty(),
            "hasRefreshToken": !tokens.refresh_token.is_empty(),
            "tokenExpiresAt": tokens.expires_at,
        })
    }

    // ---- sync runs ----

    pub fn add_run(&self, service: &str, status: &str, detail: &str) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO sync_runs(service,status,detail,created_at) VALUES(?1,?2,?3,?4)",
            params![service, status, detail, now()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn recent_runs(&self, limit: u32) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT id, service, status, detail, created_at
             FROM sync_runs ORDER BY id DESC LIMIT ?1",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        stmt.query_map(params![limit], |r| {
            Ok(json!({
                "id": r.get::<_, i64>(0)?,
                "service": r.get::<_, String>(1)?,
                "status": r.get::<_, String>(2)?,
                "detail": r.get::<_, String>(3)?,
                "created_at": r.get::<_, i64>(4)?,
            }))
        })
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
    }

    pub fn last_run(&self) -> Option<Value> {
        self.recent_runs(1).into_iter().next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_roundtrip_and_defaults() {
        let db = Db::open_memory().unwrap();
        assert_eq!(db.days(), 7);
        assert_eq!(db.services(), vec!["gmail", "calendar", "drive"]);
        assert!(!db.connected());

        db.set_setting("client_id", "abc.apps.googleusercontent.com")
            .unwrap();
        db.set_setting("days", "30").unwrap();
        assert_eq!(db.client_id(), "abc.apps.googleusercontent.com");
        assert_eq!(db.days(), 30);
    }

    #[test]
    fn tokens_roundtrip_and_masking() {
        let db = Db::open_memory().unwrap();
        db.set_setting("client_secret", "s3cret").unwrap();
        db.save_tokens(&Tokens {
            access_token: "ya29.x".into(),
            refresh_token: "1//r".into(),
            expires_at: 123,
        })
        .unwrap();

        assert!(db.connected());
        let masked = db.masked_settings();
        assert_eq!(masked["clientSecret"], "***");
        assert_eq!(masked["connected"], true);
        assert_eq!(masked["hasRefreshToken"], true);
        // Raw token strings must never appear in the masked payload.
        assert!(!masked.to_string().contains("ya29"));
        assert!(!masked.to_string().contains("s3cret"));

        db.clear_tokens().unwrap();
        assert!(!db.connected());
    }

    #[test]
    fn sync_runs_log() {
        let db = Db::open_memory().unwrap();
        db.add_run("gmail", "completed", "20 emails").unwrap();
        db.add_run("calendar", "error", "no token").unwrap();
        let runs = db.recent_runs(10);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0]["service"], "calendar"); // newest first
        assert_eq!(db.last_run().unwrap()["status"], "error");
    }
}
