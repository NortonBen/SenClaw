use app_space_sdk::SpaceClient;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// A pinned code snippet the user attached from the editor (Cmd/Ctrl+L).
#[derive(Deserialize)]
pub struct Pin {
    pub path: String,
    #[serde(default)]
    pub start_line: u32,
    #[serde(default)]
    pub end_line: u32,
    pub code: String,
    #[serde(default)]
    pub lang: Option<String>,
}

#[derive(Deserialize)]
pub struct ChatBody {
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub pins: Vec<Pin>,
    /// Optional: the path of the file currently focused in the editor.
    #[serde(default)]
    pub active_file: Option<String>,
    /// Run mode: "chat" (default), "plan", "agent", "dag". Shapes the system prompt.
    #[serde(default)]
    pub mode: Option<String>,
}

/// Extra system guidance per run mode.
fn mode_suffix(mode: &str) -> &'static str {
    match mode {
        "plan" => " MODE: PLAN. Do not write full implementations. Instead produce a concise, \
            numbered step-by-step plan to accomplish the request, listing the files to change \
            (as `path`) and what to do in each. End with any risks or open questions.",
        "agent" => " MODE: AGENT. Work autonomously toward the goal. When you need to read or change \
            files, state the exact code-ide MCP tool call you would make (ide_read_file / \
            ide_write_file / ide_search) and why, then give the resulting edit as an applyable block.",
        "dag" => " MODE: DAG. Decompose the task into a small dependency graph of sub-tasks. For each \
            node give: id, short title, which nodes it depends on, and the concrete action. Present \
            it as an ordered list grouped by dependency level.",
        _ => "",
    }
}

const SYSTEM: &str =
    "You are SenClaw Code, an AI pair-programmer embedded in a VSCode-style editor. \
The user works in a local workspace and may pin specific code selections as context. \
Ground every answer in the provided code — cite files as `path:line`. When proposing a change, \
show the full replacement inside a fenced code block and name the target file on the line above it \
(e.g. `// file: src/app.ts`) so it can be applied. Be concise and practical. \
Reply in the same language as the user's message.";

/// Assemble the pinned snippets + conversation into a single prompt for the
/// one-shot bridge LLM, and return (answer, model).
pub async fn chat(body: &ChatBody) -> Result<(String, String), String> {
    let mut prompt = String::new();

    if let Some(f) = &body.active_file {
        if !f.is_empty() {
            prompt.push_str(&format!("Currently open file: {f}\n\n"));
        }
    }

    if !body.pins.is_empty() {
        prompt.push_str("Pinned code context:\n");
        for p in &body.pins {
            let lang = p.lang.clone().unwrap_or_default();
            let loc = if p.end_line > 0 {
                format!("{}:{}-{}", p.path, p.start_line, p.end_line)
            } else {
                p.path.clone()
            };
            prompt.push_str(&format!("\n// {loc}\n```{lang}\n{}\n```\n", p.code));
        }
        prompt.push('\n');
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

    let mode = body.mode.as_deref().unwrap_or("chat");
    let system = format!("{SYSTEM}{}", mode_suffix(mode));
    bridge_llm(&system, &prompt, 1500).await
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

/// Set the daemon's active main model via the SDK.
pub async fn set_active_model(id: &str) -> Result<(), String> {
    client()
        .set_active_model(id)
        .await
        .map_err(|e| e.to_string())
}

/// The app's single gateway to SenClaw services. Every LLM call goes through the
/// app-space-sdk (which talks to the daemon's Space-App open API) — the IDE never
/// contacts an LLM provider directly.
fn client() -> SpaceClient {
    // Default the app id to "code-ide" when not injected by the daemon.
    if std::env::var("SENCLAW_SPACE_APP_ID").is_err() {
        std::env::set_var("SENCLAW_SPACE_APP_ID", "code-ide");
    }
    SpaceClient::from_env()
}

/// One-shot completion on SenClaw's active LLM via the SDK open API.
pub async fn bridge_llm(
    system: &str,
    user: &str,
    max_tokens: u32,
) -> Result<(String, String), String> {
    client()
        .llm_request(system, user, max_tokens)
        .await
        .map_err(|e| e.to_string())
}
