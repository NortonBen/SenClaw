use app_space_sdk::SpaceClient;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::db::GenNode;

/// One turn in the chat panel.
#[derive(Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Deserialize)]
pub struct ChatBody {
    pub messages: Vec<ChatMessage>,
    /// Optional: an outline of the current map, so answers are grounded in it.
    #[serde(default)]
    pub map_outline: Option<String>,
}

const CHAT_SYSTEM: &str = "You are SenClaw Mindmap, an AI thinking partner embedded in a \
visual mind-mapping app. Help the user brainstorm, structure ideas, and reason about the \
mind map they are building. When the current map's outline is provided, ground your answer \
in it and suggest concrete branches, sub-topics, or restructurings. Be concise and use \
markdown (bullet lists, short headers). If the user asks you to generate or expand the map, \
tell them to use the ✨ Generate / Expand buttons so the nodes are inserted directly. \
Reply in the same language as the user's message.";

/// Conversational chat about the current map. Returns (answer, model).
pub async fn chat(body: &ChatBody) -> Result<(String, String), String> {
    let mut prompt = String::new();
    if let Some(outline) = &body.map_outline {
        if !outline.trim().is_empty() {
            prompt.push_str("Current mind map outline:\n");
            prompt.push_str(outline);
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
    bridge_llm(CHAT_SYSTEM, &prompt, 1200).await
}

const GEN_SYSTEM: &str = "You are a mind-map generator. Given a topic (and optionally the \
path of an existing branch and extra instructions), produce a well-structured hierarchy of \
sub-topics. Return ONLY valid JSON, no prose and no markdown fences, in exactly this shape:\n\
{\"children\":[{\"text\":\"Sub-topic\",\"note\":\"\",\"children\":[{\"text\":\"Detail\"}]}]}\n\
Rules: 'text' is a short label (2-6 words), 'note' is optional and may be empty, nest with \
'children'. Produce a balanced tree: aim for 4-7 top-level branches, each with 2-4 children, \
going 2-3 levels deep where useful. Do NOT repeat the parent topic as a child. Write the \
labels in the same language as the topic/instruction.";

const GEN_FROM_SOURCE_SYSTEM: &str = "You turn a piece of source content (a document, notes, \
OCR text, or a chat answer) into a structured mind map. Read the content and organize its key \
ideas into a clean hierarchy. Return ONLY valid JSON, no prose and no markdown fences, in \
exactly this shape:\n\
{\"children\":[{\"text\":\"Main idea\",\"note\":\"\",\"children\":[{\"text\":\"Detail\"}]}]}\n\
Rules: 'text' is a short label (2-6 words) faithful to the content, 'note' is optional. Group \
related points under 4-8 top-level branches, nest supporting details 1-3 levels deep. Do NOT \
invent facts not present in the content. Keep the mind map's language the same as the content.";

/// Generated tree parsed from the LLM.
pub struct Generated {
    pub children: Vec<GenNode>,
    pub model: String,
}

#[derive(Deserialize)]
struct GenRoot {
    #[serde(default)]
    children: Vec<GenNode>,
}

/// Ask the LLM to generate a subtree for `topic`. `parent_path` is the chain of
/// ancestor labels (root → parent) so generation stays in context; `instruction`
/// is optional extra guidance.
pub async fn generate(
    topic: &str,
    parent_path: &[String],
    instruction: Option<&str>,
    source: Option<&str>,
) -> Result<Generated, String> {
    let has_source = source.map(|s| !s.trim().is_empty()).unwrap_or(false);
    let system = if has_source { GEN_FROM_SOURCE_SYSTEM } else { GEN_SYSTEM };

    let mut prompt = String::new();
    if !topic.trim().is_empty() {
        prompt.push_str(&format!("Topic: {topic}\n"));
    }
    if !parent_path.is_empty() {
        prompt.push_str(&format!("Branch path: {}\n", parent_path.join(" › ")));
    }
    if let Some(i) = instruction {
        if !i.trim().is_empty() {
            prompt.push_str(&format!("Extra instruction: {i}\n"));
        }
    }
    if let Some(src) = source {
        if !src.trim().is_empty() {
            // Cap the source so we never blow the model's context.
            let capped = truncate(src.trim(), 8000);
            prompt.push_str(&format!("\nSource content to structure:\n\"\"\"\n{capped}\n\"\"\"\n"));
        }
    }
    prompt.push_str("\nReturn the JSON now.");

    let (text, model) = bridge_llm(system, &prompt, 2600).await?;
    let root = parse_gen(&text).ok_or_else(|| {
        format!("could not parse mind-map JSON from model output:\n{}", truncate(&text, 400))
    })?;
    if root.children.is_empty() {
        return Err("model returned an empty mind map".into());
    }
    Ok(Generated { children: root.children, model })
}

/// Extract a JSON object from possibly-fenced / chatty model output.
fn parse_gen(text: &str) -> Option<GenRoot> {
    // Direct parse first.
    if let Ok(r) = serde_json::from_str::<GenRoot>(text.trim()) {
        return Some(r);
    }
    // Strip ```json fences.
    let cleaned = strip_fences(text);
    if let Ok(r) = serde_json::from_str::<GenRoot>(cleaned.trim()) {
        return Some(r);
    }
    // Fall back to the first balanced {...} block.
    if let Some(block) = first_json_object(&cleaned) {
        // Some models emit a bare array — wrap it.
        if let Ok(children) = serde_json::from_str::<Vec<GenNode>>(block.trim()) {
            return Some(GenRoot { children });
        }
        if let Ok(r) = serde_json::from_str::<GenRoot>(&block) {
            return Some(r);
        }
    }
    // Last resort: the output was likely truncated mid-tree (token budget). Repair
    // it by trimming to the last complete node and closing open brackets.
    let repaired = repair_truncated_json(&cleaned)?;
    if let Ok(children) = serde_json::from_str::<Vec<GenNode>>(repaired.trim()) {
        return Some(GenRoot { children });
    }
    serde_json::from_str::<GenRoot>(&repaired).ok()
}

/// Salvage a truncated JSON object/array: cut back to the last complete node
/// (the last `}` outside a string) and append the closers for any still-open
/// brackets, so a cut-off model response still yields a usable subtree.
fn repair_truncated_json(text: &str) -> Option<String> {
    let start = text.find(|c| c == '{' || c == '[')?;
    let s = &text[start..];
    let bytes = s.as_bytes();
    // Find the last '}' that sits outside a string literal.
    let mut in_str = false;
    let mut esc = false;
    let mut last_close: Option<usize> = None;
    for (i, &b) in bytes.iter().enumerate() {
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'}' => last_close = Some(i),
            _ => {}
        }
    }
    let end = last_close?;
    let head = &s[..=end];
    // Re-scan the kept prefix to learn which brackets are still open.
    let mut stack: Vec<u8> = Vec::new();
    let mut in_str = false;
    let mut esc = false;
    for &b in head.as_bytes() {
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => stack.push(b'}'),
            b'[' => stack.push(b']'),
            b'}' | b']' => {
                stack.pop();
            }
            _ => {}
        }
    }
    let mut out = head.to_string();
    while let Some(closer) = stack.pop() {
        out.push(closer as char);
    }
    Some(out)
}

fn strip_fences(text: &str) -> String {
    let t = text.trim();
    if let Some(rest) = t.strip_prefix("```") {
        // drop an optional language tag on the first line
        let rest = rest.splitn(2, '\n').nth(1).unwrap_or(rest);
        return rest.trim_end_matches("```").to_string();
    }
    t.to_string()
}

/// Return the first balanced `{...}` (or `[...]`) substring.
fn first_json_object(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let start = bytes.iter().position(|&b| b == b'{' || b == b'[')?;
    let open = bytes[start];
    let close = if open == b'{' { b'}' } else { b']' };
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            x if x == open => depth += 1,
            x if x == close => {
                depth -= 1;
                if depth == 0 {
                    return Some(text[start..=i].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect::<String>() + "…"
    }
}

/// The daemon's configured LLMs via the SDK → { activeId, configs:[…] }.
pub async fn list_models() -> Result<Value, String> {
    let (active, configs) = client().list_models().await.map_err(|e| e.to_string())?;
    let configs: Vec<Value> = configs
        .into_iter()
        .map(|m| json!({ "id": m.id, "modelName": m.model_name, "provider": m.provider }))
        .collect();
    Ok(json!({ "activeId": active, "configs": configs }))
}

pub async fn set_active_model(id: &str) -> Result<(), String> {
    client().set_active_model(id).await.map_err(|e| e.to_string())
}

/// The app's single gateway to SenClaw services. Every LLM call goes through the
/// app-space-sdk (the daemon's Space-App open API) — the app never contacts an
/// LLM provider directly.
fn client() -> SpaceClient {
    if std::env::var("SENCLAW_SPACE_APP_ID").is_err() {
        std::env::set_var("SENCLAW_SPACE_APP_ID", "mindmap");
    }
    SpaceClient::from_env()
}

/// One-shot completion on SenClaw's active LLM via the SDK open API.
pub async fn bridge_llm(system: &str, user: &str, max_tokens: u32) -> Result<(String, String), String> {
    client()
        .llm_request(system, user, max_tokens)
        .await
        .map_err(|e| e.to_string())
}
