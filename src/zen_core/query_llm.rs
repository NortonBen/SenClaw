//! LLM query layer — routes to OpenAI or Anthropic adapters.
//!
//! Each adapter streams the response via SSE, accumulates content blocks,
//! and returns a complete assistant [`Message`].
//!
//! Port of TS `queryLLM.ts` + `adapt/openai.ts` + `adapt/anthropic.ts`.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use futures::StreamExt;
use reqwest::Client;
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info};

use super::*;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

// ============================================================================
// Main entry point
// ============================================================================

/// Query an LLM and return the assistant message.
///
/// Routes to OpenAI or Anthropic adapter based on `profile.adapt` or
/// auto-detection from the provider field.
///
/// For `provider = local-mlx`, the in-process path always wins so a stale
/// `adapt: "openai"` left over from merged/copied LLM configs cannot force an HTTP request.
pub async fn query_llm(
    client: &Client,
    messages: &[Message],
    system_prompt: &str,
    tools: &[Arc<dyn Tool>],
    cancel: &CancellationToken,
    profile: &ModelProfile,
    thinking: bool,
    stream: bool,
) -> Result<Message> {
    let adapt = effective_adapter(profile);
    info!(
        "[llm] request start provider={} model={} adapter={} stream={} messages={} tools={}",
        profile.provider,
        profile.model_name,
        adapt,
        stream,
        messages.len(),
        tools.len()
    );

    let result = match adapt {
        "anthropic" => {
            query_anthropic(
                client,
                messages,
                system_prompt,
                tools,
                cancel,
                profile,
                thinking,
                stream,
            )
            .await
        }
        "local-mlx-native" => {
            query_local_mlx_native(messages, system_prompt, tools, cancel, profile).await
        }
        _ => {
            query_openai(
                client,
                messages,
                system_prompt,
                tools,
                cancel,
                profile,
                thinking,
                stream,
            )
            .await
        }
    };
    match &result {
        Ok(msg) => {
            let blocks = msg.message.content.len();
            let tool_calls = msg
                .message
                .content
                .iter()
                .filter(|b| matches!(b, ContentBlock::ToolUse { .. }))
                .count();
            info!(
                "[llm] request complete provider={} model={} blocks={} tool_calls={}",
                profile.provider, profile.model_name, blocks, tool_calls
            );
        }
        Err(e) => {
            tracing::warn!(
                "[llm] request error provider={} model={}: {e}",
                profile.provider,
                profile.model_name
            );
        }
    }
    result
}

/// Auto-detect adapter from provider name.
fn resolve_adapter(provider: &str) -> &str {
    let lower = provider.to_lowercase();
    if lower.contains("anthropic") || lower.contains("claude") {
        "anthropic"
    } else if lower == "local-mlx" || lower == "local-mlx-native" {
        "local-mlx-native"
    } else {
        "openai"
    }
}

/// Prefer routing implied by `provider` for local MLX; otherwise use explicit `adapt`, then inference from provider.
fn effective_adapter(profile: &ModelProfile) -> &str {
    let p = profile.provider.to_lowercase();
    if p == "local-mlx" || p == "local-mlx-native" {
        return "local-mlx-native";
    }
    profile
        .adapt
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| resolve_adapter(&profile.provider))
}

// ============================================================================
// Local MLX native adapter (in-process, Apple Silicon)
// ============================================================================

/// Run inference through the local mlx-rs engine. Uses the same OpenAI-shaped
/// `messages` / `tools` as `query_openai` for chat-template rendering; parses
/// Qwen-style `<tool_call>…</tool_call>` segments from the generated text into
/// `ContentBlock::ToolUse` when present.
#[allow(unused_variables)]
async fn query_local_mlx_native(
    messages: &[Message],
    system_prompt: &str,
    tools: &[Arc<dyn Tool>],
    cancel: &CancellationToken,
    profile: &ModelProfile,
) -> Result<Message> {
    #[cfg(not(feature = "local-mlx"))]
    {
        bail!(
            "local-mlx-native adapter requires the `local-mlx` cargo feature; \
             rebuild with `cargo build --features local-mlx` (Apple Silicon + Metal toolchain)"
        );
    }

    #[cfg(feature = "local-mlx")]
    {
        use crate::local_model::{LocalModelRuntime, MlxNativeEngine};
        use crate::config::Config;

        let api_msgs = openai_messages_for_api(messages, system_prompt)?;
        let tool_objs = build_openai_tools(tools);

        // Resolve model directory: <local_models_dir>/<model_name with '/' → '__'>
        let cfg = Config::from_env();
        let model_key =
            crate::gateway::ui_server::local_models::canonical_local_model_id(&profile.model_name);
        let safe = model_key.replace('/', "__");
        let model_dir = cfg.paths.local_models_dir.join(safe);

        // Pick up KV-cache settings (turboquant bit width) when the user opted in.
        let kv_bits = crate::gateway::ui_server::local_models::load_settings_blocking(
            &cfg.paths.local_models_dir,
        )
        .kv_cache_bits;
        // Single global registry: one [`MlxNativeEngine`] per canonical model id + KV settings.
        let engine = crate::gateway::ui_server::local_models::get_or_create_loaded_engine(
            &model_key,
            &model_dir,
            kv_bits,
        );
        let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(32);

        let engine_clone = engine.clone();
        let msgs_clone = api_msgs.clone();
        let tools_clone = tool_objs.clone();
        let gen_handle = tokio::spawn(async move {
            engine_clone
                .generate_stream(msgs_clone, tools_clone, tx)
                .await
        });

        let mut text_buf = String::new();
        loop {
            tokio::select! {
                chunk = rx.recv() => match chunk {
                    Some(c) => text_buf.push_str(&c),
                    None => break, // sender dropped → generation done
                },
                _ = cancel.cancelled() => {
                    gen_handle.abort();
                    bail!("local-mlx-native: cancelled");
                }
            }
        }
        gen_handle
            .await
            .context("local-mlx-native: generation task panicked")??;
        debug!("[local-mlx-native] generated {} chars", text_buf.len());

        let (clean_text, tool_calls_from_text) = split_qwen_tool_calls_from_model_text(&text_buf);
        build_assistant_message(&clean_text, "", &tool_calls_from_text)
    }
}

// ============================================================================
// OpenAI adapter
// ============================================================================

pub(crate) fn build_openai_tools(tools: &[Arc<dyn Tool>]) -> Vec<Value> {
    tools
        .iter()
        .map(|t| {
            let schema = t.input_schema();
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": t.name(),
                    "description": t.description(),
                    "parameters": schema,
                }
            })
        })
        .collect()
}

/// Strip Qwen / HF-style `<tool_call>…</tool_call>` segments and turn them into OpenAI-style
/// `tool_calls` objects for [`build_assistant_message`].
#[cfg(feature = "local-mlx")]
fn split_qwen_tool_calls_from_model_text(s: &str) -> (String, Vec<Value>) {
    const OPEN: &str = "<tool_call>";
    const CLOSE: &str = "</tool_call>";

    let mut rest = s;
    let mut out = String::with_capacity(s.len());
    let mut tool_calls: Vec<Value> = Vec::new();
    let mut idx: u32 = 0;

    while let Some(start) = rest.find(OPEN) {
        out.push_str(&rest[..start]);
        let after = &rest[start + OPEN.len()..];
        let Some(end_rel) = after.find(CLOSE) else {
            out.push_str(&rest[start..]);
            return (out, tool_calls);
        };
        let json_str = after[..end_rel].trim();
        rest = &after[end_rel + CLOSE.len()..];

        let Ok(v) = serde_json::from_str::<Value>(json_str) else {
            continue;
        };

        let items: Vec<Value> = match v {
            Value::Array(a) => a,
            one => vec![one],
        };

        for item in items {
            let Some(name) = item.get("name").and_then(|x| x.as_str()) else {
                continue;
            };
            if name.is_empty() {
                continue;
            }
            let args = item
                .get("arguments")
                .cloned()
                .unwrap_or(Value::Object(Default::default()));
            let args_str = serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string());
            let id = format!("local_tool_{idx}");
            idx += 1;
            tool_calls.push(serde_json::json!({
                "id": id,
                "type": "function",
                "function": {
                    "name": name,
                    "arguments": args_str,
                }
            }));
        }
    }
    out.push_str(rest);
    (out, tool_calls)
}

/// Convert internal [`Message`] history to OpenAI Chat Completions `messages` JSON.
///
/// OpenAI-compatible APIs (DeepSeek, OpenRouter, etc.) expect:
/// - `assistant` + tools: `tool_calls` on the assistant message, **not** `content` parts with `tool_use`.
/// - tool outputs: separate messages with `role: "tool"` and `tool_call_id`.
///
/// Our internal format mirrors Anthropic (`ToolUse` / `ToolResult` inside `content`), so we expand
/// that here — otherwise providers reject the body (`unknown variant tool_use`).
///
/// Thinking / reasoning: [`ContentBlock::Thinking`] is serialized as `reasoning_content` on
/// `assistant` messages (required by DeepSeek and similar when thinking mode is on).
pub(crate) fn openai_messages_for_api(messages: &[Message], system_prompt: &str) -> Result<Vec<Value>> {
    let mut api_msgs: Vec<Value> = Vec::new();

    if !system_prompt.is_empty() {
        api_msgs.push(serde_json::json!({
            "role": "system",
            "content": system_prompt,
        }));
    }

    for msg in messages {
        match msg.message.role.as_str() {
            "user" => {
                let mut text_acc = String::new();
                let mut content_parts: Vec<Value> = Vec::new();
                for b in &msg.message.content {
                    match b {
                        ContentBlock::Text { text } => {
                            if !text_acc.is_empty() {
                                text_acc.push('\n');
                            }
                            text_acc.push_str(text);
                        }
                        ContentBlock::Image { source } => {
                            // Flush text accumulator first if not empty
                            if !text_acc.is_empty() {
                                content_parts.push(serde_json::json!({
                                    "type": "text",
                                    "text": text_acc,
                                }));
                                text_acc.clear();
                            }
                            // Add image content
                            content_parts.push(serde_json::json!({
                                "type": "image_url",
                                "image_url": {
                                    "url": format!("data:{};base64,{}", source.media_type, source.data),
                                }
                            }));
                        }
                        ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            ..
                        } => {
                            // Flush any accumulated content first
                            if !text_acc.is_empty() || !content_parts.is_empty() {
                                if !text_acc.is_empty() {
                                    content_parts.push(serde_json::json!({
                                        "type": "text",
                                        "text": text_acc,
                                    }));
                                    text_acc.clear();
                                }
                                api_msgs.push(serde_json::json!({
                                    "role": "user",
                                    "content": content_parts,
                                }));
                                content_parts.clear();
                            }
                            api_msgs.push(serde_json::json!({
                                "role": "tool",
                                "tool_call_id": tool_use_id,
                                "content": content,
                            }));
                        }
                        ContentBlock::Thinking { .. } => {}
                        ContentBlock::ToolUse { .. } => {
                            bail!("OpenAI adapter: unexpected ToolUse block in user message");
                        }
                    }
                }
                // Flush any remaining content
                if !text_acc.is_empty() || !content_parts.is_empty() {
                    if !text_acc.is_empty() {
                        content_parts.push(serde_json::json!({
                            "type": "text",
                            "text": text_acc,
                        }));
                    }
                    if content_parts.len() == 1 {
                        // Single text part - use simple string format
                        if let Some(Value::String(text)) = content_parts.first() {
                            api_msgs.push(serde_json::json!({
                                "role": "user",
                                "content": text,
                            }));
                        } else {
                            api_msgs.push(serde_json::json!({
                                "role": "user",
                                "content": content_parts,
                            }));
                        }
                    } else {
                        api_msgs.push(serde_json::json!({
                            "role": "user",
                            "content": content_parts,
                        }));
                    }
                }
            }
            "assistant" => {
                let mut text_buf = String::new();
                let mut reasoning_buf = String::new();
                let mut tool_calls: Vec<Value> = Vec::new();
                for b in &msg.message.content {
                    match b {
                        ContentBlock::Text { text } => text_buf.push_str(text),
                        ContentBlock::Thinking { thinking } => {
                            // DeepSeek (and some OpenAI-compatible "thinking" models) require
                            // `reasoning_content` to be echoed on the next request.
                            reasoning_buf.push_str(thinking);
                        }
                        ContentBlock::ToolUse { id, name, input } => {
                            let args =
                                serde_json::to_string(input).unwrap_or_else(|_| "{}".to_string());
                            tool_calls.push(serde_json::json!({
                                "id": id,
                                "type": "function",
                                "function": {
                                    "name": name,
                                    "arguments": args,
                                }
                            }));
                        }
                        ContentBlock::ToolResult { .. } => {}
                        ContentBlock::Image { .. } => {
                            // Images in assistant messages are not standard, but handle gracefully
                            tracing::warn!("OpenAI adapter: unexpected Image block in assistant message");
                        }
                    }
                }

                let mut obj = serde_json::Map::new();
                obj.insert("role".into(), serde_json::json!("assistant"));
                if text_buf.is_empty() {
                    obj.insert("content".into(), serde_json::Value::Null);
                } else {
                    obj.insert("content".into(), serde_json::json!(text_buf));
                }
                if !reasoning_buf.is_empty() {
                    obj.insert("reasoning_content".into(), serde_json::json!(reasoning_buf));
                }
                if !tool_calls.is_empty() {
                    obj.insert("tool_calls".into(), serde_json::json!(tool_calls));
                }
                api_msgs.push(Value::Object(obj));
            }
            other => {
                bail!("OpenAI adapter: unsupported message role {other}");
            }
        }
    }

    Ok(api_msgs)
}

async fn query_openai(
    client: &Client,
    messages: &[Message],
    system_prompt: &str,
    tools: &[Arc<dyn Tool>],
    cancel: &CancellationToken,
    profile: &ModelProfile,
    _thinking: bool,
    stream: bool,
) -> Result<Message> {
    let url = format!(
        "{}/chat/completions",
        profile.base_url.trim_end_matches('/')
    );

    let api_messages = openai_messages_for_api(messages, system_prompt)?;
    let openai_tools = if tools.is_empty() {
        None
    } else {
        Some(build_openai_tools(tools))
    };

    let mut body = serde_json::json!({
        "model": profile.model_name,
        "messages": api_messages,
        "max_tokens": profile.max_tokens,
        "stream": stream,
    });

    if let Some(ref t) = openai_tools {
        body["tools"] = serde_json::Value::Array(t.clone());
    }

    debug!("[openai] POST {url}");

    let request = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", profile.api_key))
        .timeout(REQUEST_TIMEOUT)
        .json(&body);

    // Check for cancellation before sending
    if cancel.is_cancelled() {
        bail!("Request cancelled before send");
    }

    let response = request.send().await.context("OpenAI request failed")?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        bail!("OpenAI API error ({status}): {body}");
    }

    if stream {
        parse_openai_stream(response, cancel).await
    } else {
        let json: Value = response.json().await.context("OpenAI JSON parse")?;
        parse_openai_non_stream(&json)
    }
}

async fn parse_openai_stream(
    response: reqwest::Response,
    cancel: &CancellationToken,
) -> Result<Message> {
    let mut stream = response.bytes_stream();
    let mut text_buf = String::new();
    let mut reasoning_buf = String::new();
    let mut tool_calls: Vec<Value> = Vec::new();
    let mut model_name = String::new();

    while let Some(chunk_result) = stream.next().await {
        if cancel.is_cancelled() {
            bail!("Stream cancelled");
        }

        let chunk = chunk_result.context("OpenAI stream chunk error")?;
        let chunk_str = String::from_utf8_lossy(&chunk);

        for line in chunk_str.lines() {
            let line = line.trim();
            if line.is_empty() || line == "data: [DONE]" {
                continue;
            }
            if !line.starts_with("data: ") {
                continue;
            }
            let json_str = &line[6..];
            let delta: Value = match serde_json::from_str(json_str) {
                Ok(v) => v,
                Err(_) => continue,
            };

            if model_name.is_empty() {
                model_name = delta["model"].as_str().unwrap_or("").to_string();
            }

            if let Some(choices) = delta["choices"].as_array() {
                for choice in choices {
                    if let Some(delta) = choice.get("delta") {
                        if let Some(content) = delta["content"].as_str() {
                            text_buf.push_str(content);
                        }
                        if let Some(reasoning) = delta["reasoning_content"].as_str() {
                            reasoning_buf.push_str(reasoning);
                        }
                        if let Some(tc_deltas) = delta["tool_calls"].as_array() {
                            for tc in tc_deltas {
                                let idx = tc["index"].as_u64().unwrap_or(0) as usize;
                                while tool_calls.len() <= idx {
                                    tool_calls.push(serde_json::json!({
                                        "id": "",
                                        "function": {"name": "", "arguments": ""}
                                    }));
                                }
                                if let Some(id) = tc["id"].as_str() {
                                    tool_calls[idx]["id"] = Value::String(id.to_string());
                                }
                                if let Some(func) = tc.get("function") {
                                    if let Some(name) = func["name"].as_str() {
                                        tool_calls[idx]["function"]["name"] =
                                            Value::String(format!(
                                                "{}{}",
                                                tool_calls[idx]["function"]["name"]
                                                    .as_str()
                                                    .unwrap_or(""),
                                                name
                                            ));
                                    }
                                    if let Some(args) = func["arguments"].as_str() {
                                        tool_calls[idx]["function"]["arguments"] =
                                            Value::String(format!(
                                                "{}{}",
                                                tool_calls[idx]["function"]["arguments"]
                                                    .as_str()
                                                    .unwrap_or(""),
                                                args
                                            ));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    build_assistant_message(&text_buf, &reasoning_buf, &tool_calls)
}

fn parse_openai_non_stream(json: &Value) -> Result<Message> {
    let choice = &json["choices"][0];
    let msg = &choice["message"];

    let text = msg["content"].as_str().unwrap_or("").to_string();
    let reasoning = msg
        .get("reasoning_content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let mut tool_calls = Vec::new();
    if let Some(tc_list) = msg["tool_calls"].as_array() {
        for tc in tc_list {
            tool_calls.push(tc.clone());
        }
    }

    build_assistant_message(&text, &reasoning, &tool_calls)
}

// ============================================================================
// Anthropic adapter
// ============================================================================

fn anthropic_tools_for_api(tools: &[Arc<dyn Tool>]) -> Vec<Value> {
    tools
        .iter()
        .map(|t| {
            let schema = t.input_schema();
            serde_json::json!({
                "name": t.name(),
                "description": t.description(),
                "input_schema": schema,
            })
        })
        .collect()
}

fn anthropic_messages_for_api(messages: &[Message]) -> Vec<Value> {
    let mut api_msgs: Vec<Value> = Vec::new();
    for msg in messages {
        let role = &msg.message.role;
        let content = anthropic_content_blocks(&msg.message.content);
        api_msgs.push(serde_json::json!({
            "role": role,
            "content": content,
        }));
    }
    api_msgs
}

fn anthropic_content_blocks(blocks: &[ContentBlock]) -> Value {
    let mut parts: Vec<Value> = Vec::new();
    for b in blocks {
        match b {
            ContentBlock::Text { text } => {
                parts.push(serde_json::json!({"type": "text", "text": text}));
            }
            ContentBlock::ToolUse { id, name, input } => {
                parts.push(serde_json::json!({
                    "type": "tool_use",
                    "id": id,
                    "name": name,
                    "input": input,
                }));
            }
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                parts.push(serde_json::json!({
                    "type": "tool_result",
                    "tool_use_id": tool_use_id,
                    "content": content,
                    "is_error": is_error,
                }));
            }
            ContentBlock::Thinking { thinking } => {
                parts.push(serde_json::json!({
                    "type": "thinking",
                    "thinking": thinking,
                }));
            }
            ContentBlock::Image { source } => {
                parts.push(serde_json::json!({
                    "type": "image",
                    "source": {
                        "type": source.source_type,
                        "media_type": source.media_type,
                        "data": source.data,
                    }
                }));
            }
        }
    }
    Value::Array(parts)
}

async fn query_anthropic(
    client: &Client,
    messages: &[Message],
    system_prompt: &str,
    tools: &[Arc<dyn Tool>],
    cancel: &CancellationToken,
    profile: &ModelProfile,
    thinking: bool,
    stream: bool,
) -> Result<Message> {
    let url = format!("{}/v1/messages", profile.base_url.trim_end_matches('/'));

    let api_messages = anthropic_messages_for_api(messages);
    let anthropic_tools = if tools.is_empty() {
        None
    } else {
        Some(anthropic_tools_for_api(tools))
    };

    let mut body = serde_json::json!({
        "model": profile.model_name,
        "max_tokens": profile.max_tokens,
        "messages": api_messages,
        "stream": stream,
    });

    if !system_prompt.is_empty() {
        body["system"] = Value::String(system_prompt.to_string());
    }

    if let Some(ref t) = anthropic_tools {
        body["tools"] = serde_json::Value::Array(t.clone());
    }

    if thinking {
        body["thinking"] = serde_json::json!({
            "type": "enabled",
            "budget_tokens": 4096,
        });
    }

    debug!("[anthropic] POST {url}");

    if cancel.is_cancelled() {
        bail!("Request cancelled before send");
    }

    let request = client
        .post(&url)
        .header("x-api-key", &profile.api_key)
        .header("anthropic-version", "2023-06-01")
        .timeout(REQUEST_TIMEOUT)
        .json(&body);

    let response = request.send().await.context("Anthropic request failed")?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        bail!("Anthropic API error ({status}): {body}");
    }

    if stream {
        parse_anthropic_stream(response, cancel).await
    } else {
        let json: Value = response.json().await.context("Anthropic JSON parse")?;
        parse_anthropic_non_stream(&json)
    }
}

async fn parse_anthropic_stream(
    response: reqwest::Response,
    cancel: &CancellationToken,
) -> Result<Message> {
    let mut stream = response.bytes_stream();
    let mut text_buf = String::new();
    let mut reasoning_buf = String::new();
    let mut tool_use_blocks: Vec<Value> = Vec::new();
    let mut current_tool_idx: Option<usize> = None;

    while let Some(chunk_result) = stream.next().await {
        if cancel.is_cancelled() {
            bail!("Stream cancelled");
        }

        let chunk = chunk_result.context("Anthropic stream chunk error")?;
        let chunk_str = String::from_utf8_lossy(&chunk);

        for line in chunk_str.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if !line.starts_with("data: ") {
                continue;
            }
            let json_str = &line[6..];
            let event: Value = match serde_json::from_str(json_str) {
                Ok(v) => v,
                Err(_) => continue,
            };

            let event_type = event["type"].as_str().unwrap_or("");

            match event_type {
                "content_block_start" => {
                    if let Some(cb) = event.get("content_block") {
                        match cb["type"].as_str().unwrap_or("") {
                            "tool_use" => {
                                let idx = cb["index"].as_u64().unwrap_or(0) as usize;
                                current_tool_idx = Some(idx);
                                while tool_use_blocks.len() <= idx {
                                    tool_use_blocks.push(serde_json::json!({
                                        "id": "",
                                        "name": "",
                                        "input": {},
                                    }));
                                }
                                tool_use_blocks[idx]["id"] = cb["id"].clone();
                                tool_use_blocks[idx]["name"] = cb["name"].clone();
                            }
                            _ => {}
                        }
                    }
                }
                "content_block_delta" => {
                    if let Some(delta) = event.get("delta") {
                        match delta["type"].as_str().unwrap_or("") {
                            "text_delta" => {
                                if let Some(t) = delta["text"].as_str() {
                                    text_buf.push_str(t);
                                }
                            }
                            "thinking_delta" => {
                                if let Some(t) = delta["thinking"].as_str() {
                                    reasoning_buf.push_str(t);
                                }
                            }
                            "input_json_delta" => {
                                if let Some(json_str) = delta["partial_json"].as_str() {
                                    if let Some(idx) = current_tool_idx {
                                        if idx < tool_use_blocks.len() {
                                            let current = tool_use_blocks[idx]["input"]
                                                .as_str()
                                                .unwrap_or("");
                                            let merged = format!("{current}{json_str}");
                                            // Store as string during accumulation, parse at end
                                            tool_use_blocks[idx]["_input_json"] =
                                                Value::String(merged);
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                "message_delta" => {
                    // Stop reason, usage, etc.
                }
                _ => {}
            }
        }
    }

    // Convert accumulated JSON strings to parsed objects
    for block in &mut tool_use_blocks {
        if let Some(json_str) = block.get("_input_json").and_then(|v| v.as_str()) {
            block["input"] =
                serde_json::from_str(json_str).unwrap_or(Value::Object(Default::default()));
        }
    }

    build_assistant_message_anthropic(&text_buf, &reasoning_buf, &tool_use_blocks)
}

fn parse_anthropic_non_stream(json: &Value) -> Result<Message> {
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut tool_use_blocks: Vec<Value> = Vec::new();

    if let Some(content) = json["content"].as_array() {
        for block in content {
            match block["type"].as_str().unwrap_or("") {
                "text" => {
                    if let Some(t) = block["text"].as_str() {
                        text.push_str(t);
                    }
                }
                "thinking" => {
                    if let Some(t) = block["thinking"].as_str() {
                        reasoning.push_str(t);
                    }
                }
                "tool_use" => {
                    tool_use_blocks.push(block.clone());
                }
                _ => {}
            }
        }
    }

    build_assistant_message_anthropic(&text, &reasoning, &tool_use_blocks)
}

// ============================================================================
// Message construction helpers
// ============================================================================

fn build_assistant_message(text: &str, reasoning: &str, tool_calls: &[Value]) -> Result<Message> {
    let mut content: Vec<ContentBlock> = Vec::new();

    if !reasoning.is_empty() {
        content.push(ContentBlock::Thinking {
            thinking: reasoning.to_string(),
        });
    }

    if !text.is_empty() {
        content.push(ContentBlock::Text {
            text: text.to_string(),
        });
    }

    for tc in tool_calls {
        let id = tc["id"].as_str().unwrap_or("").to_string();
        let name = if let Some(n) = tc["function"]["name"].as_str() {
            n.to_string()
        } else {
            tc["name"].as_str().unwrap_or("").to_string()
        };
        let input = if let Some(args) = tc["function"]["arguments"].as_str() {
            serde_json::from_str(args).unwrap_or(Value::Object(Default::default()))
        } else {
            tc["input"].clone()
        };

        content.push(ContentBlock::ToolUse { id, name, input });
    }

    Ok(Message {
        msg_type: "assistant".to_string(),
        message: MessagePayload {
            role: "assistant".to_string(),
            content,
        },
        uuid: uuid::Uuid::new_v4().to_string(),
    })
}

fn build_assistant_message_anthropic(
    text: &str,
    reasoning: &str,
    tool_uses: &[Value],
) -> Result<Message> {
    let mut content: Vec<ContentBlock> = Vec::new();

    if !reasoning.is_empty() {
        content.push(ContentBlock::Thinking {
            thinking: reasoning.to_string(),
        });
    }

    if !text.is_empty() {
        content.push(ContentBlock::Text {
            text: text.to_string(),
        });
    }

    for tu in tool_uses {
        let id = tu["id"].as_str().unwrap_or("").to_string();
        let name = tu["name"].as_str().unwrap_or("").to_string();
        let input = tu["input"].clone();
        content.push(ContentBlock::ToolUse { id, name, input });
    }

    Ok(Message {
        msg_type: "assistant".to_string(),
        message: MessagePayload {
            role: "assistant".to_string(),
            content,
        },
        uuid: uuid::Uuid::new_v4().to_string(),
    })
}

// ============================================================================
// Error classification (mirrors TS emitSessionError)
// ============================================================================

/// Classified error from an LLM call.
#[derive(Debug, Clone)]
pub struct LlmError {
    pub code: String,
    pub message: String,
    pub error_type: String,
    pub is_context_length: bool,
}

impl LlmError {
    pub fn classify(err: &anyhow::Error) -> Self {
        let msg = err.to_string();
        let msg_lower = msg.to_lowercase();

        // Check for cancellation first — not an error to report
        if msg_lower.contains("cancelled") || msg_lower.contains("aborted") {
            return Self {
                code: "CANCELLED".into(),
                message: msg.clone(),
                error_type: "cancelled".into(),
                is_context_length: false,
            };
        }

        // OpenAI context length error
        if msg_lower.contains("context_length_exceeded")
            || msg_lower.contains("maximum context length")
            || msg_lower.contains("reduce the length")
        {
            return Self {
                code: "CONTEXT_TOO_LONG".into(),
                message: "Context length exceeded".into(),
                error_type: "context_length_exceeded".into(),
                is_context_length: true,
            };
        }

        // HTTP status codes
        if let Some(code) = extract_http_status(&msg) {
            let error_code = format!("API_ERROR_{code}");
            return Self {
                code: error_code,
                message: msg.clone(),
                error_type: "api_error".into(),
                is_context_length: false,
            };
        }

        // Auth
        if msg_lower.contains("401")
            || msg_lower.contains("auth")
            || msg_lower.contains("unauthorized")
        {
            return Self {
                code: "AUTH_ERROR".into(),
                message: "API authentication failed — check API key".into(),
                error_type: "api_error".into(),
                is_context_length: false,
            };
        }

        // Rate limit
        if msg_lower.contains("429") || msg_lower.contains("rate limit") {
            return Self {
                code: "RATE_LIMIT".into(),
                message: "API rate limit exceeded — retry later".into(),
                error_type: "api_error".into(),
                is_context_length: false,
            };
        }

        // Network
        if msg_lower.contains("fetch")
            || msg_lower.contains("network")
            || msg_lower.contains("connection")
            || msg_lower.contains("timeout")
        {
            return Self {
                code: "NETWORK_ERROR".into(),
                message: "Network error — check connectivity".into(),
                error_type: "api_error".into(),
                is_context_length: false,
            };
        }

        // JSON parse
        if msg_lower.contains("json") || msg_lower.contains("parse") || msg_lower.contains("serde")
        {
            return Self {
                code: "API_RESPONSE_ERROR".into(),
                message: format!("API response parse error: {msg}"),
                error_type: "api_error".into(),
                is_context_length: false,
            };
        }

        // Default
        Self {
            code: "UNKNOWN_ERROR".into(),
            message: msg.clone(),
            error_type: "api_error".into(),
            is_context_length: false,
        }
    }

    /// Whether this error should be surfaced as `session:error`.
    pub fn should_emit(&self) -> bool {
        self.error_type != "cancelled"
    }

    /// Convert to SessionErrorData for emission.
    pub fn to_session_error(&self) -> SessionErrorData {
        SessionErrorData {
            error_type: self.error_type.clone(),
            error: SessionErrorDetail {
                code: self.code.clone(),
                message: self.message.clone(),
                details: None,
            },
        }
    }
}

fn extract_http_status(msg: &str) -> Option<u16> {
    // Match patterns like "API error (429)" or "status: 500"
    if let Some(start) = msg.find('(') {
        let rest = &msg[start + 1..];
        if let Some(end) = rest.find(')') {
            if let Ok(code) = rest[..end].parse::<u16>() {
                return Some(code);
            }
        }
    }
    None
}

// ============================================================================
// Re-export helper for creating a configured reqwest client
// ============================================================================

pub fn create_llm_client() -> Result<Client> {
    Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .context("Failed to create HTTP client")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_adapter_detects_anthropic() {
        assert_eq!(resolve_adapter("anthropic"), "anthropic");
        assert_eq!(resolve_adapter("claude"), "anthropic");
        assert_eq!(resolve_adapter("Anthropic"), "anthropic");
    }

    #[test]
    fn resolve_adapter_defaults_to_openai() {
        assert_eq!(resolve_adapter("openai"), "openai");
        assert_eq!(resolve_adapter("openrouter"), "openai");
        assert_eq!(resolve_adapter("unknown"), "openai");
    }

    #[test]
    fn error_classify_cancelled() {
        let err = anyhow::anyhow!("Request cancelled");
        let classified = LlmError::classify(&err);
        assert!(!classified.should_emit());
    }

    #[test]
    fn error_classify_context_length() {
        let err = anyhow::anyhow!("context_length_exceeded: maximum context length");
        let classified = LlmError::classify(&err);
        assert!(classified.is_context_length);
        assert_eq!(classified.code, "CONTEXT_TOO_LONG");
    }

    #[test]
    fn error_classify_auth() {
        let err = anyhow::anyhow!("HTTP 401 Unauthorized");
        let classified = LlmError::classify(&err);
        assert_eq!(classified.code, "AUTH_ERROR");
    }

    #[test]
    fn error_classify_rate_limit() {
        let err = anyhow::anyhow!("429 rate limit exceeded");
        let classified = LlmError::classify(&err);
        assert_eq!(classified.code, "RATE_LIMIT");
    }

    #[test]
    fn extract_http_status_finds_code() {
        assert_eq!(extract_http_status("API error (429)"), Some(429));
        assert_eq!(extract_http_status("error (500) internal"), Some(500));
        assert_eq!(extract_http_status("no status here"), None);
    }

    #[test]
    #[cfg(feature = "local-mlx")]
    fn split_qwen_tool_calls_strips_tags_and_builds_openai_shapes() {
        let raw = "OK.\n<tool_call>\n{\"name\": \"weather\", \"arguments\": {\"city\": \"HN\"}}\n</tool_call>\n";
        let (text, tc) = split_qwen_tool_calls_from_model_text(raw);
        assert_eq!(text.trim(), "OK.");
        assert_eq!(tc.len(), 1);
        assert_eq!(tc[0]["function"]["name"], "weather");
        assert!(tc[0]["function"]["arguments"].as_str().unwrap().contains("HN"));
    }

    #[test]
    fn openai_messages_expand_tool_use_and_tool_results() {
        let msgs = vec![
            create_user_message(vec![ContentBlock::Text {
                text: "read project".into(),
            }]),
            Message {
                msg_type: "assistant".into(),
                message: MessagePayload {
                    role: "assistant".into(),
                    content: vec![ContentBlock::ToolUse {
                        id: "call_1".into(),
                        name: "Read".into(),
                        input: serde_json::json!({"path": "/tmp/x"}),
                    }],
                },
                uuid: "a1".into(),
            },
            create_user_message(vec![ContentBlock::ToolResult {
                tool_use_id: "call_1".into(),
                content: "file contents".into(),
                is_error: false,
            }]),
        ];
        let out = openai_messages_for_api(&msgs, "You are helpful.").unwrap();
        assert_eq!(out.len(), 4);
        assert_eq!(out[0]["role"], "system");
        assert_eq!(out[1]["role"], "user");
        assert_eq!(out[1]["content"], "read project");
        assert_eq!(out[2]["role"], "assistant");
        assert!(out[2]["content"].is_null());
        assert!(out[2]["tool_calls"].is_array());
        assert_eq!(out[2]["tool_calls"][0]["id"], "call_1");
        assert_eq!(out[3]["role"], "tool");
        assert_eq!(out[3]["tool_call_id"], "call_1");
        assert_eq!(out[3]["content"], "file contents");
    }

    #[test]
    fn openai_messages_include_reasoning_content_for_thinking() {
        let msgs = vec![Message {
            msg_type: "assistant".into(),
            message: MessagePayload {
                role: "assistant".into(),
                content: vec![
                    ContentBlock::Thinking {
                        thinking: "step by step...".into(),
                    },
                    ContentBlock::Text {
                        text: "Hello".into(),
                    },
                ],
            },
            uuid: "a1".into(),
        }];
        let out = openai_messages_for_api(&msgs, "").unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["role"], "assistant");
        assert_eq!(out[0]["content"], "Hello");
        assert_eq!(out[0]["reasoning_content"], "step by step...");
    }
}
