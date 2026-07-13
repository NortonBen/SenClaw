//! In-app "Agent" panel backend: proxies the user's chat messages to the
//! SenClaw daemon's active LLM via the app-space-sdk bridge, grounding each
//! turn in the current document's plain text.

use app_space_sdk::SpaceClient;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Deserialize)]
pub struct ChatBody {
    #[allow(dead_code)]
    #[serde(default)]
    pub doc_id: Option<i64>,
    pub doc_text: String,
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub instruction: Option<String>,
}

const SYSTEM: &str = "You are DOCX Writer, an AI writing partner embedded in the SenClaw \
DOCX Editor. Help the user reason about, critique, and improve the Word document they are \
editing. When they ask for a rewrite, produce clean prose ready to paste in. When they ask \
for feedback, be specific: quote a short snippet and say what to change and why. Use \
markdown lightly (short headers, bullets) when it helps clarity. Keep answers under ~250 \
words unless the user explicitly asks for more. Reply in the same language as the user's \
last message.\n\n\
If the user asks you to REWRITE the whole document, put the finished replacement between \
lines that read exactly `<<<DOC>>>` and `<<<END>>>` — no other text between those markers, \
no code fences. The UI will offer the user a one-click 'Apply' button.";

fn client() -> SpaceClient {
    if std::env::var("SENCLAW_SPACE_APP_ID").is_err() {
        std::env::set_var("SENCLAW_SPACE_APP_ID", "docx-editor");
    }
    SpaceClient::from_env()
}

pub async fn chat(body: &ChatBody) -> Result<(String, String), String> {
    let mut prompt = String::new();
    prompt.push_str("Current document (plain text):\n<<<DOC>>>\n");
    prompt.push_str(&body.doc_text);
    prompt.push_str("\n<<<END>>>\n\n");
    if let Some(inst) = &body.instruction {
        if !inst.trim().is_empty() {
            prompt.push_str("Extra author guidance: ");
            prompt.push_str(inst);
            prompt.push_str("\n\n");
        }
    }
    prompt.push_str("Conversation:\n");
    for m in &body.messages {
        let who = match m.role.as_str() {
            "assistant" => "Assistant",
            "system" => "System",
            _ => "User",
        };
        prompt.push_str(&format!("{who}: {}\n", m.content));
    }
    prompt.push_str("Assistant:");
    client()
        .llm_request(SYSTEM, &prompt, 1600)
        .await
        .map_err(|e| e.to_string())
}

/// Best-effort extraction of a full-document rewrite delimited by
/// `<<<DOC>>>` / `<<<END>>>` in the assistant's reply.
pub fn extract_rewrite(reply: &str) -> Option<String> {
    let start_tag = "<<<DOC>>>";
    let end_tag = "<<<END>>>";
    let start = reply.find(start_tag)? + start_tag.len();
    let rest = &reply[start..];
    let end = rest.find(end_tag)?;
    let body = &rest[..end];
    Some(body.trim_matches(['\r', '\n']).to_string())
}
