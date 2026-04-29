//! WeChat iLink Bot channel adapter. Mirrors `src-old/channels/wechat.ts`.
//!
//! Protocol: Tencent WeChat official iLink Bot HTTP API.
//! Message transport: HTTP long polling (getUpdates, server hold up to 35s).
//! Authentication: Bearer ilink_bot_token.
//!
//! JID format: `wx:user:{ilink_user_id}` (1:1 chats only; group_id reserved but empty).

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::channels::{Channel, MessageCallback, MetadataCallback};
use crate::types::{ChatType, IncomingMessage, InlineButton};

// ===== Constants =====

const DEFAULT_BASE_URL: &str = "https://ilinkai.weixin.qq.com";
const DEFAULT_LONG_POLL_TIMEOUT_MS: u64 = 35_000;
const DEFAULT_API_TIMEOUT_MS: u64 = 15_000;
const QR_LONG_POLL_TIMEOUT_MS: u64 = 35_000;
const MAX_QR_REFRESH: u32 = 3;
const MAX_CONSECUTIVE_FAILURES: u32 = 3;
const BACKOFF_DELAY_SECS: u64 = 30;
const RETRY_DELAY_SECS: u64 = 2;
const SESSION_EXPIRED_ERRCODE: i32 = -14;
const MENU_TTL_SECS: u64 = 5 * 60;
const WECHAT_MAX_LEN: usize = 2000;

const MSG_TYPE_USER: u32 = 1;
const MSG_TYPE_BOT: u32 = 2;
const ITEM_TYPE_TEXT: u32 = 1;
const ITEM_TYPE_VOICE: u32 = 3;
const ITEM_TYPE_IMAGE: u32 = 2;
const ITEM_TYPE_FILE: u32 = 4;
const ITEM_TYPE_VIDEO: u32 = 5;
const MSG_STATE_FINISH: u32 = 2;

// ===== JSON types =====

#[derive(Debug, Serialize, Deserialize)]
struct WeixinAccountData {
    token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    base_url: Option<String>,
    #[serde(rename = "userId", skip_serializing_if = "Option::is_none")]
    user_id: Option<String>,
    #[serde(rename = "savedAt")]
    saved_at: String,
}

#[derive(Debug, Serialize)]
struct SendMessageRequest {
    msg: SendMessageMsg,
}

#[derive(Debug, Serialize)]
struct SendMessageMsg {
    from_user_id: String,
    to_user_id: String,
    client_id: String,
    message_type: u32,
    message_state: u32,
    item_list: Vec<MessageItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    context_token: Option<String>,
}

#[derive(Debug, Serialize)]
struct MessageItem {
    #[serde(rename = "type")]
    item_type: u32,
    text_item: Option<TextItem>,
}

#[derive(Debug, Serialize)]
struct TextItem {
    text: String,
}

#[derive(Debug, Deserialize)]
struct GetUpdatesResponse {
    ret: Option<i32>,
    errcode: Option<i32>,
    errmsg: Option<String>,
    msgs: Option<Vec<WeixinMessage>>,
    get_updates_buf: Option<String>,
    longpolling_timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct WeixinMessage {
    #[serde(rename = "message_id")]
    message_id: Option<u64>,
    from_user_id: Option<String>,
    to_user_id: Option<String>,
    create_time_ms: Option<i64>,
    message_type: Option<u32>,
    item_list: Option<Vec<WeixinMessageItem>>,
    context_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WeixinMessageItem {
    #[serde(rename = "type")]
    item_type: Option<u32>,
    text_item: Option<WeixinTextItem>,
    voice_item: Option<WeixinTextItem>,
}

#[derive(Debug, Deserialize)]
struct WeixinTextItem {
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct QrCodeResponse {
    qrcode: String,
    qrcode_img_content: String,
}

#[derive(Debug, Deserialize)]
struct QrStatusResponse {
    status: String,
    bot_token: Option<String>,
    ilink_bot_id: Option<String>,
    baseurl: Option<String>,
    ilink_user_id: Option<String>,
}

// ===== Helpers =====

fn random_hex(n: usize) -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    (0..n).map(|_| format!("{:02x}", rng.gen::<u8>())).collect()
}

fn random_wechat_uin() -> String {
    use rand::Rng;
    let u: u32 = rand::thread_rng().gen();
    u.to_string()
}

fn wechat_state_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".senclaw")
        .join("wechat")
        .join("accounts")
}

fn account_path(account_id: &str) -> PathBuf {
    wechat_state_dir().join(format!("{account_id}.json"))
}

fn sync_buf_path(account_id: &str) -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".senclaw")
        .join("wechat")
        .join(format!("sync-buf-{account_id}.bin"))
}

fn context_tokens_path(account_id: &str) -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".senclaw")
        .join("wechat")
        .join(format!("context-tokens-{account_id}.json"))
}

fn load_account(account_id: &str) -> Option<WeixinAccountData> {
    let p = account_path(account_id);
    if !p.exists() {
        return None;
    }
    let raw = std::fs::read_to_string(&p).ok()?;
    serde_json::from_str(&raw).ok()
}

fn save_account(account_id: &str, data: &WeixinAccountData) {
    let p = account_path(account_id);
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let raw = serde_json::to_string_pretty(data).unwrap_or_default();
    let _ = std::fs::write(&p, raw);
}

fn load_sync_buf(account_id: &str) -> String {
    let p = sync_buf_path(account_id);
    std::fs::read_to_string(&p).unwrap_or_default().trim().to_string()
}

fn save_sync_buf(account_id: &str, buf: &str) {
    let p = sync_buf_path(account_id);
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&p, buf);
}

fn load_context_tokens(account_id: &str) -> HashMap<String, String> {
    let p = context_tokens_path(account_id);
    if !p.exists() {
        return HashMap::new();
    }
    let raw = std::fs::read_to_string(&p).unwrap_or_default();
    serde_json::from_str(&raw).unwrap_or_default()
}

fn save_context_tokens(account_id: &str, tokens: &HashMap<String, String>) {
    let p = context_tokens_path(account_id);
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let raw = serde_json::to_string(tokens).unwrap_or_default();
    let _ = std::fs::write(&p, raw);
}

// ===== JID utilities =====

fn user_id_to_jid(user_id: &str) -> String {
    format!("wx:user:{user_id}")
}

fn jid_to_user_id(jid: &str) -> Option<&str> {
    jid.strip_prefix("wx:user:")
}

// ===== Markdown to plain text =====

fn markdown_to_plain(text: &str) -> String {
    let mut r = text.to_string();
    // Code blocks: keep content, remove fences
    r = regex::Regex::new(r"```[^\n]*\n?([\s\S]*?)```")
        .unwrap()
        .replace_all(&r, |caps: &regex::Captures| caps[1].trim().to_string())
        .to_string();
    // Images: remove entirely
    r = regex::Regex::new(r"!\[[^\]]*\]\([^)]*\)")
        .unwrap()
        .replace_all(&r, "")
        .to_string();
    // Links: keep display text
    r = regex::Regex::new(r"\[([^\]]+)\]\([^)]*\)")
        .unwrap()
        .replace_all(&r, "$1")
        .to_string();
    // Table separator rows: remove
    r = regex::Regex::new(r"^\|[\s:|-]+\|$")
        .unwrap()
        .replace_all(&r, "")
        .to_string();
    // Bold/italic markers
    r = regex::Regex::new(r"\*\*([^*]+)\*\*")
        .unwrap()
        .replace_all(&r, "$1")
        .to_string();
    r = regex::Regex::new(r"\*([^*]+)\*")
        .unwrap()
        .replace_all(&r, "$1")
        .to_string();
    r = regex::Regex::new(r"__([^_]+)__")
        .unwrap()
        .replace_all(&r, "$1")
        .to_string();
    r = regex::Regex::new(r"_([^_]+)_")
        .unwrap()
        .replace_all(&r, "$1")
        .to_string();
    // Headings: remove # prefix
    r = regex::Regex::new(r"^#{1,6}\s+")
        .unwrap()
        .replace_all(&r, "")
        .to_string();
    // Inline code backticks
    r = regex::Regex::new(r"`([^`]+)`")
        .unwrap()
        .replace_all(&r, "$1")
        .to_string();
    r.trim().to_string()
}

fn split_text(text: &str, max_len: usize) -> Vec<String> {
    if text.len() <= max_len {
        return vec![text.to_string()];
    }
    let mut parts: Vec<String> = Vec::new();
    let mut remaining = text;
    while remaining.len() > max_len {
        let chunk = &remaining[..max_len];
        let at = chunk
            .rfind('\n')
            .filter(|&pos| pos > max_len / 2)
            .unwrap_or(max_len);
        parts.push(remaining[..at].to_string());
        remaining = remaining[at..].trim_start_matches('\n');
    }
    if !remaining.is_empty() {
        parts.push(remaining.to_string());
    }
    parts
}

fn extract_text(items: Option<&[WeixinMessageItem]>) -> String {
    let items = match items {
        Some(v) if !v.is_empty() => v,
        _ => return String::new(),
    };
    for item in items {
        if item.item_type == Some(ITEM_TYPE_TEXT) {
            if let Some(ref ti) = item.text_item {
                if let Some(ref text) = ti.text {
                    return text.clone();
                }
            }
        }
        if item.item_type == Some(ITEM_TYPE_VOICE) {
            if let Some(ref vi) = item.voice_item {
                if let Some(ref text) = vi.text {
                    return format!("[Voice] {text}");
                }
            }
        }
    }
    let type_label = match items[0].item_type {
        Some(ITEM_TYPE_IMAGE) => "[Image]",
        Some(ITEM_TYPE_VOICE) => "[Voice]",
        Some(ITEM_TYPE_FILE) => "[File]",
        Some(ITEM_TYPE_VIDEO) => "[Video]",
        _ => "[Message]",
    };
    type_label.to_string()
}

// ===== Pending menu queue =====

struct PendingMenuEntry {
    options: Vec<InlineButton>,
    app_id: String,
}

// ===== WeChatChannel =====

pub struct WeChatChannel {
    account_id: String,
    api_base_url: String,
    token: Mutex<Option<String>>,
    base_url: Mutex<String>,
    context_tokens: Mutex<HashMap<String, String>>,
    get_updates_buf: Mutex<String>,
    connected: AtomicBool,
    shutdown: AtomicBool,
    http: reqwest::Client,
    handlers: Arc<RwLock<Vec<MessageCallback>>>,
    meta_handlers: Arc<RwLock<Vec<MetadataCallback>>>,
    menu_queues: Mutex<HashMap<String, Vec<PendingMenuEntry>>>,
}

impl WeChatChannel {
    pub fn new(account_id: String, api_base_url: Option<String>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("reqwest::Client::build");

        Self {
            account_id: account_id.clone(),
            api_base_url: api_base_url.unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
            token: Mutex::new(None),
            base_url: Mutex::new(DEFAULT_BASE_URL.to_string()),
            context_tokens: Mutex::new(load_context_tokens(&account_id)),
            get_updates_buf: Mutex::new(load_sync_buf(&account_id)),
            connected: AtomicBool::new(false),
            shutdown: AtomicBool::new(false),
            http,
            handlers: Arc::new(RwLock::new(Vec::new())),
            meta_handlers: Arc::new(RwLock::new(Vec::new())),
            menu_queues: Mutex::new(HashMap::new()),
        }
    }

    pub fn get_owner_jid(&self) -> Option<String> {
        let account = load_account(&self.account_id)?;
        let uid = account.user_id?.trim().to_string();
        if uid.is_empty() {
            None
        } else {
            Some(user_id_to_jid(&uid))
        }
    }

    async fn start_polling(self: Arc<Self>) {
        let mut next_timeout_ms = DEFAULT_LONG_POLL_TIMEOUT_MS;
        let mut consecutive_failures: u32 = 0;

        loop {
            if self.shutdown.load(Ordering::SeqCst) {
                return;
            }

            match self.do_poll(next_timeout_ms).await {
                Ok(resp) => {
                    if let Some(t) = resp.longpolling_timeout_ms {
                        if t > 0 {
                            next_timeout_ms = t;
                        }
                    }

                    let is_api_error =
                        resp.ret.map_or(false, |r| r != 0)
                            || resp.errcode.map_or(false, |e| e != 0);

                    if is_api_error {
                        if resp.errcode == Some(SESSION_EXPIRED_ERRCODE)
                            || resp.ret == Some(SESSION_EXPIRED_ERRCODE)
                        {
                            tracing::error!(
                                "[WeChatChannel] Session expired, QR login required again"
                            );
                            self.connected.store(false, Ordering::SeqCst);
                            return;
                        }
                        consecutive_failures += 1;
                        tracing::error!(
                            "[WeChatChannel] getUpdates error ret={:?} errcode={:?} ({consecutive_failures}/{})",
                            resp.ret,
                            resp.errcode,
                            MAX_CONSECUTIVE_FAILURES
                        );
                        if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                            consecutive_failures = 0;
                            tokio::time::sleep(Duration::from_secs(BACKOFF_DELAY_SECS)).await;
                        } else {
                            tokio::time::sleep(Duration::from_secs(RETRY_DELAY_SECS)).await;
                        }
                        continue;
                    }

                    consecutive_failures = 0;

                    // Update cursor
                    if let Some(ref buf) = resp.get_updates_buf {
                        *self.get_updates_buf.lock().await = buf.clone();
                        save_sync_buf(&self.account_id, buf);
                    }

                    // Process messages
                    if let Some(ref msgs) = resp.msgs {
                        for msg in msgs {
                            if msg.message_type != Some(MSG_TYPE_USER) {
                                continue;
                            }
                            let Some(ref from_user_id) = msg.from_user_id else {
                                continue;
                            };

                            // Cache context token
                            if let Some(ref ctx) = msg.context_token {
                                let mut tokens = self.context_tokens.lock().await;
                                tokens.insert(from_user_id.clone(), ctx.clone());
                                drop(tokens);
                                save_context_tokens(
                                    &self.account_id,
                                    &*self.context_tokens.lock().await,
                                );
                            }

                            let text = extract_text(msg.item_list.as_deref());
                            let chat_jid = user_id_to_jid(from_user_id);
                            let sender_name = from_user_id.split('@').next().unwrap_or("unknown");

                            // Numeric menu
                            if self.try_handle_menu(&chat_jid, &text).await {
                                continue;
                            }

                            let ts = msg.create_time_ms.map_or_else(
                                || {
                                    chrono::Utc::now()
                                        .format("%Y-%m-%dT%H:%M:%S.000Z")
                                        .to_string()
                                },
                                |ms| {
                                    let secs = ms / 1000;
                                    let ns = ((ms % 1000) * 1_000_000) as u32;
                                    chrono::DateTime::from_timestamp(secs, ns)
                                        .map(|dt| dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
                                        .unwrap_or_default()
                                },
                            );

                            let incoming = IncomingMessage {
                                id: msg
                                    .message_id
                                    .map(|mid| mid.to_string())
                                    .unwrap_or_else(|| {
                                        format!("{from_user_id}-{}", std::time::UNIX_EPOCH.elapsed().unwrap_or_default().as_millis())
                                    }),
                                chat_jid: chat_jid.clone(),
                                sender_jid: chat_jid.clone(),
                                sender_name: sender_name.to_string(),
                                content: text,
                                timestamp: ts,
                                is_from_me: false,
                                chat_type: ChatType::Private,
                                mentions_bot_username: Some(false),
                                bot_token: Some(self.account_id.clone()),
                                native_msg_id: None,
                            };

                            if let Ok(guard) = self.handlers.read() {
                                for h in guard.iter() {
                                    h(incoming.clone());
                                }
                            }
                        }
                    }
                }
                Err(_e) => {
                    if self.shutdown.load(Ordering::SeqCst) {
                        return;
                    }
                    consecutive_failures += 1;
                    tracing::error!(
                        "[WeChatChannel] getUpdates exception ({consecutive_failures}/{})",
                        MAX_CONSECUTIVE_FAILURES
                    );
                    if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                        consecutive_failures = 0;
                        tokio::time::sleep(Duration::from_secs(BACKOFF_DELAY_SECS)).await;
                    } else {
                        tokio::time::sleep(Duration::from_secs(RETRY_DELAY_SECS)).await;
                    }
                }
            }
        }
    }

    async fn do_poll(&self, timeout_ms: u64) -> Result<GetUpdatesResponse> {
        let base_url = {
            let guard = self.base_url.lock().await;
            guard.clone()
        };
        let token = {
            let guard = self.token.lock().await;
            guard.clone()
        };
        let buf = {
            let guard = self.get_updates_buf.lock().await;
            guard.clone()
        };

        let url = format!("{base_url}/ilink/bot/getupdates");
        let body = serde_json::json!({"get_updates_buf": buf}).to_string();

        let mut req = self
            .http
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .header("AuthorizationType", "ilink_bot_token")
            .header("X-WECHAT-UIN", random_wechat_uin())
            .body(body)
            .timeout(Duration::from_millis(timeout_ms + 5_000));

        if let Some(ref t) = token {
            req = req.header("Authorization", format!("Bearer {t}"));
        }

        let resp = req.send().await.context("getUpdates request")?;
        let text = resp.text().await.context("getUpdates response body")?;
        serde_json::from_str(&text).with_context(|| format!("parse getUpdates: {text}"))
    }

    async fn send_message_internal(
        &self,
        chat_jid: &str,
        text: &str,
    ) -> Result<()> {
        let token = {
            let guard = self.token.lock().await;
            guard
                .clone()
                .ok_or_else(|| anyhow::anyhow!("Not logged in"))?
        };
        let user_id = jid_to_user_id(chat_jid)
            .ok_or_else(|| anyhow::anyhow!("Invalid WeChat JID: {chat_jid}"))?
            .to_string();

        let context_token = {
            let tokens = self.context_tokens.lock().await;
            tokens.get(&user_id).cloned()
        };
        let Some(context_token) = context_token else {
            tracing::warn!("[WeChatChannel] No context_token for userId={user_id}, skipping send");
            return Ok(());
        };

        let base_url = {
            let guard = self.base_url.lock().await;
            guard.clone()
        };

        let plain = markdown_to_plain(text);
        for chunk in split_text(&plain, WECHAT_MAX_LEN) {
            let client_id = format!("senclaw-{}-{}", std::time::UNIX_EPOCH.elapsed().unwrap_or_default().as_millis(), random_hex(4));
            let body = SendMessageRequest {
                msg: SendMessageMsg {
                    from_user_id: String::new(),
                    to_user_id: user_id.clone(),
                    client_id,
                    message_type: MSG_TYPE_BOT,
                    message_state: MSG_STATE_FINISH,
                    item_list: vec![MessageItem {
                        item_type: ITEM_TYPE_TEXT,
                        text_item: Some(TextItem {
                            text: chunk,
                        }),
                    }],
                    context_token: Some(context_token.clone()),
                },
            };

            let url = format!("{base_url}/ilink/bot/sendmessage");
            let body_str = serde_json::to_string(&body)?;

            let resp = self
                .http
                .post(&url)
                .header("Content-Type", "application/json")
                .header("Content-Length", body_str.len())
                .header("AuthorizationType", "ilink_bot_token")
                .header("Authorization", format!("Bearer {token}"))
                .header("X-WECHAT-UIN", random_wechat_uin())
                .body(body_str)
                .timeout(Duration::from_millis(DEFAULT_API_TIMEOUT_MS))
                .send()
                .await
                .context("sendMessage request")?;

            if !resp.status().is_success() {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                anyhow::bail!("sendMessage HTTP {status}: {text}");
            }
        }
        Ok(())
    }

    async fn send_text_menu(
        &self,
        chat_jid: &str,
        text: &str,
        buttons: &[InlineButton],
    ) -> Result<()> {
        let numbered: Vec<String> = buttons
            .iter()
            .enumerate()
            .map(|(i, b)| format!("{}. {}", i + 1, b.label))
            .collect();
        let full_text = format!(
            "{text}\n\n{}\n\n(Reply with the number to choose)",
            numbered.join("\n")
        );

        {
            let mut queues = self.menu_queues.lock().await;
            queues
                .entry(chat_jid.to_string())
                .or_default()
                .push(PendingMenuEntry {
                    options: buttons.to_vec(),
                    app_id: self.account_id.clone(),
                });
        }

        // Auto-expire
        let _jid = chat_jid.to_string();
        // TODO: spawn cleanup task for menu expiry

        self.send_message_internal(chat_jid, &full_text).await
    }

    async fn try_handle_menu(&self, chat_jid: &str, content: &str) -> bool {
        let mut queues = self.menu_queues.lock().await;
        let queue = match queues.get_mut(chat_jid) {
            Some(q) if !q.is_empty() => q,
            _ => return false,
        };

        let num: Result<usize, _> = content.trim().parse();
        let Ok(num) = num else { return false };
        if num < 1 || num > queue[0].options.len() {
            return false;
        }

        let entry = queue.remove(0);
        if queue.is_empty() {
            queues.remove(chat_jid);
        }
        drop(queues);

        let selected = entry.options[num - 1].clone();
        // Callback dispatch via handlers
        let answer = call_callback_handlers(&self.handlers, &selected.callback_data, chat_jid);
        if let Some(ans) = answer {
            let _ = self.send_message_internal(chat_jid, &ans).await;
        }
        true
    }
}

fn call_callback_handlers(
    _handlers: &Arc<RwLock<Vec<MessageCallback>>>,
    _callback_data: &str,
    _jid: &str,
) -> Option<String> {
    // In the full implementation, callbackQueryHandlers are stored separately.
    // For now, this is a stub.
    None
}

#[async_trait]
impl Channel for WeChatChannel {
    fn id(&self) -> &'static str {
        "wechat"
    }

    async fn connect(&mut self) -> Result<()> {
        if self.connected.load(Ordering::SeqCst) {
            return Ok(());
        }

        let account = load_account(&self.account_id);

        if account.as_ref().map_or(true, |a| a.token.is_empty()) {
            // QR login needed
            tracing::info!("[WeChatChannel] No saved credentials, starting QR login...");
            let result = run_qr_login(&self.http, &self.api_base_url).await?;
            *self.token.lock().await = Some(result.token.clone());
            *self.base_url.lock().await = result
                .base_url
                .clone()
                .unwrap_or_else(|| self.api_base_url.clone());

            let data = WeixinAccountData {
                token: result.token,
                base_url: result.base_url,
                user_id: result.user_id,
                saved_at: chrono::Utc::now().to_rfc3339(),
            };
            save_account(&self.account_id, &data);
            let _account = Some(data);
        } else {
            let acc = account.as_ref().unwrap();
            *self.token.lock().await = Some(acc.token.clone());
            *self.base_url.lock().await = acc
                .base_url
                .clone()
                .unwrap_or_else(|| self.api_base_url.clone());
            tracing::info!(
                "[WeChatChannel] Loaded credentials userId={:?}",
                acc.user_id
            );
        }

        // Restore state
        *self.get_updates_buf.lock().await = load_sync_buf(&self.account_id);
        *self.context_tokens.lock().await = load_context_tokens(&self.account_id);

        self.connected.store(true, Ordering::SeqCst);
        self.shutdown.store(false, Ordering::SeqCst);

        // Background polling is started by the daemon layer after connect() returns.

        tracing::info!(
            "[WeChatChannel] Connected (accountId={} baseUrl={})",
            self.account_id,
            { let g = self.base_url.lock().await; g.clone() }
        );
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        self.shutdown.store(true, Ordering::SeqCst);
        self.connected.store(false, Ordering::SeqCst);
        tracing::info!("[WeChatChannel] Disconnected");
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    async fn send_message(
        &self,
        chat_jid: &str,
        text: &str,
        _bot_token: Option<&str>,
    ) -> Result<()> {
        self.send_message_internal(chat_jid, text).await
    }

    async fn send_file(
        &self,
        _chat_jid: &str,
        _file_path: &str,
        _caption: Option<&str>,
        _bot_token: Option<&str>,
    ) -> Result<()> {
        // iLink Bot API doesn't support file upload
        Ok(())
    }

    fn owns_jid(&self, chat_jid: &str) -> bool {
        if !chat_jid.starts_with("wx:") {
            return false;
        }
        // Multi-instance: only claim JIDs whose context_token we know
        // We can't block_on here, so check if jid matches known pattern
        true // simplified; full impl checks context_tokens
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
        _bot_token: Option<&str>,
    ) -> Result<()> {
        self.send_text_menu(chat_jid, text, buttons).await
    }
}

// ===== QR Login flow =====

struct QrLoginResult {
    token: String,
    base_url: Option<String>,
    user_id: Option<String>,
}

async fn run_qr_login(http: &reqwest::Client, api_base_url: &str) -> Result<QrLoginResult> {
    let base = if api_base_url.ends_with('/') {
        api_base_url.to_string()
    } else {
        format!("{api_base_url}/")
    };

    // Fetch QR code
    let qr_url = format!("{base}ilink/bot/get_bot_qrcode?bot_type=3");
    let resp = http
        .get(&qr_url)
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .context("fetch QR code")?;
    let mut qr_data: QrCodeResponse = resp.json().await.context("parse QR code response")?;

    println!("\n[WeChatChannel] Please scan the QR code with WeChat to log in:");
    // Print QR code to terminal if possible
    if let Ok(qr_img) = qrcode::QrCode::new(&qr_data.qrcode_img_content) {
        let rendered: String = qr_img
            .render::<char>()
            .quiet_zone(false)
            .module_dimensions(2, 1)
            .build();
        for line in rendered.split('\n') {
            println!("  {line}");
        }
    }
    println!("  {}\n", qr_data.qrcode_img_content);

    let mut refresh_count = 0u32;
    let mut scanned_printed = false;

    loop {
        let status_url = format!(
            "{}ilink/bot/get_qrcode_status?qrcode={}",
            base, qr_data.qrcode
        );
        let resp = http
            .get(&status_url)
            .header("iLink-App-ClientVersion", "1")
            .timeout(Duration::from_millis(QR_LONG_POLL_TIMEOUT_MS))
            .send()
            .await
            .context("poll QR status")?;

        let status: QrStatusResponse = resp.json().await.context("parse QR status")?;

        match status.status.as_str() {
            "wait" => {
                eprint!(".");
                std::io::stderr().flush().ok();
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            "scaned" => {
                if !scanned_printed {
                    println!("\n[WeChatChannel] QR scanned, please confirm in WeChat...");
                    scanned_printed = true;
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            "expired" => {
                refresh_count += 1;
                if refresh_count > MAX_QR_REFRESH {
                    anyhow::bail!(
                        "QR code expired multiple times, please restart login flow"
                    );
                }
                println!(
                    "\n[WeChatChannel] QR code expired, refreshing ({refresh_count}/{})...",
                    MAX_QR_REFRESH
                );
                // Refresh QR
                let resp2 = http
                    .get(&qr_url)
                    .timeout(Duration::from_secs(15))
                    .send()
                    .await
                    .context("refresh QR code")?;
                qr_data = resp2.json().await.context("parse refreshed QR")?;
                if let Ok(qr_img) = qrcode::QrCode::new(&qr_data.qrcode_img_content) {
                    let rendered: String = qr_img.render::<char>().quiet_zone(false).module_dimensions(2, 1).build();
                    for line in rendered.split('\n') {
                        println!("  {line}");
                    }
                }
                println!("  {}\n", qr_data.qrcode_img_content);
                scanned_printed = false;
            }
            "confirmed" => {
                let _ilink_bot_id = status
                    .ilink_bot_id
                    .ok_or_else(|| anyhow::anyhow!("missing ilink_bot_id"))?;
                let token = status
                    .bot_token
                    .ok_or_else(|| anyhow::anyhow!("missing bot_token"))?;
                println!("\n[WeChatChannel] WeChat login successful!");
                if let Some(ref uid) = status.ilink_user_id {
                    println!("[WeChatChannel] Bound user: {uid}");
                }
                return Ok(QrLoginResult {
                    token,
                    base_url: status.baseurl.or(Some(api_base_url.to_string())),
                    user_id: status.ilink_user_id,
                });
            }
            _ => {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

// ===== Tests =====

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jid_roundtrip() {
        let jid = user_id_to_jid("user123@im.wechat");
        assert_eq!(jid, "wx:user:user123@im.wechat");
        assert_eq!(jid_to_user_id(&jid), Some("user123@im.wechat"));
    }

    #[test]
    fn test_jid_to_user_id_none() {
        assert_eq!(jid_to_user_id("tg:user:123"), None);
        assert_eq!(jid_to_user_id("feishu:user:abc"), None);
        assert_eq!(jid_to_user_id("qq:user:xyz"), None);
    }

    #[test]
    fn test_owns_jid() {
        let ch = WeChatChannel::new("test".into(), None);
        assert!(ch.owns_jid("wx:user:abc123"));
        assert!(!ch.owns_jid("tg:123:user:456"));
    }

    #[test]
    fn test_markdown_to_plain_bold() {
        let result = markdown_to_plain("**hello** world");
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_markdown_to_plain_code_block() {
        let result = markdown_to_plain("```\nfn main() {}\n```");
        assert_eq!(result, "fn main() {}");
    }

    #[test]
    fn test_markdown_to_plain_link() {
        let result = markdown_to_plain("[click here](https://example.com)");
        assert_eq!(result, "click here");
    }

    #[test]
    fn test_markdown_to_plain_image() {
        let result = markdown_to_plain("text ![alt](img.png) more");
        assert_eq!(result, "text  more");
    }

    #[test]
    fn test_markdown_to_plain_heading() {
        let result = markdown_to_plain("## Section Title");
        assert_eq!(result, "Section Title");
    }

    #[test]
    fn test_extract_text_text() {
        let items = vec![WeixinMessageItem {
            item_type: Some(ITEM_TYPE_TEXT),
            text_item: Some(WeixinTextItem {
                text: Some("Hello".into()),
            }),
            voice_item: None,
        }];
        assert_eq!(extract_text(Some(&items)), "Hello");
    }

    #[test]
    fn test_extract_text_voice() {
        let items = vec![WeixinMessageItem {
            item_type: Some(ITEM_TYPE_VOICE),
            text_item: None,
            voice_item: Some(WeixinTextItem {
                text: Some("transcribed".into()),
            }),
        }];
        assert_eq!(extract_text(Some(&items)), "[Voice] transcribed");
    }

    #[test]
    fn test_extract_text_image() {
        let items = vec![WeixinMessageItem {
            item_type: Some(ITEM_TYPE_IMAGE),
            text_item: None,
            voice_item: None,
        }];
        assert_eq!(extract_text(Some(&items)), "[Image]");
    }

    #[test]
    fn test_extract_text_empty() {
        assert_eq!(extract_text(None), "");
        assert_eq!(extract_text(Some(&[])), "");
    }

    #[test]
    fn test_split_text_short() {
        let parts = split_text("hello", 2000);
        assert_eq!(parts, vec!["hello"]);
    }

    #[test]
    fn test_split_text_long() {
        let long = "x".repeat(3000);
        let parts = split_text(&long, 2000);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].len(), 2000);
    }
}
