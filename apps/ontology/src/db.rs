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

/// A logic function's definition: `(kind, input_kind, target, instruction, auto_apply)`.
pub type FunctionDef = (String, String, String, String, bool);

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
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
  created_at   INTEGER NOT NULL,
  origin       TEXT NOT NULL DEFAULT '',
  note         TEXT NOT NULL DEFAULT ''
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
CREATE TABLE IF NOT EXISTS settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL DEFAULT ''
);
-- AIP Logic: LLM-powered functions that emit typed Actions as proposals.
CREATE TABLE IF NOT EXISTS logic_functions (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  project_id   INTEGER NOT NULL,
  name         TEXT NOT NULL,
  kind         TEXT NOT NULL DEFAULT 'extract',   -- extract | classify
  input_kind   TEXT NOT NULL DEFAULT 'text',      -- text | source
  target       TEXT NOT NULL DEFAULT '',          -- source name (classify) or class hint
  instruction  TEXT NOT NULL DEFAULT '',          -- the human's plain-language task
  auto_apply   INTEGER NOT NULL DEFAULT 0,        -- 0 = human-in-the-loop (default)
  created_at   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_lf_project ON logic_functions(project_id);
-- The review queue. Every LLM edit lands here first; nothing writes data until approved.
CREATE TABLE IF NOT EXISTS proposals (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  project_id   INTEGER NOT NULL,
  function_id  INTEGER,
  action_json  TEXT NOT NULL,                     -- the typed Action
  summary      TEXT NOT NULL DEFAULT '',
  rationale    TEXT NOT NULL DEFAULT '',
  confidence   REAL NOT NULL DEFAULT 0,
  status       TEXT NOT NULL DEFAULT 'pending',   -- pending | approved | rejected | invalid
  valid        INTEGER NOT NULL DEFAULT 1,        -- passed the type checker?
  invalid_reason TEXT NOT NULL DEFAULT '',
  batch        TEXT NOT NULL DEFAULT '',          -- provenance batch once applied
  created_at   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_prop_project ON proposals(project_id, status);
-- Evals: input → expected, run across models.
CREATE TABLE IF NOT EXISTS eval_cases (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  function_id  INTEGER NOT NULL,
  input        TEXT NOT NULL,
  expect       TEXT NOT NULL DEFAULT '',          -- substring the summary set must contain
  created_at   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_eval_fn ON eval_cases(function_id);
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
    /// Storage kind the pipeline understands: `csv | json | text`.
    pub kind: String,
    /// Format the file actually arrived in (`xlsx`, `pdf`, `jsonl`, …).
    pub origin: String,
    /// One-line record of what the ingest sniffer did.
    pub note: String,
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
        // Migrations for databases created before universal ingest. ALTER TABLE
        // ADD COLUMN errors on an existing column, which is the "already
        // migrated" signal — ignore it.
        for stmt in [
            "ALTER TABLE sources ADD COLUMN origin TEXT NOT NULL DEFAULT ''",
            "ALTER TABLE sources ADD COLUMN note TEXT NOT NULL DEFAULT ''",
        ] {
            let _ = conn.execute(stmt, []);
        }
        Ok(Self {
            conn: Mutex::new(conn),
        })
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
            .map(
                |(id, name, description, base_iri, prefixes, ca, ua)| Project {
                    id,
                    name,
                    description,
                    base_iri,
                    prefixes: serde_json::from_str(&prefixes)
                        .unwrap_or_else(|_| serde_json::json!({})),
                    created_at: ca,
                    updated_at: ua,
                    triple_count: triple_counts(id),
                },
            )
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
        c.execute(
            "DELETE FROM competency_questions WHERE project_id = ?1",
            [id],
        )?;
        c.execute("DELETE FROM run_logs WHERE project_id = ?1", [id])?;
        c.execute("DELETE FROM proposals WHERE project_id = ?1", [id])?;
        c.execute(
            "DELETE FROM eval_cases WHERE function_id IN (SELECT id FROM logic_functions WHERE project_id = ?1)",
            [id],
        )?;
        c.execute("DELETE FROM logic_functions WHERE project_id = ?1", [id])?;
        c.execute("DELETE FROM projects WHERE id = ?1", [id])?;
        Ok(())
    }

    fn touch(&self, id: i64) {
        let c = self.lock();
        let _ = c.execute(
            "UPDATE projects SET updated_at = ?2 WHERE id = ?1",
            params![id, now()],
        );
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
        self.lock().execute(
            "UPDATE projects SET base_iri = ?2 WHERE id = ?1",
            params![id, base],
        )?;
        self.touch(id);
        Ok(())
    }

    // ---- mapping & shapes blobs ------------------------------------------

    pub fn get_mapping(&self, id: i64) -> Result<serde_json::Value> {
        let s: Option<String> = self
            .lock()
            .query_row(
                "SELECT mapping_json FROM projects WHERE id = ?1",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(s.and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| serde_json::json!({})))
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
            .query_row(
                "SELECT shapes_json FROM projects WHERE id = ?1",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(s.and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| serde_json::json!({"nodeShapes": []})))
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
            .query_row(
                "SELECT dataset_trig FROM projects WHERE id = ?1",
                [id],
                |r| r.get(0),
            )
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

    #[allow(clippy::too_many_arguments)]
    pub fn add_source(
        &self,
        project_id: i64,
        name: &str,
        kind: &str,
        content: &str,
        columns: &serde_json::Value,
        row_count: i64,
        origin: &str,
        note: &str,
    ) -> Result<i64> {
        let c = self.lock();
        // Re-uploading the same logical name replaces it — otherwise a mapping
        // that references the name would silently keep lifting the stale copy
        // (source_by_name picks the newest, but the old rows linger in the UI).
        c.execute(
            "DELETE FROM sources WHERE project_id = ?1 AND name = ?2",
            params![project_id, name],
        )?;
        c.execute(
            "INSERT INTO sources (project_id, name, kind, content, columns_json, row_count, created_at, origin, note)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![project_id, name, kind, content, columns.to_string(), row_count, now(), origin, note],
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
            "SELECT id, name, kind, columns_json, row_count, created_at, origin, note
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
                    origin: r.get(6)?,
                    note: r.get(7)?,
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
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                },
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
        self.lock()
            .execute("DELETE FROM sources WHERE id = ?1", [source_id])?;
        Ok(())
    }

    // ---- competency questions --------------------------------------------

    pub fn add_cq(
        &self,
        project_id: i64,
        question: &str,
        sparql: &str,
        expect: &str,
    ) -> Result<i64> {
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
        self.lock()
            .execute("DELETE FROM competency_questions WHERE id = ?1", [id])?;
        Ok(())
    }

    // ---- settings (small key/value store) --------------------------------

    /// Read a setting, or `default` when unset.
    pub fn get_setting(&self, key: &str, default: &str) -> String {
        self.lock()
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
                r.get::<_, String>(0)
            })
            .optional()
            .ok()
            .flatten()
            .unwrap_or_else(|| default.to_string())
    }

    /// Upsert a setting.
    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.lock().execute(
            "INSERT INTO settings(key, value) VALUES(?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    // ---- AIP logic functions ---------------------------------------------

    #[allow(clippy::too_many_arguments)]
    pub fn create_function(
        &self,
        project_id: i64,
        name: &str,
        kind: &str,
        input_kind: &str,
        target: &str,
        instruction: &str,
        auto_apply: bool,
    ) -> Result<i64> {
        let c = self.lock();
        c.execute(
            "INSERT INTO logic_functions (project_id, name, kind, input_kind, target, instruction, auto_apply, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![project_id, name, kind, input_kind, target, instruction, auto_apply as i64, now()],
        )?;
        Ok(c.last_insert_rowid())
    }

    pub fn list_functions(&self, project_id: i64) -> Result<Vec<serde_json::Value>> {
        let c = self.lock();
        let mut stmt = c.prepare(
            "SELECT id,name,kind,input_kind,target,instruction,auto_apply,created_at
             FROM logic_functions WHERE project_id=?1 ORDER BY id",
        )?;
        let rows = stmt
            .query_map([project_id], |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_,i64>(0)?, "name": r.get::<_,String>(1)?, "kind": r.get::<_,String>(2)?,
                    "inputKind": r.get::<_,String>(3)?, "target": r.get::<_,String>(4)?,
                    "instruction": r.get::<_,String>(5)?, "autoApply": r.get::<_,i64>(6)? != 0,
                    "createdAt": r.get::<_,i64>(7)?,
                }))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// `(kind, input_kind, target, instruction, auto_apply)` for one function.
    pub fn get_function(&self, id: i64) -> Result<Option<FunctionDef>> {
        self.lock()
            .query_row(
                "SELECT kind,input_kind,target,instruction,auto_apply FROM logic_functions WHERE id=?1",
                [id],
                |r| Ok((r.get::<_,String>(0)?, r.get::<_,String>(1)?, r.get::<_,String>(2)?, r.get::<_,String>(3)?, r.get::<_,i64>(4)? != 0)),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn delete_function(&self, id: i64) -> Result<()> {
        let c = self.lock();
        c.execute("DELETE FROM eval_cases WHERE function_id=?1", [id])?;
        c.execute("DELETE FROM logic_functions WHERE id=?1", [id])?;
        Ok(())
    }

    // ---- proposal queue ---------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    pub fn add_proposal(
        &self,
        project_id: i64,
        function_id: Option<i64>,
        action_json: &str,
        summary: &str,
        rationale: &str,
        confidence: f64,
        valid: bool,
        invalid_reason: &str,
    ) -> Result<i64> {
        let status = if valid { "pending" } else { "invalid" };
        let c = self.lock();
        c.execute(
            "INSERT INTO proposals (project_id,function_id,action_json,summary,rationale,confidence,status,valid,invalid_reason,created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![project_id, function_id, action_json, summary, rationale, confidence, status, valid as i64, invalid_reason, now()],
        )?;
        Ok(c.last_insert_rowid())
    }

    pub fn list_proposals(
        &self,
        project_id: i64,
        status: Option<&str>,
    ) -> Result<Vec<serde_json::Value>> {
        let c = self.lock();
        let sql = "SELECT id,function_id,action_json,summary,rationale,confidence,status,valid,invalid_reason,batch,created_at
                   FROM proposals WHERE project_id=?1"
            .to_string();
        let map = |r: &rusqlite::Row| {
            Ok(serde_json::json!({
                "id": r.get::<_,i64>(0)?, "functionId": r.get::<_,Option<i64>>(1)?,
                "action": serde_json::from_str::<serde_json::Value>(&r.get::<_,String>(2)?).unwrap_or(serde_json::Value::Null),
                "summary": r.get::<_,String>(3)?, "rationale": r.get::<_,String>(4)?,
                "confidence": r.get::<_,f64>(5)?, "status": r.get::<_,String>(6)?,
                "valid": r.get::<_,i64>(7)? != 0, "invalidReason": r.get::<_,String>(8)?,
                "batch": r.get::<_,String>(9)?, "createdAt": r.get::<_,i64>(10)?,
            }))
        };
        if let Some(st) = status {
            let mut stmt = c.prepare(&format!("{sql} AND status=?2 ORDER BY id DESC"))?;
            let rows = stmt
                .query_map(params![project_id, st], map)?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(rows)
        } else {
            let mut stmt = c.prepare(&format!("{sql} ORDER BY id DESC"))?;
            let rows = stmt
                .query_map([project_id], map)?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(rows)
        }
    }

    /// Counts by status: `(pending, approved, rejected, invalid)`.
    pub fn proposal_counts(&self, project_id: i64) -> Result<serde_json::Value> {
        let c = self.lock();
        let mut stmt = c.prepare(
            "SELECT status, COUNT(*) FROM proposals WHERE project_id=?1 GROUP BY status",
        )?;
        let mut out = serde_json::Map::new();
        let rows = stmt.query_map([project_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?;
        for row in rows {
            let (s, n) = row?;
            out.insert(s, serde_json::json!(n));
        }
        Ok(serde_json::Value::Object(out))
    }

    /// The pending proposals' action JSON, for approval.
    pub fn pending_actions(&self, project_id: i64, ids: &[i64]) -> Result<Vec<(i64, String)>> {
        let c = self.lock();
        let mut out = Vec::new();
        if ids.is_empty() {
            let mut stmt = c.prepare(
                "SELECT id,action_json FROM proposals WHERE project_id=?1 AND status='pending' AND valid=1 ORDER BY id",
            )?;
            let rows = stmt.query_map([project_id], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            })?;
            for row in rows {
                out.push(row?);
            }
        } else {
            for id in ids {
                if let Some(j) = c
                    .query_row(
                        "SELECT action_json FROM proposals WHERE id=?1 AND project_id=?2 AND status='pending' AND valid=1",
                        params![id, project_id],
                        |r| r.get::<_, String>(0),
                    )
                    .optional()?
                {
                    out.push((*id, j));
                }
            }
        }
        Ok(out)
    }

    /// Revert proposals that were applied into `batch` back to pending — used
    /// when the batch is dropped (undo an approval), so the audit trail never
    /// claims a proposal is "approved" after its triples were removed. Returns
    /// how many were reverted.
    pub fn revert_proposals_for_batch(&self, project_id: i64, batch: &str) -> Result<usize> {
        Ok(self.lock().execute(
            "UPDATE proposals SET status='pending', batch='' WHERE project_id=?1 AND batch=?2 AND status='approved'",
            params![project_id, batch],
        )?)
    }

    pub fn set_proposal_status(&self, id: i64, status: &str, batch: &str) -> Result<()> {
        self.lock().execute(
            "UPDATE proposals SET status=?2, batch=?3 WHERE id=?1",
            params![id, status, batch],
        )?;
        Ok(())
    }

    /// Reject every pending proposal, or a specific set. Returns the count.
    pub fn reject_proposals(&self, project_id: i64, ids: &[i64]) -> Result<usize> {
        let c = self.lock();
        if ids.is_empty() {
            Ok(c.execute(
                "UPDATE proposals SET status='rejected' WHERE project_id=?1 AND status='pending'",
                [project_id],
            )?)
        } else {
            let mut n = 0;
            for id in ids {
                n += c.execute(
                    "UPDATE proposals SET status='rejected' WHERE id=?1 AND project_id=?2 AND status='pending'",
                    params![id, project_id],
                )?;
            }
            Ok(n)
        }
    }

    // ---- eval cases -------------------------------------------------------

    pub fn add_eval_case(&self, function_id: i64, input: &str, expect: &str) -> Result<i64> {
        let c = self.lock();
        c.execute(
            "INSERT INTO eval_cases (function_id,input,expect,created_at) VALUES (?1,?2,?3,?4)",
            params![function_id, input, expect, now()],
        )?;
        Ok(c.last_insert_rowid())
    }

    pub fn list_eval_cases(&self, function_id: i64) -> Result<Vec<(i64, String, String)>> {
        let c = self.lock();
        let mut stmt =
            c.prepare("SELECT id,input,expect FROM eval_cases WHERE function_id=?1 ORDER BY id")?;
        let rows = stmt
            .query_map([function_id], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // ---- logs -------------------------------------------------------------

    /// Recent activity for a project, newest first — the "what happened here"
    /// half of the metadata AIP Assist indexes.
    pub fn list_logs(&self, project_id: i64, limit: i64) -> Result<Vec<(String, String, i64)>> {
        let c = self.lock();
        let mut stmt = c.prepare(
            "SELECT kind, detail, created_at FROM run_logs
             WHERE project_id = ?1 ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![project_id, limit], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

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
            std::env::var("HOME")
                .map(PathBuf::from)
                .unwrap_or_default()
                .join(".senclaw")
        });
    let _ = std::fs::create_dir_all(base.join("apps").join("ontology"));
    base.join("apps").join("ontology").join("ontology.db")
}

#[allow(dead_code)]
fn _unused(_: &dyn Fn() -> anyhow::Error) {
    let _ = anyhow!("");
}
