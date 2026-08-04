use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// SQLite store for the Diagrams app: one row per draw.io diagram. The `xml`
/// column holds the uncompressed mxGraphModel/mxfile XML (the source of truth);
/// `svg` caches the last snapshot exported by the editor (only the editor can
/// render, so headless MCP exports serve this cache with a staleness flag).
pub struct Db {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS diagrams (
  id             INTEGER PRIMARY KEY AUTOINCREMENT,
  name           TEXT NOT NULL,
  kind           TEXT NOT NULL DEFAULT 'flowchart',
  xml            TEXT NOT NULL DEFAULT '',
  svg            TEXT NOT NULL DEFAULT '',
  svg_updated_at INTEGER NOT NULL DEFAULT 0,
  created_at     INTEGER NOT NULL,
  updated_at     INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS ai_log (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  diagram_id INTEGER NOT NULL DEFAULT 0,
  prompt     TEXT NOT NULL,
  mode       TEXT NOT NULL,
  model      TEXT NOT NULL DEFAULT '',
  finish     TEXT NOT NULL DEFAULT '',
  ok         INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL
);
"#;

/// Columns added after v1 — applied to pre-existing DBs (errors on already-present
/// columns are ignored).
const MIGRATIONS: &[&str] = &[];

/// A diagram's metadata (list view / MCP list).
#[derive(Serialize)]
pub struct DiagramMeta {
    pub id: i64,
    pub name: String,
    pub kind: String,
    /// Number of `<mxCell` occurrences — a cheap size indicator.
    pub cells: usize,
    /// True when the cached SVG snapshot is older than the last XML write.
    pub svg_stale: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Serialize)]
pub struct Diagram {
    #[serde(flatten)]
    pub meta: DiagramMeta,
    pub xml: String,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(SCHEMA)?;
        for m in MIGRATIONS {
            let _ = conn.execute_batch(m); // already-applied → ignore
        }
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn meta_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DiagramMeta> {
        let xml: String = row.get("xml")?;
        let updated_at: i64 = row.get("updated_at")?;
        let svg: String = row.get("svg")?;
        let svg_updated_at: i64 = row.get("svg_updated_at")?;
        Ok(DiagramMeta {
            id: row.get("id")?,
            name: row.get("name")?,
            kind: row.get("kind")?,
            cells: xml.matches("<mxCell").count(),
            svg_stale: svg.is_empty() || svg_updated_at < updated_at,
            created_at: row.get("created_at")?,
            updated_at,
        })
    }

    pub fn list(&self) -> Result<Vec<DiagramMeta>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, kind, xml, svg, svg_updated_at, created_at, updated_at
             FROM diagrams ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], Self::meta_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn create(&self, name: &str, kind: &str, xml: &str, now: i64) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO diagrams (name, kind, xml, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)",
            params![name, kind, xml, now],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get(&self, id: i64) -> Result<Option<Diagram>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, kind, xml, svg, svg_updated_at, created_at, updated_at
             FROM diagrams WHERE id = ?1",
        )?;
        let d = stmt
            .query_row(params![id], |row| {
                let xml: String = row.get("xml")?;
                Ok(Diagram {
                    meta: Self::meta_from_row(row)?,
                    xml,
                })
            })
            .optional()?;
        Ok(d)
    }

    pub fn get_svg(&self, id: i64) -> Result<Option<(String, bool)>> {
        let conn = self.conn.lock().unwrap();
        let r = conn
            .query_row(
                "SELECT svg, svg_updated_at < updated_at OR svg = '' FROM diagrams WHERE id = ?1",
                params![id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?)),
            )
            .optional()?;
        Ok(r)
    }

    pub fn set_xml(&self, id: i64, xml: &str, now: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "UPDATE diagrams SET xml = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, xml, now],
        )?;
        if n == 0 {
            return Err(anyhow!("diagram {id} not found"));
        }
        Ok(())
    }

    pub fn set_svg(&self, id: i64, svg: &str, now: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "UPDATE diagrams SET svg = ?2, svg_updated_at = ?3 WHERE id = ?1",
            params![id, svg, now],
        )?;
        if n == 0 {
            return Err(anyhow!("diagram {id} not found"));
        }
        Ok(())
    }

    pub fn rename(&self, id: i64, name: &str, now: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "UPDATE diagrams SET name = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, name, now],
        )?;
        if n == 0 {
            return Err(anyhow!("diagram {id} not found"));
        }
        Ok(())
    }

    pub fn delete(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM diagrams WHERE id = ?1", params![id])?;
        conn.execute("DELETE FROM ai_log WHERE diagram_id = ?1", params![id])?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn log_ai(
        &self,
        diagram_id: i64,
        prompt: &str,
        mode: &str,
        model: &str,
        finish: &str,
        ok: bool,
        now: i64,
    ) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO ai_log (diagram_id, prompt, mode, model, finish, ok, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![diagram_id, prompt, mode, model, finish, ok as i64, now],
        );
    }
}

pub fn default_data_dir(app: &str) -> PathBuf {
    let base = std::env::var("SENCLAW_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(".senclaw")
        });
    base.join("space-apps").join(app)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_db() -> Db {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        Db {
            conn: Mutex::new(conn),
        }
    }

    #[test]
    fn crud_roundtrip() {
        let db = mem_db();
        let id = db
            .create("Test", "flowchart", "<mxGraphModel/>", 100)
            .unwrap();
        let d = db.get(id).unwrap().unwrap();
        assert_eq!(d.meta.name, "Test");
        assert!(d.meta.svg_stale);

        db.set_xml(
            id,
            "<mxGraphModel><root><mxCell id=\"0\"/></root></mxGraphModel>",
            200,
        )
        .unwrap();
        db.set_svg(id, "data:image/svg+xml;base64,AAA", 300)
            .unwrap();
        let d = db.get(id).unwrap().unwrap();
        assert_eq!(d.meta.cells, 1);
        assert!(!d.meta.svg_stale);

        // XML written after the snapshot → stale again.
        db.set_xml(id, "<mxGraphModel/>", 400).unwrap();
        let (svg, stale) = db.get_svg(id).unwrap().unwrap();
        assert!(svg.starts_with("data:image/svg+xml"));
        assert!(stale);

        db.rename(id, "Renamed", 500).unwrap();
        assert_eq!(db.list().unwrap()[0].name, "Renamed");
        db.delete(id).unwrap();
        assert!(db.get(id).unwrap().is_none());
    }

    #[test]
    fn missing_rows_error() {
        let db = mem_db();
        assert!(db.set_xml(99, "x", 1).is_err());
        assert!(db.set_svg(99, "x", 1).is_err());
        assert!(db.rename(99, "x", 1).is_err());
    }
}
