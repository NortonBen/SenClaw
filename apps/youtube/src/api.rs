use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::db::{default_data_dir, Db};
use crate::extbridge::ExtBridge;
use crate::{llm, youtube};

pub struct AppState {
    pub db: Arc<Db>,
    pub bridge: ExtBridge,
    /// Broadcasts the raw JSON-RPC responses to any connected MCP SSE clients.
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
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn make_state() -> Arc<AppState> {
    let db_path = default_data_dir("youtube").join("youtube.db");
    let db = Arc::new(Db::open(&db_path).expect("open youtube db"));
    let bridge = ExtBridge::new();

    // When the extension pushes an auth/context event, snapshot it into the db so
    // the UI + `youtube_status` can report login state. We store only presence
    // flags + non-secret InnerTube context — never raw cookies.
    {
        let db = db.clone();
        bridge.set_event_handler(move |ev: Value| {
            let kind = ev.get("type").and_then(|t| t.as_str()).unwrap_or("");
            match kind {
                "token_captured" | "auth_state" | "yt_context" => {
                    let payload = ev.get("data").cloned().unwrap_or(ev.clone());
                    let _ = db.set_kv("auth", &payload);
                    db.log("auth", kind, now());
                }
                "extension_ready" => db.log("extension", "ready", now()),
                _ => {}
            }
        });
    }

    let (mcp_tx, _) = tokio::sync::broadcast::channel(100);
    Arc::new(AppState { db, bridge, mcp_tx })
}

pub fn api_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/status", get(status))
        .route("/llm-info", get(llm_info))
        .route("/models", get(models))
        .route("/model-active", post(model_active))
        // YouTube read
        .route("/search", get(search))
        .route("/browse", get(browse))
        .route("/comments", get(comments))
        .route("/comments/sync", post(sync_comments))
        .route("/comments/cached", get(cached_comments))
        .route("/comments/analyze", post(analyze_comments))
        .route("/comments/stats", get(comment_stats))
        .route("/comments/scan", get(scan_keywords))
        .route("/comments/index", post(index_comments))
        .route("/comment/action", post(comment_action))
        // CRM pull-feed (a CRM channel of kind "social" polls these; mirrors apps/social)
        .route("/inbox", get(inbox))
        .route("/inbox/reply", post(inbox_reply))
        // P11 — OAuth (Data API moderation)
        .route("/oauth/status", get(oauth_status))
        .route("/oauth/config", post(oauth_config))
        .route("/oauth/start", get(oauth_start))
        .route("/oauth/callback", get(oauth_callback))
        .route("/oauth/me", get(oauth_me))
        .route("/oauth/logout", post(oauth_logout))
        .route("/moderate", post(moderate))
        // Drafts (write, human-in-the-loop)
        .route("/drafts", get(list_drafts))
        .route("/draft", post(create_draft))
        .route("/draft/ai", post(ai_draft))
        .route("/draft/approve", post(approve_draft))
        .route("/draft/send", post(send_draft))
        .route("/activity", get(activity))
        // Extension bridge callback (extension → app RPC replies)
        .route("/ext/callback", post(ext_callback))
        // MCP
        .route(
            "/mcp/sse",
            get(crate::mcp::mcp_sse).post(crate::mcp::mcp_message),
        )
        .route("/mcp/message", post(crate::mcp::mcp_message))
        .with_state(state)
}

async fn status(State(s): State<Arc<AppState>>) -> Json<Value> {
    Json(json!({ "ok": true, "app": "youtube", "status": youtube::status(&s.bridge, &s.db) }))
}

// ---- YouTube read ----

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
}

async fn search(
    State(s): State<Arc<AppState>>,
    Query(q): Query<SearchQuery>,
) -> Result<Json<Value>, ApiError> {
    if q.q.trim().is_empty() {
        return Err(bad("thiếu tham số tìm kiếm `q`"));
    }
    let res = youtube::search(&s.bridge, q.q.trim())
        .await
        .map_err(gateway)?;
    s.db.log("search", q.q.trim(), now());
    Ok(Json(res))
}

#[derive(Deserialize)]
struct BrowseQuery {
    id: String,
    #[serde(default)]
    params: Option<String>,
}

async fn browse(
    State(s): State<Arc<AppState>>,
    Query(q): Query<BrowseQuery>,
) -> Result<Json<Value>, ApiError> {
    if q.id.trim().is_empty() {
        return Err(bad("thiếu `id` (browseId / channelId)"));
    }
    let res = youtube::browse(&s.bridge, q.id.trim(), q.params.as_deref())
        .await
        .map_err(gateway)?;
    Ok(Json(res))
}

#[derive(Deserialize)]
struct CommentsQuery {
    /// Either a video id (derive the comment token) …
    #[serde(default)]
    video_id: Option<String>,
    /// … or a comment-section continuation token directly.
    #[serde(default)]
    continuation: Option<String>,
}

async fn comments(
    State(s): State<Arc<AppState>>,
    Query(q): Query<CommentsQuery>,
) -> Result<Json<Value>, ApiError> {
    let res = match (q.continuation.as_deref(), q.video_id.as_deref()) {
        (Some(tok), _) if !tok.trim().is_empty() => youtube::comments(&s.bridge, tok.trim()).await,
        (_, Some(vid)) if !vid.trim().is_empty() => {
            youtube::comments_for_video(&s.bridge, vid.trim()).await
        }
        _ => return Err(bad("cần `video_id` hoặc `continuation`")),
    };
    res.map(Json).map_err(gateway)
}

#[derive(Deserialize)]
struct SyncBody {
    video_id: String,
    #[serde(default)]
    max_pages: Option<u32>,
}

/// Pull + cache a video's comments (foundation for analytics).
async fn sync_comments(
    State(s): State<Arc<AppState>>,
    Json(b): Json<SyncBody>,
) -> Result<Json<Value>, ApiError> {
    if b.video_id.trim().is_empty() {
        return Err(bad("thiếu video_id"));
    }
    let res = youtube::sync_comments(
        &s.bridge,
        &s.db,
        b.video_id.trim(),
        b.max_pages.unwrap_or(3),
        now(),
    )
    .await
    .map_err(gateway)?;
    Ok(Json(res))
}

#[derive(Deserialize)]
struct CachedQuery {
    video_id: String,
    #[serde(default)]
    limit: Option<i64>,
}

async fn cached_comments(
    State(s): State<Arc<AppState>>,
    Query(q): Query<CachedQuery>,
) -> Result<Json<Value>, ApiError> {
    if q.video_id.trim().is_empty() {
        return Err(bad("thiếu video_id"));
    }
    let limit = q.limit.unwrap_or(100).clamp(1, 1000);
    let rows = s.db.list_comments(q.video_id.trim(), limit).map_err(bad)?;
    Ok(Json(json!({ "count": rows.len(), "comments": rows })))
}

// ---- P7 analytics ----

#[derive(Deserialize)]
struct AnalyzeBody {
    #[serde(default)]
    max: Option<usize>,
}

async fn analyze_comments(
    State(s): State<Arc<AppState>>,
    Json(b): Json<AnalyzeBody>,
) -> Result<Json<Value>, ApiError> {
    let res = youtube::analyze_pending(&s.db, b.max.unwrap_or(60), now())
        .await
        .map_err(gateway)?;
    Ok(Json(res))
}

async fn comment_stats(
    State(s): State<Arc<AppState>>,
    Query(q): Query<CachedQuery>,
) -> Result<Json<Value>, ApiError> {
    if q.video_id.trim().is_empty() {
        return Err(bad("thiếu video_id"));
    }
    Ok(Json(s.db.comment_stats(q.video_id.trim()).map_err(bad)?))
}

#[derive(Deserialize)]
struct ScanQuery {
    #[serde(default)]
    video_id: Option<String>,
    /// Comma-separated keywords.
    keywords: String,
    #[serde(default)]
    limit: Option<i64>,
}

async fn scan_keywords(
    State(s): State<Arc<AppState>>,
    Query(q): Query<ScanQuery>,
) -> Result<Json<Value>, ApiError> {
    let kws: Vec<String> = q
        .keywords
        .split(',')
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .collect();
    if kws.is_empty() {
        return Err(bad("thiếu keywords"));
    }
    let rows =
        s.db.search_comments(
            q.video_id.as_deref(),
            &kws,
            q.limit.unwrap_or(100).clamp(1, 500),
        )
        .map_err(bad)?;
    Ok(Json(
        json!({ "count": rows.len(), "keywords": kws, "comments": rows }),
    ))
}

#[derive(Deserialize)]
struct IndexBody {
    video_id: String,
    #[serde(default)]
    limit: Option<i64>,
}

async fn index_comments(
    State(s): State<Arc<AppState>>,
    Json(b): Json<IndexBody>,
) -> Result<Json<Value>, ApiError> {
    if b.video_id.trim().is_empty() {
        return Err(bad("thiếu video_id"));
    }
    let res = youtube::index_comments(
        &s.db,
        b.video_id.trim(),
        b.limit.unwrap_or(50).clamp(1, 500),
    )
    .await
    .map_err(gateway)?;
    Ok(Json(res))
}

// ---- P8 comment action ----

#[derive(Deserialize)]
struct ActionBody {
    comment_id: String,
    action: String,
    #[serde(default)]
    confirm: bool,
}

async fn comment_action(
    State(s): State<Arc<AppState>>,
    Json(b): Json<ActionBody>,
) -> Result<Json<Value>, ApiError> {
    let res = youtube::comment_action(
        &s.bridge,
        &s.db,
        b.comment_id.trim(),
        b.action.trim(),
        b.confirm,
    )
    .await
    .map_err(gateway)?;
    Ok(Json(res))
}

// ---- P9 CRM pull-feed (mirrors apps/social) ----

#[derive(Deserialize)]
struct InboxQuery {
    #[serde(default)]
    since: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
}

/// Cursor feed a CRM `social` channel polls: `GET /api/inbox?since=&limit=`.
async fn inbox(
    State(s): State<Arc<AppState>>,
    Query(q): Query<InboxQuery>,
) -> Result<Json<Value>, ApiError> {
    let since = q.since.unwrap_or(0);
    let limit = q.limit.unwrap_or(200).clamp(1, 500);
    let msgs = s.db.feed_since(since, limit).map_err(bad)?;
    Ok(Json(json!({ "messages": msgs })))
}

#[derive(Deserialize)]
struct InboxReplyBody {
    #[serde(default)]
    #[allow(dead_code)]
    platform: Option<String>,
    /// The comment id to reply to (feed `external_id`).
    external_id: String,
    text: String,
}

/// A CRM operator's reply routed back to YouTube: `POST /api/inbox/reply`.
async fn inbox_reply(
    State(s): State<Arc<AppState>>,
    Json(b): Json<InboxReplyBody>,
) -> Result<Json<Value>, ApiError> {
    if b.text.trim().is_empty() {
        return Err(bad("nội dung reply rỗng"));
    }
    let rp =
        s.db.reply_params_of(b.external_id.trim())
            .map_err(bad)?
            .ok_or_else(|| bad("comment không có replyParams đã cache — chạy sync trước"))?;
    let res = youtube::send_action(&s.bridge, "reply", &rp, b.text.trim())
        .await
        .map_err(gateway)?;
    Ok(Json(res))
}

// ---- P11 OAuth / moderation ----

async fn oauth_status(State(s): State<Arc<AppState>>) -> Json<Value> {
    Json(crate::oauth::status(&s.db))
}

#[derive(Deserialize)]
struct OAuthConfigBody {
    client_id: String,
    client_secret: String,
}

async fn oauth_config(
    State(s): State<Arc<AppState>>,
    Json(b): Json<OAuthConfigBody>,
) -> Result<Json<Value>, ApiError> {
    if b.client_id.trim().is_empty() || b.client_secret.trim().is_empty() {
        return Err(bad("thiếu client_id / client_secret"));
    }
    crate::oauth::set_config(&s.db, b.client_id.trim(), b.client_secret.trim()).map_err(bad)?;
    Ok(Json(
        json!({ "ok": true, "authUrl": crate::oauth::auth_url(&s.db).map_err(bad)? }),
    ))
}

/// Redirect the browser to Google's consent screen.
async fn oauth_start(State(s): State<Arc<AppState>>) -> Result<Response, ApiError> {
    let url = crate::oauth::auth_url(&s.db).map_err(bad)?;
    Ok(axum::response::Redirect::to(&url).into_response())
}

#[derive(Deserialize)]
struct CallbackQuery {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

/// Google redirects here with `?code=`. Exchange it and show a done page.
async fn oauth_callback(
    State(s): State<Arc<AppState>>,
    Query(q): Query<CallbackQuery>,
) -> Response {
    let html = |msg: &str, ok: bool| -> Response {
        let color = if ok { "#16a34a" } else { "#dc2626" };
        axum::response::Html(format!(
            "<!doctype html><meta charset=utf-8><body style=\"font:16px system-ui;text-align:center;padding:60px\">\
             <h2 style=\"color:{color}\">{msg}</h2><p>Bạn có thể đóng tab này và quay lại app SenClaw YouTube.</p></body>"
        ))
        .into_response()
    };
    if let Some(err) = q.error {
        return html(&format!("Uỷ quyền bị từ chối: {err}"), false);
    }
    let Some(code) = q.code else {
        return html("Thiếu mã uỷ quyền", false);
    };
    match crate::oauth::exchange_code(&s.db, &code).await {
        Ok(()) => {
            s.db.log("oauth", "authorized", now());
            // Best-effort: cache the signed-in channel identity now.
            let _ = crate::oauth::whoami(&s.db).await;
            html("✓ Đã đăng nhập YouTube (Google)", true)
        }
        Err(e) => html(&format!("Lỗi đổi token: {e}"), false),
    }
}

/// The signed-in Google/YouTube channel identity (calls the Data API).
async fn oauth_me(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    let id = crate::oauth::whoami(&s.db).await.map_err(gateway)?;
    Ok(Json(id))
}

/// Sign out of Google (drop tokens + identity).
async fn oauth_logout(State(s): State<Arc<AppState>>) -> Json<Value> {
    crate::oauth::logout(&s.db);
    s.db.log("oauth", "logout", now());
    Json(json!({ "ok": true }))
}

#[derive(Deserialize)]
struct ModerateBody {
    comment_id: String,
    /// heldForReview | published | rejected
    status: String,
    #[serde(default)]
    ban_author: bool,
}

async fn moderate(
    State(s): State<Arc<AppState>>,
    Json(b): Json<ModerateBody>,
) -> Result<Json<Value>, ApiError> {
    let res = crate::oauth::moderate(&s.db, b.comment_id.trim(), b.status.trim(), b.ban_author)
        .await
        .map_err(gateway)?;
    Ok(Json(res))
}

// ---- Drafts ----

#[derive(Deserialize)]
struct DraftFilter {
    #[serde(default)]
    status: Option<String>,
}

async fn list_drafts(
    State(s): State<Arc<AppState>>,
    Query(q): Query<DraftFilter>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(s
        .db
        .list_drafts(q.status.as_deref())
        .map_err(bad)?)))
}

#[derive(Deserialize)]
struct CreateDraftBody {
    kind: String,
    #[serde(default)]
    target: String,
    body: String,
}

/// Store a WRITE draft. Nothing is sent — a draft must be approved then sent.
async fn create_draft(
    State(s): State<Arc<AppState>>,
    Json(b): Json<CreateDraftBody>,
) -> Result<Json<Value>, ApiError> {
    let kind =
        norm_kind(&b.kind).ok_or_else(|| bad("kind phải là comment | reply | community_post"))?;
    if b.body.trim().is_empty() {
        return Err(bad("nội dung draft rỗng"));
    }
    let id =
        s.db.create_draft(kind, b.target.trim(), b.body.trim(), now())
            .map_err(bad)?;
    Ok(Json(json!({ "id": id, "status": "draft" })))
}

#[derive(Deserialize)]
struct AiDraftBody {
    kind: String,
    #[serde(default)]
    target: String,
    /// Context to write about (a video title, a post body, etc.).
    context: String,
    #[serde(default)]
    instruction: Option<String>,
}

/// AI-write a comment/reply body from context and store it as a draft.
async fn ai_draft(
    State(s): State<Arc<AppState>>,
    Json(b): Json<AiDraftBody>,
) -> Result<Json<Value>, ApiError> {
    let kind =
        norm_kind(&b.kind).ok_or_else(|| bad("kind phải là comment | reply | community_post"))?;
    let (text, model) = llm::draft_body(kind, &b.context, b.instruction.as_deref())
        .await
        .map_err(gateway)?;
    let id =
        s.db.create_draft(kind, b.target.trim(), text.trim(), now())
            .map_err(bad)?;
    Ok(Json(
        json!({ "id": id, "status": "draft", "body": text.trim(), "model": model }),
    ))
}

#[derive(Deserialize)]
struct DraftIdBody {
    id: String,
}

async fn approve_draft(
    State(s): State<Arc<AppState>>,
    Json(b): Json<DraftIdBody>,
) -> Result<Json<Value>, ApiError> {
    let d =
        s.db.get_draft(&b.id)
            .map_err(bad)?
            .ok_or_else(|| bad("draft không tồn tại"))?;
    if d.status == "sent" {
        return Err(bad("draft đã gửi rồi"));
    }
    s.db.set_draft_status(&b.id, "approved", None, now())
        .map_err(bad)?;
    Ok(Json(json!({ "id": b.id, "status": "approved" })))
}

/// Send an APPROVED draft. Only approved drafts may be sent (guardrail).
async fn send_draft(
    State(s): State<Arc<AppState>>,
    Json(b): Json<DraftIdBody>,
) -> Result<Json<Value>, ApiError> {
    let d =
        s.db.get_draft(&b.id)
            .map_err(bad)?
            .ok_or_else(|| bad("draft không tồn tại"))?;
    if d.status != "approved" {
        return Err(bad("chỉ gửi được draft đã DUYỆT (approve trước)"));
    }
    match youtube::send_action(&s.bridge, &d.kind, &d.target, &d.body).await {
        Ok(res) => {
            s.db.set_draft_status(&b.id, "sent", Some(&res.to_string()), now())
                .map_err(bad)?;
            s.db.log("send", &d.kind, now());
            Ok(Json(json!({ "id": b.id, "status": "sent", "result": res })))
        }
        Err(e) => {
            s.db.set_draft_status(
                &b.id,
                "failed",
                Some(&json!({ "error": e }).to_string()),
                now(),
            )
            .map_err(bad)?;
            Err(gateway(e))
        }
    }
}

async fn activity(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(s.db.recent_activity(50).map_err(bad)?)))
}

/// Normalize a draft kind, or None if invalid.
pub fn norm_kind(k: &str) -> Option<&'static str> {
    match k.trim() {
        "comment" => Some("comment"),
        "reply" => Some("reply"),
        "community_post" | "post" => Some("community_post"),
        _ => None,
    }
}

// ---- Extension bridge HTTP callback ----

/// The extension POSTs its RPC replies here (resilient to WS drops). Must present
/// the `secret` handed to it on connect.
async fn ext_callback(
    State(s): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let secret = body.get("secret").and_then(|x| x.as_str()).unwrap_or("");
    if secret != s.bridge.secret() {
        return Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "sai callback secret".into(),
        ));
    }
    let id = body.get("id").and_then(|x| x.as_str()).unwrap_or("");
    if id.is_empty() {
        return Err(bad("thiếu id trong callback"));
    }
    let delivered = s.bridge.complete_callback(id, body.clone());
    Ok(Json(json!({ "ok": delivered })))
}

// ---- LLM / models ----

async fn models() -> Result<Json<Value>, ApiError> {
    llm::list_models().await.map(Json).map_err(gateway)
}

#[derive(Deserialize)]
struct ModelActiveBody {
    id: String,
}

async fn model_active(Json(b): Json<ModelActiveBody>) -> Result<Json<Value>, ApiError> {
    llm::set_active_model(&b.id).await.map_err(gateway)?;
    Ok(Json(json!({ "success": true, "activeId": b.id })))
}

/// Which SenClaw LLM the bridge will use (probes the daemon's llm-config).
async fn llm_info() -> Json<Value> {
    let base =
        std::env::var("SENCLAW_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:18788".into());
    let url = format!("{}/api/llm-config", base.trim_end_matches('/'));
    let fetch = reqwest::Client::new()
        .get(&url)
        .timeout(std::time::Duration::from_secs(6))
        .send()
        .await;
    match fetch {
        Ok(r) => match r.json::<Value>().await {
            Ok(v) => {
                let active = v.get("activeId").and_then(|x| x.as_str()).unwrap_or("");
                let cfg = v.get("configs").and_then(|a| a.as_array()).and_then(|a| {
                    a.iter()
                        .find(|c| c.get("id").and_then(|x| x.as_str()) == Some(active))
                });
                let model = cfg
                    .and_then(|c| c.get("modelName"))
                    .and_then(|x| x.as_str());
                Json(json!({ "ok": model.is_some(), "daemon": base, "model": model }))
            }
            Err(e) => Json(json!({ "ok": false, "daemon": base, "error": format!("parse: {e}") })),
        },
        Err(e) => Json(
            json!({ "ok": false, "daemon": base, "error": format!("Không kết nối daemon: {e}") }),
        ),
    }
}
