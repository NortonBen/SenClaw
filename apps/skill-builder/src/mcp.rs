//! MCP server (HTTP/SSE) exposing the Skill Builder to SenClaw agents. This is
//! what lets the *agent itself* build a new skill on the user's behalf: inspect
//! the current inventory, draft a skill from a requirement, and install it (with
//! auto-load triggers) — all without leaving the chat.
//!
//! Server name: `skill-builder-mcp`. Tools: `skill_inventory`, `skill_draft`,
//! `skill_create`, `skill_list`, `skill_remove`.

use axum::{
    extract::State,
    response::sse::{Event, Sse},
    Json,
};
use futures_util::stream::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{convert::Infallible, sync::Arc};

use crate::api::AppState;
use crate::generate;

#[derive(Deserialize, Debug)]
pub struct JsonRpcRequest {
    #[allow(dead_code)]
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    pub params: Option<Value>,
}

pub async fn mcp_sse(
    State(state): State<Arc<AppState>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = state.mcp_tx.subscribe();
    let stream = async_stream::stream! {
        yield Ok(Event::default().event("endpoint").data("/api/mcp/message".to_string()));
        while let Ok(msg) = rx.recv().await {
            yield Ok(Event::default().event("message").data(msg));
        }
    };
    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default())
}

fn text_result(text: String) -> Value {
    json!({ "content": [{ "type": "text", "text": text }] })
}
fn json_result(v: Value) -> Value {
    text_result(serde_json::to_string_pretty(&v).unwrap_or_default())
}
fn error_result(text: String) -> Value {
    json!({ "isError": true, "content": [{ "type": "text", "text": text }] })
}

pub async fn mcp_message(
    State(state): State<Arc<AppState>>,
    Json(req): Json<JsonRpcRequest>,
) -> Json<Value> {
    let reply = |result: Value| -> Json<Value> {
        let resp = json!({ "jsonrpc": "2.0", "id": req.id, "result": result });
        let _ = state.mcp_tx.send(resp.to_string());
        Json(resp)
    };

    match req.method.as_str() {
        "initialize" => reply(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "skill-builder-mcp", "version": "1.0.0" }
        })),
        "ping" => reply(json!({})),
        "notifications/initialized" => {
            Json(json!({ "jsonrpc": "2.0", "id": req.id, "result": {} }))
        }
        "tools/list" => reply(json!({ "tools": tools_list() })),
        "tools/call" => {
            let params = req.params.clone().unwrap_or_default();
            let name = params["name"].as_str().unwrap_or("").to_string();
            let args = params["arguments"].clone();
            reply(call_tool(&state, &name, &args).await)
        }
        _ => Json(json!("ok")),
    }
}

fn tools_list() -> Value {
    json!([
        {
            "name": "skill_inventory",
            "description": "List the capabilities already available in this SenClaw instance — installed skills (with their triggers), sub-agents, and MCP servers/tools. ALWAYS call this first when building a new skill, so the new skill reuses existing tools/sub-agents and does not duplicate an existing skill.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "skill_draft",
            "description": "Design (but do NOT install) a new SenClaw skill from a plain-language requirement. Returns the proposed name, description, triggers, markdown body, and the AI's rationale (what it reuses / why). Use this to preview a skill before creating it. Grounds the design in the live inventory automatically.",
            "inputSchema": { "type": "object", "properties": {
                "requirement": { "type": "string", "description": "What the skill is for — the task it should accomplish." },
                "when_to_run": { "type": "string", "description": "Optional: when the skill should trigger / auto-load (conditions, example user phrasings)." }
            }, "required": ["requirement"] }
        },
        {
            "name": "skill_create",
            "description": "Design AND install a new skill into SenClaw in one step, writing its `triggers` into the frontmatter so it auto-surfaces on matching prompts. Use when the user asks to 'tạo skill / create a skill' and wants it ready to use. Returns the installed skill plus the design rationale. Set overwrite=true to replace a skill of the same name.",
            "inputSchema": { "type": "object", "properties": {
                "requirement": { "type": "string", "description": "What the skill is for." },
                "when_to_run": { "type": "string", "description": "Optional: when it should trigger / auto-load." },
                "overwrite": { "type": "boolean", "description": "Overwrite an existing skill of the same generated name (default false)." }
            }, "required": ["requirement"] }
        },
        {
            "name": "skill_create_exact",
            "description": "Install a skill from EXACT fields you provide (no AI generation), writing triggers into frontmatter. Use when you have already drafted a skill (e.g. via skill_draft) and want to install it verbatim, or to hand-author one.",
            "inputSchema": { "type": "object", "properties": {
                "name": { "type": "string", "description": "Skill slug (lowercase, digits, hyphens)." },
                "description": { "type": "string", "description": "'Use when …' one-line description for matching." },
                "content": { "type": "string", "description": "The markdown body of SKILL.md (instructions)." },
                "triggers": { "type": "array", "items": { "type": "string" }, "description": "Keyword phrases that auto-surface the skill." },
                "overwrite": { "type": "boolean" }
            }, "required": ["name", "description", "content"] }
        },
        {
            "name": "skill_list",
            "description": "List the skills currently installed in this SenClaw instance (name, description, triggers, source).",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "skill_remove",
            "description": "Uninstall a local skill by name. Only removes user/managed skills.",
            "inputSchema": { "type": "object", "properties": {
                "name": { "type": "string", "description": "The skill name/slug to remove." }
            }, "required": ["name"] }
        }
    ])
}

async fn call_tool(state: &Arc<AppState>, name: &str, args: &Value) -> Value {
    match name {
        "skill_inventory" => {
            let inv = state.daemon.inventory().await;
            json_result(json!({
                "skills": inv.skills,
                "subagents": inv.subagents,
                "mcpServers": inv.mcp_servers,
            }))
        }
        "skill_draft" | "skill_create" => {
            let requirement = args["requirement"].as_str().unwrap_or("").trim();
            if requirement.is_empty() {
                return error_result("requirement is required".into());
            }
            let when = args["when_to_run"].as_str().unwrap_or("");
            let inv = state.daemon.inventory().await;
            let draft = match generate::draft(requirement, when, &inv).await {
                Ok(d) => d,
                Err(e) => return error_result(e),
            };
            if name == "skill_draft" {
                return json_result(serde_json::to_value(&draft).unwrap_or_default());
            }
            // skill_create: also install it.
            let overwrite = args["overwrite"].as_bool().unwrap_or(false);
            match state
                .daemon
                .create_skill(&draft.name, &draft.description, &draft.content, &draft.triggers, overwrite)
                .await
            {
                Ok(_) => json_result(json!({
                    "installed": true,
                    "name": draft.name,
                    "description": draft.description,
                    "triggers": draft.triggers,
                    "uses_mcp": draft.uses_mcp,
                    "uses_subagents": draft.uses_subagents,
                    "rationale": draft.rationale,
                    "hint": format!("Skill '{}' installed. It will auto-surface when a prompt matches its triggers, or the agent can load it via the Skill tool.", draft.name),
                })),
                Err(e) => error_result(format!(
                    "drafted '{}' but install failed: {e}. (If it already exists, retry with overwrite=true.)",
                    draft.name
                )),
            }
        }
        "skill_create_exact" => {
            let nm = args["name"].as_str().unwrap_or("").trim();
            let desc = args["description"].as_str().unwrap_or("").trim();
            let content = args["content"].as_str().unwrap_or("").trim();
            if nm.is_empty() || content.is_empty() {
                return error_result("name and content are required".into());
            }
            let triggers: Vec<String> = args["triggers"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let overwrite = args["overwrite"].as_bool().unwrap_or(false);
            match state
                .daemon
                .create_skill(nm, desc, content, &triggers, overwrite)
                .await
            {
                Ok(_) => {
                    json_result(json!({ "installed": true, "name": nm, "triggers": triggers }))
                }
                Err(e) => error_result(e.to_string()),
            }
        }
        "skill_list" => match state.daemon.list_skills().await {
            Ok(v) => json_result(v),
            Err(e) => error_result(e.to_string()),
        },
        "skill_remove" => {
            let nm = args["name"].as_str().unwrap_or("").trim();
            if nm.is_empty() {
                return error_result("name is required".into());
            }
            match state.daemon.delete_skill(nm).await {
                Ok(_) => json_result(json!({ "removed": true, "name": nm })),
                Err(e) => error_result(e.to_string()),
            }
        }
        _ => error_result(format!("Unknown tool: {name}")),
    }
}
