//! Bridge between MCP external tools and zen-core Tool trait.
//!
//! Each external MCP tool (from a user-configured MCP server) is wrapped in
//! an `McpBridgeTool` so it can participate in the agent's tool roster.

use std::sync::Arc;

use anyhow::Result;
use serde_json::Value;

use crate::zen_core::{Tool, ToolContext, ToolOutput, ToolResultMessage};

use super::manager::McpManager;

/// A single external MCP tool, adapted to the zen-core `Tool` trait.
pub struct McpBridgeTool {
    /// Full `mcp__<server>__<tool>` name.
    pub full_name: String,
    /// Short tool name (without mcp prefix).
    pub display_name: String,
    /// Human-readable description.
    pub desc: String,
    /// JSON Schema for input parameters (may be Null).
    pub schema: Value,
    /// Reference to the manager for call dispatch.
    pub manager: Arc<McpManager>,
}

impl McpBridgeTool {
    /// Create bridge tools for all currently-connected external MCP servers.
    pub async fn from_manager(manager: &Arc<McpManager>) -> Vec<Arc<dyn Tool>> {
        let infos = manager.get_external_tools_full().await;
        infos
            .into_iter()
            .map(|info| {
                let display_name = info.name.clone();
                Arc::new(Self {
                    full_name: info.name,
                    display_name,
                    desc: info.description,
                    schema: info.input_schema,
                    manager: Arc::clone(manager),
                }) as Arc<dyn Tool>
            })
            .collect()
    }
}

#[async_trait::async_trait]
impl Tool for McpBridgeTool {
    fn name(&self) -> &str {
        &self.full_name
    }

    fn description(&self) -> &str {
        &self.desc
    }

    fn input_schema(&self) -> Value {
        self.schema.clone()
    }

    fn is_read_only(&self) -> bool {
        false
    }

    async fn call(&self, input: Value, _ctx: &ToolContext<'_>) -> Result<Vec<ToolOutput>> {
        let result = self
            .manager
            .call_external_tool(&self.full_name, input)
            .await?;
        let summary = match &result {
            Value::String(s) => s.clone(),
            other => serde_json::to_string_pretty(other).unwrap_or_else(|_| format!("{other:?}")),
        };
        Ok(vec![ToolOutput::Result {
            data: result,
            result_for_assistant: summary,
        }])
    }

    fn gen_tool_result_message(&self, data: &Value, _input: &Value) -> ToolResultMessage {
        let summary = match data {
            Value::String(s) => s.clone(),
            other => serde_json::to_string_pretty(other).unwrap_or_else(|_| format!("{other:?}")),
        };
        ToolResultMessage {
            title: self.display_name.clone(),
            summary,
            content: data.clone(),
        }
    }

    fn get_display_title(&self, _input: &Value) -> String {
        self.display_name.clone()
    }
}
