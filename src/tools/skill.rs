//! Skill tool — loads and activates agent skills by name.
//!
//! Port of TS `node_modules/sema-core/dist/tools/Skill/Skill.js`.

use std::sync::Arc;

use anyhow::{bail, Result};
use async_trait::async_trait;
use serde_json::Value;

use crate::skills::SkillRegistry;
use crate::zen_core::{Tool, ToolContext, ToolOutput, ToolResultMessage};

pub struct SkillTool {
    registry: Arc<SkillRegistry>,
}

impl SkillTool {
    pub fn new(registry: Arc<SkillRegistry>) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl Tool for SkillTool {
    fn name(&self) -> &str {
        "Skill"
    }

    fn description(&self) -> &str {
        "Execute an agent skill"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "skill": {
                    "type": "string",
                    "description": "The skill name. E.g., \"slides-master\", \"pdf-skill\", or \"markdown-converter\""
                },
                "args": {
                    "type": "string",
                    "description": "Optional arguments for the skill"
                }
            },
            "required": ["skill"]
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
        let skill_name = input
            .get("skill")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if skill_name.is_empty() {
            return Err("skill name is required".to_string());
        }
        if self.registry.find(skill_name).is_none() {
            let available = self.registry.names().join(", ");
            return Err(format!(
                "Skill \"{skill_name}\" not found. Available skills: {}",
                if available.is_empty() { "none" } else { &available }
            ));
        }
        Ok(())
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolContext<'_>,
    ) -> Result<Vec<ToolOutput>> {
        let skill_name = input
            .get("skill")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let args = input
            .get("args")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let skill = match self.registry.find(&skill_name) {
            Some(s) => s,
            None => bail!("Skill \"{skill_name}\" not found"),
        };

        let mut content = skill.content.clone();
        if let Some(ref trimmed_args) = args {
            if content.contains("$ARGUMENTS") {
                content = content.replace("$ARGUMENTS", trimmed_args);
            } else {
                content = format!("{content}\n\nARGUMENTS: {trimmed_args}");
            }
        }

        let allowed_tools = skill.metadata.allowed_tools.clone();
        let base_dir = skill.base_dir.clone();
        let result_for_assistant = gen_result_for_assistant(
            &skill.metadata.name,
            &content,
            &allowed_tools,
            &base_dir,
            args.as_deref(),
        );

        Ok(vec![ToolOutput::Result {
            data: serde_json::json!({
                "skillName": skill.metadata.name,
                "skillContent": content,
                "allowedTools": allowed_tools,
                "baseDir": base_dir,
                "skill": skill_name,
                "args": args,
            }),
            result_for_assistant,
        }])
    }

    fn gen_tool_result_message(&self, data: &Value, _input: &Value) -> ToolResultMessage {
        let skill_name = data
            .get("skillName")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown");
        let base_dir = data
            .get("baseDir")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let allowed_tools: Vec<String> = data
            .get("allowedTools")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let skill_content = data
            .get("skillContent")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let mut content = format!("Base directory: {base_dir}\n\n");
        if !allowed_tools.is_empty() {
            content.push_str(&format!(
                "Recommended tools: {}\n\n",
                allowed_tools.join(", ")
            ));
        }
        let preview = if skill_content.len() > 500 {
            format!("{}...", &skill_content[..500])
        } else {
            skill_content.to_string()
        };
        content.push_str(&preview);

        ToolResultMessage {
            title: skill_name.to_string(),
            summary: format!("Skill \"{skill_name}\" loaded successfully"),
            content: serde_json::Value::String(content),
        }
    }

    fn get_display_title(&self, input: &Value) -> String {
        let skill = input
            .get("skill")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if skill.is_empty() {
            "Skill".into()
        } else {
            let mut parts = format!("skill: \"{skill}\"");
            if let Some(args) = input.get("args").and_then(|v| v.as_str()) {
                if !args.is_empty() {
                    parts.push_str(&format!(", args: \"{args}\""));
                }
            }
            parts
        }
    }
}

fn gen_result_for_assistant(
    skill_name: &str,
    skill_content: &str,
    allowed_tools: &[String],
    base_dir: &str,
    args: Option<&str>,
) -> String {
    let mut result = format!("# Skill Activated: {skill_name}\n\n");
    result.push_str(&format!(
        "Base directory for this skill: {base_dir}\n\n"
    ));
    if let Some(a) = args {
        result.push_str(&format!("Arguments: {a}\n\n"));
    }
    result.push_str(skill_content);
    result.push('\n');

    if !allowed_tools.is_empty() {
        result.push_str("\n---\n\n");
        result.push_str("<system-reminder>\n");
        result.push_str(&format!(
            "While working on this skill, you should prioritize using the following tools: {}.\n",
            allowed_tools.join(", ")
        ));
        result.push_str(
            "These tools are recommended for this skill's workflow. You may use other tools if absolutely necessary.\n",
        );
        result.push_str("</system-reminder>\n");
    }

    result.push_str("\n---\n\n");
    result.push_str(
        "Now that you have loaded the skill instructions, please proceed with the task based on the guidelines above.",
    );
    if let Some(a) = args {
        result.push_str(&format!(
            " Remember to process the provided arguments: {a}"
        ));
    }
    result
}
