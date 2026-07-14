use crate::db::{default_data_dir, Db};
use crate::engine;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

pub struct AppState {
    pub db: Arc<Db>,
    /// Broadcasts raw JSON-RPC responses to connected MCP SSE clients.
    pub mcp_tx: tokio::sync::broadcast::Sender<String>,
}

pub fn make_state() -> Arc<AppState> {
    let db_path = default_data_dir("ai-office").join("ai-office.db");
    let db = Arc::new(Db::open(&db_path).expect("open ai-office db"));
    // A previous process may have died mid-task; don't leave zombies "running".
    let _ = db.fail_stale_running();
    let _ = db.reset_agent_statuses();
    let (mcp_tx, _) = tokio::sync::broadcast::channel(100);
    Arc::new(AppState { db, mcp_tx })
}

pub fn api_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/status", get(status))
        .route("/llm-info", get(llm_info))
        .route("/stats", get(stats))
        .route("/agents", get(list_agents))
        .route("/agents/:key", patch(update_agent))
        .route("/tasks", get(list_tasks).post(create_task))
        .route("/tasks/:id", get(get_task))
        .route("/tasks/:id/events", get(task_events))
        .route("/events/recent", get(recent_events))
        .route("/mcp/sse", get(crate::mcp::mcp_sse).post(crate::mcp::mcp_message))
        .route("/mcp/message", post(crate::mcp::mcp_message))
        .with_state(state)
}

type ApiError = (StatusCode, Json<Value>);

fn err(code: StatusCode, msg: impl std::fmt::Display) -> ApiError {
    (code, Json(json!({ "error": msg.to_string() })))
}

fn internal(e: impl std::fmt::Display) -> ApiError {
    err(StatusCode::INTERNAL_SERVER_ERROR, e)
}

async fn status() -> Json<Value> {
    Json(json!({ "ok": true, "app": "ai-office" }))
}

async fn llm_info() -> Json<Value> {
    Json(crate::llm::llm_info().await)
}

async fn stats(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(s.db.stats().map_err(internal)?))
}

async fn list_agents(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "agents": s.db.list_agents().map_err(internal)? })))
}

#[derive(Deserialize)]
struct AgentPatch {
    name: Option<String>,
    role: Option<String>,
    duty: Option<String>,
}

async fn update_agent(
    State(s): State<Arc<AppState>>,
    Path(key): Path<String>,
    Json(body): Json<AgentPatch>,
) -> Result<Json<Value>, ApiError> {
    let found = s
        .db
        .update_agent(&key, body.name.as_deref(), body.role.as_deref(), body.duty.as_deref())
        .map_err(internal)?;
    if !found {
        return Err(err(StatusCode::NOT_FOUND, format!("không có agent '{}'", key)));
    }
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct TaskListQuery {
    limit: Option<i64>,
}

async fn list_tasks(
    State(s): State<Arc<AppState>>,
    Query(q): Query<TaskListQuery>,
) -> Result<Json<Value>, ApiError> {
    let tasks = s.db.list_tasks(q.limit.unwrap_or(30).clamp(1, 200)).map_err(internal)?;
    Ok(Json(json!({ "tasks": tasks })))
}

#[derive(Deserialize)]
struct CreateTask {
    title: String,
    mode: Option<String>,
}

async fn create_task(
    State(s): State<Arc<AppState>>,
    Json(body): Json<CreateTask>,
) -> Result<Json<Value>, ApiError> {
    let title = body.title.trim();
    if title.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "nhiệm vụ trống"));
    }
    if s.db.has_running_task().map_err(internal)? {
        return Err(err(
            StatusCode::CONFLICT,
            "phòng đang xử lý một nhiệm vụ khác — chờ xong rồi giao tiếp",
        ));
    }
    let mode = match body.mode.as_deref() {
        Some("live") => "live",
        _ => "demo",
    };
    let task = s.db.create_task(title, mode).map_err(internal)?;
    engine::spawn(s.db.clone(), task.id);
    Ok(Json(json!({ "task": task })))
}

async fn get_task(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let task = s
        .db
        .get_task(id)
        .map_err(internal)?
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "không có nhiệm vụ này"))?;
    let steps = s.db.list_steps(id).map_err(internal)?;
    Ok(Json(json!({ "task": task, "steps": steps })))
}

#[derive(Deserialize)]
struct EventsQuery {
    after: Option<i64>,
    limit: Option<i64>,
}

async fn task_events(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Query(q): Query<EventsQuery>,
) -> Result<Json<Value>, ApiError> {
    let events = s
        .db
        .list_events(Some(id), q.after.unwrap_or(0), q.limit.unwrap_or(200).clamp(1, 500))
        .map_err(internal)?;
    Ok(Json(json!({ "events": events })))
}

async fn recent_events(
    State(s): State<Arc<AppState>>,
    Query(q): Query<EventsQuery>,
) -> Result<Json<Value>, ApiError> {
    let events = s
        .db
        .recent_events(q.limit.unwrap_or(40).clamp(1, 200))
        .map_err(internal)?;
    Ok(Json(json!({ "events": events })))
}
