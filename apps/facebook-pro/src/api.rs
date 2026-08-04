//! HTTP API for the Facebook Pro app. Draft-first: creating a post/comment/reply
//! queues a draft; only `POST /drafts/:id/approve` (or `autonomy=live`) actually
//! calls the Graph API to publish. The MCP server ([`crate::mcp`]) and the
//! heartbeat ([`crate::engine`]) reuse the same helpers so an agent can never
//! bypass the human-approval default.

use crate::db::{Db, DraftInput, TriggerInput};
use crate::fb::{Client, Config};
use crate::llm;
use app_space_sdk::SpaceClient;
use axum::{
    extract::{Multipart, Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Db>,
    pub sc: SpaceClient,
    /// Fan-out of MCP JSON-RPC responses to any connected SSE client.
    pub mcp_tx: tokio::sync::broadcast::Sender<String>,
}

pub fn make_state() -> AppState {
    let db = Arc::new(Db::open_default().expect("open facebook-pro db"));
    let (mcp_tx, _) = tokio::sync::broadcast::channel(100);
    AppState {
        db,
        sc: SpaceClient::from_env(),
        mcp_tx,
    }
}

/// Build a Graph client from the stored developer-app credentials, or `None` if
/// app_id/app_secret aren't configured yet.
pub(crate) fn client_from_settings(db: &Db) -> Option<Client> {
    let app_id = db.get_setting("app_id").filter(|s| !s.is_empty())?;
    let app_secret = db.get_setting("app_secret").filter(|s| !s.is_empty())?;
    Some(Client::new(Config {
        app_id,
        app_secret,
        version: db.version(),
    }))
}

/// Resolve the target Page (explicit `page_id` or the active one) to a signed
/// client + its Page Access Token.
pub(crate) fn resolve_page(
    db: &Db,
    page_id: Option<&str>,
) -> Result<(Client, String, String), String> {
    let client =
        client_from_settings(db).ok_or_else(|| "chưa cấu hình App ID/App Secret".to_string())?;
    let pid = page_id
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| db.active_page_id())
        .ok_or_else(|| "chưa chọn Trang (active page)".to_string())?;
    let token = db
        .page_token(&pid)
        .ok_or_else(|| format!("không có Page Access Token cho {pid}"))?;
    Ok((client, pid, token))
}

/// Enqueue a write (draft-first). In `autonomy=live` it publishes immediately via
/// the same [`send_draft`] gate. Shared by REST, MCP, and the heartbeat.
pub(crate) async fn enqueue_or_send(s: &AppState, d: DraftInput) -> Value {
    let kind = d.kind.clone();
    let draft_id = match s.db.add_draft(&d) {
        Ok(id) => id,
        Err(e) => return json!({ "error": e.to_string() }),
    };
    if s.db.autonomy() == "live" {
        return send_draft(s, draft_id).await;
    }
    json!({ "ok": true, "draft_id": draft_id, "status": "pending", "kind": kind })
}

/// The single publish gate: actually call the Graph API for a queued draft.
pub(crate) async fn send_draft(s: &AppState, draft_id: i64) -> Value {
    let Some(d) = s.db.get_draft(draft_id) else {
        return json!({ "error": "draft không tồn tại" });
    };
    if d.status != "pending" {
        return json!({ "error": format!("draft đã ở trạng thái {}", d.status) });
    }
    let (client, _pid, token) = match resolve_page(&s.db, Some(&d.page_id)) {
        Ok(v) => v,
        Err(e) => return json!({ "error": e }),
    };
    let result = match d.kind.as_str() {
        "post" => {
            client
                .create_post(&d.page_id, &token, &d.message, opt(&d.link))
                .await
        }
        "photo" => publish_photo(&client, &d.page_id, &d.image_url, &d.message, &token).await,
        "comment" | "reply" => {
            client
                .create_comment(&d.target_id, &token, &d.message)
                .await
        }
        "message" => {
            client
                .send_message(&d.page_id, &token, &d.target_id, &d.message)
                .await
        }
        "edit" => client.edit_post(&d.target_id, &token, &d.message).await,
        other => Err(anyhow::anyhow!("kind không hỗ trợ: {other}")),
    };
    match result {
        Ok(v) => {
            let result_id = v
                .get("post_id")
                .or_else(|| v.get("id"))
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let _ = s.db.decide_draft(draft_id, "published", &result_id, "");
            s.db.log(
                &d.kind,
                &format!("đã đăng ({}) trên trang {}", d.kind, d.page_id),
                &result_id,
            );
            json!({ "ok": true, "draft_id": draft_id, "status": "published", "result_id": result_id, "result": v })
        }
        Err(e) => {
            let _ = s.db.decide_draft(draft_id, "error", "", &e.to_string());
            json!({ "error": e.to_string(), "draft_id": draft_id })
        }
    }
}

fn opt(s: &str) -> Option<&str> {
    if s.trim().is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Graph/Marketing API returns numbers sometimes as JSON numbers, sometimes as
/// strings. Coerce either to a plain string.
fn val_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

/// Publish a photo draft: if `image_url` points at a locally-uploaded file, send
/// its bytes via multipart and clean the file up; otherwise post it by URL.
async fn publish_photo(
    client: &Client,
    page_id: &str,
    image_url: &str,
    caption: &str,
    token: &str,
) -> anyhow::Result<Value> {
    let path = std::path::Path::new(image_url);
    if path.is_file() {
        let bytes = tokio::fs::read(path).await?;
        let filename = path
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("upload.jpg");
        let mime = crate::fb::image_mime(filename);
        let v = client
            .create_photo_bytes(page_id, token, bytes, filename, mime, caption)
            .await?;
        let _ = tokio::fs::remove_file(path).await; // best-effort cleanup after publish
        Ok(v)
    } else {
        client
            .create_photo(page_id, token, image_url, caption)
            .await
    }
}

/// Directory for user-uploaded images awaiting publish (mirrors the DB path).
fn uploads_dir() -> PathBuf {
    let base = std::env::var("SENCLAW_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(home)
                .join(".senclaw")
                .join("apps")
                .join("facebook-pro")
        });
    base.join("uploads")
}

/// Sanitize an uploaded file's extension to a short alphanumeric token.
fn safe_ext(filename: &str) -> String {
    let ext: String = filename
        .rsplit('.')
        .next()
        .unwrap_or("")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(5)
        .collect::<String>()
        .to_ascii_lowercase();
    if ext.is_empty() {
        "jpg".into()
    } else {
        ext
    }
}

pub fn api_router(state: AppState) -> Router {
    Router::new()
        .route("/status", get(status))
        .route("/settings", get(get_settings).post(set_settings))
        .route("/oauth/link", get(oauth_link))
        .route("/oauth/callback", get(oauth_callback))
        .route("/connect/token", post(connect_token))
        .route("/pages", get(pages))
        .route("/pages/select", post(select_page))
        .route("/posts", get(posts).post(create_post_h))
        .route("/posts/photo_upload", post(photo_upload_h))
        .route("/posts/get", get(post_get))
        .route("/posts/edit", post(edit_post_h))
        .route("/posts/delete", post(delete_post_h))
        .route("/comments", get(comments).post(create_comment_h))
        .route("/comments/reply", post(reply_comment_h))
        .route("/like", post(like_h))
        .route("/conversations", get(conversations_h))
        .route("/conversations/messages", get(conversation_messages_h))
        .route("/messages/reply", post(message_reply_h))
        .route("/overview", get(overview_h))
        .route("/analyze", post(analyze_h))
        .route("/insights/page", get(page_insights_h))
        .route("/insights/post", get(post_insights_h))
        .route("/ads/accounts", get(ad_accounts_h))
        .route("/ads/select", post(select_ad_account_h))
        .route("/ads/campaigns", get(ad_campaigns_h))
        .route("/ads/insights", get(ads_insights_h))
        .route("/ads/analyze", post(ads_analyze_h))
        .route("/ads/status", post(ad_status_h))
        .route("/drafts", get(list_drafts))
        .route("/drafts/:id/approve", post(approve_draft))
        .route("/drafts/:id/reject", post(reject_draft))
        .route("/triggers", get(list_triggers).post(create_trigger_h))
        .route("/triggers/:id/delete", post(delete_trigger_h))
        .route("/activity", get(activity))
        .route("/engine/tick", post(engine_tick))
        // MCP (HTTP + SSE), same shape as the other Space Apps.
        .route(
            "/mcp/sse",
            get(crate::mcp::mcp_sse).post(crate::mcp::mcp_message),
        )
        .route("/mcp/message", post(crate::mcp::mcp_message))
        .with_state(state)
}

// ---- status / settings ----

async fn status(State(s): State<AppState>) -> Json<Value> {
    Json(status_value(&s))
}

pub(crate) fn status_value(s: &AppState) -> Value {
    let configured = client_from_settings(&s.db).is_some();
    let connected = configured
        && s.db
            .get_setting("user_token")
            .map(|t| !t.is_empty())
            .unwrap_or(false);
    json!({
        "ok": true,
        "app": "facebook-pro",
        "configured": configured,
        "connected": connected,
        "active_page_id": s.db.active_page_id().unwrap_or_default(),
        "pages": s.db.list_pages().len(),
        "autonomy": s.db.autonomy(),
        "pending_drafts": s.db.list_drafts("pending").len(),
    })
}

async fn get_settings(State(s): State<AppState>) -> Json<Value> {
    Json(s.db.settings_public())
}

#[derive(Deserialize)]
struct SettingsIn {
    app_id: Option<String>,
    app_secret: Option<String>,
    version: Option<String>,
    autonomy: Option<String>,
}

async fn set_settings(State(s): State<AppState>, Json(body): Json<SettingsIn>) -> Json<Value> {
    if let Some(v) = body.app_id {
        let _ = s.db.set_setting("app_id", &v);
    }
    if let Some(v) = body.app_secret {
        if !v.is_empty() {
            let _ = s.db.set_setting("app_secret", &v);
        }
    }
    if let Some(v) = body.version {
        if !v.is_empty() {
            let _ = s.db.set_setting("version", &v);
        }
    }
    if let Some(v) = body.autonomy {
        let v = match v.as_str() {
            "observe" | "draft" | "live" => v,
            _ => "draft".into(),
        };
        let _ = s.db.set_setting("autonomy", &v);
    }
    Json(s.db.settings_public())
}

// ---- OAuth / connect ----

#[derive(Deserialize)]
struct LinkQuery {
    redirect: String,
}

async fn oauth_link(State(s): State<AppState>, Query(q): Query<LinkQuery>) -> Json<Value> {
    match client_from_settings(&s.db) {
        Some(client) => Json(json!({ "url": client.connect_url(&q.redirect) })),
        None => Json(json!({ "error": "chưa cấu hình App ID/App Secret" })),
    }
}

async fn oauth_callback(
    State(s): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> axum::response::Html<String> {
    if let Some(err) = q.get("error_description").or_else(|| q.get("error")) {
        return oauth_page(&json!({ "error": err }));
    }
    let Some(code) = q.get("code") else {
        return oauth_page(&json!({ "error": "thiếu code" }));
    };
    let Some(client) = client_from_settings(&s.db) else {
        return oauth_page(&json!({ "error": "chưa cấu hình App ID/App Secret" }));
    };
    let redirect = q
        .get("redirect_uri")
        .cloned()
        .unwrap_or_else(|| format!("{}://{}/api/oauth/callback", "http", "127.0.0.1:4590"));
    let short = match client.token_by_code(code, &redirect).await {
        Ok(t) => t,
        Err(e) => return oauth_page(&json!({ "error": e.to_string() })),
    };
    oauth_page(&connect_with_token(&s, &short).await)
}

/// A small self-closing HTML page shown after the Facebook OAuth redirect.
fn oauth_page(result: &Value) -> axum::response::Html<String> {
    let (icon, title, detail) = if let Some(err) = result.get("error").and_then(|x| x.as_str()) {
        ("❌", "Kết nối thất bại".to_string(), err.to_string())
    } else {
        let pages = result.get("pages").and_then(|x| x.as_i64()).unwrap_or(0);
        (
            "✅",
            "Đã kết nối Facebook".to_string(),
            format!("Lấy được {pages} Trang. Bạn có thể đóng tab này và quay lại app."),
        )
    };
    let detail = detail.replace('<', "&lt;").replace('>', "&gt;");
    axum::response::Html(format!(
        r#"<!doctype html><html lang="vi"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Facebook Pro — {title}</title>
<style>body{{font-family:-apple-system,Segoe UI,Roboto,sans-serif;background:#111;color:#eee;
display:flex;min-height:100vh;align-items:center;justify-content:center;margin:0}}
.card{{background:#1c1c1e;border:1px solid #333;border-radius:16px;padding:36px 40px;max-width:440px;text-align:center}}
.icon{{font-size:44px}} h1{{font-size:20px;margin:12px 0 8px}} p{{color:#aaa;line-height:1.5}}
button{{margin-top:18px;background:#1877f2;color:#fff;border:0;border-radius:10px;padding:10px 18px;font-size:14px;cursor:pointer}}</style>
</head><body><div class="card"><div class="icon">{icon}</div><h1>{title}</h1><p>{detail}</p>
<button onclick="window.close()">Đóng tab</button></div></body></html>"#
    ))
}

#[derive(Deserialize)]
struct ConnectTokenIn {
    user_token: String,
}

async fn connect_token(State(s): State<AppState>, Json(body): Json<ConnectTokenIn>) -> Json<Value> {
    if body.user_token.trim().is_empty() {
        return Json(json!({ "error": "thiếu user_token" }));
    }
    Json(connect_with_token(&s, body.user_token.trim()).await)
}

/// Store a user token (exchanging it for a long-lived one when possible) and
/// fetch the admin's Pages + their Page Access Tokens.
pub(crate) async fn connect_with_token(s: &AppState, user_token: &str) -> Value {
    let Some(client) = client_from_settings(&s.db) else {
        return json!({ "error": "chưa cấu hình App ID/App Secret" });
    };
    // Best-effort upgrade to a long-lived token; fall back to the given token.
    let long = client
        .exchange_long_lived(user_token)
        .await
        .unwrap_or_else(|_| user_token.to_string());
    let _ = s.db.set_setting("user_token", &long);
    let pages = match client.get_pages(&long).await {
        Ok(v) => v,
        Err(e) => return json!({ "error": format!("lấy danh sách Trang thất bại: {e}") }),
    };
    let list = pages
        .get("data")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();
    let mut saved = 0;
    for p in &list {
        let (Some(id), Some(tok)) = (
            p.get("id").and_then(|x| x.as_str()),
            p.get("access_token").and_then(|x| x.as_str()),
        ) else {
            continue;
        };
        let name = p.get("name").and_then(|x| x.as_str()).unwrap_or("");
        let cat = p.get("category").and_then(|x| x.as_str()).unwrap_or("");
        if s.db.save_page(id, name, tok, cat).is_ok() {
            saved += 1;
        }
    }
    // Default the active page to the first one if none chosen yet.
    if s.db.active_page_id().is_none() {
        if let Some(first) = list
            .first()
            .and_then(|p| p.get("id"))
            .and_then(|x| x.as_str())
        {
            let _ = s.db.set_setting("active_page_id", first);
        }
    }
    s.db.log("oauth", &format!("kết nối {saved} Trang"), "");
    json!({ "ok": true, "pages": saved, "active_page_id": s.db.active_page_id().unwrap_or_default() })
}

async fn pages(State(s): State<AppState>) -> Json<Value> {
    Json(pages_value(&s).await)
}

/// Return stored pages; if a user token exists, refresh from Graph first.
pub(crate) async fn pages_value(s: &AppState) -> Value {
    if let (Some(client), Some(tok)) = (
        client_from_settings(&s.db),
        s.db.get_setting("user_token").filter(|t| !t.is_empty()),
    ) {
        if let Ok(v) = client.get_pages(&tok).await {
            for p in v
                .get("data")
                .and_then(|x| x.as_array())
                .cloned()
                .unwrap_or_default()
            {
                if let (Some(id), Some(t)) = (
                    p.get("id").and_then(|x| x.as_str()),
                    p.get("access_token").and_then(|x| x.as_str()),
                ) {
                    let name = p.get("name").and_then(|x| x.as_str()).unwrap_or("");
                    let cat = p.get("category").and_then(|x| x.as_str()).unwrap_or("");
                    let _ = s.db.save_page(id, name, t, cat);
                }
            }
        }
    }
    json!({ "pages": s.db.list_pages(), "active_page_id": s.db.active_page_id().unwrap_or_default() })
}

#[derive(Deserialize)]
struct SelectPageIn {
    page_id: String,
}

async fn select_page(State(s): State<AppState>, Json(body): Json<SelectPageIn>) -> Json<Value> {
    if s.db.page_token(&body.page_id).is_none() {
        return Json(json!({ "error": "page_id không có trong danh sách đã kết nối" }));
    }
    let _ = s.db.set_setting("active_page_id", &body.page_id);
    Json(json!({ "ok": true, "active_page_id": body.page_id }))
}

// ---- posts ----

#[derive(Deserialize)]
struct PostsQuery {
    page_id: Option<String>,
    limit: Option<i64>,
}

async fn posts(State(s): State<AppState>, Query(q): Query<PostsQuery>) -> Json<Value> {
    Json(posts_value(&s, q.page_id.as_deref(), q.limit.unwrap_or(15)).await)
}

pub(crate) async fn posts_value(s: &AppState, page_id: Option<&str>, limit: i64) -> Value {
    let (client, pid, token) = match resolve_page(&s.db, page_id) {
        Ok(v) => v,
        Err(e) => return json!({ "error": e }),
    };
    match client.list_posts(&pid, &token, limit).await {
        Ok(v) => v,
        Err(e) => json!({ "error": e.to_string() }),
    }
}

#[derive(Deserialize)]
struct PostIdQuery {
    id: String,
    page_id: Option<String>,
}

async fn post_get(State(s): State<AppState>, Query(q): Query<PostIdQuery>) -> Json<Value> {
    Json(post_get_value(&s, &q.id, q.page_id.as_deref()).await)
}

pub(crate) async fn post_get_value(s: &AppState, post_id: &str, page_id: Option<&str>) -> Value {
    let (client, _pid, token) = match resolve_page(&s.db, page_id) {
        Ok(v) => v,
        Err(e) => return json!({ "error": e }),
    };
    match client.get_post(post_id, &token).await {
        Ok(v) => v,
        Err(e) => json!({ "error": e.to_string() }),
    }
}

#[derive(Deserialize)]
struct CreatePostIn {
    page_id: Option<String>,
    message: String,
    link: Option<String>,
    image_url: Option<String>,
}

async fn create_post_h(State(s): State<AppState>, Json(b): Json<CreatePostIn>) -> Json<Value> {
    let pid = b
        .page_id
        .or_else(|| s.db.active_page_id())
        .unwrap_or_default();
    let has_image = b
        .image_url
        .as_deref()
        .map(|x| !x.trim().is_empty())
        .unwrap_or(false);
    let d = DraftInput {
        kind: if has_image {
            "photo".into()
        } else {
            "post".into()
        },
        page_id: pid,
        message: b.message,
        link: b.link.unwrap_or_default(),
        image_url: b.image_url.unwrap_or_default(),
        source: "user".into(),
        ..Default::default()
    };
    Json(enqueue_or_send(&s, d).await)
}

/// Upload a LOCAL image and queue a photo-post draft. Multipart fields: `file`
/// (the image), `message` (caption), optional `page_id`. The file is saved under
/// the app's uploads dir and published (multipart) only when the draft is approved.
async fn photo_upload_h(State(s): State<AppState>, mut mp: Multipart) -> Json<Value> {
    let mut message = String::new();
    let mut page_id: Option<String> = None;
    let mut bytes: Option<Vec<u8>> = None;
    let mut filename = String::from("upload.jpg");
    while let Ok(Some(field)) = mp.next_field().await {
        match field.name().unwrap_or("") {
            "message" => message = field.text().await.unwrap_or_default(),
            "page_id" => page_id = Some(field.text().await.unwrap_or_default()),
            "file" => {
                if let Some(fname) = field.file_name() {
                    filename = fname.to_string();
                }
                bytes = field.bytes().await.ok().map(|b| b.to_vec());
            }
            _ => {}
        }
    }
    let Some(bytes) = bytes.filter(|b| !b.is_empty()) else {
        return Json(json!({ "error": "thiếu file ảnh" }));
    };
    let dir = uploads_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return Json(json!({ "error": "không tạo được thư mục uploads" }));
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = dir.join(format!("{stamp}.{}", safe_ext(&filename)));
    if std::fs::write(&path, &bytes).is_err() {
        return Json(json!({ "error": "không lưu được ảnh" }));
    }
    let pid = page_id
        .filter(|p| !p.is_empty())
        .or_else(|| s.db.active_page_id())
        .unwrap_or_default();
    let d = DraftInput {
        kind: "photo".into(),
        page_id: pid,
        message,
        image_url: path.to_string_lossy().to_string(),
        source: "user".into(),
        ..Default::default()
    };
    Json(enqueue_or_send(&s, d).await)
}

#[derive(Deserialize)]
struct EditPostIn {
    page_id: Option<String>,
    post_id: String,
    message: String,
}

async fn edit_post_h(State(s): State<AppState>, Json(b): Json<EditPostIn>) -> Json<Value> {
    let pid = b
        .page_id
        .or_else(|| s.db.active_page_id())
        .unwrap_or_default();
    let d = DraftInput {
        kind: "edit".into(),
        page_id: pid,
        target_id: b.post_id,
        message: b.message,
        source: "user".into(),
        ..Default::default()
    };
    Json(enqueue_or_send(&s, d).await)
}

#[derive(Deserialize)]
struct DeletePostIn {
    page_id: Option<String>,
    post_id: String,
}

async fn delete_post_h(State(s): State<AppState>, Json(b): Json<DeletePostIn>) -> Json<Value> {
    Json(delete_post_value(&s, &b.post_id, b.page_id.as_deref()).await)
}

/// Delete is an explicit, immediate action (never automated by the heartbeat).
pub(crate) async fn delete_post_value(s: &AppState, post_id: &str, page_id: Option<&str>) -> Value {
    let (client, _pid, token) = match resolve_page(&s.db, page_id) {
        Ok(v) => v,
        Err(e) => return json!({ "error": e }),
    };
    match client.delete_post(post_id, &token).await {
        Ok(v) => {
            s.db.log("delete", &format!("đã xoá bài {post_id}"), post_id);
            v
        }
        Err(e) => json!({ "error": e.to_string() }),
    }
}

// ---- comments ----

#[derive(Deserialize)]
struct CommentsQuery {
    object_id: String,
    page_id: Option<String>,
    limit: Option<i64>,
}

async fn comments(State(s): State<AppState>, Query(q): Query<CommentsQuery>) -> Json<Value> {
    Json(
        comments_value(
            &s,
            &q.object_id,
            q.page_id.as_deref(),
            q.limit.unwrap_or(25),
        )
        .await,
    )
}

pub(crate) async fn comments_value(
    s: &AppState,
    object_id: &str,
    page_id: Option<&str>,
    limit: i64,
) -> Value {
    let (client, _pid, token) = match resolve_page(&s.db, page_id) {
        Ok(v) => v,
        Err(e) => return json!({ "error": e }),
    };
    match client.list_comments(object_id, &token, limit).await {
        Ok(v) => v,
        Err(e) => json!({ "error": e.to_string() }),
    }
}

#[derive(Deserialize)]
struct CommentIn {
    page_id: Option<String>,
    object_id: String,
    message: String,
}

async fn create_comment_h(State(s): State<AppState>, Json(b): Json<CommentIn>) -> Json<Value> {
    let pid = b
        .page_id
        .or_else(|| s.db.active_page_id())
        .unwrap_or_default();
    let d = DraftInput {
        kind: "comment".into(),
        page_id: pid,
        target_id: b.object_id,
        message: b.message,
        source: "user".into(),
        ..Default::default()
    };
    Json(enqueue_or_send(&s, d).await)
}

#[derive(Deserialize)]
struct ReplyIn {
    page_id: Option<String>,
    comment_id: String,
    message: Option<String>,
    comment_text: Option<String>,
    hint: Option<String>,
}

async fn reply_comment_h(State(s): State<AppState>, Json(b): Json<ReplyIn>) -> Json<Value> {
    let pid = b
        .page_id
        .clone()
        .or_else(|| s.db.active_page_id())
        .unwrap_or_default();
    // If no explicit message, compose one from the comment text.
    let (message, model) = match b.message.filter(|m| !m.trim().is_empty()) {
        Some(m) => (m, String::new()),
        None => {
            let page_name = page_name(&s.db, &pid);
            llm::compose_reply(
                &s.sc,
                &page_name,
                b.comment_text.as_deref().unwrap_or(""),
                b.hint.as_deref().unwrap_or(""),
            )
            .await
        }
    };
    let d = DraftInput {
        kind: "reply".into(),
        page_id: pid,
        target_id: b.comment_id,
        message,
        model,
        source: "user".into(),
        ..Default::default()
    };
    Json(enqueue_or_send(&s, d).await)
}

fn page_name(db: &Db, page_id: &str) -> String {
    db.list_pages()
        .into_iter()
        .find(|p| p.get("page_id").and_then(|x| x.as_str()) == Some(page_id))
        .and_then(|p| {
            p.get("name")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "Trang".into())
}

#[derive(Deserialize)]
struct LikeIn {
    page_id: Option<String>,
    object_id: String,
}

async fn like_h(State(s): State<AppState>, Json(b): Json<LikeIn>) -> Json<Value> {
    Json(like_value(&s, &b.object_id, b.page_id.as_deref()).await)
}

/// Like is an explicit, immediate engagement (never automated by the heartbeat).
pub(crate) async fn like_value(s: &AppState, object_id: &str, page_id: Option<&str>) -> Value {
    let (client, _pid, token) = match resolve_page(&s.db, page_id) {
        Ok(v) => v,
        Err(e) => return json!({ "error": e }),
    };
    match client.like_object(object_id, &token).await {
        Ok(v) => {
            s.db.log("like", &format!("đã thả like {object_id}"), object_id);
            v
        }
        Err(e) => json!({ "error": e.to_string() }),
    }
}

// ---- messaging (Page inbox) ----

#[derive(Deserialize)]
struct ConversationsQuery {
    page_id: Option<String>,
    limit: Option<i64>,
}

async fn conversations_h(
    State(s): State<AppState>,
    Query(q): Query<ConversationsQuery>,
) -> Json<Value> {
    Json(conversations_value(&s, q.page_id.as_deref(), q.limit.unwrap_or(25)).await)
}

pub(crate) async fn conversations_value(s: &AppState, page_id: Option<&str>, limit: i64) -> Value {
    let (client, pid, token) = match resolve_page(&s.db, page_id) {
        Ok(v) => v,
        Err(e) => return json!({ "error": e }),
    };
    match client.list_conversations(&pid, &token, limit).await {
        Ok(v) => v,
        Err(e) => json!({ "error": e.to_string() }),
    }
}

#[derive(Deserialize)]
struct ConvMessagesQuery {
    id: String,
    page_id: Option<String>,
    limit: Option<i64>,
}

async fn conversation_messages_h(
    State(s): State<AppState>,
    Query(q): Query<ConvMessagesQuery>,
) -> Json<Value> {
    Json(conversation_messages_value(&s, &q.id, q.page_id.as_deref(), q.limit.unwrap_or(25)).await)
}

pub(crate) async fn conversation_messages_value(
    s: &AppState,
    conversation_id: &str,
    page_id: Option<&str>,
    limit: i64,
) -> Value {
    let (client, _pid, token) = match resolve_page(&s.db, page_id) {
        Ok(v) => v,
        Err(e) => return json!({ "error": e }),
    };
    match client
        .conversation_messages(conversation_id, &token, limit)
        .await
    {
        Ok(v) => v,
        Err(e) => json!({ "error": e.to_string() }),
    }
}

#[derive(Deserialize)]
struct MessageReplyIn {
    page_id: Option<String>,
    recipient_id: String,
    message: Option<String>,
    customer_msg: Option<String>,
    hint: Option<String>,
}

async fn message_reply_h(State(s): State<AppState>, Json(b): Json<MessageReplyIn>) -> Json<Value> {
    Json(
        message_reply_value(
            &s,
            b.page_id.as_deref(),
            &b.recipient_id,
            b.message.as_deref(),
            b.customer_msg.as_deref(),
            b.hint.as_deref(),
            "user",
        )
        .await,
    )
}

/// Draft-first message reply. Composes via LLM when `message` is empty. Shared by
/// REST + MCP.
pub(crate) async fn message_reply_value(
    s: &AppState,
    page_id: Option<&str>,
    recipient_id: &str,
    message: Option<&str>,
    customer_msg: Option<&str>,
    hint: Option<&str>,
    source: &str,
) -> Value {
    if recipient_id.trim().is_empty() {
        return json!({ "error": "thiếu recipient_id" });
    }
    let pid = page_id
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| s.db.active_page_id())
        .unwrap_or_default();
    let (msg, model) = match message.filter(|m| !m.trim().is_empty()) {
        Some(m) => (m.to_string(), String::new()),
        None => {
            let name = page_name(&s.db, &pid);
            llm::compose_reply(&s.sc, &name, customer_msg.unwrap_or(""), hint.unwrap_or("")).await
        }
    };
    enqueue_or_send(
        s,
        DraftInput {
            kind: "message".into(),
            page_id: pid,
            target_id: recipient_id.to_string(),
            message: msg,
            model,
            source: source.into(),
            ..Default::default()
        },
    )
    .await
}

// ---- overview (interactions + comment stats) ----

async fn overview_h(State(s): State<AppState>) -> Json<Value> {
    Json(overview_value(&s).await)
}

/// Aggregate the active Page's recent posts into a dashboard: engagement totals,
/// top posts, and per-post comment counts. Cheap — one posts call.
pub(crate) async fn overview_value(s: &AppState) -> Value {
    let posts = posts_value(s, None, 25).await;
    if let Some(err) = posts.get("error").and_then(|x| x.as_str()) {
        return json!({ "error": err });
    }
    let list = posts
        .get("data")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();
    let mut total_reactions = 0i64;
    let mut total_comments = 0i64;
    let mut total_shares = 0i64;
    let mut rows: Vec<Value> = Vec::new();
    for p in &list {
        let reactions = p
            .get("reactions")
            .and_then(|r| r.get("summary"))
            .and_then(|s| s.get("total_count"))
            .and_then(|x| x.as_i64())
            .unwrap_or(0);
        let comments = p
            .get("comments")
            .and_then(|r| r.get("summary"))
            .and_then(|s| s.get("total_count"))
            .and_then(|x| x.as_i64())
            .unwrap_or(0);
        let shares = p
            .get("shares")
            .and_then(|r| r.get("count"))
            .and_then(|x| x.as_i64())
            .unwrap_or(0);
        total_reactions += reactions;
        total_comments += comments;
        total_shares += shares;
        rows.push(json!({
            "id": p.get("id").and_then(|x| x.as_str()).unwrap_or(""),
            "message": p.get("message").and_then(|x| x.as_str()).or_else(|| p.get("story").and_then(|x| x.as_str())).unwrap_or(""),
            "created_time": p.get("created_time").and_then(|x| x.as_str()).unwrap_or(""),
            "permalink_url": p.get("permalink_url").and_then(|x| x.as_str()).unwrap_or(""),
            "reactions": reactions,
            "comments": comments,
            "shares": shares,
            "engagement": reactions + comments + shares,
        }));
    }
    let mut top = rows.clone();
    top.sort_by_key(|r| -(r.get("engagement").and_then(|x| x.as_i64()).unwrap_or(0)));
    top.truncate(5);
    json!({
        "ok": true,
        "active_page_id": s.db.active_page_id().unwrap_or_default(),
        "totals": {
            "posts": list.len(),
            "reactions": total_reactions,
            "comments": total_comments,
            "shares": total_shares,
            "engagement": total_reactions + total_comments + total_shares,
            "pending_drafts": s.db.list_drafts("pending").len(),
        },
        "top_posts": top,
        "posts": rows,
    })
}

// ---- analyze ----

#[derive(Deserialize)]
struct AnalyzeIn {
    page_id: Option<String>,
    post_id: Option<String>,
    message: Option<String>,
}

async fn analyze_h(State(s): State<AppState>, Json(b): Json<AnalyzeIn>) -> Json<Value> {
    Json(
        analyze_value(
            &s,
            b.post_id.as_deref(),
            b.message.as_deref(),
            b.page_id.as_deref(),
        )
        .await,
    )
}

pub(crate) async fn analyze_value(
    s: &AppState,
    post_id: Option<&str>,
    message: Option<&str>,
    page_id: Option<&str>,
) -> Value {
    // Prefer a live post fetch (real content + engagement); fall back to given text.
    let (content, engagement) = if let Some(pid) = post_id.filter(|p| !p.trim().is_empty()) {
        let v = post_get_value(s, pid, page_id).await;
        if let Some(err) = v.get("error").and_then(|x| x.as_str()) {
            return json!({ "error": err });
        }
        let msg = v
            .get("message")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        (msg, engagement_summary(&v))
    } else {
        (message.unwrap_or("").to_string(), String::new())
    };
    if content.trim().is_empty() && engagement.is_empty() {
        return json!({ "error": "cần post_id hoặc message để phân tích" });
    }
    let (analysis, model) = llm::analyze_post(&s.sc, &content, &engagement).await;
    s.db.log("analyze", "phân tích bài viết", post_id.unwrap_or(""));
    json!({ "ok": true, "analysis": analysis, "model": model, "engagement": engagement })
}

/// A compact "reactions=… comments=… shares=…" summary from a post value.
fn engagement_summary(v: &Value) -> String {
    let reactions = v
        .get("reactions")
        .and_then(|r| r.get("summary"))
        .and_then(|s| s.get("total_count"))
        .and_then(|x| x.as_i64());
    let comments = v
        .get("comments")
        .and_then(|r| r.get("summary"))
        .and_then(|s| s.get("total_count"))
        .and_then(|x| x.as_i64());
    let shares = v
        .get("shares")
        .and_then(|r| r.get("count"))
        .and_then(|x| x.as_i64());
    let mut parts = Vec::new();
    if let Some(x) = reactions {
        parts.push(format!("reactions={x}"));
    }
    if let Some(x) = comments {
        parts.push(format!("comments={x}"));
    }
    if let Some(x) = shares {
        parts.push(format!("shares={x}"));
    }
    parts.join(" ")
}

// ---- insights ----

#[derive(Deserialize)]
struct PageInsightsQuery {
    page_id: Option<String>,
    metric: Option<String>,
    period: Option<String>,
}

async fn page_insights_h(
    State(s): State<AppState>,
    Query(q): Query<PageInsightsQuery>,
) -> Json<Value> {
    Json(
        page_insights_value(
            &s,
            q.page_id.as_deref(),
            q.metric.as_deref(),
            q.period.as_deref(),
        )
        .await,
    )
}

const DEFAULT_PAGE_METRICS: &str = "page_impressions,page_post_engagements,page_fans";
const DEFAULT_POST_METRICS: &str =
    "post_impressions,post_impressions_unique,post_clicks,post_reactions_by_type_total";

pub(crate) async fn page_insights_value(
    s: &AppState,
    page_id: Option<&str>,
    metric: Option<&str>,
    period: Option<&str>,
) -> Value {
    let (client, pid, token) = match resolve_page(&s.db, page_id) {
        Ok(v) => v,
        Err(e) => return json!({ "error": e }),
    };
    let metrics = metric
        .filter(|m| !m.trim().is_empty())
        .unwrap_or(DEFAULT_PAGE_METRICS);
    let period = period.filter(|p| !p.trim().is_empty()).unwrap_or("day");
    match client.page_insights(&pid, &token, metrics, period).await {
        Ok(v) => v,
        Err(e) => json!({ "error": e.to_string() }),
    }
}

#[derive(Deserialize)]
struct PostInsightsQuery {
    id: String,
    page_id: Option<String>,
    metric: Option<String>,
}

async fn post_insights_h(
    State(s): State<AppState>,
    Query(q): Query<PostInsightsQuery>,
) -> Json<Value> {
    Json(post_insights_value(&s, &q.id, q.page_id.as_deref(), q.metric.as_deref()).await)
}

pub(crate) async fn post_insights_value(
    s: &AppState,
    post_id: &str,
    page_id: Option<&str>,
    metric: Option<&str>,
) -> Value {
    let (client, _pid, token) = match resolve_page(&s.db, page_id) {
        Ok(v) => v,
        Err(e) => return json!({ "error": e }),
    };
    let metrics = metric
        .filter(|m| !m.trim().is_empty())
        .unwrap_or(DEFAULT_POST_METRICS);
    match client.post_insights(post_id, &token, metrics).await {
        Ok(v) => v,
        Err(e) => json!({ "error": e.to_string() }),
    }
}

// ---- ads (Marketing API) ----

/// Ads calls use the USER token (with ads_read/ads_management), not a page token.
fn ads_client_token(db: &Db) -> Result<(Client, String), String> {
    let client =
        client_from_settings(db).ok_or_else(|| "chưa cấu hình App ID/App Secret".to_string())?;
    let token = db
        .get_setting("user_token")
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "chưa kết nối (thiếu user token có quyền ads_read)".to_string())?;
    Ok((client, token))
}

/// The ad account object id to act on — explicit, else the active one. Ensures
/// the `act_` prefix Facebook expects for account-level objects.
fn resolve_ad_account(db: &Db, account_id: Option<&str>) -> Result<String, String> {
    let id = account_id
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            db.get_setting("active_ad_account")
                .filter(|s| !s.is_empty())
        })
        .ok_or_else(|| "chưa chọn Tài khoản quảng cáo (ad account)".to_string())?;
    Ok(if id.starts_with("act_") {
        id
    } else {
        format!("act_{id}")
    })
}

async fn ad_accounts_h(State(s): State<AppState>) -> Json<Value> {
    Json(ad_accounts_value(&s).await)
}

pub(crate) async fn ad_accounts_value(s: &AppState) -> Value {
    let (client, token) = match ads_client_token(&s.db) {
        Ok(v) => v,
        Err(e) => return json!({ "error": e }),
    };
    match client.get_ad_accounts(&token).await {
        Ok(v) => {
            json!({ "accounts": v.get("data").cloned().unwrap_or(json!([])), "active_ad_account": s.db.get_setting("active_ad_account").unwrap_or_default() })
        }
        Err(e) => json!({ "error": e.to_string() }),
    }
}

#[derive(Deserialize)]
struct SelectAdAccountIn {
    account_id: String,
}

async fn select_ad_account_h(
    State(s): State<AppState>,
    Json(b): Json<SelectAdAccountIn>,
) -> Json<Value> {
    let id = if b.account_id.starts_with("act_") {
        b.account_id.clone()
    } else {
        format!("act_{}", b.account_id)
    };
    let _ = s.db.set_setting("active_ad_account", &id);
    Json(json!({ "ok": true, "active_ad_account": id }))
}

#[derive(Deserialize)]
struct AdCampaignsQuery {
    account_id: Option<String>,
}

async fn ad_campaigns_h(
    State(s): State<AppState>,
    Query(q): Query<AdCampaignsQuery>,
) -> Json<Value> {
    Json(ad_campaigns_value(&s, q.account_id.as_deref()).await)
}

pub(crate) async fn ad_campaigns_value(s: &AppState, account_id: Option<&str>) -> Value {
    let (client, token) = match ads_client_token(&s.db) {
        Ok(v) => v,
        Err(e) => return json!({ "error": e }),
    };
    let act = match resolve_ad_account(&s.db, account_id) {
        Ok(v) => v,
        Err(e) => return json!({ "error": e }),
    };
    match client.list_campaigns(&act, &token).await {
        Ok(v) => v,
        Err(e) => json!({ "error": e.to_string() }),
    }
}

#[derive(Deserialize)]
struct AdsInsightsQuery {
    object_id: Option<String>,
    level: Option<String>,
    date_preset: Option<String>,
}

async fn ads_insights_h(
    State(s): State<AppState>,
    Query(q): Query<AdsInsightsQuery>,
) -> Json<Value> {
    Json(
        ads_insights_value(
            &s,
            q.object_id.as_deref(),
            q.level.as_deref(),
            q.date_preset.as_deref(),
        )
        .await,
    )
}

/// Normalize the `level`/`date_preset` inputs to values the Marketing API accepts.
pub(crate) fn norm_level(level: Option<&str>) -> String {
    match level {
        Some("account") => "account",
        Some("adset") => "adset",
        Some("ad") => "ad",
        _ => "campaign",
    }
    .to_string()
}

pub(crate) fn norm_date_preset(dp: Option<&str>) -> String {
    match dp {
        Some(x) if !x.trim().is_empty() => x.trim().to_string(),
        _ => "last_7d".to_string(),
    }
}

pub(crate) async fn ads_insights_value(
    s: &AppState,
    object_id: Option<&str>,
    level: Option<&str>,
    date_preset: Option<&str>,
) -> Value {
    let (client, token) = match ads_client_token(&s.db) {
        Ok(v) => v,
        Err(e) => return json!({ "error": e }),
    };
    // Object defaults to the active ad account (with act_ prefix).
    let object = match object_id.filter(|o| !o.trim().is_empty()) {
        Some(o) => o.to_string(),
        None => match resolve_ad_account(&s.db, None) {
            Ok(v) => v,
            Err(e) => return json!({ "error": e }),
        },
    };
    let level = norm_level(level);
    let dp = norm_date_preset(date_preset);
    match client.ad_insights(&object, &token, &level, &dp).await {
        Ok(v) => {
            let (rows, _) = summarize_ads_rows(&v);
            json!({ "object_id": object, "level": level, "date_preset": dp, "rows": rows, "raw": v.get("data").cloned().unwrap_or(json!([])) })
        }
        Err(e) => json!({ "error": e.to_string() }),
    }
}

/// Flatten Ads Insights `data` rows into UI-friendly records + an LLM summary
/// string. Extracts a single headline "results" + cost-per-result + ROAS from the
/// nested `actions`/`cost_per_action_type`/`purchase_roas` arrays.
pub(crate) fn summarize_ads_rows(v: &Value) -> (Vec<Value>, String) {
    let data = v
        .get("data")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();
    // Priority of "result" action types (most business-meaningful first).
    const PRIORITY: [&str; 6] = [
        "purchase",
        "onsite_conversion.purchase",
        "lead",
        "onsite_conversion.lead_grouped",
        "link_click",
        "landing_page_view",
    ];
    let pick = |arr: &Value| -> Option<(String, String)> {
        let a = arr.as_array()?;
        for want in PRIORITY {
            if let Some(hit) = a
                .iter()
                .find(|x| x.get("action_type").and_then(|t| t.as_str()) == Some(want))
            {
                let val = hit.get("value").map(val_str).unwrap_or_default();
                return Some((want.to_string(), val));
            }
        }
        // Fall back to the first action, whatever it is.
        a.first().map(|hit| {
            (
                hit.get("action_type")
                    .and_then(|t| t.as_str())
                    .unwrap_or("action")
                    .to_string(),
                hit.get("value").map(val_str).unwrap_or_default(),
            )
        })
    };

    let mut rows = Vec::new();
    let mut lines = Vec::new();
    for r in &data {
        let name = r
            .get("campaign_name")
            .or_else(|| r.get("adset_name"))
            .or_else(|| r.get("ad_name"))
            .and_then(|x| x.as_str())
            .unwrap_or("(tài khoản)")
            .to_string();
        let get = |k: &str| r.get(k).map(val_str).unwrap_or_default();
        let (result_type, results) = r.get("actions").and_then(|a| pick(a)).unwrap_or_default();
        let cost_per_result = r
            .get("cost_per_action_type")
            .and_then(|a| pick(a))
            .map(|(_, v)| v)
            .unwrap_or_default();
        let roas = r
            .get("purchase_roas")
            .and_then(|a| a.as_array())
            .and_then(|a| a.first())
            .and_then(|x| x.get("value"))
            .map(val_str)
            .unwrap_or_default();
        let (spend, ctr, cpc, cpm) = (get("spend"), get("ctr"), get("cpc"), get("cpm"));
        rows.push(json!({
            "name": name,
            "impressions": get("impressions"),
            "clicks": get("clicks"),
            "spend": spend,
            "ctr": ctr,
            "cpc": cpc,
            "cpm": cpm,
            "reach": get("reach"),
            "result_type": result_type,
            "results": results,
            "cost_per_result": cost_per_result,
            "roas": roas,
        }));
        lines.push(format!(
            "- {name}: chi={spend} CTR={ctr}% CPC={cpc} CPM={cpm} kết_quả={results}({result_type}) chi/kết_quả={cost_per_result} ROAS={roas}",
            name = name, spend = get("spend"), ctr = get("ctr"), cpc = get("cpc"), cpm = get("cpm"),
        ));
    }
    (rows, lines.join("\n"))
}

#[derive(Deserialize)]
struct AdsAnalyzeIn {
    object_id: Option<String>,
    level: Option<String>,
    date_preset: Option<String>,
    currency: Option<String>,
}

async fn ads_analyze_h(State(s): State<AppState>, Json(b): Json<AdsAnalyzeIn>) -> Json<Value> {
    Json(
        ads_analyze_value(
            &s,
            b.object_id.as_deref(),
            b.level.as_deref(),
            b.date_preset.as_deref(),
            b.currency.as_deref(),
        )
        .await,
    )
}

pub(crate) async fn ads_analyze_value(
    s: &AppState,
    object_id: Option<&str>,
    level: Option<&str>,
    date_preset: Option<&str>,
    currency: Option<&str>,
) -> Value {
    let (client, token) = match ads_client_token(&s.db) {
        Ok(v) => v,
        Err(e) => return json!({ "error": e }),
    };
    let object = match object_id.filter(|o| !o.trim().is_empty()) {
        Some(o) => o.to_string(),
        None => match resolve_ad_account(&s.db, None) {
            Ok(v) => v,
            Err(e) => return json!({ "error": e }),
        },
    };
    let level = norm_level(level);
    let dp = norm_date_preset(date_preset);
    let insights = match client.ad_insights(&object, &token, &level, &dp).await {
        Ok(v) => v,
        Err(e) => return json!({ "error": e.to_string() }),
    };
    let (rows, summary) = summarize_ads_rows(&insights);
    if summary.trim().is_empty() {
        return json!({ "ok": true, "rows": rows, "verdict": "Không có dữ liệu chi tiêu trong khoảng thời gian này.", "model": "" });
    }
    let (verdict, model) = llm::analyze_ads(&s.sc, currency.unwrap_or("VND"), &summary).await;
    s.db.log("ads", &format!("phân tích ads ({level}, {dp})"), &object);
    json!({ "ok": true, "object_id": object, "level": level, "date_preset": dp, "rows": rows, "verdict": verdict, "model": model })
}

#[derive(Deserialize)]
struct AdStatusIn {
    entity_id: String,
    status: String,
}

async fn ad_status_h(State(s): State<AppState>, Json(b): Json<AdStatusIn>) -> Json<Value> {
    Json(ad_status_value(&s, &b.entity_id, &b.status).await)
}

/// Pause/resume an ad entity — explicit, immediate (never automated). `status`
/// is normalized to PAUSED/ACTIVE.
pub(crate) async fn ad_status_value(s: &AppState, entity_id: &str, status: &str) -> Value {
    let want = match status.to_uppercase().as_str() {
        "PAUSED" | "PAUSE" | "OFF" | "TẮT" => "PAUSED",
        "ACTIVE" | "RESUME" | "ON" | "BẬT" => "ACTIVE",
        other => {
            return json!({ "error": format!("status phải là PAUSED hoặc ACTIVE (nhận '{other}')") })
        }
    };
    let (client, token) = match ads_client_token(&s.db) {
        Ok(v) => v,
        Err(e) => return json!({ "error": e }),
    };
    match client.set_entity_status(entity_id, &token, want).await {
        Ok(v) => {
            s.db.log("ads", &format!("đặt {entity_id} = {want}"), entity_id);
            json!({ "ok": true, "entity_id": entity_id, "status": want, "result": v })
        }
        Err(e) => json!({ "error": e.to_string() }),
    }
}

// ---- drafts ----

async fn list_drafts(State(s): State<AppState>) -> Json<Value> {
    Json(json!({ "pending": s.db.list_drafts("pending") }))
}

async fn approve_draft(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    Json(send_draft(&s, id).await)
}

async fn reject_draft(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    // Clean up an uploaded local image that will never be published.
    if let Some(d) = s.db.get_draft(id) {
        if d.kind == "photo" && std::path::Path::new(&d.image_url).is_file() {
            let _ = std::fs::remove_file(&d.image_url);
        }
    }
    let _ = s.db.decide_draft(id, "rejected", "", "");
    Json(json!({ "ok": true, "status": "rejected" }))
}

// ---- triggers ----

async fn list_triggers(State(s): State<AppState>) -> Json<Value> {
    Json(json!({ "triggers": s.db.list_triggers(None) }))
}

#[derive(Deserialize)]
struct TriggerIn {
    name: Option<String>,
    page_id: Option<String>,
    match_type: Option<String>,
    match_value: Option<String>,
    action: Option<String>,
    reply_hint: Option<String>,
    enabled: Option<bool>,
}

async fn create_trigger_h(State(s): State<AppState>, Json(b): Json<TriggerIn>) -> Json<Value> {
    let match_type = normalize_match_type(b.match_type.as_deref());
    let action = match b.action.as_deref() {
        Some("notify") => "notify",
        _ => "draft_reply",
    }
    .to_string();
    let t = TriggerInput {
        name: b.name.unwrap_or_else(|| "trigger".into()),
        page_id: b.page_id.unwrap_or_default(),
        event: "new_comment".into(),
        match_type,
        match_value: b.match_value.unwrap_or_default(),
        action,
        reply_hint: b.reply_hint.unwrap_or_default(),
        enabled: b.enabled.unwrap_or(true),
    };
    match s.db.add_trigger(&t) {
        Ok(id) => Json(json!({ "ok": true, "id": id })),
        Err(e) => Json(json!({ "error": e.to_string() })),
    }
}

pub(crate) fn normalize_match_type(t: Option<&str>) -> String {
    match t {
        Some("keyword") => "keyword",
        Some("question") => "question",
        _ => "all",
    }
    .to_string()
}

async fn delete_trigger_h(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    let _ = s.db.delete_trigger(id);
    Json(json!({ "ok": true }))
}

// ---- activity / engine ----

async fn activity(State(s): State<AppState>) -> Json<Value> {
    Json(json!({ "activity": s.db.recent_activity(50) }))
}

async fn engine_tick(State(s): State<AppState>) -> Json<Value> {
    Json(crate::engine::tick(&s).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> AppState {
        let db = Arc::new(Db::open_memory().unwrap());
        let (mcp_tx, _) = tokio::sync::broadcast::channel(8);
        AppState {
            db,
            sc: SpaceClient::new("http://127.0.0.1:1", "facebook-pro"),
            mcp_tx,
        }
    }

    #[tokio::test]
    async fn draft_mode_queues_without_publishing() {
        let s = state();
        s.db.set_setting("app_id", "1").unwrap();
        s.db.set_setting("app_secret", "x").unwrap();
        s.db.save_page("P1", "Trang", "tok", "Shop").unwrap();
        s.db.set_setting("active_page_id", "P1").unwrap();
        // autonomy defaults to "draft" → enqueue must NOT hit the network.
        let r = enqueue_or_send(
            &s,
            DraftInput {
                kind: "post".into(),
                page_id: "P1".into(),
                message: "Xin chào".into(),
                source: "user".into(),
                ..Default::default()
            },
        )
        .await;
        assert_eq!(r["status"], "pending");
        assert_eq!(s.db.list_drafts("pending").len(), 1);
    }

    #[test]
    fn safe_ext_sanitizes() {
        assert_eq!(safe_ext("photo.JPG"), "jpg");
        assert_eq!(safe_ext("a.png?x=1"), "pngx1");
        assert_eq!(safe_ext("noext"), "noext");
        assert_eq!(safe_ext("trailing."), "jpg");
    }

    #[test]
    fn match_type_normalizes() {
        assert_eq!(normalize_match_type(Some("keyword")), "keyword");
        assert_eq!(normalize_match_type(Some("question")), "question");
        assert_eq!(normalize_match_type(Some("garbage")), "all");
        assert_eq!(normalize_match_type(None), "all");
    }

    #[test]
    fn ads_level_and_date_normalize() {
        assert_eq!(norm_level(Some("ad")), "ad");
        assert_eq!(norm_level(Some("account")), "account");
        assert_eq!(norm_level(Some("junk")), "campaign");
        assert_eq!(norm_level(None), "campaign");
        assert_eq!(norm_date_preset(None), "last_7d");
        assert_eq!(norm_date_preset(Some("last_30d")), "last_30d");
    }

    #[test]
    fn summarize_ads_extracts_ctr_cpc_and_results() {
        let v = json!({ "data": [
            {
                "campaign_name": "Sale T7",
                "impressions": "10000", "clicks": "150", "spend": "500000",
                "ctr": "1.5", "cpc": "3333", "cpm": "50000", "reach": "8000",
                "actions": [ { "action_type": "link_click", "value": "150" }, { "action_type": "purchase", "value": "12" } ],
                "cost_per_action_type": [ { "action_type": "purchase", "value": "41666" } ],
                "purchase_roas": [ { "action_type": "omni_purchase", "value": "2.3" } ]
            }
        ]});
        let (rows, summary) = summarize_ads_rows(&v);
        assert_eq!(rows.len(), 1);
        // purchase wins over link_click by priority.
        assert_eq!(rows[0]["result_type"], "purchase");
        assert_eq!(rows[0]["results"], "12");
        assert_eq!(rows[0]["cost_per_result"], "41666");
        assert_eq!(rows[0]["roas"], "2.3");
        assert_eq!(rows[0]["ctr"], "1.5");
        assert!(summary.contains("Sale T7"));
        assert!(summary.contains("ROAS=2.3"));
    }

    #[test]
    fn summarize_ads_handles_empty() {
        let (rows, summary) = summarize_ads_rows(&json!({ "data": [] }));
        assert!(rows.is_empty());
        assert!(summary.is_empty());
    }

    #[tokio::test]
    async fn message_draft_queues_and_composes_when_empty() {
        let s = state();
        s.db.set_setting("app_id", "1").unwrap();
        s.db.set_setting("app_secret", "x").unwrap();
        s.db.save_page("P1", "Trang", "tok", "Shop").unwrap();
        s.db.set_setting("active_page_id", "P1").unwrap();
        // No message → LLM compose is attempted (bridge unreachable → empty), still
        // enqueues a pending 'message' draft to the recipient. Draft mode: no network send.
        let r = message_reply_value(
            &s,
            None,
            "PSID123",
            None,
            Some("Ship bao lâu?"),
            None,
            "user",
        )
        .await;
        assert_eq!(r["status"], "pending");
        assert_eq!(r["kind"], "message");
        let d = &s.db.list_drafts("pending")[0];
        assert_eq!(d.kind, "message");
        assert_eq!(d.target_id, "PSID123");
    }

    #[test]
    fn engagement_summary_reads_nested_counts() {
        let v = json!({
            "reactions": { "summary": { "total_count": 12 } },
            "comments": { "summary": { "total_count": 3 } },
            "shares": { "count": 2 }
        });
        assert_eq!(engagement_summary(&v), "reactions=12 comments=3 shares=2");
    }
}
