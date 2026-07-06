use anyhow::Result;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Tiny SQLite store for the IDE: a key/value `meta` table (current workspace
/// root, settings) and a `recents` table (recently opened folders).
pub struct Db {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS recents (
  path      TEXT PRIMARY KEY,
  name      TEXT NOT NULL,
  opened_at INTEGER NOT NULL
);
"#;

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    pub fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let conn = self.conn.lock().unwrap();
        f(&conn)
    }

    pub fn get_meta(&self, key: &str) -> Result<Option<String>> {
        self.with_conn(|c| {
            let v = c
                .query_row("SELECT value FROM meta WHERE key=?1", [key], |r| {
                    r.get::<_, String>(0)
                })
                .ok();
            Ok(v)
        })
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                rusqlite::params![key, value],
            )?;
            Ok(())
        })
    }

    /// Record a folder as recently opened (bumps `opened_at`).
    pub fn touch_recent(&self, path: &str, name: &str, now: i64) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO recents(path,name,opened_at) VALUES(?1,?2,?3)
                 ON CONFLICT(path) DO UPDATE SET opened_at=excluded.opened_at, name=excluded.name",
                rusqlite::params![path, name, now],
            )?;
            Ok(())
        })
    }

    pub fn recents(&self, limit: usize) -> Result<Vec<(String, String, i64)>> {
        self.with_conn(|c| {
            let mut stmt = c
                .prepare("SELECT path,name,opened_at FROM recents ORDER BY opened_at DESC LIMIT ?1")?;
            let rows = stmt
                .query_map([limit as i64], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?,
                    ))
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }
}

/// Per-app data dir, e.g. `~/.senclaw/space-apps/code-ide/`.
pub fn default_data_dir(app: &str) -> PathBuf {
    let base = std::env::var("SENCLAW_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(home).join(".senclaw")
        });
    base.join("space-apps").join(app)
}
