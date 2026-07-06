use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures_util::stream::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::db::{default_data_dir, Db};
use crate::llm::{self, ChatBody};
use crate::workspace;

pub struct AppState {
    pub db: Arc<Db>,
    /// The currently open workspace root (absolute). None until a folder opens.
    pub root: Mutex<Option<PathBuf>>,
    pub mcp_tx: tokio::sync::broadcast::Sender<String>,
    pub events_tx: tokio::sync::broadcast::Sender<String>,
    pub watcher: Mutex<Option<notify::RecommendedWatcher>>,
}

impl AppState {
    /// The open workspace root, or a 400 if none is open yet.
    pub fn root(&self) -> Result<PathBuf, ApiError> {
        self.root
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| ApiError(StatusCode::BAD_REQUEST, "no workspace open".into()))
    }
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
fn server(e: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

pub fn expand(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return format!("{}/{}", home.to_string_lossy(), rest);
        }
    }
    path.to_string()
}

pub fn make_state() -> Arc<AppState> {
    let db_path = default_data_dir("code-ide").join("ide.db");
    let db = Arc::new(Db::open(&db_path).expect("open code-ide db"));
    let (mcp_tx, _) = tokio::sync::broadcast::channel(100);
    let (events_tx, _) = tokio::sync::broadcast::channel(256);
    let state = Arc::new(AppState {
        db,
        root: Mutex::new(None),
        mcp_tx,
        events_tx,
        watcher: Mutex::new(None),
    });
    // Restore last workspace if it still exists.
    if let Ok(Some(last)) = state.db.get_meta("root") {
        let p = PathBuf::from(&last);
        if p.is_dir() {
            *state.root.lock().unwrap() = Some(p.clone());
            crate::watch::install_watcher(&state, &p);
        }
    }
    state
}

pub fn api_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/status", get(status))
        .route("/llm-info", get(llm_info))
        .route("/open", post(open))
        .route("/browse", get(browse))
        .route("/recents", get(recents))
        .route("/tree", get(tree))
        .route("/file", get(file))
        .route("/files", get(files))
        .route("/raw", get(raw))
        .route("/save", post(save))
        .route("/create", post(create))
        .route("/rename", post(rename))
        .route("/delete", post(delete))
        .route("/search", get(search))
        .route("/git-status", get(git_status))
        .route("/git/filediff", get(git_filediff))
        .route("/git/stage", post(git_stage))
        .route("/git/unstage", post(git_unstage))
        .route("/git/discard", post(git_discard))
        .route("/git/commit", post(git_commit))
        .route("/git/log", get(git_log))
        .route("/git/head", get(git_head))
        .route("/chat", post(chat))
        .route("/models", get(models))
        .route("/model-active", post(model_active))
        .route("/events", get(events))
        .route("/terminal", get(crate::pty::terminal_ws))
        .route("/mcp/sse", get(crate::mcp::mcp_sse).post(crate::mcp::mcp_message))
        .route("/mcp/message", post(crate::mcp::mcp_message))
        .with_state(state)
}

async fn status(State(s): State<Arc<AppState>>) -> Json<Value> {
    let root = s.root.lock().unwrap().clone();
    let (root_str, name) = match &root {
        Some(p) => (
            Some(p.to_string_lossy().to_string()),
            p.file_name().map(|n| n.to_string_lossy().to_string()),
        ),
        None => (None, None),
    };
    Json(json!({ "root": root_str, "name": name, "hasRoot": root.is_some() }))
}

#[derive(Deserialize)]
struct OpenBody {
    path: String,
}

async fn open(
    State(s): State<Arc<AppState>>,
    Json(b): Json<OpenBody>,
) -> Result<Json<Value>, ApiError> {
    let root = PathBuf::from(expand(&b.path));
    let root = root.canonicalize().map_err(|e| bad(format!("{}: {e}", b.path)))?;
    if !root.is_dir() {
        return Err(bad(format!("not a directory: {}", root.display())));
    }
    let name = root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    *s.root.lock().unwrap() = Some(root.clone());
    let _ = s.db.set_meta("root", &root.to_string_lossy());
    let _ = s.db.touch_recent(&root.to_string_lossy(), &name, now());
    crate::watch::install_watcher(&s, &root);
    let tree = workspace::list_dir(&root, "").map_err(server)?;
    Ok(Json(json!({ "root": root.to_string_lossy(), "name": name, "tree": tree })))
}

#[derive(Deserialize)]
struct BrowseQuery {
    #[serde(default)]
    path: Option<String>,
}

/// List sub-directories of `path` (default: $HOME) for the folder picker.
/// Returns the canonical path, its parent (if any), and immediate child dirs.
async fn browse(Query(q): Query<BrowseQuery>) -> Result<Json<Value>, ApiError> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
    let start = q.path.map(|p| expand(&p)).filter(|p| !p.is_empty()).unwrap_or_else(|| home.clone());
    let dir = PathBuf::from(&start)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(&home));
    if !dir.is_dir() {
        return Err(bad(format!("not a directory: {}", dir.display())));
    }
    let mut dirs: Vec<Value> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                dirs.push(json!({ "name": name, "path": e.path().to_string_lossy() }));
            }
        }
    }
    dirs.sort_by(|a, b| {
        a["name"].as_str().unwrap_or("").to_lowercase().cmp(&b["name"].as_str().unwrap_or("").to_lowercase())
    });
    let parent = dir.parent().map(|p| p.to_string_lossy().to_string());
    Ok(Json(json!({ "path": dir.to_string_lossy(), "parent": parent, "dirs": dirs })))
}

async fn recents(State(s): State<Arc<AppState>>) -> Json<Value> {
    let rows = s.db.recents(20).unwrap_or_default();
    let list: Vec<Value> = rows
        .into_iter()
        .filter(|(p, _, _)| PathBuf::from(p).is_dir())
        .map(|(path, name, at)| json!({ "path": path, "name": name, "openedAt": at }))
        .collect();
    Json(json!(list))
}

#[derive(Deserialize)]
struct PathQuery {
    #[serde(default)]
    path: String,
}

async fn tree(
    State(s): State<Arc<AppState>>,
    Query(q): Query<PathQuery>,
) -> Result<Json<Value>, ApiError> {
    let root = s.root()?;
    Ok(Json(json!(workspace::list_dir(&root, &q.path).map_err(bad)?)))
}

async fn file(
    State(s): State<Arc<AppState>>,
    Query(q): Query<PathQuery>,
) -> Result<Json<Value>, ApiError> {
    let root = s.root()?;
    Ok(Json(json!(workspace::read_file(&root, &q.path).map_err(bad)?)))
}

/// Flat list of workspace files for the chat `@`-mention picker.
async fn files(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    let root = s.root()?;
    let list = tokio::task::spawn_blocking(move || workspace::list_all_files(&root, 6000))
        .await
        .map_err(server)?;
    Ok(Json(json!(list)))
}

/// Serve a file's raw bytes with a guessed content-type (for image preview).
async fn raw(
    State(s): State<Arc<AppState>>,
    Query(q): Query<PathQuery>,
) -> Result<Response, ApiError> {
    let root = s.root()?;
    let path = workspace::safe_join(&root, &q.path).map_err(bad)?;
    if !path.is_file() {
        return Err(ApiError(StatusCode::NOT_FOUND, format!("not a file: {}", q.path)));
    }
    let bytes = tokio::fs::read(&path).await.map_err(server)?;
    let ct = match path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "avif" => "image/avif",
        _ => "application/octet-stream",
    };
    Ok((
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, ct)],
        bytes,
    )
        .into_response())
}

#[derive(Deserialize)]
struct SaveBody {
    path: String,
    content: String,
}

async fn save(
    State(s): State<Arc<AppState>>,
    Json(b): Json<SaveBody>,
) -> Result<Json<Value>, ApiError> {
    let root = s.root()?;
    workspace::write_file(&root, &b.path, &b.content).map_err(bad)?;
    Ok(Json(json!({ "success": true, "path": b.path })))
}

#[derive(Deserialize)]
struct CreateBody {
    path: String,
    #[serde(default)]
    dir: bool,
}

async fn create(
    State(s): State<Arc<AppState>>,
    Json(b): Json<CreateBody>,
) -> Result<Json<Value>, ApiError> {
    let root = s.root()?;
    workspace::create_path(&root, &b.path, b.dir).map_err(bad)?;
    Ok(Json(json!({ "success": true, "path": b.path })))
}

#[derive(Deserialize)]
struct RenameBody {
    from: String,
    to: String,
}

async fn rename(
    State(s): State<Arc<AppState>>,
    Json(b): Json<RenameBody>,
) -> Result<Json<Value>, ApiError> {
    let root = s.root()?;
    workspace::rename_path(&root, &b.from, &b.to).map_err(bad)?;
    Ok(Json(json!({ "success": true })))
}

async fn delete(
    State(s): State<Arc<AppState>>,
    Json(b): Json<PathQuery>,
) -> Result<Json<Value>, ApiError> {
    let root = s.root()?;
    workspace::delete_path(&root, &b.path).map_err(bad)?;
    Ok(Json(json!({ "success": true })))
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
    #[serde(default = "default_limit")]
    limit: usize,
}
fn default_limit() -> usize {
    100
}

async fn search(
    State(s): State<Arc<AppState>>,
    Query(q): Query<SearchQuery>,
) -> Result<Json<Value>, ApiError> {
    let root = s.root()?;
    let db = s.db.clone();
    let _ = db;
    let hits = tokio::task::spawn_blocking(move || workspace::search_text(&root, &q.q, q.limit))
        .await
        .map_err(server)?
        .map_err(server)?;
    Ok(Json(json!(hits)))
}

/// `git status --porcelain` for the open workspace → { path: statusCode } map.
async fn git_status(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    let root = s.root()?;
    let out = tokio::process::Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["status", "--porcelain=v1", "-z"])
        .output()
        .await;
    let mut map = serde_json::Map::new();
    if let Ok(out) = out {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            for entry in text.split('\0').filter(|e| e.len() > 3) {
                let code = entry[..2].to_string(); // raw 2-char XY (index, worktree)
                let path = entry[3..].to_string();
                map.insert(path, json!(code));
            }
        }
    }
    Ok(Json(json!({ "files": map })))
}

// ===== Git =====
/// Run `git -C <root> <args>` and return stdout (trimmed of trailing newline).
async fn git_out(root: &std::path::Path, args: &[&str]) -> Result<String, ApiError> {
    let out = tokio::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, format!("git: {e}")))?;
    if !out.status.success() {
        return Err(ApiError(StatusCode::BAD_REQUEST, String::from_utf8_lossy(&out.stderr).trim().to_string()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

#[derive(Deserialize)]
struct GitPathsQuery {
    path: String,
    #[serde(default)]
    staged: bool,
}

/// Side-by-side diff: HEAD (or index) content vs the working/index copy.
async fn git_filediff(
    State(s): State<Arc<AppState>>,
    Query(q): Query<GitPathsQuery>,
) -> Result<Json<Value>, ApiError> {
    let root = s.root()?;
    // Original = HEAD version (empty if the file is new / untracked).
    let original = git_out(&root, &["show", &format!("HEAD:{}", q.path)]).await.unwrap_or_default();
    // Modified = staged (index) copy or the working-tree file.
    let modified = if q.staged {
        git_out(&root, &["show", &format!(":{}", q.path)]).await.unwrap_or_default()
    } else {
        let p = workspace::safe_join(&root, &q.path).map_err(bad)?;
        tokio::fs::read_to_string(&p).await.unwrap_or_default()
    };
    Ok(Json(json!({ "path": q.path, "original": original, "modified": modified })))
}

#[derive(Deserialize)]
struct GitPathsBody {
    paths: Vec<String>,
}

async fn git_stage(State(s): State<Arc<AppState>>, Json(b): Json<GitPathsBody>) -> Result<Json<Value>, ApiError> {
    let root = s.root()?;
    let mut args = vec!["add", "--"];
    args.extend(b.paths.iter().map(|s| s.as_str()));
    git_out(&root, &args).await?;
    Ok(Json(json!({ "success": true })))
}

async fn git_unstage(State(s): State<Arc<AppState>>, Json(b): Json<GitPathsBody>) -> Result<Json<Value>, ApiError> {
    let root = s.root()?;
    let mut args = vec!["reset", "-q", "HEAD", "--"];
    args.extend(b.paths.iter().map(|s| s.as_str()));
    git_out(&root, &args).await?;
    Ok(Json(json!({ "success": true })))
}

async fn git_discard(State(s): State<Arc<AppState>>, Json(b): Json<GitPathsBody>) -> Result<Json<Value>, ApiError> {
    let root = s.root()?;
    let mut args = vec!["checkout", "--"];
    args.extend(b.paths.iter().map(|s| s.as_str()));
    git_out(&root, &args).await?;
    Ok(Json(json!({ "success": true })))
}

#[derive(Deserialize)]
struct GitCommitBody {
    message: String,
}

async fn git_commit(State(s): State<Arc<AppState>>, Json(b): Json<GitCommitBody>) -> Result<Json<Value>, ApiError> {
    let root = s.root()?;
    if b.message.trim().is_empty() {
        return Err(bad("commit message trống"));
    }
    let out = git_out(&root, &["commit", "-m", b.message.trim()]).await?;
    Ok(Json(json!({ "success": true, "output": out.trim() })))
}

async fn git_head(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    let root = s.root()?;
    let branch = git_out(&root, &["rev-parse", "--abbrev-ref", "HEAD"]).await.unwrap_or_default().trim().to_string();
    Ok(Json(json!({ "branch": branch })))
}

#[derive(Deserialize)]
struct GitLogQuery {
    #[serde(default = "git_log_limit")]
    limit: u32,
}
fn git_log_limit() -> u32 {
    120
}

/// Commit history across all branches for the graph view.
async fn git_log(
    State(s): State<Arc<AppState>>,
    Query(q): Query<GitLogQuery>,
) -> Result<Json<Value>, ApiError> {
    let root = s.root()?;
    let fmt = "%H%x1f%P%x1f%an%x1f%at%x1f%D%x1f%s";
    let text = git_out(
        &root,
        &["log", "--all", "--date-order", &format!("-n{}", q.limit), &format!("--pretty=format:{fmt}")],
    )
    .await
    .unwrap_or_default();
    let commits: Vec<Value> = text
        .lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\u{1f}').collect();
            if f.len() < 6 {
                return None;
            }
            let parents: Vec<&str> = f[1].split_whitespace().collect();
            let refs: Vec<String> = f[4]
                .split(',')
                .map(|r| r.trim().replace("HEAD -> ", "").to_string())
                .filter(|r| !r.is_empty())
                .collect();
            Some(json!({
                "hash": f[0], "parents": parents, "author": f[2],
                "time": f[3].parse::<i64>().unwrap_or(0), "refs": refs, "subject": f[5],
            }))
        })
        .collect();
    Ok(Json(json!({ "commits": commits })))
}

async fn chat(
    State(_s): State<Arc<AppState>>,
    Json(b): Json<ChatBody>,
) -> Result<Json<Value>, ApiError> {
    if b.messages.is_empty() {
        return Err(bad("no messages"));
    }
    match llm::chat(&b).await {
        Ok((text, model)) => Ok(Json(json!({ "text": text, "model": model }))),
        Err(e) => Err(ApiError(StatusCode::BAD_GATEWAY, e)),
    }
}

/// List the daemon's configured LLM models (id + display name).
async fn models() -> Result<Json<Value>, ApiError> {
    llm::list_models().await.map(Json).map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e))
}

#[derive(Deserialize)]
struct ModelActiveBody {
    id: String,
}

/// Set the daemon's active main model.
async fn model_active(Json(b): Json<ModelActiveBody>) -> Result<Json<Value>, ApiError> {
    llm::set_active_model(&b.id).await.map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e))?;
    Ok(Json(json!({ "success": true, "activeId": b.id })))
}

/// SSE stream of filesystem change events for the open workspace.
async fn events(
    State(s): State<Arc<AppState>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = s.events_tx.subscribe();
    let stream = async_stream::stream! {
        loop {
            match rx.recv().await {
                Ok(msg) => yield Ok(Event::default().event("fs").data(msg)),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// Which SenClaw LLM the bridge will use (mirrors DeepWiki's llm-info probe).
async fn llm_info() -> Json<Value> {
    let base = std::env::var("SENCLAW_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:18788".into());
    let url = format!("{}/api/llm-config", base.trim_end_matches('/'));
    let fetch = reqwest::Client::new()
        .get(&url)
        .timeout(Duration::from_secs(6))
        .send()
        .await;
    match fetch {
        Ok(r) => match r.json::<Value>().await {
            Ok(v) => {
                let active = v.get("activeId").and_then(|x| x.as_str()).unwrap_or("");
                let cfg = v.get("configs").and_then(|a| a.as_array()).and_then(|a| {
                    a.iter().find(|c| c.get("id").and_then(|x| x.as_str()) == Some(active))
                });
                let model = cfg.and_then(|c| c.get("modelName")).and_then(|x| x.as_str());
                Json(json!({ "ok": model.is_some(), "daemon": base, "model": model }))
            }
            Err(e) => Json(json!({ "ok": false, "daemon": base, "error": format!("parse: {e}") })),
        },
        Err(e) => Json(json!({ "ok": false, "daemon": base, "error": format!("Không kết nối daemon: {e}") })),
    }
}
