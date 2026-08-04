//! Local SQLite store for the TikTok Downloader app. Everything lives on this
//! machine — the only outbound traffic is the resolver API + the TikTok CDN
//! the files come from. Tables:
//!   * `downloads`     — one row per download job, from `queued` through
//!                       `done`/`error`/`canceled`. The queue IS this table
//!                       (status='queued'), so pending jobs survive restarts.
//!   * `downloads_fts` — FTS5 mirror (title/author/url,
//!                       unicode61 remove_diacritics 2 → tìm không dấu được)
//!   * `settings`      — key/value (thư mục lưu, chất lượng mặc định…)
//!   * `activity`      — nhật ký gọn các sự kiện chính

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Db {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS downloads (
  id             INTEGER PRIMARY KEY AUTOINCREMENT,
  input_url      TEXT NOT NULL,
  video_id       TEXT NOT NULL DEFAULT '',
  -- what the post turned out to be after resolve: video|images|audio|avatar
  kind           TEXT NOT NULL DEFAULT '',
  -- what the user asked for: nowm|hd|wm|audio|avatar
  quality        TEXT NOT NULL DEFAULT 'nowm',
  title          TEXT NOT NULL DEFAULT '',
  author_id      TEXT NOT NULL DEFAULT '',
  author_name    TEXT NOT NULL DEFAULT '',
  cover_url      TEXT NOT NULL DEFAULT '',
  duration       INTEGER NOT NULL DEFAULT 0,
  files          TEXT NOT NULL DEFAULT '[]',
  dir            TEXT NOT NULL DEFAULT '',
  total_bytes    INTEGER NOT NULL DEFAULT 0,
  progress_bytes INTEGER NOT NULL DEFAULT 0,
  status         TEXT NOT NULL DEFAULT 'queued',
  error          TEXT NOT NULL DEFAULT '',
  stats          TEXT NOT NULL DEFAULT '{}',
  music_title    TEXT NOT NULL DEFAULT '',
  created_at     INTEGER NOT NULL,
  started_at     INTEGER NOT NULL DEFAULT 0,
  finished_at    INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_downloads_status  ON downloads(status);
CREATE INDEX IF NOT EXISTS idx_downloads_created ON downloads(created_at DESC);
CREATE VIRTUAL TABLE IF NOT EXISTS downloads_fts USING fts5(
  download_id UNINDEXED,
  title,
  author,
  url,
  tokenize='unicode61 remove_diacritics 2'
);
CREATE TABLE IF NOT EXISTS settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS activity (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  kind       TEXT NOT NULL,
  message    TEXT NOT NULL,
  ref_id     TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL
);
"#;

pub fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn iso(ts: i64) -> String {
    if ts <= 0 {
        return String::new();
    }
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_default()
}

/// Settings the app ships with. `download_dir` is resolved lazily so a moved
/// HOME (tests, packaged runs) never bakes a stale absolute path into the DB.
pub fn default_settings() -> Vec<(&'static str, String)> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    vec![
        (
            "download_dir",
            format!("{home}/Downloads/TikTok"),
        ),
        ("default_quality", "nowm".into()),
        ("filename_template", "{author}_{id}".into()),
        ("max_concurrent", "2".into()),
        // Post ảnh: tải kèm file nhạc nền hay không.
        ("photo_audio", "1".into()),
        // Ghi <tên file>.json chứa metadata (caption, stats…) cạnh file tải về.
        ("save_meta_json", "0".into()),
        // Số video tối đa khi tải cả trang cá nhân.
        ("profile_max", "30".into()),
    ]
}

impl Db {
    pub fn open(path: PathBuf) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(SCHEMA)?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.reset_stale_active();
        Ok(db)
    }

    #[cfg(test)]
    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn open_default() -> Result<Self> {
        let dir = data_dir();
        std::fs::create_dir_all(&dir).ok();
        Self::open(dir.join("tiktok-dl.db"))
    }

    /// Jobs left `resolving`/`downloading` by a previous process (crash, quit
    /// mid-download) go back to `queued` so the worker picks them up again —
    /// their `.part` files are overwritten on the retry.
    fn reset_stale_active(&self) {
        let c = self.conn.lock().unwrap();
        let _ = c.execute(
            "UPDATE downloads SET status='queued', progress_bytes=0
             WHERE status IN ('resolving','downloading')",
            [],
        );
    }

    // ---- settings ----

    pub fn setting(&self, key: &str, fallback: &str) -> String {
        let c = self.conn.lock().unwrap();
        c.query_row(
            "SELECT value FROM settings WHERE key=?1",
            params![key],
            |r| r.get::<_, String>(0),
        )
        .optional()
        .ok()
        .flatten()
        .unwrap_or_else(|| {
            default_settings()
                .into_iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v)
                .unwrap_or_else(|| fallback.to_string())
        })
    }

    pub fn set_setting(&self, key: &str, value: &str) {
        let c = self.conn.lock().unwrap();
        let _ = c.execute(
            "INSERT INTO settings(key,value) VALUES(?1,?2)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        );
    }

    pub fn all_settings(&self) -> Value {
        let mut out = serde_json::Map::new();
        for (k, v) in default_settings() {
            out.insert(k.to_string(), json!(v));
        }
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT key, value FROM settings").unwrap();
        let rows = st
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .unwrap();
        for row in rows.flatten() {
            out.insert(row.0, json!(row.1));
        }
        Value::Object(out)
    }

    // ---- downloads / queue ----

    /// Insert a new job in `queued` state. `meta` is an optional resolved
    /// snapshot (title/author/cover…) so the queue UI has something to show
    /// before the worker re-resolves; the worker always refreshes it because
    /// CDN links expire within minutes.
    pub fn enqueue(&self, input_url: &str, quality: &str, meta: Option<&Value>) -> Result<i64> {
        let c = self.conn.lock().unwrap();
        let m = meta.cloned().unwrap_or(json!({}));
        let s = |k: &str| m[k].as_str().unwrap_or("").to_string();
        c.execute(
            "INSERT INTO downloads(input_url, video_id, kind, quality, title, author_id,
                                   author_name, cover_url, duration, stats, music_title, created_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![
                input_url,
                s("video_id"),
                s("kind"),
                quality,
                s("title"),
                s("author_id"),
                s("author_name"),
                s("cover_url"),
                m["duration"].as_i64().unwrap_or(0),
                m["stats"].to_string(),
                s("music_title"),
                now_ts()
            ],
        )?;
        let id = c.last_insert_rowid();
        c.execute(
            "INSERT INTO downloads_fts(download_id, title, author, url) VALUES(?1,?2,?3,?4)",
            params![id, fold_vi(&s("title")), fold_vi(&s("author_name")), input_url],
        )?;
        Ok(id)
    }

    /// A `done` row with the same input URL and quality — used to skip
    /// accidental double-downloads unless the caller forces.
    pub fn find_done_duplicate(&self, input_url: &str, quality: &str) -> Option<i64> {
        let c = self.conn.lock().unwrap();
        c.query_row(
            "SELECT id FROM downloads WHERE input_url=?1 AND quality=?2 AND status='done'
             ORDER BY id DESC LIMIT 1",
            params![input_url, quality],
            |r| r.get(0),
        )
        .optional()
        .ok()
        .flatten()
    }

    /// Atomically claim the oldest queued job for a worker (single connection
    /// behind a Mutex → SELECT+UPDATE cannot interleave with another claim).
    pub fn claim_next_queued(&self) -> Option<i64> {
        let c = self.conn.lock().unwrap();
        let id: Option<i64> = c
            .query_row(
                "SELECT id FROM downloads WHERE status='queued' ORDER BY id LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()
            .ok()
            .flatten();
        let id = id?;
        c.execute(
            "UPDATE downloads SET status='resolving', started_at=?2, error='' WHERE id=?1",
            params![id, now_ts()],
        )
        .ok()?;
        Some(id)
    }

    /// Write the resolved metadata onto the row and refresh the FTS mirror.
    pub fn apply_resolved(&self, id: i64, meta: &Value) {
        let s = |k: &str| meta[k].as_str().unwrap_or("").to_string();
        let c = self.conn.lock().unwrap();
        let _ = c.execute(
            "UPDATE downloads SET video_id=?2, kind=?3, title=?4, author_id=?5,
                    author_name=?6, cover_url=?7, duration=?8, stats=?9, music_title=?10
             WHERE id=?1",
            params![
                id,
                s("video_id"),
                s("kind"),
                s("title"),
                s("author_id"),
                s("author_name"),
                s("cover_url"),
                meta["duration"].as_i64().unwrap_or(0),
                meta["stats"].to_string(),
                s("music_title"),
            ],
        );
        let _ = c.execute("DELETE FROM downloads_fts WHERE download_id=?1", params![id]);
        let url: String = c
            .query_row(
                "SELECT input_url FROM downloads WHERE id=?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap_or_default();
        let _ = c.execute(
            "INSERT INTO downloads_fts(download_id, title, author, url) VALUES(?1,?2,?3,?4)",
            params![id, fold_vi(&s("title")), fold_vi(&s("author_name")), url],
        );
    }

    /// Job kind can differ from what the resolver saw (quality=audio on a
    /// video post → job kind "audio"). Kind is not FTS-indexed → plain update.
    pub fn set_kind(&self, id: i64, kind: &str) {
        let c = self.conn.lock().unwrap();
        let _ = c.execute(
            "UPDATE downloads SET kind=?2 WHERE id=?1",
            params![id, kind],
        );
    }

    pub fn set_status(&self, id: i64, status: &str, error: &str) {
        let c = self.conn.lock().unwrap();
        let finished = matches!(status, "done" | "error" | "canceled");
        let _ = c.execute(
            "UPDATE downloads SET status=?2, error=?3,
                    finished_at = CASE WHEN ?4 THEN ?5 ELSE finished_at END
             WHERE id=?1",
            params![id, status, error, finished, now_ts()],
        );
    }

    pub fn set_progress(&self, id: i64, progress: i64, total: i64) {
        let c = self.conn.lock().unwrap();
        let _ = c.execute(
            "UPDATE downloads SET progress_bytes=?2, total_bytes=?3 WHERE id=?1",
            params![id, progress, total],
        );
    }

    pub fn finish_files(&self, id: i64, dir: &str, files: &[String], total: i64) {
        let c = self.conn.lock().unwrap();
        let _ = c.execute(
            "UPDATE downloads SET dir=?2, files=?3, total_bytes=?4, progress_bytes=?4,
                    status='done', error='', finished_at=?5
             WHERE id=?1",
            params![id, dir, json!(files).to_string(), total, now_ts()],
        );
    }

    /// True when the job is queued/resolving/downloading — states a cancel
    /// request can still act on.
    pub fn is_active(&self, id: i64) -> bool {
        let c = self.conn.lock().unwrap();
        c.query_row(
            "SELECT 1 FROM downloads WHERE id=?1 AND status IN ('queued','resolving','downloading')",
            params![id],
            |_| Ok(()),
        )
        .optional()
        .ok()
        .flatten()
        .is_some()
    }

    pub fn requeue(&self, id: i64) -> bool {
        let c = self.conn.lock().unwrap();
        c.execute(
            "UPDATE downloads SET status='queued', error='', progress_bytes=0, total_bytes=0,
                    started_at=0, finished_at=0
             WHERE id=?1 AND status IN ('error','canceled','done')",
            params![id],
        )
        .map(|n| n > 0)
        .unwrap_or(false)
    }

    pub fn get_download(&self, id: i64) -> Option<Value> {
        let c = self.conn.lock().unwrap();
        c.query_row(
            &format!("SELECT {ROW_COLS} FROM downloads d WHERE d.id=?1"),
            params![id],
            row_to_value,
        )
        .optional()
        .ok()
        .flatten()
    }

    /// History + queue listing. `q` goes through FTS (diacritics-insensitive);
    /// the other filters are plain WHERE clauses.
    pub fn list_downloads(
        &self,
        q: Option<&str>,
        status: Option<&str>,
        kind: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Vec<Value> {
        let c = self.conn.lock().unwrap();
        let limit = limit.clamp(1, 500);
        let mut sql = format!("SELECT {ROW_COLS} FROM downloads d");
        let mut wheres: Vec<String> = Vec::new();
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(q) = q.map(str::trim).filter(|s| !s.is_empty()) {
            sql.push_str(" JOIN downloads_fts f ON f.download_id = d.id");
            wheres.push("downloads_fts MATCH ?".into());
            args.push(Box::new(fts_query(q)));
        }
        if let Some(st) = status.map(str::trim).filter(|s| !s.is_empty()) {
            if st == "active" {
                wheres.push("d.status IN ('queued','resolving','downloading')".into());
            } else {
                wheres.push("d.status = ?".into());
                args.push(Box::new(st.to_string()));
            }
        }
        if let Some(k) = kind.map(str::trim).filter(|s| !s.is_empty()) {
            wheres.push("d.kind = ?".into());
            args.push(Box::new(k.to_string()));
        }
        if !wheres.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&wheres.join(" AND "));
        }
        sql.push_str(" ORDER BY d.id DESC LIMIT ? OFFSET ?");
        args.push(Box::new(limit));
        args.push(Box::new(offset.max(0)));
        let mut st = match c.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        let refs: Vec<&dyn rusqlite::ToSql> = args.iter().map(|b| b.as_ref()).collect();
        st.query_map(refs.as_slice(), row_to_value)
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
    }

    pub fn delete_download(&self, id: i64) -> Option<Value> {
        let row = self.get_download(id)?;
        let c = self.conn.lock().unwrap();
        let _ = c.execute("DELETE FROM downloads WHERE id=?1", params![id]);
        let _ = c.execute("DELETE FROM downloads_fts WHERE download_id=?1", params![id]);
        Some(row)
    }

    /// Bulk-remove finished records (never touches active ones). Returns the
    /// removed rows so the caller can also delete files when asked to.
    pub fn clear_downloads(&self, status: Option<&str>) -> Vec<Value> {
        let cond = match status {
            Some("done") => "status='done'",
            Some("error") => "status='error'",
            Some("canceled") => "status='canceled'",
            _ => "status IN ('done','error','canceled')",
        };
        let rows = {
            let c = self.conn.lock().unwrap();
            let mut st = c
                .prepare(&format!("SELECT {ROW_COLS} FROM downloads d WHERE {cond}"))
                .unwrap();
            let rows: Vec<Value> = st
                .query_map([], row_to_value)
                .map(|r| r.flatten().collect())
                .unwrap_or_default();
            let _ = c.execute(
                &format!(
                    "DELETE FROM downloads_fts WHERE download_id IN
                     (SELECT id FROM downloads WHERE {cond})"
                ),
                [],
            );
            let _ = c.execute(&format!("DELETE FROM downloads WHERE {cond}"), []);
            rows
        };
        rows
    }

    /// Counters for the status bar: `{active, queued, done, error, total, bytes_done}`.
    pub fn counters(&self) -> Value {
        let c = self.conn.lock().unwrap();
        let count = |cond: &str| -> i64 {
            c.query_row(
                &format!("SELECT COUNT(*) FROM downloads WHERE {cond}"),
                [],
                |r| r.get(0),
            )
            .unwrap_or(0)
        };
        let bytes: i64 = c
            .query_row(
                "SELECT COALESCE(SUM(total_bytes),0) FROM downloads WHERE status='done'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        json!({
            "active": count("status IN ('resolving','downloading')"),
            "queued": count("status='queued'"),
            "done": count("status='done'"),
            "error": count("status='error'"),
            "total": count("1=1"),
            "bytes_done": bytes,
        })
    }

    // ---- activity ----

    pub fn log(&self, kind: &str, message: &str, ref_id: &str) {
        let c = self.conn.lock().unwrap();
        let _ = c.execute(
            "INSERT INTO activity(kind, message, ref_id, created_at) VALUES(?1,?2,?3,?4)",
            params![kind, message, ref_id, now_ts()],
        );
        // Keep the log bounded — it is a convenience trail, not an audit log.
        let _ = c.execute(
            "DELETE FROM activity WHERE id NOT IN (SELECT id FROM activity ORDER BY id DESC LIMIT 500)",
            [],
        );
    }

    pub fn recent_activity(&self, limit: i64) -> Vec<Value> {
        let c = self.conn.lock().unwrap();
        let mut st = c
            .prepare("SELECT kind, message, ref_id, created_at FROM activity ORDER BY id DESC LIMIT ?1")
            .unwrap();
        st.query_map(params![limit.clamp(1, 200)], |r| {
            Ok(json!({
                "kind": r.get::<_, String>(0)?,
                "message": r.get::<_, String>(1)?,
                "ref_id": r.get::<_, String>(2)?,
                "at": iso(r.get::<_, i64>(3)?),
            }))
        })
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
    }
}

pub fn data_dir() -> PathBuf {
    std::env::var("SENCLAW_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(home)
                .join(".senclaw")
                .join("apps")
                .join("tiktok-dl")
        })
}

const ROW_COLS: &str = "d.id, d.input_url, d.video_id, d.kind, d.quality, d.title, d.author_id,
    d.author_name, d.cover_url, d.duration, d.files, d.dir, d.total_bytes, d.progress_bytes,
    d.status, d.error, d.stats, d.music_title, d.created_at, d.started_at, d.finished_at";

fn row_to_value(r: &rusqlite::Row) -> rusqlite::Result<Value> {
    let files: String = r.get(10)?;
    let stats: String = r.get(16)?;
    Ok(json!({
        "id": r.get::<_, i64>(0)?,
        "input_url": r.get::<_, String>(1)?,
        "video_id": r.get::<_, String>(2)?,
        "kind": r.get::<_, String>(3)?,
        "quality": r.get::<_, String>(4)?,
        "title": r.get::<_, String>(5)?,
        "author_id": r.get::<_, String>(6)?,
        "author_name": r.get::<_, String>(7)?,
        "cover_url": r.get::<_, String>(8)?,
        "duration": r.get::<_, i64>(9)?,
        "files": serde_json::from_str::<Value>(&files).unwrap_or(json!([])),
        "dir": r.get::<_, String>(11)?,
        "total_bytes": r.get::<_, i64>(12)?,
        "progress_bytes": r.get::<_, i64>(13)?,
        "status": r.get::<_, String>(14)?,
        "error": r.get::<_, String>(15)?,
        "stats": serde_json::from_str::<Value>(&stats).unwrap_or(json!({})),
        "music_title": r.get::<_, String>(17)?,
        "created_at": iso(r.get::<_, i64>(18)?),
        "started_at": iso(r.get::<_, i64>(19)?),
        "finished_at": iso(r.get::<_, i64>(20)?),
    }))
}

/// unicode61's remove_diacritics strips accent marks but NOT the stroke of
/// đ/Đ (they are distinct letters, not letter+diacritic) — fold them by hand
/// on BOTH the indexed text and the query, so "duong pho" matches "đường phố".
/// FTS content is never displayed (rows render from the main table), so the
/// folding is invisible to users.
fn fold_vi(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'đ' => 'd',
            'Đ' => 'D',
            c => c,
        })
        .collect()
}

/// Every whitespace-separated term becomes `"term"*` — prefix match, quoted so
/// FTS operators typed by the user (`-`, `OR`…) cannot break the query.
fn fts_query(q: &str) -> String {
    fold_vi(q)
        .split_whitespace()
        .map(|t| format!("\"{}\"*", t.replace('"', "")))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enqueue_claim_finish_roundtrip() {
        let db = Db::open_memory().unwrap();
        let id = db
            .enqueue("https://www.tiktok.com/@a/video/1", "nowm", None)
            .unwrap();
        let id2 = db
            .enqueue("https://www.tiktok.com/@a/video/2", "hd", None)
            .unwrap();
        // FIFO: oldest first.
        assert_eq!(db.claim_next_queued(), Some(id));
        assert_eq!(db.claim_next_queued(), Some(id2));
        assert_eq!(db.claim_next_queued(), None);

        db.apply_resolved(
            id,
            &json!({"video_id":"1","kind":"video","title":"Giá vàng hôm nay","author_name":"anv","stats":{"play_count":9}}),
        );
        db.finish_files(id, "/tmp/x", &["/tmp/x/a.mp4".into()], 123);
        let row = db.get_download(id).unwrap();
        assert_eq!(row["status"], "done");
        assert_eq!(row["total_bytes"], 123);
        assert_eq!(row["files"][0], "/tmp/x/a.mp4");
        assert_eq!(row["stats"]["play_count"], 9);
    }

    #[test]
    fn fts_search_ignores_diacritics() {
        let db = Db::open_memory().unwrap();
        let id = db
            .enqueue("https://www.tiktok.com/@a/video/9", "nowm", None)
            .unwrap();
        db.apply_resolved(
            id,
            &json!({"video_id":"9","kind":"video","title":"Món ăn đường phố Sài Gòn","author_name":"foodtour"}),
        );
        let hit = db.list_downloads(Some("duong pho"), None, None, 10, 0);
        assert_eq!(hit.len(), 1, "gõ không dấu vẫn phải khớp");
        let miss = db.list_downloads(Some("hà nội"), None, None, 10, 0);
        assert!(miss.is_empty());
    }

    #[test]
    fn duplicate_detection_only_matches_done() {
        let db = Db::open_memory().unwrap();
        let url = "https://www.tiktok.com/@a/video/5";
        let id = db.enqueue(url, "nowm", None).unwrap();
        assert_eq!(db.find_done_duplicate(url, "nowm"), None, "queued ≠ done");
        db.claim_next_queued();
        db.finish_files(id, "/tmp", &[], 1);
        assert_eq!(db.find_done_duplicate(url, "nowm"), Some(id));
        assert_eq!(db.find_done_duplicate(url, "hd"), None, "khác chất lượng");
    }

    #[test]
    fn requeue_resets_progress_and_only_finished_rows() {
        let db = Db::open_memory().unwrap();
        let id = db.enqueue("u", "nowm", None).unwrap();
        assert!(!db.requeue(id), "đang queued thì không requeue");
        db.claim_next_queued();
        db.set_progress(id, 50, 100);
        db.set_status(id, "error", "mạng rớt");
        assert!(db.requeue(id));
        let row = db.get_download(id).unwrap();
        assert_eq!(row["status"], "queued");
        assert_eq!(row["progress_bytes"], 0);
        assert_eq!(row["error"], "");
    }

    #[test]
    fn clear_skips_active_jobs() {
        let db = Db::open_memory().unwrap();
        let a = db.enqueue("a", "nowm", None).unwrap();
        let b = db.enqueue("b", "nowm", None).unwrap();
        db.claim_next_queued(); // a → resolving
        db.set_status(b, "error", "x");
        let removed = db.clear_downloads(None);
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0]["id"], b);
        assert!(db.get_download(a).is_some(), "job đang chạy phải còn nguyên");
    }

    #[test]
    fn settings_fall_back_to_defaults() {
        let db = Db::open_memory().unwrap();
        assert_eq!(db.setting("default_quality", ""), "nowm");
        db.set_setting("default_quality", "hd");
        assert_eq!(db.setting("default_quality", ""), "hd");
        let all = db.all_settings();
        assert_eq!(all["default_quality"], "hd");
        assert_eq!(all["max_concurrent"], "2");
    }
}
