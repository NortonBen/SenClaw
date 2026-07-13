use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub struct Db {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS documents (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  title        TEXT NOT NULL,
  content_text TEXT NOT NULL DEFAULT '',
  docx_blob    BLOB,
  created_at   INTEGER NOT NULL,
  updated_at   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_docs_updated ON documents(updated_at);
"#;

#[derive(Serialize, Clone)]
pub struct DocMeta {
    pub id: i64,
    pub title: String,
    pub excerpt: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub size_bytes: i64,
}

#[derive(Serialize, Clone)]
pub struct Doc {
    pub id: i64,
    pub title: String,
    pub content_text: String,
    pub created_at: i64,
    pub updated_at: i64,
}

pub fn default_data_dir(app: &str) -> PathBuf {
    let home = dirs_home_dir().unwrap_or_else(|| PathBuf::from("."));
    let dir = home.join(".senclaw").join("apps").join(app);
    std::fs::create_dir_all(&dir).ok();
    dir
}

fn dirs_home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    pub fn list_docs(&self) -> Result<Vec<DocMeta>> {
        let c = self.conn.lock().unwrap();
        let mut stmt = c.prepare(
            "SELECT id, title, content_text, created_at, updated_at, COALESCE(length(docx_blob), 0) \
             FROM documents ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            let content: String = row.get(2)?;
            let mut excerpt: String = content.chars().take(160).collect();
            if content.chars().count() > 160 {
                excerpt.push('…');
            }
            Ok(DocMeta {
                id: row.get(0)?,
                title: row.get(1)?,
                excerpt,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
                size_bytes: row.get(5)?,
            })
        })?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    pub fn create_doc(&self, title: &str, text: &str, now: i64) -> Result<i64> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "INSERT INTO documents (title, content_text, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
            params![title, text, now],
        )?;
        Ok(c.last_insert_rowid())
    }

    pub fn get_doc(&self, id: i64) -> Result<Option<Doc>> {
        let c = self.conn.lock().unwrap();
        let doc = c
            .query_row(
                "SELECT id, title, content_text, created_at, updated_at FROM documents WHERE id=?1",
                params![id],
                |row| {
                    Ok(Doc {
                        id: row.get(0)?,
                        title: row.get(1)?,
                        content_text: row.get(2)?,
                        created_at: row.get(3)?,
                        updated_at: row.get(4)?,
                    })
                },
            )
            .optional()?;
        Ok(doc)
    }

    pub fn get_docx_blob(&self, id: i64) -> Result<Option<Vec<u8>>> {
        let c = self.conn.lock().unwrap();
        let blob = c
            .query_row(
                "SELECT docx_blob FROM documents WHERE id=?1",
                params![id],
                |row| row.get::<_, Option<Vec<u8>>>(0),
            )
            .optional()?;
        Ok(blob.flatten())
    }

    pub fn save_doc(&self, id: i64, title: Option<&str>, text: &str, blob: Option<&[u8]>, now: i64) -> Result<()> {
        let c = self.conn.lock().unwrap();
        if let Some(t) = title {
            c.execute(
                "UPDATE documents SET title=?1, content_text=?2, docx_blob=COALESCE(?3, docx_blob), updated_at=?4 WHERE id=?5",
                params![t, text, blob, now, id],
            )?;
        } else {
            c.execute(
                "UPDATE documents SET content_text=?1, docx_blob=COALESCE(?2, docx_blob), updated_at=?3 WHERE id=?4",
                params![text, blob, now, id],
            )?;
        }
        Ok(())
    }

    pub fn rename_doc(&self, id: i64, title: &str, now: i64) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "UPDATE documents SET title=?1, updated_at=?2 WHERE id=?3",
            params![title, now, id],
        )?;
        Ok(())
    }

    pub fn delete_doc(&self, id: i64) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute("DELETE FROM documents WHERE id=?1", params![id])?;
        Ok(())
    }

    pub fn find_by_title(&self, title: &str) -> Result<Option<i64>> {
        let c = self.conn.lock().unwrap();
        let id = c
            .query_row(
                "SELECT id FROM documents WHERE title=?1 ORDER BY updated_at DESC LIMIT 1",
                params![title],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        Ok(id)
    }
}
