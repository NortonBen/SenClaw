//! HTTP API — port of `internal/api` (server.go + the full handlers.go route
//! surface). Paths are registered WITHOUT the `/api` prefix; main.rs nests this
//! router under `/api`. `/health` and `/status` both serve the health JSON
//! (manifest healthPath = `/api/status`).

use crate::db::{self, Row};
use crate::state::AppState;
use crate::{llm, material, media, pipeline, script, skillcat, souls};
use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, Path, Query, State, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::HashMap;

/// Routes the Go backend served at the server root, not under `/api`: the
/// dashboard WebSocket (the React app dials `/ws/dashboard`) and `/health`.
/// Merged at root by main.rs; the `/api/*` copies stay for callers using them.
pub fn root_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ws/dashboard", get(ws_dashboard))
        .with_state(state)
}

pub fn api_router(state: AppState) -> Router {
    Router::new()
        // Health (also /status: manifest healthPath is /api/status)
        .route("/health", get(health))
        .route("/status", get(health))
        // Dashboard WebSocket
        .route("/ws/dashboard", get(ws_dashboard))
        // Extension callback
        .route("/ext/callback", post(ext_callback))
        // Ensure a real Flow project exists (look up / create) and report it
        .route("/flow/ensure-project", post(ensure_flow_project))
        // Projects
        .route("/projects", get(list_projects).post(create_project))
        .route(
            "/projects/:pid",
            get(get_project)
                .patch(update_project)
                .put(update_project)
                .delete(delete_project),
        )
        .route("/projects/:pid/duplicate", post(duplicate_project))
        .route("/projects/:pid/clone-ai", post(clone_project_ai))
        .route("/projects/:pid/output-dir", get(project_output_dir))
        .route(
            "/projects/:pid/characters",
            get(list_project_characters).post(create_project_character),
        )
        .route("/projects/:pid/characters/:cid/link", post(link_character))
        .route(
            "/projects/:pid/characters/:cid/unlink",
            delete(unlink_character),
        )
        // Characters
        .route(
            "/characters/:cid",
            get(get_character)
                .patch(update_character)
                .delete(delete_character),
        )
        // Videos
        .route("/videos", get(list_videos).post(create_video))
        .route(
            "/videos/:vid",
            get(get_video).patch(update_video).delete(delete_video),
        )
        // Scenes
        .route("/scenes", get(list_scenes).post(create_scene))
        .route(
            "/scenes/:sid",
            get(get_scene).patch(update_scene).delete(delete_scene),
        )
        // Requests
        .route(
            "/requests",
            get(list_requests)
                .post(create_request)
                .delete(clear_requests),
        )
        .route("/requests/batch", post(batch_requests))
        .route("/requests/batch-status", get(batch_request_status))
        .route("/requests/pending", get(list_pending_requests))
        .route(
            "/requests/:rid",
            get(get_request)
                .patch(update_request)
                .delete(delete_request),
        )
        // Pipeline
        .route("/pipeline/create", post(create_pipeline))
        .route("/pipeline/project/:projectId", get(list_project_pipelines))
        .route("/pipeline/:pid", get(get_pipeline).delete(delete_pipeline))
        .route("/pipeline/:pid/start", post(start_pipeline))
        .route("/pipeline/:pid/pause", post(pause_pipeline))
        .route("/pipeline/:pid/cancel", post(cancel_pipeline))
        .route("/pipeline/:pid/task/:tid/retry", post(retry_task))
        .route("/pipeline/:pid/task/:tid/stop", post(stop_task))
        // Workflow-engine pipeline (one node per scene; runs on the daemon)
        .route("/pipeline/workflow", post(start_workflow))
        .route("/pipeline/custom-workflow", post(start_custom_workflow))
        .route("/pipeline/workflow/project/:pid", get(project_workflow_run))
        .route("/pipeline/workflow/:runId", get(get_workflow_run))
        .route(
            "/pipeline/workflow/:runId/cancel",
            post(cancel_workflow_run),
        )
        // Blocking step endpoints the workflow definition curls back into
        .route("/steps/agent", post(crate::steps::run_agent_step))
        .route("/steps/scene", post(crate::steps::run_scene_step))
        .route("/steps/entity", post(crate::steps::run_entity_step))
        .route("/steps/catchup", post(crate::steps::run_catchup_step))
        // Script parser (standalone)
        .route("/script/parse", post(parse_script))
        // Agent image generation (on-demand)
        .route("/agent/image", post(agent_generate_image))
        // Agents (built-in)
        .route("/agents", get(list_agents))
        .route("/agents/log", get(agent_log))
        .route(
            "/agents/history",
            get(agent_history).delete(clear_agent_history),
        )
        .route("/agents/:agentType/soul", put(put_agent_soul))
        .route("/agents/:agentType", patch(patch_builtin_agent))
        // Skill agents (user-created, DB-backed)
        .route(
            "/skill-agents",
            get(list_skill_agents).post(create_skill_agent),
        )
        .route(
            "/skill-agents/:id",
            put(update_skill_agent).delete(delete_skill_agent),
        )
        // AI suggestions
        .route("/ai/suggest-project", post(suggest_project))
        .route("/ai/suggest-scenes", post(suggest_scenes))
        .route("/ai/suggest-entities", post(suggest_entities))
        .route("/ai/providers", get(list_providers))
        // Settings
        .route("/settings/llm", get(get_llm_settings).put(put_llm_settings))
        .route("/settings/tools", get(list_tools))
        // Materials
        .route("/materials", get(list_materials).post(create_material))
        .route("/materials/seed", post(seed_materials))
        .route("/materials/restore", post(restore_materials))
        .route("/materials/import", post(import_materials))
        .route("/materials/:mid", delete(delete_material))
        // Skills (playbook catalog)
        .route("/skills", get(list_skills))
        // Media
        .route("/media", get(list_media))
        .route("/media/upload", post(media::upload_media))
        .route("/media/batch-delete", post(delete_media_batch))
        .route("/media/localize", post(localize_media))
        .route("/media/fetch-urls", post(fetch_media_urls))
        .route("/media/:mid", get(get_media).delete(delete_media))
        .route("/media/:mid/file", get(media::download_media))
        // MCP
        .route(
            "/mcp/sse",
            get(crate::mcp::mcp_sse).post(crate::mcp::mcp_message),
        )
        .route("/mcp/message", post(crate::mcp::mcp_message))
        .layer(DefaultBodyLimit::max(media::MAX_UPLOAD_BYTES))
        .with_state(state)
}

// ---------- helpers ----------

fn respond(code: StatusCode, v: Value) -> Response {
    (code, Json(v)).into_response()
}

fn err(code: StatusCode, msg: impl Into<String>) -> Response {
    (code, Json(json!({ "error": msg.into() }))).into_response()
}

fn err500(e: impl ToString) -> Response {
    err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn rows_json(rows: Vec<Row>) -> Value {
    Value::Array(rows.into_iter().map(Value::Object).collect())
}

fn row_or_null(row: Option<Row>) -> Value {
    row.map(Value::Object).unwrap_or(Value::Null)
}

type MaybeJson = Result<Json<Value>, JsonRejection>;

/// Decode a JSON object body (400 with `{"error"}` on failure, like the Go
/// `decodeBody` + `writeError` pair).
fn body_obj(body: MaybeJson) -> Result<Row, Response> {
    match body {
        Ok(Json(Value::Object(m))) => Ok(m),
        Ok(_) => Err(err(StatusCode::BAD_REQUEST, "invalid body")),
        Err(e) => Err(err(StatusCode::BAD_REQUEST, e.to_string())),
    }
}

fn parse_body<T: for<'de> Deserialize<'de>>(body: MaybeJson) -> Result<T, Response> {
    match body {
        Ok(Json(v)) => {
            serde_json::from_value(v).map_err(|e| err(StatusCode::BAD_REQUEST, e.to_string()))
        }
        Err(e) => Err(err(StatusCode::BAD_REQUEST, e.to_string())),
    }
}

fn slugify(s: &str) -> String {
    let mut b = String::new();
    for c in s.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            b.push(c);
        } else {
            b.push('-');
        }
    }
    b.trim_matches('-').to_string()
}

fn vstr(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(x) => x.to_string(),
    }
}

// ---------- health / ws / ext ----------

async fn health(State(st): State<AppState>) -> Response {
    respond(
        StatusCode::OK,
        json!({
            "status": "ok",
            "extension_connected": st.core.ext.is_connected(),
            "ws": st.core.ext.stats(),
            // The real, browsable Flow project the app generates into (empty
            // until the first generation looks one up / creates it).
            "flow_project": st.core.db.kv_get("flow.session_project"),
        }),
    )
}

/// Force a real Flow project to exist now (look up or create) and report it, so
/// the user can confirm/trigger it without generating first. Needs the
/// extension connected + a signed-in Flow session.
async fn ensure_flow_project(State(st): State<AppState>) -> Response {
    match crate::process::ensure_flow_project(&st.core).await {
        Ok(pid) => respond(
            StatusCode::OK,
            json!({
                "flow_project_id": pid,
                "url": format!("https://labs.google/fx/vi/tools/flow/project/{pid}"),
            }),
        ),
        Err(e) => err(StatusCode::BAD_GATEWAY, &e),
    }
}

async fn ws_dashboard(State(st): State<AppState>, ws: WebSocketUpgrade) -> Response {
    let dash = st.core.dash.clone();
    ws.on_upgrade(move |socket| async move { dash.serve(socket).await })
}

async fn ext_callback(State(st): State<AppState>, body: MaybeJson) -> Response {
    let payload = match body {
        Ok(Json(v @ Value::Object(_))) => v,
        _ => return err(StatusCode::BAD_REQUEST, "invalid body"),
    };
    let id = payload
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if id.is_empty() {
        return err(StatusCode::BAD_REQUEST, "missing id");
    }
    st.core.ext.complete_callback(&id, payload.clone());
    if let Some(t) = payload.get("type").and_then(|v| v.as_str()) {
        if !t.is_empty() {
            st.core.dash.emit(t, payload.clone());
        }
    }
    respond(StatusCode::OK, json!({ "status": "ok" }))
}

// ---------- projects ----------

async fn list_projects(
    State(st): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let status = q.get("status").cloned().unwrap_or_default();
    let r = if !status.is_empty() {
        st.core.db.query(
            "SELECT * FROM project WHERE status = ?1 ORDER BY created_at DESC",
            &[&status],
        )
    } else {
        st.core.db.query(
            "SELECT * FROM project WHERE status != 'DELETED' ORDER BY created_at DESC",
            &[],
        )
    };
    match r {
        Ok(rows) => respond(StatusCode::OK, rows_json(rows)),
        Err(e) => err500(e),
    }
}

async fn create_project(State(st): State<AppState>, body: MaybeJson) -> Response {
    let mut b = match body_obj(body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    if b.get("status").map(|v| v.is_null()).unwrap_or(true) {
        b.insert("status".into(), json!("ACTIVE"));
    }
    match st.core.db.insert("project", &b) {
        Ok(id) => match st.core.db.get("project", &id) {
            Ok(row) => respond(StatusCode::CREATED, row_or_null(row)),
            Err(e) => err500(e),
        },
        Err(e) => err500(e),
    }
}

async fn get_project(State(st): State<AppState>, Path(pid): Path<String>) -> Response {
    match st.core.db.get("project", &pid) {
        Ok(Some(row)) => respond(StatusCode::OK, Value::Object(row)),
        _ => err(StatusCode::NOT_FOUND, "project not found"),
    }
}

async fn update_project(
    State(st): State<AppState>,
    Path(pid): Path<String>,
    body: MaybeJson,
) -> Response {
    let mut b = match body_obj(body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    b.remove("id");
    if let Err(e) = st.core.db.update("project", &pid, &b) {
        return err500(e);
    }
    match st.core.db.get("project", &pid) {
        Ok(row) => respond(StatusCode::OK, row_or_null(row)),
        Err(e) => err500(e),
    }
}

async fn delete_project(State(st): State<AppState>, Path(pid): Path<String>) -> Response {
    match st.core.db.execute(
        "UPDATE project SET status = 'DELETED', updated_at = ?1 WHERE id = ?2",
        &[&db::now(), &pid],
    ) {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => err500(e),
    }
}

/// Port of cloneProjectBaseFields.
fn clone_project_base_fields(src: &Row) -> Row {
    let mut base_story = db::str_of(src, "story");
    if base_story.is_empty() {
        base_story = db::str_of(src, "story_original");
    }
    let mut base_material = db::str_of(src, "material");
    if base_material.is_empty() {
        base_material = "realistic".to_string();
    }
    let mut base_language = db::str_of(src, "language");
    if base_language.is_empty() {
        base_language = "vi".to_string();
    }
    let mut m = Map::new();
    m.insert("id".into(), json!(db::new_id()));
    m.insert(
        "name".into(),
        json!(format!("{} (Copy)", db::str_of(src, "name"))),
    );
    m.insert("description".into(), json!(db::str_of(src, "description")));
    m.insert("story".into(), json!(base_story));
    m.insert("story_original".into(), json!(base_story));
    m.insert(
        "thumbnail_url".into(),
        json!(db::str_of(src, "thumbnail_url")),
    );
    m.insert("language".into(), json!(base_language));
    m.insert("status".into(), json!("ACTIVE"));
    m.insert(
        "user_paygate_tier".into(),
        json!(db::str_of(src, "user_paygate_tier")),
    );
    m.insert(
        "narrator_voice".into(),
        json!(db::str_of(src, "narrator_voice")),
    );
    m.insert(
        "narrator_ref_audio".into(),
        json!(db::str_of(src, "narrator_ref_audio")),
    );
    m.insert("material".into(), json!(base_material));
    m.insert(
        "allow_music".into(),
        src.get("allow_music").cloned().unwrap_or(Value::Null),
    );
    m.insert(
        "allow_voice".into(),
        src.get("allow_voice").cloned().unwrap_or(Value::Null),
    );
    m
}

async fn duplicate_project(State(st): State<AppState>, Path(pid): Path<String>) -> Response {
    let src = match st.core.db.get("project", &pid) {
        Ok(Some(r)) => r,
        _ => return err(StatusCode::NOT_FOUND, "project not found"),
    };
    let mut fields = clone_project_base_fields(&src);
    if db::str_of(&src, "name").is_empty() {
        fields.insert("name".into(), json!("Untitled Project (Copy)"));
    }
    match st.core.db.insert("project", &fields) {
        Ok(id) => match st.core.db.get("project", &id) {
            Ok(row) => respond(StatusCode::CREATED, row_or_null(row)),
            Err(e) => err500(e),
        },
        Err(e) => err500(e),
    }
}

async fn clone_project_ai(
    State(st): State<AppState>,
    Path(pid): Path<String>,
    body: Option<Json<Value>>,
) -> Response {
    let src = match st.core.db.get("project", &pid) {
        Ok(Some(r)) => r,
        _ => return err(StatusCode::NOT_FOUND, "project not found"),
    };
    let extra_prompt = body
        .as_ref()
        .and_then(|Json(v)| v.get("prompt").and_then(|p| p.as_str()))
        .unwrap_or("")
        .trim()
        .to_string();

    let base_name = db::str_of(&src, "name");
    let mut base_story = db::str_of(&src, "story");
    if base_story.is_empty() {
        base_story = db::str_of(&src, "story_original");
    }
    let base_material = db::str_of(&src, "material");
    let base_language = db::str_of(&src, "language");

    let mut user_prompt = format!(
        "You are cloning a video project into a fresh variant.\n\
         Return ONLY one JSON object with fields: name, story, material, language.\n\
         Keep the same genre and core idea, but make it a distinct variation.\n\n\
         SOURCE PROJECT:\n\
         name: {base_name}\nstory: {base_story}\nmaterial: {base_material}\nlanguage: {base_language}\n"
    );
    if !extra_prompt.is_empty() {
        user_prompt.push_str(&format!("\nADDITIONAL INSTRUCTION:\n{extra_prompt}\n"));
    }

    let reply = match llm::complete("You are a creative producer.", &user_prompt, 4000).await {
        Ok((text, _)) => text,
        Err(e) => return err500(e),
    };

    let mut fields = clone_project_base_fields(&src);
    if let (Some(start), Some(end)) = (reply.find('{'), reply.rfind('}')) {
        if end > start {
            if let Ok(parsed) = serde_json::from_str::<Value>(&reply[start..=end]) {
                for (key, dual) in [
                    ("name", false),
                    ("story", true),
                    ("material", false),
                    ("language", false),
                ] {
                    let v = parsed
                        .get(key)
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    if !v.is_empty() {
                        fields.insert(key.into(), json!(v));
                        if dual {
                            fields.insert("story_original".into(), json!(v));
                        }
                    }
                }
            }
        }
    }
    if db::str_of(&fields, "name").is_empty() {
        fields.insert("name".into(), json!("Untitled Project (AI Clone)"));
    }
    match st.core.db.insert("project", &fields) {
        Ok(id) => match st.core.db.get("project", &id) {
            Ok(row) => respond(StatusCode::CREATED, row_or_null(row)),
            Err(e) => err500(e),
        },
        Err(e) => err500(e),
    }
}

async fn project_output_dir(State(st): State<AppState>, Path(pid): Path<String>) -> Response {
    let proj = match st.core.db.get("project", &pid) {
        Ok(Some(r)) => r,
        _ => return err(StatusCode::NOT_FOUND, "not found"),
    };
    let slug = slugify(&db::str_of(&proj, "name"));
    let out_dir = format!("output/{pid}-{slug}");
    let _ = std::fs::create_dir_all(&out_dir);
    respond(StatusCode::OK, json!({ "output_dir": out_dir }))
}

async fn list_project_characters(State(st): State<AppState>, Path(pid): Path<String>) -> Response {
    match st.core.db.query(
        "SELECT c.* FROM character c \
         JOIN project_character pc ON pc.character_id = c.id \
         WHERE pc.project_id = ?1 \
         ORDER BY c.name",
        &[&pid],
    ) {
        Ok(rows) => respond(StatusCode::OK, rows_json(rows)),
        Err(e) => err500(e),
    }
}

async fn link_character(
    State(st): State<AppState>,
    Path((pid, cid)): Path<(String, String)>,
) -> Response {
    match st.core.db.execute(
        "INSERT OR IGNORE INTO project_character (project_id, character_id) VALUES (?1, ?2)",
        &[&pid, &cid],
    ) {
        Ok(_) => respond(StatusCode::OK, json!({ "status": "linked" })),
        Err(e) => err500(e),
    }
}

async fn unlink_character(
    State(st): State<AppState>,
    Path((pid, cid)): Path<(String, String)>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let delete_row = q.get("delete_row").map(|v| v == "1").unwrap_or(false);
    let mut char_deleted = false;
    if delete_row {
        char_deleted = st.core.db.delete("character", &cid).is_ok();
    }
    if let Err(e) = st.core.db.execute(
        "DELETE FROM project_character WHERE project_id = ?1 AND character_id = ?2",
        &[&pid, &cid],
    ) {
        return err500(e);
    }
    respond(
        StatusCode::OK,
        json!({
            "ok": true,
            "character_deleted": char_deleted,
            "still_linked_to_other_projects": false,
        }),
    )
}

async fn create_project_character(
    State(st): State<AppState>,
    Path(pid): Path<String>,
    body: MaybeJson,
) -> Response {
    let mut b = match body_obj(body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    b.insert("id".into(), json!(db::new_id()));
    let cid = match st.core.db.insert("character", &b) {
        Ok(id) => id,
        Err(e) => return err500(e),
    };
    if let Err(e) = st.core.db.execute(
        "INSERT OR IGNORE INTO project_character (project_id, character_id) VALUES (?1, ?2)",
        &[&pid, &cid],
    ) {
        return err500(e);
    }
    match st.core.db.get("character", &cid) {
        Ok(row) => respond(StatusCode::CREATED, row_or_null(row)),
        Err(e) => err500(e),
    }
}

// ---------- characters ----------

async fn get_character(State(st): State<AppState>, Path(cid): Path<String>) -> Response {
    match st.core.db.get("character", &cid) {
        Ok(Some(row)) => respond(StatusCode::OK, Value::Object(row)),
        _ => err(StatusCode::NOT_FOUND, "character not found"),
    }
}

async fn update_character(
    State(st): State<AppState>,
    Path(cid): Path<String>,
    body: MaybeJson,
) -> Response {
    let mut b = match body_obj(body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    b.remove("id");
    if let Err(e) = st.core.db.update("character", &cid, &b) {
        return err500(e);
    }
    match st.core.db.get("character", &cid) {
        Ok(row) => respond(StatusCode::OK, row_or_null(row)),
        Err(e) => err500(e),
    }
}

async fn delete_character(State(st): State<AppState>, Path(cid): Path<String>) -> Response {
    match st.core.db.delete("character", &cid) {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => err500(e),
    }
}

// ---------- videos ----------

async fn list_videos(
    State(st): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let project_id = q.get("project_id").cloned().unwrap_or_default();
    if project_id.is_empty() {
        return err(StatusCode::BAD_REQUEST, "project_id required");
    }
    match st.core.db.query(
        "SELECT * FROM video WHERE project_id = ?1 ORDER BY display_order",
        &[&project_id],
    ) {
        Ok(rows) => respond(StatusCode::OK, rows_json(rows)),
        Err(e) => err500(e),
    }
}

async fn create_video(State(st): State<AppState>, body: MaybeJson) -> Response {
    let b = match body_obj(body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    match st.core.db.insert("video", &b) {
        Ok(id) => match st.core.db.get("video", &id) {
            Ok(row) => respond(StatusCode::CREATED, row_or_null(row)),
            Err(e) => err500(e),
        },
        Err(e) => err500(e),
    }
}

async fn get_video(State(st): State<AppState>, Path(vid): Path<String>) -> Response {
    match st.core.db.get("video", &vid) {
        Ok(Some(row)) => respond(StatusCode::OK, Value::Object(row)),
        _ => err(StatusCode::NOT_FOUND, "video not found"),
    }
}

async fn update_video(
    State(st): State<AppState>,
    Path(vid): Path<String>,
    body: MaybeJson,
) -> Response {
    let mut b = match body_obj(body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    b.remove("id");
    if let Err(e) = st.core.db.update("video", &vid, &b) {
        return err500(e);
    }
    match st.core.db.get("video", &vid) {
        Ok(row) => respond(StatusCode::OK, row_or_null(row)),
        Err(e) => err500(e),
    }
}

async fn delete_video(State(st): State<AppState>, Path(vid): Path<String>) -> Response {
    match st.core.db.delete("video", &vid) {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => err500(e),
    }
}

// ---------- scenes ----------

async fn list_scenes(
    State(st): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let video_id = q.get("video_id").cloned().unwrap_or_default();
    if video_id.is_empty() {
        return err(StatusCode::BAD_REQUEST, "video_id required");
    }
    match st.core.db.query(
        "SELECT * FROM scene WHERE video_id = ?1 ORDER BY display_order",
        &[&video_id],
    ) {
        Ok(rows) => respond(StatusCode::OK, rows_json(rows)),
        Err(e) => err500(e),
    }
}

/// character_names arrives as an array from the UI; the column stores JSON text.
fn serialize_scene_fields(body: &mut Row) {
    if let Some(v) = body.get("character_names") {
        if v.is_array() {
            let s = v.to_string();
            body.insert("character_names".into(), json!(s));
        }
    }
}

async fn create_scene(State(st): State<AppState>, body: MaybeJson) -> Response {
    let mut b = match body_obj(body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    serialize_scene_fields(&mut b);
    match st.core.db.insert("scene", &b) {
        Ok(id) => match st.core.db.get("scene", &id) {
            Ok(row) => respond(StatusCode::CREATED, row_or_null(row)),
            Err(e) => err500(e),
        },
        Err(e) => err500(e),
    }
}

async fn get_scene(State(st): State<AppState>, Path(sid): Path<String>) -> Response {
    match st.core.db.get("scene", &sid) {
        Ok(Some(row)) => respond(StatusCode::OK, Value::Object(row)),
        _ => err(StatusCode::NOT_FOUND, "scene not found"),
    }
}

async fn update_scene(
    State(st): State<AppState>,
    Path(sid): Path<String>,
    body: MaybeJson,
) -> Response {
    let mut b = match body_obj(body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    b.remove("id");
    serialize_scene_fields(&mut b);
    if let Err(e) = st.core.db.update("scene", &sid, &b) {
        return err500(e);
    }
    st.core
        .dash
        .emit("scene_updated", json!({ "scene_id": sid }));
    match st.core.db.get("scene", &sid) {
        Ok(row) => respond(StatusCode::OK, row_or_null(row)),
        Err(e) => err500(e),
    }
}

async fn delete_scene(State(st): State<AppState>, Path(sid): Path<String>) -> Response {
    match st.core.db.delete("scene", &sid) {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => err500(e),
    }
}

// ---------- requests ----------

const REQUEST_FILTER_KEYS: [&str; 6] = [
    "scene_id",
    "video_id",
    "project_id",
    "status",
    "type",
    "character_id",
];

async fn list_requests(
    State(st): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let mut conds: Vec<String> = Vec::new();
    let mut args: Vec<String> = Vec::new();
    for k in REQUEST_FILTER_KEYS {
        if let Some(v) = q.get(k).filter(|v| !v.is_empty()) {
            conds.push(format!("{k} = ?"));
            args.push(v.clone());
        }
    }
    let mut sql = "SELECT * FROM request".to_string();
    if !conds.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&conds.join(" AND "));
    }
    sql.push_str(" ORDER BY created_at DESC");
    let params: Vec<&dyn rusqlite::ToSql> =
        args.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
    match st.core.db.query(&sql, &params) {
        Ok(rows) => respond(StatusCode::OK, rows_json(rows)),
        Err(e) => err500(e),
    }
}

/// Request types handled by the image agent via a DAG task.
fn is_image_req_type(t: &str) -> bool {
    matches!(
        t,
        "GENERATE_CHARACTER_IMAGE"
            | "REGENERATE_CHARACTER_IMAGE"
            | "EDIT_CHARACTER_IMAGE"
            | "GENERATE_IMAGE"
            | "REGENERATE_IMAGE"
            | "EDIT_IMAGE"
    )
}

fn request_type_to_mode(t: &str) -> &'static str {
    match t {
        "GENERATE_CHARACTER_IMAGE" | "REGENERATE_CHARACTER_IMAGE" | "EDIT_CHARACTER_IMAGE" => {
            "single_entity"
        }
        _ => "single_scene",
    }
}

/// Create a DAG parent + image task so the DAG engine runs it (port of
/// scheduleImageDAGTask). The image agent completes/fails the request row.
fn schedule_image_dag_task(
    st: &AppState,
    project_id: &str,
    request_id: &str,
    mode: &str,
    character_id: &str,
    scene_id: &str,
    video_id: &str,
    orientation: &str,
) -> anyhow::Result<()> {
    let mut params = Map::new();
    params.insert("mode".into(), json!(mode));
    params.insert("request_id".into(), json!(request_id));
    params.insert("orientation".into(), json!(orientation));
    if !character_id.is_empty() {
        params.insert("character_id".into(), json!(character_id));
    }
    if !scene_id.is_empty() {
        params.insert("scene_id".into(), json!(scene_id));
    }
    if !video_id.is_empty() {
        params.insert("video_id".into(), json!(video_id));
    }
    let params_json = Value::Object(params).to_string();

    let mut parent = Map::new();
    parent.insert("id".into(), json!(db::new_id()));
    parent.insert("project_id".into(), json!(project_id));
    parent.insert("status".into(), json!("queued"));
    parent.insert("orientation".into(), json!(orientation));
    let parent_id = st.core.db.insert("dag_parents", &parent)?;

    let mut task = Map::new();
    task.insert("id".into(), json!(db::new_id()));
    task.insert("parent_id".into(), json!(parent_id));
    task.insert("label".into(), json!("image"));
    task.insert("agent_type".into(), json!("image"));
    task.insert("prompt".into(), json!(params_json));
    task.insert("depends_on".into(), json!("[]"));
    task.insert("status".into(), json!("registered"));
    st.core.db.insert("dag_tasks", &task)?;
    Ok(())
}

fn create_one_request(st: &AppState, mut b: Row) -> Result<Row, Response> {
    let req_type = db::str_of(&b, "type");
    let image_agent = st.pool.get("image").is_some();
    let schedule = is_image_req_type(&req_type) && image_agent;
    if schedule {
        b.insert("status".into(), json!("PROCESSING"));
    } else if b.get("status").map(|v| v.is_null()).unwrap_or(true) {
        b.insert("status".into(), json!("PENDING"));
    }
    let id = st.core.db.insert("request", &b).map_err(err500)?;
    let row = st
        .core
        .db
        .get("request", &id)
        .map_err(err500)?
        .unwrap_or_default();
    if schedule {
        let mut orientation = db::str_of(&row, "orientation");
        if orientation.is_empty() {
            orientation = "VERTICAL".to_string();
        }
        let _ = schedule_image_dag_task(
            st,
            &db::str_of(&row, "project_id"),
            &id,
            request_type_to_mode(&req_type),
            &db::str_of(&row, "character_id"),
            &db::str_of(&row, "scene_id"),
            &db::str_of(&row, "video_id"),
            &orientation,
        );
    }
    Ok(row)
}

async fn create_request(State(st): State<AppState>, body: MaybeJson) -> Response {
    let b = match body_obj(body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    match create_one_request(&st, b) {
        Ok(row) => respond(StatusCode::CREATED, Value::Object(row)),
        Err(r) => r,
    }
}

async fn batch_requests(State(st): State<AppState>, body: MaybeJson) -> Response {
    #[derive(Deserialize, Default)]
    struct Body {
        #[serde(default)]
        requests: Vec<Value>,
    }
    let b: Body = match parse_body(body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let mut created: Vec<Value> = Vec::new();
    for req in b.requests {
        let Some(obj) = req.as_object().cloned() else {
            continue;
        };
        match create_one_request(&st, obj) {
            Ok(row) => created.push(Value::Object(row)),
            Err(r) => return r,
        }
    }
    respond(StatusCode::CREATED, Value::Array(created))
}

/// Progress of a batch of generation requests.
///
/// The Go original returned the raw `{STATUS: count}` map, but the UI waits on
/// `done` — so its wave loop never advanced and every batch ended in a 4-minute
/// "Timeout" even though the requests had completed. Return the shape the UI
/// actually binds to; the per-status counts stay under `counts` for debugging.
async fn batch_request_status(
    State(st): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let mut conds: Vec<String> = Vec::new();
    let mut args: Vec<String> = Vec::new();
    // `orientation` matters here: vertical and horizontal are independent
    // generations, so a batch must not wait on the other orientation's rows.
    for k in ["project_id", "video_id", "scene_id", "type", "orientation"] {
        if let Some(v) = q.get(k).filter(|v| !v.is_empty()) {
            conds.push(format!("{k} = ?"));
            args.push(v.clone());
        }
    }
    let mut sql = "SELECT status, COUNT(*) as cnt FROM request".to_string();
    if !conds.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&conds.join(" AND "));
    }
    sql.push_str(" GROUP BY status");
    let params: Vec<&dyn rusqlite::ToSql> =
        args.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
    let rows = match st.core.db.query(&sql, &params) {
        Ok(r) => r,
        Err(e) => return err500(e),
    };

    let mut counts = Map::new();
    let (mut pending, mut processing, mut completed, mut failed) = (0i64, 0i64, 0i64, 0i64);
    for r in rows {
        let status = db::str_of(&r, "status");
        let n = db::i64_of(&r, "cnt");
        match status.as_str() {
            "PENDING" => pending += n,
            "PROCESSING" => processing += n,
            "COMPLETED" => completed += n,
            "FAILED" => failed += n,
            _ => {}
        }
        counts.insert(status, json!(n));
    }
    let total = pending + processing + completed + failed;
    // Nothing outstanding ⇒ this wave is over, whether or not it all succeeded.
    let done = pending == 0 && processing == 0;
    respond(
        StatusCode::OK,
        json!({
            "total": total,
            "pending": pending,
            "processing": processing,
            "completed": completed,
            "failed": failed,
            "done": done,
            "all_succeeded": done && failed == 0 && total > 0,
            "orientation": q.get("orientation").cloned().unwrap_or_default(),
            "counts": counts,
        }),
    )
}

async fn list_pending_requests(State(st): State<AppState>) -> Response {
    match st.core.db.query(
        "SELECT * FROM request WHERE status = 'PENDING' ORDER BY created_at ASC",
        &[],
    ) {
        Ok(rows) => respond(StatusCode::OK, rows_json(rows)),
        Err(e) => err500(e),
    }
}

async fn get_request(State(st): State<AppState>, Path(rid): Path<String>) -> Response {
    match st.core.db.get("request", &rid) {
        Ok(Some(row)) => respond(StatusCode::OK, Value::Object(row)),
        _ => err(StatusCode::NOT_FOUND, "request not found"),
    }
}

async fn update_request(
    State(st): State<AppState>,
    Path(rid): Path<String>,
    body: MaybeJson,
) -> Response {
    let mut b = match body_obj(body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    b.remove("id");
    if let Err(e) = st.core.db.update("request", &rid, &b) {
        return err500(e);
    }
    match st.core.db.get("request", &rid) {
        Ok(row) => respond(StatusCode::OK, row_or_null(row)),
        Err(e) => err500(e),
    }
}

async fn delete_request(State(st): State<AppState>, Path(rid): Path<String>) -> Response {
    match st.core.db.delete("request", &rid) {
        Ok(_) => respond(StatusCode::OK, json!({ "status": "ok" })),
        Err(e) => err500(e),
    }
}

async fn clear_requests(
    State(st): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let video_id = q.get("video_id").cloned().unwrap_or_default();
    let r = if !video_id.is_empty() {
        st.core
            .db
            .execute("DELETE FROM request WHERE video_id = ?1", &[&video_id])
    } else {
        st.core.db.execute("DELETE FROM request", &[])
    };
    match r {
        Ok(_) => respond(StatusCode::OK, json!({ "status": "ok" })),
        Err(e) => err500(e),
    }
}

// ---------- pipeline ----------

async fn create_pipeline(State(st): State<AppState>, body: MaybeJson) -> Response {
    #[derive(Deserialize, Default)]
    struct Body {
        #[serde(default)]
        project_id: String,
        #[serde(default)]
        script: String,
        #[serde(default)]
        orientation: String,
        #[serde(default)]
        goal: String,
        #[serde(default)]
        pipeline_mode: String,
    }
    let b: Body = match parse_body(body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    if b.project_id.is_empty() {
        return err(StatusCode::BAD_REQUEST, "project_id required");
    }
    // pipeline::create emits `pipeline:created` itself.
    match pipeline::create(
        &st.core,
        &st.pool,
        &b.project_id,
        &b.script,
        &b.orientation,
        &b.goal,
        &b.pipeline_mode,
    )
    .await
    {
        Ok((pipeline_id, task_count)) => respond(
            StatusCode::CREATED,
            json!({ "id": pipeline_id, "task_count": task_count }),
        ),
        Err(e) => err500(e),
    }
}

async fn get_pipeline(State(st): State<AppState>, Path(pid): Path<String>) -> Response {
    match pipeline::get_status(&st.core, &pid) {
        Ok(v) => respond(StatusCode::OK, v),
        Err(e) => err(StatusCode::NOT_FOUND, e),
    }
}

async fn start_pipeline(State(st): State<AppState>, Path(pid): Path<String>) -> Response {
    match pipeline::start(&st.core, &pid) {
        Ok(()) => respond(StatusCode::OK, json!({ "status": "queued" })),
        Err(e) => err500(e),
    }
}

async fn pause_pipeline(State(st): State<AppState>, Path(pid): Path<String>) -> Response {
    match pipeline::pause(&st.core, &pid) {
        Ok(()) => respond(StatusCode::OK, json!({ "status": "paused" })),
        Err(e) => err500(e),
    }
}

async fn cancel_pipeline(State(st): State<AppState>, Path(pid): Path<String>) -> Response {
    match pipeline::cancel(&st.core, &pid) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => err500(e),
    }
}

// ---------- workflow-engine pipeline ----------

#[derive(Deserialize, Default)]
struct StartWorkflowBody {
    #[serde(default)]
    project_id: String,
    #[serde(default)]
    video_id: String,
    #[serde(default)]
    orientation: String,
    #[serde(default)]
    with_audio: bool,
    #[serde(default)]
    with_critic: bool,
}

/// Register the project's workflow definition on the daemon and start a run.
/// 202: the run is executing on the engine, not here — poll the run endpoints.
async fn start_workflow(State(st): State<AppState>, body: MaybeJson) -> Response {
    let b: StartWorkflowBody = match parse_body(body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    if b.project_id.trim().is_empty() {
        return err(StatusCode::BAD_REQUEST, "project_id là bắt buộc");
    }
    match crate::wfclient::launch_project_workflow(
        &st,
        b.project_id.trim(),
        &b.video_id,
        &b.orientation,
        b.with_audio,
        b.with_critic,
    )
    .await
    {
        Ok((workflow, run_id)) => respond(
            StatusCode::ACCEPTED,
            json!({ "workflow": workflow, "run_id": run_id }),
        ),
        Err(e) => err500(e),
    }
}

async fn start_custom_workflow(State(st): State<AppState>, body: MaybeJson) -> Response {
    #[derive(Deserialize, Default)]
    struct Body {
        #[serde(default)]
        project_id: String,
        #[serde(default)]
        video_id: String,
        #[serde(default)]
        orientation: String,
        /// Ordered stages; agents within a stage run in parallel.
        #[serde(default)]
        stages: Vec<Vec<String>>,
    }
    let b: Body = match parse_body(body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    if b.project_id.trim().is_empty() {
        return err(StatusCode::BAD_REQUEST, "project_id là bắt buộc");
    }
    if b.stages.is_empty() {
        return err(StatusCode::BAD_REQUEST, "stages rỗng");
    }
    match crate::wfclient::launch_custom_workflow(
        &st,
        b.project_id.trim(),
        &b.video_id,
        &b.orientation,
        &b.stages,
    )
    .await
    {
        Ok((workflow, run_id)) => respond(
            StatusCode::ACCEPTED,
            json!({ "workflow": workflow, "run_id": run_id }),
        ),
        Err(e) => err500(e),
    }
}

async fn get_workflow_run(Path(run_id): Path<String>) -> Response {
    let run = match crate::wfclient::get_run(&run_id).await {
        Ok(r) => r,
        Err(e) => return err500(e),
    };
    // Activity is a cheap in-memory read on the daemon; fold it in so the UI
    // gets node state and the live agent/script log in one round trip.
    let activity = crate::wfclient::run_activity(&run_id)
        .await
        .unwrap_or(Value::Array(vec![]));
    respond(
        StatusCode::OK,
        json!({
            "run": run,
            "summary": crate::wfclient::summarize_run(&run),
            "activity": activity,
        }),
    )
}

async fn cancel_workflow_run(Path(run_id): Path<String>) -> Response {
    match crate::wfclient::cancel_run(&run_id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => err500(e),
    }
}

/// The run this project last launched. `run_id: null` (200) rather than a 404 —
/// "never run" is a normal state for the UI, not an error.
async fn project_workflow_run(State(st): State<AppState>, Path(pid): Path<String>) -> Response {
    let Some((workflow, run_id)) = crate::wfclient::stored_run(&st, &pid) else {
        return respond(
            StatusCode::OK,
            json!({ "run_id": null, "workflow": null, "run": null }),
        );
    };
    let run = crate::wfclient::get_run(&run_id).await.ok();
    let summary = run.as_ref().map(crate::wfclient::summarize_run);
    respond(
        StatusCode::OK,
        json!({ "run_id": run_id, "workflow": workflow, "run": run, "summary": summary }),
    )
}

async fn delete_pipeline(
    State(st): State<AppState>,
    Path(pid): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let project_id = q.get("project_id").cloned().unwrap_or_default();
    if project_id.is_empty() {
        return err(StatusCode::BAD_REQUEST, "project_id query param required");
    }
    match st.core.db.delete_pipeline_cascade(&pid, &project_id) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => err500(e),
    }
}

async fn retry_task(
    State(st): State<AppState>,
    Path((pid, tid)): Path<(String, String)>,
) -> Response {
    match pipeline::retry_task(&st.core, &pid, &tid) {
        Ok(()) => respond(StatusCode::OK, json!({ "ok": true })),
        Err(e) => err500(e),
    }
}

async fn stop_task(
    State(st): State<AppState>,
    Path((_pid, tid)): Path<(String, String)>,
) -> Response {
    // Cancel the running task if active; if it was merely registered (queued
    // but not started), mark it error directly like the Go fallback.
    st.engine.stop_task(&tid);
    if let Ok(Some(t)) = st.core.db.get("dag_tasks", &tid) {
        if db::str_of(&t, "status") == "registered" {
            let mut up = Map::new();
            up.insert("status".into(), json!("error"));
            up.insert("result".into(), json!(r#"{"error":"cancelled by user"}"#));
            if let Err(e) = st.core.db.update("dag_tasks", &tid, &up) {
                return err500(e);
            }
        }
    }
    respond(StatusCode::OK, json!({ "ok": true }))
}

async fn list_project_pipelines(
    State(st): State<AppState>,
    Path(project_id): Path<String>,
) -> Response {
    match st.core.db.query(
        "SELECT * FROM dag_parents WHERE project_id = ?1 ORDER BY created_at DESC",
        &[&project_id],
    ) {
        Ok(rows) => respond(StatusCode::OK, rows_json(rows)),
        Err(e) => err500(e),
    }
}

// ---------- script parse ----------

async fn parse_script(State(st): State<AppState>, body: MaybeJson) -> Response {
    #[derive(Deserialize, Default)]
    struct Body {
        #[serde(default)]
        script: String,
    }
    let b: Body = match parse_body(body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    if b.script.is_empty() {
        return err(StatusCode::BAD_REQUEST, "script required");
    }
    let soul = souls::load(&st.core.souls_dir, "script_parser");
    match script::parse(&soul, &b.script).await {
        Ok(parsed) => respond(
            StatusCode::OK,
            serde_json::to_value(&parsed).unwrap_or_default(),
        ),
        Err(e) => err500(e),
    }
}

// ---------- agents ----------

fn is_builtin_agent_type(st: &AppState, agent_type: &str) -> bool {
    agent_type == "orchestrator" || st.pool.builtin_order.contains(&agent_type)
}

fn soul_summary(prompt: &str) -> String {
    let mut s = prompt.trim();
    if s.starts_with("---") {
        let parts: Vec<&str> = s.splitn(3, "---").collect();
        if parts.len() == 3 {
            s = parts[2].trim();
        }
    }
    for line in s.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let runes: Vec<char> = line.chars().collect();
        if runes.len() > 120 {
            return runes[..120].iter().collect::<String>() + "…";
        }
        return line.to_string();
    }
    String::new()
}

fn parse_skill_agent_skill_ids(raw: &str, legacy: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(raw) {
        for v in arr {
            if let Some(s) = v.as_str() {
                let s = s.trim();
                if !s.is_empty() && !out.iter().any(|x| x == s) {
                    out.push(s.to_string());
                }
            }
        }
    }
    if out.is_empty() {
        let l = legacy.trim();
        if !l.is_empty() && l != "-" {
            out.push(l.to_string());
        }
    }
    out
}

fn normalize_skill_ids(ids: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in ids {
        let s = s.trim();
        if s.is_empty() || out.iter().any(|x| x == s) {
            continue;
        }
        out.push(s.to_string());
    }
    out
}

async fn list_agents(State(st): State<AppState>) -> Response {
    let mut out: Vec<Value> = Vec::new();
    for info in st
        .pool
        .list_info()
        .into_iter()
        .filter(|i| i.kind == "built-in")
    {
        let prompt = souls::load_raw(&st.core.souls_dir, &info.agent_type);
        out.push(json!({
            "type": info.agent_type,
            "name": info.name,
            "description": info.description,
            "soul_summary": soul_summary(&prompt),
            "prompt": prompt,
            "soul_file": souls::canonical_basename(&info.agent_type),
            "kind": "built-in",
            "enabled": !st.core.db.builtin_agent_disabled(&info.agent_type),
        }));
    }
    if let Ok(rows) = st.core.db.query(
        "SELECT id, name, skill_id, skill_ids, prompt, enabled FROM skill_agent ORDER BY created_at",
        &[],
    ) {
        for r in rows {
            let ids = parse_skill_agent_skill_ids(&db::str_of(&r, "skill_ids"), &db::str_of(&r, "skill_id"));
            let name = db::str_of(&r, "name");
            out.push(json!({
                "type": db::str_of(&r, "id"),
                "name": name,
                "skill_id": db::str_of(&r, "skill_id"),
                "skill_ids": ids,
                "description": name,
                "prompt": db::str_of(&r, "prompt"),
                "kind": "skill",
                "enabled": db::i64_of(&r, "enabled") == 1,
            }));
        }
    }
    respond(StatusCode::OK, Value::Array(out))
}

/// One Agent Log / History entry (port of the Go `entry` struct, incl. the
/// virtual `blocked` status derived from "blocked: <dep> failed" results).
fn task_log_entry(pipeline_id: &str, project_id: &str, pipeline_status: &str, t: &Row) -> Value {
    let st = db::str_of(t, "status");
    let result = db::str_of(t, "result");
    let mut status = st.clone();
    let mut error_message = String::new();
    let mut blocked_by = String::new();
    if (st == "error" || st == "timeout") && !result.is_empty() {
        if let Ok(res) = serde_json::from_str::<Value>(&result) {
            if let Some(msg) = res.get("error").and_then(|v| v.as_str()) {
                error_message = msg.to_string();
                if let Some(dep) = msg
                    .strip_prefix("blocked: ")
                    .and_then(|r| r.strip_suffix(" failed"))
                {
                    blocked_by = dep.to_string();
                    status = "blocked".to_string(); // virtual status for the UI
                }
            }
        }
        if error_message.is_empty() && st == "timeout" {
            error_message = "Agent timed out".to_string();
        }
    }
    let mut e = Map::new();
    e.insert("pipeline_id".into(), json!(pipeline_id));
    e.insert("project_id".into(), json!(project_id));
    e.insert("pipeline_status".into(), json!(pipeline_status));
    e.insert("task_id".into(), json!(db::str_of(t, "id")));
    e.insert("task_label".into(), json!(db::str_of(t, "label")));
    e.insert("agent_type".into(), json!(db::str_of(t, "agent_type")));
    e.insert("status".into(), json!(status));
    e.insert("started_at".into(), json!(db::str_of(t, "started_at")));
    e.insert("completed_at".into(), json!(db::str_of(t, "completed_at")));
    if !error_message.is_empty() {
        e.insert("error_message".into(), json!(error_message));
    }
    if !blocked_by.is_empty() {
        e.insert("blocked_by".into(), json!(blocked_by));
    }
    if !result.is_empty() {
        e.insert("result".into(), json!(result));
    }
    Value::Object(e)
}

async fn agent_log(State(st): State<AppState>) -> Response {
    let parents = match st.core.db.query(
        "SELECT * FROM dag_parents WHERE status IN ('queued','active') \
         OR (status IN ('done','failed') AND updated_at >= datetime('now','-1 hour')) \
         ORDER BY created_at ASC",
        &[],
    ) {
        Ok(p) => p,
        Err(e) => return err500(e),
    };
    let mut out: Vec<Value> = Vec::new();
    for p in &parents {
        let pid = db::str_of(p, "id");
        let Ok(tasks) = st.core.db.query(
            "SELECT * FROM dag_tasks WHERE parent_id = ?1 ORDER BY rowid",
            &[&pid],
        ) else {
            continue;
        };
        for t in &tasks {
            let status = db::str_of(t, "status");
            if status != "active"
                && status != "registered"
                && status != "error"
                && status != "timeout"
            {
                continue;
            }
            out.push(task_log_entry(
                &pid,
                &db::str_of(p, "project_id"),
                &db::str_of(p, "status"),
                t,
            ));
        }
    }
    respond(StatusCode::OK, Value::Array(out))
}

async fn agent_history(State(st): State<AppState>) -> Response {
    let cleared = st
        .core
        .db
        .kv_get("agent_history_cleared_at")
        .trim()
        .to_string();
    let limit = 100i64;
    let r = if !cleared.is_empty() {
        st.core.db.query(
            "SELECT t.*, p.project_id, p.status AS pipeline_status \
             FROM dag_tasks t JOIN dag_parents p ON p.id = t.parent_id \
             WHERE t.status IN ('done','error','timeout') AND t.completed_at >= ?1 \
             ORDER BY t.completed_at DESC LIMIT ?2",
            &[&cleared as &dyn rusqlite::ToSql, &limit],
        )
    } else {
        st.core.db.query(
            "SELECT t.*, p.project_id, p.status AS pipeline_status \
             FROM dag_tasks t JOIN dag_parents p ON p.id = t.parent_id \
             WHERE t.status IN ('done','error','timeout') \
             ORDER BY t.completed_at DESC LIMIT ?1",
            &[&limit],
        )
    };
    match r {
        Ok(rows) => {
            let out: Vec<Value> = rows
                .iter()
                .map(|t| {
                    task_log_entry(
                        &db::str_of(t, "parent_id"),
                        &db::str_of(t, "project_id"),
                        &db::str_of(t, "pipeline_status"),
                        t,
                    )
                })
                .collect();
            respond(StatusCode::OK, Value::Array(out))
        }
        Err(e) => err500(e),
    }
}

async fn clear_agent_history(State(st): State<AppState>) -> Response {
    match st.core.db.kv_set("agent_history_cleared_at", &db::now()) {
        Ok(()) => respond(StatusCode::OK, json!({ "ok": "true" })),
        Err(e) => err500(e),
    }
}

async fn put_agent_soul(
    State(st): State<AppState>,
    Path(agent_type): Path<String>,
    body: MaybeJson,
) -> Response {
    if !is_builtin_agent_type(&st, &agent_type) {
        return err(StatusCode::NOT_FOUND, "unknown built-in agent type");
    }
    #[derive(Deserialize, Default)]
    struct Body {
        #[serde(default)]
        prompt: String,
    }
    let b: Body = match parse_body(body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    match souls::write(&st.core.souls_dir, &agent_type, &b.prompt) {
        Ok(path) => respond(
            StatusCode::OK,
            json!({
                "ok": "true",
                "path": path.display().to_string(),
                "soul_file": souls::canonical_basename(&agent_type),
            }),
        ),
        Err(e) => err500(e),
    }
}

async fn patch_builtin_agent(
    State(st): State<AppState>,
    Path(agent_type): Path<String>,
    body: MaybeJson,
) -> Response {
    if !is_builtin_agent_type(&st, &agent_type) {
        return err(StatusCode::NOT_FOUND, "unknown built-in agent type");
    }
    #[derive(Deserialize, Default)]
    struct Body {
        #[serde(default)]
        enabled: bool,
    }
    let b: Body = match parse_body(body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let val = if b.enabled { "0" } else { "1" };
    match st
        .core
        .db
        .kv_set(&format!("builtin_agent_disabled:{agent_type}"), val)
    {
        Ok(()) => respond(
            StatusCode::OK,
            json!({ "type": agent_type, "enabled": b.enabled }),
        ),
        Err(e) => err500(e),
    }
}

// ---------- skill agents CRUD ----------

async fn list_skill_agents(State(st): State<AppState>) -> Response {
    match st.core.db.query(
        "SELECT id, name, skill_id, skill_ids, prompt, enabled, created_at FROM skill_agent ORDER BY created_at",
        &[],
    ) {
        Ok(rows) => respond(StatusCode::OK, rows_json(rows)),
        Err(e) => err500(e),
    }
}

async fn create_skill_agent(State(st): State<AppState>, body: MaybeJson) -> Response {
    #[derive(Deserialize, Default)]
    struct Body {
        #[serde(default)]
        id: String,
        #[serde(default)]
        name: String,
        #[serde(default)]
        skill_id: String,
        #[serde(default)]
        skill_ids: Vec<String>,
        #[serde(default)]
        prompt: String,
    }
    let b: Body = match parse_body(body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    if b.name.trim().is_empty() {
        return err(StatusCode::BAD_REQUEST, "name is required");
    }
    let mut ids = normalize_skill_ids(&b.skill_ids);
    if ids.is_empty() && !b.skill_id.trim().is_empty() {
        ids = normalize_skill_ids(&[b.skill_id.clone()]);
    }
    if ids.is_empty() && b.prompt.trim().is_empty() {
        return err(
            StatusCode::BAD_REQUEST,
            "chọn ít nhất một skill hoặc điền prompt tùy chỉnh",
        );
    }
    let id = if b.id.is_empty() {
        format!("{}-{}", slugify(&b.name), &db::new_id()[..8])
    } else {
        b.id.clone()
    };
    let mirror = ids.first().cloned().unwrap_or_else(|| "-".to_string());
    let mut row = Map::new();
    row.insert("id".into(), json!(id));
    row.insert("name".into(), json!(b.name));
    row.insert("skill_id".into(), json!(mirror));
    row.insert("skill_ids".into(), json!(json!(ids).to_string()));
    row.insert("prompt".into(), json!(b.prompt));
    row.insert("enabled".into(), json!(1));
    let id = match st.core.db.insert("skill_agent", &row) {
        Ok(id) => id,
        Err(e) => return err500(e),
    };
    if let Ok(Some(saved)) = st.core.db.get("skill_agent", &id) {
        crate::agents::skill_agent::register_skill_agent(&st.pool, &saved);
    }
    respond(StatusCode::CREATED, json!({ "id": id }))
}

async fn update_skill_agent(
    State(st): State<AppState>,
    Path(id): Path<String>,
    body: MaybeJson,
) -> Response {
    #[derive(Deserialize, Default)]
    struct Body {
        name: Option<String>,
        prompt: Option<String>,
        enabled: Option<bool>,
        skill_ids: Option<Vec<String>>,
    }
    let b: Body = match parse_body(body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let row = match st.core.db.get("skill_agent", &id) {
        Ok(Some(r)) => r,
        _ => return err(StatusCode::NOT_FOUND, "skill agent not found"),
    };
    let mut name = db::str_of(&row, "name");
    let mut prompt = db::str_of(&row, "prompt");
    let mut enabled: i64 = if db::i64_of(&row, "enabled") == 0 {
        0
    } else {
        1
    };
    if let Some(n) = b.name {
        name = n;
    }
    if let Some(p) = b.prompt {
        prompt = p;
    }
    if let Some(e) = b.enabled {
        enabled = if e { 1 } else { 0 };
    }
    let mut merged_ids = parse_skill_agent_skill_ids(
        &db::str_of(&row, "skill_ids"),
        &db::str_of(&row, "skill_id"),
    );
    let skill_cols_in_request = b.skill_ids.is_some();
    if let Some(replace) = &b.skill_ids {
        merged_ids = normalize_skill_ids(replace);
    }
    if enabled == 1 && merged_ids.is_empty() && prompt.trim().is_empty() {
        return err(
            StatusCode::BAD_REQUEST,
            "agent cần ít nhất một skill hoặc prompt tùy chỉnh",
        );
    }
    let mut up = Map::new();
    up.insert("name".into(), json!(name));
    up.insert("prompt".into(), json!(prompt));
    up.insert("enabled".into(), json!(enabled));
    if skill_cols_in_request {
        let mirror = merged_ids
            .first()
            .cloned()
            .unwrap_or_else(|| "-".to_string());
        up.insert("skill_id".into(), json!(mirror));
        up.insert("skill_ids".into(), json!(json!(merged_ids).to_string()));
    }
    if let Err(e) = st.core.db.update("skill_agent", &id, &up) {
        return err500(e);
    }
    if enabled == 1 {
        if let Ok(Some(saved)) = st.core.db.get("skill_agent", &id) {
            crate::agents::skill_agent::register_skill_agent(&st.pool, &saved);
        }
    } else {
        st.pool.unregister(&id);
    }
    respond(StatusCode::OK, json!({ "ok": "true" }))
}

async fn delete_skill_agent(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    if let Err(e) = st.core.db.delete("skill_agent", &id) {
        return err500(e);
    }
    st.pool.unregister(&id);
    StatusCode::NO_CONTENT.into_response()
}

// ---------- AI suggestions ----------

/// AI autofill for the Create Project form.
///
/// The Go original read `script` and answered with free prose, while the UI
/// sends `prompt` and needs `{suggestion: {name, story, material, language}}` —
/// so the button never filled anything. Accept every field name the UI has used
/// and return the structured object it binds to.
async fn suggest_project(State(st): State<AppState>, body: MaybeJson) -> Response {
    #[derive(Deserialize, Default)]
    struct Body {
        #[serde(default)]
        prompt: String,
        #[serde(default)]
        script: String,
        #[serde(default)]
        idea: String,
    }
    let b: Body = match parse_body(body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let idea = [b.prompt.trim(), b.script.trim(), b.idea.trim()]
        .into_iter()
        .find(|s| !s.is_empty())
        .unwrap_or("")
        .to_string();
    if idea.is_empty() {
        return err(StatusCode::BAD_REQUEST, "cần mô tả ý tưởng (prompt)");
    }

    // Offer the real material ids so the model can't invent a style the form
    // would silently drop.
    let materials: Vec<String> = st
        .core
        .db
        .query("SELECT id, name FROM material ORDER BY id", &[])
        .unwrap_or_default()
        .iter()
        .map(|m| {
            let id = crate::db::str_of(m, "id");
            let name = crate::db::str_of(m, "name");
            if name.is_empty() {
                id
            } else {
                format!("{id} ({name})")
            }
        })
        .collect();
    let material_line = if materials.is_empty() {
        "realistic".to_string()
    } else {
        materials.join(", ")
    };

    let system = "You are a creative film producer helping fill in a video project form. \
Reply with ONLY a JSON object (no prose, no markdown fences) in EXACTLY this shape:\n\
{\"name\":\"...\",\"story\":\"...\",\"material\":\"...\",\"language\":\"vi\"}\n\
- name: a short, evocative project title (max 8 words), in the SAME language as the idea.\n\
- story: 3-6 sentences of premise — setting, main character(s), the emotional arc. Same language as the idea.\n\
- material: the single best-fitting visual style id from the allowed list given by the user. Use the bare id only.\n\
- language: \"vi\" if the idea is Vietnamese, else \"en\".";
    let user = format!(
        "Idea:\n{idea}\n\nAllowed material ids: {material_line}\n\nReturn the JSON object now."
    );

    #[derive(Deserialize, Default)]
    struct Suggestion {
        #[serde(default)]
        name: String,
        #[serde(default)]
        story: String,
        #[serde(default)]
        material: String,
        #[serde(default)]
        language: String,
    }

    // A reply cut at the token cap still "parses" — every field defaults, so a
    // truncated object silently yields a name and nothing else. Treat a missing
    // story as a failed attempt and ask again, tighter.
    let mut last_err = String::new();
    let mut last_text = String::new();
    let mut parsed: Option<Suggestion> = None;
    for attempt in 0..2u8 {
        let u = if attempt == 0 {
            user.clone()
        } else {
            format!(
                "{user}\n\nLƯU Ý: lần trước trả về không đầy đủ. Trả JSON GỌN — story tối đa 4 câu, \
không xuống dòng thừa, và phải có đủ 4 khoá name/story/material/language."
            )
        };
        let (text, _model) = match llm::complete(system, &u, 2000).await {
            Ok(v) => v,
            Err(e) => return err500(e),
        };
        match llm::parse_json::<Suggestion>(&text) {
            Ok(s) if !s.name.trim().is_empty() && !s.story.trim().is_empty() => {
                parsed = Some(s);
                break;
            }
            Ok(s) => {
                last_err = "thiếu trường (có thể bị cắt vì hết token)".into();
                last_text = text;
                // Keep a partial result as a fallback rather than failing outright.
                if parsed.is_none() && !s.name.trim().is_empty() {
                    parsed = Some(s);
                }
            }
            Err(e) => {
                last_err = e;
                last_text = text;
            }
        }
    }
    let s = match parsed {
        Some(s) => s,
        None => {
            return err500(format!(
                "không đọc được gợi ý từ LLM ({last_err}): {}",
                llm::truncate(last_text.trim(), 200)
            ))
        }
    };

    // Keep only a material the form actually has; strip any "(name)" suffix the
    // model echoed back.
    let mat = s
        .material
        .trim()
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_string();
    let known: Vec<String> = st
        .core
        .db
        .query("SELECT id FROM material", &[])
        .unwrap_or_default()
        .iter()
        .map(|m| crate::db::str_of(m, "id"))
        .collect();
    let material = if known.iter().any(|k| *k == mat) {
        mat
    } else {
        String::new()
    };

    let language = match s.language.trim().to_lowercase().as_str() {
        "vi" | "vietnamese" | "tiếng việt" => "vi",
        "" => "",
        _ => "en",
    };

    respond(
        StatusCode::OK,
        json!({
            "suggestion": {
                "name": s.name.trim(),
                "story": s.story.trim(),
                "material": material,
                "language": language,
                "entities": [],
                "scene_hints": [],
            },
            "provider": "senclaw",
        }),
    )
}

async fn suggest_scenes(State(_st): State<AppState>, body: MaybeJson) -> Response {
    #[derive(Deserialize, Default)]
    struct Body {
        #[serde(default)]
        prompt: String,
        #[serde(default)]
        story: String,
        #[serde(default)]
        characters_hint: String,
    }
    let b: Body = match parse_body(body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let provider = "senclaw";

    let mut user_msg = String::from("Generate cinematic scene prompts for a video.\n");
    if !b.story.is_empty() {
        user_msg.push_str(&format!("STORY:\n{}\n\n", b.story));
    }
    if !b.characters_hint.is_empty() {
        user_msg.push_str(&format!("CHARACTERS: {}\n\n", b.characters_hint));
    }
    if !b.prompt.is_empty() {
        user_msg.push_str(&format!("INSTRUCTIONS:\n{}\n\n", b.prompt));
    }
    user_msg.push_str(
        "Return a JSON array of scene objects. Each object must have:\n\
         - \"order\" (integer starting at 0)\n\
         - \"prompt\" (cinematic image description, action-focused)\n\
         - \"video_prompt\" (camera movement and timing, e.g. \"0-3s: close-up...\")\n\
         - \"character_names\" (array of character name strings)\n\n\
         Example format:\n\
         [{\"order\":0,\"prompt\":\"...\",\"video_prompt\":\"...\",\"character_names\":[\"Hero\"]},...]\n\n\
         Return ONLY the JSON array, no markdown fences.",
    );

    let reply = match llm::complete(
        "You are a professional cinematographer and screenwriter.",
        &user_msg,
        8000,
    )
    .await
    {
        Ok((t, _)) => t,
        Err(e) => return err500(e),
    };

    let (Some(start), Some(end)) = (reply.find('['), reply.rfind(']')) else {
        return respond(
            StatusCode::OK,
            json!({ "scene_hints": [], "provider": provider }),
        );
    };
    if end <= start {
        return respond(
            StatusCode::OK,
            json!({ "scene_hints": [], "provider": provider }),
        );
    }
    let hints: Vec<Map<String, Value>> = match serde_json::from_str(&reply[start..=end]) {
        Ok(h) => h,
        Err(e) => {
            return respond(
                StatusCode::OK,
                json!({ "scene_hints": [], "provider": provider, "error": e.to_string() }),
            )
        }
    };
    let scenes: Vec<Value> = hints
        .iter()
        .enumerate()
        .map(|(i, h)| {
            let order = h.get("order").and_then(|v| v.as_i64()).unwrap_or(i as i64);
            let names: Vec<String> = h
                .get("character_names")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|x| x.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            json!({
                "order": order,
                "prompt": vstr(h.get("prompt")),
                "video_prompt": vstr(h.get("video_prompt")),
                "character_names": names,
            })
        })
        .collect();
    respond(
        StatusCode::OK,
        json!({ "scene_hints": scenes, "provider": provider }),
    )
}

async fn suggest_entities(State(_st): State<AppState>, body: MaybeJson) -> Response {
    #[derive(Deserialize, Default)]
    struct Body {
        #[serde(default)]
        story: String,
        #[serde(default)]
        prompt: String,
    }
    let b: Body = match parse_body(body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let provider = "senclaw";

    let mut user_msg = String::from(
        "Analyze the following story and suggest characters, locations, and key entities for a video production.\n\n",
    );
    if !b.story.is_empty() {
        user_msg.push_str(&format!("STORY:\n{}\n\n", b.story));
    }
    if !b.prompt.is_empty() {
        user_msg.push_str(&format!("ADDITIONAL INSTRUCTIONS:\n{}\n\n", b.prompt));
    }
    user_msg.push_str(
        "Return a JSON array of entity objects. Each must have:\n\
         - \"name\": entity name\n\
         - \"entity_type\": one of \"character\", \"location\", \"creature\", \"visual_asset\", \"generic_troop\", \"faction\"\n\
         - \"description\": brief visual/role description\n\n\
         Return ONLY the JSON array, no markdown fences.",
    );

    let reply = match llm::complete(
        "You are a creative film production designer.",
        &user_msg,
        4000,
    )
    .await
    {
        Ok((t, _)) => t,
        Err(e) => return err500(e),
    };
    let (Some(start), Some(end)) = (reply.find('['), reply.rfind(']')) else {
        return respond(
            StatusCode::OK,
            json!({ "entities": [], "provider": provider }),
        );
    };
    if end <= start {
        return respond(
            StatusCode::OK,
            json!({ "entities": [], "provider": provider }),
        );
    }
    let raw: Vec<Map<String, Value>> = match serde_json::from_str(&reply[start..=end]) {
        Ok(v) => v,
        Err(_) => {
            return respond(
                StatusCode::OK,
                json!({ "entities": [], "provider": provider }),
            )
        }
    };
    let out: Vec<Value> = raw
        .iter()
        .map(|e| {
            json!({
                "name": vstr(e.get("name")),
                "entity_type": vstr(e.get("entity_type")),
                "description": vstr(e.get("description")),
            })
        })
        .collect();
    respond(
        StatusCode::OK,
        json!({ "entities": out, "provider": provider }),
    )
}

async fn list_providers() -> Response {
    // SenClaw adaptation: the daemon bridge is the only provider.
    respond(StatusCode::OK, json!([{ "name": "senclaw", "env": "" }]))
}

// ---------- LLM settings (SenClaw adaptation) ----------

/// GET shape kept superset-compatible with the old UI: provider/model/base_url/
/// api_key/env_hints still present; `profile` + `profiles` are the SenClaw
/// LLM-config profile selection.
async fn llm_settings_payload(st: &AppState) -> Value {
    let profile = st.core.db.kv_get("llm.profile");
    let models = llm::list_models().await.unwrap_or_else(|_| json!({}));
    let raw = models
        .get("available")
        .cloned()
        .or_else(|| models.get("profiles").cloned())
        .or_else(|| models.get("configs").cloned())
        .unwrap_or_else(|| json!([]));
    // The daemon's llm-config entries carry provider secrets (`apiKey`,
    // `baseURL`). This response is served on the app's own port, so project
    // only the fields the picker needs — never pass the entries through.
    let profiles: Vec<Value> = raw
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|c| {
                    json!({
                        "id":        c.get("id").cloned().unwrap_or(Value::Null),
                        "label":     c.get("label").cloned().unwrap_or(Value::Null),
                        "model":     c.get("modelName").or_else(|| c.get("model")).cloned().unwrap_or(Value::Null),
                        "provider":  c.get("provider").cloned().unwrap_or(Value::Null),
                        "vision":    c.get("vision").cloned().unwrap_or(Value::Null),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    // Which Flow video model the pipeline submits with. `auto` follows what the
    // extension learns from Flow; `lite` is the 0-credit all-tier Veo 3.1 Lite;
    // `fast` is the credit-charged `_ultra` family (needs a higher service tier).
    let video_model = {
        let t = st.core.db.kv_get("video.model_tier");
        if t.is_empty() {
            "auto".to_string()
        } else {
            t
        }
    };
    json!({
        "provider": "senclaw",
        "profile": profile,
        "model": "",
        "profiles": profiles,
        "base_url": "",
        "api_key": "",
        "providers_catalog": [],
        "env_hints": {},
        "video_model": video_model,
        "video_model_learned": st.core.db.kv_get("flow.video_model"),
    })
}

async fn get_llm_settings(State(st): State<AppState>) -> Response {
    respond(StatusCode::OK, llm_settings_payload(&st).await)
}

async fn put_llm_settings(State(st): State<AppState>, body: MaybeJson) -> Response {
    #[derive(Deserialize, Default)]
    struct Body {
        #[serde(default)]
        profile: String,
        /// `auto` | `lite` | `fast`. Absent = leave the video model unchanged.
        #[serde(default)]
        video_model: Option<String>,
    }
    let b: Body = match parse_body(body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    if let Err(e) = st.core.db.kv_set("llm.profile", &b.profile) {
        return err500(e);
    }
    llm::set_profile(&b.profile);
    if let Some(vm) = b.video_model {
        let vm = vm.trim();
        if matches!(vm, "auto" | "lite" | "fast") {
            if let Err(e) = st.core.db.kv_set("video.model_tier", vm) {
                return err500(e);
            }
        }
    }
    respond(StatusCode::OK, llm_settings_payload(&st).await)
}

/// Download every still-remote generated asset into local media.
/// `{project_id}` scopes the sweep; omit it to repair the whole DB.
async fn localize_media(State(st): State<AppState>, body: MaybeJson) -> Response {
    #[derive(Deserialize, Default)]
    struct Body {
        #[serde(default)]
        project_id: String,
    }
    let b: Body = match parse_body(body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let rep = crate::mediastore::localize_project(&st.core, b.project_id.trim()).await;
    respond(
        StatusCode::OK,
        serde_json::to_value(rep).unwrap_or_else(|_| json!({})),
    )
}

/// Ask the extension to load the Flow project page so its tRPC calls expose
/// media URLs, then pull whatever it scraped into local storage.
async fn fetch_media_urls(State(st): State<AppState>, body: MaybeJson) -> Response {
    #[derive(Deserialize, Default)]
    struct Body {
        #[serde(default)]
        project_id: String,
    }
    let b: Body = match parse_body(body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let project_id = b.project_id.trim();
    if project_id.is_empty() {
        return err(StatusCode::BAD_REQUEST, "project_id là bắt buộc");
    }
    if let Err(e) = crate::process::recover_media_urls(&st.core, project_id).await {
        return err500(e);
    }
    // The scraper writes asynchronously; wait briefly so the caller sees the result.
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    let rep = crate::mediastore::localize_project(&st.core, project_id).await;
    let missing = st
        .core
        .db
        .query(
            "SELECT COUNT(*) AS n FROM scene s JOIN video v ON v.id = s.video_id \
             WHERE v.project_id = ?1 AND s.vertical_video_status = 'COMPLETED' \
             AND (s.vertical_video_url IS NULL OR s.vertical_video_url = '')",
            &[&project_id],
        )
        .ok()
        .and_then(|r| r.first().map(|x| db::i64_of(x, "n")))
        .unwrap_or(0);
    respond(
        StatusCode::OK,
        json!({
            "downloaded": rep.downloaded,
            "failed": rep.failed,
            "scenes_still_without_url": missing,
        }),
    )
}

async fn list_tools(State(st): State<AppState>) -> Response {
    let reg = crate::tools::registry(&st.core);
    let tools: Vec<Value> = reg
        .specs()
        .into_iter()
        .map(|s| json!({ "name": s.name, "description": s.description, "input_schema": s.input_schema }))
        .collect();
    respond(StatusCode::OK, json!({ "tools": tools }))
}

// ---------- materials ----------

async fn list_materials(State(st): State<AppState>) -> Response {
    match st
        .core
        .db
        .query("SELECT * FROM material ORDER BY name", &[])
    {
        Ok(rows) => respond(StatusCode::OK, json!({ "materials": rows_json(rows) })),
        Err(e) => err500(e),
    }
}

async fn create_material(State(st): State<AppState>, body: MaybeJson) -> Response {
    #[derive(Deserialize, Default)]
    struct Body {
        #[serde(default)]
        id: String,
        #[serde(default)]
        name: String,
        #[serde(default)]
        style_instruction: String,
        #[serde(default)]
        negative_prompt: String,
        #[serde(default)]
        scene_prefix: String,
        #[serde(default)]
        lighting: String,
    }
    let mut b: Body = match parse_body(body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    if b.id.is_empty() || b.name.is_empty() || b.style_instruction.is_empty() {
        return err(
            StatusCode::BAD_REQUEST,
            "id, name, style_instruction required",
        );
    }
    if b.lighting.is_empty() {
        b.lighting = "Studio lighting, highly detailed".to_string();
    }
    if let Err(e) = st.core.db.execute(
        "INSERT INTO material(id,name,style_instruction,negative_prompt,scene_prefix,lighting) VALUES(?1,?2,?3,?4,?5,?6)",
        &[&b.id, &b.name, &b.style_instruction, &b.negative_prompt, &b.scene_prefix, &b.lighting],
    ) {
        return err500(e);
    }
    match st
        .core
        .db
        .query_one("SELECT * FROM material WHERE id = ?1", &[&b.id])
    {
        Ok(row) => respond(StatusCode::CREATED, row_or_null(row)),
        Err(e) => err500(e),
    }
}

async fn delete_material(State(st): State<AppState>, Path(mid): Path<String>) -> Response {
    match st
        .core
        .db
        .execute("DELETE FROM material WHERE id = ?1", &[&mid])
    {
        Ok(_) => respond(StatusCode::OK, json!({ "ok": true })),
        Err(e) => err500(e),
    }
}

async fn seed_materials(State(st): State<AppState>) -> Response {
    let n = material::seed(&st.core.db);
    respond(
        StatusCode::OK,
        json!({ "inserted": n, "message": format!("Đã thêm {n} materials mới (bỏ qua trùng lặp)") }),
    )
}

async fn restore_materials(State(st): State<AppState>) -> Response {
    let n = material::restore(&st.core.db);
    respond(
        StatusCode::OK,
        json!({ "updated": n, "message": format!("Đã khôi phục {n} built-in materials") }),
    )
}

async fn import_materials(State(st): State<AppState>, body: axum::body::Bytes) -> Response {
    match material::import(&st.core.db, &body) {
        Ok((inserted, skipped)) => respond(
            StatusCode::OK,
            json!({
                "inserted": inserted,
                "skipped": skipped,
                "message": format!("Import xong: {inserted} thêm mới, {skipped} bỏ qua (đã tồn tại)"),
            }),
        ),
        Err(e) => {
            if e.starts_with("insert ") {
                err500(e)
            } else {
                err(StatusCode::BAD_REQUEST, e)
            }
        }
    }
}

// ---------- skills (playbook catalog) ----------

async fn list_skills(State(st): State<AppState>) -> Response {
    match skillcat::scan(&st.core.playbooks_dir) {
        Ok(skills) => respond(
            StatusCode::OK,
            serde_json::to_value(&skills).unwrap_or_default(),
        ),
        Err(e) => err500(e),
    }
}

// ---------- media ----------

/// Every scene/character/request media_id column that can tie a media row to a
/// project (port of the Go ListMedia subquery block; 10 `?` args).
const PROJECT_MEDIA_COND: &str = "(\
 m.id IN (SELECT c.media_id FROM character c JOIN project_character pc ON pc.character_id = c.id \
   WHERE pc.project_id = ? AND c.media_id IS NOT NULL AND c.media_id != '') \
 OR m.id IN (SELECT r.media_id FROM request r \
   WHERE r.project_id = ? AND r.media_id IS NOT NULL AND r.media_id != '') \
 OR m.id IN (SELECT s.vertical_image_media_id FROM scene s JOIN video v ON v.id = s.video_id \
   WHERE v.project_id = ? AND s.vertical_image_media_id IS NOT NULL AND s.vertical_image_media_id != '') \
 OR m.id IN (SELECT s.horizontal_image_media_id FROM scene s JOIN video v ON v.id = s.video_id \
   WHERE v.project_id = ? AND s.horizontal_image_media_id IS NOT NULL AND s.horizontal_image_media_id != '') \
 OR m.id IN (SELECT s.vertical_video_media_id FROM scene s JOIN video v ON v.id = s.video_id \
   WHERE v.project_id = ? AND s.vertical_video_media_id IS NOT NULL AND s.vertical_video_media_id != '') \
 OR m.id IN (SELECT s.horizontal_video_media_id FROM scene s JOIN video v ON v.id = s.video_id \
   WHERE v.project_id = ? AND s.horizontal_video_media_id IS NOT NULL AND s.horizontal_video_media_id != '') \
 OR m.id IN (SELECT s.vertical_upscale_media_id FROM scene s JOIN video v ON v.id = s.video_id \
   WHERE v.project_id = ? AND s.vertical_upscale_media_id IS NOT NULL AND s.vertical_upscale_media_id != '') \
 OR m.id IN (SELECT s.horizontal_upscale_media_id FROM scene s JOIN video v ON v.id = s.video_id \
   WHERE v.project_id = ? AND s.horizontal_upscale_media_id IS NOT NULL AND s.horizontal_upscale_media_id != '') \
 OR m.id IN (SELECT s.vertical_end_scene_media_id FROM scene s JOIN video v ON v.id = s.video_id \
   WHERE v.project_id = ? AND s.vertical_end_scene_media_id IS NOT NULL AND s.vertical_end_scene_media_id != '') \
 OR m.id IN (SELECT s.horizontal_end_scene_media_id FROM scene s JOIN video v ON v.id = s.video_id \
   WHERE v.project_id = ? AND s.horizontal_end_scene_media_id IS NOT NULL AND s.horizontal_end_scene_media_id != '') \
)";

async fn list_media(
    State(st): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let media_type = q.get("type").cloned().unwrap_or_default();
    let search = q
        .get("search")
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let project_id = q
        .get("project_id")
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let orientation = q
        .get("orientation")
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    let mut conds: Vec<String> = Vec::new();
    let mut args: Vec<String> = Vec::new();
    if !media_type.is_empty() {
        conds.push("m.media_type = ?".to_string());
        args.push(media_type);
    }
    if !search.is_empty() {
        conds.push("LOWER(m.file_name) LIKE ?".to_string());
        args.push(format!("%{}%", search.to_lowercase()));
    }
    if !project_id.is_empty() {
        conds.push(PROJECT_MEDIA_COND.to_string());
        for _ in 0..10 {
            args.push(project_id.clone());
        }
    }
    match orientation.to_lowercase().as_str() {
        "portrait" => conds
            .push("m.width_px > 0 AND m.height_px > 0 AND m.height_px > m.width_px".to_string()),
        "landscape" => conds
            .push("m.width_px > 0 AND m.height_px > 0 AND m.width_px > m.height_px".to_string()),
        "square" => conds.push(
            "m.width_px > 0 AND m.height_px > 0 AND ABS(m.width_px - m.height_px) <= 2".to_string(),
        ),
        "unknown" => conds.push("(m.width_px = 0 OR m.height_px = 0)".to_string()),
        _ => {}
    }
    let mut sql = "SELECT m.* FROM media m".to_string();
    if !conds.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&conds.join(" AND "));
    }
    sql.push_str(" ORDER BY m.created_at DESC");
    let params: Vec<&dyn rusqlite::ToSql> =
        args.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
    match st.core.db.query(&sql, &params) {
        Ok(rows) => respond(StatusCode::OK, rows_json(rows)),
        Err(e) => err500(e),
    }
}

async fn get_media(State(st): State<AppState>, Path(mid): Path<String>) -> Response {
    match st.core.db.get("media", &mid) {
        Ok(Some(row)) => respond(StatusCode::OK, Value::Object(row)),
        _ => err(StatusCode::NOT_FOUND, "not found"),
    }
}

async fn delete_media(State(st): State<AppState>, Path(mid): Path<String>) -> Response {
    match media::delete_one_media(&st.core.db, &mid) {
        Ok(true) => err(StatusCode::NOT_FOUND, "not found"),
        Ok(false) => respond(StatusCode::OK, json!({ "ok": true })),
        Err(e) => err500(e),
    }
}

async fn delete_media_batch(State(st): State<AppState>, body: MaybeJson) -> Response {
    #[derive(Deserialize, Default)]
    struct Body {
        #[serde(default)]
        ids: Vec<String>,
    }
    let b: Body = match parse_body(body) {
        Ok(b) => b,
        Err(_) => return err(StatusCode::BAD_REQUEST, "invalid body"),
    };
    let mut uniq: Vec<String> = Vec::new();
    for id in b.ids {
        let id = id.trim().to_string();
        if id.is_empty() || uniq.contains(&id) {
            continue;
        }
        uniq.push(id);
    }
    if uniq.is_empty() {
        return err(StatusCode::BAD_REQUEST, "ids required");
    }
    let mut deleted = 0;
    let mut missing: Vec<String> = Vec::new();
    for id in &uniq {
        match media::delete_one_media(&st.core.db, id) {
            Ok(true) => missing.push(id.clone()),
            Ok(false) => deleted += 1,
            Err(e) => return err500(e),
        }
    }
    respond(
        StatusCode::OK,
        json!({ "deleted": deleted, "requested": uniq.len(), "missing_ids": missing }),
    )
}

// ---------- agent image (on-demand) ----------

async fn agent_generate_image(State(st): State<AppState>, body: MaybeJson) -> Response {
    #[derive(Deserialize, Default)]
    struct Body {
        #[serde(default)]
        mode: String,
        #[serde(default)]
        project_id: String,
        #[serde(default)]
        character_id: String,
        #[serde(default)]
        scene_id: String,
        #[serde(default)]
        video_id: String,
        #[serde(default)]
        orientation: String,
    }
    let mut b: Body = match parse_body(body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    if b.project_id.is_empty() {
        return err(StatusCode::BAD_REQUEST, "project_id required");
    }
    if b.mode.is_empty() {
        b.mode = "all_scenes".to_string();
    }
    if b.orientation.is_empty() {
        b.orientation = "VERTICAL".to_string();
    }
    if st.pool.get("image").is_none() {
        return err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "image agent not initialized",
        );
    }

    let rid = db::new_id();
    let mut req = Map::new();
    req.insert("id".into(), json!(rid));
    req.insert("project_id".into(), json!(b.project_id));
    req.insert(
        "type".into(),
        json!(format!("AGENT_IMAGE_{}", b.mode.to_uppercase())),
    );
    req.insert("orientation".into(), json!(b.orientation));
    req.insert("status".into(), json!("PROCESSING"));
    if !b.character_id.is_empty() {
        req.insert("character_id".into(), json!(b.character_id));
    }
    if !b.scene_id.is_empty() {
        req.insert("scene_id".into(), json!(b.scene_id));
    }
    if !b.video_id.is_empty() {
        req.insert("video_id".into(), json!(b.video_id));
    }
    if let Err(e) = st.core.db.insert("request", &req) {
        return err500(e);
    }

    if let Err(e) = schedule_image_dag_task(
        &st,
        &b.project_id,
        &rid,
        &b.mode,
        &b.character_id,
        &b.scene_id,
        &b.video_id,
        &b.orientation,
    ) {
        let mut up = Map::new();
        up.insert("status".into(), json!("FAILED"));
        up.insert("error_message".into(), json!(e.to_string()));
        let _ = st.core.db.update("request", &rid, &up);
        return err500(e);
    }

    respond(
        StatusCode::ACCEPTED,
        json!({ "request_id": rid, "status": "queued" }),
    )
}
