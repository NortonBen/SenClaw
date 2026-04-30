use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::Channel;
use crate::proto::relay::*;
use crate::proto::relay::channel_relay_client::ChannelRelayClient;
use crate::util::crypto::Crypto;
use anyhow::{anyhow, Result, Context};
use tracing::info;

pub type RelayMessageHandler = Arc<dyn Fn(RelayMessage) + Send + Sync + 'static>;

pub struct RelayClient {
    client: ChannelRelayClient<Channel>,
    crypto: Arc<Crypto>,
    channel_id: String,
    sender_id: String,
    outbound_tx: mpsc::Sender<RelayMessage>,
}

impl RelayClient {
    pub async fn connect(
        hub_url: String,
        channel_id: String,
        sender_id: String,
        access_token: String,
        encryption_key: [u8; 32],
        handler: Option<RelayMessageHandler>,
    ) -> Result<Self> {
        info!("Connecting to gRPC relay at {}...", hub_url);
        let is_https = hub_url.starts_with("https://");
        let mut endpoint = Channel::from_shared(hub_url.clone())
            .context("invalid relay URL")?;
        if is_https {
            let tls = tonic::transport::ClientTlsConfig::new()
                .with_native_roots();
            endpoint = endpoint.tls_config(tls).context("TLS config")?;
        }
        let channel = endpoint
            .connect_timeout(std::time::Duration::from_secs(10))
            .connect()
            .await
            .map_err(|e| {
                tracing::error!("Failed to connect to gRPC relay: {e}");
                anyhow!(e)
            })?;
        info!("gRPC channel established.");
        
        let client = ChannelRelayClient::new(channel);
        let crypto = Arc::new(Crypto::new(encryption_key));
        let (outbound_tx, outbound_rx) = mpsc::channel(100);

        let mut client_clone = client.clone();
        let outbound_stream = ReceiverStream::new(outbound_rx);

        let mut request = tonic::Request::new(outbound_stream);
        info!("Establishing gRPC stream for channel_id: {}", channel_id);
        request.metadata_mut().insert("channel_id", channel_id.parse()?);
        request.metadata_mut().insert("access_token", access_token.parse()?);
        // Send initial handshake message to unblock the stream
        let handshake_msg = RelayMessage {
            channel_id: channel_id.clone(),
            sender_id: sender_id.clone(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            message_id: format!("handshake-{}", uuid::Uuid::new_v4()),
            payload: Some(relay_message::Payload::Control(ControlMessage {
                r#type: 0, // PING
                metadata: String::new(),
            })),
        };
        let _ = outbound_tx.send(handshake_msg).await;

        // Establish the stream immediately
        let response: tonic::Response<tonic::Streaming<RelayMessage>> = client_clone.stream(request).await?;
        let mut inbound_stream = response.into_inner();

        let cid_clone = channel_id.clone();
        let crypto_clone = Arc::clone(&crypto);

        // Spawn inbound processor
        tokio::spawn(async move {
            info!("Inbound relay stream processor started for channel: {}", cid_clone);
            while let Ok(Some(msg)) = inbound_stream.message().await {
                info!("Relay stream received message: {} from {}", msg.message_id, msg.sender_id);
                if msg.channel_id != cid_clone { 
                    tracing::warn!("Received message for wrong channel_id: expected {}, got {}", cid_clone, msg.channel_id);
                    continue; 
                }
                
                if let Some(ref h) = handler {
                    h(msg);
                }
            }
            tracing::warn!("Inbound relay stream closed for channel: {}", cid_clone);
        });

        Ok(Self {
            client,
            crypto,
            channel_id,
            sender_id,
            outbound_tx,
        })
    }

    pub async fn send_message(&self, text: &str) -> Result<()> {
        let (nonce, ciphertext, tag) = self.crypto.encrypt(text.as_bytes())?;
        
        info!("Sending relay message ({} bytes encrypted)", ciphertext.len());
        let msg = RelayMessage {
            channel_id: self.channel_id.clone(),
            sender_id: self.sender_id.clone(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            message_id: uuid::Uuid::new_v4().to_string(),
            payload: Some(relay_message::Payload::EncryptedData(EncryptedData {
                nonce,
                ciphertext,
                tag,
            })),
        };

        self.outbound_tx.send(msg).await?;
        Ok(())
    }

    pub fn decrypt_payload(&self, data: &EncryptedData) -> Result<String> {
        let plaintext = self.crypto.decrypt(&data.nonce, &data.ciphertext, &data.tag)?;
        String::from_utf8(plaintext).map_err(|e| anyhow!("Invalid UTF-8: {}", e))
    }
}
