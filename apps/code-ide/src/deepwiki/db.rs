use anyhow::Result;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// SQLite-backed code index: files, symbols, call/import edges, and an FTS5
/// search table. Shared by the codegraph and deepwiki App Spaces.
pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    /// Open (creating if needed) the index DB at `path`.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let conn = self.conn.lock().unwrap();
        f(&conn)
    }

    pub fn with_conn_mut<T>(&self, f: impl FnOnce(&mut Connection) -> Result<T>) -> Result<T> {
        let mut conn = self.conn.lock().unwrap();
        f(&mut conn)
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
}

/// Default per-app data directory under `~/.senclaw/space-apps-data/<app>/`.
pub fn default_data_dir(app: &str) -> PathBuf {
    dirs_home()
        .join(".senclaw")
        .join("space-apps-data")
        .join(app)
}

fn dirs_home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// A per-workspace wiki/index DB path, keyed by a hash of the repo root, so each
/// opened workspace keeps its own wiki and code index (never shared/overwritten).
pub fn wiki_db_path(root: &Path) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    root.to_string_lossy().hash(&mut h);
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let safe: String = name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .take(24)
        .collect();
    default_data_dir("deepwiki").join(format!("ws-{safe}-{:016x}.db", h.finish()))
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- History of repo roots that have been indexed (survives re-index/wipe of a
-- different root), so the UI can offer quick re-selection.
CREATE TABLE IF NOT EXISTS indexed_roots (
    path         TEXT PRIMARY KEY,
    last_indexed INTEGER NOT NULL,
    files        INTEGER NOT NULL DEFAULT 0,
    symbols      INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS files (
    id         INTEGER PRIMARY KEY,
    path       TEXT UNIQUE NOT NULL,
    lang       TEXT NOT NULL,
    hash       TEXT NOT NULL,
    mtime      INTEGER NOT NULL,
    loc        INTEGER NOT NULL,
    indexed_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS symbols (
    id         INTEGER PRIMARY KEY,
    file_id    INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    kind       TEXT NOT NULL,
    parent     TEXT,
    start_line INTEGER NOT NULL,
    end_line   INTEGER NOT NULL,
    signature  TEXT NOT NULL DEFAULT '',
    doc        TEXT
);
CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);
CREATE INDEX IF NOT EXISTS idx_symbols_file ON symbols(file_id);

CREATE TABLE IF NOT EXISTS edges (
    id          INTEGER PRIMARY KEY,
    src_file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    src_symbol  TEXT,
    kind        TEXT NOT NULL,
    target      TEXT NOT NULL,
    line        INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_edges_target ON edges(target);
CREATE INDEX IF NOT EXISTS idx_edges_src ON edges(src_file_id, src_symbol);
CREATE INDEX IF NOT EXISTS idx_edges_kind ON edges(kind);

-- Porter stemming + unicode61 so natural-language queries match identifiers:
-- "indexing" → stem "index" matches the "index" token in "index_repo".
CREATE VIRTUAL TABLE IF NOT EXISTS symbols_fts USING fts5(
    name, signature, doc, kind UNINDEXED, symbol_id UNINDEXED,
    tokenize = 'porter unicode61'
);
"#;
