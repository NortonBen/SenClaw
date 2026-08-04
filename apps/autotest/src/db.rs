//! Local SQLite store cho app AutoTest. Local-first — không service ngoài nào
//! giữ dữ liệu này. Tables:
//!   * `suites`       — bộ kiểm thử (nhóm test case, environment mặc định)
//!   * `cases`        — test case: kind http|script|web, config/assertions/extract là JSON text
//!   * `environments` — bộ biến {{var}} (base_url, token…) theo môi trường
//!   * `runs`         — mỗi lần chạy suite/case: trạng thái + đếm pass/fail
//!   * `results`      — kết quả từng case trong một run: log + assertion đã đánh giá
//!   * `schedules`    — lịch chạy định kỳ theo suite (interval phút)
//!   * `activity`     — log hành động của app/agent
//!   * `settings`     — kv (URL mini-browser…)

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Db {
    conn: Mutex<Connection>,
}

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS suites (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  name        TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  env_id      INTEGER,
  status      TEXT NOT NULL DEFAULT 'active',
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS cases (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  suite_id   INTEGER NOT NULL,
  name       TEXT NOT NULL,
  kind       TEXT NOT NULL DEFAULT 'http',
  position   INTEGER NOT NULL DEFAULT 0,
  enabled    INTEGER NOT NULL DEFAULT 1,
  timeout_ms INTEGER NOT NULL DEFAULT 30000,
  config     TEXT NOT NULL DEFAULT '{}',
  assertions TEXT NOT NULL DEFAULT '[]',
  extract    TEXT NOT NULL DEFAULT '[]',
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_cases_suite ON cases(suite_id);
CREATE TABLE IF NOT EXISTS environments (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  name       TEXT NOT NULL UNIQUE,
  vars       TEXT NOT NULL DEFAULT '{}',
  created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS runs (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  suite_id     INTEGER,
  case_id      INTEGER,
  env_id       INTEGER,
  trigger_kind TEXT NOT NULL DEFAULT 'manual',
  status       TEXT NOT NULL DEFAULT 'running',
  started_at   INTEGER NOT NULL,
  finished_at  INTEGER,
  total        INTEGER NOT NULL DEFAULT 0,
  passed       INTEGER NOT NULL DEFAULT 0,
  failed       INTEGER NOT NULL DEFAULT 0,
  errors       INTEGER NOT NULL DEFAULT 0,
  skipped      INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_runs_suite ON runs(suite_id);
CREATE INDEX IF NOT EXISTS idx_runs_start ON runs(started_at);
CREATE TABLE IF NOT EXISTS results (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id      INTEGER NOT NULL,
  case_id     INTEGER NOT NULL,
  name        TEXT NOT NULL,
  kind        TEXT NOT NULL,
  status      TEXT NOT NULL,
  duration_ms INTEGER NOT NULL DEFAULT 0,
  log         TEXT NOT NULL DEFAULT '',
  assertions  TEXT NOT NULL DEFAULT '[]',
  error       TEXT NOT NULL DEFAULT '',
  started_at  INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_results_run  ON results(run_id);
CREATE INDEX IF NOT EXISTS idx_results_case ON results(case_id);
CREATE TABLE IF NOT EXISTS schedules (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  suite_id     INTEGER NOT NULL UNIQUE,
  interval_min INTEGER NOT NULL DEFAULT 60,
  enabled      INTEGER NOT NULL DEFAULT 1,
  last_run_at  INTEGER,
  created_at   INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS activity (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  kind       TEXT NOT NULL,
  text       TEXT NOT NULL,
  ref_id     TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
"#;

/// Một test case đầy đủ — đơn vị mà runner thực thi.
#[derive(Debug, Clone)]
pub struct CaseRow {
    pub id: i64,
    pub suite_id: i64,
    pub name: String,
    pub kind: String,
    pub position: i64,
    pub enabled: bool,
    pub timeout_ms: i64,
    /// JSON text — cấu trúc tuỳ kind (http: method/url/…, script: command/…, web: steps).
    pub config: String,
    /// JSON array text các assertion.
    pub assertions: String,
    /// JSON array text các rule trích biến.
    pub extract: String,
}

impl CaseRow {
    pub fn to_value(&self) -> Value {
        json!({
            "id": self.id,
            "suite_id": self.suite_id,
            "name": self.name,
            "kind": self.kind,
            "position": self.position,
            "enabled": self.enabled,
            "timeout_ms": self.timeout_ms,
            "config": serde_json::from_str::<Value>(&self.config).unwrap_or(json!({})),
            "assertions": serde_json::from_str::<Value>(&self.assertions).unwrap_or(json!([])),
            "extract": serde_json::from_str::<Value>(&self.extract).unwrap_or(json!([])),
        })
    }
}

fn map_case(r: &rusqlite::Row<'_>) -> rusqlite::Result<CaseRow> {
    Ok(CaseRow {
        id: r.get(0)?,
        suite_id: r.get(1)?,
        name: r.get(2)?,
        kind: r.get(3)?,
        position: r.get(4)?,
        enabled: r.get::<_, i64>(5)? != 0,
        timeout_ms: r.get(6)?,
        config: r.get(7)?,
        assertions: r.get(8)?,
        extract: r.get(9)?,
    })
}

const CASE_COLS: &str =
    "id, suite_id, name, kind, position, enabled, timeout_ms, config, assertions, extract";

impl Db {
    pub fn open_default() -> Result<Self> {
        let dir = std::env::var("SENCLAW_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                PathBuf::from(home)
                    .join(".senclaw")
                    .join("apps")
                    .join("autotest")
            });
        std::fs::create_dir_all(&dir).ok();
        Self::open(dir.join("autotest.db"))
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

    // ---- settings ----

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

    pub fn set_setting(&self, key: &str, value: &str) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO settings(key,value) VALUES(?1,?2)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        );
    }

    // ---- suites ----

    pub fn add_suite(&self, name: &str, description: &str, env_id: Option<i64>) -> Result<i64> {
        let name = name.trim();
        if name.is_empty() {
            return Err(anyhow!("tên suite không được rỗng"));
        }
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO suites(name,description,env_id,created_at,updated_at) VALUES(?1,?2,?3,?4,?4)",
            params![name, description, env_id, now()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Suite + số case + lần chạy gần nhất (status/thời điểm) — đủ cho list UI.
    pub fn list_suites(&self, include_archived: bool) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "SELECT s.id, s.name, s.description, s.env_id, s.status, s.created_at,
                    (SELECT COUNT(*) FROM cases c WHERE c.suite_id = s.id),
                    (SELECT COUNT(*) FROM cases c WHERE c.suite_id = s.id AND c.enabled = 1),
                    (SELECT r.status FROM runs r WHERE r.suite_id = s.id ORDER BY r.id DESC LIMIT 1),
                    (SELECT r.started_at FROM runs r WHERE r.suite_id = s.id ORDER BY r.id DESC LIMIT 1)
             FROM suites s {} ORDER BY s.id DESC",
            if include_archived { "" } else { "WHERE s.status != 'archived'" }
        );
        let mut stmt = match conn.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        let rows = stmt.query_map([], |r| {
            Ok(json!({
                "id": r.get::<_, i64>(0)?,
                "name": r.get::<_, String>(1)?,
                "description": r.get::<_, String>(2)?,
                "env_id": r.get::<_, Option<i64>>(3)?,
                "status": r.get::<_, String>(4)?,
                "created_at": r.get::<_, i64>(5)?,
                "case_count": r.get::<_, i64>(6)?,
                "enabled_count": r.get::<_, i64>(7)?,
                "last_run_status": r.get::<_, Option<String>>(8)?,
                "last_run_at": r.get::<_, Option<i64>>(9)?,
            }))
        });
        rows.map(|it| it.filter_map(|x| x.ok()).collect())
            .unwrap_or_default()
    }

    pub fn get_suite(&self, id: i64) -> Option<Value> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, name, description, env_id, status, created_at FROM suites WHERE id=?1",
            params![id],
            |r| {
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "name": r.get::<_, String>(1)?,
                    "description": r.get::<_, String>(2)?,
                    "env_id": r.get::<_, Option<i64>>(3)?,
                    "status": r.get::<_, String>(4)?,
                    "created_at": r.get::<_, i64>(5)?,
                }))
            },
        )
        .optional()
        .ok()
        .flatten()
    }

    pub fn update_suite(
        &self,
        id: i64,
        name: Option<&str>,
        description: Option<&str>,
        env_id: Option<Option<i64>>,
        status: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        if let Some(n) = name {
            conn.execute(
                "UPDATE suites SET name=?2, updated_at=?3 WHERE id=?1",
                params![id, n.trim(), now()],
            )?;
        }
        if let Some(d) = description {
            conn.execute(
                "UPDATE suites SET description=?2, updated_at=?3 WHERE id=?1",
                params![id, d, now()],
            )?;
        }
        if let Some(e) = env_id {
            conn.execute(
                "UPDATE suites SET env_id=?2, updated_at=?3 WHERE id=?1",
                params![id, e, now()],
            )?;
        }
        if let Some(s) = status {
            conn.execute(
                "UPDATE suites SET status=?2, updated_at=?3 WHERE id=?1",
                params![id, s, now()],
            )?;
        }
        Ok(())
    }

    /// Xoá suite + toàn bộ case, schedule, run, result thuộc nó.
    pub fn delete_suite(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM results WHERE run_id IN (SELECT id FROM runs WHERE suite_id=?1)",
            params![id],
        )?;
        conn.execute("DELETE FROM runs WHERE suite_id=?1", params![id])?;
        conn.execute("DELETE FROM schedules WHERE suite_id=?1", params![id])?;
        conn.execute("DELETE FROM cases WHERE suite_id=?1", params![id])?;
        conn.execute("DELETE FROM suites WHERE id=?1", params![id])?;
        Ok(())
    }

    // ---- cases ----

    #[allow(clippy::too_many_arguments)]
    pub fn add_case(
        &self,
        suite_id: i64,
        name: &str,
        kind: &str,
        position: Option<i64>,
        enabled: bool,
        timeout_ms: i64,
        config: &str,
        assertions: &str,
        extract: &str,
    ) -> Result<i64> {
        let name = name.trim();
        if name.is_empty() {
            return Err(anyhow!("tên case không được rỗng"));
        }
        if !matches!(kind, "http" | "script" | "web") {
            return Err(anyhow!("kind phải là http | script | web"));
        }
        // Validate JSON ngay lúc ghi — case hỏng cấu trúc sẽ hỏng khó hiểu lúc chạy.
        serde_json::from_str::<Value>(config)
            .map_err(|e| anyhow!("config không phải JSON: {e}"))?;
        let asserts: Value = serde_json::from_str(assertions)
            .map_err(|e| anyhow!("assertions không phải JSON: {e}"))?;
        if !asserts.is_array() {
            return Err(anyhow!("assertions phải là mảng JSON"));
        }
        let ext: Value =
            serde_json::from_str(extract).map_err(|e| anyhow!("extract không phải JSON: {e}"))?;
        if !ext.is_array() {
            return Err(anyhow!("extract phải là mảng JSON"));
        }
        let conn = self.conn.lock().unwrap();
        let pos = match position {
            Some(p) => p,
            None => conn
                .query_row(
                    "SELECT COALESCE(MAX(position),0)+1 FROM cases WHERE suite_id=?1",
                    params![suite_id],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap_or(1),
        };
        conn.execute(
            "INSERT INTO cases(suite_id,name,kind,position,enabled,timeout_ms,config,assertions,extract,created_at,updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?10)",
            params![suite_id, name, kind, pos, enabled as i64, timeout_ms, config, assertions, extract, now()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_cases(&self, suite_id: i64) -> Vec<CaseRow> {
        let conn = self.conn.lock().unwrap();
        let sql = format!("SELECT {CASE_COLS} FROM cases WHERE suite_id=?1 ORDER BY position, id");
        let mut stmt = match conn.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        stmt.query_map(params![suite_id], map_case)
            .map(|it| it.filter_map(|x| x.ok()).collect())
            .unwrap_or_default()
    }

    pub fn get_case(&self, id: i64) -> Option<CaseRow> {
        let conn = self.conn.lock().unwrap();
        let sql = format!("SELECT {CASE_COLS} FROM cases WHERE id=?1");
        conn.query_row(&sql, params![id], map_case)
            .optional()
            .ok()
            .flatten()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_case(
        &self,
        id: i64,
        name: Option<&str>,
        kind: Option<&str>,
        position: Option<i64>,
        enabled: Option<bool>,
        timeout_ms: Option<i64>,
        config: Option<&str>,
        assertions: Option<&str>,
        extract: Option<&str>,
    ) -> Result<()> {
        if let Some(k) = kind {
            if !matches!(k, "http" | "script" | "web") {
                return Err(anyhow!("kind phải là http | script | web"));
            }
        }
        for (label, v) in [
            ("config", config),
            ("assertions", assertions),
            ("extract", extract),
        ] {
            if let Some(s) = v {
                serde_json::from_str::<Value>(s)
                    .map_err(|e| anyhow!("{label} không phải JSON: {e}"))?;
            }
        }
        let conn = self.conn.lock().unwrap();
        let set = |sql: &str, val: &dyn rusqlite::ToSql| {
            conn.execute(sql, params![id, val, now()]).map(|_| ())
        };
        if let Some(v) = name {
            set(
                "UPDATE cases SET name=?2, updated_at=?3 WHERE id=?1",
                &v.trim(),
            )?;
        }
        if let Some(v) = kind {
            set("UPDATE cases SET kind=?2, updated_at=?3 WHERE id=?1", &v)?;
        }
        if let Some(v) = position {
            set(
                "UPDATE cases SET position=?2, updated_at=?3 WHERE id=?1",
                &v,
            )?;
        }
        if let Some(v) = enabled {
            set(
                "UPDATE cases SET enabled=?2, updated_at=?3 WHERE id=?1",
                &(v as i64),
            )?;
        }
        if let Some(v) = timeout_ms {
            set(
                "UPDATE cases SET timeout_ms=?2, updated_at=?3 WHERE id=?1",
                &v,
            )?;
        }
        if let Some(v) = config {
            set("UPDATE cases SET config=?2, updated_at=?3 WHERE id=?1", &v)?;
        }
        if let Some(v) = assertions {
            set(
                "UPDATE cases SET assertions=?2, updated_at=?3 WHERE id=?1",
                &v,
            )?;
        }
        if let Some(v) = extract {
            set("UPDATE cases SET extract=?2, updated_at=?3 WHERE id=?1", &v)?;
        }
        Ok(())
    }

    pub fn delete_case(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM cases WHERE id=?1", params![id])?;
        Ok(())
    }

    // ---- environments ----

    /// Upsert theo tên; vars là JSON object text.
    pub fn env_set(&self, name: &str, vars: &str) -> Result<i64> {
        let name = name.trim();
        if name.is_empty() {
            return Err(anyhow!("tên environment không được rỗng"));
        }
        let parsed: Value =
            serde_json::from_str(vars).map_err(|e| anyhow!("vars không phải JSON: {e}"))?;
        if !parsed.is_object() {
            return Err(anyhow!("vars phải là JSON object {{\"key\":\"value\"}}"));
        }
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO environments(name,vars,created_at) VALUES(?1,?2,?3)
             ON CONFLICT(name) DO UPDATE SET vars=excluded.vars",
            params![name, vars, now()],
        )?;
        let id = conn.query_row(
            "SELECT id FROM environments WHERE name=?1",
            params![name],
            |r| r.get(0),
        )?;
        Ok(id)
    }

    pub fn env_list(&self) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn
            .prepare("SELECT id, name, vars, created_at FROM environments ORDER BY name")
        {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        stmt.query_map([], |r| {
            let vars: String = r.get(2)?;
            Ok(json!({
                "id": r.get::<_, i64>(0)?,
                "name": r.get::<_, String>(1)?,
                "vars": serde_json::from_str::<Value>(&vars).unwrap_or(json!({})),
                "created_at": r.get::<_, i64>(3)?,
            }))
        })
        .map(|it| it.filter_map(|x| x.ok()).collect())
        .unwrap_or_default()
    }

    /// `(name, vars-json-text)`.
    pub fn env_get(&self, id: i64) -> Option<(String, String)> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT name, vars FROM environments WHERE id=?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .ok()
        .flatten()
    }

    pub fn env_delete(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE suites SET env_id=NULL WHERE env_id=?1", params![id])?;
        conn.execute("DELETE FROM environments WHERE id=?1", params![id])?;
        Ok(())
    }

    // ---- runs & results ----

    pub fn create_run(
        &self,
        suite_id: Option<i64>,
        case_id: Option<i64>,
        env_id: Option<i64>,
        trigger_kind: &str,
    ) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO runs(suite_id,case_id,env_id,trigger_kind,status,started_at) VALUES(?1,?2,?3,?4,'running',?5)",
            params![suite_id, case_id, env_id, trigger_kind, now()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn finish_run(&self, id: i64, status: &str) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "UPDATE runs SET status=?2, finished_at=?3,
               total   = (SELECT COUNT(*) FROM results WHERE run_id=?1),
               passed  = (SELECT COUNT(*) FROM results WHERE run_id=?1 AND status='pass'),
               failed  = (SELECT COUNT(*) FROM results WHERE run_id=?1 AND status='fail'),
               errors  = (SELECT COUNT(*) FROM results WHERE run_id=?1 AND status='error'),
               skipped = (SELECT COUNT(*) FROM results WHERE run_id=?1 AND status='skipped')
             WHERE id=?1",
            params![id, status, now()],
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_result(
        &self,
        run_id: i64,
        case_id: i64,
        name: &str,
        kind: &str,
        status: &str,
        duration_ms: i64,
        log: &str,
        assertions: &str,
        error: &str,
    ) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO results(run_id,case_id,name,kind,status,duration_ms,log,assertions,error,started_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![run_id, case_id, name, kind, status, duration_ms, log, assertions, error, now()],
        );
    }

    pub fn list_runs(&self, suite_id: Option<i64>, limit: i64) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let sql = "SELECT r.id, r.suite_id, r.case_id, r.env_id, r.trigger_kind, r.status,
                          r.started_at, r.finished_at, r.total, r.passed, r.failed, r.errors, r.skipped,
                          COALESCE((SELECT c.name FROM cases c WHERE c.id = r.case_id), s.name, '')
                   FROM runs r LEFT JOIN suites s ON s.id = r.suite_id
                   WHERE (?1 IS NULL OR r.suite_id = ?1)
                   ORDER BY r.id DESC LIMIT ?2";
        let mut stmt = match conn.prepare(sql) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        stmt.query_map(params![suite_id, limit.clamp(1, 500)], |r| {
            Ok(json!({
                "id": r.get::<_, i64>(0)?,
                "suite_id": r.get::<_, Option<i64>>(1)?,
                "case_id": r.get::<_, Option<i64>>(2)?,
                "env_id": r.get::<_, Option<i64>>(3)?,
                "trigger": r.get::<_, String>(4)?,
                "status": r.get::<_, String>(5)?,
                "started_at": r.get::<_, i64>(6)?,
                "finished_at": r.get::<_, Option<i64>>(7)?,
                "total": r.get::<_, i64>(8)?,
                "passed": r.get::<_, i64>(9)?,
                "failed": r.get::<_, i64>(10)?,
                "errors": r.get::<_, i64>(11)?,
                "skipped": r.get::<_, i64>(12)?,
                "target": r.get::<_, String>(13)?,
            }))
        })
        .map(|it| it.filter_map(|x| x.ok()).collect())
        .unwrap_or_default()
    }

    /// Run + toàn bộ results (assertion đã parse) — màn chi tiết & AI chẩn đoán.
    pub fn get_run(&self, id: i64) -> Option<Value> {
        let run = {
            let conn = self.conn.lock().unwrap();
            conn.query_row(
                "SELECT r.id, r.suite_id, r.case_id, r.env_id, r.trigger_kind, r.status,
                        r.started_at, r.finished_at, r.total, r.passed, r.failed, r.errors, r.skipped,
                        COALESCE((SELECT c.name FROM cases c WHERE c.id = r.case_id), s.name, '')
                 FROM runs r LEFT JOIN suites s ON s.id = r.suite_id WHERE r.id=?1",
                params![id],
                |r| {
                    Ok(json!({
                        "id": r.get::<_, i64>(0)?,
                        "suite_id": r.get::<_, Option<i64>>(1)?,
                        "case_id": r.get::<_, Option<i64>>(2)?,
                        "env_id": r.get::<_, Option<i64>>(3)?,
                        "trigger": r.get::<_, String>(4)?,
                        "status": r.get::<_, String>(5)?,
                        "started_at": r.get::<_, i64>(6)?,
                        "finished_at": r.get::<_, Option<i64>>(7)?,
                        "total": r.get::<_, i64>(8)?,
                        "passed": r.get::<_, i64>(9)?,
                        "failed": r.get::<_, i64>(10)?,
                        "errors": r.get::<_, i64>(11)?,
                        "skipped": r.get::<_, i64>(12)?,
                        "target": r.get::<_, String>(13)?,
                    }))
                },
            )
            .optional()
            .ok()
            .flatten()
        };
        let mut run = run?;
        run["results"] = Value::Array(self.list_results(id));
        Some(run)
    }

    pub fn list_results(&self, run_id: i64) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT id, case_id, name, kind, status, duration_ms, log, assertions, error, started_at
             FROM results WHERE run_id=?1 ORDER BY id",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        stmt.query_map(params![run_id], |r| {
            let asserts: String = r.get(7)?;
            Ok(json!({
                "id": r.get::<_, i64>(0)?,
                "case_id": r.get::<_, i64>(1)?,
                "name": r.get::<_, String>(2)?,
                "kind": r.get::<_, String>(3)?,
                "status": r.get::<_, String>(4)?,
                "duration_ms": r.get::<_, i64>(5)?,
                "log": r.get::<_, String>(6)?,
                "assertions": serde_json::from_str::<Value>(&asserts).unwrap_or(json!([])),
                "error": r.get::<_, String>(8)?,
                "started_at": r.get::<_, i64>(9)?,
            }))
        })
        .map(|it| it.filter_map(|x| x.ok()).collect())
        .unwrap_or_default()
    }

    /// Đánh dấu các run 'running' bị bỏ dở (app restart giữa chừng) thành error.
    pub fn reap_stale_runs(&self) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "UPDATE runs SET status='error', finished_at=?1 WHERE status='running' AND started_at < ?1 - 3600",
            params![now()],
        );
    }

    // ---- schedules ----

    pub fn schedule_set(&self, suite_id: i64, interval_min: i64, enabled: bool) -> Result<()> {
        if interval_min < 1 {
            return Err(anyhow!("interval_min phải ≥ 1"));
        }
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO schedules(suite_id,interval_min,enabled,created_at) VALUES(?1,?2,?3,?4)
             ON CONFLICT(suite_id) DO UPDATE SET interval_min=excluded.interval_min, enabled=excluded.enabled",
            params![suite_id, interval_min, enabled as i64, now()],
        )?;
        Ok(())
    }

    pub fn schedule_list(&self) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT sc.id, sc.suite_id, s.name, sc.interval_min, sc.enabled, sc.last_run_at
             FROM schedules sc JOIN suites s ON s.id = sc.suite_id ORDER BY sc.id",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        stmt.query_map([], |r| {
            Ok(json!({
                "id": r.get::<_, i64>(0)?,
                "suite_id": r.get::<_, i64>(1)?,
                "suite_name": r.get::<_, String>(2)?,
                "interval_min": r.get::<_, i64>(3)?,
                "enabled": r.get::<_, i64>(4)? != 0,
                "last_run_at": r.get::<_, Option<i64>>(5)?,
            }))
        })
        .map(|it| it.filter_map(|x| x.ok()).collect())
        .unwrap_or_default()
    }

    pub fn schedule_delete(&self, suite_id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM schedules WHERE suite_id=?1", params![suite_id])?;
        Ok(())
    }

    /// Các suite đến hạn chạy tại thời điểm `at` (đã bật, đủ interval từ lần trước).
    pub fn due_schedules(&self, at: i64) -> Vec<i64> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT sc.suite_id FROM schedules sc JOIN suites s ON s.id = sc.suite_id
             WHERE sc.enabled=1 AND s.status='active'
               AND (sc.last_run_at IS NULL OR sc.last_run_at + sc.interval_min*60 <= ?1)",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        stmt.query_map(params![at], |r| r.get::<_, i64>(0))
            .map(|it| it.filter_map(|x| x.ok()).collect())
            .unwrap_or_default()
    }

    pub fn schedule_touch(&self, suite_id: i64, at: i64) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "UPDATE schedules SET last_run_at=?2 WHERE suite_id=?1",
            params![suite_id, at],
        );
    }

    // ---- báo cáo / thống kê ----

    /// Case flaky: trong ≤`window` kết quả gần nhất có ≥2 lần đổi trạng thái
    /// pass↔fail (bỏ qua skipped). Test lúc pass lúc fail nguy hiểm hơn test
    /// fail hẳn — nó bào mòn niềm tin vào cả bộ kiểm thử.
    pub fn flaky_cases(&self, window: i64) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT c.id, c.name, c.suite_id, s.name FROM cases c JOIN suites s ON s.id = c.suite_id",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        let cases: Vec<(i64, String, i64, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .map(|it| it.filter_map(|x| x.ok()).collect())
            .unwrap_or_default();
        let mut out = vec![];
        for (case_id, name, suite_id, suite_name) in cases {
            let mut stmt = match conn.prepare(
                "SELECT status FROM results WHERE case_id=?1 AND status IN ('pass','fail','error')
                 ORDER BY id DESC LIMIT ?2",
            ) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let statuses: Vec<String> = stmt
                .query_map(params![case_id, window], |r| r.get(0))
                .map(|it| it.filter_map(|x| x.ok()).collect())
                .unwrap_or_default();
            let flips = statuses
                .windows(2)
                .filter(|w| (w[0] == "pass") != (w[1] == "pass"))
                .count();
            if flips >= 2 {
                let passes = statuses.iter().filter(|s| *s == "pass").count();
                out.push(json!({
                    "case_id": case_id,
                    "name": name,
                    "suite_id": suite_id,
                    "suite_name": suite_name,
                    "recent": statuses,
                    "flips": flips,
                    "pass_rate": if statuses.is_empty() { 0.0 } else { passes as f64 / statuses.len() as f64 },
                }));
            }
        }
        out.sort_by(|a, b| b["flips"].as_u64().cmp(&a["flips"].as_u64()));
        out
    }

    /// Case fail/error nhiều nhất trong `days` ngày gần đây.
    pub fn top_failing(&self, days: i64, limit: i64) -> Vec<Value> {
        let since = now() - days * 86400;
        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT r.case_id, r.name, COUNT(*) AS fails,
                    (SELECT COUNT(*) FROM results r2 WHERE r2.case_id = r.case_id AND r2.started_at > ?1)
             FROM results r
             WHERE r.status IN ('fail','error') AND r.started_at > ?1
             GROUP BY r.case_id ORDER BY fails DESC LIMIT ?2",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        stmt.query_map(params![since, limit], |r| {
            Ok(json!({
                "case_id": r.get::<_, i64>(0)?,
                "name": r.get::<_, String>(1)?,
                "fail_count": r.get::<_, i64>(2)?,
                "total_count": r.get::<_, i64>(3)?,
            }))
        })
        .map(|it| it.filter_map(|x| x.ok()).collect())
        .unwrap_or_default()
    }

    /// Xu hướng pass của `limit` run hoàn tất gần nhất (cũ → mới) cho chart.
    pub fn pass_trend(&self, suite_id: Option<i64>, limit: i64) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT id, suite_id, status, started_at, total, passed, failed, errors
             FROM runs WHERE status != 'running' AND (?1 IS NULL OR suite_id = ?1)
             ORDER BY id DESC LIMIT ?2",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        let mut rows: Vec<Value> = stmt
            .query_map(params![suite_id, limit], |r| {
                Ok(json!({
                    "run_id": r.get::<_, i64>(0)?,
                    "suite_id": r.get::<_, Option<i64>>(1)?,
                    "status": r.get::<_, String>(2)?,
                    "started_at": r.get::<_, i64>(3)?,
                    "total": r.get::<_, i64>(4)?,
                    "passed": r.get::<_, i64>(5)?,
                    "failed": r.get::<_, i64>(6)?,
                    "errors": r.get::<_, i64>(7)?,
                }))
            })
            .map(|it| it.filter_map(|x| x.ok()).collect())
            .unwrap_or_default();
        rows.reverse();
        rows
    }

    pub fn dashboard(&self) -> Value {
        let (suites, cases, envs, schedules_on): (i64, i64, i64, i64) = {
            let conn = self.conn.lock().unwrap();
            (
                conn.query_row(
                    "SELECT COUNT(*) FROM suites WHERE status='active'",
                    [],
                    |r| r.get(0),
                )
                .unwrap_or(0),
                conn.query_row("SELECT COUNT(*) FROM cases", [], |r| r.get(0))
                    .unwrap_or(0),
                conn.query_row("SELECT COUNT(*) FROM environments", [], |r| r.get(0))
                    .unwrap_or(0),
                conn.query_row("SELECT COUNT(*) FROM schedules WHERE enabled=1", [], |r| {
                    r.get(0)
                })
                .unwrap_or(0),
            )
        };
        let day_start = now() - now() % 86400;
        let (runs_today, running): (i64, i64) = {
            let conn = self.conn.lock().unwrap();
            (
                conn.query_row(
                    "SELECT COUNT(*) FROM runs WHERE started_at >= ?1",
                    params![day_start],
                    |r| r.get(0),
                )
                .unwrap_or(0),
                conn.query_row(
                    "SELECT COUNT(*) FROM runs WHERE status='running'",
                    [],
                    |r| r.get(0),
                )
                .unwrap_or(0),
            )
        };
        let trend = self.pass_trend(None, 20);
        let finished: Vec<&Value> = trend
            .iter()
            .filter(|r| r["status"] != "cancelled")
            .collect();
        let pass_rate = if finished.is_empty() {
            Value::Null
        } else {
            let ok = finished.iter().filter(|r| r["status"] == "pass").count();
            json!(ok as f64 / finished.len() as f64)
        };
        json!({
            "suites": suites,
            "cases": cases,
            "environments": envs,
            "schedules_enabled": schedules_on,
            "runs_today": runs_today,
            "running": running,
            "pass_rate_recent": pass_rate,
            "trend": trend,
            "recent_runs": self.list_runs(None, 10),
            "flaky": self.flaky_cases(10),
            "top_failing": self.top_failing(30, 10),
        })
    }

    // ---- activity ----

    pub fn log(&self, kind: &str, text: &str, ref_id: &str) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO activity(kind,text,ref_id,created_at) VALUES(?1,?2,?3,?4)",
            params![kind, text, ref_id, now()],
        );
    }

    pub fn activity(&self, limit: i64) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT id, kind, text, ref_id, created_at FROM activity ORDER BY id DESC LIMIT ?1",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        stmt.query_map(params![limit.clamp(1, 500)], |r| {
            Ok(json!({
                "id": r.get::<_, i64>(0)?,
                "kind": r.get::<_, String>(1)?,
                "text": r.get::<_, String>(2)?,
                "ref": r.get::<_, String>(3)?,
                "created_at": r.get::<_, i64>(4)?,
            }))
        })
        .map(|it| it.filter_map(|x| x.ok()).collect())
        .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(db: &Db) -> (i64, i64) {
        let sid = db.add_suite("API chính", "smoke", None).unwrap();
        let cid = db
            .add_case(
                sid,
                "GET /health",
                "http",
                None,
                true,
                5000,
                r#"{"method":"GET","url":"{{base_url}}/health"}"#,
                r#"[{"type":"status","value":200}]"#,
                "[]",
            )
            .unwrap();
        (sid, cid)
    }

    #[test]
    fn suite_case_crud() {
        let db = Db::open_memory().unwrap();
        let (sid, cid) = seed(&db);
        assert_eq!(db.list_suites(false).len(), 1);
        assert_eq!(db.list_cases(sid).len(), 1);
        db.update_case(
            cid,
            Some("GET /healthz"),
            None,
            None,
            Some(false),
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let c = db.get_case(cid).unwrap();
        assert_eq!(c.name, "GET /healthz");
        assert!(!c.enabled);
        db.update_suite(sid, None, None, None, Some("archived"))
            .unwrap();
        assert!(db.list_suites(false).is_empty());
        assert_eq!(db.list_suites(true).len(), 1);
        db.delete_suite(sid).unwrap();
        assert!(db.get_case(cid).is_none());
    }

    #[test]
    fn add_case_validates_json_and_kind() {
        let db = Db::open_memory().unwrap();
        let sid = db.add_suite("s", "", None).unwrap();
        assert!(db
            .add_case(sid, "x", "ftp", None, true, 1000, "{}", "[]", "[]")
            .is_err());
        assert!(db
            .add_case(sid, "x", "http", None, true, 1000, "not-json", "[]", "[]")
            .is_err());
        assert!(db
            .add_case(sid, "x", "http", None, true, 1000, "{}", "{}", "[]")
            .is_err());
        assert!(db
            .add_case(sid, "", "http", None, true, 1000, "{}", "[]", "[]")
            .is_err());
    }

    #[test]
    fn env_upsert_and_delete() {
        let db = Db::open_memory().unwrap();
        let id1 = db.env_set("staging", r#"{"base_url":"http://s"}"#).unwrap();
        let id2 = db
            .env_set("staging", r#"{"base_url":"http://s2"}"#)
            .unwrap();
        assert_eq!(id1, id2);
        assert!(db.env_set("x", "[]").is_err());
        let (_, vars) = db.env_get(id1).unwrap();
        assert!(vars.contains("s2"));
        db.env_delete(id1).unwrap();
        assert!(db.env_get(id1).is_none());
    }

    #[test]
    fn run_lifecycle_counts() {
        let db = Db::open_memory().unwrap();
        let (sid, cid) = seed(&db);
        let rid = db.create_run(Some(sid), None, None, "manual").unwrap();
        db.add_result(rid, cid, "GET /health", "http", "pass", 42, "log", "[]", "");
        db.add_result(rid, cid, "GET /health", "http", "fail", 10, "", "[]", "");
        db.finish_run(rid, "fail");
        let run = db.get_run(rid).unwrap();
        assert_eq!(run["total"], 2);
        assert_eq!(run["passed"], 1);
        assert_eq!(run["failed"], 1);
        assert_eq!(run["results"].as_array().unwrap().len(), 2);
        assert_eq!(db.list_runs(Some(sid), 10).len(), 1);
    }

    #[test]
    fn schedule_due_logic() {
        let db = Db::open_memory().unwrap();
        let (sid, _) = seed(&db);
        db.schedule_set(sid, 30, true).unwrap();
        assert!(db.schedule_set(sid, 0, true).is_err());
        let t = now();
        assert_eq!(db.due_schedules(t), vec![sid]);
        db.schedule_touch(sid, t);
        assert!(db.due_schedules(t + 29 * 60).is_empty());
        assert_eq!(db.due_schedules(t + 30 * 60), vec![sid]);
        db.schedule_set(sid, 30, false).unwrap();
        assert!(db.due_schedules(t + 31 * 60).is_empty());
    }

    #[test]
    fn flaky_detection() {
        let db = Db::open_memory().unwrap();
        let (sid, cid) = seed(&db);
        // pass→fail→pass = 2 flips → flaky.
        for st in ["pass", "fail", "pass"] {
            let rid = db.create_run(Some(sid), None, None, "manual").unwrap();
            db.add_result(rid, cid, "c", "http", st, 1, "", "[]", "");
            db.finish_run(rid, st);
        }
        let flaky = db.flaky_cases(10);
        assert_eq!(flaky.len(), 1);
        assert_eq!(flaky[0]["case_id"], cid);
        // Case fail hẳn thì KHÔNG flaky.
        let cid2 = db
            .add_case(sid, "c2", "http", None, true, 1000, "{}", "[]", "[]")
            .unwrap();
        for _ in 0..3 {
            let rid = db.create_run(Some(sid), None, None, "manual").unwrap();
            db.add_result(rid, cid2, "c2", "http", "fail", 1, "", "[]", "");
            db.finish_run(rid, "fail");
        }
        assert_eq!(db.flaky_cases(10).len(), 1);
    }

    #[test]
    fn dashboard_shape() {
        let db = Db::open_memory().unwrap();
        let (sid, cid) = seed(&db);
        let rid = db.create_run(Some(sid), None, None, "manual").unwrap();
        db.add_result(rid, cid, "c", "http", "pass", 1, "", "[]", "");
        db.finish_run(rid, "pass");
        let d = db.dashboard();
        assert_eq!(d["suites"], 1);
        assert_eq!(d["cases"], 1);
        assert_eq!(d["pass_rate_recent"], 1.0);
        assert_eq!(d["recent_runs"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn top_failing_counts() {
        let db = Db::open_memory().unwrap();
        let (sid, cid) = seed(&db);
        for _ in 0..2 {
            let rid = db.create_run(Some(sid), None, None, "manual").unwrap();
            db.add_result(rid, cid, "c", "http", "fail", 1, "", "[]", "");
            db.finish_run(rid, "fail");
        }
        let top = db.top_failing(30, 5);
        assert_eq!(top.len(), 1);
        assert_eq!(top[0]["fail_count"], 2);
    }
}
