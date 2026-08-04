//! Local SQLite store for the Thinking app (phân tích vấn đề theo 6 Mũ Tư Duy
//! + 5W). Everything is local-first — no external service holds this data.
//! Tables:
//!   * `problems`    — vấn đề cần phân tích (mô tả, bối cảnh, mục tiêu, trạng
//!                     thái open → analyzing → decided → closed, quyết định)
//!   * `five_w`      — 5 dòng phân tích Who/What/When/Where/Why (upsert theo
//!                     cặp problem+w)
//!   * `hats`        — 6 dòng góc nhìn white/red/black/yellow/green/blue
//!                     (upsert theo cặp problem+hat)
//!   * `solutions`   — các giải pháp đề xuất cho một vấn đề
//!   * `evaluations` — đánh giá một giải pháp: 4 tiêu chí 0–10 + điểm tổng hợp
//!                     0–100 (điểm tổng LUÔN do code tính — xem `logic.rs`)
//!   * `activity`    — log hành động của app/agent
//!   * `settings`    — kv dự phòng
//!
//! Độ hoàn thiện phân tích (completeness) KHÔNG lưu thành cột — luôn suy ra từ
//! số ô 5W/6 mũ đã điền, nên không bao giờ lệch với dữ liệu thật.

use crate::logic::{self, completeness_pct, overall_score};
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
CREATE TABLE IF NOT EXISTS problems (
  id                  INTEGER PRIMARY KEY AUTOINCREMENT,
  title               TEXT NOT NULL,
  description         TEXT NOT NULL DEFAULT '',
  context             TEXT NOT NULL DEFAULT '',
  goal                TEXT NOT NULL DEFAULT '',
  priority            TEXT NOT NULL DEFAULT 'normal',
  status              TEXT NOT NULL DEFAULT 'open',
  tags                TEXT NOT NULL DEFAULT '',
  synthesis           TEXT NOT NULL DEFAULT '',
  decision            TEXT NOT NULL DEFAULT '',
  decided_solution_id INTEGER,
  created_at          INTEGER NOT NULL,
  updated_at          INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_problems_status ON problems(status);
CREATE TABLE IF NOT EXISTS five_w (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  problem_id INTEGER NOT NULL,
  w          TEXT NOT NULL,
  content    TEXT NOT NULL DEFAULT '',
  source     TEXT NOT NULL DEFAULT 'user',
  updated_at INTEGER NOT NULL,
  UNIQUE(problem_id, w)
);
CREATE TABLE IF NOT EXISTS hats (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  problem_id INTEGER NOT NULL,
  hat        TEXT NOT NULL,
  content    TEXT NOT NULL DEFAULT '',
  source     TEXT NOT NULL DEFAULT 'user',
  updated_at INTEGER NOT NULL,
  UNIQUE(problem_id, hat)
);
CREATE TABLE IF NOT EXISTS solutions (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  problem_id INTEGER NOT NULL,
  title      TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  status     TEXT NOT NULL DEFAULT 'proposed',
  source     TEXT NOT NULL DEFAULT 'user',
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_solutions_problem ON solutions(problem_id);
CREATE TABLE IF NOT EXISTS evaluations (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  solution_id INTEGER NOT NULL UNIQUE,
  benefit     REAL NOT NULL,
  risk        REAL NOT NULL,
  feasibility REAL NOT NULL,
  effort      REAL NOT NULL,
  overall     REAL NOT NULL,
  verdict     TEXT NOT NULL DEFAULT '',
  detail      TEXT NOT NULL DEFAULT '',
  source      TEXT NOT NULL DEFAULT 'ai',
  updated_at  INTEGER NOT NULL
);
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

const PROBLEM_COLS: &str = "p.id, p.title, p.description, p.context, p.goal, p.priority, p.status, p.tags, p.synthesis, p.decision, p.decided_solution_id, p.created_at, p.updated_at, \
    (SELECT COUNT(*) FROM five_w f WHERE f.problem_id = p.id AND TRIM(f.content) <> '') AS w_filled, \
    (SELECT COUNT(*) FROM hats h WHERE h.problem_id = p.id AND TRIM(h.content) <> '') AS hats_filled, \
    (SELECT COUNT(*) FROM solutions s WHERE s.problem_id = p.id) AS solution_count";

fn row_to_problem(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let w_filled: i64 = row.get(13)?;
    let hats_filled: i64 = row.get(14)?;
    Ok(json!({
        "id": row.get::<_, i64>(0)?,
        "title": row.get::<_, String>(1)?,
        "description": row.get::<_, String>(2)?,
        "context": row.get::<_, String>(3)?,
        "goal": row.get::<_, String>(4)?,
        "priority": row.get::<_, String>(5)?,
        "status": row.get::<_, String>(6)?,
        "tags": row.get::<_, String>(7)?,
        "synthesis": row.get::<_, String>(8)?,
        "decision": row.get::<_, String>(9)?,
        "decided_solution_id": row.get::<_, Option<i64>>(10)?,
        "created_at": row.get::<_, i64>(11)?,
        "updated_at": row.get::<_, i64>(12)?,
        "w_filled": w_filled,
        "hats_filled": hats_filled,
        "solution_count": row.get::<_, i64>(15)?,
        "completeness": completeness_pct(w_filled as usize, hats_filled as usize),
    }))
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
                    .join("thinking")
            });
        std::fs::create_dir_all(&dir).ok();
        Self::open(dir.join("thinking.db"))
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

    // ---- activity ----

    pub fn log(&self, kind: &str, text: &str, r: &str) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO activity(kind, text, ref, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![kind, text, r, now()],
        );
    }

    pub fn recent_activity(&self, limit: i64) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT kind, text, ref, created_at FROM activity ORDER BY id DESC LIMIT ?1")
            .unwrap();
        stmt.query_map(params![limit], |row| {
            Ok(json!({
                "kind": row.get::<_, String>(0)?,
                "text": row.get::<_, String>(1)?,
                "ref": row.get::<_, String>(2)?,
                "created_at": row.get::<_, i64>(3)?,
            }))
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    // ---- problems ----

    pub fn add_problem(
        &self,
        title: &str,
        description: &str,
        context: &str,
        goal: &str,
        priority: &str,
        tags: &str,
    ) -> Result<i64> {
        let title = title.trim();
        if title.is_empty() {
            return Err(anyhow!("tiêu đề vấn đề không được để trống"));
        }
        let priority = match priority {
            "" => "normal",
            "low" | "normal" | "high" => priority,
            other => return Err(anyhow!("priority không hợp lệ: {other} (low|normal|high)")),
        };
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO problems(title, description, context, goal, priority, tags, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            params![title, description.trim(), context.trim(), goal.trim(), priority, tags.trim(), now()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn problem_brief(&self, id: i64) -> Option<Value> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            &format!("SELECT {PROBLEM_COLS} FROM problems p WHERE p.id = ?1"),
            params![id],
            row_to_problem,
        )
        .optional()
        .unwrap_or(None)
    }

    pub fn list_problems(&self, q: Option<&str>, status: Option<&str>, limit: i64) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut sql = format!("SELECT {PROBLEM_COLS} FROM problems p WHERE 1=1");
        let mut binds: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(q) = q.map(str::trim).filter(|s| !s.is_empty()) {
            sql.push_str(" AND (p.title LIKE ?1 OR p.description LIKE ?1 OR p.tags LIKE ?1)");
            binds.push(Box::new(format!("%{q}%")));
        }
        if let Some(st) = status.map(str::trim).filter(|s| !s.is_empty()) {
            sql.push_str(&format!(" AND p.status = ?{}", binds.len() + 1));
            binds.push(Box::new(st.to_string()));
        }
        sql.push_str(&format!(
            " ORDER BY p.updated_at DESC LIMIT ?{}",
            binds.len() + 1
        ));
        binds.push(Box::new(limit));
        let mut stmt = conn.prepare(&sql).unwrap();
        let refs: Vec<&dyn rusqlite::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
        stmt.query_map(refs.as_slice(), row_to_problem)
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    }

    /// Full detail: problem + map 5W + map 6 mũ (đủ mọi key, ô chưa điền content
    /// rỗng) + danh sách giải pháp kèm đánh giá.
    pub fn get_problem(&self, id: i64) -> Option<Value> {
        let problem = self.problem_brief(id)?;

        let mut five_w = serde_json::Map::new();
        for w in logic::W_KEYS {
            five_w.insert(
                w.into(),
                json!({ "content": "", "source": "", "updated_at": 0 }),
            );
        }
        let mut hats = serde_json::Map::new();
        for h in logic::HAT_KEYS {
            hats.insert(
                h.into(),
                json!({ "content": "", "source": "", "updated_at": 0 }),
            );
        }
        {
            let conn = self.conn.lock().unwrap();
            for (table, col, map) in [("five_w", "w", &mut five_w), ("hats", "hat", &mut hats)] {
                let mut stmt = conn
                    .prepare(&format!(
                        "SELECT {col}, content, source, updated_at FROM {table} WHERE problem_id = ?1"
                    ))
                    .unwrap();
                let rows = stmt
                    .query_map(params![id], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            json!({
                                "content": row.get::<_, String>(1)?,
                                "source": row.get::<_, String>(2)?,
                                "updated_at": row.get::<_, i64>(3)?,
                            }),
                        ))
                    })
                    .unwrap()
                    .filter_map(|r| r.ok());
                for (k, v) in rows {
                    map.insert(k, v);
                }
            }
        }

        Some(json!({
            "problem": problem,
            "five_w": five_w,
            "hats": hats,
            "solutions": self.list_solutions(id),
        }))
    }

    pub fn update_problem(&self, id: i64, patch: &Value) -> Result<()> {
        if self.problem_brief(id).is_none() {
            return Err(anyhow!("vấn đề #{id} không tồn tại"));
        }
        if let Some(st) = patch.get("status").and_then(|v| v.as_str()) {
            if !["open", "analyzing", "decided", "closed"].contains(&st) {
                return Err(anyhow!(
                    "status không hợp lệ: {st} (open|analyzing|decided|closed)"
                ));
            }
        }
        if let Some(pr) = patch.get("priority").and_then(|v| v.as_str()) {
            if !["low", "normal", "high"].contains(&pr) {
                return Err(anyhow!("priority không hợp lệ: {pr} (low|normal|high)"));
            }
        }
        let mut sets = Vec::new();
        let mut binds: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        for key in [
            "title",
            "description",
            "context",
            "goal",
            "priority",
            "status",
            "tags",
            "synthesis",
            "decision",
        ] {
            if let Some(v) = patch.get(key).and_then(|v| v.as_str()) {
                if key == "title" && v.trim().is_empty() {
                    return Err(anyhow!("tiêu đề vấn đề không được để trống"));
                }
                sets.push(format!("{key} = ?{}", binds.len() + 1));
                binds.push(Box::new(v.trim().to_string()));
            }
        }
        if sets.is_empty() {
            return Err(anyhow!("không có trường nào để cập nhật"));
        }
        sets.push(format!("updated_at = ?{}", binds.len() + 1));
        binds.push(Box::new(now()));
        let sql = format!(
            "UPDATE problems SET {} WHERE id = ?{}",
            sets.join(", "),
            binds.len() + 1
        );
        binds.push(Box::new(id));
        let conn = self.conn.lock().unwrap();
        let refs: Vec<&dyn rusqlite::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
        conn.execute(&sql, refs.as_slice())?;
        Ok(())
    }

    pub fn delete_problem(&self, id: i64) -> Result<Value> {
        let Some(p) = self.problem_brief(id) else {
            return Err(anyhow!("vấn đề #{id} không tồn tại"));
        };
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM evaluations WHERE solution_id IN (SELECT id FROM solutions WHERE problem_id = ?1)",
            params![id],
        )?;
        conn.execute("DELETE FROM solutions WHERE problem_id = ?1", params![id])?;
        conn.execute("DELETE FROM five_w WHERE problem_id = ?1", params![id])?;
        conn.execute("DELETE FROM hats WHERE problem_id = ?1", params![id])?;
        conn.execute("DELETE FROM problems WHERE id = ?1", params![id])?;
        Ok(json!({ "ok": true, "deleted": p["title"] }))
    }

    fn touch_problem(&self, id: i64) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "UPDATE problems SET updated_at = ?1 WHERE id = ?2",
            params![now(), id],
        );
    }

    // ---- 5W & hats ----

    pub fn set_w(&self, problem_id: i64, w: &str, content: &str, source: &str) -> Result<()> {
        if !logic::W_KEYS.contains(&w) {
            return Err(anyhow!("W không hợp lệ: {w} (who|what|when|where|why)"));
        }
        if self.problem_brief(problem_id).is_none() {
            return Err(anyhow!("vấn đề #{problem_id} không tồn tại"));
        }
        {
            let conn = self.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO five_w(problem_id, w, content, source, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(problem_id, w) DO UPDATE SET content = ?3, source = ?4, updated_at = ?5",
                params![problem_id, w, content.trim(), source, now()],
            )?;
        }
        self.touch_problem(problem_id);
        Ok(())
    }

    pub fn set_hat(&self, problem_id: i64, hat: &str, content: &str, source: &str) -> Result<()> {
        if !logic::HAT_KEYS.contains(&hat) {
            return Err(anyhow!(
                "mũ không hợp lệ: {hat} (white|red|black|yellow|green|blue)"
            ));
        }
        if self.problem_brief(problem_id).is_none() {
            return Err(anyhow!("vấn đề #{problem_id} không tồn tại"));
        }
        {
            let conn = self.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO hats(problem_id, hat, content, source, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(problem_id, hat) DO UPDATE SET content = ?3, source = ?4, updated_at = ?5",
                params![problem_id, hat, content.trim(), source, now()],
            )?;
        }
        self.touch_problem(problem_id);
        Ok(())
    }

    // ---- solutions & evaluations ----

    pub fn add_solution(
        &self,
        problem_id: i64,
        title: &str,
        description: &str,
        source: &str,
    ) -> Result<i64> {
        let title = title.trim();
        if title.is_empty() {
            return Err(anyhow!("tiêu đề giải pháp không được để trống"));
        }
        if self.problem_brief(problem_id).is_none() {
            return Err(anyhow!("vấn đề #{problem_id} không tồn tại"));
        }
        let id = {
            let conn = self.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO solutions(problem_id, title, description, source, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                params![problem_id, title, description.trim(), source, now()],
            )?;
            conn.last_insert_rowid()
        };
        self.touch_problem(problem_id);
        Ok(id)
    }

    pub fn get_solution(&self, id: i64) -> Option<Value> {
        let conn = self.conn.lock().unwrap();
        let base = conn
            .query_row(
                "SELECT id, problem_id, title, description, status, source, created_at, updated_at
                 FROM solutions WHERE id = ?1",
                params![id],
                |row| {
                    Ok(json!({
                        "id": row.get::<_, i64>(0)?,
                        "problem_id": row.get::<_, i64>(1)?,
                        "title": row.get::<_, String>(2)?,
                        "description": row.get::<_, String>(3)?,
                        "status": row.get::<_, String>(4)?,
                        "source": row.get::<_, String>(5)?,
                        "created_at": row.get::<_, i64>(6)?,
                        "updated_at": row.get::<_, i64>(7)?,
                    }))
                },
            )
            .optional()
            .unwrap_or(None)?;
        let eval = conn
            .query_row(
                "SELECT benefit, risk, feasibility, effort, overall, verdict, detail, source, updated_at
                 FROM evaluations WHERE solution_id = ?1",
                params![id],
                |row| {
                    Ok(json!({
                        "benefit": row.get::<_, f64>(0)?,
                        "risk": row.get::<_, f64>(1)?,
                        "feasibility": row.get::<_, f64>(2)?,
                        "effort": row.get::<_, f64>(3)?,
                        "overall": row.get::<_, f64>(4)?,
                        "verdict": row.get::<_, String>(5)?,
                        "detail": row.get::<_, String>(6)?,
                        "source": row.get::<_, String>(7)?,
                        "updated_at": row.get::<_, i64>(8)?,
                    }))
                },
            )
            .optional()
            .unwrap_or(None);
        let mut v = base;
        v["evaluation"] = eval.unwrap_or(Value::Null);
        Some(v)
    }

    pub fn list_solutions(&self, problem_id: i64) -> Vec<Value> {
        let ids: Vec<i64> = {
            let conn = self.conn.lock().unwrap();
            let mut stmt = conn
                .prepare("SELECT id FROM solutions WHERE problem_id = ?1 ORDER BY id")
                .unwrap();
            stmt.query_map(params![problem_id], |row| row.get(0))
                .unwrap()
                .filter_map(|r| r.ok())
                .collect()
        };
        ids.into_iter()
            .filter_map(|id| self.get_solution(id))
            .collect()
    }

    pub fn update_solution(&self, id: i64, patch: &Value) -> Result<()> {
        let Some(sol) = self.get_solution(id) else {
            return Err(anyhow!("giải pháp #{id} không tồn tại"));
        };
        if let Some(st) = patch.get("status").and_then(|v| v.as_str()) {
            if !["proposed", "chosen", "rejected"].contains(&st) {
                return Err(anyhow!(
                    "status không hợp lệ: {st} (proposed|chosen|rejected)"
                ));
            }
        }
        let mut sets = Vec::new();
        let mut binds: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        for key in ["title", "description", "status"] {
            if let Some(v) = patch.get(key).and_then(|v| v.as_str()) {
                if key == "title" && v.trim().is_empty() {
                    return Err(anyhow!("tiêu đề giải pháp không được để trống"));
                }
                sets.push(format!("{key} = ?{}", binds.len() + 1));
                binds.push(Box::new(v.trim().to_string()));
            }
        }
        if sets.is_empty() {
            return Err(anyhow!("không có trường nào để cập nhật"));
        }
        sets.push(format!("updated_at = ?{}", binds.len() + 1));
        binds.push(Box::new(now()));
        let sql = format!(
            "UPDATE solutions SET {} WHERE id = ?{}",
            sets.join(", "),
            binds.len() + 1
        );
        binds.push(Box::new(id));
        {
            let conn = self.conn.lock().unwrap();
            let refs: Vec<&dyn rusqlite::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
            conn.execute(&sql, refs.as_slice())?;
        }
        self.touch_problem(sol["problem_id"].as_i64().unwrap_or(0));
        Ok(())
    }

    pub fn delete_solution(&self, id: i64) -> Result<Value> {
        let Some(sol) = self.get_solution(id) else {
            return Err(anyhow!("giải pháp #{id} không tồn tại"));
        };
        let pid = sol["problem_id"].as_i64().unwrap_or(0);
        {
            let conn = self.conn.lock().unwrap();
            conn.execute(
                "DELETE FROM evaluations WHERE solution_id = ?1",
                params![id],
            )?;
            conn.execute("DELETE FROM solutions WHERE id = ?1", params![id])?;
            // Nếu đây là giải pháp đã chọn của vấn đề thì gỡ tham chiếu.
            conn.execute(
                "UPDATE problems SET decided_solution_id = NULL WHERE id = ?1 AND decided_solution_id = ?2",
                params![pid, id],
            )?;
        }
        self.touch_problem(pid);
        Ok(json!({ "ok": true, "deleted": sol["title"] }))
    }

    /// Ghi (upsert) đánh giá của một giải pháp. Điểm tổng hợp `overall` luôn do
    /// `logic::overall_score` tính — caller không bao giờ tự đặt.
    pub fn set_evaluation(
        &self,
        solution_id: i64,
        benefit: f64,
        risk: f64,
        feasibility: f64,
        effort: f64,
        verdict: &str,
        detail: &str,
        source: &str,
    ) -> Result<Value> {
        let Some(sol) = self.get_solution(solution_id) else {
            return Err(anyhow!("giải pháp #{solution_id} không tồn tại"));
        };
        let b = logic::clamp10(benefit);
        let r = logic::clamp10(risk);
        let f = logic::clamp10(feasibility);
        let e = logic::clamp10(effort);
        let overall = overall_score(b, r, f, e);
        {
            let conn = self.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO evaluations(solution_id, benefit, risk, feasibility, effort, overall, verdict, detail, source, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(solution_id) DO UPDATE SET benefit = ?2, risk = ?3, feasibility = ?4,
                   effort = ?5, overall = ?6, verdict = ?7, detail = ?8, source = ?9, updated_at = ?10",
                params![solution_id, b, r, f, e, overall, verdict.trim(), detail.trim(), source, now()],
            )?;
        }
        self.touch_problem(sol["problem_id"].as_i64().unwrap_or(0));
        Ok(self.get_solution(solution_id).unwrap_or(Value::Null))
    }

    /// Bảng so sánh deterministic: giải pháp đã đánh giá xếp hạng theo điểm
    /// tổng hợp giảm dần, kèm danh sách chưa đánh giá.
    pub fn compare(&self, problem_id: i64) -> Result<Value> {
        let Some(p) = self.problem_brief(problem_id) else {
            return Err(anyhow!("vấn đề #{problem_id} không tồn tại"));
        };
        let mut ranked: Vec<Value> = Vec::new();
        let mut unevaluated: Vec<Value> = Vec::new();
        for sol in self.list_solutions(problem_id) {
            if sol["evaluation"].is_null() {
                unevaluated.push(json!({ "id": sol["id"], "title": sol["title"] }));
            } else {
                ranked.push(sol);
            }
        }
        ranked.sort_by(|a, b| {
            let oa = a["evaluation"]["overall"].as_f64().unwrap_or(0.0);
            let ob = b["evaluation"]["overall"].as_f64().unwrap_or(0.0);
            ob.partial_cmp(&oa).unwrap_or(std::cmp::Ordering::Equal)
        });
        let best = ranked.first().cloned().unwrap_or(Value::Null);
        Ok(json!({
            "problem_id": problem_id,
            "title": p["title"],
            "ranked": ranked,
            "unevaluated": unevaluated,
            "best": best,
        }))
    }

    /// Chốt quyết định: chọn một giải pháp, ghi lý do, chuyển vấn đề sang
    /// `decided`. Giải pháp từng được chọn trước đó quay về `proposed`.
    pub fn decide(&self, problem_id: i64, solution_id: i64, rationale: &str) -> Result<Value> {
        if self.problem_brief(problem_id).is_none() {
            return Err(anyhow!("vấn đề #{problem_id} không tồn tại"));
        }
        let Some(sol) = self.get_solution(solution_id) else {
            return Err(anyhow!("giải pháp #{solution_id} không tồn tại"));
        };
        if sol["problem_id"].as_i64() != Some(problem_id) {
            return Err(anyhow!(
                "giải pháp #{solution_id} không thuộc vấn đề #{problem_id}"
            ));
        }
        {
            let conn = self.conn.lock().unwrap();
            conn.execute(
                "UPDATE solutions SET status = 'proposed', updated_at = ?1 WHERE problem_id = ?2 AND status = 'chosen'",
                params![now(), problem_id],
            )?;
            conn.execute(
                "UPDATE solutions SET status = 'chosen', updated_at = ?1 WHERE id = ?2",
                params![now(), solution_id],
            )?;
            conn.execute(
                "UPDATE problems SET status = 'decided', decision = ?1, decided_solution_id = ?2, updated_at = ?3 WHERE id = ?4",
                params![rationale.trim(), solution_id, now(), problem_id],
            )?;
        }
        Ok(json!({
            "ok": true,
            "problem": self.problem_brief(problem_id),
            "chosen": self.get_solution(solution_id),
        }))
    }

    // ---- dashboard & report ----

    pub fn dashboard(&self) -> Value {
        let count_status = |st: &str| -> i64 {
            let conn = self.conn.lock().unwrap();
            conn.query_row(
                "SELECT COUNT(*) FROM problems WHERE status = ?1",
                params![st],
                |r| r.get(0),
            )
            .unwrap_or(0)
        };
        let (total, solutions_total): (i64, i64) = {
            let conn = self.conn.lock().unwrap();
            (
                conn.query_row("SELECT COUNT(*) FROM problems", [], |r| r.get(0))
                    .unwrap_or(0),
                conn.query_row("SELECT COUNT(*) FROM solutions", [], |r| r.get(0))
                    .unwrap_or(0),
            )
        };
        let recent = self.list_problems(None, None, 8);
        let attention: Vec<Value> = self
            .list_problems(None, None, 200)
            .into_iter()
            .filter(|p| {
                let st = p["status"].as_str().unwrap_or("");
                (st == "open" || st == "analyzing")
                    && (p["completeness"].as_i64().unwrap_or(0) < 100
                        || p["solution_count"].as_i64().unwrap_or(0) == 0)
            })
            .take(8)
            .collect();
        json!({
            "problems_total": total,
            "by_status": {
                "open": count_status("open"),
                "analyzing": count_status("analyzing"),
                "decided": count_status("decided"),
                "closed": count_status("closed"),
            },
            "solutions_total": solutions_total,
            "recent": recent,
            "attention": attention,
            "activity": self.recent_activity(10),
        })
    }

    /// Báo cáo markdown deterministic của một vấn đề — cấu trúc đúng trình tự
    /// phương pháp: vấn đề → 5W → 6 mũ → giải pháp & điểm → tổng hợp → quyết định.
    pub fn report(&self, problem_id: i64) -> Result<String> {
        let Some(d) = self.get_problem(problem_id) else {
            return Err(anyhow!("vấn đề #{problem_id} không tồn tại"));
        };
        let p = &d["problem"];
        let mut md = String::new();
        let status_vi = match p["status"].as_str().unwrap_or("") {
            "open" => "Mới",
            "analyzing" => "Đang phân tích",
            "decided" => "Đã quyết định",
            "closed" => "Đã đóng",
            other => other,
        };
        md.push_str(&format!(
            "# 🎩 Báo cáo phân tích: {}\n\n",
            p["title"].as_str().unwrap_or("")
        ));
        md.push_str(&format!(
            "Trạng thái: **{}** · Độ hoàn thiện phân tích: **{}%** · Giải pháp: **{}**\n\n",
            status_vi,
            p["completeness"].as_i64().unwrap_or(0),
            p["solution_count"].as_i64().unwrap_or(0)
        ));
        md.push_str("## Vấn đề\n\n");
        for (label, key) in [
            ("Mô tả", "description"),
            ("Bối cảnh", "context"),
            ("Mục tiêu", "goal"),
        ] {
            let v = p[key].as_str().unwrap_or("").trim().to_string();
            if !v.is_empty() {
                md.push_str(&format!("- **{label}:** {v}\n"));
            }
        }
        md.push_str("\n## Phân tích 5W\n\n");
        for w in logic::W_KEYS {
            let c = d["five_w"][w]["content"]
                .as_str()
                .unwrap_or("")
                .trim()
                .to_string();
            let c = if c.is_empty() {
                "_chưa phân tích_".into()
            } else {
                c
            };
            md.push_str(&format!("- **{}:** {}\n", logic::w_label(w), c));
        }
        md.push_str("\n## 6 Mũ Tư Duy\n\n");
        for h in logic::HAT_KEYS {
            let c = d["hats"][h]["content"]
                .as_str()
                .unwrap_or("")
                .trim()
                .to_string();
            let c = if c.is_empty() {
                "_chưa phân tích_".into()
            } else {
                c
            };
            md.push_str(&format!("### {}\n\n{}\n\n", logic::hat_label(h), c));
        }
        md.push_str("## Giải pháp & đánh giá\n\n");
        let sols = d["solutions"].as_array().cloned().unwrap_or_default();
        if sols.is_empty() {
            md.push_str("_Chưa có giải pháp nào._\n\n");
        } else {
            md.push_str(
                "| Giải pháp | Lợi ích | Rủi ro | Khả thi | Công sức | Tổng | Trạng thái |\n",
            );
            md.push_str("|---|---|---|---|---|---|---|\n");
            for s in &sols {
                let e = &s["evaluation"];
                let cell = |k: &str| {
                    e.get(k)
                        .and_then(|v| v.as_f64())
                        .map(|v| format!("{v}"))
                        .unwrap_or_else(|| "—".into())
                };
                let st = match s["status"].as_str().unwrap_or("") {
                    "chosen" => "✅ Đã chọn",
                    "rejected" => "❌ Loại",
                    _ => "Đề xuất",
                };
                md.push_str(&format!(
                    "| {} | {} | {} | {} | {} | {} | {} |\n",
                    s["title"].as_str().unwrap_or("").replace('|', "/"),
                    cell("benefit"),
                    cell("risk"),
                    cell("feasibility"),
                    cell("effort"),
                    e.get("overall")
                        .and_then(|v| v.as_f64())
                        .map(|v| format!("**{v}**"))
                        .unwrap_or_else(|| "—".into()),
                    st,
                ));
            }
            md.push('\n');
            for s in &sols {
                let det = s["evaluation"]["detail"]
                    .as_str()
                    .unwrap_or("")
                    .trim()
                    .to_string();
                let ver = s["evaluation"]["verdict"]
                    .as_str()
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if !ver.is_empty() || !det.is_empty() {
                    md.push_str(&format!(
                        "**{}** — {}\n\n",
                        s["title"].as_str().unwrap_or(""),
                        ver
                    ));
                    if !det.is_empty() {
                        md.push_str(&format!("{det}\n\n"));
                    }
                }
            }
        }
        let synthesis = p["synthesis"].as_str().unwrap_or("").trim().to_string();
        if !synthesis.is_empty() {
            md.push_str(&format!(
                "## 🔵 Tổng hợp (Mũ Xanh Dương)\n\n{synthesis}\n\n"
            ));
        }
        let decision = p["decision"].as_str().unwrap_or("").trim().to_string();
        if !decision.is_empty() || p["decided_solution_id"].as_i64().is_some() {
            md.push_str("## ✅ Quyết định\n\n");
            if let Some(sid) = p["decided_solution_id"].as_i64() {
                if let Some(sol) = self.get_solution(sid) {
                    md.push_str(&format!(
                        "Giải pháp được chọn: **{}**\n\n",
                        sol["title"].as_str().unwrap_or("")
                    ));
                }
            }
            if !decision.is_empty() {
                md.push_str(&format!("{decision}\n"));
            }
        }
        Ok(md)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Db {
        Db::open_memory().unwrap()
    }

    fn seed_problem(db: &Db) -> i64 {
        db.add_problem(
            "Doanh số giảm 30%",
            "Doanh số quý này giảm mạnh",
            "Cửa hàng bán lẻ",
            "Phục hồi doanh số",
            "high",
            "kinh doanh",
        )
        .unwrap()
    }

    #[test]
    fn add_and_get_problem() {
        let db = db();
        let id = seed_problem(&db);
        let p = db.problem_brief(id).unwrap();
        assert_eq!(p["title"], "Doanh số giảm 30%");
        assert_eq!(p["status"], "open");
        assert_eq!(p["priority"], "high");
        assert_eq!(p["completeness"], 0);
        assert_eq!(p["solution_count"], 0);
    }

    #[test]
    fn empty_title_rejected() {
        let db = db();
        assert!(db.add_problem("  ", "", "", "", "", "").is_err());
        assert!(db.add_problem("x", "", "", "", "urgent", "").is_err());
    }

    #[test]
    fn five_w_upsert_and_completeness() {
        let db = db();
        let id = seed_problem(&db);
        db.set_w(id, "who", "Khách hàng quen, đội bán hàng", "user")
            .unwrap();
        db.set_w(id, "why", "Đối thủ mới giảm giá sâu", "ai")
            .unwrap();
        assert!(
            db.set_w(id, "how", "x", "user").is_err(),
            "5W không có 'how'"
        );
        assert!(db.set_w(999, "who", "x", "user").is_err());
        let p = db.problem_brief(id).unwrap();
        assert_eq!(p["w_filled"], 2);
        assert_eq!(p["completeness"], 16); // 2/5 * 40
                                           // Upsert: ghi đè không tạo dòng mới.
        db.set_w(id, "who", "Cập nhật lại", "user").unwrap();
        let p = db.problem_brief(id).unwrap();
        assert_eq!(p["w_filled"], 2);
        let d = db.get_problem(id).unwrap();
        assert_eq!(d["five_w"]["who"]["content"], "Cập nhật lại");
        assert_eq!(d["five_w"]["why"]["source"], "ai");
        assert_eq!(d["five_w"]["what"]["content"], "");
    }

    #[test]
    fn hats_upsert_and_completeness() {
        let db = db();
        let id = seed_problem(&db);
        for h in logic::HAT_KEYS {
            db.set_hat(id, h, &format!("nội dung mũ {h}"), "ai")
                .unwrap();
        }
        assert!(db.set_hat(id, "purple", "x", "user").is_err());
        let p = db.problem_brief(id).unwrap();
        assert_eq!(p["hats_filled"], 6);
        assert_eq!(p["completeness"], 60);
    }

    #[test]
    fn solutions_and_evaluation_flow() {
        let db = db();
        let id = seed_problem(&db);
        let s1 = db
            .add_solution(id, "Giảm giá 15%", "khuyến mãi ngắn hạn", "user")
            .unwrap();
        let s2 = db
            .add_solution(id, "Mở kênh online", "bán qua sàn TMĐT", "ai")
            .unwrap();
        assert!(db.add_solution(id, " ", "", "user").is_err());

        let v = db
            .set_evaluation(s1, 6.0, 7.0, 8.0, 3.0, "ổn", "chi tiết", "ai")
            .unwrap();
        // overall = (0.35*6 + 0.30*3 + 0.25*8 + 0.10*7)*10 = 57.0
        assert_eq!(v["evaluation"]["overall"], 57.0);
        db.set_evaluation(s2, 9.0, 4.0, 7.0, 6.0, "tốt", "", "ai")
            .unwrap();
        // overall = (0.35*9 + 0.30*6 + 0.25*7 + 0.10*4)*10 = 71.0

        let cmp = db.compare(id).unwrap();
        assert_eq!(
            cmp["ranked"][0]["id"], s2,
            "xếp hạng theo điểm tổng giảm dần"
        );
        assert_eq!(cmp["best"]["id"], s2);
        assert_eq!(cmp["unevaluated"].as_array().unwrap().len(), 0);

        // Đánh giá lại (upsert) không nhân đôi.
        db.set_evaluation(s1, 9.9, 0.0, 9.9, 0.0, "", "", "user")
            .unwrap();
        let sol = db.get_solution(s1).unwrap();
        assert!(sol["evaluation"]["overall"].as_f64().unwrap() > 95.0);
    }

    #[test]
    fn decide_flow() {
        let db = db();
        let id = seed_problem(&db);
        let s1 = db.add_solution(id, "A", "", "user").unwrap();
        let s2 = db.add_solution(id, "B", "", "user").unwrap();
        assert!(db.decide(id, 999, "x").is_err());
        let r = db.decide(id, s1, "chọn A vì rẻ").unwrap();
        assert_eq!(r["problem"]["status"], "decided");
        assert_eq!(r["chosen"]["status"], "chosen");
        // Đổi ý sang B: A quay về proposed.
        db.decide(id, s2, "đổi sang B").unwrap();
        assert_eq!(db.get_solution(s1).unwrap()["status"], "proposed");
        assert_eq!(db.get_solution(s2).unwrap()["status"], "chosen");
        let p = db.problem_brief(id).unwrap();
        assert_eq!(p["decided_solution_id"], s2);
        assert_eq!(p["decision"], "đổi sang B");
    }

    #[test]
    fn delete_cascades() {
        let db = db();
        let id = seed_problem(&db);
        db.set_w(id, "who", "x", "user").unwrap();
        db.set_hat(id, "white", "x", "user").unwrap();
        let s = db.add_solution(id, "A", "", "user").unwrap();
        db.set_evaluation(s, 5.0, 5.0, 5.0, 5.0, "", "", "ai")
            .unwrap();
        db.delete_problem(id).unwrap();
        assert!(db.problem_brief(id).is_none());
        assert!(db.get_solution(s).is_none());
        assert!(db.delete_problem(id).is_err());
    }

    #[test]
    fn delete_solution_clears_decision_ref() {
        let db = db();
        let id = seed_problem(&db);
        let s = db.add_solution(id, "A", "", "user").unwrap();
        db.decide(id, s, "ok").unwrap();
        db.delete_solution(s).unwrap();
        let p = db.problem_brief(id).unwrap();
        assert!(p["decided_solution_id"].is_null());
    }

    #[test]
    fn list_filters() {
        let db = db();
        let a = seed_problem(&db);
        let _b = db
            .add_problem("Tuyển dụng chậm", "", "", "", "low", "nhân sự")
            .unwrap();
        db.update_problem(a, &json!({ "status": "analyzing" }))
            .unwrap();
        assert_eq!(db.list_problems(None, None, 50).len(), 2);
        assert_eq!(db.list_problems(Some("doanh"), None, 50).len(), 1);
        assert_eq!(db.list_problems(None, Some("analyzing"), 50).len(), 1);
        assert_eq!(db.list_problems(Some("nhân sự"), Some("open"), 50).len(), 1);
        assert!(db.update_problem(a, &json!({ "status": "done" })).is_err());
    }

    #[test]
    fn report_structure() {
        let db = db();
        let id = seed_problem(&db);
        db.set_w(id, "who", "khách quen", "user").unwrap();
        db.set_hat(id, "black", "rủi ro mất khách", "ai").unwrap();
        let s = db.add_solution(id, "Giảm giá", "", "user").unwrap();
        db.set_evaluation(s, 6.0, 7.0, 8.0, 3.0, "tạm ổn", "", "ai")
            .unwrap();
        db.decide(id, s, "làm ngay tháng này").unwrap();
        let md = db.report(id).unwrap();
        assert!(md.contains("# 🎩 Báo cáo phân tích: Doanh số giảm 30%"));
        assert!(md.contains("Phân tích 5W"));
        assert!(md.contains("khách quen"));
        assert!(md.contains("_chưa phân tích_"));
        assert!(md.contains("Mũ Đen"));
        assert!(md.contains("| Giảm giá |"));
        assert!(md.contains("✅ Quyết định"));
        assert!(md.contains("làm ngay tháng này"));
        assert!(db.report(999).is_err());
    }

    #[test]
    fn dashboard_counts() {
        let db = db();
        let a = seed_problem(&db);
        let b = db.add_problem("Vấn đề B", "", "", "", "", "").unwrap();
        let s = db.add_solution(a, "A1", "", "user").unwrap();
        db.decide(a, s, "ok").unwrap();
        let dash = db.dashboard();
        assert_eq!(dash["problems_total"], 2);
        assert_eq!(dash["by_status"]["decided"], 1);
        assert_eq!(dash["by_status"]["open"], 1);
        assert_eq!(dash["solutions_total"], 1);
        // B mới tạo, chưa phân tích gì → nằm trong danh sách cần chú ý.
        let att: Vec<i64> = dash["attention"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["id"].as_i64().unwrap())
            .collect();
        assert!(att.contains(&b));
        assert!(!att.contains(&a), "vấn đề đã decided không cần chú ý");
    }
}
