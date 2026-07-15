//! HTTP API for the Moltbook app. Local state (settings, drafts, activity,
//! feed cache) lives in [`crate::db`]; live reads/writes go to Moltbook through
//! [`crate::moltbook`]. The MCP server ([`crate::mcp`]) reuses this same
//! [`AppState`] so the agent's view and the UI never drift.

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
use std::time::{SystemTime, UNIX_EPOCH};

use crate::db::{default_data_dir, Db, DraftCreate};
use crate::engine;
use crate::llm;
use crate::moltbook::{Moltbook, DEFAULT_BASE};

pub struct AppState {
    pub db: Db,
    /// Broadcasts raw JSON-RPC responses to connected MCP SSE clients.
    pub mcp_tx: tokio::sync::broadcast::Sender<String>,
}

pub fn make_state() -> Arc<AppState> {
    let data_dir = default_data_dir("moltbook");
    let db = Db::open(&data_dir.join("moltbook.db")).expect("failed to open Moltbook database");
    // Seed the demo feed once so the UI is demonstrable before a live key is
    // connected. Demo rows are flagged and never mix with live data.
    if db.list_cached(1).map(|c| c.is_empty()).unwrap_or(true) {
        let _ = db.seed_demo(now_ts());
    }
    let (mcp_tx, _) = tokio::sync::broadcast::channel(100);
    Arc::new(AppState { db, mcp_tx })
}

pub fn now_ts() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Build a Moltbook client from stored settings. The API key is only ever
/// attached to requests aimed at the stored base URL (defaults to www.moltbook.com).
pub fn client(db: &Db) -> Moltbook {
    let base = db.get_str("base_url", DEFAULT_BASE);
    let key = db.get_str("api_key", "");
    Moltbook::new(Some(&base), if key.is_empty() { None } else { Some(&key) })
}

/// The persona voice injected into planner/composer prompts.
pub fn voice(db: &Db) -> String {
    let v = db.get_str("persona_voice", "");
    if v.trim().is_empty() { default_voice() } else { v }
}

fn default_voice() -> String {
    "Bạn là một AI agent tò mò, điềm đạm và chân thành trên Moltbook. Bạn tham gia có chọn lọc: \
chỉ upvote khi thật sự thấy hay, chỉ bình luận khi có điều đáng nói, và không spam. Bạn thích \
những chủ đề về bản chất của agent, kỹ thuật thực chiến, và xây-dựng-công-khai. Giọng văn ngắn \
gọn, thật, không nịnh."
        .to_string()
}

// ---- error plumbing ----

pub struct ApiError(pub StatusCode, pub String);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}
fn bad(e: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, e.to_string())
}
fn not_found(e: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::NOT_FOUND, e.to_string())
}
fn server(e: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}
fn upstream(e: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::BAD_GATEWAY, e.to_string())
}

pub fn api_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/status", get(status))
        .route("/account", get(get_account))
        .route("/account/register", post(register))
        .route("/account/claim-info", post(claim_info))
        .route("/account/connect", post(connect))
        .route("/account/refresh", post(refresh))
        .route("/account/disconnect", post(disconnect))
        .route("/settings", get(get_settings).put(put_settings))
        .route("/feed", get(get_feed))
        .route("/home", get(get_home))
        .route("/posts/:id", get(get_post))
        .route("/search", get(get_search))
        .route("/submolts", get(get_submolts))
        .route("/notifications", get(get_notifications))
        .route("/notifications/read-all", post(read_all_notifications))
        .route("/activity", get(get_activity))
        .route("/drafts", get(list_drafts).post(create_draft))
        .route("/drafts/count", get(drafts_count))
        .route("/drafts/compose", post(compose_reply_draft))
        .route("/drafts/compose-post", post(compose_post_draft))
        .route("/drafts/:id/approve", post(approve_draft))
        .route("/drafts/:id/reject", post(reject_draft))
        .route("/drafts/:id", delete(delete_draft))
        .route("/actions/vote", post(action_vote))
        .route("/actions/comment", post(action_comment))
        .route("/actions/post", post(action_post))
        .route("/actions/follow", post(action_follow))
        .route("/actions/subscribe", post(action_subscribe))
        .route("/actions/submolt", post(action_submolt))
        .route("/engine/run", post(engine_run))
        .route("/demo/seed", post(demo_seed))
        .route("/models", get(get_models).post(set_model))
        .route("/mcp/sse", get(crate::mcp::mcp_sse).post(crate::mcp::mcp_message))
        .route("/mcp/message", post(crate::mcp::mcp_message))
        .with_state(state)
}

async fn status() -> Json<Value> {
    Json(json!({ "ok": true, "app": "moltbook" }))
}

// ---- account ----

pub fn account_summary(db: &Db) -> Value {
    json!({
        "connected": db.connected(),
        "base_url": db.get_str("base_url", DEFAULT_BASE),
        "agent_name": db.get_str("agent_name", ""),
        "claim_url": db.get_str("claim_url", ""),
        "verification_code": db.get_str("verification_code", ""),
        "claimed": db.get_bool("claimed", false),
        "autonomy": db.autonomy(),
        "heartbeat_enabled": db.get_bool("heartbeat_enabled", false),
        "heartbeat_minutes": db.get_i64("heartbeat_minutes", 60),
        "engage_limit": db.get_i64("engage_limit", 2),
        "default_submolt": db.get_str("default_submolt", "general"),
        "persona": db.get_str("persona", "molty"),
        "persona_voice": db.get_str("persona_voice", ""),
        "last_heartbeat_at": db.get_i64("last_heartbeat_at", 0),
        "last_post_at": db.get_i64("last_post_at", 0),
        "profile": db.get_json("profile"),
        "pending_drafts": db.count_pending_drafts().unwrap_or(0),
    })
}

async fn get_account(State(s): State<Arc<AppState>>) -> Json<Value> {
    Json(account_summary(&s.db))
}

#[derive(Deserialize)]
struct RegisterBody {
    name: String,
    #[serde(default)]
    description: String,
}

async fn register(
    State(s): State<Arc<AppState>>,
    Json(b): Json<RegisterBody>,
) -> Result<Json<Value>, ApiError> {
    if b.name.trim().is_empty() {
        return Err(bad("name là bắt buộc"));
    }
    let base = s.db.get_str("base_url", DEFAULT_BASE);
    let mb = Moltbook::new(Some(&base), None);
    let v = mb.register(b.name.trim(), b.description.trim()).await.map_err(upstream)?;
    let now = now_ts();
    let (api_key, claim_url, vcode) = crate::moltbook::extract_register_fields(&v);
    if !api_key.is_empty() {
        s.db.set_str("api_key", &api_key).ok();
    }
    s.db.set_str("agent_name", b.name.trim()).ok();
    s.db.set_str("claim_url", &claim_url).ok();
    s.db.set_str("verification_code", &vcode).ok();
    s.db.set_bool("claimed", false).ok();
    // Persist the raw response so the claim link is never lost, even if Moltbook
    // changes its field names.
    s.db.set_json("last_register_response", &v).ok();
    s.db.log("register", &format!("đăng ký agent '{}' trên Moltbook", b.name.trim()), "", now).ok();
    Ok(Json(json!({
        "ok": true,
        "claim_url": claim_url,
        "verification_code": vcode,
        "raw": v,
        "note": "Mở link claim và xác nhận bằng tài khoản X để kích hoạt agent. API key đã lưu cục bộ. Nếu không thấy link, xem phản hồi thô bên dưới hoặc bấm 'Lấy lại link claim'.",
        "account": account_summary(&s.db),
    })))
}

/// Recover the claim/verification link for an already-registered agent by
/// asking Moltbook's status + profile endpoints and scanning them for a
/// claim URL — so the user never has to create a second agent just to get the
/// link back.
async fn claim_info(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    let mb = client(&s.db);
    if !mb.is_authenticated() {
        return Err(bad("chưa có API key — đăng ký hoặc kết nối trước"));
    }
    let status = mb.account_status().await.ok();
    let me = mb.me().await.ok();
    let stored = s.db.get_json("last_register_response");
    // Prefer a fresh claim URL from status/me; fall back to the stored register response.
    let claim_url = status
        .as_ref()
        .and_then(crate::moltbook::find_claim_url)
        .or_else(|| me.as_ref().and_then(crate::moltbook::find_claim_url))
        .or_else(|| stored.as_ref().and_then(crate::moltbook::find_claim_url))
        .unwrap_or_else(|| s.db.get_str("claim_url", ""));
    if !claim_url.is_empty() {
        s.db.set_str("claim_url", &claim_url).ok();
    }
    // Reflect the current claim state if the status endpoint reports it.
    let claimed = status
        .as_ref()
        .and_then(|v| {
            v.get("claimed")
                .and_then(|x| x.as_bool())
                .or_else(|| v.get("status").and_then(|x| x.as_str()).map(|s| s == "claimed" || s == "verified"))
        })
        .unwrap_or_else(|| s.db.get_bool("claimed", false));
    s.db.set_bool("claimed", claimed).ok();
    Ok(Json(json!({
        "ok": true,
        "claim_url": claim_url,
        "claimed": claimed,
        "status": status,
        "me": me,
        "last_register_response": stored,
    })))
}

#[derive(Deserialize)]
struct ConnectBody {
    api_key: String,
    #[serde(default)]
    base_url: Option<String>,
}

async fn connect(
    State(s): State<Arc<AppState>>,
    Json(b): Json<ConnectBody>,
) -> Result<Json<Value>, ApiError> {
    let key = b.api_key.trim();
    if key.is_empty() {
        return Err(bad("api_key là bắt buộc"));
    }
    if let Some(base) = b.base_url.as_deref().map(str::trim).filter(|b| !b.is_empty()) {
        s.db.set_str("base_url", base).ok();
    }
    s.db.set_str("api_key", key).ok();
    // Verify by fetching /me and cache the profile.
    let mb = client(&s.db);
    match mb.me().await {
        Ok(me) => {
            let name = me.get("name").or_else(|| me.get("agent").and_then(|a| a.get("name"))).and_then(|x| x.as_str()).unwrap_or("");
            if !name.is_empty() {
                s.db.set_str("agent_name", name).ok();
            }
            s.db.set_json("profile", &me).ok();
            s.db.set_bool("claimed", true).ok();
            s.db.log("connect", "kết nối agent Moltbook thành công", "", now_ts()).ok();
            Ok(Json(json!({ "ok": true, "profile": me, "account": account_summary(&s.db) })))
        }
        Err(e) => {
            // Key stored but not verified — keep it so the user can retry; report why.
            s.db.log("error", &format!("kết nối thất bại: {e}"), "", now_ts()).ok();
            Err(upstream(e))
        }
    }
}

async fn refresh(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    let mb = client(&s.db);
    if !mb.is_authenticated() {
        return Err(bad("chưa kết nối agent"));
    }
    let me = mb.me().await.map_err(upstream)?;
    s.db.set_json("profile", &me).ok();
    Ok(Json(json!({ "ok": true, "profile": me, "account": account_summary(&s.db) })))
}

async fn disconnect(State(s): State<Arc<AppState>>) -> Json<Value> {
    for k in ["api_key", "profile"] {
        s.db.set_json(k, &Value::Null).ok();
    }
    s.db.set_bool("claimed", false).ok();
    s.db.log("disconnect", "ngắt kết nối agent (xoá API key khỏi máy)", "", now_ts()).ok();
    Json(account_summary(&s.db))
}

// ---- settings ----

async fn get_settings(State(s): State<Arc<AppState>>) -> Json<Value> {
    Json(account_summary(&s.db))
}

#[derive(Deserialize)]
struct SettingsPatch {
    autonomy: Option<String>,
    heartbeat_enabled: Option<bool>,
    heartbeat_minutes: Option<i64>,
    engage_limit: Option<i64>,
    default_submolt: Option<String>,
    persona: Option<String>,
    persona_voice: Option<String>,
    base_url: Option<String>,
}

async fn put_settings(
    State(s): State<Arc<AppState>>,
    Json(p): Json<SettingsPatch>,
) -> Result<Json<Value>, ApiError> {
    if let Some(a) = p.autonomy {
        let a = a.trim();
        if !matches!(a, "observe" | "draft" | "live") {
            return Err(bad("autonomy phải là observe | draft | live"));
        }
        s.db.set_str("autonomy", a).ok();
    }
    if let Some(v) = p.heartbeat_enabled {
        s.db.set_bool("heartbeat_enabled", v).ok();
    }
    if let Some(v) = p.heartbeat_minutes {
        s.db.set_i64("heartbeat_minutes", v.max(5)).ok();
    }
    if let Some(v) = p.engage_limit {
        s.db.set_i64("engage_limit", v.clamp(0, 10)).ok();
    }
    if let Some(v) = p.default_submolt {
        s.db.set_str("default_submolt", v.trim().trim_start_matches("m/")).ok();
    }
    if let Some(v) = p.persona {
        s.db.set_str("persona", v.trim()).ok();
    }
    if let Some(v) = p.persona_voice {
        s.db.set_str("persona_voice", v.trim()).ok();
    }
    if let Some(v) = p.base_url {
        let v = v.trim();
        if !v.is_empty() {
            s.db.set_str("base_url", v).ok();
        }
    }
    Ok(Json(account_summary(&s.db)))
}

// ---- feed / reads ----

#[derive(Deserialize)]
struct FeedQuery {
    #[serde(default)]
    sort: Option<String>,
    #[serde(default)]
    filter: Option<String>,
    #[serde(default)]
    refresh: Option<bool>,
    #[serde(default)]
    limit: Option<i64>,
}

async fn get_feed(
    State(s): State<Arc<AppState>>,
    Query(q): Query<FeedQuery>,
) -> Json<Value> {
    let db = &s.db;
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let connected = db.connected();
    let mut source = if connected { "cache" } else { "demo" };
    let mut warning = Value::Null;

    if connected && q.refresh.unwrap_or(true) {
        let mb = client(db);
        let sort = q.sort.as_deref().unwrap_or("hot");
        let filter = q.filter.as_deref().unwrap_or("all");
        match mb.feed(sort, filter, None).await {
            Ok(v) => {
                let items = engine::extract_posts(&v);
                if !items.is_empty() {
                    db.clear_live_cache().ok();
                    let now = now_ts();
                    let rows: Vec<_> = items
                        .iter()
                        .map(|f| crate::db::CachedPost {
                            post_id: f.id.clone(),
                            submolt: f.submolt.clone(),
                            author: f.author.clone(),
                            title: f.title.clone(),
                            content: f.content.clone(),
                            url: String::new(),
                            score: f.score,
                            comment_count: 0,
                            posted_at: now,
                            cached_at: now,
                            demo: false,
                        })
                        .collect();
                    db.upsert_posts(&rows).ok();
                }
                source = "live";
            }
            Err(e) => {
                warning = json!(e.to_string());
            }
        }
    }

    let posts = db.list_cached(limit).unwrap_or_default();
    Json(json!({ "posts": posts, "source": source, "connected": connected, "warning": warning, "count": posts.len() }))
}

async fn get_home(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    let mb = client(&s.db);
    if !mb.is_authenticated() {
        return Err(bad("chưa kết nối agent"));
    }
    mb.home().await.map(Json).map_err(upstream)
}

async fn get_post(
    State(s): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let mb = client(&s.db);
    if !mb.is_authenticated() {
        return Err(bad("chưa kết nối agent"));
    }
    let post = mb.get_post(&id).await.map_err(upstream)?;
    let comments = mb.comments(&id, "best", None).await.unwrap_or(json!({ "comments": [] }));
    Ok(Json(json!({ "post": post, "comments": comments })))
}

#[derive(Deserialize)]
struct SearchQuery {
    #[serde(default)]
    q: String,
    #[serde(default)]
    r#type: Option<String>,
    #[serde(default)]
    limit: Option<i64>,
}

async fn get_search(
    State(s): State<Arc<AppState>>,
    Query(q): Query<SearchQuery>,
) -> Result<Json<Value>, ApiError> {
    if q.q.trim().is_empty() {
        return Err(bad("q là bắt buộc"));
    }
    let mb = client(&s.db);
    if !mb.is_authenticated() {
        return Err(bad("chưa kết nối agent"));
    }
    mb.search(q.q.trim(), q.r#type.as_deref().unwrap_or("all"), q.limit.unwrap_or(20))
        .await
        .map(Json)
        .map_err(upstream)
}

async fn get_submolts(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    let mb = client(&s.db);
    if !mb.is_authenticated() {
        return Err(bad("chưa kết nối agent"));
    }
    mb.submolts(None).await.map(Json).map_err(upstream)
}

async fn get_notifications(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    let mb = client(&s.db);
    if !mb.is_authenticated() {
        return Err(bad("chưa kết nối agent"));
    }
    mb.notifications().await.map(Json).map_err(upstream)
}

async fn read_all_notifications(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    let mb = client(&s.db);
    if !mb.is_authenticated() {
        return Err(bad("chưa kết nối agent"));
    }
    mb.read_all_notifications().await.map(Json).map_err(upstream)
}

#[derive(Deserialize)]
struct ActivityQuery {
    #[serde(default)]
    limit: Option<i64>,
}

async fn get_activity(
    State(s): State<Arc<AppState>>,
    Query(q): Query<ActivityQuery>,
) -> Result<Json<Value>, ApiError> {
    let items = s.db.list_activity(q.limit.unwrap_or(100)).map_err(server)?;
    Ok(Json(json!({ "items": items })))
}

// ---- drafts ----

#[derive(Deserialize)]
struct DraftsQuery {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    limit: Option<i64>,
}

async fn list_drafts(
    State(s): State<Arc<AppState>>,
    Query(q): Query<DraftsQuery>,
) -> Result<Json<Value>, ApiError> {
    let drafts = s.db.list_drafts(q.status.as_deref(), q.limit.unwrap_or(200)).map_err(server)?;
    Ok(Json(json!({ "drafts": drafts, "count": drafts.len() })))
}

async fn drafts_count(State(s): State<Arc<AppState>>) -> Json<Value> {
    Json(json!({ "pending": s.db.count_pending_drafts().unwrap_or(0) }))
}

#[derive(Deserialize)]
struct CreateDraftBody {
    kind: String,
    #[serde(default)]
    submolt: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    target_post_id: String,
    #[serde(default)]
    target_title: String,
    #[serde(default)]
    parent_id: String,
    #[serde(default)]
    vote_dir: String,
    #[serde(default)]
    target_name: String,
    #[serde(default)]
    reason: String,
}

async fn create_draft(
    State(s): State<Arc<AppState>>,
    Json(b): Json<CreateDraftBody>,
) -> Result<Json<Value>, ApiError> {
    let dc = DraftCreate {
        kind: b.kind,
        submolt: b.submolt.trim_start_matches("m/").to_string(),
        title: b.title,
        content: b.content,
        url: b.url,
        target_post_id: b.target_post_id,
        target_title: b.target_title,
        parent_id: b.parent_id,
        vote_dir: b.vote_dir,
        target_name: b.target_name,
        reason: b.reason,
        source: "user".into(),
        model: String::new(),
    };
    if dc.kind.trim().is_empty() {
        return Err(bad("kind là bắt buộc"));
    }
    let id = s.db.create_draft(&dc, now_ts()).map_err(bad)?;
    let d = s.db.get_draft(id).map_err(server)?;
    Ok(Json(json!({ "ok": true, "draft": d })))
}

#[derive(Deserialize)]
struct ComposeReplyBody {
    target_post_id: String,
    #[serde(default)]
    post_title: String,
    #[serde(default)]
    post_content: String,
    #[serde(default)]
    instruction: String,
}

/// LLM-draft a reply to a post and queue it for approval.
async fn compose_reply_draft(
    State(s): State<Arc<AppState>>,
    Json(b): Json<ComposeReplyBody>,
) -> Result<Json<Value>, ApiError> {
    if b.target_post_id.trim().is_empty() {
        return Err(bad("target_post_id là bắt buộc"));
    }
    let voice = voice(&s.db);
    // Pull post text from the cache when the caller didn't supply it.
    let (title, content) = if b.post_title.is_empty() && b.post_content.is_empty() {
        s.db.list_cached(500)
            .unwrap_or_default()
            .into_iter()
            .find(|p| p.post_id == b.target_post_id)
            .map(|p| (p.title, p.content))
            .unwrap_or((b.post_title.clone(), b.post_content.clone()))
    } else {
        (b.post_title.clone(), b.post_content.clone())
    };
    let (text, model) = llm::compose_reply(&voice, &title, &content, &b.instruction).await.map_err(upstream)?;
    let dc = DraftCreate {
        kind: "comment".into(),
        target_post_id: b.target_post_id.clone(),
        target_title: title,
        content: text,
        reason: if b.instruction.is_empty() { "soạn trả lời".into() } else { b.instruction.clone() },
        source: "user".into(),
        model,
        ..Default::default()
    };
    let id = s.db.create_draft(&dc, now_ts()).map_err(bad)?;
    let d = s.db.get_draft(id).map_err(server)?;
    Ok(Json(json!({ "ok": true, "draft": d })))
}

#[derive(Deserialize)]
struct ComposePostBody {
    #[serde(default)]
    submolt: String,
    #[serde(default)]
    topic: String,
}

async fn compose_post_draft(
    State(s): State<Arc<AppState>>,
    Json(b): Json<ComposePostBody>,
) -> Result<Json<Value>, ApiError> {
    let voice = voice(&s.db);
    let submolt = if b.submolt.trim().is_empty() { s.db.get_str("default_submolt", "general") } else { b.submolt.trim().trim_start_matches("m/").to_string() };
    let (post, model) = llm::compose_post(&voice, &submolt, &b.topic).await.map_err(upstream)?;
    let dc = DraftCreate {
        kind: "post".into(),
        submolt,
        title: post.title,
        content: post.content,
        reason: if b.topic.is_empty() { "soạn bài mới".into() } else { b.topic.clone() },
        source: "user".into(),
        model,
        ..Default::default()
    };
    let id = s.db.create_draft(&dc, now_ts()).map_err(bad)?;
    let d = s.db.get_draft(id).map_err(server)?;
    Ok(Json(json!({ "ok": true, "draft": d })))
}

/// Approve = the publish gate. Executes the queued draft against Moltbook.
async fn approve_draft(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let draft = s.db.get_draft(id).map_err(server)?.ok_or_else(|| not_found("draft không tồn tại"))?;
    if draft.status != "pending" {
        return Err(bad(format!("draft đã ở trạng thái '{}'", draft.status)));
    }
    match engine::execute_draft(&s, &draft).await {
        Ok(reference) => {
            let now = now_ts();
            s.db.set_draft_result(id, "posted", &reference, "", now).ok();
            s.db.log(&draft.kind, &format!("duyệt & đăng {} (#{id})", draft.kind), &reference, now).ok();
            let d = s.db.get_draft(id).map_err(server)?;
            Ok(Json(json!({ "ok": true, "draft": d, "ref": reference })))
        }
        Err(e) => {
            s.db.set_draft_result(id, "error", "", &e, now_ts()).ok();
            let d = s.db.get_draft(id).map_err(server)?;
            Ok(Json(json!({ "ok": false, "error": e, "draft": d })))
        }
    }
}

async fn reject_draft(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let draft = s.db.get_draft(id).map_err(server)?.ok_or_else(|| not_found("draft không tồn tại"))?;
    s.db.set_draft_result(id, "rejected", "", "", now_ts()).map_err(server)?;
    s.db.log("reject", &format!("từ chối nháp {} (#{id})", draft.kind), "", now_ts()).ok();
    let d = s.db.get_draft(id).map_err(server)?;
    Ok(Json(json!({ "ok": true, "draft": d })))
}

async fn delete_draft(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    s.db.delete_draft(id).map_err(server)?;
    Ok(Json(json!({ "ok": true, "id": id })))
}

// ---- direct actions (honour the autonomy gate) ----

/// Queue a draft (observe → refuse, draft → queue, live → publish now). The one
/// place the autonomy setting is enforced for direct user/agent actions.
pub async fn enqueue_or_publish(state: &Arc<AppState>, dc: DraftCreate) -> Value {
    let db = &state.db;
    let autonomy = db.autonomy();
    if autonomy == "observe" {
        return json!({ "ok": false, "gated": "observe", "message": "Đang ở chế độ chỉ quan sát — bật 'draft' hoặc 'live' ở Cài đặt để tham gia." });
    }
    let now = now_ts();
    let id = match db.create_draft(&dc, now) {
        Ok(id) => id,
        Err(e) => return json!({ "ok": false, "error": e.to_string() }),
    };
    if autonomy == "live" {
        if let Ok(Some(draft)) = db.get_draft(id) {
            return match engine::execute_draft(state, &draft).await {
                Ok(reference) => {
                    db.set_draft_result(id, "posted", &reference, "", now_ts()).ok();
                    db.log(&draft.kind, &format!("live: {} (#{id})", draft.kind), &reference, now_ts()).ok();
                    json!({ "ok": true, "published": true, "ref": reference, "draft_id": id })
                }
                Err(e) => {
                    db.set_draft_result(id, "error", "", &e, now_ts()).ok();
                    json!({ "ok": false, "published": false, "error": e, "draft_id": id })
                }
            };
        }
    }
    let d = db.get_draft(id).ok().flatten();
    json!({ "ok": true, "queued": true, "draft_id": id, "draft": d, "message": "Đã đưa vào hàng chờ duyệt." })
}

#[derive(Deserialize)]
struct VoteBody {
    post_id: String,
    #[serde(default)]
    dir: Option<String>,
    #[serde(default)]
    title: Option<String>,
}
async fn action_vote(State(s): State<Arc<AppState>>, Json(b): Json<VoteBody>) -> Json<Value> {
    let dc = DraftCreate {
        kind: "vote".into(),
        vote_dir: b.dir.unwrap_or_else(|| "up".into()),
        target_post_id: b.post_id,
        target_title: b.title.unwrap_or_default(),
        source: "user".into(),
        ..Default::default()
    };
    Json(enqueue_or_publish(&s, dc).await)
}

#[derive(Deserialize)]
struct CommentBody {
    post_id: String,
    content: String,
    #[serde(default)]
    parent_id: String,
    #[serde(default)]
    title: String,
}
async fn action_comment(State(s): State<Arc<AppState>>, Json(b): Json<CommentBody>) -> Json<Value> {
    let dc = DraftCreate {
        kind: "comment".into(),
        target_post_id: b.post_id,
        target_title: b.title,
        content: b.content,
        parent_id: b.parent_id,
        source: "user".into(),
        ..Default::default()
    };
    Json(enqueue_or_publish(&s, dc).await)
}

#[derive(Deserialize)]
struct PostBody {
    #[serde(default)]
    submolt: String,
    title: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    url: String,
}
async fn action_post(State(s): State<Arc<AppState>>, Json(b): Json<PostBody>) -> Json<Value> {
    let submolt = if b.submolt.trim().is_empty() { s.db.get_str("default_submolt", "general") } else { b.submolt.trim().trim_start_matches("m/").to_string() };
    let dc = DraftCreate {
        kind: "post".into(),
        submolt,
        title: b.title,
        content: b.content,
        url: b.url,
        source: "user".into(),
        ..Default::default()
    };
    Json(enqueue_or_publish(&s, dc).await)
}

#[derive(Deserialize)]
struct NameBody {
    name: String,
}
async fn action_follow(State(s): State<Arc<AppState>>, Json(b): Json<NameBody>) -> Json<Value> {
    let dc = DraftCreate { kind: "follow".into(), target_name: b.name, source: "user".into(), ..Default::default() };
    Json(enqueue_or_publish(&s, dc).await)
}
async fn action_subscribe(State(s): State<Arc<AppState>>, Json(b): Json<NameBody>) -> Json<Value> {
    let dc = DraftCreate {
        kind: "subscribe".into(),
        target_name: b.name.trim_start_matches("m/").to_string(),
        source: "user".into(),
        ..Default::default()
    };
    Json(enqueue_or_publish(&s, dc).await)
}

#[derive(Deserialize)]
struct SubmoltBody {
    name: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    description: String,
}
async fn action_submolt(State(s): State<Arc<AppState>>, Json(b): Json<SubmoltBody>) -> Json<Value> {
    let dc = DraftCreate {
        kind: "submolt".into(),
        submolt: b.name.trim_start_matches("m/").to_string(),
        title: b.display_name,
        content: b.description,
        source: "user".into(),
        ..Default::default()
    };
    Json(enqueue_or_publish(&s, dc).await)
}

// ---- engine ----

async fn engine_run(State(s): State<Arc<AppState>>) -> Json<Value> {
    Json(engine::run_once(&s, "manual").await)
}

// ---- demo ----

async fn demo_seed(State(s): State<Arc<AppState>>) -> Json<Value> {
    let n = s.db.seed_demo(now_ts()).unwrap_or(0);
    Json(json!({ "ok": true, "seeded": n }))
}

// ---- models ----

async fn get_models() -> Result<Json<Value>, ApiError> {
    llm::list_models().await.map(Json).map_err(upstream)
}

#[derive(Deserialize)]
struct SetModelBody {
    id: String,
}
async fn set_model(Json(b): Json<SetModelBody>) -> Result<Json<Value>, ApiError> {
    llm::set_active_model(&b.id).await.map_err(upstream)?;
    Ok(Json(json!({ "ok": true, "id": b.id })))
}
