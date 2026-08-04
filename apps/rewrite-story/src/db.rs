//! SQLite layer.
//!
//! Single serialized connection behind a `Mutex` with WAL enabled, matching the
//! other Space Apps. Unlike `video-flow` — which carries dynamic
//! `serde_json::Map` rows and per-table column allowlists because it was porting
//! a Go `map[string]any` layer — this app uses typed structs. The schema is
//! small, fixed, and the background worker reads it on every tick, so the
//! compiler is more useful here than the flexibility is.

use std::sync::{Arc, Mutex};

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

const SCHEMA: &str = include_str!("schema.sql");

/// Process status values. These strings are persisted and appear on the wire —
/// the web UI and the MCP tools both match on them.
pub mod status {
    pub const QUEUED: &str = "queued";
    pub const PROCESSING: &str = "processing";
    pub const COMPLETED: &str = "completed";
    pub const FAILED: &str = "failed";
    pub const CANCELLED: &str = "cancelled";

    pub fn is_terminal(s: &str) -> bool {
        matches!(s, COMPLETED | FAILED | CANCELLED)
    }

    pub fn is_active(s: &str) -> bool {
        matches!(s, QUEUED | PROCESSING)
    }
}

/// Pipeline stages, used to shape the progress percentage.
pub mod stage {
    pub const PENDING: &str = "pending";
    pub const ANALYZING: &str = "analyzing";
    pub const REWRITING: &str = "rewriting";
    pub const SAVING: &str = "saving";
    pub const COMPLETED: &str = "completed";
    pub const FAILED: &str = "failed";
    pub const CANCELLED: &str = "cancelled";
}

#[derive(Debug, Clone, Serialize)]
pub struct Story {
    pub id: i64,
    pub name: String,
    pub parent_story_id: Option<i64>,
    pub version_number: i64,
    pub original_text: String,
    pub original_length: i64,
    pub source_type: String,
    pub creativity_ratio: Option<i64>,
    pub target_length_variance: Option<i64>,
    pub processing_time: Option<f64>,
    pub created_at: String,
}

/// Everything about a story except its text.
#[derive(Debug, Clone, Serialize)]
pub struct StoryMeta {
    pub id: i64,
    pub name: String,
    pub parent_story_id: Option<i64>,
    pub version_number: i64,
    pub original_length: i64,
    pub source_type: String,
    pub creativity_ratio: Option<i64>,
    pub target_length_variance: Option<i64>,
    pub processing_time: Option<f64>,
    pub created_at: String,
}

/// List-view projection. Stories hold entire novels — never send `original_text`
/// in a list response.
#[derive(Debug, Clone, Serialize)]
pub struct StorySummary {
    pub id: i64,
    pub name: String,
    pub parent_story_id: Option<i64>,
    pub version_number: i64,
    pub original_length: i64,
    pub source_type: String,
    pub created_at: String,
    pub preview: String,
    pub version_count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RewriteProcess {
    pub id: i64,
    pub story_id: i64,
    pub status: String,
    pub current_stage: String,
    pub progress_percentage: i64,
    pub total_chunks: i64,
    pub current_chunk: i64,
    pub error_message: Option<String>,
    pub creativity_ratio: i64,
    pub target_length_variance: i64,
    pub system_instruction: Option<String>,
    pub user_prompt: Option<String>,
    pub version_plan: Option<String>,
    pub model: Option<String>,
    pub result_story_id: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RewriteChunk {
    pub chunk_index: i64,
    pub original_content: String,
    pub rewritten_content: String,
}

/// Parameters for a new rewrite run.
#[derive(Debug, Clone)]
pub struct NewProcess {
    pub story_id: i64,
    pub creativity_ratio: i64,
    pub target_length_variance: i64,
    pub system_instruction: Option<String>,
    pub user_prompt: Option<String>,
    pub version_plan: Option<String>,
    pub model: Option<String>,
}

/// Settings a client may write. Shared by the REST and MCP surfaces so they
/// can't drift — the HTTP endpoint used to accept any key at all, storing
/// `max_concurrent_processes = "lots"` that read back in the UI while the poller
/// silently used the default.
pub const WRITABLE_SETTINGS: &[&str] = &[
    "hybrid_split_min_size",
    "hybrid_split_max_size",
    "hybrid_split_threshold",
    "default_creativity_ratio",
    "default_length_variance",
    "max_concurrent_processes",
    "parallel_chunks",
    "max_output_tokens",
    "llm_profile",
];

/// Rejects unknown keys and values that don't parse as the setting's type.
pub fn validate_setting(key: &str, value: &str) -> Result<()> {
    if !WRITABLE_SETTINGS.contains(&key) {
        anyhow::bail!(
            "cấu hình '{key}' không hợp lệ; hợp lệ: {}",
            WRITABLE_SETTINGS.join(", ")
        );
    }
    let range = |lo: i64, hi: i64| -> Result<()> {
        let n: i64 = value
            .trim()
            .parse()
            .map_err(|_| anyhow::anyhow!("'{key}' phải là số nguyên, nhận '{value}'"))?;
        if n < lo || n > hi {
            anyhow::bail!("'{key}' phải trong khoảng {lo}..{hi}, nhận {n}");
        }
        Ok(())
    };
    match key {
        "hybrid_split_min_size" => range(100, 100_000),
        "hybrid_split_max_size" => range(200, 200_000),
        "default_creativity_ratio" | "default_length_variance" => range(0, 100),
        "max_concurrent_processes" | "parallel_chunks" => range(1, 8),
        "max_output_tokens" => range(2048, 200_000),
        "hybrid_split_threshold" => {
            let f: f64 = value
                .trim()
                .parse()
                .map_err(|_| anyhow::anyhow!("'{key}' phải là số, nhận '{value}'"))?;
            if !(0.0..=1.0).contains(&f) {
                anyhow::bail!("'{key}' phải trong khoảng 0..1, nhận {f}");
            }
            Ok(())
        }
        // Free-form.
        _ => Ok(()),
    }
}

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

fn now() -> String {
    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// First `n` characters of `s`, char-safe, with an ellipsis when truncated.
fn preview_of(s: &str, n: usize) -> String {
    let mut out: String = s.chars().take(n).collect();
    if s.chars().nth(n).is_some() {
        out.push('…');
    }
    out
}

impl Db {
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    #[cfg(test)]
    pub fn open_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "busy_timeout", 5000)?;
        // The schema declares FKs and we rely on ON DELETE CASCADE, so unlike
        // video-flow these are enforced.
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA)?;
        let db = Db {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.migrate()?;
        Ok(db)
    }

    /// One-shot data migrations. `schema.sql` itself is idempotent
    /// `CREATE TABLE IF NOT EXISTS` + `INSERT OR IGNORE`, so it cannot fix values
    /// that already exist in an installed database.
    fn migrate(&self) -> Result<()> {
        if self.setting_i64("schema_version", 0) < 1 {
            // v1: the splitter switched from byte sizing to character sizing, and
            // `max_size` became bounded by what a provider will actually emit.
            // An install carrying the old 15000 would now ask for a chunk whose
            // rewrite cannot fit under the output cap, failing on chunk 1 of
            // every story. Only values above the cap are touched — a deliberate
            // smaller setting is left alone.
            let cap = crate::llm::MAX_CHUNK_CHARS as i64;
            if self.setting_i64("hybrid_split_max_size", cap) > cap {
                self.set_setting("hybrid_split_max_size", &cap.to_string())?;
                let min = self.setting_i64("hybrid_split_min_size", cap / 2);
                if min >= cap {
                    self.set_setting("hybrid_split_min_size", &(cap / 2).to_string())?;
                }
                println!("[db] migrated splitter sizes to characters (max {cap})");
            }
            self.set_setting("schema_version", "1")?;
        }
        if self.setting_i64("schema_version", 0) < 2 {
            // v2: the chunk ceiling was re-derived from measurement rather than
            // from a token budget. The model writes back a fixed amount of prose
            // regardless of input size, so anything above the ceiling comes back
            // as a summary — silently, with a normal finish reason. Existing
            // installs carrying the larger bound are producing summaries right
            // now, so clamp them.
            let cap = crate::llm::MAX_CHUNK_CHARS as i64;
            if self.setting_i64("hybrid_split_max_size", cap) > cap {
                self.set_setting("hybrid_split_max_size", &cap.to_string())?;
                if self.setting_i64("hybrid_split_min_size", 0) >= cap {
                    self.set_setting("hybrid_split_min_size", &(cap * 3 / 5).to_string())?;
                }
                println!("[db] clamped chunk size to the measured model ceiling ({cap} chars)");
            }
            self.set_setting("schema_version", "2")?;
        }
        Ok(())
    }

    /// Run `f` with the (locked) connection.
    pub fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> Result<T> {
        let conn = self.conn.lock().unwrap();
        Ok(f(&conn)?)
    }

    // ---- settings ----

    pub fn setting(&self, key: &str, default: &str) -> String {
        self.with_conn(|c| {
            c.query_row(
                "SELECT value FROM app_settings WHERE key = ?1",
                params![key],
                |r| r.get::<_, String>(0),
            )
            .optional()
        })
        .ok()
        .flatten()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| default.to_string())
    }

    pub fn setting_i64(&self, key: &str, default: i64) -> i64 {
        self.setting(key, "").parse().unwrap_or(default)
    }

    pub fn setting_f64(&self, key: &str, default: f64) -> f64 {
        self.setting(key, "").parse().unwrap_or(default)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "INSERT OR REPLACE INTO app_settings (key, value) VALUES (?1, ?2)",
                params![key, value],
            )
        })?;
        Ok(())
    }

    pub fn all_settings(&self) -> Result<Vec<(String, String)>> {
        self.with_conn(|c| {
            let mut st = c.prepare("SELECT key, value FROM app_settings ORDER BY key")?;
            let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect()
        })
    }

    // ---- stories ----

    /// Import an original story — a root of the version tree.
    pub fn create_story(&self, name: &str, text: &str) -> Result<i64> {
        self.insert_story(name, text, None, 1, "human", None, None, None)
    }

    /// Record a rewrite result as a new version of `parent_story_id`.
    #[allow(clippy::too_many_arguments)]
    pub fn create_version(
        &self,
        parent_story_id: i64,
        name: &str,
        text: &str,
        version_number: i64,
        creativity_ratio: i64,
        target_length_variance: i64,
        processing_time: f64,
    ) -> Result<i64> {
        self.insert_story(
            name,
            text,
            Some(parent_story_id),
            version_number,
            "ai",
            Some(creativity_ratio),
            Some(target_length_variance),
            Some(processing_time),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_story(
        &self,
        name: &str,
        text: &str,
        parent_story_id: Option<i64>,
        version_number: i64,
        source_type: &str,
        creativity_ratio: Option<i64>,
        target_length_variance: Option<i64>,
        processing_time: Option<f64>,
    ) -> Result<i64> {
        // Length is a character count, not Go's byte count.
        let length = text.chars().count() as i64;
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO stories (name, parent_story_id, version_number, original_text,
                    original_length, source_type, creativity_ratio, target_length_variance,
                    processing_time, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    name,
                    parent_story_id,
                    version_number,
                    text,
                    length,
                    source_type,
                    creativity_ratio,
                    target_length_variance,
                    processing_time,
                    now()
                ],
            )?;
            Ok(c.last_insert_rowid())
        })
    }

    /// Existence check that does not drag the whole novel out of the DB.
    pub fn story_exists(&self, id: i64) -> Result<bool> {
        self.with_conn(|c| {
            c.query_row("SELECT 1 FROM stories WHERE id = ?1", params![id], |_| {
                Ok(())
            })
            .optional()
        })
        .map(|o| o.is_some())
    }

    /// Story name only. `get_story` loads `original_text`, which for a full novel
    /// is millions of characters — never call it just to read a field.
    pub fn story_name(&self, id: i64) -> Result<Option<String>> {
        self.with_conn(|c| {
            c.query_row("SELECT name FROM stories WHERE id = ?1", params![id], |r| {
                r.get(0)
            })
            .optional()
        })
    }

    /// Story fields without the text column.
    pub fn story_meta(&self, id: i64) -> Result<Option<StoryMeta>> {
        self.with_conn(|c| {
            c.query_row(
                "SELECT id, name, parent_story_id, version_number, original_length,
                        source_type, creativity_ratio, target_length_variance,
                        processing_time, created_at
                 FROM stories WHERE id = ?1",
                params![id],
                |r| {
                    Ok(StoryMeta {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        parent_story_id: r.get(2)?,
                        version_number: r.get(3)?,
                        original_length: r.get(4)?,
                        source_type: r.get(5)?,
                        creativity_ratio: r.get(6)?,
                        target_length_variance: r.get(7)?,
                        processing_time: r.get(8)?,
                        created_at: r.get(9)?,
                    })
                },
            )
            .optional()
        })
    }

    pub fn story_text(&self, id: i64) -> Result<Option<String>> {
        self.with_conn(|c| {
            c.query_row(
                "SELECT original_text FROM stories WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .optional()
        })
    }

    pub fn get_story(&self, id: i64) -> Result<Option<Story>> {
        self.with_conn(|c| {
            c.query_row(
                "SELECT * FROM stories WHERE id = ?1",
                params![id],
                story_from_row,
            )
            .optional()
        })
    }

    /// Root stories (imported originals) newest first, with their version counts.
    pub fn list_stories(&self) -> Result<Vec<StorySummary>> {
        self.with_conn(|c| {
            let mut st = c.prepare(
                "SELECT s.id, s.name, s.parent_story_id, s.version_number, s.original_length,
                        s.source_type, s.created_at, substr(s.original_text, 1, 400),
                        (SELECT COUNT(*) FROM stories v WHERE v.parent_story_id = s.id)
                 FROM stories s
                 WHERE s.parent_story_id IS NULL
                 ORDER BY s.created_at DESC",
            )?;
            let rows = st.query_map([], summary_from_row)?;
            rows.collect()
        })
    }

    /// Rewritten versions of `story_id`, oldest version first.
    pub fn list_versions(&self, story_id: i64) -> Result<Vec<StorySummary>> {
        self.with_conn(|c| {
            let mut st = c.prepare(
                "SELECT s.id, s.name, s.parent_story_id, s.version_number, s.original_length,
                        s.source_type, s.created_at, substr(s.original_text, 1, 400), 0
                 FROM stories s
                 WHERE s.parent_story_id = ?1
                 ORDER BY s.version_number ASC",
            )?;
            let rows = st.query_map(params![story_id], summary_from_row)?;
            rows.collect()
        })
    }

    pub fn delete_story(&self, id: i64) -> Result<usize> {
        self.with_conn(|c| c.execute("DELETE FROM stories WHERE id = ?1", params![id]))
    }

    /// Ids of this story's queued or running processes.
    ///
    /// `rewrite_processes.story_id` cascades, so deleting a story mid-run pulls
    /// the process row and its chunks out from under a live worker: the worker
    /// keeps paying for model calls, then fails silently because there is no row
    /// left to write the error to.
    pub fn active_processes_for_story(&self, story_id: i64) -> Result<Vec<i64>> {
        self.with_conn(|c| {
            let mut st = c.prepare(
                "SELECT id FROM rewrite_processes
                 WHERE story_id = ?1 AND status IN ('queued', 'processing')",
            )?;
            let rows = st.query_map(params![story_id], |r| r.get(0))?;
            rows.collect()
        })
    }

    /// Next version number for children of `story_id` (1-based).
    pub fn next_version_number(&self, story_id: i64) -> Result<i64> {
        self.with_conn(|c| {
            c.query_row(
                "SELECT COALESCE(MAX(version_number), 0) + 1 FROM stories WHERE parent_story_id = ?1",
                params![story_id],
                |r| r.get(0),
            )
        })
    }

    // ---- source chunks ----

    pub fn get_chunks(&self, story_id: i64) -> Result<Vec<String>> {
        self.with_conn(|c| {
            let mut st = c.prepare(
                "SELECT content FROM story_chunks WHERE story_id = ?1 ORDER BY chunk_index ASC",
            )?;
            let rows = st.query_map(params![story_id], |r| r.get(0))?;
            rows.collect()
        })
    }

    pub fn save_chunks(&self, story_id: i64, chunks: &[String]) -> Result<()> {
        if chunks.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        for (i, content) in chunks.iter().enumerate() {
            tx.execute(
                "INSERT OR REPLACE INTO story_chunks (story_id, chunk_index, content)
                 VALUES (?1, ?2, ?3)",
                params![story_id, i as i64, content],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    // ---- processes ----

    pub fn create_process(&self, p: &NewProcess) -> Result<i64> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO rewrite_processes (story_id, status, current_stage, creativity_ratio,
                    target_length_variance, system_instruction, user_prompt, version_plan, model,
                    created_at, updated_at)
                 VALUES (?1, 'queued', 'pending', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
                params![
                    p.story_id,
                    p.creativity_ratio,
                    p.target_length_variance,
                    p.system_instruction,
                    p.user_prompt,
                    p.version_plan,
                    p.model,
                    now()
                ],
            )?;
            Ok(c.last_insert_rowid())
        })
    }

    pub fn get_process(&self, id: i64) -> Result<Option<RewriteProcess>> {
        self.with_conn(|c| {
            c.query_row(
                "SELECT * FROM rewrite_processes WHERE id = ?1",
                params![id],
                process_from_row,
            )
            .optional()
        })
    }

    pub fn list_processes(&self, status_filter: Option<&str>) -> Result<Vec<RewriteProcess>> {
        self.with_conn(|c| match status_filter {
            Some(s) => {
                let mut st = c.prepare(
                    "SELECT * FROM rewrite_processes WHERE status = ?1 ORDER BY created_at DESC",
                )?;
                let rows = st.query_map(params![s], process_from_row)?;
                rows.collect()
            }
            None => {
                let mut st = c.prepare(
                    "SELECT * FROM rewrite_processes ORDER BY created_at DESC LIMIT 200",
                )?;
                let rows = st.query_map([], process_from_row)?;
                rows.collect()
            }
        })
    }

    pub fn count_by_status(&self, status: &str) -> Result<i64> {
        self.with_conn(|c| {
            c.query_row(
                "SELECT COUNT(*) FROM rewrite_processes WHERE status = ?1",
                params![status],
                |r| r.get(0),
            )
        })
    }

    /// Oldest queued processes first — the worker's claim order.
    pub fn pending_processes(&self, limit: i64) -> Result<Vec<RewriteProcess>> {
        self.with_conn(|c| {
            let mut st = c.prepare(
                "SELECT * FROM rewrite_processes WHERE status = 'queued'
                 ORDER BY created_at ASC LIMIT ?1",
            )?;
            let rows = st.query_map(params![limit], process_from_row)?;
            rows.collect()
        })
    }

    /// Atomically move a queued process to `processing`.
    ///
    /// The `WHERE status = 'queued'` predicate is what makes this a claim: two
    /// racing pollers cannot both get 1 row back. Go flipped the status with an
    /// unconditional UPDATE and relied on there being a single instance.
    pub fn claim_process(&self, id: i64) -> Result<bool> {
        let n = self.with_conn(|c| {
            c.execute(
                "UPDATE rewrite_processes
                 SET status = 'processing', current_stage = 'pending',
                     started_at = COALESCE(started_at, ?2), updated_at = ?2
                 WHERE id = ?1 AND status = 'queued'",
                params![id, now()],
            )
        })?;
        Ok(n == 1)
    }

    /// Persist a progress update. Returns false if the write was rejected.
    ///
    /// Two guards, both load-bearing, and both expressed as predicates on the
    /// UPDATE itself rather than as a preceding read:
    ///
    /// * A process already in a terminal state cannot be moved out of one, so an
    ///   in-flight worker can't resurrect a cancelled or watchdog-failed run.
    /// * With `only_if_running`, the write additionally requires the row to still
    ///   be `processing`. Workers set this: a task that has been superseded —
    ///   cancelled, then retried while it was still blocked in a model call —
    ///   would otherwise stamp its stale terminal status over the queued retry
    ///   and silently undo it.
    ///
    /// Checking these in Rust first was a time-of-check/time-of-use hole: the
    /// read and the write are separate lock acquisitions, so a cancel landing
    /// between them was overwritten anyway — producing a row that read
    /// `processing` while carrying a `completed_at` and "cancelled by user".
    /// The monotonic-progress rule is in the statement for the same reason.
    #[allow(clippy::too_many_arguments)]
    pub fn update_progress_guarded(
        &self,
        id: i64,
        status: &str,
        stage: &str,
        progress: i64,
        current_chunk: i64,
        total_chunks: i64,
        error_message: Option<&str>,
        result_story_id: Option<i64>,
        only_if_running: bool,
    ) -> Result<bool> {
        let completed_at = status::is_terminal(status).then(now);
        let n = self.with_conn(|c| {
            c.execute(
                "UPDATE rewrite_processes SET
                    status = ?2, current_stage = ?3,
                    -- Progress never goes backwards within a single status.
                    progress_percentage = CASE WHEN status = ?2
                        THEN MAX(progress_percentage, ?4) ELSE ?4 END,
                    current_chunk = CASE WHEN ?5 > 0 THEN ?5 ELSE current_chunk END,
                    total_chunks  = CASE WHEN ?6 > 0 THEN ?6 ELSE total_chunks  END,
                    error_message = COALESCE(?7, error_message),
                    result_story_id = COALESCE(?8, result_story_id),
                    completed_at  = COALESCE(?9, completed_at),
                    updated_at = ?10
                 WHERE id = ?1
                   AND (status NOT IN ('completed', 'failed', 'cancelled') OR status = ?2)
                   AND (?11 = 0 OR status = 'processing')",
                params![
                    id,
                    status,
                    stage,
                    progress,
                    current_chunk,
                    total_chunks,
                    error_message,
                    result_story_id,
                    completed_at,
                    now(),
                    only_if_running as i64
                ],
            )
        })?;
        Ok(n == 1)
    }

    /// Progress update from outside the worker (the cancel endpoint, the
    /// watchdog) — allowed to act on a queued process too.
    #[allow(clippy::too_many_arguments)]
    pub fn update_progress(
        &self,
        id: i64,
        status: &str,
        stage: &str,
        progress: i64,
        current_chunk: i64,
        total_chunks: i64,
        error_message: Option<&str>,
        result_story_id: Option<i64>,
    ) -> Result<bool> {
        self.update_progress_guarded(
            id,
            status,
            stage,
            progress,
            current_chunk,
            total_chunks,
            error_message,
            result_story_id,
            false,
        )
    }

    /// Reset a process for a retry. Chunks are deliberately left in place —
    /// that is what makes retry a resume.
    pub fn requeue_process(&self, id: i64) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "UPDATE rewrite_processes SET
                    status = 'queued', current_stage = 'pending', progress_percentage = 0,
                    current_chunk = 0, error_message = NULL, completed_at = NULL,
                    updated_at = ?2
                 WHERE id = ?1",
                params![id, now()],
            )
        })?;
        Ok(())
    }

    pub fn delete_process(&self, id: i64) -> Result<usize> {
        self.with_conn(|c| c.execute("DELETE FROM rewrite_processes WHERE id = ?1", params![id]))
    }

    /// Fail every process left mid-flight by a crash or restart.
    pub fn reconcile_orphans(&self, message: &str) -> Result<usize> {
        self.with_conn(|c| {
            c.execute(
                "UPDATE rewrite_processes
                 SET status = 'failed', current_stage = 'failed', error_message = ?1,
                     completed_at = ?2, updated_at = ?2
                 WHERE status = 'processing'",
                params![message, now()],
            )
        })
    }

    /// Processes whose status is stale past `cutoff` (an SQL datetime string).
    pub fn stale_processes(&self, status: &str, column: &str, cutoff: &str) -> Result<Vec<i64>> {
        // `column` is caller-controlled but never client-controlled; restrict it
        // anyway so this can't grow into an injection point.
        let column = match column {
            "updated_at" => "updated_at",
            "created_at" => "created_at",
            other => anyhow::bail!("unsupported staleness column: {other}"),
        };
        let sql = format!("SELECT id FROM rewrite_processes WHERE status = ?1 AND {column} < ?2");
        self.with_conn(|c| {
            let mut st = c.prepare(&sql)?;
            let rows = st.query_map(params![status, cutoff], |r| r.get(0))?;
            rows.collect()
        })
    }

    // ---- rewrite chunks ----

    /// Which chunk indices a previous run already finished.
    ///
    /// The resume scan only needs the index set. Pulling the text as well kept
    /// the entire rewritten novel resident for the whole run to answer
    /// `contains_key`.
    pub fn rewritten_indices(&self, process_id: i64) -> Result<Vec<i64>> {
        self.with_conn(|c| {
            let mut st = c.prepare(
                "SELECT chunk_index FROM rewrite_chunks WHERE process_id = ?1
                 ORDER BY chunk_index ASC",
            )?;
            let rows = st.query_map(params![process_id], |r| r.get(0))?;
            rows.collect()
        })
    }

    /// The rewritten text of a single chunk.
    pub fn rewritten_chunk(&self, process_id: i64, chunk_index: i64) -> Result<Option<String>> {
        self.with_conn(|c| {
            c.query_row(
                "SELECT rewritten_content FROM rewrite_chunks
                 WHERE process_id = ?1 AND chunk_index = ?2",
                params![process_id, chunk_index],
                |r| r.get(0),
            )
            .optional()
        })
    }

    /// A character window of a story, sliced in SQL.
    ///
    /// Returns `(slice, total_chars)`. Paginating in Rust means decoding the
    /// whole novel per page; `substr` on a TEXT column counts characters, so the
    /// database does it once and returns only what was asked for.
    pub fn story_slice(&self, id: i64, offset: i64, limit: i64) -> Result<Option<(String, i64)>> {
        self.with_conn(|c| {
            c.query_row(
                "SELECT substr(original_text, ?2 + 1, ?3), length(original_text)
                 FROM stories WHERE id = ?1",
                params![id, offset.max(0), limit.max(0)],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
        })
    }

    pub fn get_rewrite_chunks(&self, process_id: i64) -> Result<Vec<RewriteChunk>> {
        self.with_conn(|c| {
            let mut st = c.prepare(
                "SELECT chunk_index, original_content, rewritten_content
                 FROM rewrite_chunks WHERE process_id = ?1 ORDER BY chunk_index ASC",
            )?;
            let rows = st.query_map(params![process_id], |r| {
                Ok(RewriteChunk {
                    chunk_index: r.get(0)?,
                    original_content: r.get(1)?,
                    rewritten_content: r.get(2)?,
                })
            })?;
            rows.collect()
        })
    }

    pub fn save_rewrite_chunk(
        &self,
        process_id: i64,
        chunk_index: i64,
        original: &str,
        rewritten: &str,
    ) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "INSERT OR REPLACE INTO rewrite_chunks
                    (process_id, chunk_index, original_content, rewritten_content, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![process_id, chunk_index, original, rewritten, now()],
            )
        })?;
        Ok(())
    }

    /// Reassemble the finished text from persisted chunks.
    ///
    /// Errors unless the stored indices are exactly `0..total`. Concatenating
    /// whatever rows happen to exist would silently ship a novel with a chapter
    /// missing — and the process would be marked `completed` while it was.
    pub fn assemble_rewrite(&self, process_id: i64, total: i64) -> Result<String> {
        let rows: Vec<(i64, String)> = self.with_conn(|c| {
            let mut st = c.prepare(
                "SELECT chunk_index, rewritten_content FROM rewrite_chunks
                 WHERE process_id = ?1 ORDER BY chunk_index ASC",
            )?;
            let rows = st.query_map(params![process_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect()
        })?;

        if rows.len() as i64 != total {
            anyhow::bail!("thiếu chunk khi ghép: có {} / cần {total}", rows.len());
        }
        for (expected, (actual, _)) in rows.iter().enumerate() {
            if expected as i64 != *actual {
                anyhow::bail!("chunk không liên tục khi ghép: gặp {actual}, cần {expected}");
            }
        }

        Ok(rows
            .iter()
            .map(|(_, text)| text.trim())
            .collect::<Vec<_>>()
            .join("\n\n")
            .trim()
            .to_string())
    }
}

fn story_from_row(r: &Row<'_>) -> rusqlite::Result<Story> {
    Ok(Story {
        id: r.get("id")?,
        name: r.get("name")?,
        parent_story_id: r.get("parent_story_id")?,
        version_number: r.get("version_number")?,
        original_text: r.get("original_text")?,
        original_length: r.get("original_length")?,
        source_type: r.get("source_type")?,
        creativity_ratio: r.get("creativity_ratio")?,
        target_length_variance: r.get("target_length_variance")?,
        processing_time: r.get("processing_time")?,
        created_at: r.get("created_at")?,
    })
}

fn summary_from_row(r: &Row<'_>) -> rusqlite::Result<StorySummary> {
    let head: String = r.get(7)?;
    Ok(StorySummary {
        id: r.get(0)?,
        name: r.get(1)?,
        parent_story_id: r.get(2)?,
        version_number: r.get(3)?,
        original_length: r.get(4)?,
        source_type: r.get(5)?,
        created_at: r.get(6)?,
        preview: preview_of(&head, 200),
        version_count: r.get(8)?,
    })
}

fn process_from_row(r: &Row<'_>) -> rusqlite::Result<RewriteProcess> {
    Ok(RewriteProcess {
        id: r.get("id")?,
        story_id: r.get("story_id")?,
        status: r.get("status")?,
        current_stage: r.get("current_stage")?,
        progress_percentage: r.get("progress_percentage")?,
        total_chunks: r.get("total_chunks")?,
        current_chunk: r.get("current_chunk")?,
        error_message: r.get("error_message")?,
        creativity_ratio: r.get("creativity_ratio")?,
        target_length_variance: r.get("target_length_variance")?,
        system_instruction: r.get("system_instruction")?,
        user_prompt: r.get("user_prompt")?,
        version_plan: r.get("version_plan")?,
        model: r.get("model")?,
        result_story_id: r.get("result_story_id")?,
        created_at: r.get("created_at")?,
        updated_at: r.get("updated_at")?,
        started_at: r.get("started_at")?,
        completed_at: r.get("completed_at")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(db: &Db) -> i64 {
        db.create_story("Truyện gốc", "Nội dung truyện.").unwrap()
    }

    fn new_process(story_id: i64) -> NewProcess {
        NewProcess {
            story_id,
            creativity_ratio: 40,
            target_length_variance: 5,
            system_instruction: None,
            user_prompt: None,
            version_plan: None,
            model: None,
        }
    }

    #[test]
    fn story_roundtrip_counts_characters_not_bytes() {
        let db = Db::open_memory().unwrap();
        // 5 Vietnamese chars, 10 UTF-8 bytes.
        let id = db.create_story("T", "đằng ẵ").unwrap();
        let s = db.get_story(id).unwrap().unwrap();
        assert_eq!(s.original_length, "đằng ẵ".chars().count() as i64);
        assert_ne!(s.original_length, "đằng ẵ".len() as i64);
    }

    #[test]
    fn claim_is_atomic() {
        let db = Db::open_memory().unwrap();
        let story = seed(&db);
        let pid = db.create_process(&new_process(story)).unwrap();

        assert!(db.claim_process(pid).unwrap(), "first claim wins");
        assert!(!db.claim_process(pid).unwrap(), "second claim must lose");
        assert_eq!(
            db.get_process(pid).unwrap().unwrap().status,
            status::PROCESSING
        );
    }

    #[test]
    fn terminal_state_cannot_be_resurrected() {
        let db = Db::open_memory().unwrap();
        let story = seed(&db);
        let pid = db.create_process(&new_process(story)).unwrap();

        db.update_progress(
            pid,
            status::CANCELLED,
            stage::CANCELLED,
            0,
            0,
            0,
            None,
            None,
        )
        .unwrap();
        // An in-flight worker tries to report progress after the user cancelled.
        let applied = db
            .update_progress(
                pid,
                status::PROCESSING,
                stage::REWRITING,
                50,
                2,
                4,
                None,
                None,
            )
            .unwrap();

        assert!(!applied, "update should be rejected");
        assert_eq!(
            db.get_process(pid).unwrap().unwrap().status,
            status::CANCELLED
        );
    }

    /// Regression: a task that was cancelled and then re-queued by a retry must
    /// not be able to stamp its stale terminal status over the fresh run. Seen
    /// live — the row read "cancelled, 0%" while chunks kept being written.
    #[test]
    fn a_superseded_worker_cannot_overwrite_a_retried_process() {
        let db = Db::open_memory().unwrap();
        let story = seed(&db);
        let pid = db.create_process(&new_process(story)).unwrap();
        db.claim_process(pid).unwrap();

        // User cancels; the worker is still blocked in a model call.
        db.update_progress(
            pid,
            status::CANCELLED,
            stage::CANCELLED,
            0,
            0,
            0,
            None,
            None,
        )
        .unwrap();
        // User retries — the row goes back to queued.
        db.requeue_process(pid).unwrap();

        // Now the old worker finally wakes up and tries to report the cancel.
        let applied = db
            .update_progress_guarded(
                pid,
                status::CANCELLED,
                stage::CANCELLED,
                0,
                0,
                0,
                Some("Bị hủy bởi người dùng"),
                None,
                true,
            )
            .unwrap();

        assert!(!applied, "stale worker write must be rejected");
        assert_eq!(
            db.get_process(pid).unwrap().unwrap().status,
            status::QUEUED,
            "the retry must survive"
        );
    }

    #[test]
    fn progress_never_goes_backwards_within_a_status() {
        let db = Db::open_memory().unwrap();
        let story = seed(&db);
        let pid = db.create_process(&new_process(story)).unwrap();
        db.claim_process(pid).unwrap();

        db.update_progress(
            pid,
            status::PROCESSING,
            stage::REWRITING,
            60,
            3,
            5,
            None,
            None,
        )
        .unwrap();
        db.update_progress(
            pid,
            status::PROCESSING,
            stage::REWRITING,
            20,
            3,
            5,
            None,
            None,
        )
        .unwrap();

        assert_eq!(
            db.get_process(pid).unwrap().unwrap().progress_percentage,
            60
        );
    }

    #[test]
    fn rewrite_chunks_are_idempotent_and_assemble_in_order() {
        let db = Db::open_memory().unwrap();
        let story = seed(&db);
        let pid = db.create_process(&new_process(story)).unwrap();

        db.save_rewrite_chunk(pid, 1, "b", "Phần hai").unwrap();
        db.save_rewrite_chunk(pid, 0, "a", "Phần một").unwrap();
        // A re-run of chunk 0 must overwrite, not duplicate.
        db.save_rewrite_chunk(pid, 0, "a", "Phần một").unwrap();

        assert_eq!(db.get_rewrite_chunks(pid).unwrap().len(), 2);
        assert_eq!(db.assemble_rewrite(pid, 2).unwrap(), "Phần một\n\nPhần hai");
    }

    /// Assembly must refuse an incomplete set rather than ship a novel with a
    /// hole in it under a `completed` status.
    #[test]
    fn assemble_refuses_a_gap() {
        let db = Db::open_memory().unwrap();
        let sid = seed(&db);
        let pid = db.create_process(&new_process(sid)).unwrap();

        db.save_rewrite_chunk(pid, 0, "a", "A").unwrap();
        db.save_rewrite_chunk(pid, 2, "c", "C").unwrap();

        assert!(db.assemble_rewrite(pid, 3).is_err(), "missing chunk 1");
        assert!(db.assemble_rewrite(pid, 2).is_err(), "indices are not 0..2");
    }

    /// An install created before the byte→character switch carries a max_size
    /// that no provider can emit a rewrite for. Migration must clamp it.
    #[test]
    fn migration_clamps_an_oversized_split_bound() {
        let db = Db::open_memory().unwrap();
        let cap = crate::llm::MAX_CHUNK_CHARS as i64;

        // Simulate the pre-migration state.
        db.set_setting("hybrid_split_max_size", "15000").unwrap();
        db.set_setting("hybrid_split_min_size", "8000").unwrap();
        db.set_setting("schema_version", "0").unwrap();
        db.migrate().unwrap();

        assert_eq!(db.setting_i64("hybrid_split_max_size", 0), cap);
        assert!(db.setting_i64("hybrid_split_min_size", 0) < cap);
        assert!(db.setting_i64("schema_version", 0) >= 1);
    }

    /// A deliberate setting under the cap must survive untouched, and the
    /// migration must not re-run.
    #[test]
    fn migration_leaves_a_valid_setting_alone_and_runs_once() {
        let db = Db::open_memory().unwrap();

        let under_cap = (crate::llm::MAX_CHUNK_CHARS / 2).to_string();
        db.set_setting("hybrid_split_max_size", &under_cap).unwrap();
        db.set_setting("schema_version", "0").unwrap();
        db.migrate().unwrap();
        assert_eq!(
            db.setting("hybrid_split_max_size", ""),
            under_cap,
            "a setting already under the ceiling must be left alone"
        );

        // A later oversized value set by the user is theirs to keep.
        db.set_setting("hybrid_split_max_size", "40000").unwrap();
        db.migrate().unwrap();
        assert_eq!(db.setting_i64("hybrid_split_max_size", 0), 40000);
    }

    #[test]
    fn settings_validation_rejects_bad_keys_and_values() {
        assert!(validate_setting("nonsense", "1").is_err());
        assert!(validate_setting("parallel_chunks", "lots").is_err());
        assert!(validate_setting("parallel_chunks", "99").is_err());
        assert!(validate_setting("hybrid_split_threshold", "1.5").is_err());

        assert!(validate_setting("parallel_chunks", "4").is_ok());
        assert!(validate_setting("hybrid_split_threshold", "0.25").is_ok());
        assert!(validate_setting("llm_profile", "anything").is_ok());
    }

    #[test]
    fn active_processes_block_story_deletion() {
        let db = Db::open_memory().unwrap();
        let sid = seed(&db);
        let pid = db.create_process(&new_process(sid)).unwrap();

        assert_eq!(db.active_processes_for_story(sid).unwrap(), vec![pid]);

        db.update_progress(
            pid,
            status::COMPLETED,
            stage::COMPLETED,
            100,
            0,
            0,
            None,
            None,
        )
        .unwrap();
        assert!(db.active_processes_for_story(sid).unwrap().is_empty());
    }

    #[test]
    fn deleting_a_story_cascades_to_processes_and_chunks() {
        let db = Db::open_memory().unwrap();
        let story = seed(&db);
        let pid = db.create_process(&new_process(story)).unwrap();
        db.save_rewrite_chunk(pid, 0, "a", "A").unwrap();
        db.save_chunks(story, &["a".to_string()]).unwrap();

        db.delete_story(story).unwrap();

        assert!(db.get_process(pid).unwrap().is_none());
        assert!(db.get_rewrite_chunks(pid).unwrap().is_empty());
        assert!(db.get_chunks(story).unwrap().is_empty());
    }

    #[test]
    fn version_numbers_increment_per_parent() {
        let db = Db::open_memory().unwrap();
        let parent = seed(&db);

        let v1 = db.next_version_number(parent).unwrap();
        db.create_version(parent, "Truyện gốc", "v1", v1, 40, 5, 0.0)
            .unwrap();
        let v2 = db.next_version_number(parent).unwrap();

        assert_eq!((v1, v2), (1, 2));
        assert_eq!(db.list_versions(parent).unwrap().len(), 1);
        // Versions are not roots, so the library listing stays clean.
        assert_eq!(db.list_stories().unwrap().len(), 1);
        assert_eq!(db.list_stories().unwrap()[0].version_count, 1);
    }

    #[test]
    fn stale_processes_rejects_an_unexpected_column() {
        let db = Db::open_memory().unwrap();
        assert!(db
            .stale_processes("queued", "id; DROP TABLE stories", "x")
            .is_err());
    }
}
