//! SQLite layer — port of the Go backend's `internal/repo`. Single serialized
//! connection (the Go side ran with SetMaxOpenConns(1)), WAL mode, foreign keys
//! OFF (schema declares FKs but they were never enforced). Rows travel as
//! `serde_json::Map` like the Go `map[string]any` layer, with per-table column
//! allowlists so client JSON can never inject a column name.

use rusqlite::types::ValueRef;
use rusqlite::Connection;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

pub type Row = Map<String, Value>;

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

pub fn now() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// The schema is the Go `schema.sql` with every `ensureColumns` ALTER already
/// folded in (scene_environment / narrative_context / action_sequence on scene;
/// original_url / width_px / height_px on media). Fresh DBs get it in one shot;
/// existing Flow Kit DBs are upgraded by `ensure_columns`.
const SCHEMA: &str = include_str!("schema.sql");

impl Db {
    pub fn open(path: &str) -> anyhow::Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "busy_timeout", 5000)?;
        conn.pragma_update(None, "foreign_keys", "OFF")?;
        let db = Db {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.migrate()?;
        Ok(db)
    }

    pub fn open_memory() -> anyhow::Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Db {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(SCHEMA)?;
        // Upgrade path for DBs created by the Go backend (pre-fold columns).
        ensure_columns(
            &conn,
            "scene",
            &[
                ("scene_environment", "TEXT"),
                ("narrative_context", "TEXT"),
                ("action_sequence", "TEXT"),
                // Narration synthesized by the SenClaw TTS subsystem. Not in the Go
                // schema — the Go audio agent only collected narrator_text.
                ("narrator_audio_url", "TEXT"),
                ("narrator_audio_media_id", "TEXT"),
                ("narrator_audio_status", "TEXT NOT NULL DEFAULT 'PENDING'"),
            ],
        );
        ensure_columns(
            &conn,
            "media",
            &[
                ("original_url", "TEXT"),
                ("width_px", "INTEGER"),
                ("height_px", "INTEGER"),
            ],
        );
        ensure_columns(
            &conn,
            "material",
            &[("is_builtin", "INTEGER NOT NULL DEFAULT 0")],
        );
        // Compact invariant appearance ("35yo, short black hair, blue shirt…") woven
        // into every scene prompt so a character stays consistent across scenes.
        ensure_columns(
            &conn,
            "character",
            &[("appearance_tags", "TEXT NOT NULL DEFAULT ''")],
        );
        ensure_columns(
            &conn,
            "skill_agent",
            &[("skill_ids", "TEXT NOT NULL DEFAULT '[]'")],
        );
        ensure_columns(
            &conn,
            "dag_tasks",
            &[("input_from", "TEXT NOT NULL DEFAULT '[]'")],
        );
        ensure_columns(
            &conn,
            "request",
            &[
                ("next_retry_at", "TEXT"),
                ("edit_prompt", "TEXT"),
                ("source_media_id", "TEXT"),
            ],
        );
        Ok(())
    }

    /// Run `f` with the (locked) connection.
    pub fn with_conn<T>(
        &self,
        f: impl FnOnce(&Connection) -> rusqlite::Result<T>,
    ) -> anyhow::Result<T> {
        let conn = self.conn.lock().unwrap();
        Ok(f(&conn)?)
    }

    // ---- dynamic row CRUD (allowlisted columns) ----

    /// Insert-or-replace `data` into `table`, keeping only allowlisted columns.
    /// Fills `id`/`created_at`/`updated_at` when missing. Returns the row id.
    pub fn insert(&self, table: &str, data: &Row) -> anyhow::Result<String> {
        let cols = table_columns(table).ok_or_else(|| anyhow::anyhow!("unknown table {table}"))?;
        let mut m: Row = data
            .iter()
            .filter(|(k, _)| cols.contains(&k.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let id = match m.get("id").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => new_id(),
        };
        m.insert("id".into(), json!(id));
        if cols.contains(&"created_at") && !m.contains_key("created_at") {
            m.insert("created_at".into(), json!(now()));
        }
        if cols.contains(&"updated_at") {
            m.insert("updated_at".into(), json!(now()));
        }
        let keys: Vec<&String> = m.keys().collect();
        let placeholders: Vec<String> = (1..=keys.len()).map(|i| format!("?{i}")).collect();
        let sql = format!(
            "INSERT OR REPLACE INTO {table} ({}) VALUES ({})",
            keys.iter()
                .map(|k| k.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            placeholders.join(", ")
        );
        let params: Vec<Box<dyn rusqlite::ToSql>> = m.values().map(json_to_sql).collect();
        self.with_conn(|c| {
            c.execute(
                &sql,
                rusqlite::params_from_iter(params.iter().map(|b| b.as_ref())),
            )
        })?;
        Ok(id)
    }

    /// Patch allowlisted columns of one row. No-op when nothing matches.
    pub fn update(&self, table: &str, id: &str, data: &Row) -> anyhow::Result<()> {
        let cols = table_columns(table).ok_or_else(|| anyhow::anyhow!("unknown table {table}"))?;
        let mut m: Row = data
            .iter()
            .filter(|(k, _)| {
                cols.contains(&k.as_str()) && k.as_str() != "id" && k.as_str() != "created_at"
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        if m.is_empty() {
            return Ok(());
        }
        if cols.contains(&"updated_at") {
            m.insert("updated_at".into(), json!(now()));
        }
        let sets: Vec<String> = m
            .keys()
            .enumerate()
            .map(|(i, k)| format!("{k} = ?{}", i + 1))
            .collect();
        let sql = format!(
            "UPDATE {table} SET {} WHERE id = ?{}",
            sets.join(", "),
            m.len() + 1
        );
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = m.values().map(json_to_sql).collect();
        params.push(Box::new(id.to_string()));
        self.with_conn(|c| {
            c.execute(
                &sql,
                rusqlite::params_from_iter(params.iter().map(|b| b.as_ref())),
            )
        })?;
        Ok(())
    }

    pub fn get(&self, table: &str, id: &str) -> anyhow::Result<Option<Row>> {
        table_columns(table).ok_or_else(|| anyhow::anyhow!("unknown table {table}"))?;
        let sql = format!("SELECT * FROM {table} WHERE id = ?1");
        self.query_one(&sql, &[&id])
    }

    pub fn delete(&self, table: &str, id: &str) -> anyhow::Result<usize> {
        table_columns(table).ok_or_else(|| anyhow::anyhow!("unknown table {table}"))?;
        let sql = format!("DELETE FROM {table} WHERE id = ?1");
        Ok(self.with_conn(|c| c.execute(&sql, [&id]))?)
    }

    /// Query returning JSON rows (every column, dynamically typed).
    pub fn query(&self, sql: &str, params: &[&dyn rusqlite::ToSql]) -> anyhow::Result<Vec<Row>> {
        self.with_conn(|c| {
            let mut stmt = c.prepare(sql)?;
            let names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
            let mut rows = stmt.query(params)?;
            let mut out = Vec::new();
            while let Some(r) = rows.next()? {
                out.push(row_to_json(r, &names));
            }
            Ok(out)
        })
    }

    pub fn query_one(
        &self,
        sql: &str,
        params: &[&dyn rusqlite::ToSql],
    ) -> anyhow::Result<Option<Row>> {
        Ok(self.query(sql, params)?.into_iter().next())
    }

    pub fn execute(&self, sql: &str, params: &[&dyn rusqlite::ToSql]) -> anyhow::Result<usize> {
        self.with_conn(|c| c.execute(sql, params))
    }

    // ---- app_kv ----

    pub fn kv_get(&self, k: &str) -> String {
        self.query_one("SELECT v FROM app_kv WHERE k = ?1", &[&k])
            .ok()
            .flatten()
            .and_then(|r| r.get("v").and_then(|v| v.as_str().map(|s| s.to_string())))
            .unwrap_or_default()
    }

    pub fn kv_set(&self, k: &str, v: &str) -> anyhow::Result<()> {
        self.execute(
            "INSERT OR REPLACE INTO app_kv (k, v, updated_at) VALUES (?1, ?2, ?3)",
            &[&k, &v, &now()],
        )?;
        Ok(())
    }

    pub fn builtin_agent_disabled(&self, agent_type: &str) -> bool {
        self.kv_get(&format!("builtin_agent_disabled:{agent_type}")) == "1"
    }

    // ---- cascades (port of repo cascade helpers) ----

    /// A fresh image invalidates the downstream video + upscale for that orientation.
    pub fn cascade_after_image(&self, scene_id: &str, orientation: &str) -> anyhow::Result<()> {
        let o = ori_prefix(orientation);
        let sql = format!(
            "UPDATE scene SET \
             {o}_video_url = NULL, {o}_video_media_id = NULL, {o}_video_status = 'PENDING', \
             {o}_upscale_url = NULL, {o}_upscale_media_id = NULL, {o}_upscale_status = 'PENDING', \
             updated_at = ?2 WHERE id = ?1"
        );
        self.execute(&sql, &[&scene_id, &now()])?;
        Ok(())
    }

    /// A fresh video invalidates the upscale for that orientation.
    pub fn cascade_after_video(&self, scene_id: &str, orientation: &str) -> anyhow::Result<()> {
        let o = ori_prefix(orientation);
        let sql = format!(
            "UPDATE scene SET \
             {o}_upscale_url = NULL, {o}_upscale_media_id = NULL, {o}_upscale_status = 'PENDING', \
             updated_at = ?2 WHERE id = ?1"
        );
        self.execute(&sql, &[&scene_id, &now()])?;
        Ok(())
    }

    /// Delete a pipeline and everything it produced (tx, port of DeletePipelineCascade).
    pub fn delete_pipeline_cascade(
        &self,
        pipeline_id: &str,
        project_id: &str,
    ) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        let tx_now = now();
        let _ = tx_now;
        conn.execute_batch("BEGIN")?;
        let r = (|| -> rusqlite::Result<()> {
            conn.execute("DELETE FROM dag_tasks WHERE parent_id = ?1", [pipeline_id])?;
            conn.execute("DELETE FROM dag_parents WHERE id = ?1", [pipeline_id])?;
            if !project_id.is_empty() {
                conn.execute("DELETE FROM request WHERE project_id = ?1", [project_id])?;
                conn.execute(
                    "DELETE FROM scene WHERE video_id IN (SELECT id FROM video WHERE project_id = ?1)",
                    [project_id],
                )?;
                conn.execute("DELETE FROM video WHERE project_id = ?1", [project_id])?;
                conn.execute(
                    "DELETE FROM project_character WHERE project_id = ?1",
                    [project_id],
                )?;
            }
            Ok(())
        })();
        match r {
            Ok(()) => conn.execute_batch("COMMIT")?,
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(e.into());
            }
        }
        Ok(())
    }
}

fn ensure_columns(conn: &Connection, table: &str, cols: &[(&str, &str)]) {
    for (name, ty) in cols {
        // Errors mean "duplicate column" — fine, the point is idempotence.
        let _ = conn.execute(&format!("ALTER TABLE {table} ADD COLUMN {name} {ty}"), []);
    }
}

fn json_to_sql(v: &Value) -> Box<dyn rusqlite::ToSql> {
    match v {
        Value::Null => Box::new(None::<String>),
        Value::Bool(b) => Box::new(*b as i64),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Box::new(i)
            } else {
                Box::new(n.as_f64().unwrap_or(0.0))
            }
        }
        Value::String(s) => Box::new(s.clone()),
        other => Box::new(other.to_string()), // arrays/objects stored as JSON text
    }
}

fn row_to_json(r: &rusqlite::Row<'_>, names: &[String]) -> Row {
    let mut m = Map::new();
    for (i, name) in names.iter().enumerate() {
        let v = match r.get_ref(i) {
            Ok(ValueRef::Null) => Value::Null,
            Ok(ValueRef::Integer(x)) => json!(x),
            Ok(ValueRef::Real(x)) => json!(x),
            Ok(ValueRef::Text(t)) => json!(String::from_utf8_lossy(t).to_string()),
            Ok(ValueRef::Blob(b)) => json!(format!("<{} bytes>", b.len())),
            Err(_) => Value::Null,
        };
        m.insert(name.clone(), v);
    }
    m
}

/// `VERTICAL` → `vertical`, everything else → `horizontal` guarded to the two
/// legal prefixes (never trust caller strings inside format!()-built SQL).
pub fn ori_prefix(orientation: &str) -> &'static str {
    if orientation.eq_ignore_ascii_case("horizontal") {
        "horizontal"
    } else {
        "vertical"
    }
}

/// Column-name bundle for one orientation (port of SceneColsFor).
pub struct SceneCols {
    pub image_url: String,
    pub image_media_id: String,
    pub image_status: String,
    pub video_url: String,
    pub video_media_id: String,
    pub video_status: String,
    pub upscale_url: String,
    pub upscale_media_id: String,
    pub upscale_status: String,
    pub end_scene_media_id: String,
}

pub fn scene_cols(orientation: &str) -> SceneCols {
    let o = ori_prefix(orientation);
    SceneCols {
        image_url: format!("{o}_image_url"),
        image_media_id: format!("{o}_image_media_id"),
        image_status: format!("{o}_image_status"),
        video_url: format!("{o}_video_url"),
        video_media_id: format!("{o}_video_media_id"),
        video_status: format!("{o}_video_status"),
        upscale_url: format!("{o}_upscale_url"),
        upscale_media_id: format!("{o}_upscale_media_id"),
        upscale_status: format!("{o}_upscale_status"),
        end_scene_media_id: format!("{o}_end_scene_media_id"),
    }
}

/// Allowlisted columns per table — the only names `insert`/`update` will emit.
pub fn table_columns(table: &str) -> Option<&'static [&'static str]> {
    static MAP: OnceLock<HashMap<&'static str, Vec<&'static str>>> = OnceLock::new();
    let map = MAP.get_or_init(|| {
        let mut m: HashMap<&'static str, Vec<&'static str>> = HashMap::new();
        m.insert(
            "project",
            vec![
                "id",
                "name",
                "description",
                "story",
                "story_original",
                "thumbnail_url",
                "language",
                "status",
                "user_paygate_tier",
                "narrator_voice",
                "narrator_ref_audio",
                "material",
                "allow_music",
                "allow_voice",
                "created_at",
                "updated_at",
            ],
        );
        m.insert(
            "character",
            vec![
                "id",
                "name",
                "slug",
                "entity_type",
                "description",
                "image_prompt",
                "voice_description",
                "reference_image_url",
                "media_id",
                "appearance_tags",
                "created_at",
                "updated_at",
            ],
        );
        m.insert("project_character", vec!["project_id", "character_id"]);
        m.insert(
            "video",
            vec![
                "id",
                "project_id",
                "title",
                "description",
                "display_order",
                "status",
                "vertical_url",
                "horizontal_url",
                "thumbnail_url",
                "duration",
                "resolution",
                "orientation",
                "youtube_id",
                "privacy",
                "tags",
                "created_at",
                "updated_at",
            ],
        );
        m.insert(
            "scene",
            vec![
                "id",
                "video_id",
                "display_order",
                "prompt",
                "image_prompt",
                "video_prompt",
                "camera_movement",
                "character_names",
                "parent_scene_id",
                "chain_type",
                "source",
                "vertical_image_url",
                "vertical_image_media_id",
                "vertical_image_status",
                "vertical_video_url",
                "vertical_video_media_id",
                "vertical_video_status",
                "vertical_upscale_url",
                "vertical_upscale_media_id",
                "vertical_upscale_status",
                "horizontal_image_url",
                "horizontal_image_media_id",
                "horizontal_image_status",
                "horizontal_video_url",
                "horizontal_video_media_id",
                "horizontal_video_status",
                "horizontal_upscale_url",
                "horizontal_upscale_media_id",
                "horizontal_upscale_status",
                "vertical_end_scene_media_id",
                "horizontal_end_scene_media_id",
                "trim_start",
                "trim_end",
                "duration",
                "transition_prompt",
                "narrator_text",
                "shot_type",
                "scene_environment",
                "narrative_context",
                "action_sequence",
                "narrator_audio_url",
                "narrator_audio_media_id",
                "narrator_audio_status",
                "created_at",
                "updated_at",
            ],
        );
        m.insert(
            "request",
            vec![
                "id",
                "project_id",
                "video_id",
                "scene_id",
                "character_id",
                "type",
                "orientation",
                "status",
                "request_id",
                "media_id",
                "output_url",
                "error_message",
                "retry_count",
                "next_retry_at",
                "edit_prompt",
                "source_media_id",
                "created_at",
                "updated_at",
            ],
        );
        m.insert(
            "material",
            vec![
                "id",
                "name",
                "style_instruction",
                "negative_prompt",
                "scene_prefix",
                "lighting",
                "is_builtin",
                "created_at",
            ],
        );
        m.insert(
            "pipe_skill_prompt",
            vec![
                "id",
                "slug",
                "title",
                "group_id",
                "group_title",
                "display_order",
                "description",
                "applies_to",
                "prompt_template",
                "is_active",
                "version",
                "created_at",
                "updated_at",
            ],
        );
        m.insert(
            "project_pipe_skill",
            vec![
                "project_id",
                "prompt_slug",
                "enabled",
                "display_order",
                "updated_at",
            ],
        );
        m.insert(
            "project_skill",
            vec!["project_id", "skill_slug", "enabled", "updated_at"],
        );
        m.insert(
            "skill_agent",
            vec![
                "id",
                "name",
                "skill_id",
                "skill_ids",
                "prompt",
                "enabled",
                "created_at",
                "updated_at",
            ],
        );
        m.insert(
            "dag_parents",
            vec![
                "id",
                "project_id",
                "status",
                "goal",
                "orientation",
                "script_md",
                "created_at",
                "updated_at",
            ],
        );
        m.insert(
            "dag_tasks",
            vec![
                "id",
                "parent_id",
                "label",
                "agent_type",
                "prompt",
                "depends_on",
                "input_from",
                "status",
                "result",
                "timeout_seconds",
                "started_at",
                "completed_at",
            ],
        );
        m.insert(
            "media",
            vec![
                "id",
                "file_name",
                "file_path",
                "mime_type",
                "size_bytes",
                "media_type",
                "original_url",
                "width_px",
                "height_px",
                "created_at",
            ],
        );
        m
    });
    map.get(table).map(|v| v.as_slice())
}

/// Trimmed string field (port of repo.Str).
pub fn str_of(m: &Row, k: &str) -> String {
    m.get(k)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string()
}

pub fn i64_of(m: &Row, k: &str) -> i64 {
    m.get(k)
        .map(|v| {
            v.as_i64()
                .unwrap_or_else(|| v.as_f64().unwrap_or(0.0) as i64)
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_get_update_roundtrip() {
        let db = Db::open_memory().unwrap();
        let mut m = Map::new();
        m.insert("name".into(), json!("Test"));
        m.insert("bogus_column".into(), json!("dropped"));
        let id = db.insert("project", &m).unwrap();
        let row = db.get("project", &id).unwrap().unwrap();
        assert_eq!(str_of(&row, "name"), "Test");
        assert_eq!(str_of(&row, "status"), "ACTIVE");
        assert!(row.get("bogus_column").is_none());

        let mut patch = Map::new();
        patch.insert("status".into(), json!("ARCHIVED"));
        db.update("project", &id, &patch).unwrap();
        let row = db.get("project", &id).unwrap().unwrap();
        assert_eq!(str_of(&row, "status"), "ARCHIVED");
    }

    #[test]
    fn cascade_after_image_resets_downstream() {
        let db = Db::open_memory().unwrap();
        let mut v = Map::new();
        v.insert("project_id".into(), json!("p1"));
        v.insert("title".into(), json!("v"));
        let vid = db.insert("video", &v).unwrap();
        let mut s = Map::new();
        s.insert("video_id".into(), json!(vid));
        s.insert("vertical_video_url".into(), json!("http://x/v.mp4"));
        s.insert("vertical_video_status".into(), json!("COMPLETED"));
        s.insert("vertical_upscale_status".into(), json!("COMPLETED"));
        let sid = db.insert("scene", &s).unwrap();
        db.cascade_after_image(&sid, "VERTICAL").unwrap();
        let row = db.get("scene", &sid).unwrap().unwrap();
        assert_eq!(str_of(&row, "vertical_video_status"), "PENDING");
        assert_eq!(str_of(&row, "vertical_upscale_status"), "PENDING");
        assert!(row.get("vertical_video_url").unwrap().is_null());
    }

    #[test]
    fn kv_roundtrip() {
        let db = Db::open_memory().unwrap();
        db.kv_set("llm.profile", "MyProfile").unwrap();
        assert_eq!(db.kv_get("llm.profile"), "MyProfile");
        assert_eq!(db.kv_get("missing"), "");
    }
}
