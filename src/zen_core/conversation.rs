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

/// Fallback context window when a model profile reports no `context_length`.
/// Mirrors the TS default in `util/tokens.ts` / `util/compact.ts`.
const DEFAULT_CONTEXT_LENGTH: u64 = 128_000;

/// Trigger proactive auto-compaction once input tokens reach this fraction of
/// the model context window. Mirrors TS `AUTO_COMPACT_THRESHOLD_RATIO`.
const AUTO_COMPACT_THRESHOLD_RATIO: f64 = 0.75;
/// Minimum message count before the **after-process** stage will proactively
/// compact a completed conversation. Below this the history is small enough that
/// summarizing it would cost an LLM call without meaningful benefit (and risk
/// losing recent detail), so `compact_now` is a no-op.
const AFTER_PROCESS_MIN_MESSAGES: usize = 16;
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

/// Prompt used to ask the main model for a lossless session snapshot.
/// Ported verbatim from TS `prompt/compact.ts` `COMPRESSION_PROMPT`.
const COMPRESSION_PROMPT: &str = "Create a lossless state snapshot of this session so any later instance can seamlessly resume work.

Cover the following (merge sections freely, but omit nothing):

A. **Intent evolution** — User requests in time order, how they changed, final shape. Include key user messages verbatim.
B. **Technical context** — Frameworks, toolchains, architecture, runtime environment.
C. **Artifacts & changes** — Files examined/modified/created. Embed full source for key changes.
D. **Errors & fixes** — All anomalies, fix paths, and user corrections.
E. **Open items** — Closed vs in-progress vs remaining work, with blockers.
F. **Interruption point** — Exact files, functions, edit actions at the moment of interruption.
G. **Continuation path** (only if applicable) — Quote user's follow-up intent, task name, suggested handoff.

## Rules
- Archive only from conversation content — no speculation or fabrication.
- Label gaps as \"not confirmed in context\".
- No tool calls — pure text reasoning and archival.
- Prefer full source over vague description.
";

const COMPACT_NOTICE: &str = "[Context Compression Notice]
The conversation has been automatically compressed due to token limit. Below is a comprehensive summary.";

/// Fire a PreCompact/PostCompact hook (non-blocking, spawned). No-op when no
/// hook manager or no hooks registered for the event.
fn spawn_compact_hook(config: &QueryConfig, messages: &[Message], event: HookEvent) {
    let Some(hm) = config.hook_manager.clone() else {
        return;
    };
    if !hm.has_hooks_for_event(&event) {
        return;
    }
    let base = HookInputBase {
        hook_event_name: event.clone(),
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
    tokio::spawn(async move {
        let _ = super::hooks::execute_hooks(
            &hm,
            &event,
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

/// Index of the last "real" user message (not a tool_result carrier). History
/// before this index can be safely compacted while the current turn is kept
/// intact, guaranteeing the message list still ends on a user message and that
/// tool_use/tool_result pairs are not split. Mirrors TS `autoCompact`.
fn last_real_user_index(messages: &[Message]) -> Option<usize> {
    messages.iter().rposition(|m| {
        m.msg_type == "user"
            && !matches!(
                m.message.content.first(),
                Some(ContentBlock::ToolResult { .. })
            )
    })
}

/// Build the post-compaction history: a compression notice (user) followed by
/// the model-generated summary (assistant). The summary message carries a
/// corrected usage reflecting only notice + summary, so the token gauge drops
/// immediately. Mirrors TS `executeAutoCompact`.
async fn summarize_history(
    history: &[Message],
    config: &QueryConfig,
    cancel: &CancellationToken,
) -> Result<(Vec<Message>, String)> {
    let mut msgs = history.to_vec();
    msgs.push(create_user_message(vec![ContentBlock::Text {
        text: COMPRESSION_PROMPT.to_string(),
    }]));

    let summary_msg = query_llm::query_llm(
        &config.http_client,
        &msgs,
        "An AI assistant that helps summarize coding conversations.",
        &[],
        cancel,
        &config.profile,
        false,
        false,
    )
    .await?;

    let (summary_text, _reasoning, _tools) = extract_content(&summary_msg);
    if summary_text.trim().is_empty() {
        anyhow::bail!("compaction produced an empty summary");
    }

    let notice = create_user_message(vec![ContentBlock::Text {
        text: COMPACT_NOTICE.to_string(),
    }]);

    // Correct usage: post-compaction context ≈ notice (~30 tokens) + summary.
    let summary_tokens = summary_msg.usage.as_ref().map(|u| u.output()).unwrap_or(0);
    let corrected_usage = if summary_tokens > 0 {
        Some(RawUsage {
            input_tokens: Some(30 + summary_tokens),
            output_tokens: Some(summary_tokens),
            ..Default::default()
        })
    } else {
        None
    };

    let summary_assistant = Message {
        msg_type: "assistant".to_string(),
        message: MessagePayload {
            role: "assistant".to_string(),
            content: vec![ContentBlock::Text {
                text: summary_text.clone(),
            }],
        },
        uuid: uuid::Uuid::new_v4().to_string(),
        usage: corrected_usage,
    };

    Ok((vec![notice, summary_assistant], summary_text))
}

/// Result of an auto-compaction attempt.
struct CompactOutcome {
    messages: Vec<Message>,
    exec: CompactExecData,
    /// Usage to broadcast after compaction (reflects the compacted history).
    usage: UsageData,
    /// True when compaction actually changed the message list.
    changed: bool,
}

/// **After-process stage.** Run *after* a turn completes (not in the query
/// loop) to keep the stored conversation compact and coherent — the same
/// Claude-Code-style LLM summarization as the in-loop safety compaction, but
/// triggered eagerly once the history has grown past
/// [`AFTER_PROCESS_MIN_MESSAGES`] rather than only at the 75% context threshold.
///
/// Earlier messages are summarized into a lossless snapshot while the most
/// recent turn is preserved verbatim; on summary failure it falls back to
/// keep-recent truncation (inherited from [`auto_compact`]). Emits the same
/// Compact/Usage events + Pre/PostCompact hooks as the in-loop path so the UI
/// reflects the compaction. Returns the (possibly unchanged) message list;
/// no-op for subagents and trivially short conversations.
pub async fn compact_now(
    mut messages: Vec<Message>,
    config: &QueryConfig,
    cancel: &CancellationToken,
) -> Vec<Message> {
    if config.is_subagent || messages.len() < AFTER_PROCESS_MIN_MESSAGES {
        return messages;
    }
    info!(
        agent_id = %config.agent_id,
        msg_count = messages.len(),
        "after-process: proactively compacting completed conversation"
    );
    config
        .event_bus
        .emit(EngineEvent::CompactStart(CompactStartData {
            message_count: messages.len(),
        }));
    spawn_compact_hook(config, &messages, HookEvent::PreCompact);

    let outcome = auto_compact(std::mem::take(&mut messages), config, cancel).await;
    messages = outcome.messages;

    config
        .event_bus
        .emit(EngineEvent::CompactExec(outcome.exec));
    spawn_compact_hook(config, &messages, HookEvent::PostCompact);
    if outcome.changed {
        config
            .event_bus
            .emit(EngineEvent::ConversationUsage(ConversationUsageData {
                usage: outcome.usage,
            }));
        info!(
            agent_id = %config.agent_id,
            new_msg_count = messages.len(),
            "after-process: conversation compacted"
        );
    }
    messages
}

/// Proactively compact the conversation via LLM summarization, preserving the
/// current turn. Falls back to deterministic keep-recent truncation if the
/// summary call fails. Mirrors TS `autoCompact` + `compactMessages`.
async fn auto_compact(
    messages: Vec<Message>,
    config: &QueryConfig,
    cancel: &CancellationToken,
) -> CompactOutcome {
    let ctx = config.profile.context_length;
    let token_before = count_tokens(&messages, ctx).use_tokens;

    let Some(idx) = last_real_user_index(&messages) else {
        return CompactOutcome {
            usage: count_tokens(&messages, ctx),
            exec: CompactExecData {
                err_msg: Some("no user message to anchor compaction".into()),
                token_before,
                token_compact: token_before,
                compact_rate: 1.0,
                summary: None,
            },
            messages,
            changed: false,
        };
    };

    let history = &messages[..idx];
    let keep = messages[idx..].to_vec();

    if history.len() < 2 {
        return CompactOutcome {
            usage: count_tokens(&messages, ctx),
            exec: CompactExecData {
                err_msg: Some("history too small to compact".into()),
                token_before,
                token_compact: token_before,
                compact_rate: 1.0,
                summary: None,
            },
            messages,
            changed: false,
        };
    }

    let (compacted_history, summary, err_msg) =
        match summarize_history(history, config, cancel).await {
            Ok((compacted, summary)) => (compacted, Some(summary), None),
            Err(e) => {
                // Fallback: deterministic keep-recent truncation over the full list.
                warn!(
                    agent_id = %config.agent_id,
                    error = %e,
                    "LLM compaction failed — falling back to keep-recent truncation"
                );
                let mut truncated = messages.clone();
                let did = compact_messages(&mut truncated);
                if !did {
                    return CompactOutcome {
                        usage: count_tokens(&messages, ctx),
                        exec: CompactExecData {
                            err_msg: Some(format!("compaction failed: {e}")),
                            token_before,
                            token_compact: token_before,
                            compact_rate: 1.0,
                            summary: None,
                        },
                        messages,
                        changed: false,
                    };
                }
                // For the truncation fallback the whole list is the result.
                let usage = count_tokens(&truncated, ctx);
                return CompactOutcome {
                    exec: CompactExecData {
                        err_msg: None,
                        token_before,
                        token_compact: usage.use_tokens,
                        compact_rate: if token_before > 0 {
                            usage.use_tokens as f64 / token_before as f64
                        } else {
                            1.0
                        },
                        summary: None,
                    },
                    usage,
                    messages: truncated,
                    changed: true,
                };
            }
        };

    // Usage after compaction reflects only the compacted history (notice +
    // summary); the kept current turn re-counts on the next real LLM turn.
    let usage_after = count_tokens(&compacted_history, ctx);
    let mut final_messages = compacted_history;
    final_messages.extend(keep);

    CompactOutcome {
        exec: CompactExecData {
            err_msg,
            token_before,
            token_compact: usage_after.use_tokens,
            compact_rate: if token_before > 0 {
                usage_after.use_tokens as f64 / token_before as f64
            } else {
                1.0
            },
            summary,
        },
        usage: usage_after,
        messages: final_messages,
        changed: true,
    }
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

/// Absolute cap on LLM turns per user input — a backstop against runaway
/// agentic loops (small models can call tools forever). Override with
/// `SENCLAW_MAX_AGENT_TURNS`.
fn max_agent_turns() -> usize {
    std::env::var("SENCLAW_MAX_AGENT_TURNS")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .filter(|n| *n > 0)
        .unwrap_or(30)
}

/// After this many *consecutive* turns that call the same tool(s) and produce
/// no user-facing text, nudge the model to stop and answer. Override with
/// `SENCLAW_STALL_TOOL_TURNS`. The hard stop is twice this value.
fn stall_tool_turns() -> usize {
    std::env::var("SENCLAW_STALL_TOOL_TURNS")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .filter(|n| *n > 0)
        .unwrap_or(4)
}

/// Instruction injected when the agent is detected spinning on the same tool.
const STALL_NUDGE: &str = "You have called the same tool several times in a row \
without producing an answer. You already have enough information. Stop calling tools now \
and write your final answer to the user, in the user's language, based on what you have gathered.";

/// Sorted, deduped set of tool names in a batch — a coarse signature used to
/// detect the model re-invoking the same tool turn after turn (args may vary
/// slightly, e.g. a tweaked search query, so names alone are the stable signal).
fn tool_names_sig(tool_uses: &[ContentBlock]) -> String {
    let mut names: Vec<&str> = tool_uses
        .iter()
        .filter_map(|b| match b {
            ContentBlock::ToolUse { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    names.sort_unstable();
    names.dedup();
    names.join(",")
}

/// Exact signature of one tool call: name + canonical JSON args. Used to catch
/// a model re-issuing an *identical* call (same query, same URL) that can only
/// return what it already has.
fn tool_call_sig(block: &ContentBlock) -> Option<String> {
    if let ContentBlock::ToolUse { name, input, .. } = block {
        Some(format!(
            "{name}\u{1}{}",
            serde_json::to_string(input).unwrap_or_default()
        ))
    } else {
        None
    }
}

/// Synthetic tool result returned in place of re-executing an identical call.
const DUPLICATE_CALL_NOTE: &str = "Duplicate call: you already called this tool with these exact \
arguments earlier in this conversation and received a result above. Re-running it returns nothing \
new. Use the information you already have to write your final answer to the user now. If you truly \
need different information, change the arguments — do not repeat the identical call.";

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
    let mut proactively_compacted = false;

    // Loop-guard state (see `max_agent_turns` / `stall_tool_turns`).
    let max_turns = max_agent_turns();
    let stall_limit = stall_tool_turns();
    let mut turn: usize = 0;
    let mut last_sig: Option<String> = None;
    let mut stall_streak: usize = 0;
    let mut nudged = false;
    // Exact (name+args) tool-call signatures already executed this user input,
    // so identical re-issues can be short-circuited instead of re-run.
    let mut seen_tool_sigs: std::collections::HashSet<String> = std::collections::HashSet::new();

    loop {
        // Check cancellation before each LLM call
        if cancel.is_cancelled() {
            info!("[{}] query loop cancelled before LLM call", config.agent_id);
            return Ok(messages);
        }

        // Backstop: hard cap on turns per user input so a model that ignores
        // every nudge still terminates instead of looping until the user pauses.
        turn += 1;
        if turn > max_turns {
            warn!(
                "[{}] agent loop limit reached ({} turns) — stopping",
                config.agent_id, max_turns
            );
            config
                .event_bus
                .emit(EngineEvent::SessionError(SessionErrorData {
                    error_type: "agent_loop_limit".to_string(),
                    error: SessionErrorDetail {
                        code: "AGENT_LOOP_LIMIT".to_string(),
                        message: format!(
                            "Stopped after {max_turns} tool-calling turns without a final answer \
                             (likely a loop). Increase SENCLAW_MAX_AGENT_TURNS to allow more."
                        ),
                        details: None,
                    },
                }));
            return Ok(messages);
        }

        // 0. Proactive compaction: if the context is approaching the model
        //    limit, summarize the history *before* the next call so we don't
        //    hit a hard context_length error. Runs at most once per user input
        //    (the reactive path below remains as a safety net). Subagents skip.
        if !config.is_subagent
            && !proactively_compacted
            && needs_auto_compact(&messages, config.profile.context_length)
        {
            info!(
                agent_id = %config.agent_id,
                msg_count = messages.len(),
                "Context near limit — proactively compacting"
            );
            config
                .event_bus
                .emit(EngineEvent::CompactStart(CompactStartData {
                    message_count: messages.len(),
                }));
            spawn_compact_hook(config, &messages, HookEvent::PreCompact);

            let outcome = auto_compact(std::mem::take(&mut messages), config, cancel).await;
            messages = outcome.messages;
            proactively_compacted = true;

            config
                .event_bus
                .emit(EngineEvent::CompactExec(outcome.exec));
            spawn_compact_hook(config, &messages, HookEvent::PostCompact);
            if outcome.changed {
                config
                    .event_bus
                    .emit(EngineEvent::ConversationUsage(ConversationUsageData {
                        usage: outcome.usage,
                    }));
                info!(
                    agent_id = %config.agent_id,
                    new_msg_count = messages.len(),
                    "Proactive compaction complete"
                );
            }
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
                        let (client, profile) =
                            (config.hook_client.clone(), config.hook_profile.clone());
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
                            let (client, profile) =
                                (config.hook_client.clone(), config.hook_profile.clone());
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
                    spawn_compact_hook(config, &messages, HookEvent::PreCompact);

                    // Reactive path: the API already rejected the request as
                    // too large, so a summarization LLM call would likely fail
                    // too. Use deterministic keep-recent truncation, but report
                    // real before/after token metrics.
                    let token_before =
                        count_tokens(&messages, config.profile.context_length).use_tokens;
                    let did_compact = compact_messages(&mut messages);
                    let usage_after = count_tokens(&messages, config.profile.context_length);
                    config
                        .event_bus
                        .emit(EngineEvent::CompactExec(CompactExecData {
                            err_msg: if did_compact {
                                None
                            } else {
                                Some("compaction had no effect".into())
                            },
                            token_before,
                            token_compact: usage_after.use_tokens,
                            compact_rate: if token_before > 0 {
                                usage_after.use_tokens as f64 / token_before as f64
                            } else {
                                1.0
                            },
                            summary: None,
                        }));
                    spawn_compact_hook(config, &messages, HookEvent::PostCompact);

                    if did_compact {
                        info!(
                            agent_id = %config.agent_id,
                            new_msg_count = messages.len(),
                            "Auto-compact complete, retrying"
                        );
                        if !config.is_subagent {
                            config.event_bus.emit(EngineEvent::ConversationUsage(
                                ConversationUsageData { usage: usage_after },
                            ));
                        }
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
                // Prefer real API usage; fall back to a char-based estimate of
                // THIS message's output for providers that don't report usage
                // (e.g. local MLX/Candle inference), so the per-message token
                // badge still shows a meaningful number.
                output_tokens: assistant_msg
                    .usage
                    .as_ref()
                    .map(|u| u.output() as u32)
                    .filter(|&t| t > 0)
                    .unwrap_or_else(|| {
                        estimate_tokens_by_chars(std::slice::from_ref(&assistant_msg)) as u32
                    }),
            }));

        // 3. Emit conversation:usage (subagents skip this)
        if !config.is_subagent {
            let updated = {
                let mut msgs = messages.clone();
                msgs.push(assistant_msg.clone());
                msgs
            };
            let usage = count_tokens(&updated, config.profile.context_length);
            config
                .event_bus
                .emit(EngineEvent::ConversationUsage(ConversationUsageData {
                    usage,
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
            tools_resolver: Some(config.tools.as_ref()),
            fire: &|event| config.event_bus.emit(event),
            permission_checker: config.permission_checker.as_ref(),
            event_bus: Some(&config.event_bus),
            response_registry: config.response_registry.as_deref(),
            hook_manager: config.hook_manager.clone(),
            hook_client: config.hook_client.clone(),
            hook_profile: config.hook_profile.clone(),
            session_id: config.session_id.clone(),
        };

        // Intercept exact-duplicate tool calls. A weak model often re-issues
        // the same call (identical search query, identical Skill load) without
        // ever using the result. Execute only fresh calls; for duplicates,
        // synthesize a result telling the model the data is already available.
        let mut fresh: Vec<ContentBlock> = Vec::new();
        let mut dup_ids: Vec<String> = Vec::new();
        for tu in &tool_uses {
            let id = match tu {
                ContentBlock::ToolUse { id, .. } => id.clone(),
                _ => continue,
            };
            match tool_call_sig(tu) {
                Some(sig) if seen_tool_sigs.contains(&sig) => dup_ids.push(id),
                Some(sig) => {
                    seen_tool_sigs.insert(sig);
                    fresh.push(tu.clone());
                }
                None => fresh.push(tu.clone()),
            }
        }

        let mut tool_results = if fresh.is_empty() {
            Vec::new()
        } else {
            run_tools::run_tools(&fresh, cancel, &ctx).await
        };

        // Check for control signals (e.g. clearContextAndStart)
        let mut clear_context_payload = None;
        for b in &tool_results {
            if let ContentBlock::ControlSignal {
                signal_type,
                payload,
            } = b
            {
                if signal_type == "ClearContextAndStart" {
                    clear_context_payload = Some(payload.clone());
                }
            }
        }
        if let Some(payload) = clear_context_payload {
            let plan_content = payload
                .get("plan_content")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            info!(
                "[{}] received ClearContextAndStart control signal — clearing history",
                config.agent_id
            );
            messages.clear();
            messages.push(create_user_message(vec![ContentBlock::Text {
                text: format!("按照以下计划进行实现：\n\n{}", plan_content),
            }]));
            // Trigger mode switch if needed (this matches TS behaviour)
            // The ZenEngine will handle PlanImplement event and set agent_mode
            // wait, but the event wasn't emitted here? Actually, the UI emits PlanExitResponse to engine,
            // which flips mode. But we can emit a PlanImplement event just in case, or just continue.
            continue;
        }

        if !dup_ids.is_empty() {
            info!(
                "[{}] intercepted {} duplicate tool call(s) — returning 'already have this' note",
                config.agent_id,
                dup_ids.len()
            );
            for id in &dup_ids {
                tool_results.push(ContentBlock::ToolResult {
                    tool_use_id: id.clone(),
                    content: DUPLICATE_CALL_NOTE.to_string(),
                    is_error: false,
                });
            }
        }

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

        // 5b. Answer-and-stop: the model produced a real answer this turn AND
        //     every tool call it made was an intercepted duplicate (nothing
        //     fresh ran). There is no work left — deliver the answer instead of
        //     looping and re-generating it. Observed with weak models that emit
        //     the full answer plus a redundant tool call on every turn, so the
        //     "no tool calls → done" exit never triggers.
        if fresh.is_empty() && !dup_ids.is_empty() && !text_content.trim().is_empty() {
            info!(
                "[{}] answer present and all {} tool call(s) were duplicates — completing",
                config.agent_id,
                tool_uses.len()
            );
            messages.push(assistant_msg);
            if !tool_results.is_empty() {
                messages.push(create_user_message(tool_results));
            }
            return Ok(messages);
        }

        // 6. Stall detection: count consecutive turns that call the same
        //    tool(s) and emit no user-facing text. A weak model can spin on
        //    e.g. browser_search forever (observed: 6+ identical search turns,
        //    context ballooning 7k→72k tokens). Nudge once, then hard-stop.
        let sig = tool_names_sig(&tool_uses);
        if text_content.trim().is_empty() && Some(&sig) == last_sig.as_ref() {
            stall_streak += 1;
        } else {
            stall_streak = 0;
        }
        last_sig = Some(sig);

        let hard_stop = stall_streak >= stall_limit * 2;
        let should_nudge = !nudged && stall_streak >= stall_limit;

        // 7. Recurse: append the assistant turn and the tool results.
        messages.push(assistant_msg);
        if !tool_results.is_empty() {
            let mut blocks = tool_results;
            if should_nudge {
                nudged = true;
                info!(
                    "[{}] tool-call stall detected ({} consecutive turns on '{}') — nudging to finalize",
                    config.agent_id,
                    stall_streak,
                    last_sig.as_deref().unwrap_or("")
                );
                blocks.push(ContentBlock::Text {
                    text: STALL_NUDGE.to_string(),
                });
            }
            messages.push(create_user_message(blocks));
        }

        if hard_stop {
            warn!(
                "[{}] tool-call stall not resolved after nudge ({} consecutive turns) — stopping",
                config.agent_id, stall_streak
            );
            config
                .event_bus
                .emit(EngineEvent::SessionError(SessionErrorData {
                    error_type: "agent_loop_limit".to_string(),
                    error: SessionErrorDetail {
                        code: "AGENT_TOOL_STALL".to_string(),
                        message: format!(
                            "Stopped: the agent called the same tool {stall_streak} times in a row \
                             without producing an answer."
                        ),
                        details: None,
                    },
                }));
            return Ok(messages);
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

/// Sum of input + output tokens reported by the most recent assistant message
/// that carries real API usage. Returns `None` if no message reported usage
/// (e.g. local inference, or a fresh conversation). Mirrors TS `countTokens`.
fn real_token_usage(messages: &[Message]) -> Option<(u64, u64)> {
    messages.iter().rev().find_map(|m| match &m.usage {
        Some(u) if !u.is_empty() => Some((u.input(), u.output())),
        _ => None,
    })
}

/// Rough char-based token estimate over all text-bearing content. Fallback for
/// providers that don't report usage. ~4 chars ≈ 1 token.
fn estimate_tokens_by_chars(messages: &[Message]) -> u64 {
    let total_chars: usize = messages
        .iter()
        .flat_map(|m| m.message.content.iter())
        .map(|b| match b {
            ContentBlock::Text { text } => text.len(),
            ContentBlock::Thinking { thinking } => thinking.len(),
            ContentBlock::ToolResult { content, .. } => content.len(),
            _ => 0,
        })
        .sum();
    (total_chars as f64 / 4.0).ceil() as u64
}

/// Compute conversation token usage for the UI gauge.
///
/// Mirrors TS `getTokens`: prefer real API usage from the latest assistant
/// message, and report `max_tokens` as the model's actual context window
/// (not a hardcoded constant). Falls back to a char estimate when no usage
/// was reported.
fn count_tokens(messages: &[Message], context_length: u32) -> UsageData {
    let max_tokens = if context_length > 0 {
        context_length as u64
    } else {
        DEFAULT_CONTEXT_LENGTH
    };

    if let Some((input, output)) = real_token_usage(messages) {
        return UsageData {
            use_tokens: input + output,
            max_tokens,
            prompt_tokens: input,
        };
    }

    let use_tokens = estimate_tokens_by_chars(messages);
    UsageData {
        use_tokens,
        max_tokens,
        prompt_tokens: use_tokens,
    }
}

/// Current input-token count for compaction decisions: real prompt tokens when
/// available, else the char-based estimate.
fn current_input_tokens(messages: &[Message]) -> u64 {
    match real_token_usage(messages) {
        Some((input, _)) => input,
        None => estimate_tokens_by_chars(messages),
    }
}

/// Whether the conversation should be proactively compacted before the next
/// LLM call. Mirrors TS `needsAutoCompact`.
fn needs_auto_compact(messages: &[Message], context_length: u32) -> bool {
    if messages.len() < 3 {
        return false;
    }
    let limit = if context_length > 0 {
        context_length as u64
    } else {
        DEFAULT_CONTEXT_LENGTH
    };
    let threshold = (limit as f64 * AUTO_COMPACT_THRESHOLD_RATIO) as u64;
    current_input_tokens(messages) >= threshold
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn tool_use(name: &str, args: serde_json::Value) -> ContentBlock {
        ContentBlock::ToolUse {
            id: "x".into(),
            name: name.into(),
            input: args,
        }
    }

    #[test]
    fn tool_names_sig_is_stable_across_arg_changes() {
        // Same tool, different args (e.g. a tweaked search query) → same sig,
        // which is exactly what lets the stall detector catch a search loop.
        let a = tool_names_sig(&[tool_use("search", serde_json::json!({"q": "gold"}))]);
        let b = tool_names_sig(&[tool_use("search", serde_json::json!({"q": "gold price"}))]);
        assert_eq!(a, b);
        assert_eq!(a, "search");

        // Order-independent and deduped.
        let multi = tool_names_sig(&[
            tool_use("b", serde_json::json!({})),
            tool_use("a", serde_json::json!({})),
            tool_use("a", serde_json::json!({})),
        ]);
        assert_eq!(multi, "a,b");

        // Non-tool blocks ignored.
        assert_eq!(
            tool_names_sig(&[ContentBlock::Text { text: "hi".into() }]),
            ""
        );
    }

    #[test]
    fn tool_call_sig_distinguishes_args_not_just_name() {
        let a = tool_call_sig(&tool_use("search", serde_json::json!({"q": "gold"})));
        let a2 = tool_use("search", serde_json::json!({"q": "gold"}));
        let b = tool_call_sig(&tool_use("search", serde_json::json!({"q": "silver"})));
        // Identical name+args → identical signature (caught as duplicate).
        assert_eq!(a, tool_call_sig(&a2));
        // Different args → different signature (allowed through).
        assert_ne!(a, b);
        // Non-tool block → no signature.
        assert!(tool_call_sig(&ContentBlock::Text { text: "x".into() }).is_none());
    }

    #[test]
    fn loop_guard_defaults_are_sane() {
        std::env::remove_var("SENCLAW_MAX_AGENT_TURNS");
        std::env::remove_var("SENCLAW_STALL_TOOL_TURNS");
        assert_eq!(max_agent_turns(), 30);
        assert_eq!(stall_tool_turns(), 4);
    }

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
            usage: None,
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
            usage: None,
        };
        let (text, reasoning, tools) = extract_content(&msg);
        assert!(text.is_empty());
        assert!(reasoning.is_empty());
        assert!(tools.is_empty());
    }

    #[test]
    fn count_tokens_falls_back_to_char_estimate() {
        let msgs = vec![Message {
            msg_type: "user".into(),
            message: MessagePayload {
                role: "user".into(),
                content: vec![ContentBlock::Text {
                    text: "Hello, world!".into(),
                }],
            },
            uuid: "uuid-1".into(),
            usage: None,
        }];
        let usage = count_tokens(&msgs, 128_000);
        // 13 chars / 4 ≈ 4 tokens, and max_tokens reflects the model context.
        assert!(usage.use_tokens >= 3);
        assert_eq!(usage.max_tokens, 128_000);
    }

    #[test]
    fn count_tokens_prefers_real_usage() {
        let mut assistant = Message {
            msg_type: "assistant".into(),
            message: MessagePayload {
                role: "assistant".into(),
                content: vec![ContentBlock::Text {
                    text: "short".into(),
                }],
            },
            uuid: "a1".into(),
            usage: Some(RawUsage {
                input_tokens: Some(1000),
                output_tokens: Some(200),
                cache_read_input_tokens: Some(50),
                ..Default::default()
            }),
        };
        // Anthropic shape: input includes cache tokens.
        let usage = count_tokens(std::slice::from_ref(&assistant), 200_000);
        assert_eq!(usage.prompt_tokens, 1050);
        assert_eq!(usage.use_tokens, 1250);
        assert_eq!(usage.max_tokens, 200_000);

        // OpenAI shape on a later message wins (most recent).
        assistant.usage = Some(RawUsage {
            prompt_tokens: Some(900),
            completion_tokens: Some(100),
            ..Default::default()
        });
        let usage = count_tokens(std::slice::from_ref(&assistant), 200_000);
        assert_eq!(usage.prompt_tokens, 900);
        assert_eq!(usage.use_tokens, 1000);
    }

    #[test]
    fn needs_auto_compact_respects_threshold() {
        let make = |input: u64| Message {
            msg_type: "assistant".into(),
            message: MessagePayload {
                role: "assistant".into(),
                content: vec![ContentBlock::Text { text: "x".into() }],
            },
            uuid: "a".into(),
            usage: Some(RawUsage {
                input_tokens: Some(input),
                output_tokens: Some(1),
                ..Default::default()
            }),
        };
        // Need >= 3 messages and input >= 75% of context.
        let below = vec![make(10), make(20), make(70_000)];
        assert!(!needs_auto_compact(&below, 128_000));
        let above = vec![make(10), make(20), make(100_000)];
        assert!(needs_auto_compact(&above, 128_000));
        // Too few messages never compacts.
        assert!(!needs_auto_compact(&[make(200_000)], 128_000));
    }
}
