//! Read tool — reads file contents with offset and limit support.
//!
//! Port of TS `node_modules/sema-core/dist/tools/Read/`.

use std::path::PathBuf;

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::Value;

use crate::zen_core::{Tool, ToolContext, ToolOutput, ToolResultMessage};

const MAX_LINE_LENGTH: usize = 2000;

pub struct ReadTool;

#[async_trait]
impl Tool for ReadTool {
    fn name(&self) -> &str {
        "Read"
    }

    fn description(&self) -> &str {
        "Read a file from the local filesystem"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Absolute path to the file to read"
                },
                "offset": {
                    "type": "integer",
                    "description": "Line number to start reading from (1-indexed)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Number of lines to read"
                }
            },
            "required": ["file_path"]
        })
    }

    fn is_read_only(&self) -> bool {
        true
    }

    async fn validate_input(
        &self,
        input: &Value,
        _ctx: &ToolContext<'_>,
    ) -> std::result::Result<(), String> {
        let path = input
            .get("file_path")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if path.is_empty() {
            return Err("file_path is required".to_string());
        }
        let p = PathBuf::from(path);
        if !p.exists() {
            return Err(format!("File not found: {path}"));
        }
        if !p.is_file() {
            return Err(format!("Not a file: {path}"));
        }
        Ok(())
    }

    async fn call(&self, input: Value, _ctx: &ToolContext<'_>) -> Result<Vec<ToolOutput>> {
        let path = input
            .get("file_path")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let offset = input
            .get("offset")
            .and_then(|v| v.as_u64())
            .unwrap_or(1)
            .max(1);
        let limit = input.get("limit").and_then(|v| v.as_u64());

        let content = std::fs::read_to_string(path).context("Failed to read file")?;

        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len() as u64;

        let start = (offset - 1) as usize;
        let end = limit.map_or(lines.len(), |l| (start + l as usize).min(lines.len()));

        if start >= lines.len() {
            return Ok(vec![ToolOutput::Result {
                data: serde_json::json!({
                    "content": "",
                    "lines": 0,
                    "totalLines": total_lines,
                }),
                result_for_assistant: format!(
                    "File has {total_lines} lines. Offset {offset} exceeds file length."
                ),
            }]);
        }

        let selected: Vec<&str> = lines[start..end].to_vec();
        let output = selected
            .iter()
            .map(|l| {
                if l.len() > MAX_LINE_LENGTH {
                    format!("{}... [line truncated]", &l[..MAX_LINE_LENGTH])
                } else {
                    l.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        Ok(vec![ToolOutput::Result {
            data: serde_json::json!({
                "content": output,
                "lines": selected.len(),
                "totalLines": total_lines,
            }),
            result_for_assistant: output,
        }])
    }

    fn gen_tool_result_message(&self, data: &Value, _input: &Value) -> ToolResultMessage {
        ToolResultMessage {
            title: "Read".into(),
            summary: format!(
                "{} lines",
                data.get("lines").and_then(|v| v.as_u64()).unwrap_or(0)
            ),
            content: data.clone(),
        }
    }

    fn get_display_title(&self, input: &Value) -> String {
        let path = input
            .get("file_path")
            .and_then(|v| v.as_str())
            .unwrap_or("file");
        let fname = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string());
        format!("Read {fname}")
    }
}
