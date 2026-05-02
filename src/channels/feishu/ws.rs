use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use tokio::sync::Mutex;

use crate::channels::MessageCallback;
use crate::types::{ChatType, IncomingMessage};

use super::helpers::{
    check_bot_mention, make_jid, parse_text_content, remove_bot_mention_placeholders,
};
use super::types::{DedupState, WsEventEnvelope, WsMessageEvent};

// ===== WS event types & processing =====

/// Process a raw WS event payload (JSON bytes from data frame) into an IncomingMessage
/// and dispatch to registered handlers. Runs synchronously inside the WS callback.
pub(crate) fn process_ws_event(
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
