//! Channel layer: the CRM's connected accounts and the traffic over them.
//!
//! Each messaging platform is an adapter module exposing a uniform pair of
//! async fns:
//!
//! - `poll(db, channel) -> Result<Vec<Inbound>>` — fetch NEW inbound messages
//!   (deduped via the channel's stored `cursor`, which the adapter advances +
//!   persists itself). No webhooks: everything is polling.
//! - `send(db, channel, external_id, text)` — deliver one outbound message.
//!
//! The `ChannelManager` drives them: a slow scheduler polls Zalo/Facebook/
//! TikTok, and a supervisor runs a long-poll task per Telegram channel.
//!
//! **Inbound does not auto-reply.** A CRM is not a support bot: `ingest` lands
//! the message on a conversation, announces it on the event bus, and hands it
//! to `sale::on_inbound` to decide what — if anything — happens next. Every
//! outbound path goes through the sales guardrail, never straight back out of
//! the poller.
//!
//! Two units of time meet here. `Db` speaks Unix SECONDS; Zalo and Facebook
//! timestamp their messages in MILLIS, so their `cursor` stays in millis as the
//! adapter's own business and never reaches a `Db` method.

pub mod facebook;
pub mod social;
pub mod telegram;
pub mod tiktok;
pub mod zalo;

use crate::db::Db;
use crate::db_inbox::{Channel, Conversation};
use serde_json::json;
use std::collections::HashSet;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;

/// One inbound customer message, normalized across platforms.
pub struct Inbound {
    /// Platform-side chat/user/conversation id (the reply target).
    pub external_id: String,
    pub customer_name: String,
    pub text: String,
}

const POLL_INTERVAL: Duration = Duration::from_secs(15);
const TG_SUPERVISOR_INTERVAL: Duration = Duration::from_secs(10);

/// Shared HTTP client for every adapter. The timeout has to clear Telegram's
/// 25s long-poll with room to spare, or `getUpdates` would abort every cycle.
pub(crate) fn http() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(125))
            .build()
            .expect("build http client")
    })
}

/// Unix SECONDS — the unit every `Db` method takes.
pub(crate) fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Unix MILLIS — the unit Zalo/Facebook stamp their messages with, and so the
/// unit those adapters keep their `cursor` in.
pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub struct ChannelManager {
    pub db: Arc<Db>,
    pub events: broadcast::Sender<String>,
    /// Telegram channel ids that already have a running long-poll task.
    tg_running: Mutex<HashSet<i64>>,
}

impl ChannelManager {
    pub fn new(db: Arc<Db>, events: broadcast::Sender<String>) -> Arc<Self> {
        Arc::new(Self {
            db,
            events,
            tg_running: Mutex::new(HashSet::new()),
        })
    }

    /// Spawn the background pollers.
    pub fn spawn(self: &Arc<Self>) {
        let me = self.clone();
        tokio::spawn(async move { me.poll_scheduler().await });
        let me = self.clone();
        tokio::spawn(async move { me.telegram_supervisor().await });
    }

    /// Zalo / Facebook / TikTok: one shared slow loop over enabled channels.
    async fn poll_scheduler(self: Arc<Self>) {
        loop {
            let channels = self.db.enabled_channels().unwrap_or_default();
            for ch in channels
                .iter()
                .filter(|c| matches!(c.kind.as_str(), "zalo" | "facebook" | "tiktok" | "social"))
            {
                let res = match ch.kind.as_str() {
                    "zalo" => zalo::poll(&self.db, ch).await,
                    "facebook" => facebook::poll(&self.db, ch).await,
                    "tiktok" => tiktok::poll(&self.db, ch).await,
                    "social" => social::poll(&self.db, ch).await,
                    _ => Ok(Vec::new()),
                };
                match res {
                    Ok(msgs) => {
                        let n = msgs.len();
                        self.ingest(ch, msgs).await;
                        let _ = self.db.set_channel_sync(
                            ch.id,
                            "ok",
                            &format!("{n} tin mới"),
                            None,
                            now_secs(),
                        );
                    }
                    Err(e) => {
                        let _ = self
                            .db
                            .set_channel_sync(ch.id, "error", &e, None, now_secs());
                    }
                }
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    /// Ensure a long-poll task exists for each enabled Telegram channel.
    async fn telegram_supervisor(self: Arc<Self>) {
        loop {
            let channels = self.db.enabled_channels().unwrap_or_default();
            for ch in channels.iter().filter(|c| c.kind == "telegram") {
                let already = self.tg_running.lock().unwrap().contains(&ch.id);
                if already {
                    continue;
                }
                self.tg_running.lock().unwrap().insert(ch.id);
                let me = self.clone();
                let cid = ch.id;
                tokio::spawn(async move { me.telegram_loop(cid).await });
            }
            tokio::time::sleep(TG_SUPERVISOR_INTERVAL).await;
        }
    }

    /// One Telegram channel's long-poll loop; exits when the channel is
    /// disabled/removed so the supervisor can restart it if it comes back.
    async fn telegram_loop(self: Arc<Self>, channel_id: i64) {
        loop {
            let Some(ch) = self.db.get_channel(channel_id).ok().flatten() else {
                break;
            };
            if !ch.enabled {
                break;
            }
            match telegram::poll(&self.db, &ch).await {
                Ok(msgs) => {
                    let n = msgs.len();
                    self.ingest(&ch, msgs).await;
                    let _ = self.db.set_channel_sync(
                        ch.id,
                        "ok",
                        &format!("{n} tin mới"),
                        None,
                        now_secs(),
                    );
                }
                Err(e) => {
                    let _ = self
                        .db
                        .set_channel_sync(ch.id, "error", &e, None, now_secs());
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
        self.tg_running.lock().unwrap().remove(&channel_id);
    }

    /// Land a batch of inbound messages: resolve the thread (and, through it,
    /// the customer), record the message, announce it, then let the sales engine
    /// decide. Deliberately never replies from here.
    async fn ingest(&self, ch: &Channel, msgs: Vec<Inbound>) {
        if msgs.is_empty() {
            return;
        }
        for m in msgs {
            let now = now_secs();
            let conv = match self.db.get_or_create_conversation(
                ch.id,
                &ch.kind,
                &m.external_id,
                &m.customer_name,
                now,
            ) {
                Ok(c) => c,
                Err(_) => continue,
            };
            if self
                .db
                .add_conv_message(conv.id, "inbound", "user", &m.text, "received", now)
                .is_err()
            {
                continue;
            }
            crate::api::emit(
                &self.events,
                "message",
                json!({
                    "conversationId": conv.id,
                    "customerId": conv.customer_id,
                    "channel": ch.kind,
                    "externalId": m.external_id,
                    "direction": "inbound",
                    "role": "user",
                    "content": m.text,
                    "createdAt": now,
                }),
            );
            crate::sale::on_inbound(&self.db, &self.events, &conv, &m.text).await;
        }
    }

    /// Deliver text to a counterpart on `channel`. The single egress point for
    /// every platform — operator replies, approved sales drafts, MCP sends.
    pub async fn send_raw(
        &self,
        ch: &Channel,
        external_id: &str,
        text: &str,
    ) -> Result<(), String> {
        match ch.kind.as_str() {
            "telegram" => telegram::send(&self.db, ch, external_id, text).await,
            "zalo" => zalo::send(&self.db, ch, external_id, text).await,
            "facebook" => facebook::send(&self.db, ch, external_id, text).await,
            "tiktok" => tiktok::send(&self.db, ch, external_id, text).await,
            "social" => social::send(&self.db, ch, external_id, text).await,
            "websocket" => {
                // No HTTP call to make: the connected browser socket picks this
                // up off the event bus.
                crate::api::emit(
                    &self.events,
                    "outbound",
                    json!({ "externalId": external_id, "content": text }),
                );
                Ok(())
            }
            other => Err(format!("kênh '{other}' chưa hỗ trợ gửi")),
        }
    }

    /// Send to a conversation, resolving its channel first. A thread imported or
    /// seeded before any account was wired carries `channel_id = 0`, so fall back
    /// to whichever enabled account of that kind exists.
    pub async fn send_to_conversation(
        &self,
        conv: &Conversation,
        text: &str,
    ) -> Result<(), String> {
        let ch = match self
            .db
            .get_channel(conv.channel_id)
            .map_err(|e| e.to_string())?
        {
            Some(c) => c,
            None => self
                .db
                .channel_of_kind(&conv.channel_kind)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| {
                    format!(
                        "không tìm thấy kênh '{}' đang bật để gửi",
                        conv.channel_kind
                    )
                })?,
        };
        self.send_raw(&ch, &conv.external_id, text).await
    }

    /// Credential check for the Channels "Test" button. Only Telegram has a free
    /// idempotent identity endpoint (`getMe`); for the rest, a real probe would
    /// mean a live poll — which for Zalo can burn a token refresh — so we report
    /// what the config can tell us and let the first poll be the true test.
    pub async fn probe(&self, ch: &Channel) -> Result<String, String> {
        let has = |key: &str| {
            !ch.config
                .get(key)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .is_empty()
        };
        match ch.kind.as_str() {
            "telegram" => telegram::health_check(ch).await,
            "websocket" => Ok("Web chat luôn sẵn sàng".to_string()),
            "zalo" => {
                if has("access_token") {
                    Ok("đã có access_token (kiểm tra thật khi poll)".to_string())
                } else {
                    Err("thiếu access_token".to_string())
                }
            }
            "facebook" => {
                if has("page_id") && has("access_token") {
                    Ok("đã cấu hình (kiểm tra thật khi poll)".to_string())
                } else {
                    Err("thiếu page_id/access_token".to_string())
                }
            }
            "tiktok" => Err("TikTok Shop IM là kênh thử nghiệm".to_string()),
            "social" => social::health_check(ch).await,
            other => Err(format!("kênh '{other}' không hỗ trợ kiểm tra")),
        }
    }
}
