use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use crate::db::{default_data_dir, Db};
use crate::query;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::wiki;

pub struct AppState {
    pub db: Arc<Db>,
    pub mcp_tx: tokio::sync::broadcast::Sender<String>,
    pub watch_tx: tokio::sync::mpsc::Sender<()>,
    pub watcher: Mutex<Option<notify::RecommendedWatcher>>,
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

pub fn api_router() -> Router {
    let db_path = default_data_dir("deepwiki").join("index.db");
    let db = Arc::new(Db::open(&db_path).expect("open deepwiki index db"));
    wiki::migrate(&db).expect("migrate wiki tables");
    let (mcp_tx, _) = tokio::sync::broadcast::channel(100);
    let (watch_tx, watch_rx) = tokio::sync::mpsc::channel::<()>(64);
    crate::watch::spawn_reindexer(db.clone(), watch_rx);

    let state = Arc::new(AppState {
        db,
        mcp_tx,
        watch_tx,
        watcher: Mutex::new(None),
    });

    Router::new()
        // index + wiki
        .route("/status", get(status))
        .route("/recents", get(recents))
        .route("/index", post(index))
        .route("/outline", get(outline))
        .route("/context", get(context))
        .route("/ask", get(context))
        .route("/pages", get(pages))
        .route("/page", get(get_page).post(save_page).delete(delete_page))
        // code graph
        .route("/search", get(search))
        .route("/symbol", get(symbol))
        .route("/explore", get(explore))
        .route("/file", get(file))
        .route("/files", get(files))
        .route("/snippet", get(snippet))
        // mcp
        .route("/mcp/sse", get(crate::mcp::mcp_sse).post(crate::mcp::mcp_message))
        .route("/mcp/message", post(crate::mcp::mcp_message))
        .with_state(state)
}

async fn status(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    let stats = query::stats(&s.db).map_err(server)?;
    let root = s.db.get_meta("root").map_err(server)?;
    let pages = wiki::page_count(&s.db).map_err(server)?;
    Ok(Json(json!({ "root": root, "stats": stats, "pages": pages })))
}

#[derive(Deserialize)]
struct IndexBody {
    path: String,
}

async fn index(
    State(s): State<Arc<AppState>>,
    Json(b): Json<IndexBody>,
) -> Result<Json<Value>, ApiError> {
    let root = PathBuf::from(expand(&b.path));
    if !root.is_dir() {
        return Err(bad(format!("not a directory: {}", root.display())));
    }
    let db = s.db.clone();
    let root_clone = root.clone();
    let report = tokio::task::spawn_blocking(move || crate::index::index_repo(&db, &root_clone))
        .await
        .map_err(server)?
        .map_err(server)?;
    crate::watch::install_watcher(&s, &root);
    Ok(Json(serde_json::to_value(report).unwrap_or_default()))
}

async fn recents(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(query::recent_roots(&s.db).map_err(server)?)))
}

async fn outline(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(wiki::outline(&s.db).map_err(server)?))
}

#[derive(Deserialize)]
struct ContextQuery {
    q: String,
    #[serde(default = "default_depth")]
    depth: u32,
}
fn default_depth() -> u32 {
    3
}

async fn context(
    State(s): State<Arc<AppState>>,
    Query(q): Query<ContextQuery>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(wiki::context(&s.db, &q.q, q.depth).map_err(server)?))
}

async fn pages(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(wiki::list_pages(&s.db).map_err(server)?)))
}

#[derive(Deserialize)]
struct SlugQuery {
    slug: String,
}

async fn get_page(
    State(s): State<Arc<AppState>>,
    Query(q): Query<SlugQuery>,
) -> Result<Json<Value>, ApiError> {
    match wiki::get_page(&s.db, &q.slug).map_err(server)? {
        Some(p) => Ok(Json(json!(p))),
        None => Err(ApiError(StatusCode::NOT_FOUND, format!("no page: {}", q.slug))),
    }
}

async fn save_page(
    State(s): State<Arc<AppState>>,
    Json(p): Json<wiki::PageInput>,
) -> Result<Json<Value>, ApiError> {
    wiki::save_page(&s.db, &p).map_err(server)?;
    Ok(Json(json!({ "success": true, "slug": p.slug })))
}

async fn delete_page(
    State(s): State<Arc<AppState>>,
    Query(q): Query<SlugQuery>,
) -> Result<Json<Value>, ApiError> {
    wiki::delete_page(&s.db, &q.slug).map_err(server)?;
    Ok(Json(json!({ "success": true })))
}

// ===== Code-graph endpoints =====

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
    #[serde(default = "default_limit")]
    limit: u32,
}
fn default_limit() -> u32 {
    30
}

async fn search(
    State(s): State<Arc<AppState>>,
    Query(q): Query<SearchQuery>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(query::search(&s.db, &q.q, q.limit).map_err(server)?)))
}

#[derive(Deserialize)]
struct NameQuery {
    name: String,
}

async fn symbol(
    State(s): State<Arc<AppState>>,
    Query(q): Query<NameQuery>,
) -> Result<Json<Value>, ApiError> {
    let defs = query::symbols_by_name(&s.db, &q.name).map_err(server)?;
    let callers = query::callers(&s.db, &q.name, 100).map_err(server)?;
    let callees = query::callees(&s.db, &q.name, 100).map_err(server)?;
    Ok(Json(json!({ "name": q.name, "definitions": defs, "callers": callers, "callees": callees })))
}

async fn explore(
    State(s): State<Arc<AppState>>,
    Query(q): Query<ContextQuery>,
) -> Result<Json<Value>, ApiError> {
    let ex = query::explore(&s.db, &q.q, q.depth).map_err(server)?;
    Ok(Json(serde_json::to_value(ex).unwrap_or_default()))
}

#[derive(Deserialize)]
struct PathQuery {
    path: String,
}

async fn file(
    State(s): State<Arc<AppState>>,
    Query(q): Query<PathQuery>,
) -> Result<Json<Value>, ApiError> {
    let outline = query::file_outline(&s.db, &q.path).map_err(server)?;
    let imports = query::imports_of_file(&s.db, &q.path).map_err(server)?;
    Ok(Json(json!({ "path": q.path, "outline": outline, "imports": imports })))
}

async fn files(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(query::list_files(&s.db).map_err(server)?)))
}

#[derive(Deserialize)]
struct SnippetQuery {
    name: Option<String>,
    path: Option<String>,
    start: Option<i64>,
    end: Option<i64>,
    #[serde(default = "default_ctx")]
    context: i64,
}
fn default_ctx() -> i64 {
    2
}

async fn snippet(
    State(s): State<Arc<AppState>>,
    Query(q): Query<SnippetQuery>,
) -> Result<Json<Value>, ApiError> {
    let v = if let Some(name) = q.name.filter(|s| !s.is_empty()) {
        query::symbol_source(&s.db, &name, q.context).map_err(server)?
    } else if let Some(path) = q.path.filter(|s| !s.is_empty()) {
        let start = q.start.unwrap_or(1);
        let end = q.end.unwrap_or(start + 40);
        query::snippet(&s.db, &path, start, end, q.context).map_err(server)?
    } else {
        return Err(bad("provide either name or path+start/end"));
    };
    Ok(Json(v))
}

pub fn expand(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return format!("{}/{}", home.to_string_lossy(), rest);
        }
    }
    path.to_string()
}
