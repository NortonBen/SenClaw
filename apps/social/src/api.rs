//! REST API. Paths are registered without the `/api` prefix; `main.rs` nests them.

use crate::mcp::{mcp_message, mcp_sse};
use crate::state::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use serde_json::{json, Value};

fn respond(v: Value) -> Response {
    Json(v).into_response()
}

fn err(status: StatusCode, msg: impl Into<String>) -> Response {
    (status, Json(json!({ "error": msg.into() }))).into_response()
}

pub fn root_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .with_state(state)
}

pub fn api_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/status", get(status))
        .route("/settings", get(get_settings).put(put_settings))
        .route("/accounts", get(list_accounts).post(connect_account))
        .route("/accounts/:id", delete(delete_account))
        .route("/logs", get(post_logs))
        .route("/inbox", get(inbox))
        .route("/inbox/reply", post(inbox_reply))
        .route("/compose", post(compose))
        .route("/drafts", get(drafts))
        .route("/drafts/:id/approve", post(approve_draft))
        .route("/drafts/:id/reject", post(reject_draft))
        .route("/actions", get(actions))
        .route("/sessions", get(sessions))
        .route("/ext/status", get(ext_status))
        .route("/ext/login", post(ext_login))
        .route("/ext/whoami", post(ext_whoami))
        .route("/ext/fb-template", get(ext_fb_template))
        .route("/ext/fb-test", post(ext_fb_test))
        .route("/ext/callback", post(ext_callback))
        .route("/mcp/sse", get(mcp_sse).post(mcp_message))
        .route("/mcp/message", post(mcp_message))
        .with_state(state)
}

async fn health() -> Response {
    respond(json!({ "ok": true }))
}

async fn status(State(state): State<AppState>) -> Response {
    let db = &state.core.db;
    let pending = db
        .list_drafts(Some("pending"), 1000)
        .map(|d| d.len())
        .unwrap_or(0);
    let mut caps = serde_json::Map::new();
    for p in crate::channels::Platform::ALL {
        let mut m = serde_json::Map::new();
        for c in crate::channels::Platform::CAPS {
            m.insert(c.to_string(), json!(p.capability(c).as_str()));
        }
        m.insert("note".into(), json!(p.official_note()));
        caps.insert(p.as_str().to_string(), Value::Object(m));
    }
    respond(json!({
        "ok": true,
        "app": "social",
        "platforms": crate::channels::Platform::ALL.iter().map(|p| p.as_str()).collect::<Vec<_>>(),
        "capabilities": caps,
        // Real ports this process is using — the UI must not hardcode them.
        "port": crate::config::http_port(),
        "ext_ws_port": crate::config::ext_ws_port(),
        "autonomy": db.autonomy(),
        "accounts": db.account_count(),
        "drafts_pending": pending,
        "posts_logged": db.recent_posts(100000).map(|p| p.len()).unwrap_or(0),
        "actions_logged": db.recent_actions(100000).map(|a| a.len()).unwrap_or(0),
        "extension_connected": state.ext.is_connected(),
        "extension_hosts_ready": state.ext.hosts_ready(),
        "fb_composer_ready": state.ext.fb_composer_ready(),
        "extension_uptime_s": state.ext.stats().get("uptime_s").cloned().unwrap_or(json!(0)),
        // Identity of the Chrome extension remotely driving this app.
        "extension_name": state.ext.ext_info().map(|i| i.name).unwrap_or_default(),
        "extension_version": state.ext.ext_info().map(|i| i.version).unwrap_or_default(),
        "extension_label": state.ext.ext_label(),
    }))
}

async fn get_settings(State(state): State<AppState>) -> Response {
    match state.core.db.all_settings() {
        Ok(pairs) => {
            let map: serde_json::Map<String, Value> =
                pairs.into_iter().map(|(k, v)| (k, json!(v))).collect();
            respond(json!({ "settings": map }))
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn put_settings(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    if let Some(obj) = body.as_object() {
        for (k, v) in obj {
            let val = v
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| v.to_string());
            if let Err(e) = state.core.db.set_setting(k, &val) {
                return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
            }
        }
    }
    respond(json!({ "ok": true }))
}

async fn list_accounts(State(state): State<AppState>) -> Response {
    match state.core.db.list_accounts() {
        Ok(accounts) => respond(json!({ "accounts": accounts })),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn connect_account(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let platform = body.get("platform").and_then(|v| v.as_str()).unwrap_or("");
    let handle = body.get("handle").and_then(|v| v.as_str()).unwrap_or("");
    if crate::channels::Platform::parse(platform).is_none() {
        return err(StatusCode::BAD_REQUEST, "platform không hợp lệ");
    }
    if handle.is_empty() {
        return err(StatusCode::BAD_REQUEST, "thiếu handle");
    }
    let display = body
        .get("display_name")
        .and_then(|v| v.as_str())
        .unwrap_or(handle);
    let cfg = body.get("official_config").cloned().unwrap_or(json!({}));
    match state
        .core
        .db
        .upsert_account(platform, handle, display, &cfg)
    {
        Ok(id) => respond(json!({ "ok": true, "id": id })),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn delete_account(State(state): State<AppState>, Path(id): Path<i64>) -> Response {
    match state.core.db.delete_account(id) {
        Ok(()) => respond(json!({ "ok": true })),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn post_logs(State(state): State<AppState>) -> Response {
    respond(json!({ "posts": state.core.db.recent_posts(50).unwrap_or_default() }))
}

async fn inbox(
    State(state): State<AppState>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> Response {
    // With ?since=<id> this is the cursor feed for external pullers (CRM):
    // inbound-only, id-ascending. Without it, the recent view for the UI.
    if let Some(since) = q.get("since").and_then(|s| s.parse::<i64>().ok()) {
        let limit = q
            .get("limit")
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(200)
            .clamp(1, 500);
        return respond(
            json!({ "messages": state.core.db.inbox_since(since, limit).unwrap_or_default() }),
        );
    }
    respond(json!({ "messages": state.core.db.list_inbox(None, 50).unwrap_or_default() }))
}

/// Send an operator reply into a conversation. Used by CRM's `social` channel
/// adapter (and any operator UI). Routes through the autonomy gate + cadence,
/// exactly like `social_send_dm`, so it respects the app's draft/live mode.
async fn inbox_reply(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let platform = body.get("platform").and_then(|v| v.as_str()).unwrap_or("");
    let handle = body.get("handle").and_then(|v| v.as_str()).unwrap_or("");
    let external_id = body
        .get("external_id")
        .or_else(|| body.get("thread_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let text = body.get("text").and_then(|v| v.as_str()).unwrap_or("");
    let Some(p) = crate::channels::Platform::parse(platform) else {
        return err(StatusCode::BAD_REQUEST, "platform không hợp lệ");
    };
    if external_id.is_empty() || text.is_empty() {
        return err(StatusCode::BAD_REQUEST, "cần external_id và text");
    }
    match crate::gate::submit(&state, "reply", p, handle, text, external_id, &json!([])).await {
        Ok(v) => respond(v),
        Err(e) => err(StatusCode::BAD_GATEWAY, e),
    }
}

/// Compose a post or a DM directly from the UI. Routes through the autonomy
/// gate exactly like the agent's `social_post`/`social_send_dm`: in draft mode
/// it records a pending draft; in live mode it sends (via cadence + audit).
/// Body: `{ platform, handle, kind: "post"|"dm", text, thread_id? }`.
async fn compose(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let platform = body.get("platform").and_then(|v| v.as_str()).unwrap_or("");
    let handle = body.get("handle").and_then(|v| v.as_str()).unwrap_or("");
    let text = body.get("text").and_then(|v| v.as_str()).unwrap_or("");
    let thread_id = body
        .get("thread_id")
        .or_else(|| body.get("external_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let Some(p) = crate::channels::Platform::parse(platform) else {
        return err(StatusCode::BAD_REQUEST, "platform không hợp lệ");
    };
    if handle.is_empty() {
        return err(StatusCode::BAD_REQUEST, "thiếu handle");
    }
    if text.trim().is_empty() {
        return err(StatusCode::BAD_REQUEST, "thiếu nội dung");
    }
    // "dm"/"reply" both mean the extension DM path (needs a thread/recipient).
    let kind = match body.get("kind").and_then(|v| v.as_str()).unwrap_or("post") {
        "dm" | "reply" => "reply",
        _ => "post",
    };
    if kind == "reply" && thread_id.is_empty() {
        return err(
            StatusCode::BAD_REQUEST,
            "nhắn tin cần thread_id (người nhận / cuộc trò chuyện)",
        );
    }
    // Media: a JSON array of image data URLs. Cap the count + total size so a
    // stray upload can't bloat the drafts table.
    let media = body.get("media").cloned().unwrap_or_else(|| json!([]));
    if let Some(arr) = media.as_array() {
        if arr.len() > 4 {
            return err(StatusCode::BAD_REQUEST, "tối đa 4 ảnh");
        }
        let total: usize = arr.iter().filter_map(|v| v.as_str()).map(|s| s.len()).sum();
        if total > 24_000_000 {
            return err(
                StatusCode::BAD_REQUEST,
                "tổng dung lượng ảnh quá lớn (>~18MB)",
            );
        }
    } else {
        return err(StatusCode::BAD_REQUEST, "media phải là mảng");
    }
    match crate::gate::submit(&state, kind, p, handle, text, thread_id, &media).await {
        Ok(v) => respond(v),
        Err(e) => err(StatusCode::BAD_GATEWAY, e),
    }
}

async fn drafts(State(state): State<AppState>) -> Response {
    respond(json!({ "drafts": state.core.db.list_drafts(None, 100).unwrap_or_default() }))
}

async fn approve_draft(State(state): State<AppState>, Path(id): Path<i64>) -> Response {
    let draft = match state.core.db.get_draft(id) {
        Ok(Some(d)) => d,
        _ => return err(StatusCode::NOT_FOUND, format!("không thấy nháp #{id}")),
    };
    if draft["status"] != "pending" {
        return err(StatusCode::BAD_REQUEST, "nháp không còn pending");
    }
    let platform = match crate::channels::Platform::parse(draft["platform"].as_str().unwrap_or(""))
    {
        Some(p) => p,
        None => return err(StatusCode::BAD_REQUEST, "platform không hợp lệ"),
    };
    let handle = draft["handle"].as_str().unwrap_or("");
    let kind = draft["kind"].as_str().unwrap_or("post");
    let text = draft["text"].as_str().unwrap_or("");
    let thread_id = draft["thread_id"].as_str().unwrap_or("");
    match crate::gate::execute_write(&state, kind, platform, handle, text, thread_id).await {
        Ok(ref_id) => {
            let _ = state.core.db.set_draft_status(id, "sent", &ref_id, "");
            respond(json!({ "ok": true, "ref_id": ref_id }))
        }
        Err(e) => {
            let _ = state.core.db.set_draft_status(id, "pending", "", &e);
            err(StatusCode::BAD_GATEWAY, e)
        }
    }
}

async fn reject_draft(State(state): State<AppState>, Path(id): Path<i64>) -> Response {
    match state
        .core
        .db
        .set_draft_status(id, "rejected", "", "rejected by user")
    {
        Ok(()) => respond(json!({ "ok": true })),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn actions(State(state): State<AppState>) -> Response {
    respond(json!({ "actions": state.core.db.recent_actions(100).unwrap_or_default() }))
}

async fn sessions(State(state): State<AppState>) -> Response {
    respond(json!({ "sessions": state.core.db.recent_sessions(100).unwrap_or_default() }))
}

async fn ext_status(State(state): State<AppState>) -> Response {
    respond(state.ext.stats())
}

/// Validate `platform` and ensure the extension is connected. Returns the
/// canonical platform id on success, or a ready-made error response.
fn require_ext_platform(state: &AppState, body: &Value) -> Result<String, Response> {
    let platform = body.get("platform").and_then(|v| v.as_str()).unwrap_or("");
    let Some(p) = crate::channels::Platform::parse(platform) else {
        return Err(err(StatusCode::BAD_REQUEST, "platform không hợp lệ"));
    };
    if !state.ext.is_connected() {
        return Err(err(StatusCode::BAD_GATEWAY, "extension chưa kết nối"));
    }
    Ok(p.as_str().to_string())
}

/// Ask the extension to open the platform's login page in the user's Chrome.
/// The user signs in there; the app never handles credentials.
async fn ext_login(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let platform = match require_ext_platform(&state, &body) {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    match state
        .ext
        .call(
            "OpenLogin",
            json!({ "platform": platform }),
            std::time::Duration::from_secs(15),
        )
        .await
    {
        Ok(v) if v.get("error").is_none() => {
            state
                .core
                .db
                .log_action(&platform, "open_login", "ok", &state.ext.ext_label());
            respond(json!({ "ok": true, "result": v }))
        }
        Ok(v) => {
            let msg = v
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("lỗi extension");
            state
                .core
                .db
                .log_action(&platform, "open_login", "error", msg);
            err(StatusCode::BAD_GATEWAY, msg)
        }
        Err(e) => {
            state
                .core
                .db
                .log_action(&platform, "open_login", "error", &e);
            err(StatusCode::BAD_GATEWAY, e)
        }
    }
}

/// Ask the extension whether a platform has a live session and who is logged in.
/// Returns `{ logged_in, handle?, name?, id? }`. The UI polls this after opening
/// the login tab, then prefills the account form for the operator to confirm.
async fn ext_whoami(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let platform = match require_ext_platform(&state, &body) {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    match state
        .ext
        .call(
            "WhoAmI",
            json!({ "platform": platform }),
            std::time::Duration::from_secs(15),
        )
        .await
    {
        Ok(mut v) if v.get("error").is_none() => {
            // The extension answers `{ id, result }`; unwrap the result payload.
            let payload = v.get_mut("result").map(|r| r.take()).unwrap_or(v);
            respond(payload)
        }
        Ok(v) => {
            let msg = v
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("lỗi extension");
            state.core.db.log_action(&platform, "whoami", "error", msg);
            err(StatusCode::BAD_GATEWAY, msg)
        }
        Err(e) => {
            state.core.db.log_action(&platform, "whoami", "error", &e);
            err(StatusCode::BAD_GATEWAY, e)
        }
    }
}

/// Diagnostics: what Facebook composer template the extension has learned.
async fn ext_fb_template(State(state): State<AppState>) -> Response {
    if !state.ext.is_connected() {
        return err(StatusCode::BAD_GATEWAY, "extension chưa kết nối");
    }
    match state
        .ext
        .call(
            "GetFbTemplate",
            json!({}),
            std::time::Duration::from_secs(10),
        )
        .await
    {
        Ok(v) => respond(v.get("result").cloned().unwrap_or(v)),
        Err(e) => err(StatusCode::BAD_GATEWAY, e),
    }
}

/// Diagnostics: fire the FB GraphQL replay once and return the raw outcome
/// (FB's real response/error). Publishes a real post — for debugging.
async fn ext_fb_test(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    if !state.ext.is_connected() {
        return err(StatusCode::BAD_GATEWAY, "extension chưa kết nối");
    }
    let text = body.get("text").and_then(|v| v.as_str()).unwrap_or("");
    match state
        .ext
        .call(
            "FbTestPost",
            json!({ "text": text }),
            std::time::Duration::from_secs(30),
        )
        .await
    {
        Ok(v) => respond(v.get("result").cloned().unwrap_or(v)),
        Err(e) => err(StatusCode::BAD_GATEWAY, e),
    }
}

/// HTTP fallback path for the extension to answer a callback (besides WS).
/// Body: `{ "secret": "...", "id": "...", ... }`. The secret must match the one
/// handed to the extension on connect.
async fn ext_callback(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let secret = body.get("secret").and_then(|v| v.as_str()).unwrap_or("");
    if secret != state.ext.secret() {
        return err(StatusCode::UNAUTHORIZED, "sai secret");
    }
    let id = body
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if id.is_empty() {
        return err(StatusCode::BAD_REQUEST, "thiếu id");
    }
    let delivered = state.ext.complete_callback(&id, body);
    respond(json!({ "ok": delivered }))
}
