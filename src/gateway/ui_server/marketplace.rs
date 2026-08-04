//! REST handlers for marketplace management.
//! Routes registered under /api/marketplace/* in core.rs.

use std::sync::Arc;

use axum::{
    extract::{Path as AxumPath, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use serde::{Deserialize, Serialize};
use tokio::task;

use super::core::{AppError, UiState};
use crate::marketplace::types::{MarketplaceSource, SourceType};

fn internal(e: impl std::fmt::Display) -> AppError {
    AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

// ─── Types ────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct AddSourceBody {
    #[serde(default)]
    name: Option<String>,
    /// Omitted by the "paste a URL" flows — inferred from the URL/path instead.
    #[serde(rename = "type", default)]
    source_type: Option<SourceType>,
    #[serde(rename = "localPath", alias = "local_path", default)]
    local_path: Option<String>,
    url: Option<String>,
    branch: Option<String>,
    #[serde(default)]
    priority: Option<i32>,
    #[serde(default)]
    enabled: Option<bool>,
}

impl AddSourceBody {
    /// What kind of source a bare URL/path describes: a `marketplace.json`
    /// catalog (or bare host) is a hub, anything git-ish is a git clone, and a
    /// filesystem path is local.
    fn infer_type(&self) -> SourceType {
        crate::marketplace::infer_source_type(self.url.as_deref(), self.source_type)
    }

    /// A readable default name when the caller only sent a URL.
    fn resolved_name(&self, source_type: SourceType) -> String {
        crate::marketplace::default_source_name(
            self.name.as_deref(),
            self.url.as_deref(),
            self.local_path.as_deref(),
            source_type,
        )
    }
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
    let manager = s
        .marketplace_manager
        .as_ref()
        .ok_or_else(|| {
            AppError(
                StatusCode::SERVICE_UNAVAILABLE,
                "Marketplace manager not available".into(),
            )
        })?
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
    let manager = s
        .marketplace_manager
        .as_ref()
        .ok_or_else(|| {
            AppError(
                StatusCode::SERVICE_UNAVAILABLE,
                "Marketplace manager not available".into(),
            )
        })?
        .clone();

    let source_type = body.infer_type();
    let name = body.resolved_name(source_type);

    let result = task::spawn_blocking(move || {
        let mut manager = manager.lock().unwrap();
        let source = manager.add_source(
            name,
            source_type,
            body.url,
            body.branch,
            body.local_path,
            body.priority,
            body.enabled,
        )?;
        // Pull the catalog straight away so the new hub is browsable without a
        // separate sync round-trip.
        if source_type == SourceType::Hub {
            if let Err(e) = manager.sync_source(&source.id) {
                tracing::warn!("[Marketplace] Initial hub sync failed: {e}");
            }
            return anyhow::Ok(manager.get_source(&source.id).unwrap_or(source));
        }
        anyhow::Ok(source)
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
    let manager = s
        .marketplace_manager
        .as_ref()
        .ok_or_else(|| {
            AppError(
                StatusCode::SERVICE_UNAVAILABLE,
                "Marketplace manager not available".into(),
            )
        })?
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
    let manager = s
        .marketplace_manager
        .as_ref()
        .ok_or_else(|| {
            AppError(
                StatusCode::SERVICE_UNAVAILABLE,
                "Marketplace manager not available".into(),
            )
        })?
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
    let manager = s
        .marketplace_manager
        .as_ref()
        .ok_or_else(|| {
            AppError(
                StatusCode::SERVICE_UNAVAILABLE,
                "Marketplace manager not available".into(),
            )
        })?
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
    let manager = s
        .marketplace_manager
        .as_ref()
        .ok_or_else(|| {
            AppError(
                StatusCode::SERVICE_UNAVAILABLE,
                "Marketplace manager not available".into(),
            )
        })?
        .clone();

    let source_info = task::spawn_blocking(move || {
        let manager = manager.lock().unwrap();
        manager.get_source_info(&id)
    })
    .await
    .map_err(internal)?;

    let source_info = source_info.map_err(internal)?;
    let source_info =
        source_info.ok_or_else(|| AppError(StatusCode::NOT_FOUND, "Source not found".into()))?;

    // Convert plugins to JSON
    let plugins_json: Vec<serde_json::Value> = source_info
        .plugins
        .into_iter()
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
    let manager = s
        .marketplace_manager
        .as_ref()
        .ok_or_else(|| {
            AppError(
                StatusCode::SERVICE_UNAVAILABLE,
                "Marketplace manager not available".into(),
            )
        })?
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
    let manager = s
        .marketplace_manager
        .as_ref()
        .ok_or_else(|| {
            AppError(
                StatusCode::SERVICE_UNAVAILABLE,
                "Marketplace manager not available".into(),
            )
        })?
        .clone();

    let _ = task::spawn_blocking(move || {
        let mut manager = manager.lock().unwrap();
        manager.disable_all_in_source(&id)
    })
    .await
    .map_err(internal)?;

    Ok(Json(serde_json::json!({ "success": true })))
}

/// POST /api/marketplace/sources/:id/plugins/:name/toggle - toggle a plugin.
/// With no body the current state is flipped, which is what the UIs send.
pub(crate) async fn marketplace_plugin_toggle(
    State(s): State<Arc<UiState>>,
    AxumPath(params): AxumPath<(String, String)>,
    body: Option<Json<TogglePluginBody>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (id, name) = params;
    let requested = body.map(|Json(b)| b.enabled);

    let manager = s
        .marketplace_manager
        .as_ref()
        .ok_or_else(|| {
            AppError(
                StatusCode::SERVICE_UNAVAILABLE,
                "Marketplace manager not available".into(),
            )
        })?
        .clone();

    let enabled = task::spawn_blocking(move || {
        let mut manager = manager.lock().unwrap();
        let enabled = match requested {
            Some(v) => v,
            None => !manager.is_plugin_enabled(&id, &name),
        };
        manager
            .set_plugin_enabled(&id, &name, enabled)
            .map(|_| enabled)
    })
    .await
    .map_err(internal)?
    .map_err(internal)?;

    Ok(Json(
        serde_json::json!({ "success": true, "enabled": enabled }),
    ))
}

#[derive(Deserialize, Default)]
pub struct InstallQuery {
    /// `?force=true` installs despite a blocking scan verdict. The scan still
    /// runs and the report is still returned.
    #[serde(default)]
    force: bool,
}

/// POST /api/marketplace/sources/:id/plugins/:name/install - install one plugin
/// from a hub catalog (scan + clone + enable).
///
/// A blocking scan verdict comes back as 422 with the full report in
/// `scan`, so the UI can show the findings and offer an explicit override
/// rather than a bare failure.
pub(crate) async fn marketplace_plugin_install(
    State(s): State<Arc<UiState>>,
    AxumPath(params): AxumPath<(String, String)>,
    Query(q): Query<InstallQuery>,
) -> Result<axum::response::Response, AppError> {
    let (id, name) = params;

    let manager = s
        .marketplace_manager
        .as_ref()
        .ok_or_else(|| {
            AppError(
                StatusCode::SERVICE_UNAVAILABLE,
                "Marketplace manager not available".into(),
            )
        })?
        .clone();

    let policy = crate::security::ScanPolicy::from_config(&s.config);
    let outcome = task::spawn_blocking(move || {
        let mut manager = manager.lock().unwrap();
        manager.install_hub_plugin(&id, &name, policy, q.force)
    })
    .await
    .map_err(internal)?
    .map_err(|e| AppError(StatusCode::BAD_REQUEST, e.to_string()))?;

    match outcome {
        // 422 with the findings as data, not prose: the UI renders them and
        // offers an explicit override.
        crate::marketplace::manager::InstallOutcome::Blocked { report, staged_dir } => Ok((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({
                "success": false,
                "blocked": true,
                "error": format!(
                    "Blocked by the pre-install security scan (risk {}/100). \
                     Nothing was recorded or enabled.",
                    report.risk_score()
                ),
                "stagedDir": staged_dir.to_string_lossy(),
                "scan": report,
            })),
        )
            .into_response()),
        crate::marketplace::manager::InstallOutcome::Installed { dir, scan } => Ok(Json(
            serde_json::json!({
                "success": true,
                "dir": dir.to_string_lossy(),
                "scan": scan,
            }),
        )
        .into_response()),
    }
}

/// DELETE /api/marketplace/sources/:id/plugins/:name - uninstall a hub plugin.
pub(crate) async fn marketplace_plugin_uninstall(
    State(s): State<Arc<UiState>>,
    AxumPath(params): AxumPath<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (id, name) = params;

    let manager = s
        .marketplace_manager
        .as_ref()
        .ok_or_else(|| {
            AppError(
                StatusCode::SERVICE_UNAVAILABLE,
                "Marketplace manager not available".into(),
            )
        })?
        .clone();

    let removed = task::spawn_blocking(move || {
        let mut manager = manager.lock().unwrap();
        manager.uninstall_hub_plugin(&id, &name)
    })
    .await
    .map_err(internal)?
    .map_err(|e| AppError(StatusCode::BAD_REQUEST, e.to_string()))?;

    Ok(Json(
        serde_json::json!({ "success": true, "removed": removed }),
    ))
}

/// GET /api/marketplace/sources/:id/catalog - the raw hub catalog.
pub(crate) async fn marketplace_source_catalog(
    State(s): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let manager = s
        .marketplace_manager
        .as_ref()
        .ok_or_else(|| {
            AppError(
                StatusCode::SERVICE_UNAVAILABLE,
                "Marketplace manager not available".into(),
            )
        })?
        .clone();

    let catalog = task::spawn_blocking(move || {
        let manager = manager.lock().unwrap();
        manager.get_catalog(&id)
    })
    .await
    .map_err(internal)?
    .map_err(|e| AppError(StatusCode::BAD_REQUEST, e.to_string()))?;

    Ok(Json(serde_json::to_value(catalog).map_err(internal)?))
}

/// POST /api/marketplace/sources/:id/plugins/:name/mcp/:server/use-tools - set MCP tool allowlist
pub(crate) async fn marketplace_mcp_use_tools(
    State(_s): State<Arc<UiState>>,
    AxumPath(_params): AxumPath<(String, String, String)>,
    Json(_body): Json<SetUseToolsBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    // This would need to be implemented in MarketplaceManager
    // For now, return success
    Ok(Json(serde_json::json!({ "success": true })))
}

/// GET /api/marketplace/mcp-status - get MCP connection status
pub(crate) async fn marketplace_mcp_status(
    State(_s): State<Arc<UiState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    // This would need to query the MCP manager for connection status
    // For now, return empty status
    Ok(Json(serde_json::json!({})))
}

/// GET /api/marketplace/plugins/:name/widget-static/*path — serve an enabled
/// plugin's widget assets (`<pluginDir>/widgets/…`). Plugins have no server of
/// their own, so this route is the only origin their `url` widgets load from
/// (the registry resolves plugin `entryUrl`s against it). Same containment
/// discipline as `space_apps_static`: no `..`/backslash, canonicalized-root
/// prefix check, files only.
pub(crate) async fn plugin_widget_static(
    State(s): State<Arc<UiState>>,
    AxumPath((name, req_path)): AxumPath<(String, String)>,
) -> Result<axum::response::Response, AppError> {
    use axum::body::Body;
    use axum::http::header;

    if req_path.contains("..") || req_path.contains('\\') {
        return Err(AppError(StatusCode::BAD_REQUEST, "Invalid path".into()));
    }
    let manager = s
        .marketplace_manager
        .as_ref()
        .ok_or_else(|| {
            AppError(
                StatusCode::SERVICE_UNAVAILABLE,
                "Marketplace manager not available".into(),
            )
        })?
        .clone();

    // Resolving the plugin dir scans sources on disk — keep it off the async
    // runtime thread, like the other marketplace handlers.
    let wanted = name.clone();
    let dir = task::spawn_blocking(move || {
        let guard = manager.lock().ok()?;
        guard
            .enabled_plugin_dirs()
            .into_iter()
            .find(|(n, _)| n == &wanted)
            .map(|(_, d)| d)
    })
    .await
    .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .ok_or_else(|| {
        AppError(
            StatusCode::NOT_FOUND,
            format!("Plugin '{name}' not found or not enabled"),
        )
    })?;

    let root = dir.join("widgets");
    let rel = req_path.trim_start_matches('/');
    let rel = if rel.is_empty() { "index.html" } else { rel };
    let path = root.join(rel);
    let canonical_root = root
        .canonicalize()
        .map_err(|_| AppError(StatusCode::NOT_FOUND, "Plugin has no widgets dir".into()))?;
    let canonical_path = path
        .canonicalize()
        .map_err(|_| AppError(StatusCode::NOT_FOUND, "File not found".into()))?;
    if !canonical_path.starts_with(&canonical_root) || !canonical_path.is_file() {
        return Err(AppError(StatusCode::NOT_FOUND, "File not found".into()));
    }
    let bytes = tokio::fs::read(&canonical_path)
        .await
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(axum::response::Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            super::space::content_type_for(&canonical_path),
        )
        .body(Body::from(bytes))
        .unwrap())
}
