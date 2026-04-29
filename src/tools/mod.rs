//! Concrete tool implementations that implement [`zen_core::Tool`].
//!
//! Each tool mirrors the TS sema-core tool surface.

pub mod ask_user;
pub mod ask_user_question;
pub mod bash;
pub mod edit;
pub mod glob;
pub mod grep;
pub mod notebook_edit;
pub mod read;
pub mod skill;
pub mod task;
pub mod todo_write;
pub mod write;

use std::sync::Arc;

use crate::zen_core::Tool;

pub use ask_user::AskUserTool;
pub use ask_user_question::AskUserQuestionTool;
pub use bash::BashTool;
pub use edit::EditTool;
pub use glob::GlobTool;
pub use grep::GrepTool;
pub use notebook_edit::NotebookEditTool;
pub use read::ReadTool;
pub use skill::SkillTool;
pub use task::{AgentConfig, TaskTool};
pub use todo_write::TodoWriteTool;
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
    ]
}
