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

use super::*;

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
    /// Fire an engine event (provided by the engine for callback emission).
    pub fire: &'a (dyn Fn(EngineEvent) + Send + Sync),
    /// Permission checker instance.
    pub permission_checker: &'a dyn PermissionChecker,
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
            ctx.tools.iter().any(|t| t.name() == name && t.is_read_only())
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
        results.extend(run_single_tool(tu, cancel, ctx).await);
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

    // Find the tool
    let tool = match ctx.tools.iter().find(|t| t.name() == tool_name) {
        Some(t) => t.clone(),
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

    // Permission check for write tools
    if !tool.is_read_only() {
        if cancel.is_cancelled() {
            return vec![create_tool_result_stop(&tool_id)];
        }

        match ctx.permission_checker.check(tool.as_ref(), &input, cancel, ctx.agent_id).await {
            Ok(true) => {
                // Permission granted — proceed
            }
            Ok(false) => {
                // Permission denied
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
    match tool.call(input.clone(), &tool_ctx).await {
        Ok(outputs) => {
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

                        results.push(ContentBlock::ToolResult {
                            tool_use_id: tool_id.clone(),
                            content: result_for_assistant,
                            is_error: false,
                        });
                    }
                }
            }
            results
        }
        Err(e) => {
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
fn validate_tool_input(tool: &Arc<dyn Tool>, input: &serde_json::Value) -> std::result::Result<(), String> {
    let schema = tool.input_schema();

    // If schema is empty or just {}, skip validation
    if schema.is_null() || (schema.is_object() && schema.as_object().map_or(false, |o| o.is_empty())) {
        return Ok(());
    }

    // Use jsonschema crate for validation if available, otherwise basic check
    // For now: basic required-field check
    if let Some(required) = schema.get("required").and_then(|r| r.as_array()) {
        for field in required {
            if let Some(field_name) = field.as_str() {
                if input.get(field_name).is_none() || input.get(field_name) == Some(&serde_json::Value::Null) {
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

    struct TestReadTool;
    #[async_trait::async_trait]
    impl Tool for TestReadTool {
        fn name(&self) -> &str { "read" }
        fn description(&self) -> &str { "Read a file" }
        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"}
                },
                "required": ["path"]
            })
        }
        fn is_read_only(&self) -> bool { true }
        async fn call(&self, _input: serde_json::Value, _ctx: &ToolContext<'_>) -> Result<Vec<ToolOutput>> {
            Ok(vec![ToolOutput::Result {
                data: serde_json::json!({"content": "hello"}),
                result_for_assistant: "hello".into(),
            }])
        }
        fn gen_tool_result_message(&self, _data: &serde_json::Value, _input: &serde_json::Value) -> ToolResultMessage {
            ToolResultMessage { title: "Read".into(), summary: "Read file".into(), content: serde_json::json!({}) }
        }
        fn get_display_title(&self, _input: &serde_json::Value) -> String { "Read file".into() }
    }

    struct TestWriteTool;
    #[async_trait::async_trait]
    impl Tool for TestWriteTool {
        fn name(&self) -> &str { "write" }
        fn description(&self) -> &str { "Write a file" }
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
        fn is_read_only(&self) -> bool { false }
        async fn call(&self, _input: serde_json::Value, _ctx: &ToolContext<'_>) -> Result<Vec<ToolOutput>> {
            Ok(vec![ToolOutput::Result {
                data: serde_json::json!({"written": true}),
                result_for_assistant: "written".into(),
            }])
        }
        fn gen_tool_result_message(&self, _data: &serde_json::Value, _input: &serde_json::Value) -> ToolResultMessage {
            ToolResultMessage { title: "Write".into(), summary: "Wrote file".into(), content: serde_json::json!({}) }
        }
        fn get_display_title(&self, _input: &serde_json::Value) -> String { "Write file".into() }
    }

    fn test_ctx<'a>(tools: &'a [Arc<dyn Tool>], checker: &'a dyn PermissionChecker) -> RunContext<'a> {
        RunContext {
            agent_id: "main",
            working_dir: "/tmp",
            agent_data_dir: "/tmp",
            tools,
            fire: &|_| {},
            permission_checker: checker,
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
        ).await;

        assert_eq!(results.len(), 1);
        if let ContentBlock::ToolResult { content, is_error, .. } = &results[0] {
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
        ).await;

        assert_eq!(results.len(), 1);
        if let ContentBlock::ToolResult { content, is_error, .. } = &results[0] {
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
        ).await;

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
        ).await;

        assert_eq!(results.len(), 1);
        if let ContentBlock::ToolResult { content, .. } = &results[0] {
            assert!(content.contains("interrupted"));
        } else {
            panic!("Expected ToolResult stop");
        }
    }
}
