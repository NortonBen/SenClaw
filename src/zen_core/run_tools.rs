//! Tool execution engine.
//!
//! Validates input, checks permissions, and executes tools (concurrently
//! for read-only tools, serially for write tools).
//!
//! Port of TS `RunTools.ts`.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use super::hooks::{
    self as zen_hooks, ExecuteHooksOptions, HookEvent, HookInput, HookInputBase, HookManager,
    OutputFilterInput, PermissionRequestInput, PostToolUseInput, PrePermissionInput,
    PreToolUseInput,
};
use super::*;

/// Outcome of running the `PrePermission` hook chain. `Passthrough` means
/// no hook expressed an opinion, so the normal user-prompted permission
/// flow should run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrePermissionDecision {
    Allow,
    Deny,
    Passthrough,
}

/// Map an `AggregatedHookResult` from a PrePermission hook chain into a
/// concrete decision. Deny wins over Allow when both signals are present
/// in the same chain (safer default for ambiguous configs).
pub fn classify_pre_permission(
    res: &super::hooks::AggregatedHookResult,
) -> PrePermissionDecision {
    if res.blocked || res.abort {
        return PrePermissionDecision::Deny;
    }
    if res.allow {
        return PrePermissionDecision::Allow;
    }
    PrePermissionDecision::Passthrough
}

/// Abstract permission checker — injected by the engine so RunTools doesn't
/// need to know about PermissionManager internals.
#[async_trait]
pub trait PermissionChecker: Send + Sync {
    /// Returns `Ok(true)` if tool execution is allowed, `Ok(false)` if denied,
    /// or `Err(...)` if the check itself failed (allow by default).
    async fn check(
        &self,
        tool: &dyn Tool,
        input: &serde_json::Value,
        cancel: &CancellationToken,
        agent_id: &str,
    ) -> Result<bool>;
}

/// A no-op checker that allows everything (used when permissions are disabled).
pub struct AllowAllPermissions;

#[async_trait]
impl PermissionChecker for AllowAllPermissions {
    async fn check(
        &self,
        _tool: &dyn Tool,
        _input: &serde_json::Value,
        _cancel: &CancellationToken,
        _agent_id: &str,
    ) -> Result<bool> {
        Ok(true)
    }
}

/// Context passed through the tool execution pipeline.
pub struct RunContext<'a> {
    pub agent_id: &'a str,
    pub working_dir: &'a str,
    pub agent_data_dir: &'a str,
    pub tools: &'a [Arc<dyn Tool>],
    /// Re-fetch the live tool list (includes ToolSearch discoveries). When
    /// set, serial execution refreshes after `ToolSearch` so deferred tools
    /// can be called in the same assistant turn.
    pub tools_resolver: Option<&'a (dyn Fn() -> Vec<Arc<dyn Tool>> + Send + Sync)>,
    /// Fire an engine event (provided by the engine for callback emission).
    pub fire: &'a (dyn Fn(EngineEvent) + Send + Sync),
    /// Permission checker instance.
    pub permission_checker: &'a dyn PermissionChecker,
    /// Event bus for tools that need it (AskUser, etc.).
    pub event_bus: Option<&'a EventBus>,
    /// Response registry for tools that need request-response (AskUser, etc.).
    pub response_registry: Option<&'a ResponseRegistry>,
    /// Hook manager for PreToolUse / PostToolUse hooks (optional).
    pub hook_manager: Option<Arc<HookManager>>,
    /// HTTP client passed to prompt hooks.
    pub hook_client: Option<reqwest::Client>,
    /// Model profile passed to prompt hooks.
    pub hook_profile: Option<ModelProfile>,
    /// Session id for hook base payload.
    pub session_id: String,
}

// ============================================================================
// Public entry points
// ============================================================================

/// Execute a list of tool_use blocks from an assistant message.
/// Read-only tools run concurrently; write tools run serially.
pub async fn run_tools(
    tool_uses: &[ContentBlock],
    cancel: &CancellationToken,
    ctx: &RunContext<'_>,
) -> Vec<ContentBlock> {
    // Determine if all tools are read-only
    let all_read_only = tool_uses.iter().all(|tu| {
        if let ContentBlock::ToolUse { name, .. } = tu {
            ctx.tools
                .iter()
                .any(|t| t.name() == name && t.is_read_only())
        } else {
            false
        }
    });

    if all_read_only && tool_uses.len() > 1 {
        run_concurrently(tool_uses, cancel, ctx).await
    } else {
        run_serially(tool_uses, cancel, ctx).await
    }
}

async fn run_concurrently(
    tool_uses: &[ContentBlock],
    cancel: &CancellationToken,
    ctx: &RunContext<'_>,
) -> Vec<ContentBlock> {
    let futures: Vec<_> = tool_uses
        .iter()
        .map(|tu| run_single_tool(tu, cancel, ctx))
        .collect();

    let results = futures::future::join_all(futures).await;

    // Flatten — each future returns a Vec<ContentBlock> (typically 1)
    let mut output = Vec::new();
    for group in results {
        output.extend(group);
    }
    output
}

async fn run_serially(
    tool_uses: &[ContentBlock],
    cancel: &CancellationToken,
    ctx: &RunContext<'_>,
) -> Vec<ContentBlock> {
    let mut results = Vec::new();
    let mut active_tools: Vec<Arc<dyn Tool>> = ctx.tools.to_vec();
    for tu in tool_uses {
        if cancel.is_cancelled() {
            // Generate stop messages for remaining tools
            for remaining in tool_uses.iter().skip(results.len()) {
                if let ContentBlock::ToolUse { id, .. } = remaining {
                    results.push(create_tool_result_stop(id));
                }
            }
            break;
        }
        let dynamic_ctx = RunContext {
            agent_id: ctx.agent_id,
            working_dir: ctx.working_dir,
            agent_data_dir: ctx.agent_data_dir,
            tools: &active_tools,
            tools_resolver: ctx.tools_resolver,
            fire: ctx.fire,
            permission_checker: ctx.permission_checker,
            event_bus: ctx.event_bus,
            response_registry: ctx.response_registry,
            hook_manager: ctx.hook_manager.clone(),
            hook_client: ctx.hook_client.clone(),
            hook_profile: ctx.hook_profile.clone(),
            session_id: ctx.session_id.clone(),
        };
        results.extend(run_single_tool(tu, cancel, &dynamic_ctx).await);

        if let ContentBlock::ToolUse { name, .. } = tu {
            if name == "ToolSearch" {
                if let Some(resolver) = ctx.tools_resolver {
                    active_tools = resolver();
                }
            }
        }
    }
    results
}

// ============================================================================
// Single tool execution
// ============================================================================

async fn run_single_tool(
    tool_use: &ContentBlock,
    cancel: &CancellationToken,
    ctx: &RunContext<'_>,
) -> Vec<ContentBlock> {
    let (tool_name, tool_id, input) = match tool_use {
        ContentBlock::ToolUse { name, id, input } => (name.clone(), id.clone(), input.clone()),
        _ => return vec![],
    };

    // Find the tool — try active set, then refresh from resolver (ToolSearch
    // may have loaded deferred tools earlier in this serial batch).
    let tool = crate::tools::tool_search::resolve_tool_by_name(&tool_name, ctx.tools).or_else(|| {
        ctx.tools_resolver.map(|resolver| {
            let fresh = resolver();
            crate::tools::tool_search::resolve_tool_by_name(&tool_name, &fresh)
        })?
    });

    let tool = match tool {
        Some(t) => t,
        None => {
            let error_msg = format!("Error: No such tool available: {tool_name}");
            (ctx.fire)(EngineEvent::ToolExecutionError(ToolExecutionErrorData {
                agent_id: ctx.agent_id.to_string(),
                tool_name: tool_name.clone(),
                title: tool_name.clone(),
                content: error_msg.clone(),
            }));
            return vec![ContentBlock::ToolResult {
                tool_use_id: tool_id,
                content: error_msg,
                is_error: true,
            }];
        }
    };

    // Checkpoint: cancelled before starting
    if cancel.is_cancelled() {
        tracing::info!(
            "[RunTools] skipped cancelled tool agent={} tool={} id={}",
            ctx.agent_id,
            tool_name,
            tool_id
        );
        return vec![create_tool_result_stop(&tool_id)];
    }

    // Validate input schema (basic JSON schema check)
    if let Err(validation_err) = validate_tool_input(&tool, &input) {
        (ctx.fire)(EngineEvent::ToolExecutionError(ToolExecutionErrorData {
            agent_id: ctx.agent_id.to_string(),
            tool_name: tool_name.clone(),
            title: tool.get_display_title(&input),
            content: validation_err.clone(),
        }));
        return vec![ContentBlock::ToolResult {
            tool_use_id: tool_id,
            content: validation_err,
            is_error: true,
        }];
    }

    // Custom validate_input
    let tool_ctx = ToolContext {
        agent_id: ctx.agent_id,
        working_dir: ctx.working_dir,
        agent_data_dir: ctx.agent_data_dir,
        abort: cancel.clone(),
        event_bus: ctx.event_bus,
        response_registry: ctx.response_registry,
    };
    if let Err(validation_msg) = tool.validate_input(&input, &tool_ctx).await {
        (ctx.fire)(EngineEvent::ToolExecutionError(ToolExecutionErrorData {
            agent_id: ctx.agent_id.to_string(),
            tool_name: tool_name.clone(),
            title: tool.get_display_title(&input),
            content: validation_msg.clone(),
        }));
        return vec![ContentBlock::ToolResult {
            tool_use_id: tool_id,
            content: validation_msg,
            is_error: true,
        }];
    }

    // PreToolUse hook — may block the tool or update its input
    let input = if let Some(ref hm) = ctx.hook_manager {
        if hm.has_hooks_for_event(&HookEvent::PreToolUse) {
            let base = HookInputBase {
                hook_event_name: HookEvent::PreToolUse,
                session_id: ctx.session_id.clone(),
                agent_id: ctx.agent_id.to_string(),
                timestamp: chrono::Utc::now().to_rfc3339(),
                cwd: ctx.working_dir.to_string(),
            };
            let hook_input = HookInput::PreToolUse(PreToolUseInput {
                base,
                tool_name: tool_name.clone(),
                tool_input: input.clone(),
            });
            let result = zen_hooks::execute_hooks(
                hm,
                &HookEvent::PreToolUse,
                &hook_input,
                &ExecuteHooksOptions {
                    client: ctx.hook_client.as_ref(),
                    profile: ctx.hook_profile.as_ref(),
                    ..Default::default()
                },
            )
            .await;

            if result.blocked {
                let reason = result.reason.unwrap_or_else(|| "Blocked by hook".into());
                (ctx.fire)(EngineEvent::ToolExecutionError(ToolExecutionErrorData {
                    agent_id: ctx.agent_id.to_string(),
                    tool_name: tool_name.clone(),
                    title: tool.get_display_title(&input),
                    content: reason.clone(),
                }));
                return vec![ContentBlock::ToolResult {
                    tool_use_id: tool_id,
                    content: reason,
                    is_error: true,
                }];
            }

            // Hook may supply updated input
            result.updated_input.unwrap_or(input)
        } else {
            input
        }
    } else {
        input
    };

    // Permission check for write tools
    if !tool.is_read_only() {
        if cancel.is_cancelled() {
            tracing::info!(
                "[RunTools] skipped cancelled write tool agent={} tool={} id={}",
                ctx.agent_id,
                tool_name,
                tool_id
            );
            return vec![create_tool_result_stop(&tool_id)];
        }

        // PrePermission hook — runs synchronously so it can short-circuit
        // the user prompt entirely. `decision: "allow"` skips the prompt
        // and grants the tool; blocked/`decision: "reject"` denies the
        // tool without bothering the user; otherwise we fall through to
        // the normal permission flow.
        let pre_perm_decision: PrePermissionDecision =
            if let Some(ref hm) = ctx.hook_manager {
                if hm.has_hooks_for_event(&HookEvent::PrePermission) {
                    let base = HookInputBase {
                        hook_event_name: HookEvent::PrePermission,
                        session_id: ctx.session_id.clone(),
                        agent_id: ctx.agent_id.to_string(),
                        timestamp: chrono::Utc::now().to_rfc3339(),
                        cwd: ctx.working_dir.to_string(),
                    };
                    let input_for_hook = HookInput::PrePermission(PrePermissionInput {
                        base,
                        tool_name: tool_name.clone(),
                        tool_input: input.clone(),
                    });
                    let res = zen_hooks::execute_hooks(
                        hm,
                        &HookEvent::PrePermission,
                        &input_for_hook,
                        &ExecuteHooksOptions {
                            client: ctx.hook_client.as_ref(),
                            profile: ctx.hook_profile.as_ref(),
                            ..Default::default()
                        },
                    )
                    .await;
                    classify_pre_permission(&res)
                } else {
                    PrePermissionDecision::Passthrough
                }
            } else {
                PrePermissionDecision::Passthrough
            };

        // Short-circuit on allow/deny; otherwise continue to the user prompt.
        let permission_result: Result<bool> = match pre_perm_decision {
            PrePermissionDecision::Allow => {
                tracing::info!(
                    "[RunTools] PrePermission hook allowed tool={} id={}",
                    tool_name, tool_id
                );
                Ok(true)
            }
            PrePermissionDecision::Deny => {
                tracing::warn!(
                    "[RunTools] PrePermission hook denied tool={} id={}",
                    tool_name, tool_id
                );
                Ok(false)
            }
            PrePermissionDecision::Passthrough => {
                tracing::info!(
                    "[RunTools] permission check agent={} tool={} id={}",
                    ctx.agent_id,
                    tool_name,
                    tool_id
                );
                ctx.permission_checker
                    .check(tool.as_ref(), &input, cancel, ctx.agent_id)
                    .await
            }
        };
        match permission_result {
            Ok(true) => {
                // Permission granted — proceed
                tracing::info!(
                    "[RunTools] permission granted agent={} tool={} id={}",
                    ctx.agent_id,
                    tool_name,
                    tool_id
                );

                // Fire PermissionRequest hook
                if let Some(ref hm) = ctx.hook_manager {
                    if hm.has_hooks_for_event(&HookEvent::PermissionRequest) {
                        let base = HookInputBase {
                            hook_event_name: HookEvent::PermissionRequest,
                            session_id: ctx.session_id.clone(),
                            agent_id: ctx.agent_id.to_string(),
                            timestamp: chrono::Utc::now().to_rfc3339(),
                            cwd: ctx.working_dir.to_string(),
                        };
                        let hook_input = HookInput::PermissionRequest(PermissionRequestInput {
                            base,
                            tool_name: tool_name.clone(),
                            tool_input: input.clone(),
                        });
                        let (client, profile) = (ctx.hook_client.clone(), ctx.hook_profile.clone());
                        let hm_clone = hm.clone();
                        tokio::spawn(async move {
                            let _ = zen_hooks::execute_hooks(
                                &hm_clone,
                                &HookEvent::PermissionRequest,
                                &hook_input,
                                &ExecuteHooksOptions {
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
            Ok(false) => {
                // Permission denied
                tracing::warn!(
                    "[RunTools] permission denied agent={} tool={} id={}",
                    ctx.agent_id,
                    tool_name,
                    tool_id
                );
                let msg = "Tool execution was cancelled by user.".to_string();
                return vec![ContentBlock::ToolResult {
                    tool_use_id: tool_id,
                    content: msg,
                    is_error: true,
                }];
            }
            Err(_) => {
                // Permission check error — allow by default in case of errors
                warn!("Permission check error for {tool_name}, allowing by default");
            }
        }
    }

    // Execute the tool
    tracing::info!(
        "[RunTools] start agent={} tool={} id={} read_only={}",
        ctx.agent_id,
        tool_name,
        tool_id,
        tool.is_read_only()
    );
    match tool.call(input.clone(), &tool_ctx).await {
        Ok(outputs) => {
            tracing::info!(
                "[RunTools] complete agent={} tool={} id={} outputs={}",
                ctx.agent_id,
                tool_name,
                tool_id,
                outputs.len()
            );
            let mut results = Vec::new();
            for output in outputs {
                match output {
                    ToolOutput::Progress { message } => {
                        debug!("[{tool_name}] progress: {message}");
                    }
                    ToolOutput::Result {
                        data,
                        result_for_assistant,
                    } => {
                        // OutputFilter hook — last chance to redact / truncate
                        // the structured tool output before it reaches the
                        // chat UI and the engine context.
                        let mut data = data;
                        let mut result_for_assistant = result_for_assistant;
                        if let Some(ref hm) = ctx.hook_manager {
                            if hm.has_hooks_for_event(&HookEvent::OutputFilter) {
                                let base = HookInputBase {
                                    hook_event_name: HookEvent::OutputFilter,
                                    session_id: ctx.session_id.clone(),
                                    agent_id: ctx.agent_id.to_string(),
                                    timestamp: chrono::Utc::now().to_rfc3339(),
                                    cwd: ctx.working_dir.to_string(),
                                };
                                let res = zen_hooks::execute_hooks(
                                    hm,
                                    &HookEvent::OutputFilter,
                                    &HookInput::OutputFilter(OutputFilterInput {
                                        base,
                                        tool_name: tool_name.clone(),
                                        tool_input: input.clone(),
                                        tool_output: data.clone(),
                                    }),
                                    &ExecuteHooksOptions {
                                        client: ctx.hook_client.as_ref(),
                                        profile: ctx.hook_profile.as_ref(),
                                        ..Default::default()
                                    },
                                )
                                .await;
                                if let Some(new_out) = res.updated_output {
                                    // Mirror the replacement into the
                                    // assistant-facing text too.
                                    if let Some(s) = new_out.as_str() {
                                        result_for_assistant = s.to_string();
                                    } else {
                                        result_for_assistant =
                                            serde_json::to_string(&new_out)
                                                .unwrap_or(result_for_assistant);
                                    }
                                    data = new_out;
                                }
                            }
                        }

                        // Emit tool:execution:complete
                        let msg = tool.gen_tool_result_message(&data, &input);
                        (ctx.fire)(EngineEvent::ToolExecutionComplete(
                            ToolExecutionCompleteData {
                                agent_id: ctx.agent_id.to_string(),
                                tool_name: tool_name.clone(),
                                title: msg.title,
                                summary: msg.summary,
                                content: msg.content,
                            },
                        ));

                        // PostToolUse hook (non-blockable, fire-and-forget semantics ok)
                        if let Some(ref hm) = ctx.hook_manager {
                            if hm.has_hooks_for_event(&HookEvent::PostToolUse) {
                                let base = HookInputBase {
                                    hook_event_name: HookEvent::PostToolUse,
                                    session_id: ctx.session_id.clone(),
                                    agent_id: ctx.agent_id.to_string(),
                                    timestamp: chrono::Utc::now().to_rfc3339(),
                                    cwd: ctx.working_dir.to_string(),
                                };
                                zen_hooks::execute_hooks(
                                    hm,
                                    &HookEvent::PostToolUse,
                                    &HookInput::PostToolUse(PostToolUseInput {
                                        base,
                                        tool_name: tool_name.clone(),
                                        tool_input: input.clone(),
                                        tool_response: data.clone(),
                                    }),
                                    &ExecuteHooksOptions {
                                        client: ctx.hook_client.as_ref(),
                                        profile: ctx.hook_profile.as_ref(),
                                        ..Default::default()
                                    },
                                )
                                .await;
                            }
                        }

                        results.push(ContentBlock::ToolResult {
                            tool_use_id: tool_id.clone(),
                            content: result_for_assistant,
                            is_error: false,
                        });
                    }
                    ToolOutput::ClearContextAndStart {
                        plan_file_path,
                        plan_content,
                    } => {
                        (ctx.fire)(EngineEvent::ToolExecutionComplete(
                            ToolExecutionCompleteData {
                                agent_id: ctx.agent_id.to_string(),
                                tool_name: tool_name.clone(),
                                title: "ExitPlanMode".to_string(),
                                summary: "clearContextAndStart".to_string(),
                                content: serde_json::json!({
                                    "planFilePath": plan_file_path,
                                    "selected": "clearContextAndStart"
                                }),
                            },
                        ));
                        results.push(ContentBlock::ControlSignal {
                            signal_type: "ClearContextAndStart".to_string(),
                            payload: serde_json::json!({
                                "plan_file_path": plan_file_path,
                                "plan_content": plan_content
                            }),
                        });
                    }
                }
            }
            results
        }
        Err(e) => {
            tracing::warn!(
                "[RunTools] error agent={} tool={} id={}: {e}",
                ctx.agent_id,
                tool_name,
                tool_id
            );
            let error_msg = format!("Tool execution failed: {e}");
            (ctx.fire)(EngineEvent::ToolExecutionError(ToolExecutionErrorData {
                agent_id: ctx.agent_id.to_string(),
                tool_name: tool_name.clone(),
                title: tool.get_display_title(&input),
                content: error_msg.clone(),
            }));
            vec![ContentBlock::ToolResult {
                tool_use_id: tool_id,
                content: error_msg,
                is_error: true,
            }]
        }
    }
}

// ============================================================================
// Input validation
// ============================================================================

/// Validate tool input against the tool's JSON Schema.
fn validate_tool_input(
    tool: &Arc<dyn Tool>,
    input: &serde_json::Value,
) -> std::result::Result<(), String> {
    let schema = tool.input_schema();

    // If schema is empty or just {}, skip validation
    if schema.is_null()
        || (schema.is_object() && schema.as_object().map_or(false, |o| o.is_empty()))
    {
        return Ok(());
    }

    // Use jsonschema crate for validation if available, otherwise basic check
    // For now: basic required-field check
    if let Some(required) = schema.get("required").and_then(|r| r.as_array()) {
        for field in required {
            if let Some(field_name) = field.as_str() {
                if input.get(field_name).is_none()
                    || input.get(field_name) == Some(&serde_json::Value::Null)
                {
                    return Err(format!("Missing required field: {field_name}"));
                }
            }
        }
    }

    Ok(())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zen_core::hooks::AggregatedHookResult;

    fn empty_aggr() -> AggregatedHookResult {
        AggregatedHookResult::empty()
    }

    #[test]
    fn classify_pre_permission_no_signals_is_passthrough() {
        assert_eq!(classify_pre_permission(&empty_aggr()), PrePermissionDecision::Passthrough);
    }

    #[test]
    fn classify_pre_permission_allow_flag_is_allow() {
        let mut a = empty_aggr();
        a.allow = true;
        assert_eq!(classify_pre_permission(&a), PrePermissionDecision::Allow);
    }

    #[test]
    fn classify_pre_permission_blocked_overrides_allow() {
        let mut a = empty_aggr();
        a.allow = true;
        a.blocked = true;
        assert_eq!(classify_pre_permission(&a), PrePermissionDecision::Deny);
    }

    #[test]
    fn classify_pre_permission_abort_is_deny() {
        let mut a = empty_aggr();
        a.abort = true;
        assert_eq!(classify_pre_permission(&a), PrePermissionDecision::Deny);
    }

    struct TestReadTool;
    #[async_trait::async_trait]
    impl Tool for TestReadTool {
        fn name(&self) -> &str {
            "read"
        }
        fn description(&self) -> &str {
            "Read a file"
        }
        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"}
                },
                "required": ["path"]
            })
        }
        fn is_read_only(&self) -> bool {
            true
        }
        async fn call(
            &self,
            _input: serde_json::Value,
            _ctx: &ToolContext<'_>,
        ) -> Result<Vec<ToolOutput>> {
            Ok(vec![ToolOutput::Result {
                data: serde_json::json!({"content": "hello"}),
                result_for_assistant: "hello".into(),
            }])
        }
        fn gen_tool_result_message(
            &self,
            _data: &serde_json::Value,
            _input: &serde_json::Value,
        ) -> ToolResultMessage {
            ToolResultMessage {
                title: "Read".into(),
                summary: "Read file".into(),
                content: serde_json::json!({}),
            }
        }
        fn get_display_title(&self, _input: &serde_json::Value) -> String {
            "Read file".into()
        }
    }

    struct TestWriteTool;
    #[async_trait::async_trait]
    impl Tool for TestWriteTool {
        fn name(&self) -> &str {
            "write"
        }
        fn description(&self) -> &str {
            "Write a file"
        }
        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "content": {"type": "string"}
                },
                "required": ["path", "content"]
            })
        }
        fn is_read_only(&self) -> bool {
            false
        }
        async fn call(
            &self,
            _input: serde_json::Value,
            _ctx: &ToolContext<'_>,
        ) -> Result<Vec<ToolOutput>> {
            Ok(vec![ToolOutput::Result {
                data: serde_json::json!({"written": true}),
                result_for_assistant: "written".into(),
            }])
        }
        fn gen_tool_result_message(
            &self,
            _data: &serde_json::Value,
            _input: &serde_json::Value,
        ) -> ToolResultMessage {
            ToolResultMessage {
                title: "Write".into(),
                summary: "Wrote file".into(),
                content: serde_json::json!({}),
            }
        }
        fn get_display_title(&self, _input: &serde_json::Value) -> String {
            "Write file".into()
        }
    }

    fn test_ctx<'a>(
        tools: &'a [Arc<dyn Tool>],
        checker: &'a dyn PermissionChecker,
    ) -> RunContext<'a> {
        RunContext {
            agent_id: "main",
            working_dir: "/tmp",
            agent_data_dir: "/tmp",
            tools,
            tools_resolver: None,
            fire: &|_| {},
            permission_checker: checker,
            event_bus: None,
            response_registry: None,
            hook_manager: None,
            hook_client: None,
            hook_profile: None,
            session_id: String::new(),
        }
    }

    #[tokio::test]
    async fn run_readonly_tool_succeeds() {
        let tool: Arc<dyn Tool> = Arc::new(TestReadTool);
        let tools = vec![tool];
        let ctx = test_ctx(&tools, &AllowAllPermissions);
        let cancel = CancellationToken::new();

        let results = run_single_tool(
            &ContentBlock::ToolUse {
                id: "tu-1".into(),
                name: "read".into(),
                input: serde_json::json!({"path": "/tmp/test"}),
            },
            &cancel,
            &ctx,
        )
        .await;

        assert_eq!(results.len(), 1);
        if let ContentBlock::ToolResult {
            content, is_error, ..
        } = &results[0]
        {
            assert!(!is_error);
            assert_eq!(content, "hello");
        } else {
            panic!("Expected ToolResult");
        }
    }

    #[tokio::test]
    async fn unknown_tool_returns_error() {
        let tools: Vec<Arc<dyn Tool>> = vec![];
        let ctx = test_ctx(&tools, &AllowAllPermissions);
        let cancel = CancellationToken::new();

        let results = run_single_tool(
            &ContentBlock::ToolUse {
                id: "tu-1".into(),
                name: "nonexistent".into(),
                input: serde_json::json!({}),
            },
            &cancel,
            &ctx,
        )
        .await;

        assert_eq!(results.len(), 1);
        if let ContentBlock::ToolResult {
            content, is_error, ..
        } = &results[0]
        {
            assert!(*is_error);
            assert!(content.contains("No such tool"));
        } else {
            panic!("Expected ToolResult");
        }
    }

    #[tokio::test]
    async fn validation_fails_on_missing_required() {
        let tool: Arc<dyn Tool> = Arc::new(TestReadTool);
        let tools = vec![tool.clone()];
        let ctx = test_ctx(&tools, &AllowAllPermissions);
        let cancel = CancellationToken::new();

        let results = run_single_tool(
            &ContentBlock::ToolUse {
                id: "tu-1".into(),
                name: "read".into(),
                input: serde_json::json!({}),
            },
            &cancel,
            &ctx,
        )
        .await;

        assert_eq!(results.len(), 1);
        if let ContentBlock::ToolResult { is_error, .. } = &results[0] {
            assert!(*is_error);
        } else {
            panic!("Expected ToolResult error");
        }
    }

    #[tokio::test]
    async fn cancelled_before_tool_returns_stop() {
        let tool: Arc<dyn Tool> = Arc::new(TestReadTool);
        let tools = vec![tool];
        let ctx = test_ctx(&tools, &AllowAllPermissions);
        let cancel = CancellationToken::new();
        cancel.cancel(); // Cancel immediately

        let results = run_single_tool(
            &ContentBlock::ToolUse {
                id: "tu-1".into(),
                name: "read".into(),
                input: serde_json::json!({"path": "/tmp/test"}),
            },
            &cancel,
            &ctx,
        )
        .await;

        assert_eq!(results.len(), 1);
        if let ContentBlock::ToolResult { content, .. } = &results[0] {
            assert!(content.contains("interrupted"));
        } else {
            panic!("Expected ToolResult stop");
        }
    }
}
