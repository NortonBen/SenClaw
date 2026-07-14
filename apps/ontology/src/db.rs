//! App-shell metadata store (rusqlite). The *semantic* layer (triples) lives in
//! Oxigraph; here we keep projects, raw sources, the mapping/shapes/prefixes
//! JSON blobs, competency questions, run logs — and a serialized TriG snapshot
//! of each project's dataset for persistence across restarts.

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

pub struct Db {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS projects (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  name          TEXT NOT NULL,
  description   TEXT NOT NULL DEFAULT '',
  base_iri      TEXT NOT NULL DEFAULT 'http://senclaw.local/onto/',
  prefixes_json TEXT NOT NULL DEFAULT '{}',
  mapping_json  TEXT NOT NULL DEFAULT '{}',
  shapes_json   TEXT NOT NULL DEFAULT '{}',
  dataset_trig  TEXT NOT NULL DEFAULT '',
  created_at    INTEGER NOT NULL,
  updated_at    INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS sources (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  project_id   INTEGER NOT NULL,
  name         TEXT NOT NULL,
  kind         TEXT NOT NULL DEFAULT 'csv',
  content      TEXT NOT NULL DEFAULT '',
  columns_json TEXT NOT NULL DEFAULT '[]',
  row_count    INTEGER NOT NULL DEFAULT 0,
  created_at   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_sources_project ON sources(project_id);
CREATE TABLE IF NOT EXISTS competency_questions (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  project_id INTEGER NOT NULL,
  question   TEXT NOT NULL,
  sparql     TEXT NOT NULL DEFAULT '',
  expect     TEXT NOT NULL DEFAULT 'nonempty',
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_cq_project ON competency_questions(project_id);
CREATE TABLE IF NOT EXISTS run_logs (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  project_id INTEGER NOT NULL,
  kind       TEXT NOT NULL,
  detail     TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_logs_project ON run_logs(project_id);
"#;

#[derive(Serialize)]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub description: String,
    #[serde(rename = "baseIri")]
    pub base_iri: String,
    pub prefixes: serde_json::Value,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
    #[serde(rename = "tripleCount")]
    pub triple_count: i64,
}

#[derive(Serialize)]
pub struct Source {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub columns: serde_json::Value,
    #[serde(rename = "rowCount")]
    pub row_count: i64,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
}

#[derive(Serialize)]
pub struct CompetencyQuestion {
    pub id: i64,
    pub question: String,
    pub sparql: String,
    pub expect: String,
}

impl Db {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap()
    }

    // ---- projects ---------------------------------------------------------

    pub fn create_project(&self, name: &str, description: &str, base_iri: &str) -> Result<i64> {
        let c = self.lock();
        c.execute(
            "INSERT INTO projects (name, description, base_iri, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)",
            params![name, description, base_iri, now()],
        )?;
        Ok(c.last_insert_rowid())
    }

    pub fn list_projects(&self, triple_counts: &dyn Fn(i64) -> i64) -> Result<Vec<Project>> {
        // Collect the rows, then DROP the DB lock BEFORE invoking `triple_counts`
        // — that closure re-enters the DB (graph_for → get_dataset), and the std
        // Mutex is not reentrant, so holding the lock here would deadlock.
        let rows = {
            let c = self.lock();
            let mut stmt = c.prepare(
                "SELECT id, name, description, base_iri, prefixes_json, created_at, updated_at
                 FROM projects ORDER BY updated_at DESC",
            )?;
            let v = stmt
                .query_map([], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, i64>(5)?,
                        r.get::<_, i64>(6)?,
                    ))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            v
        };
        Ok(rows
            .into_iter()
            .map(|(id, name, description, base_iri, prefixes, ca, ua)| Project {
                id,
                name,
                description,
                base_iri,
                prefixes: serde_json::from_str(&prefixes).unwrap_or_else(|_| serde_json::json!({})),
                created_at: ca,
                updated_at: ua,
                triple_count: triple_counts(id),
            })
            .collect())
    }

    pub fn get_project(&self, id: i64) -> Result<Option<Project>> {
        let c = self.lock();
        c.query_row(
            "SELECT id, name, description, base_iri, prefixes_json, created_at, updated_at
             FROM projects WHERE id = ?1",
            [id],
            |r| {
                Ok(Project {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    description: r.get(2)?,
                    base_iri: r.get(3)?,
                    prefixes: serde_json::from_str(&r.get::<_, String>(4)?)
                        .unwrap_or_else(|_| serde_json::json!({})),
                    created_at: r.get(5)?,
                    updated_at: r.get(6)?,
                    triple_count: 0,
                })
            },
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn delete_project(&self, id: i64) -> Result<()> {
        let c = self.lock();
        c.execute("DELETE FROM sources WHERE project_id = ?1", [id])?;
        c.execute("DELETE FROM competency_questions WHERE project_id = ?1", [id])?;
        c.execute("DELETE FROM run_logs WHERE project_id = ?1", [id])?;
        c.execute("DELETE FROM projects WHERE id = ?1", [id])?;
        Ok(())
    }

    fn touch(&self, id: i64) {
        let c = self.lock();
        let _ = c.execute("UPDATE projects SET updated_at = ?2 WHERE id = ?1", params![id, now()]);
    }

    pub fn set_prefixes(&self, id: i64, prefixes: &serde_json::Value) -> Result<()> {
        self.lock().execute(
            "UPDATE projects SET prefixes_json = ?2 WHERE id = ?1",
            params![id, prefixes.to_string()],
        )?;
        self.touch(id);
        Ok(())
    }

    pub fn set_base_iri(&self, id: i64, base: &str) -> Result<()> {
        self.lock()
            .execute("UPDATE projects SET base_iri = ?2 WHERE id = ?1", params![id, base])?;
        self.touch(id);
        Ok(())
    }

    // ---- mapping & shapes blobs ------------------------------------------

    pub fn get_mapping(&self, id: i64) -> Result<serde_json::Value> {
        let s: Option<String> = self
            .lock()
            .query_row("SELECT mapping_json FROM projects WHERE id = ?1", [id], |r| r.get(0))
            .optional()?;
        Ok(s.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_else(|| serde_json::json!({})))
    }

    pub fn set_mapping(&self, id: i64, mapping: &serde_json::Value) -> Result<()> {
        self.lock().execute(
            "UPDATE projects SET mapping_json = ?2 WHERE id = ?1",
            params![id, mapping.to_string()],
        )?;
        self.touch(id);
        Ok(())
    }

    pub fn get_shapes(&self, id: i64) -> Result<serde_json::Value> {
        let s: Option<String> = self
            .lock()
            .query_row("SELECT shapes_json FROM projects WHERE id = ?1", [id], |r| r.get(0))
            .optional()?;
        Ok(s.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_else(|| serde_json::json!({"nodeShapes": []})))
    }

    pub fn set_shapes(&self, id: i64, shapes: &serde_json::Value) -> Result<()> {
        self.lock().execute(
            "UPDATE projects SET shapes_json = ?2 WHERE id = ?1",
            params![id, shapes.to_string()],
        )?;
        self.touch(id);
        Ok(())
    }

    // ---- dataset TriG snapshot -------------------------------------------

    pub fn get_dataset(&self, id: i64) -> Result<String> {
        let s: Option<String> = self
            .lock()
            .query_row("SELECT dataset_trig FROM projects WHERE id = ?1", [id], |r| r.get(0))
            .optional()?;
        Ok(s.unwrap_or_default())
    }

    pub fn set_dataset(&self, id: i64, trig: &str) -> Result<()> {
        self.lock().execute(
            "UPDATE projects SET dataset_trig = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, trig, now()],
        )?;
        Ok(())
    }

    // ---- sources ----------------------------------------------------------

    pub fn add_source(
        &self,
        project_id: i64,
        name: &str,
        kind: &str,
        content: &str,
        columns: &serde_json::Value,
        row_count: i64,
    ) -> Result<i64> {
        let c = self.lock();
        c.execute(
            "INSERT INTO sources (project_id, name, kind, content, columns_json, row_count, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![project_id, name, kind, content, columns.to_string(), row_count, now()],
        )?;
        Ok(c.last_insert_rowid())
    }

    pub fn set_source_columns(&self, source_id: i64, columns: &serde_json::Value) -> Result<()> {
        self.lock().execute(
            "UPDATE sources SET columns_json = ?2 WHERE id = ?1",
            params![source_id, columns.to_string()],
        )?;
        Ok(())
    }

    pub fn list_sources(&self, project_id: i64) -> Result<Vec<Source>> {
        let c = self.lock();
        let mut stmt = c.prepare(
            "SELECT id, name, kind, columns_json, row_count, created_at
             FROM sources WHERE project_id = ?1 ORDER BY id",
        )?;
        let rows = stmt
            .query_map([project_id], |r| {
                Ok(Source {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    kind: r.get(2)?,
                    columns: serde_json::from_str(&r.get::<_, String>(3)?)
                        .unwrap_or_else(|_| serde_json::json!([])),
                    row_count: r.get(4)?,
                    created_at: r.get(5)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Raw content + kind + name of one source.
    pub fn get_source(&self, source_id: i64) -> Result<Option<(String, String, String)>> {
        self.lock()
            .query_row(
                "SELECT name, kind, content FROM sources WHERE id = ?1",
                [source_id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)),
            )
            .optional()
            .map_err(Into::into)
    }

    /// Look up a source's raw content by its logical name within a project.
    pub fn source_by_name(&self, project_id: i64, name: &str) -> Result<Option<(String, String)>> {
        self.lock()
            .query_row(
                "SELECT kind, content FROM sources WHERE project_id = ?1 AND name = ?2 ORDER BY id DESC LIMIT 1",
                params![project_id, name],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn delete_source(&self, source_id: i64) -> Result<()> {
        self.lock().execute("DELETE FROM sources WHERE id = ?1", [source_id])?;
        Ok(())
    }

    // ---- competency questions --------------------------------------------

    pub fn add_cq(&self, project_id: i64, question: &str, sparql: &str, expect: &str) -> Result<i64> {
        let c = self.lock();
        c.execute(
            "INSERT INTO competency_questions (project_id, question, sparql, expect, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![project_id, question, sparql, expect, now()],
        )?;
        Ok(c.last_insert_rowid())
    }

    pub fn update_cq(&self, id: i64, question: &str, sparql: &str, expect: &str) -> Result<()> {
        self.lock().execute(
            "UPDATE competency_questions SET question = ?2, sparql = ?3, expect = ?4 WHERE id = ?1",
            params![id, question, sparql, expect],
        )?;
        Ok(())
    }

    pub fn list_cq(&self, project_id: i64) -> Result<Vec<CompetencyQuestion>> {
        let c = self.lock();
        let mut stmt = c.prepare(
            "SELECT id, question, sparql, expect FROM competency_questions
             WHERE project_id = ?1 ORDER BY id",
        )?;
        let rows = stmt
            .query_map([project_id], |r| {
                Ok(CompetencyQuestion {
                    id: r.get(0)?,
                    question: r.get(1)?,
                    sparql: r.get(2)?,
                    expect: r.get(3)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn delete_cq(&self, id: i64) -> Result<()> {
        self.lock().execute("DELETE FROM competency_questions WHERE id = ?1", [id])?;
        Ok(())
    }

    // ---- logs -------------------------------------------------------------

    pub fn log(&self, project_id: i64, kind: &str, detail: &str) {
        let c = self.lock();
        let _ = c.execute(
            "INSERT INTO run_logs (project_id, kind, detail, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![project_id, kind, detail, now()],
        );
    }
}

/// Default on-disk location for the app DB.
pub fn default_db_path() -> PathBuf {
    if let Ok(p) = std::env::var("ONTOLOGY_DB") {
        return PathBuf::from(p);
    }
    let base = std::env::var("SENCLAW_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::var("HOME").map(PathBuf::from).unwrap_or_default().join(".senclaw")
        });
    let _ = std::fs::create_dir_all(base.join("apps").join("ontology"));
    base.join("apps").join("ontology").join("ontology.db")
}

#[allow(dead_code)]
fn _unused(_: &dyn Fn() -> anyhow::Error) {
    let _ = anyhow!("");
}
