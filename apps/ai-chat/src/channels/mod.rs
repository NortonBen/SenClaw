//! Channel layer. Each messaging platform is an adapter module exposing a
//! uniform pair of async fns:
//!
//! - `poll(db, channel) -> Result<Vec<Inbound>>` — fetch NEW inbound messages
//!   (deduped via the channel's stored `cursor`, which the adapter advances +
//!   persists itself). No webhooks: everything is polling.
//! - `send(db, channel, external_id, text)` — deliver one outbound reply.
//!
//! The `ChannelManager` drives them: a slow scheduler polls Zalo/Facebook/
//! TikTok, and a supervisor runs a long-poll task per Telegram channel. Both
//! funnel inbound messages through `engine::process_inbound` (transport-
//! agnostic) and send the returned reply back out. WebSocket is special —
//! it's served directly by the axum handler in `api.rs`; outbound to a WS
//! session is delivered over the live event stream.

pub mod facebook;
pub mod telegram;
pub mod tiktok;
pub mod zalo;

use crate::db::{Channel, Db, Session};
use crate::engine;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;
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

pub struct ChannelManager {
    pub db: Arc<Db>,
    pub events: broadcast::Sender<String>,
    /// Telegram channel ids that already have a running long-poll task.
    tg_running: Mutex<HashSet<i64>>,
}

impl ChannelManager {
    pub fn new(db: Arc<Db>, events: broadcast::Sender<String>) -> Arc<Self> {
        Arc::new(Self { db, events, tg_running: Mutex::new(HashSet::new()) })
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
            let channels = self.db.list_enabled_channels().unwrap_or_default();
            for ch in channels
                .iter()
                .filter(|c| matches!(c.kind.as_str(), "zalo" | "facebook" | "tiktok"))
            {
                let res = match ch.kind.as_str() {
                    "zalo" => zalo::poll(&self.db, ch).await,
                    "facebook" => facebook::poll(&self.db, ch).await,
                    "tiktok" => tiktok::poll(&self.db, ch).await,
                    _ => Ok(Vec::new()),
                };
                match res {
                    Ok(msgs) => {
                        let n = msgs.len();
                        self.ingest(ch, msgs).await;
                        let _ = self.db.set_channel_sync(ch.id, "ok", &format!("{n} tin mới"), None);
                    }
                    Err(e) => {
                        let _ = self.db.set_channel_sync(ch.id, "error", &e, None);
                    }
                }
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    /// Ensure a long-poll task exists for each enabled Telegram channel.
    async fn telegram_supervisor(self: Arc<Self>) {
        loop {
            let channels = self.db.list_enabled_channels().unwrap_or_default();
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
                    let _ = self.db.set_channel_sync(ch.id, "ok", &format!("{n} tin mới"), None);
                }
                Err(e) => {
                    let _ = self.db.set_channel_sync(ch.id, "error", &e, None);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
        self.tg_running.lock().unwrap().remove(&channel_id);
    }

    /// Run a batch of inbound messages through the engine and deliver replies.
    async fn ingest(&self, ch: &Channel, msgs: Vec<Inbound>) {
        if msgs.is_empty() {
            return;
        }
        let Some(bot) = self.db.get_bot(&ch.bot_key).ok().flatten() else {
            return;
        };
        for m in msgs {
            let jid = format!("{}:{}:{}", ch.kind, ch.id, m.external_id);
            let session = match self.db.get_or_create_session(
                &ch.bot_key,
                &ch.kind,
                ch.id,
                &m.external_id,
                &jid,
                &m.customer_name,
            ) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let outcome = engine::process_inbound(&self.db, &self.events, &bot, &session, &m.text).await;
            if let Some(reply) = outcome.reply.filter(|r| !r.trim().is_empty()) {
                let _ = self.send_raw(ch, &m.external_id, &reply).await;
            }
        }
    }

    /// Deliver text to a customer on `channel` (used for bot replies + operator
    /// handoff replies + the `chat_send` MCP tool).
    pub async fn send_raw(&self, ch: &Channel, external_id: &str, text: &str) -> Result<(), String> {
        match ch.kind.as_str() {
            "telegram" => telegram::send(&self.db, ch, external_id, text).await,
            "zalo" => zalo::send(&self.db, ch, external_id, text).await,
            "facebook" => facebook::send(&self.db, ch, external_id, text).await,
            "tiktok" => tiktok::send(&self.db, ch, external_id, text).await,
            "websocket" => {
                // The connected browser socket picks this up over the event bus.
                let _ = self.events.send(
                    serde_json::json!({ "type": "outbound", "externalId": external_id, "content": text })
                        .to_string(),
                );
                Ok(())
            }
            other => Err(format!("kênh '{other}' chưa hỗ trợ gửi")),
        }
    }

    /// Send to a session (resolves the channel first). Used by the operator
    /// handoff-reply endpoint and the `chat_send` MCP tool.
    pub async fn send_to_session(&self, session: &Session, text: &str) -> Result<(), String> {
        let ch = self
            .db
            .get_channel(session.channel_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "không tìm thấy kênh của phiên này".to_string())?;
        self.send_raw(&ch, &session.external_id, text).await
    }
}
