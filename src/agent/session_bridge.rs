//! Message format conversion for agent input. Mirrors `src-old/agent/SessionBridge.ts`.
//!
//! Fetches recent group messages from SQLite and formats them as XML.

use crate::db::Db;
use crate::types::StoredMessage;

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Format [`StoredMessage`] list into an XML string for agent input.
/// Returns empty string when the list is empty.
pub fn format_messages_for_agent(messages: &[StoredMessage]) -> String {
    if messages.is_empty() {
        return String::new();
    }
    let lines: Vec<String> = messages
        .iter()
        .map(|m| {
            let sender = if m.is_bot_reply {
                "assistant".to_string()
            } else {
                escape_xml(&m.sender_name)
            };
            format!(
                "<message sender=\"{}\" time=\"{}\">{}</message>",
                sender,
                m.timestamp,
                escape_xml(&m.content)
            )
        })
        .collect();
    format!("<messages>\n{}\n</messages>", lines.join("\n"))
}

/// Load messages after the last agent timestamp for a group, and format for agent input.
pub fn build_prompt_for_group(db: &Db, chat_jid: &str) -> (String, Option<String>) {
    let since = db
        .get_last_agent_timestamp(chat_jid)
        .ok()
        .flatten();
    let messages = db
        .get_messages(chat_jid, since.as_deref())
        .unwrap_or_default();
    let last_msg_ts = messages.last().map(|m| m.timestamp.clone());
    (format_messages_for_agent(&messages), last_msg_ts)
}
