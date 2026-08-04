//! REST API. Paths are registered without the `/api` prefix; `main.rs` nests them.

use crate::mcp::{mcp_message, mcp_sse};
use crate::pipeline::{self, SearchRequest};
use crate::state::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

fn respond(v: Value) -> Response {
    Json(v).into_response()
}

fn err(status: StatusCode, msg: impl Into<String>) -> Response {
    (status, Json(json!({ "error": msg.into() }))).into_response()
}

pub fn root_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .with_state(state)
}

pub fn api_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/status", get(status))
        .route("/search", post(search))
        .route("/ask", post(ask))
        .route("/research", post(research))
        .route("/reports", get(list_reports))
        .route("/reports/:id", get(get_report))
        .route("/runs/:id/claims", get(run_claims))
        .route("/sources", get(sources))
        .route("/sources/:id", put(update_source))
        .route("/sources/mcp", post(add_mcp_source))
        .route("/sources/mcp/:id", axum::routing::delete(remove_mcp_source))
        .route("/corpus", get(list_corpus).post(upload_corpus))
        .route("/corpus/:id", axum::routing::delete(delete_corpus))
        .route("/source-templates", get(source_templates))
        .route("/mcp-tools", get(mcp_tools))
        .route("/sync", post(sync_sources))
        .route("/runs", get(list_runs))
        .route("/runs/:id", get(get_run).delete(delete_run))
        .route("/mcp/sse", get(mcp_sse).post(mcp_message))
        .route("/mcp/message", post(mcp_message))
        .with_state(state)
}

async fn health() -> Response {
    respond(json!({ "ok": true }))
}

async fn status(State(state): State<AppState>) -> Response {
    let stats = state.core.db.stats().unwrap_or(json!({}));
    let sources = state.core.registry.read().await.describe().await;
    respond(json!({
        "ok": true,
        "app": "zeach",
        "stats": stats,
        "sources": sources,
    }))
}

/// Run a federated search and persist it.
pub async fn search(State(state): State<AppState>, Json(req): Json<SearchRequest>) -> Response {
    if req.query.trim().is_empty() {
        return err(StatusCode::BAD_REQUEST, "query trống");
    }
    let registry = state.core.registry.read().await.clone();
    let out = pipeline::run(&registry, &state.core.transports, &req).await;

    let params = serde_json::to_value(json!({
        "sources": req.sources,
        "limit": req.limit,
        "lang": req.lang,
        "depth": req.depth,
    }))
    .unwrap_or(Value::Null);

    // A failure to persist must not lose the results the caller is waiting for.
    let run_id = match state.core.db.save_run(&out, &params, "cited") {
        Ok(id) => Some(id),
        Err(e) => {
            eprintln!("[search] không lưu được run: {e}");
            None
        }
    };

    let mut body = serde_json::to_value(&out).unwrap_or(json!({}));
    body["run_id"] = json!(run_id);
    respond(body)
}

async fn ask(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    via_mcp(&state, "zeach_ask", body).await
}

/// Deep multi-round research → a cited report. Delegates to the MCP tool so the
/// web UI and other components run the exact same pipeline.
async fn research(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    if body
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .is_empty()
    {
        return err(StatusCode::BAD_REQUEST, "query trống");
    }
    via_mcp(&state, "zeach_research", body).await
}

async fn list_reports(State(state): State<AppState>, Query(q): Query<LimitQuery>) -> Response {
    match state
        .core
        .db
        .list_reports(q.limit.unwrap_or(30).clamp(1, 200))
    {
        Ok(reports) => respond(json!({ "reports": reports })),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn get_report(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.core.db.get_report(&id) {
        Ok(Some(r)) => respond(r),
        Ok(None) => err(
            StatusCode::NOT_FOUND,
            format!("chưa có báo cáo cho run `{id}`"),
        ),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn run_claims(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    via_mcp(&state, "zeach_claims", json!({ "run_id": id })).await
}

async fn sources(State(state): State<AppState>) -> Response {
    let list = state.core.registry.read().await.describe().await;
    respond(json!({ "sources": list }))
}

#[derive(Deserialize)]
struct SourceConfigBody {
    enabled: Option<bool>,
    weight: Option<f32>,
    max_results: Option<usize>,
    timeout_ms: Option<u64>,
}

async fn update_source(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<SourceConfigBody>,
) -> Response {
    let ok = state.core.registry.write().await.set_config(
        &id,
        b.enabled,
        b.weight,
        b.max_results,
        b.timeout_ms,
    );
    if !ok {
        return err(StatusCode::NOT_FOUND, format!("không có nguồn `{id}`"));
    }
    if let Err(e) =
        state
            .core
            .db
            .save_source_config(&id, b.enabled, b.weight, b.max_results, b.timeout_ms)
    {
        return err(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("lưu cấu hình thất bại: {e}"),
        );
    }
    respond(json!({ "ok": true, "source": id }))
}

#[derive(Deserialize)]
struct LimitQuery {
    limit: Option<usize>,
}

/// Run an MCP tool and render its result as HTTP.
///
/// The MCP dispatch is the single implementation of every source-management
/// action; REST delegating to it is what keeps the two surfaces from drifting
/// apart as the tool set grows.
async fn via_mcp(state: &AppState, tool: &str, args: Value) -> Response {
    let result = crate::mcp::call_tool(state, tool, &args).await;
    let text = result["content"][0]["text"].as_str().unwrap_or_default();
    let body: Value = serde_json::from_str(text).unwrap_or_else(|_| json!({ "message": text }));
    if result["isError"] == json!(true) {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": text }))).into_response();
    }
    respond(body)
}

async fn add_mcp_source(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    via_mcp(&state, "zeach_source_add", body).await
}

async fn remove_mcp_source(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    via_mcp(&state, "zeach_source_remove", json!({ "source_id": id })).await
}

/// Cap a single upload. Extraction and chunking are in-memory, so an
/// unbounded file is an easy way to take the app down.
const MAX_UPLOAD_BYTES: usize = 64 * 1024 * 1024;

async fn list_corpus(State(state): State<AppState>) -> Response {
    via_mcp(&state, "zeach_corpus_list", json!({})).await
}

async fn delete_corpus(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    via_mcp(&state, "zeach_corpus_remove", json!({ "doc_id": id })).await
}

/// Multipart upload — the only path that reaches the PDF/DOCX extractors.
///
/// Every file is reported individually: a batch where one PDF is a scan must
/// say which one failed and why, not fail the whole upload or, worse, succeed
/// silently with one document missing.
async fn upload_corpus(
    State(state): State<AppState>,
    mut mp: axum::extract::Multipart,
) -> Response {
    let mut added = Vec::new();
    let mut failed = Vec::new();

    loop {
        let field = match mp.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) => return err(StatusCode::BAD_REQUEST, format!("multipart lỗi: {e}")),
        };
        let name = field
            .file_name()
            .map(str::to_string)
            .unwrap_or_else(|| "tài-liệu".to_string());
        let mime = field
            .content_type()
            .unwrap_or("application/octet-stream")
            .to_string();
        let bytes = match field.bytes().await {
            Ok(b) => b,
            Err(e) => {
                failed.push(json!({ "name": name, "error": format!("đọc tệp lỗi: {e}") }));
                continue;
            }
        };
        if bytes.len() > MAX_UPLOAD_BYTES {
            failed.push(json!({
                "name": name,
                "error": format!("tệp {} MB vượt giới hạn {} MB",
                    bytes.len() / 1_048_576, MAX_UPLOAD_BYTES / 1_048_576)
            }));
            continue;
        }

        match crate::corpus::extract(&name, &bytes) {
            Err(e) => failed.push(json!({ "name": name, "error": e.to_string() })),
            Ok(extracted) => {
                match crate::mcp::ingest_text(&state, &name, &mime, &bytes, &extracted.text) {
                    Ok(mut v) => {
                        v["note"] = json!(extracted.note);
                        added.push(v);
                    }
                    Err(e) => failed.push(json!({ "name": name, "error": e })),
                }
            }
        }
    }

    if added.is_empty() && !failed.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "added": [], "failed": failed })),
        )
            .into_response();
    }
    respond(json!({ "added": added, "failed": failed }))
}

async fn source_templates(State(state): State<AppState>) -> Response {
    via_mcp(&state, "zeach_source_templates", json!({})).await
}

#[derive(Deserialize)]
struct McpToolsQuery {
    app_id: Option<String>,
    rpc_url: Option<String>,
}

async fn mcp_tools(State(state): State<AppState>, Query(q): Query<McpToolsQuery>) -> Response {
    via_mcp(
        &state,
        "zeach_mcp_tools",
        json!({ "app_id": q.app_id, "rpc_url": q.rpc_url }),
    )
    .await
}

async fn sync_sources(State(state): State<AppState>) -> Response {
    via_mcp(&state, "zeach_sync", json!({})).await
}

async fn list_runs(State(state): State<AppState>, Query(q): Query<LimitQuery>) -> Response {
    match state.core.db.list_runs(q.limit.unwrap_or(30).clamp(1, 200)) {
        Ok(runs) => respond(json!({ "runs": runs })),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn get_run(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.core.db.get_run(&id) {
        Ok(Some(run)) => respond(run),
        Ok(None) => err(StatusCode::NOT_FOUND, format!("không có run `{id}`")),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn delete_run(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.core.db.delete_run(&id) {
        Ok(true) => respond(json!({ "ok": true })),
        Ok(false) => err(StatusCode::NOT_FOUND, format!("không có run `{id}`")),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}
