//! SQLite access — a single serialized connection behind a mutex.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{Arc, Mutex};

const SCHEMA: &str = include_str!("schema.sql");

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// The `scene_id` of a scene, as text.
///
/// The model is asked for `"scene_id":"3"` but sometimes emits a bare number,
/// so both shapes have to land in the column the same way.
fn scene_key(scene: &Value) -> String {
    match scene.get("scene_id") {
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: i64,
    pub name: String,
    #[serde(skip_serializing)]
    pub video_path: String,
    pub video_mime: String,
    pub video_size: i64,
    pub video_filename: String,
    #[serde(skip_serializing)]
    pub file_uri: String,
    #[serde(skip_serializing)]
    pub file_uri_at: String,
    #[serde(skip_serializing)]
    pub char_image_path: String,
    #[serde(skip_serializing)]
    pub char_image_mime: String,
    pub has_char_image: bool,
    pub style: String,
    pub model: String,
    pub char_description: String,
    pub custom_dialogue: String,
    pub bg_description: String,
    pub auto_magic: bool,
    pub visual_similarity: i64,
    pub created_at: String,
    pub updated_at: String,
}

fn project_from_row(r: &Row) -> rusqlite::Result<Project> {
    let char_image_path: String = r.get("char_image_path")?;
    Ok(Project {
        id: r.get("id")?,
        name: r.get("name")?,
        video_path: r.get("video_path")?,
        video_mime: r.get("video_mime")?,
        video_size: r.get("video_size")?,
        video_filename: r.get("video_filename")?,
        file_uri: r.get("file_uri")?,
        file_uri_at: r.get("file_uri_at")?,
        has_char_image: !char_image_path.is_empty(),
        char_image_path,
        char_image_mime: r.get("char_image_mime")?,
        style: r.get("style")?,
        model: r.get("model")?,
        char_description: r.get("char_description")?,
        custom_dialogue: r.get("custom_dialogue")?,
        bg_description: r.get("bg_description")?,
        auto_magic: r.get::<_, i64>("auto_magic")? != 0,
        visual_similarity: r.get("visual_similarity")?,
        created_at: r.get("created_at")?,
        updated_at: r.get("updated_at")?,
    })
}

/// Config knobs that drive prompt building. Kept separate from `Project` so a
/// caller can override the stored config for a single run without persisting it.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CloneConfig {
    pub style: String,
    pub model: String,
    pub char_description: String,
    pub custom_dialogue: String,
    pub bg_description: String,
    pub auto_magic: bool,
    pub visual_similarity: i64,
}

impl From<&Project> for CloneConfig {
    fn from(p: &Project) -> Self {
        CloneConfig {
            style: p.style.clone(),
            model: p.model.clone(),
            char_description: p.char_description.clone(),
            custom_dialogue: p.custom_dialogue.clone(),
            bg_description: p.bg_description.clone(),
            auto_magic: p.auto_magic,
            visual_similarity: p.visual_similarity,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Scene {
    pub id: i64,
    pub project_id: i64,
    pub position: i64,
    pub scene_id: String,
    /// Parsed scene object. Stored as text, surfaced to clients as real JSON.
    pub json: Value,
    /// The analysis run that produced this scene; 0 means it came from a restore.
    pub job_id: i64,
    pub created_at: String,
}

fn scene_from_row(r: &Row) -> rusqlite::Result<Scene> {
    let raw: String = r.get("json")?;
    Ok(Scene {
        id: r.get("id")?,
        project_id: r.get("project_id")?,
        position: r.get("position")?,
        scene_id: r.get("scene_id")?,
        json: serde_json::from_str(&raw).unwrap_or(Value::Null),
        job_id: r.get("job_id")?,
        created_at: r.get("created_at")?,
    })
}

/// A restore point: metadata only. The scene payload is fetched separately so
/// listing history never drags whole projects into memory.
#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub id: i64,
    pub project_id: i64,
    pub reason: String,
    pub label: String,
    pub scene_count: i64,
    pub created_at: String,
}

fn snapshot_from_row(r: &Row) -> rusqlite::Result<Snapshot> {
    Ok(Snapshot {
        id: r.get("id")?,
        project_id: r.get("project_id")?,
        reason: r.get("reason")?,
        label: r.get("label")?,
        scene_count: r.get("scene_count")?,
        created_at: r.get("created_at")?,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct Job {
    pub id: i64,
    pub project_id: i64,
    pub kind: String,
    pub status: String,
    pub from_scene: i64,
    pub scenes_added: i64,
    pub model: String,
    pub temperature: f64,
    pub error: String,
    pub created_at: String,
    pub updated_at: String,
}

fn job_from_row(r: &Row) -> rusqlite::Result<Job> {
    Ok(Job {
        id: r.get("id")?,
        project_id: r.get("project_id")?,
        kind: r.get("kind")?,
        status: r.get("status")?,
        from_scene: r.get("from_scene")?,
        scenes_added: r.get("scenes_added")?,
        model: r.get("model")?,
        temperature: r.get("temperature")?,
        error: r.get("error")?,
        created_at: r.get("created_at")?,
        updated_at: r.get("updated_at")?,
    })
}

/// Bring a database created by an older build up to the current schema.
///
/// `CREATE TABLE IF NOT EXISTS` silently leaves an existing table at its old
/// shape, so columns added later have to be patched in explicitly or every
/// query naming them fails on an upgraded install.
fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    for (table, column, ddl) in [(
        "scenes",
        "job_id",
        "ALTER TABLE scenes ADD COLUMN job_id INTEGER NOT NULL DEFAULT 0",
    )] {
        if !has_column(conn, table, column)? {
            conn.execute_batch(ddl)?;
        }
    }
    Ok(())
}

fn has_column(conn: &Connection, table: &str, column: &str) -> rusqlite::Result<bool> {
    let mut st = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = st.query([])?;
    while let Some(r) = rows.next()? {
        if r.get::<_, String>(1)? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

/// How many restore points to keep per project.
///
/// Each snapshot holds a full copy of the scene list, so this is bounded on
/// purpose; the oldest are pruned as new ones arrive.
const SNAPSHOT_KEEP: i64 = 20;

pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    pub fn open(path: &str) -> Result<Self> {
        Self::init(Connection::open(path)?)
    }

    #[cfg(test)]
    pub fn open_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "busy_timeout", 5000)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA)?;
        migrate(&conn)?;
        Ok(Db {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

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

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
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

    /// The API key actually used for a request: the one saved in Settings wins,
    /// falling back to the process environment.
    pub fn gemini_api_key(&self) -> String {
        let stored = self.setting("gemini_api_key", "");
        if !stored.trim().is_empty() {
            return stored.trim().to_string();
        }
        crate::config::env_gemini_api_key()
    }

    // ---- projects ----

    #[allow(clippy::too_many_arguments)]
    pub fn create_project(
        &self,
        name: &str,
        video_path: &str,
        video_mime: &str,
        video_size: i64,
        video_filename: &str,
        cfg: &CloneConfig,
    ) -> Result<i64> {
        let ts = now();
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO projects
                   (name, video_path, video_mime, video_size, video_filename,
                    style, model, char_description, custom_dialogue, bg_description,
                    auto_magic, visual_similarity, created_at, updated_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?13)",
                params![
                    name,
                    video_path,
                    video_mime,
                    video_size,
                    video_filename,
                    cfg.style,
                    cfg.model,
                    cfg.char_description,
                    cfg.custom_dialogue,
                    cfg.bg_description,
                    cfg.auto_magic as i64,
                    cfg.visual_similarity,
                    ts,
                ],
            )?;
            Ok(c.last_insert_rowid())
        })
    }

    pub fn project(&self, id: i64) -> Result<Option<Project>> {
        self.with_conn(|c| {
            c.query_row("SELECT * FROM projects WHERE id = ?1", params![id], |r| {
                project_from_row(r)
            })
            .optional()
        })
    }

    pub fn list_projects(&self) -> Result<Vec<Project>> {
        self.with_conn(|c| {
            let mut st = c.prepare("SELECT * FROM projects ORDER BY id DESC")?;
            let rows = st.query_map([], project_from_row)?;
            rows.collect()
        })
    }

    pub fn update_project_config(&self, id: i64, cfg: &CloneConfig) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "UPDATE projects SET style=?2, model=?3, char_description=?4,
                    custom_dialogue=?5, bg_description=?6, auto_magic=?7,
                    visual_similarity=?8, updated_at=?9
                 WHERE id=?1",
                params![
                    id,
                    cfg.style,
                    cfg.model,
                    cfg.char_description,
                    cfg.custom_dialogue,
                    cfg.bg_description,
                    cfg.auto_magic as i64,
                    cfg.visual_similarity,
                    now(),
                ],
            )
        })?;
        Ok(())
    }

    pub fn set_project_name(&self, id: i64, name: &str) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "UPDATE projects SET name=?2, updated_at=?3 WHERE id=?1",
                params![id, name, now()],
            )
        })?;
        Ok(())
    }

    pub fn set_char_image(&self, id: i64, path: &str, mime: &str) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "UPDATE projects SET char_image_path=?2, char_image_mime=?3, updated_at=?4 WHERE id=?1",
                params![id, path, mime, now()],
            )
        })?;
        Ok(())
    }

    pub fn set_file_uri(&self, id: i64, uri: &str) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "UPDATE projects SET file_uri=?2, file_uri_at=?3, updated_at=?3 WHERE id=?1",
                params![id, uri, now()],
            )
        })?;
        Ok(())
    }

    pub fn delete_project(&self, id: i64) -> Result<()> {
        self.with_conn(|c| c.execute("DELETE FROM projects WHERE id = ?1", params![id]))?;
        Ok(())
    }

    // ---- scenes ----

    pub fn scenes(&self, project_id: i64) -> Result<Vec<Scene>> {
        self.with_conn(|c| {
            let mut st =
                c.prepare("SELECT * FROM scenes WHERE project_id = ?1 ORDER BY position, id")?;
            let rows = st.query_map(params![project_id], scene_from_row)?;
            rows.collect()
        })
    }

    pub fn scene_count(&self, project_id: i64) -> Result<i64> {
        self.with_conn(|c| {
            c.query_row(
                "SELECT COUNT(*) FROM scenes WHERE project_id = ?1",
                params![project_id],
                |r| r.get(0),
            )
        })
    }

    /// Append scenes at the end of a project's list, tagged with the run that
    /// produced them.
    pub fn append_scenes(&self, project_id: i64, scenes: &[Value], job_id: i64) -> Result<usize> {
        if scenes.is_empty() {
            return Ok(0);
        }
        let ts = now();
        self.with_conn(|c| {
            let next: i64 = c
                .query_row(
                    "SELECT COALESCE(MAX(position), -1) + 1 FROM scenes WHERE project_id = ?1",
                    params![project_id],
                    |r| r.get(0),
                )
                .unwrap_or(0);
            let mut st = c.prepare(
                "INSERT INTO scenes (project_id, position, scene_id, json, job_id, created_at)
                 VALUES (?1,?2,?3,?4,?5,?6)",
            )?;
            for (i, s) in scenes.iter().enumerate() {
                st.execute(params![
                    project_id,
                    next + i as i64,
                    scene_key(s),
                    s.to_string(),
                    job_id,
                    ts
                ])?;
            }
            Ok(scenes.len())
        })
    }

    /// Drop the last scene — used by "regenerate the last segment", which
    /// re-runs the model from the segment before it and replaces the tail.
    pub fn delete_last_scene(&self, project_id: i64) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "DELETE FROM scenes WHERE id = (
                     SELECT id FROM scenes WHERE project_id = ?1
                     ORDER BY position DESC, id DESC LIMIT 1
                 )",
                params![project_id],
            )
        })?;
        Ok(())
    }

    pub fn clear_scenes(&self, project_id: i64) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "DELETE FROM scenes WHERE project_id = ?1",
                params![project_id],
            )
        })?;
        Ok(())
    }

    /// Replace the whole scene list in one transaction.
    ///
    /// Each entry is `(job_id, scene)` so a bulk edit keeps pointing at the run
    /// that originally produced each segment; a restore passes 0.
    pub fn replace_all_scenes(&self, project_id: i64, entries: &[(i64, Value)]) -> Result<()> {
        let ts = now();
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM scenes WHERE project_id = ?1",
            params![project_id],
        )?;
        {
            let mut st = tx.prepare(
                "INSERT INTO scenes (project_id, position, scene_id, json, job_id, created_at)
                 VALUES (?1,?2,?3,?4,?5,?6)",
            )?;
            for (i, (job_id, s)) in entries.iter().enumerate() {
                st.execute(params![
                    project_id,
                    i as i64,
                    scene_key(s),
                    s.to_string(),
                    job_id,
                    ts
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    // ---- history: snapshots ----

    /// Capture the project's current scene list as a restore point.
    ///
    /// Returns `None` when there is nothing to protect — snapshotting an empty
    /// project would bury the useful restore points under noise.
    pub fn snapshot(&self, project_id: i64, reason: &str, label: &str) -> Result<Option<i64>> {
        let scenes = self.scenes(project_id)?;
        if scenes.is_empty() {
            return Ok(None);
        }
        let payload: Vec<Value> = scenes.iter().map(|s| s.json.clone()).collect();
        let count = payload.len() as i64;
        let body = Value::Array(payload).to_string();
        let ts = now();

        let id = self.with_conn(|c| {
            c.execute(
                "INSERT INTO snapshots (project_id, reason, label, scene_count, scenes, created_at)
                 VALUES (?1,?2,?3,?4,?5,?6)",
                params![project_id, reason, label, count, body, ts],
            )?;
            Ok(c.last_insert_rowid())
        })?;

        self.with_conn(|c| {
            c.execute(
                "DELETE FROM snapshots WHERE project_id = ?1 AND id NOT IN (
                     SELECT id FROM snapshots WHERE project_id = ?1
                     ORDER BY id DESC LIMIT ?2
                 )",
                params![project_id, SNAPSHOT_KEEP],
            )
        })?;

        Ok(Some(id))
    }

    pub fn list_snapshots(&self, project_id: i64) -> Result<Vec<Snapshot>> {
        self.with_conn(|c| {
            let mut st = c.prepare(
                "SELECT id, project_id, reason, label, scene_count, created_at
                 FROM snapshots WHERE project_id = ?1 ORDER BY id DESC",
            )?;
            let rows = st.query_map(params![project_id], snapshot_from_row)?;
            rows.collect()
        })
    }

    pub fn snapshot_meta(&self, id: i64) -> Result<Option<Snapshot>> {
        self.with_conn(|c| {
            c.query_row(
                "SELECT id, project_id, reason, label, scene_count, created_at
                 FROM snapshots WHERE id = ?1",
                params![id],
                snapshot_from_row,
            )
            .optional()
        })
    }

    pub fn snapshot_scenes(&self, id: i64) -> Result<Option<Vec<Value>>> {
        let raw: Option<String> = self.with_conn(|c| {
            c.query_row(
                "SELECT scenes FROM snapshots WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .optional()
        })?;
        Ok(raw.map(|s| match serde_json::from_str::<Value>(&s) {
            Ok(Value::Array(a)) => a,
            _ => Vec::new(),
        }))
    }

    // ---- history: jobs ----

    pub fn list_jobs(&self, project_id: i64, limit: i64) -> Result<Vec<Job>> {
        self.with_conn(|c| {
            let mut st =
                c.prepare("SELECT * FROM jobs WHERE project_id = ?1 ORDER BY id DESC LIMIT ?2")?;
            let rows = st.query_map(params![project_id, limit], job_from_row)?;
            rows.collect()
        })
    }

    /// The untruncated model response for a run, kept so a parse failure can be
    /// diagnosed after the fact.
    pub fn job_raw(&self, id: i64) -> Result<Option<String>> {
        self.with_conn(|c| {
            c.query_row("SELECT raw FROM jobs WHERE id = ?1", params![id], |r| {
                r.get(0)
            })
            .optional()
        })
    }

    // ---- jobs ----

    pub fn create_job(
        &self,
        project_id: i64,
        kind: &str,
        from_scene: i64,
        model: &str,
        temperature: f64,
    ) -> Result<i64> {
        let ts = now();
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO jobs (project_id, kind, status, from_scene, model, temperature, created_at, updated_at)
                 VALUES (?1,?2,'queued',?3,?4,?5,?6,?6)",
                params![project_id, kind, from_scene, model, temperature, ts],
            )?;
            Ok(c.last_insert_rowid())
        })
    }

    pub fn job(&self, id: i64) -> Result<Option<Job>> {
        self.with_conn(|c| {
            c.query_row(
                "SELECT * FROM jobs WHERE id = ?1",
                params![id],
                job_from_row,
            )
            .optional()
        })
    }

    pub fn latest_job(&self, project_id: i64) -> Result<Option<Job>> {
        self.with_conn(|c| {
            c.query_row(
                "SELECT * FROM jobs WHERE project_id = ?1 ORDER BY id DESC LIMIT 1",
                params![project_id],
                job_from_row,
            )
            .optional()
        })
    }

    pub fn set_job_status(&self, id: i64, status: &str) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "UPDATE jobs SET status=?2, updated_at=?3 WHERE id=?1",
                params![id, status, now()],
            )
        })?;
        Ok(())
    }

    pub fn finish_job(&self, id: i64, added: usize, raw: &str) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "UPDATE jobs SET status='completed', scenes_added=?2, raw=?3, updated_at=?4 WHERE id=?1",
                params![id, added as i64, raw, now()],
            )
        })?;
        Ok(())
    }

    pub fn fail_job(&self, id: i64, error: &str) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "UPDATE jobs SET status='failed', error=?2, updated_at=?3 WHERE id=?1",
                params![id, error, now()],
            )
        })?;
        Ok(())
    }

    /// Jobs left `queued`/`processing` by a crash can never make progress —
    /// the worker task that owned them died with the process. Mark them failed
    /// at boot so the UI does not show a spinner forever.
    pub fn reconcile_orphans(&self) -> Result<usize> {
        self.with_conn(|c| {
            c.execute(
                "UPDATE jobs SET status='failed',
                    error='tiến trình bị dừng khi app khởi động lại',
                    updated_at=?1
                 WHERE status IN ('queued','processing')",
                params![now()],
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn db() -> Db {
        Db::open_memory().unwrap()
    }

    fn make_project(db: &Db) -> i64 {
        db.create_project(
            "test",
            "/tmp/v.mp4",
            "video/mp4",
            100,
            "v.mp4",
            &CloneConfig {
                style: "Original".into(),
                model: "gemini-3-flash-preview".into(),
                visual_similarity: 100,
                ..Default::default()
            },
        )
        .unwrap()
    }

    #[test]
    fn append_scenes_keeps_order_across_calls() {
        let db = db();
        let p = make_project(&db);
        db.append_scenes(p, &[json!({"scene_id":"1"}), json!({"scene_id":"2"})], 7)
            .unwrap();
        db.append_scenes(p, &[json!({"scene_id":"3"})], 8).unwrap();

        let scenes = db.scenes(p).unwrap();
        let ids: Vec<_> = scenes.iter().map(|s| s.scene_id.as_str()).collect();
        assert_eq!(ids, vec!["1", "2", "3"]);
        assert_eq!(scenes[2].position, 2);
    }

    #[test]
    fn delete_last_scene_removes_only_the_tail() {
        let db = db();
        let p = make_project(&db);
        db.append_scenes(
            p,
            &[
                json!({"scene_id":"1"}),
                json!({"scene_id":"2"}),
                json!({"scene_id":"3"}),
            ],
            1,
        )
        .unwrap();
        db.delete_last_scene(p).unwrap();

        let ids: Vec<_> = db
            .scenes(p)
            .unwrap()
            .iter()
            .map(|s| s.scene_id.clone())
            .collect();
        assert_eq!(ids, vec!["1", "2"]);
    }

    #[test]
    fn replace_all_scenes_renumbers_from_zero() {
        let db = db();
        let p = make_project(&db);
        db.append_scenes(p, &[json!({"scene_id":"1"}), json!({"scene_id":"2"})], 1)
            .unwrap();
        db.replace_all_scenes(p, &[(1, json!({"scene_id":"9"}))])
            .unwrap();

        let scenes = db.scenes(p).unwrap();
        assert_eq!(scenes.len(), 1);
        assert_eq!(scenes[0].position, 0);
        assert_eq!(scenes[0].scene_id, "9");
    }

    #[test]
    fn settings_key_falls_back_to_default_when_blank() {
        let db = db();
        assert_eq!(db.setting("gemini_api_key", "none"), "none");
        db.set_setting("gemini_api_key", "abc").unwrap();
        assert_eq!(db.setting("gemini_api_key", "none"), "abc");
    }

    #[test]
    fn reconcile_orphans_fails_stuck_jobs() {
        let db = db();
        let p = make_project(&db);
        let j = db.create_job(p, "start", 0, "m", 0.1).unwrap();
        db.set_job_status(j, "processing").unwrap();

        assert_eq!(db.reconcile_orphans().unwrap(), 1);
        assert_eq!(db.job(j).unwrap().unwrap().status, "failed");
    }

    #[test]
    fn a_snapshot_captures_the_scene_list_and_survives_a_wipe() {
        let db = db();
        let p = make_project(&db);
        db.append_scenes(p, &[json!({"scene_id":"1"}), json!({"scene_id":"2"})], 1)
            .unwrap();

        let snap = db
            .snapshot(p, "analyze_start", "trước khi chạy lại")
            .unwrap()
            .unwrap();
        db.clear_scenes(p).unwrap();
        assert_eq!(db.scenes(p).unwrap().len(), 0);

        let recovered = db.snapshot_scenes(snap).unwrap().unwrap();
        assert_eq!(recovered.len(), 2);
        assert_eq!(recovered[1]["scene_id"], "2");
    }

    #[test]
    fn snapshotting_an_empty_project_is_skipped() {
        let db = db();
        let p = make_project(&db);
        assert!(db.snapshot(p, "replace", "").unwrap().is_none());
        assert_eq!(db.list_snapshots(p).unwrap().len(), 0);
    }

    #[test]
    fn snapshots_are_pruned_to_the_cap() {
        let db = db();
        let p = make_project(&db);
        db.append_scenes(p, &[json!({"scene_id":"1"})], 1).unwrap();
        for i in 0..(SNAPSHOT_KEEP + 5) {
            db.snapshot(p, "replace", &format!("lần {i}")).unwrap();
        }
        let kept = db.list_snapshots(p).unwrap();
        assert_eq!(kept.len() as i64, SNAPSHOT_KEEP);
        // Newest first, and the oldest ones are the ones dropped.
        assert_eq!(kept[0].label, format!("lần {}", SNAPSHOT_KEEP + 4));
    }

    #[test]
    fn scenes_remember_which_run_produced_them() {
        let db = db();
        let p = make_project(&db);
        db.append_scenes(p, &[json!({"scene_id":"1"})], 11).unwrap();
        db.append_scenes(p, &[json!({"scene_id":"2"})], 12).unwrap();

        let jobs: Vec<i64> = db.scenes(p).unwrap().iter().map(|s| s.job_id).collect();
        assert_eq!(jobs, vec![11, 12]);
    }

    #[test]
    fn a_bulk_edit_keeps_each_scene_pointing_at_its_run() {
        let db = db();
        let p = make_project(&db);
        db.append_scenes(p, &[json!({"scene_id":"1"})], 11).unwrap();
        db.append_scenes(p, &[json!({"scene_id":"2"})], 12).unwrap();

        let edited = vec![
            (11, json!({"scene_id":"1","x":1})),
            (12, json!({"scene_id":"2","x":1})),
        ];
        db.replace_all_scenes(p, &edited).unwrap();

        let jobs: Vec<i64> = db.scenes(p).unwrap().iter().map(|s| s.job_id).collect();
        assert_eq!(jobs, vec![11, 12]);
    }

    #[test]
    fn full_raw_output_is_stored_untruncated() {
        let db = db();
        let p = make_project(&db);
        let j = db.create_job(p, "start", 0, "m", 0.1).unwrap();
        let long = "x".repeat(50_000);
        db.finish_job(j, 0, &long).unwrap();

        assert_eq!(db.job_raw(j).unwrap().unwrap().len(), 50_000);
    }

    #[test]
    fn job_history_is_newest_first() {
        let db = db();
        let p = make_project(&db);
        let a = db.create_job(p, "start", 0, "m", 0.1).unwrap();
        let b = db.create_job(p, "continue", 1, "m", 0.1).unwrap();

        let ids: Vec<i64> = db.list_jobs(p, 10).unwrap().iter().map(|j| j.id).collect();
        assert_eq!(ids, vec![b, a]);
    }

    #[test]
    fn deleting_a_project_cascades_to_snapshots() {
        let db = db();
        let p = make_project(&db);
        db.append_scenes(p, &[json!({"scene_id":"1"})], 1).unwrap();
        db.snapshot(p, "replace", "x").unwrap();
        db.delete_project(p).unwrap();
        assert_eq!(db.list_snapshots(p).unwrap().len(), 0);
    }

    #[test]
    fn migrate_adds_job_id_to_a_pre_existing_scenes_table() {
        // A database shaped like the previous release: no job_id column.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE scenes (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 project_id INTEGER NOT NULL,
                 position INTEGER NOT NULL,
                 scene_id TEXT NOT NULL DEFAULT '',
                 json TEXT NOT NULL,
                 created_at TEXT NOT NULL
             );
             INSERT INTO scenes (project_id, position, scene_id, json, created_at)
             VALUES (1, 0, '1', '{\"scene_id\":\"1\"}', '2026-01-01T00:00:00Z');",
        )
        .unwrap();

        assert!(!has_column(&conn, "scenes", "job_id").unwrap());
        let db = Db::init(conn).unwrap();
        assert_eq!(db.scenes(1).unwrap().len(), 1, "existing rows must survive");
        assert_eq!(db.scenes(1).unwrap()[0].job_id, 0);
    }

    #[test]
    fn deleting_a_project_cascades_to_scenes() {
        let db = db();
        let p = make_project(&db);
        db.append_scenes(p, &[json!({"scene_id":"1"})], 1).unwrap();
        db.delete_project(p).unwrap();
        assert_eq!(db.scenes(p).unwrap().len(), 0);
    }
}
