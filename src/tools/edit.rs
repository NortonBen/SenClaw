//! Edit tool — exact string replacement in files.
//!
//! Port of TS `node_modules/sema-core/dist/tools/Edit/`.

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use serde_json::Value;

use crate::zen_core::{Tool, ToolContext, ToolOutput, ToolPermissionInfo, ToolResultMessage};

pub struct EditTool;

#[async_trait]
impl Tool for EditTool {
    fn name(&self) -> &str {
        "Edit"
    }

    fn description(&self) -> &str {
        "Perform exact string replacements in an existing file"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Absolute path to the file to modify"
                },
                "old_string": {
                    "type": "string",
                    "description": "The text to replace"
                },
                "new_string": {
                    "type": "string",
                    "description": "The text to replace it with (must be different from old_string)"
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Replace all occurrences of old_string (default false)"
                }
            },
            "required": ["file_path", "old_string", "new_string"]
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

        let old = input.get("old_string")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let new = input.get("new_string")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if old == new {
            return Err("old_string and new_string must be different".to_string());
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

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolContext<'_>,
    ) -> Result<Vec<ToolOutput>> {
        let path = input.get("file_path")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let old_string = input.get("old_string")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let new_string = input.get("new_string")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let replace_all = input.get("replace_all")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let p = PathBuf::from(path);
        let content = std::fs::read_to_string(&p)
            .context("Failed to read file")?;

        // Find the old_string
        let occurrences = content.match_indices(old_string).count();

        if occurrences == 0 {
            return Ok(vec![ToolOutput::Result {
                data: serde_json::json!({"error": true, "notFound": true}),
                result_for_assistant: format!(
                    "Error: old_string not found in {}. The file has not been modified.",
                    p.file_name().unwrap_or_default().to_string_lossy()
                ),
            }]);
        }

        if !replace_all && occurrences > 1 {
            return Ok(vec![ToolOutput::Result {
                data: serde_json::json!({"error": true, "multipleOccurrences": true, "count": occurrences}),
                result_for_assistant: format!(
                    "Error: old_string appears {occurrences} times in the file. \
                     Use replace_all=true to replace all, or provide a larger string \
                     with more surrounding context to make it unique."
                ),
            }]);
        }

        let new_content = if replace_all {
            content.replace(old_string, new_string)
        } else {
            let pos = content.find(old_string).unwrap();
            let mut result = String::with_capacity(content.len() + new_string.len() - old_string.len());
            result.push_str(&content[..pos]);
            result.push_str(new_string);
            result.push_str(&content[pos + old_string.len()..]);
            result
        };

        if new_content == content {
            bail!("old_string and new_string are identical — no change made");
        }

        std::fs::write(&p, &new_content)
            .context("Failed to write file")?;

        let fname = p.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string());
        let summary = format!(
            "Edited {fname} ({} replacement{})",
            occurrences,
            if occurrences > 1 { "s" } else { "" }
        );

        Ok(vec![ToolOutput::Result {
            data: serde_json::json!({
                "path": path,
                "replacements": occurrences,
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
            title: "Edit".into(),
            summary: format!("{} replacements", data.get("replacements").and_then(|v| v.as_u64()).unwrap_or(0)),
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
        format!("Edit {fname}")
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
