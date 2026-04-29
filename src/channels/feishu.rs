//! Feishu/Lark channel adapter. Mirrors `src-old/channels/feishu.ts` + `feishu-client.ts`.
//!
//! JID format: `feishu:user:{open_id}` (private) or `feishu:group:{chat_id}` (group).
//!
//! Multi-app support: each appId/appSecret pair maintains its own REST client & WS listener.
//! WebSocket event receiving is stubbed (requires Feishu WS framing protocol).
//!
//! Connection mode: REST for sending, WS stub for receiving (TODO: full WS protocol).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::channels::feishu_ws;
use crate::channels::{Channel, MessageCallback, MetadataCallback};
use crate::types::{ChatType, IncomingMessage, InlineButton};

// ===== Constants =====

const FEISHU_MAX_LEN: usize = 4000;
const FEISHU_CARD_MAX_LEN: usize = 20_000;
const APP_INIT_TIMEOUT_SECS: u64 = 15;
const DEDUP_TTL_MS: i64 = 30 * 60 * 1000;
const DEDUP_MAX_SIZE: usize = 1000;
const DEDUP_CLEANUP_INTERVAL_MS: i64 = 5 * 60 * 1000;
const SENDER_NAME_TTL_SECS: u64 = 10 * 60;
const TOKEN_REFRESH_MARGIN_SECS: i64 = 60;

// ===== Types =====

#[derive(Debug, Clone, PartialEq)]
pub enum FeishuDomain {
    Feishu,
    Lark,
    Custom(String),
}

impl FeishuDomain {
    fn base_url(&self) -> &str {
        match self {
            FeishuDomain::Feishu => "https://open.feishu.cn",
            FeishuDomain::Lark => "https://open.larksuite.com",
            FeishuDomain::Custom(ref s) => s.as_str(),
        }
    }
}

#[derive(Debug, Clone)]
struct FeishuBotInfo {
    open_id: String,
    name: String,
}

// ===== Token management =====

#[derive(Debug, Clone)]
struct CachedToken {
    token: String,
    expires_at: i64, // unix timestamp
}

#[derive(Debug, Deserialize)]
struct TenantTokenResponse {
    code: i32,
    msg: Option<String>,
    tenant_access_token: Option<String>,
    expire: Option<i64>,
}

#[derive(Debug, Serialize)]
struct TenantTokenRequest<'a> {
    app_id: &'a str,
    app_secret: &'a str,
}

async fn get_tenant_access_token(
    http: &reqwest::Client,
    base_url: &str,
    app_id: &str,
    app_secret: &str,
) -> Result<CachedToken> {
    let url = format!("{base_url}/open-apis/auth/v3/tenant_access_token/internal");
    let resp = http
        .post(&url)
        .json(&TenantTokenRequest { app_id, app_secret })
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .context("fetch tenant_access_token")?;

    let body: TenantTokenResponse = resp.json().await.context("parse token response")?;
    if body.code != 0 {
        anyhow::bail!(
            "tenant_access_token failed: code={}, msg={:?}",
            body.code,
            body.msg
        );
    }
    let token = body.tenant_access_token.context("missing token in response")?;
    let expire = body.expire.unwrap_or(7200);
    let expires_at = now_secs() + expire - TOKEN_REFRESH_MARGIN_SECS;
    Ok(CachedToken { token, expires_at })
}

// ===== Message dedup =====

struct DedupState {
    ids: HashMap<String, i64>, // key → added_at_ms
    last_cleanup_ms: i64,
}

impl DedupState {
    fn new() -> Self {
        Self {
            ids: HashMap::new(),
            last_cleanup_ms: now_ms(),
        }
    }

    /// Returns true if the message is new (not seen before).
    fn try_record(&mut self, message_id: &str, app_id: &str) -> bool {
        let now = now_ms();
        let key = format!("{app_id}:{message_id}");

        if now - self.last_cleanup_ms > DEDUP_CLEANUP_INTERVAL_MS {
            self.ids.retain(|_, ts| now - *ts <= DEDUP_TTL_MS);
            self.last_cleanup_ms = now;
        }

        if self.ids.contains_key(&key) {
            return false;
        }

        if self.ids.len() >= DEDUP_MAX_SIZE {
            if let Some(oldest_key) = self.ids.keys().next().cloned() {
                self.ids.remove(&oldest_key);
            }
        }

        self.ids.insert(key, now);
        true
    }
}

// ===== Content parsing =====

fn parse_text_content(content: &str, message_type: &str) -> String {
    let parsed: serde_json::Value = match serde_json::from_str(content) {
        Ok(v) => v,
        Err(_) => return content.to_string(),
    };

    if message_type == "text" {
        return parsed["text"].as_str().unwrap_or("").to_string();
    }

    if message_type == "post" {
        let blocks = parsed["zh_cn"]["content"]
            .as_array()
            .or_else(|| parsed["en_us"]["content"].as_array())
            .or_else(|| parsed["content"].as_array());

        let mut lines: Vec<String> = Vec::new();
        if let Some(blocks) = blocks {
            for paragraph in blocks {
                let arr = match paragraph.as_array() {
                    Some(a) => a,
                    None => continue,
                };
                let line: String = arr
                    .iter()
                    .map(|node| match node["tag"].as_str().unwrap_or("") {
                        "text" => node["text"].as_str().unwrap_or("").to_string(),
                        "a" => node["text"]
                            .as_str()
                            .or_else(|| node["href"].as_str())
                            .unwrap_or("")
                            .to_string(),
                        "at" => String::new(),
                        "img" => "[Image]".to_string(),
                        _ => node["text"].as_str().unwrap_or("").to_string(),
                    })
                    .collect::<Vec<_>>()
                    .join("");
                let trimmed = line.trim().to_string();
                if !trimmed.is_empty() {
                    lines.push(trimmed);
                }
            }
        }

        let title = parsed["zh_cn"]["title"]
            .as_str()
            .or_else(|| parsed["en_us"]["title"].as_str())
            .unwrap_or("");
        let body = lines.join("\n");
        return if title.is_empty() {
            body
        } else {
            format!("{title}\n{body}")
        };
    }

    parsed["text"].as_str().unwrap_or(content).to_string()
}

// ===== Mention handling =====

fn check_bot_mention(
    mentions: Option<&[serde_json::Value]>,
    bot_open_id: &str,
) -> bool {
    let Some(mentions) = mentions else { return false };
    if bot_open_id.is_empty() {
        return false;
    }
    mentions.iter().any(|m| {
        m.get("id")
            .and_then(|id| id.get("open_id"))
            .and_then(|v| v.as_str())
            .map_or(false, |oid| oid == bot_open_id)
    })
}

fn remove_bot_mention_placeholders(
    text: &str,
    mentions: Option<&[serde_json::Value]>,
    bot_open_id: &str,
) -> String {
    let Some(mentions) = mentions else { return text.to_string() };
    if bot_open_id.is_empty() {
        return text.to_string();
    }
    let mut result = text.to_string();
    for m in mentions {
        let is_bot = m
            .get("id")
            .and_then(|id| id.get("open_id"))
            .and_then(|v| v.as_str())
            .map_or(false, |oid| oid == bot_open_id);
        if is_bot {
            if let Some(key) = m.get("key").and_then(|v| v.as_str()) {
                result = result.replace(key, "").trim().to_string();
            }
        }
    }
    result
}

// ===== Message splitting =====

fn split_message(text: &str) -> Vec<String> {
    if text.len() <= FEISHU_MAX_LEN {
        return vec![text.to_string()];
    }
    let mut parts: Vec<String> = Vec::new();
    let mut remaining = text;
    while remaining.len() > FEISHU_MAX_LEN {
        let chunk = &remaining[..FEISHU_MAX_LEN];
        let split_at = chunk
            .rfind('\n')
            .filter(|&pos| pos > FEISHU_MAX_LEN / 2)
            .unwrap_or(FEISHU_MAX_LEN);
        parts.push(remaining[..split_at].to_string());
        remaining = remaining[split_at..].trim_start_matches('\n');
    }
    if !remaining.is_empty() {
        parts.push(remaining.to_string());
    }
    parts
}

// ===== JID utilities =====

fn jid_to_receive_id_type(jid: &str) -> &'static str {
    if jid.starts_with("feishu:user:") {
        "open_id"
    } else {
        "chat_id"
    }
}

fn jid_to_chat_id(jid: &str) -> Option<&str> {
    jid.strip_prefix("feishu:user:")
        .or_else(|| jid.strip_prefix("feishu:group:"))
}

fn make_jid(chat_type: &str, id: &str) -> String {
    match chat_type {
        "p2p" | "private" => format!("feishu:user:{id}"),
        _ => format!("feishu:group:{id}"),
    }
}

// ===== Bot info API =====

#[derive(Debug, Deserialize)]
struct BotInfoResponse {
    code: i32,
    msg: Option<String>,
    bot: Option<serde_json::Value>,
}

async fn fetch_bot_info(
    http: &reqwest::Client,
    base_url: &str,
    token: &str,
) -> Result<FeishuBotInfo> {
    let url = format!("{base_url}/open-apis/bot/v3/info");
    let resp = http
        .get(&url)
        .bearer_auth(token)
        .timeout(Duration::from_secs(APP_INIT_TIMEOUT_SECS))
        .send()
        .await
        .context("fetch bot info")?;

    let body: BotInfoResponse = resp.json().await.context("parse bot info")?;
    if body.code != 0 {
        anyhow::bail!("fetch bot info: code={}, msg={:?}", body.code, body.msg);
    }

    let bot = body.bot.context("bot not found in response")?;
    let open_id = bot["open_id"]
        .as_str()
        .context("bot missing open_id")?
        .to_string();
    let name = bot["bot_name"]
        .as_str()
        .unwrap_or("Feishu Bot")
        .to_string();

    Ok(FeishuBotInfo { open_id, name })
}

// ===== User info API =====

#[derive(Debug, Deserialize)]
struct UserInfoResponse {
    code: i32,
    data: Option<serde_json::Value>,
}

async fn fetch_user_name(
    http: &reqwest::Client,
    base_url: &str,
    token: &str,
    open_id: &str,
) -> Result<String> {
    let url = format!("{base_url}/open-apis/contact/v3/users/{open_id}");
    let resp = http
        .get(&url)
        .bearer_auth(token)
        .query(&[("user_id_type", "open_id")])
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .context("fetch user info")?;

    let body: UserInfoResponse = resp.json().await.context("parse user info")?;
    if body.code != 0 {
        return Ok(open_id.chars().take(8).collect::<String>() + "...");
    }

    Ok(body
        .data
        .as_ref()
        .and_then(|d| d["user"]["name"].as_str())
        .unwrap_or("Unknown")
        .to_string())
}

// ===== Feishu API v1 messages =====

#[derive(Debug, Serialize)]
struct SendMessageBody {
    receive_id: String,
    msg_type: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ApiResponse {
    code: i32,
    msg: Option<String>,
    data: Option<serde_json::Value>,
}

async fn call_api(
    http: &reqwest::Client,
    base_url: &str,
    token: &str,
    path: &str,
    body: &SendMessageBody,
    receive_id_type: &str,
) -> Result<()> {
    let url = format!("{base_url}{path}?receive_id_type={receive_id_type}");
    let resp = http
        .post(&url)
        .bearer_auth(token)
        .json(body)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .context("feishu api call")?;

    let r: ApiResponse = resp.json().await.context("parse api response")?;
    if r.code != 0 {
        anyhow::bail!("feishu api error: code={}, msg={:?}", r.code, r.msg);
    }
    Ok(())
}

/// Upload a file, return the file_key.
async fn upload_file(
    http: &reqwest::Client,
    base_url: &str,
    token: &str,
    file_path: &str,
    file_name: &str,
) -> Result<String> {
    let data = tokio::fs::read(file_path)
        .await
        .context("read upload file")?;
    let part = reqwest::multipart::Part::bytes(data)
        .file_name(file_name.to_string())
        .mime_str("application/octet-stream")?;

    let form = reqwest::multipart::Form::new()
        .text("file_type", "stream")
        .text("file_name", file_name.to_string())
        .part("file", part);

    let url = format!("{base_url}/open-apis/im/v1/files");
    let resp = http
        .post(&url)
        .bearer_auth(token)
        .multipart(form)
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .context("upload file to feishu")?;

    let r: ApiResponse = resp.json().await.context("parse upload response")?;
    if r.code != 0 {
        anyhow::bail!("feishu upload error: code={}, msg={:?}", r.code, r.msg);
    }
    r.data
        .as_ref()
        .and_then(|d| d["file_key"].as_str())
        .map(|s| s.to_string())
        .context("missing file_key in upload response")
}

// ===== App entry =====

#[derive(Debug, Clone)]
struct AppEntry {
    base_url: String,
    app_id: String,
    app_secret: String,
    bot_info: FeishuBotInfo,
}

// ===== WS event types & processing =====

#[derive(Debug, Deserialize)]
struct WsEventEnvelope {
    header: WsEventHeader,
    event: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct WsEventHeader {
    #[serde(default)]
    #[serde(alias = "event_type")]
    #[serde(alias = "eventType")]
    event_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WsMessageEvent {
    sender: Option<WsSender>,
    message: Option<WsMessage>,
}

#[derive(Debug, Deserialize)]
struct WsSender {
    sender_id: Option<WsSenderId>,
}

#[derive(Debug, Deserialize)]
struct WsSenderId {
    open_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WsMessage {
    message_id: String,
    chat_id: String,
    chat_type: String,
    message_type: String,
    content: String,
    #[serde(default)]
    mentions: Option<Vec<serde_json::Value>>,
}

/// Process a raw WS event payload (JSON bytes from data frame) into an IncomingMessage
/// and dispatch to registered handlers. Runs synchronously inside the WS callback.
fn process_ws_event(
    payload: &[u8],
    app_id: &str,
    bot_open_id: &str,
    handlers: &Arc<RwLock<Vec<MessageCallback>>>,
    dedup: &Arc<Mutex<DedupState>>,
    sender_cache: &Arc<Mutex<HashMap<String, (String, i64)>>>,
) {
    let envelope: WsEventEnvelope = match serde_json::from_slice(payload) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("[FeishuWS:{app_id}] Failed to parse event: {e}");
            return;
        }
    };

    let event_type = envelope.header.event_type.as_deref().unwrap_or("");
    if event_type != "im.message.receive_v1" {
        return;
    }

    let event = match envelope.event {
        Some(ref e) => e,
        None => return,
    };
    let msg_event: WsMessageEvent = match serde_json::from_value(event.clone()) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!("[FeishuWS:{app_id}] Failed to parse message event: {e}");
            return;
        }
    };

    let message = match msg_event.message {
        Some(ref m) => m,
        None => return,
    };
    let sender = match msg_event.sender {
        Some(ref s) => s,
        None => return,
    };

    // Dedup
    {
        let mut dedup_guard = match dedup.try_lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        if !dedup_guard.try_record(&message.message_id, app_id) {
            return;
        }
    }

    let chat_jid = make_jid(&message.chat_type, &message.chat_id);
    let sender_open_id = sender
        .sender_id
        .as_ref()
        .and_then(|sid| sid.open_id.as_deref())
        .unwrap_or("");
    let sender_jid = if sender_open_id.is_empty() {
        String::new()
    } else {
        format!("feishu:user:{sender_open_id}")
    };

    // Sender name (best-effort from cache, sync-only)
    let sender_name = sender_cache
        .try_lock()
        .ok()
        .and_then(|cache| cache.get(sender_open_id).map(|(n, _)| n.clone()))
        .unwrap_or_else(|| {
            if sender_open_id.len() >= 8 {
                format!("{}...", &sender_open_id[..8])
            } else {
                sender_open_id.to_string()
            }
        });

    let content = parse_text_content(&message.content, &message.message_type);
    let is_mentioned = check_bot_mention(message.mentions.as_deref(), bot_open_id);
    let clean_content =
        remove_bot_mention_placeholders(&content, message.mentions.as_deref(), bot_open_id);

    let chat_type = match message.chat_type.as_str() {
        "p2p" | "private" => ChatType::Private,
        _ => ChatType::Group,
    };

    let incoming = IncomingMessage {
        id: format!("feishu:{app_id}:{}", message.message_id),
        chat_jid,
        sender_name,
        sender_jid,
        content: clean_content,
        timestamp: chrono::Utc::now().to_rfc3339(),
        is_from_me: false,
        chat_type,
        mentions_bot_username: Some(is_mentioned),
        bot_token: Some(app_id.to_string()),
        native_msg_id: Some(message.message_id.clone()),
    };

    let handler_guard = match handlers.read() {
        Ok(g) => g,
        Err(_) => return,
    };
    for handler in handler_guard.iter() {
        handler(incoming.clone());
    }
}

// ===== FeishuChannel =====

pub struct FeishuChannel {
    default_app_id: String,
    default_app_secret: String,
    default_domain: FeishuDomain,
    http: reqwest::Client,
    apps: Mutex<HashMap<String, AppEntry>>,
    tokens: Mutex<HashMap<String, CachedToken>>,
    handlers: Arc<RwLock<Vec<MessageCallback>>>,
    meta_handlers: Arc<RwLock<Vec<MetadataCallback>>>,
    connected: AtomicBool,
    dedup: Arc<Mutex<DedupState>>,
    sender_name_cache: Arc<Mutex<HashMap<String, (String, i64)>>>,
    ws_connections: Mutex<HashMap<String, feishu_ws::WsConnection>>,
}

impl FeishuChannel {
    pub fn new(app_id: String, app_secret: String, domain: Option<String>) -> Self {
        let domain = match domain.as_deref() {
            Some("lark") => FeishuDomain::Lark,
            Some("feishu") | None => FeishuDomain::Feishu,
            Some(other) => FeishuDomain::Custom(other.to_string()),
        };
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("reqwest::Client::build");
        Self {
            default_app_id: app_id,
            default_app_secret: app_secret,
            default_domain: domain,
            http,
            apps: Mutex::new(HashMap::new()),
            tokens: Mutex::new(HashMap::new()),
            handlers: Arc::new(RwLock::new(Vec::new())),
            meta_handlers: Arc::new(RwLock::new(Vec::new())),
            connected: AtomicBool::new(false),
            dedup: Arc::new(Mutex::new(DedupState::new())),
            sender_name_cache: Arc::new(Mutex::new(HashMap::new())),
            ws_connections: Mutex::new(HashMap::new()),
        }
    }

    /// Register and start a Feishu app (idempotent).
    pub async fn add_app(
        &self,
        app_id: &str,
        app_secret: &str,
        domain: Option<FeishuDomain>,
    ) -> Result<bool> {
        if app_id.is_empty() || app_secret.is_empty() {
            return Ok(false);
        }
        {
            let apps = self.apps.lock().await;
            if apps.contains_key(app_id) {
                return Ok(true);
            }
        }

        let domain = domain.unwrap_or_else(|| self.default_domain.clone());
        let base_url = domain.base_url().to_string();

        tracing::info!("[FeishuChannel] add_app: starting {app_id} (domain={domain:?})");

        // Fetch bot info (with timeout)
        let bot_info = tokio::time::timeout(
            Duration::from_secs(APP_INIT_TIMEOUT_SECS),
            async {
                let token = get_or_refresh_token(
                    &self.http,
                    &base_url,
                    app_id,
                    app_secret,
                    &self.tokens,
                )
                .await?;
                fetch_bot_info(&self.http, &base_url, &token).await
            },
        )
        .await
        .map_err(|_| anyhow::anyhow!("add_app timed out after {APP_INIT_TIMEOUT_SECS}s"))??;

        tracing::info!(
            "[FeishuChannel] Bot info OK for {app_id}: name=\"{}\", open_id={}",
            bot_info.name,
            bot_info.open_id
        );

        let bot_open_id = bot_info.open_id.clone();

        {
            let mut apps = self.apps.lock().await;
            apps.insert(
                app_id.to_string(),
                AppEntry {
                    base_url: base_url.clone(),
                    app_id: app_id.to_string(),
                    app_secret: app_secret.to_string(),
                    bot_info,
                },
            );
        }
        self.connected.store(true, Ordering::SeqCst);

        // Start WS event listener
        let app_id_c = app_id.to_string();
        let handlers = Arc::clone(&self.handlers);
        let dedup = Arc::clone(&self.dedup);
        let sender_cache = Arc::clone(&self.sender_name_cache);

        let on_event = Arc::new(move |payload: Vec<u8>| {
            process_ws_event(
                &payload,
                &app_id_c,
                &bot_open_id,
                &handlers,
                &dedup,
                &sender_cache,
            );
        });

        match feishu_ws::start_event_listener(
            &base_url,
            app_id,
            app_secret,
            self.http.clone(),
            on_event,
        )
        .await
        {
            Ok(conn) => {
                self.ws_connections
                    .lock()
                    .await
                    .insert(app_id.to_string(), conn);
                tracing::info!("[FeishuChannel] App {app_id} connected (REST + WS)");
            }
            Err(e) => {
                tracing::warn!(
                    "[FeishuChannel] App {app_id} connected (REST only; WS failed: {e})"
                );
            }
        }

        Ok(true)
    }

    async fn resolve_app(&self, bot_token: Option<&str>) -> Option<AppEntry> {
        let token = bot_token.unwrap_or(&self.default_app_id);
        let apps = self.apps.lock().await;
        apps.get(token).cloned()
    }

    async fn get_token(&self, app_id: &str, app_secret: &str, base_url: &str) -> Result<String> {
        get_or_refresh_token(&self.http, base_url, app_id, app_secret, &self.tokens).await
    }

    // ===== Sender name cache =====

    async fn resolve_sender_name(
        &self,
        entry: &AppEntry,
        open_id: &str,
    ) -> String {
        if open_id.is_empty() {
            return "Unknown".to_string();
        }

        let now = now_secs();
        {
            let cache = self.sender_name_cache.lock().await;
            if let Some((name, expires)) = cache.get(open_id) {
                if now < *expires {
                    return name.clone();
                }
            }
        }

        // Fetch from API
        let token = match self.get_token(&entry.app_id, &entry.app_secret, &entry.base_url).await {
            Ok(t) => t,
            Err(_) => return format!("{}...", &open_id[..open_id.len().min(8)]),
        };

        let name = match fetch_user_name(&self.http, &entry.base_url, &token, open_id).await {
            Ok(n) => n,
            Err(_) => format!("{}...", &open_id[..open_id.len().min(8)]),
        };

        let expires = now + SENDER_NAME_TTL_SECS as i64;
        {
            let mut cache = self.sender_name_cache.lock().await;
            cache.insert(open_id.to_string(), (name.clone(), expires));
        }
        name
    }
}

#[async_trait]
impl Channel for FeishuChannel {
    fn id(&self) -> &'static str {
        "feishu"
    }

    async fn connect(&self) -> Result<()> {
        if self.connected.load(Ordering::SeqCst) {
            return Ok(());
        }
        if self.default_app_id.is_empty() || self.default_app_secret.is_empty() {
            tracing::warn!("[FeishuChannel] No app credentials configured, disabled");
            return Ok(());
        }
        self.add_app(
            &self.default_app_id,
            &self.default_app_secret,
            Some(self.default_domain.clone()),
        )
        .await?;
        Ok(())
    }

    async fn disconnect(&self) -> Result<()> {
        // Shut down WS listeners
        let ws_conns: Vec<feishu_ws::WsConnection> = {
            let mut guard = self.ws_connections.lock().await;
            std::mem::take(&mut *guard).into_values().collect()
        };
        for conn in ws_conns {
            conn.shutdown();
        }
        self.apps.lock().await.clear();
        self.tokens.lock().await.clear();
        self.sender_name_cache.lock().await.clear();
        self.connected.store(false, Ordering::SeqCst);
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    async fn send_message(
        &self,
        chat_jid: &str,
        text: &str,
        bot_token: Option<&str>,
    ) -> Result<()> {
        let entry = self
            .resolve_app(bot_token)
            .await
            .ok_or_else(|| anyhow::anyhow!("Feishu app not found for JID: {chat_jid}"))?;
        let chat_id = jid_to_chat_id(chat_jid)
            .ok_or_else(|| anyhow::anyhow!("Invalid Feishu JID: {chat_jid}"))?;
        let receive_id_type = jid_to_receive_id_type(chat_jid);
        let token = self
            .get_token(&entry.app_id, &entry.app_secret, &entry.base_url)
            .await?;

        for part in split_message(text) {
            let content = serde_json::json!({"text": part}).to_string();
            call_api(
                &self.http,
                &entry.base_url,
                &token,
                "/open-apis/im/v1/messages",
                &SendMessageBody {
                    receive_id: chat_id.to_string(),
                    msg_type: "text".to_string(),
                    content,
                },
                receive_id_type,
            )
            .await?;
        }
        Ok(())
    }

    async fn send_file(
        &self,
        chat_jid: &str,
        file_path: &str,
        caption: Option<&str>,
        bot_token: Option<&str>,
    ) -> Result<()> {
        let entry = self
            .resolve_app(bot_token)
            .await
            .ok_or_else(|| anyhow::anyhow!("Feishu app not found for JID: {chat_jid}"))?;
        let chat_id = jid_to_chat_id(chat_jid)
            .ok_or_else(|| anyhow::anyhow!("Invalid Feishu JID: {chat_jid}"))?;
        let receive_id_type = jid_to_receive_id_type(chat_jid);
        let token = self
            .get_token(&entry.app_id, &entry.app_secret, &entry.base_url)
            .await?;

        let file_name = std::path::Path::new(file_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file");

        let file_key = upload_file(&self.http, &entry.base_url, &token, file_path, file_name)
            .await?;

        let content = serde_json::json!({"file_key": file_key}).to_string();
        call_api(
            &self.http,
            &entry.base_url,
            &token,
            "/open-apis/im/v1/messages",
            &SendMessageBody {
                receive_id: chat_id.to_string(),
                msg_type: "file".to_string(),
                content,
            },
            receive_id_type,
        )
        .await?;

        if let Some(cap) = caption {
            self.send_message(chat_jid, cap, bot_token).await?;
        }
        Ok(())
    }

    fn owns_jid(&self, chat_jid: &str) -> bool {
        chat_jid.starts_with("feishu:")
    }

    fn on_message(&self, handler: MessageCallback) {
        if let Ok(mut guard) = self.handlers.write() {
            guard.push(handler);
        }
    }

    fn on_metadata(&self, handler: MetadataCallback) {
        if let Ok(mut guard) = self.meta_handlers.write() {
            guard.push(handler);
        }
    }

    fn get_bot_username(&self, _bot_token: Option<&str>) -> Option<String> {
        // Can't block on async; caller should use after connect
        None
    }

    async fn send_with_buttons(
        &self,
        chat_jid: &str,
        text: &str,
        buttons: &[InlineButton],
        bot_token: Option<&str>,
    ) -> Result<()> {
        let entry = self
            .resolve_app(bot_token)
            .await
            .ok_or_else(|| anyhow::anyhow!("Feishu app not found for JID: {chat_jid}"))?;
        let chat_id = jid_to_chat_id(chat_jid)
            .ok_or_else(|| anyhow::anyhow!("Invalid Feishu JID: {chat_jid}"))?;
        let receive_id_type = jid_to_receive_id_type(chat_jid);
        let token = self
            .get_token(&entry.app_id, &entry.app_secret, &entry.base_url)
            .await?;

        let truncated = if text.len() > FEISHU_CARD_MAX_LEN {
            format!(
                "{}…(content truncated)",
                &text[..FEISHU_CARD_MAX_LEN]
            )
        } else {
            text.to_string()
        };

        let actions: Vec<serde_json::Value> = buttons
            .iter()
            .map(|btn| {
                let btn_type = if btn.callback_data.contains("refuse")
                    || btn.callback_data.contains("deny")
                {
                    "danger"
                } else {
                    "primary"
                };
                serde_json::json!({
                    "tag": "button",
                    "text": { "tag": "plain_text", "content": btn.label },
                    "type": btn_type,
                    "value": { "action": btn.callback_data },
                })
            })
            .collect();

        let card = serde_json::json!({
            "config": { "wide_screen_mode": true },
            "elements": [
                { "tag": "markdown", "content": truncated },
                { "tag": "action", "actions": actions },
            ],
        });

        call_api(
            &self.http,
            &entry.base_url,
            &token,
            "/open-apis/im/v1/messages",
            &SendMessageBody {
                receive_id: chat_id.to_string(),
                msg_type: "interactive".to_string(),
                content: card.to_string(),
            },
            receive_id_type,
        )
        .await?;
        Ok(())
    }
}

// ===== Token helpers =====

async fn get_or_refresh_token(
    http: &reqwest::Client,
    base_url: &str,
    app_id: &str,
    app_secret: &str,
    tokens: &Mutex<HashMap<String, CachedToken>>,
) -> Result<String> {
    {
        let tokens = tokens.lock().await;
        if let Some(cached) = tokens.get(app_id) {
            if now_secs() < cached.expires_at {
                return Ok(cached.token.clone());
            }
        }
    }

    let cached = get_tenant_access_token(http, base_url, app_id, app_secret).await?;
    let token = cached.token.clone();
    {
        let mut tokens = tokens.lock().await;
        tokens.insert(app_id.to_string(), cached);
    }
    Ok(token)
}

// ===== Time helpers =====

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

// ===== Tests =====

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jid_roundtrip() {
        assert_eq!(
            make_jid("p2p", "ou_abc123"),
            "feishu:user:ou_abc123"
        );
        assert_eq!(
            make_jid("group", "oc_xyz789"),
            "feishu:group:oc_xyz789"
        );
        assert_eq!(jid_to_chat_id("feishu:user:ou_abc"), Some("ou_abc"));
        assert_eq!(jid_to_chat_id("feishu:group:oc_xyz"), Some("oc_xyz"));
        assert_eq!(jid_to_chat_id("tg:user:123"), None);
    }

    #[test]
    fn test_receive_id_type() {
        assert_eq!(jid_to_receive_id_type("feishu:user:ou_abc"), "open_id");
        assert_eq!(
            jid_to_receive_id_type("feishu:group:oc_xyz"),
            "chat_id"
        );
    }

    #[test]
    fn test_owns_jid() {
        let ch = FeishuChannel::new("app".into(), "secret".into(), None);
        assert!(ch.owns_jid("feishu:user:ou_abc"));
        assert!(ch.owns_jid("feishu:group:oc_xyz"));
        assert!(!ch.owns_jid("tg:123:user:456"));
        assert!(!ch.owns_jid("wx:user:xyz"));
    }

    #[test]
    fn test_domain_base_url() {
        assert_eq!(
            FeishuDomain::Feishu.base_url(),
            "https://open.feishu.cn"
        );
        assert_eq!(
            FeishuDomain::Lark.base_url(),
            "https://open.larksuite.com"
        );
        assert_eq!(
            FeishuDomain::Custom("https://open.example.com".into()).base_url(),
            "https://open.example.com"
        );
    }

    #[test]
    fn test_parse_text_content_text_type() {
        let content = r#"{"text":"Hello world"}"#;
        let result = parse_text_content(content, "text");
        assert_eq!(result, "Hello world");
    }

    #[test]
    fn test_parse_text_content_post_type() {
        let content = r#"{
            "zh_cn": {
                "title": "Rich Title",
                "content": [
                    [{"tag": "text", "text": "Hello"}, {"tag": "text", "text": " World"}],
                    [{"tag": "a", "text": "Link", "href": "https://example.com"}],
                    [{"tag": "img", "image_key": "xxx"}],
                    [{"tag": "at", "user_id": "ou_xxx"}]
                ]
            }
        }"#;
        let result = parse_text_content(content, "post");
        assert!(result.contains("Rich Title"));
        assert!(result.contains("Hello World"));
        assert!(result.contains("Link"));
        assert!(result.contains("[Image]"));
        assert!(!result.contains("@"));
    }

    #[test]
    fn test_parse_invalid_json() {
        let result = parse_text_content("plain text", "text");
        assert_eq!(result, "plain text");
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
        assert_eq!(parts[0].len(), FEISHU_MAX_LEN);
        assert_eq!(parts[1].len(), 5000 - FEISHU_MAX_LEN);
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
    fn test_dedup() {
        let mut state = DedupState::new();
        assert!(state.try_record("msg1", "app1"));
        assert!(!state.try_record("msg1", "app1"));
        assert!(state.try_record("msg1", "app2")); // different app
        assert!(state.try_record("msg2", "app1"));
    }

    #[test]
    fn test_check_bot_mention() {
        let mentions = vec![
            serde_json::json!({"key": "@bot", "id": {"open_id": "bot123"}, "name": "Bot"}),
            serde_json::json!({"key": "@user", "id": {"open_id": "user456"}, "name": "User"}),
        ];
        assert!(check_bot_mention(Some(&mentions), "bot123"));
        assert!(!check_bot_mention(Some(&mentions), "other789"));
        assert!(!check_bot_mention(None, "bot123"));
        assert!(!check_bot_mention(Some(&mentions), ""));
    }

    #[test]
    fn test_remove_bot_mention() {
        let mentions = vec![
            serde_json::json!({"key": "@bot", "id": {"open_id": "bot123"}, "name": "Bot"}),
        ];
        let text = "@bot hello world";
        let result = remove_bot_mention_placeholders(text, Some(&mentions), "bot123");
        assert_eq!(result, "hello world");
        assert!(!result.contains("@bot"));
    }

    #[test]
    fn test_remove_bot_mention_no_match() {
        let mentions = vec![
            serde_json::json!({"key": "@user", "id": {"open_id": "user456"}, "name": "User"}),
        ];
        let text = "@bot hello world";
        let result = remove_bot_mention_placeholders(text, Some(&mentions), "bot123");
        assert_eq!(result, "@bot hello world");
    }

    #[test]
    fn test_feishu_domain_constructor() {
        let ch1 = FeishuChannel::new("a".into(), "s".into(), None);
        assert_eq!(ch1.id(), "feishu");

        let ch2 = FeishuChannel::new("a".into(), "s".into(), Some("lark".into()));
        assert_eq!(ch2.id(), "feishu");
    }
}
