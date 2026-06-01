//! UI HTTP server. Port target: src-old/gateway/UIServer.ts
//!
//! Listens on 127.0.0.1:18788 by default (overridable via `GATEWAY_UI_PORT`).
//! Serves the React web UI from `web/dist/` and exposes REST API endpoints for
//! the frontend: config, skills, subagents, wiki, admin permissions, quicknotes.
//!
//! LLM config endpoints (`/api/llm-config/*`) are stubbed — they require the
//! `sema-code-core` model manager which hasn't been ported yet.

mod chat;
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
pub mod relay_bridge;
mod space;
pub mod space_mcp;
mod space_skills;
mod plugins;
mod skills;
mod spa;
mod subagents;
pub mod types;
mod whisper;
mod wiki;
mod workbench;

// Re-exports for external use
pub use code::subscribe_code_chat;
pub use core::{build_router, start_ui_server, AppError, UiApi, UiState};
pub use relay_bridge::{dispatch as dispatch_api, ApiBridgeState, ApiRequest, ApiResponse};
pub use types::AdminPermissionsConfig;
