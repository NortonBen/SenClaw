//! Concrete tool implementations that implement [`zen_core::Tool`].
//!
//! Each tool mirrors the TS sema-core tool surface.

pub mod ask_user;
pub mod ask_user_question;
pub mod bash;
pub mod bg_jobs;
pub mod edit;
pub mod enter_plan_mode;
pub mod exit_plan_mode;
pub mod glob;
pub mod grep;
pub mod notebook_edit;
pub mod peek_bg_job;
pub mod read;
pub mod skill;
pub mod stop_bg_job;
pub mod task;
pub mod time;
pub mod todo_write;
pub mod tool_search;
pub mod web_fetch;
pub mod write;

use std::sync::Arc;

use crate::zen_core::Tool;

pub use ask_user::AskUserTool;
pub use ask_user_question::AskUserQuestionTool;
pub use bash::BashTool;
pub use edit::EditTool;
pub use enter_plan_mode::{EnterPlanFn, EnterPlanModeTool};
pub use exit_plan_mode::ExitPlanModeTool;
pub use glob::GlobTool;
pub use grep::GrepTool;
pub use notebook_edit::NotebookEditTool;
pub use peek_bg_job::PeekBgJobTool;
pub use read::ReadTool;
pub use skill::SkillTool;
pub use stop_bg_job::StopBgJobTool;
pub use task::{AgentConfig, TaskTool};
pub use time::TimeTool;
pub use todo_write::TodoWriteTool;
pub use tool_search::{DeferredToolsFn, ToolSearchTool};
pub use web_fetch::WebFetchTool;
pub use write::WriteTool;

/// All built-in tools (without engine dependencies).
/// Tools that need engine state (TodoWrite, Skill, Task) must be
/// registered separately by the engine.
pub fn all_tools() -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(AskUserTool),
        Arc::new(AskUserQuestionTool),
        Arc::new(BashTool),
        Arc::new(GlobTool),
        Arc::new(GrepTool),
        Arc::new(ReadTool),
        Arc::new(WriteTool),
        Arc::new(EditTool),
        Arc::new(NotebookEditTool),
        Arc::new(TimeTool),
        Arc::new(WebFetchTool),
        Arc::new(ExitPlanModeTool),
        Arc::new(PeekBgJobTool),
        Arc::new(StopBgJobTool),
    ]
}
