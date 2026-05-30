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
/// For `provider = local-candle`, the in-process path always wins so a stale
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

    // Persist the full request (system prompt + message history + tool list)
    // to `~/.senclaw/llm_logs/` so prompts can be analyzed/optimized after the
    // fact — e.g. diagnosing an agent stuck re-invoking a skill in a loop.
    let tool_names: Vec<(String, String)> = tools
        .iter()
        .map(|t| (t.name().to_string(), t.description().to_string()))
        .collect();
    crate::util::llm_log::log_request(
        &profile.model_name,
        system_prompt,
        messages,
        &tool_names,
        thinking,
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
        "local-candle-native" => {
            query_local_candle_native(messages, system_prompt, tools, cancel, profile).await
        }
        "local-mlx" => {
            query_local_mlx(client, messages, system_prompt, tools, cancel, profile, stream).await
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
            crate::util::llm_log::log_response(msg);
            let blocks = msg.message.content.len();
            let tool_calls = msg
                .message
                .content
                .iter()
                .filter(|b| matches!(b, ContentBlock::ToolUse { .. }))
                .count();
            if blocks == 0 && tool_calls == 0 {
                // Silent upstream failure — adapter parsed 200 OK but no
                // content came through. Log a loud WARN so it's findable in
                // production logs; `conversation.rs` will catch this and
                // surface a SessionError to the UI.
                tracing::warn!(
                    "[llm] EMPTY response provider={} model={} adapter={} \
                     blocks=0 tool_calls=0 — endpoint returned 200 OK with no content. \
                     Check endpoint logs (auth / rate-limit / tool count overload).",
                    profile.provider, profile.model_name, adapt
                );
            } else {
                info!(
                    "[llm] request complete provider={} model={} blocks={} tool_calls={}",
                    profile.provider, profile.model_name, blocks, tool_calls
                );
            }
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
    } else if is_local_candle_provider(&lower) {
        "local-candle-native"
    } else if is_local_mlx_provider(&lower) {
        "local-mlx"
    } else {
        "openai"
    }
}

fn is_local_candle_provider(lower: &str) -> bool {
    matches!(
        lower,
        "local-candle" | "local-candle-native" | "local-candle-accelerate"
    )
}

fn is_local_mlx_provider(lower: &str) -> bool {
    matches!(lower, "local-mlx" | "local-mlx-native" | "local-mlx-server")
}

/// Prefer routing implied by `provider`; otherwise use explicit `adapt`.
fn effective_adapter(profile: &ModelProfile) -> &str {
    let p = profile.provider.to_lowercase();
    if is_local_candle_provider(&p) {
        return "local-candle-native";
    }
    if is_local_mlx_provider(&p) {
        return "local-mlx";
    }
    profile
        .adapt
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| {
            let lower = s.to_lowercase();
            if is_local_candle_provider(&lower) {
                "local-candle-native"
            } else if is_local_mlx_provider(&lower) {
                "local-mlx"
            } else {
                s
            }
        })
        .unwrap_or_else(|| resolve_adapter(&profile.provider))
}

// ============================================================================
// Local Candle native adapter (in-process, CPU / Metal)
// ============================================================================

/// Run inference through the local Candle engine.
///
/// Uses the same OpenAI-shaped `messages` / `tools` for chat-template rendering;
/// parses Qwen-style `<tool_call>…</tool_call>` from the generated text.
#[allow(unused_variables)]
async fn query_local_candle_native(
    messages: &[Message],
    system_prompt: &str,
    tools: &[Arc<dyn Tool>],
    cancel: &CancellationToken,
    profile: &ModelProfile,
) -> Result<Message> {
    #[cfg(not(feature = "local-candle"))]
    {
        bail!(
            "local-candle-native adapter requires the `local-candle` cargo feature; \
             rebuild with `cargo build --features local-candle` \
             (or `local-candle-metal` for Apple Silicon Metal acceleration)"
        );
    }

    #[cfg(feature = "local-candle")]
    {
        use crate::config::Config;
        use crate::local_model::LocalModelRuntime;

        let api_msgs = openai_messages_for_api(messages, system_prompt)?;
        let tool_objs = build_hf_style_tools(tools);

        let cfg = Config::from_env();
        let model_key =
            crate::gateway::ui_server::local_models::canonical_local_model_id(&profile.model_name);
        let safe = model_key.replace('/', "__");
        let model_dir = cfg.paths.local_models_dir.join(safe);

        // Global registry: one CandleEngine per model_id, weights cached in memory.
        let engine = crate::gateway::ui_server::local_models::get_or_create_loaded_engine(
            &model_key,
            &model_dir,
        );
        let _idle_gen = crate::gateway::ui_server::local_models::CandleInferenceGuard::new(&model_key);
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
                    None => break,
                },
                _ = cancel.cancelled() => {
                    gen_handle.abort();
                    bail!("local-candle-native: cancelled");
                }
            }
        }
        gen_handle
            .await
            .context("local-candle-native: generation task panicked")??;
        debug!("[local-candle-native] generated {} chars", text_buf.len());

        let (reasoning, visible) =
            crate::local_model::thinking_parse::split_thinking_blocks(&text_buf);
        let (clean_text, tool_calls_from_text) =
            split_qwen_tool_calls_from_model_text(&visible);
        build_assistant_message(&clean_text, &reasoning, &tool_calls_from_text, None)
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

/// Build HF-style tools (direct function objects, no OpenAI wrapper)
/// for models like Qwen that use Jinja templates expecting this format.
pub(crate) fn build_hf_style_tools(tools: &[Arc<dyn Tool>]) -> Vec<Value> {
    tools
        .iter()
        .map(|t| {
            let schema = t.input_schema();
            serde_json::json!({
                "name": t.name(),
                "description": t.description(),
                "parameters": schema,
            })
        })
        .collect()
}

/// Strip Qwen / HF-style `<tool_call>…</tool_call>` segments and turn them into OpenAI-style
/// `tool_calls` objects for [`build_assistant_message`].
/// Parse one Qwen3.5 / Hermes-style XML function call body into the same
/// `{"name", "arguments"}` shape the JSON path produces. Body looks like:
/// `\n<function=NAME>\n<parameter=P1>\nval1\n</parameter>\n…\n</function>\n`.
/// Parameter values are type-coerced (JSON number/bool/object/array, else string).
fn parse_xml_tool_call(body: &str) -> Option<Value> {
    const FN_OPEN: &str = "<function=";
    const PARAM_OPEN: &str = "<parameter=";
    const PARAM_CLOSE: &str = "</parameter>";

    let fstart = body.find(FN_OPEN)? + FN_OPEN.len();
    let fname_rel = body[fstart..].find('>')?;
    let name = body[fstart..fstart + fname_rel].trim();
    if name.is_empty() {
        return None;
    }

    let mut args = serde_json::Map::new();
    let mut rest = &body[fstart + fname_rel + 1..];
    while let Some(ps) = rest.find(PARAM_OPEN) {
        let after = &rest[ps + PARAM_OPEN.len()..];
        let Some(name_rel) = after.find('>') else { break };
        let pname = after[..name_rel].trim().to_string();
        let val_region = &after[name_rel + 1..];
        let Some(close) = val_region.find(PARAM_CLOSE) else { break };
        let raw = val_region[..close].trim();
        let val = serde_json::from_str::<Value>(raw)
            .unwrap_or_else(|_| Value::String(raw.to_string()));
        if !pname.is_empty() {
            args.insert(pname, val);
        }
        rest = &val_region[close + PARAM_CLOSE.len()..];
    }

    Some(serde_json::json!({ "name": name, "arguments": Value::Object(args) }))
}

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
            // Model emitted `<tool_call>` but never closed it before EOS /
            // `<|im_end|>`. Surface this loudly — the user will otherwise see
            // a raw `<tool_call> {"name": ...` blob in the UI with no
            // explanation. Common with greedy decoding + low-precision
            // (4-bit) quantization + long tools-heavy prompts: the model
            // emits `<|im_end|>` token mid-arguments.
            let partial: String = after.chars().take(160).collect();
            tracing::warn!(
                "[llm] truncated tool_call: `<tool_call>` opened but no `</tool_call>` close \
                 before end of stream. Partial body (first 160 chars): {partial:?}. \
                 Common fix: set `enable_thinking: false` in local model settings, or use a \
                 less-quantized model — Qwen3 4-bit + thinking-on is prone to mid-args EOS."
            );
            out.push_str(&rest[start..]);
            return (out, tool_calls);
        };
        let body = after[..end_rel].trim();
        rest = &after[end_rel + CLOSE.len()..];

        // Two wire formats share the `<tool_call>…</tool_call>` envelope:
        //  - Qwen3:   JSON  `{"name": ..., "arguments": {...}}`
        //  - Qwen3.5: XML   `<function=NAME><parameter=P>val</parameter>…</function>`
        let items: Vec<Value> = if body.contains("<function=") {
            match parse_xml_tool_call(body) {
                Some(v) => vec![v],
                None => {
                    tracing::warn!("[llm] tool_call XML parse failed (skipping). Body: {body:?}");
                    continue;
                }
            }
        } else {
            let Ok(v) = serde_json::from_str::<Value>(body) else {
                tracing::warn!("[llm] tool_call JSON parse failed (skipping). Body: {body:?}");
                continue;
            };
            match v {
                Value::Array(a) => a,
                one => vec![one],
            }
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

// ============================================================================
// Local MLX adapter — auto-starts mlx_lm.server, routes via OpenAI HTTP
// ============================================================================

/// Run inference through the local MLX engine (mlx_lm.server sidecar).
///
/// In-process MLX native inference via mlx-rs.
///
/// Performance vs Candle on M4 Pro (Qwen3-0.6B):
/// - MLX native: ~60–100 tok/s decode (BF16 GEMV kernels, full GPU memory bandwidth)
/// - Candle Accelerate: ~12 tok/s (F32 BLAS on CPU)
/// - Candle Metal: ~7 tok/s (BM=32 GEMM tile, 3% GPU occupancy at M=1)
#[allow(unused_variables)]
async fn query_local_mlx(
    _client: &Client,
    messages: &[Message],
    system_prompt: &str,
    tools: &[Arc<dyn Tool>],
    cancel: &CancellationToken,
    profile: &ModelProfile,
    _stream: bool,
) -> Result<Message> {
    #[cfg(not(feature = "local-mlx"))]
    {
        bail!(
            "local-mlx adapter requires the `local-mlx` cargo feature; \
             rebuild with `cargo build --features local-mlx` (Apple Silicon only)"
        );
    }

    #[cfg(feature = "local-mlx")]
    {
        use crate::config::Config;
        use crate::local_model::LocalModelRuntime;

        let api_msgs = openai_messages_for_api(messages, system_prompt)?;
        let tool_objs = build_hf_style_tools(tools);

        let cfg = Config::from_env();
        let model_key =
            crate::gateway::ui_server::local_models::canonical_local_model_id(&profile.model_name);
        let safe = model_key.replace('/', "__");
        let model_dir = cfg.paths.local_models_dir.join(safe);

        // Global registry: one MlxNativeEngine per model_id, weights cached in memory.
        let engine = crate::gateway::ui_server::local_models::get_or_create_mlx_engine(
            &model_key,
            &model_dir,
        );
        let _idle_gen =
            crate::gateway::ui_server::local_models::MlxInferenceGuard::new(&model_key);

        // warm_up() loads weights if not yet loaded.
        let engine_wu = engine.clone();
        tokio::task::spawn_blocking(move || engine_wu.warm_up())
            .await
            .context("mlx warm_up task panicked")??;

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
                    None => break,
                },
                _ = cancel.cancelled() => {
                    gen_handle.abort();
                    bail!("local-mlx: cancelled");
                }
            }
        }
        gen_handle
            .await
            .context("local-mlx: generation task panicked")??;
        debug!("[local-mlx] generated {} chars", text_buf.len());

        let (reasoning, visible) =
            crate::local_model::thinking_parse::split_thinking_blocks(&text_buf);
        let (clean_text, tool_calls_from_text) =
            split_qwen_tool_calls_from_model_text(&visible);
        build_assistant_message(&clean_text, &reasoning, &tool_calls_from_text, None)
    }
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

    // Ask the server to emit a final usage chunk so we can capture real token
    // counts from streamed responses (no-op for providers that ignore it).
    if stream {
        body["stream_options"] = serde_json::json!({ "include_usage": true });
    }

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
    let mut usage: Option<RawUsage> = None;

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

            // With `stream_options.include_usage`, the final chunk carries a
            // top-level `usage` object (and an empty `choices` array).
            if let Some(u) = RawUsage::from_json(&delta["usage"]) {
                usage = Some(u);
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

    build_assistant_message(&text_buf, &reasoning_buf, &tool_calls, usage)
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

    let usage = RawUsage::from_json(&json["usage"]);
    build_assistant_message(&text, &reasoning, &tool_calls, usage)
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
    let mut usage: Option<RawUsage> = None;

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
                "message_start" => {
                    // Carries input/cache token counts (output is ~1 here).
                    if let Some(u) = RawUsage::from_json(&event["message"]["usage"]) {
                        usage.get_or_insert_with(RawUsage::default).merge(&u);
                    }
                }
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
                    // Carries the cumulative output_tokens (and stop_reason).
                    if let Some(u) = RawUsage::from_json(&event["usage"]) {
                        usage.get_or_insert_with(RawUsage::default).merge(&u);
                    }
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

    build_assistant_message_anthropic(&text_buf, &reasoning_buf, &tool_use_blocks, usage)
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

    let usage = RawUsage::from_json(&json["usage"]);
    build_assistant_message_anthropic(&text, &reasoning, &tool_use_blocks, usage)
}

// ============================================================================
// Message construction helpers
// ============================================================================

fn build_assistant_message(
    text: &str,
    reasoning: &str,
    tool_calls: &[Value],
    usage: Option<RawUsage>,
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
        usage,
    })
}

fn build_assistant_message_anthropic(
    text: &str,
    reasoning: &str,
    tool_uses: &[Value],
    usage: Option<RawUsage>,
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
        usage,
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

        // Auth — **only** clear HTTP / API-key signals. Broad `contains("auth")` or bare
        // `401` false-positive on local MLX + tools (paths like `.../authors/...`, "oauth"
        // in JSON schema, tensor sizes mentioning 401, etc.) and surfaces misleading
        // "check API key" even though no remote API is involved.
        if looks_like_http_auth_failure(&msg_lower) {
            return Self {
                code: "AUTH_ERROR".into(),
                message: "API authentication failed — check API key".into(),
                error_type: "api_error".into(),
                is_context_length: false,
            };
        }

        // Rate limit — avoid bare `429` (can appear in unrelated numeric errors).
        if msg_lower.contains("rate limit")
            || msg_lower.contains("too many requests")
            || msg_lower.contains("429 too many")
            || msg_lower.contains("http 429")
            || msg_lower.contains("status 429")
        {
            return Self {
                code: "RATE_LIMIT".into(),
                message: "API rate limit exceeded — retry later".into(),
                error_type: "api_error".into(),
                is_context_length: false,
            };
        }

        // Network — bare `timeout` / `connection` / `fetch` match MCP tool JSON (timeout_ms,
        // "connection state", "Fetch …") when errors embed the full `tools` payload; classify
        // only clear transport / HTTP-client signals.
        if looks_like_network_transport_failure(&msg_lower) {
            return Self {
                code: "NETWORK_ERROR".into(),
                message: "Network error — check connectivity".into(),
                error_type: "api_error".into(),
                is_context_length: false,
            };
        }

        // JSON / body parse — the word `json` appears in every `$schema` URL inside tool defs;
        // avoid treating template / MLX errors as "API response parse" unless it looks like serde/JSON.
        if looks_like_response_parse_failure(&msg_lower) {
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

/// True when `msg_lower` reads like an HTTP/API credential failure (not substring "auth"
/// inside unrelated words such as `authors`, `oauth`, or bare `401` in tensor sizes).
fn looks_like_http_auth_failure(msg_lower: &str) -> bool {
    const PHRASES: &[&str] = &[
        "401 unauthorized",
        "http 401",
        "https 401",
        "status 401",
        "status: 401",
        "status = 401",
        "unauthorized",
        "invalid_api_key",
        "invalid api key",
        "incorrect api key",
        "missing api key",
        "api key missing",
        "api key not found",
        "api key expired",
        "authentication failed",
        "access token invalid",
        "access token expired",
        "no api key",
        "wrong api key",
        "bearer token",
    ];
    PHRASES.iter().any(|p| msg_lower.contains(p))
}

/// True for HTTP client / OS transport failures — not substrings like `timeout_ms` inside MCP schemas.
fn looks_like_network_transport_failure(msg_lower: &str) -> bool {
    const PHRASES: &[&str] = &[
        "operation timed out",
        "request timed out",
        "timed out waiting",
        "deadline has elapsed",
        "connection refused",
        "connection reset",
        "connection aborted",
        "broken pipe",
        "unexpected eof",
        "error sending request",
        "error trying to connect",
        "could not connect",
        "failed to connect",
        "tcp connect",
        "dns error",
        "failed to lookup",
        "name or service not known",
        "getaddrinfo",
        "ssl error",
        "tls handshake",
        "certificate verify",
        "reqwest::",
        "hyper::",
        "http connect",
        "network unreachable",
        "host unreachable",
        "no route to host",
    ];
    PHRASES.iter().any(|p| msg_lower.contains(p))
}

/// True when the failure reads like JSON/body parsing — not `$schema` URLs in embedded tool JSON.
fn looks_like_response_parse_failure(msg_lower: &str) -> bool {
    const PHRASES: &[&str] = &[
        "serde_json::error",
        "serde_json::err",
        "invalid escape",
        "trailing characters",
        "expected value at line",
        "key must be a string",
        "invalid json",
        "failed to parse json",
        "error decoding response body",
        "error decoding response",
        "json parse error",
        "unexpected end of json",
        "expected `,` or `}`",
        "expected `:`",
    ];
    PHRASES.iter().any(|p| msg_lower.contains(p))
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
    fn error_classify_auth_not_triggered_by_authors_path_or_bare_401() {
        let err = anyhow::anyhow!(
            "chat template apply failed: /Users/x/docs/authors/guide.md:12:5 error"
        );
        let c = LlmError::classify(&err);
        assert_ne!(c.code, "AUTH_ERROR", "expected not AUTH_ERROR: {}", c.code);

        let err2 = anyhow::anyhow!("mlx forward failed: shape [32, 401, 128] mismatch");
        let c2 = LlmError::classify(&err2);
        assert_ne!(c2.code, "AUTH_ERROR");
    }

    #[test]
    fn error_classify_network_not_triggered_by_mcp_tool_json_noise() {
        let err = anyhow::anyhow!(
            "render failed: {{\n  \"tools\": [{{\n    \"timeout_ms\": 30000,\n    \"description\": \"connection state\"\n  }}]\n}}"
        );
        let c = LlmError::classify(&err);
        assert_ne!(c.code, "NETWORK_ERROR");

        let err2 = anyhow::anyhow!("https://json-schema.org/draft/2020-12/schema parse error in tool");
        let c2 = LlmError::classify(&err2);
        assert_ne!(c2.code, "API_RESPONSE_ERROR");
    }

    #[test]
    fn error_classify_network_still_detects_connection_refused() {
        let err = anyhow::anyhow!("error sending request: connection refused (os error 61)");
        let c = LlmError::classify(&err);
        assert_eq!(c.code, "NETWORK_ERROR");
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
    fn split_qwen_tool_calls_strips_tags_and_builds_openai_shapes() {
        let raw = "OK.\n<tool_call>\n{\"name\": \"weather\", \"arguments\": {\"city\": \"HN\"}}\n</tool_call>\n";
        let (text, tc) = split_qwen_tool_calls_from_model_text(raw);
        assert_eq!(text.trim(), "OK.");
        assert_eq!(tc.len(), 1);
        assert_eq!(tc[0]["function"]["name"], "weather");
        assert!(tc[0]["function"]["arguments"].as_str().unwrap().contains("HN"));
    }

    // ── Qwen3.5 end-to-end pipeline: think + tool-call ────────────────────
    // These mirror the exact production sequence in `query_local_mlx`:
    //   (reasoning, visible) = split_thinking_blocks(raw)
    //   (clean_text, tool_calls) = split_qwen_tool_calls_from_model_text(visible)
    // using raw buffers in Qwen3.5's wire format (chat_template.jinja prefills
    // `<think>\n`, so generation has a dangling `</think>`; tool calls are XML).

    fn qwen35_pipeline(raw: &str) -> (String, String, Vec<Value>) {
        let (reasoning, visible) =
            crate::local_model::thinking_parse::split_thinking_blocks(raw);
        let (clean, tcs) = split_qwen_tool_calls_from_model_text(&visible);
        (reasoning, clean, tcs)
    }

    #[test]
    fn qwen35_think_then_answer_pipeline() {
        // enable_thinking=true → prefilled open, dangling close (the reported bug).
        let raw = "User said hi, I should ask 1-4 related questions in a single turn.\n</think>\n\nHi! What are your questions?";
        let (reasoning, clean, tcs) = qwen35_pipeline(raw);
        assert_eq!(reasoning, "User said hi, I should ask 1-4 related questions in a single turn.");
        assert_eq!(clean, "Hi! What are your questions?");
        assert!(!clean.contains("</think>"), "stray closing tag leaked: {clean:?}");
        assert!(tcs.is_empty());
    }

    #[test]
    fn qwen35_unclosed_thinking_no_leak() {
        // Common Qwen3.5 case: enable_thinking=true, the engine re-emits the
        // synthetic `<think>\n`, and a long chain-of-thought never closes within
        // the token budget. The reasoning must NOT leak into the visible answer.
        let raw = "<think>\nStep 1: analyze. Step 2: still reasoning, never finished";
        let (reasoning, clean, tcs) = qwen35_pipeline(raw);
        assert!(reasoning.contains("Step 1: analyze"));
        assert_eq!(clean, "", "unclosed reasoning leaked into answer: {clean:?}");
        assert!(tcs.is_empty());
    }

    #[test]
    fn qwen35_think_then_tool_call_pipeline() {
        // Reasoning, then an XML tool call (no plain answer after).
        let raw = "I should look up the weather for the user.\n</think>\n\n<tool_call>\n<function=get_weather>\n<parameter=city>\nHanoi\n</parameter>\n<parameter=days>\n3\n</parameter>\n</function>\n</tool_call>";
        let (reasoning, clean, tcs) = qwen35_pipeline(raw);
        assert_eq!(reasoning, "I should look up the weather for the user.");
        assert!(!clean.contains("<tool_call>") && !clean.contains("</think>"), "tags leaked: {clean:?}");
        assert_eq!(tcs.len(), 1, "tool call not parsed");
        assert_eq!(tcs[0]["function"]["name"], "get_weather");
        let args = tcs[0]["function"]["arguments"].as_str().unwrap();
        assert!(args.contains("\"city\":\"Hanoi\""), "args: {args}");
        assert!(args.contains("\"days\":3"), "days→number: {args}");
    }

    #[test]
    fn split_qwen35_xml_tool_call() {
        // Qwen3.5 / Hermes XML function-call wire format.
        let raw = "Sure.\n<tool_call>\n<function=weather>\n<parameter=city>\nHN\n</parameter>\n<parameter=days>\n3\n</parameter>\n</function>\n</tool_call>";
        let (text, tc) = split_qwen_tool_calls_from_model_text(raw);
        assert_eq!(text.trim(), "Sure.");
        assert_eq!(tc.len(), 1);
        assert_eq!(tc[0]["function"]["name"], "weather");
        let args = tc[0]["function"]["arguments"].as_str().unwrap();
        assert!(args.contains("\"city\":\"HN\""), "args: {args}");
        assert!(args.contains("\"days\":3"), "days should be coerced to number: {args}");
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
                usage: None,
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
            usage: None,
        }];
        let out = openai_messages_for_api(&msgs, "").unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["role"], "assistant");
        assert_eq!(out[0]["content"], "Hello");
        assert_eq!(out[0]["reasoning_content"], "step by step...");
    }
}
