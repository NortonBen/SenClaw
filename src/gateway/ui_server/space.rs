//! REST handlers for the Space feature (notes, calendar, email, schedules, apps).
//!
//! Routes are registered in `core.rs` under the `/api/space/*` prefix.
//! All DB access goes through `Db::with_conn` on the SQLite pool.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Path as AxumPath, Query, State},
    http::{header, StatusCode},
    response::{Json, Response},
};
use axum_extra::extract::Multipart;
use base64::Engine as _;
use chrono::Utc;
use rusqlite::params;
use rusqlite::types::Value as SqlValue;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use super::core::{AppError, UiState};

// ─── Helper ──────────────────────────────────────────────────────────────────

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

fn valid_space_app_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn space_apps_dir(s: &UiState) -> PathBuf {
    s.config.paths.workspace_dir.join("space-apps")
}

fn space_app_dir(s: &UiState, id: &str) -> Result<PathBuf, AppError> {
    if !valid_space_app_id(id) {
        return Err(AppError(StatusCode::BAD_REQUEST, "Invalid app id".into()));
    }
    Ok(space_apps_dir(s).join(id))
}

fn json_to_sql_value(v: &serde_json::Value) -> SqlValue {
    match v {
        serde_json::Value::Null => SqlValue::Null,
        serde_json::Value::Bool(b) => SqlValue::Integer(if *b { 1 } else { 0 }),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                SqlValue::Integer(i)
            } else {
                SqlValue::Real(n.as_f64().unwrap_or_default())
            }
        }
        serde_json::Value::String(s) => SqlValue::Text(s.clone()),
        _ => SqlValue::Text(v.to_string()),
    }
}

fn sql_value_to_json(v: SqlValue) -> serde_json::Value {
    match v {
        SqlValue::Null => serde_json::Value::Null,
        SqlValue::Integer(i) => serde_json::Value::Number(i.into()),
        SqlValue::Real(f) => serde_json::json!(f),
        SqlValue::Text(s) => serde_json::Value::String(s),
        SqlValue::Blob(b) => serde_json::json!({
            "type": "blob",
            "base64": base64::engine::general_purpose::STANDARD.encode(b),
        }),
    }
}

fn read_space_app_manifest_from_zip(zip_bytes: &[u8]) -> Result<serde_json::Value, AppError> {
    let cursor = std::io::Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(cursor).map_err(internal)?;
    for name in ["senclaw-manifest.json", "senclaw-app.json"] {
        if let Ok(mut file) = archive.by_name(name) {
            let mut raw = String::new();
            std::io::Read::read_to_string(&mut file, &mut raw).map_err(internal)?;
            return serde_json::from_str(&raw)
                .map_err(|e| AppError(StatusCode::BAD_REQUEST, format!("Invalid {name}: {e}")));
        }
    }
    Err(AppError(
        StatusCode::BAD_REQUEST,
        "Zip must contain senclaw-manifest.json or senclaw-app.json at archive root".into(),
    ))
}

fn content_type_for(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

// ─── Notes ───────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct NoteRow {
    id: String,
    title: String,
    body: String,
    tags: serde_json::Value,
    folder_id: Option<String>,
    pinned: bool,
    created_at: i64,
    updated_at: i64,
}

#[derive(Deserialize)]
pub(crate) struct NoteListQuery {
    tag: Option<String>,
    folder_id: Option<String>,
}

pub(crate) async fn space_notes_list(
    State(s): State<Arc<UiState>>,
    Query(q): Query<NoteListQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = db(&s)?;
    let rows: Vec<NoteRow> = db
        .with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, title, body, tags, folder_id, pinned, created_at, updated_at
                 FROM space_notes
                 WHERE deleted_at IS NULL
                 ORDER BY pinned DESC, updated_at DESC
                 LIMIT 200",
            )?;
            let rows: Vec<NoteRow> = stmt
                .query_map([], |row| {
                    Ok(NoteRow {
                        id: row.get(0)?,
                        title: row.get(1)?,
                        body: row.get(2)?,
                        tags: serde_json::from_str(&row.get::<_, String>(3).unwrap_or_default())
                            .unwrap_or_default(),
                        folder_id: row.get(4)?,
                        pinned: row.get::<_, i32>(5)? != 0,
                        created_at: row.get(6)?,
                        updated_at: row.get(7)?,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
        .map_err(internal)?;

    // Tag filter (client-side after fetch, tags stored as JSON array)
    let rows: Vec<NoteRow> = if let Some(tag) = &q.tag {
        rows.into_iter()
            .filter(|n| {
                n.tags
                    .as_array()
                    .map(|arr| arr.iter().any(|v| v.as_str() == Some(tag.as_str())))
                    .unwrap_or(false)
            })
            .collect()
    } else if let Some(fid) = &q.folder_id {
        rows.into_iter()
            .filter(|n| n.folder_id.as_deref() == Some(fid.as_str()))
            .collect()
    } else {
        rows
    };

    Ok(Json(serde_json::to_value(rows).unwrap_or_default()))
}

#[derive(Deserialize)]
pub(crate) struct NoteSearchQuery {
    q: String,
    #[serde(default = "default_limit")]
    limit: u32,
}

fn default_limit() -> u32 {
    20
}

pub(crate) async fn space_notes_search(
    State(s): State<Arc<UiState>>,
    Query(q): Query<NoteSearchQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = db(&s)?;
    let rows = db
        .with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT n.id, n.title, n.tags,
                        snippet(space_notes_fts, 2, '<b>', '</b>', '…', 20) AS excerpt
                 FROM space_notes_fts f
                 JOIN space_notes n ON n.id = f.id
                 WHERE f.space_notes_fts MATCH ?1 AND n.deleted_at IS NULL
                 ORDER BY rank LIMIT ?2",
            )?;
            let rows: Vec<serde_json::Value> = stmt
                .query_map(params![q.q, q.limit], |row| {
                    Ok(serde_json::json!({
                        "id":      row.get::<_, String>(0)?,
                        "title":   row.get::<_, String>(1)?,
                        "tags":    serde_json::from_str::<serde_json::Value>(
                                       &row.get::<_, String>(2).unwrap_or_default()
                                   ).unwrap_or_default(),
                        "excerpt": row.get::<_, String>(3)?,
                    }))
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
        .map_err(internal)?;

    Ok(Json(serde_json::to_value(rows).unwrap_or_default()))
}

#[derive(Deserialize)]
pub(crate) struct NoteCreateBody {
    title: String,
    body: String,
    #[serde(default)]
    tags: Vec<String>,
    folder_id: Option<String>,
}

pub(crate) async fn space_notes_create(
    State(s): State<Arc<UiState>>,
    Json(b): Json<NoteCreateBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = db(&s)?;
    let id = Uuid::new_v4().to_string();
    let now = now_ms();
    let tags_json = serde_json::to_string(&b.tags).unwrap_or_default();

    db.with_conn(|conn| {
        conn.execute(
            "INSERT INTO space_notes (id, title, body, tags, folder_id, pinned, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?6)",
            params![id, b.title, b.body, tags_json, b.folder_id, now],
        )?;
        Ok(())
    })
    .map_err(internal)?;

    Ok(Json(serde_json::json!({
        "id": id, "title": b.title, "body": b.body,
        "tags": b.tags, "folder_id": b.folder_id,
        "pinned": false, "created_at": now, "updated_at": now,
    })))
}

#[derive(Deserialize)]
pub(crate) struct NoteUpdateBody {
    title: Option<String>,
    body: Option<String>,
    tags: Option<Vec<String>>,
    pinned: Option<bool>,
}

pub(crate) async fn space_notes_update(
    State(s): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
    Json(b): Json<NoteUpdateBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = db(&s)?;
    let now = now_ms();
    db.with_conn(|conn| {
        if let Some(t) = &b.title {
            conn.execute(
                "UPDATE space_notes SET title=?1, updated_at=?2 WHERE id=?3 AND deleted_at IS NULL",
                params![t, now, id],
            )?;
        }
        if let Some(body) = &b.body {
            conn.execute(
                "UPDATE space_notes SET body=?1, updated_at=?2 WHERE id=?3 AND deleted_at IS NULL",
                params![body, now, id],
            )?;
        }
        if let Some(tags) = &b.tags {
            let j = serde_json::to_string(tags).unwrap_or_default();
            conn.execute(
                "UPDATE space_notes SET tags=?1, updated_at=?2 WHERE id=?3 AND deleted_at IS NULL",
                params![j, now, id],
            )?;
        }
        if let Some(pin) = b.pinned {
            conn.execute(
                "UPDATE space_notes SET pinned=?1, updated_at=?2 WHERE id=?3 AND deleted_at IS NULL",
                params![pin as i32, now, id],
            )?;
        }
        Ok(())
    })
    .map_err(internal)?;

    Ok(Json(serde_json::json!({ "success": true, "id": id })))
}

pub(crate) async fn space_notes_delete(
    State(s): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = db(&s)?;
    let now = now_ms();
    db.with_conn(|conn| {
        conn.execute(
            "UPDATE space_notes SET deleted_at=?1 WHERE id=?2",
            params![now, id],
        )?;
        Ok(())
    })
    .map_err(internal)?;

    Ok(Json(serde_json::json!({ "success": true, "id": id })))
}

// ─── Calendar ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct EventListQuery {
    from: i64,
    to: i64,
}

#[derive(Deserialize)]
pub(crate) struct EventSearchQuery {
    #[serde(default)]
    q: Option<String>,
    /// "today" | "tomorrow" | "YYYY-MM-DD"
    #[serde(default)]
    date: Option<String>,
    #[serde(default)]
    from: Option<i64>,
    #[serde(default)]
    to: Option<i64>,
    #[serde(default = "default_limit")]
    limit: u32,
}

pub(crate) async fn space_events_search(
    State(s): State<Arc<UiState>>,
    Query(q): Query<EventSearchQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_arc =
        s.db.clone()
            .ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "DB not available".into()))?;

    let srv = crate::mcp::space_server::SpaceServer::new(db_arc);
    let result = srv.event_search(q.q, q.date, q.from, q.to, q.limit);

    if result.is_error {
        return Err(AppError(StatusCode::INTERNAL_SERVER_ERROR, result.content));
    }
    let v: serde_json::Value = serde_json::from_str(&result.content).unwrap_or_default();
    Ok(Json(v))
}

pub(crate) async fn space_events_list(
    State(s): State<Arc<UiState>>,
    Query(q): Query<EventListQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = db(&s)?;
    let rows = db
        .with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, title, description, start_at, end_at, all_day,
                        location, color, reminder_min, source, status, renotify_min
                 FROM space_events
                 WHERE deleted_at IS NULL AND start_at >= ?1 AND start_at <= ?2
                 ORDER BY start_at ASC",
            )?;
            let rows: Vec<serde_json::Value> = stmt
                .query_map(params![q.from, q.to], |row| {
                    Ok(serde_json::json!({
                        "id":           row.get::<_,String>(0)?,
                        "title":        row.get::<_,String>(1)?,
                        "description":  row.get::<_,Option<String>>(2)?,
                        "start_at":     row.get::<_,i64>(3)?,
                        "end_at":       row.get::<_,i64>(4)?,
                        "all_day":      row.get::<_,i32>(5)? != 0,
                        "location":     row.get::<_,Option<String>>(6)?,
                        "color":        row.get::<_,Option<String>>(7)?,
                        "reminder_min": row.get::<_,Option<i64>>(8)?,
                        "source":       row.get::<_,String>(9)?,
                        "status":       row.get::<_,Option<String>>(10)?.unwrap_or_else(|| "upcoming".into()),
                        "renotify_min": row.get::<_,Option<i64>>(11)?,
                    }))
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
        .map_err(internal)?;

    Ok(Json(serde_json::to_value(rows).unwrap_or_default()))
}

#[derive(Deserialize)]
pub(crate) struct EventCreateBody {
    title: String,
    start_at: i64,
    end_at: i64,
    description: Option<String>,
    location: Option<String>,
    #[serde(default)]
    all_day: bool,
    reminder_min: Option<i64>,
    renotify_min: Option<i64>,
    color: Option<String>,
    /// Group + jid required to schedule a reminder task
    group_folder: Option<String>,
    chat_jid: Option<String>,
}

pub(crate) async fn space_events_create(
    State(s): State<Arc<UiState>>,
    Json(b): Json<EventCreateBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_arc =
        s.db.clone()
            .ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "DB not available".into()))?;

    let space_srv = crate::mcp::space_server::SpaceServer::new(db_arc);
    let result = space_srv.event_create(
        b.title,
        b.start_at,
        b.end_at,
        b.description,
        b.location,
        b.all_day,
        b.reminder_min,
        b.renotify_min,
        b.color,
        b.group_folder.as_deref().unwrap_or("default"),
        b.chat_jid.as_deref().unwrap_or(""),
    );

    if result.is_error {
        return Err(AppError(StatusCode::INTERNAL_SERVER_ERROR, result.content));
    }

    let v: serde_json::Value = serde_json::from_str(&result.content).unwrap_or_default();
    Ok(Json(v))
}

#[derive(Deserialize)]
pub(crate) struct EventUpdateBody {
    title: Option<String>,
    description: Option<String>,
    start_at: Option<i64>,
    end_at: Option<i64>,
    location: Option<String>,
    color: Option<String>,
    reminder_min: Option<i64>,
    renotify_min: Option<i64>,
    #[serde(default)]
    all_day: Option<bool>,
    #[serde(default)]
    reset_reminder: Option<bool>,
}

pub(crate) async fn space_events_update(
    State(s): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
    Json(b): Json<EventUpdateBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = db(&s)?;
    db.with_conn(|conn| {
        let now_ms = chrono::Utc::now().timestamp_millis();
        if let Some(v) = &b.title {
            conn.execute("UPDATE space_events SET title=?1 WHERE id=?2", params![v, id])?;
        }
        if b.description.is_some() {
            conn.execute("UPDATE space_events SET description=?1 WHERE id=?2", params![b.description, id])?;
        }
        if let Some(v) = b.start_at {
            // Moving start_at re-arms both the pre-event reminder and the
            // start-time notification so the event pings again at its new
            // time (otherwise a rescheduled event stays silent).
            conn.execute(
                "UPDATE space_events
                 SET start_at=?1, reminder_sent_at=NULL, start_sent_at=NULL
                 WHERE id=?2",
                params![v, id],
            )?;
        }
        if let Some(v) = b.end_at {
            conn.execute("UPDATE space_events SET end_at=?1 WHERE id=?2", params![v, id])?;
        }
        if b.location.is_some() {
            conn.execute("UPDATE space_events SET location=?1 WHERE id=?2", params![b.location, id])?;
        }
        if b.color.is_some() {
            conn.execute("UPDATE space_events SET color=?1 WHERE id=?2", params![b.color, id])?;
        }
        if b.reminder_min.is_some() {
            conn.execute("UPDATE space_events SET reminder_min=?1 WHERE id=?2", params![b.reminder_min, id])?;
        }
        if b.renotify_min.is_some() {
            conn.execute("UPDATE space_events SET renotify_min=?1 WHERE id=?2", params![b.renotify_min, id])?;
        }
        if let Some(v) = b.all_day {
            conn.execute("UPDATE space_events SET all_day=?1 WHERE id=?2", params![v as i32, id])?;
        }
        if b.reset_reminder.unwrap_or(false) {
            conn.execute(
                "UPDATE space_events SET reminder_sent_at=NULL, renotify_sent_at=NULL, start_sent_at=NULL WHERE id=?1",
                params![id],
            )?;
        }
        conn.execute("UPDATE space_events SET updated_at=?1 WHERE id=?2", params![now_ms, id])?;
        Ok(())
    })
    .map_err(internal)?;

    Ok(Json(serde_json::json!({ "success": true, "id": id })))
}

pub(crate) async fn space_events_delete(
    State(s): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = db(&s)?;
    let now = now_ms();
    db.with_conn(|conn| {
        conn.execute(
            "UPDATE space_events SET deleted_at=?1 WHERE id=?2",
            params![now, id],
        )?;
        Ok(())
    })
    .map_err(internal)?;

    Ok(Json(serde_json::json!({ "success": true, "id": id })))
}

pub(crate) async fn space_today_summary(
    State(s): State<Arc<UiState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_arc =
        s.db.clone()
            .ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "DB not available".into()))?;

    let space_srv = crate::mcp::space_server::SpaceServer::new(db_arc);
    let result = space_srv.today_summary();

    let v: serde_json::Value = serde_json::from_str(&result.content).unwrap_or_default();
    Ok(Json(v))
}

// ─── Email ────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct EmailInboxQuery {
    account_id: Option<String>,
    #[serde(default = "default_limit")]
    limit: u32,
}

pub(crate) async fn space_email_inbox(
    State(s): State<Arc<UiState>>,
    Query(q): Query<EmailInboxQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_arc =
        s.db.clone()
            .ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "DB not available".into()))?;

    let srv = crate::mcp::space_server::SpaceServer::new(db_arc);
    let result = srv.email_inbox(q.account_id, q.limit);

    let v: serde_json::Value = serde_json::from_str(&result.content).unwrap_or_default();
    Ok(Json(v))
}

pub(crate) async fn space_email_read(
    State(s): State<Arc<UiState>>,
    AxumPath(msg_id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_arc =
        s.db.clone()
            .ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "DB not available".into()))?;

    let srv = crate::mcp::space_server::SpaceServer::new(db_arc);
    let result = srv.email_read(msg_id);
    if result.is_error {
        return Err(AppError(StatusCode::NOT_FOUND, result.content));
    }
    let v: serde_json::Value = serde_json::from_str(&result.content).unwrap_or_default();
    Ok(Json(v))
}

#[derive(Deserialize)]
pub(crate) struct EmailSearchQuery {
    q: String,
    account_id: Option<String>,
    #[serde(default = "default_limit")]
    limit: u32,
}

pub(crate) async fn space_email_search(
    State(s): State<Arc<UiState>>,
    Query(q): Query<EmailSearchQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_arc =
        s.db.clone()
            .ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "DB not available".into()))?;

    let srv = crate::mcp::space_server::SpaceServer::new(db_arc);
    let result = srv.email_search(q.q, q.account_id, q.limit);
    let v: serde_json::Value = serde_json::from_str(&result.content).unwrap_or_default();
    Ok(Json(v))
}

#[derive(Deserialize)]
pub(crate) struct EmailSendBody {
    to: String,
    subject: String,
    body: String,
    account_id: Option<String>,
}

pub(crate) async fn space_email_send(
    State(s): State<Arc<UiState>>,
    Json(b): Json<EmailSendBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_arc =
        s.db.clone()
            .ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "DB not available".into()))?;

    let srv = crate::mcp::space_server::SpaceServer::new(db_arc);
    let result = srv.email_compose(b.to, b.subject, b.body, b.account_id);
    if result.is_error {
        return Err(AppError(StatusCode::BAD_GATEWAY, result.content));
    }
    let v: serde_json::Value = serde_json::from_str(&result.content).unwrap_or_default();
    Ok(Json(v))
}

/// AI email draft — stub: returns a skeleton body that the space-assistant
/// persona would normally fill via agent loop. The frontend calls this
/// endpoint when user clicks "AI soạn thảo".
#[derive(Deserialize)]
pub(crate) struct EmailDraftBody {
    prompt: String,
}

pub(crate) async fn space_email_draft(
    State(_s): State<Arc<UiState>>,
    Json(b): Json<EmailDraftBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Minimal template-based draft — replace with actual agent call when
    // the agent loop integration is ready (Phase 3).
    let subject = format!("Re: {}", b.prompt.chars().take(60).collect::<String>());
    let body = format!("Kính gửi,\n\n{}\n\nTrân trọng,\n[Tên của bạn]", b.prompt);
    Ok(Json(
        serde_json::json!({ "subject": subject, "body": body }),
    ))
}

// ─── Email accounts ───────────────────────────────────────────────────────────

pub(crate) async fn space_email_accounts_list(
    State(s): State<Arc<UiState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = db(&s)?;
    let rows = db
        .with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, label, email, imap_host, imap_port, smtp_host, smtp_port, use_tls, created_at
                 FROM space_email_accounts ORDER BY created_at DESC",
            )?;
            let rows: Vec<serde_json::Value> = stmt
                .query_map([], |row| {
                    Ok(serde_json::json!({
                        "id":        row.get::<_,String>(0)?,
                        "label":     row.get::<_,String>(1)?,
                        "email":     row.get::<_,String>(2)?,
                        "imap_host": row.get::<_,String>(3)?,
                        "imap_port": row.get::<_,i64>(4)?,
                        "smtp_host": row.get::<_,String>(5)?,
                        "smtp_port": row.get::<_,i64>(6)?,
                        "use_tls":   row.get::<_,i32>(7)? != 0,
                        "created_at":row.get::<_,i64>(8)?,
                    }))
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
        .map_err(internal)?;

    Ok(Json(serde_json::to_value(rows).unwrap_or_default()))
}

#[derive(Deserialize)]
pub(crate) struct EmailAccountCreateBody {
    label: String,
    email: String,
    imap_host: String,
    #[serde(default = "default_imap_port")]
    imap_port: i64,
    smtp_host: String,
    #[serde(default = "default_smtp_port")]
    smtp_port: i64,
    username: String,
    password: String,
    #[serde(default = "default_true")]
    use_tls: bool,
}

fn default_imap_port() -> i64 {
    993
}
fn default_smtp_port() -> i64 {
    587
}
fn default_true() -> bool {
    true
}

pub(crate) async fn space_email_accounts_create(
    State(s): State<Arc<UiState>>,
    Json(b): Json<EmailAccountCreateBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    if b.label.trim().is_empty()
        || b.email.trim().is_empty()
        || b.imap_host.trim().is_empty()
        || b.smtp_host.trim().is_empty()
        || b.username.trim().is_empty()
        || b.password.is_empty()
    {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            "Missing required email account fields".into(),
        ));
    }
    if !(1..=65_535).contains(&b.imap_port) || !(1..=65_535).contains(&b.smtp_port) {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            "Invalid email port".into(),
        ));
    }

    let db = db(&s)?;
    let id = Uuid::new_v4().to_string();
    let now = now_ms();

    // Password should be AES-GCM encrypted in production (Phase 3).
    // Stored as-is for Phase 0 to unblock development — mark clearly.
    let password_stored = format!("plaintext:{}", b.password);

    db.with_conn(|conn| {
        conn.execute(
            "INSERT INTO space_email_accounts
             (id, label, email, imap_host, imap_port, smtp_host, smtp_port, username, password, use_tls, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                id, b.label, b.email, b.imap_host, b.imap_port,
                b.smtp_host, b.smtp_port, b.username, password_stored,
                b.use_tls as i32, now
            ],
        )?;
        Ok(())
    })
    .map_err(internal)?;

    Ok(Json(serde_json::json!({
        "id": id,
        "label": b.label,
        "email": b.email,
        "imap_host": b.imap_host,
        "imap_port": b.imap_port,
        "smtp_host": b.smtp_host,
        "smtp_port": b.smtp_port,
        "use_tls": b.use_tls,
        "created_at": now,
    })))
}

pub(crate) async fn space_email_accounts_delete(
    State(s): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = db(&s)?;
    db.with_conn(|conn| {
        conn.execute("DELETE FROM space_email_accounts WHERE id=?1", params![&id])?;
        conn.execute(
            "DELETE FROM space_email_cache WHERE account_id=?1",
            params![&id],
        )?;
        Ok(())
    })
    .map_err(internal)?;

    Ok(Json(serde_json::json!({ "success": true })))
}

// ─── Schedules ────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct ScheduleListQuery {
    group: String,
}

pub(crate) async fn space_schedules_list(
    State(s): State<Arc<UiState>>,
    Query(q): Query<ScheduleListQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_arc =
        s.db.clone()
            .ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "DB not available".into()))?;

    let srv = crate::mcp::space_server::SpaceServer::new(db_arc);
    let result = srv.list_schedules(q.group);
    let v: serde_json::Value = serde_json::from_str(&result.content).unwrap_or_default();
    Ok(Json(v))
}

#[derive(Deserialize)]
pub(crate) struct ScheduleCreateBody {
    prompt: String,
    cron: String,
    group_folder: String,
    chat_jid: String,
}

pub(crate) async fn space_schedules_create(
    State(s): State<Arc<UiState>>,
    Json(b): Json<ScheduleCreateBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_arc =
        s.db.clone()
            .ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "DB not available".into()))?;

    let srv = crate::mcp::space_server::SpaceServer::new(db_arc);
    let result = srv
        .schedule_activity(b.prompt, b.cron, b.group_folder, b.chat_jid)
        .await;
    if result.is_error {
        return Err(AppError(StatusCode::BAD_REQUEST, result.content));
    }
    let v: serde_json::Value = serde_json::from_str(&result.content).unwrap_or_default();
    Ok(Json(v))
}

#[derive(Deserialize)]
pub(crate) struct ScheduleCancelBody {
    group_folder: String,
}

pub(crate) async fn space_schedules_cancel(
    State(s): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
    Json(b): Json<ScheduleCancelBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_arc =
        s.db.clone()
            .ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "DB not available".into()))?;

    let srv = crate::mcp::space_server::SpaceServer::new(db_arc);
    let result = srv.list_schedules(b.group_folder.clone()); // validate ownership
    if result.is_error {
        return Err(AppError(StatusCode::INTERNAL_SERVER_ERROR, result.content));
    }

    // Cancel = set status completed
    let db_ref =
        s.db.as_deref()
            .ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "DB not available".into()))?;
    db_ref
        .with_conn(|conn| {
            conn.execute(
                "UPDATE scheduled_tasks SET status='completed' WHERE id=?1 AND group_folder=?2",
                params![id, b.group_folder],
            )?;
            Ok(())
        })
        .map_err(internal)?;

    Ok(Json(serde_json::json!({ "success": true, "id": id })))
}

// ─── Apps (micro-frontend registry) ──────────────────────────────────────────

pub(crate) async fn space_apps_list(
    State(s): State<Arc<UiState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = db(&s)?;
    let rows = db
        .with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, manifest, enabled, installed_at FROM space_apps ORDER BY installed_at DESC",
            )?;
            let rows: Vec<serde_json::Value> = stmt
                .query_map([], |row| {
                    let manifest_str: String = row.get(1)?;
                    let manifest: serde_json::Value =
                        serde_json::from_str(&manifest_str).unwrap_or_default();
                    Ok(serde_json::json!({
                        "id":           row.get::<_,String>(0)?,
                        "manifest":     manifest,
                        "enabled":      row.get::<_,i32>(2)? != 0,
                        "installed_at": row.get::<_,i64>(3)?,
                    }))
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
        .map_err(internal)?;

    Ok(Json(serde_json::to_value(rows).unwrap_or_default()))
}

#[derive(Deserialize)]
pub(crate) struct AppRegisterBody {
    manifest_url: String,
}

pub(crate) async fn space_apps_register(
    State(s): State<Arc<UiState>>,
    Json(b): Json<AppRegisterBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Fetch the manifest from the given URL
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(internal)?;

    let manifest_json: serde_json::Value = client
        .get(&b.manifest_url)
        .send()
        .await
        .map_err(|e| {
            AppError(
                StatusCode::BAD_GATEWAY,
                format!("Fetch manifest failed: {e}"),
            )
        })?
        .json()
        .await
        .map_err(|e| {
            AppError(
                StatusCode::BAD_GATEWAY,
                format!("Parse manifest failed: {e}"),
            )
        })?;

    let app_id = manifest_json["id"]
        .as_str()
        .unwrap_or(&Uuid::new_v4().to_string())
        .to_string();
    if !valid_space_app_id(&app_id) {
        return Err(AppError(StatusCode::BAD_REQUEST, "Invalid app id".into()));
    }

    let now = now_ms();
    let manifest_str = serde_json::to_string(&manifest_json).unwrap_or_default();

    let db = db(&s)?;
    db.with_conn(|conn| {
        conn.execute(
            "INSERT OR REPLACE INTO space_apps (id, manifest, enabled, installed_at, last_seen_at)
             VALUES (?1, ?2, 1, ?3, ?3)",
            params![app_id, manifest_str, now],
        )?;
        Ok(())
    })
    .map_err(internal)?;

    // Auto-register the app's declared MCP server (launch + register) if any.
    try_autoregister_app_mcp(&s, &app_id, &manifest_json).await;

    Ok(Json(serde_json::json!({
        "id": app_id,
        "manifest": manifest_json,
        "enabled": true,
        "installed_at": now,
    })))
}

#[derive(Deserialize)]
pub(crate) struct AppRegisterLocalBody {
    /// Absolute path to a Space App directory containing senclaw-manifest.json.
    path: String,
}

/// Register a Space App from a local directory (for "server" apps the daemon
/// runs in place via `runtime.start`). Reads the manifest, records the local
/// path, then installs skills + launches + auto-registers the MCP.
pub(crate) async fn space_apps_register_local(
    State(s): State<Arc<UiState>>,
    Json(b): Json<AppRegisterLocalBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let dir = PathBuf::from(b.path.trim());
    if !dir.is_dir() {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            "Path is not a directory".into(),
        ));
    }
    let manifest_path = ["senclaw-manifest.json", "senclaw-app.json"]
        .iter()
        .map(|n| dir.join(n))
        .find(|p| p.is_file())
        .ok_or_else(|| {
            AppError(
                StatusCode::BAD_REQUEST,
                "No senclaw-manifest.json in directory".into(),
            )
        })?;
    let raw = tokio::fs::read_to_string(&manifest_path)
        .await
        .map_err(internal)?;
    let mut manifest: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, format!("Invalid manifest: {e}")))?;

    let app_id = manifest["id"]
        .as_str()
        .map(|s| s.trim().to_string())
        .filter(|s| valid_space_app_id(s))
        .ok_or_else(|| AppError(StatusCode::BAD_REQUEST, "Manifest missing valid id".into()))?;

    let canonical = dir.canonicalize().unwrap_or(dir);
    manifest["install"] = serde_json::json!({
        "type": "local",
        "localPath": canonical.to_string_lossy(),
    });

    let now = now_ms();
    let manifest_str = serde_json::to_string(&manifest).unwrap_or_default();
    let db = db(&s)?;
    db.with_conn(|conn| {
        conn.execute(
            "INSERT OR REPLACE INTO space_apps (id, manifest, enabled, installed_at, last_seen_at)
             VALUES (?1, ?2, 1, ?3, ?3)",
            params![app_id, manifest_str, now],
        )?;
        Ok(())
    })
    .map_err(internal)?;

    try_autoregister_app_mcp(&s, &app_id, &manifest).await;

    // Re-read the manifest (run_and_register may have stamped runtime.url/port).
    let stored: Option<serde_json::Value> = db
        .with_conn(|conn| {
            let raw: Result<String, rusqlite::Error> = conn.query_row(
                "SELECT manifest FROM space_apps WHERE id=?1",
                params![&app_id],
                |row| row.get(0),
            );
            Ok(raw.ok().and_then(|s| serde_json::from_str(&s).ok()))
        })
        .map_err(internal)?;

    Ok(Json(serde_json::json!({
        "id": app_id,
        "manifest": stored.unwrap_or(manifest),
        "enabled": true,
        "installed_at": now,
    })))
}

pub(crate) async fn space_apps_install_zip(
    State(s): State<Arc<UiState>>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, AppError> {
    let mut zip_bytes: Option<Vec<u8>> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, format!("Invalid upload: {e}")))?
    {
        let is_zip = field
            .file_name()
            .map(|name| name.to_ascii_lowercase().ends_with(".zip"))
            .unwrap_or(false);
        if field.name() == Some("file") || is_zip {
            let bytes = field.bytes().await.map_err(|e| {
                AppError(StatusCode::BAD_REQUEST, format!("Read upload failed: {e}"))
            })?;
            zip_bytes = Some(bytes.to_vec());
            break;
        }
    }

    let zip_bytes = zip_bytes.ok_or_else(|| {
        AppError(
            StatusCode::BAD_REQUEST,
            "Upload a zip file in multipart field `file`".into(),
        )
    })?;
    if zip_bytes.len() > 50 * 1024 * 1024 {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            "Zip file too large (max 50MB)".into(),
        ));
    }

    let mut manifest = read_space_app_manifest_from_zip(&zip_bytes)?;
    let app_id = manifest["id"]
        .as_str()
        .map(|s| s.trim().to_string())
        .filter(|s| valid_space_app_id(s))
        .unwrap_or_else(|| format!("space-app-{}", Uuid::new_v4()));
    manifest["id"] = serde_json::Value::String(app_id.clone());

    let root = space_apps_dir(&s);
    let target = root.join(&app_id);
    if target.exists() {
        tokio::fs::remove_dir_all(&target).await.map_err(internal)?;
    }
    crate::clawhub::lockfile::extract_zip_to_dir(&zip_bytes, &target).map_err(internal)?;

    // "server" apps ship a runnable program started by `runtime.start` — any
    // runtime (Node, a native/Rust binary, Python, a static-file server, …). We
    // validate the declared `runtime.entrypoint` exists if given, else that the
    // archive is non-empty. Static apps ship a built index.html.
    let is_server = manifest
        .get("runtime")
        .and_then(|r| r.get("kind"))
        .and_then(|k| k.as_str())
        == Some("server");
    if is_server {
        let entrypoint = manifest
            .get("runtime")
            .and_then(|r| r.get("entrypoint"))
            .and_then(|e| e.as_str());
        let valid = match entrypoint {
            Some(ep) => target.join(ep).is_file(),
            None => std::fs::read_dir(&target)
                .map(|mut it| it.next().is_some())
                .unwrap_or(false),
        };
        if !valid {
            let _ = tokio::fs::remove_dir_all(&target).await;
            return Err(AppError(
                StatusCode::BAD_REQUEST,
                "Server Space App zip must contain its runtime.entrypoint (or be non-empty)."
                    .into(),
            ));
        }
    } else if !target.join("index.html").is_file() {
        let _ = tokio::fs::remove_dir_all(&target).await;
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            "Space App zip must contain a built index.html at archive root. Run the app build and zip the build output directory.".into(),
        ));
    }

    if manifest.get("integration").is_none() {
        manifest["integration"] = if is_server {
            serde_json::json!({ "type": "iframe", "url": "/" })
        } else {
            serde_json::json!({
                "type": "iframe",
                "url": format!("/api/space/apps/{app_id}/static/index.html"),
            })
        };
    }
    manifest["install"] = serde_json::json!({
        "type": "zip",
        "localPath": target.to_string_lossy(),
    });
    if manifest.get("bridge").is_none() {
        manifest["bridge"] = serde_json::json!({
            "postMessage": true,
            "capabilities": ["space.rest"],
        });
    }

    let now = now_ms();
    let manifest_str = serde_json::to_string(&manifest).unwrap_or_default();
    let db = db(&s)?;
    db.with_conn(|conn| {
        conn.execute(
            "INSERT OR REPLACE INTO space_apps (id, manifest, enabled, installed_at, last_seen_at)
             VALUES (?1, ?2, 1, ?3, ?3)",
            params![app_id, manifest_str, now],
        )?;
        Ok(())
    })
    .map_err(internal)?;

    // Auto-register the app's declared MCP server (launch + register) if any.
    try_autoregister_app_mcp(&s, &app_id, &manifest).await;

    Ok(Json(serde_json::json!({
        "id": app_id,
        "manifest": manifest,
        "enabled": true,
        "installed_at": now,
    })))
}

pub(crate) async fn space_apps_delete(
    State(s): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = db(&s)?;
    let manifest: Option<serde_json::Value> = db
        .with_conn(|conn| {
            let raw: Result<String, rusqlite::Error> = conn.query_row(
                "SELECT manifest FROM space_apps WHERE id=?1",
                params![&id],
                |row| row.get(0),
            );
            Ok(raw.ok().and_then(|s| serde_json::from_str(&s).ok()))
        })
        .map_err(internal)?;

    if let Some(path) = manifest
        .as_ref()
        .and_then(|m| m["install"]["localPath"].as_str())
        .map(PathBuf::from)
    {
        let root = space_apps_dir(&s);
        let canonical_root = root.canonicalize().unwrap_or(root);
        let canonical_path = path.canonicalize().unwrap_or(path);
        if canonical_path.starts_with(&canonical_root) {
            let _ = tokio::fs::remove_dir_all(canonical_path).await;
        }
    }

    // Remove the app's bundled skills, stop its server process, and unregister
    // its MCP server.
    super::space_skills::remove_app_skills(&s.config, &id);
    if let Some(launcher) = s.space_mcp_launcher.as_ref() {
        launcher.stop_app(&id).await;
    }
    if let (Some(mgr), Some(name)) = (
        s.mcp_manager.as_ref(),
        manifest
            .as_ref()
            .and_then(|m| m["mcp"]["name"].as_str())
            .map(str::to_string),
    ) {
        let _ = mgr
            .remove(&name, crate::mcp::config::McpScopeType::Project)
            .await;
    }

    db.with_conn(|conn| {
        conn.execute("DELETE FROM space_apps WHERE id=?1", params![id])?;
        Ok(())
    })
    .map_err(internal)?;

    Ok(Json(serde_json::json!({ "success": true })))
}

pub(crate) async fn space_apps_static(
    State(s): State<Arc<UiState>>,
    AxumPath((id, req_path)): AxumPath<(String, String)>,
) -> Result<Response, AppError> {
    if !valid_space_app_id(&id) {
        return Err(AppError(StatusCode::BAD_REQUEST, "Invalid app id".into()));
    }
    if req_path.contains("..") || req_path.contains('\\') {
        return Err(AppError(StatusCode::BAD_REQUEST, "Invalid app path".into()));
    }
    let root = space_apps_dir(&s).join(&id);
    let rel = if req_path.trim().is_empty() {
        "index.html"
    } else {
        req_path.trim_start_matches('/')
    };
    let path = root.join(rel);
    let canonical_root = root
        .canonicalize()
        .map_err(|_| AppError(StatusCode::NOT_FOUND, "App not found".into()))?;
    let canonical_path = path
        .canonicalize()
        .map_err(|_| AppError(StatusCode::NOT_FOUND, "File not found".into()))?;
    if !canonical_path.starts_with(&canonical_root) || !canonical_path.is_file() {
        return Err(AppError(StatusCode::NOT_FOUND, "File not found".into()));
    }
    let bytes = tokio::fs::read(&canonical_path).await.map_err(internal)?;
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type_for(&canonical_path))
        .body(Body::from(bytes))
        .unwrap())
}

#[derive(Deserialize)]
pub(crate) struct SpaceAppBridgeBody {
    action: String,
    payload: Option<serde_json::Value>,
}

pub(crate) async fn space_apps_bridge(
    AxumPath(id): AxumPath<String>,
    Json(b): Json<SpaceAppBridgeBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !valid_space_app_id(&id) {
        return Err(AppError(StatusCode::BAD_REQUEST, "Invalid app id".into()));
    }
    match b.action.as_str() {
        "capabilities" => Ok(Json(serde_json::json!({
            "appId": id,
            "capabilities": ["llm.request", "mcp.call", "space.rest"],
            "status": "available",
            "note": "Bridge contract is available. Direct LLM/MCP execution is intentionally gated and will be wired through approved backend handlers.",
        }))),
        "llm.request" | "mcp.call" => Ok(Json(serde_json::json!({
            "appId": id,
            "action": b.action,
            "payload": b.payload,
            "status": "pending",
            "message": "This bridge action is declared for Space Apps but execution is not enabled yet.",
        }))),
        _ => Err(AppError(
            StatusCode::BAD_REQUEST,
            "Unknown bridge action".into(),
        )),
    }
}

pub(crate) async fn space_app_env(
    State(s): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let app_dir = space_app_dir(&s, &id)?;
    Ok(Json(serde_json::json!({
        "appId": id,
        "apiBase": "/api/space/apps",
        "coreBase": "/api",
        "staticBase": format!("/api/space/apps/{id}/static"),
        "appDir": app_dir.to_string_lossy(),
        "sqlite": {
            "endpoint": format!("/api/space/apps/{id}/sqlite/query"),
        },
        "config": {
            "endpoint": format!("/api/space/apps/{id}/config"),
        },
        "mcp": {
            "registerEndpoint": format!("/api/space/apps/{id}/mcp/register"),
        },
    })))
}

#[derive(Deserialize)]
pub(crate) struct AppConfigSetBody {
    value: serde_json::Value,
}

pub(crate) async fn space_app_config_list(
    State(s): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !valid_space_app_id(&id) {
        return Err(AppError(StatusCode::BAD_REQUEST, "Invalid app id".into()));
    }
    let db = db(&s)?;
    let values = db
        .with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT key, value, updated_at FROM space_app_config WHERE app_id=?1 ORDER BY key",
            )?;
            let rows: Vec<serde_json::Value> = stmt
                .query_map(params![&id], |row| {
                    let raw: String = row.get(1)?;
                    Ok(serde_json::json!({
                        "key": row.get::<_, String>(0)?,
                        "value": serde_json::from_str::<serde_json::Value>(&raw).unwrap_or(serde_json::Value::String(raw)),
                        "updated_at": row.get::<_, i64>(2)?,
                    }))
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
        .map_err(internal)?;
    Ok(Json(serde_json::json!({ "appId": id, "items": values })))
}

pub(crate) async fn space_app_config_get(
    State(s): State<Arc<UiState>>,
    AxumPath((id, key)): AxumPath<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !valid_space_app_id(&id) {
        return Err(AppError(StatusCode::BAD_REQUEST, "Invalid app id".into()));
    }
    let db = db(&s)?;
    let value = db
        .with_conn(|conn| {
            let raw: Result<String, rusqlite::Error> = conn.query_row(
                "SELECT value FROM space_app_config WHERE app_id=?1 AND key=?2",
                params![&id, &key],
                |row| row.get(0),
            );
            Ok(raw.ok())
        })
        .map_err(internal)?;
    match value {
        Some(raw) => Ok(Json(serde_json::json!({
            "key": key,
            "value": serde_json::from_str::<serde_json::Value>(&raw).unwrap_or(serde_json::Value::String(raw)),
        }))),
        None => Err(AppError(
            StatusCode::NOT_FOUND,
            "Config key not found".into(),
        )),
    }
}

pub(crate) async fn space_app_config_set(
    State(s): State<Arc<UiState>>,
    AxumPath((id, key)): AxumPath<(String, String)>,
    Json(b): Json<AppConfigSetBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !valid_space_app_id(&id) {
        return Err(AppError(StatusCode::BAD_REQUEST, "Invalid app id".into()));
    }
    if key.trim().is_empty() || key.len() > 120 {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            "Invalid config key".into(),
        ));
    }
    let raw = serde_json::to_string(&b.value).map_err(internal)?;
    let now = now_ms();
    let db = db(&s)?;
    db.with_conn(|conn| {
        conn.execute(
            "INSERT INTO space_app_config (app_id, key, value, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(app_id, key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at",
            params![&id, &key, raw, now],
        )?;
        Ok(())
    })
    .map_err(internal)?;
    Ok(Json(
        serde_json::json!({ "key": key, "value": b.value, "updated_at": now }),
    ))
}

pub(crate) async fn space_app_config_delete(
    State(s): State<Arc<UiState>>,
    AxumPath((id, key)): AxumPath<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !valid_space_app_id(&id) {
        return Err(AppError(StatusCode::BAD_REQUEST, "Invalid app id".into()));
    }
    let db = db(&s)?;
    db.with_conn(|conn| {
        conn.execute(
            "DELETE FROM space_app_config WHERE app_id=?1 AND key=?2",
            params![&id, &key],
        )?;
        Ok(())
    })
    .map_err(internal)?;
    Ok(Json(serde_json::json!({ "success": true })))
}

#[derive(Deserialize)]
pub(crate) struct SpaceAppSqliteQueryBody {
    sql: String,
    params: Option<Vec<serde_json::Value>>,
}

pub(crate) async fn space_app_sqlite_query(
    State(s): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
    Json(b): Json<SpaceAppSqliteQueryBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let app_dir = space_app_dir(&s, &id)?;
    tokio::fs::create_dir_all(&app_dir)
        .await
        .map_err(internal)?;
    let db_path = app_dir.join("app.sqlite");
    let sql = b.sql.trim().to_string();
    if sql.is_empty() {
        return Err(AppError(StatusCode::BAD_REQUEST, "SQL is required".into()));
    }
    if sql.contains('\0') {
        return Err(AppError(StatusCode::BAD_REQUEST, "Invalid SQL".into()));
    }
    let params_json = b.params.unwrap_or_default();
    let result = tokio::task::spawn_blocking(move || -> Result<serde_json::Value, String> {
        let conn = rusqlite::Connection::open(db_path).map_err(|e| e.to_string())?;
        let values: Vec<SqlValue> = params_json.iter().map(json_to_sql_value).collect();
        let refs: Vec<&dyn rusqlite::ToSql> =
            values.iter().map(|v| v as &dyn rusqlite::ToSql).collect();
        let verb = sql
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if matches!(verb.as_str(), "select" | "with" | "pragma") {
            let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
            let column_names: Vec<String> = stmt
                .column_names()
                .into_iter()
                .map(ToString::to_string)
                .collect();
            let rows = stmt
                .query_map(&refs[..], |row| {
                    let mut obj = serde_json::Map::new();
                    for (idx, name) in column_names.iter().enumerate() {
                        let value: SqlValue = row.get(idx)?;
                        obj.insert(name.clone(), sql_value_to_json(value));
                    }
                    Ok(serde_json::Value::Object(obj))
                })
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!({ "rows": rows }))
        } else {
            let changed = conn.execute(&sql, &refs[..]).map_err(|e| e.to_string())?;
            Ok(serde_json::json!({
                "rowsAffected": changed,
                "lastInsertRowId": conn.last_insert_rowid(),
            }))
        }
    })
    .await
    .map_err(internal)?
    .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(result))
}

#[derive(Deserialize)]
pub(crate) struct SpaceAppMcpRegisterBody {
    name: Option<String>,
    transport: String,
    description: Option<String>,
    url: Option<String>,
    command: Option<String>,
    args: Option<Vec<String>>,
    env: Option<HashMap<String, String>>,
    headers: Option<HashMap<String, String>>,
    use_tools: Option<Vec<String>>,
    enabled: Option<bool>,
}

pub(crate) async fn space_app_mcp_register(
    State(s): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
    Json(b): Json<SpaceAppMcpRegisterBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !valid_space_app_id(&id) {
        return Err(AppError(StatusCode::BAD_REQUEST, "Invalid app id".into()));
    }
    let mgr = s.mcp_manager.as_ref().ok_or_else(|| {
        AppError(
            StatusCode::SERVICE_UNAVAILABLE,
            "MCP manager not initialized".into(),
        )
    })?;
    let transport = match b.transport.as_str() {
        "stdio" => crate::mcp::config::McpTransportType::Stdio,
        "sse" => crate::mcp::config::McpTransportType::Sse,
        "http" => crate::mcp::config::McpTransportType::Http,
        _ => {
            return Err(AppError(
                StatusCode::BAD_REQUEST,
                "Invalid MCP transport".into(),
            ))
        }
    };
    let name = b.name.unwrap_or_else(|| format!("space-app-{id}"));
    let mut env = b.env.unwrap_or_default();
    env.insert("SENCLAW_SPACE_APP_ID".into(), id.clone());
    env.insert("SENCLAW_SPACE_API_BASE".into(), "/api/space/apps".into());
    let config = crate::mcp::config::ExternalMcpServerConfig {
        name,
        transport,
        description: b.description,
        enabled: b.enabled.unwrap_or(true),
        use_tools: b.use_tools,
        command: b.command,
        args: b.args.unwrap_or_default(),
        env,
        url: b.url,
        headers: b.headers.unwrap_or_default(),
    };
    let info = mgr
        .add_or_update(config, crate::mcp::config::McpScopeType::Project)
        .await
        .map_err(internal)?;
    Ok(Json(serde_json::to_value(info).unwrap_or_default()))
}

/// App detail: the manifest's declared `mcp` block plus the live MCP server
/// info (status + tools) for the detail page.
pub(crate) async fn space_app_mcp_info(
    State(s): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !valid_space_app_id(&id) {
        return Err(AppError(StatusCode::BAD_REQUEST, "Invalid app id".into()));
    }
    let db = db(&s)?;
    let manifest: Option<serde_json::Value> = db
        .with_conn(|conn| {
            let raw: Result<String, rusqlite::Error> = conn.query_row(
                "SELECT manifest FROM space_apps WHERE id=?1",
                params![&id],
                |row| row.get(0),
            );
            Ok(raw.ok().and_then(|s| serde_json::from_str(&s).ok()))
        })
        .map_err(internal)?;

    let declared = manifest.as_ref().and_then(|m| m.get("mcp")).cloned();

    let server = match (
        declared
            .as_ref()
            .and_then(|m| m.get("name"))
            .and_then(|v| v.as_str()),
        s.mcp_manager.as_ref(),
    ) {
        (Some(name), Some(mgr)) => {
            let info = mgr.get_server_info(name).await;
            Some(serde_json::to_value(info).unwrap_or_default())
        }
        _ => None,
    };

    Ok(Json(serde_json::json!({
        "appId": id,
        "declared": declared,
        "server": server,
    })))
}

/// Best-effort: after install, install the app's bundled skills, then launch
/// its server runtime (if any) and auto-register its declared MCP.
async fn try_autoregister_app_mcp(s: &UiState, app_id: &str, manifest: &serde_json::Value) {
    // Resolve where the app's files live (explicit localPath wins).
    let app_dir = manifest
        .get("install")
        .and_then(|i| i.get("localPath"))
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
        .or_else(|| space_app_dir(s, app_id).ok())
        .unwrap_or_default();

    // Install bundled skills (read-only, tied to the app).
    super::space_skills::install_app_skills(&s.config, app_id, &app_dir, manifest);

    let (Some(launcher), Some(mgr), Some(db)) = (
        s.space_mcp_launcher.as_ref(),
        s.mcp_manager.as_ref(),
        s.db.as_deref(),
    ) else {
        return;
    };
    let base_url = format!("http://127.0.0.1:{}", s.config.ui_server.port);
    match launcher
        .run_and_register(db, mgr, app_id, &app_dir, manifest, &base_url)
        .await
    {
        Ok(Some(name)) => {
            tracing::info!("[space-mcp] auto-registered '{name}' on install of '{app_id}'")
        }
        Ok(None) => {}
        Err(e) => tracing::warn!("[space-mcp] install auto-register for '{app_id}' failed: {e}"),
    }
}

// ─── Reminder (set reminder on existing event) ────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct SetReminderBody {
    reminder_min: i64,
    group_folder: Option<String>,
    chat_jid: Option<String>,
}

pub(crate) async fn space_events_set_reminder(
    State(s): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
    Json(b): Json<SetReminderBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_arc =
        s.db.clone()
            .ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "DB not available".into()))?;

    let srv = crate::mcp::space_server::SpaceServer::new(db_arc);
    let result = srv.set_reminder(
        id,
        b.reminder_min,
        b.group_folder.as_deref().unwrap_or("default"),
        b.chat_jid.as_deref().unwrap_or(""),
    );

    if result.is_error {
        return Err(AppError(StatusCode::INTERNAL_SERVER_ERROR, result.content));
    }
    let v: serde_json::Value = serde_json::from_str(&result.content).unwrap_or_default();
    Ok(Json(v))
}

// ─── External sync endpoints (delegate to SpaceServer stubs) ─────────────────

#[derive(Deserialize)]
pub(crate) struct SyncBody {
    token: String,
    days: Option<u32>,
}

pub(crate) async fn space_sync_google_calendar(
    State(s): State<Arc<UiState>>,
    Json(b): Json<SyncBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_arc =
        s.db.clone()
            .ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "DB not available".into()))?;
    let srv = crate::mcp::space_server::SpaceServer::new(db_arc);
    let r = srv.sync_google_calendar(b.token, b.days.unwrap_or(30));
    Ok(Json(serde_json::from_str(&r.content).unwrap_or_default()))
}

pub(crate) async fn space_sync_apple_calendar(
    State(s): State<Arc<UiState>>,
    Json(b): Json<SyncBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_arc =
        s.db.clone()
            .ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "DB not available".into()))?;
    let srv = crate::mcp::space_server::SpaceServer::new(db_arc);
    let r = srv.sync_apple_calendar(b.token, b.days.unwrap_or(30));
    Ok(Json(serde_json::from_str(&r.content).unwrap_or_default()))
}

pub(crate) async fn space_sync_apple_notes(
    State(s): State<Arc<UiState>>,
    Json(b): Json<SyncBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_arc =
        s.db.clone()
            .ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "DB not available".into()))?;
    let srv = crate::mcp::space_server::SpaceServer::new(db_arc);
    let r = srv.sync_apple_notes(b.token);
    Ok(Json(serde_json::from_str(&r.content).unwrap_or_default()))
}

pub(crate) async fn space_sync_gmail(
    State(s): State<Arc<UiState>>,
    Json(b): Json<SyncBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_arc =
        s.db.clone()
            .ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "DB not available".into()))?;
    let srv = crate::mcp::space_server::SpaceServer::new(db_arc);
    let r = srv.sync_gmail(b.token, b.days.unwrap_or(7));
    Ok(Json(serde_json::from_str(&r.content).unwrap_or_default()))
}

#[derive(Deserialize)]
pub(crate) struct GoogleWorkspaceSyncBody {
    token: String,
    days: Option<u32>,
    services: Option<Vec<String>>,
}

pub(crate) async fn space_sync_google_workspace(
    State(s): State<Arc<UiState>>,
    Json(b): Json<GoogleWorkspaceSyncBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let token = b.token.trim().to_string();
    if token.is_empty() {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            "Google access token required".into(),
        ));
    }

    let services = b.services.unwrap_or_else(|| {
        vec![
            "gmail".to_string(),
            "calendar".to_string(),
            "notes".to_string(),
        ]
    });
    let days = b.days.unwrap_or(7);
    let db_arc =
        s.db.clone()
            .ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "DB not available".into()))?;
    let srv = crate::mcp::space_server::SpaceServer::new(db_arc);

    let mut results = serde_json::Map::new();
    for service in services {
        match service.as_str() {
            "gmail" => {
                let r = srv.sync_gmail(token.clone(), days);
                results.insert(
                    "gmail".to_string(),
                    serde_json::from_str(&r.content)
                        .unwrap_or_else(|_| serde_json::json!({ "status": "error" })),
                );
            }
            "calendar" => {
                let r = srv.sync_google_calendar(token.clone(), days);
                results.insert(
                    "calendar".to_string(),
                    serde_json::from_str(&r.content)
                        .unwrap_or_else(|_| serde_json::json!({ "status": "error" })),
                );
            }
            "notes" => {
                results.insert(
                    "notes".to_string(),
                    serde_json::json!({
                        "status": "pending",
                        "message": "Google Workspace notes sync is not implemented yet. The connector reserves this slot for Keep/Drive-based notes import.",
                    }),
                );
            }
            other => {
                results.insert(
                    other.to_string(),
                    serde_json::json!({
                        "status": "skipped",
                        "message": "Unknown Google Workspace service",
                    }),
                );
            }
        }
    }

    Ok(Json(serde_json::json!({
        "status": "completed",
        "days": days,
        "results": results,
    })))
}
