use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Path as AxumPath, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Json, Response},
};
use axum_extra::extract::Multipart;
use serde::Deserialize;

use crate::cowork::CoworkManager;
use crate::db::Db;

use super::core::{AppError, UiState};
use super::types::path_to_mime;

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Format bytes as a hex dump preview (similar to xxd command)
fn format_hex_preview(bytes: &[u8], max_bytes: usize) -> String {
    let bytes_to_show = &bytes[..bytes.len().min(max_bytes)];
    let mut lines = Vec::new();

    for (i, chunk) in bytes_to_show.chunks(16).enumerate() {
        let offset = i * 16;

        // Hex part
        let hex_part: Vec<String> = chunk
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect();
        let hex_str = hex_part.join(" ");
        let hex_padded = format!("{:48}", hex_str); // Pad to align columns

        // ASCII part
        let ascii_part: String = chunk
            .iter()
            .map(|&b| {
                if b.is_ascii_graphic() || b == b' ' {
                    b as char
                } else {
                    '.'
                }
            })
            .collect();

        lines.push(format!("{:08x}: {}  {}", offset, hex_padded, ascii_part));
    }

    lines.join("\n")
}

/// OS filesystem root (e.g. `/` or `C:\`).
fn filesystem_root() -> PathBuf {
    #[cfg(windows)]
    {
        std::env::var("SystemDrive")
            .map(|d| {
                let d = d.trim_end_matches('\\').to_string();
                PathBuf::from(format!(r"{}\", d))
            })
            .unwrap_or_else(|_| PathBuf::from(r"C:\"))
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("/")
    }
}

/// Shortcuts to common system directories (only returned if they exist).
fn system_quick_paths() -> Vec<(String, PathBuf)> {
    #[cfg(target_os = "macos")]
    {
        vec![
            ("System".to_string(), PathBuf::from("/System")),
            ("Applications".to_string(), PathBuf::from("/Applications")),
            ("Library".to_string(), PathBuf::from("/Library")),
            ("usr/local".to_string(), PathBuf::from("/usr/local")),
        ]
    }
    #[cfg(target_os = "linux")]
    {
        vec![
            ("opt".to_string(), PathBuf::from("/opt")),
            ("usr".to_string(), PathBuf::from("/usr")),
            ("etc".to_string(), PathBuf::from("/etc")),
            ("var".to_string(), PathBuf::from("/var")),
        ]
    }
    #[cfg(target_os = "windows")]
    {
        let sys = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_string());
        let sys = sys.trim_end_matches('\\').to_string();
        vec![
            (
                "Program Files".to_string(),
                PathBuf::from(format!(r"{}\Program Files", sys)),
            ),
            (
                "Program Files (x86)".to_string(),
                PathBuf::from(format!(r"{}\Program Files (x86)", sys)),
            ),
            (
                "Windows".to_string(),
                PathBuf::from(format!(r"{}\Windows", sys)),
            ),
        ]
    }
    #[cfg(not(any(
        target_os = "macos",
        target_os = "linux",
        target_os = "windows"
    )))]
    {
        vec![]
    }
}

// ===== Cowork helpers =====

pub(crate) fn cowork_mgr(s: &UiState) -> Result<&CoworkManager, AppError> {
    s.cowork_manager
        .as_ref()
        .map(|m| m.as_ref())
        .ok_or_else(|| {
            AppError(
                StatusCode::SERVICE_UNAVAILABLE,
                "Cowork not initialized".into(),
            )
        })
}

pub(crate) fn cowork_db(s: &UiState) -> Result<&Db, AppError> {
    s.db.as_ref()
        .map(|d| d.as_ref())
        .ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "DB not available".into()))
}

// ===== Cowork Working Dir Browser =====

#[derive(Deserialize)]
pub(crate) struct BrowseQuery {
    pub(crate) path: Option<String>,
}

/// Browse the workspace's workingDir — returns a flat list of children under the given path.
pub(crate) async fn cowork_ws_browse(
    State(s): State<Arc<UiState>>,
    AxumPath(ws_id): AxumPath<String>,
    Query(q): Query<BrowseQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mgr = cowork_mgr(&s)?;
    let db = cowork_db(&s)?;
    let ws = mgr
        .get_workspace(db, &ws_id)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| AppError(StatusCode::NOT_FOUND, "Workspace not found".into()))?;

    let working_dir = ws.working_dir.as_deref().unwrap_or(&ws.root_dir);
    let base = PathBuf::from(working_dir);

    if !base.exists() {
        return Ok(Json(
            serde_json::json!({ "entries": [], "path": "", "error": "Working directory does not exist" }),
        ));
    }

    let target = match q.path.as_deref() {
        Some(p) if !p.is_empty() => base.join(p),
        _ => base.clone(),
    };

    // Security: must stay within working_dir
    let canonical_base = base.canonicalize().unwrap_or_else(|_| base.clone());
    let canonical_target = target.canonicalize().unwrap_or_else(|_| target.clone());
    if !canonical_target.starts_with(&canonical_base) {
        return Err(AppError(
            StatusCode::FORBIDDEN,
            "Path outside working directory".into(),
        ));
    }

    let rel = canonical_target
        .strip_prefix(&canonical_base)
        .unwrap_or(&canonical_target)
        .to_string_lossy()
        .to_string();

    // If target is a file, return its content
    if canonical_target.is_file() {
        let mime = path_to_mime(&canonical_target.to_string_lossy());
        let size = canonical_target.metadata().map(|m| m.len()).unwrap_or(0);

        // Try reading as text first - a file is binary if:
        // 1. It can't be read as valid UTF-8, OR
        // 2. Its content contains null bytes
        let content_result = fs::read_to_string(&canonical_target);
        let is_binary = match &content_result {
            Ok(text) => text.contains('\0'),
            Err(_) => true, // Failed to read as UTF-8, treat as binary
        };

        let content = if is_binary {
            // Read bytes and create hex preview
            let bytes = fs::read(&canonical_target).unwrap_or_default();
            let hex_preview = format_hex_preview(&bytes, 256); // First 256 bytes
            format!("[Binary file - {} bytes]\n\nHex preview (first {} bytes):\n{}",
                size,
                bytes.len().min(256),
                hex_preview
            )
        } else {
            content_result.unwrap_or_default()
        };

        return Ok(Json(serde_json::json!({
            "path": rel,
            "content": content,
            "mime": mime,
            "isBinary": is_binary,
            "size": size,
            "isFile": true,
            "workingDir": working_dir,
        })));
    }

    let mut entries = Vec::new();
    if let Ok(read_dir) = fs::read_dir(&canonical_target) {
        for entry in read_dir.flatten() {
            let path = entry.path();
            let is_dir = path.is_dir();
            if let Ok(meta) = path.metadata() {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                // Skip hidden files/dirs
                if name.starts_with('.') {
                    continue;
                }
                entries.push(serde_json::json!({
                    "name": name,
                    "path": if rel.is_empty() { name.clone() } else { format!("{}/{}", rel, name) },
                    "isDir": is_dir,
                    "size": meta.len(),
                    "modified": meta.modified().ok().map(|t| {
                        chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339()
                    }),
                }));
            }
        }
    }

    // dirs first, then alphabetical
    entries.sort_by(|a, b| {
        let a_dir = a["isDir"].as_bool().unwrap_or(false);
        let b_dir = b["isDir"].as_bool().unwrap_or(false);
        b_dir.cmp(&a_dir).then_with(|| {
            a["name"]
                .as_str()
                .unwrap_or("")
                .cmp(b["name"].as_str().unwrap_or(""))
        })
    });

    Ok(Json(serde_json::json!({
        "entries": entries,
        "path": canonical_target.to_string_lossy(),
        "workingDir": working_dir,
    })))
}

/// Browse filesystem for folder picking (Create Workspace) — local UI only; starts at user home.
pub(crate) async fn cowork_fs_browse(
    State(s): State<Arc<UiState>>,
    Query(q): Query<BrowseQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let _ = cowork_mgr(&s)?;

    let home = dirs::home_dir().ok_or_else(|| {
        AppError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not resolve home directory".into(),
        )
    })?;

    let target = match q.path.as_deref() {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => home.clone(),
    };

    let canonical = target.canonicalize().map_err(|_| {
        AppError(
            StatusCode::NOT_FOUND,
            format!(
                "Path not found or inaccessible: {}",
                target.to_string_lossy()
            ),
        )
    })?;

    if !canonical.is_dir() {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            "Not a directory".into(),
        ));
    }

    let home_canon = home
        .canonicalize()
        .unwrap_or_else(|_| home.clone());

    let root_base = filesystem_root();
    let root_canon = root_base.canonicalize().unwrap_or(root_base);

    let quick_paths: Vec<serde_json::Value> = system_quick_paths()
        .into_iter()
        .filter_map(|(label, p)| {
            let c = p.canonicalize().ok()?;
            if c.is_dir() {
                Some(serde_json::json!({
                    "label": label,
                    "path": c.to_string_lossy(),
                }))
            } else {
                None
            }
        })
        .collect();

    let parent_path = canonical
        .parent()
        .map(|p| p.to_string_lossy().to_string());

    let mut entries = Vec::new();
    if let Ok(read_dir) = fs::read_dir(&canonical) {
        for entry in read_dir.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if let Ok(meta) = path.metadata() {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                if name.starts_with('.') {
                    continue;
                }
                let abs = path.to_string_lossy().to_string();
                entries.push(serde_json::json!({
                    "name": name,
                    "path": abs,
                    "isDir": true,
                    "size": meta.len(),
                    "modified": meta.modified().ok().map(|t| {
                        chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339()
                    }),
                }));
            }
        }
    }

    entries.sort_by(|a, b| {
        a["name"]
            .as_str()
            .unwrap_or("")
            .cmp(b["name"].as_str().unwrap_or(""))
    });

    Ok(Json(serde_json::json!({
        "entries": entries,
        "absolutePath": canonical.to_string_lossy(),
        "parentPath": parent_path,
        "homePath": home_canon.to_string_lossy(),
        "rootPath": root_canon.to_string_lossy(),
        "quickPaths": quick_paths,
    })))
}

// ===== Cowork Templates =====

pub(crate) async fn cowork_templates_list(
    State(s): State<Arc<UiState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mgr = cowork_mgr(&s)?;
    mgr.ensure_builtin_templates(&s.config);
    let templates = mgr
        .list_templates(&s.config)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "templates": templates })))
}

pub(crate) async fn cowork_templates_get(
    State(s): State<Arc<UiState>>,
    AxumPath(name): AxumPath<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mgr = cowork_mgr(&s)?;
    mgr.ensure_builtin_templates(&s.config);
    let tmpl = mgr
        .get_template(&s.config, &name)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| AppError(StatusCode::NOT_FOUND, "Template not found".into()))?;
    Ok(Json(serde_json::to_value(&tmpl).unwrap_or_default()))
}

// ===== Cowork Workspaces =====

pub(crate) async fn cowork_ws_list(
    State(s): State<Arc<UiState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mgr = cowork_mgr(&s)?;
    let db = cowork_db(&s)?;
    let wss = mgr
        .list_workspaces(db)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "workspaces": wss })))
}

#[derive(Deserialize)]
pub(crate) struct CreateWorkspaceBody {
    name: String,
    description: Option<String>,
    #[serde(rename = "workingDir")]
    working_dir: Option<String>,
    template: Option<String>,
}

pub(crate) async fn cowork_ws_create(
    State(s): State<Arc<UiState>>,
    Json(body): Json<CreateWorkspaceBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mgr = cowork_mgr(&s)?;
    let db = cowork_db(&s)?;
    let now = now_iso();
    let ws = mgr
        .create_workspace_with_template(
            db,
            &s.config,
            &body.name,
            body.description.as_deref(),
            body.working_dir.as_deref(),
            body.template.as_deref(),
            &now,
        )
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::to_value(&ws).unwrap_or_default()))
}

pub(crate) async fn cowork_ws_get(
    State(s): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mgr = cowork_mgr(&s)?;
    let db = cowork_db(&s)?;
    let ws = mgr
        .get_workspace(db, &id)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| AppError(StatusCode::NOT_FOUND, "Workspace not found".into()))?;
    Ok(Json(serde_json::to_value(&ws).unwrap_or_default()))
}

#[derive(Deserialize)]
pub(crate) struct UpdateWorkspaceBody {
    name: Option<String>,
    description: Option<String>,
    status: Option<String>,
    #[serde(rename = "workingDir")]
    working_dir: Option<String>,
}

pub(crate) async fn cowork_ws_update(
    State(s): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<UpdateWorkspaceBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mgr = cowork_mgr(&s)?;
    let db = cowork_db(&s)?;
    let now = now_iso();
    mgr.update_workspace(
        db,
        &id,
        body.name.as_deref(),
        body.description.as_deref(),
        body.status.as_deref(),
        body.working_dir.as_deref(),
        &now,
    )
    .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let ws = mgr
        .get_workspace(db, &id)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| AppError(StatusCode::NOT_FOUND, "Workspace not found".into()))?;
    Ok(Json(serde_json::to_value(&ws).unwrap_or_default()))
}

pub(crate) async fn cowork_ws_delete(
    State(s): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mgr = cowork_mgr(&s)?;
    let db = cowork_db(&s)?;
    mgr.delete_workspace(db, &id)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ===== Cowork Members =====

pub(crate) async fn cowork_members_list(
    State(s): State<Arc<UiState>>,
    AxumPath(ws_id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mgr = cowork_mgr(&s)?;
    let db = cowork_db(&s)?;
    let members = mgr
        .list_members(db, &ws_id)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "members": members })))
}

#[derive(Deserialize)]
pub(crate) struct AddMemberBody {
    #[serde(rename = "memberId")]
    member_id: String,
    #[serde(default = "default_role")]
    role: String,
    jid: Option<String>,
    subdir: Option<String>,
}

fn default_role() -> String {
    "worker".into()
}

pub(crate) async fn cowork_members_add(
    State(s): State<Arc<UiState>>,
    AxumPath(ws_id): AxumPath<String>,
    Json(body): Json<AddMemberBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mgr = cowork_mgr(&s)?;
    let db = cowork_db(&s)?;
    let now = now_iso();
    let m = mgr
        .add_member(
            db,
            &s.config,
            &ws_id,
            &body.member_id,
            &body.role,
            body.jid.as_deref(),
            body.subdir.as_deref(),
            &now,
        )
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::to_value(&m).unwrap_or_default()))
}

#[derive(Deserialize)]
pub(crate) struct UpdateMemberBody {
    role: Option<String>,
    persona: Option<String>,
    responsibilities: Option<String>,
    triggers: Option<String>,
    #[serde(rename = "handoffRules")]
    handoff_rules: Option<String>,
    #[serde(rename = "acceptanceCriteria")]
    acceptance_criteria: Option<String>,
    #[serde(rename = "outputFormat")]
    output_format: Option<String>,
    sla: Option<String>,
    limits: Option<String>,
}

pub(crate) async fn cowork_members_update(
    State(s): State<Arc<UiState>>,
    AxumPath((ws_id, member_id)): AxumPath<(String, String)>,
    Json(body): Json<UpdateMemberBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mgr = cowork_mgr(&s)?;
    let db = cowork_db(&s)?;
    let now = now_iso();
    mgr.update_member_spec(
        db,
        &ws_id,
        &member_id,
        body.role.as_deref(),
        body.persona.as_deref(),
        body.responsibilities.as_deref(),
        body.triggers.as_deref(),
        body.handoff_rules.as_deref(),
        body.acceptance_criteria.as_deref(),
        body.output_format.as_deref(),
        body.sla.as_deref(),
        body.limits.as_deref(),
        &now,
    )
    .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let m = mgr
        .get_member(db, &ws_id, &member_id)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| AppError(StatusCode::NOT_FOUND, "Member not found".into()))?;
    Ok(Json(serde_json::to_value(&m).unwrap_or_default()))
}

pub(crate) async fn cowork_members_remove(
    State(s): State<Arc<UiState>>,
    AxumPath((ws_id, member_id)): AxumPath<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mgr = cowork_mgr(&s)?;
    let db = cowork_db(&s)?;
    mgr.remove_member(db, &ws_id, &member_id)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ===== Cowork Board =====

#[derive(Deserialize)]
pub(crate) struct BoardQuery {
    section: Option<String>,
}

pub(crate) async fn cowork_board_get(
    State(s): State<Arc<UiState>>,
    AxumPath(ws_id): AxumPath<String>,
    Query(q): Query<BoardQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mgr = cowork_mgr(&s)?;
    let db = cowork_db(&s)?;
    let entries = mgr
        .get_board(db, &ws_id, q.section.as_deref())
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "entries": entries })))
}

#[derive(Deserialize)]
pub(crate) struct UpdateBoardBody {
    title: Option<String>,
    content: Option<String>,
    author: Option<String>,
}

pub(crate) async fn cowork_board_update(
    State(s): State<Arc<UiState>>,
    AxumPath((ws_id, section)): AxumPath<(String, String)>,
    Json(body): Json<UpdateBoardBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mgr = cowork_mgr(&s)?;
    let db = cowork_db(&s)?;
    let now = now_iso();
    let author = body.author.as_deref().unwrap_or("system");
    let content = body.content.as_deref().unwrap_or("");
    let entry = mgr
        .upsert_board_entry(
            db,
            &ws_id,
            &section,
            body.title.as_deref(),
            content,
            author,
            &now,
        )
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::to_value(&entry).unwrap_or_default()))
}

// ===== Cowork Tasks =====

#[derive(Deserialize)]
pub(crate) struct TasksQuery {
    status: Option<String>,
}

pub(crate) async fn cowork_tasks_list(
    State(s): State<Arc<UiState>>,
    AxumPath(ws_id): AxumPath<String>,
    Query(q): Query<TasksQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mgr = cowork_mgr(&s)?;
    let db = cowork_db(&s)?;
    let tasks = mgr
        .list_tasks(db, &ws_id, q.status.as_deref())
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "tasks": tasks })))
}

#[derive(Deserialize)]
pub(crate) struct CreateTaskBody {
    title: String,
    description: Option<String>,
    assignee: Option<String>,
    reviewer: Option<String>,
    priority: Option<String>,
    #[serde(rename = "dependsOn")]
    depends_on: Option<String>,
    #[serde(rename = "createdBy")]
    created_by: Option<String>,
    attachments: Option<Vec<String>>,
}

pub(crate) async fn cowork_tasks_create(
    State(s): State<Arc<UiState>>,
    AxumPath(ws_id): AxumPath<String>,
    Json(body): Json<CreateTaskBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mgr = cowork_mgr(&s)?;
    let db = cowork_db(&s)?;
    let now = now_iso();
    let created_by = body.created_by.as_deref().unwrap_or("user");
    let attachments_json = body
        .attachments
        .as_ref()
        .and_then(|v| serde_json::to_string(v).ok());
    let task = mgr
        .create_task(
            db,
            &ws_id,
            &body.title,
            body.description.as_deref(),
            body.assignee.as_deref(),
            body.reviewer.as_deref(),
            body.priority.as_deref(),
            body.depends_on.as_deref(),
            created_by,
            attachments_json.as_deref(),
            &now,
        )
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::to_value(&task).unwrap_or_default()))
}

pub(crate) async fn cowork_tasks_get(
    State(s): State<Arc<UiState>>,
    AxumPath((_ws_id, task_id)): AxumPath<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mgr = cowork_mgr(&s)?;
    let db = cowork_db(&s)?;
    let task = mgr
        .get_task(db, &task_id)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| AppError(StatusCode::NOT_FOUND, "Task not found".into()))?;
    Ok(Json(serde_json::to_value(&task).unwrap_or_default()))
}

#[derive(Deserialize)]
pub(crate) struct UpdateTaskBody {
    title: Option<String>,
    description: Option<String>,
    status: Option<String>,
    assignee: Option<String>,
    reviewer: Option<String>,
    priority: Option<String>,
    #[serde(rename = "dependsOn")]
    depends_on: Option<String>,
    attachments: Option<String>,
}

pub(crate) async fn cowork_tasks_update(
    State(s): State<Arc<UiState>>,
    AxumPath((_ws_id, task_id)): AxumPath<(String, String)>,
    Json(body): Json<UpdateTaskBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mgr = cowork_mgr(&s)?;
    let db = cowork_db(&s)?;
    let now = now_iso();
    mgr.update_task(
        db,
        &task_id,
        body.title.as_deref(),
        body.description.as_deref(),
        body.status.as_deref(),
        body.assignee.as_deref(),
        body.reviewer.as_deref(),
        body.priority.as_deref(),
        body.depends_on.as_deref(),
        body.attachments.as_deref(),
        &now,
    )
    .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let task = mgr
        .get_task(db, &task_id)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| AppError(StatusCode::NOT_FOUND, "Task not found".into()))?;
    Ok(Json(serde_json::to_value(&task).unwrap_or_default()))
}

pub(crate) async fn cowork_tasks_delete(
    State(s): State<Arc<UiState>>,
    AxumPath((_ws_id, task_id)): AxumPath<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mgr = cowork_mgr(&s)?;
    let db = cowork_db(&s)?;
    mgr.delete_task(db, &task_id)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ===== Cowork Task Comments =====

pub(crate) async fn cowork_task_comments_list(
    State(s): State<Arc<UiState>>,
    AxumPath((_ws_id, task_id)): AxumPath<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mgr = cowork_mgr(&s)?;
    let db = cowork_db(&s)?;
    let comments = mgr
        .list_task_comments(db, &task_id)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "comments": comments })))
}

#[derive(Deserialize)]
pub(crate) struct AddCommentBody {
    author: String,
    content: String,
}

pub(crate) async fn cowork_task_comments_add(
    State(s): State<Arc<UiState>>,
    AxumPath((_ws_id, task_id)): AxumPath<(String, String)>,
    Json(body): Json<AddCommentBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mgr = cowork_mgr(&s)?;
    let db = cowork_db(&s)?;
    let now = now_iso();
    let id = mgr
        .add_task_comment(db, &task_id, &body.author, &body.content, &now)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "id": id })))
}

// ===== Cowork Messages =====

#[derive(Deserialize)]
pub(crate) struct MessagesQuery {
    limit: Option<u32>,
    since: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct SendMessageBody {
    from_member: String,
    content: String,
    #[serde(default = "default_message_type")]
    message_type: String,
}

fn default_message_type() -> String {
    "status".to_string()
}

pub(crate) async fn cowork_messages_send(
    State(s): State<Arc<UiState>>,
    AxumPath(ws_id): AxumPath<String>,
    axum::extract::Json(body): axum::extract::Json<SendMessageBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mgr = cowork_mgr(&s)?;
    let db = cowork_db(&s)?;
    let now = now_iso();
    let mgr_arc = s
        .cowork_manager
        .as_ref()
        .ok_or_else(|| {
            AppError(
                StatusCode::SERVICE_UNAVAILABLE,
                "Cowork not initialized".into(),
            )
        })?
        .clone();
    let agent_api = s
        .cowork_agent_api
        .as_ref()
        .map(|api| (Arc::clone(api), Arc::clone(s.db.as_ref().unwrap())));
    let (msg, tasks) = mgr
        .process_user_message(
            db,
            &ws_id,
            &body.from_member,
            &body.content,
            Some(body.message_type.as_str()),
            &now,
            agent_api,
            mgr_arc,
        )
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "message": msg, "tasks": tasks })))
}

pub(crate) async fn cowork_messages_list(
    State(s): State<Arc<UiState>>,
    AxumPath(ws_id): AxumPath<String>,
    Query(q): Query<MessagesQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = cowork_db(&s)?;
    let limit = q.limit.unwrap_or(50);
    let msgs = db
        .list_cowork_messages(&ws_id, limit, q.since.as_deref())
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "messages": msgs })))
}

// ===== Cowork Documents =====

pub(crate) async fn cowork_documents_upload(
    State(s): State<Arc<UiState>>,
    AxumPath(ws_id): AxumPath<String>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, AppError> {
    let mgr = cowork_mgr(&s)?;
    let db = cowork_db(&s)?;

    // Verify workspace exists
    let ws = mgr
        .get_workspace(db, &ws_id)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| AppError(StatusCode::NOT_FOUND, "Workspace not found".into()))?;

    let mut saved = Vec::new();
    while let Ok(Some(field)) = multipart.next_field().await {
        let name = match field.file_name() {
            Some(n) => n.to_string(),
            None => field.name().unwrap_or("document").to_string(),
        };
        let data = field.bytes().await.map_err(|e| {
            AppError(
                StatusCode::BAD_REQUEST,
                format!("Failed to read field: {e}"),
            )
        })?;

        let docs_dir = PathBuf::from(&ws.root_dir).join("shared");
        fs::create_dir_all(&docs_dir).ok();

        let safe_name = name.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
        let path = docs_dir.join(&safe_name);
        fs::write(&path, &data).map_err(|e| {
            AppError(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to save: {e}"),
            )
        })?;
        saved.push(serde_json::json!({
            "name": safe_name,
            "size": data.len(),
            "path": path.to_string_lossy(),
        }));
    }

    Ok(Json(serde_json::json!({ "documents": saved })))
}

// ===== Cowork Files =====

#[derive(Deserialize)]
pub(crate) struct FilesQuery {
    pub(crate) path: Option<String>,
}

pub(crate) async fn cowork_files_list(
    State(s): State<Arc<UiState>>,
    AxumPath(ws_id): AxumPath<String>,
    Query(q): Query<FilesQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mgr = cowork_mgr(&s)?;
    let db = cowork_db(&s)?;
    let ws = mgr
        .get_workspace(db, &ws_id)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| AppError(StatusCode::NOT_FOUND, "Workspace not found".into()))?;

    let base = PathBuf::from(&ws.root_dir);

    if let Some(ref file_path) = q.path {
        // Read specific file content
        let full_path = if file_path.starts_with('/') {
            // Absolute path — resolve relative to workspace root
            let relative = file_path.trim_start_matches('/');
            base.join(relative)
        } else {
            base.join(file_path)
        };

        // Security: must be within workspace root
        if !full_path.starts_with(&base) {
            return Err(AppError(
                StatusCode::FORBIDDEN,
                "Path outside workspace".into(),
            ));
        }
        if !full_path.exists() {
            return Err(AppError(StatusCode::NOT_FOUND, "File not found".into()));
        }
        if !full_path.is_file() {
            return Err(AppError(StatusCode::BAD_REQUEST, "Not a file".into()));
        }

        let mime = path_to_mime(&full_path.to_string_lossy());
        let size = full_path.metadata().map(|m| m.len()).unwrap_or(0);

        // Try reading as text first - a file is binary if:
        // 1. It can't be read as valid UTF-8, OR
        // 2. Its content contains null bytes
        let content_result = fs::read_to_string(&full_path);
        let is_binary = match &content_result {
            Ok(text) => text.contains('\0'),
            Err(_) => true, // Failed to read as UTF-8, treat as binary
        };

        let content = if is_binary {
            // Read bytes and create hex preview
            let bytes = fs::read(&full_path).map_err(|e| {
                AppError(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to read: {e}"),
                )
            })?;
            let hex_preview = format_hex_preview(&bytes, 256); // First 256 bytes
            format!("[Binary file - {} bytes]\n\nHex preview (first {} bytes):\n{}",
                size,
                bytes.len().min(256),
                hex_preview
            )
        } else {
            content_result.map_err(|e| {
                AppError(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to read: {e}"),
                )
            })?
        };

        Ok(Json(serde_json::json!({
            "path": full_path.strip_prefix(&base).unwrap_or(&full_path).to_string_lossy(),
            "content": content,
            "mime": mime,
            "isBinary": is_binary,
            "size": size,
        })))
    } else {
        // List files recursively
        let mut files = Vec::new();
        list_files_recursive(&base, &base, &mut files);
        Ok(Json(serde_json::json!({ "files": files })))
    }
}

fn list_files_recursive(dir: &PathBuf, base: &PathBuf, out: &mut Vec<serde_json::Value>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let relative = path.strip_prefix(base).unwrap_or(&path).to_string_lossy();
            let is_dir = path.is_dir();
            if let Ok(meta) = path.metadata() {
                out.push(serde_json::json!({
                    "name": path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                    "path": relative,
                    "isDir": is_dir,
                    "size": meta.len(),
                    "modified": meta.modified().ok().map(|t| {
                        chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339()
                    }),
                }));
            }
            if is_dir {
                list_files_recursive(&path, base, out);
            }
        }
    }
}

pub(crate) async fn cowork_files_download(
    State(s): State<Arc<UiState>>,
    AxumPath(ws_id): AxumPath<String>,
    Query(q): Query<FilesQuery>,
) -> Result<Response, AppError> {
    let mgr = cowork_mgr(&s)?;
    let db = cowork_db(&s)?;
    let ws = mgr
        .get_workspace(db, &ws_id)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| AppError(StatusCode::NOT_FOUND, "Workspace not found".into()))?;

    let file_path = q
        .path
        .as_deref()
        .ok_or_else(|| AppError(StatusCode::BAD_REQUEST, "Missing ?path=".into()))?;
    let base = PathBuf::from(&ws.root_dir);
    let full_path = if file_path.starts_with('/') {
        base.join(file_path.trim_start_matches('/'))
    } else {
        base.join(file_path)
    };

    if !full_path.starts_with(&base) || !full_path.exists() || !full_path.is_file() {
        return Err(AppError(StatusCode::NOT_FOUND, "File not found".into()));
    }

    let content = fs::read(&full_path).map_err(|e| {
        AppError(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to read: {e}"),
        )
    })?;
    let filename = full_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "download".into());
    let mime = path_to_mime(&filename);

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", filename),
        )
        .header(header::CONTENT_LENGTH, content.len().to_string())
        .body(Body::from(content))
        .unwrap())
}
