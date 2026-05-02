//! UI HTTP server. Port target: src-old/gateway/UIServer.ts
//!
//! Listens on 127.0.0.1:18788 by default (overridable via `GATEWAY_UI_PORT`).
//! Serves the React web UI from `web/dist/` and exposes REST API endpoints for
//! the frontend: config, skills, subagents, wiki, admin permissions, quicknotes.
//!
//! LLM config endpoints (`/api/llm-config/*`) are stubbed — they require the
//! `sema-code-core` model manager which hasn't been ported yet.

pub mod types;
pub mod core;
mod config_handler;
mod skills;
mod subagents;
mod quicknotes;
mod llm_config;
mod wiki;
mod mcp;
mod cowork;
mod spa;

// Re-exports for external use
pub use core::{AppError, UiApi, UiState, build_router, start_ui_server};
pub use types::AdminPermissionsConfig;
