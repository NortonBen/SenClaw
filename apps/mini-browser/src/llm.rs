//! Deep AI integration: page-grounded chat, an agentic `act` loop (natural
//! language → real browser actions), and structured `extract`. Every model call
//! goes through the app-space-sdk — the app never talks to a provider directly.

use app_space_sdk::SpaceClient;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::session::BrowserSession;

#[derive(Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Deserialize)]
pub struct ChatBody {
    pub messages: Vec<ChatMessage>,
    /// Optional compact snapshot of the current page for grounding.
    #[serde(default)]
    pub page_context: Option<String>,
}

const CHAT_SYSTEM: &str = "You are SenClaw Browser, an AI assistant embedded in a real web \
browser. You can see the current page (its title, URL and a text summary are provided when \
available). Help the user understand pages, find information, summarize content, and plan \
what to do next. If the user asks you to actually DO something on the page (click, fill a \
form, log in, navigate a flow), tell them you can run it via the Act ▶ button / browser_act \
tool. Be concise, use markdown. Reply in the user's language (Vietnamese or English).";

/// Page-grounded conversational chat. Returns (answer, model).
pub async fn chat(body: &ChatBody) -> Result<(String, String), String> {
    let mut prompt = String::new();
    if let Some(ctx) = &body.page_context {
        if !ctx.trim().is_empty() {
            prompt.push_str("Current page:\n");
            prompt.push_str(ctx);
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

const ACT_SYSTEM: &str = "You are a web-automation agent driving a real browser. Each turn you \
receive the current page URL/title and a numbered list of interactive elements. Decide the \
SINGLE next action to progress the user's goal. Respond with ONLY a JSON object, no prose, no \
markdown fences, in this shape:\n\
{\"action\":\"click|type|navigate|scroll|press|done\",\"index\":<number>,\"text\":\"...\",\"url\":\"...\",\"key\":\"Enter\",\"reason\":\"short why\"}\n\
Rules: use 'index' from the element list for click/type. 'type' fills the element at 'index' \
with 'text' (set \"submit\":true to press Enter after). 'navigate' goes to 'url'. 'scroll' \
uses 'text' = \"down\"|\"up\". 'press' sends a key like Enter/Escape. When the goal is \
achieved (or truly impossible), use action \"done\" and put the outcome in 'reason'. Never \
invent an index that isn't listed.";

/// Agentic loop: pursue `instruction` on the live page for up to `max_steps`.
/// Returns a JSON log of the steps taken plus the model used.
pub async fn act(
    session: &BrowserSession,
    instruction: &str,
    max_steps: usize,
) -> Result<Value, String> {
    let max_steps = max_steps.clamp(1, 12);
    let mut steps: Vec<Value> = Vec::new();
    let mut model = String::new();

    for step in 0..max_steps {
        let snap = session.snapshot().await.map_err(|e| e.to_string())?;
        let listing = format_elements(&snap);
        let url = snap["url"].as_str().unwrap_or("");
        let title = snap["title"].as_str().unwrap_or("");

        let mut prompt = format!("Goal: {instruction}\n\nStep {}/{max_steps}\nURL: {url}\nTitle: {title}\n\nInteractive elements:\n{listing}\n", step + 1);
        if !steps.is_empty() {
            prompt.push_str("\nActions so far:\n");
            for (i, s) in steps.iter().enumerate() {
                prompt.push_str(&format!("{}. {}\n", i + 1, s));
            }
        }
        prompt.push_str("\nReturn the next action JSON now.");

        let (text, m) = bridge_llm(ACT_SYSTEM, &prompt, 400).await?;
        if model.is_empty() {
            model = m;
        }
        let action = parse_action(&text).ok_or_else(|| format!("could not parse action from: {}", truncate(&text, 200)))?;
        let kind = action["action"].as_str().unwrap_or("done").to_string();

        if kind == "done" {
            steps.push(json!({ "action": "done", "reason": action["reason"] }));
            break;
        }

        let result = exec_action(session, &action).await;
        steps.push(json!({
            "action": kind,
            "detail": action,
            "result": match &result { Ok(v) => v.clone(), Err(e) => json!({ "error": e }) },
        }));
        if result.is_err() {
            // Let the model see the failure and adapt on the next turn, but stop
            // if we're on the last step.
        }
        // brief settle time for navigations/clicks to take effect
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    let final_info = session.info().await.map_err(|e| e.to_string())?;
    Ok(json!({ "goal": instruction, "steps": steps, "final": final_info, "model": model }))
}

async fn exec_action(session: &BrowserSession, a: &Value) -> Result<Value, String> {
    let kind = a["action"].as_str().unwrap_or("");
    let m = |r: anyhow::Result<Value>| r.map_err(|e| e.to_string());
    match kind {
        "click" => m(session.click_index(idx(a)).await),
        "type" => {
            let submit = a["submit"].as_bool().unwrap_or(false);
            m(session.type_index(idx(a), a["text"].as_str().unwrap_or(""), submit).await)
        }
        "navigate" => m(session.navigate(a["url"].as_str().unwrap_or("")).await),
        "scroll" => {
            let dir = a["text"].as_str().unwrap_or("down");
            let dy = if dir.eq_ignore_ascii_case("up") { -600.0 } else { 600.0 };
            m(session.scroll(0.0, dy).await)
        }
        "press" => m(session.press_key(a["key"].as_str().unwrap_or("Enter")).await),
        other => Err(format!("unknown action: {other}")),
    }
}

fn idx(a: &Value) -> i64 {
    a["index"].as_i64().or_else(|| a["index"].as_str().and_then(|s| s.parse().ok())).unwrap_or(-1)
}

fn format_elements(snap: &Value) -> String {
    let empty = vec![];
    let els = snap["elements"].as_array().unwrap_or(&empty);
    if els.is_empty() {
        return "(no interactive elements detected)".to_string();
    }
    let mut out = String::new();
    for e in els {
        let idx = e["idx"].as_i64().unwrap_or(-1);
        let tag = e["tag"].as_str().unwrap_or("");
        let role = e["role"].as_str().unwrap_or("");
        let ty = e["type"].as_str().unwrap_or("");
        let text = e["text"].as_str().unwrap_or("");
        let kind = if !role.is_empty() { role } else if !ty.is_empty() { ty } else { tag };
        out.push_str(&format!("[{idx}] {kind} \"{}\"\n", truncate(text, 80)));
    }
    out
}

const EXTRACT_SYSTEM: &str = "You extract information from a web page. You are given the page's \
text content and a request. If the request is a question, answer it from the content only. If \
it asks for structured data, return valid JSON matching the requested shape. Do not invent \
facts not present in the content. Be concise.";

/// Answer a question about the page or extract structured data from it.
pub async fn extract(session: &BrowserSession, request: &str) -> Result<(String, String), String> {
    let snap = session.snapshot().await.map_err(|e| e.to_string())?;
    let text = snap["text"].as_str().unwrap_or("");
    let url = snap["url"].as_str().unwrap_or("");
    let title = snap["title"].as_str().unwrap_or("");
    let prompt = format!(
        "URL: {url}\nTitle: {title}\n\nPage content:\n\"\"\"\n{}\n\"\"\"\n\nRequest: {request}\n",
        truncate(text, 8000)
    );
    bridge_llm(EXTRACT_SYSTEM, &prompt, 1000).await
}

/// Tolerant JSON-action parser.
fn parse_action(text: &str) -> Option<Value> {
    if let Ok(v) = serde_json::from_str::<Value>(text.trim()) {
        if v.is_object() {
            return Some(v);
        }
    }
    let cleaned = strip_fences(text);
    if let Ok(v) = serde_json::from_str::<Value>(cleaned.trim()) {
        if v.is_object() {
            return Some(v);
        }
    }
    let block = first_json_object(&cleaned)?;
    serde_json::from_str::<Value>(&block).ok()
}

fn strip_fences(text: &str) -> String {
    let t = text.trim();
    if let Some(rest) = t.strip_prefix("```") {
        let rest = rest.splitn(2, '\n').nth(1).unwrap_or(rest);
        return rest.trim_end_matches("```").to_string();
    }
    t.to_string()
}

fn first_json_object(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let start = bytes.iter().position(|&b| b == b'{')?;
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_str {
            if esc { esc = false; } else if b == b'\\' { esc = true; } else if b == b'"' { in_str = false; }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => depth += 1,
            b'}' => {
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

fn client() -> SpaceClient {
    if std::env::var("SENCLAW_SPACE_APP_ID").is_err() {
        std::env::set_var("SENCLAW_SPACE_APP_ID", "mini-browser");
    }
    SpaceClient::from_env()
}

pub async fn bridge_llm(system: &str, user: &str, max_tokens: u32) -> Result<(String, String), String> {
    client().llm_request(system, user, max_tokens).await.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::{first_json_object, parse_action};

    #[test]
    fn parses_plain_json() {
        let a = parse_action(r#"{"action":"click","index":3}"#).unwrap();
        assert_eq!(a["action"], "click");
        assert_eq!(a["index"], 3);
    }

    #[test]
    fn parses_fenced_json() {
        let a = parse_action("```json\n{\"action\":\"done\",\"reason\":\"ok\"}\n```").unwrap();
        assert_eq!(a["action"], "done");
    }

    #[test]
    fn parses_chatty_json() {
        let a = parse_action("Sure! Here is the action:\n{\"action\":\"type\",\"index\":1,\"text\":\"hi\"} — done").unwrap();
        assert_eq!(a["action"], "type");
        assert_eq!(a["text"], "hi");
    }

    #[test]
    fn first_object_is_balanced() {
        let b = first_json_object("prefix {\"a\":{\"b\":1}} suffix").unwrap();
        assert_eq!(b, "{\"a\":{\"b\":1}}");
    }
}
