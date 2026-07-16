use crate::db::{default_data_dir, Db};
use axum::extract::{Multipart, Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
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
    // A previous process may have died mid-task; fail actively-running jobs
    // (queued `pending` tasks are kept and resumed by the drainer).
    let _ = db.fail_stale_running();
    let _ = db.reset_agent_statuses("");
    let (mcp_tx, _) = tokio::sync::broadcast::channel(100);
    // Scheduler: each team drains its own queue; teams run in parallel.
    crate::engine::spawn_scheduler(db.clone());
    Arc::new(AppState { db, mcp_tx })
}

pub fn api_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/status", get(status))
        .route("/llm-info", get(llm_info))
        .route("/stats", get(stats))
        .route("/teams", get(list_teams).post(add_team))
        .route("/teams/:key", patch(update_team).delete(delete_team))
        .route("/agents", get(list_agents).post(add_agent))
        .route("/agents/:key", patch(update_agent).delete(delete_agent))
        .route("/agents/:key/knowledge", get(agent_knowledge))
        .route("/skills-inventory", get(skills_inventory))
        .route("/settings", get(get_settings).post(update_settings))
        .route("/workspace/files", get(workspace_files))
        .route("/fs/dirs", get(fs_dirs))
        .route("/stt", post(stt))
        .route("/tts", post(tts))
        .route("/tasks", get(list_tasks).post(create_task))
        .route("/queue", get(queue))
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
    enabled: Option<bool>,
    auto_assign: Option<bool>,
    skills: Option<Vec<String>>,
}

async fn update_agent(
    State(s): State<Arc<AppState>>,
    Path(key): Path<String>,
    Json(body): Json<AgentPatch>,
) -> Result<Json<Value>, ApiError> {
    if body.enabled == Some(false) {
        let agents = s.db.list_agents().map_err(internal)?;
        if let Some(agent) = agents.iter().find(|a| a.key == key) {
            // Disabling staff mid-task would strand that team's pipeline.
            if s.db.has_running_task(&agent.team).map_err(internal)? {
                return Err(err(
                    StatusCode::CONFLICT,
                    "đội đang xử lý nhiệm vụ — chờ xong rồi tạm dừng nhân sự",
                ));
            }
            if agent.kind == "manager" {
                return Err(err(StatusCode::BAD_REQUEST, "không thể tạm dừng Trưởng nhóm"));
            }
            if agent.kind == "worker"
                && agents.iter().filter(|a| a.team == agent.team && a.kind == "worker" && a.enabled).count() <= 1
            {
                return Err(err(
                    StatusCode::BAD_REQUEST,
                    "đội cần ít nhất một nhân sự chuyên môn đang hoạt động",
                ));
            }
        }
    }
    let found = s
        .db
        .update_agent(
            &key,
            body.name.as_deref(),
            body.role.as_deref(),
            body.duty.as_deref(),
            body.enabled,
            body.auto_assign,
            body.skills.as_deref(),
        )
        .map_err(internal)?;
    if !found {
        return Err(err(StatusCode::NOT_FOUND, format!("không có agent '{}'", key)));
    }
    Ok(Json(json!({ "ok": true })))
}

// ---- teams ----

async fn list_teams(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "teams": s.db.list_teams().map_err(internal)? })))
}

#[derive(Deserialize)]
struct TeamCreate {
    name: String,
    description: Option<String>,
}

async fn add_team(
    State(s): State<Arc<AppState>>,
    Json(body): Json<TeamCreate>,
) -> Result<Json<Value>, ApiError> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "tên đội trống"));
    }
    let team = s
        .db
        .add_team(name, body.description.as_deref().unwrap_or(""))
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(json!({ "team": team })))
}

#[derive(Deserialize)]
struct TeamPatch {
    name: Option<String>,
    description: Option<String>,
}

async fn update_team(
    State(s): State<Arc<AppState>>,
    Path(key): Path<String>,
    Json(body): Json<TeamPatch>,
) -> Result<Json<Value>, ApiError> {
    let found = s
        .db
        .update_team(&key, body.name.as_deref(), body.description.as_deref())
        .map_err(internal)?;
    if !found {
        return Err(err(StatusCode::NOT_FOUND, format!("không có đội '{}'", key)));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn delete_team(
    State(s): State<Arc<AppState>>,
    Path(key): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let teams = s.db.list_teams().map_err(internal)?;
    if teams.len() <= 1 {
        return Err(err(StatusCode::BAD_REQUEST, "văn phòng cần ít nhất một đội"));
    }
    if s.db.has_running_task(&key).map_err(internal)? {
        return Err(err(StatusCode::CONFLICT, "đội đang xử lý nhiệm vụ — chờ xong rồi xoá"));
    }
    if !s.db.delete_team(&key).map_err(internal)? {
        return Err(err(StatusCode::NOT_FOUND, format!("không có đội '{}'", key)));
    }
    Ok(Json(json!({ "ok": true })))
}

// ---- agents ----

#[derive(Deserialize)]
struct AgentCreate {
    name: String,
    role: Option<String>,
    duty: Option<String>,
    kind: Option<String>,
    team: Option<String>,
}

async fn add_agent(
    State(s): State<Arc<AppState>>,
    Json(body): Json<AgentCreate>,
) -> Result<Json<Value>, ApiError> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "tên nhân sự trống"));
    }
    let kind = match body.kind.as_deref() {
        Some("manager") => "manager",
        Some("qa") => "qa",
        _ => "worker",
    };
    let teams = s.db.list_teams().map_err(internal)?;
    let team = body
        .team
        .as_deref()
        .filter(|t| teams.iter().any(|x| &x.key == t))
        .or_else(|| teams.first().map(|t| t.key.as_str()))
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "chưa có đội nào"))?
        .to_string();
    let agents = s.db.list_agents().map_err(internal)?;
    if kind != "worker" && agents.iter().any(|a| a.team == team && a.kind == kind) {
        return Err(err(
            StatusCode::CONFLICT,
            format!("đội đã có một nhân sự giữ vai trò '{}'", kind),
        ));
    }
    let agent = s
        .db
        .add_agent(name, body.role.as_deref().unwrap_or(""), body.duty.as_deref().unwrap_or(""), kind, &team)
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(json!({ "agent": agent })))
}

async fn delete_agent(
    State(s): State<Arc<AppState>>,
    Path(key): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let agents = s.db.list_agents().map_err(internal)?;
    let Some(agent) = agents.iter().find(|a| a.key == key) else {
        return Err(err(StatusCode::NOT_FOUND, format!("không có agent '{}'", key)));
    };
    if s.db.has_running_task(&agent.team).map_err(internal)? {
        return Err(err(
            StatusCode::CONFLICT,
            "đội đang xử lý nhiệm vụ — chờ xong rồi thay đổi nhân sự",
        ));
    }
    if agent.kind == "manager" {
        return Err(err(StatusCode::BAD_REQUEST, "không thể xoá Trưởng nhóm"));
    }
    if agent.kind == "worker"
        && agents.iter().filter(|a| a.team == agent.team && a.kind == "worker").count() <= 1
    {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "đội cần ít nhất một nhân sự chuyên môn (worker)",
        ));
    }
    s.db.delete_agent(&key).map_err(internal)?;
    Ok(Json(json!({ "ok": true })))
}

/// Per-agent private-memory SUMMARY (count + space id). The staff dialog
/// deliberately shows only this — browsing the actual items belongs to the
/// Knowledge screen (desktop_app) with its space picker.
async fn agent_knowledge(
    State(s): State<Arc<AppState>>,
    Path(key): Path<String>,
    Query(_q): Query<EventsQuery>,
) -> Result<Json<Value>, ApiError> {
    if s.db.get_agent(&key).map_err(internal)?.is_none() {
        return Err(err(StatusCode::NOT_FOUND, format!("không có agent '{}'", key)));
    }
    let space = crate::senclaw::agent_space(&key);
    match crate::senclaw::knowledge_count(&space).await {
        Ok(count) => Ok(Json(json!({ "space": space, "count": count }))),
        Err(e) => Err(err(StatusCode::BAD_GATEWAY, e)),
    }
}

/// Skills + sub-agents (personas) available on the daemon — feeds the
/// staff-dialog picker.
async fn skills_inventory() -> Json<Value> {
    Json(crate::senclaw::skills_inventory_grouped().await)
}

async fn get_settings(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    let dir = s.db.workspace_dir();
    let files = crate::workspace::list_files(&dir);
    Ok(Json(json!({
        "workspaceDir": dir.to_string_lossy(),
        "workspaceFiles": files.len(),
        "workspaceIsDefault": s.db.get_setting("workspace_dir").map_err(internal)?
            .map(|v| v.trim().is_empty()).unwrap_or(true),
        "features": s.db.features_json(),
    })))
}

#[derive(Deserialize)]
struct SettingsPatch {
    #[serde(rename = "workspaceDir")]
    workspace_dir: Option<String>,
    /// Feature toggles: memory, wiki, workspace, tools, autocontinue.
    features: Option<std::collections::HashMap<String, bool>>,
}

async fn update_settings(
    State(s): State<Arc<AppState>>,
    Json(body): Json<SettingsPatch>,
) -> Result<Json<Value>, ApiError> {
    if let Some(feats) = body.features {
        for (k, v) in feats {
            if ["memory", "wiki", "workspace", "tools", "autocontinue"].contains(&k.as_str()) {
                s.db.set_setting(&format!("feat_{k}"), if v { "1" } else { "0" }).map_err(internal)?;
            }
        }
    }
    if let Some(dir) = body.workspace_dir {
        let trimmed = dir.trim().to_string();
        if trimmed.is_empty() {
            // Quay về thư mục mặc định.
            s.db.set_setting("workspace_dir", "").map_err(internal)?;
        } else {
            let path = crate::workspace::resolve(&trimmed);
            if !path.is_absolute() {
                return Err(err(
                    StatusCode::BAD_REQUEST,
                    "đường dẫn workspace phải là đường dẫn tuyệt đối (hoặc bắt đầu bằng ~/)",
                ));
            }
            crate::workspace::ensure_dir(&path)
                .map_err(|e| err(StatusCode::BAD_REQUEST, format!("không tạo được thư mục: {}", e)))?;
            s.db.set_setting("workspace_dir", &trimmed).map_err(internal)?;
        }
    }
    get_settings(State(s)).await
}

async fn workspace_files(State(s): State<Arc<AppState>>) -> Json<Value> {
    Json(crate::workspace::files_json(&s.db.workspace_dir()))
}

/// Speech → text: forward the recorded clip to the daemon's Whisper endpoint.
async fn stt(mut multipart: Multipart) -> Result<Json<Value>, ApiError> {
    let mut audio: Option<(String, Vec<u8>)> = None;
    let mut language: Option<String> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| err(StatusCode::BAD_REQUEST, format!("đọc audio lỗi: {}", e)))?
    {
        match field.name().unwrap_or("") {
            "language" => language = field.text().await.ok().filter(|s| !s.is_empty()),
            _ => {
                let fname = field.file_name().unwrap_or("audio.webm").to_string();
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|e| err(StatusCode::BAD_REQUEST, format!("đọc audio lỗi: {}", e)))?;
                audio = Some((fname, bytes.to_vec()));
            }
        }
    }
    let (fname, bytes) = audio.ok_or_else(|| err(StatusCode::BAD_REQUEST, "thiếu audio"))?;
    match crate::senclaw::stt(bytes, &fname, language.as_deref()).await {
        Ok(text) => Ok(Json(json!({ "text": text }))),
        Err(e) => Err(err(StatusCode::BAD_GATEWAY, e)),
    }
}

#[derive(Deserialize)]
struct TtsBody {
    text: String,
}

/// Text → speech: return the daemon's synthesized WAV for the browser to play.
async fn tts(Json(body): Json<TtsBody>) -> Result<Response, ApiError> {
    if body.text.trim().is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "text trống"));
    }
    // Cap length so read-aloud of a huge report stays responsive.
    let text: String = body.text.chars().take(4000).collect();
    match crate::senclaw::tts(&text).await {
        Ok(wav) => Ok(([(header::CONTENT_TYPE, "audio/wav")], wav).into_response()),
        Err(e) => Err(err(StatusCode::BAD_GATEWAY, e)),
    }
}

#[derive(Deserialize)]
struct FsDirsQuery {
    path: Option<String>,
}

/// Server-side directory browser for the workspace folder picker (the web UI
/// runs in an iframe and has no native folder dialog with real paths).
/// Lists only directories, hidden entries skipped.
async fn fs_dirs(Query(q): Query<FsDirsQuery>) -> Result<Json<Value>, ApiError> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
    let path = match q.path.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
        Some(p) => crate::workspace::resolve(p),
        None => std::path::PathBuf::from(&home),
    };
    let canon = path.canonicalize().unwrap_or(path.clone());
    if !canon.is_dir() {
        return Err(err(StatusCode::BAD_REQUEST, "không phải thư mục"));
    }
    let mut dirs: Vec<String> = std::fs::read_dir(&canon)
        .map_err(|e| err(StatusCode::BAD_REQUEST, format!("không đọc được thư mục: {}", e)))?
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| !n.starts_with('.'))
        .collect();
    dirs.sort_by_key(|n| n.to_lowercase());
    dirs.truncate(300);
    Ok(Json(json!({
        "path": canon.to_string_lossy(),
        "parent": canon.parent().map(|p| p.to_string_lossy().to_string()),
        "home": home,
        "dirs": dirs,
    })))
}

#[derive(Deserialize)]
struct TaskListQuery {
    limit: Option<i64>,
    team: Option<String>,
}

async fn list_tasks(
    State(s): State<Arc<AppState>>,
    Query(q): Query<TaskListQuery>,
) -> Result<Json<Value>, ApiError> {
    let limit = q.limit.unwrap_or(30).clamp(1, 200);
    let mut tasks = s.db.list_tasks(limit.max(50)).map_err(internal)?;
    if let Some(team) = q.team.as_deref().filter(|t| !t.is_empty()) {
        tasks.retain(|t| t.team == team);
    }
    tasks.truncate(limit as usize);
    Ok(Json(json!({ "tasks": tasks })))
}

#[derive(Deserialize)]
struct CreateTask {
    title: String,
    /// Which team handles the task. Defaults to the first team.
    team: Option<String>,
    #[allow(dead_code)]
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
    let teams = s.db.list_teams().map_err(internal)?;
    let team = body
        .team
        .as_deref()
        .filter(|t| teams.iter().any(|x| &x.key == t))
        .or_else(|| teams.first().map(|t| t.key.as_str()))
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "chưa có đội nào"))?
        .to_string();
    let agents = s.db.list_agents_in(&team).map_err(internal)?;
    if !agents.iter().any(|a| a.kind == "worker" && a.enabled) {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "đội này không còn nhân sự chuyên môn nào đang hoạt động — bật lại trong mục Nhân sự",
        ));
    }
    // Always queue; the scheduler runs one task per team (busy → hàng đợi).
    let task = s.db.create_task(title, "live", &team).map_err(internal)?;
    let queued = s.db.has_running_task(&team).map_err(internal)?;
    Ok(Json(json!({ "task": task, "queued": queued })))
}

async fn queue(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "pending": s.db.pending_tasks().map_err(internal)? })))
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
