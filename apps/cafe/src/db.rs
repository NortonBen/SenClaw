//! SQLite cho app Quán Cafe. Nguyên tắc:
//! - Tồn kho KHÔNG lưu cột riêng — luôn suy ra từ `stock_moves` (SUM qty) để
//!   không bao giờ lệch giữa sổ và thẻ kho.
//! - Giá vốn nguyên liệu là bình quân gia quyền (BQGQ) cập nhật tại mỗi phiếu
//!   nhập; đơn bán chốt (snapshot) giá vốn tại thời điểm bán vào `sale_lines`
//!   và `stock_moves` — báo cáo lãi cũ không đổi khi giá nhập sau này đổi.
//! - Mọi hàm đọc trả `serde_json::Value` cho REST/MCP dùng chung.

use crate::calc::{
    self, date_add, doc_code, fold_vi, forecast_series, qty_display, round2, round3, unit_factor,
    BASE_UNITS,
};
use anyhow::{anyhow, Result};
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Db {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS ingredients (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  name        TEXT NOT NULL,
  name_folded TEXT NOT NULL DEFAULT '',
  unit        TEXT NOT NULL DEFAULT 'g',
  min_stock   REAL NOT NULL DEFAULT 0,
  avg_cost    REAL NOT NULL DEFAULT 0,
  note        TEXT NOT NULL DEFAULT '',
  status      TEXT NOT NULL DEFAULT 'active',
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS stock_moves (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  ingredient_id INTEGER NOT NULL,
  kind          TEXT NOT NULL,
  qty           REAL NOT NULL,
  unit_cost     REAL NOT NULL DEFAULT 0,
  ref_kind      TEXT NOT NULL DEFAULT '',
  ref_id        INTEGER,
  note          TEXT NOT NULL DEFAULT '',
  move_date     TEXT NOT NULL,
  created_at    INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_sm_ing  ON stock_moves(ingredient_id);
CREATE INDEX IF NOT EXISTS idx_sm_date ON stock_moves(move_date);
CREATE INDEX IF NOT EXISTS idx_sm_kind ON stock_moves(kind);
CREATE TABLE IF NOT EXISTS purchases (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  code          TEXT NOT NULL DEFAULT '',
  supplier      TEXT NOT NULL DEFAULT '',
  purchase_date TEXT NOT NULL,
  note          TEXT NOT NULL DEFAULT '',
  total         REAL NOT NULL DEFAULT 0,
  created_at    INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_p_date ON purchases(purchase_date);
CREATE TABLE IF NOT EXISTS purchase_lines (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  purchase_id   INTEGER NOT NULL,
  ingredient_id INTEGER NOT NULL,
  qty           REAL NOT NULL,
  qty_input     REAL NOT NULL,
  unit_input    TEXT NOT NULL,
  unit_price    REAL NOT NULL,
  amount        REAL NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_pl_p ON purchase_lines(purchase_id);
CREATE INDEX IF NOT EXISTS idx_pl_i ON purchase_lines(ingredient_id);
CREATE TABLE IF NOT EXISTS menu_items (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  name         TEXT NOT NULL,
  name_folded  TEXT NOT NULL DEFAULT '',
  category     TEXT NOT NULL DEFAULT '',
  price        REAL NOT NULL DEFAULT 0,
  instructions TEXT NOT NULL DEFAULT '',
  status       TEXT NOT NULL DEFAULT 'active',
  created_at   INTEGER NOT NULL,
  updated_at   INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS recipe_lines (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  menu_id       INTEGER NOT NULL,
  ingredient_id INTEGER NOT NULL,
  qty           REAL NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_rl_m ON recipe_lines(menu_id);
CREATE TABLE IF NOT EXISTS sales (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  code       TEXT NOT NULL DEFAULT '',
  sale_date  TEXT NOT NULL,
  note       TEXT NOT NULL DEFAULT '',
  total      REAL NOT NULL DEFAULT 0,
  cogs       REAL NOT NULL DEFAULT 0,
  status     TEXT NOT NULL DEFAULT 'done',
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_s_date ON sales(sale_date);
CREATE TABLE IF NOT EXISTS sale_lines (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  sale_id    INTEGER NOT NULL,
  menu_id    INTEGER NOT NULL,
  menu_name  TEXT NOT NULL DEFAULT '',
  qty        REAL NOT NULL,
  unit_price REAL NOT NULL,
  amount     REAL NOT NULL,
  cogs       REAL NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_sl_s ON sale_lines(sale_id);
CREATE INDEX IF NOT EXISTS idx_sl_m ON sale_lines(menu_id);
"#;

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Dòng phiếu nhập như API/MCP gửi lên: qty theo `unit` người dùng khai
/// (g|kg|ml|l|lít|cái), unit_price là giá cho MỘT `unit` đó.
#[derive(Debug, Clone)]
pub struct PurchaseLineIn {
    pub ingredient_id: i64,
    pub qty: f64,
    pub unit: String,
    pub unit_price: f64,
}

/// Dòng đơn bán: bỏ `unit_price` = lấy giá thực đơn hiện tại.
#[derive(Debug, Clone)]
pub struct SaleLineIn {
    pub menu_id: i64,
    pub qty: f64,
    pub unit_price: Option<f64>,
}

/// Một dòng công thức: qty theo ĐƠN VỊ GỐC của nguyên liệu.
#[derive(Debug, Clone)]
pub struct RecipeItemIn {
    pub ingredient_id: i64,
    pub qty: f64,
}

impl Db {
    pub fn open_default() -> Result<Self> {
        let dir = std::env::var("SENCLAW_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                PathBuf::from(home).join(".senclaw").join("apps").join("cafe")
            });
        std::fs::create_dir_all(&dir).ok();
        Self::open(dir.join("cafe.db"))
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

    // ---------------------------------------------------------------- helpers

    fn stock_of(conn: &Connection, ingredient_id: i64) -> f64 {
        conn.query_row(
            "SELECT COALESCE(SUM(qty),0) FROM stock_moves WHERE ingredient_id=?1",
            params![ingredient_id],
            |r| r.get::<_, f64>(0),
        )
        .unwrap_or(0.0)
    }

    fn stocks_map(conn: &Connection) -> BTreeMap<i64, f64> {
        let mut out = BTreeMap::new();
        let Ok(mut stmt) =
            conn.prepare("SELECT ingredient_id, SUM(qty) FROM stock_moves GROUP BY ingredient_id")
        else {
            return out;
        };
        if let Ok(rows) = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?))) {
            for (id, q) in rows.flatten() {
                out.insert(id, q);
            }
        }
        out
    }

    /// Tiêu hao thực tế trung bình/ngày trong 14 ngày gần nhất (từ move bán).
    fn usage14_map(conn: &Connection, today: &str) -> BTreeMap<i64, f64> {
        let from = date_add(today, -13);
        let mut out = BTreeMap::new();
        let Ok(mut stmt) = conn.prepare(
            "SELECT ingredient_id, SUM(-qty) FROM stock_moves
             WHERE kind='sale' AND move_date>=?1 AND move_date<=?2
             GROUP BY ingredient_id",
        ) else {
            return out;
        };
        if let Ok(rows) = stmt.query_map(params![from, today], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?))
        }) {
            for (id, q) in rows.flatten() {
                out.insert(id, q / 14.0);
            }
        }
        out
    }

    fn ingredient_head(conn: &Connection, id: i64) -> Option<(String, String, f64)> {
        conn.query_row(
            "SELECT name, unit, avg_cost FROM ingredients WHERE id=?1",
            params![id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, f64>(2)?,
                ))
            },
        )
        .optional()
        .ok()
        .flatten()
    }

    /// Giá vốn hiện tại của từng món = Σ định lượng × giá vốn BQGQ nguyên liệu.
    fn menu_cost_map(conn: &Connection) -> BTreeMap<i64, f64> {
        let mut out = BTreeMap::new();
        let Ok(mut stmt) = conn.prepare(
            "SELECT r.menu_id, SUM(r.qty * i.avg_cost)
             FROM recipe_lines r JOIN ingredients i ON i.id=r.ingredient_id
             GROUP BY r.menu_id",
        ) else {
            return out;
        };
        if let Ok(rows) = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?))) {
            for (id, c) in rows.flatten() {
                out.insert(id, c);
            }
        }
        out
    }

    fn name_taken(conn: &Connection, table: &str, folded: &str, except_id: Option<i64>) -> bool {
        let sql = format!(
            "SELECT id FROM {table} WHERE name_folded=?1 AND id != ?2 LIMIT 1"
        );
        conn.query_row(&sql, params![folded, except_id.unwrap_or(-1)], |r| {
            r.get::<_, i64>(0)
        })
        .optional()
        .unwrap_or(None)
        .is_some()
    }

    // ------------------------------------------------------------ ingredients

    pub fn add_ingredient(&self, name: &str, unit: &str, min_stock: f64, note: &str) -> Result<i64> {
        let name = name.trim();
        if name.is_empty() {
            return Err(anyhow!("thiếu tên nguyên liệu"));
        }
        let unit = unit.trim();
        if !BASE_UNITS.contains(&unit) {
            return Err(anyhow!(
                "đơn vị gốc phải là một trong: {} (nhập kg/lít sẽ tự quy đổi ở phiếu nhập)",
                BASE_UNITS.join(", ")
            ));
        }
        if min_stock < 0.0 {
            return Err(anyhow!("min_stock phải ≥ 0"));
        }
        let folded = fold_vi(name);
        let conn = self.conn.lock().unwrap();
        if Self::name_taken(&conn, "ingredients", &folded, None) {
            return Err(anyhow!("nguyên liệu \"{name}\" đã tồn tại"));
        }
        conn.execute(
            "INSERT INTO ingredients(name,name_folded,unit,min_stock,note,created_at,updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,?6)",
            params![name, folded, unit, min_stock, note.trim(), now()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn update_ingredient(&self, id: i64, patch: &Value) -> Result<Value> {
        let conn = self.conn.lock().unwrap();
        if Self::ingredient_head(&conn, id).is_none() {
            return Err(anyhow!("nguyên liệu #{id} không tồn tại"));
        }
        let mut sets: Vec<String> = Vec::new();
        let mut vals: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(name) = patch.get("name").and_then(|x| x.as_str()) {
            let name = name.trim();
            if name.is_empty() {
                return Err(anyhow!("tên không được rỗng"));
            }
            let folded = fold_vi(name);
            if Self::name_taken(&conn, "ingredients", &folded, Some(id)) {
                return Err(anyhow!("nguyên liệu \"{name}\" đã tồn tại"));
            }
            sets.push("name=?".into());
            vals.push(Box::new(name.to_string()));
            sets.push("name_folded=?".into());
            vals.push(Box::new(folded));
        }
        if let Some(unit) = patch.get("unit").and_then(|x| x.as_str()) {
            let unit = unit.trim();
            if !BASE_UNITS.contains(&unit) {
                return Err(anyhow!("đơn vị gốc phải là một trong: {}", BASE_UNITS.join(", ")));
            }
            let has_moves: bool = conn
                .query_row(
                    "SELECT 1 FROM stock_moves WHERE ingredient_id=?1 LIMIT 1",
                    params![id],
                    |_| Ok(true),
                )
                .optional()?
                .unwrap_or(false);
            if has_moves {
                return Err(anyhow!(
                    "nguyên liệu đã có biến động kho — không đổi được đơn vị gốc (tạo nguyên liệu mới nếu cần)"
                ));
            }
            sets.push("unit=?".into());
            vals.push(Box::new(unit.to_string()));
        }
        if let Some(m) = patch.get("min_stock").and_then(|x| x.as_f64()) {
            if m < 0.0 {
                return Err(anyhow!("min_stock phải ≥ 0"));
            }
            sets.push("min_stock=?".into());
            vals.push(Box::new(m));
        }
        if let Some(n) = patch.get("note").and_then(|x| x.as_str()) {
            sets.push("note=?".into());
            vals.push(Box::new(n.trim().to_string()));
        }
        if let Some(a) = patch.get("active").and_then(|x| x.as_bool()) {
            sets.push("status=?".into());
            vals.push(Box::new(if a { "active" } else { "inactive" }.to_string()));
        }
        if sets.is_empty() {
            return Err(anyhow!(
                "không có gì để cập nhật (name, unit, min_stock, note, active)"
            ));
        }
        sets.push("updated_at=?".into());
        vals.push(Box::new(now()));
        vals.push(Box::new(id));
        let sql = format!("UPDATE ingredients SET {} WHERE id=?", sets.join(", "));
        conn.execute(&sql, params_from_iter(vals.iter().map(|b| b.as_ref())))?;
        drop(conn);
        self.get_ingredient(id)
            .ok_or_else(|| anyhow!("nguyên liệu #{id} không tồn tại"))
    }

    pub fn get_ingredient(&self, id: i64) -> Option<Value> {
        let conn = self.conn.lock().unwrap();
        let today = calc::today();
        Self::ingredient_rows(&conn, &today, None, false, true)
            .into_iter()
            .find(|v| v["id"] == json!(id))
    }

    pub fn list_ingredients(&self, q: Option<&str>, low_only: bool, include_inactive: bool) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let today = calc::today();
        Self::ingredient_rows(&conn, &today, q, low_only, include_inactive)
    }

    fn ingredient_rows(
        conn: &Connection,
        today: &str,
        q: Option<&str>,
        low_only: bool,
        include_inactive: bool,
    ) -> Vec<Value> {
        let stocks = Self::stocks_map(conn);
        let usage = Self::usage14_map(conn, today);
        let mut sql = String::from(
            "SELECT id,name,unit,min_stock,avg_cost,note,status,created_at,updated_at
             FROM ingredients WHERE 1=1",
        );
        let mut vals: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if !include_inactive {
            sql.push_str(" AND status='active'");
        }
        if let Some(q) = q.map(fold_vi).filter(|s| !s.trim().is_empty()) {
            sql.push_str(" AND name_folded LIKE ?");
            vals.push(Box::new(format!("%{}%", q.trim())));
        }
        sql.push_str(" ORDER BY name COLLATE NOCASE");
        let Ok(mut stmt) = conn.prepare(&sql) else {
            return Vec::new();
        };
        let rows = stmt
            .query_map(params_from_iter(vals.iter().map(|b| b.as_ref())), |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, f64>(3)?,
                    r.get::<_, f64>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, i64>(7)?,
                    r.get::<_, i64>(8)?,
                ))
            })
            .map(|it| it.filter_map(|r| r.ok()).collect::<Vec<_>>())
            .unwrap_or_default();
        let mut out = Vec::new();
        for (id, name, unit, min_stock, avg_cost, note, status, created_at, updated_at) in rows {
            let stock = *stocks.get(&id).unwrap_or(&0.0);
            let low = min_stock > 0.0 && stock < min_stock;
            if low_only && !low {
                continue;
            }
            let daily = *usage.get(&id).unwrap_or(&0.0);
            let days_left = if stock > 0.0 && daily > 0.0 {
                Some((stock / daily).floor() as i64)
            } else {
                None
            };
            out.push(json!({
                "id": id,
                "name": name,
                "unit": unit,
                "min_stock": round3(min_stock),
                "avg_cost": round2(avg_cost),
                "note": note,
                "status": status,
                "stock": round3(stock),
                "stock_display": qty_display(stock, &unit),
                "stock_value": round2(stock.max(0.0) * avg_cost),
                "low_stock": low,
                "avg_daily_14d": round3(daily),
                "days_left": days_left,
                "created_at": created_at,
                "updated_at": updated_at,
            }));
        }
        out
    }

    /// Điều chỉnh kiểm kê: `delta` có dấu HOẶC `set_qty` (đặt số đếm thực tế).
    pub fn adjust_stock(
        &self,
        id: i64,
        delta: Option<f64>,
        set_qty: Option<f64>,
        reason: &str,
    ) -> Result<Value> {
        let conn = self.conn.lock().unwrap();
        let Some((name, unit, avg)) = Self::ingredient_head(&conn, id) else {
            return Err(anyhow!("nguyên liệu #{id} không tồn tại"));
        };
        let stock = Self::stock_of(&conn, id);
        let d = match (delta, set_qty) {
            (Some(_), Some(_)) => {
                return Err(anyhow!("chỉ dùng MỘT trong 'delta' hoặc 'set_qty'"))
            }
            (Some(d), None) => d,
            (None, Some(s)) => s - stock,
            (None, None) => {
                return Err(anyhow!(
                    "cần 'delta' (chênh lệch có dấu) hoặc 'set_qty' (số đếm thực tế)"
                ))
            }
        };
        if d.abs() < 1e-9 {
            return Err(anyhow!("không có gì thay đổi (chênh lệch = 0)"));
        }
        conn.execute(
            "INSERT INTO stock_moves(ingredient_id,kind,qty,unit_cost,ref_kind,ref_id,note,move_date,created_at)
             VALUES(?1,'adjust',?2,?3,'',NULL,?4,?5,?6)",
            params![id, round3(d), avg, reason.trim(), calc::today(), now()],
        )?;
        Ok(json!({
            "ok": true,
            "ingredient_id": id,
            "name": name,
            "delta": round3(d),
            "stock": round3(stock + d),
            "stock_display": qty_display(stock + d, &unit),
        }))
    }

    /// Thẻ kho một nguyên liệu: số dư luỹ kế, lọc theo khoảng ngày, lấy tail `limit`.
    pub fn stock_card(
        &self,
        id: i64,
        from: Option<&str>,
        to: Option<&str>,
        limit: i64,
    ) -> Result<Value> {
        let conn = self.conn.lock().unwrap();
        let Some((name, unit, _avg)) = Self::ingredient_head(&conn, id) else {
            return Err(anyhow!("nguyên liệu #{id} không tồn tại"));
        };
        let mut stmt = conn.prepare(
            "SELECT kind, qty, unit_cost, ref_kind, ref_id, note, move_date
             FROM stock_moves WHERE ingredient_id=?1 ORDER BY created_at, id",
        )?;
        let all: Vec<(String, f64, f64, String, Option<i64>, String, String)> = stmt
            .query_map(params![id], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .collect();
        let from = from.unwrap_or("").trim().to_string();
        let to = to.unwrap_or("").trim().to_string();
        let mut balance = 0.0;
        let mut opening = 0.0;
        let mut rows: Vec<Value> = Vec::new();
        for (kind, qty, unit_cost, ref_kind, ref_id, note, move_date) in all {
            balance += qty;
            let in_window = (from.is_empty() || move_date.as_str() >= from.as_str())
                && (to.is_empty() || move_date.as_str() <= to.as_str());
            if !in_window {
                if rows.is_empty() {
                    opening = balance;
                }
                continue;
            }
            let r#ref = match (ref_kind.as_str(), ref_id) {
                ("purchase", Some(rid)) => doc_code("NH", rid),
                ("sale", Some(rid)) => doc_code("BH", rid),
                _ => String::new(),
            };
            rows.push(json!({
                "date": move_date,
                "kind": kind,
                "qty": round3(qty),
                "unit_cost": round2(unit_cost),
                "balance": round3(balance),
                "ref": r#ref,
                "note": note,
            }));
        }
        let limit = limit.clamp(1, 1000) as usize;
        if rows.len() > limit {
            let cut = rows.len() - limit;
            opening = rows[cut - 1]["balance"].as_f64().unwrap_or(opening);
            rows = rows.split_off(cut);
        }
        Ok(json!({
            "ingredient": { "id": id, "name": name, "unit": unit },
            "opening": round3(opening),
            "closing": round3(rows.last().and_then(|r| r["balance"].as_f64()).unwrap_or(balance)),
            "rows": rows,
        }))
    }

    // -------------------------------------------------------------- purchases

    pub fn create_purchase(
        &self,
        supplier: &str,
        date: &str,
        note: &str,
        lines: &[PurchaseLineIn],
    ) -> Result<Value> {
        if lines.is_empty() {
            return Err(anyhow!("phiếu nhập phải có ít nhất 1 dòng"));
        }
        let mut conn = self.conn.lock().unwrap();
        let date = if date.trim().is_empty() {
            calc::today()
        } else {
            date.trim().to_string()
        };
        struct Norm {
            ingredient_id: i64,
            qty_base: f64,
            qty_input: f64,
            unit_input: String,
            unit_price: f64,
            amount: f64,
            price_base: f64,
        }
        let mut norms: Vec<Norm> = Vec::new();
        // (tồn, BQGQ) hiện tại của từng nguyên liệu liên quan, chạy dồn theo dòng.
        let mut cur: BTreeMap<i64, (f64, f64)> = BTreeMap::new();
        for l in lines {
            let Some((name, base_unit, avg)) = Self::ingredient_head(&conn, l.ingredient_id)
            else {
                return Err(anyhow!("nguyên liệu #{} không tồn tại", l.ingredient_id));
            };
            let _ = name;
            if l.qty <= 0.0 {
                return Err(anyhow!("số lượng nhập phải > 0"));
            }
            if l.unit_price < 0.0 {
                return Err(anyhow!("đơn giá phải ≥ 0"));
            }
            let Some(factor) = unit_factor(&l.unit, &base_unit) else {
                return Err(anyhow!(
                    "đơn vị \"{}\" không dùng được cho nguyên liệu gốc \"{}\" (hợp lệ: g/kg cho g, ml/l/lít cho ml, cái cho cái)",
                    l.unit, base_unit
                ));
            };
            cur.entry(l.ingredient_id)
                .or_insert_with(|| (Self::stock_of(&conn, l.ingredient_id), avg));
            norms.push(Norm {
                ingredient_id: l.ingredient_id,
                qty_base: l.qty * factor,
                qty_input: l.qty,
                unit_input: l.unit.trim().to_lowercase(),
                unit_price: l.unit_price,
                amount: round2(l.qty * l.unit_price),
                price_base: l.unit_price / factor,
            });
        }
        // BQGQ chạy theo thứ tự dòng; tồn ≤ 0 hoặc chưa có giá → lấy giá nhập mới.
        for n in &norms {
            let e = cur.get_mut(&n.ingredient_id).unwrap();
            let (st, avg) = *e;
            let new_st = st + n.qty_base;
            let new_avg = if st <= 0.0 || avg <= 0.0 || new_st <= 0.0 {
                n.price_base
            } else {
                (st * avg + n.qty_base * n.price_base) / new_st
            };
            *e = (new_st, new_avg);
        }
        let total: f64 = norms.iter().map(|n| n.amount).sum();
        let ts = now();
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO purchases(code,supplier,purchase_date,note,total,created_at)
             VALUES('',?1,?2,?3,?4,?5)",
            params![supplier.trim(), date, note.trim(), round2(total), ts],
        )?;
        let id = tx.last_insert_rowid();
        let code = doc_code("NH", id);
        tx.execute("UPDATE purchases SET code=?2 WHERE id=?1", params![id, code])?;
        for n in &norms {
            tx.execute(
                "INSERT INTO purchase_lines(purchase_id,ingredient_id,qty,qty_input,unit_input,unit_price,amount)
                 VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![
                    id,
                    n.ingredient_id,
                    round3(n.qty_base),
                    n.qty_input,
                    n.unit_input,
                    n.unit_price,
                    n.amount
                ],
            )?;
            tx.execute(
                "INSERT INTO stock_moves(ingredient_id,kind,qty,unit_cost,ref_kind,ref_id,note,move_date,created_at)
                 VALUES(?1,'purchase',?2,?3,'purchase',?4,?5,?6,?7)",
                params![n.ingredient_id, round3(n.qty_base), n.price_base, id, "", date, ts],
            )?;
        }
        for (ing, (_st, avg)) in &cur {
            tx.execute(
                "UPDATE ingredients SET avg_cost=?2, updated_at=?3 WHERE id=?1",
                params![ing, avg, ts],
            )?;
        }
        tx.commit()?;
        drop(conn);
        Ok(self
            .get_purchase(id)
            .unwrap_or_else(|| json!({ "ok": true, "purchase_id": id, "code": code })))
    }

    pub fn get_purchase(&self, id: i64) -> Option<Value> {
        let conn = self.conn.lock().unwrap();
        let head = conn
            .query_row(
                "SELECT code,supplier,purchase_date,note,total,created_at FROM purchases WHERE id=?1",
                params![id],
                |r| {
                    Ok(json!({
                        "id": id,
                        "code": r.get::<_, String>(0)?,
                        "supplier": r.get::<_, String>(1)?,
                        "purchase_date": r.get::<_, String>(2)?,
                        "note": r.get::<_, String>(3)?,
                        "total": r.get::<_, f64>(4)?,
                        "created_at": r.get::<_, i64>(5)?,
                    }))
                },
            )
            .optional()
            .ok()
            .flatten()?;
        let mut head = head;
        let mut stmt = conn
            .prepare(
                "SELECT l.id,l.ingredient_id,i.name,i.unit,l.qty,l.qty_input,l.unit_input,l.unit_price,l.amount
                 FROM purchase_lines l JOIN ingredients i ON i.id=l.ingredient_id
                 WHERE l.purchase_id=?1 ORDER BY l.id",
            )
            .ok()?;
        let lines: Vec<Value> = stmt
            .query_map(params![id], |r| {
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "ingredient_id": r.get::<_, i64>(1)?,
                    "name": r.get::<_, String>(2)?,
                    "unit": r.get::<_, String>(3)?,
                    "qty": r.get::<_, f64>(4)?,
                    "qty_input": r.get::<_, f64>(5)?,
                    "unit_input": r.get::<_, String>(6)?,
                    "unit_price": r.get::<_, f64>(7)?,
                    "amount": r.get::<_, f64>(8)?,
                }))
            })
            .ok()?
            .filter_map(|r| r.ok())
            .collect();
        head["lines"] = json!(lines);
        Some(head)
    }

    pub fn list_purchases(
        &self,
        from: Option<&str>,
        to: Option<&str>,
        supplier: Option<&str>,
        limit: i64,
    ) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut sql = String::from(
            "SELECT p.id,p.code,p.supplier,p.purchase_date,p.note,p.total,p.created_at,
                    (SELECT COUNT(*) FROM purchase_lines l WHERE l.purchase_id=p.id)
             FROM purchases p WHERE 1=1",
        );
        let mut vals: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(f) = from.filter(|s| !s.trim().is_empty()) {
            sql.push_str(" AND p.purchase_date >= ?");
            vals.push(Box::new(f.trim().to_string()));
        }
        if let Some(t) = to.filter(|s| !s.trim().is_empty()) {
            sql.push_str(" AND p.purchase_date <= ?");
            vals.push(Box::new(t.trim().to_string()));
        }
        if let Some(s) = supplier.filter(|s| !s.trim().is_empty()) {
            sql.push_str(" AND p.supplier LIKE ?");
            vals.push(Box::new(format!("%{}%", s.trim())));
        }
        sql.push_str(" ORDER BY p.purchase_date DESC, p.id DESC LIMIT ?");
        vals.push(Box::new(limit.clamp(1, 500)));
        let Ok(mut stmt) = conn.prepare(&sql) else {
            return Vec::new();
        };
        stmt.query_map(params_from_iter(vals.iter().map(|b| b.as_ref())), |r| {
            Ok(json!({
                "id": r.get::<_, i64>(0)?,
                "code": r.get::<_, String>(1)?,
                "supplier": r.get::<_, String>(2)?,
                "purchase_date": r.get::<_, String>(3)?,
                "note": r.get::<_, String>(4)?,
                "total": r.get::<_, f64>(5)?,
                "created_at": r.get::<_, i64>(6)?,
                "line_count": r.get::<_, i64>(7)?,
            }))
        })
        .map(|it| it.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
    }

    /// Báo cáo nhập hàng theo supplier | ingredient | day trong khoảng ngày.
    pub fn report_purchases(&self, from: &str, to: &str, group_by: &str) -> Value {
        let conn = self.conn.lock().unwrap();
        let from = if from.trim().is_empty() { "0000-01-01" } else { from.trim() };
        let to = if to.trim().is_empty() { "9999-12-31" } else { to.trim() };
        let rows: Vec<Value> = match group_by {
            "supplier" => {
                let mut stmt = conn
                    .prepare(
                        "SELECT COALESCE(NULLIF(p.supplier,''),'(không rõ)') s,
                                COUNT(DISTINCT p.id), SUM(l.amount)
                         FROM purchases p JOIN purchase_lines l ON l.purchase_id=p.id
                         WHERE p.purchase_date BETWEEN ?1 AND ?2
                         GROUP BY s ORDER BY SUM(l.amount) DESC",
                    )
                    .unwrap();
                stmt.query_map(params![from, to], |r| {
                    Ok(json!({
                        "supplier": r.get::<_, String>(0)?,
                        "purchase_count": r.get::<_, i64>(1)?,
                        "amount": round2(r.get::<_, f64>(2)?),
                    }))
                })
                .map(|it| it.filter_map(|r| r.ok()).collect())
                .unwrap_or_default()
            }
            "day" => {
                let mut stmt = conn
                    .prepare(
                        "SELECT p.purchase_date, COUNT(DISTINCT p.id), SUM(l.amount)
                         FROM purchases p JOIN purchase_lines l ON l.purchase_id=p.id
                         WHERE p.purchase_date BETWEEN ?1 AND ?2
                         GROUP BY p.purchase_date ORDER BY p.purchase_date",
                    )
                    .unwrap();
                stmt.query_map(params![from, to], |r| {
                    Ok(json!({
                        "date": r.get::<_, String>(0)?,
                        "purchase_count": r.get::<_, i64>(1)?,
                        "amount": round2(r.get::<_, f64>(2)?),
                    }))
                })
                .map(|it| it.filter_map(|r| r.ok()).collect())
                .unwrap_or_default()
            }
            _ => {
                let mut stmt = conn
                    .prepare(
                        "SELECT i.name, i.unit, SUM(l.qty), SUM(l.amount)
                         FROM purchase_lines l
                         JOIN purchases p ON p.id=l.purchase_id
                         JOIN ingredients i ON i.id=l.ingredient_id
                         WHERE p.purchase_date BETWEEN ?1 AND ?2
                         GROUP BY l.ingredient_id ORDER BY SUM(l.amount) DESC",
                    )
                    .unwrap();
                stmt.query_map(params![from, to], |r| {
                    let qty: f64 = r.get(2)?;
                    let unit: String = r.get(1)?;
                    Ok(json!({
                        "ingredient": r.get::<_, String>(0)?,
                        "unit": unit,
                        "qty": round3(qty),
                        "qty_display": qty_display(qty, &unit),
                        "amount": round2(r.get::<_, f64>(3)?),
                    }))
                })
                .map(|it| it.filter_map(|r| r.ok()).collect())
                .unwrap_or_default()
            }
        };
        let (count, total): (i64, f64) = conn
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(total),0) FROM purchases WHERE purchase_date BETWEEN ?1 AND ?2",
                params![from, to],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap_or((0, 0.0));
        json!({
            "group_by": if group_by == "supplier" || group_by == "day" { group_by } else { "ingredient" },
            "from": from,
            "to": to,
            "rows": rows,
            "purchase_count": count,
            "total_amount": round2(total),
        })
    }

    // ------------------------------------------------------------------- menu

    pub fn add_menu(&self, name: &str, category: &str, price: f64, instructions: &str) -> Result<i64> {
        let name = name.trim();
        if name.is_empty() {
            return Err(anyhow!("thiếu tên món"));
        }
        if price < 0.0 {
            return Err(anyhow!("giá bán phải ≥ 0"));
        }
        let folded = fold_vi(name);
        let conn = self.conn.lock().unwrap();
        if Self::name_taken(&conn, "menu_items", &folded, None) {
            return Err(anyhow!("món \"{name}\" đã tồn tại"));
        }
        conn.execute(
            "INSERT INTO menu_items(name,name_folded,category,price,instructions,created_at,updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,?6)",
            params![name, folded, category.trim(), price, instructions.trim(), now()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn update_menu(&self, id: i64, patch: &Value) -> Result<Value> {
        let conn = self.conn.lock().unwrap();
        let exists: bool = conn
            .query_row("SELECT 1 FROM menu_items WHERE id=?1", params![id], |_| Ok(true))
            .optional()?
            .unwrap_or(false);
        if !exists {
            return Err(anyhow!("món #{id} không tồn tại"));
        }
        let mut sets: Vec<String> = Vec::new();
        let mut vals: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(name) = patch.get("name").and_then(|x| x.as_str()) {
            let name = name.trim();
            if name.is_empty() {
                return Err(anyhow!("tên không được rỗng"));
            }
            let folded = fold_vi(name);
            if Self::name_taken(&conn, "menu_items", &folded, Some(id)) {
                return Err(anyhow!("món \"{name}\" đã tồn tại"));
            }
            sets.push("name=?".into());
            vals.push(Box::new(name.to_string()));
            sets.push("name_folded=?".into());
            vals.push(Box::new(folded));
        }
        if let Some(c) = patch.get("category").and_then(|x| x.as_str()) {
            sets.push("category=?".into());
            vals.push(Box::new(c.trim().to_string()));
        }
        if let Some(p) = patch.get("price").and_then(|x| x.as_f64()) {
            if p < 0.0 {
                return Err(anyhow!("giá bán phải ≥ 0"));
            }
            sets.push("price=?".into());
            vals.push(Box::new(p));
        }
        if let Some(ins) = patch.get("instructions").and_then(|x| x.as_str()) {
            sets.push("instructions=?".into());
            vals.push(Box::new(ins.trim().to_string()));
        }
        if let Some(a) = patch.get("active").and_then(|x| x.as_bool()) {
            sets.push("status=?".into());
            vals.push(Box::new(if a { "active" } else { "inactive" }.to_string()));
        }
        if sets.is_empty() {
            return Err(anyhow!(
                "không có gì để cập nhật (name, category, price, instructions, active)"
            ));
        }
        sets.push("updated_at=?".into());
        vals.push(Box::new(now()));
        vals.push(Box::new(id));
        let sql = format!("UPDATE menu_items SET {} WHERE id=?", sets.join(", "));
        conn.execute(&sql, params_from_iter(vals.iter().map(|b| b.as_ref())))?;
        drop(conn);
        self.get_menu(id).ok_or_else(|| anyhow!("món #{id} không tồn tại"))
    }

    pub fn get_menu(&self, id: i64) -> Option<Value> {
        let conn = self.conn.lock().unwrap();
        Self::menu_value(&conn, id)
    }

    fn menu_value(conn: &Connection, id: i64) -> Option<Value> {
        let mut head = conn
            .query_row(
                "SELECT name,category,price,instructions,status,created_at,updated_at
                 FROM menu_items WHERE id=?1",
                params![id],
                |r| {
                    Ok(json!({
                        "id": id,
                        "name": r.get::<_, String>(0)?,
                        "category": r.get::<_, String>(1)?,
                        "price": r.get::<_, f64>(2)?,
                        "instructions": r.get::<_, String>(3)?,
                        "status": r.get::<_, String>(4)?,
                        "created_at": r.get::<_, i64>(5)?,
                        "updated_at": r.get::<_, i64>(6)?,
                    }))
                },
            )
            .optional()
            .ok()
            .flatten()?;
        let mut stmt = conn
            .prepare(
                "SELECT r.id, r.ingredient_id, i.name, i.unit, r.qty, i.avg_cost
                 FROM recipe_lines r JOIN ingredients i ON i.id=r.ingredient_id
                 WHERE r.menu_id=?1 ORDER BY r.id",
            )
            .ok()?;
        let recipe: Vec<Value> = stmt
            .query_map(params![id], |r| {
                let qty: f64 = r.get(4)?;
                let avg: f64 = r.get(5)?;
                let unit: String = r.get(3)?;
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "ingredient_id": r.get::<_, i64>(1)?,
                    "name": r.get::<_, String>(2)?,
                    "unit": unit,
                    "qty": round3(qty),
                    "unit_cost": round2(avg),
                    "cost": round2(qty * avg),
                }))
            })
            .ok()?
            .filter_map(|r| r.ok())
            .collect();
        let cost: f64 = recipe.iter().map(|l| l["cost"].as_f64().unwrap_or(0.0)).sum();
        let price = head["price"].as_f64().unwrap_or(0.0);
        head["recipe"] = json!(recipe);
        head["has_recipe"] = json!(!recipe.is_empty());
        head["cost"] = json!(round2(cost));
        head["margin"] = json!(round2(price - cost));
        head["margin_pct"] = json!(if price > 0.0 {
            round2((price - cost) / price * 100.0)
        } else {
            0.0
        });
        Some(head)
    }

    pub fn list_menu(&self, q: Option<&str>, category: Option<&str>, include_inactive: bool) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let costs = Self::menu_cost_map(&conn);
        let mut sql = String::from(
            "SELECT id,name,category,price,instructions,status,created_at,updated_at
             FROM menu_items WHERE 1=1",
        );
        let mut vals: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if !include_inactive {
            sql.push_str(" AND status='active'");
        }
        if let Some(q) = q.map(fold_vi).filter(|s| !s.trim().is_empty()) {
            sql.push_str(" AND name_folded LIKE ?");
            vals.push(Box::new(format!("%{}%", q.trim())));
        }
        if let Some(c) = category.filter(|s| !s.trim().is_empty()) {
            sql.push_str(" AND category = ?");
            vals.push(Box::new(c.trim().to_string()));
        }
        sql.push_str(" ORDER BY category COLLATE NOCASE, name COLLATE NOCASE");
        let Ok(mut stmt) = conn.prepare(&sql) else {
            return Vec::new();
        };
        stmt.query_map(params_from_iter(vals.iter().map(|b| b.as_ref())), |r| {
            let id: i64 = r.get(0)?;
            let price: f64 = r.get(3)?;
            let cost = *costs.get(&id).unwrap_or(&0.0);
            Ok(json!({
                "id": id,
                "name": r.get::<_, String>(1)?,
                "category": r.get::<_, String>(2)?,
                "price": price,
                "instructions": r.get::<_, String>(4)?,
                "status": r.get::<_, String>(5)?,
                "cost": round2(cost),
                "margin": round2(price - cost),
                "margin_pct": if price > 0.0 { round2((price - cost) / price * 100.0) } else { 0.0 },
                "has_recipe": costs.contains_key(&id),
                "created_at": r.get::<_, i64>(6)?,
                "updated_at": r.get::<_, i64>(7)?,
            }))
        })
        .map(|it| it.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
    }

    /// Đặt công thức món (THAY THẾ toàn bộ). `items` rỗng = xoá công thức.
    pub fn set_recipe(&self, menu_id: i64, items: &[RecipeItemIn]) -> Result<Value> {
        let mut conn = self.conn.lock().unwrap();
        let exists: bool = conn
            .query_row("SELECT 1 FROM menu_items WHERE id=?1", params![menu_id], |_| Ok(true))
            .optional()?
            .unwrap_or(false);
        if !exists {
            return Err(anyhow!("món #{menu_id} không tồn tại"));
        }
        let mut seen = std::collections::BTreeSet::new();
        for it in items {
            if it.qty <= 0.0 {
                return Err(anyhow!("định lượng phải > 0 (theo đơn vị gốc của nguyên liệu)"));
            }
            if Self::ingredient_head(&conn, it.ingredient_id).is_none() {
                return Err(anyhow!("nguyên liệu #{} không tồn tại", it.ingredient_id));
            }
            if !seen.insert(it.ingredient_id) {
                return Err(anyhow!(
                    "nguyên liệu #{} bị lặp trong công thức — gộp định lượng vào một dòng",
                    it.ingredient_id
                ));
            }
        }
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM recipe_lines WHERE menu_id=?1", params![menu_id])?;
        for it in items {
            tx.execute(
                "INSERT INTO recipe_lines(menu_id,ingredient_id,qty) VALUES(?1,?2,?3)",
                params![menu_id, it.ingredient_id, round3(it.qty)],
            )?;
        }
        tx.execute(
            "UPDATE menu_items SET updated_at=?2 WHERE id=?1",
            params![menu_id, now()],
        )?;
        tx.commit()?;
        drop(conn);
        self.get_menu(menu_id)
            .ok_or_else(|| anyhow!("món #{menu_id} không tồn tại"))
    }

    // ------------------------------------------------------------------ sales

    /// Ghi đơn bán: trừ kho theo công thức, chốt giá vốn tại thời điểm bán.
    /// Trả về đơn kèm `warnings` (món chưa công thức, nguyên liệu âm kho).
    pub fn create_sale(&self, date: &str, note: &str, lines: &[SaleLineIn]) -> Result<Value> {
        if lines.is_empty() {
            return Err(anyhow!("đơn bán phải có ít nhất 1 dòng"));
        }
        let mut conn = self.conn.lock().unwrap();
        let date = if date.trim().is_empty() {
            calc::today()
        } else {
            date.trim().to_string()
        };
        struct OutLine {
            menu_id: i64,
            name: String,
            qty: f64,
            unit_price: f64,
            amount: f64,
            cogs: f64,
        }
        let mut out_lines: Vec<OutLine> = Vec::new();
        let mut usage: BTreeMap<i64, f64> = BTreeMap::new();
        let mut warnings: Vec<String> = Vec::new();
        for l in lines {
            let row = conn
                .query_row(
                    "SELECT name, price, status FROM menu_items WHERE id=?1",
                    params![l.menu_id],
                    |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, f64>(1)?,
                            r.get::<_, String>(2)?,
                        ))
                    },
                )
                .optional()?;
            let Some((name, price, status)) = row else {
                return Err(anyhow!("món #{} không tồn tại", l.menu_id));
            };
            if status != "active" {
                return Err(anyhow!("món \"{name}\" đang ngừng bán"));
            }
            if l.qty <= 0.0 {
                return Err(anyhow!("số lượng bán phải > 0"));
            }
            let unit_price = l.unit_price.unwrap_or(price);
            if unit_price < 0.0 {
                return Err(anyhow!("đơn giá phải ≥ 0"));
            }
            let recipe: Vec<(i64, f64, f64)> = {
                let mut stmt = conn.prepare(
                    "SELECT r.ingredient_id, r.qty, i.avg_cost
                     FROM recipe_lines r JOIN ingredients i ON i.id=r.ingredient_id
                     WHERE r.menu_id=?1",
                )?;
                let v = stmt
                    .query_map(params![l.menu_id], |r| {
                        Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                    })?
                    .filter_map(|r| r.ok())
                    .collect();
                v
            };
            if recipe.is_empty() {
                warnings.push(format!(
                    "món \"{name}\" chưa có công thức — không trừ kho, giá vốn tính 0"
                ));
            }
            let mut cogs = 0.0;
            for (ing, rqty, avg) in &recipe {
                let u = rqty * l.qty;
                *usage.entry(*ing).or_default() += u;
                cogs += u * avg;
            }
            out_lines.push(OutLine {
                menu_id: l.menu_id,
                name,
                qty: l.qty,
                unit_price,
                amount: round2(l.qty * unit_price),
                cogs: round2(cogs),
            });
        }
        // Cảnh báo thiếu nguyên liệu (vẫn ghi — thực tế đã bán; kho có thể âm).
        let mut ing_avg: BTreeMap<i64, f64> = BTreeMap::new();
        for (ing, need) in &usage {
            let Some((name, unit, avg)) = Self::ingredient_head(&conn, *ing) else {
                return Err(anyhow!("nguyên liệu #{ing} không tồn tại"));
            };
            let stock = Self::stock_of(&conn, *ing);
            if stock + 1e-6 < *need {
                warnings.push(format!(
                    "nguyên liệu \"{name}\" thiếu: cần {} nhưng còn {} — kho sẽ âm, hãy nhập thêm hoặc kiểm kê",
                    qty_display(*need, &unit),
                    qty_display(stock, &unit)
                ));
            }
            ing_avg.insert(*ing, avg);
        }
        let total: f64 = out_lines.iter().map(|l| l.amount).sum();
        let cogs_total: f64 = out_lines.iter().map(|l| l.cogs).sum();
        let ts = now();
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO sales(code,sale_date,note,total,cogs,status,created_at)
             VALUES('',?1,?2,?3,?4,'done',?5)",
            params![date, note.trim(), round2(total), round2(cogs_total), ts],
        )?;
        let id = tx.last_insert_rowid();
        let code = doc_code("BH", id);
        tx.execute("UPDATE sales SET code=?2 WHERE id=?1", params![id, code])?;
        for l in &out_lines {
            tx.execute(
                "INSERT INTO sale_lines(sale_id,menu_id,menu_name,qty,unit_price,amount,cogs)
                 VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![id, l.menu_id, l.name, round3(l.qty), l.unit_price, l.amount, l.cogs],
            )?;
        }
        for (ing, u) in &usage {
            tx.execute(
                "INSERT INTO stock_moves(ingredient_id,kind,qty,unit_cost,ref_kind,ref_id,note,move_date,created_at)
                 VALUES(?1,'sale',?2,?3,'sale',?4,'',?5,?6)",
                params![ing, -round3(*u), ing_avg.get(ing).unwrap_or(&0.0), id, date, ts],
            )?;
        }
        tx.commit()?;
        drop(conn);
        let mut v = self
            .get_sale(id)
            .unwrap_or_else(|| json!({ "ok": true, "sale_id": id, "code": code }));
        v["warnings"] = json!(warnings);
        Ok(v)
    }

    pub fn get_sale(&self, id: i64) -> Option<Value> {
        let conn = self.conn.lock().unwrap();
        let mut head = conn
            .query_row(
                "SELECT code,sale_date,note,total,cogs,status,created_at FROM sales WHERE id=?1",
                params![id],
                |r| {
                    let total: f64 = r.get(3)?;
                    let cogs: f64 = r.get(4)?;
                    Ok(json!({
                        "id": id,
                        "code": r.get::<_, String>(0)?,
                        "sale_date": r.get::<_, String>(1)?,
                        "note": r.get::<_, String>(2)?,
                        "total": total,
                        "cogs": cogs,
                        "profit": round2(total - cogs),
                        "status": r.get::<_, String>(5)?,
                        "created_at": r.get::<_, i64>(6)?,
                    }))
                },
            )
            .optional()
            .ok()
            .flatten()?;
        let mut stmt = conn
            .prepare(
                "SELECT id,menu_id,menu_name,qty,unit_price,amount,cogs
                 FROM sale_lines WHERE sale_id=?1 ORDER BY id",
            )
            .ok()?;
        let lines: Vec<Value> = stmt
            .query_map(params![id], |r| {
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "menu_id": r.get::<_, i64>(1)?,
                    "menu_name": r.get::<_, String>(2)?,
                    "qty": r.get::<_, f64>(3)?,
                    "unit_price": r.get::<_, f64>(4)?,
                    "amount": r.get::<_, f64>(5)?,
                    "cogs": r.get::<_, f64>(6)?,
                }))
            })
            .ok()?
            .filter_map(|r| r.ok())
            .collect();
        head["lines"] = json!(lines);
        Some(head)
    }

    pub fn list_sales(
        &self,
        from: Option<&str>,
        to: Option<&str>,
        status: Option<&str>,
        limit: i64,
    ) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut sql = String::from(
            "SELECT s.id,s.code,s.sale_date,s.note,s.total,s.cogs,s.status,s.created_at,
                    (SELECT GROUP_CONCAT(printf('%gx %s', l.qty, l.menu_name), ', ')
                     FROM sale_lines l WHERE l.sale_id=s.id)
             FROM sales s WHERE 1=1",
        );
        let mut vals: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(f) = from.filter(|s| !s.trim().is_empty()) {
            sql.push_str(" AND s.sale_date >= ?");
            vals.push(Box::new(f.trim().to_string()));
        }
        if let Some(t) = to.filter(|s| !s.trim().is_empty()) {
            sql.push_str(" AND s.sale_date <= ?");
            vals.push(Box::new(t.trim().to_string()));
        }
        if let Some(st) = status.filter(|s| !s.trim().is_empty()) {
            sql.push_str(" AND s.status = ?");
            vals.push(Box::new(st.trim().to_string()));
        }
        sql.push_str(" ORDER BY s.sale_date DESC, s.id DESC LIMIT ?");
        vals.push(Box::new(limit.clamp(1, 500)));
        let Ok(mut stmt) = conn.prepare(&sql) else {
            return Vec::new();
        };
        stmt.query_map(params_from_iter(vals.iter().map(|b| b.as_ref())), |r| {
            let total: f64 = r.get(4)?;
            let cogs: f64 = r.get(5)?;
            Ok(json!({
                "id": r.get::<_, i64>(0)?,
                "code": r.get::<_, String>(1)?,
                "sale_date": r.get::<_, String>(2)?,
                "note": r.get::<_, String>(3)?,
                "total": total,
                "cogs": cogs,
                "profit": round2(total - cogs),
                "status": r.get::<_, String>(6)?,
                "created_at": r.get::<_, i64>(7)?,
                "items": r.get::<_, Option<String>>(8)?.unwrap_or_default(),
            }))
        })
        .map(|it| it.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
    }

    /// Huỷ đơn: hoàn nguyên liệu về kho (move `void`), loại khỏi báo cáo.
    pub fn void_sale(&self, id: i64, reason: &str) -> Result<Value> {
        let mut conn = self.conn.lock().unwrap();
        let status: Option<String> = conn
            .query_row("SELECT status FROM sales WHERE id=?1", params![id], |r| r.get(0))
            .optional()?;
        let Some(status) = status else {
            return Err(anyhow!("đơn #{id} không tồn tại"));
        };
        if status != "done" {
            return Err(anyhow!("đơn #{id} đã huỷ rồi"));
        }
        let moves: Vec<(i64, f64, f64)> = {
            let mut stmt = conn.prepare(
                "SELECT ingredient_id, qty, unit_cost FROM stock_moves
                 WHERE kind='sale' AND ref_kind='sale' AND ref_id=?1",
            )?;
            let v = stmt
                .query_map(params![id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .filter_map(|r| r.ok())
                .collect();
            v
        };
        let ts = now();
        let today = calc::today();
        let tx = conn.transaction()?;
        for (ing, qty, cost) in &moves {
            tx.execute(
                "INSERT INTO stock_moves(ingredient_id,kind,qty,unit_cost,ref_kind,ref_id,note,move_date,created_at)
                 VALUES(?1,'void',?2,?3,'sale',?4,?5,?6,?7)",
                params![ing, -qty, cost, id, reason.trim(), today, ts],
            )?;
        }
        tx.execute("UPDATE sales SET status='void' WHERE id=?1", params![id])?;
        tx.commit()?;
        drop(conn);
        self.get_sale(id).ok_or_else(|| anyhow!("đơn #{id} không tồn tại"))
    }

    // ---------------------------------------------------------------- reports

    /// Báo cáo doanh thu – giá vốn – lãi gộp theo day | item | category.
    pub fn report_revenue(&self, from: &str, to: &str, group_by: &str) -> Value {
        let conn = self.conn.lock().unwrap();
        let from = if from.trim().is_empty() { "0000-01-01" } else { from.trim() };
        let to = if to.trim().is_empty() { "9999-12-31" } else { to.trim() };
        let rows: Vec<Value> = match group_by {
            "item" => {
                let mut stmt = conn
                    .prepare(
                        "SELECT MAX(l.menu_name), SUM(l.qty), SUM(l.amount), SUM(l.cogs)
                         FROM sale_lines l JOIN sales s ON s.id=l.sale_id
                         WHERE s.status='done' AND s.sale_date BETWEEN ?1 AND ?2
                         GROUP BY l.menu_id ORDER BY SUM(l.amount) DESC",
                    )
                    .unwrap();
                stmt.query_map(params![from, to], |r| {
                    let rev: f64 = r.get(2)?;
                    let cogs: f64 = r.get(3)?;
                    Ok(json!({
                        "item": r.get::<_, String>(0)?,
                        "qty": round3(r.get::<_, f64>(1)?),
                        "revenue": round2(rev),
                        "cogs": round2(cogs),
                        "profit": round2(rev - cogs),
                        "margin_pct": if rev > 0.0 { round2((rev - cogs) / rev * 100.0) } else { 0.0 },
                    }))
                })
                .map(|it| it.filter_map(|r| r.ok()).collect())
                .unwrap_or_default()
            }
            "category" => {
                let mut stmt = conn
                    .prepare(
                        "SELECT COALESCE(NULLIF(m.category,''),'(chưa phân nhóm)') c,
                                SUM(l.qty), SUM(l.amount), SUM(l.cogs)
                         FROM sale_lines l
                         JOIN sales s ON s.id=l.sale_id
                         LEFT JOIN menu_items m ON m.id=l.menu_id
                         WHERE s.status='done' AND s.sale_date BETWEEN ?1 AND ?2
                         GROUP BY c ORDER BY SUM(l.amount) DESC",
                    )
                    .unwrap();
                stmt.query_map(params![from, to], |r| {
                    let rev: f64 = r.get(2)?;
                    let cogs: f64 = r.get(3)?;
                    Ok(json!({
                        "category": r.get::<_, String>(0)?,
                        "qty": round3(r.get::<_, f64>(1)?),
                        "revenue": round2(rev),
                        "cogs": round2(cogs),
                        "profit": round2(rev - cogs),
                        "margin_pct": if rev > 0.0 { round2((rev - cogs) / rev * 100.0) } else { 0.0 },
                    }))
                })
                .map(|it| it.filter_map(|r| r.ok()).collect())
                .unwrap_or_default()
            }
            _ => {
                let mut stmt = conn
                    .prepare(
                        "SELECT sale_date, COUNT(*), SUM(total), SUM(cogs)
                         FROM sales WHERE status='done' AND sale_date BETWEEN ?1 AND ?2
                         GROUP BY sale_date ORDER BY sale_date",
                    )
                    .unwrap();
                stmt.query_map(params![from, to], |r| {
                    let rev: f64 = r.get(2)?;
                    let cogs: f64 = r.get(3)?;
                    Ok(json!({
                        "date": r.get::<_, String>(0)?,
                        "orders": r.get::<_, i64>(1)?,
                        "revenue": round2(rev),
                        "cogs": round2(cogs),
                        "profit": round2(rev - cogs),
                    }))
                })
                .map(|it| it.filter_map(|r| r.ok()).collect())
                .unwrap_or_default()
            }
        };
        let (orders, revenue, cogs): (i64, f64, f64) = conn
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(total),0), COALESCE(SUM(cogs),0)
                 FROM sales WHERE status='done' AND sale_date BETWEEN ?1 AND ?2",
                params![from, to],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap_or((0, 0.0, 0.0));
        let items_sold: f64 = conn
            .query_row(
                "SELECT COALESCE(SUM(l.qty),0) FROM sale_lines l JOIN sales s ON s.id=l.sale_id
                 WHERE s.status='done' AND s.sale_date BETWEEN ?1 AND ?2",
                params![from, to],
                |r| r.get(0),
            )
            .unwrap_or(0.0);
        json!({
            "group_by": if group_by == "item" || group_by == "category" { group_by } else { "day" },
            "from": from,
            "to": to,
            "rows": rows,
            "orders": orders,
            "items_sold": round3(items_sold),
            "revenue": round2(revenue),
            "cogs": round2(cogs),
            "profit": round2(revenue - cogs),
        })
    }

    /// Tồn kho hiện tại: giá trị từng nguyên liệu, sắp hết, âm kho.
    pub fn report_inventory(&self) -> Value {
        let conn = self.conn.lock().unwrap();
        let today = calc::today();
        let rows = Self::ingredient_rows(&conn, &today, None, false, false);
        let total_value: f64 = rows.iter().map(|r| r["stock_value"].as_f64().unwrap_or(0.0)).sum();
        let low: Vec<Value> = rows.iter().filter(|r| r["low_stock"] == json!(true)).cloned().collect();
        let negative: Vec<Value> = rows
            .iter()
            .filter(|r| r["stock"].as_f64().unwrap_or(0.0) < -0.0005)
            .cloned()
            .collect();
        json!({
            "items": rows,
            "total_value": round2(total_value),
            "low_count": low.len(),
            "low": low,
            "negative": negative,
        })
    }

    fn day_totals(conn: &Connection, from: &str, to: &str) -> (i64, f64, f64) {
        conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(total),0), COALESCE(SUM(cogs),0)
             FROM sales WHERE status='done' AND sale_date BETWEEN ?1 AND ?2",
            params![from, to],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap_or((0, 0.0, 0.0))
    }

    /// Toàn cảnh quán cho UI + MCP + AI: hôm nay, 7 ngày, chuỗi 14 ngày, top
    /// món, cảnh báo kho + món chưa công thức.
    pub fn dashboard(&self, today: &str) -> Value {
        let conn = self.conn.lock().unwrap();
        let (t_orders, t_rev, t_cogs) = Self::day_totals(&conn, today, today);
        let week_from = date_add(today, -6);
        let (w_orders, w_rev, w_cogs) = Self::day_totals(&conn, &week_from, today);
        // Chuỗi 14 ngày (điền 0 ngày trống) cho biểu đồ.
        let mut daymap: BTreeMap<String, (f64, f64)> = BTreeMap::new();
        {
            let from14 = date_add(today, -13);
            let mut stmt = conn
                .prepare(
                    "SELECT sale_date, SUM(total), SUM(cogs) FROM sales
                     WHERE status='done' AND sale_date BETWEEN ?1 AND ?2 GROUP BY sale_date",
                )
                .unwrap();
            let rows: Vec<(String, f64, f64)> = stmt
                .query_map(params![from14, today], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?, r.get::<_, f64>(2)?))
                })
                .map(|it| it.filter_map(|r| r.ok()).collect())
                .unwrap_or_default();
            for (d, rev, cogs) in rows {
                daymap.insert(d, (rev, cogs));
            }
        }
        let revenue_14d: Vec<Value> = (0..14)
            .map(|i| {
                let d = date_add(today, i - 13);
                let (rev, cogs) = *daymap.get(&d).unwrap_or(&(0.0, 0.0));
                json!({ "date": d, "revenue": round2(rev), "profit": round2(rev - cogs) })
            })
            .collect();
        // Top món 7 ngày theo doanh thu.
        let top_items: Vec<Value> = {
            let mut stmt = conn
                .prepare(
                    "SELECT MAX(l.menu_name), SUM(l.qty), SUM(l.amount)
                     FROM sale_lines l JOIN sales s ON s.id=l.sale_id
                     WHERE s.status='done' AND s.sale_date BETWEEN ?1 AND ?2
                     GROUP BY l.menu_id ORDER BY SUM(l.amount) DESC LIMIT 5",
                )
                .unwrap();
            stmt.query_map(params![week_from, today], |r| {
                Ok(json!({
                    "name": r.get::<_, String>(0)?,
                    "qty": round3(r.get::<_, f64>(1)?),
                    "revenue": round2(r.get::<_, f64>(2)?),
                }))
            })
            .map(|it| it.filter_map(|r| r.ok()).collect())
            .unwrap_or_default()
        };
        let ing_rows = Self::ingredient_rows(&conn, today, None, false, false);
        let low: Vec<Value> = ing_rows
            .iter()
            .filter(|r| r["low_stock"] == json!(true))
            .map(|r| {
                json!({
                    "id": r["id"], "name": r["name"], "unit": r["unit"],
                    "stock": r["stock"], "stock_display": r["stock_display"],
                    "min_stock": r["min_stock"], "days_left": r["days_left"],
                })
            })
            .collect();
        let negative: Vec<Value> = ing_rows
            .iter()
            .filter(|r| r["stock"].as_f64().unwrap_or(0.0) < -0.0005)
            .map(|r| json!({ "id": r["id"], "name": r["name"], "stock_display": r["stock_display"] }))
            .collect();
        let stock_value: f64 = ing_rows.iter().map(|r| r["stock_value"].as_f64().unwrap_or(0.0)).sum();
        let no_recipe: Vec<Value> = {
            let mut stmt = conn
                .prepare(
                    "SELECT id, name FROM menu_items m
                     WHERE status='active'
                       AND NOT EXISTS(SELECT 1 FROM recipe_lines r WHERE r.menu_id=m.id)
                     ORDER BY name",
                )
                .unwrap();
            stmt.query_map([], |r| {
                Ok(json!({ "id": r.get::<_, i64>(0)?, "name": r.get::<_, String>(1)? }))
            })
            .map(|it| it.filter_map(|r| r.ok()).collect())
            .unwrap_or_default()
        };
        let menu_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM menu_items WHERE status='active'", [], |r| r.get(0))
            .unwrap_or(0);
        let ingredient_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM ingredients WHERE status='active'", [], |r| r.get(0))
            .unwrap_or(0);
        drop(conn);
        let recent_sales = self.list_sales(None, None, None, 8);
        let mut alerts: Vec<String> = Vec::new();
        for l in &low {
            alerts.push(format!(
                "nguyên liệu \"{}\" sắp hết: còn {} (ngưỡng {})",
                l["name"].as_str().unwrap_or(""),
                l["stock_display"].as_str().unwrap_or(""),
                l["min_stock"]
            ));
        }
        for n in &negative {
            alerts.push(format!(
                "kho ÂM: \"{}\" đang {} — cần kiểm kê",
                n["name"].as_str().unwrap_or(""),
                n["stock_display"].as_str().unwrap_or("")
            ));
        }
        if !no_recipe.is_empty() {
            let names: Vec<&str> = no_recipe.iter().filter_map(|m| m["name"].as_str()).collect();
            alerts.push(format!(
                "{} món chưa có công thức (giá vốn tính 0): {}",
                no_recipe.len(),
                names.join(", ")
            ));
        }
        json!({
            "today": { "date": today, "orders": t_orders, "revenue": round2(t_rev),
                        "cogs": round2(t_cogs), "profit": round2(t_rev - t_cogs) },
            "last7": { "from": week_from, "orders": w_orders, "revenue": round2(w_rev),
                        "profit": round2(w_rev - w_cogs) },
            "revenue_14d": revenue_14d,
            "top_items_7d": top_items,
            "low_stock": low,
            "negative_stock": negative,
            "no_recipe": no_recipe,
            "stock_value": round2(stock_value),
            "menu_count": menu_count,
            "ingredient_count": ingredient_count,
            "recent_sales": recent_sales,
            "alerts": alerts,
        })
    }

    // --------------------------------------------------------------- forecast

    /// Dự báo lượng bán per-item cho `days` ngày tới (trung bình cùng thứ 4
    /// tuần, lịch sử 28 ngày kết thúc hôm qua). Trả (item rows, future dates).
    fn item_forecasts(
        conn: &Connection,
        today: &str,
        days: usize,
    ) -> (Vec<(i64, String, f64, Vec<f64>)>, Vec<String>) {
        let from = date_add(today, -28);
        let hist_dates: Vec<String> = (0..28).map(|i| date_add(&from, i)).collect();
        let mut per_item: BTreeMap<i64, BTreeMap<String, f64>> = BTreeMap::new();
        if let Ok(mut stmt) = conn.prepare(
            "SELECT l.menu_id, s.sale_date, SUM(l.qty)
             FROM sale_lines l JOIN sales s ON s.id=l.sale_id
             WHERE s.status='done' AND s.sale_date >= ?1 AND s.sale_date < ?2
             GROUP BY l.menu_id, s.sale_date",
        ) {
            if let Ok(rows) = stmt.query_map(params![from, today], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, f64>(2)?))
            }) {
                for (mid, d, q) in rows.flatten() {
                    per_item.entry(mid).or_default().insert(d, q);
                }
            }
        }
        let items: Vec<(i64, String, f64)> = conn
            .prepare("SELECT id, name, price FROM menu_items WHERE status='active'")
            .and_then(|mut s| {
                let v = s
                    .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                    .filter_map(|r| r.ok())
                    .collect::<Vec<_>>();
                Ok(v)
            })
            .unwrap_or_default();
        let future_dates: Vec<String> = (0..days as i64).map(|i| date_add(today, i)).collect();
        let mut out = Vec::new();
        for (id, name, price) in items {
            let Some(map) = per_item.get(&id) else { continue };
            let series: Vec<f64> = hist_dates
                .iter()
                .map(|d| *map.get(d).unwrap_or(&0.0))
                .collect();
            if series.iter().sum::<f64>() <= 0.0 {
                continue;
            }
            let fc = forecast_series(&series, days);
            out.push((id, name, price, fc));
        }
        (out, future_dates)
    }

    /// Dự đoán lượng bán + doanh thu N ngày tới.
    pub fn forecast_sales(&self, today: &str, days: i64) -> Value {
        let days = days.clamp(1, 30) as usize;
        let conn = self.conn.lock().unwrap();
        let costs = Self::menu_cost_map(&conn);
        let (items, future_dates) = Self::item_forecasts(&conn, today, days);
        let mut per_day_rev = vec![0.0f64; days];
        let mut per_day_profit = vec![0.0f64; days];
        let mut item_rows: Vec<Value> = Vec::new();
        for (id, name, price, fc) in &items {
            let qty: f64 = fc.iter().sum();
            let cost = *costs.get(id).unwrap_or(&0.0);
            for (d, q) in fc.iter().enumerate() {
                per_day_rev[d] += q * price;
                per_day_profit[d] += q * (price - cost);
            }
            item_rows.push(json!({
                "menu_id": id,
                "name": name,
                "price": price,
                "forecast_qty": round3(qty),
                "forecast_revenue": round2(qty * price),
                "forecast_profit": round2(qty * (price - cost)),
                "per_day": fc.iter().map(|v| round3(*v)).collect::<Vec<_>>(),
            }));
        }
        item_rows.sort_by(|a, b| {
            b["forecast_revenue"]
                .as_f64()
                .unwrap_or(0.0)
                .partial_cmp(&a["forecast_revenue"].as_f64().unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let future: Vec<Value> = future_dates
            .iter()
            .enumerate()
            .map(|(i, d)| {
                json!({ "date": d, "revenue": round2(per_day_rev[i]), "profit": round2(per_day_profit[i]) })
            })
            .collect();
        json!({
            "days": days,
            "from": future_dates.first().cloned().unwrap_or_default(),
            "to": future_dates.last().cloned().unwrap_or_default(),
            "future": future,
            "items": item_rows,
            "total_revenue": round2(per_day_rev.iter().sum::<f64>()),
            "total_profit": round2(per_day_profit.iter().sum::<f64>()),
            "note": "Dự báo theo trung bình cùng thứ 4 tuần gần nhất (lịch sử 28 ngày) — chỉ là ước tính từ dữ liệu bán cũ.",
        })
    }

    fn forecast_ingredient_rows(conn: &Connection, today: &str, days: usize) -> Vec<Value> {
        let (items, _dates) = Self::item_forecasts(conn, today, days);
        let mut recipes: BTreeMap<i64, Vec<(i64, f64)>> = BTreeMap::new();
        if let Ok(mut stmt) =
            conn.prepare("SELECT menu_id, ingredient_id, qty FROM recipe_lines")
        {
            if let Ok(rows) = stmt.query_map([], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, f64>(2)?))
            }) {
                for (mid, ing, q) in rows.flatten() {
                    recipes.entry(mid).or_default().push((ing, q));
                }
            }
        }
        // usage[ingredient] = tiêu hao dự kiến từng ngày tương lai.
        let mut usage: BTreeMap<i64, Vec<f64>> = BTreeMap::new();
        for (mid, _name, _price, fc) in &items {
            if let Some(rs) = recipes.get(mid) {
                for (ing, rqty) in rs {
                    let e = usage.entry(*ing).or_insert_with(|| vec![0.0; days]);
                    for (d, q) in fc.iter().enumerate() {
                        e[d] += q * rqty;
                    }
                }
            }
        }
        let stocks = Self::stocks_map(conn);
        let actual14 = Self::usage14_map(conn, today);
        let mut rows: Vec<Value> = Vec::new();
        let Ok(mut stmt) = conn.prepare(
            "SELECT id,name,unit,min_stock,avg_cost FROM ingredients WHERE status='active' ORDER BY name",
        ) else {
            return rows;
        };
        let ings: Vec<(i64, String, String, f64, f64)> = stmt
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .map(|it| it.filter_map(|r| r.ok()).collect())
            .unwrap_or_default();
        for (id, name, unit, min_stock, avg_cost) in ings {
            let per_day = usage.get(&id).cloned().unwrap_or_else(|| vec![0.0; days]);
            let total_usage: f64 = per_day.iter().sum();
            let stock = *stocks.get(&id).unwrap_or(&0.0);
            let daily_actual = *actual14.get(&id).unwrap_or(&0.0);
            if total_usage <= 0.0 && stock == 0.0 && min_stock <= 0.0 && daily_actual <= 0.0 {
                continue; // nguyên liệu chưa dùng bao giờ — khỏi nhiễu báo cáo
            }
            // Cầm cự được bao nhiêu ngày: trừ dần theo dự báo, hết horizon thì
            // chạy tiếp bằng tốc độ trung bình.
            let mut s = stock;
            let mut days_left: Option<i64> = None;
            for (d, u) in per_day.iter().enumerate() {
                s -= u;
                if s < -1e-9 {
                    days_left = Some(d as i64);
                    break;
                }
            }
            if days_left.is_none() {
                let rate = if total_usage > 0.0 {
                    total_usage / days as f64
                } else {
                    daily_actual
                };
                if rate > 1e-9 && s >= 0.0 {
                    let extra = (s / rate).floor() as i64;
                    let total = days as i64 + extra;
                    days_left = if total > 365 { None } else { Some(total) };
                }
            }
            let need = (total_usage + min_stock - stock).max(0.0);
            rows.push(json!({
                "ingredient_id": id,
                "name": name,
                "unit": unit,
                "stock": round3(stock),
                "stock_display": qty_display(stock, &unit),
                "min_stock": round3(min_stock),
                "avg_cost": round2(avg_cost),
                "avg_daily_14d": round3(daily_actual),
                "forecast_usage": round3(total_usage),
                "usage_display": qty_display(total_usage, &unit),
                "days_left": days_left,
                "stockout_date": days_left.map(|d| date_add(today, d)),
                "need": round3(need),
                "need_display": qty_display(need, &unit),
                "est_cost": round2(need * avg_cost),
            }));
        }
        rows.sort_by(|a, b| {
            let da = a["days_left"].as_i64().unwrap_or(i64::MAX);
            let db = b["days_left"].as_i64().unwrap_or(i64::MAX);
            da.cmp(&db)
        });
        rows
    }

    /// Dự báo tiêu hao nguyên liệu N ngày tới + ngày hết hàng.
    pub fn forecast_ingredients(&self, today: &str, days: i64) -> Value {
        let days = days.clamp(1, 30) as usize;
        let conn = self.conn.lock().unwrap();
        let rows = Self::forecast_ingredient_rows(&conn, today, days);
        json!({
            "days": days,
            "rows": rows,
            "note": "Tiêu hao = dự báo lượng bán từng món × công thức hiện tại. Chỉ là ước tính.",
        })
    }

    /// Đề xuất nhập hàng: cần = tiêu hao dự kiến + tồn tối thiểu − tồn hiện tại.
    pub fn purchase_suggest(&self, today: &str, days: i64) -> Value {
        let days = days.clamp(1, 30) as usize;
        let conn = self.conn.lock().unwrap();
        let rows = Self::forecast_ingredient_rows(&conn, today, days);
        let suggest: Vec<Value> = rows
            .into_iter()
            .filter(|r| r["need"].as_f64().unwrap_or(0.0) > 0.0005)
            .collect();
        let total: f64 = suggest.iter().map(|r| r["est_cost"].as_f64().unwrap_or(0.0)).sum();
        json!({
            "days": days,
            "rows": suggest,
            "est_total_cost": round2(total),
            "note": "Cần nhập = tiêu hao dự kiến + tồn tối thiểu − tồn hiện tại. Giá ước theo giá vốn bình quân.",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Seed: cafe (g), sữa đặc (ml), ly nhựa (cái) + món "Cafe sữa" 30k
    /// công thức 20 g cafe + 30 ml sữa + 1 cái ly.
    fn seed(db: &Db) -> (i64, i64, i64, i64) {
        let cafe = db.add_ingredient("Cà phê bột", "g", 500.0, "").unwrap();
        let sua = db.add_ingredient("Sữa đặc", "ml", 300.0, "").unwrap();
        let ly = db.add_ingredient("Ly nhựa", "cái", 20.0, "").unwrap();
        let menu = db.add_menu("Cafe sữa", "Cà phê", 30_000.0, "Pha phin, thêm sữa").unwrap();
        db.set_recipe(
            menu,
            &[
                RecipeItemIn { ingredient_id: cafe, qty: 20.0 },
                RecipeItemIn { ingredient_id: sua, qty: 30.0 },
                RecipeItemIn { ingredient_id: ly, qty: 1.0 },
            ],
        )
        .unwrap();
        (cafe, sua, ly, menu)
    }

    fn pline(id: i64, qty: f64, unit: &str, price: f64) -> PurchaseLineIn {
        PurchaseLineIn { ingredient_id: id, qty, unit: unit.into(), unit_price: price }
    }

    fn stock_purchases(db: &Db, cafe: i64, sua: i64, ly: i64) {
        db.create_purchase(
            "NCC A",
            "2026-07-01",
            "",
            &[
                pline(cafe, 2.0, "kg", 200_000.0),
                pline(sua, 2.0, "l", 40_000.0),
                pline(ly, 100.0, "cái", 500.0),
            ],
        )
        .unwrap();
    }

    #[test]
    fn ingredient_validation_and_search() {
        let db = Db::open_memory().unwrap();
        assert!(db.add_ingredient("Đường", "thùng", 0.0, "").is_err());
        assert!(db.add_ingredient("", "g", 0.0, "").is_err());
        let id = db.add_ingredient("Đường cát", "g", 100.0, "").unwrap();
        // Trùng tên sau khi bỏ dấu cũng bị chặn.
        assert!(db.add_ingredient("Duong cat", "g", 0.0, "").is_err());
        let found = db.list_ingredients(Some("duong"), false, false);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0]["id"], json!(id));
        assert_eq!(db.list_ingredients(Some("ca phe"), false, false).len(), 0);
    }

    #[test]
    fn purchase_converts_units_and_weights_avg_cost() {
        let db = Db::open_memory().unwrap();
        let (cafe, sua, ly, _menu) = seed(&db);
        let p = db
            .create_purchase("NCC A", "2026-07-01", "lô đầu", &[pline(cafe, 2.0, "kg", 200_000.0)])
            .unwrap();
        assert_eq!(p["code"], "NH-0001");
        assert_eq!(p["total"], 400_000.0);
        assert_eq!(p["lines"][0]["qty"], 2000.0); // 2 kg → 2000 g
        let ing = db.get_ingredient(cafe).unwrap();
        assert_eq!(ing["stock"], 2000.0);
        assert_eq!(ing["avg_cost"], 200.0); // 200 000 đ/kg = 200 đ/g
        // Nhập thêm 1 kg giá 260 000 → BQGQ (2000×200 + 1000×260)/3000 = 220.
        db.create_purchase("NCC B", "2026-07-02", "", &[pline(cafe, 1.0, "kg", 260_000.0)])
            .unwrap();
        assert_eq!(db.get_ingredient(cafe).unwrap()["avg_cost"], 220.0);
        // Đơn vị sai loại bị chặn.
        assert!(db
            .create_purchase("x", "", "", &[pline(sua, 1.0, "kg", 10_000.0)])
            .is_err());
        assert!(db
            .create_purchase("x", "", "", &[pline(ly, 0.0, "cái", 500.0)])
            .is_err());
    }

    #[test]
    fn sale_deducts_stock_by_recipe_and_snapshots_cogs() {
        let db = Db::open_memory().unwrap();
        let (cafe, sua, ly, menu) = seed(&db);
        stock_purchases(&db, cafe, sua, ly);
        // Giá vốn món: 20×200 + 30×40 + 1×500 = 4000+1200+500 = 5700.
        let m = db.get_menu(menu).unwrap();
        assert_eq!(m["cost"], 5700.0);
        assert_eq!(m["margin"], 24_300.0);
        let s = db
            .create_sale("2026-07-02", "", &[SaleLineIn { menu_id: menu, qty: 2.0, unit_price: None }])
            .unwrap();
        assert_eq!(s["code"], "BH-0001");
        assert_eq!(s["total"], 60_000.0);
        assert_eq!(s["cogs"], 11_400.0);
        assert_eq!(s["profit"], 48_600.0);
        assert_eq!(s["warnings"].as_array().unwrap().len(), 0);
        assert_eq!(db.get_ingredient(cafe).unwrap()["stock"], 1960.0);
        assert_eq!(db.get_ingredient(sua).unwrap()["stock"], 1940.0);
        assert_eq!(db.get_ingredient(ly).unwrap()["stock"], 98.0);
        // Giá nhập sau đó tăng — cogs đơn cũ không đổi.
        db.create_purchase("NCC C", "2026-07-03", "", &[pline(cafe, 1.0, "kg", 500_000.0)])
            .unwrap();
        assert_eq!(db.get_sale(s["id"].as_i64().unwrap()).unwrap()["cogs"], 11_400.0);
    }

    #[test]
    fn sale_price_override_and_multi_lines() {
        let db = Db::open_memory().unwrap();
        let (cafe, sua, ly, menu) = seed(&db);
        stock_purchases(&db, cafe, sua, ly);
        let menu2 = db.add_menu("Bạc xỉu", "Cà phê", 35_000.0, "").unwrap();
        db.set_recipe(menu2, &[RecipeItemIn { ingredient_id: sua, qty: 50.0 }]).unwrap();
        let s = db
            .create_sale(
                "2026-07-02",
                "khách quen",
                &[
                    SaleLineIn { menu_id: menu, qty: 1.0, unit_price: Some(25_000.0) },
                    SaleLineIn { menu_id: menu2, qty: 2.0, unit_price: None },
                ],
            )
            .unwrap();
        assert_eq!(s["total"], 95_000.0); // 25k + 2×35k
        // Sữa dùng chung 2 món: 30 + 2×50 = 130 ml.
        assert_eq!(db.get_ingredient(sua).unwrap()["stock"], 1870.0);
        assert_eq!(s["lines"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn sale_without_recipe_warns_and_keeps_stock() {
        let db = Db::open_memory().unwrap();
        let menu = db.add_menu("Trà đá", "Trà", 5_000.0, "").unwrap();
        let s = db
            .create_sale("", "", &[SaleLineIn { menu_id: menu, qty: 3.0, unit_price: None }])
            .unwrap();
        assert_eq!(s["total"], 15_000.0);
        assert_eq!(s["cogs"], 0.0);
        let w = s["warnings"].as_array().unwrap();
        assert!(w.iter().any(|x| x.as_str().unwrap().contains("chưa có công thức")));
    }

    #[test]
    fn sale_over_stock_warns_and_goes_negative() {
        let db = Db::open_memory().unwrap();
        let (cafe, sua, ly, menu) = seed(&db);
        stock_purchases(&db, cafe, sua, ly); // ly: 100 cái
        let s = db
            .create_sale("", "", &[SaleLineIn { menu_id: menu, qty: 101.0, unit_price: None }])
            .unwrap();
        let w = s["warnings"].as_array().unwrap();
        assert!(w.iter().any(|x| x.as_str().unwrap().contains("thiếu")));
        assert_eq!(db.get_ingredient(ly).unwrap()["stock"], -1.0);
        // 101 ly ngốn 2020 g cafe / 3030 ml sữa / 101 ly — cả 3 đều âm kho.
        let inv = db.report_inventory();
        assert_eq!(inv["negative"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn void_sale_restores_stock_and_excludes_from_report() {
        let db = Db::open_memory().unwrap();
        let (cafe, sua, ly, menu) = seed(&db);
        stock_purchases(&db, cafe, sua, ly);
        let s = db
            .create_sale("2026-07-02", "", &[SaleLineIn { menu_id: menu, qty: 2.0, unit_price: None }])
            .unwrap();
        let sid = s["id"].as_i64().unwrap();
        let v = db.void_sale(sid, "ghi nhầm").unwrap();
        assert_eq!(v["status"], "void");
        assert_eq!(db.get_ingredient(cafe).unwrap()["stock"], 2000.0);
        assert_eq!(db.get_ingredient(ly).unwrap()["stock"], 100.0);
        let rep = db.report_revenue("2026-07-01", "2026-07-31", "day");
        assert_eq!(rep["revenue"], 0.0);
        assert!(db.void_sale(sid, "").is_err());
    }

    #[test]
    fn adjust_stock_delta_and_set_qty() {
        let db = Db::open_memory().unwrap();
        let (cafe, sua, ly, _menu) = seed(&db);
        stock_purchases(&db, cafe, sua, ly);
        let a = db.adjust_stock(cafe, Some(-50.0), None, "rơi vãi").unwrap();
        assert_eq!(a["stock"], 1950.0);
        let b = db.adjust_stock(cafe, None, Some(1900.0), "kiểm kê").unwrap();
        assert_eq!(b["delta"], -50.0);
        assert_eq!(b["stock"], 1900.0);
        assert!(db.adjust_stock(cafe, None, None, "").is_err());
        assert!(db.adjust_stock(cafe, Some(1.0), Some(2.0), "").is_err());
        assert!(db.adjust_stock(cafe, Some(0.0), None, "").is_err());
    }

    #[test]
    fn stock_card_running_balance() {
        let db = Db::open_memory().unwrap();
        let (cafe, sua, ly, menu) = seed(&db);
        stock_purchases(&db, cafe, sua, ly);
        db.create_sale("2026-07-02", "", &[SaleLineIn { menu_id: menu, qty: 2.0, unit_price: None }])
            .unwrap();
        db.adjust_stock(cafe, Some(10.0), None, "kiểm kê thừa").unwrap();
        let card = db.stock_card(cafe, None, None, 100).unwrap();
        let rows = card["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0]["kind"], "purchase");
        assert_eq!(rows[0]["balance"], 2000.0);
        assert_eq!(rows[1]["kind"], "sale");
        assert_eq!(rows[1]["balance"], 1960.0);
        assert_eq!(rows[1]["ref"], "BH-0001");
        assert_eq!(rows[2]["balance"], 1970.0);
        assert_eq!(card["closing"], 1970.0);
        assert!(db.stock_card(9999, None, None, 10).is_err());
    }

    #[test]
    fn recipe_replace_validate_and_clear() {
        let db = Db::open_memory().unwrap();
        let (cafe, sua, _ly, menu) = seed(&db);
        // Lặp nguyên liệu bị chặn.
        assert!(db
            .set_recipe(
                menu,
                &[
                    RecipeItemIn { ingredient_id: cafe, qty: 10.0 },
                    RecipeItemIn { ingredient_id: cafe, qty: 5.0 },
                ],
            )
            .is_err());
        assert!(db
            .set_recipe(menu, &[RecipeItemIn { ingredient_id: 9999, qty: 1.0 }])
            .is_err());
        // Thay thế toàn bộ.
        let m = db
            .set_recipe(menu, &[RecipeItemIn { ingredient_id: sua, qty: 40.0 }])
            .unwrap();
        assert_eq!(m["recipe"].as_array().unwrap().len(), 1);
        // Xoá công thức.
        let m = db.set_recipe(menu, &[]).unwrap();
        assert_eq!(m["has_recipe"], false);
        assert_eq!(m["cost"], 0.0);
    }

    #[test]
    fn update_ingredient_unit_locked_after_moves() {
        let db = Db::open_memory().unwrap();
        let id = db.add_ingredient("Bột cacao", "g", 0.0, "").unwrap();
        db.update_ingredient(id, &json!({ "unit": "ml" })).unwrap();
        db.create_purchase("", "", "", &[pline(id, 1.0, "l", 50_000.0)]).unwrap();
        assert!(db.update_ingredient(id, &json!({ "unit": "g" })).is_err());
        let v = db.update_ingredient(id, &json!({ "min_stock": 200.0, "active": false })).unwrap();
        assert_eq!(v["min_stock"], 200.0);
        assert_eq!(v["status"], "inactive");
    }

    #[test]
    fn menu_update_and_duplicate_checks() {
        let db = Db::open_memory().unwrap();
        let m1 = db.add_menu("Trà đào", "Trà", 40_000.0, "").unwrap();
        assert!(db.add_menu("Tra dao", "Trà", 1.0, "").is_err());
        let m2 = db.add_menu("Trà vải", "Trà", 40_000.0, "").unwrap();
        assert!(db.update_menu(m2, &json!({ "name": "Trà Đào" })).is_err());
        let v = db.update_menu(m1, &json!({ "price": 45_000.0, "active": false })).unwrap();
        assert_eq!(v["price"], 45_000.0);
        assert_eq!(v["status"], "inactive");
        // Món ngừng bán không cho lên đơn.
        assert!(db
            .create_sale("", "", &[SaleLineIn { menu_id: m1, qty: 1.0, unit_price: None }])
            .is_err());
    }

    #[test]
    fn report_revenue_groupings() {
        let db = Db::open_memory().unwrap();
        let (cafe, sua, ly, menu) = seed(&db);
        stock_purchases(&db, cafe, sua, ly);
        let menu2 = db.add_menu("Trà đào", "Trà", 40_000.0, "").unwrap();
        db.create_sale("2026-07-01", "", &[SaleLineIn { menu_id: menu, qty: 2.0, unit_price: None }])
            .unwrap();
        db.create_sale("2026-07-02", "", &[SaleLineIn { menu_id: menu2, qty: 1.0, unit_price: None }])
            .unwrap();
        let by_day = db.report_revenue("2026-07-01", "2026-07-31", "day");
        assert_eq!(by_day["rows"].as_array().unwrap().len(), 2);
        assert_eq!(by_day["revenue"], 100_000.0);
        assert_eq!(by_day["orders"], 2);
        assert_eq!(by_day["items_sold"], 3.0);
        let by_item = db.report_revenue("", "", "item");
        assert_eq!(by_item["rows"][0]["item"], "Cafe sữa"); // doanh thu cao hơn
        let by_cat = db.report_revenue("", "", "category");
        let cats: Vec<&str> = by_cat["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["category"].as_str().unwrap())
            .collect();
        assert!(cats.contains(&"Cà phê") && cats.contains(&"Trà"));
    }

    #[test]
    fn report_purchases_groupings() {
        let db = Db::open_memory().unwrap();
        let (cafe, sua, _ly, _menu) = seed(&db);
        db.create_purchase("NCC A", "2026-07-01", "", &[pline(cafe, 1.0, "kg", 200_000.0)])
            .unwrap();
        db.create_purchase("NCC B", "2026-07-02", "", &[pline(sua, 3.0, "l", 40_000.0)])
            .unwrap();
        let by_sup = db.report_purchases("2026-07-01", "2026-07-31", "supplier");
        assert_eq!(by_sup["rows"].as_array().unwrap().len(), 2);
        assert_eq!(by_sup["total_amount"], 320_000.0);
        assert_eq!(by_sup["purchase_count"], 2);
        let by_ing = db.report_purchases("", "", "ingredient");
        assert_eq!(by_ing["rows"][0]["ingredient"], "Cà phê bột");
        assert_eq!(by_ing["rows"][0]["qty"], 1000.0);
        let by_day = db.report_purchases("", "", "day");
        assert_eq!(by_day["rows"].as_array().unwrap().len(), 2);
        // Lọc ngày cắt đúng.
        let narrow = db.report_purchases("2026-07-02", "2026-07-02", "supplier");
        assert_eq!(narrow["total_amount"], 120_000.0);
    }

    #[test]
    fn dashboard_shape_and_alerts() {
        let db = Db::open_memory().unwrap();
        let (cafe, sua, ly, menu) = seed(&db);
        stock_purchases(&db, cafe, sua, ly);
        let today = calc::today();
        db.create_sale(&today, "", &[SaleLineIn { menu_id: menu, qty: 3.0, unit_price: None }])
            .unwrap();
        db.add_menu("Món mồ côi", "", 10_000.0, "").unwrap();
        let d = db.dashboard(&today);
        assert_eq!(d["today"]["orders"], 1);
        assert_eq!(d["today"]["revenue"], 90_000.0);
        assert_eq!(d["revenue_14d"].as_array().unwrap().len(), 14);
        assert_eq!(d["top_items_7d"][0]["name"], "Cafe sữa");
        // sữa: tồn 1910 ml — trên ngưỡng 300; ly: 97 — trên 20; cafe 1940 trên 500.
        assert_eq!(d["low_stock"].as_array().unwrap().len(), 0);
        assert_eq!(d["no_recipe"].as_array().unwrap().len(), 1);
        assert!(d["alerts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a.as_str().unwrap().contains("chưa có công thức")));
        assert!(d["stock_value"].as_f64().unwrap() > 0.0);
    }

    #[test]
    fn forecast_sales_from_history() {
        let db = Db::open_memory().unwrap();
        let menu = db.add_menu("Cafe đen", "Cà phê", 20_000.0, "").unwrap();
        let today = calc::today();
        // 28 ngày lịch sử, mỗi ngày bán đúng 10 ly (món không công thức — khỏi lo kho).
        for i in 1..=28 {
            let d = date_add(&today, -i);
            db.create_sale(&d, "", &[SaleLineIn { menu_id: menu, qty: 10.0, unit_price: None }])
                .unwrap();
        }
        let f = db.forecast_sales(&today, 7);
        assert_eq!(f["days"], 7);
        assert_eq!(f["future"].as_array().unwrap().len(), 7);
        assert_eq!(f["items"][0]["forecast_qty"], 70.0);
        assert_eq!(f["items"][0]["forecast_revenue"], 1_400_000.0);
        assert_eq!(f["total_revenue"], 1_400_000.0);
    }

    #[test]
    fn forecast_ingredients_and_purchase_suggest() {
        let db = Db::open_memory().unwrap();
        let bot = db.add_ingredient("Bột trà", "g", 100.0, "").unwrap();
        let menu = db.add_menu("Trà sữa", "Trà", 25_000.0, "").unwrap();
        db.set_recipe(menu, &[RecipeItemIn { ingredient_id: bot, qty: 10.0 }]).unwrap();
        let today = calc::today();
        db.create_purchase("NCC", &date_add(&today, -29), "", &[pline(bot, 10.0, "kg", 100_000.0)])
            .unwrap();
        for i in 1..=28 {
            let d = date_add(&today, -i);
            db.create_sale(&d, "", &[SaleLineIn { menu_id: menu, qty: 10.0, unit_price: None }])
                .unwrap();
        }
        // Đã dùng 28×100 g = 2800 g; kiểm kê đặt lại còn 500 g.
        db.adjust_stock(bot, None, Some(500.0), "kiểm kê").unwrap();
        let f = db.forecast_ingredients(&today, 7);
        let row = &f["rows"][0];
        assert_eq!(row["forecast_usage"], 700.0); // 7 ngày × 100 g
        assert_eq!(row["days_left"], 5); // 500 g / 100 g mỗi ngày
        assert_eq!(row["stockout_date"], json!(date_add(&today, 5)));
        // Cần nhập = 700 + min 100 − 500 = 300 g, giá vốn 100 đ/g → 30 000 đ.
        let s = db.purchase_suggest(&today, 7);
        assert_eq!(s["rows"][0]["need"], 300.0);
        assert_eq!(s["rows"][0]["est_cost"], 30_000.0);
        assert_eq!(s["est_total_cost"], 30_000.0);
    }

    #[test]
    fn list_and_get_helpers() {
        let db = Db::open_memory().unwrap();
        let (cafe, sua, ly, menu) = seed(&db);
        stock_purchases(&db, cafe, sua, ly);
        db.create_sale("2026-07-02", "", &[SaleLineIn { menu_id: menu, qty: 1.0, unit_price: None }])
            .unwrap();
        assert!(db.get_ingredient(9999).is_none());
        assert!(db.get_menu(9999).is_none());
        assert!(db.get_sale(9999).is_none());
        assert!(db.get_purchase(9999).is_none());
        assert_eq!(db.list_purchases(None, None, Some("NCC"), 50).len(), 1);
        assert_eq!(db.list_purchases(Some("2026-07-02"), None, None, 50).len(), 0);
        let sales = db.list_sales(None, None, Some("done"), 50);
        assert_eq!(sales.len(), 1);
        assert!(sales[0]["items"].as_str().unwrap().contains("Cafe sữa"));
        let menus = db.list_menu(Some("cafe sua"), None, false);
        assert_eq!(menus.len(), 1);
        assert_eq!(menus[0]["has_recipe"], true);
        assert_eq!(menus[0]["cost"], 5700.0);
    }
}
