use std::sync::Arc;

use axum::{extract::State, http::StatusCode, response::Json};
use serde::Deserialize;

use crate::gateway::group_manager::{
    get_admin_permissions_config, get_thinking_enabled, save_admin_permissions_config,
    save_thinking_enabled, AdminPermissions,
};

use super::core::{AppError, UiState};
use super::types::AdminPermissionsConfig;

// ===== /api/config =====

pub(crate) async fn config_handler(State(s): State<Arc<UiState>>) -> Json<serde_json::Value> {
    let admin_perms = get_admin_permissions_config(&s.config.paths.global_config_path);
    Json(serde_json::json!({
        "wsPort": s.ws_port,
        "token": s.ws_token,
        "thinkingEnabled": get_thinking_enabled(&s.config.paths.global_config_path),
        "skipMainAgentPermissions": admin_perms.skip_main_agent_permissions,
        "skipAllAgentsPermissions": admin_perms.skip_all_agents_permissions,
    }))
}

// ===== /api/thinking =====

#[derive(Deserialize)]
pub(crate) struct ThinkingBody {
    enabled: bool,
}

pub(crate) async fn thinking_handler(
    State(s): State<Arc<UiState>>,
    Json(body): Json<ThinkingBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    save_thinking_enabled(&s.config.paths.global_config_path, body.enabled)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if let Some(ref api) = s.agent_api {
        api.set_thinking_enabled(body.enabled);
    }
    Ok(Json(serde_json::json!({ "thinkingEnabled": body.enabled })))
}

// ===== /api/admin-permissions =====

pub(crate) async fn admin_perms_get(State(s): State<Arc<UiState>>) -> Json<serde_json::Value> {
    let cfg = get_admin_permissions_config(&s.config.paths.global_config_path);
    Json(serde_json::json!({
        "skipMainAgentPermissions": cfg.skip_main_agent_permissions,
        "skipAllAgentsPermissions": cfg.skip_all_agents_permissions,
    }))
}

pub(crate) async fn admin_perms_set(
    State(s): State<Arc<UiState>>,
    Json(body): Json<AdminPermissionsConfig>,
) -> Result<Json<serde_json::Value>, AppError> {
    let perm = AdminPermissions {
        skip_main_agent_permissions: body.skip_main_agent_permissions,
        skip_all_agents_permissions: body.skip_all_agents_permissions,
    };
    save_admin_permissions_config(&s.config.paths.global_config_path, &perm)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if let Some(ref api) = s.agent_api {
        api.set_permissions_config(body.clone());
    }
    Ok(Json(serde_json::to_value(body).unwrap_or_default()))
}
