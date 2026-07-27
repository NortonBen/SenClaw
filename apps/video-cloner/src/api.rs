//! REST API. Paths are registered without the `/api` prefix; `main.rs` nests them.

use crate::db::CloneConfig;
use crate::mcp::{mcp_message, mcp_sse};
use crate::process::{self, Mode};
use crate::scenes::{self, ReplaceRequest, Voice};
use crate::state::AppState;
use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Multipart, Path, Query, State, WebSocketUpgrade},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Videos are the payload here; the axum default of 2 MB is far too small.
const MAX_UPLOAD_BYTES: usize = 2 * 1024 * 1024 * 1024;

fn respond(v: Value) -> Response {
    Json(v).into_response()
}

fn err(status: StatusCode, msg: impl Into<String>) -> Response {
    (status, Json(json!({ "error": msg.into() }))).into_response()
}

fn err400(msg: impl Into<String>) -> Response {
    err(StatusCode::BAD_REQUEST, msg)
}

fn err404(msg: impl Into<String>) -> Response {
    err(StatusCode::NOT_FOUND, msg)
}

fn err500(e: impl std::fmt::Display) -> Response {
    err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

pub fn root_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ws/dashboard", get(ws_dashboard))
        .with_state(state)
}

pub fn api_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/status", get(status))
        .route("/settings", get(get_settings).put(put_settings))
        .route("/presets", get(presets))
        .route("/projects", get(list_projects).post(create_project))
        .route("/projects/:id", get(get_project).delete(delete_project))
        .route("/projects/:id/config", put(update_config))
        .route("/projects/:id/char-image", post(upload_char_image))
        .route("/projects/:id/char-image", delete(clear_char_image))
        .route("/projects/:id/video", get(stream_video))
        .route("/projects/:id/scenes", get(get_scenes))
        .route("/projects/:id/analyze", post(analyze))
        .route("/projects/:id/replace", post(replace))
        .route("/projects/:id/export", get(export))
        .route("/projects/:id/export/bundle", get(export_bundle))
        .route("/projects/:id/export/markdown", get(export_markdown))
        .route("/projects/:id/export/file", post(export_to_dir))
        .route("/projects/:id/export/wiki", post(export_to_wiki))
        .route("/projects/:id/handoff/video-flow", post(handoff_video_flow))
        .route("/projects/:id/jobs", get(list_jobs))
        .route("/projects/:id/snapshots", get(list_snapshots))
        .route("/projects/:id/restore", post(restore))
        .route("/snapshots/:id", get(get_snapshot))
        .route("/jobs/:id", get(get_job))
        .route("/jobs/:id/raw", get(job_raw))
        .route("/mcp/sse", get(mcp_sse).post(mcp_message))
        .route("/mcp/message", post(mcp_message))
        .layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES))
        .with_state(state)
}

// ---- health ----

async fn health(State(state): State<AppState>) -> Response {
    status(State(state)).await
}

async fn status(State(state): State<AppState>) -> Response {
    let db = &state.core.db;
    let projects = db.list_projects().map(|p| p.len()).unwrap_or(0);
    respond(json!({
        "ok": true,
        "app": "video-cloner",
        "version": env!("CARGO_PKG_VERSION"),
        "projects": projects,
        "has_api_key": !db.gemini_api_key().trim().is_empty(),
    }))
}

// ---- settings ----

async fn get_settings(State(state): State<AppState>) -> Response {
    let db = &state.core.db;
    let all: HashMap<String, String> = match db.all_settings() {
        Ok(v) => v.into_iter().collect(),
        Err(e) => return err500(e),
    };
    // Never echo the key back; the UI only needs to know whether one is set.
    respond(json!({
        "has_api_key": !db.gemini_api_key().trim().is_empty(),
        "api_key_from_env": all.get("gemini_api_key").map(|v| v.trim().is_empty()).unwrap_or(true)
            && !crate::config::env_gemini_api_key().is_empty(),
        "default_model": all.get("default_model").cloned().unwrap_or_default(),
    }))
}

#[derive(Deserialize)]
struct SettingsBody {
    gemini_api_key: Option<String>,
    default_model: Option<String>,
}

async fn put_settings(State(state): State<AppState>, body: Bytes) -> Response {
    let body: SettingsBody = match serde_json::from_slice(&body) {
        Ok(b) => b,
        Err(e) => return err400(format!("body không hợp lệ: {e}")),
    };
    let db = &state.core.db;
    if let Some(k) = body.gemini_api_key {
        if let Err(e) = db.set_setting("gemini_api_key", k.trim()) {
            return err500(e);
        }
    }
    if let Some(m) = body.default_model {
        if !m.trim().is_empty() {
            if let Err(e) = db.set_setting("default_model", m.trim()) {
                return err500(e);
            }
        }
    }
    get_settings(State(state)).await
}

/// The style / character / background pickers, served from the backend so the
/// MCP tools and the UI offer the same choices.
async fn presets() -> Response {
    respond(json!({
        "styles": crate::presets::STYLES,
        "models": crate::presets::models(),
        "characters": crate::presets::character_presets(),
        "backgrounds": crate::presets::background_presets(),
    }))
}

// ---- projects ----

async fn list_projects(State(state): State<AppState>) -> Response {
    match state.core.db.list_projects() {
        Ok(items) => {
            let enriched: Vec<Value> = items
                .into_iter()
                .map(|p| {
                    let count = state.core.db.scene_count(p.id).unwrap_or(0);
                    let busy = state.core.is_busy(p.id);
                    let mut v = serde_json::to_value(&p).unwrap_or(Value::Null);
                    v["scene_count"] = json!(count);
                    v["running"] = json!(busy);
                    v
                })
                .collect();
            respond(json!({ "projects": enriched }))
        }
        Err(e) => err500(e),
    }
}

async fn create_project(State(state): State<AppState>, mut mp: Multipart) -> Response {
    let db = &state.core.db;
    let mut cfg = CloneConfig {
        style: crate::presets::STYLES[0].to_string(),
        model: db.setting("default_model", "gemini-3-flash-preview"),
        visual_similarity: 100,
        ..Default::default()
    };
    let mut name = String::new();
    let mut video: Option<(String, String, Vec<u8>)> = None;
    let mut char_image: Option<(String, Vec<u8>)> = None;

    loop {
        let field = match mp.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) => return err400(format!("đọc dữ liệu tải lên thất bại: {e}")),
        };
        let field_name = field.name().unwrap_or("").to_string();
        let file_name = field.file_name().unwrap_or("").to_string();
        let mime = field.content_type().unwrap_or("").to_string();

        match field_name.as_str() {
            "video" => {
                let bytes = match field.bytes().await {
                    Ok(b) => b,
                    Err(e) => return err400(format!("đọc file video thất bại: {e}")),
                };
                let mime = if mime.is_empty() { "video/mp4".to_string() } else { mime };
                video = Some((mime, file_name, bytes.to_vec()));
            }
            "char_image" => {
                let bytes = match field.bytes().await {
                    Ok(b) => b,
                    Err(e) => return err400(format!("đọc ảnh nhân vật thất bại: {e}")),
                };
                if !bytes.is_empty() {
                    let mime = if mime.is_empty() { "image/jpeg".to_string() } else { mime };
                    char_image = Some((mime, bytes.to_vec()));
                }
            }
            other => {
                let text = field.text().await.unwrap_or_default();
                apply_config_field(&mut cfg, &mut name, other, &text);
            }
        }
    }

    let Some((mime, filename, bytes)) = video else {
        return err400("thiếu file video (field \"video\")");
    };
    if bytes.is_empty() {
        return err400("file video rỗng");
    }
    if name.trim().is_empty() {
        name = if filename.trim().is_empty() {
            "Dự án không tên".to_string()
        } else {
            filename.clone()
        };
    }

    let size = bytes.len() as i64;
    let stored = match store_media(&bytes, &filename, "video").await {
        Ok(p) => p,
        Err(e) => return err500(e),
    };

    let id = match db.create_project(name.trim(), &stored, &mime, size, &filename, &cfg) {
        Ok(id) => id,
        Err(e) => return err500(e),
    };

    if let Some((img_mime, img_bytes)) = char_image {
        match store_media(&img_bytes, "char", "image").await {
            Ok(p) => {
                if let Err(e) = db.set_char_image(id, &p, &img_mime) {
                    return err500(e);
                }
            }
            Err(e) => return err500(e),
        }
    }

    state
        .core
        .dash
        .emit("project:created", json!({ "project_id": id }));

    match db.project(id) {
        Ok(Some(p)) => respond(json!({ "project": p })),
        Ok(None) => err500("không đọc lại được dự án vừa tạo"),
        Err(e) => err500(e),
    }
}

fn apply_config_field(cfg: &mut CloneConfig, name: &mut String, key: &str, value: &str) {
    match key {
        "name" => *name = value.to_string(),
        "style" => {
            if !value.trim().is_empty() {
                cfg.style = value.to_string()
            }
        }
        "model" => {
            if !value.trim().is_empty() {
                cfg.model = value.to_string()
            }
        }
        "char_description" => cfg.char_description = value.to_string(),
        "custom_dialogue" => cfg.custom_dialogue = value.to_string(),
        "bg_description" => cfg.bg_description = value.to_string(),
        "auto_magic" => cfg.auto_magic = matches!(value.trim(), "1" | "true" | "yes"),
        "visual_similarity" => {
            if let Ok(v) = value.trim().parse::<i64>() {
                cfg.visual_similarity = v.clamp(0, 100);
            }
        }
        _ => {}
    }
}

/// Persist an upload under the media directory with a collision-free name.
async fn store_media(bytes: &[u8], original: &str, kind: &str) -> anyhow::Result<String> {
    let dir = crate::config::media_dir();
    tokio::fs::create_dir_all(&dir).await?;

    let ext = std::path::Path::new(original)
        .extension()
        .and_then(|e| e.to_str())
        .filter(|e| e.len() <= 8 && e.chars().all(|c| c.is_ascii_alphanumeric()))
        .map(|e| format!(".{e}"))
        .unwrap_or_else(|| if kind == "video" { ".mp4".into() } else { ".jpg".into() });

    let stamp = chrono::Utc::now().format("%Y%m%d%H%M%S%3f");
    let path = dir.join(format!("{kind}-{stamp}{ext}"));
    tokio::fs::write(&path, bytes).await?;
    Ok(path.to_string_lossy().to_string())
}

async fn get_project(State(state): State<AppState>, Path(id): Path<i64>) -> Response {
    match state.core.db.project(id) {
        Ok(Some(p)) => {
            let scenes = state.core.db.scenes(id).unwrap_or_default();
            let values: Vec<Value> = scenes.iter().map(|s| s.json.clone()).collect();
            let job = state.core.db.latest_job(id).ok().flatten();
            respond(json!({
                "project": p,
                "scene_count": values.len(),
                "characters": scenes::detect_characters(&values),
                "running": state.core.is_busy(id),
                "latest_job": job,
            }))
        }
        Ok(None) => err404("không tìm thấy dự án"),
        Err(e) => err500(e),
    }
}

async fn delete_project(State(state): State<AppState>, Path(id): Path<i64>) -> Response {
    let project = match state.core.db.project(id) {
        Ok(Some(p)) => p,
        Ok(None) => return err404("không tìm thấy dự án"),
        Err(e) => return err500(e),
    };
    if let Err(e) = state.core.db.delete_project(id) {
        return err500(e);
    }
    // Best-effort media cleanup; a leftover file must not fail the delete.
    let _ = tokio::fs::remove_file(&project.video_path).await;
    if !project.char_image_path.is_empty() {
        let _ = tokio::fs::remove_file(&project.char_image_path).await;
    }
    state
        .core
        .dash
        .emit("project:deleted", json!({ "project_id": id }));
    respond(json!({ "ok": true }))
}

async fn update_config(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    body: Bytes,
) -> Response {
    let patch: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return err400(format!("body không hợp lệ: {e}")),
    };
    let project = match state.core.db.project(id) {
        Ok(Some(p)) => p,
        Ok(None) => return err404("không tìm thấy dự án"),
        Err(e) => return err500(e),
    };

    if let Some(name) = patch.get("name").and_then(|v| v.as_str()) {
        if !name.trim().is_empty() {
            if let Err(e) = state.core.db.set_project_name(id, name.trim()) {
                return err500(e);
            }
        }
    }

    let cfg = merge_config(&CloneConfig::from(&project), &patch);
    if let Err(e) = state.core.db.update_project_config(id, &cfg) {
        return err500(e);
    }
    get_project(State(state), Path(id)).await
}

pub fn merge_config(base: &CloneConfig, patch: &Value) -> CloneConfig {
    let mut cfg = base.clone();
    if let Some(v) = patch.get("style").and_then(|v| v.as_str()) {
        if !v.trim().is_empty() {
            cfg.style = v.to_string();
        }
    }
    if let Some(v) = patch.get("model").and_then(|v| v.as_str()) {
        if !v.trim().is_empty() {
            cfg.model = v.to_string();
        }
    }
    if let Some(v) = patch.get("char_description").and_then(|v| v.as_str()) {
        cfg.char_description = v.to_string();
    }
    if let Some(v) = patch.get("custom_dialogue").and_then(|v| v.as_str()) {
        cfg.custom_dialogue = v.to_string();
    }
    if let Some(v) = patch.get("bg_description").and_then(|v| v.as_str()) {
        cfg.bg_description = v.to_string();
    }
    if let Some(v) = patch.get("auto_magic").and_then(|v| v.as_bool()) {
        cfg.auto_magic = v;
    }
    if let Some(v) = patch.get("visual_similarity").and_then(|v| v.as_i64()) {
        cfg.visual_similarity = v.clamp(0, 100);
    }
    cfg
}

async fn upload_char_image(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    mut mp: Multipart,
) -> Response {
    if matches!(state.core.db.project(id), Ok(None)) {
        return err404("không tìm thấy dự án");
    }
    while let Ok(Some(field)) = mp.next_field().await {
        if field.name().unwrap_or("") != "char_image" {
            continue;
        }
        let mime = field.content_type().unwrap_or("image/jpeg").to_string();
        let bytes = match field.bytes().await {
            Ok(b) => b,
            Err(e) => return err400(format!("đọc ảnh thất bại: {e}")),
        };
        if bytes.is_empty() {
            return err400("ảnh rỗng");
        }
        return match store_media(&bytes, "char", "image").await {
            Ok(p) => match state.core.db.set_char_image(id, &p, &mime) {
                Ok(()) => respond(json!({ "ok": true })),
                Err(e) => err500(e),
            },
            Err(e) => err500(e),
        };
    }
    err400("thiếu field \"char_image\"")
}

async fn clear_char_image(State(state): State<AppState>, Path(id): Path<i64>) -> Response {
    match state.core.db.set_char_image(id, "", "") {
        Ok(()) => respond(json!({ "ok": true })),
        Err(e) => err500(e),
    }
}

/// Serve the stored video back so the UI can preview it.
async fn stream_video(State(state): State<AppState>, Path(id): Path<i64>) -> Response {
    let project = match state.core.db.project(id) {
        Ok(Some(p)) => p,
        Ok(None) => return err404("không tìm thấy dự án"),
        Err(e) => return err500(e),
    };
    match tokio::fs::read(&project.video_path).await {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, project.video_mime.as_str()),
                (header::ACCEPT_RANGES, "bytes"),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => err404("file video không còn trên đĩa"),
    }
}

// ---- scenes & analysis ----

async fn get_scenes(State(state): State<AppState>, Path(id): Path<i64>) -> Response {
    match state.core.db.scenes(id) {
        Ok(items) => {
            let values: Vec<Value> = items.iter().map(|s| s.json.clone()).collect();
            respond(json!({
                "scenes": items,
                "characters": scenes::detect_characters(&values),
                "text": scenes::export_text(&values),
            }))
        }
        Err(e) => err500(e),
    }
}

async fn analyze(State(state): State<AppState>, Path(id): Path<i64>, body: Bytes) -> Response {
    let patch: Value = if body.is_empty() {
        json!({})
    } else {
        match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(e) => return err400(format!("body không hợp lệ: {e}")),
        }
    };

    let project = match state.core.db.project(id) {
        Ok(Some(p)) => p,
        Ok(None) => return err404("không tìm thấy dự án"),
        Err(e) => return err500(e),
    };

    let mode_str = patch.get("mode").and_then(|v| v.as_str()).unwrap_or("start");
    let Some(mode) = Mode::parse(mode_str) else {
        return err400(format!(
            "mode không hợp lệ: {mode_str} (dùng start | continue | regenerate)"
        ));
    };

    let scene_count = state.core.db.scene_count(id).unwrap_or(0);
    if matches!(mode, Mode::Continue | Mode::Regenerate) && scene_count == 0 {
        return err400("chưa có scene nào — chạy mode \"start\" trước");
    }

    // Config sent with the request is persisted, so the next segment inherits it.
    let cfg = merge_config(&CloneConfig::from(&project), &patch);
    if let Err(e) = state.core.db.update_project_config(id, &cfg) {
        return err500(e);
    }
    let project = match state.core.db.project(id) {
        Ok(Some(p)) => p,
        _ => project,
    };

    match process::start(&state.core, &project, mode, cfg) {
        Ok(job_id) => respond(json!({
            "job_id": job_id,
            "mode": mode.as_str(),
            "status": "queued",
            "next": "poll GET /api/jobs/{job_id} — một đoạn 8 giây thường mất vài phút",
        })),
        Err(e) => err(StatusCode::CONFLICT, e.to_string()),
    }
}

async fn get_job(State(state): State<AppState>, Path(id): Path<i64>) -> Response {
    match state.core.db.job(id) {
        Ok(Some(j)) => {
            let total = state.core.db.scene_count(j.project_id).unwrap_or(0);
            let mut v = serde_json::to_value(&j).unwrap_or(Value::Null);
            v["total_scenes"] = json!(total);
            respond(json!({ "job": v }))
        }
        Ok(None) => err404("không tìm thấy tiến trình"),
        Err(e) => err500(e),
    }
}

#[derive(Deserialize, Default)]
struct ReplaceBody {
    #[serde(default)]
    find: String,
    #[serde(default)]
    replace: String,
    #[serde(default)]
    only_with_dialogue: bool,
    /// char_id → "male" | "female"
    #[serde(default)]
    voice_overrides: HashMap<String, String>,
}

async fn replace(State(state): State<AppState>, Path(id): Path<i64>, body: Bytes) -> Response {
    let req: ReplaceBody = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return err400(format!("body không hợp lệ: {e}")),
    };
    if req.find.trim().is_empty() && req.voice_overrides.is_empty() {
        return err400("cần ít nhất \"find\" hoặc \"voice_overrides\"");
    }

    let project = match state.core.db.project(id) {
        Ok(Some(p)) => p,
        Ok(None) => return err404("không tìm thấy dự án"),
        Err(e) => return err500(e),
    };

    let mut voices = HashMap::new();
    for (char_id, v) in &req.voice_overrides {
        match Voice::parse(v) {
            Some(voice) => {
                voices.insert(char_id.clone(), voice);
            }
            None => return err400(format!("giọng không hợp lệ cho {char_id}: {v} (male | female)")),
        }
    }

    let stored = match state.core.db.scenes(id) {
        Ok(s) => s,
        Err(e) => return err500(e),
    };
    let values: Vec<Value> = stored.iter().map(|s| s.json.clone()).collect();

    let outcome = match scenes::apply_replace(
        &values,
        &ReplaceRequest {
            find: req.find.clone(),
            replace: req.replace.clone(),
            only_with_dialogue: req.only_with_dialogue,
            voice_overrides: voices,
            style: project.style.clone(),
        },
    ) {
        Ok(o) => o,
        Err(e) => return err400(e),
    };

    // A bulk edit rewrites every scene at once, so it gets a restore point
    // before the write, not after.
    let label = if req.find.trim().is_empty() {
        "trước khi đổi giọng hàng loạt".to_string()
    } else {
        format!("trước khi đổi \"{}\" → \"{}\"", req.find.trim(), req.replace.trim())
    };
    let snapshot_id = state.core.db.snapshot(id, "replace", &label).ok().flatten();

    // Keep each scene pointing at the run that produced it.
    let entries = pair_with_jobs(&stored, &outcome.scenes);
    if let Err(e) = state.core.db.replace_all_scenes(id, &entries) {
        return err500(e);
    }
    state
        .core
        .dash
        .emit("scenes:updated", json!({ "project_id": id }));

    respond(json!({
        "ok": true,
        "replaced_text": outcome.replaced_text,
        "voices_applied": outcome.voices_applied,
        "characters": scenes::detect_characters(&outcome.scenes),
        "snapshot_id": snapshot_id,
    }))
}

/// Re-attach the originating run to each rewritten scene.
///
/// A bulk edit maps 1:1 over the stored list, so position carries the link; a
/// scene with no counterpart (there should be none) falls back to 0.
pub fn pair_with_jobs(stored: &[crate::db::Scene], edited: &[Value]) -> Vec<(i64, Value)> {
    edited
        .iter()
        .enumerate()
        .map(|(i, v)| (stored.get(i).map(|s| s.job_id).unwrap_or(0), v.clone()))
        .collect()
}

// ---- history ----

async fn list_jobs(State(state): State<AppState>, Path(id): Path<i64>) -> Response {
    match state.core.db.list_jobs(id, 100) {
        Ok(jobs) => respond(json!({ "jobs": jobs })),
        Err(e) => err500(e),
    }
}

async fn job_raw(State(state): State<AppState>, Path(id): Path<i64>) -> Response {
    match state.core.db.job_raw(id) {
        Ok(Some(raw)) => respond(json!({ "job_id": id, "raw": raw, "chars": raw.chars().count() })),
        Ok(None) => err404("không tìm thấy tiến trình"),
        Err(e) => err500(e),
    }
}

async fn list_snapshots(State(state): State<AppState>, Path(id): Path<i64>) -> Response {
    match state.core.db.list_snapshots(id) {
        Ok(items) => respond(json!({ "snapshots": items })),
        Err(e) => err500(e),
    }
}

async fn get_snapshot(State(state): State<AppState>, Path(id): Path<i64>) -> Response {
    let meta = match state.core.db.snapshot_meta(id) {
        Ok(Some(m)) => m,
        Ok(None) => return err404("không tìm thấy điểm khôi phục"),
        Err(e) => return err500(e),
    };
    let scenes_json = state.core.db.snapshot_scenes(id).ok().flatten().unwrap_or_default();
    respond(json!({
        "snapshot": meta,
        "scenes": scenes_json,
        "text": scenes::export_text(&scenes_json),
    }))
}

#[derive(Deserialize)]
struct RestoreBody {
    snapshot_id: i64,
}

/// Roll a project's scenes back to a restore point.
///
/// The current state is snapshotted first, so restoring is itself undoable and
/// a mis-click cannot become the second irreversible step.
async fn restore(State(state): State<AppState>, Path(id): Path<i64>, body: Bytes) -> Response {
    let req: RestoreBody = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return err400(format!("body không hợp lệ: {e}")),
    };

    let meta = match state.core.db.snapshot_meta(req.snapshot_id) {
        Ok(Some(m)) => m,
        Ok(None) => return err404("không tìm thấy điểm khôi phục"),
        Err(e) => return err500(e),
    };
    if meta.project_id != id {
        return err400("điểm khôi phục này thuộc dự án khác");
    }
    if state.core.is_busy(id) {
        return err(
            StatusCode::CONFLICT,
            "dự án đang chạy phân tích — chờ xong rồi hãy khôi phục",
        );
    }

    let scenes_json = match state.core.db.snapshot_scenes(req.snapshot_id) {
        Ok(Some(s)) => s,
        Ok(None) => return err404("điểm khôi phục không còn nội dung"),
        Err(e) => return err500(e),
    };

    let undo = state
        .core
        .db
        .snapshot(id, "restore", &format!("trước khi khôi phục #{}", req.snapshot_id))
        .ok()
        .flatten();

    // Restored scenes no longer belong to the run that is current, hence job 0.
    let entries: Vec<(i64, Value)> = scenes_json.iter().map(|v| (0, v.clone())).collect();
    if let Err(e) = state.core.db.replace_all_scenes(id, &entries) {
        return err500(e);
    }

    state
        .core
        .dash
        .emit("scenes:updated", json!({ "project_id": id }));

    respond(json!({
        "ok": true,
        "restored_scenes": scenes_json.len(),
        "undo_snapshot_id": undo,
    }))
}

// ---- export & handoff ----

/// Load a project plus its scenes, or the response explaining why not.
fn load_for_export(state: &AppState, id: i64) -> Result<(crate::db::Project, Vec<crate::db::Scene>), Response> {
    let project = match state.core.db.project(id) {
        Ok(Some(p)) => p,
        Ok(None) => return Err(err404("không tìm thấy dự án")),
        Err(e) => return Err(err500(e)),
    };
    let stored = match state.core.db.scenes(id) {
        Ok(s) => s,
        Err(e) => return Err(err500(e)),
    };
    if stored.is_empty() {
        return Err(err400("dự án chưa có đoạn nào để xuất"));
    }
    Ok((project, stored))
}

#[derive(Deserialize, Default)]
struct BundleQuery {
    #[serde(default)]
    download: bool,
}

async fn export_bundle(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(q): Query<BundleQuery>,
) -> Response {
    let (project, stored) = match load_for_export(&state, id) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let bundle = crate::export::bundle(&project, &stored, &crate::db::now());

    if !q.download {
        return respond(bundle);
    }
    let body = serde_json::to_string_pretty(&bundle).unwrap_or_default();
    (
        [
            (header::CONTENT_TYPE, "application/json; charset=utf-8".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!(
                    "attachment; filename=\"{}.bundle.json\"",
                    crate::export::slug(&project.name, project.id)
                ),
            ),
        ],
        body,
    )
        .into_response()
}

async fn export_markdown(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(q): Query<BundleQuery>,
) -> Response {
    let (project, stored) = match load_for_export(&state, id) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let md = crate::export::markdown(&project, &stored, &crate::db::now());

    if !q.download {
        return respond(json!({ "markdown": md }));
    }
    (
        [
            (header::CONTENT_TYPE, "text/markdown; charset=utf-8".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!(
                    "attachment; filename=\"{}.md\"",
                    crate::export::slug(&project.name, project.id)
                ),
            ),
        ],
        md,
    )
        .into_response()
}

/// Write the bundle and the screenplay into the shared export directory, where
/// any other Space App can pick them up without knowing this app's internals.
async fn export_to_dir(State(state): State<AppState>, Path(id): Path<i64>) -> Response {
    let (project, stored) = match load_for_export(&state, id) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let now = crate::db::now();
    let slug = crate::export::slug(&project.name, project.id);
    let dir = crate::config::export_dir();

    if let Err(e) = tokio::fs::create_dir_all(&dir).await {
        return err500(format!("không tạo được thư mục {}: {e}", dir.display()));
    }

    let bundle_path = dir.join(format!("{slug}.bundle.json"));
    let md_path = dir.join(format!("{slug}.md"));
    let bundle = crate::export::bundle(&project, &stored, &now);

    if let Err(e) = tokio::fs::write(
        &bundle_path,
        serde_json::to_string_pretty(&bundle).unwrap_or_default(),
    )
    .await
    {
        return err500(format!("ghi {} thất bại: {e}", bundle_path.display()));
    }
    if let Err(e) = tokio::fs::write(
        &md_path,
        crate::export::markdown(&project, &stored, &now),
    )
    .await
    {
        return err500(format!("ghi {} thất bại: {e}", md_path.display()));
    }

    respond(json!({
        "ok": true,
        "dir": dir.to_string_lossy(),
        "bundle": bundle_path.to_string_lossy(),
        "markdown": md_path.to_string_lossy(),
        "scene_count": stored.len(),
    }))
}

#[derive(Deserialize, Default)]
struct WikiBody {
    /// Wiki path; defaults to `video-cloner/<slug>.md`.
    #[serde(default)]
    path: String,
}

/// Publish the screenplay as a SenClaw wiki page.
///
/// Talks to the daemon's REST API directly rather than through the Space
/// bridge: the bridge advertises `space.rest` but has no handler for it.
async fn export_to_wiki(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    body: Bytes,
) -> Response {
    let req: WikiBody = if body.is_empty() {
        WikiBody::default()
    } else {
        match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(e) => return err400(format!("body không hợp lệ: {e}")),
        }
    };

    let (project, stored) = match load_for_export(&state, id) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let md = crate::export::markdown(&project, &stored, &crate::db::now());

    let path = if req.path.trim().is_empty() {
        format!(
            "video-cloner/{}.md",
            crate::export::slug(&project.name, project.id)
        )
    } else {
        req.path.trim().to_string()
    };

    let url = format!(
        "{}/api/wiki/file",
        crate::config::senclaw_base_url().trim_end_matches('/')
    );
    let payload = json!({
        "path": path,
        "content": md,
        "source": "video-cloner",
        "tags": ["video", "veo3", "kịch bản", "video-cloner"],
        "commitMsg": format!("video-cloner: kịch bản \"{}\"", project.name),
    });

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
    {
        Ok(c) => c,
        Err(e) => return err500(e),
    };

    match client.put(&url).json(&payload).send().await {
        Ok(resp) if resp.status().is_success() => respond(json!({
            "ok": true,
            "path": path,
            "scene_count": stored.len(),
        })),
        Ok(resp) => {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            err500(format!(
                "wiki trả {status}: {}",
                scenes::truncate_chars(text.trim(), 300)
            ))
        }
        Err(e) => err500(format!(
            "không gọi được wiki tại {url}: {e} — daemon SenClaw có đang chạy không?"
        )),
    }
}

#[derive(Deserialize, Default)]
struct HandoffBody {
    #[serde(default)]
    orientation: String,
    /// Translate visual prompts to English before pushing.
    #[serde(default)]
    translate: bool,
    /// Build the payload and return it without creating anything downstream.
    #[serde(default)]
    dry_run: bool,
    /// Override the target video-flow base URL.
    #[serde(default)]
    target_url: String,
}

async fn handoff_video_flow(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    body: Bytes,
) -> Response {
    let req: HandoffBody = if body.is_empty() {
        HandoffBody::default()
    } else {
        match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(e) => return err400(format!("body không hợp lệ: {e}")),
        }
    };

    let (project, stored) = match load_for_export(&state, id) {
        Ok(v) => v,
        Err(r) => return r,
    };

    let mut plan = crate::handoff::plan(&project, &stored, &req.orientation);
    let mut translated = 0usize;
    if req.translate {
        match crate::handoff::translate_plan(&mut plan).await {
            Ok(n) => translated = n,
            Err(e) => return err500(format!("dịch prompt sang tiếng Anh thất bại: {e}")),
        }
    }

    if req.dry_run {
        return respond(json!({
            "ok": true,
            "dry_run": true,
            "translated_scenes": translated,
            "plan": plan,
        }));
    }

    let base = if req.target_url.trim().is_empty() {
        crate::config::video_flow_url()
    } else {
        req.target_url.trim().to_string()
    };

    if let Err(e) = crate::handoff::probe(&base).await {
        return err(
            StatusCode::BAD_GATEWAY,
            format!("{e}. Hãy mở app video-flow trước khi bàn giao."),
        );
    }

    match crate::handoff::push(&base, &plan).await {
        Ok(p) => respond(json!({
            "ok": true,
            "target": base,
            "project_id": p.project_id,
            "video_id": p.video_id,
            "entities_created": p.entity_count,
            "scenes_created": p.scene_count,
            "translated_scenes": translated,
            "next": "mở video-flow và chạy workflow để sinh ảnh/video. ĐỪNG chạy pipeline/create — nó sẽ xoá sạch các đoạn vừa bàn giao.",
        })),
        Err(e) => err(StatusCode::BAD_GATEWAY, e),
    }
}

#[derive(Deserialize)]
struct ExportQuery {
    #[serde(default)]
    download: bool,
}

async fn export(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(q): Query<ExportQuery>,
) -> Response {
    let project = match state.core.db.project(id) {
        Ok(Some(p)) => p,
        Ok(None) => return err404("không tìm thấy dự án"),
        Err(e) => return err500(e),
    };
    let values: Vec<Value> = match state.core.db.scenes(id) {
        Ok(s) => s.iter().map(|x| x.json.clone()).collect(),
        Err(e) => return err500(e),
    };
    let text = scenes::export_text(&values);

    if !q.download {
        return respond(json!({ "text": text, "scene_count": values.len() }));
    }

    let filename = format!(
        "video_copy_{}.txt",
        project
            .name
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect::<String>()
    );
    (
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        text,
    )
        .into_response()
}

// ---- websocket ----

async fn ws_dashboard(State(state): State<AppState>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |mut socket| async move {
        use axum::extract::ws::Message;
        let mut rx = state.core.dash.subscribe();
        while let Ok(msg) = rx.recv().await {
            if socket.send(Message::Text(msg)).await.is_err() {
                break;
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_config_only_touches_the_fields_present() {
        let base = CloneConfig {
            style: "A".into(),
            model: "m".into(),
            char_description: "old".into(),
            visual_similarity: 100,
            ..Default::default()
        };
        let out = merge_config(&base, &json!({ "visual_similarity": 40 }));
        assert_eq!(out.style, "A");
        assert_eq!(out.char_description, "old");
        assert_eq!(out.visual_similarity, 40);
    }

    #[test]
    fn merge_config_clamps_the_similarity_slider() {
        let base = CloneConfig::default();
        assert_eq!(merge_config(&base, &json!({"visual_similarity": 900})).visual_similarity, 100);
        assert_eq!(merge_config(&base, &json!({"visual_similarity": -5})).visual_similarity, 0);
    }

    #[test]
    fn blank_style_does_not_wipe_the_stored_one() {
        let base = CloneConfig { style: "Keep me".into(), ..Default::default() };
        assert_eq!(merge_config(&base, &json!({ "style": "  " })).style, "Keep me");
    }

    #[test]
    fn empty_description_fields_can_be_cleared() {
        let base = CloneConfig { char_description: "old".into(), ..Default::default() };
        assert_eq!(merge_config(&base, &json!({ "char_description": "" })).char_description, "");
    }

    #[test]
    fn config_form_fields_parse_the_checkbox_and_slider() {
        let mut cfg = CloneConfig::default();
        let mut name = String::new();
        apply_config_field(&mut cfg, &mut name, "auto_magic", "true");
        apply_config_field(&mut cfg, &mut name, "visual_similarity", "150");
        apply_config_field(&mut cfg, &mut name, "name", "Dự án A");
        assert!(cfg.auto_magic);
        assert_eq!(cfg.visual_similarity, 100);
        assert_eq!(name, "Dự án A");
    }
}
