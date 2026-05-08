//! Marketplace plugin management. Mirrors `src-old/marketplace/*.ts`.
//!
//! Manages git/local sources for plugins (skills, subagents, hooks, MCP servers)
//! with enable/disable state and priority-based loading.

pub mod git_sync;
pub mod manager;
pub mod types;
