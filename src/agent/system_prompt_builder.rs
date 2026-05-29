//! System prompt builder. Mirrors `sema-core/dist/services/agents/genSystemPrompt.js`.
//!
//! Provides dynamic system prompt generation based on context and tool availability.

use serde::{Deserialize, Serialize};

use super::system_prompts::*;

/// Context information for system prompt generation.
#[derive(Debug, Clone, Default)]
pub struct PromptContext {
    /// Environment variables and system information.
    pub env: Option<String>,
    /// Git status information.
    pub git_status: Option<String>,
}

/// Options for system prompt formatting.
#[derive(Debug, Clone)]
pub struct FormatSystemPromptOptions {
    /// Whether TodoWrite tool is available.
    pub has_todo_write_tool: bool,
    /// Whether AskUser tool is available.
    pub has_ask_user_tool: bool,
}

impl Default for FormatSystemPromptOptions {
    fn default() -> Self {
        Self {
            has_todo_write_tool: true,
            has_ask_user_tool: true,
        }
    }
}

/// Content block for system prompt (compatible with Anthropic API format).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    pub text: String,
}

impl ContentBlock {
    /// Create a new text content block.
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            block_type: "text".to_string(),
            text: text.into(),
        }
    }
}

/// Format system prompt with context and options.
///
/// This assembles the complete system prompt from various components based on
/// tool availability and context information.
pub fn format_system_prompt(
    context: &PromptContext,
    options: &FormatSystemPromptOptions,
) -> Vec<ContentBlock> {
    let base_prompt = get_system_prompt(context, options);
    vec![ContentBlock::text(base_prompt)]
}

/// Generate todo-related reminders.
///
/// Returns content blocks with todo management reminders when the todo list is empty.
pub fn generate_todo_reminders() -> Vec<ContentBlock> {
    let reminder = format!(
        "<system-reminder>\n{}\n</system-reminder>",
        EMPTY_TODO_REMINDER_PROMPT
    );
    vec![ContentBlock::text(reminder)]
}

/// Generate plan mode reminders.
///
/// Returns content blocks with plan mode instructions and file location guidance.
pub fn generate_plan_reminders(task_description: Option<&str>) -> Vec<ContentBlock> {
    let plan_file_instruction = if let Some(desc) = task_description {
        format!("Create your plan at .sema/plans/<kebab-case-title>.md (e.g. based on: \"{}\")", desc)
    } else {
        "Create your plan at .sema/plans/<kebab-case-title>.md".to_string()
    };

    let reminder = format!(
        "<system-reminder>\n\
         Plan mode: read-only. Edits, tool writes, and config changes are disabled until execution is approved by user. This supersedes any other instructions you have received.\n\
         \n\
         ## Plan File Info:\n\
         No plan file exists yet. {}\n\
         Build your plan incrementally by writing to or editing this file. \n\
         ATTENTION that this is the only file you are allowed to edit during this plan session - other than this you are only allowed to take READ-ONLY actions.\n\
         \n\
         {}\n\
         </system-reminder>",
        plan_file_instruction, PLAN_MODE_REMINDER_PROMPT
    );

    vec![ContentBlock::text(reminder)]
}

/// Generate environment information section.
pub fn gen_env(context: &PromptContext) -> String {
    if let Some(env) = &context.env {
        format!(
            "Here is useful information about the environment you are running in:\n<env>{}</env>",
            env
        )
    } else {
        String::new()
    }
}

/// Generate git status information section.
pub fn gen_git_status(context: &PromptContext) -> String {
    if let Some(git_status) = &context.git_status {
        format!("gitStatus: {}", git_status)
    } else {
        String::new()
    }
}

/// Build agent system prompt for subagents (researcher, creator, architect).
///
/// Combines the agent-specific prompt with subagent notes and context information.
pub fn build_agent_system_prompt(agent_prompt: &str, context: &PromptContext) -> Vec<ContentBlock> {
    let full_prompt = format!(
        "{}{}{}{}",
        agent_prompt,
        SUBAGENT_NOTES,
        gen_env(context),
        gen_git_status(context)
    );
    vec![ContentBlock::text(full_prompt)]
}

/// Get the base system prompt assembled from components.
///
/// This combines all prompt sections based on tool availability and context.
fn get_system_prompt(context: &PromptContext, options: &FormatSystemPromptOptions) -> String {
    let skills_summary = ""; // TODO: Integrate with skill registry when available

    let (todo_write_prompt, todo_write_important) = if options.has_todo_write_tool {
        (
            WITH_TODOWRITE_PROMPT,
            "IMPORTANT: Always use the TodoWrite tool to plan and track tasks throughout the conversation.",
        )
    } else {
        (WITHOUT_TODOWRITE_PROMPT, "")
    };

    let ask_question_prompt = if options.has_ask_user_tool {
        ASK_QUESTION_PROMPT
    } else {
        ""
    };

    format!(
        r#"
{agent_summary}

{skills_summary}

{style_and_professional}

{todo_write_prompt}

{ask_question_prompt}

{doing_tasks}

{tool_usage_policy}

{todo_write_important}

{code_references}

{space_notes}

{memory_notes}

{env_section}

{git_status_section}
"#,
        agent_summary = AGENT_SUMMARY_PROMPT,
        skills_summary = skills_summary,
        style_and_professional = STYLE_AND_PROFESSIONAL_PROMPT,
        todo_write_prompt = todo_write_prompt,
        ask_question_prompt = ask_question_prompt,
        doing_tasks = DOING_TASKS_PROMPT,
        tool_usage_policy = TOOL_USAGE_POLICY_PROMPT,
        todo_write_important = todo_write_important,
        code_references = CODE_REFERENCES_PROMPT,
        space_notes = SPACE_NOTES,
        memory_notes = MEMORY_NOTES,
        env_section = gen_env(context),
        git_status_section = gen_git_status(context)
    )
    .trim()
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_system_prompt_default() {
        let context = PromptContext::default();
        let options = FormatSystemPromptOptions::default();
        let result = format_system_prompt(&context, &options);
        assert!(!result.is_empty());
        assert_eq!(result.len(), 1);
        assert!(!result[0].text.is_empty());
    }

    #[test]
    fn test_format_system_prompt_includes_space_section() {
        // The Space notes must land in the assembled prompt so the agent
        // is aware of `space_event_*` tools without being told inline.
        let context = PromptContext::default();
        let options = FormatSystemPromptOptions::default();
        let result = format_system_prompt(&context, &options);
        let text = &result[0].text;
        assert!(text.contains("# Space"), "Space section header missing");
        assert!(text.contains("space_event_list"));
        assert!(text.contains("space_event_delete"));
    }

    #[test]
    fn test_format_system_prompt_with_context() {
        let context = PromptContext {
            env: Some("test env".to_string()),
            git_status: Some("test git status".to_string()),
        };
        let options = FormatSystemPromptOptions::default();
        let result = format_system_prompt(&context, &options);
        assert!(!result.is_empty());
        assert!(result[0].text.contains("test env"));
        assert!(result[0].text.contains("test git status"));
    }

    #[test]
    fn test_generate_todo_reminders() {
        let result = generate_todo_reminders();
        assert_eq!(result.len(), 1);
        assert!(result[0].text.contains("system-reminder"));
        assert!(result[0].text.contains("TodoWrite"));
    }

    #[test]
    fn test_generate_plan_reminders() {
        let result = generate_plan_reminders(Some("test task"));
        assert_eq!(result.len(), 1);
        assert!(result[0].text.contains("Plan mode"));
        assert!(result[0].text.contains("test task"));
    }

    #[test]
    fn test_gen_env() {
        let context = PromptContext {
            env: Some("test env".to_string()),
            git_status: None,
        };
        let result = gen_env(&context);
        assert!(result.contains("test env"));
        assert!(result.contains("environment"));
    }

    #[test]
    fn test_gen_env_empty() {
        let context = PromptContext::default();
        let result = gen_env(&context);
        assert!(result.is_empty());
    }

    #[test]
    fn test_gen_git_status() {
        let context = PromptContext {
            env: None,
            git_status: Some("test git status".to_string()),
        };
        let result = gen_git_status(&context);
        assert!(result.contains("test git status"));
        assert!(result.contains("gitStatus"));
    }

    #[test]
    fn test_gen_git_status_empty() {
        let context = PromptContext::default();
        let result = gen_git_status(&context);
        assert!(result.is_empty());
    }

    #[test]
    fn test_build_agent_system_prompt() {
        let agent_prompt = "You are a test agent.";
        let context = PromptContext {
            env: Some("test env".to_string()),
            git_status: Some("test git status".to_string()),
        };
        let result = build_agent_system_prompt(agent_prompt, &context);
        assert_eq!(result.len(), 1);
        assert!(result[0].text.contains("test agent"));
        assert!(result[0].text.contains("NOTES"));
        assert!(result[0].text.contains("test env"));
        assert!(result[0].text.contains("test git status"));
    }

    #[test]
    fn test_format_system_prompt_without_todo_write() {
        let context = PromptContext::default();
        let options = FormatSystemPromptOptions {
            has_todo_write_tool: false,
            has_ask_user_tool: true,
        };
        let result = format_system_prompt(&context, &options);
        assert!(!result[0].text.contains("TodoWrite"));
        assert!(result[0].text.contains("Skip time estimates"));
    }

    #[test]
    fn test_format_system_prompt_without_ask_user() {
        let context = PromptContext::default();
        let options = FormatSystemPromptOptions {
            has_todo_write_tool: true,
            has_ask_user_tool: false,
        };
        let result = format_system_prompt(&context, &options);
        assert!(!result[0].text.contains("AskUser"));
    }

    #[test]
    fn test_content_block_serialization() {
        let block = ContentBlock::text("test text");
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains("text"));
        assert!(json.contains("test text"));
    }
}
