//! Prompt hook executor — queries an LLM with event context and parses a decision.
//!
//! Port of TS `hooks/PromptExecutor.ts`.

use anyhow::{anyhow, Result};
use reqwest::Client;
use tokio::time::{timeout, Duration};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::zen_core::query_llm;
use crate::zen_core::{
    create_user_message, ContentBlock, ModelProfile, Tool, ToolContext, ToolOutput,
    ToolResultMessage,
};

use super::types::{HookDefinition, HookOutput};

const DECISION_SYSTEM_PROMPT: &str = r#"You are a hook decision agent in an AI agent system.
You will receive context about an event and must make a decision.

IMPORTANT: You MUST respond with ONLY a JSON object, no other text. Valid responses:
- {"decision": "approve"} — allow the action to proceed
- {"decision": "reject", "reason": "..."} — block the action with explanation
- {"decision": "skip"} — skip permission check, auto-approve
- {"decision": "approve", "updatedInput": {...}} — approve with modified tool input (PreToolUse only)
- {"decision": "approve", "additionalContext": "..."} — approve and inject context into conversation"#;

/// A placeholder tool used when invoking the LLM for prompt hooks.
/// The LLM should never call it; it exists only to satisfy the tools parameter.
struct NullTool;

#[async_trait::async_trait]
impl Tool for NullTool {
    fn name(&self) -> &str {
        "null"
    }
    fn description(&self) -> &str {
        "Placeholder tool. Do not call."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {}})
    }
    fn is_read_only(&self) -> bool {
        true
    }
    async fn call(
        &self,
        _: serde_json::Value,
        _: &ToolContext<'_>,
    ) -> anyhow::Result<Vec<ToolOutput>> {
        Ok(vec![ToolOutput::Result {
            data: serde_json::Value::Null,
            result_for_assistant: String::new(),
        }])
    }
    fn gen_tool_result_message(
        &self,
        _: &serde_json::Value,
        _: &serde_json::Value,
    ) -> ToolResultMessage {
        ToolResultMessage {
            title: String::new(),
            summary: String::new(),
            content: serde_json::Value::Null,
        }
    }
    fn get_display_title(&self, _: &serde_json::Value) -> String {
        String::new()
    }
}

/// Execute a prompt hook — query the configured LLM and parse its JSON decision.
///
/// Timeout defaults to 30 s.
pub async fn execute_prompt_hook(
    hook: &HookDefinition,
    input_json: &str,
    client: &Client,
    profile: &ModelProfile,
    cancel: Option<&CancellationToken>,
) -> Result<HookOutput> {
    let prompt_text = match &hook.prompt {
        Some(p) if !p.trim().is_empty() => p.clone(),
        _ => return Ok(HookOutput::default()),
    };

    if let Some(tok) = cancel {
        if tok.is_cancelled() {
            return Err(anyhow!("Hook aborted"));
        }
    }

    let timeout_secs = hook.timeout.unwrap_or(30);

    let system_prompt = format!("{DECISION_SYSTEM_PROMPT}\nYour task: {prompt_text}");
    let user_msg = create_user_message(vec![ContentBlock::Text {
        text: format!("Event context:\n```json\n{input_json}\n```"),
    }]);

    let inner_cancel = CancellationToken::new();
    let _cancel_guard = if let Some(tok) = cancel {
        let c = inner_cancel.clone();
        let tok = tok.clone();
        Some(tokio::spawn(async move {
            tok.cancelled().await;
            c.cancel();
        }))
    } else {
        None
    };

    let null_tool: std::sync::Arc<dyn Tool> = std::sync::Arc::new(NullTool);
    let tools_slice = vec![null_tool];
    let messages_slice = vec![user_msg];

    let run = query_llm::query_llm(
        client,
        &messages_slice,
        &system_prompt,
        &tools_slice,
        &inner_cancel,
        profile,
        false,
        false,
    );

    let response = match timeout(Duration::from_secs(timeout_secs), run).await {
        Ok(Ok(msg)) => msg,
        Ok(Err(e)) => return Err(e),
        Err(_) => {
            inner_cancel.cancel();
            return Err(anyhow!("Prompt hook timed out after {timeout_secs}s"));
        }
    };

    // Extract text content
    let mut result_text = String::new();
    for block in &response.message.content {
        if let ContentBlock::Text { text } = block {
            result_text = text.clone();
            break;
        }
    }

    info!(
        "[hooks] Prompt hook response: {}",
        &result_text[..result_text.len().min(200)]
    );

    Ok(parse_prompt_response(&result_text))
}

fn parse_prompt_response(text: &str) -> HookOutput {
    // Try to extract a JSON object from the response
    if let Some(start) = text.find('{') {
        if let Some(end) = text.rfind('}') {
            let json_str = &text[start..=end];
            match serde_json::from_str::<serde_json::Value>(json_str) {
                Ok(v) => {
                    let decision = v["decision"].as_str().map(str::to_string);
                    let blocked = decision.as_deref() == Some("reject");
                    return HookOutput {
                        decision: decision.clone(),
                        blocked: Some(blocked),
                        reason: v["reason"].as_str().map(str::to_string),
                        updated_input: v.get("updatedInput").cloned(),
                        additional_context: v["additionalContext"].as_str().map(str::to_string),
                        response: Some(text.to_string()),
                        ..Default::default()
                    };
                }
                Err(e) => {
                    warn!("[hooks] Failed to parse prompt hook JSON: {e}");
                }
            }
        }
    }

    // Keyword fallback
    let lower = text.to_lowercase();
    if lower.contains("reject") || lower.contains("block") || lower.contains("deny") {
        return HookOutput {
            blocked: Some(true),
            reason: Some(text.to_string()),
            decision: Some("reject".into()),
            response: Some(text.to_string()),
            ..Default::default()
        };
    }

    HookOutput {
        decision: Some("approve".into()),
        response: Some(text.to_string()),
        ..Default::default()
    }
}
