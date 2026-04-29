//! Glob tool — file pattern matching.
//!
//! Port of TS `node_modules/sema-core/dist/tools/Glob/`.

use std::path::{Path, PathBuf};

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use crate::zen_core::{Tool, ToolContext, ToolOutput, ToolResultMessage};

const MAX_RESULTS: usize = 100;
const MAX_DISPLAY: usize = 10;

pub struct GlobTool;

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &str {
        "Glob"
    }

    fn description(&self) -> &str {
        "Find files matching a glob pattern. Supports *, **, ?, [abc], {a,b}."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern (not regex) to match file paths. Supports *, **, ?, [abc], {a,b}. Example: src/**/*.rs"
                },
                "path": {
                    "type": "string",
                    "description": "Directory to search in. Defaults to current working directory."
                }
            },
            "required": ["pattern"]
        })
    }

    fn is_read_only(&self) -> bool {
        true
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolContext<'_>,
    ) -> Result<Vec<ToolOutput>> {
        let pattern = input.get("pattern")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let search_path = input.get("path")
            .and_then(|v| v.as_str())
            .unwrap_or(ctx.working_dir);

        let base = PathBuf::from(search_path);
        let full_pattern = base.join(pattern);

        let mut files: Vec<String> = Vec::new();
        let pattern_str = full_pattern.to_string_lossy().to_string();

        if let Ok(paths) = glob::glob(&pattern_str) {
            for entry in paths.flatten() {
                if files.len() >= MAX_RESULTS {
                    break;
                }
                // Convert to relative path from working_dir
                if let Ok(rel) = entry.strip_prefix(ctx.working_dir) {
                    files.push(rel.to_string_lossy().to_string());
                } else {
                    files.push(entry.to_string_lossy().to_string());
                }
            }
        }

        let truncated = files.len() >= MAX_RESULTS;
        let num_files = files.len();

        let output_text = if files.is_empty() {
            "No files found".to_string()
        } else {
            let mut s = files.iter().take(MAX_DISPLAY).cloned().collect::<Vec<_>>().join("\n");
            let remaining = num_files.saturating_sub(MAX_DISPLAY);
            if remaining > 0 {
                s.push_str(&format!("\n... (+{remaining} files)"));
            }
            if truncated {
                s.push_str("\n(Results are truncated. Consider using a more specific path or pattern.)");
            }
            s
        };

        let title = get_title(pattern, search_path);

        Ok(vec![ToolOutput::Result {
            data: serde_json::json!({
                "pattern": pattern,
                "path": search_path,
                "filenames": files,
                "numFiles": num_files,
                "truncated": truncated,
                "durationMs": 0,
            }),
            result_for_assistant: output_text,
        }])
    }

    fn gen_tool_result_message(
        &self,
        data: &Value,
        _input: &Value,
    ) -> ToolResultMessage {
        let num = data.get("numFiles").and_then(|v| v.as_u64()).unwrap_or(0);
        let title = format!(
            "pattern: \"{}\"",
            data.get("pattern").and_then(|v| v.as_str()).unwrap_or("")
        );
        let summary = format!(
            "Found {} {}",
            num,
            if num == 1 { "file" } else { "files" }
        );
        ToolResultMessage { title, summary, content: data.clone() }
    }

    fn get_display_title(&self, input: &Value) -> String {
        let pattern = input.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
        let path = input.get("path").and_then(|v| v.as_str());
        get_title(pattern, path.unwrap_or(""))
    }
}

fn get_title(pattern: &str, path: &str) -> String {
    let mut parts = vec![format!("pattern: \"{}\"", pattern)];
    if !path.is_empty() {
        // Show relative path if possible
        parts.push(format!("path: \"{}\"", path));
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glob_tool_read_only() {
        let tool = GlobTool;
        assert!(tool.is_read_only());
        assert_eq!(tool.name(), "Glob");
    }
}
