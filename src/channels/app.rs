//! App Connector channel adapter using gRPC relay.
//!
//! JID format: `app:{channel_id}:user:{sender_id}`

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

use crate::channels::{Channel, MessageCallback, MetadataCallback};
use crate::clawhub::relay_client::{RelayClient, RelayMessageHandler};
use crate::proto::relay::relay_message;
use crate::types::{ChatType, IncomingMessage};
use chrono;

pub struct AppChannel {
    hub_url: String,
    channel_id: String,
    access_token: String,
    encryption_key: [u8; 32],
    handlers: Arc<RwLock<Vec<MessageCallback>>>,
    meta_handlers: Arc<RwLock<Vec<MetadataCallback>>>,
    client: Mutex<Option<Arc<RelayClient>>>,
    connected: AtomicBool,
}

impl AppChannel {
    pub fn new(
        hub_url: String,
        channel_id: String,
        access_token: String,
        encryption_key: [u8; 32],
    ) -> Self {
        Self {
            hub_url,
            channel_id,
            access_token,
            encryption_key,
            handlers: Arc::new(RwLock::new(Vec::new())),
            meta_handlers: Arc::new(RwLock::new(Vec::new())),
            client: Mutex::new(None),
            connected: AtomicBool::new(false),
        }
    }

    pub fn parse_jid(jid: &str) -> Option<(String, String)> {
        let parts: Vec<&str> = jid.split(':').collect();
        if parts.len() >= 4 && parts[0] == "app" && parts[2] == "user" {
            Some((parts[1].to_string(), parts[3].to_string()))
        } else {
            None
        }
    }
}

#[async_trait]
impl Channel for AppChannel {
    fn id(&self) -> &'static str {
        "app"
    }

    async fn connect(&self) -> Result<()> {
        let hub_url = self.hub_url.clone();
        let channel_id = self.channel_id.clone();
        let access_token = self.access_token.clone();
        let encryption_key = self.encryption_key;

        let handlers = Arc::clone(&self.handlers);
        let cid = channel_id.clone();

        let (msg_tx, mut msg_rx) = tokio::sync::mpsc::channel(100);

        let handler: RelayMessageHandler = Arc::new(move |msg| {
            let _ = msg_tx.try_send(msg);
        });

        info!(
            "[AppChannel] Connecting to relay at {} for channel {} (access_token {})",
            hub_url, channel_id, access_token
        );
        let mut backoff_secs = 2u64;
        let client: RelayClient = loop {
            match RelayClient::connect(
                hub_url.clone(),
                channel_id.clone(),
                "semaclaw-daemon".to_string(),
                access_token.clone(),
                encryption_key,
                Some(Arc::clone(&handler)),
            )
            .await
            {
                Ok(c) => break c,
                Err(e) => {
                    error!(
                        "[AppChannel] Failed to connect to relay: {e}; retrying in {backoff_secs}s"
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
                    backoff_secs = (backoff_secs * 2).min(60);
                }
            }
        };

        let client_arc = Arc::new(client);
        let client_for_inbound = Arc::clone(&client_arc);

        tokio::spawn(async move {
            while let Some(msg) = msg_rx.recv().await {
                info!("[AppChannel] Received relay message: {}", msg.message_id);
                if let Some(relay_message::Payload::EncryptedData(data)) = msg.payload {
                    match client_for_inbound.decrypt_payload(&data) {
                        Ok(text) => {
                            info!(
                                "[AppChannel] Decrypted message from {}: {}",
                                msg.sender_id, text
                            );
                            let incoming = IncomingMessage {
                                id: msg.message_id,
                                chat_jid: format!("app:{}:user:{}", cid, msg.sender_id),
                                sender_name: msg.sender_id.clone(),
                                sender_jid: format!("app:{}:user:{}", cid, msg.sender_id),
                                content: text,
                                timestamp: chrono::Utc::now().to_rfc3339(),
                                is_from_me: false,
                                chat_type: ChatType::Private,
                                mentions_bot_username: Some(true),
                                bot_token: None,
                                native_msg_id: None,
                            };

                            let h_list = handlers.read().unwrap();
                            for h in h_list.iter() {
                                h(incoming.clone());
                            }
                        }
                        Err(e) => error!("[AppChannel] Failed to decrypt relay message: {e}"),
                    }
                } else if matches!(msg.payload, Some(relay_message::Payload::Control(_))) {
                    // server heartbeat / control frame — ignore silently
                } else {
                    warn!(
                        "[AppChannel] Received relay message with no encrypted payload: {}",
                        msg.message_id
                    );
                }
            }
        });

        *self.client.lock().await = Some(client_arc);
        self.connected.store(true, Ordering::SeqCst);
        info!(
            "[AppChannel] Connected to relay for channel {}",
            self.channel_id
        );
        Ok(())
    }

    async fn disconnect(&self) -> Result<()> {
        *self.client.lock().await = None;
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
        _bot_token: Option<&str>,
    ) -> Result<()> {
        let lock = self.client.lock().await;
        if let Some(ref client) = *lock {
            info!("[AppChannel] Sending message to {}: {}", chat_jid, text);
            client.send_message(text).await?;
            Ok(())
        } else {
            Err(anyhow!("AppChannel not connected"))
        }
    }

    async fn send_file(
        &self,
        _chat_jid: &str,
        _file_path: &str,
        _caption: Option<&str>,
        _bot_token: Option<&str>,
    ) -> Result<()> {
        // TODO: implement file transfer via relay
        warn!("AppChannel::send_file not implemented yet");
        Ok(())
    }

    fn owns_jid(&self, chat_jid: &str) -> bool {
        if let Some((cid, _)) = Self::parse_jid(chat_jid) {
            cid == self.channel_id
        } else {
            false
        }
    }

    fn on_message(&self, handler: MessageCallback) {
        self.handlers.write().unwrap().push(handler);
    }

    fn on_metadata(&self, handler: MetadataCallback) {
        self.meta_handlers.write().unwrap().push(handler);
    }
}
