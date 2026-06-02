use anyhow::Result;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::Mutex;

/// SQLite-backed store for email accounts and the message cache.
///
/// Schema mirrors the tables that previously lived in senclaw core
/// (`space_email_accounts`, `space_email_cache`) but is now owned by this
/// standalone Space App and stored under
/// `~/.senclaw/space-apps-data/email/email.db`.
pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    pub fn open() -> Result<Self> {
        let dir = data_dir();
        std::fs::create_dir_all(&dir)?;
        let conn = Connection::open(dir.join("email.db"))?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let conn = self.conn.lock().unwrap();
        f(&conn)
    }
}

pub fn data_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".senclaw")
        .join("space-apps-data")
        .join("email")
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS space_email_accounts (
    id          TEXT PRIMARY KEY,
    label       TEXT NOT NULL,
    email       TEXT NOT NULL,
    imap_host   TEXT NOT NULL,
    imap_port   INTEGER NOT NULL DEFAULT 993,
    smtp_host   TEXT NOT NULL,
    smtp_port   INTEGER NOT NULL DEFAULT 587,
    username    TEXT NOT NULL,
    password    TEXT NOT NULL,
    use_tls     INTEGER DEFAULT 1,
    created_at  INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS space_email_cache (
    id          TEXT PRIMARY KEY,
    account_id  TEXT NOT NULL,
    folder      TEXT NOT NULL DEFAULT 'INBOX',
    subject     TEXT,
    from_addr   TEXT,
    to_addrs    TEXT,
    date        INTEGER,
    body_text   TEXT,
    body_html   TEXT,
    flags       TEXT DEFAULT '[]',
    synced_at   INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_space_email_cache_account
    ON space_email_cache(account_id, folder, date);
"#;
