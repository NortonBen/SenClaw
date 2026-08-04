//! HTTP API. Paths are registered WITHOUT the `/api` prefix; `main.rs` nests
//! this router under `/api`. `/health` and `/status` both serve the health JSON
//! (manifest `healthPath` = `/api/status`).

use std::collections::HashMap;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, State, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::db::{status, NewProcess};
use crate::export;
use crate::state::AppState;
use crate::text;

/// Routes served at the server root rather than under `/api`.
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
        .route("/ws/dashboard", get(ws_dashboard))
        // Stories
        .route("/stories", get(list_stories).post(create_story))
        .route("/stories/:id", get(get_story).delete(delete_story))
        .route("/stories/:id/versions", get(list_versions))
        .route("/stories/:id/chunks", get(list_story_chunks))
        .route("/stories/:id/export", get(export_story))
        // Processes
        .route("/processes", get(list_processes).post(create_process))
        .route("/processes/:id", get(get_process).delete(delete_process))
        .route("/processes/:id/chunks", get(list_process_chunks))
        .route(
            "/processes/:id/cancel",
            put(cancel_process).post(cancel_process),
        )
        .route("/processes/:id/retry", post(retry_process))
        // Settings
        .route("/settings", get(get_settings).put(put_settings))
        // MCP
        .route(
            "/mcp/sse",
            get(crate::mcp::mcp_sse).post(crate::mcp::mcp_message),
        )
        .route("/mcp/message", post(crate::mcp::mcp_message))
        .with_state(state)
}

// ---- response helpers ----

fn respond(code: StatusCode, v: Value) -> Response {
    (code, Json(v)).into_response()
}

fn err(code: StatusCode, msg: impl Into<String>) -> Response {
    (code, Json(json!({ "error": msg.into() }))).into_response()
}

fn err500(e: impl ToString) -> Response {
    err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

type MaybeJson = Result<Json<Value>, JsonRejection>;

// `Response` is a fat Err variant, but every handler here already returns
// `Response`; boxing would add an allocation on the error path to satisfy a lint
// about a type that never propagates up a call stack.
#[allow(clippy::result_large_err)]
/// Decode a typed JSON body, answering 400 with `{"error"}` instead of axum's
/// plain-text 422.
fn parse_body<T: for<'de> Deserialize<'de>>(body: MaybeJson) -> Result<T, Response> {
    match body {
        Ok(Json(v)) => {
            serde_json::from_value(v).map_err(|e| err(StatusCode::BAD_REQUEST, e.to_string()))
        }
        Err(e) => Err(err(StatusCode::BAD_REQUEST, e.to_string())),
    }
}

// ---- health & ws ----

async fn health(State(st): State<AppState>) -> Response {
    let db = &st.core.db;
    respond(
        StatusCode::OK,
        json!({
            "status": "ok",
            "app": "rewrite-story",
            "running_jobs": st.core.running_count(),
            "queued": db.count_by_status(status::QUEUED).unwrap_or(0),
            "processing": db.count_by_status(status::PROCESSING).unwrap_or(0),
        }),
    )
}

async fn ws_dashboard(State(st): State<AppState>, ws: WebSocketUpgrade) -> Response {
    let dash = st.core.dash.clone();
    ws.on_upgrade(move |socket| async move { dash.serve(socket).await })
}

// ---- stories ----

async fn list_stories(State(st): State<AppState>) -> Response {
    match st.core.db.list_stories() {
        Ok(rows) => respond(StatusCode::OK, json!(rows)),
        Err(e) => err500(e),
    }
}

#[derive(Deserialize)]
struct CreateStoryReq {
    name: Option<String>,
    text: String,
}

async fn create_story(State(st): State<AppState>, body: MaybeJson) -> Response {
    let req: CreateStoryReq = match parse_body(body) {
        Ok(r) => r,
        Err(r) => return r,
    };
    if req.text.trim().is_empty() {
        return err(StatusCode::BAD_REQUEST, "text không được rỗng");
    }
    let name = req
        .name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "Truyện chưa đặt tên".to_string());

    match st.core.db.create_story(&name, &req.text) {
        // Metadata only — echoing the novel the client just uploaded back at it
        // doubles the transfer for nothing.
        Ok(id) => match st.core.db.story_meta(id) {
            Ok(Some(m)) => respond(StatusCode::CREATED, json!(m)),
            _ => err500("không đọc lại được truyện vừa tạo"),
        },
        Err(e) => err500(e),
    }
}

/// A story's metadata plus a **window** of its text.
///
/// Stories are entire novels. Returning `original_text` whole meant a ~15 MB
/// JSON response per detail-page visit, cached in the browser, to render the
/// 20 000 characters the page actually shows. `offset`/`limit` are sliced in SQL.
async fn get_story(
    State(st): State<AppState>,
    Path(id): Path<i64>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let db = &st.core.db;
    let offset = q
        .get("offset")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0)
        .max(0);
    let limit = q
        .get("limit")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(20_000)
        .clamp(1, 200_000);

    let meta = match db.story_meta(id) {
        Ok(Some(m)) => m,
        Ok(None) => return err(StatusCode::NOT_FOUND, "không tìm thấy truyện"),
        Err(e) => return err500(e),
    };
    let (slice, total) = match db.story_slice(id, offset, limit) {
        Ok(Some(v)) => v,
        Ok(None) => return err(StatusCode::NOT_FOUND, "không tìm thấy truyện"),
        Err(e) => return err500(e),
    };

    let mut body = json!(meta);
    body["original_text"] = json!(slice);
    body["total_length"] = json!(total);
    body["offset"] = json!(offset);
    body["has_more"] = json!(offset + limit < total);
    respond(StatusCode::OK, body)
}

async fn delete_story(State(st): State<AppState>, Path(id): Path<i64>) -> Response {
    // Refuse while a rewrite of it is live — the cascade would delete the
    // process row underneath the running worker.
    match st.core.db.active_processes_for_story(id) {
        Ok(active) if !active.is_empty() => {
            return err(
                StatusCode::BAD_REQUEST,
                format!(
                    "Truyện đang có {} tiến trình viết lại chạy/chờ. Hãy huỷ trước khi xoá.",
                    active.len()
                ),
            )
        }
        Ok(_) => {}
        Err(e) => return err500(e),
    }
    match st.core.db.delete_story(id) {
        Ok(0) => err(StatusCode::NOT_FOUND, "không tìm thấy truyện"),
        Ok(_) => respond(StatusCode::OK, json!({ "status": "ok" })),
        Err(e) => err500(e),
    }
}

async fn list_versions(State(st): State<AppState>, Path(id): Path<i64>) -> Response {
    match st.core.db.list_versions(id) {
        Ok(rows) => respond(StatusCode::OK, json!(rows)),
        Err(e) => err500(e),
    }
}

/// Chunk boundaries for a story. If the story hasn't been chunked yet this
/// previews the split without persisting it, so the user can tune the splitter
/// settings before committing to a rewrite.
async fn list_story_chunks(State(st): State<AppState>, Path(id): Path<i64>) -> Response {
    let db = &st.core.db;
    let stored = match db.get_chunks(id) {
        Ok(c) => c,
        Err(e) => return err500(e),
    };

    let (chunks, persisted) = if stored.is_empty() {
        let Ok(Some(story)) = db.get_story(id) else {
            return err(StatusCode::NOT_FOUND, "không tìm thấy truyện");
        };
        let min = db
            .setting_i64(
                "hybrid_split_min_size",
                (crate::llm::MAX_CHUNK_CHARS as i64) * 3 / 5,
            )
            .max(1) as usize;
        let max = db
            .setting_i64("hybrid_split_max_size", crate::llm::MAX_CHUNK_CHARS as i64)
            .max(1) as usize;
        let (min, max) = if min > max { (max, min) } else { (min, max) };
        let threshold = db
            .setting_f64("hybrid_split_threshold", 0.2)
            .clamp(0.0, 1.0);
        (
            text::hybrid_split(&story.original_text, min, max, threshold),
            false,
        )
    } else {
        (stored, true)
    };

    let items: Vec<Value> = chunks
        .iter()
        .enumerate()
        .map(|(i, c)| {
            json!({
                "chunk_index": i,
                "length": c.chars().count(),
                "preview": c.chars().take(200).collect::<String>(),
            })
        })
        .collect();

    respond(
        StatusCode::OK,
        json!({ "persisted": persisted, "total": items.len(), "chunks": items }),
    )
}

/// Export a story for the video pipeline.
///
/// `format=screenplay` is the one that matters: it is the markdown shape
/// `vf_pipeline_create(mode="production")` and video-flow's `/api/script/parse`
/// both consume. The response carries `Content-Disposition: attachment` so the
/// browser saves a file the user can hand to any mini app.
async fn export_story(
    State(st): State<AppState>,
    Path(id): Path<i64>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let db = &st.core.db;

    let meta = match db.story_meta(id) {
        Ok(Some(m)) => m,
        Ok(None) => return err(StatusCode::NOT_FOUND, "không tìm thấy truyện"),
        Err(e) => return err500(e),
    };
    let text_body = match db.story_text(id) {
        Ok(Some(t)) => t,
        Ok(None) => return err(StatusCode::NOT_FOUND, "không tìm thấy truyện"),
        Err(e) => return err500(e),
    };

    let scene_chars = q
        .get("scene_chars")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(export::DEFAULT_SCENE_CHARS);
    let bundle = export::bundle(
        meta.id,
        &meta.name,
        &meta.source_type,
        meta.version_number,
        &text_body,
        scene_chars,
    );

    let format = q.get("format").map(String::as_str).unwrap_or("screenplay");
    let (body, mime, ext) = match format {
        "screenplay" => (
            export::to_screenplay(&bundle),
            "text/markdown; charset=utf-8",
            "md",
        ),
        "markdown" => (
            export::to_markdown(&bundle),
            "text/markdown; charset=utf-8",
            "md",
        ),
        "json" => (
            serde_json::to_string_pretty(&bundle).unwrap_or_default(),
            "application/json; charset=utf-8",
            "json",
        ),
        "txt" => (text_body, "text/plain; charset=utf-8", "txt"),
        other => {
            return err(
                StatusCode::BAD_REQUEST,
                format!("format '{other}' không hợp lệ (screenplay | markdown | json | txt)"),
            )
        }
    };

    let filename = format!("{}-{}.{ext}", export::slug(&meta.name), meta.id);
    (
        StatusCode::OK,
        [
            (axum::http::header::CONTENT_TYPE, mime.to_string()),
            (
                axum::http::header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        body,
    )
        .into_response()
}

// ---- processes ----

async fn list_processes(
    State(st): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let filter = q
        .get("status")
        .map(|s| s.as_str())
        .filter(|s| !s.is_empty());
    match st.core.db.list_processes(filter) {
        Ok(rows) => respond(StatusCode::OK, json!(rows)),
        Err(e) => err500(e),
    }
}

#[derive(Deserialize)]
struct CreateProcessReq {
    story_id: i64,
    creativity_ratio: Option<i64>,
    target_length_variance: Option<i64>,
    system_instruction: Option<String>,
    user_prompt: Option<String>,
    version_plan: Option<String>,
    model: Option<String>,
}

async fn create_process(State(st): State<AppState>, body: MaybeJson) -> Response {
    let req: CreateProcessReq = match parse_body(body) {
        Ok(r) => r,
        Err(r) => return r,
    };
    let db = &st.core.db;

    match db.story_exists(req.story_id) {
        Ok(true) => {}
        Ok(false) => return err(StatusCode::NOT_FOUND, "không tìm thấy truyện"),
        Err(e) => return err500(e),
    }

    // Backpressure: refuse to pile work onto a queue that isn't draining.
    let in_flight = db.count_by_status(status::QUEUED).unwrap_or(0)
        + db.count_by_status(status::PROCESSING).unwrap_or(0);
    if in_flight >= 10 {
        return err(
            StatusCode::TOO_MANY_REQUESTS,
            "Đang có quá nhiều tiến trình trong hàng chờ (tối đa 10)",
        );
    }

    let p = NewProcess {
        story_id: req.story_id,
        creativity_ratio: req
            .creativity_ratio
            .unwrap_or_else(|| db.setting_i64("default_creativity_ratio", 40))
            .clamp(0, 100),
        target_length_variance: req
            .target_length_variance
            .unwrap_or_else(|| db.setting_i64("default_length_variance", 5))
            .clamp(0, 100),
        system_instruction: req.system_instruction,
        user_prompt: req.user_prompt,
        version_plan: req.version_plan,
        model: req.model,
    };

    match db.create_process(&p) {
        Ok(id) => match db.get_process(id) {
            Ok(Some(row)) => respond(StatusCode::ACCEPTED, json!(row)),
            _ => err500("không đọc lại được tiến trình vừa tạo"),
        },
        Err(e) => err500(e),
    }
}

async fn get_process(State(st): State<AppState>, Path(id): Path<i64>) -> Response {
    match st.core.db.get_process(id) {
        Ok(Some(p)) => respond(StatusCode::OK, json!(p)),
        Ok(None) => err(StatusCode::NOT_FOUND, "không tìm thấy tiến trình"),
        Err(e) => err500(e),
    }
}

async fn list_process_chunks(State(st): State<AppState>, Path(id): Path<i64>) -> Response {
    match st.core.db.get_rewrite_chunks(id) {
        Ok(rows) => respond(StatusCode::OK, json!(rows)),
        Err(e) => err500(e),
    }
}

async fn cancel_process(State(st): State<AppState>, Path(id): Path<i64>) -> Response {
    let db = &st.core.db;
    let Ok(Some(p)) = db.get_process(id) else {
        return err(StatusCode::NOT_FOUND, "không tìm thấy tiến trình");
    };
    if !status::is_active(&p.status) {
        return err(
            StatusCode::BAD_REQUEST,
            "Tiến trình không thể hủy ở trạng thái này",
        );
    }

    // Signal the running task first, then flip the row. The DB's terminal-state
    // guard then stops the task from writing anything after this point.
    st.core.cancel_job(id);
    if let Err(e) = db.update_progress(
        id,
        status::CANCELLED,
        crate::db::stage::CANCELLED,
        p.progress_percentage,
        0,
        0,
        Some("Bị hủy bởi người dùng"),
        None,
    ) {
        return err500(e);
    }
    if let Ok(Some(row)) = db.get_process(id) {
        st.core
            .dash
            .emit(crate::dashws::event::PROCESS_CANCELLED, json!(row));
    }
    respond(StatusCode::OK, json!({ "status": "ok" }))
}

async fn retry_process(State(st): State<AppState>, Path(id): Path<i64>) -> Response {
    let db = &st.core.db;
    let Ok(Some(p)) = db.get_process(id) else {
        return err(StatusCode::NOT_FOUND, "không tìm thấy tiến trình");
    };
    if !matches!(p.status.as_str(), status::FAILED | status::CANCELLED) {
        return err(
            StatusCode::BAD_REQUEST,
            "Chỉ có thể thử lại tiến trình thất bại hoặc bị hủy",
        );
    }
    // Finished chunks are left in place — that is what makes this a resume.
    if let Err(e) = db.requeue_process(id) {
        return err500(e);
    }
    let done = db.get_rewrite_chunks(id).map(|c| c.len()).unwrap_or(0);
    match db.get_process(id) {
        Ok(Some(row)) => respond(
            StatusCode::ACCEPTED,
            json!({ "process": row, "resuming_from_chunk": done }),
        ),
        _ => err500("không đọc lại được tiến trình"),
    }
}

async fn delete_process(State(st): State<AppState>, Path(id): Path<i64>) -> Response {
    let db = &st.core.db;
    let Ok(Some(p)) = db.get_process(id) else {
        return err(StatusCode::NOT_FOUND, "không tìm thấy tiến trình");
    };
    if status::is_active(&p.status) {
        return err(
            StatusCode::BAD_REQUEST,
            "Không thể xóa tiến trình đang chờ hoặc đang chạy",
        );
    }
    match db.delete_process(id) {
        Ok(_) => respond(StatusCode::OK, json!({ "status": "ok" })),
        Err(e) => err500(e),
    }
}

// ---- settings ----

async fn get_settings(State(st): State<AppState>) -> Response {
    match st.core.db.all_settings() {
        Ok(kv) => {
            let map: serde_json::Map<String, Value> =
                kv.into_iter().map(|(k, v)| (k, json!(v))).collect();
            respond(StatusCode::OK, Value::Object(map))
        }
        Err(e) => err500(e),
    }
}

async fn put_settings(State(st): State<AppState>, body: MaybeJson) -> Response {
    let patch: serde_json::Map<String, Value> = match parse_body(body) {
        Ok(m) => m,
        Err(r) => return r,
    };
    // Validate the whole patch before writing any of it, so a bad key can't
    // leave settings half-applied.
    let mut writes = Vec::with_capacity(patch.len());
    for (k, v) in &patch {
        let s = match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        if let Err(e) = crate::db::validate_setting(k, &s) {
            return err(StatusCode::BAD_REQUEST, e.to_string());
        }
        writes.push((k.clone(), s));
    }
    for (k, s) in &writes {
        if let Err(e) = st.core.db.set_setting(k, s) {
            return err500(e);
        }
    }
    // The LLM profile is cached in a global; keep it in step without a restart.
    if let Some(p) = patch.get("llm_profile").and_then(|v| v.as_str()) {
        crate::llm::set_profile(p);
    }
    get_settings(State(st)).await
}
