//! Local settings store (SQLite): Dipper Hub connection profile.
//! One row of settings — base URL + credentials — so the app reconnects after restart.

use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HubSettings {
    /// Base URL of the Dipper Hub API gateway, e.g. "http://localhost:8080".
    pub base_url: String,
    pub username: String,
    pub password: String,
    /// Optional namespace/tenant id to scope device queries.
    #[serde(default)]
    pub namespace: String,
}

pub struct Store {
    conn: Mutex<Connection>,
}

fn db_path() -> PathBuf {
    if let Ok(dir) = std::env::var("HUB_DATA_DIR") {
        let p = PathBuf::from(dir);
        std::fs::create_dir_all(&p).ok();
        return p.join("hub.db");
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let dir = PathBuf::from(home).join(".senclaw").join("apps").join("hub");
    std::fs::create_dir_all(&dir).ok();
    dir.join("hub.db")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HtmlPanel {
    pub id: i64,
    pub name: String,
    pub html: String,
    pub updated_at: String,
}

impl Store {
    pub fn open() -> Result<Self> {
        let conn = Connection::open(db_path())?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS settings (
                 id INTEGER PRIMARY KEY CHECK (id = 1),
                 json TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS html_panels (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 name TEXT NOT NULL,
                 html TEXT NOT NULL,
                 updated_at TEXT NOT NULL DEFAULT (datetime('now'))
             );",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn load_settings(&self) -> Option<HubSettings> {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT json FROM settings WHERE id = 1", [], |row| {
            row.get::<_, String>(0)
        })
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
    }

    pub fn save_settings(&self, s: &HubSettings) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO settings (id, json) VALUES (1, ?1)
             ON CONFLICT(id) DO UPDATE SET json = excluded.json",
            [serde_json::to_string(s)?],
        )?;
        Ok(())
    }

    pub fn list_panels(&self) -> Result<Vec<HtmlPanel>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, name, html, updated_at FROM html_panels ORDER BY updated_at DESC")?;
        let rows = stmt
            .query_map([], |row| {
                Ok(HtmlPanel {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    html: row.get(2)?,
                    updated_at: row.get(3)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn save_panel(&self, id: Option<i64>, name: &str, html: &str) -> Result<HtmlPanel> {
        let conn = self.conn.lock().unwrap();
        let id = match id {
            Some(id) => {
                conn.execute(
                    "UPDATE html_panels SET name = ?1, html = ?2, updated_at = datetime('now') WHERE id = ?3",
                    rusqlite::params![name, html, id],
                )?;
                id
            }
            None => {
                conn.execute(
                    "INSERT INTO html_panels (name, html) VALUES (?1, ?2)",
                    rusqlite::params![name, html],
                )?;
                conn.last_insert_rowid()
            }
        };
        conn.query_row(
            "SELECT id, name, html, updated_at FROM html_panels WHERE id = ?1",
            [id],
            |row| {
                Ok(HtmlPanel {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    html: row.get(2)?,
                    updated_at: row.get(3)?,
                })
            },
        )
        .map_err(Into::into)
    }

    pub fn delete_panel(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM html_panels WHERE id = ?1", [id])?;
        Ok(())
    }
}
