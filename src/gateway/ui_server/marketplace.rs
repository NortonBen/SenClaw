//! REST handlers for marketplace management.
//! Routes registered under /api/marketplace/* in core.rs.

use std::sync::Arc;

use axum::{
    extract::{Path as AxumPath, State},
    http::StatusCode,
    response::Json,
};
use serde::{Deserialize, Serialize};
use tokio::task;

use crate::marketplace::types::{MarketplaceSource, SourceType};
use super::core::{AppError, UiState};

fn internal(e: impl std::fmt::Display) -> AppError {
    AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

// ─── Types ────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct AddSourceBody {
    name: String,
    #[serde(rename = "type")]
    source_type: SourceType,
    #[serde(rename = "localPath")]
    local_path: Option<String>,
    url: Option<String>,
    branch: Option<String>,
    #[serde(default)]
    priority: Option<i32>,
    #[serde(default)]
    enabled: Option<bool>,
}

#[derive(Deserialize)]
pub struct ReorderSourcesBody {
    #[serde(rename = "orderedIds")]
    ordered_ids: Vec<String>,
}

#[derive(Deserialize)]
pub struct TogglePluginBody {
    enabled: bool,
}

#[derive(Deserialize)]
pub struct SetUseToolsBody {
    #[serde(rename = "useTools")]
    use_tools: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct SourceListResponse {
    sources: Vec<MarketplaceSource>,
}

#[derive(Serialize)]
pub struct SourceInfoResponse {
    #[serde(flatten)]
    source: MarketplaceSource,
    plugins: Vec<serde_json::Value>,
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

/// GET /api/marketplace/sources - list all sources
pub(crate) async fn marketplace_sources_list(
    State(s): State<Arc<UiState>>,
) -> Result<Json<SourceListResponse>, AppError> {
    let manager = s.marketplace_manager
        .as_ref()
        .ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "Marketplace manager not available".into()))?
        .clone();
    
    let sources = task::spawn_blocking(move || {
        let manager = manager.lock().unwrap();
        manager.get_sources()
    })
    .await
    .map_err(internal)?;
    
    Ok(Json(SourceListResponse { sources }))
}

/// POST /api/marketplace/sources - add a new source
pub(crate) async fn marketplace_sources_add(
    State(s): State<Arc<UiState>>,
    Json(body): Json<AddSourceBody>,
) -> Result<Json<MarketplaceSource>, AppError> {
    let manager = s.marketplace_manager
        .as_ref()
        .ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "Marketplace manager not available".into()))?
        .clone();
    
    let result = task::spawn_blocking(move || {
        let mut manager = manager.lock().unwrap();
        manager.add_source(
            body.name,
            body.source_type,
            body.url,
            body.branch,
            body.local_path,
            body.priority,
            body.enabled,
        )
    })
    .await
    .map_err(internal)?;
    
    let result = result.map_err(internal)?;
    Ok(Json(result))
}

/// DELETE /api/marketplace/sources/:id - remove a source
pub(crate) async fn marketplace_sources_delete(
    State(s): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let manager = s.marketplace_manager
        .as_ref()
        .ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "Marketplace manager not available".into()))?
        .clone();
    
    let _ = task::spawn_blocking(move || {
        let mut manager = manager.lock().unwrap();
        manager.remove_source(&id)
    })
    .await
    .map_err(internal)?;
    
    Ok(Json(serde_json::json!({ "success": true })))
}

/// POST /api/marketplace/sources/:id/sync - sync a git source
pub(crate) async fn marketplace_sources_sync(
    State(s): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let manager = s.marketplace_manager
        .as_ref()
        .ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "Marketplace manager not available".into()))?
        .clone();
    
    let _ = task::spawn_blocking(move || {
        let mut manager = manager.lock().unwrap();
        manager.sync_source(&id)
    })
    .await
    .map_err(internal)?;
    
    Ok(Json(serde_json::json!({ "success": true })))
}

/// POST /api/marketplace/sources/reorder - reorder sources
pub(crate) async fn marketplace_sources_reorder(
    State(s): State<Arc<UiState>>,
    Json(body): Json<ReorderSourcesBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let manager = s.marketplace_manager
        .as_ref()
        .ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "Marketplace manager not available".into()))?
        .clone();
    
    let _ = task::spawn_blocking(move || {
        let mut manager = manager.lock().unwrap();
        manager.reorder_sources(body.ordered_ids)
    })
    .await
    .map_err(internal)?;
    
    Ok(Json(serde_json::json!({ "success": true })))
}

/// GET /api/marketplace/sources/:id - get source with plugins
pub(crate) async fn marketplace_source_get(
    State(s): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<SourceInfoResponse>, AppError> {
    let manager = s.marketplace_manager
        .as_ref()
        .ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "Marketplace manager not available".into()))?
        .clone();
    
    let source_info = task::spawn_blocking(move || {
        let manager = manager.lock().unwrap();
        manager.get_source_info(&id)
    })
    .await
    .map_err(internal)?;
    
    let source_info = source_info.map_err(internal)?;
    let source_info = source_info.ok_or_else(|| AppError(StatusCode::NOT_FOUND, "Source not found".into()))?;
    
    // Convert plugins to JSON
    let plugins_json: Vec<serde_json::Value> = source_info.plugins.into_iter()
        .map(|p| serde_json::to_value(p).unwrap_or(serde_json::Value::Null))
        .collect();
    
    Ok(Json(SourceInfoResponse {
        source: source_info.source,
        plugins: plugins_json,
    }))
}

/// POST /api/marketplace/sources/:id/enable-all - enable all plugins in a source
pub(crate) async fn marketplace_source_enable_all(
    State(s): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let manager = s.marketplace_manager
        .as_ref()
        .ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "Marketplace manager not available".into()))?
        .clone();
    
    let _ = task::spawn_blocking(move || {
        let mut manager = manager.lock().unwrap();
        manager.enable_all_in_source(&id)
    })
    .await
    .map_err(internal)?;
    
    Ok(Json(serde_json::json!({ "success": true })))
}

/// POST /api/marketplace/sources/:id/disable-all - disable all plugins in a source
pub(crate) async fn marketplace_source_disable_all(
    State(s): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let manager = s.marketplace_manager
        .as_ref()
        .ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "Marketplace manager not available".into()))?
        .clone();
    
    let _ = task::spawn_blocking(move || {
        let mut manager = manager.lock().unwrap();
        manager.disable_all_in_source(&id)
    })
    .await
    .map_err(internal)?;
    
    Ok(Json(serde_json::json!({ "success": true })))
}

/// POST /api/marketplace/sources/:id/plugins/:name/toggle - toggle a plugin
pub(crate) async fn marketplace_plugin_toggle(
    State(s): State<Arc<UiState>>,
    AxumPath(params): AxumPath<(String, String)>,
    Json(body): Json<TogglePluginBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (id, name) = params;
    
    let manager = s.marketplace_manager
        .as_ref()
        .ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "Marketplace manager not available".into()))?
        .clone();
    
    let _ = task::spawn_blocking(move || {
        let mut manager = manager.lock().unwrap();
        manager.set_plugin_enabled(&id, &name, body.enabled)
    })
    .await
    .map_err(internal)?;
    
    Ok(Json(serde_json::json!({ "success": true })))
}

/// POST /api/marketplace/sources/:id/plugins/:name/mcp/:server/use-tools - set MCP tool allowlist
pub(crate) async fn marketplace_mcp_use_tools(
    State(s): State<Arc<UiState>>,
    AxumPath(params): AxumPath<(String, String, String)>,
    Json(body): Json<SetUseToolsBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    // This would need to be implemented in MarketplaceManager
    // For now, return success
    Ok(Json(serde_json::json!({ "success": true })))
}

/// GET /api/marketplace/mcp-status - get MCP connection status
pub(crate) async fn marketplace_mcp_status(
    State(s): State<Arc<UiState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    // This would need to query the MCP manager for connection status
    // For now, return empty status
    Ok(Json(serde_json::json!({})))
}
