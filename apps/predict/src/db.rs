//! Local SQLite store for the Siêu Dự Đoán app. External services are the source
//! of truth for raw data (fixtures, draws, prices, forecasts); we keep:
//!   * `settings`      — kv: api keys (optional), tracked leagues, cities, last-run marks
//!   * `elo_teams`     — ClubElo snapshot (club → Elo)
//!   * `fixtures`      — upcoming/past matches from TheSportsDB (résults used to resolve)
//!   * `lottery_draws` — XSMB history (one row per draw, 27 prize numbers + loto set)
//!   * `price_history` — XAU_USD / USD_VND / XAU_VND_LUONG time series
//!   * `weather_cache` — Open-Meteo payload per city
//!   * `predictions`   — the ledger: every forecast made, auto-scored (Brier) on resolve
//!   * `activity`      — local log

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Db {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS elo_teams (
  name       TEXT PRIMARY KEY,
  country    TEXT NOT NULL DEFAULT '',
  elo        REAL NOT NULL DEFAULT 1500,
  rank       INTEGER NOT NULL DEFAULT 0,
  updated_at INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS fixtures (
  event_id    TEXT PRIMARY KEY,
  league_id   TEXT NOT NULL DEFAULT '',
  league_name TEXT NOT NULL DEFAULT '',
  home        TEXT NOT NULL DEFAULT '',
  away        TEXT NOT NULL DEFAULT '',
  kickoff_ts  INTEGER NOT NULL DEFAULT 0,
  home_score  INTEGER,
  away_score  INTEGER,
  status      TEXT NOT NULL DEFAULT 'scheduled',
  updated_at  INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_fixtures_kickoff ON fixtures(kickoff_ts);
CREATE TABLE IF NOT EXISTS lottery_draws (
  date         TEXT PRIMARY KEY,
  numbers_json TEXT NOT NULL,
  loto_json    TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS price_history (
  id    INTEGER PRIMARY KEY AUTOINCREMENT,
  ts    INTEGER NOT NULL,
  asset TEXT NOT NULL,
  price REAL NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_price_asset_ts ON price_history(asset, ts);
CREATE TABLE IF NOT EXISTS weather_cache (
  city       TEXT PRIMARY KEY,
  payload    TEXT NOT NULL,
  fetched_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS predictions (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  domain       TEXT NOT NULL,             -- football | lottery | weather | market | generic
  subject      TEXT NOT NULL,             -- human-readable label
  detail       TEXT NOT NULL DEFAULT '{}',-- machine detail (event_id, numbers, city/date…)
  probs        TEXT NOT NULL DEFAULT '{}',-- outcome → probability map
  predicted_at INTEGER NOT NULL,
  due_at       INTEGER NOT NULL,
  resolved_at  INTEGER,
  outcome      TEXT,
  brier        REAL,
  correct      INTEGER
);
CREATE INDEX IF NOT EXISTS idx_pred_domain ON predictions(domain);
CREATE INDEX IF NOT EXISTS idx_pred_unresolved ON predictions(resolved_at) WHERE resolved_at IS NULL;
CREATE TABLE IF NOT EXISTS activity (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  kind       TEXT NOT NULL,
  text       TEXT NOT NULL DEFAULT '',
  ref        TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL
);
-- Generic, user-configurable prediction topics ("form chung").
CREATE TABLE IF NOT EXISTS topics (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  name        TEXT NOT NULL UNIQUE,
  description TEXT NOT NULL DEFAULT '',
  fields_json TEXT NOT NULL DEFAULT '[]',
  created_at  INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS topic_records (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  topic_id   INTEGER NOT NULL,
  data       TEXT NOT NULL,
  note       TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_topic_records ON topic_records(topic_id, id);
CREATE TABLE IF NOT EXISTS topic_rules (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  topic_id   INTEGER NOT NULL,
  rule       TEXT NOT NULL,
  confidence REAL NOT NULL DEFAULT 0.5,
  source     TEXT NOT NULL DEFAULT 'ai',
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_topic_rules ON topic_rules(topic_id);
-- Tài liệu / thông tin NGOÀI SỐ LIỆU của chủ đề: ghi chú, bài báo, giải thích…
-- `date` gắn tài liệu với một ngày (khớp bản ghi cùng ngày); `ref` gắn với một
-- giá trị/ngữ cảnh cụ thể (vd "giá=124", "đợt lạnh"). Cả hai đều tuỳ chọn.
CREATE TABLE IF NOT EXISTS topic_docs (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  topic_id   INTEGER NOT NULL,
  title      TEXT NOT NULL DEFAULT '',
  content    TEXT NOT NULL DEFAULT '',
  date       TEXT NOT NULL DEFAULT '',
  ref        TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_topic_docs ON topic_docs(topic_id, date);
"#;

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Additive migrations for DBs created before a column existed.
fn migrate(conn: &Connection) -> Result<()> {
    let has_source: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('topics') WHERE name='source_json'")?
        .exists([])?;
    if !has_source {
        conn.execute_batch("ALTER TABLE topics ADD COLUMN source_json TEXT NOT NULL DEFAULT '{\"kind\":\"manual\"}';")?;
    }
    // Cấu hình TĨNH của chủ đề (vị trí, thông số cố định) + tài liệu hướng dẫn
    // phân tích/prompt — khác với `fields_json` là dữ liệu ĐỘNG theo thời gian.
    let has_static: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('topics') WHERE name='static_json'")?
        .exists([])?;
    if !has_static {
        conn.execute_batch(
            "ALTER TABLE topics ADD COLUMN static_json TEXT NOT NULL DEFAULT '{}';
             ALTER TABLE topics ADD COLUMN guide TEXT NOT NULL DEFAULT '';",
        )?;
    }
    // DB tạo trước khi có kho tài liệu.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS topic_docs (
           id         INTEGER PRIMARY KEY AUTOINCREMENT,
           topic_id   INTEGER NOT NULL,
           title      TEXT NOT NULL DEFAULT '',
           content    TEXT NOT NULL DEFAULT '',
           date       TEXT NOT NULL DEFAULT '',
           ref        TEXT NOT NULL DEFAULT '',
           created_at INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_topic_docs ON topic_docs(topic_id, date);",
    )?;
    Ok(())
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
                    .join("predict")
            });
        std::fs::create_dir_all(&dir).ok();
        Self::open(dir.join("predict.db"))
    }

    pub fn open(path: PathBuf) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch(SCHEMA)?;
        migrate(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    #[cfg(test)]
    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        migrate(&conn)?;
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

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO settings(key,value) VALUES(?1,?2)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// Địa điểm cần fetch thời tiết = địa điểm của CÁC CHỦ ĐỀ weather.
    /// Nguồn dữ liệu do từng chủ đề khai, không còn setting toàn cục.
    pub fn cities(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for (_, _, source) in self.connector_topics() {
            if source["kind"] == "weather" {
                if let Some(c) = source["city"].as_str() {
                    if !out.iter().any(|x| x == c) {
                        out.push(c.to_string());
                    }
                }
            }
        }
        out
    }

    /// Giải cần fetch fixtures = giải của CÁC CHỦ ĐỀ football. Khi chưa có chủ
    /// đề nào, giữ EPL để các MCP tool bóng đá của agent vẫn có dữ liệu.
    pub fn leagues(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for (_, _, source) in self.connector_topics() {
            if source["kind"] == "football" {
                if let Some(l) = source["league"].as_str() {
                    if !out.iter().any(|x| x == l) {
                        out.push(l.to_string());
                    }
                }
            }
        }
        if out.is_empty() {
            out.push("4328".into());
        }
        out
    }

    /// User-added places: name → (lat, lon, note). Stored as one JSON setting so
    /// the city list is not limited to the built-in table.
    pub fn custom_places(&self) -> Value {
        self.get_setting("custom_places")
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| json!({}))
    }

    pub fn add_custom_place(&self, name: &str, lat: f64, lon: f64, note: &str) -> Result<()> {
        let mut places = self.custom_places();
        if let Some(obj) = places.as_object_mut() {
            obj.insert(
                name.trim().to_string(),
                json!({ "lat": lat, "lon": lon, "note": note }),
            );
        }
        self.set_setting("custom_places", &places.to_string())
    }

    pub fn remove_custom_place(&self, name: &str) -> Result<()> {
        let mut places = self.custom_places();
        if let Some(obj) = places.as_object_mut() {
            obj.remove(name.trim());
        }
        self.set_setting("custom_places", &places.to_string())
    }

    /// Coordinates for a place: built-in table first, then user-added.
    pub fn city_coord(&self, name: &str) -> Option<(String, f64, f64)> {
        if let Some((n, lat, lon)) = crate::fetch::find_city(name) {
            return Some((n.to_string(), lat, lon));
        }
        let places = self.custom_places();
        let obj = places.as_object()?;
        let key = obj.keys().find(|k| {
            k.eq_ignore_ascii_case(name.trim()) || k.to_lowercase() == name.trim().to_lowercase()
        })?;
        let p = &obj[key];
        Some((key.clone(), p["lat"].as_f64()?, p["lon"].as_f64()?))
    }

    /// User-added leagues: id → display name (beyond the built-in six).
    pub fn custom_leagues(&self) -> Value {
        self.get_setting("custom_leagues")
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| json!({}))
    }

    pub fn add_custom_league(&self, id: &str, name: &str) -> Result<()> {
        let mut m = self.custom_leagues();
        if let Some(obj) = m.as_object_mut() {
            obj.insert(id.trim().to_string(), json!(name.trim()));
        }
        self.set_setting("custom_leagues", &m.to_string())
    }

    /// Display name for a league id: built-in table, then user-added.
    pub fn league_label(&self, id: &str) -> String {
        let built_in = crate::fetch::league_name(id);
        if built_in != "Giải khác" {
            return built_in.to_string();
        }
        self.custom_leagues()[id]
            .as_str()
            .unwrap_or(built_in)
            .to_string()
    }

    // ---- elo ----

    pub fn upsert_elo(&self, name: &str, country: &str, elo: f64, rank: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO elo_teams(name,country,elo,rank,updated_at) VALUES(?1,?2,?3,?4,?5)
             ON CONFLICT(name) DO UPDATE SET country=excluded.country, elo=excluded.elo,
               rank=excluded.rank, updated_at=excluded.updated_at",
            params![name, country, elo, rank, now()],
        )?;
        Ok(())
    }

    pub fn all_elo(&self) -> Vec<(String, String, f64)> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT name,country,elo FROM elo_teams")
            .unwrap();
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, f64>(2)?,
                ))
            })
            .unwrap();
        rows.filter_map(|r| r.ok()).collect()
    }

    pub fn elo_top(&self, limit: i64) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT name,country,elo,rank FROM elo_teams WHERE rank>0 ORDER BY elo DESC LIMIT ?1")
            .unwrap();
        let rows = stmt
            .query_map(params![limit], |r| {
                Ok(json!({
                    "name": r.get::<_, String>(0)?,
                    "country": r.get::<_, String>(1)?,
                    "elo": (r.get::<_, f64>(2)? * 10.0).round() / 10.0,
                    "rank": r.get::<_, i64>(3)?,
                }))
            })
            .unwrap();
        rows.filter_map(|r| r.ok()).collect()
    }

    pub fn elo_count(&self) -> i64 {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT COUNT(*) FROM elo_teams", [], |r| r.get(0))
            .unwrap_or(0)
    }

    pub fn elo_updated_at(&self) -> i64 {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT MAX(updated_at) FROM elo_teams", [], |r| {
            r.get::<_, Option<i64>>(0)
        })
        .ok()
        .flatten()
        .unwrap_or(0)
    }

    // ---- fixtures ----

    #[allow(clippy::too_many_arguments)]
    pub fn upsert_fixture(
        &self,
        event_id: &str,
        league_id: &str,
        league_name: &str,
        home: &str,
        away: &str,
        kickoff_ts: i64,
        home_score: Option<i64>,
        away_score: Option<i64>,
        status: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO fixtures(event_id,league_id,league_name,home,away,kickoff_ts,home_score,away_score,status,updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
             ON CONFLICT(event_id) DO UPDATE SET
               league_id=excluded.league_id, league_name=excluded.league_name,
               home=excluded.home, away=excluded.away, kickoff_ts=excluded.kickoff_ts,
               home_score=excluded.home_score, away_score=excluded.away_score,
               status=excluded.status, updated_at=excluded.updated_at",
            params![event_id, league_id, league_name, home, away, kickoff_ts, home_score, away_score, status, now()],
        )?;
        Ok(())
    }

    pub fn fixtures_upcoming(&self, from_ts: i64, limit: i64) -> Vec<Fixture> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT event_id,league_id,league_name,home,away,kickoff_ts,home_score,away_score,status
                 FROM fixtures WHERE kickoff_ts>=?1 AND home_score IS NULL ORDER BY kickoff_ts LIMIT ?2",
            )
            .unwrap();
        let rows = stmt
            .query_map(params![from_ts, limit], Fixture::from_row)
            .unwrap();
        rows.filter_map(|r| r.ok()).collect()
    }

    pub fn fixture(&self, event_id: &str) -> Option<Fixture> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT event_id,league_id,league_name,home,away,kickoff_ts,home_score,away_score,status
             FROM fixtures WHERE event_id=?1",
            params![event_id],
            Fixture::from_row,
        )
        .optional()
        .ok()
        .flatten()
    }

    pub fn fixtures_count(&self) -> i64 {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT COUNT(*) FROM fixtures", [], |r| r.get(0))
            .unwrap_or(0)
    }

    /// Finished matches of a league, newest first (topic-connector feed).
    pub fn fixtures_finished(&self, league_id: &str, limit: i64) -> Vec<Fixture> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT event_id,league_id,league_name,home,away,kickoff_ts,home_score,away_score,status
                 FROM fixtures WHERE league_id=?1 AND home_score IS NOT NULL
                 ORDER BY kickoff_ts DESC LIMIT ?2",
            )
            .unwrap();
        let rows = stmt
            .query_map(params![league_id, limit], Fixture::from_row)
            .unwrap();
        rows.filter_map(|r| r.ok()).collect()
    }

    // ---- lottery ----

    pub fn upsert_draw(&self, date: &str, numbers: &[i64], loto: &[u8]) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO lottery_draws(date,numbers_json,loto_json) VALUES(?1,?2,?3)
             ON CONFLICT(date) DO UPDATE SET numbers_json=excluded.numbers_json, loto_json=excluded.loto_json",
            params![date, serde_json::to_string(numbers)?, serde_json::to_string(loto)?],
        )?;
        Ok(())
    }

    pub fn latest_draw(&self) -> Option<(String, Vec<i64>, Vec<u8>)> {
        self.draws(1).into_iter().next()
    }

    pub fn draw_by_date(&self, date: &str) -> Option<(String, Vec<i64>, Vec<u8>)> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT date,numbers_json,loto_json FROM lottery_draws WHERE date=?1",
            params![date],
            row_to_draw,
        )
        .optional()
        .ok()
        .flatten()
    }

    /// Newest-first draws, `limit` rows.
    pub fn draws(&self, limit: i64) -> Vec<(String, Vec<i64>, Vec<u8>)> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT date,numbers_json,loto_json FROM lottery_draws ORDER BY date DESC LIMIT ?1",
            )
            .unwrap();
        let rows = stmt.query_map(params![limit], row_to_draw).unwrap();
        rows.filter_map(|r| r.ok()).collect()
    }

    pub fn draws_count(&self) -> i64 {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT COUNT(*) FROM lottery_draws", [], |r| r.get(0))
            .unwrap_or(0)
    }

    // ---- prices ----

    pub fn add_price(&self, asset: &str, price: f64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO price_history(ts,asset,price) VALUES(?1,?2,?3)",
            params![now(), asset, price],
        )?;
        Ok(())
    }

    pub fn latest_price(&self, asset: &str) -> Option<(i64, f64)> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT ts,price FROM price_history WHERE asset=?1 ORDER BY ts DESC LIMIT 1",
            params![asset],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .ok()
        .flatten()
    }

    /// Ascending (ts, price) series since `since_ts`.
    pub fn price_series(&self, asset: &str, since_ts: i64) -> Vec<(i64, f64)> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT ts,price FROM price_history WHERE asset=?1 AND ts>=?2 ORDER BY ts")
            .unwrap();
        let rows = stmt
            .query_map(params![asset, since_ts], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?))
            })
            .unwrap();
        rows.filter_map(|r| r.ok()).collect()
    }

    // ---- weather cache ----

    pub fn weather_get(&self, city: &str) -> Option<(Value, i64)> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT payload,fetched_at FROM weather_cache WHERE city=?1",
            params![city],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
        )
        .optional()
        .ok()
        .flatten()
        .and_then(|(p, t)| serde_json::from_str(&p).ok().map(|v| (v, t)))
    }

    pub fn weather_set(&self, city: &str, payload: &Value) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO weather_cache(city,payload,fetched_at) VALUES(?1,?2,?3)
             ON CONFLICT(city) DO UPDATE SET payload=excluded.payload, fetched_at=excluded.fetched_at",
            params![city, payload.to_string(), now()],
        )?;
        Ok(())
    }

    // ---- predictions ledger ----

    pub fn add_prediction(&self, p: &PredictionInput) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO predictions(domain,subject,detail,probs,predicted_at,due_at)
             VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                p.domain,
                p.subject,
                p.detail.to_string(),
                p.probs.to_string(),
                now(),
                p.due_at
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Skip duplicate ledger rows: an unresolved prediction in `domain` whose
    /// detail contains `"key":"value"` already exists.
    pub fn has_open_prediction(&self, domain: &str, detail_key: &str, detail_value: &str) -> bool {
        let needle = format!("\"{}\":\"{}\"", detail_key, detail_value);
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT 1 FROM predictions WHERE domain=?1 AND resolved_at IS NULL AND instr(detail,?2)>0 LIMIT 1",
            params![domain, needle],
            |_| Ok(()),
        )
        .optional()
        .ok()
        .flatten()
        .is_some()
    }

    pub fn get_prediction(&self, id: i64) -> Option<Prediction> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id,domain,subject,detail,probs,predicted_at,due_at,resolved_at,outcome,brier,correct
             FROM predictions WHERE id=?1",
            params![id],
            Prediction::from_row,
        )
        .optional()
        .ok()
        .flatten()
    }

    pub fn unresolved_due(&self, due_before: i64) -> Vec<Prediction> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id,domain,subject,detail,probs,predicted_at,due_at,resolved_at,outcome,brier,correct
                 FROM predictions WHERE resolved_at IS NULL AND due_at<=?1 ORDER BY due_at LIMIT 200",
            )
            .unwrap();
        let rows = stmt
            .query_map(params![due_before], Prediction::from_row)
            .unwrap();
        rows.filter_map(|r| r.ok()).collect()
    }

    pub fn resolve_prediction(
        &self,
        id: i64,
        outcome: &str,
        brier: f64,
        correct: bool,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE predictions SET resolved_at=?2, outcome=?3, brier=?4, correct=?5
             WHERE id=?1 AND resolved_at IS NULL",
            params![id, now(), outcome, brier, correct as i64],
        )?;
        Ok(())
    }

    pub fn list_predictions(
        &self,
        domain: Option<&str>,
        status: Option<&str>,
        limit: i64,
    ) -> Vec<Prediction> {
        let conn = self.conn.lock().unwrap();
        let status_sql = match status {
            Some("open") => " AND resolved_at IS NULL",
            Some("resolved") => " AND resolved_at IS NOT NULL",
            _ => "",
        };
        let (sql, dom) = match domain {
            Some(d) => (
                format!(
                    "SELECT id,domain,subject,detail,probs,predicted_at,due_at,resolved_at,outcome,brier,correct
                     FROM predictions WHERE domain=?1{status_sql} ORDER BY id DESC LIMIT ?2"
                ),
                d.to_string(),
            ),
            None => (
                format!(
                    "SELECT id,domain,subject,detail,probs,predicted_at,due_at,resolved_at,outcome,brier,correct
                     FROM predictions WHERE 1=1{status_sql} ORDER BY id DESC LIMIT ?2"
                ),
                String::new(),
            ),
        };
        let mut stmt = conn.prepare(&sql).unwrap();
        let rows = if domain.is_some() {
            stmt.query_map(params![dom, limit], Prediction::from_row)
                .unwrap()
                .filter_map(|r| r.ok())
                .collect()
        } else {
            // The `1=1` placeholder keeps a single query shape; bind only limit.
            let sql2 = sql.replace("?2", "?1");
            drop(stmt);
            let mut stmt2 = conn.prepare(&sql2).unwrap();
            stmt2
                .query_map(params![limit], Prediction::from_row)
                .unwrap()
                .filter_map(|r| r.ok())
                .collect()
        };
        rows
    }

    /// Track record của một domain (nền tảng tri thức đánh giá): các dự đoán
    /// đã chấm — chủ đề, p đã cam kết cho outcome dự kiến, kết quả, Brier.
    pub fn track_record(&self, domain: &str, limit: i64) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT subject, probs, outcome, brier, correct FROM predictions
                 WHERE domain=?1 AND resolved_at IS NOT NULL ORDER BY resolved_at DESC LIMIT ?2",
            )
            .unwrap();
        let rows = stmt
            .query_map(params![domain, limit], |r| {
                let probs: Value =
                    serde_json::from_str(&r.get::<_, String>(1)?).unwrap_or(Value::Null);
                let p_committed = probs.as_object().and_then(|m| {
                    m.values()
                        .filter_map(|v| v.as_f64())
                        .fold(None, |acc: Option<f64>, v| {
                            Some(acc.map_or(v, |a| a.max(v)))
                        })
                });
                Ok(json!({
                    "subject": r.get::<_, String>(0)?,
                    "p_committed": p_committed,
                    "outcome": r.get::<_, Option<String>>(2)?,
                    "brier": r.get::<_, Option<f64>>(3)?,
                    "correct": r.get::<_, Option<i64>>(4)?.map(|v| v != 0),
                }))
            })
            .unwrap();
        rows.filter_map(|r| r.ok()).collect()
    }

    /// Per-domain calibration summary: total, open, resolved, hits, avg Brier.
    pub fn score_summary(&self) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT domain,
                        COUNT(*),
                        SUM(CASE WHEN resolved_at IS NULL THEN 1 ELSE 0 END),
                        SUM(CASE WHEN resolved_at IS NOT NULL THEN 1 ELSE 0 END),
                        SUM(CASE WHEN correct=1 THEN 1 ELSE 0 END),
                        AVG(CASE WHEN resolved_at IS NOT NULL THEN brier END)
                 FROM predictions GROUP BY domain ORDER BY domain",
            )
            .unwrap();
        let rows = stmt
            .query_map([], |r| {
                let resolved: i64 = r.get(3)?;
                let hits: i64 = r.get::<_, Option<i64>>(4)?.unwrap_or(0);
                Ok(json!({
                    "domain": r.get::<_, String>(0)?,
                    "total": r.get::<_, i64>(1)?,
                    "open": r.get::<_, i64>(2)?,
                    "resolved": resolved,
                    "hits": hits,
                    "accuracy": if resolved > 0 { Some((hits as f64 / resolved as f64 * 1000.0).round() / 1000.0) } else { None },
                    "avg_brier": r.get::<_, Option<f64>>(5)?.map(|b| (b * 1000.0).round() / 1000.0),
                }))
            })
            .unwrap();
        rows.filter_map(|r| r.ok()).collect()
    }

    /// Calibration buckets over resolved predictions: for each 10%-band of the
    /// forecast probability assigned to the *predicted* (argmax) outcome, how
    /// often that outcome actually happened.
    pub fn calibration_buckets(&self) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT probs, outcome, correct FROM predictions WHERE resolved_at IS NOT NULL",
            )
            .unwrap();
        let rows: Vec<(String, String, i64)> = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                ))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        let mut buckets = vec![(0usize, 0usize); 10]; // (n, hits)
        for (probs, _outcome, correct) in rows {
            let Some(map) = serde_json::from_str::<Value>(&probs)
                .ok()
                .and_then(|v| v.as_object().cloned())
            else {
                continue;
            };
            let p_max = map
                .values()
                .filter_map(|v| v.as_f64())
                .fold(0.0f64, f64::max);
            if p_max <= 0.0 {
                continue;
            }
            let idx = ((p_max * 10.0).floor() as usize).min(9);
            buckets[idx].0 += 1;
            if correct == 1 {
                buckets[idx].1 += 1;
            }
        }
        buckets
            .iter()
            .enumerate()
            .filter(|(_, (n, _))| *n > 0)
            .map(|(i, (n, h))| {
                json!({
                    "band": format!("{}–{}%", i * 10, (i + 1) * 10),
                    "n": n,
                    "hit_rate": ((*h as f64 / *n as f64) * 1000.0).round() / 1000.0,
                })
            })
            .collect()
    }

    // ---- generic topics ----

    pub fn create_topic_src(
        &self,
        name: &str,
        description: &str,
        fields_json: &Value,
        source: &Value,
    ) -> Result<i64> {
        self.create_topic_full(name, description, fields_json, source, &json!({}), "")
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_topic_full(
        &self,
        name: &str,
        description: &str,
        fields_json: &Value,
        source: &Value,
        static_map: &Value,
        guide: &str,
    ) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO topics(name,description,fields_json,source_json,static_json,guide,created_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                name.trim(), description.trim(), fields_json.to_string(), source.to_string(),
                static_map.to_string(), guide.trim(), now()
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Cấu hình TĨNH + tài liệu hướng dẫn của chủ đề: `(static_map, guide)`.
    pub fn topic_context(&self, topic_id: i64) -> (Value, String) {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT static_json, guide FROM topics WHERE id=?1",
            params![topic_id],
            |r| {
                Ok((
                    serde_json::from_str::<Value>(&r.get::<_, String>(0)?)
                        .unwrap_or_else(|_| json!({})),
                    r.get::<_, String>(1)?,
                ))
            },
        )
        .optional()
        .ok()
        .flatten()
        .unwrap_or_else(|| (json!({}), String::new()))
    }

    /// Sửa cấu hình tĩnh / tài liệu hướng dẫn (None = giữ nguyên).
    pub fn set_topic_context(
        &self,
        topic_id: i64,
        static_map: Option<&Value>,
        guide: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        if let Some(m) = static_map {
            conn.execute(
                "UPDATE topics SET static_json=?2 WHERE id=?1",
                params![topic_id, m.to_string()],
            )?;
        }
        if let Some(g) = guide {
            conn.execute(
                "UPDATE topics SET guide=?2 WHERE id=?1",
                params![topic_id, g.trim()],
            )?;
        }
        Ok(())
    }

    /// Đặt/ghi đè một khoá cấu hình tĩnh (dùng khi connector đổi nguồn).
    pub fn set_topic_static_key(&self, topic_id: i64, key: &str, value: &str) -> Result<()> {
        let (mut m, _) = self.topic_context(topic_id);
        if let Some(obj) = m.as_object_mut() {
            obj.insert(key.to_string(), json!(value));
        }
        self.set_topic_context(topic_id, Some(&m), None)
    }

    pub fn topic_source(&self, topic_id: i64) -> Value {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT source_json FROM topics WHERE id=?1",
            params![topic_id],
            |r| {
                Ok(serde_json::from_str::<Value>(&r.get::<_, String>(0)?)
                    .unwrap_or(json!({ "kind": "manual" })))
            },
        )
        .optional()
        .ok()
        .flatten()
        .unwrap_or(json!({ "kind": "manual" }))
    }

    /// Topics whose source kind is a connector (not manual): (id, name, source).
    pub fn connector_topics(&self) -> Vec<(i64, String, Value)> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id,name,source_json FROM topics")
            .unwrap();
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    serde_json::from_str::<Value>(&r.get::<_, String>(2)?).unwrap_or(json!({})),
                ))
            })
            .unwrap();
        rows.filter_map(|r| r.ok())
            .filter(|(_, _, s)| {
                let kind = s["kind"].as_str().unwrap_or("manual");
                kind != "manual" && !kind.is_empty()
            })
            .collect()
    }

    /// (id, name, description, fields, record_count, rule_count)
    pub fn list_topics(&self) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT t.id, t.name, t.description, t.fields_json, t.source_json,
                        (SELECT COUNT(*) FROM topic_records r WHERE r.topic_id=t.id),
                        (SELECT COUNT(*) FROM topic_rules u WHERE u.topic_id=t.id),
                        (SELECT COUNT(*) FROM topic_docs d WHERE d.topic_id=t.id)
                 FROM topics t ORDER BY t.id",
            )
            .unwrap();
        let rows = stmt
            .query_map([], |r| {
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "name": r.get::<_, String>(1)?,
                    "description": r.get::<_, String>(2)?,
                    "fields": serde_json::from_str::<Value>(&r.get::<_, String>(3)?).unwrap_or(json!([])),
                    "source": serde_json::from_str::<Value>(&r.get::<_, String>(4)?).unwrap_or(json!({ "kind": "manual" })),
                    "records": r.get::<_, i64>(5)?,
                    "rules": r.get::<_, i64>(6)?,
                    "docs": r.get::<_, i64>(7)?,
                }))
            })
            .unwrap();
        rows.filter_map(|r| r.ok()).collect()
    }

    /// Resolve a topic by numeric id or (case-insensitive) name.
    pub fn find_topic(&self, key: &str) -> Option<(i64, String, String, Value)> {
        let conn = self.conn.lock().unwrap();
        // INTEGER affinity converts a numeric string for the id comparison.
        let sql = if key.trim().parse::<i64>().is_ok() {
            "SELECT id,name,description,fields_json FROM topics WHERE id=?1"
        } else {
            "SELECT id,name,description,fields_json FROM topics WHERE lower(name)=lower(?1)"
        };
        conn.query_row(sql, params![key.trim()], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                serde_json::from_str::<Value>(&r.get::<_, String>(3)?).unwrap_or(json!([])),
            ))
        })
        .optional()
        .ok()
        .flatten()
    }

    pub fn set_topic_source(&self, id: i64, source: &Value) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE topics SET source_json=?2 WHERE id=?1",
            params![id, source.to_string()],
        )?;
        Ok(())
    }

    /// Sửa chủ đề: đổi tên / mô tả / schema trường (None = giữ nguyên).
    /// Bản ghi cũ giữ nguyên — trường bị xoá khỏi schema chỉ thôi hiển thị.
    pub fn update_topic(
        &self,
        id: i64,
        name: Option<&str>,
        description: Option<&str>,
        fields_json: Option<&Value>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        if let Some(n) = name.map(str::trim).filter(|n| !n.is_empty()) {
            conn.execute("UPDATE topics SET name=?2 WHERE id=?1", params![id, n])?;
        }
        if let Some(d) = description {
            conn.execute(
                "UPDATE topics SET description=?2 WHERE id=?1",
                params![id, d.trim()],
            )?;
        }
        if let Some(f) = fields_json {
            conn.execute(
                "UPDATE topics SET fields_json=?2 WHERE id=?1",
                params![id, f.to_string()],
            )?;
        }
        Ok(())
    }

    /// Đổi domain của các dự đoán cũ khi chủ đề đổi tên (sổ điểm không đứt gãy).
    pub fn rename_prediction_domain(&self, from: &str, to: &str) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.execute(
            "UPDATE predictions SET domain=?2 WHERE domain=?1",
            params![from, to],
        )?)
    }

    pub fn delete_topic(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM topic_records WHERE topic_id=?1", params![id])?;
        conn.execute("DELETE FROM topic_docs WHERE topic_id=?1", params![id])?;
        conn.execute("DELETE FROM topic_rules WHERE topic_id=?1", params![id])?;
        conn.execute("DELETE FROM topics WHERE id=?1", params![id])?;
        Ok(())
    }

    pub fn add_topic_record(&self, topic_id: i64, data: &Value, note: &str) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO topic_records(topic_id,data,note,created_at) VALUES(?1,?2,?3,?4)",
            params![topic_id, data.to_string(), note, now()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Newest-first records; `q` filters by substring over the JSON blob + note.
    pub fn search_topic_records(&self, topic_id: i64, q: &str, limit: i64) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, data, note, created_at FROM topic_records
                 WHERE topic_id=?1 AND (?2='' OR data LIKE '%'||?2||'%' OR note LIKE '%'||?2||'%')
                 ORDER BY id DESC LIMIT ?3",
            )
            .unwrap();
        let rows = stmt
            .query_map(params![topic_id, q.trim(), limit], |r| {
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "data": serde_json::from_str::<Value>(&r.get::<_, String>(1)?).unwrap_or(Value::Null),
                    "note": r.get::<_, String>(2)?,
                    "created_at": r.get::<_, i64>(3)?,
                }))
            })
            .unwrap();
        rows.filter_map(|r| r.ok()).collect()
    }

    pub fn delete_topic_record(&self, topic_id: i64, record_id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM topic_records WHERE topic_id=?1 AND id=?2",
            params![topic_id, record_id],
        )?;
        Ok(())
    }

    // ---- tài liệu / thông tin ngoài số liệu ----

    pub fn add_topic_doc(
        &self,
        topic_id: i64,
        title: &str,
        content: &str,
        date: &str,
        r#ref: &str,
    ) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO topic_docs(topic_id,title,content,date,ref,created_at) VALUES(?1,?2,?3,?4,?5,?6)",
            params![topic_id, title.trim(), content.trim(), date.trim(), r#ref.trim(), now()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Tài liệu của chủ đề, mới nhất trước. `q` lọc theo tiêu đề/nội dung/ngày/ref.
    pub fn list_topic_docs(&self, topic_id: i64, q: &str, limit: i64) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id,title,content,date,ref,created_at FROM topic_docs
                 WHERE topic_id=?1 AND (?2='' OR title LIKE '%'||?2||'%' OR content LIKE '%'||?2||'%'
                                        OR date LIKE '%'||?2||'%' OR ref LIKE '%'||?2||'%')
                 ORDER BY (date='') ASC, date DESC, id DESC LIMIT ?3",
            )
            .unwrap();
        let rows = stmt
            .query_map(params![topic_id, q.trim(), limit], |r| {
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "title": r.get::<_, String>(1)?,
                    "content": r.get::<_, String>(2)?,
                    "date": r.get::<_, String>(3)?,
                    "ref": r.get::<_, String>(4)?,
                    "created_at": r.get::<_, i64>(5)?,
                }))
            })
            .unwrap();
        rows.filter_map(|r| r.ok()).collect()
    }

    pub fn delete_topic_doc(&self, topic_id: i64, doc_id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM topic_docs WHERE topic_id=?1 AND id=?2",
            params![topic_id, doc_id],
        )?;
        Ok(())
    }

    pub fn topic_docs_count(&self, topic_id: i64) -> i64 {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM topic_docs WHERE topic_id=?1",
            params![topic_id],
            |r| r.get(0),
        )
        .unwrap_or(0)
    }

    pub fn add_topic_rule(
        &self,
        topic_id: i64,
        rule: &str,
        confidence: f64,
        source: &str,
    ) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO topic_rules(topic_id,rule,confidence,source,created_at) VALUES(?1,?2,?3,?4,?5)",
            params![topic_id, rule.trim(), confidence.clamp(0.0, 1.0), source, now()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_topic_rules(&self, topic_id: i64) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, rule, confidence, source, created_at FROM topic_rules WHERE topic_id=?1 ORDER BY confidence DESC, id")
            .unwrap();
        let rows = stmt
            .query_map(params![topic_id], |r| {
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "rule": r.get::<_, String>(1)?,
                    "confidence": r.get::<_, f64>(2)?,
                    "source": r.get::<_, String>(3)?,
                    "created_at": r.get::<_, i64>(4)?,
                }))
            })
            .unwrap();
        rows.filter_map(|r| r.ok()).collect()
    }

    /// Drop AI-derived rules before a re-derive (user rules survive).
    pub fn clear_ai_rules(&self, topic_id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM topic_rules WHERE topic_id=?1 AND source='ai'",
            params![topic_id],
        )?;
        Ok(())
    }

    pub fn delete_topic_rule(&self, topic_id: i64, rule_id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM topic_rules WHERE topic_id=?1 AND id=?2",
            params![topic_id, rule_id],
        )?;
        Ok(())
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

fn row_to_draw(r: &rusqlite::Row) -> rusqlite::Result<(String, Vec<i64>, Vec<u8>)> {
    let date: String = r.get(0)?;
    let numbers: Vec<i64> = serde_json::from_str(&r.get::<_, String>(1)?).unwrap_or_default();
    let loto: Vec<u8> = serde_json::from_str(&r.get::<_, String>(2)?).unwrap_or_default();
    Ok((date, numbers, loto))
}

#[derive(Debug, Clone, Serialize)]
pub struct Fixture {
    pub event_id: String,
    pub league_id: String,
    pub league_name: String,
    pub home: String,
    pub away: String,
    pub kickoff_ts: i64,
    pub home_score: Option<i64>,
    pub away_score: Option<i64>,
    pub status: String,
}

impl Fixture {
    fn from_row(r: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Self {
            event_id: r.get(0)?,
            league_id: r.get(1)?,
            league_name: r.get(2)?,
            home: r.get(3)?,
            away: r.get(4)?,
            kickoff_ts: r.get(5)?,
            home_score: r.get(6)?,
            away_score: r.get(7)?,
            status: r.get(8)?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct PredictionInput {
    pub domain: String,
    pub subject: String,
    pub detail: Value,
    pub probs: Value,
    pub due_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Prediction {
    pub id: i64,
    pub domain: String,
    pub subject: String,
    pub detail: Value,
    pub probs: Value,
    pub predicted_at: i64,
    pub due_at: i64,
    pub resolved_at: Option<i64>,
    pub outcome: Option<String>,
    pub brier: Option<f64>,
    pub correct: Option<bool>,
}

impl Prediction {
    fn from_row(r: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Self {
            id: r.get(0)?,
            domain: r.get(1)?,
            subject: r.get(2)?,
            detail: serde_json::from_str(&r.get::<_, String>(3)?).unwrap_or(Value::Null),
            probs: serde_json::from_str(&r.get::<_, String>(4)?).unwrap_or(Value::Null),
            predicted_at: r.get(5)?,
            due_at: r.get(6)?,
            resolved_at: r.get(7)?,
            outcome: r.get(8)?,
            brier: r.get(9)?,
            correct: r.get::<_, Option<i64>>(10)?.map(|v| v != 0),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prediction_lifecycle_and_summary() {
        let db = Db::open_memory().unwrap();
        let id = db
            .add_prediction(&PredictionInput {
                domain: "football".into(),
                subject: "Arsenal vs Chelsea".into(),
                detail: json!({ "event_id": "E1" }),
                probs: json!({ "H": 0.6, "D": 0.25, "A": 0.15 }),
                due_at: 100,
            })
            .unwrap();
        assert!(db.has_open_prediction("football", "event_id", "E1"));
        assert!(!db.has_open_prediction("football", "event_id", "E2"));
        assert_eq!(db.unresolved_due(i64::MAX).len(), 1);

        db.resolve_prediction(id, "H", 0.245, true).unwrap();
        assert!(db.unresolved_due(i64::MAX).is_empty());
        assert!(!db.has_open_prediction("football", "event_id", "E1"));
        let p = db.get_prediction(id).unwrap();
        assert_eq!(p.outcome.as_deref(), Some("H"));
        assert_eq!(p.correct, Some(true));

        let summary = db.score_summary();
        assert_eq!(summary.len(), 1);
        assert_eq!(summary[0]["resolved"], 1);
        assert_eq!(summary[0]["accuracy"], 1.0);
        let cal = db.calibration_buckets();
        assert_eq!(cal.len(), 1);
        assert_eq!(cal[0]["band"], "60–70%");
    }

    #[test]
    fn list_predictions_filters() {
        let db = Db::open_memory().unwrap();
        for d in ["football", "lottery"] {
            db.add_prediction(&PredictionInput {
                domain: d.into(),
                subject: format!("s-{d}"),
                detail: json!({}),
                probs: json!({ "x": 0.5 }),
                due_at: 1,
            })
            .unwrap();
        }
        assert_eq!(db.list_predictions(None, None, 10).len(), 2);
        assert_eq!(db.list_predictions(Some("football"), None, 10).len(), 1);
        assert_eq!(db.list_predictions(None, Some("open"), 10).len(), 2);
        assert_eq!(db.list_predictions(None, Some("resolved"), 10).len(), 0);
    }

    #[test]
    fn draw_roundtrip() {
        let db = Db::open_memory().unwrap();
        let numbers: Vec<i64> = (0..27).collect();
        let loto: Vec<u8> = numbers.iter().map(|n| (n % 100) as u8).collect();
        db.upsert_draw("2026-07-27", &numbers, &loto).unwrap();
        let (date, nums, l) = db.latest_draw().unwrap();
        assert_eq!(date, "2026-07-27");
        assert_eq!(nums.len(), 27);
        assert_eq!(l.len(), 27);
    }

    #[test]
    fn topic_crud_and_search() {
        let db = Db::open_memory().unwrap();
        let fields =
            json!([{ "name": "ngày", "kind": "date" }, { "name": "giá", "kind": "number" }]);
        let tid = db
            .create_topic_src(
                "Giá cafe",
                "theo dõi giá cafe",
                &fields,
                &json!({ "kind": "manual" }),
            )
            .unwrap();
        // Duplicate name rejected.
        assert!(db
            .create_topic_src("Giá cafe", "", &fields, &json!({ "kind": "manual" }))
            .is_err());
        // Lookup by name (case-insensitive) and by id string.
        assert_eq!(db.find_topic("giá cafe").unwrap().0, tid);
        assert_eq!(db.find_topic(&tid.to_string()).unwrap().0, tid);
        assert!(db.find_topic("khác").is_none());

        db.add_topic_record(tid, &json!({ "ngày": "2026-07-26", "giá": 100 }), "")
            .unwrap();
        db.add_topic_record(
            tid,
            &json!({ "ngày": "2026-07-27", "giá": 105 }),
            "tăng mạnh",
        )
        .unwrap();
        assert_eq!(db.search_topic_records(tid, "", 10).len(), 2);
        assert_eq!(db.search_topic_records(tid, "tăng mạnh", 10).len(), 1);
        assert_eq!(db.search_topic_records(tid, "105", 10).len(), 1);

        db.add_topic_rule(tid, "giá tăng vào cuối tuần", 0.7, "ai")
            .unwrap();
        db.add_topic_rule(tid, "user rule", 0.9, "user").unwrap();
        assert_eq!(db.list_topic_rules(tid).len(), 2);
        db.clear_ai_rules(tid).unwrap();
        let rules = db.list_topic_rules(tid);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0]["source"], "user");

        let listed = db.list_topics();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0]["records"], 2);
        assert_eq!(listed[0]["rules"], 1);

        // Tài liệu ngoài số liệu.
        db.add_topic_doc(
            tid,
            "Tin sương muối",
            "Đợt lạnh về Đà Lạt cuối tuần",
            "2026-07-27",
            "nhiệt độ",
        )
        .unwrap();
        db.add_topic_doc(tid, "Ghi chú chung", "Vườn mới xuống giống", "", "")
            .unwrap();
        assert_eq!(db.topic_docs_count(tid), 2);
        let docs = db.list_topic_docs(tid, "", 10);
        assert_eq!(docs.len(), 2);
        assert_eq!(docs[0]["date"], "2026-07-27"); // có ngày xếp trước
        assert_eq!(db.list_topic_docs(tid, "sương muối", 10).len(), 1);
        assert_eq!(db.list_topic_docs(tid, "2026-07-27", 10).len(), 1);
        db.delete_topic_doc(tid, docs[1]["id"].as_i64().unwrap())
            .unwrap();
        assert_eq!(db.topic_docs_count(tid), 1);

        db.delete_topic(tid).unwrap();
        assert!(db.list_topics().is_empty());
        assert!(db.search_topic_records(tid, "", 10).is_empty());
        assert_eq!(db.topic_docs_count(tid), 0);
    }

    #[test]
    fn price_series_order() {
        let db = Db::open_memory().unwrap();
        db.add_price("XAU_USD", 4000.0).unwrap();
        db.add_price("XAU_USD", 4100.0).unwrap();
        let (_, latest) = db.latest_price("XAU_USD").unwrap();
        assert_eq!(latest, 4100.0);
        assert_eq!(db.price_series("XAU_USD", 0).len(), 2);
    }
}
