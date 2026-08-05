//! Local SQLite store cho app Terraform. Local-first — không service ngoài nào
//! giữ dữ liệu này. Tables:
//!   * `workspaces` — dự án Terraform: folder local hoặc repo git đã clone về
//!   * `runs`       — mỗi lần chạy (init/plan/apply/…/sync/clone/install): trạng thái + exit code
//!   * `run_lines`  — console output từng dòng của một run (seq tăng dần, poll bằng `after`)
//!   * `settings`   — kv (đường dẫn terraform override, …)
//!   * `activity`   — log hành động của app/agent

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

/// Thư mục dữ liệu của app: `~/.senclaw/apps/terraform` (hoặc `SENCLAW_DATA_DIR`).
pub fn data_dir() -> PathBuf {
    std::env::var("SENCLAW_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .unwrap_or_else(|_| ".".into());
            PathBuf::from(home).join(".senclaw").join("apps").join("terraform")
        })
}

/// Nơi chứa các repo git app tự clone (xoá được khi xoá workspace).
pub fn repos_dir() -> PathBuf {
    data_dir().join("repos")
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS workspaces (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  name       TEXT NOT NULL,
  source     TEXT NOT NULL DEFAULT 'folder',
  dir        TEXT NOT NULL DEFAULT '',
  repo_url   TEXT NOT NULL DEFAULT '',
  branch     TEXT NOT NULL DEFAULT '',
  var_file   TEXT NOT NULL DEFAULT '',
  auto_sync  INTEGER NOT NULL DEFAULT 1,
  status     TEXT NOT NULL DEFAULT 'ready',
  last_error TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS runs (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  workspace_id INTEGER,
  kind         TEXT NOT NULL,
  status       TEXT NOT NULL DEFAULT 'running',
  exit_code    INTEGER,
  started_at   INTEGER NOT NULL,
  finished_at  INTEGER
);
CREATE INDEX IF NOT EXISTS idx_runs_ws ON runs(workspace_id, started_at);
CREATE TABLE IF NOT EXISTS run_lines (
  id     INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id INTEGER NOT NULL,
  seq    INTEGER NOT NULL,
  stream TEXT NOT NULL DEFAULT 'out',
  line   TEXT NOT NULL,
  at     INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_lines_run ON run_lines(run_id, seq);
CREATE TABLE IF NOT EXISTS settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS activity (
  id  INTEGER PRIMARY KEY AUTOINCREMENT,
  at  INTEGER NOT NULL,
  msg TEXT NOT NULL
);
"#;

impl Db {
    pub fn open_default() -> Result<Self> {
        let dir = data_dir();
        std::fs::create_dir_all(&dir).ok();
        Self::open(dir.join("terraform.db"))
    }

    pub fn open(path: PathBuf) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch(SCHEMA)?;
        // Migration: DB tạo trước khi có cột `subdir` (root Terraform trong repo).
        let has_subdir: i64 = conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('workspaces') WHERE name='subdir'",
            [],
            |r| r.get(0),
        )?;
        if has_subdir == 0 {
            conn.execute_batch(
                "ALTER TABLE workspaces ADD COLUMN subdir TEXT NOT NULL DEFAULT ''",
            )?;
        }
        // Daemon restart giữa chừng: run "running" mồ côi → đánh fail để UI không treo.
        conn.execute(
            "UPDATE runs SET status='failed', finished_at=?1 WHERE status='running'",
            params![now()],
        )?;
        conn.execute(
            "UPDATE workspaces SET status='error', last_error='app khởi động lại giữa lúc clone' \
             WHERE status='cloning'",
            params![],
        )?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let conn = self.conn.lock().map_err(|_| anyhow!("db lock poisoned"))?;
        f(&conn)
    }

    // ---- workspaces ----

    pub fn workspace_add(
        &self,
        name: &str,
        source: &str,
        dir: &str,
        repo_url: &str,
        branch: &str,
        status: &str,
    ) -> Result<i64> {
        self.with(|c| {
            c.execute(
                "INSERT INTO workspaces(name, source, dir, repo_url, branch, status, created_at, updated_at) \
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
                params![name, source, dir, repo_url, branch, status, now()],
            )?;
            Ok(c.last_insert_rowid())
        })
    }

    fn row_to_workspace(row: &rusqlite::Row) -> rusqlite::Result<Value> {
        Ok(json!({
            "id": row.get::<_, i64>(0)?,
            "name": row.get::<_, String>(1)?,
            "source": row.get::<_, String>(2)?,
            "dir": row.get::<_, String>(3)?,
            "repo_url": row.get::<_, String>(4)?,
            "branch": row.get::<_, String>(5)?,
            "var_file": row.get::<_, String>(6)?,
            "auto_sync": row.get::<_, i64>(7)? != 0,
            "status": row.get::<_, String>(8)?,
            "last_error": row.get::<_, String>(9)?,
            "created_at": row.get::<_, i64>(10)?,
            "updated_at": row.get::<_, i64>(11)?,
            "subdir": row.get::<_, String>(12)?,
        }))
    }

    const WS_COLS: &'static str =
        "id, name, source, dir, repo_url, branch, var_file, auto_sync, status, last_error, created_at, updated_at, subdir";

    pub fn workspace_list(&self) -> Result<Vec<Value>> {
        self.with(|c| {
            let mut st = c.prepare(&format!(
                "SELECT {} FROM workspaces ORDER BY name COLLATE NOCASE",
                Self::WS_COLS
            ))?;
            let rows = st
                .query_map([], Self::row_to_workspace)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    pub fn workspace_get(&self, id: i64) -> Result<Option<Value>> {
        self.with(|c| {
            let mut st = c.prepare(&format!(
                "SELECT {} FROM workspaces WHERE id=?1",
                Self::WS_COLS
            ))?;
            Ok(st.query_row(params![id], Self::row_to_workspace).optional()?)
        })
    }

    /// Patch từng trường; chỉ trường `Some` mới đổi.
    #[allow(clippy::too_many_arguments)]
    pub fn workspace_update(
        &self,
        id: i64,
        name: Option<&str>,
        branch: Option<&str>,
        var_file: Option<&str>,
        auto_sync: Option<bool>,
        status: Option<&str>,
        last_error: Option<&str>,
        dir: Option<&str>,
        subdir: Option<&str>,
    ) -> Result<()> {
        self.with(|c| {
            let mut sets: Vec<String> = vec!["updated_at=?1".into()];
            let mut vals: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(now())];
            let mut push = |col: &str, v: Box<dyn rusqlite::ToSql>| {
                vals.push(v);
                sets.push(format!("{}=?{}", col, vals.len()));
            };
            if let Some(v) = name {
                push("name", Box::new(v.to_string()));
            }
            if let Some(v) = branch {
                push("branch", Box::new(v.to_string()));
            }
            if let Some(v) = var_file {
                push("var_file", Box::new(v.to_string()));
            }
            if let Some(v) = auto_sync {
                push("auto_sync", Box::new(v as i64));
            }
            if let Some(v) = status {
                push("status", Box::new(v.to_string()));
            }
            if let Some(v) = last_error {
                push("last_error", Box::new(v.to_string()));
            }
            if let Some(v) = dir {
                push("dir", Box::new(v.to_string()));
            }
            if let Some(v) = subdir {
                push("subdir", Box::new(v.to_string()));
            }
            vals.push(Box::new(id));
            let sql = format!(
                "UPDATE workspaces SET {} WHERE id=?{}",
                sets.join(", "),
                vals.len()
            );
            let refs: Vec<&dyn rusqlite::ToSql> = vals.iter().map(|b| b.as_ref()).collect();
            c.execute(&sql, refs.as_slice())?;
            Ok(())
        })
    }

    pub fn workspace_delete(&self, id: i64) -> Result<()> {
        self.with(|c| {
            c.execute("DELETE FROM workspaces WHERE id=?1", params![id])?;
            c.execute(
                "DELETE FROM run_lines WHERE run_id IN (SELECT id FROM runs WHERE workspace_id=?1)",
                params![id],
            )?;
            c.execute("DELETE FROM runs WHERE workspace_id=?1", params![id])?;
            Ok(())
        })
    }

    // ---- runs ----

    pub fn run_create(&self, workspace_id: Option<i64>, kind: &str) -> Result<i64> {
        self.with(|c| {
            c.execute(
                "INSERT INTO runs(workspace_id, kind, started_at) VALUES(?1, ?2, ?3)",
                params![workspace_id, kind, now()],
            )?;
            Ok(c.last_insert_rowid())
        })
    }

    pub fn run_finish(&self, id: i64, status: &str, exit_code: Option<i64>) -> Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE runs SET status=?2, exit_code=?3, finished_at=?4 WHERE id=?1",
                params![id, status, exit_code, now()],
            )?;
            Ok(())
        })
    }

    fn row_to_run(row: &rusqlite::Row) -> rusqlite::Result<Value> {
        Ok(json!({
            "id": row.get::<_, i64>(0)?,
            "workspace_id": row.get::<_, Option<i64>>(1)?,
            "kind": row.get::<_, String>(2)?,
            "status": row.get::<_, String>(3)?,
            "exit_code": row.get::<_, Option<i64>>(4)?,
            "started_at": row.get::<_, i64>(5)?,
            "finished_at": row.get::<_, Option<i64>>(6)?,
        }))
    }

    pub fn run_get(&self, id: i64) -> Result<Option<Value>> {
        self.with(|c| {
            let mut st = c.prepare(
                "SELECT id, workspace_id, kind, status, exit_code, started_at, finished_at \
                 FROM runs WHERE id=?1",
            )?;
            Ok(st.query_row(params![id], Self::row_to_run).optional()?)
        })
    }

    pub fn run_list(&self, workspace_id: Option<i64>, limit: i64) -> Result<Vec<Value>> {
        self.with(|c| {
            let sql = match workspace_id {
                Some(_) => {
                    "SELECT id, workspace_id, kind, status, exit_code, started_at, finished_at \
                     FROM runs WHERE workspace_id=?1 ORDER BY id DESC LIMIT ?2"
                }
                None => {
                    "SELECT id, workspace_id, kind, status, exit_code, started_at, finished_at \
                     FROM runs WHERE ?1 IS NULL ORDER BY id DESC LIMIT ?2"
                }
            };
            let mut st = c.prepare(sql)?;
            let rows = st
                .query_map(params![workspace_id, limit], Self::row_to_run)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    /// Run đang chạy của một workspace (mỗi workspace tối đa 1 run một lúc).
    pub fn running_run(&self, workspace_id: i64) -> Result<Option<i64>> {
        self.with(|c| {
            Ok(c.query_row(
                "SELECT id FROM runs WHERE workspace_id=?1 AND status='running' LIMIT 1",
                params![workspace_id],
                |r| r.get(0),
            )
            .optional()?)
        })
    }

    /// Có run global kind này đang chạy không (cài CLI chỉ 1 lần một lúc).
    pub fn running_kind(&self, kind: &str) -> Result<Option<i64>> {
        self.with(|c| {
            Ok(c.query_row(
                "SELECT id FROM runs WHERE kind=?1 AND status='running' LIMIT 1",
                params![kind],
                |r| r.get(0),
            )
            .optional()?)
        })
    }

    pub fn run_append(&self, run_id: i64, stream: &str, line: &str) -> Result<i64> {
        self.with(|c| {
            let seq: i64 = c.query_row(
                "SELECT COALESCE(MAX(seq), 0) + 1 FROM run_lines WHERE run_id=?1",
                params![run_id],
                |r| r.get(0),
            )?;
            c.execute(
                "INSERT INTO run_lines(run_id, seq, stream, line, at) VALUES(?1, ?2, ?3, ?4, ?5)",
                params![run_id, seq, stream, line, now()],
            )?;
            Ok(seq)
        })
    }

    /// Số dòng đã ghi của run (để cap output).
    pub fn run_line_count(&self, run_id: i64) -> Result<i64> {
        self.with(|c| {
            Ok(c.query_row(
                "SELECT COUNT(*) FROM run_lines WHERE run_id=?1",
                params![run_id],
                |r| r.get(0),
            )?)
        })
    }

    /// Các dòng seq > after, kèm seq cuối để client poll tiếp.
    pub fn run_lines_after(&self, run_id: i64, after: i64, limit: i64) -> Result<(Vec<Value>, i64)> {
        self.with(|c| {
            let mut st = c.prepare(
                "SELECT seq, stream, line, at FROM run_lines \
                 WHERE run_id=?1 AND seq>?2 ORDER BY seq LIMIT ?3",
            )?;
            let rows = st
                .query_map(params![run_id, after, limit], |r| {
                    Ok(json!({
                        "seq": r.get::<_, i64>(0)?,
                        "stream": r.get::<_, String>(1)?,
                        "line": r.get::<_, String>(2)?,
                        "at": r.get::<_, i64>(3)?,
                    }))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let last = rows
                .last()
                .and_then(|v| v["seq"].as_i64())
                .unwrap_or(after);
            Ok((rows, last))
        })
    }

    /// Đuôi output của run (cho MCP trả kết quả gọn).
    pub fn run_tail(&self, run_id: i64, lines: i64) -> Result<String> {
        self.with(|c| {
            let mut st = c.prepare(
                "SELECT line FROM run_lines WHERE run_id=?1 ORDER BY seq DESC LIMIT ?2",
            )?;
            let mut rows = st
                .query_map(params![run_id, lines], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows.reverse();
            Ok(rows.join("\n"))
        })
    }

    // ---- settings / activity ----

    pub fn setting_get(&self, key: &str) -> Result<Option<String>> {
        self.with(|c| {
            Ok(c.query_row(
                "SELECT value FROM settings WHERE key=?1",
                params![key],
                |r| r.get(0),
            )
            .optional()?)
        })
    }

    pub fn setting_set(&self, key: &str, value: &str) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO settings(key, value) VALUES(?1, ?2) \
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                params![key, value],
            )?;
            Ok(())
        })
    }

    pub fn log(&self, msg: &str) {
        let _ = self.with(|c| {
            c.execute(
                "INSERT INTO activity(at, msg) VALUES(?1, ?2)",
                params![now(), msg],
            )?;
            Ok(())
        });
    }

    pub fn activity(&self, limit: i64) -> Result<Vec<Value>> {
        self.with(|c| {
            let mut st =
                c.prepare("SELECT at, msg FROM activity ORDER BY id DESC LIMIT ?1")?;
            let rows = st
                .query_map(params![limit], |r| {
                    Ok(json!({ "at": r.get::<_, i64>(0)?, "msg": r.get::<_, String>(1)? }))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_db() -> Db {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("t.db")).unwrap();
        // tempdir bị drop thì file mất — giữ db mở trong RAM đủ cho test vì
        // Connection giữ fd; nhưng an toàn nhất là leak tempdir.
        std::mem::forget(dir);
        db
    }

    #[test]
    fn workspace_crud_roundtrip() {
        let db = mem_db();
        let id = db
            .workspace_add("demo", "git", "/tmp/x", "https://x/y.git", "main", "cloning")
            .unwrap();
        let ws = db.workspace_get(id).unwrap().unwrap();
        assert_eq!(ws["source"], "git");
        assert_eq!(ws["status"], "cloning");
        assert_eq!(ws["auto_sync"], true);

        db.workspace_update(id, None, None, Some("prod.tfvars"), Some(false), Some("ready"), None, None, None)
            .unwrap();
        let ws = db.workspace_get(id).unwrap().unwrap();
        assert_eq!(ws["var_file"], "prod.tfvars");
        assert_eq!(ws["auto_sync"], false);
        assert_eq!(ws["status"], "ready");
        assert_eq!(ws["subdir"], "");

        db.workspace_update(id, None, None, None, None, None, None, None, Some("infra/prod"))
            .unwrap();
        let ws = db.workspace_get(id).unwrap().unwrap();
        assert_eq!(ws["subdir"], "infra/prod");

        assert_eq!(db.workspace_list().unwrap().len(), 1);
        db.workspace_delete(id).unwrap();
        assert!(db.workspace_get(id).unwrap().is_none());
    }

    #[test]
    fn run_lines_seq_and_poll() {
        let db = mem_db();
        let ws = db.workspace_add("w", "folder", "/tmp", "", "", "ready").unwrap();
        let run = db.run_create(Some(ws), "plan").unwrap();
        assert_eq!(db.running_run(ws).unwrap(), Some(run));

        db.run_append(run, "sys", "$ terraform plan").unwrap();
        db.run_append(run, "out", "No changes.").unwrap();
        let (lines, last) = db.run_lines_after(run, 0, 100).unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(last, 2);
        let (lines, last) = db.run_lines_after(run, last, 100).unwrap();
        assert!(lines.is_empty());
        assert_eq!(last, 2);

        db.run_finish(run, "success", Some(0)).unwrap();
        assert_eq!(db.running_run(ws).unwrap(), None);
        let r = db.run_get(run).unwrap().unwrap();
        assert_eq!(r["status"], "success");
        assert_eq!(db.run_tail(run, 10).unwrap(), "$ terraform plan\nNo changes.");
    }

    #[test]
    fn stale_running_runs_fail_on_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        {
            let db = Db::open(path.clone()).unwrap();
            let ws = db.workspace_add("w", "folder", "/tmp", "", "", "ready").unwrap();
            db.run_create(Some(ws), "apply").unwrap();
        }
        let db = Db::open(path).unwrap();
        let runs = db.run_list(None, 10).unwrap();
        assert_eq!(runs[0]["status"], "failed");
    }
}
