use axum::{
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, Multipart, Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::chat;
use crate::db::{default_data_dir, Db};
use crate::docx;

pub struct AppState {
    pub db: Arc<Db>,
    pub mcp_tx: tokio::sync::broadcast::Sender<String>,
}

pub struct ApiError(pub StatusCode, pub String);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}
fn bad(e: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, e.to_string())
}
fn nf() -> ApiError {
    ApiError(StatusCode::NOT_FOUND, "not found".to_string())
}
fn err(e: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

pub fn make_state() -> Arc<AppState> {
    let db_path = default_data_dir("docx-editor").join("docx-editor.db");
    let db = Arc::new(Db::open(&db_path).expect("open docx-editor db"));
    let (mcp_tx, _) = tokio::sync::broadcast::channel(100);
    Arc::new(AppState { db, mcp_tx })
}

pub fn api_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/status", get(status))
        .route("/docs", get(list_docs).post(create_doc))
        .route("/doc", get(get_doc))
        .route("/doc/save", post(save_doc))
        .route("/doc/rename", post(rename_doc))
        .route("/doc/delete", post(delete_doc))
        .route("/doc/upload", post(upload_doc))
        .route("/doc/:id/download", get(download_doc))
        .route("/doc/:id/raw", get(get_raw).put(put_raw))
        .route("/chat", post(chat_handler))
        .route("/chat/apply", post(chat_apply))
        .route("/mcp/sse", get(crate::mcp::mcp_sse).post(crate::mcp::mcp_message))
        .route("/mcp/message", post(crate::mcp::mcp_message))
        .layer(DefaultBodyLimit::max(64 * 1024 * 1024))
        .with_state(state)
}

async fn status() -> Json<Value> {
    Json(json!({ "ok": true, "app": "docx-editor", "version": env!("CARGO_PKG_VERSION") }))
}

async fn list_docs(State(state): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    let docs = state.db.list_docs().map_err(err)?;
    Ok(Json(json!({ "docs": docs })))
}

#[derive(Deserialize)]
pub struct CreateBody {
    pub title: String,
    #[serde(default)]
    pub content: String,
}

async fn create_doc(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateBody>,
) -> Result<Json<Value>, ApiError> {
    let title = body.title.trim();
    if title.is_empty() {
        return Err(bad("title required"));
    }
    let id = state.db.create_doc(title, &body.content, now()).map_err(err)?;
    let blob = docx::build_docx(&body.content).map_err(err)?;
    state.db.save_doc(id, None, &body.content, Some(&blob), now()).map_err(err)?;
    Ok(Json(json!({ "id": id })))
}

#[derive(Deserialize)]
pub struct IdQuery {
    pub id: i64,
}

async fn get_doc(
    State(state): State<Arc<AppState>>,
    Query(q): Query<IdQuery>,
) -> Result<Json<Value>, ApiError> {
    let doc = state.db.get_doc(q.id).map_err(err)?.ok_or_else(nf)?;
    Ok(Json(json!({ "doc": doc })))
}

#[derive(Deserialize)]
pub struct SaveBody {
    pub id: i64,
    pub content: String,
    #[serde(default)]
    pub title: Option<String>,
}

async fn save_doc(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SaveBody>,
) -> Result<Json<Value>, ApiError> {
    if state.db.get_doc(body.id).map_err(err)?.is_none() {
        return Err(nf());
    }
    let blob = docx::build_docx(&body.content).map_err(err)?;
    state
        .db
        .save_doc(body.id, body.title.as_deref(), &body.content, Some(&blob), now())
        .map_err(err)?;
    Ok(Json(json!({ "ok": true, "size_bytes": blob.len() })))
}

#[derive(Deserialize)]
pub struct RenameBody {
    pub id: i64,
    pub title: String,
}

async fn rename_doc(
    State(state): State<Arc<AppState>>,
    Json(body): Json<RenameBody>,
) -> Result<Json<Value>, ApiError> {
    let title = body.title.trim();
    if title.is_empty() {
        return Err(bad("title required"));
    }
    state.db.rename_doc(body.id, title, now()).map_err(err)?;
    Ok(Json(json!({ "ok": true })))
}

async fn delete_doc(
    State(state): State<Arc<AppState>>,
    Json(body): Json<IdQuery>,
) -> Result<Json<Value>, ApiError> {
    state.db.delete_doc(body.id).map_err(err)?;
    Ok(Json(json!({ "ok": true })))
}

async fn upload_doc(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Json<Value>, ApiError> {
    let mut filename = String::from("Untitled.docx");
    let mut bytes: Option<Vec<u8>> = None;

    while let Some(field) = multipart.next_field().await.map_err(bad)? {
        let name = field.name().unwrap_or("").to_string();
        if name == "file" {
            if let Some(fname) = field.file_name() {
                filename = fname.to_string();
            }
            bytes = Some(field.bytes().await.map_err(bad)?.to_vec());
        }
    }
    let bytes = bytes.ok_or_else(|| bad("missing file field"))?;
    let text = docx::extract_text(&bytes).map_err(err)?;
    let title = strip_docx_ext(&filename);
    let id = state.db.create_doc(&title, &text, now()).map_err(err)?;
    state
        .db
        .save_doc(id, None, &text, Some(&bytes), now())
        .map_err(err)?;
    Ok(Json(json!({ "id": id, "title": title, "chars": text.chars().count() })))
}

fn strip_docx_ext(name: &str) -> String {
    let n = name.trim();
    let lower = n.to_ascii_lowercase();
    if let Some(stripped) = lower.strip_suffix(".docx") {
        n[..stripped.len()].to_string()
    } else {
        n.to_string()
    }
}

async fn download_doc(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Response, ApiError> {
    let doc = state.db.get_doc(id).map_err(err)?.ok_or_else(nf)?;
    let blob = match state.db.get_docx_blob(id).map_err(err)? {
        Some(b) => b,
        None => docx::build_docx(&doc.content_text).map_err(err)?,
    };
    let disposition = format!("attachment; filename=\"{}.docx\"", sanitize_filename(&doc.title));
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        )
        .header(header::CONTENT_DISPOSITION, disposition)
        .body(Body::from(blob))
        .unwrap())
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_control() || "\\/:*?\"<>|".contains(c) { '_' } else { c })
        .collect()
}

/// GET /doc/:id/raw — return the stored .docx bytes. Used by the WYSIWYG editor
/// to hydrate its `documentBuffer` prop with the on-disk file (formatting,
/// tables, images intact — the plain-text projection is only for MCP reads).
async fn get_raw(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Response, ApiError> {
    let doc = state.db.get_doc(id).map_err(err)?.ok_or_else(nf)?;
    let blob = match state.db.get_docx_blob(id).map_err(err)? {
        Some(b) => b,
        None => docx::build_docx(&doc.content_text).map_err(err)?,
    };
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        )
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(blob))
        .unwrap())
}

/// PUT /doc/:id/raw — replace the stored .docx with the request body bytes and
/// re-extract plain text so MCP tools stay in sync with what the WYSIWYG editor
/// just saved.
async fn put_raw(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    if state.db.get_doc(id).map_err(err)?.is_none() {
        return Err(nf());
    }
    let bytes = body.to_vec();
    let text = docx::extract_text(&bytes).map_err(err)?;
    state
        .db
        .save_doc(id, None, &text, Some(&bytes), now())
        .map_err(err)?;
    Ok(Json(json!({ "ok": true, "size_bytes": bytes.len(), "chars": text.chars().count() })))
}

/// POST /chat — one turn in the in-app Agent panel. Grounded in the current
/// document text, returns markdown reply + (optionally) a proposed rewrite that
/// the UI can show as a one-click Apply.
async fn chat_handler(Json(body): Json<chat::ChatBody>) -> Result<Json<Value>, ApiError> {
    let (reply, model) = chat::chat(&body).await.map_err(|e| gateway(e))?;
    let rewrite = chat::extract_rewrite(&reply);
    Ok(Json(json!({
        "reply": reply,
        "model": model,
        "rewrite": rewrite,
    })))
}

#[derive(Deserialize)]
pub struct ApplyBody {
    pub id: i64,
    pub content: String,
}

/// POST /chat/apply — the user accepted a full-document rewrite from the Agent
/// panel; save it (regenerating the .docx blob).
async fn chat_apply(
    State(state): State<Arc<AppState>>,
    Json(body): Json<ApplyBody>,
) -> Result<Json<Value>, ApiError> {
    if state.db.get_doc(body.id).map_err(err)?.is_none() {
        return Err(nf());
    }
    let blob = docx::build_docx(&body.content).map_err(err)?;
    state
        .db
        .save_doc(body.id, None, &body.content, Some(&blob), now())
        .map_err(err)?;
    Ok(Json(json!({ "ok": true, "size_bytes": blob.len() })))
}

fn gateway(e: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::BAD_GATEWAY, e.to_string())
}
