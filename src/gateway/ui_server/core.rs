use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use async_trait::async_trait;
use axum::{
    extract::DefaultBodyLimit,
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::{delete, get, patch, post, put},
    Router,
};
use tower_http::services::ServeDir;

use crate::config::Config;
use crate::db::Db;
use crate::mcp::manager::McpManager;
use crate::wiki::manager::WikiManager;

use super::chat::{
    chat_form_respond, chat_history, chat_permission_respond, chat_plan_respond,
    chat_question_respond, chat_states,
};
use super::code::code_run;
use super::code_artifacts::{
    create_artifact, delete_artifact, get_artifact, list_artifacts, run_artifact, update_artifact,
};
use super::config_handler::{admin_perms_get, admin_perms_set, config_handler, thinking_handler};
use super::embedding_config::{embedding_config_get, embedding_config_save};
use super::hf_validate::{local_models_validate, tts_validate, whisper_validate};
use super::llm_config::{
    llm_config_create, llm_config_delete, llm_config_fetch_models, llm_config_list,
    llm_config_set_active, llm_config_test, llm_config_update,
};
use super::local_models::{
    local_models_cancel, local_models_delete, local_models_download, local_models_list,
    local_models_load, local_models_load_mlx, local_models_loaded_list, local_models_runtime,
    local_models_settings_get, local_models_settings_put, local_models_status, local_models_unload,
    local_models_unload_all, local_models_use_as_llm,
};
use super::marketplace::{
    marketplace_mcp_status, marketplace_mcp_use_tools, marketplace_plugin_install,
    marketplace_plugin_toggle, marketplace_plugin_uninstall, marketplace_source_catalog,
    marketplace_source_disable_all, marketplace_source_enable_all, marketplace_source_get,
    marketplace_sources_add, marketplace_sources_delete, marketplace_sources_list,
    marketplace_sources_reorder, marketplace_sources_sync,
};
use super::mcp::{
    hooks_get, hooks_put, mcp_servers_connect, mcp_servers_delete, mcp_servers_disconnect,
    mcp_servers_enabled, mcp_servers_get, mcp_servers_list, mcp_servers_save, mcp_servers_test,
    mcp_servers_tools,
};
use super::ocr::{
    ocr_cancel, ocr_custom_download, ocr_delete, ocr_download, ocr_models_list, ocr_recognize,
    ocr_settings_get, ocr_settings_put, ocr_status,
};
use super::open_url::open_url_handler;
use super::plugins::{
    plugins_configure, plugins_disable, plugins_enable, plugins_get, plugins_install, plugins_list,
    plugins_remote_search, plugins_uninstall,
};
use super::quicknotes::quicknotes_save;
use super::skills::{
    skills_create, skills_install, skills_list, skills_readme, skills_readme_save,
    skills_remote_search, skills_toggle, skills_uninstall,
};
use super::spa::spa_fallback;
use super::space::{
    space_app_config_delete, space_app_config_get, space_app_config_list, space_app_config_set,
    space_app_env, space_app_logs_clear, space_app_logs_get, space_app_mcp_info,
    space_app_mcp_register, space_app_sandbox_get, space_app_sandbox_put, space_app_sqlite_query,
    space_apps_bridge, space_apps_delete,
    space_apps_install_zip, space_apps_list, space_apps_proxy, space_apps_proxy_root,
    space_apps_register, space_apps_register_local, space_apps_restart, space_apps_static,
    space_apps_update, space_apps_updates, space_events_create, space_events_delete,
    space_events_get, space_events_list, space_events_search, space_events_set_reminder,
    space_events_update, space_notes_create, space_notes_delete, space_notes_list,
    space_notes_search, space_notes_update, space_schedules_cancel, space_schedules_create,
    space_schedules_detail, space_schedules_list, space_schedules_run_now, space_schedules_update,
    space_screenshot_extract, space_screenshot_get, space_sync_apple_calendar,
    space_sync_apple_notes, space_sync_google_calendar, space_sync_google_workspace,
    space_today_summary,
};
use super::subagents::{
    subagents_create, subagents_list, subagents_readme, subagents_readme_save, subagents_toggle,
};
use super::tts::{
    tts_cancel, tts_delete, tts_download, tts_models_list, tts_settings_get, tts_settings_put,
    tts_status, tts_synthesize,
};
use super::types::AdminPermissionsConfig;
use super::whisper::{
    whisper_cancel, whisper_delete, whisper_download, whisper_models_list, whisper_settings_get,
    whisper_settings_put, whisper_status, whisper_transcribe,
};
use super::wiki::{
    wiki_dir_delete, wiki_file_delete, wiki_history, wiki_mkdir, wiki_read, wiki_search,
    wiki_stats, wiki_tags, wiki_tree, wiki_upload, wiki_write,
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

    /// Resolve a pending tool-permission request (mobile parity with the web
    /// WS `permission:response`). No-op by default.
    fn resolve_permission(&self, _request_id: &str, _option_key: &str) {}

    /// Resolve a pending ask-question batch. `answers` is keyed by question
    /// index → selected option index (or array for multi-select); `-1` means
    /// the "Other" free-text in `other_texts`.
    fn resolve_ask_question(
        &self,
        _request_id: &str,
        _answers: &serde_json::Value,
        _other_texts: Option<&serde_json::Value>,
    ) {
    }

    /// Resolve a pending FormUI form. `values` is keyed by field `key`;
    /// `submitted = false` means the user skipped. No-op by default.
    fn resolve_form(&self, _request_id: &str, _values: &serde_json::Value, _submitted: bool) {}

    /// Resolve a pending ExitPlanMode request. `selected` is
    /// `startEditing` | `clearContextAndStart` | (anything else = cancelled).
    fn resolve_plan_exit(&self, _group_jid: &str, _agent_id: &str, _selected: &str) {}
}

// ===== Shared state =====

pub struct UiState {
    pub config: Arc<Config>,
    pub db: Option<Arc<Db>>,
    pub group_manager: Option<Arc<crate::gateway::group_manager::GroupManager>>,
    pub wiki_manager: Option<Arc<WikiManager>>,
    pub persona_registry: Option<Arc<Mutex<crate::agent::persona_registry::PersonaRegistry>>>,
    pub agent_api: Option<Arc<dyn UiApi>>,
    pub mcp_manager: Option<Arc<McpManager>>,
    pub marketplace_manager: Option<Arc<Mutex<crate::marketplace::manager::MarketplaceManager>>>,
    pub workbench_bridge: Option<Arc<crate::agent::workbench_bridge::WorkbenchBridge>>,
    pub space_mcp_launcher: Option<Arc<super::space_mcp::SpaceMcpLauncher>>,
    pub workflow_service: Option<Arc<crate::workflow::WorkflowService>>,
    /// Headless agent runtime (tools + MCP + browser). Lets Space Apps run a
    /// full tool-enabled agent via the `agent.run` bridge action.
    pub virtual_worker_pool: Option<Arc<crate::agent::virtual_worker_pool::VirtualWorkerPool>>,
    /// Autonomous background work (no chat session). Backs `/api/background/*`.
    pub background_scheduler: Option<Arc<crate::background::BackgroundScheduler>>,
    /// Live per-group agent state map (`jid → "processing"/"idle"/…`), shared
    /// with the WebSocket gateway's `last_known_states`. Backs
    /// `GET /api/chat/states` so relay clients can reconcile after a drop.
    pub agent_states: Option<Arc<tokio::sync::Mutex<std::collections::HashMap<String, String>>>>,
    /// Token accounting sink for LLM calls the UI server brokers (bridge
    /// `llm.request`, internal draft completions). `None` in bare test setups.
    pub usage_recorder: Option<Arc<crate::usage::UsageRecorder>>,
    pub ws_port: u16,
    pub ws_token: String,
    /// API access-token policy. `ApiAuth::disabled()` for the default
    /// loopback bind; enforcing when the daemon is exposed beyond loopback.
    /// The middleware itself is layered at the serve sites (`lib.rs`,
    /// `start_ui_server`) — the relay bridge reuses `build_router` without it
    /// because relay frames are authenticated by relay pairing instead.
    pub api_auth: Arc<super::auth::ApiAuth>,
}

/// Return the web/dist directory, falling back to cwd-based path.
fn resolve_dist_dir() -> PathBuf {
    // Desktop app bundles web/dist as a resource and points here via env.
    if let Ok(dir) = std::env::var("SENCLAW_WEB_DIST") {
        let p = PathBuf::from(dir);
        if p.exists() {
            return p;
        }
    }
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

    // SPA fallback for client-side routes: if ServeDir can't resolve a
    // path to a real file, hand back index.html with HTTP 200 so
    // React-Router takes over. Without this any deep-link URL like
    // /chat/cowork:abc returns a hard 404 and the bundle never loads.
    let index_path = dist_dir.join("index.html");
    let spa_index = tower_http::services::ServeFile::new(&index_path);
    let serve_dir = ServeDir::new(&dist_dir)
        .precompressed_gzip()
        .precompressed_br()
        .fallback(spa_index);

    // OS-sandbox engine (`src/sandbox`): the whole Space-App REST surface,
    // nested under /api/sandbox. Carries its own state (the engine DB); when
    // the engine cannot open, the subtree answers 503 instead of vanishing.
    let sandbox_router = match crate::sandbox::shared_db() {
        Some(db) => crate::sandbox::api::api_router(crate::sandbox::state::AppState { db }),
        None => Router::new().fallback(|| async {
            (
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                axum::Json(serde_json::json!({
                    "error": "sandbox engine unavailable (data dir or DB failed to open — see daemon log)"
                })),
            )
        }),
    };

    // Token handshake for remote (non-loopback) clients. Routed on a plain
    // sub-router so the handlers see `Arc<ApiAuth>` state directly.
    let auth_router = Router::new()
        .route("/api/auth/login", post(super::auth::auth_login))
        .route("/api/auth/status", get(super::auth::auth_status))
        .with_state(Arc::clone(&state.api_auth));

    Router::new()
        // API endpoints
        .merge(auth_router)
        .nest_service("/api/sandbox", sandbox_router)
        .route("/api/config", get(config_handler))
        // Open a URL in the host machine's default browser (see open_url.rs).
        .route("/api/ui/open-url", post(open_url_handler))
        .route("/api/skills", get(skills_list))
        .route("/api/skills/remote-search", get(skills_remote_search))
        .route("/api/skills/create", post(skills_create))
        .route("/api/skills/install", post(skills_install))
        .route("/api/skills/:name", delete(skills_uninstall))
        .route(
            "/api/skills/:name/readme",
            get(skills_readme).put(skills_readme_save),
        )
        .route("/api/skills/:name/:action", post(skills_toggle))
        // ── Plugins API ──────────────────────────────────────────────────────
        .route("/api/plugins", get(plugins_list))
        .route("/api/plugins/remote-search", get(plugins_remote_search))
        .route("/api/plugins/install", post(plugins_install))
        .route(
            "/api/plugins/:slug",
            get(plugins_get).delete(plugins_uninstall),
        )
        .route("/api/plugins/:slug/enable", post(plugins_enable))
        .route("/api/plugins/:slug/disable", post(plugins_disable))
        .route("/api/plugins/:slug/configure", post(plugins_configure))
        // ── Marketplace API ──────────────────────────────────────────────────────
        .route(
            "/api/marketplace/sources",
            get(marketplace_sources_list).post(marketplace_sources_add),
        )
        .route(
            "/api/marketplace/sources/reorder",
            post(marketplace_sources_reorder),
        )
        .route(
            "/api/marketplace/sources/:id",
            get(marketplace_source_get).delete(marketplace_sources_delete),
        )
        .route(
            "/api/marketplace/sources/:id/sync",
            post(marketplace_sources_sync),
        )
        .route(
            "/api/marketplace/sources/:id/enable-all",
            post(marketplace_source_enable_all),
        )
        .route(
            "/api/marketplace/sources/:id/disable-all",
            post(marketplace_source_disable_all),
        )
        .route(
            "/api/marketplace/sources/:id/plugins/:name/toggle",
            post(marketplace_plugin_toggle),
        )
        .route(
            "/api/marketplace/sources/:id/catalog",
            get(marketplace_source_catalog),
        )
        .route(
            "/api/marketplace/sources/:id/plugins/:name/install",
            post(marketplace_plugin_install),
        )
        .route(
            "/api/marketplace/sources/:id/plugins/:name",
            delete(marketplace_plugin_uninstall),
        )
        .route(
            "/api/marketplace/sources/:id/plugins/:name/mcp/:server/use-tools",
            post(marketplace_mcp_use_tools),
        )
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
            "/api/agent-behavior",
            get(super::agent_behavior_config::agent_behavior_get)
                .post(super::agent_behavior_config::agent_behavior_set),
        )
        // Widget catalog + default-flow settings (Plugins → Widget).
        .route("/api/widgets", get(super::widgets::widgets_list))
        .route("/api/widgets/:id", put(super::widgets::widget_toggle))
        .route(
            "/api/defaults",
            get(super::widgets::defaults_get).put(super::widgets::defaults_set),
        )
        // Enabled marketplace plugins' widget assets (widgets/ dir).
        .route(
            "/api/marketplace/plugins/:name/widget-static/*path",
            get(super::marketplace::plugin_widget_static),
        )
        .route(
            "/api/dispatch-config",
            get(super::dispatch_config::dispatch_config_get)
                .post(super::dispatch_config::dispatch_config_set),
        )
        .route(
            "/api/admin-permissions",
            get(admin_perms_get).post(admin_perms_set),
        )
        .route("/api/quicknotes", post(quicknotes_save))
        // Workspace file discovery + folder creation
        .route("/api/workspace/files", get(super::workspace::list_files))
        .route("/api/chat/files", get(super::workspace::mention_files))
        .route("/api/workspace/file", get(super::workspace::read_file))
        .route("/api/ws/terminal", get(super::terminal::ws_terminal))
        .route("/api/workspace/mkdir", post(super::workspace::mkdir))
        // Profile file editor — SOUL.md + MEMORY.md per agent folder
        .route(
            "/api/agents/:folder/files",
            get(super::profile_files::get_files).put(super::profile_files::put_files),
        )
        // Workflows (saved DAGs of agent + script steps)
        .route(
            "/api/workflows",
            get(super::workflow::workflows_list).post(super::workflow::workflows_def_create),
        )
        .route(
            "/api/workflows/draft",
            post(super::workflow::workflows_draft),
        )
        .route(
            "/api/workflows/settings",
            get(super::workflow::workflows_settings_get)
                .put(super::workflow::workflows_settings_put),
        )
        .route("/api/workflows/runs", get(super::workflow::workflows_runs))
        .route(
            "/api/workflows/runs/:id",
            get(super::workflow::workflows_run_get)
                .patch(super::workflow::workflows_run_rename)
                .delete(super::workflow::workflows_run_delete),
        )
        .route(
            "/api/workflows/runs/:id/cancel",
            post(super::workflow::workflows_run_cancel),
        )
        .route(
            "/api/workflows/runs/:id/activity",
            get(super::workflow::workflows_run_activity),
        )
        .route(
            "/api/workflows/:name/run",
            post(super::workflow::workflows_run_start),
        )
        .route(
            "/api/workflows/:name/definition",
            get(super::workflow::workflows_def_get)
                .put(super::workflow::workflows_def_update)
                .patch(super::workflow::workflows_def_patch)
                .delete(super::workflow::workflows_def_delete),
        )
        .route(
            "/api/workflows/:name",
            delete(super::workflow::workflows_def_delete),
        )
        // Cowork teams (multi-agent dispatch)
        .route(
            "/api/cowork/teams",
            get(super::cowork::list_teams).post(super::cowork::create_team),
        )
        .route(
            "/api/cowork/teams/:id",
            patch(super::cowork::update_team).delete(super::cowork::delete_team),
        )
        .route(
            "/api/cowork/teams/:id/members",
            put(super::cowork::update_team_member),
        )
        .route(
            "/api/cowork/teams/:id/members/:folder",
            delete(super::cowork::remove_team_member),
        )
        .route(
            "/api/cowork/teams/:id/tasks",
            get(super::cowork::list_team_tasks).post(super::cowork::create_team_task),
        )
        .route(
            "/api/cowork/teams/:id/workspace",
            get(super::cowork::browse_team_workspace),
        )
        .route(
            "/api/cowork/teams/:team_id/tasks/:task_id",
            patch(super::cowork::update_team_task).delete(super::cowork::delete_team_task),
        )
        .route(
            "/api/cowork/teams/from-template",
            post(super::cowork::create_from_template),
        )
        .route(
            "/api/cowork/templates",
            get(super::cowork::list_templates).post(super::cowork::create_template),
        )
        .route(
            "/api/cowork/templates/:id",
            put(super::cowork::update_template).delete(super::cowork::delete_template),
        )
        .route(
            "/api/cowork/teams/:id/save-as-template",
            post(super::cowork::save_team_as_template),
        )
        .route("/api/cowork/personas", get(super::cowork::list_personas))
        .route(
            "/api/cowork/personas/:name/file",
            get(super::cowork::get_persona_file).put(super::cowork::put_persona_file),
        )
        // LLM config (specific routes before parameterized)
        .route(
            "/api/llm-config",
            get(llm_config_list).post(llm_config_create),
        )
        .route("/api/llm-config/active", post(llm_config_set_active))
        .route("/api/llm-config/test", post(llm_config_test))
        .route("/api/llm-config/models", post(llm_config_fetch_models))
        .route(
            "/api/llm-config/:id",
            delete(llm_config_delete).patch(llm_config_update),
        )
        // OAuth sign-in for subscription providers (Claude Code / Codex /
        // Antigravity). Responses are token-free; see ui_server::oauth.
        .route(
            "/api/oauth/providers",
            get(super::oauth::oauth_providers_list),
        )
        .route(
            "/api/oauth/accounts",
            get(super::oauth::oauth_accounts_list),
        )
        .route(
            "/api/oauth/accounts/:id",
            delete(super::oauth::oauth_account_delete),
        )
        .route(
            "/api/oauth/accounts/:id/refresh",
            post(super::oauth::oauth_account_refresh),
        )
        .route("/api/oauth/bind", post(super::oauth::oauth_bind_config))
        .route("/api/oauth/test-model", post(super::oauth::oauth_test_model))
        .route(
            "/api/oauth/accounts/:id/models",
            get(super::oauth::oauth_account_models),
        )
        .route("/api/oauth/flows/:id", get(super::oauth::oauth_flow_status))
        // Parameterized last so it cannot shadow the literal routes above.
        .route(
            "/api/oauth/:provider/start",
            post(super::oauth::oauth_start),
        )
        // Ready-made API-key provider presets (free tiers).
        .route(
            "/api/provider-catalog",
            get(super::oauth::provider_catalog_list),
        )
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
        .route("/api/local-models/:id/validate", get(local_models_validate))
        .route("/api/local-models/:id/status", get(local_models_status))
        .route("/api/local-models/:id/cancel", post(local_models_cancel))
        .route("/api/local-models/:id", delete(local_models_delete))
        .route("/api/local-models/:id/load", post(local_models_load))
        .route(
            "/api/local-models/:id/load-mlx",
            post(local_models_load_mlx),
        )
        .route("/api/local-models/:id/unload", post(local_models_unload))
        .route(
            "/api/local-models/unload-all",
            post(local_models_unload_all),
        )
        .route("/api/local-models/loaded", get(local_models_loaded_list))
        .route(
            "/api/local-models/:id/use-as-llm",
            post(local_models_use_as_llm),
        )
        // Whisper ASR management + transcription
        .route("/api/whisper/models", get(whisper_models_list))
        .route(
            "/api/whisper/settings",
            get(whisper_settings_get).put(whisper_settings_put),
        )
        .route("/api/whisper/models/:id/validate", get(whisper_validate))
        .route("/api/whisper/models/:id/download", post(whisper_download))
        .route("/api/whisper/models/:id/status", get(whisper_status))
        .route("/api/whisper/models/:id/cancel", post(whisper_cancel))
        .route("/api/whisper/models/:id", delete(whisper_delete))
        .route("/api/whisper/transcribe", post(whisper_transcribe))
        // TTS (Text-to-Speech) model management + synthesis
        .route("/api/tts/models", get(tts_models_list))
        .route(
            "/api/tts/settings",
            get(tts_settings_get).put(tts_settings_put),
        )
        .route("/api/tts/models/:id/validate", get(tts_validate))
        .route("/api/tts/models/:id/download", post(tts_download))
        .route("/api/tts/models/:id/status", get(tts_status))
        .route("/api/tts/models/:id/cancel", post(tts_cancel))
        .route("/api/tts/models/:id", delete(tts_delete))
        .route("/api/tts/synthesize", post(tts_synthesize))
        // OCR (PaddleOCR + MNN) model management + inference
        .route("/api/ocr/models", get(ocr_models_list))
        .route(
            "/api/ocr/settings",
            get(ocr_settings_get).put(ocr_settings_put),
        )
        .route("/api/ocr/models/:id/download", post(ocr_download))
        .route("/api/ocr/models/custom", post(ocr_custom_download))
        .route("/api/ocr/models/:id/status", get(ocr_status))
        .route("/api/ocr/models/:id/cancel", post(ocr_cancel))
        .route("/api/ocr/models/:id", delete(ocr_delete))
        .route("/api/ocr/recognize", post(ocr_recognize))
        // Code executor REPL — sandboxed JS via senclaw-js engine.
        .route("/api/code/run", post(code_run))
        // Code artifacts — publish/browse/run reusable snippets.
        .route(
            "/api/code/artifacts",
            get(list_artifacts).post(create_artifact),
        )
        .route(
            "/api/code/artifacts/:id",
            get(get_artifact)
                .put(update_artifact)
                .delete(delete_artifact),
        )
        .route("/api/code/artifacts/:id/run", post(run_artifact))
        // Embedding provider config
        .route(
            "/api/embedding-config",
            get(embedding_config_get).post(embedding_config_save),
        )
        // Cognitive config
        .route(
            "/api/cognitive-config",
            get(super::cognitive_config::cognitive_config_get)
                .post(super::cognitive_config::cognitive_config_save),
        )
        // Wiki API
        .route("/api/wiki/tree", get(wiki_tree))
        .route(
            "/api/wiki/file",
            get(wiki_read).put(wiki_write).delete(wiki_file_delete),
        )
        .route("/api/wiki/search", get(wiki_search))
        .route("/api/wiki/stats", get(wiki_stats))
        .route("/api/wiki/history", get(wiki_history))
        .route("/api/wiki/tags", get(wiki_tags))
        .route("/api/wiki/mkdir", post(wiki_mkdir))
        .route(
            "/api/wiki/upload",
            post(wiki_upload).layer(DefaultBodyLimit::max(12 * 1024 * 1024)),
        )
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
        // MCP tool aliases (Plugins → Alias): rename or override tools
        .route(
            "/api/tool-aliases",
            get(super::tool_aliases::aliases_list).post(super::tool_aliases::aliases_create),
        )
        .route(
            "/api/tool-aliases/:alias",
            axum::routing::put(super::tool_aliases::aliases_update)
                .delete(super::tool_aliases::aliases_delete),
        )
        .route(
            "/api/tool-aliases/:alias/enabled",
            post(super::tool_aliases::aliases_set_enabled),
        )
        // Hooks config
        .route("/api/hooks", get(hooks_get).put(hooks_put))
        // ── Space API ─────────────────────────────────────────────────────────
        // Notes
        .route(
            "/api/space/notes",
            get(space_notes_list).post(space_notes_create),
        )
        .route("/api/space/notes/search", get(space_notes_search))
        .route(
            "/api/space/notes/:id",
            axum::routing::put(space_notes_update).delete(space_notes_delete),
        )
        // Calendar
        .route(
            "/api/space/calendar/events",
            get(space_events_list).post(space_events_create),
        )
        .route(
            "/api/space/calendar/events/search",
            get(space_events_search),
        )
        .route(
            "/api/space/calendar/events/:id",
            get(space_events_get)
                .patch(space_events_update)
                .delete(space_events_delete),
        )
        .route(
            "/api/space/calendar/events/:id/reminder",
            post(space_events_set_reminder),
        )
        .route("/api/space/calendar/today", get(space_today_summary))
        // Tray screen captures (read-only; written by the desktop tray)
        .route("/api/space/screenshots/:name", get(space_screenshot_get))
        // AI-fill a captured shot's note fields (vision, or OCR → text LLM)
        .route(
            "/api/space/screenshots/extract",
            post(space_screenshot_extract),
        )
        // Schedules
        .route(
            "/api/space/schedules",
            get(space_schedules_list).post(space_schedules_create),
        )
        .route(
            "/api/space/schedules/:id",
            get(space_schedules_detail)
                .patch(space_schedules_update)
                .delete(space_schedules_cancel),
        )
        .route(
            "/api/space/schedules/:id/run-now",
            post(space_schedules_run_now),
        )
        // Token usage accounting (llm_usage_log / llm_usage_daily / pricing).
        .route("/api/usage/overview", get(super::usage::usage_overview))
        .route("/api/usage/daily", get(super::usage::usage_daily))
        .route("/api/usage/breakdown", get(super::usage::usage_breakdown))
        .route("/api/usage/log", get(super::usage::usage_log))
        .route(
            "/api/usage/pricing",
            get(super::usage::pricing_list).put(super::usage::pricing_upsert),
        )
        .route(
            "/api/usage/pricing/:model",
            delete(super::usage::pricing_delete),
        )
        // Background tasks — autonomous work, no chat session. Distinct from
        // the schedules above, which run in a chat and reply to a human.
        .route(
            "/api/background/tasks",
            get(super::background::list).post(super::background::create),
        )
        .route(
            "/api/background/parse",
            post(super::background::parse_quick),
        )
        .route(
            "/api/background/tasks/:id",
            get(super::background::detail)
                .patch(super::background::update)
                .delete(super::background::delete),
        )
        .route(
            "/api/background/tasks/:id/run-now",
            post(super::background::run_now),
        )
        .route(
            "/api/background/tasks/:id/runs",
            get(super::background::runs),
        )
        .route(
            "/api/background/runs/:id",
            get(super::background::run_detail),
        )
        .route(
            "/api/background/runs/:id/cancel",
            post(super::background::cancel_run),
        )
        .route("/api/background/stats", get(super::background::stats))
        // Apps
        .route("/api/space/apps", get(space_apps_list))
        .route("/api/space/apps/updates", get(space_apps_updates))
        .route("/api/space/apps/register", post(space_apps_register))
        .route(
            "/api/space/apps/register-local",
            post(space_apps_register_local),
        )
        .route(
            "/api/space/apps/install-zip",
            // Server-app ZIPs (Next.js standalone) are tens of MB — raise the
            // default 2 MB body limit to the handler's 50 MB cap (+slack).
            post(space_apps_install_zip).layer(DefaultBodyLimit::max(64 * 1024 * 1024)),
        )
        .route("/api/space/apps/:id/env", get(space_app_env))
        .route(
            "/api/space/apps/:id/runtime",
            get(super::space_runtime::space_app_runtime),
        )
        // Literal segment, so it wins over `:id` — same shape as the existing
        // `/api/space/apps/updates`.
        .route(
            "/api/space/apps/sandbox-overview",
            get(super::space_runtime::space_apps_sandbox_overview),
        )
        .route(
            "/api/space/apps/:id/sandbox",
            get(space_app_sandbox_get).put(space_app_sandbox_put),
        )
        .route("/api/space/apps/:id/config", get(space_app_config_list))
        .route(
            "/api/space/apps/:id/config/:key",
            get(space_app_config_get)
                .put(space_app_config_set)
                .delete(space_app_config_delete),
        )
        .route(
            "/api/space/apps/:id/sqlite/query",
            post(space_app_sqlite_query),
        )
        .route("/api/space/apps/:id/mcp", get(space_app_mcp_info))
        .route(
            "/api/space/apps/:id/mcp/register",
            post(space_app_mcp_register),
        )
        .route(
            "/api/space/apps/:id/logs",
            get(space_app_logs_get).delete(space_app_logs_clear),
        )
        .route("/api/space/apps/:id/bridge", post(space_apps_bridge))
        .route("/api/space/apps/:id/static/*path", get(space_apps_static))
        .route(
            "/api/space/apps/:id/proxy/*path",
            axum::routing::any(space_apps_proxy),
        )
        .route(
            "/api/space/apps/:id/proxy/",
            axum::routing::any(space_apps_proxy_root),
        )
        .route(
            "/api/space/apps/:id/proxy",
            axum::routing::any(space_apps_proxy_root),
        )
        .route("/api/space/apps/:id", delete(space_apps_delete))
        .route("/api/space/apps/:id/update", post(space_apps_update))
        .route("/api/space/apps/:id/restart", post(space_apps_restart))
        // External sync
        .route(
            "/api/space/sync/google-calendar",
            post(space_sync_google_calendar),
        )
        .route(
            "/api/space/sync/google-workspace",
            post(space_sync_google_workspace),
        )
        .route(
            "/api/space/sync/apple-calendar",
            post(space_sync_apple_calendar),
        )
        .route("/api/space/sync/apple-notes", post(space_sync_apple_notes))
        // ── Chat interaction resolve (mobile parity with WS permission/question) ─
        .route(
            "/api/chat/permission/respond",
            post(chat_permission_respond),
        )
        .route("/api/chat/question/respond", post(chat_question_respond))
        .route("/api/chat/form/respond", post(chat_form_respond))
        .route("/api/chat/plan/respond", post(chat_plan_respond))
        // ── Chat sync for relay clients (delta history + agent-state snapshot) ──
        .route("/api/chat/history", get(chat_history))
        .route("/api/chat/states", get(chat_states))
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
        .route(
            "/api/cognitive/stats",
            get(super::cognitive::cognitive_stats),
        )
        .route(
            "/api/cognitive/spaces",
            get(super::cognitive::cognitive_spaces),
        )
        .route(
            "/api/cognitive/nodes",
            get(super::cognitive::cognitive_list_nodes),
        )
        .route(
            "/api/cognitive/node/:id",
            get(super::cognitive::cognitive_get_node).delete(super::cognitive::cognitive_forget),
        )
        .route(
            "/api/cognitive/node/:id/re-extract",
            post(super::cognitive::cognitive_re_extract),
        )
        .route(
            "/api/cognitive/re-extract-pending",
            post(super::cognitive::cognitive_re_extract_pending),
        )
        .route(
            "/api/cognitive/decay-log",
            get(super::cognitive::cognitive_decay_log),
        )
        .route(
            "/api/cognitive/search",
            post(super::cognitive::cognitive_search),
        )
        .route(
            "/api/cognitive/recall",
            post(super::cognitive::cognitive_recall),
        )
        .route("/api/cognitive/add", post(super::cognitive::cognitive_add))
        .route(
            "/api/cognitive/upload",
            post(super::cognitive::cognitive_upload).layer(DefaultBodyLimit::max(12 * 1024 * 1024)),
        )
        .route(
            "/api/cognitive/subgraph",
            get(super::cognitive::cognitive_subgraph),
        )
        .route(
            "/api/cognitive/top-nodes",
            get(super::cognitive::cognitive_top_nodes),
        )
        .route(
            "/api/cognitive/sample",
            get(super::cognitive::cognitive_sample),
        )
        .route(
            "/api/cognitive/full-graph",
            get(super::cognitive::cognitive_full_graph),
        )
        .route(
            "/api/cognitive/cleanup",
            post(super::cognitive::cognitive_cleanup),
        )
        .route(
            "/api/cognitive/maintenance",
            post(super::cognitive::cognitive_maintenance),
        )
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
        // Static files. ServeDir handles real assets (/assets/*, favicon,
        // etc.); paths it can't resolve fall through to the SPA fallback
        // which serves the right HTML shell with HTTP 200.
        .nest_service("/", serve_dir)
        // SPA fallback — must return 200 so React-Router can take over.
        .fallback(get(move |uri: axum::http::Uri| {
            spa_fallback(dist_dir.clone(), uri)
        }))
        // Loopback-origin allowlist. The old `CorsLayer::permissive()` here
        // (ACAO `*`) let any web page the user visited read API responses off
        // the loopback daemon — /api/llm-config serves cleartext provider
        // keys. See auth::restrictive_cors.
        .layer(super::auth::restrictive_cors())
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

/// Start the UI HTTP server on the configured port. Binds to
/// `ui_server.bind_host` (default 127.0.0.1); non-loopback binds are token-
/// gated by the auth middleware.
pub async fn start_ui_server(state: Arc<UiState>, port: u16) -> Result<()> {
    let host = state.config.ui_server.bind_host.clone();
    let api_auth = Arc::clone(&state.api_auth);
    let router = build_router(state).layer(axum::middleware::from_fn_with_state(
        api_auth,
        super::auth::http_auth_mw,
    ));
    let addr = format!("{host}:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("bind {addr}"))?;
    tracing::info!("[UIServer] Web UI at http://{addr}");
    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;
    Ok(())
}
