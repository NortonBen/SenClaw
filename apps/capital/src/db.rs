//! Local SQLite store for the Capital app (quản lý nguồn vốn). Everything is
//! local-first — no external service holds this data. Tables:
//!   * `sources`      — các nguồn vốn (vốn chủ, vay ngân hàng, hạn mức tín dụng…)
//!   * `transactions` — sổ cái: giải ngân / trả gốc / trả lãi / phí, gắn nguồn + phân bổ
//!   * `allocations`  — mục đích sử dụng vốn (dự án), theo dõi đã rót bao nhiêu
//!   * `schedule`     — lịch trả nợ sinh từ finance::generate_schedule
//!   * `activity`     — log hành động của app/agent
//!   * `settings`     — kv (currency mặc định, ngưỡng cảnh báo…)

use crate::finance::{self, round2, Installment};
use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Db {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS sources (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  name          TEXT NOT NULL,
  kind          TEXT NOT NULL DEFAULT 'bank_loan',
  provider      TEXT NOT NULL DEFAULT '',
  total_amount  REAL NOT NULL DEFAULT 0,
  currency      TEXT NOT NULL DEFAULT 'VND',
  interest_rate REAL NOT NULL DEFAULT 0,
  rate_type     TEXT NOT NULL DEFAULT 'fixed',
  start_date    TEXT NOT NULL DEFAULT '',
  end_date      TEXT NOT NULL DEFAULT '',
  status        TEXT NOT NULL DEFAULT 'active',
  note          TEXT NOT NULL DEFAULT '',
  created_at    INTEGER NOT NULL,
  updated_at    INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS transactions (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  source_id  INTEGER NOT NULL,
  alloc_id   INTEGER,
  kind       TEXT NOT NULL,
  amount     REAL NOT NULL,
  tx_date    TEXT NOT NULL,
  note       TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_tx_source ON transactions(source_id);
CREATE INDEX IF NOT EXISTS idx_tx_date   ON transactions(tx_date);
CREATE INDEX IF NOT EXISTS idx_tx_alloc  ON transactions(alloc_id);
CREATE TABLE IF NOT EXISTS allocations (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  name          TEXT NOT NULL,
  description   TEXT NOT NULL DEFAULT '',
  target_amount REAL NOT NULL DEFAULT 0,
  status        TEXT NOT NULL DEFAULT 'active',
  created_at    INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS schedule (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  source_id     INTEGER NOT NULL,
  seq           INTEGER NOT NULL,
  due_date      TEXT NOT NULL,
  principal_due REAL NOT NULL DEFAULT 0,
  interest_due  REAL NOT NULL DEFAULT 0,
  status        TEXT NOT NULL DEFAULT 'upcoming',
  paid_at       INTEGER,
  paid_date     TEXT NOT NULL DEFAULT '',
  note          TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_sched_source ON schedule(source_id);
CREATE INDEX IF NOT EXISTS idx_sched_due    ON schedule(due_date);
CREATE TABLE IF NOT EXISTS goals (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  name          TEXT NOT NULL,
  kind          TEXT NOT NULL,
  target_amount REAL NOT NULL DEFAULT 0,
  baseline      REAL NOT NULL DEFAULT 0,
  source_id     INTEGER,
  deadline      TEXT NOT NULL DEFAULT '',
  status        TEXT NOT NULL DEFAULT 'active',
  note          TEXT NOT NULL DEFAULT '',
  created_date  TEXT NOT NULL DEFAULT '',
  created_at    INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS goal_steps (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  goal_id    INTEGER NOT NULL,
  seq        INTEGER NOT NULL DEFAULT 0,
  title      TEXT NOT NULL,
  due_date   TEXT NOT NULL DEFAULT '',
  amount     REAL NOT NULL DEFAULT 0,
  status     TEXT NOT NULL DEFAULT 'todo',
  source     TEXT NOT NULL DEFAULT 'manual',
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_steps_goal ON goal_steps(goal_id);
CREATE TABLE IF NOT EXISTS activity (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  kind       TEXT NOT NULL,
  text       TEXT NOT NULL DEFAULT '',
  ref        TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
"#;

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// A source row with ledger aggregates joined in. `outstanding`/`available`
/// are derived in [`Self::to_value`].
#[derive(Debug, Clone)]
pub struct SourceRow {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub provider: String,
    pub total_amount: f64,
    pub currency: String,
    pub interest_rate: f64,
    pub rate_type: String,
    pub start_date: String,
    pub end_date: String,
    pub status: String,
    pub note: String,
    pub disbursed: f64,
    pub repaid_principal: f64,
    pub interest_paid: f64,
    pub fees_paid: f64,
}

impl SourceRow {
    /// Dư nợ hiện tại = đã giải ngân − đã trả gốc.
    pub fn outstanding(&self) -> f64 {
        round2(self.disbursed - self.repaid_principal)
    }

    /// Vốn còn có thể rút. Hạn mức tín dụng quay vòng được (trả gốc → rút lại);
    /// các nguồn khác chỉ tính phần chưa giải ngân.
    pub fn available(&self) -> f64 {
        let used = if self.kind == "credit_line" {
            self.outstanding()
        } else {
            self.disbursed
        };
        round2((self.total_amount - used).max(0.0))
    }

    pub fn to_value(&self) -> Value {
        json!({
            "id": self.id,
            "name": self.name,
            "kind": self.kind,
            "provider": self.provider,
            "total_amount": self.total_amount,
            "currency": self.currency,
            "interest_rate": self.interest_rate,
            "rate_type": self.rate_type,
            "start_date": self.start_date,
            "end_date": self.end_date,
            "status": self.status,
            "note": self.note,
            "disbursed": self.disbursed,
            "repaid_principal": self.repaid_principal,
            "interest_paid": self.interest_paid,
            "fees_paid": self.fees_paid,
            "outstanding": self.outstanding(),
            "available": self.available(),
            "is_debt": finance::is_debt_kind(&self.kind),
        })
    }

    fn from_row(r: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Self {
            id: r.get(0)?,
            name: r.get(1)?,
            kind: r.get(2)?,
            provider: r.get(3)?,
            total_amount: r.get(4)?,
            currency: r.get(5)?,
            interest_rate: r.get(6)?,
            rate_type: r.get(7)?,
            start_date: r.get(8)?,
            end_date: r.get(9)?,
            status: r.get(10)?,
            note: r.get(11)?,
            disbursed: r.get(12)?,
            repaid_principal: r.get(13)?,
            interest_paid: r.get(14)?,
            fees_paid: r.get(15)?,
        })
    }
}

const SOURCE_SELECT: &str = r#"
SELECT s.id, s.name, s.kind, s.provider, s.total_amount, s.currency, s.interest_rate,
       s.rate_type, s.start_date, s.end_date, s.status, s.note,
       COALESCE(SUM(CASE WHEN t.kind='disburse'        THEN t.amount END),0),
       COALESCE(SUM(CASE WHEN t.kind='repay_principal' THEN t.amount END),0),
       COALESCE(SUM(CASE WHEN t.kind='repay_interest'  THEN t.amount END),0),
       COALESCE(SUM(CASE WHEN t.kind='fee'             THEN t.amount END),0)
FROM sources s LEFT JOIN transactions t ON t.source_id = s.id
"#;

impl Db {
    pub fn open_default() -> Result<Self> {
        let dir = std::env::var("SENCLAW_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                PathBuf::from(home)
                    .join(".senclaw")
                    .join("apps")
                    .join("capital")
            });
        std::fs::create_dir_all(&dir).ok();
        Self::open(dir.join("capital.db"))
    }

    pub fn open(path: PathBuf) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch(SCHEMA)?;
        // Migration for DBs created before paid_date existed; harmless when
        // the column is already there (duplicate-column error is ignored).
        let _ = conn.execute(
            "ALTER TABLE schedule ADD COLUMN paid_date TEXT NOT NULL DEFAULT ''",
            [],
        );
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
    // Kv store kept for forward-compat (alert thresholds, default currency…);
    // not yet surfaced in the UI.

    #[allow(dead_code)]
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

    #[allow(dead_code)]
    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO settings(key,value) VALUES(?1,?2)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    // ---- sources ----

    #[allow(clippy::too_many_arguments)]
    pub fn add_source(
        &self,
        name: &str,
        kind: &str,
        provider: &str,
        total_amount: f64,
        currency: &str,
        interest_rate: f64,
        rate_type: &str,
        start_date: &str,
        end_date: &str,
        note: &str,
    ) -> Result<i64> {
        if name.trim().is_empty() {
            return Err(anyhow!("tên nguồn vốn không được rỗng"));
        }
        if !finance::SOURCE_KINDS.contains(&kind) {
            return Err(anyhow!(
                "kind không hợp lệ: {kind} (hợp lệ: {})",
                finance::SOURCE_KINDS.join(", ")
            ));
        }
        if total_amount < 0.0 {
            return Err(anyhow!("total_amount phải ≥ 0"));
        }
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO sources(name,kind,provider,total_amount,currency,interest_rate,rate_type,start_date,end_date,note,created_at,updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?11)",
            params![name.trim(), kind, provider, total_amount,
                    if currency.is_empty() { "VND" } else { currency },
                    interest_rate, rate_type, start_date, end_date, note, now()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Patch-style update: only fields present in `patch` change. `status`
    /// accepts active|closed|pending.
    pub fn update_source(&self, id: i64, patch: &Value) -> Result<()> {
        if self.get_source(id).is_none() {
            return Err(anyhow!("nguồn vốn #{id} không tồn tại"));
        }
        if let Some(k) = patch.get("kind").and_then(|x| x.as_str()) {
            if !finance::SOURCE_KINDS.contains(&k) {
                return Err(anyhow!("kind không hợp lệ: {k}"));
            }
        }
        if let Some(st) = patch.get("status").and_then(|x| x.as_str()) {
            if !matches!(st, "active" | "closed" | "pending") {
                return Err(anyhow!("status không hợp lệ: {st}"));
            }
        }
        let conn = self.conn.lock().unwrap();
        let mut sets: Vec<String> = Vec::new();
        let mut vals: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        let push_str = |field: &str,
                        v: Option<&str>,
                        sets: &mut Vec<String>,
                        vals: &mut Vec<Box<dyn rusqlite::ToSql>>| {
            if let Some(v) = v {
                sets.push(format!("{field}=?{}", vals.len() + 1));
                vals.push(Box::new(v.to_string()));
            }
        };
        for f in [
            "name",
            "kind",
            "provider",
            "currency",
            "rate_type",
            "start_date",
            "end_date",
            "status",
            "note",
        ] {
            push_str(
                f,
                patch.get(f).and_then(|x| x.as_str()),
                &mut sets,
                &mut vals,
            );
        }
        for f in ["total_amount", "interest_rate"] {
            if let Some(v) = patch.get(f).and_then(|x| x.as_f64()) {
                sets.push(format!("{f}=?{}", vals.len() + 1));
                vals.push(Box::new(v));
            }
        }
        if sets.is_empty() {
            return Ok(());
        }
        sets.push(format!("updated_at=?{}", vals.len() + 1));
        vals.push(Box::new(now()));
        vals.push(Box::new(id));
        let sql = format!(
            "UPDATE sources SET {} WHERE id=?{}",
            sets.join(","),
            vals.len()
        );
        conn.execute(
            &sql,
            rusqlite::params_from_iter(vals.iter().map(|b| b.as_ref())),
        )?;
        Ok(())
    }

    pub fn get_source(&self, id: i64) -> Option<SourceRow> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            &format!("{SOURCE_SELECT} WHERE s.id=?1 GROUP BY s.id"),
            params![id],
            SourceRow::from_row,
        )
        .optional()
        .ok()
        .flatten()
    }

    pub fn list_sources(&self, status: Option<&str>) -> Vec<SourceRow> {
        let conn = self.conn.lock().unwrap();
        let (sql, filter) = match status {
            Some(st) => (
                format!("{SOURCE_SELECT} WHERE s.status=?1 GROUP BY s.id ORDER BY s.id"),
                Some(st.to_string()),
            ),
            None => (format!("{SOURCE_SELECT} GROUP BY s.id ORDER BY s.id"), None),
        };
        let mut stmt = conn.prepare(&sql).unwrap();
        let rows = match filter {
            Some(st) => stmt.query_map(params![st], SourceRow::from_row),
            None => stmt.query_map([], SourceRow::from_row),
        };
        rows.map(|it| it.filter_map(|r| r.ok()).collect())
            .unwrap_or_default()
    }

    // ---- transactions ----

    pub fn add_tx(
        &self,
        source_id: i64,
        alloc_id: Option<i64>,
        kind: &str,
        amount: f64,
        tx_date: &str,
        note: &str,
    ) -> Result<i64> {
        if !finance::TX_KINDS.contains(&kind) {
            return Err(anyhow!(
                "kind giao dịch không hợp lệ: {kind} (hợp lệ: {})",
                finance::TX_KINDS.join(", ")
            ));
        }
        if amount <= 0.0 {
            return Err(anyhow!("amount phải > 0"));
        }
        if self.get_source(source_id).is_none() {
            return Err(anyhow!("nguồn vốn #{source_id} không tồn tại"));
        }
        if let Some(aid) = alloc_id {
            let conn = self.conn.lock().unwrap();
            let exists: bool = conn
                .query_row(
                    "SELECT 1 FROM allocations WHERE id=?1",
                    params![aid],
                    |_| Ok(true),
                )
                .optional()?
                .unwrap_or(false);
            if !exists {
                return Err(anyhow!("phân bổ #{aid} không tồn tại"));
            }
        }
        let tx_date = if tx_date.trim().is_empty() {
            finance::today()
        } else {
            tx_date.trim().to_string()
        };
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO transactions(source_id,alloc_id,kind,amount,tx_date,note,created_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                source_id,
                alloc_id,
                kind,
                round2(amount),
                tx_date,
                note,
                now()
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn delete_tx(&self, id: i64) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.execute("DELETE FROM transactions WHERE id=?1", params![id])? > 0)
    }

    pub fn list_tx(
        &self,
        source_id: Option<i64>,
        kind: Option<&str>,
        alloc_id: Option<i64>,
        limit: i64,
    ) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut sql = String::from(
            "SELECT t.id, t.source_id, s.name, t.alloc_id, a.name, t.kind, t.amount, t.tx_date, t.note, t.created_at, s.currency
             FROM transactions t
             JOIN sources s ON s.id = t.source_id
             LEFT JOIN allocations a ON a.id = t.alloc_id WHERE 1=1",
        );
        let mut vals: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(sid) = source_id {
            vals.push(Box::new(sid));
            sql.push_str(&format!(" AND t.source_id=?{}", vals.len()));
        }
        if let Some(k) = kind {
            vals.push(Box::new(k.to_string()));
            sql.push_str(&format!(" AND t.kind=?{}", vals.len()));
        }
        if let Some(aid) = alloc_id {
            vals.push(Box::new(aid));
            sql.push_str(&format!(" AND t.alloc_id=?{}", vals.len()));
        }
        vals.push(Box::new(limit.clamp(1, 1000)));
        sql.push_str(&format!(
            " ORDER BY t.tx_date DESC, t.id DESC LIMIT ?{}",
            vals.len()
        ));
        let mut stmt = conn.prepare(&sql).unwrap();
        let rows = stmt.query_map(
            rusqlite::params_from_iter(vals.iter().map(|b| b.as_ref())),
            |r| {
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "source_id": r.get::<_, i64>(1)?,
                    "source_name": r.get::<_, String>(2)?,
                    "alloc_id": r.get::<_, Option<i64>>(3)?,
                    "alloc_name": r.get::<_, Option<String>>(4)?,
                    "kind": r.get::<_, String>(5)?,
                    "amount": r.get::<_, f64>(6)?,
                    "tx_date": r.get::<_, String>(7)?,
                    "note": r.get::<_, String>(8)?,
                    "created_at": r.get::<_, i64>(9)?,
                    "currency": r.get::<_, String>(10)?,
                }))
            },
        );
        rows.map(|it| it.filter_map(|r| r.ok()).collect())
            .unwrap_or_default()
    }

    // ---- allocations ----

    pub fn add_alloc(&self, name: &str, description: &str, target_amount: f64) -> Result<i64> {
        if name.trim().is_empty() {
            return Err(anyhow!("tên phân bổ không được rỗng"));
        }
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO allocations(name,description,target_amount,created_at) VALUES(?1,?2,?3,?4)",
            params![name.trim(), description, target_amount.max(0.0), now()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn update_alloc(&self, id: i64, patch: &Value) -> Result<()> {
        if let Some(st) = patch.get("status").and_then(|x| x.as_str()) {
            if !matches!(st, "active" | "done") {
                return Err(anyhow!("status phân bổ chỉ nhận active|done"));
            }
        }
        let conn = self.conn.lock().unwrap();
        let mut sets: Vec<String> = Vec::new();
        let mut vals: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        for f in ["name", "description", "status"] {
            if let Some(v) = patch.get(f).and_then(|x| x.as_str()) {
                sets.push(format!("{f}=?{}", vals.len() + 1));
                vals.push(Box::new(v.to_string()));
            }
        }
        if let Some(v) = patch.get("target_amount").and_then(|x| x.as_f64()) {
            sets.push(format!("target_amount=?{}", vals.len() + 1));
            vals.push(Box::new(v));
        }
        if sets.is_empty() {
            return Ok(());
        }
        vals.push(Box::new(id));
        let sql = format!(
            "UPDATE allocations SET {} WHERE id=?{}",
            sets.join(","),
            vals.len()
        );
        let n = conn.execute(
            &sql,
            rusqlite::params_from_iter(vals.iter().map(|b| b.as_ref())),
        )?;
        if n == 0 {
            return Err(anyhow!("phân bổ #{id} không tồn tại"));
        }
        Ok(())
    }

    /// Allocations with `used` = tổng giải ngân đã gắn vào phân bổ đó.
    pub fn list_allocs(&self) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT a.id, a.name, a.description, a.target_amount, a.status, a.created_at,
                        COALESCE(SUM(CASE WHEN t.kind='disburse' THEN t.amount END),0)
                 FROM allocations a LEFT JOIN transactions t ON t.alloc_id = a.id
                 GROUP BY a.id ORDER BY a.id",
            )
            .unwrap();
        let rows = stmt.query_map([], |r| {
            let target: f64 = r.get(3)?;
            let used: f64 = r.get(6)?;
            Ok(json!({
                "id": r.get::<_, i64>(0)?,
                "name": r.get::<_, String>(1)?,
                "description": r.get::<_, String>(2)?,
                "target_amount": target,
                "status": r.get::<_, String>(4)?,
                "created_at": r.get::<_, i64>(5)?,
                "used": round2(used),
                "remaining": round2((target - used).max(0.0)),
            }))
        });
        rows.map(|it| it.filter_map(|r| r.ok()).collect())
            .unwrap_or_default()
    }

    // ---- schedule ----

    /// Replace the UNPAID schedule of a source with a freshly generated one.
    /// Paid installments are history and stay untouched.
    pub fn replace_schedule(&self, source_id: i64, items: &[Installment]) -> Result<usize> {
        if self.get_source(source_id).is_none() {
            return Err(anyhow!("nguồn vốn #{source_id} không tồn tại"));
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM schedule WHERE source_id=?1 AND status!='paid'",
            params![source_id],
        )?;
        for it in items {
            tx.execute(
                "INSERT INTO schedule(source_id,seq,due_date,principal_due,interest_due) VALUES(?1,?2,?3,?4,?5)",
                params![source_id, it.seq, it.due_date, it.principal, it.interest],
            )?;
        }
        tx.commit()?;
        Ok(items.len())
    }

    /// List installments; `status` filter accepts upcoming|overdue|paid.
    /// "overdue" is derived: unpaid + due_date < `today`.
    pub fn list_schedule(
        &self,
        source_id: Option<i64>,
        status: Option<&str>,
        today: &str,
        limit: i64,
    ) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut sql = String::from(
            "SELECT sc.id, sc.source_id, s.name, sc.seq, sc.due_date, sc.principal_due,
                    sc.interest_due, sc.status, sc.paid_at, sc.note, s.currency, sc.paid_date
             FROM schedule sc JOIN sources s ON s.id = sc.source_id WHERE 1=1",
        );
        let mut vals: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(sid) = source_id {
            vals.push(Box::new(sid));
            sql.push_str(&format!(" AND sc.source_id=?{}", vals.len()));
        }
        match status {
            Some("paid") => sql.push_str(" AND sc.status='paid'"),
            Some("overdue") => {
                vals.push(Box::new(today.to_string()));
                sql.push_str(&format!(
                    " AND sc.status!='paid' AND sc.due_date < ?{}",
                    vals.len()
                ));
            }
            Some("upcoming") => {
                vals.push(Box::new(today.to_string()));
                sql.push_str(&format!(
                    " AND sc.status!='paid' AND sc.due_date >= ?{}",
                    vals.len()
                ));
            }
            _ => {}
        }
        vals.push(Box::new(limit.clamp(1, 2000)));
        sql.push_str(&format!(
            " ORDER BY sc.due_date, sc.id LIMIT ?{}",
            vals.len()
        ));
        let mut stmt = conn.prepare(&sql).unwrap();
        let rows = stmt.query_map(
            rusqlite::params_from_iter(vals.iter().map(|b| b.as_ref())),
            |r| {
                let raw_status: String = r.get(7)?;
                let due: String = r.get(4)?;
                let status = if raw_status != "paid" && due.as_str() < today {
                    "overdue".to_string()
                } else if raw_status != "paid" {
                    "upcoming".to_string()
                } else {
                    raw_status
                };
                let principal: f64 = r.get(5)?;
                let interest: f64 = r.get(6)?;
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "source_id": r.get::<_, i64>(1)?,
                    "source_name": r.get::<_, String>(2)?,
                    "seq": r.get::<_, i64>(3)?,
                    "due_date": due,
                    "principal_due": principal,
                    "interest_due": interest,
                    "total_due": round2(principal + interest),
                    "status": status,
                    "paid_at": r.get::<_, Option<i64>>(8)?,
                    "note": r.get::<_, String>(9)?,
                    "currency": r.get::<_, String>(10)?,
                    "paid_date": r.get::<_, String>(11)?,
                }))
            },
        );
        rows.map(|it| it.filter_map(|r| r.ok()).collect())
            .unwrap_or_default()
    }

    /// Mark an installment paid. When `create_tx` is true (the default path)
    /// the matching repay_principal / repay_interest ledger rows are inserted
    /// so dư nợ and lãi đã trả update automatically.
    pub fn pay_schedule(&self, id: i64, create_tx: bool, pay_date: &str) -> Result<Value> {
        let (source_id, principal, interest, status): (i64, f64, f64, String) = {
            let conn = self.conn.lock().unwrap();
            conn.query_row(
                "SELECT source_id, principal_due, interest_due, status FROM schedule WHERE id=?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?
            .ok_or_else(|| anyhow!("kỳ trả nợ #{id} không tồn tại"))?
        };
        if status == "paid" {
            return Err(anyhow!("kỳ trả nợ #{id} đã thanh toán rồi"));
        }
        let pay_date = if pay_date.trim().is_empty() {
            finance::today()
        } else {
            pay_date.trim().to_string()
        };
        let mut tx_ids = Vec::new();
        if create_tx {
            if principal > 0.0 {
                tx_ids.push(self.add_tx(
                    source_id,
                    None,
                    "repay_principal",
                    principal,
                    &pay_date,
                    &format!("kỳ trả nợ #{id}"),
                )?);
            }
            if interest > 0.0 {
                tx_ids.push(self.add_tx(
                    source_id,
                    None,
                    "repay_interest",
                    interest,
                    &pay_date,
                    &format!("kỳ trả nợ #{id}"),
                )?);
            }
        }
        {
            let conn = self.conn.lock().unwrap();
            conn.execute(
                "UPDATE schedule SET status='paid', paid_at=?2, paid_date=?3 WHERE id=?1",
                params![id, now(), pay_date],
            )?;
        }
        Ok(
            json!({ "ok": true, "schedule_id": id, "principal": principal, "interest": interest, "tx_ids": tx_ids }),
        )
    }

    // ---- reports ----

    /// Monthly cash flow (last `months`): inflow = giải ngân, outflow = trả gốc
    /// + trả lãi + phí, grouped by YYYY-MM.
    pub fn cashflow(&self, months: i64) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT substr(tx_date,1,7) ym,
                        COALESCE(SUM(CASE WHEN kind='disburse'        THEN amount END),0),
                        COALESCE(SUM(CASE WHEN kind='repay_principal' THEN amount END),0),
                        COALESCE(SUM(CASE WHEN kind='repay_interest'  THEN amount END),0),
                        COALESCE(SUM(CASE WHEN kind='fee'             THEN amount END),0)
                 FROM transactions GROUP BY ym ORDER BY ym DESC LIMIT ?1",
            )
            .unwrap();
        let mut rows: Vec<Value> = stmt
            .query_map(params![months.clamp(1, 120)], |r| {
                let inflow: f64 = r.get(1)?;
                let p: f64 = r.get(2)?;
                let i: f64 = r.get(3)?;
                let f: f64 = r.get(4)?;
                Ok(json!({
                    "month": r.get::<_, String>(0)?,
                    "inflow": round2(inflow),
                    "repay_principal": round2(p),
                    "repay_interest": round2(i),
                    "fees": round2(f),
                    "outflow": round2(p + i + f),
                    "net": round2(inflow - p - i - f),
                }))
            })
            .map(|it| it.filter_map(|r| r.ok()).collect())
            .unwrap_or_default();
        rows.reverse(); // oldest → newest for charting
        rows
    }

    /// The dashboard aggregate the UI, MCP and AI analysis all share.
    pub fn dashboard(&self, today: &str) -> Value {
        let sources = self.list_sources(None);
        let active: Vec<&SourceRow> = sources.iter().filter(|s| s.status == "active").collect();

        let mut equity_in = 0.0;
        let mut debt_outstanding = 0.0;
        let mut total_committed = 0.0;
        let mut total_disbursed = 0.0;
        let mut available = 0.0;
        let mut interest_paid = 0.0;
        let mut fees_paid = 0.0;
        let mut weighted_rate_num = 0.0;

        for s in &active {
            total_committed += s.total_amount;
            total_disbursed += s.disbursed;
            available += s.available();
            interest_paid += s.interest_paid;
            fees_paid += s.fees_paid;
            if finance::is_debt_kind(&s.kind) {
                let out = s.outstanding();
                debt_outstanding += out;
                weighted_rate_num += out * s.interest_rate;
            } else {
                equity_in += s.outstanding().max(0.0);
            }
        }
        let weighted_rate = if debt_outstanding > 0.0 {
            round2(weighted_rate_num / debt_outstanding)
        } else {
            0.0
        };
        let de_ratio = if equity_in > 0.0 {
            Some(round2(debt_outstanding / equity_in))
        } else {
            None
        };

        let horizon = finance::add_months(today, 1);
        let upcoming: Vec<Value> = self
            .list_schedule(None, Some("upcoming"), today, 500)
            .into_iter()
            .filter(|it| it["due_date"].as_str().unwrap_or("") <= horizon.as_str())
            .collect();
        let overdue = self.list_schedule(None, Some("overdue"), today, 500);
        let sum_due = |items: &[Value]| -> f64 {
            round2(
                items
                    .iter()
                    .map(|i| i["total_due"].as_f64().unwrap_or(0.0))
                    .sum(),
            )
        };

        json!({
            "today": today,
            "sources_active": active.len(),
            "sources_total": sources.len(),
            "equity_in": round2(equity_in),
            "debt_outstanding": round2(debt_outstanding),
            "total_committed": round2(total_committed),
            "total_disbursed": round2(total_disbursed),
            "available": round2(available),
            "interest_paid": round2(interest_paid),
            "fees_paid": round2(fees_paid),
            "weighted_debt_rate": weighted_rate,
            "de_ratio": de_ratio,
            "upcoming_30d": { "count": upcoming.len(), "total_due": sum_due(&upcoming), "items": upcoming },
            "overdue": { "count": overdue.len(), "total_due": sum_due(&overdue), "items": overdue },
            "cashflow_12m": self.cashflow(12),
            "sources": sources.iter().map(|s| s.to_value()).collect::<Vec<_>>(),
        })
    }

    // ---- goals & steps (mục tiêu + kế hoạch) ----

    pub const GOAL_KINDS: [&'static str; 5] = [
        "reduce_debt",
        "payoff_source",
        "raise_equity",
        "raise_funding",
        "build_reserve",
    ];

    #[allow(clippy::too_many_arguments)]
    pub fn add_goal(
        &self,
        name: &str,
        kind: &str,
        target_amount: f64,
        baseline: f64,
        source_id: Option<i64>,
        deadline: &str,
        note: &str,
        created_date: &str,
    ) -> Result<i64> {
        if name.trim().is_empty() {
            return Err(anyhow!("tên mục tiêu không được rỗng"));
        }
        if !Self::GOAL_KINDS.contains(&kind) {
            return Err(anyhow!(
                "kind mục tiêu không hợp lệ: {kind} (hợp lệ: {})",
                Self::GOAL_KINDS.join(", ")
            ));
        }
        if kind == "payoff_source" && source_id.is_none() {
            return Err(anyhow!("payoff_source cần 'source_id'"));
        }
        if let Some(sid) = source_id {
            if self.get_source(sid).is_none() {
                return Err(anyhow!("nguồn vốn #{sid} không tồn tại"));
            }
        }
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO goals(name,kind,target_amount,baseline,source_id,deadline,note,created_date,created_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![name.trim(), kind, target_amount.max(0.0), baseline, source_id, deadline, note, created_date, now()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_goal(&self, id: i64) -> Option<Value> {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT id,name,kind,target_amount,baseline,source_id,deadline,status,note,created_date FROM goals WHERE id=?1",
            params![id], goal_row)
            .optional()
            .ok()
            .flatten()
    }

    pub fn list_goals(&self, status: Option<&str>) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let base = "SELECT id,name,kind,target_amount,baseline,source_id,deadline,status,note,created_date FROM goals";
        let (sql, filter) = match status {
            Some(st) => (
                format!("{base} WHERE status=?1 ORDER BY id"),
                Some(st.to_string()),
            ),
            None => (format!("{base} ORDER BY id"), None),
        };
        let mut stmt = conn.prepare(&sql).unwrap();
        let rows = match filter {
            Some(st) => stmt.query_map(params![st], goal_row),
            None => stmt.query_map([], goal_row),
        };
        rows.map(|it| it.filter_map(|r| r.ok()).collect())
            .unwrap_or_default()
    }

    pub fn update_goal(&self, id: i64, patch: &Value) -> Result<()> {
        if let Some(st) = patch.get("status").and_then(|x| x.as_str()) {
            if !matches!(st, "active" | "done" | "cancelled") {
                return Err(anyhow!("status mục tiêu chỉ nhận active|done|cancelled"));
            }
        }
        let conn = self.conn.lock().unwrap();
        let mut sets: Vec<String> = Vec::new();
        let mut vals: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        for f in ["name", "deadline", "status", "note"] {
            if let Some(v) = patch.get(f).and_then(|x| x.as_str()) {
                sets.push(format!("{f}=?{}", vals.len() + 1));
                vals.push(Box::new(v.to_string()));
            }
        }
        if let Some(v) = patch.get("target_amount").and_then(|x| x.as_f64()) {
            sets.push(format!("target_amount=?{}", vals.len() + 1));
            vals.push(Box::new(v));
        }
        if sets.is_empty() {
            return Ok(());
        }
        vals.push(Box::new(id));
        let sql = format!(
            "UPDATE goals SET {} WHERE id=?{}",
            sets.join(","),
            vals.len()
        );
        let n = conn.execute(
            &sql,
            rusqlite::params_from_iter(vals.iter().map(|b| b.as_ref())),
        )?;
        if n == 0 {
            return Err(anyhow!("mục tiêu #{id} không tồn tại"));
        }
        Ok(())
    }

    pub fn add_step(
        &self,
        goal_id: i64,
        title: &str,
        due_date: &str,
        amount: f64,
        source: &str,
    ) -> Result<i64> {
        if self.get_goal(goal_id).is_none() {
            return Err(anyhow!("mục tiêu #{goal_id} không tồn tại"));
        }
        if title.trim().is_empty() {
            return Err(anyhow!("title bước không được rỗng"));
        }
        let conn = self.conn.lock().unwrap();
        let seq: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(seq),0)+1 FROM goal_steps WHERE goal_id=?1",
                params![goal_id],
                |r| r.get(0),
            )
            .unwrap_or(1);
        conn.execute(
            "INSERT INTO goal_steps(goal_id,seq,title,due_date,amount,source,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![goal_id, seq, title.trim(), due_date, amount, source, now()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_steps(&self, goal_id: i64) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id,seq,title,due_date,amount,status,source FROM goal_steps WHERE goal_id=?1 ORDER BY seq,id")
            .unwrap();
        let rows = stmt.query_map(params![goal_id], |r| {
            Ok(json!({
                "id": r.get::<_, i64>(0)?,
                "seq": r.get::<_, i64>(1)?,
                "title": r.get::<_, String>(2)?,
                "due_date": r.get::<_, String>(3)?,
                "amount": r.get::<_, f64>(4)?,
                "status": r.get::<_, String>(5)?,
                "source": r.get::<_, String>(6)?,
            }))
        });
        rows.map(|it| it.filter_map(|r| r.ok()).collect())
            .unwrap_or_default()
    }

    pub fn set_step_status(&self, step_id: i64, status: &str) -> Result<()> {
        if !matches!(status, "todo" | "done") {
            return Err(anyhow!("status bước chỉ nhận todo|done"));
        }
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "UPDATE goal_steps SET status=?2 WHERE id=?1",
            params![step_id, status],
        )?;
        if n == 0 {
            return Err(anyhow!("bước #{step_id} không tồn tại"));
        }
        Ok(())
    }

    pub fn delete_step(&self, step_id: i64) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.execute("DELETE FROM goal_steps WHERE id=?1", params![step_id])? > 0)
    }

    /// Before regenerating a plan: drop machine-generated steps that are still
    /// open; human-entered and completed steps are history and stay.
    pub fn clear_generated_todo_steps(&self, goal_id: i64) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.execute(
            "DELETE FROM goal_steps WHERE goal_id=?1 AND status='todo' AND source IN ('ai','auto')",
            params![goal_id],
        )?)
    }

    // ---- activity ----

    pub fn log(&self, kind: &str, text: &str, r#ref: &str) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO activity(kind,text,ref,created_at) VALUES(?1,?2,?3,?4)",
            params![kind, text, r#ref, now()],
        );
    }

    pub fn recent_activity(&self, limit: i64) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT kind,text,ref,created_at FROM activity ORDER BY id DESC LIMIT ?1")
            .unwrap();
        let rows = stmt
            .query_map(params![limit], |r| {
                Ok(json!({
                    "kind": r.get::<_, String>(0)?,
                    "text": r.get::<_, String>(1)?,
                    "ref": r.get::<_, String>(2)?,
                    "created_at": r.get::<_, i64>(3)?,
                }))
            })
            .unwrap();
        rows.filter_map(|r| r.ok()).collect()
    }
}

fn goal_row(r: &rusqlite::Row) -> rusqlite::Result<Value> {
    Ok(json!({
        "id": r.get::<_, i64>(0)?,
        "name": r.get::<_, String>(1)?,
        "kind": r.get::<_, String>(2)?,
        "target_amount": r.get::<_, f64>(3)?,
        "baseline": r.get::<_, f64>(4)?,
        "source_id": r.get::<_, Option<i64>>(5)?,
        "deadline": r.get::<_, String>(6)?,
        "status": r.get::<_, String>(7)?,
        "note": r.get::<_, String>(8)?,
        "created_date": r.get::<_, String>(9)?,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finance::generate_schedule;

    fn seed_loan(db: &Db) -> i64 {
        db.add_source(
            "Vay VCB",
            "bank_loan",
            "Vietcombank",
            2_000_000.0,
            "VND",
            9.0,
            "fixed",
            "2026-01-01",
            "2028-01-01",
            "",
        )
        .unwrap()
    }

    #[test]
    fn source_crud_and_aggregates() {
        let db = Db::open_memory().unwrap();
        let id = seed_loan(&db);
        db.add_tx(id, None, "disburse", 1_000_000.0, "2026-01-05", "đợt 1")
            .unwrap();
        db.add_tx(id, None, "repay_principal", 200_000.0, "2026-02-05", "")
            .unwrap();
        db.add_tx(id, None, "repay_interest", 7_500.0, "2026-02-05", "")
            .unwrap();
        db.add_tx(id, None, "fee", 1_000.0, "2026-01-05", "phí hồ sơ")
            .unwrap();
        let s = db.get_source(id).unwrap();
        assert_eq!(s.disbursed, 1_000_000.0);
        assert_eq!(s.outstanding(), 800_000.0);
        assert_eq!(s.interest_paid, 7_500.0);
        assert_eq!(s.fees_paid, 1_000.0);
        // bank_loan is not revolving: available = total - disbursed.
        assert_eq!(s.available(), 1_000_000.0);
    }

    #[test]
    fn credit_line_available_revolves() {
        let db = Db::open_memory().unwrap();
        let id = db
            .add_source(
                "HMTD BIDV",
                "credit_line",
                "BIDV",
                500_000.0,
                "VND",
                11.0,
                "floating",
                "",
                "",
                "",
            )
            .unwrap();
        db.add_tx(id, None, "disburse", 400_000.0, "2026-01-05", "")
            .unwrap();
        db.add_tx(id, None, "repay_principal", 300_000.0, "2026-02-05", "")
            .unwrap();
        let s = db.get_source(id).unwrap();
        assert_eq!(s.outstanding(), 100_000.0);
        assert_eq!(s.available(), 400_000.0); // trả gốc → rút lại được
    }

    #[test]
    fn invalid_inputs_rejected() {
        let db = Db::open_memory().unwrap();
        assert!(db
            .add_source("", "bank_loan", "", 1.0, "VND", 0.0, "fixed", "", "", "")
            .is_err());
        assert!(db
            .add_source("X", "ponzi", "", 1.0, "VND", 0.0, "fixed", "", "", "")
            .is_err());
        let id = seed_loan(&db);
        assert!(db.add_tx(id, None, "steal", 1.0, "", "").is_err());
        assert!(db.add_tx(id, None, "disburse", 0.0, "", "").is_err());
        assert!(db.add_tx(999, None, "disburse", 1.0, "", "").is_err());
        assert!(db.add_tx(id, Some(999), "disburse", 1.0, "", "").is_err());
    }

    #[test]
    fn update_source_patch() {
        let db = Db::open_memory().unwrap();
        let id = seed_loan(&db);
        db.update_source(id, &json!({ "status": "closed", "interest_rate": 8.5 }))
            .unwrap();
        let s = db.get_source(id).unwrap();
        assert_eq!(s.status, "closed");
        assert_eq!(s.interest_rate, 8.5);
        assert!(db
            .update_source(id, &json!({ "status": "vanished" }))
            .is_err());
        assert!(db.update_source(999, &json!({ "name": "x" })).is_err());
    }

    #[test]
    fn allocation_usage() {
        let db = Db::open_memory().unwrap();
        let sid = seed_loan(&db);
        let aid = db
            .add_alloc("Mở xưởng", "xưởng sản xuất", 800_000.0)
            .unwrap();
        db.add_tx(sid, Some(aid), "disburse", 500_000.0, "2026-01-10", "")
            .unwrap();
        let allocs = db.list_allocs();
        assert_eq!(allocs.len(), 1);
        assert_eq!(allocs[0]["used"], 500_000.0);
        assert_eq!(allocs[0]["remaining"], 300_000.0);
    }

    #[test]
    fn schedule_lifecycle_and_overdue_derivation() {
        let db = Db::open_memory().unwrap();
        let sid = seed_loan(&db);
        db.add_tx(sid, None, "disburse", 1_200.0, "2026-01-01", "")
            .unwrap();
        let items = generate_schedule("equal_principal", 1_200.0, 12.0, 12, "2026-01-01", 1);
        db.replace_schedule(sid, &items).unwrap();

        // As of 2026-03-15: Feb + Mar installments are overdue, rest upcoming.
        let overdue = db.list_schedule(None, Some("overdue"), "2026-03-15", 100);
        assert_eq!(overdue.len(), 2);
        assert_eq!(overdue[0]["status"], "overdue");
        let upcoming = db.list_schedule(None, Some("upcoming"), "2026-03-15", 100);
        assert_eq!(upcoming.len(), 10);

        // Pay the first installment → ledger rows appear, dư nợ drops.
        let first_id = overdue[0]["id"].as_i64().unwrap();
        let res = db.pay_schedule(first_id, true, "2026-03-16").unwrap();
        assert_eq!(res["ok"], true);
        let s = db.get_source(sid).unwrap();
        assert_eq!(s.outstanding(), 1_100.0);
        assert!(s.interest_paid > 0.0);
        // Double pay rejected.
        assert!(db.pay_schedule(first_id, true, "").is_err());

        // Regenerate keeps the paid row, replaces the unpaid ones.
        let items2 = generate_schedule("annuity", 1_100.0, 12.0, 11, "2026-02-01", 1);
        db.replace_schedule(sid, &items2).unwrap();
        let paid = db.list_schedule(Some(sid), Some("paid"), "2026-03-15", 100);
        assert_eq!(paid.len(), 1);
        let all = db.list_schedule(Some(sid), None, "2026-03-15", 100);
        assert_eq!(all.len(), 12); // 1 paid + 11 new
    }

    #[test]
    fn dashboard_metrics() {
        let db = Db::open_memory().unwrap();
        let eq = db
            .add_source(
                "Vốn chủ",
                "equity",
                "",
                1_000_000.0,
                "VND",
                0.0,
                "fixed",
                "",
                "",
                "",
            )
            .unwrap();
        db.add_tx(eq, None, "disburse", 1_000_000.0, "2026-01-01", "góp vốn")
            .unwrap();
        let loan = seed_loan(&db);
        db.add_tx(loan, None, "disburse", 500_000.0, "2026-01-02", "")
            .unwrap();

        let d = db.dashboard("2026-03-01");
        assert_eq!(d["equity_in"], 1_000_000.0);
        assert_eq!(d["debt_outstanding"], 500_000.0);
        assert_eq!(d["de_ratio"], 0.5);
        assert_eq!(d["weighted_debt_rate"], 9.0);
        assert_eq!(d["sources_active"], 2);
        // Closed sources drop out of the aggregates.
        db.update_source(loan, &json!({ "status": "closed" }))
            .unwrap();
        let d2 = db.dashboard("2026-03-01");
        assert_eq!(d2["debt_outstanding"], 0.0);
        assert_eq!(d2["sources_active"], 1);
    }

    #[test]
    fn goal_and_steps_lifecycle() {
        let db = Db::open_memory().unwrap();
        let sid = seed_loan(&db);
        // Validation.
        assert!(db
            .add_goal("", "reduce_debt", 1.0, 2.0, None, "", "", "2026-01-01")
            .is_err());
        assert!(db
            .add_goal("X", "get_rich", 1.0, 2.0, None, "", "", "2026-01-01")
            .is_err());
        assert!(db
            .add_goal("X", "payoff_source", 0.0, 2.0, None, "", "", "2026-01-01")
            .is_err()); // needs source_id
        assert!(db
            .add_goal(
                "X",
                "payoff_source",
                0.0,
                2.0,
                Some(999),
                "",
                "",
                "2026-01-01"
            )
            .is_err());

        let gid = db
            .add_goal(
                "Tất toán VCB",
                "payoff_source",
                0.0,
                800_000.0,
                Some(sid),
                "2027-06-30",
                "",
                "2026-07-27",
            )
            .unwrap();
        assert_eq!(db.list_goals(Some("active")).len(), 1);
        db.update_goal(gid, &json!({ "status": "done" })).unwrap();
        assert_eq!(db.get_goal(gid).unwrap()["status"], "done");
        assert!(db.update_goal(gid, &json!({ "status": "flying" })).is_err());

        // Steps: manual survives regeneration, generated todo does not.
        let s1 = db
            .add_step(gid, "Đàm phán lãi suất", "2026-08-15", 0.0, "manual")
            .unwrap();
        let s2 = db
            .add_step(gid, "Trả thêm 100tr", "2026-09-01", 100_000_000.0, "ai")
            .unwrap();
        let s3 = db
            .add_step(
                gid,
                "Trả thêm 100tr (T10)",
                "2026-10-01",
                100_000_000.0,
                "ai",
            )
            .unwrap();
        db.set_step_status(s3, "done").unwrap();
        assert_eq!(db.list_steps(gid).len(), 3);
        db.clear_generated_todo_steps(gid).unwrap();
        let left = db.list_steps(gid);
        assert_eq!(left.len(), 2, "manual + done ai step remain: {left:?}");
        assert!(db.delete_step(s1).unwrap());
        let _ = s2;
    }

    #[test]
    fn cashflow_grouping() {
        let db = Db::open_memory().unwrap();
        let sid = seed_loan(&db);
        db.add_tx(sid, None, "disburse", 100.0, "2026-01-10", "")
            .unwrap();
        db.add_tx(sid, None, "disburse", 50.0, "2026-01-20", "")
            .unwrap();
        db.add_tx(sid, None, "repay_principal", 30.0, "2026-02-01", "")
            .unwrap();
        db.add_tx(sid, None, "repay_interest", 5.0, "2026-02-01", "")
            .unwrap();
        let cf = db.cashflow(12);
        assert_eq!(cf.len(), 2);
        assert_eq!(cf[0]["month"], "2026-01");
        assert_eq!(cf[0]["inflow"], 150.0);
        assert_eq!(cf[1]["outflow"], 35.0);
        assert_eq!(cf[1]["net"], -35.0);
    }

    #[test]
    fn tx_list_filters() {
        let db = Db::open_memory().unwrap();
        let a = seed_loan(&db);
        let b = db
            .add_source(
                "Vay ACB",
                "bank_loan",
                "ACB",
                100.0,
                "VND",
                8.0,
                "fixed",
                "",
                "",
                "",
            )
            .unwrap();
        db.add_tx(a, None, "disburse", 10.0, "2026-01-01", "")
            .unwrap();
        db.add_tx(b, None, "disburse", 20.0, "2026-01-02", "")
            .unwrap();
        db.add_tx(b, None, "fee", 1.0, "2026-01-03", "").unwrap();
        assert_eq!(db.list_tx(Some(b), None, None, 100).len(), 2);
        assert_eq!(db.list_tx(None, Some("fee"), None, 100).len(), 1);
        assert_eq!(db.list_tx(None, None, None, 100).len(), 3);
        let id = db.list_tx(None, Some("fee"), None, 1)[0]["id"]
            .as_i64()
            .unwrap();
        assert!(db.delete_tx(id).unwrap());
        assert_eq!(db.list_tx(None, None, None, 100).len(), 2);
    }
}
