//! UI HTTP server. Port target: src-old/gateway/UIServer.ts
//!
//! Listens on 127.0.0.1:18788 by default (overridable via `GATEWAY_UI_PORT`).
//! Serves the React web UI from `web/dist/` and exposes REST API endpoints for
//! the frontend: config, skills, subagents, wiki, admin permissions, quicknotes.
//!
//! LLM config endpoints (`/api/llm-config/*`) are stubbed — they require the
//! `sema-code-core` model manager which hasn't been ported yet.

mod agent_behavior_config;
pub mod app_auth;
pub mod auth;
mod background;
pub mod bash_sandbox;
mod chat;
mod code;
mod code_artifacts;
mod cognitive;
mod cognitive_config;
mod config_handler;
pub mod core;
mod cowork;
pub mod cowork_runtime;
mod dispatch_config;
mod embedding_config;
mod embedding_models;
mod hf_validate;
mod llm_config;
pub mod local_models;
mod marketplace;
mod mcp;
mod oauth;
pub(crate) mod ocr;
mod open_url;
mod plugins;
mod profile_files;
mod quicknotes;
pub mod relay_bridge;
mod skills;
mod spa;
mod space;
pub mod space_mcp;
mod space_personas;
mod space_runtime;
mod space_skills;
mod subagents;
mod terminal;
mod tool_aliases;
pub mod tts;
pub mod types;
mod usage;
mod user_profile;
mod whisper;
mod widgets;
mod wiki;
mod workbench;
mod workflow;
mod workspace;

// Re-exports for external use
pub use core::{build_router, start_ui_server, AppError, UiApi, UiState};
pub use relay_bridge::{dispatch as dispatch_api, ApiBridgeState, ApiRequest, ApiResponse};
pub use types::AdminPermissionsConfig;
