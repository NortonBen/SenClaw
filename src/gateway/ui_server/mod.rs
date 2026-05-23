//! UI HTTP server. Port target: src-old/gateway/UIServer.ts
//!
//! Listens on 127.0.0.1:18788 by default (overridable via `GATEWAY_UI_PORT`).
//! Serves the React web UI from `web/dist/` and exposes REST API endpoints for
//! the frontend: config, skills, subagents, wiki, admin permissions, quicknotes.
//!
//! LLM config endpoints (`/api/llm-config/*`) are stubbed — they require the
//! `sema-code-core` model manager which hasn't been ported yet.

mod code;
mod cognitive;
mod cognitive_config;
mod config_handler;
pub mod core;
mod cowork;
mod embedding_config;
mod embedding_models;
mod llm_config;
pub mod local_models;
mod marketplace;
mod mcp;
mod quicknotes;
mod space;
mod plugins;
mod skills;
mod spa;
mod subagents;
pub mod types;
mod wiki;
mod workbench;

// Re-exports for external use
pub use core::{build_router, start_ui_server, AppError, UiApi, UiState};
pub use types::AdminPermissionsConfig;
