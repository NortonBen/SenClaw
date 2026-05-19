//! Core query loop — the heart of the agent engine.
//!
//! Flow:
//! ```text
//! messages + system_prompt
//!   → query_llm() → assistant message
//!   → emit message:complete + conversation:usage
//!   → if no tool calls → finalize, return
//!   → run_tools() (concurrent or serial)
//!   → handle abort checkpoints
//!   → handle control signal rebuild (mode switch)
//!   → on context_length_exceeded → compact → retry (once)
//!   → recurse
//! ```
//!
//! Port of TS `Conversation.ts`.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use reqwest::Client;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use super::hooks::{ErrorInput, HookEvent, HookInput, HookInputBase, HookManager, PreCompactInput};
use super::*;
use crate::zen_core::events::ResponseRegistry;
use crate::zen_core::query_llm;
use crate::zen_core::run_tools::{self, PermissionChecker, RunContext};

/// Interruption message inserted when the session is aborted.
pub const INTERRUPT_MESSAGE: &str =
    "Session was interrupted. The current operation has been cancelled.";

/// Number of most-recent messages to keep during auto-compaction.
const COMPACT_KEEP_RECENT: usize = 12;
/// Hard cap for a single LLM request turn (cloud/API providers).
/// Dispatch tasks have their own larger timeout, but the model call itself
/// should not be able to hang silently with no message/tool events.
const LLM_TURN_TIMEOUT: Duration = Duration::from_secs(180);

/// Timeout for local native inference (Candle CPU/Metal **and** MLX native).
///
/// CPU inference is O(L²) in sequence length and un-batched; even a small model
/// (0.6B) on a debug build can take several minutes for a short response.
/// Release builds are typically 8–15× faster, so this is intentionally generous.
/// MLX native with TurboQuant KV quantization (`kv_cache_bits` set) is similarly
/// slow on long prefill — TQ4 quantizes ~12 K tokens × 36 layers on CPU when the
/// prompt holds many MCP tool schemas, and easily exceeds the cloud-API timeout.
///
/// Hard breakdown for `local-candle-native`:
///   • prefill  ≤ 512 tokens × 28 layers → ~50 s (debug) / ~5 s (release)
///   • decode   ≤ 512 tokens              → ~50 s (debug) / ~5 s (release)
///   • total (debug)   ≈ 100–180 s
///   • total (release) ≈ 10–20 s
const LLM_TURN_TIMEOUT_LOCAL: Duration = Duration::from_secs(900);

/// Compact message history when context length is exceeded.
/// Keeps the first user message (original task framing) and the most recent messages,
/// dropping middle messages to reduce token count.
fn compact_messages(messages: &mut Vec<Message>) -> bool {
    if messages.len() <= COMPACT_KEEP_RECENT + 2 {
        return false;
    }
    let first_user_idx = messages.iter().position(|m| m.msg_type == "user");
    let keep_from = messages.len().saturating_sub(COMPACT_KEEP_RECENT);

    let mut kept: Vec<Message> = Vec::new();
    if let Some(idx) = first_user_idx {
        if idx < keep_from {
            kept.push(messages[idx].clone());
            kept.push(create_user_message(vec![ContentBlock::Text {
                text: "[Context compacted — earlier messages were dropped to stay within the \
                       token limit. The conversation continues with the most recent context preserved.]"
                    .into(),
            }]));
        }
    }
    kept.extend(messages.drain(keep_from..));
    *messages = kept;
    true
}

/// Configuration passed to the query loop. All fields are owned so the config
/// can be moved into spawned tasks.
/// Resolver returning the current tool list. Called once per turn so newly
/// `ToolSearch`-discovered tools become available without recreating the
/// engine. Mirrors the same pattern used by `TaskTool::tools_resolver`.
pub type ToolsResolver = Arc<dyn Fn() -> Vec<Arc<dyn Tool>> + Send + Sync>;

pub struct QueryConfig {
    pub agent_id: String,
    pub working_dir: String,
    pub agent_data_dir: String,
    pub system_prompt: String,
    /// Closure returning the current tool list. Re-invoked each conversation
    /// turn so `ToolSearch` discoveries flow into subsequent turns within
    /// the same user input (the loop is one user message → many LLM turns).
    pub tools: ToolsResolver,
    pub http_client: Client,
    pub event_bus: EventBus,
    pub response_registry: Option<Arc<ResponseRegistry>>,
    pub permission_checker: Arc<dyn PermissionChecker>,
    pub profile: ModelProfile,
    pub thinking: bool,
    pub stream: bool,
    pub is_subagent: bool,
    /// Optional hook manager for PreToolUse/PostToolUse hooks.
    pub hook_manager: Option<Arc<HookManager>>,
    pub hook_client: Option<Client>,
    pub hook_profile: Option<ModelProfile>,
    pub session_id: String,
    /// Enable prompt caching (Anthropic only — cache_control on system + last tool).
    pub enable_cache: bool,
}

/// Run the conversation query loop. Returns the final message history.
///
/// This is an async generator conceptually — each "turn" is a call to the LLM
/// followed by optional tool execution. Since Rust doesn't have native async
/// generators yet, we process all turns in a single future and emit events
/// along the way.
pub async fn query(
    mut messages: Vec<Message>,
    config: &QueryConfig,
    cancel: &CancellationToken,
) -> Result<Vec<Message>> {
    let mut compacted = false;
    loop {
        // Check cancellation before each LLM call
        if cancel.is_cancelled() {
            info!("[{}] query loop cancelled before LLM call", config.agent_id);
            return Ok(messages);
        }

        // 1. Call the LLM. Resolve tools fresh each turn so any tool the
        //    model discovered via `ToolSearch` earlier in this conversation
        //    is included starting from the very next turn.
        let turn_tools: Vec<Arc<dyn Tool>> = (config.tools)();
        info!(
            "[{}] LLM turn start: messages={} tools={} stream={}",
            config.agent_id,
            messages.len(),
            turn_tools.len(),
            config.stream
        );
        let llm_call = query_llm::query_llm(
            &config.http_client,
            &messages,
            &config.system_prompt,
            &turn_tools,
            cancel,
            &config.profile,
            config.thinking,
            config.stream,
        );
        // Local native inference (Candle CPU/Metal, MLX native) runs in-process
        // and is much slower than cloud APIs — especially MLX with TurboQuant
        // KV (TQ4 prefill on long prompts blows past 180 s easily).
        let adapt = config.profile.adapt.as_deref().unwrap_or("");
        let turn_timeout = if adapt.starts_with("local-") {
            LLM_TURN_TIMEOUT_LOCAL
        } else {
            LLM_TURN_TIMEOUT
        };
        let assistant_msg = match tokio::time::timeout(turn_timeout, llm_call).await {
            Err(_) => {
                let msg = format!(
                    "LLM request timed out after {}s without completing a turn",
                    turn_timeout.as_secs()
                );
                config
                    .event_bus
                    .emit(EngineEvent::SessionError(SessionErrorData {
                        error_type: "error".into(),
                        error: SessionErrorDetail {
                            code: "LLM_TIMEOUT".into(),
                            message: msg.clone(),
                            details: None,
                        },
                    }));

                // Fire Error hook
                if let Some(ref hm) = config.hook_manager {
                    if hm.has_hooks_for_event(&HookEvent::Error) {
                        let base = HookInputBase {
                            hook_event_name: HookEvent::Error,
                            session_id: config.session_id.clone(),
                            agent_id: config.agent_id.clone(),
                            timestamp: chrono::Utc::now().to_rfc3339(),
                            cwd: config.working_dir.clone(),
                        };
                        let hook_input = HookInput::Error(ErrorInput {
                            base,
                            error_message: msg.clone(),
                            error_type: Some("LLM_TIMEOUT".to_string()),
                        });
                        let (client, profile) = (config.hook_client.clone(), config.hook_profile.clone());
                        let hm_clone = hm.clone();
                        tokio::spawn(async move {
                            let _ = super::hooks::execute_hooks(
                                &hm_clone,
                                &HookEvent::Error,
                                &hook_input,
                                &super::hooks::ExecuteHooksOptions {
                                    env: std::collections::HashMap::new(),
                                    cancel: None,
                                    client: client.as_ref(),
                                    profile: profile.as_ref(),
                                    messages: None,
                                },
                            )
                            .await;
                        });
                    }
                }
                return Err(anyhow::anyhow!(msg));
            }
            Ok(Ok(msg)) => msg,
            Ok(Err(e)) => {
                let classified = query_llm::LlmError::classify(&e);
                if classified.should_emit() {
                    let error_data = classified.to_session_error();
                    config
                        .event_bus
                        .emit(EngineEvent::SessionError(error_data.clone()));

                    // Fire Error hook
                    if let Some(ref hm) = config.hook_manager {
                        if hm.has_hooks_for_event(&HookEvent::Error) {
                            let base = HookInputBase {
                                hook_event_name: HookEvent::Error,
                                session_id: config.session_id.clone(),
                                agent_id: config.agent_id.clone(),
                                timestamp: chrono::Utc::now().to_rfc3339(),
                                cwd: config.working_dir.clone(),
                            };
                            let hook_input = HookInput::Error(ErrorInput {
                                base,
                                error_message: error_data.error.message.clone(),
                                error_type: Some(error_data.error.code.clone()),
                            });
                            let (client, profile) = (config.hook_client.clone(), config.hook_profile.clone());
                            let hm_clone = hm.clone();
                            tokio::spawn(async move {
                                let _ = super::hooks::execute_hooks(
                                    &hm_clone,
                                    &HookEvent::Error,
                                    &hook_input,
                                    &super::hooks::ExecuteHooksOptions {
                                        env: std::collections::HashMap::new(),
                                        cancel: None,
                                        client: client.as_ref(),
                                        profile: profile.as_ref(),
                                        messages: None,
                                    },
                                )
                                .await;
                            });
                        }
                    }
                }
                if classified.is_context_length && !compacted {
                    warn!(
                        agent_id = %config.agent_id,
                        msg_count = messages.len(),
                        "Context length exceeded — auto-compacting"
                    );
                    config
                        .event_bus
                        .emit(EngineEvent::CompactStart(CompactStartData {
                            message_count: messages.len(),
                        }));

                    // Fire PreCompact hook
                    if let Some(ref hm) = config.hook_manager {
                        if hm.has_hooks_for_event(&HookEvent::PreCompact) {
                            let base = HookInputBase {
                                hook_event_name: HookEvent::PreCompact,
                                session_id: config.session_id.clone(),
                                agent_id: config.agent_id.clone(),
                                timestamp: chrono::Utc::now().to_rfc3339(),
                                cwd: config.working_dir.clone(),
                            };
                            let context_history: Vec<serde_json::Value> = messages
                                .iter()
                                .filter_map(|m| serde_json::to_value(m).ok())
                                .collect();
                            let hook_input = HookInput::PreCompact(PreCompactInput {
                                base,
                                message_count: messages.len(),
                                context_history,
                            });
                            let (client, profile) = (config.hook_client.clone(), config.hook_profile.clone());
                            let hm_clone = hm.clone();
                            tokio::spawn(async move {
                                let _ = super::hooks::execute_hooks(
                                    &hm_clone,
                                    &HookEvent::PreCompact,
                                    &hook_input,
                                    &super::hooks::ExecuteHooksOptions {
                                        env: std::collections::HashMap::new(),
                                        cancel: None,
                                        client: client.as_ref(),
                                        profile: profile.as_ref(),
                                        messages: None,
                                    },
                                )
                                .await;
                            });
                        }
                    }

                    let did_compact = compact_messages(&mut messages);
                    config
                        .event_bus
                        .emit(EngineEvent::CompactExec(CompactExecData {
                            err_msg: if did_compact {
                                None
                            } else {
                                Some("compaction had no effect".into())
                            },
                            token_before: 0,
                            token_compact: 0,
                            compact_rate: 0.0,
                            summary: None,
                        }));

                    // Fire PostCompact hook (non-blockable)
                    if let Some(ref hm) = config.hook_manager {
                        if hm.has_hooks_for_event(&HookEvent::PostCompact) {
                            let base = HookInputBase {
                                hook_event_name: HookEvent::PostCompact,
                                session_id: config.session_id.clone(),
                                agent_id: config.agent_id.clone(),
                                timestamp: chrono::Utc::now().to_rfc3339(),
                                cwd: config.working_dir.clone(),
                            };
                            let context_history: Vec<serde_json::Value> = messages
                                .iter()
                                .filter_map(|m| serde_json::to_value(m).ok())
                                .collect();
                            let hook_input = HookInput::PreCompact(PreCompactInput {
                                base,
                                message_count: messages.len(),
                                context_history,
                            });
                            let (client, profile) = (config.hook_client.clone(), config.hook_profile.clone());
                            let hm_clone = hm.clone();
                            tokio::spawn(async move {
                                let _ = super::hooks::execute_hooks(
                                    &hm_clone,
                                    &HookEvent::PostCompact,
                                    &hook_input,
                                    &super::hooks::ExecuteHooksOptions {
                                        env: std::collections::HashMap::new(),
                                        cancel: None,
                                        client: client.as_ref(),
                                        profile: profile.as_ref(),
                                        messages: None,
                                    },
                                )
                                .await;
                            });
                        }
                    }
                    if did_compact {
                        info!(
                            agent_id = %config.agent_id,
                            new_msg_count = messages.len(),
                            "Auto-compact complete, retrying"
                        );
                        compacted = true;
                        continue;
                    }
                    warn!(
                        agent_id = %config.agent_id,
                        "Auto-compact had no effect, giving up"
                    );
                }
                return Err(e);
            }
        };

        // Checkpoint 1: after LLM response, before tool execution
        if cancel.is_cancelled() {
            info!("[{}] cancelled after LLM response", config.agent_id);
            let pending_tools = assistant_msg
                .message
                .content
                .iter()
                .filter_map(|b| {
                    if let ContentBlock::ToolUse { id, .. } = b {
                        Some(create_tool_result_stop(id))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();

            let mut interrupt_content: Vec<ContentBlock> = pending_tools;
            interrupt_content.push(ContentBlock::Text {
                text: INTERRUPT_MESSAGE.to_string(),
            });
            messages.push(assistant_msg);
            messages.push(create_user_message(interrupt_content));

            config
                .event_bus
                .emit(EngineEvent::SessionInterrupted(SessionInterruptedData {
                    agent_id: config.agent_id.to_string(),
                    content: INTERRUPT_MESSAGE.to_string(),
                }));
            return Ok(messages);
        }

        // 2. Emit message:complete
        let (text_content, reasoning, tool_uses) = extract_content(&assistant_msg);

        let has_tool_calls = !tool_uses.is_empty();
        info!(
            "[{}] LLM turn complete: text_len={} reasoning_len={} tool_calls={}",
            config.agent_id,
            text_content.len(),
            reasoning.len(),
            tool_uses.len()
        );

        // Guard against silent empty completions. Some custom OpenAI-compat
        // endpoints (observed with `qwen3.5-4b-optiq` under heavy tool counts)
        // return 200 OK with zero blocks and zero tool calls — model didn't
        // actually generate anything. Treat as a session error rather than
        // accepting an empty assistant message into history, which would
        // poison every subsequent turn.
        if text_content.trim().is_empty() && reasoning.trim().is_empty() && tool_uses.is_empty() {
            warn!(
                "[{}] empty LLM completion (blocks=0, tool_calls=0). \
                 Likely upstream endpoint issue (auth, rate-limit, malformed SSE). \
                 Dropping empty assistant message to preserve history.",
                config.agent_id
            );
            config.event_bus.emit(EngineEvent::SessionError(
                crate::zen_core::SessionErrorData {
                    error_type: "empty_completion".to_string(),
                    error: crate::zen_core::SessionErrorDetail {
                        code: "EMPTY_COMPLETION".to_string(),
                        message: "LLM returned empty response (no text / reasoning / tool calls). \
                                 Check endpoint logs — common causes: auth failure, model overload, \
                                 tool count exceeds endpoint limit."
                            .to_string(),
                        details: None,
                    },
                },
            ));
            return Ok(messages);
        }
        let tool_call_infos: Option<Vec<ToolCallInfo>> = if has_tool_calls {
            Some(
                tool_uses
                    .iter()
                    .filter_map(|b| {
                        if let ContentBlock::ToolUse { name, input, .. } = b {
                            Some(ToolCallInfo {
                                name: name.clone(),
                                args: input.clone(),
                            })
                        } else {
                            None
                        }
                    })
                    .collect(),
            )
        } else {
            None
        };

        config
            .event_bus
            .emit(EngineEvent::MessageComplete(MessageCompleteData {
                agent_id: config.agent_id.to_string(),
                reasoning,
                content: text_content.clone(),
                has_tool_calls: has_tool_calls,
                tool_calls: tool_call_infos,
            }));

        // 3. Emit conversation:usage (subagents skip this)
        if !config.is_subagent {
            let updated = {
                let mut msgs = messages.clone();
                msgs.push(assistant_msg.clone());
                msgs
            };
            let _usage = estimate_usage(&updated);
            config
                .event_bus
                .emit(EngineEvent::ConversationUsage(ConversationUsageData {
                    usage: _usage,
                }));
        }

        // 4. No tools → done
        if tool_uses.is_empty() {
            messages.push(assistant_msg);
            info!(
                "[{}] query complete — no tool calls, {} messages",
                config.agent_id,
                messages.len()
            );
            return Ok(messages);
        }

        // 5. Run tools
        let ctx = RunContext {
            agent_id: &config.agent_id,
            working_dir: &config.working_dir,
            agent_data_dir: &config.agent_data_dir,
            tools: &turn_tools,
            fire: &|event| config.event_bus.emit(event),
            permission_checker: config.permission_checker.as_ref(),
            event_bus: Some(&config.event_bus),
            response_registry: config.response_registry.as_deref(),
            hook_manager: config.hook_manager.clone(),
            hook_client: config.hook_client.clone(),
            hook_profile: config.hook_profile.clone(),
            session_id: config.session_id.clone(),
        };

        let tool_results = run_tools::run_tools(&tool_uses, cancel, &ctx).await;

        // Checkpoint 2: after tool execution, before recursion
        if cancel.is_cancelled() {
            info!("[{}] cancelled after tool execution", config.agent_id);

            // Generate stop messages for any incomplete tools
            let completed_ids: std::collections::HashSet<String> = tool_results
                .iter()
                .filter_map(|b| {
                    if let ContentBlock::ToolResult { tool_use_id, .. } = b {
                        Some(tool_use_id.clone())
                    } else {
                        None
                    }
                })
                .collect();

            let mut all_results = tool_results;
            for tu in &tool_uses {
                if let ContentBlock::ToolUse { id, .. } = tu {
                    if !completed_ids.contains(id) {
                        all_results.push(create_tool_result_stop(id));
                    }
                }
            }
            // Append interrupt text to last result
            if let Some(last) = all_results.last_mut() {
                if let ContentBlock::ToolResult { content, .. } = last {
                    content.push_str(&format!("\n\n{INTERRUPT_MESSAGE}"));
                }
            }

            messages.push(assistant_msg);
            if !all_results.is_empty() {
                messages.push(create_user_message(all_results));
            }

            config
                .event_bus
                .emit(EngineEvent::SessionInterrupted(SessionInterruptedData {
                    agent_id: config.agent_id.to_string(),
                    content: INTERRUPT_MESSAGE.to_string(),
                }));
            return Ok(messages);
        }

        // 6. Check for control signal rebuild (mode switch)
        // For now: no rebuild — just recurse
        messages.push(assistant_msg);
        if !tool_results.is_empty() {
            messages.push(create_user_message(tool_results));
        }

        debug!(
            "[{}] recurse — {} messages, {} tool results",
            config.agent_id,
            messages.len(),
            tool_uses.len()
        );
    }
}

// ============================================================================
// Content extraction helpers
// ============================================================================

pub(crate) fn extract_content(msg: &Message) -> (String, String, Vec<ContentBlock>) {
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut tool_uses = Vec::new();

    for block in &msg.message.content {
        match block {
            ContentBlock::Text { text: t } => text.push_str(t),
            ContentBlock::Thinking { thinking: t } => reasoning.push_str(t),
            ContentBlock::ToolUse { .. } => {
                tool_uses.push(block.clone());
            }
            _ => {}
        }
    }

    (text, reasoning, tool_uses)
}

// ============================================================================
// Token estimation (rough)
// ============================================================================

fn estimate_usage(messages: &[Message]) -> UsageData {
    let total_chars: usize = messages
        .iter()
        .map(|m| {
            m.message
                .content
                .iter()
                .map(|b| match b {
                    ContentBlock::Text { text } => text.len(),
                    _ => 0,
                })
                .sum::<usize>()
        })
        .sum();

    // Rough: 4 chars ≈ 1 token
    let use_tokens = (total_chars as f64 / 4.0).ceil() as u64;
    UsageData {
        use_tokens,
        max_tokens: 200_000,
        prompt_tokens: use_tokens,
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_content_separates_text_and_tools() {
        let msg = Message {
            msg_type: "assistant".into(),
            message: MessagePayload {
                role: "assistant".into(),
                content: vec![
                    ContentBlock::Thinking {
                        thinking: "Let me think...".into(),
                    },
                    ContentBlock::Text {
                        text: "I will help.".into(),
                    },
                    ContentBlock::ToolUse {
                        id: "tu-1".into(),
                        name: "read".into(),
                        input: serde_json::json!({"path": "/tmp/test"}),
                    },
                ],
            },
            uuid: "uuid-1".into(),
        };

        let (text, reasoning, tools) = extract_content(&msg);
        assert_eq!(text, "I will help.");
        assert_eq!(reasoning, "Let me think...");
        assert_eq!(tools.len(), 1);
        if let ContentBlock::ToolUse { name, .. } = &tools[0] {
            assert_eq!(name, "read");
        } else {
            panic!("Expected ToolUse");
        }
    }

    #[test]
    fn extract_content_empty() {
        let msg = Message {
            msg_type: "assistant".into(),
            message: MessagePayload {
                role: "assistant".into(),
                content: vec![],
            },
            uuid: "uuid-1".into(),
        };
        let (text, reasoning, tools) = extract_content(&msg);
        assert!(text.is_empty());
        assert!(reasoning.is_empty());
        assert!(tools.is_empty());
    }

    #[test]
    fn estimate_usage_counts_chars() {
        let msgs = vec![Message {
            msg_type: "user".into(),
            message: MessagePayload {
                role: "user".into(),
                content: vec![ContentBlock::Text {
                    text: "Hello, world!".into(),
                }],
            },
            uuid: "uuid-1".into(),
        }];
        let usage = estimate_usage(&msgs);
        // 13 chars / 4 ≈ 4 tokens
        assert!(usage.use_tokens >= 3);
    }
}
