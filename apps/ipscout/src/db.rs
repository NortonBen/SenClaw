//! SQLite cho ipscout. Năm bảng: `projects` (nhóm công việc), `targets` (mục
//! tiêu + trạng thái xác minh sở hữu), `runs` (mỗi lần điều tra là một **ảnh
//! chụp**), `ports` và `findings` (nội dung của ảnh chụp đó).
//!
//! Ảnh chụp chứ không phải trạng thái hiện tại: đó là điểm khiến app trả lời
//! được câu hỏi đáng giá nhất — *"so với tuần trước có gì đổi?"*. Ghi đè trạng
//! thái thì mãi mãi chỉ biết hiện tại.

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Mutex;

pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS projects (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  name       TEXT NOT NULL UNIQUE,
  note       TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS targets (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  project_id INTEGER NOT NULL,
  input      TEXT NOT NULL,                 -- đúng chuỗi người dùng nhập
  host       TEXT NOT NULL,                 -- host đã rút ra
  label      TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL,
  UNIQUE(project_id, host)
);
CREATE INDEX IF NOT EXISTS idx_targets_project ON targets(project_id);

CREATE TABLE IF NOT EXISTS runs (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  target_id   INTEGER NOT NULL,
  layer       TEXT NOT NULL,                -- profile | ports
  status      TEXT NOT NULL,                -- running | done | failed
  ip          TEXT,
  started_at  INTEGER NOT NULL,
  finished_at INTEGER,
  error       TEXT,
  summary     TEXT NOT NULL DEFAULT '{}'    -- JSON: hồ sơ đầy đủ của lần chạy
);
CREATE INDEX IF NOT EXISTS idx_runs_target ON runs(target_id, started_at DESC);

CREATE TABLE IF NOT EXISTS ports (
  id        INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id    INTEGER NOT NULL,
  target_id INTEGER NOT NULL,
  port      INTEGER NOT NULL,
  service   TEXT,
  product   TEXT,
  version   TEXT,
  banner    TEXT NOT NULL DEFAULT '',
  severity  TEXT NOT NULL DEFAULT 'info',
  detail    TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX IF NOT EXISTS idx_ports_run ON ports(run_id, port);

CREATE TABLE IF NOT EXISTS findings (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id      INTEGER NOT NULL,
  target_id   INTEGER NOT NULL,
  fingerprint TEXT NOT NULL,
  severity    TEXT NOT NULL,
  category    TEXT NOT NULL,                -- ports | registry | geo | reputation | tls | os
  title       TEXT NOT NULL,
  detail      TEXT NOT NULL DEFAULT '',
  evidence    TEXT NOT NULL DEFAULT '{}',
  fix         TEXT NOT NULL DEFAULT '',
  first_seen  INTEGER NOT NULL,
  last_seen   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_find_run ON findings(run_id);
CREATE INDEX IF NOT EXISTS idx_find_fp  ON findings(target_id, fingerprint);

CREATE TABLE IF NOT EXISTS activity (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  kind       TEXT NOT NULL,
  text       TEXT NOT NULL,
  ref_id     INTEGER,
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_activity_at ON activity(created_at DESC);
"#;

/// Cột thêm sau bản đầu. `CREATE TABLE IF NOT EXISTS` không đụng bảng đã có nên
/// mỗi cột về sau cần một ALTER riêng; lỗi trùng cột nghĩa là DB này đã có rồi.
fn migrate(c: &Connection) {
    for sql in [
        // chỗ dành sẵn cho cột về sau
    ] {
        let _: std::result::Result<usize, _> = c.execute(sql, []);
    }
}

/// Unix giây → ISO 8601 UTC.
///
/// API trả chuỗi ISO chứ không trả số nguyên: người đọc chính của các tool MCP
/// là mô hình ngôn ngữ, và `1785000000` thì không đọc được. Chuỗi ISO còn tự mô
/// tả đơn vị — JS mặc định hiểu số là mili-giây nên unix-giây trần hiển thị
/// thành năm 1970.
pub fn iso(ts: i64) -> String {
    let days = ts.div_euclid(86_400);
    let secs = ts.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}

pub fn iso_opt(ts: Option<i64>) -> Value {
    ts.map(|t| Value::String(iso(t))).unwrap_or(Value::Null)
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

pub struct Db {
    conn: Mutex<Connection>,
}

/// Một phát hiện, trước khi vào DB.
#[derive(Debug, Clone)]
pub struct Finding {
    pub fingerprint: String,
    pub severity: String,
    pub category: &'static str,
    pub title: String,
    pub detail: String,
    pub evidence: Value,
    pub fix: String,
}

impl Finding {
    pub fn new(
        category: &'static str,
        severity: impl Into<String>,
        fingerprint: impl Into<String>,
        title: impl Into<String>,
    ) -> Self {
        Self {
            fingerprint: fingerprint.into(),
            severity: severity.into(),
            category,
            title: title.into(),
            detail: String::new(),
            evidence: json!({}),
            fix: String::new(),
        }
    }
    pub fn detail(mut self, d: impl Into<String>) -> Self {
        self.detail = d.into();
        self
    }
    pub fn evidence(mut self, e: Value) -> Self {
        self.evidence = e;
        self
    }
    pub fn fix(mut self, f: impl Into<String>) -> Self {
        self.fix = f.into();
        self
    }
}

impl Db {
    pub fn open_default() -> Result<Self> {
        let dir = std::env::var("SENCLAW_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                PathBuf::from(home).join(".senclaw").join("apps").join("ipscout")
            });
        std::fs::create_dir_all(&dir).ok();
        Self::open(dir.join("ipscout.db"))
    }

    pub fn open(path: PathBuf) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch(SCHEMA)?;
        migrate(&conn);
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.ensure_default_project();
        Ok(db)
    }

    #[cfg(test)]
    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        migrate(&conn);
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.ensure_default_project();
        Ok(db)
    }

    /// Luôn có sẵn một project để người dùng điều tra được ngay mà không phải
    /// dựng cấu trúc trước. Tổ chức theo project là để tiện, không phải thủ tục.
    fn ensure_default_project(&self) {
        let c = self.conn.lock().unwrap();
        let _ = c.execute(
            "INSERT OR IGNORE INTO projects(id,name,note,created_at) VALUES(1,'Mặc định','',?1)",
            params![now()],
        );
    }

    pub fn log(&self, kind: &str, text: &str, ref_id: Option<i64>) {
        let c = self.conn.lock().unwrap();
        let _ = c.execute(
            "INSERT INTO activity(kind,text,ref_id,created_at) VALUES(?1,?2,?3,?4)",
            params![kind, text, ref_id, now()],
        );
    }

    pub fn activity(&self, limit: i64) -> Vec<Value> {
        let c = self.conn.lock().unwrap();
        let Ok(mut st) = c.prepare(
            "SELECT id,kind,text,ref_id,created_at FROM activity ORDER BY id DESC LIMIT ?1",
        ) else {
            return vec![];
        };
        st.query_map(params![limit], |r| {
            Ok(json!({
                "id": r.get::<_, i64>(0)?,
                "kind": r.get::<_, String>(1)?,
                "text": r.get::<_, String>(2)?,
                "ref_id": r.get::<_, Option<i64>>(3)?,
                "created_at": iso(r.get::<_, i64>(4)?),
            }))
        })
        .map(|it| it.filter_map(|x| x.ok()).collect())
        .unwrap_or_default()
    }

    // ---------------- projects ----------------

    pub fn add_project(&self, name: &str, note: &str) -> Result<i64> {
        let name = name.trim();
        if name.is_empty() {
            return Err(anyhow!("tên project không được rỗng"));
        }
        let c = self.conn.lock().unwrap();
        c.execute(
            "INSERT INTO projects(name,note,created_at) VALUES(?1,?2,?3)",
            params![name, note, now()],
        )
        .map_err(|e| anyhow!("không tạo được project (trùng tên?): {e}"))?;
        Ok(c.last_insert_rowid())
    }

    pub fn list_projects(&self) -> Vec<Value> {
        let c = self.conn.lock().unwrap();
        let Ok(mut st) = c.prepare(
            "SELECT p.id, p.name, p.note, p.created_at,
                    (SELECT COUNT(*) FROM targets t WHERE t.project_id = p.id)
             FROM projects p ORDER BY p.id",
        ) else {
            return vec![];
        };
        st.query_map([], |r| {
            Ok(json!({
                "id": r.get::<_, i64>(0)?,
                "name": r.get::<_, String>(1)?,
                "note": r.get::<_, String>(2)?,
                "created_at": iso(r.get::<_, i64>(3)?),
                "targets": r.get::<_, i64>(4)?,
            }))
        })
        .map(|it| it.filter_map(|x| x.ok()).collect())
        .unwrap_or_default()
    }

    /// Xoá project cùng toàn bộ mục tiêu và lịch sử bên trong.
    pub fn delete_project(&self, id: i64) -> Result<()> {
        if id == 1 {
            return Err(anyhow!("không xoá được project mặc định"));
        }
        let ids: Vec<i64> = {
            let c = self.conn.lock().unwrap();
            let mut st = c.prepare("SELECT id FROM targets WHERE project_id=?1")?;
            let v = st
                .query_map(params![id], |r| r.get::<_, i64>(0))?
                .filter_map(|x| x.ok())
                .collect();
            v
        };
        for t in ids {
            self.delete_target(t)?;
        }
        let c = self.conn.lock().unwrap();
        c.execute("DELETE FROM projects WHERE id=?1", params![id])?;
        Ok(())
    }

    // ---------------- targets ----------------

    pub fn add_target(&self, project_id: i64, input: &str, host: &str, label: &str) -> Result<i64> {
        let c = self.conn.lock().unwrap();
        let exists: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM projects WHERE id=?1",
                params![project_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if exists == 0 {
            return Err(anyhow!("không có project id={project_id}"));
        }
        c.execute(
            "INSERT INTO targets(project_id,input,host,label,created_at) VALUES(?1,?2,?3,?4,?5)",
            params![project_id, input.trim(), host, label, now()],
        )
        .map_err(|e| anyhow!("không thêm được mục tiêu (đã có trong project?): {e}"))?;
        Ok(c.last_insert_rowid())
    }

    pub fn list_targets(&self, project_id: Option<i64>) -> Vec<Value> {
        let c = self.conn.lock().unwrap();
        let (sql, p): (&str, Vec<Box<dyn rusqlite::ToSql>>) = match project_id {
            Some(pid) => (
                "SELECT id,project_id,input,host,label,created_at
                 FROM targets WHERE project_id=?1 ORDER BY id DESC",
                vec![Box::new(pid)],
            ),
            None => (
                "SELECT id,project_id,input,host,label,created_at
                 FROM targets ORDER BY id DESC",
                vec![],
            ),
        };
        let Ok(mut st) = c.prepare(sql) else {
            return vec![];
        };
        let refs: Vec<&dyn rusqlite::ToSql> = p.iter().map(|b| b.as_ref()).collect();
        st.query_map(refs.as_slice(), map_target)
            .map(|it| it.filter_map(|x| x.ok()).collect())
            .unwrap_or_default()
    }

    pub fn get_target(&self, id: i64) -> Option<Value> {
        let c = self.conn.lock().unwrap();
        c.query_row(
            "SELECT id,project_id,input,host,label,created_at
             FROM targets WHERE id=?1",
            params![id],
            map_target,
        )
        .ok()
    }

    pub fn delete_target(&self, id: i64) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute("DELETE FROM findings WHERE target_id=?1", params![id])?;
        c.execute("DELETE FROM ports WHERE target_id=?1", params![id])?;
        c.execute("DELETE FROM runs WHERE target_id=?1", params![id])?;
        c.execute("DELETE FROM targets WHERE id=?1", params![id])?;
        Ok(())
    }

    // ---------------- runs ----------------

    pub fn start_run(&self, target_id: i64, layer: &str) -> Result<i64> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "INSERT INTO runs(target_id,layer,status,started_at) VALUES(?1,?2,'running',?3)",
            params![target_id, layer, now()],
        )?;
        Ok(c.last_insert_rowid())
    }

    pub fn finish_run(
        &self,
        run_id: i64,
        status: &str,
        ip: Option<&str>,
        summary: &Value,
        error: Option<&str>,
    ) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "UPDATE runs SET status=?2, ip=?3, summary=?4, error=?5, finished_at=?6 WHERE id=?1",
            params![run_id, status, ip, summary.to_string(), error, now()],
        )?;
        Ok(())
    }

    pub fn get_run(&self, id: i64) -> Option<Value> {
        let c = self.conn.lock().unwrap();
        c.query_row(
            "SELECT id,target_id,layer,status,ip,started_at,finished_at,error,summary
             FROM runs WHERE id=?1",
            params![id],
            map_run,
        )
        .ok()
    }

    pub fn list_runs(&self, target_id: Option<i64>, limit: i64) -> Vec<Value> {
        let c = self.conn.lock().unwrap();
        let (sql, p): (&str, Vec<Box<dyn rusqlite::ToSql>>) = match target_id {
            Some(t) => (
                "SELECT id,target_id,layer,status,ip,started_at,finished_at,error,summary
                 FROM runs WHERE target_id=?1 ORDER BY id DESC LIMIT ?2",
                vec![Box::new(t), Box::new(limit)],
            ),
            None => (
                "SELECT id,target_id,layer,status,ip,started_at,finished_at,error,summary
                 FROM runs ORDER BY id DESC LIMIT ?1",
                vec![Box::new(limit)],
            ),
        };
        let Ok(mut st) = c.prepare(sql) else {
            return vec![];
        };
        let refs: Vec<&dyn rusqlite::ToSql> = p.iter().map(|b| b.as_ref()).collect();
        st.query_map(refs.as_slice(), map_run)
            .map(|it| it.filter_map(|x| x.ok()).collect())
            .unwrap_or_default()
    }

    // ---------------- ports ----------------

    pub fn add_port(
        &self,
        run_id: i64,
        target_id: i64,
        port: u16,
        service: Option<&str>,
        product: Option<&str>,
        version: Option<&str>,
        banner_text: &str,
        severity: &str,
        detail: &Value,
    ) -> Result<i64> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "INSERT INTO ports(run_id,target_id,port,service,product,version,banner,severity,detail)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                run_id,
                target_id,
                port as i64,
                service,
                product,
                version,
                banner_text,
                severity,
                detail.to_string()
            ],
        )?;
        Ok(c.last_insert_rowid())
    }

    pub fn ports_of(&self, run_id: i64) -> Vec<Value> {
        let c = self.conn.lock().unwrap();
        let Ok(mut st) = c.prepare(
            "SELECT id,run_id,port,service,product,version,banner,severity,detail
             FROM ports WHERE run_id=?1 ORDER BY port",
        ) else {
            return vec![];
        };
        st.query_map(params![run_id], |r| {
            let d: String = r.get(8)?;
            Ok(json!({
                "id": r.get::<_, i64>(0)?,
                "run_id": r.get::<_, i64>(1)?,
                "port": r.get::<_, i64>(2)?,
                "service": r.get::<_, Option<String>>(3)?,
                "product": r.get::<_, Option<String>>(4)?,
                "version": r.get::<_, Option<String>>(5)?,
                "banner": r.get::<_, String>(6)?,
                "severity": r.get::<_, String>(7)?,
                "detail": serde_json::from_str::<Value>(&d).unwrap_or(json!({})),
            }))
        })
        .map(|it| it.filter_map(|x| x.ok()).collect())
        .unwrap_or_default()
    }

    // ---------------- findings ----------------

    /// Ghi một phát hiện. `fingerprint` quyết định đây là vấn đề mới hay vấn đề
    /// cũ còn đó: cùng (target, fingerprint) thì giữ nguyên `first_seen`, nhờ
    /// vậy trả lời được "cái này tồn tại từ bao giờ".
    pub fn add_finding(&self, run_id: i64, target_id: i64, f: &Finding) -> Result<i64> {
        let c = self.conn.lock().unwrap();
        let first: i64 = c
            .query_row(
                "SELECT MIN(first_seen) FROM findings WHERE target_id=?1 AND fingerprint=?2",
                params![target_id, &f.fingerprint],
                |r| r.get::<_, Option<i64>>(0),
            )
            .ok()
            .flatten()
            .unwrap_or_else(now);
        c.execute(
            "INSERT INTO findings(run_id,target_id,fingerprint,severity,category,title,detail,
                                  evidence,fix,first_seen,last_seen)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                run_id,
                target_id,
                f.fingerprint,
                f.severity,
                f.category,
                f.title,
                f.detail,
                f.evidence.to_string(),
                f.fix,
                first,
                now()
            ],
        )?;
        Ok(c.last_insert_rowid())
    }

    pub fn findings(&self, run_id: Option<i64>, target_id: Option<i64>, sev: Option<&str>) -> Vec<Value> {
        let c = self.conn.lock().unwrap();
        let mut sql = String::from(
            "SELECT id,run_id,target_id,fingerprint,severity,category,title,detail,evidence,fix,
                    first_seen,last_seen FROM findings WHERE 1=1",
        );
        let mut p: Vec<Box<dyn rusqlite::ToSql>> = vec![];
        if let Some(r) = run_id {
            p.push(Box::new(r));
            sql.push_str(&format!(" AND run_id=?{}", p.len()));
        }
        if let Some(t) = target_id {
            p.push(Box::new(t));
            sql.push_str(&format!(" AND target_id=?{}", p.len()));
        }
        if let Some(s) = sev {
            p.push(Box::new(s.to_string()));
            sql.push_str(&format!(" AND severity=?{}", p.len()));
        }
        sql.push_str(
            " ORDER BY CASE severity WHEN 'critical' THEN 0 WHEN 'high' THEN 1
               WHEN 'medium' THEN 2 WHEN 'low' THEN 3 ELSE 4 END, id DESC",
        );
        let Ok(mut st) = c.prepare(&sql) else {
            return vec![];
        };
        let refs: Vec<&dyn rusqlite::ToSql> = p.iter().map(|b| b.as_ref()).collect();
        st.query_map(refs.as_slice(), |r| {
            let ev: String = r.get(8)?;
            Ok(json!({
                "id": r.get::<_, i64>(0)?,
                "run_id": r.get::<_, i64>(1)?,
                "target_id": r.get::<_, i64>(2)?,
                "fingerprint": r.get::<_, String>(3)?,
                "severity": r.get::<_, String>(4)?,
                "category": r.get::<_, String>(5)?,
                "title": r.get::<_, String>(6)?,
                "detail": r.get::<_, String>(7)?,
                "evidence": serde_json::from_str::<Value>(&ev).unwrap_or(json!({})),
                "fix": r.get::<_, String>(9)?,
                "first_seen": iso(r.get::<_, i64>(10)?),
                "last_seen": iso(r.get::<_, i64>(11)?),
            }))
        })
        .map(|it| it.filter_map(|x| x.ok()).collect())
        .unwrap_or_default()
    }

    /// So hai lần điều tra: cổng nào vừa mở, cổng nào đã đóng, dịch vụ nào đổi
    /// phiên bản, và IP có nhảy chỗ không.
    ///
    /// "Đổi phiên bản" là thứ một danh sách phẳng không bao giờ chỉ ra được, mà
    /// lại chính là tín hiệu vận hành đáng giá nhất: nó nghĩa là ai đó vừa cập
    /// nhật — hoặc vừa cài đè lên máy chủ.
    pub fn diff(&self, from_run: i64, to_run: i64) -> Value {
        let rows = |run: i64| -> Vec<(i64, Option<String>, Option<String>, Option<String>, String)> {
            let c = self.conn.lock().unwrap();
            let Ok(mut st) = c.prepare(
                "SELECT port,service,product,version,severity FROM ports WHERE run_id=?1",
            ) else {
                return vec![];
            };
            st.query_map(params![run], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, String>(4)?,
                ))
            })
            .map(|it| it.filter_map(|x| x.ok()).collect())
            .unwrap_or_default()
        };
        let a = rows(from_run);
        let b = rows(to_run);
        let ak: std::collections::HashMap<i64, _> = a.iter().map(|r| (r.0, r)).collect();
        let bk: std::collections::HashMap<i64, _> = b.iter().map(|r| (r.0, r)).collect();

        let opened: Vec<Value> = b
            .iter()
            .filter(|r| !ak.contains_key(&r.0))
            .map(|r| json!({ "port": r.0, "service": r.1, "product": r.2, "version": r.3, "severity": r.4 }))
            .collect();
        let closed: Vec<Value> = a
            .iter()
            .filter(|r| !bk.contains_key(&r.0))
            .map(|r| json!({ "port": r.0, "service": r.1, "product": r.2, "version": r.3 }))
            .collect();
        let changed: Vec<Value> = b
            .iter()
            .filter_map(|r| {
                let prev = ak.get(&r.0)?;
                (prev.3 != r.3 || prev.2 != r.2).then(|| {
                    json!({
                        "port": r.0,
                        "from": { "product": prev.2, "version": prev.3 },
                        "to":   { "product": r.2,    "version": r.3 },
                    })
                })
            })
            .collect();

        let ip_of = |run: i64| -> Option<String> {
            self.get_run(run)
                .and_then(|r| r["ip"].as_str().map(|s| s.to_string()))
        };
        let (ip_a, ip_b) = (ip_of(from_run), ip_of(to_run));

        json!({
            "ok": true,
            "from": from_run, "to": to_run,
            "opened": opened, "closed": closed, "changed": changed,
            "unchanged": b.len() - opened.len() - changed.len(),
            "ip_changed": (ip_a.is_some() && ip_b.is_some() && ip_a != ip_b),
            "ip_from": ip_a, "ip_to": ip_b,
        })
    }

}

fn map_target(r: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    Ok(json!({
        "id": r.get::<_, i64>(0)?,
        "project_id": r.get::<_, i64>(1)?,
        "input": r.get::<_, String>(2)?,
        "host": r.get::<_, String>(3)?,
        "label": r.get::<_, String>(4)?,
        "created_at": iso(r.get::<_, i64>(5)?),
    }))
}

fn map_run(r: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let s: String = r.get(8)?;
    Ok(json!({
        "id": r.get::<_, i64>(0)?,
        "target_id": r.get::<_, i64>(1)?,
        "layer": r.get::<_, String>(2)?,
        "status": r.get::<_, String>(3)?,
        "ip": r.get::<_, Option<String>>(4)?,
        "started_at": iso(r.get::<_, i64>(5)?),
        "finished_at": iso_opt(r.get::<_, Option<i64>>(6)?),
        "error": r.get::<_, Option<String>>(7)?,
        "summary": serde_json::from_str::<Value>(&s).unwrap_or(json!({})),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(db: &Db) -> i64 {
        db.add_target(1, "example.com", "example.com", "demo").unwrap()
    }

    #[test]
    fn a_default_project_always_exists_so_the_first_investigation_needs_no_setup() {
        let db = Db::open_memory().unwrap();
        let p = db.list_projects();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0]["id"], 1);
        // và không xoá được — nếu không thì mọi mục tiêu mới mất chỗ để nằm
        assert!(db.delete_project(1).is_err());
    }

    #[test]
    fn the_same_host_can_live_in_two_projects_but_not_twice_in_one() {
        let db = Db::open_memory().unwrap();
        let p2 = db.add_project("Khách hàng A", "").unwrap();
        seed(&db);
        // trùng trong cùng project → lỗi
        assert!(db.add_target(1, "example.com", "example.com", "").is_err());
        // cùng host ở project khác → được, vì đó là hai ngữ cảnh công việc khác nhau
        assert!(db.add_target(p2, "example.com", "example.com", "").is_ok());
        assert_eq!(db.list_targets(Some(1)).len(), 1);
        assert_eq!(db.list_targets(None).len(), 2);
    }

    #[test]
    fn a_target_cannot_be_added_to_a_project_that_does_not_exist() {
        let db = Db::open_memory().unwrap();
        assert!(db.add_target(999, "a.vn", "a.vn", "").is_err());
    }

    #[test]
    fn deleting_a_project_takes_its_targets_and_history_with_it() {
        let db = Db::open_memory().unwrap();
        let p = db.add_project("Tạm", "").unwrap();
        let t = db.add_target(p, "a.vn", "a.vn", "").unwrap();
        let r = db.start_run(t, "ports").unwrap();
        db.add_port(r, t, 22, Some("ssh"), None, None, "", "info", &json!({})).unwrap();
        db.delete_project(p).unwrap();
        assert!(db.list_targets(Some(p)).is_empty());
        assert!(db.list_runs(Some(t), 10).is_empty());
        assert!(db.ports_of(r).is_empty());
    }

    #[test]
    fn iso_formats_unix_seconds_correctly() {
        assert_eq!(iso(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso(1_000_000_000), "2001-09-09T01:46:40Z");
        assert_eq!(iso(1_754_000_000), "2025-07-31T22:13:20Z");
        assert_eq!(iso(1_709_164_800), "2024-02-29T00:00:00Z"); // năm nhuận
        assert_eq!(iso(1_735_689_599), "2024-12-31T23:59:59Z");
        assert_eq!(iso(1_735_689_600), "2025-01-01T00:00:00Z");
        assert!(iso_opt(None).is_null());
    }

    #[test]
    fn diff_reports_opened_closed_and_version_changes() {
        let db = Db::open_memory().unwrap();
        let t = seed(&db);
        let r1 = db.start_run(t, "ports").unwrap();
        db.add_port(r1, t, 22, Some("ssh"), Some("OpenSSH"), Some("8.9p1"), "", "info", &json!({})).unwrap();
        db.add_port(r1, t, 3306, Some("mysql"), Some("MySQL"), Some("8.0.35"), "", "critical", &json!({})).unwrap();

        let r2 = db.start_run(t, "ports").unwrap();
        // 22 nâng cấp phiên bản, 3306 đã đóng, 443 mới mở
        db.add_port(r2, t, 22, Some("ssh"), Some("OpenSSH"), Some("9.6p1"), "", "info", &json!({})).unwrap();
        db.add_port(r2, t, 443, Some("http"), Some("nginx"), Some("1.24.0"), "", "info", &json!({})).unwrap();

        let d = db.diff(r1, r2);
        assert_eq!(d["opened"].as_array().unwrap().len(), 1);
        assert_eq!(d["opened"][0]["port"], 443);
        assert_eq!(d["closed"].as_array().unwrap().len(), 1);
        assert_eq!(d["closed"][0]["port"], 3306);
        // đổi phiên bản: thứ danh sách phẳng không bao giờ chỉ ra được
        assert_eq!(d["changed"].as_array().unwrap().len(), 1);
        assert_eq!(d["changed"][0]["from"]["version"], "8.9p1");
        assert_eq!(d["changed"][0]["to"]["version"], "9.6p1");
    }

    #[test]
    fn diff_flags_an_ip_that_moved() {
        let db = Db::open_memory().unwrap();
        let t = seed(&db);
        let r1 = db.start_run(t, "profile").unwrap();
        db.finish_run(r1, "done", Some("1.2.3.4"), &json!({}), None).unwrap();
        let r2 = db.start_run(t, "profile").unwrap();
        db.finish_run(r2, "done", Some("5.6.7.8"), &json!({}), None).unwrap();
        let d = db.diff(r1, r2);
        assert_eq!(d["ip_changed"], true);
        assert_eq!(d["ip_from"], "1.2.3.4");
        assert_eq!(d["ip_to"], "5.6.7.8");
    }

    #[test]
    fn an_unfinished_run_does_not_count_as_an_ip_change() {
        // Cả hai đầu phải có IP; thiếu một đầu là không so được, không phải "đã đổi".
        let db = Db::open_memory().unwrap();
        let t = seed(&db);
        let r1 = db.start_run(t, "profile").unwrap();
        db.finish_run(r1, "done", Some("1.2.3.4"), &json!({}), None).unwrap();
        let r2 = db.start_run(t, "profile").unwrap();
        assert_eq!(db.diff(r1, r2)["ip_changed"], false);
    }

    #[test]
    fn first_seen_survives_across_runs_so_age_is_answerable() {
        let db = Db::open_memory().unwrap();
        let t = seed(&db);
        let f = Finding::new("ports", "critical", "port:3306:exposed", "MySQL phơi ra Internet");
        let r1 = db.start_run(t, "ports").unwrap();
        db.add_finding(r1, t, &f).unwrap();
        let first = db.findings(Some(r1), None, None)[0]["first_seen"]
            .as_str()
            .unwrap()
            .to_string();
        let r2 = db.start_run(t, "ports").unwrap();
        db.add_finding(r2, t, &f).unwrap();
        assert_eq!(
            db.findings(Some(r2), None, None)[0]["first_seen"].as_str().unwrap(),
            first
        );
    }

    #[test]
    fn findings_are_sorted_worst_first() {
        let db = Db::open_memory().unwrap();
        let t = seed(&db);
        let r = db.start_run(t, "ports").unwrap();
        for (sev, fp) in [("low", "a"), ("critical", "b"), ("medium", "c"), ("info", "d")] {
            db.add_finding(r, t, &Finding::new("ports", sev, fp, "x")).unwrap();
        }
        let got = db.findings(Some(r), None, None);
        assert_eq!(got[0]["severity"], "critical");
        assert_eq!(got[1]["severity"], "medium");
        assert_eq!(got[3]["severity"], "info");
        // lọc theo mức
        assert_eq!(db.findings(Some(r), None, Some("critical")).len(), 1);
    }

    #[test]
    fn run_summary_round_trips_as_json_not_a_string() {
        let db = Db::open_memory().unwrap();
        let t = seed(&db);
        let r = db.start_run(t, "profile").unwrap();
        db.finish_run(r, "done", Some("1.1.1.1"), &json!({"asn": 13335}), None).unwrap();
        let got = db.get_run(r).unwrap();
        assert_eq!(got["summary"]["asn"], 13335);
        assert_eq!(got["status"], "done");
        assert!(got["finished_at"].as_str().unwrap().ends_with('Z'));
    }

}
