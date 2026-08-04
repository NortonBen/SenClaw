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
    /// Agent progress, relayed to the UI over the live-view socket. A run can
    /// take a minute; without this the panel would sit blank until it ended.
    pub agent_tx: tokio::sync::broadcast::Sender<Value>,
    /// One run at a time. Two agents clicking the same page would interleave
    /// their actions and neither could tell what its own click did.
    pub agent_lock: Arc<tokio::sync::Mutex<()>>,
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
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
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
        .route("/find", post(find))
        .route("/console", get(console))
        .route("/network", get(network))
        .route("/dialog", post(dialog))
        .route("/highlight", post(highlight))
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
        .route("/chat/history", get(chat_history))
        .route("/chat/clear", post(chat_clear))
        .route("/act", post(act))
        .route("/act/runs", get(act_runs))
        .route("/act/run/:id", get(act_run))
        .route("/settings", get(get_settings).post(set_settings))
        .route("/takeover", get(takeover_status).post(takeover_set))
        .route("/takeover/ping", post(takeover_ping))
        .route("/knowledge", get(knowledge))
        .route("/knowledge/forget", post(knowledge_forget))
        .route("/extract", post(extract))
        .route("/models", get(models))
        .route("/model-active", post(model_active))
        .route("/ws", get(ws_handler))
        .route(
            "/mcp/sse",
            get(crate::mcp::mcp_sse).post(crate::mcp::mcp_message),
        )
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
    let snap = s.session.snapshot().await.map_err(gateway)?;
    Ok(Json(json!({
        "url": snap.url, "title": snap.title, "count": snap.count,
        "new": snap.new_refs, "truncated": snap.truncated, "tree": snap.tree,
    })))
}

#[derive(Deserialize)]
struct FindBody {
    text: String,
}
async fn find(
    State(s): State<Arc<AppState>>,
    Json(b): Json<FindBody>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(s.session.find(&b.text).await.map_err(gateway)?))
}

async fn console(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(
        s.session.active_recorder().await.console_json(false, 200),
    ))
}

async fn network(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(
        s.session
            .active_recorder()
            .await
            .requests_json(false, None, 200),
    ))
}

#[derive(Deserialize)]
struct DialogBody {
    accept: bool,
    #[serde(default)]
    prompt_text: Option<String>,
}
async fn dialog(
    State(s): State<Arc<AppState>>,
    Json(b): Json<DialogBody>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(
        s.session
            .handle_dialog(b.accept, b.prompt_text.as_deref())
            .await
            .map_err(gateway)?,
    ))
}

#[derive(Deserialize)]
struct HighlightBody {
    r#ref: String,
    #[serde(default)]
    ms: Option<u64>,
}
async fn highlight(
    State(s): State<Arc<AppState>>,
    Json(b): Json<HighlightBody>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(
        s.session
            .highlight_ref(&b.r#ref, b.ms.unwrap_or(1200))
            .await
            .map_err(gateway)?,
    ))
}

#[derive(Deserialize)]
struct ClickBody {
    #[serde(rename = "ref")]
    element: Option<String>,
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
        let r = b.element.ok_or_else(|| bad("ref or x/y required"))?;
        s.session.click_ref(&r, "left", 1).await
    };
    Ok(Json(v.map_err(gateway)?))
}

#[derive(Deserialize)]
struct TypeBody {
    #[serde(rename = "ref")]
    element: Option<String>,
    text: String,
    #[serde(default)]
    submit: bool,
}
async fn type_text(
    State(s): State<Arc<AppState>>,
    Json(b): Json<TypeBody>,
) -> Result<Json<Value>, ApiError> {
    let v = match b.element {
        Some(r) => s.session.type_ref(&r, &b.text, b.submit, true).await,
        None => s.session.type_text(&b.text).await,
    };
    Ok(Json(v.map_err(gateway)?))
}

#[derive(Deserialize)]
struct KeyBody {
    key: String,
}
async fn key(
    State(s): State<Arc<AppState>>,
    Json(b): Json<KeyBody>,
) -> Result<Json<Value>, ApiError> {
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
    Ok(Json(
        s.session.execute_js(&b.script).await.map_err(gateway)?,
    ))
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

/// Handle a chat message: answer it, or carry it out and report back.
///
/// The whole exchange is persisted, so the conversation survives a reload, and
/// an assistant message produced by a run carries that run's id — the link the
/// Act panel follows.
async fn chat(
    State(s): State<Arc<AppState>>,
    Json(b): Json<ChatBody>,
) -> Result<Json<Value>, ApiError> {
    let user_text = b
        .messages
        .last()
        .map(|m| m.content.clone())
        .unwrap_or_default();
    if !user_text.trim().is_empty() {
        s.db.add_chat("user", &user_text, None, now()).ok();
    }

    let (goal, ack) = match llm::chat_decide(&b).await.map_err(gateway)? {
        llm::ChatPlan::Answer(text) => {
            s.db.add_chat("assistant", &text, None, now()).ok();
            return Ok(Json(json!({ "answer": text, "mode": "answer" })));
        }
        llm::ChatPlan::Act { goal, ack } => (goal, ack),
    };

    // Say what is about to happen before spending a minute on it.
    let _ = s
        .agent_tx
        .send(json!({ "type": "agent", "kind": "ack", "body": { "text": ack } }));

    let outcome = run_agent(&s, &goal, "chat").await.map_err(gateway)?;
    let answer = llm::report(&goal, &outcome).await.map_err(gateway)?;
    let run_id = outcome["run"].as_i64();
    s.db.add_chat("assistant", &answer, run_id, now()).ok();
    Ok(Json(json!({
        "answer": answer, "mode": "act", "ack": ack, "run": run_id,
        "achieved": outcome["achieved"], "plans_used": outcome["plans_used"],
    })))
}

/// The MCP entry point. Same engine, same budget, same history.
pub async fn run_agent_for_mcp(s: &Arc<AppState>, goal: &str) -> Result<Value, String> {
    s.db.add_chat("user", goal, None, now()).ok();
    let out = run_agent(s, goal, "mcp").await?;
    if let Ok(answer) = llm::report(goal, &out).await {
        s.db.add_chat("assistant", &answer, out["run"].as_i64(), now())
            .ok();
    }
    Ok(out)
}

/// Start a run, drive it to completion, record how it ended, and learn from it.
async fn run_agent(s: &Arc<AppState>, goal: &str, source: &str) -> Result<Value, String> {
    let _guard = s.agent_lock.lock().await;
    let run_id = s.db.start_run(goal, source, now()).map_err(|e| e.to_string())?;
    let ctx = llm::RunCtx { db: s.db.clone(), run_id, events: s.agent_tx.clone() };
    let _ = s.agent_tx.send(
        json!({ "type": "agent", "run": run_id, "kind": "run:start", "body": { "goal": goal } }),
    );

    // What earlier runs learned about wherever we are standing. Retrieval is by
    // host: the lessons worth keeping are about a particular site's controls,
    // and the same handful of sites recur constantly, so a host key finds them
    // without needing embeddings or a similarity search.
    let host = s
        .session
        .info()
        .await
        .ok()
        .and_then(|i| i["url"].as_str().map(llm::host_of))
        .unwrap_or_default();
    let lessons = if s.db.learning_enabled() && !host.is_empty() {
        s.db.lessons_for(&host, 8).unwrap_or_default()
    } else {
        Vec::new()
    };
    if !lessons.is_empty() {
        let _ = s.agent_tx.send(json!({
            "type": "agent", "run": run_id, "kind": "recall",
            "body": { "detail": format!("{} note(s) remembered about {host}", lessons.len()) }
        }));
    }
    let shown: Vec<i64> = lessons.iter().map(|l| l.id).collect();

    let max_plans = s.db.max_plans();
    let result = llm::run_goal(&s.session, &ctx, goal, max_plans, &lessons).await;

    match &result {
        Ok(v) => {
            let achieved = v["achieved"].as_bool().unwrap_or(false);
            s.db.finish_run(
                run_id,
                if achieved { "done" } else { "unfinished" },
                v["plans_used"].as_i64().unwrap_or(0),
                v["reason"].as_str().unwrap_or(""),
                Some(achieved),
                now(),
            )
            .ok();

            // Credit or debit the notes this run was shown. A note that keeps
            // being present when things go well earns its place; one that keeps
            // being present when they do not falls out of retrieval on its own,
            // which is the whole answer to "what happens when a site changes".
            s.db.score_lessons(&shown, achieved, now()).ok();

            if achieved && s.db.learning_enabled() {
                learn_from(s, run_id, goal, v).await;
            }
        }
        Err(e) => {
            s.db.finish_run(run_id, "error", 0, e, Some(false), now()).ok();
            s.db.score_lessons(&shown, false, now()).ok();
        }
    }
    let _ = s.agent_tx.send(json!({
        "type": "agent", "run": run_id, "kind": "run:end", "body": { "ok": result.is_ok() }
    }));
    result.map(|mut v| {
        v["run"] = json!(run_id);
        v
    })
}

/// Turn a verified run into notes for next time.
///
/// Only verified runs: an unverified one did not work, so learning from it would
/// be recording the reason it failed as advice.
async fn learn_from(s: &Arc<AppState>, run_id: i64, goal: &str, outcome: &Value) {
    let host = llm::host_of(outcome["final"]["url"].as_str().unwrap_or(""));
    if host.is_empty() {
        return;
    }
    let transcript = s
        .db
        .run_steps(run_id)
        .unwrap_or_default()
        .iter()
        .map(|st| {
            format!(
                "[{}{}] {}",
                st.kind,
                if st.ok { "" } else { " FAILED" },
                st.detail.replace('\n', " / ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let notes = llm::distil(goal, &host, &transcript, outcome["reason"].as_str().unwrap_or("")).await;
    if notes.is_empty() {
        return;
    }
    for (note, kind) in &notes {
        s.db.add_lesson(&host, note, kind, Some(run_id), now()).ok();
    }
    let _ = s.agent_tx.send(json!({
        "type": "agent", "run": run_id, "kind": "learn",
        "body": { "detail": format!("learned {} note(s) about {host}", notes.len()) }
    }));
}

async fn chat_history(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(s.db.chat_history(200).map_err(bad)?)))
}

async fn chat_clear(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    s.db.clear_chat().map_err(bad)?;
    Ok(Json(json!({ "ok": true })))
}

async fn act_runs(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(s.db.recent_runs(50).map_err(bad)?)))
}

async fn act_run(
    State(s): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let run =
        s.db.recent_runs(500)
            .map_err(bad)?
            .into_iter()
            .find(|r| r.id == id)
            .ok_or_else(|| bad("no such run"))?;
    Ok(Json(
        json!({ "run": run, "steps": s.db.run_steps(id).map_err(bad)? }),
    ))
}

async fn get_settings(State(s): State<Arc<AppState>>) -> Json<Value> {
    Json(json!({
        "max_plans": s.db.max_plans(),
        "hard_max_plans": crate::db::HARD_MAX_PLANS,
        "default_max_plans": crate::db::DEFAULT_MAX_PLANS,
        "learning": s.db.learning_enabled(),
        "headful": std::env::var("MB_HEADFUL").ok().as_deref() == Some("1"),
        "accept_language": crate::stealth::accept_language(),
    }))
}

#[derive(Deserialize)]
struct SettingsBody {
    max_plans: Option<usize>,
    learning: Option<bool>,
}
async fn set_settings(
    State(s): State<Arc<AppState>>,
    Json(b): Json<SettingsBody>,
) -> Result<Json<Value>, ApiError> {
    if let Some(n) = b.max_plans {
        let n = n.clamp(1, crate::db::HARD_MAX_PLANS);
        s.db.set_setting("max_plans", &n.to_string()).map_err(bad)?;
    }
    if let Some(on) = b.learning {
        s.db.set_setting("learning", if on { "1" } else { "0" }).map_err(bad)?;
    }
    Ok(Json(json!({ "max_plans": s.db.max_plans(), "learning": s.db.learning_enabled() })))
}

async fn takeover_status(State(s): State<Arc<AppState>>) -> Json<Value> {
    Json(json!({
        "takeover": s.session.in_takeover(),
        "remaining": s.session.takeover_remaining().await,
    }))
}

/// The UI says the person is still there.
///
/// Without this the deadline would cut a slow sign-in short — finding a phone,
/// waiting for an SMS, unlocking a password manager. With it, the deadline only
/// fires when nobody is watching the banner any more.
async fn takeover_ping(State(s): State<Arc<AppState>>) -> Json<Value> {
    let alive = s.session.touch_takeover().await;
    Json(json!({ "takeover": alive, "remaining": s.session.takeover_remaining().await }))
}

/// Give the browser back if a takeover was started and then abandoned.
///
/// A takeover that only ends when someone clicks "done" fails in exactly one
/// direction: close the tab, get distracted, and the agent is locked out until
/// the app is restarted. This is the other direction.
pub fn spawn_takeover_watchdog(state: Arc<AppState>) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(20));
        loop {
            tick.tick().await;
            // Do not fight a run that is in progress.
            let Ok(_guard) = state.agent_lock.try_lock() else { continue };
            match state.session.expire_takeover().await {
                Ok(true) => {
                    let _ = state.agent_tx.send(json!({
                        "type": "agent", "kind": "takeover:end",
                        "body": { "detail": "Takeover timed out — control returned to the app." }
                    }));
                }
                Ok(false) => {}
                Err(e) => {
                    let _ = state.agent_tx.send(json!({
                        "type": "agent", "kind": "takeover:end",
                        "body": { "detail": format!("could not end the takeover: {e}") }
                    }));
                }
            }
        }
    });
}

#[derive(Deserialize)]
struct TakeoverBody {
    on: bool,
    #[serde(default)]
    url: Option<String>,
}
/// Hand the browser to the person, or take it back.
///
/// Holding the agent lock for the duration is the point: a run cannot start
/// while the user is signing in, and the handover cannot happen underneath a
/// run that is already going.
async fn takeover_set(
    State(s): State<Arc<AppState>>,
    Json(b): Json<TakeoverBody>,
) -> Result<Json<Value>, ApiError> {
    let _guard = s.agent_lock.lock().await;
    let v = s.session.set_takeover(b.on, b.url.as_deref()).await.map_err(gateway)?;
    let _ = s.agent_tx.send(json!({
        "type": "agent", "kind": if b.on { "takeover:start" } else { "takeover:end" },
        "body": { "detail": v["note"].as_str().unwrap_or("") }
    }));
    Ok(Json(v))
}

async fn knowledge(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(s.db.all_lessons(200).map_err(bad)?)))
}

#[derive(Deserialize)]
struct ForgetBody {
    id: i64,
}
async fn knowledge_forget(
    State(s): State<Arc<AppState>>,
    Json(b): Json<ForgetBody>,
) -> Result<Json<Value>, ApiError> {
    s.db.forget_lesson(b.id).map_err(bad)?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct ActBody {
    instruction: String,
}
/// The Act panel is the same engine and the same history as Chat — just a way in
/// that skips the "should I act?" decision.
async fn act(
    State(s): State<Arc<AppState>>,
    Json(b): Json<ActBody>,
) -> Result<Json<Value>, ApiError> {
    if b.instruction.trim().is_empty() {
        return Err(bad("instruction is required"));
    }
    s.db.add_chat("user", &b.instruction, None, now()).ok();
    let outcome = run_agent(&s, &b.instruction, "act")
        .await
        .map_err(gateway)?;
    let answer = llm::report(&b.instruction, &outcome)
        .await
        .unwrap_or_default();
    s.db.add_chat("assistant", &answer, outcome["run"].as_i64(), now())
        .ok();
    Ok(Json(json!({
        "run": outcome["run"], "achieved": outcome["achieved"],
        "plans_used": outcome["plans_used"], "reason": outcome["reason"],
        "answer": answer, "final": outcome["final"],
    })))
}

#[derive(Deserialize)]
struct ExtractBody {
    request: String,
    #[serde(default)]
    schema: Option<String>,
}
async fn extract(
    State(s): State<Arc<AppState>>,
    Json(b): Json<ExtractBody>,
) -> Result<Json<Value>, ApiError> {
    let (answer, model) = llm::extract(&s.session, &b.request, b.schema.as_deref())
        .await
        .map_err(gateway)?;
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

/// The live-view socket, which is also a remote keyboard and mouse.
///
/// WebSockets are not subject to the same-origin policy — any page in any
/// browser on this machine can open `ws://127.0.0.1:4360/api/ws` without a
/// preflight and start sending `{"action":"type","text":"…"}`. Chrome's
/// local-network gating does not yet cover WebSocket upgrades, so nothing else
/// is going to stop it.
///
/// RFC 6455 §10.2 says a server not meant to take input from web pages MUST
/// check `Origin`. Chromium reached the same conclusion for its own debugging
/// endpoint after CVE-2023-0704 and settled on the rule used here: a browser
/// always sends `Origin`, a legitimate native client never does — so accept a
/// missing one, accept our own, and refuse everything else.
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(s): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> Response {
    if let Some(origin) = headers.get(axum::http::header::ORIGIN).and_then(|v| v.to_str().ok()) {
        let host = headers
            .get(axum::http::header::HOST)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        if !origin_is_ours(origin, host) {
            return (StatusCode::FORBIDDEN, "cross-origin websocket refused").into_response();
        }
    }
    ws.on_upgrade(move |socket| live_view(socket, s))
}

/// Does this `Origin` name the same server the request was addressed to?
fn origin_is_ours(origin: &str, host: &str) -> bool {
    let authority = origin.split_once("://").map(|(_, a)| a).unwrap_or(origin);
    if !host.is_empty() && authority == host {
        return true;
    }
    // The app is also served through the SenClaw daemon, so accept loopback
    // origins on any port rather than pinning one.
    let h = authority.split(':').next().unwrap_or("");
    matches!(h, "localhost" | "127.0.0.1" | "[::1]" | "::1")
}

/// Stream the page to the UI.
///
/// Frames arrive from the session's screencast pump, which pushes only when the
/// page actually changes. Page metadata — url, title, viewport, any blocking
/// dialog — is refreshed on a slower ticker instead of once per frame: reading
/// the title is a call into the renderer, and doing it ten times a second to
/// re-send a string that changes once a minute is pure waste.
async fn live_view(mut socket: WebSocket, s: Arc<AppState>) {
    let mut frames = s.session.frames();
    let mut agent = s.agent_tx.subscribe();
    let mut ticker = tokio::time::interval(Duration::from_millis(700));
    let mut last_url = String::new();
    let mut meta = json!({ "url": "", "title": "", "w": 1280, "h": 800 });
    loop {
        tokio::select! {
            frame = frames.recv() => {
                match frame {
                    Ok(data) => {
                        let msg = json!({
                            "type": "frame", "data": data,
                            "url": meta["url"], "title": meta["title"],
                            "w": meta["w"], "h": meta["h"],
                        });
                        if socket.send(Message::Text(msg.to_string())).await.is_err() {
                            break;
                        }
                    }
                    // Lagged: the viewer fell behind and older frames were
                    // dropped. That is the desired behaviour — keep going with
                    // the newest.
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(_) => break,
                }
            }
            ev = agent.recv() => {
                match ev {
                    Ok(v) => {
                        if socket.send(Message::Text(v.to_string())).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(_) => {}
                }
            }
            _ = ticker.tick() => {
                let info = s.session.info().await.unwrap_or(json!({}));

                // A dialog suspends the renderer, so no frames arrive at all
                // while one is up. Say so, or the view just looks frozen.
                if let Some(d) = info.get("dialog") {
                    let msg = json!({ "type": "dialog", "dialog": d });
                    if socket.send(Message::Text(msg.to_string())).await.is_err() {
                        break;
                    }
                    continue;
                }

                let url = info["url"].as_str().unwrap_or("").to_string();
                if url != last_url && !url.is_empty() {
                    s.db.add_history(&url, info["title"].as_str().unwrap_or(""), now()).ok();
                    last_url = url.clone();
                }
                // The viewport travels with each frame so the UI can map a click
                // onto page coordinates. It used to be hardcoded to 1280x800 in
                // the front-end, which silently offset every click the moment the
                // window was any other size.
                let (vw, vh) = s.session.viewport().await;
                meta = json!({ "url": url, "title": info["title"], "w": vw, "h": vh });
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
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return;
    };
    let action = v["action"].as_str().unwrap_or("");
    let sess = &s.session;
    match action {
        "navigate" => {
            sess.navigate(v["url"].as_str().unwrap_or("")).await.ok();
        }
        "click" => {
            if let (Some(x), Some(y)) = (v["x"].as_f64(), v["y"].as_f64()) {
                sess.click_xy(x, y).await.ok();
            }
        }
        "scroll" => {
            sess.scroll(
                v["dx"].as_f64().unwrap_or(0.0),
                v["dy"].as_f64().unwrap_or(0.0),
            )
            .await
            .ok();
        }
        "type" => {
            sess.type_text(v["text"].as_str().unwrap_or("")).await.ok();
        }
        "press" => {
            sess.press_key(v["key"].as_str().unwrap_or("")).await.ok();
        }
        "back" => {
            sess.go_back().await.ok();
        }
        "forward" => {
            sess.go_forward().await.ok();
        }
        "reload" => {
            sess.reload().await.ok();
        }
        _ => {}
    }
}


#[cfg(test)]
mod origin_tests {
    use super::origin_is_ours;

    /// The socket relays keystrokes into the browser, so an unknown page must
    /// not be able to open it.
    #[test]
    fn a_web_page_from_elsewhere_is_refused() {
        assert!(!origin_is_ours("https://evil.example.com", "127.0.0.1:4360"));
        assert!(!origin_is_ours("http://attacker.test:8080", "127.0.0.1:4360"));
    }

    #[test]
    fn our_own_page_is_allowed() {
        assert!(origin_is_ours("http://127.0.0.1:4360", "127.0.0.1:4360"));
        assert!(origin_is_ours("http://localhost:4360", "127.0.0.1:4360"));
        assert!(origin_is_ours("http://127.0.0.1:18788", "127.0.0.1:4360"));
    }
}
