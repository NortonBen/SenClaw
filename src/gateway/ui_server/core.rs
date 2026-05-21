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

use super::config_handler::{admin_perms_get, admin_perms_set, config_handler, thinking_handler};
use super::cowork::{
    cowork_board_get, cowork_board_update, cowork_documents_upload, cowork_files_download,
    cowork_files_list, cowork_fs_browse, cowork_members_add, cowork_members_list,
    cowork_members_remove, cowork_members_update, cowork_messages_list, cowork_messages_send,
    cowork_task_comments_add, cowork_task_comments_list, cowork_tasks_create, cowork_tasks_delete,
    cowork_tasks_get, cowork_tasks_list, cowork_tasks_update, cowork_templates_get,
    cowork_templates_list, cowork_ws_browse, cowork_ws_create, cowork_ws_delete, cowork_ws_get,
    cowork_ws_list, cowork_ws_update, cowork_ws_resources_list, cowork_ws_resource_upsert,
    cowork_ws_resource_delete,
};
use super::embedding_config::{embedding_config_get, embedding_config_save};
use super::local_models::{
    local_models_cancel, local_models_delete, local_models_download, local_models_list,
    local_models_load, local_models_load_mlx, local_models_loaded_list, local_models_runtime,
    local_models_settings_get, local_models_settings_put, local_models_status,
    local_models_unload, local_models_unload_all, local_models_use_as_llm,
};
use super::llm_config::{
    llm_config_create, llm_config_delete, llm_config_fetch_models, llm_config_list,
    llm_config_set_active, llm_config_test, llm_config_update,
};
use super::mcp::{
    hooks_get, hooks_put, mcp_servers_connect, mcp_servers_delete, mcp_servers_disconnect,
    mcp_servers_enabled, mcp_servers_get, mcp_servers_list, mcp_servers_save, mcp_servers_test,
    mcp_servers_tools,
};
use super::code::{
    code_chat_group_messages, code_chat_groups_create, code_chat_groups_list,
    code_chat_group_stop_current,
    code_chat_ws,
    code_sessions_archive, code_sessions_chat, code_sessions_create, code_sessions_file_content, code_sessions_files,
    code_sessions_get, code_sessions_git_log, code_sessions_list, code_sessions_rollback,
    fs_ls,
};
use super::quicknotes::quicknotes_save;
use super::space::{
    space_apps_delete, space_apps_list, space_apps_register,
    space_email_accounts_create, space_email_accounts_delete, space_email_accounts_list,
    space_email_draft, space_email_inbox, space_email_read, space_email_search, space_email_send,
    space_events_create, space_events_delete, space_events_list, space_events_search,
    space_events_set_reminder, space_events_update,
    space_notes_create, space_notes_delete, space_notes_list, space_notes_search,
    space_notes_update, space_schedules_cancel, space_schedules_create, space_schedules_list,
    space_sync_apple_calendar, space_sync_apple_notes, space_sync_gmail,
    space_sync_google_calendar, space_today_summary,
};
use super::skills::{
    skills_install, skills_list, skills_readme, skills_readme_save, skills_remote_search,
    skills_toggle,
};
use super::plugins::{
    plugins_list, plugins_remote_search, plugins_install, plugins_get,
    plugins_uninstall, plugins_enable, plugins_disable, plugins_configure,
};
use super::marketplace::{
    marketplace_sources_list, marketplace_sources_add, marketplace_sources_delete,
    marketplace_sources_sync, marketplace_sources_reorder, marketplace_source_get,
    marketplace_source_enable_all, marketplace_source_disable_all, marketplace_plugin_toggle,
    marketplace_mcp_use_tools, marketplace_mcp_status,
};
use super::spa::spa_fallback;
use super::subagents::{
    subagents_create, subagents_list, subagents_readme, subagents_readme_save, subagents_toggle,
};
use super::types::AdminPermissionsConfig;
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
    pub cowork_agent_api: Option<Arc<dyn crate::types::AgentApi>>,
    pub mcp_manager: Option<Arc<McpManager>>,
    pub marketplace_manager: Option<Arc<Mutex<crate::marketplace::manager::MarketplaceManager>>>,
    pub workbench_bridge: Option<Arc<crate::agent::workbench_bridge::WorkbenchBridge>>,
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
        .route(
            "/api/skills/:name/readme",
            get(skills_readme).put(skills_readme_save),
        )
        .route("/api/skills/:name/:action", post(skills_toggle))
        // ── Plugins API ──────────────────────────────────────────────────────
        .route("/api/plugins", get(plugins_list))
        .route("/api/plugins/remote-search", get(plugins_remote_search))
        .route("/api/plugins/install", post(plugins_install))
        .route("/api/plugins/:slug", get(plugins_get).delete(plugins_uninstall))
        .route("/api/plugins/:slug/enable", post(plugins_enable))
        .route("/api/plugins/:slug/disable", post(plugins_disable))
        .route("/api/plugins/:slug/configure", post(plugins_configure))
        // ── Marketplace API ──────────────────────────────────────────────────────
        .route("/api/marketplace/sources", get(marketplace_sources_list).post(marketplace_sources_add))
        .route("/api/marketplace/sources/reorder", post(marketplace_sources_reorder))
        .route("/api/marketplace/sources/:id", get(marketplace_source_get).delete(marketplace_sources_delete))
        .route("/api/marketplace/sources/:id/sync", post(marketplace_sources_sync))
        .route("/api/marketplace/sources/:id/enable-all", post(marketplace_source_enable_all))
        .route("/api/marketplace/sources/:id/disable-all", post(marketplace_source_disable_all))
        .route("/api/marketplace/sources/:id/plugins/:name/toggle", post(marketplace_plugin_toggle))
        .route("/api/marketplace/sources/:id/plugins/:name/mcp/:server/use-tools", post(marketplace_mcp_use_tools))
        .route("/api/marketplace/mcp-status", get(marketplace_mcp_status))
        .route("/api/subagents", get(subagents_list))
        .route("/api/subagents/create", post(subagents_create))
        .route(
            "/api/subagents/:name/readme",
            get(subagents_readme).put(subagents_readme_save),
        )
        .route("/api/subagents/:name/:action", post(subagents_toggle))
        .route("/api/thinking", post(thinking_handler))
        .route(
            "/api/admin-permissions",
            get(admin_perms_get).post(admin_perms_set),
        )
        .route("/api/quicknotes", post(quicknotes_save))
        // LLM config (specific routes before parameterized)
        .route(
            "/api/llm-config",
            get(llm_config_list).post(llm_config_create),
        )
        .route("/api/llm-config/active", post(llm_config_set_active))
        .route("/api/llm-config/test", post(llm_config_test))
        .route("/api/llm-config/models", post(llm_config_fetch_models))
        .route("/api/llm-config/:id", delete(llm_config_delete).patch(llm_config_update))
        // Local model management (MLX/HF download)
        .route("/api/local-models", get(local_models_list))
        .route("/api/local-models/runtime", get(local_models_runtime))
        .route(
            "/api/local-models/settings",
            get(local_models_settings_get).put(local_models_settings_put),
        )
        .route(
            "/api/local-models/:id/download",
            post(local_models_download),
        )
        .route("/api/local-models/:id/status", get(local_models_status))
        .route("/api/local-models/:id/cancel", post(local_models_cancel))
        .route("/api/local-models/:id", delete(local_models_delete))
        .route("/api/local-models/:id/load", post(local_models_load))
        .route("/api/local-models/:id/load-mlx", post(local_models_load_mlx))
        .route("/api/local-models/:id/unload", post(local_models_unload))
        .route("/api/local-models/unload-all", post(local_models_unload_all))
        .route("/api/local-models/loaded", get(local_models_loaded_list))
        .route("/api/local-models/:id/use-as-llm", post(local_models_use_as_llm))
        // Embedding provider config
        .route(
            "/api/embedding-config",
            get(embedding_config_get).post(embedding_config_save),
        )
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
        .route(
            "/api/mcp-servers",
            get(mcp_servers_list).post(mcp_servers_save),
        )
        .route(
            "/api/mcp-servers/:name",
            get(mcp_servers_get).delete(mcp_servers_delete),
        )
        .route("/api/mcp-servers/:name/connect", post(mcp_servers_connect))
        .route(
            "/api/mcp-servers/:name/disconnect",
            post(mcp_servers_disconnect),
        )
        .route("/api/mcp-servers/:name/tools", post(mcp_servers_tools))
        .route("/api/mcp-servers/:name/test", post(mcp_servers_test))
        .route("/api/mcp-servers/:name/enabled", post(mcp_servers_enabled))
        // Hooks config
        .route("/api/hooks", get(hooks_get).put(hooks_put))
        // Cowork API
        .route("/api/cowork/templates", get(cowork_templates_list))
        .route("/api/cowork/templates/:name", get(cowork_templates_get))
        .route("/api/cowork/fs-browse", get(cowork_fs_browse))
        .route(
            "/api/cowork/workspaces",
            get(cowork_ws_list).post(cowork_ws_create),
        )
        .route(
            "/api/cowork/workspaces/:id",
            get(cowork_ws_get)
                .patch(cowork_ws_update)
                .delete(cowork_ws_delete),
        )
        .route(
            "/api/cowork/workspaces/:id/members",
            get(cowork_members_list).post(cowork_members_add),
        )
        .route(
            "/api/cowork/workspaces/:id/members/:mid",
            patch(cowork_members_update).delete(cowork_members_remove),
        )
        .route("/api/cowork/workspaces/:id/board", get(cowork_board_get))
        .route(
            "/api/cowork/workspaces/:id/board/:section",
            patch(cowork_board_update),
        )
        .route(
            "/api/cowork/workspaces/:id/tasks",
            get(cowork_tasks_list).post(cowork_tasks_create),
        )
        .route(
            "/api/cowork/workspaces/:id/tasks/:tid",
            get(cowork_tasks_get)
                .patch(cowork_tasks_update)
                .delete(cowork_tasks_delete),
        )
        .route(
            "/api/cowork/workspaces/:id/tasks/:tid/comments",
            get(cowork_task_comments_list).post(cowork_task_comments_add),
        )
        .route(
            "/api/cowork/workspaces/:id/messages",
            get(cowork_messages_list).post(cowork_messages_send),
        )
        .route(
            "/api/cowork/workspaces/:id/documents",
            post(cowork_documents_upload),
        )
        .route("/api/cowork/workspaces/:id/files", get(cowork_files_list))
        .route(
            "/api/cowork/workspaces/:id/files/download",
            get(cowork_files_download),
        )
        .route("/api/cowork/workspaces/:id/browse", get(cowork_ws_browse))
        .route(
            "/api/cowork/workspaces/:id/resources",
            get(cowork_ws_resources_list).post(cowork_ws_resource_upsert),
        )
        .route(
            "/api/cowork/workspaces/:id/resources/:kind",
            axum::routing::delete(cowork_ws_resource_delete),
        )
        // ── Space API ─────────────────────────────────────────────────────────
        // Notes
        .route("/api/space/notes", get(space_notes_list).post(space_notes_create))
        .route("/api/space/notes/search", get(space_notes_search))
        .route(
            "/api/space/notes/:id",
            axum::routing::put(space_notes_update).delete(space_notes_delete),
        )
        // Calendar
        .route("/api/space/calendar/events", get(space_events_list).post(space_events_create))
        .route("/api/space/calendar/events/search", get(space_events_search))
        .route("/api/space/calendar/events/:id", patch(space_events_update).delete(space_events_delete))
        .route("/api/space/calendar/events/:id/reminder", post(space_events_set_reminder))
        .route("/api/space/calendar/today", get(space_today_summary))
        // Email
        .route("/api/space/email/inbox", get(space_email_inbox))
        .route("/api/space/email/messages/:id", get(space_email_read))
        .route("/api/space/email/search", get(space_email_search))
        .route("/api/space/email/send", post(space_email_send))
        .route("/api/space/email/draft", post(space_email_draft))
        .route(
            "/api/space/email/accounts",
            get(space_email_accounts_list).post(space_email_accounts_create),
        )
        .route("/api/space/email/accounts/:id", delete(space_email_accounts_delete))
        // Schedules
        .route("/api/space/schedules", get(space_schedules_list).post(space_schedules_create))
        .route("/api/space/schedules/:id", delete(space_schedules_cancel))
        // Apps
        .route("/api/space/apps", get(space_apps_list))
        .route("/api/space/apps/register", post(space_apps_register))
        .route("/api/space/apps/:id", delete(space_apps_delete))
        // External sync
        .route("/api/space/sync/google-calendar", post(space_sync_google_calendar))
        .route("/api/space/sync/apple-calendar", post(space_sync_apple_calendar))
        .route("/api/space/sync/apple-notes", post(space_sync_apple_notes))
        .route("/api/space/sync/gmail", post(space_sync_gmail))
        // ── Filesystem browser ───────────────────────────────────────────────
        .route("/api/fs/ls", get(fs_ls))
        // ── Code Engine API ──────────────────────────────────────────────────
        .route("/api/code/sessions", get(code_sessions_list).post(code_sessions_create))
        .route("/api/code/sessions/:id", get(code_sessions_get).delete(code_sessions_archive))
        .route("/api/code/sessions/:id/files", get(code_sessions_files))
        .route("/api/code/sessions/:id/file-content", get(code_sessions_file_content))
        .route("/api/code/sessions/:id/chat", post(code_sessions_chat))
        .route("/api/code/projects/:id/groups", get(code_chat_groups_list).post(code_chat_groups_create))
        .route("/api/code/groups/:id/messages", get(code_chat_group_messages))
        .route("/api/code/groups/:id/stop-current", post(code_chat_group_stop_current))
        .route("/api/code/ws", get(code_chat_ws))
        .route("/api/code/sessions/:id/git-log", get(code_sessions_git_log))
        .route("/api/code/sessions/:id/rollback", post(code_sessions_rollback))
        // Workbench reverse ops (artifacts published by tools)
        .route(
            "/api/workbench/:jid/:id/mark-viewed",
            post(super::workbench::workbench_mark_viewed),
        )
        .route(
            "/api/workbench/:jid/:id/close",
            post(super::workbench::workbench_close),
        )
        .route(
            "/api/workbench/:jid/:id/read-file",
            get(super::workbench::workbench_read_file),
        )
        .route(
            "/api/workbench/:jid/:id/logs",
            get(super::workbench::workbench_fetch_logs),
        )
        // Cognitive memory (graph + Hebbian)
        .route("/api/cognitive/stats", get(super::cognitive::cognitive_stats))
        .route("/api/cognitive/nodes", get(super::cognitive::cognitive_list_nodes))
        .route(
            "/api/cognitive/node/:id",
            get(super::cognitive::cognitive_get_node).delete(super::cognitive::cognitive_forget),
        )
        .route(
            "/api/cognitive/node/:id/re-extract",
            post(super::cognitive::cognitive_re_extract),
        )
        .route(
            "/api/cognitive/decay-log",
            get(super::cognitive::cognitive_decay_log),
        )
        .route("/api/cognitive/search", post(super::cognitive::cognitive_search))
        .route("/api/cognitive/subgraph", get(super::cognitive::cognitive_subgraph))
        .route("/api/cognitive/top-nodes", get(super::cognitive::cognitive_top_nodes))
        .route("/api/cognitive/sample", get(super::cognitive::cognitive_sample))
        // Embedding model management
        .route(
            "/api/embedding/features",
            get(super::embedding_models::embedding_features),
        )
        .route(
            "/api/embedding/models",
            get(super::embedding_models::embedding_list_models),
        )
        .route(
            "/api/embedding/download-model",
            post(super::embedding_models::embedding_download_model),
        )
        // Static files
        .nest_service("/", serve_dir)
        // SPA fallback
        .fallback(get(move |headers: HeaderMap| {
            spa_fallback(dist_dir.clone(), headers)
        }))
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
