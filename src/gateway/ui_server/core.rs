use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use async_trait::async_trait;
use axum::{
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::{delete, get, patch, post},
    Router,
};
use tower_http::{cors::CorsLayer, services::ServeDir};

use crate::config::Config;
use crate::cowork::CoworkManager;
use crate::db::Db;
use crate::mcp::manager::McpManager;
use crate::wiki::manager::WikiManager;

use super::types::AdminPermissionsConfig;
use super::spa::spa_fallback;
use super::config_handler::{admin_perms_get, admin_perms_set, config_handler, thinking_handler};
use super::cowork::{
    cowork_board_get, cowork_board_update, cowork_documents_upload, cowork_files_download,
    cowork_files_list, cowork_members_add, cowork_members_list, cowork_members_remove,
    cowork_members_update, cowork_messages_list, cowork_messages_send, cowork_task_comments_add,
    cowork_task_comments_list, cowork_tasks_create, cowork_tasks_delete, cowork_tasks_get,
    cowork_tasks_list, cowork_tasks_update, cowork_templates_get, cowork_templates_list,
    cowork_ws_create, cowork_ws_delete, cowork_ws_get, cowork_ws_list, cowork_ws_update,
};
use super::llm_config::{
    llm_config_create, llm_config_delete, llm_config_fetch_models, llm_config_list,
    llm_config_set_active, llm_config_test,
};
use super::mcp::{
    hooks_get, hooks_put, mcp_servers_connect, mcp_servers_delete, mcp_servers_disconnect,
    mcp_servers_enabled, mcp_servers_get, mcp_servers_list, mcp_servers_save, mcp_servers_tools,
};
use super::quicknotes::quicknotes_save;
use super::skills::{
    skills_install, skills_list, skills_readme, skills_readme_save, skills_remote_search,
    skills_toggle,
};
use super::subagents::{
    subagents_create, subagents_list, subagents_readme, subagents_readme_save, subagents_toggle,
};
use super::wiki::{
    wiki_dir_delete, wiki_history, wiki_mkdir, wiki_read, wiki_search, wiki_stats, wiki_tags,
    wiki_tree, wiki_write,
};

// ===== Trait for AgentPool-dependent operations =====

/// Operations the UI server needs from AgentPool (stubbed until sema-core arrives).
#[async_trait]
pub trait UiApi: Send + Sync {
    /// Signal all agents to reload their skill registries.
    fn reload_all_skills(&self) {}
    /// Get current thinking-enabled state.
    fn get_thinking_enabled(&self) -> bool {
        false
    }
    /// Set thinking-enabled state.
    fn set_thinking_enabled(&self, _enabled: bool) {}
    /// Get current admin permissions config.
    fn get_permissions_config(&self) -> AdminPermissionsConfig {
        AdminPermissionsConfig::default()
    }
    /// Set admin permissions config.
    fn set_permissions_config(&self, _cfg: AdminPermissionsConfig) {}
}

// ===== Shared state =====

pub struct UiState {
    pub config: Arc<Config>,
    pub db: Option<Arc<Db>>,
    pub cowork_manager: Option<Arc<CoworkManager>>,
    pub wiki_manager: Option<Arc<WikiManager>>,
    pub persona_registry: Option<Arc<Mutex<crate::agent::persona_registry::PersonaRegistry>>>,
    pub agent_api: Option<Arc<dyn UiApi>>,
    pub mcp_manager: Option<Arc<McpManager>>,
    pub ws_port: u16,
    pub ws_token: String,
}

/// Return the web/dist directory, falling back to cwd-based path.
fn resolve_dist_dir() -> PathBuf {
    // Try relative to the binary first, then cwd
    let cwd_dist = PathBuf::from("web/dist");
    if cwd_dist.exists() {
        return cwd_dist;
    }
    // Try from workspace root (development)
    let workspace_dist = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("web/dist");
    if workspace_dist.exists() {
        return workspace_dist;
    }
    cwd_dist
}

// ===== Router construction =====

pub fn build_router(state: Arc<UiState>) -> Router {
    let dist_dir = resolve_dist_dir();

    let serve_dir = ServeDir::new(&dist_dir)
        .precompressed_gzip()
        .precompressed_br();

    Router::new()
        // API endpoints
        .route("/api/config", get(config_handler))
        .route("/api/skills", get(skills_list))
        .route("/api/skills/remote-search", get(skills_remote_search))
        .route("/api/skills/install", post(skills_install))
        .route("/api/skills/:name/readme", get(skills_readme).put(skills_readme_save))
        .route("/api/skills/:name/:action", post(skills_toggle))
        .route("/api/subagents", get(subagents_list))
        .route("/api/subagents/create", post(subagents_create))
        .route("/api/subagents/:name/readme", get(subagents_readme).put(subagents_readme_save))
        .route("/api/subagents/:name/:action", post(subagents_toggle))
        .route("/api/thinking", post(thinking_handler))
        .route("/api/admin-permissions", get(admin_perms_get).post(admin_perms_set))
        .route("/api/quicknotes", post(quicknotes_save))
        // LLM config (specific routes before parameterized)
        .route("/api/llm-config", get(llm_config_list).post(llm_config_create))
        .route("/api/llm-config/active", post(llm_config_set_active))
        .route("/api/llm-config/test", post(llm_config_test))
        .route("/api/llm-config/models", post(llm_config_fetch_models))
        .route("/api/llm-config/:id", delete(llm_config_delete))
        // Wiki API
        .route("/api/wiki/tree", get(wiki_tree))
        .route("/api/wiki/file", get(wiki_read).put(wiki_write))
        .route("/api/wiki/search", get(wiki_search))
        .route("/api/wiki/stats", get(wiki_stats))
        .route("/api/wiki/history", get(wiki_history))
        .route("/api/wiki/tags", get(wiki_tags))
        .route("/api/wiki/mkdir", post(wiki_mkdir))
        .route("/api/wiki/dir", delete(wiki_dir_delete))
        // MCP server management
        .route("/api/mcp-servers", get(mcp_servers_list).post(mcp_servers_save))
        .route("/api/mcp-servers/:name", get(mcp_servers_get).delete(mcp_servers_delete))
        .route("/api/mcp-servers/:name/connect", post(mcp_servers_connect))
        .route("/api/mcp-servers/:name/disconnect", post(mcp_servers_disconnect))
        .route("/api/mcp-servers/:name/tools", post(mcp_servers_tools))
        .route("/api/mcp-servers/:name/enabled", post(mcp_servers_enabled))
        // Hooks config
        .route("/api/hooks", get(hooks_get).put(hooks_put))
        // Cowork API
        .route("/api/cowork/templates", get(cowork_templates_list))
        .route("/api/cowork/templates/:name", get(cowork_templates_get))
        .route("/api/cowork/workspaces", get(cowork_ws_list).post(cowork_ws_create))
        .route("/api/cowork/workspaces/:id", get(cowork_ws_get).patch(cowork_ws_update).delete(cowork_ws_delete))
        .route("/api/cowork/workspaces/:id/members", get(cowork_members_list).post(cowork_members_add))
        .route("/api/cowork/workspaces/:id/members/:mid", patch(cowork_members_update).delete(cowork_members_remove))
        .route("/api/cowork/workspaces/:id/board", get(cowork_board_get))
        .route("/api/cowork/workspaces/:id/board/:section", patch(cowork_board_update))
        .route("/api/cowork/workspaces/:id/tasks", get(cowork_tasks_list).post(cowork_tasks_create))
        .route("/api/cowork/workspaces/:id/tasks/:tid", get(cowork_tasks_get).patch(cowork_tasks_update).delete(cowork_tasks_delete))
        .route("/api/cowork/workspaces/:id/tasks/:tid/comments", get(cowork_task_comments_list).post(cowork_task_comments_add))
        .route("/api/cowork/workspaces/:id/messages", get(cowork_messages_list).post(cowork_messages_send))
        .route("/api/cowork/workspaces/:id/documents", post(cowork_documents_upload))
        .route("/api/cowork/workspaces/:id/files", get(cowork_files_list))
        .route("/api/cowork/workspaces/:id/files/download", get(cowork_files_download))
        // Static files
        .nest_service("/", serve_dir)
        // SPA fallback
        .fallback(get(move |headers: HeaderMap| spa_fallback(dist_dir.clone(), headers)))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

// ===== App error type =====

pub struct AppError(pub StatusCode, pub String);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let body = serde_json::json!({ "error": self.1 });
        (self.0, Json(body)).into_response()
    }
}

// ===== Server launcher =====

/// Start the UI HTTP server on the configured port. Binds to 127.0.0.1.
pub async fn start_ui_server(state: Arc<UiState>, port: u16) -> Result<()> {
    let router = build_router(state);
    let addr = format!("127.0.0.1:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("bind {addr}"))?;
    tracing::info!("[UIServer] Web UI at http://{addr}");
    axum::serve(listener, router).await?;
    Ok(())
}
