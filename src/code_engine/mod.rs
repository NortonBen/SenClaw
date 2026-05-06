//! Code Engine — agent-driven code editing for SemaClaw.
//!
//! # Modules
//! - `session` — CodeSession: git-backed workspace sandbox + path-traversal protection
//! - `server`  — MCP server exposing 8 tools (read_file, write_file, edit_file, bash,
//!               search_code, glob, get_skeleton, list_files)

pub mod session;
pub mod server;
pub mod prompt;

pub use session::{CodeSession, SessionFileTracker};
pub use prompt::{parse_prompt, PromptParseResult};
pub use server::run_code_server;
