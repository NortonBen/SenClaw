//! REST handlers for the Space feature (notes, calendar, email, schedules, apps).
//!
//! Routes are registered in `core.rs` under the `/api/space/*` prefix.
//! All DB access goes through `Db::with_conn` on the SQLite pool.

use std::sync::Arc;

use axum::{
    extract::{Path as AxumPath, Query, State},
    http::StatusCode,
    response::Json,
};
use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};
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
                        tags: serde_json::from_str(
                            &row.get::<_, String>(3).unwrap_or_default(),
                        )
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
    let db_arc = s
        .db
        .clone()
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
    let db_arc = s
        .db
        .clone()
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

    let v: serde_json::Value =
        serde_json::from_str(&result.content).unwrap_or_default();
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
    let db_arc = s
        .db
        .clone()
        .ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "DB not available".into()))?;

    let space_srv = crate::mcp::space_server::SpaceServer::new(db_arc);
    let result = space_srv.today_summary();

    let v: serde_json::Value =
        serde_json::from_str(&result.content).unwrap_or_default();
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
    let db_arc = s
        .db
        .clone()
        .ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "DB not available".into()))?;

    let srv = crate::mcp::space_server::SpaceServer::new(db_arc);
    let result = srv.email_inbox(q.account_id, q.limit);

    let v: serde_json::Value =
        serde_json::from_str(&result.content).unwrap_or_default();
    Ok(Json(v))
}

pub(crate) async fn space_email_read(
    State(s): State<Arc<UiState>>,
    AxumPath(msg_id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_arc = s
        .db
        .clone()
        .ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "DB not available".into()))?;

    let srv = crate::mcp::space_server::SpaceServer::new(db_arc);
    let result = srv.email_read(msg_id);
    if result.is_error {
        return Err(AppError(StatusCode::NOT_FOUND, result.content));
    }
    let v: serde_json::Value =
        serde_json::from_str(&result.content).unwrap_or_default();
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
    let db_arc = s
        .db
        .clone()
        .ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "DB not available".into()))?;

    let srv = crate::mcp::space_server::SpaceServer::new(db_arc);
    let result = srv.email_search(q.q, q.account_id, q.limit);
    let v: serde_json::Value =
        serde_json::from_str(&result.content).unwrap_or_default();
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
    let db_arc = s
        .db
        .clone()
        .ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "DB not available".into()))?;

    let srv = crate::mcp::space_server::SpaceServer::new(db_arc);
    let result = srv.email_compose(b.to, b.subject, b.body, b.account_id);
    if result.is_error {
        return Err(AppError(StatusCode::BAD_GATEWAY, result.content));
    }
    let v: serde_json::Value =
        serde_json::from_str(&result.content).unwrap_or_default();
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
    let body = format!(
        "Kính gửi,\n\n{}\n\nTrân trọng,\n[Tên của bạn]",
        b.prompt
    );
    Ok(Json(serde_json::json!({ "subject": subject, "body": body })))
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

fn default_imap_port() -> i64 { 993 }
fn default_smtp_port() -> i64 { 587 }
fn default_true() -> bool { true }

pub(crate) async fn space_email_accounts_create(
    State(s): State<Arc<UiState>>,
    Json(b): Json<EmailAccountCreateBody>,
) -> Result<Json<serde_json::Value>, AppError> {
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

    Ok(Json(serde_json::json!({ "id": id, "label": b.label, "email": b.email })))
}

pub(crate) async fn space_email_accounts_delete(
    State(s): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = db(&s)?;
    db.with_conn(|conn| {
        conn.execute("DELETE FROM space_email_accounts WHERE id=?1", params![id])?;
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
    let db_arc = s
        .db
        .clone()
        .ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "DB not available".into()))?;

    let srv = crate::mcp::space_server::SpaceServer::new(db_arc);
    let result = srv.list_schedules(q.group);
    let v: serde_json::Value =
        serde_json::from_str(&result.content).unwrap_or_default();
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
    let db_arc = s
        .db
        .clone()
        .ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "DB not available".into()))?;

    let srv = crate::mcp::space_server::SpaceServer::new(db_arc);
    let result = srv
        .schedule_activity(b.prompt, b.cron, b.group_folder, b.chat_jid)
        .await;
    if result.is_error {
        return Err(AppError(StatusCode::BAD_REQUEST, result.content));
    }
    let v: serde_json::Value =
        serde_json::from_str(&result.content).unwrap_or_default();
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
    let db_arc = s
        .db
        .clone()
        .ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "DB not available".into()))?;

    let srv = crate::mcp::space_server::SpaceServer::new(db_arc);
    let result = srv.list_schedules(b.group_folder.clone()); // validate ownership
    if result.is_error {
        return Err(AppError(StatusCode::INTERNAL_SERVER_ERROR, result.content));
    }

    // Cancel = set status completed
    let db_ref = s.db.as_deref().ok_or_else(|| {
        AppError(StatusCode::SERVICE_UNAVAILABLE, "DB not available".into())
    })?;
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
        .map_err(|e| AppError(StatusCode::BAD_GATEWAY, format!("Fetch manifest failed: {e}")))?
        .json()
        .await
        .map_err(|e| AppError(StatusCode::BAD_GATEWAY, format!("Parse manifest failed: {e}")))?;

    let app_id = manifest_json["id"]
        .as_str()
        .unwrap_or(&Uuid::new_v4().to_string())
        .to_string();

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

    Ok(Json(serde_json::json!({
        "id": app_id,
        "manifest": manifest_json,
        "enabled": true,
        "installed_at": now,
    })))
}

pub(crate) async fn space_apps_delete(
    State(s): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = db(&s)?;
    db.with_conn(|conn| {
        conn.execute("DELETE FROM space_apps WHERE id=?1", params![id])?;
        Ok(())
    })
    .map_err(internal)?;

    Ok(Json(serde_json::json!({ "success": true })))
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
    let db_arc = s
        .db
        .clone()
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
    let v: serde_json::Value =
        serde_json::from_str(&result.content).unwrap_or_default();
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
    let db_arc = s.db.clone().ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "DB not available".into()))?;
    let srv = crate::mcp::space_server::SpaceServer::new(db_arc);
    let r = srv.sync_google_calendar(b.token, b.days.unwrap_or(30));
    Ok(Json(serde_json::from_str(&r.content).unwrap_or_default()))
}

pub(crate) async fn space_sync_apple_calendar(
    State(s): State<Arc<UiState>>,
    Json(b): Json<SyncBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_arc = s.db.clone().ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "DB not available".into()))?;
    let srv = crate::mcp::space_server::SpaceServer::new(db_arc);
    let r = srv.sync_apple_calendar(b.token, b.days.unwrap_or(30));
    Ok(Json(serde_json::from_str(&r.content).unwrap_or_default()))
}

pub(crate) async fn space_sync_apple_notes(
    State(s): State<Arc<UiState>>,
    Json(b): Json<SyncBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_arc = s.db.clone().ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "DB not available".into()))?;
    let srv = crate::mcp::space_server::SpaceServer::new(db_arc);
    let r = srv.sync_apple_notes(b.token);
    Ok(Json(serde_json::from_str(&r.content).unwrap_or_default()))
}

pub(crate) async fn space_sync_gmail(
    State(s): State<Arc<UiState>>,
    Json(b): Json<SyncBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_arc = s.db.clone().ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "DB not available".into()))?;
    let srv = crate::mcp::space_server::SpaceServer::new(db_arc);
    let r = srv.sync_gmail(b.token, b.days.unwrap_or(7));
    Ok(Json(serde_json::from_str(&r.content).unwrap_or_default()))
}
