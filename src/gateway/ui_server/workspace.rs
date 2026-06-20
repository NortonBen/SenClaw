//! Workspace file discovery — minimal read-only tree listing for the New Chat folder picker.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::{
    extract::{Query, State},
    Json,
};
use serde::{Deserialize, Serialize};

use super::core::{AppError, UiState};

#[derive(Debug, Deserialize)]
pub(crate) struct WorkspaceQuery {
    /// Absolute or `~`-prefixed path to list. Tilde is expanded; relative input is rejected.
    pub path: String,
    /// Max depth of recursion (1 = direct children only). Defaults to 1.
    #[serde(default)]
    pub depth: Option<u32>,
}

#[derive(Debug, Serialize)]
pub(crate) struct WorkspaceEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: Option<u64>,
}

#[derive(Debug, Serialize)]
pub(crate) struct WorkspaceListing {
    pub root: String,
    pub entries: Vec<WorkspaceEntry>,
}

fn expand_tilde(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix('~') {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest.trim_start_matches('/'));
        }
    }
    PathBuf::from(p)
}

fn is_skipped(name: &str) -> bool {
    matches!(
        name,
        "node_modules" | ".git" | "target" | "dist" | "build" | ".next" | ".venv" | "__pycache__"
    ) || name.starts_with('.')
}

fn walk(root: &Path, depth_remaining: u32, out: &mut Vec<WorkspaceEntry>) {
    let rd = match std::fs::read_dir(root) {
        Ok(r) => r,
        Err(_) => return,
    };
    let mut items: Vec<_> = rd.filter_map(Result::ok).collect();
    items.sort_by_key(|e| e.file_name());
    for entry in items {
        let name = entry.file_name().to_string_lossy().to_string();
        if is_skipped(&name) {
            continue;
        }
        let path = entry.path();
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let is_dir = meta.is_dir();
        out.push(WorkspaceEntry {
            name,
            path: path.to_string_lossy().to_string(),
            is_dir,
            size: if is_dir { None } else { Some(meta.len()) },
        });
        if is_dir && depth_remaining > 1 {
            walk(&path, depth_remaining - 1, out);
        }
    }
}

/// GET /api/workspace/files?path=...&depth=1
pub(crate) async fn list_files(
    State(_s): State<Arc<UiState>>,
    Query(q): Query<WorkspaceQuery>,
) -> Result<Json<WorkspaceListing>, AppError> {
    let root = expand_tilde(&q.path);
    if !root.is_absolute() {
        return Err(AppError(axum::http::StatusCode::BAD_REQUEST, String::from("path must be absolute")));
    }
    if !root.exists() {
        return Err(AppError(axum::http::StatusCode::BAD_REQUEST, String::from("path does not exist")));
    }
    if !root.is_dir() {
        return Err(AppError(axum::http::StatusCode::BAD_REQUEST, String::from("path is not a directory")));
    }
    let depth = q.depth.unwrap_or(1).clamp(1, 4);
    let mut entries = Vec::new();
    walk(&root, depth, &mut entries);
    if entries.len() > 5000 {
        entries.truncate(5000);
    }
    Ok(Json(WorkspaceListing {
        root: root.to_string_lossy().to_string(),
        entries,
    }))
}
