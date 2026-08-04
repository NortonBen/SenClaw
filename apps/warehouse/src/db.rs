//! Local SQLite store for the Warehouse app (quản lý kho hàng). Everything is
//! local-first — no external service holds this data. Tables:
//!   * `products`   — danh mục sản phẩm (SKU, đơn vị, giá vốn/giá bán, tồn tối thiểu)
//!   * `warehouses` — các kho / chi nhánh
//!   * `partners`   — nhà cung cấp / khách hàng
//!   * `moves`      — phiếu kho: nhập / xuất / chuyển kho / điều chỉnh kiểm kê
//!   * `move_lines` — dòng hàng của phiếu (sản phẩm, số lượng, đơn giá)
//!   * `activity`   — log hành động của app/agent
//!   * `settings`   — kv dự phòng
//!
//! Tồn kho KHÔNG lưu thành cột — luôn được suy ra từ `moves` + `move_lines`
//! (receipt/adjust cộng, issue trừ, transfer trừ kho đi cộng kho đến), nên sổ
//! không bao giờ lệch với chứng từ.

use crate::stock::{self, move_code, round2, round3};
use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Db {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS products (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  sku        TEXT NOT NULL DEFAULT '',
  name       TEXT NOT NULL,
  unit       TEXT NOT NULL DEFAULT 'cái',
  category   TEXT NOT NULL DEFAULT '',
  barcode    TEXT NOT NULL DEFAULT '',
  cost_price REAL NOT NULL DEFAULT 0,
  sell_price REAL NOT NULL DEFAULT 0,
  min_stock  REAL NOT NULL DEFAULT 0,
  status     TEXT NOT NULL DEFAULT 'active',
  note       TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_products_sku ON products(sku);
CREATE TABLE IF NOT EXISTS warehouses (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  name       TEXT NOT NULL,
  location   TEXT NOT NULL DEFAULT '',
  note       TEXT NOT NULL DEFAULT '',
  status     TEXT NOT NULL DEFAULT 'active',
  created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS partners (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  name       TEXT NOT NULL,
  kind       TEXT NOT NULL DEFAULT 'supplier',
  phone      TEXT NOT NULL DEFAULT '',
  address    TEXT NOT NULL DEFAULT '',
  note       TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS moves (
  id              INTEGER PRIMARY KEY AUTOINCREMENT,
  code            TEXT NOT NULL DEFAULT '',
  kind            TEXT NOT NULL,
  warehouse_id    INTEGER NOT NULL,
  to_warehouse_id INTEGER,
  partner_id      INTEGER,
  move_date       TEXT NOT NULL,
  note            TEXT NOT NULL DEFAULT '',
  created_at      INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_moves_kind ON moves(kind);
CREATE INDEX IF NOT EXISTS idx_moves_wh   ON moves(warehouse_id);
CREATE INDEX IF NOT EXISTS idx_moves_date ON moves(move_date);
CREATE TABLE IF NOT EXISTS move_lines (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  move_id    INTEGER NOT NULL,
  product_id INTEGER NOT NULL,
  qty        REAL NOT NULL,
  unit_price REAL NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_lines_move    ON move_lines(move_id);
CREATE INDEX IF NOT EXISTS idx_lines_product ON move_lines(product_id);
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

/// Signed per-(product, warehouse) contributions of every move line.
/// receipt/adjust add into `warehouse_id`, issue subtracts, transfer subtracts
/// from `warehouse_id` and (second branch) adds into `to_warehouse_id`.
const ONHAND_SRC: &str = r#"
SELECT l.product_id AS pid, m.warehouse_id AS wid,
       CASE m.kind WHEN 'issue' THEN -l.qty WHEN 'transfer' THEN -l.qty ELSE l.qty END AS q
FROM move_lines l JOIN moves m ON m.id = l.move_id
UNION ALL
SELECT l.product_id, m.to_warehouse_id,
       l.qty
FROM move_lines l JOIN moves m ON m.id = l.move_id
WHERE m.kind = 'transfer' AND m.to_warehouse_id IS NOT NULL
"#;

/// One line of a move as supplied by API/MCP callers.
#[derive(Debug, Clone)]
pub struct LineIn {
    pub product_id: i64,
    pub qty: f64,
    pub unit_price: f64,
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
                    .join("warehouse")
            });
        std::fs::create_dir_all(&dir).ok();
        let db = Self::open(dir.join("warehouse.db"))?;
        // First run: seed a default kho so phiếu can be created immediately.
        if db.list_warehouses(None).is_empty() {
            let _ = db.add_warehouse("Kho chính", "", "kho mặc định");
        }
        Ok(db)
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
    // Kv store kept for forward-compat (default warehouse, alert options…).

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

    // ---- products ----

    #[allow(clippy::too_many_arguments)]
    pub fn add_product(
        &self,
        sku: &str,
        name: &str,
        unit: &str,
        category: &str,
        barcode: &str,
        cost_price: f64,
        sell_price: f64,
        min_stock: f64,
        note: &str,
    ) -> Result<i64> {
        if name.trim().is_empty() {
            return Err(anyhow!("tên sản phẩm không được rỗng"));
        }
        if cost_price < 0.0 || sell_price < 0.0 || min_stock < 0.0 {
            return Err(anyhow!("giá và tồn tối thiểu phải ≥ 0"));
        }
        let conn = self.conn.lock().unwrap();
        if !sku.trim().is_empty() {
            let dup: bool = conn
                .query_row(
                    "SELECT 1 FROM products WHERE sku=?1",
                    params![sku.trim()],
                    |_| Ok(true),
                )
                .optional()?
                .unwrap_or(false);
            if dup {
                return Err(anyhow!("SKU \"{}\" đã tồn tại", sku.trim()));
            }
        }
        conn.execute(
            "INSERT INTO products(sku,name,unit,category,barcode,cost_price,sell_price,min_stock,note,created_at,updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?10)",
            params![
                sku.trim(),
                name.trim(),
                if unit.trim().is_empty() { "cái" } else { unit.trim() },
                category.trim(),
                barcode.trim(),
                cost_price,
                sell_price,
                min_stock,
                note,
                now()
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Patch-style update: only fields present in `patch` change.
    pub fn update_product(&self, id: i64, patch: &Value) -> Result<()> {
        if let Some(st) = patch.get("status").and_then(|x| x.as_str()) {
            if !matches!(st, "active" | "inactive") {
                return Err(anyhow!("status sản phẩm chỉ nhận active|inactive"));
            }
        }
        let conn = self.conn.lock().unwrap();
        if let Some(sku) = patch.get("sku").and_then(|x| x.as_str()) {
            if !sku.trim().is_empty() {
                let dup: bool = conn
                    .query_row(
                        "SELECT 1 FROM products WHERE sku=?1 AND id!=?2",
                        params![sku.trim(), id],
                        |_| Ok(true),
                    )
                    .optional()?
                    .unwrap_or(false);
                if dup {
                    return Err(anyhow!("SKU \"{}\" đã tồn tại", sku.trim()));
                }
            }
        }
        let mut sets: Vec<String> = Vec::new();
        let mut vals: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        for f in [
            "sku", "name", "unit", "category", "barcode", "status", "note",
        ] {
            if let Some(v) = patch.get(f).and_then(|x| x.as_str()) {
                sets.push(format!("{f}=?{}", vals.len() + 1));
                vals.push(Box::new(v.trim().to_string()));
            }
        }
        for f in ["cost_price", "sell_price", "min_stock"] {
            if let Some(v) = patch.get(f).and_then(|x| x.as_f64()) {
                if v < 0.0 {
                    return Err(anyhow!("{f} phải ≥ 0"));
                }
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
            "UPDATE products SET {} WHERE id=?{}",
            sets.join(","),
            vals.len()
        );
        let n = conn.execute(
            &sql,
            rusqlite::params_from_iter(vals.iter().map(|b| b.as_ref())),
        )?;
        if n == 0 {
            return Err(anyhow!("sản phẩm #{id} không tồn tại"));
        }
        Ok(())
    }

    fn product_exists(conn: &Connection, id: i64) -> bool {
        conn.query_row("SELECT 1 FROM products WHERE id=?1", params![id], |_| {
            Ok(true)
        })
        .optional()
        .ok()
        .flatten()
        .unwrap_or(false)
    }

    /// Aggregates shared by product list/get: on-hand total per product and
    /// weighted-average receipt cost per product.
    fn product_aggregates(conn: &Connection) -> (BTreeMap<i64, f64>, BTreeMap<i64, f64>) {
        let mut onhand: BTreeMap<i64, f64> = BTreeMap::new();
        let mut stmt = conn
            .prepare(&format!(
                "SELECT pid, SUM(q) FROM ({ONHAND_SRC}) GROUP BY pid"
            ))
            .unwrap();
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?)))
            .unwrap();
        for r in rows.flatten() {
            onhand.insert(r.0, round3(r.1));
        }
        let mut avg: BTreeMap<i64, f64> = BTreeMap::new();
        let mut stmt = conn
            .prepare(
                "SELECT l.product_id, SUM(l.qty*l.unit_price), SUM(l.qty)
                 FROM move_lines l JOIN moves m ON m.id=l.move_id
                 WHERE m.kind='receipt' GROUP BY l.product_id",
            )
            .unwrap();
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, f64>(1)?,
                    r.get::<_, f64>(2)?,
                ))
            })
            .unwrap();
        for (pid, val, qty) in rows.flatten() {
            if qty > 0.0 {
                avg.insert(pid, val / qty);
            }
        }
        (onhand, avg)
    }

    /// List products with derived numbers: `on_hand`, `avg_cost` (bình quân
    /// gia quyền theo phiếu nhập, fallback giá vốn khai báo), `stock_value`,
    /// `low_stock` flag. Filters: text `q` (name/sku/barcode), category, status,
    /// `low_only`.
    pub fn list_products(
        &self,
        q: Option<&str>,
        category: Option<&str>,
        status: Option<&str>,
        low_only: bool,
    ) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let (onhand, avg) = Self::product_aggregates(&conn);
        let mut sql = String::from(
            "SELECT id, sku, name, unit, category, barcode, cost_price, sell_price, min_stock, status, note FROM products WHERE 1=1",
        );
        let mut vals: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(text) = q {
            let pat = format!("%{}%", text.trim());
            vals.push(Box::new(pat.clone()));
            sql.push_str(&format!(
                " AND (name LIKE ?{n} OR sku LIKE ?{n} OR barcode LIKE ?{n})",
                n = vals.len()
            ));
        }
        if let Some(c) = category {
            vals.push(Box::new(c.to_string()));
            sql.push_str(&format!(" AND category=?{}", vals.len()));
        }
        if let Some(st) = status {
            vals.push(Box::new(st.to_string()));
            sql.push_str(&format!(" AND status=?{}", vals.len()));
        }
        sql.push_str(" ORDER BY id");
        let mut stmt = conn.prepare(&sql).unwrap();
        let rows = stmt.query_map(
            rusqlite::params_from_iter(vals.iter().map(|b| b.as_ref())),
            |r| {
                let id: i64 = r.get(0)?;
                let cost_price: f64 = r.get(6)?;
                let min_stock: f64 = r.get(8)?;
                Ok((
                    id,
                    cost_price,
                    min_stock,
                    json!({
                        "id": id,
                        "sku": r.get::<_, String>(1)?,
                        "name": r.get::<_, String>(2)?,
                        "unit": r.get::<_, String>(3)?,
                        "category": r.get::<_, String>(4)?,
                        "barcode": r.get::<_, String>(5)?,
                        "cost_price": cost_price,
                        "sell_price": r.get::<_, f64>(7)?,
                        "min_stock": min_stock,
                        "status": r.get::<_, String>(9)?,
                        "note": r.get::<_, String>(10)?,
                    }),
                ))
            },
        );
        let mut out = Vec::new();
        for (id, cost_price, min_stock, mut v) in rows
            .map(|it| it.filter_map(|r| r.ok()).collect::<Vec<_>>())
            .unwrap_or_default()
        {
            let on = *onhand.get(&id).unwrap_or(&0.0);
            let ac = *avg.get(&id).unwrap_or(&cost_price);
            let low = min_stock > 0.0 && on < min_stock;
            if low_only && !low {
                continue;
            }
            v["on_hand"] = json!(round3(on));
            v["avg_cost"] = json!(round2(ac));
            v["stock_value"] = json!(round2(on.max(0.0) * ac));
            v["low_stock"] = json!(low);
            out.push(v);
        }
        out
    }

    pub fn get_product(&self, id: i64) -> Option<Value> {
        self.list_products(None, None, None, false)
            .into_iter()
            .find(|p| p["id"] == id)
    }

    /// Per-warehouse on-hand rows, optionally filtered. Each row carries
    /// product/warehouse names, avg cost and value so callers need no joins.
    pub fn stock_onhand(&self, product_id: Option<i64>, warehouse_id: Option<i64>) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let (_, avg) = Self::product_aggregates(&conn);
        let mut sql = format!(
            "SELECT s.pid, p.sku, p.name, p.unit, p.cost_price, s.wid, w.name, SUM(s.q) qty
             FROM ({ONHAND_SRC}) s
             JOIN products p ON p.id = s.pid
             JOIN warehouses w ON w.id = s.wid WHERE 1=1"
        );
        let mut vals: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(pid) = product_id {
            vals.push(Box::new(pid));
            sql.push_str(&format!(" AND s.pid=?{}", vals.len()));
        }
        if let Some(wid) = warehouse_id {
            vals.push(Box::new(wid));
            sql.push_str(&format!(" AND s.wid=?{}", vals.len()));
        }
        sql.push_str(" GROUP BY s.pid, s.wid HAVING ABS(SUM(s.q)) > 0.0005 ORDER BY p.id, s.wid");
        let mut stmt = conn.prepare(&sql).unwrap();
        let rows = stmt.query_map(
            rusqlite::params_from_iter(vals.iter().map(|b| b.as_ref())),
            |r| {
                let pid: i64 = r.get(0)?;
                let cost_price: f64 = r.get(4)?;
                let qty: f64 = r.get(7)?;
                Ok((
                    pid,
                    cost_price,
                    qty,
                    json!({
                        "product_id": pid,
                        "sku": r.get::<_, String>(1)?,
                        "product_name": r.get::<_, String>(2)?,
                        "unit": r.get::<_, String>(3)?,
                        "warehouse_id": r.get::<_, i64>(5)?,
                        "warehouse_name": r.get::<_, String>(6)?,
                    }),
                ))
            },
        );
        rows.map(|it| {
            it.filter_map(|r| r.ok())
                .map(|(pid, cost_price, qty, mut v)| {
                    let ac = *avg.get(&pid).unwrap_or(&cost_price);
                    v["qty"] = json!(round3(qty));
                    v["avg_cost"] = json!(round2(ac));
                    v["value"] = json!(round2(qty.max(0.0) * ac));
                    v
                })
                .collect()
        })
        .unwrap_or_default()
    }

    // ---- warehouses ----

    pub fn add_warehouse(&self, name: &str, location: &str, note: &str) -> Result<i64> {
        if name.trim().is_empty() {
            return Err(anyhow!("tên kho không được rỗng"));
        }
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO warehouses(name,location,note,created_at) VALUES(?1,?2,?3,?4)",
            params![name.trim(), location.trim(), note, now()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn update_warehouse(&self, id: i64, patch: &Value) -> Result<()> {
        if let Some(st) = patch.get("status").and_then(|x| x.as_str()) {
            if !matches!(st, "active" | "inactive") {
                return Err(anyhow!("status kho chỉ nhận active|inactive"));
            }
        }
        let conn = self.conn.lock().unwrap();
        let mut sets: Vec<String> = Vec::new();
        let mut vals: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        for f in ["name", "location", "note", "status"] {
            if let Some(v) = patch.get(f).and_then(|x| x.as_str()) {
                sets.push(format!("{f}=?{}", vals.len() + 1));
                vals.push(Box::new(v.to_string()));
            }
        }
        if sets.is_empty() {
            return Ok(());
        }
        vals.push(Box::new(id));
        let sql = format!(
            "UPDATE warehouses SET {} WHERE id=?{}",
            sets.join(","),
            vals.len()
        );
        let n = conn.execute(
            &sql,
            rusqlite::params_from_iter(vals.iter().map(|b| b.as_ref())),
        )?;
        if n == 0 {
            return Err(anyhow!("kho #{id} không tồn tại"));
        }
        Ok(())
    }

    fn warehouse_exists(conn: &Connection, id: i64) -> bool {
        conn.query_row("SELECT 1 FROM warehouses WHERE id=?1", params![id], |_| {
            Ok(true)
        })
        .optional()
        .ok()
        .flatten()
        .unwrap_or(false)
    }

    /// Warehouses with derived `sku_count` and `stock_value`.
    pub fn list_warehouses(&self, status: Option<&str>) -> Vec<Value> {
        let onhand = self.stock_onhand(None, None);
        let conn = self.conn.lock().unwrap();
        let (sql, filter) = match status {
            Some(st) => ("SELECT id,name,location,note,status,created_at FROM warehouses WHERE status=?1 ORDER BY id", Some(st.to_string())),
            None => ("SELECT id,name,location,note,status,created_at FROM warehouses ORDER BY id", None),
        };
        let mut stmt = conn.prepare(sql).unwrap();
        let map_row = |r: &rusqlite::Row| -> rusqlite::Result<Value> {
            Ok(json!({
                "id": r.get::<_, i64>(0)?,
                "name": r.get::<_, String>(1)?,
                "location": r.get::<_, String>(2)?,
                "note": r.get::<_, String>(3)?,
                "status": r.get::<_, String>(4)?,
                "created_at": r.get::<_, i64>(5)?,
            }))
        };
        let rows = match filter {
            Some(st) => stmt.query_map(params![st], map_row),
            None => stmt.query_map([], map_row),
        };
        rows.map(|it| {
            it.filter_map(|r| r.ok())
                .map(|mut v| {
                    let wid = v["id"].as_i64().unwrap_or(0);
                    let mine: Vec<&Value> =
                        onhand.iter().filter(|o| o["warehouse_id"] == wid).collect();
                    v["sku_count"] = json!(mine.len());
                    v["stock_value"] = json!(round2(
                        mine.iter()
                            .map(|o| o["value"].as_f64().unwrap_or(0.0))
                            .sum()
                    ));
                    v
                })
                .collect()
        })
        .unwrap_or_default()
    }

    // ---- partners ----

    pub fn add_partner(
        &self,
        name: &str,
        kind: &str,
        phone: &str,
        address: &str,
        note: &str,
    ) -> Result<i64> {
        if name.trim().is_empty() {
            return Err(anyhow!("tên đối tác không được rỗng"));
        }
        if !stock::PARTNER_KINDS.contains(&kind) {
            return Err(anyhow!(
                "kind đối tác không hợp lệ: {kind} (hợp lệ: {})",
                stock::PARTNER_KINDS.join(", ")
            ));
        }
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO partners(name,kind,phone,address,note,created_at) VALUES(?1,?2,?3,?4,?5,?6)",
            params![name.trim(), kind, phone.trim(), address.trim(), note, now()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_partners(&self, kind: Option<&str>) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let (sql, filter) = match kind {
            Some(k) => ("SELECT id,name,kind,phone,address,note,created_at FROM partners WHERE kind=?1 ORDER BY id", Some(k.to_string())),
            None => ("SELECT id,name,kind,phone,address,note,created_at FROM partners ORDER BY id", None),
        };
        let mut stmt = conn.prepare(sql).unwrap();
        let map_row = |r: &rusqlite::Row| -> rusqlite::Result<Value> {
            Ok(json!({
                "id": r.get::<_, i64>(0)?,
                "name": r.get::<_, String>(1)?,
                "kind": r.get::<_, String>(2)?,
                "phone": r.get::<_, String>(3)?,
                "address": r.get::<_, String>(4)?,
                "note": r.get::<_, String>(5)?,
                "created_at": r.get::<_, i64>(6)?,
            }))
        };
        let rows = match filter {
            Some(k) => stmt.query_map(params![k], map_row),
            None => stmt.query_map([], map_row),
        };
        rows.map(|it| it.filter_map(|r| r.ok()).collect())
            .unwrap_or_default()
    }

    // ---- moves ----

    fn onhand_for(conn: &Connection, product_id: i64, warehouse_id: i64) -> f64 {
        conn.query_row(
            &format!("SELECT COALESCE(SUM(q),0) FROM ({ONHAND_SRC}) WHERE pid=?1 AND wid=?2"),
            params![product_id, warehouse_id],
            |r| r.get::<_, f64>(0),
        )
        .unwrap_or(0.0)
    }

    /// Create a phiếu kho with its lines, all-or-nothing.
    ///   * receipt  — cộng vào `warehouse_id`; `unit_price` = giá vốn nhập
    ///   * issue    — trừ khỏi `warehouse_id`; `unit_price` = giá bán/xuất
    ///   * transfer — cần `to_warehouse_id` khác kho đi
    ///   * adjust   — `qty` là DELTA có dấu (kiểm kê thừa dương, thiếu âm)
    /// Xuất/chuyển/điều chỉnh âm không được vượt tồn hiện có (không cho âm kho).
    #[allow(clippy::too_many_arguments)]
    pub fn create_move(
        &self,
        kind: &str,
        warehouse_id: i64,
        to_warehouse_id: Option<i64>,
        partner_id: Option<i64>,
        move_date: &str,
        note: &str,
        lines: &[LineIn],
    ) -> Result<Value> {
        if !stock::is_move_kind(kind) {
            return Err(anyhow!(
                "kind phiếu không hợp lệ: {kind} (hợp lệ: {})",
                stock::MOVE_KINDS.join(", ")
            ));
        }
        if lines.is_empty() {
            return Err(anyhow!("phiếu phải có ít nhất 1 dòng hàng"));
        }
        let mut conn = self.conn.lock().unwrap();
        if !Self::warehouse_exists(&conn, warehouse_id) {
            return Err(anyhow!("kho #{warehouse_id} không tồn tại"));
        }
        let to_wid = match (kind, to_warehouse_id) {
            ("transfer", Some(t)) => {
                if t == warehouse_id {
                    return Err(anyhow!("kho đến phải khác kho đi"));
                }
                if !Self::warehouse_exists(&conn, t) {
                    return Err(anyhow!("kho đến #{t} không tồn tại"));
                }
                Some(t)
            }
            ("transfer", None) => return Err(anyhow!("phiếu chuyển kho cần 'to_warehouse_id'")),
            (_, Some(_)) => return Err(anyhow!("'to_warehouse_id' chỉ dùng cho phiếu chuyển kho")),
            (_, None) => None,
        };
        if let Some(pid) = partner_id {
            let ok: bool = conn
                .query_row("SELECT 1 FROM partners WHERE id=?1", params![pid], |_| {
                    Ok(true)
                })
                .optional()?
                .unwrap_or(false);
            if !ok {
                return Err(anyhow!("đối tác #{pid} không tồn tại"));
            }
        }
        // Validate lines + aggregate outbound demand per product.
        let mut need: BTreeMap<i64, f64> = BTreeMap::new();
        for l in lines {
            if !Self::product_exists(&conn, l.product_id) {
                return Err(anyhow!("sản phẩm #{} không tồn tại", l.product_id));
            }
            if l.unit_price < 0.0 {
                return Err(anyhow!("đơn giá phải ≥ 0"));
            }
            match kind {
                "adjust" => {
                    if l.qty == 0.0 {
                        return Err(anyhow!("dòng điều chỉnh phải có qty ≠ 0 (delta có dấu)"));
                    }
                    if l.qty < 0.0 {
                        *need.entry(l.product_id).or_default() += -l.qty;
                    }
                }
                _ => {
                    if l.qty <= 0.0 {
                        return Err(anyhow!("qty phải > 0"));
                    }
                    if kind != "receipt" {
                        *need.entry(l.product_id).or_default() += l.qty;
                    }
                }
            }
        }
        for (pid, qty) in &need {
            let have = Self::onhand_for(&conn, *pid, warehouse_id);
            if have + 0.0005 < *qty {
                let name: String = conn
                    .query_row("SELECT name FROM products WHERE id=?1", params![pid], |r| {
                        r.get(0)
                    })
                    .unwrap_or_else(|_| format!("#{pid}"));
                return Err(anyhow!(
                    "tồn kho không đủ: \"{name}\" còn {} nhưng cần {qty}",
                    round3(have)
                ));
            }
        }
        let move_date = if move_date.trim().is_empty() {
            stock::today()
        } else {
            move_date.trim().to_string()
        };
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO moves(code,kind,warehouse_id,to_warehouse_id,partner_id,move_date,note,created_at)
             VALUES('',?1,?2,?3,?4,?5,?6,?7)",
            params![kind, warehouse_id, to_wid, partner_id, move_date, note, now()],
        )?;
        let id = tx.last_insert_rowid();
        let code = move_code(kind, id);
        tx.execute("UPDATE moves SET code=?2 WHERE id=?1", params![id, code])?;
        for l in lines {
            tx.execute(
                "INSERT INTO move_lines(move_id,product_id,qty,unit_price) VALUES(?1,?2,?3,?4)",
                params![id, l.product_id, round3(l.qty), round2(l.unit_price)],
            )?;
        }
        tx.commit()?;
        drop(conn);
        Ok(self
            .get_move(id)
            .unwrap_or_else(|| json!({ "ok": true, "move_id": id, "code": code })))
    }

    /// Delete a phiếu. Refused when removal would push any (product, kho)
    /// balance negative — e.g. deleting a receipt whose goods were already
    /// issued — so the ledger stays consistent.
    pub fn delete_move(&self, id: i64) -> Result<Value> {
        let mut conn = self.conn.lock().unwrap();
        let code: String = conn
            .query_row("SELECT code FROM moves WHERE id=?1", params![id], |r| {
                r.get(0)
            })
            .optional()?
            .ok_or_else(|| anyhow!("phiếu #{id} không tồn tại"))?;
        // Affected (product, warehouse) pairs to re-check after removal.
        let mut affected: BTreeSet<(i64, i64)> = BTreeSet::new();
        {
            let mut stmt = conn.prepare(
                "SELECT l.product_id, m.warehouse_id, m.to_warehouse_id
                 FROM move_lines l JOIN moves m ON m.id=l.move_id WHERE m.id=?1",
            )?;
            let rows = stmt.query_map(params![id], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                ))
            })?;
            for (pid, wid, to) in rows.flatten() {
                affected.insert((pid, wid));
                if let Some(t) = to {
                    affected.insert((pid, t));
                }
            }
        }
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM move_lines WHERE move_id=?1", params![id])?;
        tx.execute("DELETE FROM moves WHERE id=?1", params![id])?;
        for (pid, wid) in &affected {
            let left: f64 = tx
                .query_row(
                    &format!(
                        "SELECT COALESCE(SUM(q),0) FROM ({ONHAND_SRC}) WHERE pid=?1 AND wid=?2"
                    ),
                    params![pid, wid],
                    |r| r.get(0),
                )
                .unwrap_or(0.0);
            if left < -0.0005 {
                // Dropping the tx without commit rolls the delete back.
                return Err(anyhow!(
                    "không xoá được {code}: xoá xong tồn của sản phẩm #{pid} tại kho #{wid} sẽ âm ({})",
                    round3(left)
                ));
            }
        }
        tx.commit()?;
        Ok(json!({ "ok": true, "deleted": code }))
    }

    /// Move headers with per-move line totals. Filters: kind, warehouse
    /// (either side), product, date range.
    #[allow(clippy::too_many_arguments)]
    pub fn list_moves(
        &self,
        kind: Option<&str>,
        warehouse_id: Option<i64>,
        product_id: Option<i64>,
        date_from: Option<&str>,
        date_to: Option<&str>,
        limit: i64,
    ) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut sql = String::from(
            "SELECT m.id, m.code, m.kind, m.warehouse_id, w.name, m.to_warehouse_id, w2.name,
                    m.partner_id, pa.name, m.move_date, m.note, m.created_at,
                    (SELECT COUNT(*) FROM move_lines l WHERE l.move_id=m.id),
                    (SELECT COALESCE(SUM(l.qty),0) FROM move_lines l WHERE l.move_id=m.id),
                    (SELECT COALESCE(SUM(l.qty*l.unit_price),0) FROM move_lines l WHERE l.move_id=m.id)
             FROM moves m
             JOIN warehouses w ON w.id=m.warehouse_id
             LEFT JOIN warehouses w2 ON w2.id=m.to_warehouse_id
             LEFT JOIN partners pa ON pa.id=m.partner_id WHERE 1=1",
        );
        let mut vals: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(k) = kind {
            vals.push(Box::new(k.to_string()));
            sql.push_str(&format!(" AND m.kind=?{}", vals.len()));
        }
        if let Some(wid) = warehouse_id {
            vals.push(Box::new(wid));
            sql.push_str(&format!(
                " AND (m.warehouse_id=?{n} OR m.to_warehouse_id=?{n})",
                n = vals.len()
            ));
        }
        if let Some(pid) = product_id {
            vals.push(Box::new(pid));
            sql.push_str(&format!(
                " AND EXISTS(SELECT 1 FROM move_lines l WHERE l.move_id=m.id AND l.product_id=?{})",
                vals.len()
            ));
        }
        if let Some(d) = date_from {
            vals.push(Box::new(d.to_string()));
            sql.push_str(&format!(" AND m.move_date >= ?{}", vals.len()));
        }
        if let Some(d) = date_to {
            vals.push(Box::new(d.to_string()));
            sql.push_str(&format!(" AND m.move_date <= ?{}", vals.len()));
        }
        vals.push(Box::new(limit.clamp(1, 1000)));
        sql.push_str(&format!(
            " ORDER BY m.move_date DESC, m.id DESC LIMIT ?{}",
            vals.len()
        ));
        let mut stmt = conn.prepare(&sql).unwrap();
        let rows = stmt.query_map(
            rusqlite::params_from_iter(vals.iter().map(|b| b.as_ref())),
            |r| {
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "code": r.get::<_, String>(1)?,
                    "kind": r.get::<_, String>(2)?,
                    "warehouse_id": r.get::<_, i64>(3)?,
                    "warehouse_name": r.get::<_, String>(4)?,
                    "to_warehouse_id": r.get::<_, Option<i64>>(5)?,
                    "to_warehouse_name": r.get::<_, Option<String>>(6)?,
                    "partner_id": r.get::<_, Option<i64>>(7)?,
                    "partner_name": r.get::<_, Option<String>>(8)?,
                    "move_date": r.get::<_, String>(9)?,
                    "note": r.get::<_, String>(10)?,
                    "created_at": r.get::<_, i64>(11)?,
                    "line_count": r.get::<_, i64>(12)?,
                    "total_qty": round3(r.get::<_, f64>(13)?),
                    "total_value": round2(r.get::<_, f64>(14)?),
                }))
            },
        );
        rows.map(|it| it.filter_map(|r| r.ok()).collect())
            .unwrap_or_default()
    }

    /// One move with its lines (product names joined in).
    pub fn get_move(&self, id: i64) -> Option<Value> {
        let mut header = {
            let list = self.list_moves(None, None, None, None, None, 1000);
            list.into_iter().find(|m| m["id"] == id)?
        };
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT l.id, l.product_id, p.sku, p.name, p.unit, l.qty, l.unit_price
                 FROM move_lines l JOIN products p ON p.id=l.product_id
                 WHERE l.move_id=?1 ORDER BY l.id",
            )
            .unwrap();
        let lines: Vec<Value> = stmt
            .query_map(params![id], |r| {
                let qty: f64 = r.get(5)?;
                let price: f64 = r.get(6)?;
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "product_id": r.get::<_, i64>(1)?,
                    "sku": r.get::<_, String>(2)?,
                    "product_name": r.get::<_, String>(3)?,
                    "unit": r.get::<_, String>(4)?,
                    "qty": qty,
                    "unit_price": price,
                    "amount": round2(qty * price),
                }))
            })
            .map(|it| it.filter_map(|r| r.ok()).collect())
            .unwrap_or_default();
        header["lines"] = json!(lines);
        Some(header)
    }

    // ---- thẻ kho (stock card) ----

    /// Ledger of one product ordered by date with running balance. With a
    /// `warehouse_id` filter the balance is per-kho (transfer shows only the
    /// side touching that kho); without it, transfers net to zero and the
    /// balance is company-wide.
    pub fn stock_card(&self, product_id: i64, warehouse_id: Option<i64>, limit: i64) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT m.id, m.code, m.kind, m.move_date, m.warehouse_id, w.name,
                        m.to_warehouse_id, w2.name, l.qty, l.unit_price, m.note
                 FROM move_lines l
                 JOIN moves m ON m.id=l.move_id
                 JOIN warehouses w ON w.id=m.warehouse_id
                 LEFT JOIN warehouses w2 ON w2.id=m.to_warehouse_id
                 WHERE l.product_id=?1 ORDER BY m.move_date, m.id, l.id",
            )
            .unwrap();
        struct Raw {
            code: String,
            kind: String,
            date: String,
            wid: i64,
            wname: String,
            to_wid: Option<i64>,
            to_wname: Option<String>,
            qty: f64,
            price: f64,
            note: String,
        }
        let raws: Vec<Raw> = stmt
            .query_map(params![product_id], |r| {
                Ok(Raw {
                    code: r.get(1)?,
                    kind: r.get(2)?,
                    date: r.get(3)?,
                    wid: r.get(4)?,
                    wname: r.get(5)?,
                    to_wid: r.get(6)?,
                    to_wname: r.get(7)?,
                    qty: r.get(8)?,
                    price: r.get(9)?,
                    note: r.get(10)?,
                })
            })
            .map(|it| it.filter_map(|r| r.ok()).collect())
            .unwrap_or_default();

        let mut balance = 0.0;
        let mut out: Vec<Value> = Vec::new();
        let mut push = |code: &str,
                        kind: &str,
                        date: &str,
                        wname: &str,
                        delta: f64,
                        price: f64,
                        note: &str,
                        balance: &mut f64| {
            *balance += delta;
            out.push(json!({
                "code": code,
                "kind": kind,
                "date": date,
                "warehouse": wname,
                "in_qty": if delta > 0.0 { round3(delta) } else { 0.0 },
                "out_qty": if delta < 0.0 { round3(-delta) } else { 0.0 },
                "unit_price": price,
                "balance": round3(*balance),
                "note": note,
            }));
        };
        for r in raws {
            match r.kind.as_str() {
                "transfer" => {
                    let to_name = r.to_wname.clone().unwrap_or_default();
                    match warehouse_id {
                        Some(wid) => {
                            if r.wid == wid {
                                push(
                                    &r.code,
                                    "transfer",
                                    &r.date,
                                    &r.wname,
                                    -r.qty,
                                    r.price,
                                    &format!("chuyển đến {to_name}"),
                                    &mut balance,
                                );
                            } else if r.to_wid == Some(wid) {
                                push(
                                    &r.code,
                                    "transfer",
                                    &r.date,
                                    &to_name,
                                    r.qty,
                                    r.price,
                                    &format!("nhận từ {}", r.wname),
                                    &mut balance,
                                );
                            }
                        }
                        // Company-wide: transfer không đổi tổng tồn, hiện 1 dòng delta 0.
                        None => push(
                            &r.code,
                            "transfer",
                            &r.date,
                            &r.wname,
                            0.0,
                            r.price,
                            &format!("{} → {}", r.wname, to_name),
                            &mut balance,
                        ),
                    }
                }
                k => {
                    if let Some(wid) = warehouse_id {
                        if r.wid != wid {
                            continue;
                        }
                    }
                    let delta = match k {
                        "issue" => -r.qty,
                        _ => r.qty, // receipt cộng; adjust là delta có dấu sẵn
                    };
                    push(
                        &r.code,
                        k,
                        &r.date,
                        &r.wname,
                        delta,
                        r.price,
                        &r.note,
                        &mut balance,
                    );
                }
            }
        }
        let skip = out.len().saturating_sub(limit.clamp(1, 5000) as usize);
        out.into_iter().skip(skip).collect()
    }

    // ---- reports ----

    /// Monthly nhập-xuất report (last `months`, oldest → newest): receipt
    /// qty/value, issue qty/value, adjust net qty, net_qty.
    pub fn report_inout(&self, months: i64) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT substr(m.move_date,1,7) ym,
                        COALESCE(SUM(CASE WHEN m.kind='receipt' THEN l.qty END),0),
                        COALESCE(SUM(CASE WHEN m.kind='receipt' THEN l.qty*l.unit_price END),0),
                        COALESCE(SUM(CASE WHEN m.kind='issue'   THEN l.qty END),0),
                        COALESCE(SUM(CASE WHEN m.kind='issue'   THEN l.qty*l.unit_price END),0),
                        COALESCE(SUM(CASE WHEN m.kind='adjust'  THEN l.qty END),0)
                 FROM move_lines l JOIN moves m ON m.id=l.move_id
                 GROUP BY ym ORDER BY ym DESC LIMIT ?1",
            )
            .unwrap();
        let mut rows: Vec<Value> = stmt
            .query_map(params![months.clamp(1, 120)], |r| {
                let in_qty: f64 = r.get(1)?;
                let out_qty: f64 = r.get(3)?;
                let adj: f64 = r.get(5)?;
                Ok(json!({
                    "month": r.get::<_, String>(0)?,
                    "in_qty": round3(in_qty),
                    "in_value": round2(r.get::<_, f64>(2)?),
                    "out_qty": round3(out_qty),
                    "out_value": round2(r.get::<_, f64>(4)?),
                    "adjust_qty": round3(adj),
                    "net_qty": round3(in_qty - out_qty + adj),
                }))
            })
            .map(|it| it.filter_map(|r| r.ok()).collect())
            .unwrap_or_default();
        rows.reverse(); // oldest → newest for charting
        rows
    }

    /// The dashboard aggregate the UI, MCP and AI analysis all share.
    pub fn dashboard(&self, today: &str) -> Value {
        let products = self.list_products(None, None, Some("active"), false);
        let low: Vec<Value> = products
            .iter()
            .filter(|p| p["low_stock"] == true)
            .cloned()
            .collect();
        let out_of_stock = products
            .iter()
            .filter(|p| p["on_hand"].as_f64().unwrap_or(0.0) <= 0.0)
            .count();
        let stock_value: f64 = products
            .iter()
            .map(|p| p["stock_value"].as_f64().unwrap_or(0.0))
            .sum();

        let mut top = products.clone();
        top.sort_by(|a, b| {
            b["stock_value"]
                .as_f64()
                .unwrap_or(0.0)
                .partial_cmp(&a["stock_value"].as_f64().unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        top.truncate(5);

        // 30-day in/out flow.
        let from = {
            let d = chrono::NaiveDate::parse_from_str(today, "%Y-%m-%d")
                .unwrap_or_else(|_| chrono::Local::now().date_naive());
            (d - chrono::Duration::days(30))
                .format("%Y-%m-%d")
                .to_string()
        };
        let sum_moves = |kind: &str| -> (f64, f64) {
            let items = self.list_moves(Some(kind), None, None, Some(&from), Some(today), 1000);
            (
                round3(
                    items
                        .iter()
                        .map(|m| m["total_qty"].as_f64().unwrap_or(0.0))
                        .sum(),
                ),
                round2(
                    items
                        .iter()
                        .map(|m| m["total_value"].as_f64().unwrap_or(0.0))
                        .sum(),
                ),
            )
        };
        let (in_qty, in_value) = sum_moves("receipt");
        let (out_qty, out_value) = sum_moves("issue");

        json!({
            "today": today,
            "products_active": products.len(),
            "warehouses": self.list_warehouses(Some("active")),
            "stock_value": round2(stock_value),
            "low_stock": { "count": low.len(), "items": low },
            "out_of_stock_count": out_of_stock,
            "in_30d": { "qty": in_qty, "value": in_value },
            "out_30d": { "qty": out_qty, "value": out_value },
            "inout_12m": self.report_inout(12),
            "top_products": top,
            "recent_moves": self.list_moves(None, None, None, None, None, 10),
        })
    }

    // ---- product performance (phân tích sản phẩm) ----

    /// Per-product sales performance over the last `window_days`, with a
    /// deterministic classification the UI/MCP/AI all share:
    ///   * `potential` — đang bán và tồn chỉ đủ ≤ 45 ngày theo tốc độ bán
    ///     (sản phẩm tiềm năng, nên nhập thêm)
    ///   * `steady`    — đang bán, tồn đủ 45–180 ngày
    ///   * `slow`      — đang bán nhưng tồn đủ bán > 180 ngày (bán chậm)
    ///   * `dead`      — có tồn mà KHÔNG bán được đơn nào trong cửa sổ (tồn đọng)
    ///   * `idle`      — không tồn, không bán (chưa kinh doanh)
    /// Numbers only — the AI narrative on top is [`crate::llm::analyze_products`].
    pub fn product_performance(&self, today: &str, window_days: i64) -> Value {
        let days = window_days.clamp(7, 365);
        let d = chrono::NaiveDate::parse_from_str(today, "%Y-%m-%d")
            .unwrap_or_else(|_| chrono::Local::now().date_naive());
        let from = (d - chrono::Duration::days(days))
            .format("%Y-%m-%d")
            .to_string();

        let products = self.list_products(None, None, Some("active"), false);

        // In-window sold/received per product + all-time last sale date.
        let mut sold: BTreeMap<i64, (f64, f64)> = BTreeMap::new();
        let mut received: BTreeMap<i64, f64> = BTreeMap::new();
        let mut last_sale: BTreeMap<i64, String> = BTreeMap::new();
        {
            let conn = self.conn.lock().unwrap();
            let mut stmt = conn
                .prepare(
                    "SELECT l.product_id,
                            COALESCE(SUM(CASE WHEN m.kind='issue'   THEN l.qty END),0),
                            COALESCE(SUM(CASE WHEN m.kind='issue'   THEN l.qty*l.unit_price END),0),
                            COALESCE(SUM(CASE WHEN m.kind='receipt' THEN l.qty END),0)
                     FROM move_lines l JOIN moves m ON m.id=l.move_id
                     WHERE m.move_date > ?1 AND m.move_date <= ?2
                     GROUP BY l.product_id",
                )
                .unwrap();
            let rows = stmt
                .query_map(params![from, today], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, f64>(1)?,
                        r.get::<_, f64>(2)?,
                        r.get::<_, f64>(3)?,
                    ))
                })
                .unwrap();
            for (pid, sq, sv, rq) in rows.flatten() {
                sold.insert(pid, (sq, sv));
                received.insert(pid, rq);
            }
            let mut stmt = conn
                .prepare(
                    "SELECT l.product_id, MAX(m.move_date)
                     FROM move_lines l JOIN moves m ON m.id=l.move_id
                     WHERE m.kind='issue' GROUP BY l.product_id",
                )
                .unwrap();
            let rows = stmt
                .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
                .unwrap();
            for (pid, date) in rows.flatten() {
                last_sale.insert(pid, date);
            }
        }

        let mut items: Vec<Value> = Vec::new();
        let mut counts: BTreeMap<&str, i64> = BTreeMap::new();
        let mut dead_value = 0.0;
        for p in &products {
            let pid = p["id"].as_i64().unwrap_or(0);
            let on_hand = p["on_hand"].as_f64().unwrap_or(0.0);
            let avg_cost = p["avg_cost"].as_f64().unwrap_or(0.0);
            let sell_price = p["sell_price"].as_f64().unwrap_or(0.0);
            let (sold_qty, sold_value) = *sold.get(&pid).unwrap_or(&(0.0, 0.0));
            let received_qty = *received.get(&pid).unwrap_or(&0.0);

            let velocity_30d = round3(sold_qty / days as f64 * 30.0);
            // Ngày còn bán được với tồn hiện tại theo tốc độ bán trong cửa sổ.
            let days_of_stock = if sold_qty > 0.0 {
                Some((on_hand / (sold_qty / days as f64)).round() as i64)
            } else {
                None
            };
            let margin_pct = if avg_cost > 0.0 {
                Some(round2((sell_price - avg_cost) / avg_cost * 100.0))
            } else {
                None
            };
            let sell_through = if sold_qty + on_hand.max(0.0) > 0.0 {
                round2(sold_qty / (sold_qty + on_hand.max(0.0)) * 100.0)
            } else {
                0.0
            };

            let class = if sold_qty <= 0.0 && on_hand <= 0.0 {
                "idle"
            } else if sold_qty <= 0.0 {
                "dead"
            } else {
                match days_of_stock {
                    Some(ds) if ds <= 45 => "potential",
                    Some(ds) if ds > 180 => "slow",
                    _ => "steady",
                }
            };
            *counts.entry(class).or_default() += 1;
            if class == "dead" {
                dead_value += p["stock_value"].as_f64().unwrap_or(0.0);
            }

            items.push(json!({
                "id": pid,
                "sku": p["sku"],
                "name": p["name"],
                "unit": p["unit"],
                "category": p["category"],
                "on_hand": on_hand,
                "stock_value": p["stock_value"],
                "avg_cost": avg_cost,
                "sell_price": sell_price,
                "sold_qty": round3(sold_qty),
                "sold_value": round2(sold_value),
                "received_qty": round3(received_qty),
                "velocity_30d": velocity_30d,
                "days_of_stock": days_of_stock,
                "margin_pct": margin_pct,
                "sell_through_pct": sell_through,
                "last_sale_date": last_sale.get(&pid),
                "class": class,
            }));
        }
        // Bán chạy nhất lên đầu; trong cùng doanh số thì tồn đọng giá trị lớn trước.
        items.sort_by(|a, b| {
            b["sold_value"]
                .as_f64()
                .partial_cmp(&a["sold_value"].as_f64())
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(
                    b["stock_value"]
                        .as_f64()
                        .partial_cmp(&a["stock_value"].as_f64())
                        .unwrap_or(std::cmp::Ordering::Equal),
                )
        });
        let top_sellers: Vec<Value> = items
            .iter()
            .filter(|i| i["sold_qty"].as_f64().unwrap_or(0.0) > 0.0)
            .take(5)
            .cloned()
            .collect();

        json!({
            "today": today,
            "window_days": days,
            "items": items,
            "summary": {
                "potential_count": counts.get("potential").copied().unwrap_or(0),
                "steady_count": counts.get("steady").copied().unwrap_or(0),
                "slow_count": counts.get("slow").copied().unwrap_or(0),
                "dead_count": counts.get("dead").copied().unwrap_or(0),
                "idle_count": counts.get("idle").copied().unwrap_or(0),
                "dead_stock_value": round2(dead_value),
                "top_sellers": top_sellers,
            },
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Seed: 1 kho, 1 product. Returns (wid, pid).
    fn seed(db: &Db) -> (i64, i64) {
        let wid = db.add_warehouse("Kho A", "HN", "").unwrap();
        let pid = db
            .add_product(
                "SP01", "Áo thun", "cái", "Áo", "", 50_000.0, 90_000.0, 10.0, "",
            )
            .unwrap();
        (wid, pid)
    }

    fn line(pid: i64, qty: f64, price: f64) -> LineIn {
        LineIn {
            product_id: pid,
            qty,
            unit_price: price,
        }
    }

    #[test]
    fn product_crud_and_sku_unique() {
        let db = Db::open_memory().unwrap();
        let (_, pid) = seed(&db);
        assert!(db
            .add_product("SP01", "Trùng SKU", "", "", "", 0.0, 0.0, 0.0, "")
            .is_err());
        assert!(db
            .add_product("", "", "", "", "", 0.0, 0.0, 0.0, "")
            .is_err());
        assert!(db
            .add_product("X", "Giá âm", "", "", "", -1.0, 0.0, 0.0, "")
            .is_err());
        db.update_product(
            pid,
            &json!({ "sell_price": 95_000.0, "category": "Thời trang" }),
        )
        .unwrap();
        let p = db.get_product(pid).unwrap();
        assert_eq!(p["sell_price"], 95_000.0);
        assert_eq!(p["category"], "Thời trang");
        assert!(db
            .update_product(pid, &json!({ "status": "vanished" }))
            .is_err());
        assert!(db.update_product(999, &json!({ "name": "x" })).is_err());
        // Second product may not steal the SKU.
        let p2 = db
            .add_product("SP02", "Quần jean", "cái", "", "", 0.0, 0.0, 0.0, "")
            .unwrap();
        assert!(db.update_product(p2, &json!({ "sku": "SP01" })).is_err());
    }

    #[test]
    fn receipt_issue_flow_and_onhand() {
        let db = Db::open_memory().unwrap();
        let (wid, pid) = seed(&db);
        let m = db
            .create_move(
                "receipt",
                wid,
                None,
                None,
                "2026-07-01",
                "nhập lô 1",
                &[line(pid, 100.0, 50_000.0)],
            )
            .unwrap();
        assert_eq!(m["code"], "NK-0001");
        assert_eq!(m["lines"][0]["amount"], 5_000_000.0);
        db.create_move(
            "issue",
            wid,
            None,
            None,
            "2026-07-02",
            "bán lẻ",
            &[line(pid, 30.0, 90_000.0)],
        )
        .unwrap();
        let p = db.get_product(pid).unwrap();
        assert_eq!(p["on_hand"], 70.0);
        assert_eq!(p["avg_cost"], 50_000.0);
        assert_eq!(p["stock_value"], 3_500_000.0);
        // Not enough stock → rejected, stock unchanged.
        let err = db.create_move("issue", wid, None, None, "", "", &[line(pid, 71.0, 0.0)]);
        assert!(err.is_err());
        assert_eq!(db.get_product(pid).unwrap()["on_hand"], 70.0);
    }

    #[test]
    fn avg_cost_weights_receipts() {
        let db = Db::open_memory().unwrap();
        let (wid, pid) = seed(&db);
        db.create_move(
            "receipt",
            wid,
            None,
            None,
            "2026-07-01",
            "",
            &[line(pid, 10.0, 40_000.0)],
        )
        .unwrap();
        db.create_move(
            "receipt",
            wid,
            None,
            None,
            "2026-07-02",
            "",
            &[line(pid, 30.0, 60_000.0)],
        )
        .unwrap();
        let p = db.get_product(pid).unwrap();
        // (10*40k + 30*60k) / 40 = 55k
        assert_eq!(p["avg_cost"], 55_000.0);
        // Without any receipt the declared cost_price is the fallback.
        let p2 = db
            .add_product("SP02", "Mũ", "cái", "", "", 20_000.0, 0.0, 0.0, "")
            .unwrap();
        assert_eq!(db.get_product(p2).unwrap()["avg_cost"], 20_000.0);
    }

    #[test]
    fn transfer_between_warehouses() {
        let db = Db::open_memory().unwrap();
        let (wa, pid) = seed(&db);
        let wb = db.add_warehouse("Kho B", "HCM", "").unwrap();
        db.create_move(
            "receipt",
            wa,
            None,
            None,
            "2026-07-01",
            "",
            &[line(pid, 50.0, 10_000.0)],
        )
        .unwrap();
        let m = db
            .create_move(
                "transfer",
                wa,
                Some(wb),
                None,
                "2026-07-03",
                "",
                &[line(pid, 20.0, 0.0)],
            )
            .unwrap();
        assert_eq!(m["code"], "CK-0002");
        let on = db.stock_onhand(Some(pid), None);
        assert_eq!(on.len(), 2);
        let qty_at = |wid: i64| {
            on.iter()
                .find(|o| o["warehouse_id"] == wid)
                .map(|o| o["qty"].as_f64().unwrap())
                .unwrap_or(0.0)
        };
        assert_eq!(qty_at(wa), 30.0);
        assert_eq!(qty_at(wb), 20.0);
        // Total on-hand unchanged by transfer.
        assert_eq!(db.get_product(pid).unwrap()["on_hand"], 50.0);
        // transfer validations
        assert!(db
            .create_move(
                "transfer",
                wa,
                Some(wa),
                None,
                "",
                "",
                &[line(pid, 1.0, 0.0)]
            )
            .is_err());
        assert!(db
            .create_move("transfer", wa, None, None, "", "", &[line(pid, 1.0, 0.0)])
            .is_err());
        assert!(db
            .create_move(
                "transfer",
                wa,
                Some(999),
                None,
                "",
                "",
                &[line(pid, 1.0, 0.0)]
            )
            .is_err());
        assert!(db
            .create_move(
                "transfer",
                wa,
                Some(wb),
                None,
                "",
                "",
                &[line(pid, 31.0, 0.0)]
            )
            .is_err());
        // Issue from kho B only has 20.
        assert!(db
            .create_move("issue", wb, None, None, "", "", &[line(pid, 21.0, 0.0)])
            .is_err());
        db.create_move(
            "issue",
            wb,
            None,
            None,
            "",
            "",
            &[line(pid, 20.0, 15_000.0)],
        )
        .unwrap();
        assert_eq!(qty_at(wb), 20.0); // snapshot from before the issue
        assert_eq!(db.get_product(pid).unwrap()["on_hand"], 30.0);
    }

    #[test]
    fn adjust_is_signed_delta_and_guards_negative() {
        let db = Db::open_memory().unwrap();
        let (wid, pid) = seed(&db);
        db.create_move(
            "receipt",
            wid,
            None,
            None,
            "2026-07-01",
            "",
            &[line(pid, 10.0, 1_000.0)],
        )
        .unwrap();
        // Kiểm kê thấy thiếu 3.
        let m = db
            .create_move(
                "adjust",
                wid,
                None,
                None,
                "2026-07-05",
                "kiểm kê Q3",
                &[line(pid, -3.0, 0.0)],
            )
            .unwrap();
        assert_eq!(m["code"], "DC-0002");
        assert_eq!(db.get_product(pid).unwrap()["on_hand"], 7.0);
        // Thừa 1.
        db.create_move(
            "adjust",
            wid,
            None,
            None,
            "2026-07-06",
            "",
            &[line(pid, 1.0, 0.0)],
        )
        .unwrap();
        assert_eq!(db.get_product(pid).unwrap()["on_hand"], 8.0);
        // Delta 0 and over-negative rejected.
        assert!(db
            .create_move("adjust", wid, None, None, "", "", &[line(pid, 0.0, 0.0)])
            .is_err());
        assert!(db
            .create_move("adjust", wid, None, None, "", "", &[line(pid, -9.0, 0.0)])
            .is_err());
    }

    #[test]
    fn move_validations() {
        let db = Db::open_memory().unwrap();
        let (wid, pid) = seed(&db);
        assert!(db
            .create_move("steal", wid, None, None, "", "", &[line(pid, 1.0, 0.0)])
            .is_err());
        assert!(db
            .create_move("receipt", 999, None, None, "", "", &[line(pid, 1.0, 0.0)])
            .is_err());
        assert!(db
            .create_move("receipt", wid, None, None, "", "", &[])
            .is_err());
        assert!(db
            .create_move("receipt", wid, None, None, "", "", &[line(999, 1.0, 0.0)])
            .is_err());
        assert!(db
            .create_move("receipt", wid, None, None, "", "", &[line(pid, 0.0, 0.0)])
            .is_err());
        assert!(db
            .create_move("receipt", wid, None, None, "", "", &[line(pid, 1.0, -5.0)])
            .is_err());
        assert!(db
            .create_move(
                "receipt",
                wid,
                Some(wid),
                None,
                "",
                "",
                &[line(pid, 1.0, 0.0)]
            )
            .is_err());
        assert!(db
            .create_move(
                "receipt",
                wid,
                None,
                Some(999),
                "",
                "",
                &[line(pid, 1.0, 0.0)]
            )
            .is_err());
        // Empty date defaults to today.
        let m = db
            .create_move("receipt", wid, None, None, "  ", "", &[line(pid, 1.0, 0.0)])
            .unwrap();
        assert_eq!(m["move_date"], stock::today());
    }

    #[test]
    fn delete_move_guards_ledger_consistency() {
        let db = Db::open_memory().unwrap();
        let (wid, pid) = seed(&db);
        let rec = db
            .create_move(
                "receipt",
                wid,
                None,
                None,
                "2026-07-01",
                "",
                &[line(pid, 10.0, 1_000.0)],
            )
            .unwrap();
        db.create_move(
            "issue",
            wid,
            None,
            None,
            "2026-07-02",
            "",
            &[line(pid, 8.0, 2_000.0)],
        )
        .unwrap();
        // Deleting the receipt would leave -8 → refused, nothing changes.
        let rec_id = rec["id"].as_i64().unwrap();
        assert!(db.delete_move(rec_id).is_err());
        assert_eq!(db.get_product(pid).unwrap()["on_hand"], 2.0);
        // Deleting the issue is fine, then the receipt too.
        let issues = db.list_moves(Some("issue"), None, None, None, None, 10);
        let del = db.delete_move(issues[0]["id"].as_i64().unwrap()).unwrap();
        assert_eq!(del["deleted"], "XK-0002");
        assert_eq!(db.get_product(pid).unwrap()["on_hand"], 10.0);
        db.delete_move(rec_id).unwrap();
        assert_eq!(db.get_product(pid).unwrap()["on_hand"], 0.0);
        assert!(db.delete_move(999).is_err());
    }

    #[test]
    fn list_moves_filters() {
        let db = Db::open_memory().unwrap();
        let (wa, pid) = seed(&db);
        let wb = db.add_warehouse("Kho B", "", "").unwrap();
        let p2 = db
            .add_product("SP02", "Quần", "cái", "", "", 0.0, 0.0, 0.0, "")
            .unwrap();
        db.create_move(
            "receipt",
            wa,
            None,
            None,
            "2026-07-01",
            "",
            &[line(pid, 10.0, 1_000.0)],
        )
        .unwrap();
        db.create_move(
            "receipt",
            wb,
            None,
            None,
            "2026-07-02",
            "",
            &[line(p2, 5.0, 2_000.0)],
        )
        .unwrap();
        db.create_move(
            "transfer",
            wa,
            Some(wb),
            None,
            "2026-07-03",
            "",
            &[line(pid, 4.0, 0.0)],
        )
        .unwrap();
        assert_eq!(
            db.list_moves(Some("receipt"), None, None, None, None, 100)
                .len(),
            2
        );
        // Kho B matches both its own receipt and the incoming transfer.
        assert_eq!(
            db.list_moves(None, Some(wb), None, None, None, 100).len(),
            2
        );
        assert_eq!(
            db.list_moves(None, None, Some(pid), None, None, 100).len(),
            2
        );
        assert_eq!(
            db.list_moves(
                None,
                None,
                None,
                Some("2026-07-02"),
                Some("2026-07-02"),
                100
            )
            .len(),
            1
        );
        let header = &db.list_moves(Some("receipt"), Some(wa), None, None, None, 100)[0];
        assert_eq!(header["total_qty"], 10.0);
        assert_eq!(header["total_value"], 10_000.0);
        assert_eq!(header["line_count"], 1);
    }

    #[test]
    fn stock_card_running_balance() {
        let db = Db::open_memory().unwrap();
        let (wa, pid) = seed(&db);
        let wb = db.add_warehouse("Kho B", "", "").unwrap();
        db.create_move(
            "receipt",
            wa,
            None,
            None,
            "2026-07-01",
            "",
            &[line(pid, 10.0, 1_000.0)],
        )
        .unwrap();
        db.create_move(
            "transfer",
            wa,
            Some(wb),
            None,
            "2026-07-02",
            "",
            &[line(pid, 4.0, 0.0)],
        )
        .unwrap();
        db.create_move(
            "issue",
            wb,
            None,
            None,
            "2026-07-03",
            "",
            &[line(pid, 1.0, 2_000.0)],
        )
        .unwrap();
        db.create_move(
            "adjust",
            wa,
            None,
            None,
            "2026-07-04",
            "",
            &[line(pid, -2.0, 0.0)],
        )
        .unwrap();

        // Company-wide: 10 → 10 (transfer nets 0) → 9 → 7.
        let card = db.stock_card(pid, None, 100);
        assert_eq!(card.len(), 4);
        assert_eq!(card[0]["balance"], 10.0);
        assert_eq!(card[1]["balance"], 10.0);
        assert_eq!(card[2]["balance"], 9.0);
        assert_eq!(card[3]["balance"], 7.0);

        // Kho A view: +10, −4 (chuyển đi), −2 = 4.
        let card_a = db.stock_card(pid, Some(wa), 100);
        assert_eq!(card_a.len(), 3);
        assert_eq!(card_a[1]["out_qty"], 4.0);
        assert_eq!(card_a[2]["balance"], 4.0);

        // Kho B view: +4 (nhận), −1 = 3.
        let card_b = db.stock_card(pid, Some(wb), 100);
        assert_eq!(card_b.len(), 2);
        assert_eq!(card_b[0]["in_qty"], 4.0);
        assert_eq!(card_b[1]["balance"], 3.0);

        // Limit keeps the LAST rows (most recent history).
        let tail = db.stock_card(pid, None, 2);
        assert_eq!(tail.len(), 2);
        assert_eq!(tail[1]["balance"], 7.0);
    }

    #[test]
    fn low_stock_and_dashboard() {
        let db = Db::open_memory().unwrap();
        let (wid, pid) = seed(&db); // min_stock 10
        let p2 = db
            .add_product("SP02", "Quần", "cái", "", "", 30_000.0, 60_000.0, 0.0, "")
            .unwrap();
        db.create_move(
            "receipt",
            wid,
            None,
            None,
            "2026-07-01",
            "",
            &[line(pid, 5.0, 50_000.0), line(p2, 20.0, 30_000.0)],
        )
        .unwrap();
        // pid: 5 < 10 → low. p2: min_stock 0 → never low.
        let low = db.list_products(None, None, None, true);
        assert_eq!(low.len(), 1);
        assert_eq!(low[0]["id"], pid);

        let d = db.dashboard("2026-07-10");
        assert_eq!(d["products_active"], 2);
        assert_eq!(d["low_stock"]["count"], 1);
        assert_eq!(d["stock_value"], 5.0 * 50_000.0 + 20.0 * 30_000.0);
        assert_eq!(d["in_30d"]["qty"], 25.0);
        assert_eq!(d["in_30d"]["value"], 850_000.0);
        assert_eq!(d["out_30d"]["qty"], 0.0);
        assert_eq!(d["warehouses"][0]["sku_count"], 2);
        assert_eq!(d["recent_moves"].as_array().unwrap().len(), 1);
        // Inactive products drop out.
        db.update_product(pid, &json!({ "status": "inactive" }))
            .unwrap();
        let d2 = db.dashboard("2026-07-10");
        assert_eq!(d2["products_active"], 1);
        assert_eq!(d2["low_stock"]["count"], 0);
    }

    #[test]
    fn report_inout_monthly() {
        let db = Db::open_memory().unwrap();
        let (wid, pid) = seed(&db);
        db.create_move(
            "receipt",
            wid,
            None,
            None,
            "2026-06-15",
            "",
            &[line(pid, 10.0, 1_000.0)],
        )
        .unwrap();
        db.create_move(
            "receipt",
            wid,
            None,
            None,
            "2026-06-20",
            "",
            &[line(pid, 5.0, 1_000.0)],
        )
        .unwrap();
        db.create_move(
            "issue",
            wid,
            None,
            None,
            "2026-07-01",
            "",
            &[line(pid, 6.0, 3_000.0)],
        )
        .unwrap();
        db.create_move(
            "adjust",
            wid,
            None,
            None,
            "2026-07-02",
            "",
            &[line(pid, -1.0, 0.0)],
        )
        .unwrap();
        let rep = db.report_inout(12);
        assert_eq!(rep.len(), 2);
        assert_eq!(rep[0]["month"], "2026-06");
        assert_eq!(rep[0]["in_qty"], 15.0);
        assert_eq!(rep[0]["in_value"], 15_000.0);
        assert_eq!(rep[1]["out_qty"], 6.0);
        assert_eq!(rep[1]["out_value"], 18_000.0);
        assert_eq!(rep[1]["adjust_qty"], -1.0);
        assert_eq!(rep[1]["net_qty"], -7.0);
    }

    #[test]
    fn partners_and_move_partner_join() {
        let db = Db::open_memory().unwrap();
        let (wid, pid) = seed(&db);
        assert!(db.add_partner("X", "alien", "", "", "").is_err());
        assert!(db.add_partner("", "supplier", "", "", "").is_err());
        let sup = db
            .add_partner("Cty Dệt May", "supplier", "0901", "HN", "")
            .unwrap();
        db.add_partner("Shop Trẻ Em", "customer", "", "", "")
            .unwrap();
        assert_eq!(db.list_partners(None).len(), 2);
        assert_eq!(db.list_partners(Some("supplier")).len(), 1);
        let m = db
            .create_move(
                "receipt",
                wid,
                None,
                Some(sup),
                "2026-07-01",
                "",
                &[line(pid, 1.0, 500.0)],
            )
            .unwrap();
        assert_eq!(m["partner_name"], "Cty Dệt May");
    }

    #[test]
    fn warehouse_crud() {
        let db = Db::open_memory().unwrap();
        let (wid, _) = seed(&db);
        assert!(db.add_warehouse("", "", "").is_err());
        db.update_warehouse(wid, &json!({ "name": "Kho Hà Nội", "status": "inactive" }))
            .unwrap();
        let list = db.list_warehouses(None);
        assert_eq!(list[0]["name"], "Kho Hà Nội");
        assert_eq!(db.list_warehouses(Some("active")).len(), 0);
        assert!(db
            .update_warehouse(wid, &json!({ "status": "gone" }))
            .is_err());
        assert!(db.update_warehouse(999, &json!({ "name": "x" })).is_err());
    }

    #[test]
    fn multi_line_move_and_get() {
        let db = Db::open_memory().unwrap();
        let (wid, pid) = seed(&db);
        let p2 = db
            .add_product("SP02", "Quần", "cái", "", "", 0.0, 0.0, 0.0, "")
            .unwrap();
        let m = db
            .create_move(
                "receipt",
                wid,
                None,
                None,
                "2026-07-01",
                "lô hỗn hợp",
                &[line(pid, 10.0, 1_000.0), line(p2, 5.0, 2_000.0)],
            )
            .unwrap();
        assert_eq!(m["line_count"], 2);
        assert_eq!(m["total_qty"], 15.0);
        assert_eq!(m["total_value"], 20_000.0);
        let got = db.get_move(m["id"].as_i64().unwrap()).unwrap();
        assert_eq!(got["lines"].as_array().unwrap().len(), 2);
        assert_eq!(got["lines"][1]["product_name"], "Quần");
        assert!(db.get_move(999).is_none());
    }

    #[test]
    fn product_performance_classification() {
        let db = Db::open_memory().unwrap();
        let wid = db.add_warehouse("Kho A", "", "").unwrap();
        // hot: bán nhanh, tồn mỏng → potential
        let hot = db
            .add_product(
                "HOT",
                "Áo hot trend",
                "cái",
                "",
                "",
                40_000.0,
                80_000.0,
                0.0,
                "",
            )
            .unwrap();
        // slow: tồn dày, bán nhỏ giọt → slow
        let slow = db
            .add_product(
                "SLOW",
                "Quần bán chậm",
                "cái",
                "",
                "",
                100_000.0,
                120_000.0,
                0.0,
                "",
            )
            .unwrap();
        // dead: có tồn, không bán được đơn nào → dead
        let dead = db
            .add_product(
                "DEAD",
                "Mũ lỗi mốt",
                "cái",
                "",
                "",
                30_000.0,
                50_000.0,
                0.0,
                "",
            )
            .unwrap();
        // idle: chưa nhập chưa bán
        let idle = db
            .add_product("IDLE", "Hàng chưa về", "cái", "", "", 0.0, 0.0, 0.0, "")
            .unwrap();

        db.create_move(
            "receipt",
            wid,
            None,
            None,
            "2026-05-01",
            "",
            &[
                line(hot, 100.0, 40_000.0),
                line(slow, 200.0, 100_000.0),
                line(dead, 50.0, 30_000.0),
            ],
        )
        .unwrap();
        // 90 ngày: hot bán 90 (còn 10 → ~10 ngày tồn), slow bán 6 (còn 194 → ~2910 ngày)
        db.create_move(
            "issue",
            wid,
            None,
            None,
            "2026-06-15",
            "",
            &[line(hot, 60.0, 80_000.0), line(slow, 4.0, 120_000.0)],
        )
        .unwrap();
        db.create_move(
            "issue",
            wid,
            None,
            None,
            "2026-07-10",
            "",
            &[line(hot, 30.0, 80_000.0), line(slow, 2.0, 120_000.0)],
        )
        .unwrap();

        let perf = db.product_performance("2026-07-27", 90);
        let items = perf["items"].as_array().unwrap();
        let cls = |pid: i64| {
            items.iter().find(|i| i["id"] == pid).unwrap()["class"]
                .as_str()
                .unwrap()
                .to_string()
        };
        assert_eq!(cls(hot), "potential");
        assert_eq!(cls(slow), "slow");
        assert_eq!(cls(dead), "dead");
        assert_eq!(cls(idle), "idle");

        let hot_item = items.iter().find(|i| i["id"] == hot).unwrap();
        assert_eq!(hot_item["sold_qty"], 90.0);
        assert_eq!(hot_item["sold_value"], 90.0 * 80_000.0);
        assert_eq!(hot_item["velocity_30d"], 30.0);
        assert_eq!(hot_item["days_of_stock"], 10);
        assert_eq!(hot_item["margin_pct"], 100.0);
        assert_eq!(hot_item["sell_through_pct"], 90.0);
        assert_eq!(hot_item["last_sale_date"], "2026-07-10");
        // Bán chạy nhất đứng đầu danh sách.
        assert_eq!(items[0]["id"], hot);

        let s = &perf["summary"];
        assert_eq!(s["potential_count"], 1);
        assert_eq!(s["slow_count"], 1);
        assert_eq!(s["dead_count"], 1);
        assert_eq!(s["idle_count"], 1);
        assert_eq!(s["dead_stock_value"], 50.0 * 30_000.0);
        assert_eq!(s["top_sellers"].as_array().unwrap().len(), 2);

        // Ngoài cửa sổ 30 ngày gần nhất: hot chỉ còn phiếu 2026-07-10 (30 cái).
        let perf30 = db.product_performance("2026-07-27", 30);
        let h30 = perf30["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["id"] == hot)
            .unwrap()
            .clone();
        assert_eq!(h30["sold_qty"], 30.0);
    }

    #[test]
    fn activity_log() {
        let db = Db::open_memory().unwrap();
        db.log("product", "thêm sản phẩm", "1");
        db.log("move", "nhập kho", "2");
        let acts = db.recent_activity(10);
        assert_eq!(acts.len(), 2);
        assert_eq!(acts[0]["kind"], "move"); // newest first
    }
}
