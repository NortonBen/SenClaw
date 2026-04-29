//! Write tool — creates or overwrites a file.
//!
//! Port of TS `node_modules/sema-core/dist/tools/Write/`.

use std::path::PathBuf;

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::Value;

use crate::zen_core::{Tool, ToolContext, ToolOutput, ToolPermissionInfo, ToolResultMessage};

pub struct WriteTool;

#[async_trait]
impl Tool for WriteTool {
    fn name(&self) -> &str {
        "Write"
    }

    fn description(&self) -> &str {
        "Write a file to the local filesystem"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Absolute path to the file to write"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write to the file"
                }
            },
            "required": ["file_path", "content"]
        })
    }

    fn is_read_only(&self) -> bool {
        false
    }

    async fn validate_input(
        &self,
        input: &Value,
        _ctx: &ToolContext<'_>,
    ) -> std::result::Result<(), String> {
        let path = input.get("file_path")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if path.is_empty() {
            return Err("file_path is required".to_string());
        }
        Ok(())
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolContext<'_>,
    ) -> Result<Vec<ToolOutput>> {
        let path = input.get("file_path")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let content = input.get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let p = PathBuf::from(path);

        // Ensure parent directory exists
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)
                .context("Failed to create parent directory")?;
        }

        std::fs::write(&p, content)
            .context("Failed to write file")?;

        let fname = p.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string());
        let size = content.len();
        let summary = format!("Wrote {fname} ({size} bytes)");

        Ok(vec![ToolOutput::Result {
            data: serde_json::json!({
                "path": path,
                "size": size,
            }),
            result_for_assistant: summary.clone(),
        }])
    }

    fn gen_tool_result_message(
        &self,
        data: &Value,
        _input: &Value,
    ) -> ToolResultMessage {
        ToolResultMessage {
            title: "Write".into(),
            summary: format!("{} bytes", data.get("size").and_then(|v| v.as_u64()).unwrap_or(0)),
            content: data.clone(),
        }
    }

    fn get_display_title(&self, input: &Value) -> String {
        let path = input.get("file_path")
            .and_then(|v| v.as_str())
            .unwrap_or("file");
        let fname = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string());
        format!("Write {fname}")
    }

    fn gen_tool_permission(&self, input: &Value) -> Option<ToolPermissionInfo> {
        let title = self.get_display_title(input);
        let path = input.get("file_path")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        Some(ToolPermissionInfo {
            title,
            content: serde_json::json!({
                "path": path,
            }),
        })
    }
}
