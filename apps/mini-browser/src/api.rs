use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::db::Db;
use crate::llm::{self, ChatBody};
use crate::session::BrowserSession;

pub struct AppState {
    pub session: Arc<BrowserSession>,
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
fn gateway(e: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::BAD_GATEWAY, e.to_string())
}

pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

pub fn api_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/status", get(status))
        .route("/info", get(info))
        .route("/navigate", post(navigate))
        .route("/back", post(back))
        .route("/forward", post(forward))
        .route("/reload", post(reload))
        .route("/snapshot", get(snapshot))
        .route("/click", post(click))
        .route("/type", post(type_text))
        .route("/key", post(key))
        .route("/scroll", post(scroll))
        .route("/execute", post(execute))
        .route("/tabs", get(tabs))
        .route("/tabs/new", post(tab_new))
        .route("/tabs/switch", post(tab_switch))
        .route("/tabs/close", post(tab_close))
        .route("/history", get(history))
        .route("/bookmarks", get(bookmarks))
        .route("/bookmark", post(bookmark_add))
        .route("/bookmark/remove", post(bookmark_remove))
        .route("/chat", post(chat))
        .route("/act", post(act))
        .route("/extract", post(extract))
        .route("/models", get(models))
        .route("/model-active", post(model_active))
        .route("/ws", get(ws_handler))
        .route("/mcp/sse", get(crate::mcp::mcp_sse).post(crate::mcp::mcp_message))
        .route("/mcp/message", post(crate::mcp::mcp_message))
        .with_state(state)
}

async fn status() -> Json<Value> {
    Json(json!({ "ok": true, "app": "mini-browser" }))
}

async fn info(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(s.session.info().await.map_err(gateway)?))
}

#[derive(Deserialize)]
struct UrlBody {
    url: String,
}
async fn navigate(
    State(s): State<Arc<AppState>>,
    Json(b): Json<UrlBody>,
) -> Result<Json<Value>, ApiError> {
    let v = s.session.navigate(&b.url).await.map_err(gateway)?;
    record_visit(&s, &v);
    Ok(Json(v))
}

async fn back(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(s.session.go_back().await.map_err(gateway)?))
}
async fn forward(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(s.session.go_forward().await.map_err(gateway)?))
}
async fn reload(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(s.session.reload().await.map_err(gateway)?))
}

async fn snapshot(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(s.session.snapshot().await.map_err(gateway)?))
}

#[derive(Deserialize)]
struct ClickBody {
    index: Option<i64>,
    x: Option<f64>,
    y: Option<f64>,
}
async fn click(
    State(s): State<Arc<AppState>>,
    Json(b): Json<ClickBody>,
) -> Result<Json<Value>, ApiError> {
    let v = if let (Some(x), Some(y)) = (b.x, b.y) {
        s.session.click_xy(x, y).await
    } else {
        s.session.click_index(b.index.ok_or_else(|| bad("index or x/y required"))?).await
    };
    Ok(Json(v.map_err(gateway)?))
}

#[derive(Deserialize)]
struct TypeBody {
    index: Option<i64>,
    text: String,
    #[serde(default)]
    submit: bool,
}
async fn type_text(
    State(s): State<Arc<AppState>>,
    Json(b): Json<TypeBody>,
) -> Result<Json<Value>, ApiError> {
    let v = match b.index {
        Some(i) => s.session.type_index(i, &b.text, b.submit).await,
        None => s.session.type_text(&b.text).await,
    };
    Ok(Json(v.map_err(gateway)?))
}

#[derive(Deserialize)]
struct KeyBody {
    key: String,
}
async fn key(State(s): State<Arc<AppState>>, Json(b): Json<KeyBody>) -> Result<Json<Value>, ApiError> {
    Ok(Json(s.session.press_key(&b.key).await.map_err(gateway)?))
}

#[derive(Deserialize)]
struct ScrollBody {
    #[serde(default)]
    dx: f64,
    #[serde(default)]
    dy: f64,
}
async fn scroll(
    State(s): State<Arc<AppState>>,
    Json(b): Json<ScrollBody>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(s.session.scroll(b.dx, b.dy).await.map_err(gateway)?))
}

#[derive(Deserialize)]
struct ExecBody {
    script: String,
}
async fn execute(
    State(s): State<Arc<AppState>>,
    Json(b): Json<ExecBody>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(s.session.execute_js(&b.script).await.map_err(gateway)?))
}

async fn tabs(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(s.session.list_tabs().await.map_err(gateway)?))
}
async fn tab_new(
    State(s): State<Arc<AppState>>,
    Json(b): Json<serde_json::Value>,
) -> Result<Json<Value>, ApiError> {
    let url = b["url"].as_str();
    Ok(Json(s.session.new_tab(url).await.map_err(gateway)?))
}
#[derive(Deserialize)]
struct IndexBody {
    index: usize,
}
async fn tab_switch(
    State(s): State<Arc<AppState>>,
    Json(b): Json<IndexBody>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(s.session.switch_tab(b.index).await.map_err(gateway)?))
}
async fn tab_close(
    State(s): State<Arc<AppState>>,
    Json(b): Json<IndexBody>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(s.session.close_tab(b.index).await.map_err(gateway)?))
}

async fn history(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(s.db.recent_history(100).map_err(bad)?)))
}
async fn bookmarks(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(s.db.list_bookmarks().map_err(bad)?)))
}
#[derive(Deserialize)]
struct BookmarkBody {
    url: String,
    #[serde(default)]
    title: String,
}
async fn bookmark_add(
    State(s): State<Arc<AppState>>,
    Json(b): Json<BookmarkBody>,
) -> Result<Json<Value>, ApiError> {
    s.db.add_bookmark(&b.url, &b.title, now()).map_err(bad)?;
    Ok(Json(json!({ "ok": true })))
}
async fn bookmark_remove(
    State(s): State<Arc<AppState>>,
    Json(b): Json<UrlBody>,
) -> Result<Json<Value>, ApiError> {
    s.db.remove_bookmark(&b.url).map_err(bad)?;
    Ok(Json(json!({ "ok": true })))
}

async fn chat(
    State(_s): State<Arc<AppState>>,
    Json(b): Json<ChatBody>,
) -> Result<Json<Value>, ApiError> {
    let (answer, model) = llm::chat(&b).await.map_err(gateway)?;
    Ok(Json(json!({ "answer": answer, "model": model })))
}

#[derive(Deserialize)]
struct ActBody {
    instruction: String,
    #[serde(default)]
    max_steps: Option<usize>,
}
async fn act(
    State(s): State<Arc<AppState>>,
    Json(b): Json<ActBody>,
) -> Result<Json<Value>, ApiError> {
    let v = llm::act(&s.session, &b.instruction, b.max_steps.unwrap_or(8)).await.map_err(gateway)?;
    Ok(Json(v))
}

#[derive(Deserialize)]
struct ExtractBody {
    request: String,
}
async fn extract(
    State(s): State<Arc<AppState>>,
    Json(b): Json<ExtractBody>,
) -> Result<Json<Value>, ApiError> {
    let (answer, model) = llm::extract(&s.session, &b.request).await.map_err(gateway)?;
    Ok(Json(json!({ "answer": answer, "model": model })))
}

async fn models() -> Result<Json<Value>, ApiError> {
    Ok(Json(llm::list_models().await.map_err(gateway)?))
}
#[derive(Deserialize)]
struct ModelActiveBody {
    id: String,
}
async fn model_active(Json(b): Json<ModelActiveBody>) -> Result<Json<Value>, ApiError> {
    llm::set_active_model(&b.id).await.map_err(gateway)?;
    Ok(Json(json!({ "ok": true })))
}

/// Record a `{url,title}` visit into history (best-effort).
fn record_visit(s: &Arc<AppState>, v: &Value) {
    let url = v["url"].as_str().unwrap_or("");
    let title = v["title"].as_str().unwrap_or("");
    s.db.add_history(url, title, now()).ok();
}

// ---- Live-view WebSocket: streams JPEG frames + relays user input ----

async fn ws_handler(ws: WebSocketUpgrade, State(s): State<Arc<AppState>>) -> Response {
    ws.on_upgrade(move |socket| live_view(socket, s))
}

async fn live_view(mut socket: WebSocket, s: Arc<AppState>) {
    let mut ticker = tokio::time::interval(Duration::from_millis(330));
    let mut last_url = String::new();
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                match s.session.screenshot_b64().await {
                    Ok(data) => {
                        let info = s.session.info().await.unwrap_or(json!({}));
                        let url = info["url"].as_str().unwrap_or("").to_string();
                        if url != last_url && !url.is_empty() {
                            s.db.add_history(&url, info["title"].as_str().unwrap_or(""), now()).ok();
                            last_url = url.clone();
                        }
                        let msg = json!({ "type": "frame", "data": data, "url": url, "title": info["title"] });
                        if socket.send(Message::Text(msg.to_string())).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => { /* page busy navigating — skip this frame */ }
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Text(t))) => { handle_input(&s, &t).await; }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }
}

/// Handle a user-input frame from the live-view UI.
async fn handle_input(s: &Arc<AppState>, text: &str) {
    let Ok(v) = serde_json::from_str::<Value>(text) else { return };
    let action = v["action"].as_str().unwrap_or("");
    let sess = &s.session;
    match action {
        "navigate" => { sess.navigate(v["url"].as_str().unwrap_or("")).await.ok(); }
        "click" => {
            if let (Some(x), Some(y)) = (v["x"].as_f64(), v["y"].as_f64()) {
                sess.click_xy(x, y).await.ok();
            }
        }
        "scroll" => { sess.scroll(v["dx"].as_f64().unwrap_or(0.0), v["dy"].as_f64().unwrap_or(0.0)).await.ok(); }
        "type" => { sess.type_text(v["text"].as_str().unwrap_or("")).await.ok(); }
        "press" => { sess.press_key(v["key"].as_str().unwrap_or("")).await.ok(); }
        "back" => { sess.go_back().await.ok(); }
        "forward" => { sess.go_forward().await.ok(); }
        "reload" => { sess.reload().await.ok(); }
        _ => {}
    }
}
