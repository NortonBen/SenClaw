//! REST handlers for the Code Engine feature.
//! Routes registered under /api/code/* and /api/fs/* in core.rs.

use std::sync::Arc;
use std::sync::OnceLock;
use std::collections::HashMap;

use axum::{
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, Path as AxumPath, Query, State},
    http::StatusCode,
    response::Json,
};
use chrono::Utc;
use futures::{SinkExt, StreamExt};
use rusqlite::{params, OptionalExtension as _};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::core::{AppError, UiState};
use crate::code_engine::session::CodeSession;
use crate::code_engine::parse_prompt;
use crate::types::{AgentApi, GroupBinding};

fn db(s: &UiState) -> Result<&crate::db::Db, AppError> {
    s.db.as_deref()
        .ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "DB not available".into()))
}

fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

fn internal(e: impl std::fmt::Display) -> AppError {
    AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

static CODE_CHAT_BROADCAST: OnceLock<tokio::sync::broadcast::Sender<String>> = OnceLock::new();
static CODE_CHAT_GROUP_LOCKS: OnceLock<std::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> =
    OnceLock::new();
const CODE_CHAT_DUPLICATE_WINDOW_MS: i64 = 1200;

fn code_chat_sender() -> tokio::sync::broadcast::Sender<String> {
    CODE_CHAT_BROADCAST
        .get_or_init(|| {
            let (tx, _rx) = tokio::sync::broadcast::channel(256);
            tx
        })
        .clone()
}

fn code_chat_group_lock(group_id: &str) -> Arc<tokio::sync::Mutex<()>> {
    let locks = CODE_CHAT_GROUP_LOCKS.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    let mut guard = locks.lock().unwrap();
    guard
        .entry(group_id.to_string())
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

#[derive(Debug, Default)]
struct QueueProcessStats {
    processed: usize,
    user_processed: usize,
    non_user_processed: usize,
}

// ─── Types ────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct CodeSessionRow {
    pub id: String,
    pub name: String,
    pub workspace: String,
    pub language: Option<String>,
    pub status: String,
    pub git_enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Deserialize)]
pub(crate) struct CreateSessionBody {
    pub name: String,
    pub workspace: String,
    pub language: Option<String>,
    pub init_git: Option<bool>,
}

#[derive(Deserialize)]
pub(crate) struct SessionListQuery {
    pub status: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct RollbackBody {
    pub steps: Option<u32>,
}

#[derive(Deserialize)]
pub(crate) struct FsLsQuery {
    pub path: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct FileContentQuery {
    pub path: String,
}

#[derive(Deserialize)]
pub(crate) struct CodeChatBody {
    pub prompt: String,
    pub group_id: String,
}

#[derive(Deserialize)]
pub(crate) struct CreateChatGroupBody {
    pub name: String,
}

#[derive(Deserialize)]
pub(crate) struct CodeWsQuery {
    pub group_id: Option<String>,
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

/// List subdirectories of a given path (defaults to home dir).
/// Used by the frontend folder-picker dialog.
pub(crate) async fn fs_ls(
    Query(q): Query<FsLsQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let base = if let Some(p) = q.path.filter(|s| !s.is_empty()) {
        std::path::PathBuf::from(p)
    } else {
        dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("/"))
    };

    if !base.exists() || !base.is_dir() {
        return Err(AppError(StatusCode::BAD_REQUEST, "path is not a directory".into()));
    }

    let canonical = base.canonicalize().map_err(internal)?;

    let mut dirs: Vec<serde_json::Value> = std::fs::read_dir(&canonical)
        .map_err(internal)?
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if !p.is_dir() { return None; }
            let name = p.file_name()?.to_string_lossy().into_owned();
            if name.starts_with('.') { return None; }
            let full = p.to_string_lossy().into_owned();
            Some(serde_json::json!({ "name": name, "path": full }))
        })
        .collect();

    dirs.sort_by(|a, b| {
        a["name"].as_str().unwrap_or("").cmp(b["name"].as_str().unwrap_or(""))
    });

    // Build parent path (None if already at root)
    let parent = canonical.parent().map(|p| p.to_string_lossy().into_owned());

    Ok(Json(serde_json::json!({
        "current": canonical.to_string_lossy(),
        "parent": parent,
        "dirs": dirs,
    })))
}

pub(crate) async fn code_sessions_list(
    State(s): State<Arc<UiState>>,
    Query(q): Query<SessionListQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = db(&s)?;
    let status_filter = q.status.unwrap_or_else(|| "active".into());
    let rows: Vec<CodeSessionRow> = db
        .with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, name, workspace, language, status, git_enabled, created_at, updated_at
                 FROM code_sessions
                 WHERE status = ?1
                 ORDER BY updated_at DESC
                 LIMIT 100",
            )?;
            let rows = stmt.query_map(params![status_filter], |row| {
                Ok(CodeSessionRow {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    workspace: row.get(2)?,
                    language: row.get(3)?,
                    status: row.get(4)?,
                    git_enabled: row.get::<_, i32>(5)? != 0,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .map_err(internal)?;
    Ok(Json(serde_json::json!({ "sessions": rows })))
}

pub(crate) async fn code_sessions_create(
    State(s): State<Arc<UiState>>,
    Json(body): Json<CreateSessionBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    if body.name.trim().is_empty() {
        return Err(AppError(StatusCode::BAD_REQUEST, "name is required".into()));
    }
    if body.workspace.trim().is_empty() {
        return Err(AppError(StatusCode::BAD_REQUEST, "workspace is required".into()));
    }

    let id = Uuid::new_v4().to_string();
    let now = now_ms();
    let init_git = body.init_git.unwrap_or(true);

    // Create the workspace directory and optionally init git
    let _ = CodeSession::open(&id, &body.workspace, init_git);

    let db = db(&s)?;
    db.with_conn(|conn| {
        conn.execute(
            "INSERT INTO code_sessions (id, name, workspace, language, status, git_enabled, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 'active', ?5, ?6, ?6)",
            params![id, body.name.trim(), body.workspace.trim(), body.language, init_git as i32, now],
        )?;
        Ok(())
    })
    .map_err(internal)?;

    Ok(Json(serde_json::json!({
        "id": id,
        "name": body.name.trim(),
        "workspace": body.workspace.trim(),
        "language": body.language,
        "status": "active",
        "git_enabled": init_git,
        "created_at": now,
        "updated_at": now,
    })))
}

pub(crate) async fn code_sessions_get(
    State(s): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = db(&s)?;
    let row: Option<CodeSessionRow> = db
        .with_conn(|conn| {
            conn.query_row(
                "SELECT id, name, workspace, language, status, git_enabled, created_at, updated_at
                 FROM code_sessions WHERE id = ?1",
                params![id],
                |row| {
                    Ok(CodeSessionRow {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        workspace: row.get(2)?,
                        language: row.get(3)?,
                        status: row.get(4)?,
                        git_enabled: row.get::<_, i32>(5)? != 0,
                        created_at: row.get(6)?,
                        updated_at: row.get(7)?,
                    })
                },
            )
            .optional()
            .map_err(anyhow::Error::from)
        })
        .map_err(internal)?;

    match row {
        None => Err(AppError(StatusCode::NOT_FOUND, "session not found".into())),
        Some(r) => Ok(Json(serde_json::json!(r))),
    }
}

pub(crate) async fn code_sessions_archive(
    State(s): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = db(&s)?;
    let n = db
        .with_conn(|conn| {
            conn.execute(
                "UPDATE code_sessions SET status='archived', updated_at=?1 WHERE id=?2 AND status='active'",
                params![now_ms(), id],
            ).map_err(anyhow::Error::from)
        })
        .map_err(internal)?;
    if n == 0 {
        return Err(AppError(StatusCode::NOT_FOUND, "session not found or already archived".into()));
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub(crate) async fn code_sessions_files(
    State(s): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = db(&s)?;

    // Fetch workspace path
    let workspace: Option<String> = db
        .with_conn(|conn| {
            conn.query_row(
                "SELECT workspace FROM code_sessions WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()
            .map_err(anyhow::Error::from)
        })
        .map_err(internal)?;

    let workspace = workspace.ok_or_else(|| AppError(StatusCode::NOT_FOUND, "session not found".into()))?;

    // Walk the workspace and return the tree (depth 4)
    let tree = walk_dir_tree(&workspace, 4);
    Ok(Json(serde_json::json!({ "workspace": workspace, "tree": tree })))
}

pub(crate) async fn code_sessions_file_content(
    State(s): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
    Query(q): Query<FileContentQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    if q.path.trim().is_empty() {
        return Err(AppError(StatusCode::BAD_REQUEST, "path is required".into()));
    }

    let db = db(&s)?;
    let workspace: Option<String> = db
        .with_conn(|conn| {
            conn.query_row(
                "SELECT workspace FROM code_sessions WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()
            .map_err(anyhow::Error::from)
        })
        .map_err(internal)?;

    let workspace =
        workspace.ok_or_else(|| AppError(StatusCode::NOT_FOUND, "session not found".into()))?;

    let ws_canon = std::path::PathBuf::from(&workspace)
        .canonicalize()
        .map_err(internal)?;
    let file_path = ws_canon.join(&q.path);
    let file_canon = file_path.canonicalize().map_err(internal)?;

    if !file_canon.starts_with(&ws_canon) {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            "path escapes workspace".into(),
        ));
    }
    if !file_canon.is_file() {
        return Err(AppError(StatusCode::BAD_REQUEST, "path is not a file".into()));
    }

    let bytes = std::fs::read(&file_canon).map_err(internal)?;
    if bytes.len() > 256 * 1024 {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            "file too large for preview (max 256KB)".into(),
        ));
    }
    let content = String::from_utf8_lossy(&bytes).to_string();

    Ok(Json(serde_json::json!({
        "path": q.path,
        "content": content,
    })))
}

pub(crate) async fn code_sessions_git_log(
    State(s): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = db(&s)?;
    let row: Option<(String, i32)> = db
        .with_conn(|conn| {
            conn.query_row(
                "SELECT workspace, git_enabled FROM code_sessions WHERE id = ?1",
                params![id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i32>(1)?)),
            )
            .optional()
            .map_err(anyhow::Error::from)
        })
        .map_err(internal)?;

    let (workspace, git_enabled) = row.ok_or_else(|| AppError(StatusCode::NOT_FOUND, "session not found".into()))?;

    if git_enabled == 0 {
        return Ok(Json(serde_json::json!({ "log": [] })));
    }

    let output = std::process::Command::new("git")
        .args(["log", "--oneline", "--format=%H|%s|%ai", "-20"])
        .current_dir(&workspace)
        .output()
        .map_err(|e| internal(e))?;

    let log: Vec<serde_json::Value> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(3, '|').collect();
            if parts.len() == 3 {
                Some(serde_json::json!({ "hash": parts[0], "message": parts[1], "date": parts[2] }))
            } else {
                None
            }
        })
        .collect();

    Ok(Json(serde_json::json!({ "log": log })))
}

pub(crate) async fn code_sessions_rollback(
    State(s): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<RollbackBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = db(&s)?;
    let row: Option<(String, i32)> = db
        .with_conn(|conn| {
            conn.query_row(
                "SELECT workspace, git_enabled FROM code_sessions WHERE id = ?1",
                params![id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i32>(1)?)),
            )
            .optional()
            .map_err(anyhow::Error::from)
        })
        .map_err(internal)?;

    let (workspace, git_enabled) = row.ok_or_else(|| AppError(StatusCode::NOT_FOUND, "session not found".into()))?;

    if git_enabled == 0 {
        return Err(AppError(StatusCode::BAD_REQUEST, "git not enabled for this session".into()));
    }

    let steps = body.steps.unwrap_or(1).min(50);
    let output = std::process::Command::new("git")
        .args(["reset", "--hard", &format!("HEAD~{steps}")])
        .current_dir(&workspace)
        .output()
        .map_err(|e| internal(e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError(StatusCode::INTERNAL_SERVER_ERROR, format!("git reset failed: {stderr}")));
    }

    db.with_conn(|conn| {
        conn.execute(
            "UPDATE code_sessions SET updated_at=?1 WHERE id=?2",
            params![now_ms(), id],
        )?;
        Ok(())
    })
    .map_err(internal)?;

    Ok(Json(serde_json::json!({ "ok": true, "steps": steps })))
}

pub(crate) async fn code_sessions_chat(
    State(s): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<CodeChatBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    tracing::info!(
        "[CodeChat] request_received session_id={} group_id={} prompt_len={}",
        id,
        body.group_id.trim(),
        body.prompt.len()
    );
    if body.prompt.trim().is_empty() {
        tracing::warn!(
            "[CodeChat] reject_empty_prompt session_id={} group_id={}",
            id,
            body.group_id.trim()
        );
        return Err(AppError(StatusCode::BAD_REQUEST, "prompt is required".into()));
    }
    if body.group_id.trim().is_empty() {
        tracing::warn!("[CodeChat] reject_missing_group_id session_id={}", id);
        return Err(AppError(StatusCode::BAD_REQUEST, "group_id is required".into()));
    }

    let db = db(&s)?;
    let row: Option<(String, String)> = db
        .with_conn(|conn| {
            conn.query_row(
                "SELECT workspace, name FROM code_sessions WHERE id = ?1",
                params![id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(anyhow::Error::from)
        })
        .map_err(internal)?;

    let (workspace, session_name) =
        row.ok_or_else(|| AppError(StatusCode::NOT_FOUND, "session not found".into()))?;

    let group_exists: bool = db
        .with_conn(|conn| {
            let found: Option<i64> = conn
                .query_row(
                    "SELECT 1 FROM code_chat_groups WHERE id = ?1 AND project_id = ?2",
                    params![body.group_id.trim(), id],
                    |r| r.get(0),
                )
                .optional()?;
            Ok(found.is_some())
        })
        .map_err(internal)?;
    if !group_exists {
        tracing::warn!(
            "[CodeChat] group_not_found session_id={} group_id={}",
            id,
            body.group_id.trim()
        );
        return Err(AppError(StatusCode::NOT_FOUND, "group not found".into()));
    }

    let session = CodeSession::open("chat", workspace, false).map_err(internal)?;
    let parsed = parse_prompt(&body.prompt);

    let mut resolved_refs = Vec::new();
    for r in &parsed.refs {
        if let Ok(abs) = session.resolve_path(r) {
            resolved_refs.push(abs.to_string_lossy().to_string());
        }
    }

    let reply = if let Some(cmd) = &parsed.command {
        format!(
            "Da nhan command /{cmd} cho session {session_name}. Dang uu tien xu ly theo command."
        )
    } else {
        format!("Da nhan yeu cau cho session {session_name}.")
    };

    let now = now_ms();
    let duplicate_recent: bool = db
        .with_conn(|conn| {
            let found: Option<i64> = conn
                .query_row(
                    "SELECT 1
                     FROM code_chat_messages
                     WHERE group_id = ?1
                       AND role = 'user'
                       AND content = ?2
                       AND created_at >= ?3
                     ORDER BY created_at DESC
                     LIMIT 1",
                    params![
                        body.group_id.trim(),
                        body.prompt,
                        now - CODE_CHAT_DUPLICATE_WINDOW_MS
                    ],
                    |r| r.get(0),
                )
                .optional()?;
            Ok(found.is_some())
        })
        .map_err(internal)?;
    if duplicate_recent {
        tracing::warn!(
            "[CodeChat] duplicate_prompt_ignored session_id={} group_id={} window_ms={}",
            id,
            body.group_id.trim(),
            CODE_CHAT_DUPLICATE_WINDOW_MS
        );
        let messages = code_chat_group_messages_inner(db, body.group_id.trim()).map_err(internal)?;
        let queued_preview: Vec<serde_json::Value> = messages
            .iter()
            .filter(|m| m["status"] == "queued")
            .take(5)
            .cloned()
            .collect();
        return Ok(Json(serde_json::json!({
            "ok": true,
            "reply": "Duplicate prompt ignored",
            "parsed": parsed,
            "resolved_refs": resolved_refs,
            "dag_plan": "",
            "messages": messages,
            "queued_preview": queued_preview,
            "duplicate_ignored": true,
        })));
    }

    let user_msg_id = Uuid::new_v4().to_string();
    // Recovery: if a previous request crashed mid-flight, requeue stale processing rows.
    db.with_conn(|conn| {
        conn.execute(
            "UPDATE code_chat_messages
             SET status='queued'
             WHERE group_id=?1 AND status='processing' AND created_at < ?2",
            params![body.group_id.trim(), now_ms() - 30_000],
        )?;
        Ok(())
    })
    .map_err(internal)?;

    let pending_before: i64 = db
        .with_conn(|conn| {
            conn.query_row(
                "SELECT COUNT(1) FROM code_chat_messages WHERE group_id = ?1 AND status IN ('queued','processing')",
                params![body.group_id.trim()],
                |r| r.get(0),
            )
            .map_err(anyhow::Error::from)
        })
        .map_err(internal)?;
    let queue_position = Some((pending_before + 1) as i64);

    db.with_conn(|conn| {
        conn.execute(
            "INSERT INTO code_chat_messages (id, group_id, role, content, status, queue_position, created_at)
             VALUES (?1, ?2, 'user', ?3, ?4, ?5, ?6)",
            params![user_msg_id, body.group_id.trim(), body.prompt, "queued", queue_position, now],
        )?;
        Ok(())
    })
    .map_err(internal)?;
    tracing::info!(
        "[CodeChat] enqueued group_id={} msg_id={} pending_before={}",
        body.group_id.trim(),
        user_msg_id,
        pending_before
    );

    let dag_plan = {
        let mut plan = Vec::new();
        plan.push("1) Parse intent and scope");
        if parsed.command.is_some() {
            plan.push("2) Route by command policy");
        } else {
            plan.push("2) Build context from project/group history");
        }
        if !parsed.refs.is_empty() {
            plan.push("3) Analyze referenced files/folders");
        }
        if !parsed.skills.is_empty() {
            plan.push("4) Load requested skills");
        }
        plan.push("5) Produce actionable tasks for team");
        plan.join("\n")
    };

    // Ensure one queue worker per group to avoid duplicate dispatch / channel close races.
    let group_lock = code_chat_group_lock(body.group_id.trim());
    let _queue_guard = group_lock.lock().await;
    tracing::info!(
        "[CodeChat] processor_start group_id={} trigger_msg_id={} (locked)",
        body.group_id.trim(),
        user_msg_id
    );
    let agent_api = s.cowork_agent_api.clone();
    let stats = process_group_queue(
        db,
        body.group_id.trim(),
        &dag_plan,
        agent_api,
        &id,
        &session_name,
        &session.workspace.to_string_lossy(),
    )
    .await
    .map_err(internal)?;
    tracing::info!(
        "[CodeChat] processor_end group_id={} processed={} user_processed={} non_user_processed={}",
        body.group_id.trim(),
        stats.processed,
        stats.user_processed,
        stats.non_user_processed
    );

    let messages = code_chat_group_messages_inner(db, body.group_id.trim()).map_err(internal)?;
    let queued_preview: Vec<serde_json::Value> = messages
        .iter()
        .filter(|m| m["status"] == "queued")
        .take(5)
        .cloned()
        .collect();
    broadcast_code_chat_update(db, body.group_id.trim()).map_err(internal)?;
    tracing::info!(
        "[CodeChat] request_done session_id={} group_id={} total_messages={}",
        id,
        body.group_id.trim(),
        messages.len()
    );

    Ok(Json(serde_json::json!({
        "ok": true,
        "reply": reply,
        "parsed": parsed,
        "resolved_refs": resolved_refs,
        "dag_plan": dag_plan,
        "messages": messages,
        "queued_preview": queued_preview,
    })))
}

pub(crate) async fn code_chat_groups_list(
    State(s): State<Arc<UiState>>,
    AxumPath(project_id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = db(&s)?;
    let groups: Vec<serde_json::Value> = db
        .with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, project_id, name, created_at, updated_at
                 FROM code_chat_groups
                 WHERE project_id = ?1
                 ORDER BY updated_at DESC",
            )?;
            let rows = stmt
                .query_map(params![project_id], |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_, String>(0)?,
                        "project_id": row.get::<_, String>(1)?,
                        "name": row.get::<_, String>(2)?,
                        "created_at": row.get::<_, i64>(3)?,
                        "updated_at": row.get::<_, i64>(4)?,
                    }))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .map_err(internal)?;
    Ok(Json(serde_json::json!({ "groups": groups })))
}

pub(crate) async fn code_chat_groups_create(
    State(s): State<Arc<UiState>>,
    AxumPath(project_id): AxumPath<String>,
    Json(body): Json<CreateChatGroupBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(AppError(StatusCode::BAD_REQUEST, "name is required".into()));
    }
    let db = db(&s)?;
    let id = Uuid::new_v4().to_string();
    let now = now_ms();
    db.with_conn(|conn| {
        conn.execute(
            "INSERT INTO code_chat_groups (id, project_id, name, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)",
            params![id, project_id, name, now],
        )?;
        Ok(())
    })
    .map_err(internal)?;
    Ok(Json(serde_json::json!({
        "id": id,
        "project_id": project_id,
        "name": name,
        "created_at": now,
        "updated_at": now,
    })))
}

pub(crate) async fn code_chat_group_messages(
    State(s): State<Arc<UiState>>,
    AxumPath(group_id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = db(&s)?;
    let messages = code_chat_group_messages_inner(db, &group_id).map_err(internal)?;
    Ok(Json(serde_json::json!({ "messages": messages })))
}

pub(crate) async fn code_chat_group_stop_current(
    State(s): State<Arc<UiState>>,
    AxumPath(group_id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = db(&s)?;
    let now = now_ms();
    let mut action = "noop";
    let mut target_id: Option<String> = None;

    let processing_id: Option<String> = db
        .with_conn(|conn| {
            conn.query_row(
                "SELECT id
                 FROM code_chat_messages
                 WHERE group_id = ?1 AND role = 'user' AND status = 'processing'
                 ORDER BY created_at ASC
                 LIMIT 1",
                params![group_id.as_str()],
                |r| r.get(0),
            )
            .optional()
            .map_err(anyhow::Error::from)
        })
        .map_err(internal)?;

    if let Some(msg_id) = processing_id {
        db.with_conn(|conn| {
            conn.execute(
                "UPDATE code_chat_messages
                 SET status='failed', processed_at=?2
                 WHERE id=?1",
                params![msg_id, now],
            )?;
            Ok(())
        })
        .map_err(internal)?;
        action = "stopped";
        target_id = Some(msg_id);
    } else {
        let queued_id: Option<String> = db
            .with_conn(|conn| {
                conn.query_row(
                    "SELECT id
                     FROM code_chat_messages
                     WHERE group_id = ?1 AND role = 'user' AND status = 'queued'
                     ORDER BY created_at ASC
                     LIMIT 1",
                    params![group_id.as_str()],
                    |r| r.get(0),
                )
                .optional()
                .map_err(anyhow::Error::from)
            })
            .map_err(internal)?;

        if let Some(msg_id) = queued_id {
            db.with_conn(|conn| {
                conn.execute("DELETE FROM code_chat_messages WHERE id=?1", params![msg_id])?;
                Ok(())
            })
            .map_err(internal)?;
            action = "removed";
            target_id = Some(msg_id);
        }
    }

    broadcast_code_chat_update(db, group_id.as_str()).map_err(internal)?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "action": action,
        "target_id": target_id,
    })))
}

pub(crate) async fn code_chat_ws(
    ws: WebSocketUpgrade,
    Query(q): Query<CodeWsQuery>,
) -> impl axum::response::IntoResponse {
    ws.on_upgrade(move |socket| async move {
        handle_code_chat_ws(socket, q.group_id).await;
    })
}

async fn handle_code_chat_ws(socket: WebSocket, group_id: Option<String>) {
    let (mut sender, mut receiver_ws) = socket.split();
    let mut rx = code_chat_sender().subscribe();

    let send_task = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(payload) => {
                    if let Some(gid) = &group_id {
                        if !payload.contains(&format!("\"group_id\":\"{gid}\"")) {
                            continue;
                        }
                    }
                    if sender.send(Message::Text(payload.into())).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    while let Some(msg) = receiver_ws.next().await {
        if msg.is_err() {
            break;
        }
    }
    send_task.abort();
}

async fn process_group_queue(
    db: &crate::db::Db,
    group_id: &str,
    dag_plan: &str,
    agent_api: Option<Arc<dyn AgentApi>>,
    session_id: &str,
    session_name: &str,
    workspace: &str,
) -> Result<QueueProcessStats, anyhow::Error> {
    let mut stats = QueueProcessStats::default();
    loop {
        let next: Option<(String, String, String, String)> = db.with_conn(|conn| {
            conn.query_row(
                "SELECT id, role, content, status
                 FROM code_chat_messages
                 WHERE group_id = ?1 AND status = 'queued'
                 ORDER BY created_at ASC LIMIT 1",
                params![group_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()
            .map_err(anyhow::Error::from)
        })?;
        let Some((msg_id, role, content, prev_status)) = next else {
            if stats.processed == 0 {
                tracing::info!("[CodeChat] processor_idle group_id={} no_pending_messages", group_id);
            }
            break;
        };

        db.with_conn(|conn| {
            conn.execute(
                "UPDATE code_chat_messages SET status='processing', queue_position=NULL WHERE id=?1",
                params![msg_id],
            )?;
            Ok(())
        })?;
        tracing::info!(
            "[CodeChat] processing group_id={} msg_id={} role={} prev_status={}",
            group_id,
            msg_id,
            role,
            prev_status
        );

        if role == "user" {
            let parsed_q = parse_prompt(&content);
            let dispatch_reply = if let Some(api) = agent_api.as_ref() {
                let code_jid = format!("code-chat:{group_id}");
                let group_binding = build_code_group_binding(&code_jid, session_id, session_name);
                let prompt_for_agent = format!(
                    "<code_chat session_id=\"{}\" group_id=\"{}\" workspace=\"{}\">\n{}\n</code_chat>",
                    session_id, group_id, workspace, parsed_q.normalized_prompt
                );
                tracing::info!(
                    "[CodeChat] dag_dispatch_start group_id={} msg_id={} jid={}",
                    group_id,
                    msg_id,
                    code_jid
                );
                let reply_rowid_watermark = latest_group_message_rowid(db, &code_jid).unwrap_or(0);
                match api
                    .process_and_wait(&code_jid, &group_binding, &prompt_for_agent)
                    .await
                {
                    Ok(_) => {
                        let final_reply = wait_for_bot_reply_after_rowid(
                            db,
                            &code_jid,
                            reply_rowid_watermark,
                        )
                        .await
                        .unwrap_or_else(|| {
                            "DAG team da chay xong nhung chua co final response de tra ve. Vui long kiem tra luong task cuoi.".to_string()
                        });
                        tracing::info!(
                            "[CodeChat] dag_dispatch_done group_id={} msg_id={} jid={}",
                            group_id,
                            msg_id,
                            code_jid
                        );
                        final_reply
                    }
                    Err(e) => {
                        tracing::error!(
                            "[CodeChat] dag_dispatch_error group_id={} msg_id={} error={}",
                            group_id,
                            msg_id,
                            e
                        );
                        tracing::warn!(
                            "[CodeChat] user_facing_fallback group_id={} msg_id={} reason=dispatch_error",
                            group_id,
                            msg_id
                        );
                        "He thong dang ban xu ly yeu cau. Vui long gui lai sau it phut neu chua nhan duoc ket qua.".to_string()
                    }
                }
            } else {
                tracing::warn!(
                    "[CodeChat] dag_dispatch_skipped group_id={} msg_id={} reason=no_agent_api",
                    group_id,
                    msg_id
                );
                if let Some(cmd) = parsed_q.command {
                    format!("DAG routed by /{cmd}. Dang phan tich va tao task cho team.")
                } else {
                    "Dang phan tich yeu cau va lap task DAG cho team.".to_string()
                }
            };
            let agent_id = Uuid::new_v4().to_string();
            db.with_conn(|conn| {
                conn.execute(
                    "INSERT INTO code_chat_messages (id, group_id, role, content, status, dag_plan, created_at, processed_at)
                     VALUES (?1, ?2, 'agent', ?3, 'done', ?4, ?5, ?6)",
                    params![agent_id, group_id, dispatch_reply, dag_plan, now_ms(), now_ms()],
                )?;
                conn.execute(
                    "UPDATE code_chat_messages SET status='done', processed_at=?2, dag_plan=?3 WHERE id=?1",
                    params![msg_id, now_ms(), dag_plan],
                )?;
                Ok(())
            })?;
            tracing::info!(
                "[CodeChat] done group_id={} msg_id={} (agent reply inserted)",
                group_id,
                msg_id
            );
            stats.user_processed += 1;
        } else {
            db.with_conn(|conn| {
                conn.execute(
                    "UPDATE code_chat_messages SET status='done', processed_at=?2 WHERE id=?1",
                    params![msg_id, now_ms()],
                )?;
                Ok(())
            })?;
            tracing::info!(
                "[CodeChat] done group_id={} msg_id={} (non-user)",
                group_id,
                msg_id
            );
            stats.non_user_processed += 1;
        }
        stats.processed += 1;
    }

    Ok(stats)
}

fn build_code_group_binding(jid: &str, session_id: &str, session_name: &str) -> GroupBinding {
    GroupBinding {
        jid: jid.to_string(),
        folder: format!("code/{session_id}"),
        name: format!("Code::{session_name}"),
        channel: "web".to_string(),
        group_type: "code".to_string(),
        is_admin: true,
        requires_trigger: false,
        allowed_tools: None,
        allowed_paths: None,
        allowed_work_dirs: None,
        bot_token: None,
        max_messages: None,
        last_active: None,
        added_at: Utc::now().to_rfc3339(),
    }
}

fn latest_group_message_rowid(
    db: &crate::db::Db,
    chat_jid: &str,
 ) -> Option<i64> {
    db.with_conn(|conn| {
        conn.query_row(
            "SELECT COALESCE(MAX(rowid), 0)
             FROM group_messages
             WHERE chat_jid = ?1",
            params![chat_jid],
            |r| r.get::<_, i64>(0),
        )
        .map_err(anyhow::Error::from)
    })
    .ok()
}

fn fetch_latest_bot_reply_after_rowid(
    db: &crate::db::Db,
    chat_jid: &str,
    rowid_after: i64,
) -> Option<String> {
    db.with_conn(|conn| {
        conn.query_row(
            "SELECT content
             FROM group_messages
             WHERE chat_jid = ?1
               AND is_bot_reply = 1
               AND rowid > ?2
             ORDER BY rowid DESC
             LIMIT 1",
            params![chat_jid, rowid_after],
            |r| r.get::<_, String>(0),
        )
        .optional()
        .map_err(anyhow::Error::from)
    })
    .ok()
    .flatten()
}

async fn wait_for_bot_reply_after_rowid(
    db: &crate::db::Db,
    chat_jid: &str,
    rowid_after: i64,
) -> Option<String> {
    for _ in 0..20 {
        if let Some(reply) = fetch_latest_bot_reply_after_rowid(db, chat_jid, rowid_after) {
            return Some(reply);
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    None
}

fn code_chat_group_messages_inner(
    db: &crate::db::Db,
    group_id: &str,
) -> Result<Vec<serde_json::Value>, anyhow::Error> {
    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, role, content, status, queue_position, dag_plan, created_at, processed_at
             FROM code_chat_messages
             WHERE group_id = ?1
             ORDER BY created_at ASC",
        )?;
        let rows = stmt
            .query_map(params![group_id], |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "role": row.get::<_, String>(1)?,
                    "content": row.get::<_, String>(2)?,
                    "status": row.get::<_, String>(3)?,
                    "queue_position": row.get::<_, Option<i64>>(4)?,
                    "dag_plan": row.get::<_, Option<String>>(5)?,
                    "created_at": row.get::<_, i64>(6)?,
                    "processed_at": row.get::<_, Option<i64>>(7)?,
                }))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

fn broadcast_code_chat_update(
    db: &crate::db::Db,
    group_id: &str,
) -> Result<(), anyhow::Error> {
    let messages = code_chat_group_messages_inner(db, group_id)?;
    let queued_preview: Vec<serde_json::Value> = messages
        .iter()
        .filter(|m| m["status"] == "queued")
        .take(5)
        .cloned()
        .collect();
    let _ = code_chat_sender().send(serde_json::json!({
        "type": "code:chat:update",
        "group_id": group_id,
        "messages": messages,
        "queued_preview": queued_preview,
    }).to_string());
    Ok(())
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct FileNode {
    name: String,
    path: String,
    #[serde(rename = "type")]
    kind: &'static str,
    children: Option<Vec<FileNode>>,
}

fn walk_dir_tree(dir: &str, max_depth: u32) -> Vec<FileNode> {
    walk_path(std::path::Path::new(dir), std::path::Path::new(dir), 0, max_depth)
}

fn walk_path(
    dir: &std::path::Path,
    root: &std::path::Path,
    depth: u32,
    max_depth: u32,
) -> Vec<FileNode> {
    if depth > max_depth {
        return vec![];
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return vec![] };
    let mut nodes: Vec<FileNode> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let name = path.file_name()?.to_string_lossy().into_owned();
            if name.starts_with('.') || matches!(name.as_str(), "node_modules" | "target" | "dist" | "build" | "__pycache__") {
                return None;
            }
            let rel = path.strip_prefix(root).ok()?.to_string_lossy().into_owned();
            if path.is_dir() {
                let children = walk_path(&path, root, depth + 1, max_depth);
                Some(FileNode { name, path: rel, kind: "dir", children: Some(children) })
            } else {
                Some(FileNode { name, path: rel, kind: "file", children: None })
            }
        })
        .collect();
    nodes.sort_by(|a, b| {
        // dirs first, then files
        match (a.kind, b.kind) {
            ("dir", "file") => std::cmp::Ordering::Less,
            ("file", "dir") => std::cmp::Ordering::Greater,
            _ => a.name.cmp(&b.name),
        }
    });
    nodes
}
