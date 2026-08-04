//! HTTP + WebSocket API for the AI Chat app.

use crate::channels::ChannelManager;
use crate::db::{default_data_dir, Db, Session, HANDOFF_BOT};
use crate::{engine, senclaw};
use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{Multipart, Path, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::broadcast;

pub struct AppState {
    pub db: Arc<Db>,
    /// JSON-RPC responses fanned out to MCP SSE clients.
    pub mcp_tx: broadcast::Sender<String>,
    /// Live chat events (messages / handoff / outbound) for WS + Support Inbox.
    pub events: broadcast::Sender<String>,
    pub channels: Arc<ChannelManager>,
}

pub fn make_state() -> Arc<AppState> {
    let db_path = default_data_dir("ai-chat").join("ai-chat.db");
    let db = Arc::new(Db::open(&db_path).expect("open ai-chat db"));
    let (mcp_tx, _) = broadcast::channel(100);
    let (events, _) = broadcast::channel(500);
    let channels = ChannelManager::new(db.clone(), events.clone());
    channels.spawn();
    Arc::new(AppState {
        db,
        mcp_tx,
        events,
        channels,
    })
}

pub fn api_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/status", get(status))
        .route("/llm-info", get(llm_info))
        .route("/stats", get(stats))
        .route("/bots", get(list_bots).post(create_bot))
        .route("/bots/:key", patch(update_bot).delete(delete_bot))
        .route("/bots/:key/knowledge", get(bot_knowledge))
        .route("/channels", get(list_channels).post(create_channel))
        .route(
            "/channels/:id",
            patch(update_channel).delete(delete_channel),
        )
        .route("/channels/:id/test", post(test_channel))
        .route("/sessions", get(list_sessions))
        .route(
            "/sessions/:id",
            get(get_session_detail).delete(delete_session),
        )
        .route("/sessions/:id/analyze", post(analyze_session))
        .route(
            "/conversations",
            get(list_conversations).post(create_conversation),
        )
        .route("/conversations/:id/send", post(conversation_send))
        .route("/crm/search", get(crm_search))
        .route("/issues", get(list_issues).post(create_issue))
        .route("/issues/:id", get(get_issue).patch(update_issue))
        .route("/analytics", get(analytics))
        .route("/chat", post(chat))
        .route("/ws/chat/:external_id", get(ws_chat))
        .route("/events", get(events_sse))
        .route("/handoff/:id", post(set_handoff))
        .route("/handoff/:id/reply", post(handoff_reply))
        .route("/knowledge", get(knowledge_search).post(knowledge_write))
        .route("/knowledge/upload", post(knowledge_upload))
        .route("/knowledge/nodes", get(knowledge_nodes))
        .route("/skills-inventory", get(skills_inventory))
        .route("/mcp-inventory", get(mcp_inventory))
        .route("/settings", get(get_settings).post(update_settings))
        .route(
            "/mcp/sse",
            get(crate::mcp::mcp_sse).post(crate::mcp::mcp_message),
        )
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
    Json(json!({ "ok": true, "app": "ai-chat" }))
}
async fn llm_info() -> Json<Value> {
    Json(crate::llm::llm_info().await)
}
async fn stats(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(s.db.stats().map_err(internal)?))
}

// ---- bots ----

/// Strip secrets from a channel config before it leaves the process.
fn redact_channel(ch: &crate::db::Channel) -> Value {
    let mut cfg = ch.config.clone();
    if let Some(obj) = cfg.as_object_mut() {
        for k in ["token", "access_token", "refresh_token", "app_secret"] {
            if let Some(v) = obj.get_mut(k) {
                if v.as_str().map(|s| !s.is_empty()).unwrap_or(false) {
                    *v = json!("••••••");
                }
            }
        }
    }
    json!({
        "id": ch.id, "botKey": ch.bot_key, "kind": ch.kind, "name": ch.name,
        "config": cfg, "enabled": ch.enabled, "lastSyncAt": ch.last_sync_at,
        "lastStatus": ch.last_status, "lastError": ch.last_error,
    })
}

async fn list_bots(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "bots": s.db.list_bots().map_err(internal)? })))
}

#[derive(Deserialize)]
struct BotCreate {
    name: String,
    #[serde(default)]
    system_prompt: String,
    #[serde(default)]
    greeting: String,
}

async fn create_bot(
    State(s): State<Arc<AppState>>,
    Json(b): Json<BotCreate>,
) -> Result<Json<Value>, ApiError> {
    let bot =
        s.db.create_bot(&b.name, &b.system_prompt, &b.greeting)
            .map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    // Every bot gets a web (WebSocket) channel by default.
    let _ =
        s.db.create_channel(&bot.key, "websocket", "Web chat", &json!({}));
    Ok(Json(json!({ "bot": bot })))
}

#[derive(Deserialize)]
struct BotPatch {
    name: Option<String>,
    system_prompt: Option<String>,
    greeting: Option<String>,
    model: Option<String>,
    knowledge_scope: Option<String>,
    allowed_mcp: Option<Vec<String>>,
    allowed_skills: Option<Vec<String>>,
    use_tools: Option<bool>,
    use_knowledge: Option<bool>,
    auto_ingest: Option<bool>,
    auto_issue: Option<bool>,
    enabled: Option<bool>,
}

async fn update_bot(
    State(s): State<Arc<AppState>>,
    Path(key): Path<String>,
    Json(b): Json<BotPatch>,
) -> Result<Json<Value>, ApiError> {
    let found =
        s.db.update_bot(
            &key,
            b.name.as_deref(),
            b.system_prompt.as_deref(),
            b.greeting.as_deref(),
            b.model.as_deref(),
            b.knowledge_scope.as_deref(),
            b.allowed_mcp.as_deref(),
            b.allowed_skills.as_deref(),
            b.use_tools,
            b.use_knowledge,
            b.auto_ingest,
            b.auto_issue,
            b.enabled,
        )
        .map_err(internal)?;
    if !found {
        return Err(err(
            StatusCode::NOT_FOUND,
            format!("không có bot '{}'", key),
        ));
    }
    Ok(Json(
        json!({ "bot": s.db.get_bot(&key).map_err(internal)? }),
    ))
}

async fn delete_bot(
    State(s): State<Arc<AppState>>,
    Path(key): Path<String>,
) -> Result<Json<Value>, ApiError> {
    if s.db.list_bots().map_err(internal)?.len() <= 1 {
        return Err(err(StatusCode::BAD_REQUEST, "cần giữ ít nhất một bot"));
    }
    if !s.db.delete_bot(&key).map_err(internal)? {
        return Err(err(
            StatusCode::NOT_FOUND,
            format!("không có bot '{}'", key),
        ));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn bot_knowledge(
    State(s): State<Arc<AppState>>,
    Path(key): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let bot =
        s.db.get_bot(&key)
            .map_err(internal)?
            .ok_or_else(|| err(StatusCode::NOT_FOUND, format!("không có bot '{}'", key)))?;
    let space = format!("ai-chat:{}", bot.key);
    match senclaw::knowledge_count(&space).await {
        Ok(count) => Ok(Json(json!({ "space": space, "count": count }))),
        Err(e) => Err(err(StatusCode::BAD_GATEWAY, e)),
    }
}

// ---- channels ----

#[derive(Deserialize)]
struct ChannelQuery {
    bot: Option<String>,
}

async fn list_channels(
    State(s): State<Arc<AppState>>,
    Query(q): Query<ChannelQuery>,
) -> Result<Json<Value>, ApiError> {
    let list = s.db.list_channels(q.bot.as_deref()).map_err(internal)?;
    let redacted: Vec<Value> = list.iter().map(redact_channel).collect();
    Ok(Json(json!({ "channels": redacted })))
}

#[derive(Deserialize)]
struct ChannelCreate {
    #[serde(rename = "botKey")]
    bot_key: String,
    kind: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    config: Value,
}

async fn create_channel(
    State(s): State<Arc<AppState>>,
    Json(b): Json<ChannelCreate>,
) -> Result<Json<Value>, ApiError> {
    if !["telegram", "websocket", "zalo", "facebook", "tiktok"].contains(&b.kind.as_str()) {
        return Err(err(StatusCode::BAD_REQUEST, "kind không hợp lệ"));
    }
    if s.db.get_bot(&b.bot_key).map_err(internal)?.is_none() {
        return Err(err(StatusCode::BAD_REQUEST, "bot không tồn tại"));
    }
    let cfg = if b.config.is_object() {
        b.config
    } else {
        json!({})
    };
    let ch =
        s.db.create_channel(&b.bot_key, &b.kind, &b.name, &cfg)
            .map_err(internal)?;
    Ok(Json(json!({ "channel": redact_channel(&ch) })))
}

#[derive(Deserialize)]
struct ChannelPatch {
    name: Option<String>,
    config: Option<Value>,
    enabled: Option<bool>,
}

async fn update_channel(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(b): Json<ChannelPatch>,
) -> Result<Json<Value>, ApiError> {
    // Merge config so redacted "••••••" placeholders don't wipe stored secrets.
    let merged = match b.config {
        Some(new_cfg) => {
            let existing =
                s.db.get_channel(id)
                    .map_err(internal)?
                    .ok_or_else(|| err(StatusCode::NOT_FOUND, "không có kênh"))?;
            Some(merge_config(existing.config, new_cfg))
        }
        None => None,
    };
    let found =
        s.db.update_channel(id, b.name.as_deref(), merged.as_ref(), b.enabled)
            .map_err(internal)?;
    if !found {
        return Err(err(StatusCode::NOT_FOUND, "không có kênh"));
    }
    let ch = s.db.get_channel(id).map_err(internal)?.unwrap();
    Ok(Json(json!({ "channel": redact_channel(&ch) })))
}

/// Overlay `new` onto `old`, but keep the old value where `new` still holds the
/// redaction placeholder (so re-saving a form doesn't clobber a secret).
fn merge_config(old: Value, new: Value) -> Value {
    let (Value::Object(mut o), Value::Object(n)) = (old, new) else {
        return json!({});
    };
    for (k, v) in n {
        if v.as_str() == Some("••••••") {
            continue;
        }
        o.insert(k, v);
    }
    Value::Object(o)
}

async fn delete_channel(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    if !s.db.delete_channel(id).map_err(internal)? {
        return Err(err(StatusCode::NOT_FOUND, "không có kênh"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn test_channel(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let ch =
        s.db.get_channel(id)
            .map_err(internal)?
            .ok_or_else(|| err(StatusCode::NOT_FOUND, "không có kênh"))?;
    let result = match ch.kind.as_str() {
        "telegram" => crate::channels::telegram::health_check(&ch).await,
        "websocket" => Ok("Web chat luôn sẵn sàng".to_string()),
        "zalo" => {
            if ch
                .config
                .get("access_token")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .is_empty()
            {
                Err("thiếu access_token".to_string())
            } else {
                Ok("đã có access_token (kiểm tra thật khi poll)".to_string())
            }
        }
        "facebook" => {
            if ch
                .config
                .get("page_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .is_empty()
            {
                Err("thiếu page_id/access_token".to_string())
            } else {
                Ok("đã cấu hình (kiểm tra thật khi poll)".to_string())
            }
        }
        "tiktok" => Err("TikTok Shop IM là kênh thử nghiệm".to_string()),
        _ => Err("kind không hỗ trợ".to_string()),
    };
    match result {
        Ok(msg) => Ok(Json(json!({ "ok": true, "message": msg }))),
        Err(e) => Ok(Json(json!({ "ok": false, "message": e }))),
    }
}

// ---- sessions ----

#[derive(Deserialize)]
struct SessionsQuery {
    bot: Option<String>,
    limit: Option<i64>,
}

async fn list_sessions(
    State(s): State<Arc<AppState>>,
    Query(q): Query<SessionsQuery>,
) -> Result<Json<Value>, ApiError> {
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    Ok(Json(
        json!({ "sessions": s.db.list_sessions(q.bot.as_deref(), limit).map_err(internal)? }),
    ))
}

async fn get_session_detail(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let session =
        s.db.get_session(id)
            .map_err(internal)?
            .ok_or_else(|| err(StatusCode::NOT_FOUND, "không có phiên"))?;
    let messages = s.db.list_messages(id, 200).map_err(internal)?;
    Ok(Json(json!({ "session": session, "messages": messages })))
}

async fn delete_session(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    if !s.db.delete_session(id).map_err(internal)? {
        return Err(err(StatusCode::NOT_FOUND, "không có phiên"));
    }
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct ConversationsQuery {
    bot: String,
    kind: Option<String>,
}

/// Conversations for the Chat list — every platform unless `kind` narrows it.
async fn list_conversations(
    State(s): State<Arc<AppState>>,
    Query(q): Query<ConversationsQuery>,
) -> Result<Json<Value>, ApiError> {
    let kind = q.kind.as_deref().map(str::trim).filter(|k| !k.is_empty());
    let convos =
        s.db.list_conversations(&q.bot, kind, 100)
            .map_err(internal)?;
    Ok(Json(json!({ "conversations": convos })))
}

#[derive(Deserialize)]
struct ConversationCreate {
    bot: String,
    /// Which channel (platform) to converse on — from the bot's channel list.
    #[serde(rename = "channelId")]
    channel_id: i64,
    /// Start with a known CRM customer (their platform id is looked up).
    #[serde(rename = "crmCustomerId")]
    crm_customer_id: Option<i64>,
    /// Explicit platform id (guest on a real channel).
    #[serde(rename = "externalId")]
    external_id: Option<String>,
    name: Option<String>,
}

/// Open a NEW conversation on a chosen channel and greet the customer for real:
/// the bot's greeting is persisted as a message and, on non-web channels, sent
/// out over that platform.
async fn create_conversation(
    State(s): State<Arc<AppState>>,
    Json(b): Json<ConversationCreate>,
) -> Result<Json<Value>, ApiError> {
    let bot =
        s.db.get_bot(&b.bot)
            .map_err(internal)?
            .ok_or_else(|| err(StatusCode::NOT_FOUND, "không có bot"))?;
    let ch =
        s.db.get_channel(b.channel_id)
            .map_err(internal)?
            .filter(|c| c.bot_key == bot.key)
            .ok_or_else(|| err(StatusCode::BAD_REQUEST, "kênh không thuộc bot này"))?;
    let web = ch.kind == "websocket";

    // Who are we talking to, and at which platform id?
    let (external_id, name) = match b.crm_customer_id {
        Some(cid) => {
            let base = crate::crm::resolve_base().await;
            let profile = crate::crm::profile_of(&base, cid).await;
            let cname = profile
                .as_ref()
                .and_then(|p| p["name"].as_str())
                .unwrap_or("Khách CRM")
                .to_string();
            if web {
                (format!("crm-{cid}-{}", crate::db::now_ms() % 100000), cname)
            } else {
                // Reach them on this platform → need their stored id for it.
                let chans = crate::crm::customer_channels(&base, cid).await;
                let found = crate::crm::value_for_kind(&chans, &ch.kind);
                match found.or(b.external_id.clone()) {
                    Some(v) => (v, cname),
                    None => {
                        return Err(err(
                            StatusCode::BAD_REQUEST,
                            format!("khách '{cname}' chưa có ID kênh '{}' trong CRM — thêm vào CRM hoặc nhập thủ công", ch.kind),
                        ))
                    }
                }
            }
        }
        None => {
            let n = b.name.clone().unwrap_or_else(|| "Khách web".to_string());
            match (web, b.external_id.clone()) {
                (true, _) => (format!("web-{}", crate::db::now_ms() % 1000000), n),
                (false, Some(v)) if !v.trim().is_empty() => (v, n),
                (false, _) => {
                    return Err(err(
                        StatusCode::BAD_REQUEST,
                        format!("cần ID của khách trên kênh '{}'", ch.kind),
                    ))
                }
            }
        }
    };

    let session =
        s.db.get_or_create_session(
            &bot.key,
            &ch.kind,
            ch.id,
            &external_id,
            &format!("{}:{external_id}", ch.kind),
            &name,
        )
        .map_err(internal)?;

    // Greet: persist it, and actually deliver it on real channels (the web
    // client replays it from the transcript when its socket connects).
    let greeting = if bot.greeting.trim().is_empty() {
        "Xin chào 👋".to_string()
    } else {
        bot.greeting.clone()
    };
    if s.db
        .list_messages(session.id, 1)
        .map_err(internal)?
        .is_empty()
    {
        if !web {
            if let Err(e) = s.channels.send_raw(&ch, &external_id, &greeting).await {
                return Err(err(
                    StatusCode::BAD_GATEWAY,
                    format!("không gửi được lời chào: {e}"),
                ));
            }
        }
        let _ = s.db.add_message(session.id, "assistant", &greeting);
        engine::emit(
            &s.events,
            json!({ "type": "message", "sessionId": session.id, "role": "assistant", "content": greeting }),
        );
    }
    Ok(Json(json!({
        "sessionId": session.id, "externalId": external_id,
        "channelKind": ch.kind, "customerName": name,
    })))
}

#[derive(Deserialize)]
struct SendBody {
    text: String,
}

/// Send an outbound message to the customer of a conversation (used by the Chat
/// page for non-web channels, where you speak to them as the shop).
async fn conversation_send(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(b): Json<SendBody>,
) -> Result<Json<Value>, ApiError> {
    let text = b.text.trim();
    if text.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "nội dung trống"));
    }
    let session =
        s.db.get_session(id)
            .map_err(internal)?
            .ok_or_else(|| err(StatusCode::NOT_FOUND, "không có phiên"))?;
    s.channels
        .send_to_session(&session, text)
        .await
        .map_err(|e| err(StatusCode::BAD_GATEWAY, e))?;
    s.db.add_message(id, "operator", text).map_err(internal)?;
    engine::emit(
        &s.events,
        json!({ "type": "message", "sessionId": id, "role": "operator", "content": text }),
    );
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct CrmSearchQuery {
    q: Option<String>,
    /// Annotate each customer with whether they're reachable on this channel.
    channel: Option<String>,
}

/// Proxy the CRM customer search/list for the "new conversation" picker.
async fn crm_search(
    State(_s): State<Arc<AppState>>,
    Query(q): Query<CrmSearchQuery>,
) -> Json<Value> {
    let base = crate::crm::resolve_base().await;
    let list = crate::crm::search_list_for_channel(
        &base,
        q.q.as_deref().unwrap_or(""),
        q.channel.as_deref(),
    )
    .await;
    Json(json!({ "customers": list }))
}

// ---- issues (support tickets) ----

#[derive(Deserialize)]
struct IssueQuery {
    status: Option<String>,
    priority: Option<String>,
    bot: Option<String>,
    search: Option<String>,
    limit: Option<i64>,
}

async fn list_issues(
    State(s): State<Arc<AppState>>,
    Query(q): Query<IssueQuery>,
) -> Result<Json<Value>, ApiError> {
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let issues =
        s.db.list_issues(
            q.status.as_deref(),
            q.priority.as_deref(),
            q.bot.as_deref(),
            q.search.as_deref(),
            limit,
        )
        .map_err(internal)?;
    Ok(Json(json!({ "issues": issues })))
}

#[derive(Deserialize)]
struct IssueCreate {
    #[serde(rename = "botKey")]
    bot_key: Option<String>,
    #[serde(rename = "sessionId")]
    session_id: Option<i64>,
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    priority: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    sentiment: String,
    #[serde(default)]
    tags: Vec<String>,
}

async fn create_issue(
    State(s): State<Arc<AppState>>,
    Json(b): Json<IssueCreate>,
) -> Result<Json<Value>, ApiError> {
    if b.title.trim().is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "thiếu tiêu đề"));
    }
    // Resolve bot/external id from the session when given.
    let (bot_key, external_id) = match b
        .session_id
        .and_then(|id| s.db.get_session(id).ok().flatten())
    {
        Some(sess) => (sess.bot_key, sess.external_id),
        None => (b.bot_key.unwrap_or_default(), String::new()),
    };
    let issue =
        s.db.create_issue(
            b.session_id,
            &bot_key,
            &external_id,
            &b.title,
            &b.description,
            if b.priority.is_empty() {
                "medium"
            } else {
                &b.priority
            },
            &b.category,
            &b.sentiment,
            "",
            &b.tags,
        )
        .map_err(internal)?;
    engine::emit(
        &s.events,
        json!({ "type": "issue", "issueId": issue.id, "title": issue.title, "priority": issue.priority }),
    );
    Ok(Json(json!({ "issue": issue })))
}

async fn get_issue(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let issue =
        s.db.get_issue(id)
            .map_err(internal)?
            .ok_or_else(|| err(StatusCode::NOT_FOUND, "không có ticket"))?;
    let events = s.db.list_issue_events(id).map_err(internal)?;
    Ok(Json(json!({ "issue": issue, "events": events })))
}

#[derive(Deserialize)]
struct IssuePatchBody {
    status: Option<String>,
    priority: Option<String>,
    category: Option<String>,
    assignee: Option<String>,
    resolution_note: Option<String>,
    title: Option<String>,
}

async fn update_issue(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(b): Json<IssuePatchBody>,
) -> Result<Json<Value>, ApiError> {
    let patch = crate::db::IssuePatch {
        status: b.status,
        priority: b.priority,
        category: b.category,
        assignee: b.assignee,
        resolution_note: b.resolution_note,
        title: b.title,
    };
    if !s
        .db
        .update_issue(id, &patch, "operator")
        .map_err(internal)?
    {
        return Err(err(StatusCode::NOT_FOUND, "không có ticket"));
    }
    engine::emit(&s.events, json!({ "type": "issue-updated", "issueId": id }));
    Ok(Json(
        json!({ "issue": s.db.get_issue(id).map_err(internal)? }),
    ))
}

async fn analytics(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(s.db.analytics().map_err(internal)?))
}

/// AI quality analysis of one conversation (sentiment / score / summary).
async fn analyze_session(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    if s.db.get_session(id).map_err(internal)?.is_none() {
        return Err(err(StatusCode::NOT_FOUND, "không có phiên"));
    }
    match engine::analyze_session(&s.db, id).await {
        Ok(v) => Ok(Json(v)),
        Err(e) => Err(err(StatusCode::BAD_GATEWAY, e)),
    }
}

// ---- chat (REST tester) ----

#[derive(Deserialize)]
struct ChatBody {
    bot: String,
    #[serde(rename = "externalId")]
    external_id: Option<String>,
    /// Optional display name (lets an integration/tester drive CRM matching).
    name: Option<String>,
    text: String,
}

/// Simple request/response chat over REST (used by the web tester as a
/// fallback to the WebSocket). Uses the bot's web channel.
async fn chat(
    State(s): State<Arc<AppState>>,
    Json(b): Json<ChatBody>,
) -> Result<Json<Value>, ApiError> {
    let bot =
        s.db.get_bot(&b.bot)
            .map_err(internal)?
            .ok_or_else(|| err(StatusCode::NOT_FOUND, "không có bot"))?;
    let external_id = b
        .external_id
        .unwrap_or_else(|| format!("web-{}", crate::db::now_ms()));
    let ch = ensure_ws_channel(&s.db, &bot.key).map_err(internal)?;
    let cust_name = b
        .name
        .as_deref()
        .filter(|n| !n.trim().is_empty())
        .unwrap_or("Khách web");
    let session =
        s.db.get_or_create_session(
            &bot.key,
            "websocket",
            ch.id,
            &external_id,
            &format!("web:{external_id}"),
            cust_name,
        )
        .map_err(internal)?;
    let outcome = engine::process_inbound(&s.db, &s.events, &bot, &session, &b.text).await;
    Ok(Json(json!({
        "sessionId": session.id,
        "externalId": external_id,
        "reply": outcome.reply,
        "escalated": outcome.escalated,
    })))
}

/// Find (or create) the WebSocket channel a bot uses for web chat.
fn ensure_ws_channel(db: &Arc<Db>, bot_key: &str) -> anyhow::Result<crate::db::Channel> {
    let channels = db.list_channels(Some(bot_key))?;
    if let Some(ch) = channels.into_iter().find(|c| c.kind == "websocket") {
        return Ok(ch);
    }
    db.create_channel(bot_key, "websocket", "Web chat", &json!({}))
}

// ---- WebSocket live chat ----

#[derive(Deserialize)]
struct WsQuery {
    bot: Option<String>,
    /// Display name for a new conversation (e.g. a CRM customer's name).
    name: Option<String>,
}

async fn ws_chat(
    State(s): State<Arc<AppState>>,
    Path(external_id): Path<String>,
    Query(q): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> Response {
    let bot_key = q.bot.unwrap_or_else(|| "support".to_string());
    let name = q.name.filter(|n| !n.trim().is_empty());
    ws.on_upgrade(move |socket| ws_chat_loop(s, external_id, bot_key, name, socket))
}

async fn ws_chat_loop(
    s: Arc<AppState>,
    external_id: String,
    bot_key: String,
    name: Option<String>,
    socket: WebSocket,
) {
    let Some(bot) = s.db.get_bot(&bot_key).ok().flatten() else {
        return;
    };
    let Ok(ch) = ensure_ws_channel(&s.db, &bot.key) else {
        return;
    };
    let Ok(session) = s.db.get_or_create_session(
        &bot.key,
        "websocket",
        ch.id,
        &external_id,
        &format!("web:{external_id}"),
        name.as_deref().unwrap_or("Khách web"),
    ) else {
        return;
    };
    let session_id = session.id;

    let (mut tx, mut rx) = socket.split();
    // On connect: replay the transcript if this conversation already has one
    // (resuming), otherwise greet (a brand-new conversation).
    let past = s.db.list_messages(session_id, 200).unwrap_or_default();
    if past.is_empty() {
        let greeting = if bot.greeting.trim().is_empty() {
            "Xin chào 👋".to_string()
        } else {
            bot.greeting.clone()
        };
        let _ = tx
            .send(WsMessage::Text(
                json!({ "type": "chat_response", "text": greeting }).to_string(),
            ))
            .await;
    } else {
        let msgs: Vec<Value> = past
            .iter()
            .map(|m| json!({ "role": m.role, "content": m.content }))
            .collect();
        let _ = tx
            .send(WsMessage::Text(
                json!({ "type": "history", "messages": msgs }).to_string(),
            ))
            .await;
    }

    // One loop owns the socket sender: it both answers inbound frames inline
    // and relays operator/handoff events for THIS session (e.g. a human took
    // over the chat from the Support Inbox). Bot replies are sent directly, so
    // the event relay skips assistant/user echoes to avoid duplicates.
    let mut events_rx = s.events.subscribe();
    loop {
        tokio::select! {
            incoming = rx.next() => {
                let Some(Ok(msg)) = incoming else { break };
                let text = match msg {
                    WsMessage::Text(t) => t,
                    WsMessage::Close(_) => break,
                    WsMessage::Ping(_) | WsMessage::Pong(_) | WsMessage::Binary(_) => continue,
                };
                let parsed: Value = serde_json::from_str(&text).unwrap_or(json!({ "text": text }));
                if parsed["type"] == "context" {
                    if let Some(ctx) = parsed.get("context") {
                        let _ = s.db.set_session_context(session_id, ctx);
                    }
                    continue;
                }
                let user_text = parsed["text"].as_str().unwrap_or("").trim().to_string();
                if user_text.is_empty() {
                    continue;
                }
                let Some(session) = s.db.get_session(session_id).ok().flatten() else { break };
                let outcome = engine::process_inbound(&s.db, &s.events, &bot, &session, &user_text).await;
                if let Some(reply) = outcome.reply.filter(|r| !r.trim().is_empty()) {
                    let frame = json!({ "type": "chat_response", "text": reply });
                    if tx.send(WsMessage::Text(frame.to_string())).await.is_err() {
                        break;
                    }
                }
            }
            ev = events_rx.recv() => {
                let raw = match ev {
                    Ok(raw) => raw,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                };
                let Ok(ev) = serde_json::from_str::<Value>(&raw) else { continue };
                if ev["sessionId"].as_i64() != Some(session_id) {
                    continue;
                }
                let is_operator = ev["type"] == "message" && ev["role"] == "operator";
                let is_handoff = ev["type"] == "handoff";
                if is_operator || is_handoff {
                    let out = json!({
                        "type": if is_handoff { "handoff" } else { "operator_message" },
                        "text": ev["content"], "state": ev["state"],
                    });
                    if tx.send(WsMessage::Text(out.to_string())).await.is_err() {
                        break;
                    }
                }
            }
        }
    }
}

// ---- live event stream (Support Inbox) ----

async fn events_sse(
    State(s): State<Arc<AppState>>,
) -> axum::response::Sse<
    impl futures_util::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>,
> {
    use axum::response::sse::Event;
    let mut rx = s.events.subscribe();
    let stream = async_stream::stream! {
        loop {
            match rx.recv().await {
                Ok(msg) => yield Ok(Event::default().data(msg)),
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    };
    axum::response::Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default())
}

// ---- handoff ----

#[derive(Deserialize)]
struct HandoffBody {
    state: String,
}

async fn set_handoff(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(b): Json<HandoffBody>,
) -> Result<Json<Value>, ApiError> {
    if !["bot", "pending", "with_operator"].contains(&b.state.as_str()) {
        return Err(err(StatusCode::BAD_REQUEST, "state không hợp lệ"));
    }
    s.db.set_handoff(id, &b.state).map_err(internal)?;
    engine::emit(
        &s.events,
        json!({ "type": "handoff", "sessionId": id, "state": b.state }),
    );
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct ReplyBody {
    text: String,
}

/// Operator reply into a handed-off session: persist, deliver over the channel,
/// and emit for the live UI.
async fn handoff_reply(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(b): Json<ReplyBody>,
) -> Result<Json<Value>, ApiError> {
    let text = b.text.trim();
    if text.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "nội dung trống"));
    }
    let session: Session =
        s.db.get_session(id)
            .map_err(internal)?
            .ok_or_else(|| err(StatusCode::NOT_FOUND, "không có phiên"))?;
    if session.handoff_state == HANDOFF_BOT {
        s.db.set_handoff(id, "with_operator").map_err(internal)?;
    }
    s.db.add_message(id, "operator", text).map_err(internal)?;
    engine::emit(
        &s.events,
        json!({ "type": "message", "sessionId": id, "role": "operator", "content": text }),
    );
    if let Err(e) = s.channels.send_to_session(&session, text).await {
        return Err(err(StatusCode::BAD_GATEWAY, e));
    }
    Ok(Json(json!({ "ok": true })))
}

// ---- knowledge ----

#[derive(Deserialize)]
struct KnowledgeQuery {
    bot: String,
    q: Option<String>,
}

async fn knowledge_search(
    State(s): State<Arc<AppState>>,
    Query(q): Query<KnowledgeQuery>,
) -> Result<Json<Value>, ApiError> {
    let bot =
        s.db.get_bot(&q.bot)
            .map_err(internal)?
            .ok_or_else(|| err(StatusCode::NOT_FOUND, "không có bot"))?;
    let space = format!("ai-chat:{}", bot.key);
    let query = q.q.unwrap_or_default();
    if query.trim().is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "thiếu q"));
    }
    match senclaw::knowledge_search(&space, &query, 10).await {
        Ok(v) => Ok(Json(v)),
        Err(e) => Err(err(StatusCode::BAD_GATEWAY, e)),
    }
}

#[derive(Deserialize)]
struct KnowledgeWrite {
    #[serde(rename = "botKey")]
    bot_key: String,
    text: String,
    #[serde(default)]
    wiki: bool,
}

async fn knowledge_write(
    State(s): State<Arc<AppState>>,
    Json(b): Json<KnowledgeWrite>,
) -> Result<Json<Value>, ApiError> {
    let bot =
        s.db.get_bot(&b.bot_key)
            .map_err(internal)?
            .ok_or_else(|| err(StatusCode::NOT_FOUND, "không có bot"))?;
    if b.text.trim().is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "nội dung trống"));
    }
    let space = format!("ai-chat:{}", bot.key);
    senclaw::knowledge_save(&space, b.text.trim(), "ai-chat:knowledge")
        .await
        .map_err(|e| err(StatusCode::BAD_GATEWAY, e))?;
    if b.wiki {
        let path = format!("ai-chat/{}/{}.md", bot.key, crate::db::now_ms());
        let _ = senclaw::wiki_write(&path, b.text.trim(), "ai-chat knowledge").await;
    }
    Ok(Json(json!({ "ok": true, "space": space })))
}

/// Upload a knowledge file for a bot (multipart: `bot` + the file field). The
/// daemon extracts text (pdf/docx/txt/md/…) and cognifies into the bot's space.
async fn knowledge_upload(
    State(s): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Json<Value>, ApiError> {
    let mut bot_key: Option<String> = None;
    let mut file: Option<(String, String, Vec<u8>)> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| err(StatusCode::BAD_REQUEST, format!("đọc file lỗi: {e}")))?
    {
        match field.name().unwrap_or("") {
            "bot" | "botKey" => bot_key = field.text().await.ok().filter(|s| !s.is_empty()),
            _ => {
                let fname = field.file_name().unwrap_or("upload.txt").to_string();
                let ctype = field.content_type().unwrap_or("").to_string();
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|e| err(StatusCode::BAD_REQUEST, format!("đọc file lỗi: {e}")))?;
                file = Some((fname, ctype, bytes.to_vec()));
            }
        }
    }
    let bot = bot_key
        .and_then(|k| s.db.get_bot(&k).ok().flatten())
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "thiếu hoặc sai 'bot'"))?;
    let (fname, ctype, bytes) = file.ok_or_else(|| err(StatusCode::BAD_REQUEST, "thiếu file"))?;
    let space = format!("ai-chat:{}", bot.key);
    match senclaw::knowledge_upload(&space, &fname, &ctype, bytes).await {
        Ok(v) => Ok(Json(
            json!({ "ok": true, "space": space, "filename": fname, "report": v }),
        )),
        Err(e) => Err(err(StatusCode::BAD_GATEWAY, e)),
    }
}

async fn knowledge_nodes(
    State(s): State<Arc<AppState>>,
    Query(q): Query<KnowledgeQuery>,
) -> Result<Json<Value>, ApiError> {
    let bot =
        s.db.get_bot(&q.bot)
            .map_err(internal)?
            .ok_or_else(|| err(StatusCode::NOT_FOUND, "không có bot"))?;
    let space = format!("ai-chat:{}", bot.key);
    match senclaw::knowledge_nodes(&space, 100).await {
        Ok(v) => Ok(Json(v)),
        Err(e) => Err(err(StatusCode::BAD_GATEWAY, e)),
    }
}

// ---- inventories + settings ----

async fn skills_inventory() -> Json<Value> {
    Json(senclaw::skills_inventory_grouped().await)
}
async fn mcp_inventory() -> Json<Value> {
    Json(senclaw::mcp_inventory().await)
}

async fn get_settings(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    // crmBase is auto-discovered from the daemon's installed Space Apps —
    // read-only info for the UI, never entered by hand.
    Ok(Json(json!({
        "features": s.db.features_json(),
        "language": s.db.get_setting("language").map_err(internal)?.unwrap_or_else(|| "vi".into()),
        "crmEnabled": s.db.get_setting("crm_enabled").map_err(internal)?.map(|v| v != "0").unwrap_or(true),
        "crmBase": crate::crm::resolve_base().await,
    })))
}

#[derive(Deserialize)]
struct SettingsPatch {
    features: Option<std::collections::HashMap<String, bool>>,
    language: Option<String>,
    #[serde(rename = "crmEnabled")]
    crm_enabled: Option<bool>,
}

async fn update_settings(
    State(s): State<Arc<AppState>>,
    Json(b): Json<SettingsPatch>,
) -> Result<Json<Value>, ApiError> {
    if let Some(feats) = b.features {
        for (k, v) in feats {
            if ["knowledge", "wiki", "tools"].contains(&k.as_str()) {
                s.db.set_setting(&format!("feat_{k}"), if v { "1" } else { "0" })
                    .map_err(internal)?;
            }
        }
    }
    if let Some(lang) = b.language.filter(|l| ["vi", "en"].contains(&l.as_str())) {
        s.db.set_setting("language", &lang).map_err(internal)?;
    }
    if let Some(v) = b.crm_enabled {
        s.db.set_setting("crm_enabled", if v { "1" } else { "0" })
            .map_err(internal)?;
    }
    get_settings(State(s)).await
}
