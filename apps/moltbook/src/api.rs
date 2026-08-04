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
    // Seed the default research workflows exactly once (a flag, not a row
    // count, so a user who deletes them all doesn't get them back on restart).
    if !db.get_bool("workflows_seeded", false) {
        let now = now_ts();
        for (name, flow, steps, extract) in crate::research::default_workflows() {
            let _ = db.add_workflow(name, flow, &steps.to_string(), extract, true, now);
        }
        db.set_bool("workflows_seeded", true).ok();
    }
    // Seed the process-wide LLM profile from stored settings.
    llm::set_profile(&db.get_str("llm_profile", ""));
    let (mcp_tx, _) = tokio::sync::broadcast::channel(100);
    Arc::new(AppState { db, mcp_tx })
}

pub fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Build a Moltbook client from stored settings. The API key is only ever
/// attached to requests aimed at the stored base URL (defaults to www.moltbook.com).
pub fn client(db: &Db) -> Moltbook {
    let base = db.get_str("base_url", DEFAULT_BASE);
    let key = db.get_str("api_key", "");
    Moltbook::new(Some(&base), if key.is_empty() { None } else { Some(&key) })
}

/// The molty's knowledge space — its **trí nhớ**. Defaults to the app id, which
/// is also what the daemon falls back to, so memory works with zero config.
pub fn memory_space(db: &Db) -> String {
    let s = db.get_str("knowledge_space", "");
    if s.trim().is_empty() {
        crate::senclaw::DEFAULT_SPACE.to_string()
    } else {
        s
    }
}

/// Build the molty's grounding for a topic: recall its own memory (trí nhớ) and
/// pull relevant wiki docs (kho thông tin). Each half is independently
/// toggleable and best-effort — a missing daemon just yields empty grounding.
pub async fn grounding_for(db: &Db, topic: &str) -> llm::Grounding {
    let mut g = llm::Grounding::default();
    if topic.trim().is_empty() {
        return g;
    }
    if db.get_bool("memory_enabled", true) {
        g.memory = crate::senclaw::knowledge_recall(&memory_space(db), topic)
            .await
            .unwrap_or_default();
    }
    if db.get_bool("wiki_enabled", true) {
        g.wiki = crate::senclaw::wiki_context(topic, 2000).await;
    }
    g
}

/// The persona voice injected into planner/composer prompts.
pub fn voice(db: &Db) -> String {
    let v = db.get_str("persona_voice", "");
    if v.trim().is_empty() {
        default_voice()
    } else {
        v
    }
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
        .route("/drafts/:id/verify", post(verify_draft))
        .route("/drafts/:id/reject", post(reject_draft))
        .route("/drafts/:id", delete(delete_draft))
        .route("/actions/vote", post(action_vote))
        .route("/actions/comment", post(action_comment))
        .route("/actions/post", post(action_post))
        .route("/actions/follow", post(action_follow))
        .route("/actions/subscribe", post(action_subscribe))
        .route("/actions/submolt", post(action_submolt))
        .route("/trending", get(list_digests).post(run_trending))
        .route("/tracked", get(list_tracked).post(track_post))
        .route("/tracked/:post_id", delete(untrack_post))
        .route("/harvest", post(harvest))
        .route("/topics", get(list_topics).post(add_topic))
        .route(
            "/topics/:id",
            axum::routing::patch(patch_topic).delete(delete_topic),
        )
        .route("/engine/run", post(engine_run))
        .route(
            "/research/workflows",
            get(list_workflows_h).post(create_workflow_h),
        )
        .route(
            "/research/workflows/:id",
            axum::routing::patch(patch_workflow_h).delete(delete_workflow_h),
        )
        .route("/research/tools", get(research_tools_h))
        .route("/research/run", post(research_run_h))
        .route("/research/ai-build", post(research_ai_build_h))
        .route("/drafts/:id/answer", post(answer_draft_h))
        .route("/integrations", get(integrations))
        .route("/memory/recall", post(memory_recall))
        .route("/memory/save", post(memory_save))
        .route("/wiki/archive", post(wiki_archive))
        .route("/demo/seed", post(demo_seed))
        .route("/models", get(get_models))
        .route(
            "/mcp/sse",
            get(crate::mcp::mcp_sse).post(crate::mcp::mcp_message),
        )
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
        // SenClaw integrations: knowledge = trí nhớ, wiki = kho thông tin.
        "memory_enabled": db.get_bool("memory_enabled", true),
        "wiki_enabled": db.get_bool("wiki_enabled", true),
        "wiki_archive": db.get_bool("wiki_archive", false),
        "knowledge_space": memory_space(db),
        // Which LLM profile this app composes with ("" = theo model active của daemon).
        "llm_profile": db.get_str("llm_profile", ""),
        // "all" = tương tác toàn bộ feed; "focus" = chỉ các chủ đề trong danh sách.
        "topic_mode": db.topic_mode(),
        // Mỗi heartbeat có tự thu thập phản hồi & cập nhật doc wiki không.
        "harvest_enabled": db.get_bool("harvest_enabled", true),
        // Mỗi ngày tự tổng hợp xu hướng agent internet vào wiki (mặc định tắt).
        "trending_daily": db.get_bool("trending_daily", false),
        "last_heartbeat_at": db.get_i64("last_heartbeat_at", 0),
        "last_post_at": db.get_i64("last_post_at", 0),
        "profile": db.get_json("profile"),
        "pending_drafts": db.count_pending_drafts().unwrap_or(0),
        // Nghiên cứu trước khi soạn: chạy các workflow MCP → tổng hợp → nếu
        // chưa chắc chắn thì hỏi lại người dùng (draft 'needs_input').
        "research_enabled": db.get_bool("research_enabled", true),
        "research_on_compose": db.get_bool("research_on_compose", true),
        "research_ask_threshold": db.get_i64("research_ask_threshold", 60),
        "research_extract_prompt": db.get_str("research_extract_prompt", ""),
        "research_max_per_tick": db.get_i64("research_max_per_tick", 3),
        "needs_input_drafts": db.count_drafts_with_status("needs_input").unwrap_or(0),
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
    let v = mb
        .register(b.name.trim(), b.description.trim())
        .await
        .map_err(upstream)?;
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
    s.db.log(
        "register",
        &format!("đăng ký agent '{}' trên Moltbook", b.name.trim()),
        "",
        now,
    )
    .ok();
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
            v.get("claimed").and_then(|x| x.as_bool()).or_else(|| {
                v.get("status")
                    .and_then(|x| x.as_str())
                    .map(|s| s == "claimed" || s == "verified")
            })
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
    if let Some(base) = b
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|b| !b.is_empty())
    {
        s.db.set_str("base_url", base).ok();
    }
    s.db.set_str("api_key", key).ok();
    // Verify by fetching /me and cache the profile.
    let mb = client(&s.db);
    match mb.me().await {
        Ok(me) => {
            let name = me
                .get("name")
                .or_else(|| me.get("agent").and_then(|a| a.get("name")))
                .and_then(|x| x.as_str())
                .unwrap_or("");
            if !name.is_empty() {
                s.db.set_str("agent_name", name).ok();
            }
            s.db.set_json("profile", &me).ok();
            s.db.set_bool("claimed", true).ok();
            s.db.log("connect", "kết nối agent Moltbook thành công", "", now_ts())
                .ok();
            Ok(Json(
                json!({ "ok": true, "profile": me, "account": account_summary(&s.db) }),
            ))
        }
        Err(e) => {
            // Key stored but not verified — keep it so the user can retry; report why.
            s.db.log("error", &format!("kết nối thất bại: {e}"), "", now_ts())
                .ok();
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
    Ok(Json(
        json!({ "ok": true, "profile": me, "account": account_summary(&s.db) }),
    ))
}

async fn disconnect(State(s): State<Arc<AppState>>) -> Json<Value> {
    for k in ["api_key", "profile"] {
        s.db.set_json(k, &Value::Null).ok();
    }
    s.db.set_bool("claimed", false).ok();
    s.db.log(
        "disconnect",
        "ngắt kết nối agent (xoá API key khỏi máy)",
        "",
        now_ts(),
    )
    .ok();
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
    memory_enabled: Option<bool>,
    wiki_enabled: Option<bool>,
    wiki_archive: Option<bool>,
    knowledge_space: Option<String>,
    llm_profile: Option<String>,
    topic_mode: Option<String>,
    harvest_enabled: Option<bool>,
    trending_daily: Option<bool>,
    research_enabled: Option<bool>,
    research_on_compose: Option<bool>,
    research_ask_threshold: Option<i64>,
    research_extract_prompt: Option<String>,
    research_max_per_tick: Option<i64>,
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
        s.db.set_str("default_submolt", v.trim().trim_start_matches("m/"))
            .ok();
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
    if let Some(v) = p.memory_enabled {
        s.db.set_bool("memory_enabled", v).ok();
    }
    if let Some(v) = p.wiki_enabled {
        s.db.set_bool("wiki_enabled", v).ok();
    }
    if let Some(v) = p.wiki_archive {
        s.db.set_bool("wiki_archive", v).ok();
    }
    if let Some(v) = p.knowledge_space {
        s.db.set_str("knowledge_space", v.trim()).ok();
    }
    if let Some(v) = p.llm_profile {
        let v = v.trim();
        s.db.set_str("llm_profile", v).ok();
        // Apply immediately — no restart needed.
        llm::set_profile(v);
    }
    if let Some(v) = p.topic_mode {
        let v = v.trim();
        if !matches!(v, "all" | "focus") {
            return Err(bad("topic_mode phải là all | focus"));
        }
        s.db.set_str("topic_mode", v).ok();
    }
    if let Some(v) = p.harvest_enabled {
        s.db.set_bool("harvest_enabled", v).ok();
    }
    if let Some(v) = p.trending_daily {
        s.db.set_bool("trending_daily", v).ok();
    }
    if let Some(v) = p.research_enabled {
        s.db.set_bool("research_enabled", v).ok();
    }
    if let Some(v) = p.research_on_compose {
        s.db.set_bool("research_on_compose", v).ok();
    }
    if let Some(v) = p.research_ask_threshold {
        s.db.set_i64("research_ask_threshold", v.clamp(0, 100)).ok();
    }
    if let Some(v) = p.research_extract_prompt {
        s.db.set_str("research_extract_prompt", v.trim()).ok();
    }
    if let Some(v) = p.research_max_per_tick {
        s.db.set_i64("research_max_per_tick", v.clamp(0, 10)).ok();
    }
    Ok(Json(account_summary(&s.db)))
}

// ---- trending: what the agent internet is talking about → wiki ----

async fn list_digests(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    let digests = s.db.list_digests(60).map_err(server)?;
    Ok(Json(json!({ "digests": digests, "count": digests.len() })))
}

#[derive(Deserialize)]
struct TrendingBody {
    /// Also write the wiki doc (default true).
    #[serde(default = "yes")]
    write_wiki: bool,
}
fn yes() -> bool {
    true
}

async fn run_trending(State(s): State<Arc<AppState>>, Json(b): Json<TrendingBody>) -> Json<Value> {
    Json(engine::trending_digest(&s, b.write_wiki).await)
}

// ---- tracked posts: feedback harvest → wiki doc ----

/// Our published posts + the state of every feedback check on them.
async fn list_tracked(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    let posts = s.db.list_tracked(200).map_err(server)?;
    let items: Vec<Value> = posts
        .iter()
        .map(|t| {
            let mut v = serde_json::to_value(t).unwrap_or(json!({}));
            // Derived: are there agent comments the doc hasn't absorbed yet?
            v["doc_is_stale"] = json!(t.doc_is_stale());
            v
        })
        .collect();
    Ok(Json(json!({ "posts": items, "count": items.len() })))
}

#[derive(Deserialize)]
struct TrackBody {
    post_id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    submolt: String,
}

async fn track_post(
    State(s): State<Arc<AppState>>,
    Json(b): Json<TrackBody>,
) -> Result<Json<Value>, ApiError> {
    if b.post_id.trim().is_empty() {
        return Err(bad("post_id là bắt buộc"));
    }
    s.db.track_post(b.post_id.trim(), &b.title, &b.submolt, "", now_ts())
        .map_err(bad)?;
    let t = s.db.get_tracked(b.post_id.trim()).map_err(server)?;
    Ok(Json(json!({ "ok": true, "post": t })))
}

async fn untrack_post(
    State(s): State<Arc<AppState>>,
    Path(post_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    s.db.untrack(&post_id).map_err(server)?;
    Ok(Json(json!({ "ok": true, "post_id": post_id })))
}

#[derive(Deserialize)]
struct HarvestBody {
    /// Harvest just this post (forces a doc refresh even with no new comments).
    #[serde(default)]
    post_id: Option<String>,
}

/// Collect other agents' comments on our posts, synthesise them, and refresh the
/// wiki docs.
async fn harvest(State(s): State<Arc<AppState>>, Json(b): Json<HarvestBody>) -> Json<Value> {
    let pid = b
        .post_id
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty());
    Json(engine::harvest(&s, pid).await)
}

// ---- topics: steering what the molty engages with / posts about ----

async fn list_topics(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    let topics = s.db.list_topics(false).map_err(server)?;
    Ok(Json(
        json!({ "topics": topics, "topic_mode": s.db.topic_mode(), "count": topics.len() }),
    ))
}

#[derive(Deserialize)]
struct AddTopicBody {
    text: String,
    #[serde(default)]
    kind: Option<String>,
}

async fn add_topic(
    State(s): State<Arc<AppState>>,
    Json(b): Json<AddTopicBody>,
) -> Result<Json<Value>, ApiError> {
    if b.text.trim().is_empty() {
        return Err(bad("text là bắt buộc"));
    }
    let id =
        s.db.add_topic(&b.text, b.kind.as_deref().unwrap_or("both"), now_ts())
            .map_err(bad)?;
    let t =
        s.db.list_topics(false)
            .map_err(server)?
            .into_iter()
            .find(|t| t.id == id);
    Ok(Json(json!({ "ok": true, "topic": t })))
}

#[derive(Deserialize)]
struct PatchTopicBody {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    enabled: Option<bool>,
}

async fn patch_topic(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(b): Json<PatchTopicBody>,
) -> Result<Json<Value>, ApiError> {
    s.db.update_topic(id, b.text.as_deref(), b.kind.as_deref(), b.enabled)
        .map_err(bad)?;
    let t =
        s.db.list_topics(false)
            .map_err(server)?
            .into_iter()
            .find(|t| t.id == id);
    Ok(Json(json!({ "ok": true, "topic": t })))
}

async fn delete_topic(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    s.db.delete_topic(id).map_err(server)?;
    Ok(Json(json!({ "ok": true, "id": id })))
}

// ---- SenClaw integrations: knowledge (trí nhớ) + wiki (kho thông tin) ----

/// Are the daemon's wiki + knowledge actually reachable right now?
async fn integrations(State(s): State<Arc<AppState>>) -> Json<Value> {
    Json(crate::senclaw::integrations_status(&memory_space(&s.db)).await)
}

#[derive(Deserialize)]
struct RecallBody {
    query: String,
}

/// Ask the molty's memory a question (synthesized answer over its space).
async fn memory_recall(
    State(s): State<Arc<AppState>>,
    Json(b): Json<RecallBody>,
) -> Result<Json<Value>, ApiError> {
    if b.query.trim().is_empty() {
        return Err(bad("query là bắt buộc"));
    }
    let space = memory_space(&s.db);
    let answer = crate::senclaw::knowledge_recall(&space, b.query.trim())
        .await
        .map_err(upstream)?;
    let hits = crate::senclaw::knowledge_search(&space, b.query.trim(), 6)
        .await
        .unwrap_or_default();
    Ok(Json(json!({
        "space": space,
        "answer": answer,
        "grounded": !answer.trim().is_empty(),
        "hits": hits.iter().map(|(n, s, sc)| json!({ "name": n, "summary": s, "score": sc })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
struct RememberBody {
    text: String,
    #[serde(default)]
    tags: Vec<String>,
}

/// Write something into the molty's memory by hand.
async fn memory_save(
    State(s): State<Arc<AppState>>,
    Json(b): Json<RememberBody>,
) -> Result<Json<Value>, ApiError> {
    if b.text.trim().is_empty() {
        return Err(bad("text là bắt buộc"));
    }
    let space = memory_space(&s.db);
    let tags: Vec<&str> = std::iter::once("moltbook")
        .chain(b.tags.iter().map(String::as_str))
        .collect();
    crate::senclaw::knowledge_save(&space, b.text.trim(), &tags, "moltbook:manual")
        .await
        .map_err(upstream)?;
    s.db.log(
        "memory",
        &format!("ghi trí nhớ thủ công vào {space}"),
        "",
        now_ts(),
    )
    .ok();
    Ok(Json(json!({ "ok": true, "space": space })))
}

#[derive(Deserialize)]
struct ArchiveBody {
    post_id: String,
}

/// Archive a Moltbook post + its discussion into the wiki (kho thông tin).
async fn wiki_archive(
    State(s): State<Arc<AppState>>,
    Json(b): Json<ArchiveBody>,
) -> Result<Json<Value>, ApiError> {
    if b.post_id.trim().is_empty() {
        return Err(bad("post_id là bắt buộc"));
    }
    let path = engine::archive_post_to_wiki(&s, b.post_id.trim())
        .await
        .map_err(upstream)?;
    Ok(Json(json!({ "ok": true, "path": path })))
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

async fn get_feed(State(s): State<Arc<AppState>>, Query(q): Query<FeedQuery>) -> Json<Value> {
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
    Json(
        json!({ "posts": posts, "source": source, "connected": connected, "warning": warning, "count": posts.len() }),
    )
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
    let comments = mb
        .comments(&id, "best", None)
        .await
        .unwrap_or(json!({ "comments": [] }));
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
    mb.search(
        q.q.trim(),
        q.r#type.as_deref().unwrap_or("all"),
        q.limit.unwrap_or(20),
    )
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
    mb.read_all_notifications()
        .await
        .map(Json)
        .map_err(upstream)
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
    let drafts =
        s.db.list_drafts(q.status.as_deref(), q.limit.unwrap_or(200))
            .map_err(server)?;
    Ok(Json(json!({ "drafts": drafts, "count": drafts.len() })))
}

async fn drafts_count(State(s): State<Arc<AppState>>) -> Json<Value> {
    Json(json!({
        "pending": s.db.count_pending_drafts().unwrap_or(0),
        "needs_input": s.db.count_drafts_with_status("needs_input").unwrap_or(0),
    }))
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
        research: String::new(),
        questions: Vec::new(),
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
    /// Override: force research on/off for this compose (default = settings).
    #[serde(default)]
    research: Option<bool>,
}

/// Should this manual compose run the research workflows first?
fn compose_research_on(db: &Db, explicit: Option<bool>) -> bool {
    explicit.unwrap_or_else(|| {
        db.get_bool("research_enabled", true) && db.get_bool("research_on_compose", true)
    })
}

/// LLM-draft a reply to a post and queue it for approval. When research is on,
/// the matching workflows run first and the reply is grounded in their
/// findings; uncertain research parks the draft as `needs_input`.
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

    let mut research_json = String::new();
    let mut questions: Vec<String> = Vec::new();
    let mut composed: Option<(String, String)> = None;
    if compose_research_on(&s.db, b.research) {
        let input = crate::research::ResearchInput {
            flow: "comment".into(),
            topic: if title.trim().is_empty() {
                b.instruction.clone()
            } else {
                title.clone()
            },
            title: title.clone(),
            content: content.clone(),
            post_id: b.target_post_id.clone(),
        };
        match crate::research::run_research(&s.db, &input).await {
            Some(Ok(bundle)) => {
                let block = bundle.render();
                if !block.is_empty() {
                    composed = llm::compose_reply_researched(
                        &voice,
                        &title,
                        &content,
                        &b.instruction,
                        &block,
                        "",
                    )
                    .await
                    .ok()
                    .filter(|(t, _)| !t.trim().is_empty());
                }
                questions = crate::research::gate_questions(&s.db, &bundle);
                research_json = bundle.to_json().to_string();
            }
            Some(Err(e)) => {
                s.db.log("error", &format!("nghiên cứu thất bại: {e}"), &b.target_post_id, now_ts())
                    .ok();
            }
            None => {}
        }
    }
    let (text, model) = match composed {
        Some(v) => v,
        None => {
            // No research (off / no workflows / failed) — classic grounding.
            let g = grounding_for(&s.db, &format!("{title} {}", b.instruction)).await;
            llm::compose_reply(&voice, &title, &content, &b.instruction, &g)
                .await
                .map_err(upstream)?
        }
    };
    let dc = DraftCreate {
        kind: "comment".into(),
        target_post_id: b.target_post_id.clone(),
        target_title: title,
        content: text,
        reason: if b.instruction.is_empty() {
            "soạn trả lời".into()
        } else {
            b.instruction.clone()
        },
        source: "user".into(),
        model,
        research: research_json,
        questions,
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
    /// Override: force research on/off for this compose (default = settings).
    #[serde(default)]
    research: Option<bool>,
}

async fn compose_post_draft(
    State(s): State<Arc<AppState>>,
    Json(b): Json<ComposePostBody>,
) -> Result<Json<Value>, ApiError> {
    let voice = voice(&s.db);
    let submolt = if b.submolt.trim().is_empty() {
        s.db.get_str("default_submolt", "general")
    } else {
        b.submolt.trim().trim_start_matches("m/").to_string()
    };
    // A new post should come from the user's real knowledge, not thin air.
    let topic = if b.topic.trim().is_empty() {
        submolt.clone()
    } else {
        b.topic.clone()
    };

    let mut research_json = String::new();
    let mut questions: Vec<String> = Vec::new();
    let mut composed: Option<(llm::DraftedPost, String)> = None;
    if compose_research_on(&s.db, b.research) {
        let input = crate::research::ResearchInput {
            flow: "post".into(),
            topic: topic.clone(),
            title: b.topic.clone(),
            content: String::new(),
            post_id: String::new(),
        };
        match crate::research::run_research(&s.db, &input).await {
            Some(Ok(bundle)) => {
                let block = bundle.render();
                if !block.is_empty() {
                    composed =
                        llm::compose_post_researched(&voice, &submolt, &b.topic, "", &block, "")
                            .await
                            .ok()
                            .filter(|(p, _)| !p.title.trim().is_empty());
                }
                questions = crate::research::gate_questions(&s.db, &bundle);
                research_json = bundle.to_json().to_string();
            }
            Some(Err(e)) => {
                s.db.log("error", &format!("nghiên cứu thất bại: {e}"), "", now_ts())
                    .ok();
            }
            None => {}
        }
    }
    let (post, model) = match composed {
        Some(v) => v,
        None => {
            let g = grounding_for(&s.db, &topic).await;
            llm::compose_post(&voice, &submolt, &b.topic, &g)
                .await
                .map_err(upstream)?
        }
    };
    let dc = DraftCreate {
        kind: "post".into(),
        submolt,
        title: post.title,
        content: post.content,
        reason: if b.topic.is_empty() {
            "soạn bài mới".into()
        } else {
            b.topic.clone()
        },
        source: "user".into(),
        model,
        research: research_json,
        questions,
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
    let draft =
        s.db.get_draft(id)
            .map_err(server)?
            .ok_or_else(|| not_found("draft không tồn tại"))?;
    if draft.status == "needs_input" {
        return Err(bad(
            "Draft này đang chờ bạn trả lời câu hỏi nghiên cứu. Trả lời (hoặc bỏ qua câu hỏi) trước, rồi mới duyệt.",
        ));
    }
    if draft.status != "pending" {
        return Err(bad(format!("draft đã ở trạng thái '{}'", draft.status)));
    }
    // The post already exists on Moltbook and only failed verification —
    // approving again would publish a duplicate. Force the retry path instead.
    if draft.awaiting_verify() {
        return Err(bad(
            "Bài này ĐÃ được đăng lên Moltbook, chỉ chưa xác minh. Dùng 'Xác minh lại' thay vì duyệt lại (duyệt lại sẽ tạo bài trùng).",
        ));
    }
    match engine::execute_draft(&s, &draft).await {
        Ok(reference) => {
            let now = now_ts();
            s.db.set_draft_result(id, "posted", &reference, "", now)
                .ok();
            s.db.log(
                &draft.kind,
                &format!("duyệt & đăng {} (#{id})", draft.kind),
                &reference,
                now,
            )
            .ok();
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

/// Retry ONLY the anti-human verification for a post that was already created.
async fn verify_draft(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    match engine::retry_verify(&s, id).await {
        Ok(reference) => {
            let d = s.db.get_draft(id).map_err(server)?;
            Ok(Json(json!({ "ok": true, "ref": reference, "draft": d })))
        }
        Err(e) => {
            let d = s.db.get_draft(id).map_err(server)?;
            Ok(Json(json!({ "ok": false, "error": e, "draft": d })))
        }
    }
}

async fn reject_draft(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let draft =
        s.db.get_draft(id)
            .map_err(server)?
            .ok_or_else(|| not_found("draft không tồn tại"))?;
    s.db.set_draft_result(id, "rejected", "", "", now_ts())
        .map_err(server)?;
    s.db.log(
        "reject",
        &format!("từ chối nháp {} (#{id})", draft.kind),
        "",
        now_ts(),
    )
    .ok();
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
                    db.set_draft_result(id, "posted", &reference, "", now_ts())
                        .ok();
                    db.log(
                        &draft.kind,
                        &format!("live: {} (#{id})", draft.kind),
                        &reference,
                        now_ts(),
                    )
                    .ok();
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
    let submolt = if b.submolt.trim().is_empty() {
        s.db.get_str("default_submolt", "general")
    } else {
        b.submolt.trim().trim_start_matches("m/").to_string()
    };
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
    let dc = DraftCreate {
        kind: "follow".into(),
        target_name: b.name,
        source: "user".into(),
        ..Default::default()
    };
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

// ---- research workflows ----

async fn list_workflows_h(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    let wfs = s.db.list_workflows(false).map_err(server)?;
    let items: Vec<Value> = wfs
        .iter()
        .map(|w| {
            let mut v = serde_json::to_value(w).unwrap_or(json!({}));
            // Hand the UI parsed steps so it never re-parses the JSON string.
            v["steps_parsed"] = serde_json::to_value(crate::research::parse_steps(&w.steps))
                .unwrap_or(json!([]));
            v
        })
        .collect();
    Ok(Json(json!({ "workflows": items, "count": items.len() })))
}

#[derive(Deserialize)]
struct WorkflowBody {
    name: String,
    #[serde(default)]
    flow: String,
    /// Steps as a JSON array (already-parsed value, not a string).
    #[serde(default)]
    steps: Value,
    #[serde(default)]
    extract_prompt: String,
}

async fn create_workflow_h(
    State(s): State<Arc<AppState>>,
    Json(b): Json<WorkflowBody>,
) -> Result<Json<Value>, ApiError> {
    if b.name.trim().is_empty() {
        return Err(bad("name là bắt buộc"));
    }
    let steps: Vec<crate::research::Step> =
        serde_json::from_value(b.steps.clone()).map_err(|e| bad(format!("steps không hợp lệ: {e}")))?;
    if steps.is_empty() {
        return Err(bad("workflow cần ít nhất 1 bước"));
    }
    let steps_json = serde_json::to_string(&steps).map_err(server)?;
    let id =
        s.db.add_workflow(&b.name, &b.flow, &steps_json, &b.extract_prompt, false, now_ts())
            .map_err(bad)?;
    s.db.log(
        "workflow",
        &format!("tạo workflow '{}' ({} bước)", b.name.trim(), steps.len()),
        &id.to_string(),
        now_ts(),
    )
    .ok();
    Ok(Json(json!({ "ok": true, "workflow": s.db.get_workflow(id).ok().flatten() })))
}

#[derive(Deserialize)]
struct WorkflowPatch {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    flow: Option<String>,
    #[serde(default)]
    steps: Option<Value>,
    #[serde(default)]
    extract_prompt: Option<String>,
    #[serde(default)]
    enabled: Option<bool>,
}

async fn patch_workflow_h(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(b): Json<WorkflowPatch>,
) -> Result<Json<Value>, ApiError> {
    let steps_json = match &b.steps {
        Some(v) => {
            let steps: Vec<crate::research::Step> = serde_json::from_value(v.clone())
                .map_err(|e| bad(format!("steps không hợp lệ: {e}")))?;
            if steps.is_empty() {
                return Err(bad("workflow cần ít nhất 1 bước"));
            }
            Some(serde_json::to_string(&steps).map_err(server)?)
        }
        None => None,
    };
    s.db.update_workflow(
        id,
        b.name.as_deref(),
        b.flow.as_deref(),
        steps_json.as_deref(),
        b.extract_prompt.as_deref(),
        b.enabled,
        now_ts(),
    )
    .map_err(bad)?;
    match s.db.get_workflow(id).map_err(server)? {
        Some(w) => Ok(Json(json!({ "ok": true, "workflow": w }))),
        None => Err(not_found(format!("workflow {id} không tồn tại"))),
    }
}

async fn delete_workflow_h(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    s.db.delete_workflow(id).map_err(server)?;
    Ok(Json(json!({ "ok": true, "id": id })))
}

/// The live tool catalog (builtin + Space Apps + daemon MCP servers).
async fn research_tools_h() -> Json<Value> {
    Json(crate::research::catalog().await)
}

#[derive(Deserialize)]
struct ResearchRunBody {
    #[serde(default)]
    flow: String,
    topic: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    post_id: String,
}

/// Run the matching workflows on a topic NOW and return the bundle — the UI's
/// "chạy thử" button and the agent's on-demand research tool.
async fn research_run_h(
    State(s): State<Arc<AppState>>,
    Json(b): Json<ResearchRunBody>,
) -> Result<Json<Value>, ApiError> {
    if b.topic.trim().is_empty() {
        return Err(bad("topic là bắt buộc"));
    }
    let input = crate::research::ResearchInput {
        flow: crate::db::norm_flow(&b.flow),
        topic: b.topic.clone(),
        title: b.title.clone(),
        content: b.content.clone(),
        post_id: b.post_id.clone(),
    };
    // Coerce 'both' to a concrete flow for matching (both matches everything).
    let input = crate::research::ResearchInput {
        flow: if input.flow == "both" {
            "post".into()
        } else {
            input.flow
        },
        ..input
    };
    match crate::research::run_research(&s.db, &input).await {
        Some(Ok(bundle)) => {
            let questions = crate::research::gate_questions(&s.db, &bundle);
            Ok(Json(json!({
                "ok": true,
                "bundle": bundle.to_json(),
                "gated_questions": questions,
                "sources": bundle.sources_line(),
                "rendered": bundle.render(),
            })))
        }
        Some(Err(e)) => Ok(Json(json!({ "ok": false, "error": e }))),
        None => Ok(Json(json!({
            "ok": false,
            "error": "nghiên cứu đang tắt hoặc không có workflow nào khớp flow này",
        }))),
    }
}

#[derive(Deserialize)]
struct AiBuildBody {
    description: String,
    #[serde(default)]
    flow: String,
}

/// AI-compose a workflow from the live tool catalog and save it.
async fn research_ai_build_h(
    State(s): State<Arc<AppState>>,
    Json(b): Json<AiBuildBody>,
) -> Result<Json<Value>, ApiError> {
    if b.description.trim().is_empty() {
        return Err(bad("description là bắt buộc"));
    }
    let (name, flow, steps) = crate::research::ai_build_workflow(&b.description, b.flow.trim())
        .await
        .map_err(upstream)?;
    let steps_json = serde_json::to_string(&steps).map_err(server)?;
    let id =
        s.db.add_workflow(&name, &flow, &steps_json, "", false, now_ts())
            .map_err(bad)?;
    s.db.log(
        "workflow",
        &format!("AI tạo workflow '{name}' ({} bước, flow {flow})", steps.len()),
        &id.to_string(),
        now_ts(),
    )
    .ok();
    Ok(Json(json!({ "ok": true, "workflow": s.db.get_workflow(id).ok().flatten() })))
}

#[derive(Deserialize)]
struct AnswerBody {
    /// The human's answer. Empty = skip the questions and release the draft.
    #[serde(default)]
    answer: String,
}

/// Answer (or skip) a needs_input draft's research questions → re-compose →
/// back to the approval queue.
async fn answer_draft_h(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(b): Json<AnswerBody>,
) -> Result<Json<Value>, ApiError> {
    match engine::answer_draft(&s, id, &b.answer).await {
        Ok(d) => Ok(Json(json!({ "ok": true, "draft": d }))),
        Err(e) => Err(bad(e)),
    }
}

// ---- demo ----

async fn demo_seed(State(s): State<Arc<AppState>>) -> Json<Value> {
    let n = s.db.seed_demo(now_ts()).unwrap_or(0);
    Json(json!({ "ok": true, "seeded": n }))
}

// ---- models ----

/// The daemon's LLM profiles (configs with their `label`) + which one is active.
/// The app only READS this — choosing a profile for Moltbook is a local setting
/// (`llm_profile`), never a change to the daemon's active model.
async fn get_models() -> Result<Json<Value>, ApiError> {
    llm::list_models().await.map(Json).map_err(upstream)
}
