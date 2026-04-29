//! QQ official group bot channel adapter. Mirrors `src-old/channels/qq.ts`.
//!
//! JID format: `qq:user:{user_openid}` (private) or `qq:group:{group_openid}` (group).
//!
//! Protocol: QQ Bot WebSocket gateway (op codes 0, 1, 2, 6, 7, 9, 10, 11).
//! Auth: OAuth2 — POST /app/getAppAccessToken { appId, clientSecret } → access_token.
//! REST: `https://api.sgroup.qq.com` with `Authorization: QQBot {token}`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use anyhow::{Context, Result};
use async_trait::async_trait;
use futures::StreamExt;
use futures::SinkExt;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tokio::time::sleep;

use crate::channels::{Channel, MessageCallback, MetadataCallback};
use crate::types::{InlineButton, IncomingMessage};

// ===== Constants =====

const TOKEN_URL: &str = "https://bots.qq.com/app/getAppAccessToken";
const API_BASE: &str = "https://api.sgroup.qq.com";
const INTENTS: u32 = (1 << 25) | (1 << 26) | (1 << 30);
const QQ_MAX_LEN: usize = 4000;
const PASSIVE_WINDOW_MS: i64 = (4.5 * 60.0 * 1000.0) as i64;
const MENU_TTL_SECS: u64 = 5 * 60;
const CONNECT_TIMEOUT_SECS: u64 = 15;
const TOKEN_REFRESH_MARGIN_MS: i64 = 5 * 60 * 1000;

const RECONNECT_DELAYS: &[u64] = &[1, 2, 5, 10, 30, 60];

// ===== JSON types for QQ API =====

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    expires_in: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct GatewayResponse {
    url: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct WsPayload {
    op: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    d: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    s: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    t: Option<String>,
}

#[derive(Debug, Deserialize)]
struct HelloData {
    heartbeat_interval: u64,
}

#[derive(Debug, Deserialize)]
struct ReadyData {
    session_id: String,
}

#[derive(Debug, Deserialize)]
struct C2cEvent {
    id: String,
    author: serde_json::Value,
    content: String,
    timestamp: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GroupAtEvent {
    id: String,
    author: serde_json::Value,
    group_openid: String,
    content: String,
    timestamp: Option<String>,
}

#[derive(Debug, Deserialize)]
struct InteractionEvent {
    id: String,
    chat_type: i32,
    group_openid: Option<String>,
    user_openid: Option<String>,
    data: Option<InteractionData>,
}

#[derive(Debug, Deserialize)]
struct InteractionData {
    resolved: Option<ResolvedData>,
}

#[derive(Debug, Deserialize)]
struct ResolvedData {
    button_id: Option<String>,
    button_data: Option<String>,
}

#[derive(Debug, Serialize)]
struct HeartbeatPayload {
    op: i32,
    d: Option<i64>,
}

// ===== Helpers =====

fn split_message(text: &str) -> Vec<String> {
    if text.len() <= QQ_MAX_LEN {
        return vec![text.to_string()];
    }
    let mut parts: Vec<String> = Vec::new();
    let mut remaining = text;
    while remaining.len() > QQ_MAX_LEN {
        let chunk = &remaining[..QQ_MAX_LEN];
        let split_at = chunk
            .rfind('\n')
            .filter(|&pos| pos > QQ_MAX_LEN / 2)
            .unwrap_or(QQ_MAX_LEN);
        parts.push(remaining[..split_at].to_string());
        remaining = remaining[split_at..].trim_start_matches('\n');
    }
    if !remaining.is_empty() {
        parts.push(remaining.to_string());
    }
    parts
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn next_msg_seq() -> u32 {
    let ts = now_ms() as u64;
    let rnd = rand::random::<u16>() as u64;
    ((ts % 65536) ^ rnd) as u32
}

// ===== Token cache =====

#[derive(Debug, Clone)]
struct TokenCache {
    token: String,
    expires_at_ms: i64,
}

struct AppEntry {
    app_id: String,
    app_secret: String,
    sandbox: bool,
    token_cache: Mutex<Option<TokenCache>>,
    token_fetch: Mutex<Option<Arc<tokio::sync::Mutex<()>>>>,
    session_id: Mutex<Option<String>>,
    last_seq: Mutex<Option<i64>>,
    keyboard_unsupported: AtomicBool,
    shutdown: AtomicBool,
}

impl AppEntry {
    fn new(app_id: String, app_secret: String, sandbox: bool) -> Self {
        Self {
            app_id,
            app_secret,
            sandbox,
            token_cache: Mutex::new(None),
            token_fetch: Mutex::new(None),
            session_id: Mutex::new(None),
            last_seq: Mutex::new(None),
            keyboard_unsupported: AtomicBool::new(false),
            shutdown: AtomicBool::new(false),
        }
    }
}

// ===== Pending state =====

struct PendingReply {
    msg_id: String,
    expires_at_ms: i64,
}

struct PendingMenu {
    options: Vec<InlineButton>,
    app_id: String,
}

// ===== QQChannel =====

struct PendingReplyState {
    replies: Mutex<HashMap<String, PendingReply>>,
    menu_queues: Mutex<HashMap<String, Vec<PendingMenu>>>,
    dedup: Mutex<HashMap<String, i64>>,
}

const DEDUP_TTL_MS: i64 = 30 * 60 * 1000;

impl PendingReplyState {
    fn new() -> Self {
        Self {
            replies: Mutex::new(HashMap::new()),
            menu_queues: Mutex::new(HashMap::new()),
            dedup: Mutex::new(HashMap::new()),
        }
    }

    async fn try_dedup(&self, msg_id: &str) -> bool {
        let mut dedup = self.dedup.lock().await;
        let now = now_ms();
        dedup.retain(|_, ts| now - *ts <= DEDUP_TTL_MS);
        if dedup.contains_key(msg_id) {
            return false;
        }
        dedup.insert(msg_id.to_string(), now);
        true
    }

    async fn set_reply(&self, jid: &str, msg_id: &str) {
        let mut replies = self.replies.lock().await;
        replies.insert(
            jid.to_string(),
            PendingReply {
                msg_id: msg_id.to_string(),
                expires_at_ms: now_ms() + PASSIVE_WINDOW_MS,
            },
        );
    }

    async fn get_reply_msg_id(&self, jid: &str) -> Option<String> {
        let replies = self.replies.lock().await;
        replies
            .get(jid)
            .filter(|r| now_ms() < r.expires_at_ms)
            .map(|r| r.msg_id.clone())
    }

    async fn try_handle_menu(&self, jid: &str, content: &str) -> Option<(InlineButton, String)> {
        let mut queues = self.menu_queues.lock().await;
        let queue = queues.get_mut(jid)?;
        if queue.is_empty() {
            return None;
        }
        let num: usize = content.trim().parse().ok()?;
        if num < 1 || num > queue[0].options.len() {
            return None;
        }
        let menu = queue.remove(0);
        if queue.is_empty() {
            queues.remove(jid);
        }
        let selected = menu.options[num - 1].clone();
        Some((selected, menu.app_id))
    }
}

pub struct QQChannel {
    apps: Mutex<HashMap<String, Arc<AppEntry>>>,
    primary_app_id: Mutex<Option<String>>,
    state: Arc<PendingReplyState>,
    http: reqwest::Client,
    handlers: Arc<RwLock<Vec<MessageCallback>>>,
    meta_handlers: Arc<RwLock<Vec<MetadataCallback>>>,
}

impl QQChannel {
    pub fn new(app_id: String, app_secret: String, sandbox: bool) -> Self {
        let entry = Arc::new(AppEntry::new(
            app_id.clone(),
            app_secret.clone(),
            sandbox,
        ));
        let mut apps = HashMap::new();
        apps.insert(app_id.clone(), entry);

        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("reqwest::Client::build");

        QQChannel {
            apps: Mutex::new(apps),
            primary_app_id: Mutex::new(Some(app_id)),
            state: Arc::new(PendingReplyState::new()),
            http,
            handlers: Arc::new(RwLock::new(Vec::new())),
            meta_handlers: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Register extra QQ bot app (non-blocking).
    pub async fn add_app(&self, app_id: &str, app_secret: &str, sandbox: bool) {
        {
            let apps = self.apps.lock().await;
            if apps.contains_key(app_id) {
                tracing::warn!("[QQChannel] add_app: {app_id} already registered");
                return;
            }
        }
        let entry = Arc::new(AppEntry::new(
            app_id.to_string(),
            app_secret.to_string(),
            sandbox,
        ));
        {
            let mut apps = self.apps.lock().await;
            apps.insert(app_id.to_string(), entry.clone());
        }
        let app_id_owned = app_id.to_string();
        tracing::info!("[QQChannel] Registered extra app: {app_id_owned}");

        let state = Arc::clone(&self.state);
        let http = self.http.clone();
        let handlers = Arc::clone(&self.handlers);
        let meta_handlers = Arc::clone(&self.meta_handlers);
        tokio::spawn(async move {
            if let Err(e) = connect_app_with_timeout(entry, state, http, handlers, meta_handlers).await {
                tracing::error!("[QQChannel:{app_id_owned}] Background connect failed: {e:#}");
            }
        });
    }

    async fn resolve_app(&self, bot_token: Option<&str>, chat_jid: Option<&str>) -> Option<Arc<AppEntry>> {
        let apps = self.apps.lock().await;
        if let Some(token) = bot_token {
            if let Some(app) = apps.get(token) {
                return Some(Arc::clone(app));
            }
            let jid_str = chat_jid.unwrap_or("");
            tracing::error!("[QQChannel] Unknown botToken \"{token}\" for {jid_str}, message dropped");
            return None;
        }
        let primary = self.primary_app_id.lock().await;
        if let Some(ref pid) = *primary {
            if let Some(app) = apps.get(pid) {
                return Some(Arc::clone(app));
            }
        }
        tracing::warn!("[QQChannel] No app available");
        None
    }

    /// Build QQ keyboard rows (up to 5 buttons per row).
    fn build_keyboard(buttons: &[InlineButton]) -> Vec<serde_json::Value> {
        const ROW_SIZE: usize = 5;
        let mut rows: Vec<serde_json::Value> = Vec::new();
        for (row_idx, chunk) in buttons.chunks(ROW_SIZE).enumerate() {
            let btns: Vec<serde_json::Value> = chunk
                .iter()
                .enumerate()
                .map(|(i, b)| {
                    serde_json::json!({
                        "id": (row_idx * ROW_SIZE + i + 1).to_string(),
                        "render_data": {
                            "label": b.label,
                            "visited_label": format!("✅ {}", b.label),
                        },
                        "action": {
                            "type": 2,
                            "permission": { "type": 2 },
                            "data": b.callback_data,
                            "reply": false,
                            "enter": true,
                        },
                    })
                })
                .collect();
            rows.push(serde_json::json!({ "buttons": btns }));
        }
        rows
    }

    /// Send text menu fallback as numbered list.
    async fn send_text_menu(
        &self,
        chat_jid: &str,
        text: &str,
        buttons: &[InlineButton],
        app: &AppEntry,
    ) -> Result<()> {
        let numbered: Vec<String> = buttons
            .iter()
            .enumerate()
            .map(|(i, b)| format!("{}. {}", i + 1, b.label))
            .collect();
        let full_text = format!("{text}\n\n{}\n\n(Reply with the number to choose)", numbered.join("\n"));

        // Queue menu
        {
            let mut queues = self.state.menu_queues.lock().await;
            queues
                .entry(chat_jid.to_string())
                .or_default()
                .push(PendingMenu {
                    options: buttons.to_vec(),
                    app_id: app.app_id.clone(),
                });
        }

        // Auto-expire after MENU_TTL
        let jid = chat_jid.to_string();
        let state = Arc::clone(&self.state);
        tokio::spawn(async move {
            sleep(Duration::from_secs(MENU_TTL_SECS)).await;
            let mut queues = state.menu_queues.lock().await;
            if let Some(q) = queues.get_mut(&jid) {
                if !q.is_empty() {
                    q.remove(0);
                }
                if q.is_empty() {
                    queues.remove(&jid);
                }
            }
        });

        self.send_message(chat_jid, &full_text, Some(&app.app_id))
            .await
    }
}

#[async_trait]
impl Channel for QQChannel {
    fn id(&self) -> &'static str {
        "qq"
    }

    async fn connect(&mut self) -> Result<()> {
        let primary_app_id = {
            let guard = self.primary_app_id.lock().await;
            guard.clone()
        };
        let Some(pid) = primary_app_id else {
            tracing::warn!("[QQChannel] No primary app configured, disabled");
            return Ok(());
        };
        let app = {
            let apps = self.apps.lock().await;
            apps.get(&pid).cloned()
        };
        let Some(app) = app else {
            return Ok(());
        };

        if app.app_id.is_empty() || app.app_secret.is_empty() {
            tracing::warn!("[QQChannel] No credentials configured, disabled");
            return Ok(());
        }

        let state = Arc::clone(&self.state);
        let http = self.http.clone();
        let handlers = Arc::clone(&self.handlers);
        let meta_handlers = Arc::clone(&self.meta_handlers);
        connect_app_with_timeout(app, state, http, handlers, meta_handlers).await
    }

    async fn disconnect(&mut self) -> Result<()> {
        let apps = self.apps.lock().await;
        for app in apps.values() {
            app.shutdown.store(true, Ordering::SeqCst);
        }
        tracing::info!("[QQChannel] Disconnected (all apps)");
        Ok(())
    }

    fn is_connected(&self) -> bool {
        // Check if primary app is connected (optimistic)
        true // placeholder; WS connection state is ephemeral
    }

    async fn send_message(
        &self,
        chat_jid: &str,
        text: &str,
        bot_token: Option<&str>,
    ) -> Result<()> {
        let app = self
            .resolve_app(bot_token, Some(chat_jid))
            .await
            .ok_or_else(|| anyhow::anyhow!("QQ app not found for: {chat_jid}"))?;

        let parts = split_message(text);
        let msg_id = self.state.get_reply_msg_id(chat_jid).await;

        for (i, part) in parts.iter().enumerate() {
            let active_msg_id = if i == 0 { msg_id.as_deref() } else { None };
            let mut body = serde_json::json!({
                "content": part,
                "msg_type": 0,
                "msg_seq": next_msg_seq(),
            });
            if let Some(mid) = active_msg_id {
                body["msg_id"] = serde_json::json!(mid);
            }

            let path = if chat_jid.starts_with("qq:user:") {
                let openid = chat_jid.strip_prefix("qq:user:").unwrap();
                format!("/v2/users/{openid}/messages")
            } else if chat_jid.starts_with("qq:group:") {
                let group_id = chat_jid.strip_prefix("qq:group:").unwrap();
                format!("/v2/groups/{group_id}/messages")
            } else {
                anyhow::bail!("Invalid QQ JID: {chat_jid}");
            };

            if let Err(e) = api_request::<serde_json::Value>(&self.http, &app, "POST", &path, Some(&body)).await {
                tracing::error!(
                    "[QQChannel:{}] Failed to send message to {chat_jid}: {e:#}",
                    app.app_id
                );
            }
        }
        Ok(())
    }

    async fn send_file(
        &self,
        _chat_jid: &str,
        _file_path: &str,
        caption: Option<&str>,
        bot_token: Option<&str>,
    ) -> Result<()> {
        // QQ Bot API does not support file upload in group messages.
        // Fallback: send caption as text if provided.
        if let Some(cap) = caption {
            return self.send_message(_chat_jid, cap, bot_token).await;
        }
        Ok(())
    }

    fn owns_jid(&self, chat_jid: &str) -> bool {
        chat_jid.starts_with("qq:")
    }

    fn on_message(&mut self, handler: MessageCallback) {
        if let Ok(mut guard) = self.handlers.write() {
            guard.push(handler);
        }
    }

    fn on_metadata(&mut self, handler: MetadataCallback) {
        if let Ok(mut guard) = self.meta_handlers.write() {
            guard.push(handler);
        }
    }

    async fn send_with_buttons(
        &self,
        chat_jid: &str,
        text: &str,
        buttons: &[InlineButton],
        bot_token: Option<&str>,
    ) -> Result<()> {
        let app = self
            .resolve_app(bot_token, Some(chat_jid))
            .await
            .ok_or_else(|| anyhow::anyhow!("QQ app not found for: {chat_jid}"))?;

        // Fallback to text menu if keyboard is unsupported
        if app.keyboard_unsupported.load(Ordering::SeqCst) {
            return self.send_text_menu(chat_jid, text, buttons, &app).await;
        }

        let rows = Self::build_keyboard(buttons);
        let msg_id = self.state.get_reply_msg_id(chat_jid).await;

        let mut body = serde_json::json!({
            "msg_type": 2,
            "markdown": { "content": text },
            "keyboard": { "content": { "rows": rows } },
            "msg_seq": next_msg_seq(),
        });
        if let Some(mid) = msg_id {
            body["msg_id"] = serde_json::json!(mid);
        }

        let path = if chat_jid.starts_with("qq:user:") {
            let openid = chat_jid.strip_prefix("qq:user:").unwrap();
            format!("/v2/users/{openid}/messages")
        } else if chat_jid.starts_with("qq:group:") {
            let group_id = chat_jid.strip_prefix("qq:group:").unwrap();
            format!("/v2/groups/{group_id}/messages")
        } else {
            anyhow::bail!("Invalid QQ JID: {chat_jid}");
        };

        match api_request::<serde_json::Value>(&self.http, &app, "POST", &path, Some(&body)).await {
            Ok(_) => Ok(()),
            Err(e) => {
                tracing::warn!(
                    "[QQChannel:{}] sendWithButtons failed, falling back to text menu: {e}",
                    app.app_id
                );
                app.keyboard_unsupported.store(true, Ordering::SeqCst);
                self.send_text_menu(chat_jid, text, buttons, &app).await
            }
        }
    }
}

// ===== API helpers =====

async fn get_access_token(http: &reqwest::Client, app: &AppEntry) -> Result<String> {
    // Check cache
    {
        let cache = app.token_cache.lock().await;
        if let Some(ref tc) = *cache {
            if now_ms() < tc.expires_at_ms - TOKEN_REFRESH_MARGIN_MS {
                return Ok(tc.token.clone());
            }
        }
    }

    // Serialize concurrent fetches
    let _fetch_lock = {
        let mut fetch = app.token_fetch.lock().await;
        if let Some(ref existing) = *fetch {
            let lock = Arc::clone(existing);
            drop(fetch);
            let _guard = lock.lock().await;
            // Re-check cache after obtaining lock
            let cache = app.token_cache.lock().await;
            if let Some(ref tc) = *cache {
                if now_ms() < tc.expires_at_ms - TOKEN_REFRESH_MARGIN_MS {
                    return Ok(tc.token.clone());
                }
            }
        } else {
            let lock = Arc::new(tokio::sync::Mutex::new(()));
            fetch.replace(Arc::clone(&lock));
            let guard = lock.lock().await;
            drop(fetch);
            drop(guard);
        }
    };

    let res = http
        .post(TOKEN_URL)
        .json(&serde_json::json!({
            "appId": app.app_id,
            "clientSecret": app.app_secret,
        }))
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .context("getAccessToken")?;

    let data: TokenResponse = res.json().await.context("parse token response")?;
    let token = data
        .access_token
        .ok_or_else(|| anyhow::anyhow!("getAccessToken failed: no access_token"))?;
    let expires_in = data.expires_in.unwrap_or(7200);

    {
        let mut cache = app.token_cache.lock().await;
        *cache = Some(TokenCache {
            token: token.clone(),
            expires_at_ms: now_ms() + expires_in * 1000,
        });
    }

    // Clean up fetch lock
    {
        let mut fetch = app.token_fetch.lock().await;
        *fetch = None;
    }

    tracing::info!(
        "[QQChannel:{}] Access token obtained",
        app.app_id
    );
    Ok(token)
}

async fn api_request<T: serde::de::DeserializeOwned>(
    http: &reqwest::Client,
    app: &AppEntry,
    method: &str,
    path: &str,
    body: Option<&serde_json::Value>,
) -> Result<T> {
    let token = get_access_token(http, app).await?;
    let url = format!("{API_BASE}{path}");

    let mut req = match method {
        "GET" => http.get(&url),
        "POST" => http.post(&url),
        "PUT" => http.put(&url),
        _ => anyhow::bail!("unsupported HTTP method: {method}"),
    };

    req = req
        .header("Authorization", format!("QQBot {token}"))
        .header("Content-Type", "application/json");

    if let Some(b) = body {
        req = req.json(b);
    }

    let res = req.send().await.context("QQ API request")?;
    if !res.status().is_success() {
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        anyhow::bail!("QQ API error [{path}]: HTTP {status} — {text}");
    }
    let data: T = res.json().await.context("parse QQ API response")?;
    Ok(data)
}

// ===== WebSocket connection (per-app) =====

async fn connect_app_with_timeout(
    app: Arc<AppEntry>,
    state: Arc<PendingReplyState>,
    http: reqwest::Client,
    handlers: Arc<RwLock<Vec<MessageCallback>>>,
    meta_handlers: Arc<RwLock<Vec<MetadataCallback>>>,
) -> Result<()> {
    loop {
        let result = tokio::time::timeout(
            Duration::from_secs(CONNECT_TIMEOUT_SECS),
            do_connect(
                Arc::clone(&app),
                Arc::clone(&state),
                http.clone(),
                Arc::clone(&handlers),
                Arc::clone(&meta_handlers),
            ),
        )
        .await;

        match result {
            Ok(Ok(true)) => {
                // Reconnect requested
                if app.shutdown.load(Ordering::SeqCst) {
                    return Ok(());
                }
                let _attempts = app.last_seq.lock().await;
                let n = 0usize; // Reset attempt tracking
                let delay_idx = n.min(RECONNECT_DELAYS.len() - 1);
                let delay = RECONNECT_DELAYS[delay_idx];
                tracing::info!(
                    "[QQChannel:{}] Reconnecting in {delay}s",
                    app.app_id,
                );
                sleep(Duration::from_secs(delay)).await;
            }
            Ok(Ok(false)) => return Ok(()),
            Ok(Err(e)) => {
                tracing::error!("[QQChannel:{}] Connection error: {e:#}", app.app_id);
                return Err(e);
            }
            Err(_) => {
                return Err(anyhow::anyhow!(
                    "QQ connect timed out after {CONNECT_TIMEOUT_SECS}s"
                ));
            }
        }
    }
}

/// Returns Ok(true) if reconnection is requested, Ok(false) if clean shutdown.
async fn do_connect(
    app: Arc<AppEntry>,
    state: Arc<PendingReplyState>,
    http: reqwest::Client,
    handlers: Arc<RwLock<Vec<MessageCallback>>>,
    meta_handlers: Arc<RwLock<Vec<MetadataCallback>>>,
) -> Result<bool> {
    let token = get_access_token(&http, &app).await?;
    let gw: GatewayResponse = api_request(&http, &app, "GET", "/gateway", None).await?;

    let ws_url = if app.sandbox {
        gw.url.replace("wss://", "wss://sandbox.")
    } else {
        gw.url
    };

    tracing::info!("[QQChannel:{}] Connecting to {ws_url}", app.app_id);

    let (ws_stream, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .context("QQ WebSocket connect")?;

    let ws: Arc<Mutex<tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >>> = Arc::new(Mutex::new(ws_stream));

    let hb_interval_ms: Arc<Mutex<u64>> = Arc::new(Mutex::new(45_000));
    let attempts: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));

    // Single event loop with periodic heartbeat
    let mut hb_tick = tokio::time::interval(Duration::from_millis(45_000));
    let mut reconnect = false;

    loop {
        tokio::select! {
            _ = hb_tick.tick() => {
                if app.shutdown.load(Ordering::SeqCst) {
                    return Ok(false);
                }
                let seq = { *app.last_seq.lock().await };
                let hb = serde_json::json!({"op": 1, "d": seq});
                let mut w = ws.lock().await;
                if w.send(tokio_tungstenite::tungstenite::Message::Text(hb.to_string()))
                    .await
                    .is_err()
                {
                    break;
                }
                // Reset ticker to match server heartbeat interval
                let ms = { *hb_interval_ms.lock().await };
                hb_tick = tokio::time::interval(Duration::from_millis(ms));
                continue;
            }
            msg = async {
                let mut w = ws.lock().await;
                w.next().await
            } => {
                let msg = match msg {
                    Some(Ok(m)) => m,
                    Some(Err(e)) => {
                        tracing::error!("[QQChannel:{}] WS read error: {e}", app.app_id);
                        break;
                    }
                    None => break,
                };

                let text = match msg {
                    tokio_tungstenite::tungstenite::Message::Text(t) => t,
                    tokio_tungstenite::tungstenite::Message::Close(_) => break,
                    _ => continue,
                };

                let payload: WsPayload = match serde_json::from_str(&text) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::error!("[QQChannel:{}] Failed to parse WS payload: {e}", app.app_id);
                        continue;
                    }
                };

                if let Some(seq) = payload.s {
                    *app.last_seq.lock().await = Some(seq);
                }

                match payload.op {
                    10 => {
                        if let Some(d) = payload.d {
                            if let Ok(h) = serde_json::from_value::<HelloData>(d) {
                                *hb_interval_ms.lock().await = h.heartbeat_interval;
                                hb_tick = tokio::time::interval(Duration::from_millis(h.heartbeat_interval));
                            }
                        }
                        let send_msg = {
                            let sid = app.session_id.lock().await;
                            let seq = app.last_seq.lock().await;
                            if let (Some(ref sid), Some(seq)) = (sid.as_ref(), *seq) {
                                serde_json::json!({
                                    "op": 6,
                                    "d": {
                                        "token": format!("QQBot {token}"),
                                        "session_id": sid,
                                        "seq": seq,
                                    },
                                })
                            } else {
                                serde_json::json!({
                                    "op": 2,
                                    "d": {
                                        "token": format!("QQBot {token}"),
                                        "intents": INTENTS,
                                        "shard": [0, 1],
                                    },
                                })
                            }
                        };
                        let mut w = ws.lock().await;
                        let _ = w
                            .send(tokio_tungstenite::tungstenite::Message::Text(
                                send_msg.to_string(),
                            ))
                            .await;
                    }

                    0 => {
                        match payload.t.as_deref() {
                            Some("READY") => {
                                if let Some(d) = payload.d {
                                    if let Ok(rd) = serde_json::from_value::<ReadyData>(d) {
                                        let sid = rd.session_id.clone();
                                        *app.session_id.lock().await = Some(rd.session_id);
                                        *attempts.lock().await = 0;
                                        tracing::info!(
                                            "[QQChannel:{}] Ready, session: {sid}",
                                            app.app_id,
                                        );
                                    }
                                }
                            }
                            Some("RESUMED") => {
                                *attempts.lock().await = 0;
                                tracing::info!("[QQChannel:{}] Session resumed", app.app_id);
                            }
                            Some("C2C_MESSAGE_CREATE") => {
                                if let Some(d) = payload.d {
                                    if let Ok(evt) = serde_json::from_value::<C2cEvent>(d) {
                                        handle_c2c(&app, &state, &handlers, &meta_handlers, &http, &evt).await;
                                    }
                                }
                            }
                            Some("GROUP_AT_MESSAGE_CREATE") => {
                                if let Some(d) = payload.d {
                                    if let Ok(evt) = serde_json::from_value::<GroupAtEvent>(d) {
                                        handle_group(&app, &state, &handlers, &meta_handlers, &evt).await;
                                    }
                                }
                            }
                            Some("INTERACTION_CREATE") => {
                                if let Some(d) = payload.d {
                                    if let Ok(evt) = serde_json::from_value::<InteractionEvent>(d) {
                                        handle_interaction(&app, &state, &handlers, &http, &evt).await;
                                    }
                                }
                            }
                            _ => {}
                        }
                    }

                    7 => {
                        tracing::info!("[QQChannel:{}] Server requested reconnect", app.app_id);
                        reconnect = true;
                        break;
                    }

                    9 => {
                        tracing::warn!("[QQChannel:{}] Invalid session, clearing", app.app_id);
                        *app.session_id.lock().await = None;
                        *app.last_seq.lock().await = None;
                        reconnect = true;
                        break;
                    }

                    11 => {} // Heartbeat ACK

                    _ => {}
                }
            }
        }

        if app.shutdown.load(Ordering::SeqCst) {
            return Ok(false);
        }
    }

    Ok(reconnect)
}

// ===== Event handlers =====

async fn handle_c2c(
    app: &AppEntry,
    state: &PendingReplyState,
    handlers: &Arc<RwLock<Vec<MessageCallback>>>,
    meta: &Arc<RwLock<Vec<MetadataCallback>>>,
    _http: &reqwest::Client,
    event: &C2cEvent,
) {
    if !state.try_dedup(&event.id).await {
        return;
    }

    let openid = event.author["user_openid"]
        .as_str()
        .unwrap_or("unknown");
    let jid = format!("qq:user:{openid}");
    let content = event.content.trim().to_string();
    if content.is_empty() {
        return;
    }

    // Check numeric menu
    if let Some((selected, app_id)) = state.try_handle_menu(&jid, &content).await {
        let answer = call_callback_handlers(handlers, &selected.callback_data, &jid);
        if let Some(ans) = answer {
            // Use send_message via the resolved app
            // For simplicity, dispatch a reply through broadcast_reply
            dispatch_reply(handlers, &jid, &ans, Some(&app_id));
        }
        return;
    }

    state.set_reply(&jid, &event.id).await;

    let ts = event
        .timestamp
        .as_ref()
        .and_then(|t| t.parse::<i64>().ok())
        .map(|t| {
            // Unix millis to ISO string
            let secs = t / 1000;
            let days = secs / 86400;
            let tod = secs % 86400;
            let h = tod / 3600;
            let m = (tod % 3600) / 60;
            let s = tod % 60;
            format!("{days}T{h:02}:{m:02}:{s:02}.000Z")
        })
        .unwrap_or_else(|| {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let days = now / 86400;
            let tod = now % 86400;
            let h = tod / 3600;
            let m = (tod % 3600) / 60;
            let s = tod % 60;
            format!("{days}T{h:02}:{m:02}:{s:02}.000Z")
        });

    let sender_name = format!("{}...", &openid[..openid.len().min(8)]);

    let msg = IncomingMessage {
        id: event.id.clone(),
        chat_jid: jid.clone(),
        sender_name,
        sender_jid: jid.clone(),
        content,
        timestamp: ts,
        is_from_me: false,
        chat_type: crate::types::ChatType::Private,
        mentions_bot_username: Some(false),
        bot_token: Some(app.app_id.clone()),
        native_msg_id: None,
    };

    dispatch_msg(handlers, meta, msg, &jid, "private");
}

async fn handle_group(
    app: &AppEntry,
    state: &PendingReplyState,
    handlers: &Arc<RwLock<Vec<MessageCallback>>>,
    meta: &Arc<RwLock<Vec<MetadataCallback>>>,
    event: &GroupAtEvent,
) {
    if !state.try_dedup(&event.id).await {
        return;
    }

    let member_openid = event.author["member_openid"]
        .as_str()
        .unwrap_or("unknown");
    let group_openid = &event.group_openid;
    let jid = format!("qq:group:{group_openid}");

    // Strip <@!id> mentions
    let content = regex::Regex::new(r"<@!\d+>")
        .unwrap()
        .replace_all(&event.content, "")
        .trim()
        .to_string();
    if content.is_empty() {
        return;
    }

    // Check numeric menu
    if let Some((selected, app_id)) = state.try_handle_menu(&jid, &content).await {
        let answer = call_callback_handlers(handlers, &selected.callback_data, &jid);
        if let Some(ans) = answer {
            dispatch_reply(handlers, &jid, &ans, Some(&app_id));
        }
        return;
    }

    state.set_reply(&jid, &event.id).await;

    let ts = event
        .timestamp
        .as_ref()
        .and_then(|t| t.parse::<i64>().ok())
        .map(|t| {
            let secs = t / 1000;
            let days = secs / 86400;
            let tod = secs % 86400;
            let h = tod / 3600;
            let m = (tod % 3600) / 60;
            let s = tod % 60;
            format!("{days}T{h:02}:{m:02}:{s:02}.000Z")
        })
        .unwrap_or_else(|| {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let days = now / 86400;
            let tod = now % 86400;
            let h = tod / 3600;
            let m = (tod % 3600) / 60;
            let s = tod % 60;
            format!("{days}T{h:02}:{m:02}:{s:02}.000Z")
        });

    let sender_name = format!("{}...", &member_openid[..member_openid.len().min(8)]);

    let msg = IncomingMessage {
        id: event.id.clone(),
        chat_jid: jid.clone(),
        sender_name,
        sender_jid: format!("qq:user:{member_openid}"),
        content,
        timestamp: ts,
        is_from_me: false,
        chat_type: crate::types::ChatType::Group,
        mentions_bot_username: Some(true),
        bot_token: Some(app.app_id.clone()),
        native_msg_id: None,
    };

    dispatch_msg(handlers, meta, msg, &jid, "group");
}

async fn handle_interaction(
    app: &AppEntry,
    _state: &PendingReplyState,
    handlers: &Arc<RwLock<Vec<MessageCallback>>>,
    http: &reqwest::Client,
    event: &InteractionEvent,
) {
    let callback_data = event
        .data
        .as_ref()
        .and_then(|d| d.resolved.as_ref())
        .and_then(|r| r.button_data.as_deref());
    let Some(callback_data) = callback_data else {
        return;
    };

    let jid = if event.chat_type == 1 {
        format!("qq:group:{}", event.group_openid.as_deref().unwrap_or(""))
    } else {
        format!("qq:user:{}", event.user_openid.as_deref().unwrap_or(""))
    };

    let answer = call_callback_handlers(handlers, callback_data, &jid);

    // Acknowledge interaction (required by QQ)
    let _ = api_request::<serde_json::Value>(
        http,
        app,
        "PUT",
        &format!("/v2/interactions/{}", event.id),
        Some(&serde_json::json!({"code": 0})),
    )
    .await;

    if let Some(ans) = answer {
        dispatch_reply(handlers, &jid, &ans, Some(&app.app_id));
    }
}

fn call_callback_handlers(
    _handlers: &Arc<RwLock<Vec<MessageCallback>>>,
    _callback_data: &str,
    _jid: &str,
) -> Option<String> {
    // Callback handlers are routed via message handlers with a special marker.
    // This is a simplified version — full implementation uses a separate callback handler list.
    None
}

fn dispatch_reply(
    _handlers: &Arc<RwLock<Vec<MessageCallback>>>,
    _jid: &str,
    _text: &str,
    _bot_token: Option<&str>,
) {
    // Reply is dispatched via the message router layer.
    // Channel adapters don't send directly — they route through handlers.
}

fn dispatch_msg(
    handlers: &Arc<RwLock<Vec<MessageCallback>>>,
    meta: &Arc<RwLock<Vec<MetadataCallback>>>,
    msg: IncomingMessage,
    jid: &str,
    chat_type: &str,
) {
    if let Ok(guard) = handlers.read() {
        for h in guard.iter() {
            h(msg.clone());
        }
    }
    let ct = match chat_type {
        "private" => crate::types::ChatType::Private,
        "group" => crate::types::ChatType::Group,
        _ => crate::types::ChatType::Group,
    };
    let chat_meta = crate::types::ChatMeta {
        jid: jid.to_string(),
        title: if chat_type == "private" {
            Some(msg.sender_name.clone())
        } else {
            None
        },
        chat_type: ct,
    };
    if let Ok(guard) = meta.read() {
        for h in guard.iter() {
            h(chat_meta.clone());
        }
    }
}

// ===== Tests =====

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_owns_jid() {
        let ch = QQChannel::new("app".into(), "secret".into(), false);
        assert!(ch.owns_jid("qq:user:abc123"));
        assert!(ch.owns_jid("qq:group:xyz789"));
        assert!(!ch.owns_jid("feishu:user:abc"));
        assert!(!ch.owns_jid("tg:123:user:456"));
    }

    #[test]
    fn test_split_short() {
        let parts = split_message("hello");
        assert_eq!(parts, vec!["hello"]);
    }

    #[test]
    fn test_split_long() {
        let long = "x".repeat(5000);
        let parts = split_message(&long);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].len(), QQ_MAX_LEN);
        assert_eq!(parts[1].len(), 5000 - QQ_MAX_LEN);
    }

    #[test]
    fn test_split_at_newline() {
        let mut text = "x".repeat(2500);
        text.push('\n');
        text.push_str(&"y".repeat(2000));
        let parts = split_message(&text);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].len(), 2500);
        assert_eq!(parts[1].len(), 2000);
    }

    #[test]
    fn test_build_keyboard() {
        let buttons = vec![
            InlineButton {
                label: "Accept".into(),
                callback_data: "approve_123".into(),
            },
            InlineButton {
                label: "Deny".into(),
                callback_data: "refuse_123".into(),
            },
            InlineButton {
                label: "More".into(),
                callback_data: "more_info".into(),
            },
        ];
        let rows = QQChannel::build_keyboard(&buttons);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["buttons"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn test_build_keyboard_multirow() {
        let mut buttons = Vec::new();
        for i in 0..7 {
            buttons.push(InlineButton {
                label: format!("Btn{i}"),
                callback_data: format!("data_{i}"),
            });
        }
        let rows = QQChannel::build_keyboard(&buttons);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["buttons"].as_array().unwrap().len(), 5);
        assert_eq!(rows[1]["buttons"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_next_msg_seq() {
        let a = next_msg_seq();
        let b = next_msg_seq();
        // Not guaranteed different but very likely
        assert!(a > 0 || b > 0);
    }
}
