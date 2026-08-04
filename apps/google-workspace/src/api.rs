//! REST API — the single engine behind both the web UI and the MCP server, so
//! a person clicking and an agent calling always hit the same code paths.
//!
//! Every endpoint answers HTTP 200 with `{ ok: true, … }` or
//! `{ ok: false, error }`; the OAuth callback is the one exception (redirects).

use axum::{
    extract::{Query, State},
    response::{Html, IntoResponse, Redirect},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

use crate::db::Db;
use crate::google::{auth_url, Google, SCOPES};

pub struct AppState {
    pub google: Google,
    pub mcp_tx: tokio::sync::broadcast::Sender<String>,
    /// Daemon base URL (set by the Space runtime) — target of calendar sync.
    pub senclaw_base: String,
    /// Port this app listens on; used to build the OAuth redirect URI.
    pub port: u16,
}

pub fn make_state(db: Arc<Db>, port: u16) -> Arc<AppState> {
    let (mcp_tx, _) = tokio::sync::broadcast::channel(100);
    let senclaw_base = std::env::var("SENCLAW_BASE_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:18788".into())
        .trim_end_matches('/')
        .to_string();
    Arc::new(AppState {
        google: Google::new(db),
        mcp_tx,
        senclaw_base,
        port,
    })
}

fn ok(v: Value) -> Json<Value> {
    let mut obj = json!({ "ok": true });
    if let (Some(o), Some(extra)) = (obj.as_object_mut(), v.as_object()) {
        for (k, val) in extra {
            o.insert(k.clone(), val.clone());
        }
    }
    Json(obj)
}

fn err(e: impl std::fmt::Display) -> Json<Value> {
    Json(json!({ "ok": false, "error": e.to_string() }))
}

fn res(r: anyhow::Result<Value>) -> Json<Value> {
    match r {
        Ok(v) => ok(v),
        Err(e) => err(e),
    }
}

// ---- status / health ----

async fn status(State(s): State<Arc<AppState>>) -> Json<Value> {
    let db = &s.google.db;
    Json(json!({
        "status": "ok",
        "app": "google-workspace",
        "connected": db.connected(),
        "hasCredentials": !db.client_id().is_empty() && !db.client_secret().is_empty(),
        "senclaw": s.senclaw_base,
    }))
}

// ---- settings ----

async fn get_settings(State(s): State<Arc<AppState>>) -> Json<Value> {
    ok(json!({
        "settings": s.google.db.masked_settings(),
        "lastRun": s.google.db.last_run(),
    }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SettingsBody {
    client_id: Option<String>,
    client_secret: Option<String>,
    days: Option<u32>,
    services: Option<Vec<String>>,
}

async fn set_settings(State(s): State<Arc<AppState>>, Json(b): Json<SettingsBody>) -> Json<Value> {
    let db = &s.google.db;
    let apply = || -> anyhow::Result<()> {
        if let Some(v) = &b.client_id {
            db.set_setting("client_id", v.trim())?;
        }
        if let Some(v) = &b.client_secret {
            // "***" is the mask we emit — never store it back
            if v != "***" {
                db.set_setting("client_secret", v.trim())?;
            }
        }
        if let Some(v) = b.days {
            db.set_setting("days", &v.clamp(1, 90).to_string())?;
        }
        if let Some(v) = &b.services {
            db.set_setting("services", &serde_json::to_string(v)?)?;
        }
        Ok(())
    };
    match apply() {
        Ok(()) => ok(json!({ "settings": db.masked_settings() })),
        Err(e) => err(e),
    }
}

// ---- auth ----

fn redirect_uri(port: u16) -> String {
    format!("http://127.0.0.1:{port}/api/auth/callback")
}

async fn auth_start(State(s): State<Arc<AppState>>) -> impl IntoResponse {
    let db = &s.google.db;
    if db.client_id().is_empty() || db.client_secret().is_empty() {
        return err("Chưa cấu hình Client ID / Client Secret — mở Settings và điền trước.")
            .into_response();
    }
    Redirect::temporary(&auth_url(&db.client_id(), &redirect_uri(s.port))).into_response()
}

async fn auth_url_route(State(s): State<Arc<AppState>>) -> Json<Value> {
    let db = &s.google.db;
    if db.client_id().is_empty() {
        return err("Chưa cấu hình Client ID.");
    }
    ok(json!({
        "url": auth_url(&db.client_id(), &redirect_uri(s.port)),
        "redirectUri": redirect_uri(s.port),
        "scopes": SCOPES,
    }))
}

/// Ask the daemon to open a URL in the HOST system browser (layer 3 of
/// docs/space-app-open-external.md). This is the only path that works
/// regardless of webview version/bridge/popup policy — and for OAuth the
/// browser MUST be on the daemon's machine anyway, because the redirect URI
/// is 127.0.0.1:4310 there.
async fn open_in_system_browser(s: &AppState, url: &str) -> anyhow::Result<()> {
    let endpoint = format!("{}/api/ui/open-url", s.senclaw_base);
    let res = s
        .google
        .http
        .post(&endpoint)
        .json(&json!({ "url": url }))
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("daemon unreachable ({endpoint}): {e}"))?;
    if !res.status().is_success() {
        let code = res.status();
        let body = res.text().await.unwrap_or_default();
        anyhow::bail!("daemon open-url {code}: {body}");
    }
    Ok(())
}

/// One-shot OAuth kick-off: build the consent URL and have the daemon open it
/// in the system browser. The UI falls back to client-side openExternal when
/// this fails (e.g. daemon too old for /api/ui/open-url).
async fn auth_open(State(s): State<Arc<AppState>>) -> Json<Value> {
    let db = &s.google.db;
    if db.client_id().is_empty() || db.client_secret().is_empty() {
        return err("Chưa cấu hình Client ID / Client Secret — mở Cài đặt và điền trước.");
    }
    let url = auth_url(&db.client_id(), &redirect_uri(s.port));
    match open_in_system_browser(&s, &url).await {
        Ok(()) => ok(json!({ "opened": true, "url": url })),
        Err(e) => Json(json!({ "ok": false, "error": e.to_string(), "url": url })),
    }
}

#[derive(Deserialize)]
struct OpenUrlBody {
    url: String,
}

/// Generic "open this link on the host browser" for the UI (external links
/// when neither the Flutter bridge nor window.open is available).
async fn open_url_route(State(s): State<Arc<AppState>>, Json(b): Json<OpenUrlBody>) -> Json<Value> {
    let url = b.url.trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return err("Chỉ nhận URL http/https.");
    }
    match open_in_system_browser(&s, url).await {
        Ok(()) => ok(json!({ "opened": true })),
        Err(e) => err(e),
    }
}

async fn auth_callback(
    State(s): State<Arc<AppState>>,
    Query(q): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let Some(code) = q.get("code").filter(|c| !c.is_empty()) else {
        let detail = q
            .get("error")
            .cloned()
            .unwrap_or_else(|| "missing code".into());
        return Html(format!(
            "<!doctype html><meta charset=utf-8><title>Google Workspace</title>\
             <body style=\"font-family:system-ui;display:grid;place-items:center;min-height:90vh;background:#f5f7fb\">\
             <div style=\"text-align:center;max-width:480px\">\
             <div style=\"font-size:44px\">⚠️</div>\
             <h2 style=\"margin:8px 0\">OAuth thất bại</h2>\
             <p style=\"color:#555\">{detail}</p>\
             <p style=\"color:#888;font-size:13px\">Đóng tab này rồi thử lại từ app Google Workspace trong SenClaw.</p>\
             </div></body>"
        ))
        .into_response();
    };
    match s.google.exchange_code(code, &redirect_uri(s.port)).await {
        Ok(_) => {
            let _ = s.google.db.add_run("auth", "completed", "OAuth connected");
            // The consent flow runs in the SYSTEM browser (openExternal), so
            // this page is a dead end there — the app in SenClaw picks the
            // connection up by polling. Tell the user to just close the tab.
            Html(
                "<!doctype html><meta charset=utf-8><title>Google Workspace</title>\
                 <body style=\"font-family:system-ui;display:grid;place-items:center;min-height:90vh;background:#f5f7fb\">\
                 <div style=\"text-align:center;max-width:420px\">\
                 <div style=\"font-size:44px\">✅</div>\
                 <h2 style=\"margin:8px 0\">Kết nối Google thành công</h2>\
                 <p style=\"color:#555\">Quay lại SenClaw — app Google Workspace sẽ tự nhận kết nối trong vài giây. Bạn có thể đóng tab này.</p>\
                 </div></body>",
            )
            .into_response()
        }
        Err(e) => Html(format!(
            "<!doctype html><meta charset=utf-8><title>Google Workspace</title>\
             <body style=\"font-family:system-ui;display:grid;place-items:center;min-height:90vh;background:#f5f7fb\">\
             <div style=\"text-align:center;max-width:480px\">\
             <div style=\"font-size:44px\">⚠️</div>\
             <h2 style=\"margin:8px 0\">Đổi code thất bại</h2>\
             <p style=\"color:#555\">{e}</p>\
             <p style=\"color:#888;font-size:13px\">Đóng tab này, kiểm tra Client ID/Secret trong app rồi thử lại.</p>\
             </div></body>"
        ))
        .into_response(),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenBody {
    access_token: String,
    #[serde(default)]
    refresh_token: String,
}

async fn auth_token(State(s): State<Arc<AppState>>, Json(b): Json<TokenBody>) -> Json<Value> {
    let token = b.access_token.trim().to_string();
    if token.is_empty() {
        return err("Thiếu accessToken.");
    }
    let db = &s.google.db;
    let mut tokens = db.tokens();
    tokens.access_token = token;
    if !b.refresh_token.trim().is_empty() {
        tokens.refresh_token = b.refresh_token.trim().to_string();
    }
    tokens.expires_at = 0; // unknown — pasted tokens carry no expiry
    match db.save_tokens(&tokens) {
        Ok(()) => {
            let _ = db.add_run("auth", "completed", "access token saved");
            ok(json!({ "settings": db.masked_settings() }))
        }
        Err(e) => err(e),
    }
}

async fn auth_disconnect(State(s): State<Arc<AppState>>) -> Json<Value> {
    match s.google.db.clear_tokens() {
        Ok(()) => ok(json!({ "settings": s.google.db.masked_settings() })),
        Err(e) => err(e),
    }
}

// ---- gmail / calendar / drive (UI proxies over the same engine MCP uses) ----

#[derive(Deserialize)]
struct ListQuery {
    max: Option<u32>,
    q: Option<String>,
    days: Option<u32>,
}

async fn gmail_list(State(s): State<Arc<AppState>>, Query(p): Query<ListQuery>) -> Json<Value> {
    res(s
        .google
        .list_emails(p.max.unwrap_or(10), p.q.as_deref().unwrap_or(""))
        .await)
}

async fn gmail_read(
    State(s): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Json<Value> {
    res(s
        .google
        .read_email(&id)
        .await
        .map(|m| json!({ "email": m })))
}

#[derive(Deserialize)]
struct SendBody {
    to: String,
    subject: String,
    body: String,
}

async fn gmail_send(State(s): State<Arc<AppState>>, Json(b): Json<SendBody>) -> Json<Value> {
    let r = s.google.send_email(&b.to, &b.subject, &b.body).await;
    if r.is_ok() {
        let _ = s
            .google
            .db
            .add_run("gmail", "completed", &format!("sent to {}", b.to));
    }
    res(r.map(|m| json!({ "message": m })))
}

async fn calendar_list(State(s): State<Arc<AppState>>, Query(p): Query<ListQuery>) -> Json<Value> {
    res(s
        .google
        .list_events(p.max.unwrap_or(10), p.days.unwrap_or(0))
        .await)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EventBody {
    summary: String,
    #[serde(default)]
    description: String,
    start_time: String,
    end_time: String,
}

async fn calendar_create(State(s): State<Arc<AppState>>, Json(b): Json<EventBody>) -> Json<Value> {
    res(s
        .google
        .create_event(&b.summary, &b.description, &b.start_time, &b.end_time)
        .await
        .map(|e| json!({ "event": e })))
}

async fn drive_list(State(s): State<Arc<AppState>>, Query(p): Query<ListQuery>) -> Json<Value> {
    res(s
        .google
        .list_files(p.max.unwrap_or(10), p.q.as_deref().unwrap_or(""))
        .await)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UploadBody {
    name: String,
    #[serde(default)]
    mime_type: String,
    text_content: String,
}

async fn drive_upload(State(s): State<Arc<AppState>>, Json(b): Json<UploadBody>) -> Json<Value> {
    res(s
        .google
        .upload_file(&b.name, &b.mime_type, &b.text_content)
        .await
        .map(|f| json!({ "file": f })))
}

// ---- sync ----

#[derive(Deserialize, Default)]
struct SyncBody {
    services: Option<Vec<String>>,
    days: Option<u32>,
}

/// Fetch a fresh snapshot per enabled service and, for calendar, push the
/// events window into the daemon's Space Calendar (its own dedup applies).
pub async fn run_sync(state: &AppState, services: Vec<String>, days: u32) -> Value {
    let db = &state.google.db;
    let mut results = serde_json::Map::new();

    for service in services {
        let outcome: anyhow::Result<Value> = match service.as_str() {
            "gmail" => state
                .google
                .list_emails(20, "")
                .await
                .map(|v| json!({ "status": "completed", "emails": v["count"] })),
            "drive" => state
                .google
                .list_files(20, "")
                .await
                .map(|v| json!({ "status": "completed", "files": v["count"] })),
            "calendar" => {
                match state.google.access_token().await {
                    Ok(token) => {
                        // Delegate to the daemon so events land in Space Calendar.
                        let url = format!("{}/api/space/sync/google-calendar", state.senclaw_base);
                        match state
                            .google
                            .http
                            .post(&url)
                            .json(&json!({ "token": token, "days": days }))
                            .send()
                            .await
                        {
                            Ok(r) if r.status().is_success() => {
                                let v: Value = r.json().await.unwrap_or(json!({}));
                                Ok(json!({ "status": "completed", "space": v }))
                            }
                            Ok(r) => {
                                let code = r.status();
                                let body = r.text().await.unwrap_or_default();
                                Err(anyhow::anyhow!("daemon {code}: {body}"))
                            }
                            Err(e) => Err(anyhow::anyhow!("daemon unreachable: {e}")),
                        }
                    }
                    Err(e) => Err(e),
                }
            }
            other => {
                Ok(json!({ "status": "skipped", "message": format!("unknown service '{other}'") }))
            }
        };

        match outcome {
            Ok(v) => {
                let _ = db.add_run(&service, "completed", &v.to_string());
                results.insert(service, v);
            }
            Err(e) => {
                let _ = db.add_run(&service, "error", &e.to_string());
                results.insert(
                    service,
                    json!({ "status": "error", "error": e.to_string() }),
                );
            }
        }
    }

    json!({ "status": "completed", "days": days, "results": results })
}

async fn sync_route(State(s): State<Arc<AppState>>, body: Option<Json<SyncBody>>) -> Json<Value> {
    let b = body.map(|Json(b)| b).unwrap_or_default();
    let services = b.services.unwrap_or_else(|| s.google.db.services());
    let days = b.days.unwrap_or_else(|| s.google.db.days());
    ok(run_sync(&s, services, days).await)
}

async fn runs(State(s): State<Arc<AppState>>) -> Json<Value> {
    ok(json!({ "runs": s.google.db.recent_runs(20) }))
}

pub fn api_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/status", get(status))
        .route("/settings", get(get_settings).post(set_settings))
        .route("/auth", get(auth_start))
        .route("/auth/url", get(auth_url_route))
        .route("/auth/open", post(auth_open))
        .route("/auth/callback", get(auth_callback))
        .route("/open-url", post(open_url_route))
        .route("/auth/token", post(auth_token))
        .route("/auth/disconnect", post(auth_disconnect))
        .route("/gmail/messages", get(gmail_list))
        .route("/gmail/messages/:id", get(gmail_read))
        .route("/gmail/send", post(gmail_send))
        .route("/calendar/events", get(calendar_list).post(calendar_create))
        .route("/drive/files", get(drive_list))
        .route("/drive/upload", post(drive_upload))
        .route("/sync", post(sync_route))
        .route("/runs", get(runs))
        .route(
            "/mcp/sse",
            get(crate::mcp::mcp_sse).post(crate::mcp::mcp_message),
        )
        .route("/mcp/message", post(crate::mcp::mcp_message))
        .with_state(state)
}
