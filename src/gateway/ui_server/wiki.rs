use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use serde::Deserialize;

use crate::wiki::manager::WikiManager;

use super::core::{AppError, UiState};

// ===== Wiki helper =====

pub(crate) fn wiki_manager(s: &UiState) -> Result<&WikiManager, AppError> {
    s.wiki_manager.as_ref().map(|w| w.as_ref()).ok_or_else(|| {
        AppError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Wiki not initialized".into(),
        )
    })
}

// ===== Wiki API handlers =====

pub(crate) async fn wiki_tree(
    State(s): State<Arc<UiState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let wm = wiki_manager(&s)?;
    let tree = wm
        .get_tree()
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "tree": tree })))
}

#[derive(Deserialize)]
pub(crate) struct WikiFileQuery {
    path: Option<String>,
}

pub(crate) async fn wiki_read(
    State(s): State<Arc<UiState>>,
    Query(q): Query<WikiFileQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let path = q
        .path
        .as_deref()
        .ok_or_else(|| AppError(StatusCode::BAD_REQUEST, "Missing path".into()))?;
    let wm = wiki_manager(&s)?;
    let doc = wm
        .read_file(path)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let json = serde_json::json!({
        "path": doc.path,
        "content": doc.content,
        "frontmatter": doc.frontmatter,
        "gitLog": doc.git_log,
    });
    Ok(Json(json))
}

#[derive(Deserialize)]
pub(crate) struct WikiWriteBody {
    path: String,
    content: String,
    #[serde(rename = "commitMsg")]
    commit_msg: Option<String>,
    source: Option<String>,
    tags: Option<Vec<String>>,
}

pub(crate) async fn wiki_write(
    State(s): State<Arc<UiState>>,
    Json(body): Json<WikiWriteBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    if body.path.is_empty() || body.content.is_empty() {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            "Missing path or content".into(),
        ));
    }
    let wm = wiki_manager(&s)?;
    wm.write_file(
        &body.path,
        &body.content,
        body.source.as_deref(),
        body.tags.as_deref(),
        body.commit_msg.as_deref(),
    )
    .await
    .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({
        "path": body.path,
        "updated": chrono::Utc::now().to_rfc3339(),
    })))
}

#[derive(Deserialize)]
pub(crate) struct WikiSearchQuery {
    q: Option<String>,
    tags: Option<String>,
    limit: Option<usize>,
}

pub(crate) async fn wiki_search(
    State(s): State<Arc<UiState>>,
    Query(q): Query<WikiSearchQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let query = q.q.unwrap_or_default();
    let tags: Option<Vec<String>> = q.tags.map(|t| {
        t.split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    });
    let limit = q.limit.unwrap_or(20);
    let wm = wiki_manager(&s)?;
    let results = wm
        .search(&query, tags.as_deref(), Some(limit))
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "results": results })))
}

pub(crate) async fn wiki_stats(
    State(s): State<Arc<UiState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let wm = wiki_manager(&s)?;
    let stats = wm
        .get_stats()
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::to_value(stats).unwrap_or_default()))
}

#[derive(Deserialize)]
pub(crate) struct WikiHistoryQuery {
    path: Option<String>,
    limit: Option<usize>,
}

pub(crate) async fn wiki_history(
    State(s): State<Arc<UiState>>,
    Query(q): Query<WikiHistoryQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let path = q
        .path
        .as_deref()
        .ok_or_else(|| AppError(StatusCode::BAD_REQUEST, "Missing path".into()))?;
    let wm = wiki_manager(&s)?;
    let commits = wm
        .get_history(path, q.limit)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "commits": commits })))
}

pub(crate) async fn wiki_tags(
    State(s): State<Arc<UiState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let wm = wiki_manager(&s)?;
    let tags = wm.get_tags();
    Ok(Json(serde_json::json!({ "tags": tags })))
}

#[derive(Deserialize)]
pub(crate) struct WikiMkdirBody {
    path: Option<String>,
}

pub(crate) async fn wiki_mkdir(
    State(s): State<Arc<UiState>>,
    Json(body): Json<WikiMkdirBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let path = body
        .path
        .as_deref()
        .ok_or_else(|| AppError(StatusCode::BAD_REQUEST, "Missing path".into()))?;
    let wm = wiki_manager(&s)?;
    wm.mkdir(path)
        .await
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "path": path })))
}

#[derive(Deserialize)]
pub(crate) struct WikiDirDeleteQuery {
    path: Option<String>,
}

pub(crate) async fn wiki_dir_delete(
    State(s): State<Arc<UiState>>,
    Query(q): Query<WikiDirDeleteQuery>,
) -> Result<impl IntoResponse, AppError> {
    let path = q
        .path
        .as_deref()
        .ok_or_else(|| AppError(StatusCode::BAD_REQUEST, "Missing path".into()))?;
    let wm = wiki_manager(&s)?;
    wm.delete_empty_dir(path)
        .await
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}
