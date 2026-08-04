//! SQLite cho secscan. Ba bảng chính: `assets` (sổ tài sản + trạng thái xác
//! minh sở hữu), `scans` (mỗi lần quét), `findings` (phát hiện, khoá theo
//! `fingerprint` để so được giữa hai lần quét).

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Mutex;

pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS assets (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  kind          TEXT NOT NULL,              -- website | host | domain
  target        TEXT NOT NULL UNIQUE,
  label         TEXT NOT NULL DEFAULT '',
  verify_method TEXT,                       -- dns-txt | dns-cname | well-known | meta | local
  verify_token  TEXT,
  verified_at   INTEGER,                    -- NULL = chưa xác minh -> chặn L2/L3
  verify_error  TEXT,
  ssh_ref       TEXT,                       -- id connection bên ssh-manager; KHÔNG chứa credential
  created_at    INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS scans (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  asset_id    INTEGER NOT NULL,
  layer       TEXT NOT NULL,                -- passive | active-light | host
  status      TEXT NOT NULL,                -- running | done | failed
  score       INTEGER,
  grade       TEXT,
  started_at  INTEGER NOT NULL,
  finished_at INTEGER,
  error       TEXT,
  raw         TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX IF NOT EXISTS idx_scans_asset ON scans(asset_id, started_at DESC);

CREATE TABLE IF NOT EXISTS findings (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  scan_id     INTEGER NOT NULL,
  asset_id    INTEGER NOT NULL,
  fingerprint TEXT NOT NULL,
  severity    TEXT NOT NULL,                -- critical|high|medium|low|info
  category    TEXT NOT NULL,                -- tls|headers|cookies|cors|dns|cve|ssh|exposure
  title       TEXT NOT NULL,
  detail      TEXT NOT NULL DEFAULT '',
  evidence    TEXT NOT NULL DEFAULT '{}',
  remediation TEXT NOT NULL DEFAULT '',
  wstg        TEXT NOT NULL DEFAULT '',     -- mã OWASP WSTG, ngôn ngữ chung của ngành
  cve         TEXT,
  epss        REAL,
  kev         INTEGER NOT NULL DEFAULT 0,
  status      TEXT NOT NULL DEFAULT 'open', -- open|acked|fixed|regressed
  ack_reason  TEXT,
  ack_until   INTEGER,
  first_seen  INTEGER NOT NULL,
  last_seen   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_find_scan ON findings(scan_id);
CREATE INDEX IF NOT EXISTS idx_find_fp   ON findings(asset_id, fingerprint);

CREATE TABLE IF NOT EXISTS settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

-- Luật tự thêm / nhập từ nguồn ngoài. Lưu nguyên JSON để định dạng tiến hoá
-- được mà không phải migrate cột mỗi lần thêm phép so khớp.
CREATE TABLE IF NOT EXISTS custom_rules (
  id         TEXT PRIMARY KEY,
  json       TEXT NOT NULL,
  source     TEXT NOT NULL DEFAULT 'manual',
  created_at INTEGER NOT NULL
);

-- Ghi đè luật DỰNG SẴN: đổi mức hoặc tắt hẳn. Tách khỏi custom_rules vì luật
-- dựng sẵn do code định nghĩa — ở đây chỉ chỉnh cách chấm, không chỉnh phép kiểm.
CREATE TABLE IF NOT EXISTS rule_overrides (
  rule_id  TEXT PRIMARY KEY,
  severity TEXT,
  enabled  INTEGER NOT NULL DEFAULT 1,
  note     TEXT
);

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
/// tả đơn vị — số nguyên thì không, và JS mặc định hiểu số là mili-giây nên
/// unix-giây trần hiển thị thành năm 1970.
pub fn iso(ts: i64) -> String {
    let days = ts.div_euclid(86_400);
    let secs = ts.rem_euclid(86_400);
    // chuyển ngày-từ-epoch sang y/m/d theo thuật toán civil_from_days (Howard Hinnant)
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
        secs / 3600, (secs % 3600) / 60, secs % 60
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

impl Db {
    pub fn open_default() -> Result<Self> {
        let dir = std::env::var("SENCLAW_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                PathBuf::from(home)
                    .join(".senclaw")
                    .join("apps")
                    .join("secscan")
            });
        std::fs::create_dir_all(&dir).ok();
        Self::open(dir.join("secscan.db"))
    }

    pub fn open(path: PathBuf) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch(SCHEMA)?;
        migrate(&conn);
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    #[cfg(test)]
    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        migrate(&conn);
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn log(&self, kind: &str, text: &str, ref_id: Option<i64>) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO activity(kind,text,ref_id,created_at) VALUES(?1,?2,?3,?4)",
            params![kind, text, ref_id, now()],
        );
    }

    // ---------------- Luật tự thêm ----------------

    pub fn put_custom_rule(&self, id: &str, json_text: &str, source: &str) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "INSERT INTO custom_rules(id,json,source,created_at) VALUES(?1,?2,?3,?4)
             ON CONFLICT(id) DO UPDATE SET json=excluded.json, source=excluded.source",
            params![id, json_text, source, now()],
        )?;
        Ok(())
    }

    pub fn delete_custom_rule(&self, id: &str) -> Result<usize> {
        let c = self.conn.lock().unwrap();
        Ok(c.execute("DELETE FROM custom_rules WHERE id=?1", params![id])?)
    }

    /// Trả JSON thô; gọi bên gọi tự deserialize để db.rs không phụ thuộc custom.rs.
    pub fn custom_rules_raw(&self) -> Vec<String> {
        let c = self.conn.lock().unwrap();
        let Ok(mut st) = c.prepare("SELECT json FROM custom_rules ORDER BY id") else {
            return vec![];
        };
        st.query_map([], |r| r.get::<_, String>(0))
            .map(|it| it.filter_map(|x| x.ok()).collect())
            .unwrap_or_default()
    }

    // ---------------- Ghi đè luật dựng sẵn ----------------

    pub fn set_override(&self, rule_id: &str, severity: Option<&str>, enabled: bool, note: Option<&str>) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "INSERT INTO rule_overrides(rule_id,severity,enabled,note) VALUES(?1,?2,?3,?4)
             ON CONFLICT(rule_id) DO UPDATE SET
               severity=excluded.severity, enabled=excluded.enabled, note=excluded.note",
            params![rule_id, severity, enabled as i64, note],
        )?;
        Ok(())
    }

    pub fn clear_override(&self, rule_id: &str) -> Result<usize> {
        let c = self.conn.lock().unwrap();
        Ok(c.execute("DELETE FROM rule_overrides WHERE rule_id=?1", params![rule_id])?)
    }

    /// (rule_id, severity ghi đè, bật/tắt, ghi chú)
    pub fn overrides(&self) -> Vec<(String, Option<String>, bool, Option<String>)> {
        let c = self.conn.lock().unwrap();
        let Ok(mut st) = c.prepare("SELECT rule_id,severity,enabled,note FROM rule_overrides") else {
            return vec![];
        };
        st.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, i64>(2)? != 0,
                r.get::<_, Option<String>>(3)?,
            ))
        })
        .map(|it| it.filter_map(|x| x.ok()).collect())
        .unwrap_or_default()
    }

    pub fn activity(&self, limit: i64) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut st = match conn
            .prepare("SELECT id,kind,text,ref_id,created_at FROM activity ORDER BY id DESC LIMIT ?1")
        {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        let rows = st.query_map(params![limit], |r| {
            Ok(json!({
                "id": r.get::<_, i64>(0)?,
                "kind": r.get::<_, String>(1)?,
                "text": r.get::<_, String>(2)?,
                "ref_id": r.get::<_, Option<i64>>(3)?,
                "created_at": iso(r.get::<_, i64>(4)?),
            }))
        });
        rows.map(|it| it.filter_map(|x| x.ok()).collect())
            .unwrap_or_default()
    }

    // ---- settings ----

    pub fn get_setting(&self, key: &str) -> Option<String> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT value FROM settings WHERE key=?1",
            params![key],
            |r| r.get::<_, String>(0),
        )
        .ok()
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO settings(key,value) VALUES(?1,?2)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn settings(&self) -> Value {
        let conn = self.conn.lock().unwrap();
        let mut out = serde_json::Map::new();
        if let Ok(mut st) = conn.prepare("SELECT key,value FROM settings") {
            if let Ok(rows) = st.query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            }) {
                for (k, v) in rows.flatten() {
                    out.insert(k, Value::String(v));
                }
            }
        }
        Value::Object(out)
    }

    // ---- assets ----

    pub fn add_asset(&self, kind: &str, target: &str, label: &str) -> Result<i64> {
        let target = target.trim();
        if target.is_empty() {
            return Err(anyhow!("target không được rỗng"));
        }
        if !matches!(kind, "website" | "host" | "domain") {
            return Err(anyhow!("kind phải là website | host | domain"));
        }
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO assets(kind,target,label,created_at) VALUES(?1,?2,?3,?4)",
            params![kind, target, label, now()],
        )
        .map_err(|e| anyhow!("không thêm được tài sản (trùng target?): {e}"))?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_assets(&self) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut st = match conn.prepare(
            "SELECT id,kind,target,label,verify_method,verify_token,verified_at,verify_error,ssh_ref,created_at
             FROM assets ORDER BY id DESC",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        let rows = st.query_map([], map_asset);
        rows.map(|it| it.filter_map(|x| x.ok()).collect())
            .unwrap_or_default()
    }

    pub fn get_asset(&self, id: i64) -> Option<Value> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id,kind,target,label,verify_method,verify_token,verified_at,verify_error,ssh_ref,created_at
             FROM assets WHERE id=?1",
            params![id],
            map_asset,
        )
        .ok()
    }

    pub fn set_asset_token(&self, id: i64, method: &str, token: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE assets SET verify_method=?2, verify_token=?3 WHERE id=?1",
            params![id, method, token],
        )?;
        Ok(())
    }

    pub fn mark_verified(&self, id: i64, ok: bool, err: Option<&str>) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE assets SET verified_at=?2, verify_error=?3 WHERE id=?1",
            params![id, if ok { Some(now()) } else { None }, err],
        )?;
        Ok(())
    }

    pub fn delete_asset(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM findings WHERE asset_id=?1", params![id])?;
        conn.execute("DELETE FROM scans WHERE asset_id=?1", params![id])?;
        conn.execute("DELETE FROM assets WHERE id=?1", params![id])?;
        Ok(())
    }

    // ---- scans ----

    pub fn start_scan(&self, asset_id: i64, layer: &str) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO scans(asset_id,layer,status,started_at) VALUES(?1,?2,'running',?3)",
            params![asset_id, layer, now()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn finish_scan(
        &self,
        scan_id: i64,
        status: &str,
        score: Option<i64>,
        grade: Option<&str>,
        error: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE scans SET status=?2, score=?3, grade=?4, error=?5, finished_at=?6 WHERE id=?1",
            params![scan_id, status, score, grade, error, now()],
        )?;
        Ok(())
    }

    pub fn get_scan(&self, id: i64) -> Option<Value> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id,asset_id,layer,status,score,grade,started_at,finished_at,error FROM scans WHERE id=?1",
            params![id],
            map_scan,
        )
        .ok()
    }

    pub fn list_scans(&self, asset_id: Option<i64>, limit: i64) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let (sql, p): (&str, Vec<Box<dyn rusqlite::ToSql>>) = match asset_id {
            Some(a) => (
                "SELECT id,asset_id,layer,status,score,grade,started_at,finished_at,error
                 FROM scans WHERE asset_id=?1 ORDER BY id DESC LIMIT ?2",
                vec![Box::new(a), Box::new(limit)],
            ),
            None => (
                "SELECT id,asset_id,layer,status,score,grade,started_at,finished_at,error
                 FROM scans ORDER BY id DESC LIMIT ?1",
                vec![Box::new(limit)],
            ),
        };
        let mut st = match conn.prepare(sql) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        let refs: Vec<&dyn rusqlite::ToSql> = p.iter().map(|b| b.as_ref()).collect();
        st.query_map(refs.as_slice(), map_scan)
            .map(|it| it.filter_map(|x| x.ok()).collect())
            .unwrap_or_default()
    }

    // ---- findings ----

    /// Ghi một phát hiện. `fingerprint` quyết định đây là vấn đề mới hay vấn đề
    /// cũ tái xuất: cùng (asset, fingerprint) thì giữ `first_seen` ban đầu, và
    /// nếu lần trước đã `fixed` thì lần này là `regressed`.
    pub fn upsert_finding(&self, scan_id: i64, asset_id: i64, f: &Finding) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let prev: Option<(i64, String)> = conn
            .query_row(
                "SELECT first_seen,status FROM findings
                 WHERE asset_id=?1 AND fingerprint=?2 ORDER BY id DESC LIMIT 1",
                params![asset_id, &f.fingerprint],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
            )
            .ok();
        let (first_seen, status) = match prev {
            Some((fs, st)) if st == "fixed" => (fs, "regressed"),
            Some((fs, st)) if st == "acked" => (fs, "acked"),
            Some((fs, _)) => (fs, "open"),
            None => (now(), "open"),
        };
        conn.execute(
            "INSERT INTO findings(scan_id,asset_id,fingerprint,severity,category,title,detail,
                                  evidence,remediation,wstg,cve,epss,kev,status,first_seen,last_seen)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
            params![
                scan_id,
                asset_id,
                f.fingerprint,
                f.severity,
                f.category,
                f.title,
                f.detail,
                f.evidence.to_string(),
                f.remediation,
                f.wstg,
                f.cve,
                f.epss,
                f.kev as i64,
                status,
                first_seen,
                now()
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn findings(&self, scan_id: Option<i64>, asset_id: Option<i64>, sev: Option<&str>) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut sql = String::from(
            "SELECT id,scan_id,asset_id,fingerprint,severity,category,title,detail,evidence,
                    remediation,wstg,cve,epss,kev,status,first_seen,last_seen FROM findings WHERE 1=1",
        );
        let mut p: Vec<Box<dyn rusqlite::ToSql>> = vec![];
        if let Some(s) = scan_id {
            p.push(Box::new(s));
            sql.push_str(&format!(" AND scan_id=?{}", p.len()));
        }
        if let Some(a) = asset_id {
            p.push(Box::new(a));
            sql.push_str(&format!(" AND asset_id=?{}", p.len()));
        }
        if let Some(s) = sev {
            p.push(Box::new(s.to_string()));
            sql.push_str(&format!(" AND severity=?{}", p.len()));
        }
        sql.push_str(" ORDER BY CASE severity WHEN 'critical' THEN 0 WHEN 'high' THEN 1
                       WHEN 'medium' THEN 2 WHEN 'low' THEN 3 ELSE 4 END, id DESC");
        let mut st = match conn.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        let refs: Vec<&dyn rusqlite::ToSql> = p.iter().map(|b| b.as_ref()).collect();
        st.query_map(refs.as_slice(), map_finding)
            .map(|it| it.filter_map(|x| x.ok()).collect())
            .unwrap_or_default()
    }

    pub fn set_finding_status(&self, id: i64, status: &str, reason: Option<&str>) -> Result<()> {
        if !matches!(status, "open" | "acked" | "fixed" | "regressed") {
            return Err(anyhow!("status không hợp lệ"));
        }
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE findings SET status=?2, ack_reason=?3 WHERE id=?1",
            params![id, status, reason],
        )?;
        Ok(())
    }

    /// So hai lần quét theo `fingerprint`: cái nào mới, cái nào đã hết.
    pub fn diff(&self, from_scan: i64, to_scan: i64) -> Value {
        let fps = |scan: i64| -> Vec<(String, String, String)> {
            let conn = self.conn.lock().unwrap();
            let mut st = match conn
                .prepare("SELECT fingerprint,severity,title FROM findings WHERE scan_id=?1")
            {
                Ok(s) => s,
                Err(_) => return vec![],
            };
            st.query_map(params![scan], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map(|it| it.filter_map(|x| x.ok()).collect())
            .unwrap_or_default()
        };
        let a = fps(from_scan);
        let b = fps(to_scan);
        let akeys: std::collections::HashSet<&str> = a.iter().map(|x| x.0.as_str()).collect();
        let bkeys: std::collections::HashSet<&str> = b.iter().map(|x| x.0.as_str()).collect();
        let new: Vec<Value> = b
            .iter()
            .filter(|x| !akeys.contains(x.0.as_str()))
            .map(|x| json!({"fingerprint": x.0, "severity": x.1, "title": x.2}))
            .collect();
        let gone: Vec<Value> = a
            .iter()
            .filter(|x| !bkeys.contains(x.0.as_str()))
            .map(|x| json!({"fingerprint": x.0, "severity": x.1, "title": x.2}))
            .collect();
        json!({
            "ok": true, "from": from_scan, "to": to_scan,
            "new": new, "fixed": gone,
            "unchanged": b.len() - new.len(),
        })
    }
}

/// Một phát hiện, trước khi vào DB.
#[derive(Debug, Clone)]
pub struct Finding {
    pub fingerprint: String,
    pub severity: &'static str,
    pub category: &'static str,
    pub title: String,
    pub detail: String,
    pub evidence: Value,
    pub remediation: String,
    pub wstg: &'static str,
    pub cve: Option<String>,
    pub epss: Option<f64>,
    pub kev: bool,
}

impl Finding {
    pub fn new(
        category: &'static str,
        severity: &'static str,
        fingerprint: impl Into<String>,
        title: impl Into<String>,
    ) -> Self {
        Self {
            fingerprint: fingerprint.into(),
            severity,
            category,
            title: title.into(),
            detail: String::new(),
            evidence: json!({}),
            remediation: String::new(),
            wstg: "",
            cve: None,
            epss: None,
            kev: false,
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
    pub fn fix(mut self, r: impl Into<String>) -> Self {
        self.remediation = r.into();
        self
    }
    pub fn wstg(mut self, w: &'static str) -> Self {
        self.wstg = w;
        self
    }
}

fn map_asset(r: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    Ok(json!({
        "id": r.get::<_, i64>(0)?,
        "kind": r.get::<_, String>(1)?,
        "target": r.get::<_, String>(2)?,
        "label": r.get::<_, String>(3)?,
        "verify_method": r.get::<_, Option<String>>(4)?,
        "verify_token": r.get::<_, Option<String>>(5)?,
        "verified_at": iso_opt(r.get::<_, Option<i64>>(6)?),
        "verify_error": r.get::<_, Option<String>>(7)?,
        "ssh_ref": r.get::<_, Option<String>>(8)?,
        "created_at": iso(r.get::<_, i64>(9)?),
    }))
}

fn map_scan(r: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    Ok(json!({
        "id": r.get::<_, i64>(0)?,
        "asset_id": r.get::<_, i64>(1)?,
        "layer": r.get::<_, String>(2)?,
        "status": r.get::<_, String>(3)?,
        "score": r.get::<_, Option<i64>>(4)?,
        "grade": r.get::<_, Option<String>>(5)?,
        "started_at": iso(r.get::<_, i64>(6)?),
        "finished_at": iso_opt(r.get::<_, Option<i64>>(7)?),
        "error": r.get::<_, Option<String>>(8)?,
    }))
}

fn map_finding(r: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let ev: String = r.get(8)?;
    Ok(json!({
        "id": r.get::<_, i64>(0)?,
        "scan_id": r.get::<_, i64>(1)?,
        "asset_id": r.get::<_, i64>(2)?,
        "fingerprint": r.get::<_, String>(3)?,
        "severity": r.get::<_, String>(4)?,
        "category": r.get::<_, String>(5)?,
        "title": r.get::<_, String>(6)?,
        "detail": r.get::<_, String>(7)?,
        "evidence": serde_json::from_str::<Value>(&ev).unwrap_or(json!({})),
        "remediation": r.get::<_, String>(9)?,
        "wstg": r.get::<_, String>(10)?,
        "cve": r.get::<_, Option<String>>(11)?,
        "epss": r.get::<_, Option<f64>>(12)?,
        "kev": r.get::<_, i64>(13)? != 0,
        "status": r.get::<_, String>(14)?,
        "first_seen": iso(r.get::<_, i64>(15)?),
        "last_seen": iso(r.get::<_, i64>(16)?),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(db: &Db) -> i64 {
        db.add_asset("website", "https://example.com", "demo").unwrap()
    }

    #[test]
    fn asset_crud_and_unique_target() {
        let db = Db::open_memory().unwrap();
        let id = seed(&db);
        assert_eq!(db.list_assets().len(), 1);
        // target là UNIQUE — thêm trùng phải lỗi, không im lặng tạo bản sao
        assert!(db.add_asset("website", "https://example.com", "x").is_err());
        assert!(db.add_asset("bogus", "https://other.com", "").is_err());
        db.delete_asset(id).unwrap();
        assert!(db.list_assets().is_empty());
    }

    #[test]
    fn verification_state_transitions() {
        let db = Db::open_memory().unwrap();
        let id = seed(&db);
        assert!(db.get_asset(id).unwrap()["verified_at"].is_null());
        db.set_asset_token(id, "dns-txt", "abc123").unwrap();
        db.mark_verified(id, false, Some("không tìm thấy TXT")).unwrap();
        let a = db.get_asset(id).unwrap();
        assert!(a["verified_at"].is_null());
        assert_eq!(a["verify_error"], "không tìm thấy TXT");
        db.mark_verified(id, true, None).unwrap();
        let at = db.get_asset(id).unwrap();
        assert!(at["verified_at"].as_str().unwrap().ends_with('Z'));
    }

    #[test]
    fn iso_formats_unix_seconds_correctly() {
        // Mốc đã biết chắc, đối chiếu được bằng `date -u -r <ts>`.
        assert_eq!(iso(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso(1_000_000_000), "2001-09-09T01:46:40Z");
        assert_eq!(iso(1_754_000_000), "2025-07-31T22:13:20Z");
        // năm nhuận: 2024-02-29 phải tồn tại
        assert_eq!(iso(1_709_164_800), "2024-02-29T00:00:00Z");
        // ranh giới cuối năm
        assert_eq!(iso(1_735_689_599), "2024-12-31T23:59:59Z");
        assert_eq!(iso(1_735_689_600), "2025-01-01T00:00:00Z");
        assert!(iso_opt(None).is_null());
    }

    #[test]
    fn finding_fingerprint_tracks_regression() {
        let db = Db::open_memory().unwrap();
        let aid = seed(&db);
        let f = Finding::new("headers", "medium", "hdr:hsts:missing", "Thiếu HSTS");

        let s1 = db.start_scan(aid, "passive").unwrap();
        db.upsert_finding(s1, aid, &f).unwrap();
        let first = db.findings(Some(s1), None, None)[0]["first_seen"]
            .as_str()
            .unwrap()
            .to_string();

        // đánh dấu đã vá, rồi lần quét sau nó quay lại -> phải là 'regressed'
        let fid = db.findings(Some(s1), None, None)[0]["id"].as_i64().unwrap();
        db.set_finding_status(fid, "fixed", None).unwrap();

        let s2 = db.start_scan(aid, "passive").unwrap();
        db.upsert_finding(s2, aid, &f).unwrap();
        let f2 = &db.findings(Some(s2), None, None)[0];
        assert_eq!(f2["status"], "regressed");
        // first_seen phải giữ nguyên từ lần đầu, không reset
        assert_eq!(f2["first_seen"].as_str().unwrap(), first);
    }

    #[test]
    fn diff_reports_new_and_fixed() {
        let db = Db::open_memory().unwrap();
        let aid = seed(&db);
        let a = Finding::new("headers", "low", "hdr:xcto:missing", "Thiếu X-Content-Type-Options");
        let b = Finding::new("dns", "medium", "dns:dmarc:none", "DMARC p=none");

        let s1 = db.start_scan(aid, "passive").unwrap();
        db.upsert_finding(s1, aid, &a).unwrap();

        let s2 = db.start_scan(aid, "passive").unwrap();
        db.upsert_finding(s2, aid, &b).unwrap();

        let d = db.diff(s1, s2);
        assert_eq!(d["new"].as_array().unwrap().len(), 1);
        assert_eq!(d["new"][0]["fingerprint"], "dns:dmarc:none");
        assert_eq!(d["fixed"].as_array().unwrap().len(), 1);
        assert_eq!(d["fixed"][0]["fingerprint"], "hdr:xcto:missing");
    }

    #[test]
    fn findings_sorted_by_severity() {
        let db = Db::open_memory().unwrap();
        let aid = seed(&db);
        let s = db.start_scan(aid, "passive").unwrap();
        for f in [
            Finding::new("headers", "low", "a", "low one"),
            Finding::new("tls", "critical", "b", "critical one"),
            Finding::new("dns", "medium", "c", "medium one"),
        ] {
            db.upsert_finding(s, aid, &f).unwrap();
        }
        let got = db.findings(Some(s), None, None);
        assert_eq!(got[0]["severity"], "critical");
        assert_eq!(got[1]["severity"], "medium");
        assert_eq!(got[2]["severity"], "low");
    }

    #[test]
    fn settings_upsert() {
        let db = Db::open_memory().unwrap();
        db.set_setting("rate_limit", "5").unwrap();
        db.set_setting("rate_limit", "3").unwrap();
        assert_eq!(db.get_setting("rate_limit").unwrap(), "3");
        assert_eq!(db.settings()["rate_limit"], "3");
    }
}
