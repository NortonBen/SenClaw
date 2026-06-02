use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::db::Db;
use crate::mailer;
use crate::models::AccountCreate;
use crate::store;

pub struct AppState {
    pub db: Arc<Db>,
    pub mcp_tx: tokio::sync::broadcast::Sender<String>,
}

/// Small error wrapper so handlers can `?` on anyhow errors.
pub struct ApiError(pub StatusCode, pub String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

fn bad(e: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, e.to_string())
}
fn server(e: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

pub fn api_router() -> Router {
    let db = Arc::new(Db::open().expect("open email db"));
    let (mcp_tx, _) = tokio::sync::broadcast::channel(100);
    let state = Arc::new(AppState { db, mcp_tx });

    Router::new()
        .route("/accounts", get(list_accounts).post(create_account))
        .route("/accounts/:id", delete(delete_account))
        .route("/inbox", get(inbox))
        .route("/messages/:id", get(read_message))
        .route("/search", get(search))
        .route("/send", post(send))
        .route("/draft", post(draft))
        .route("/sync", post(sync))
        .route("/mcp/sse", get(crate::mcp::mcp_sse).post(crate::mcp::mcp_message))
        .route("/mcp/message", post(crate::mcp::mcp_message))
        .with_state(state)
}

async fn list_accounts(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    let accounts = store::list_accounts(&s.db).map_err(server)?;
    Ok(Json(serde_json::to_value(accounts).unwrap_or_default()))
}

async fn create_account(
    State(s): State<Arc<AppState>>,
    Json(b): Json<AccountCreate>,
) -> Result<Json<Value>, ApiError> {
    let acct = store::create_account(&s.db, &b).map_err(bad)?;
    Ok(Json(serde_json::to_value(acct).unwrap_or_default()))
}

async fn delete_account(
    State(s): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    store::delete_account(&s.db, &id).map_err(server)?;
    Ok(Json(json!({ "success": true })))
}

#[derive(Deserialize)]
pub struct InboxQuery {
    account_id: Option<String>,
    #[serde(default = "default_limit")]
    limit: u32,
}
fn default_limit() -> u32 {
    50
}

async fn inbox(
    State(s): State<Arc<AppState>>,
    Query(q): Query<InboxQuery>,
) -> Result<Json<Value>, ApiError> {
    let rows = store::inbox(&s.db, q.account_id.as_deref(), q.limit).map_err(server)?;
    Ok(Json(json!(rows)))
}

async fn read_message(
    State(s): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let v = store::read_msg(&s.db, &id).map_err(|e| ApiError(StatusCode::NOT_FOUND, e.to_string()))?;
    Ok(Json(v))
}

#[derive(Deserialize)]
pub struct SearchQuery {
    q: String,
    account_id: Option<String>,
    #[serde(default = "default_limit")]
    limit: u32,
}

async fn search(
    State(s): State<Arc<AppState>>,
    Query(q): Query<SearchQuery>,
) -> Result<Json<Value>, ApiError> {
    let rows = store::search(&s.db, &q.q, q.account_id.as_deref(), q.limit).map_err(server)?;
    Ok(Json(json!(rows)))
}

#[derive(Deserialize)]
pub struct SendBody {
    to: String,
    subject: String,
    body: String,
    account_id: Option<String>,
}

async fn send(
    State(s): State<Arc<AppState>>,
    Json(b): Json<SendBody>,
) -> Result<Json<Value>, ApiError> {
    let acct = store::account_secret(&s.db, b.account_id.as_deref()).map_err(bad)?;
    let from = acct.email.clone();
    let acct_for_send = acct.clone();
    let (to, subject, body) = (b.to.clone(), b.subject.clone(), b.body.clone());

    tokio::task::spawn_blocking(move || {
        mailer::send_smtp(&acct_for_send, &acct_for_send.email, &to, &subject, &body)
    })
    .await
    .map_err(server)?
    .map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e.to_string()))?;

    let msg_id = store::record_sent(&s.db, &acct.id, &from, &b.to, &b.subject, &b.body).map_err(server)?;
    Ok(Json(json!({
        "success": true,
        "message_id": msg_id,
        "to": b.to,
    })))
}

#[derive(Deserialize)]
pub struct DraftBody {
    prompt: String,
}

/// Template-based draft. The senclaw agent (with this app's MCP) produces the
/// real content; this endpoint just gives the UI a skeleton to edit.
async fn draft(Json(b): Json<DraftBody>) -> Json<Value> {
    let subject = format!("Re: {}", b.prompt.chars().take(60).collect::<String>());
    let body = format!("Kính gửi,\n\n{}\n\nTrân trọng,\n[Tên của bạn]", b.prompt);
    Json(json!({ "subject": subject, "body": body }))
}

#[derive(Deserialize)]
pub struct SyncBody {
    account_id: Option<String>,
    #[serde(default = "default_sync_limit")]
    limit: u32,
}
fn default_sync_limit() -> u32 {
    30
}

/// Real IMAP fetch into the cache.
async fn sync(
    State(s): State<Arc<AppState>>,
    Json(b): Json<SyncBody>,
) -> Result<Json<Value>, ApiError> {
    let acct = store::account_secret(&s.db, b.account_id.as_deref()).map_err(bad)?;
    let acct_id = acct.id.clone();
    let limit = b.limit;

    let msgs = tokio::task::spawn_blocking(move || mailer::fetch_imap(&acct, limit))
        .await
        .map_err(server)?
        .map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e.to_string()))?;

    let count = store::upsert_inbox(&s.db, &acct_id, &msgs).map_err(server)?;
    Ok(Json(json!({ "success": true, "synced": count, "account_id": acct_id })))
}
