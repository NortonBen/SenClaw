//! HTTP API for the Skill Builder app. The UI (and, indirectly, the MCP tools)
//! drive four actions: read the inventory, draft a skill from a requirement,
//! install a draft, and list/remove installed skills.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::daemon::Daemon;
use crate::generate::{self, DraftSkill};

pub struct AppState {
    pub daemon: Daemon,
    /// Broadcasts raw JSON-RPC responses to any connected MCP SSE clients.
    pub mcp_tx: tokio::sync::broadcast::Sender<String>,
}

pub fn make_state() -> Arc<AppState> {
    let (mcp_tx, _) = tokio::sync::broadcast::channel(100);
    Arc::new(AppState {
        daemon: Daemon::from_env(),
        mcp_tx,
    })
}

pub struct ApiError(pub StatusCode, pub String);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}
fn err(e: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

pub fn api_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/status", get(status))
        .route("/inventory", get(get_inventory))
        .route("/skills", get(get_skills))
        .route("/generate", post(post_generate))
        .route("/install", post(post_install))
        .route("/skills/:name", delete(del_skill))
        .route("/mcp/sse", get(crate::mcp::mcp_sse).post(crate::mcp::mcp_message))
        .route("/mcp/message", post(crate::mcp::mcp_message))
        .with_state(state)
}

async fn status() -> Json<Value> {
    json!({
        "ok": true,
        "app": "skill-builder",
        "name": "SenClaw Skill Builder",
    })
    .into()
}

async fn get_inventory(State(s): State<Arc<AppState>>) -> Json<Value> {
    let inv = s.daemon.inventory().await;
    json!({
        "skills": inv.skills,
        "subagents": inv.subagents,
        "mcpServers": inv.mcp_servers,
    })
    .into()
}

async fn get_skills(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    s.daemon.list_skills().await.map(Json).map_err(err)
}

#[derive(Deserialize)]
struct GenerateBody {
    /// What the skill is for.
    requirement: String,
    /// Optional: when it should run / trigger conditions.
    #[serde(default)]
    when_to_run: String,
}

async fn post_generate(
    State(s): State<Arc<AppState>>,
    Json(body): Json<GenerateBody>,
) -> Result<Json<DraftSkill>, ApiError> {
    if body.requirement.trim().is_empty() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "requirement is required".into()));
    }
    let inv = s.daemon.inventory().await;
    let draft = generate::draft(&body.requirement, &body.when_to_run, &inv)
        .await
        .map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e))?;
    Ok(Json(draft))
}

#[derive(Deserialize)]
pub struct InstallBody {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub triggers: Vec<String>,
    #[serde(default)]
    pub overwrite: bool,
}

async fn post_install(
    State(s): State<Arc<AppState>>,
    Json(body): Json<InstallBody>,
) -> Result<Json<Value>, ApiError> {
    if body.name.trim().is_empty() || body.content.trim().is_empty() {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "name and content are required".into(),
        ));
    }
    let res = s
        .daemon
        .create_skill(
            body.name.trim(),
            body.description.trim(),
            body.content.trim(),
            &body.triggers,
            body.overwrite,
        )
        .await
        .map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e.to_string()))?;
    Ok(Json(json!({ "ok": true, "name": body.name.trim(), "daemon": res })))
}

async fn del_skill(
    State(s): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<Value>, ApiError> {
    s.daemon
        .delete_skill(&name)
        .await
        .map(|_| Json(json!({ "ok": true, "name": name })))
        .map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e.to_string()))
}
