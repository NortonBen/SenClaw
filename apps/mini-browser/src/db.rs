//! SQLite store for the mini browser: visit `history` and `bookmarks`.

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub struct Db {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS history (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  url        TEXT NOT NULL,
  title      TEXT NOT NULL DEFAULT '',
  visited_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_history_time ON history(visited_at DESC);
CREATE TABLE IF NOT EXISTS bookmarks (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  url        TEXT NOT NULL UNIQUE,
  title      TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL
);
"#;

#[derive(Serialize)]
pub struct Row {
    pub id: i64,
    pub url: String,
    pub title: String,
    pub at: i64,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Db { conn: Mutex::new(conn) })
    }

    pub fn add_history(&self, url: &str, title: &str, at: i64) -> Result<()> {
        if url.is_empty() || url == "about:blank" {
            return Ok(());
        }
        let conn = self.conn.lock().unwrap();
        // Skip if it's the same as the most recent entry.
        let last: Option<String> = conn
            .query_row("SELECT url FROM history ORDER BY id DESC LIMIT 1", [], |r| r.get(0))
            .ok();
        if last.as_deref() == Some(url) {
            conn.execute("UPDATE history SET title=?1, visited_at=?2 WHERE url=?1 AND id=(SELECT MAX(id) FROM history)", params![title, at]).ok();
            return Ok(());
        }
        conn.execute(
            "INSERT INTO history (url, title, visited_at) VALUES (?1, ?2, ?3)",
            params![url, title, at],
        )?;
        Ok(())
    }

    pub fn recent_history(&self, limit: i64) -> Result<Vec<Row>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, url, title, visited_at FROM history ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map([limit], |r| {
                Ok(Row { id: r.get(0)?, url: r.get(1)?, title: r.get(2)?, at: r.get(3)? })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    pub fn add_bookmark(&self, url: &str, title: &str, at: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO bookmarks (url, title, created_at) VALUES (?1, ?2, ?3)",
            params![url, title, at],
        )?;
        Ok(())
    }

    pub fn remove_bookmark(&self, url: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM bookmarks WHERE url=?1", params![url])?;
        Ok(())
    }

    pub fn list_bookmarks(&self) -> Result<Vec<Row>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, url, title, created_at FROM bookmarks ORDER BY id DESC")?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Row { id: r.get(0)?, url: r.get(1)?, title: r.get(2)?, at: r.get(3)? })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }
}

/// Per-app data dir, e.g. `~/.senclaw/space-apps/mini-browser/`.
pub fn default_data_dir(app: &str) -> PathBuf {
    let base = std::env::var("SENCLAW_DATA_DIR").map(PathBuf::from).unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        PathBuf::from(home).join(".senclaw")
    });
    base.join("space-apps").join(app)
}
