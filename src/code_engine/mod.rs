//! Code Engine — agent-driven code editing for SenClaw.
//!
//! # Modules
//! - `session` — CodeSession: git-backed workspace sandbox + path-traversal protection
//! - `server`  — MCP server exposing 8 tools (read_file, write_file, edit_file, bash,
//!               search_code, glob, get_skeleton, list_files)

pub mod agent_builder;
pub mod prompt;
pub mod server;
pub mod session;
pub mod system_prompt;

pub use agent_builder::{
    always_loaded_code_mcp_tools, build_code_group_binding, code_session_jid, CodeAgentSpec,
    CODE_SESSION_TOOLS,
};
pub use prompt::{parse_prompt, PromptParseResult};
pub use server::run_code_server;
pub use session::{CodeSession, SessionFileTracker};
pub use system_prompt::{build_code_system_prompt, build_user_prompt, CODE_SYSTEM_PROMPT};
