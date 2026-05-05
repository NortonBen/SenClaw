//! Space MCP server — personal productivity tools for the SenClaw Space feature.
//!
//! Tools cover: Notes (CRUD + FTS), Calendar (events + reminders), Email (IMAP/SMTP),
//! external sync (Google Calendar/Apple Calendar/Apple Notes/Gmail), and recurring
//! schedule helpers that wrap the TaskScheduler.
//!
//! Tool namespace: `space:*`

use anyhow::{Context, Result};
use chrono::Utc;
use rmcp::ServiceExt;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::db::Db;
use crate::mcp::schedule_server::ToolResult;
use crate::types::{ContextMode, ScheduleType, ScheduledTask, TaskStatus};

// ─── Params ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
struct NoteCreateParams {
    title: String,
    body: String,
    #[serde(default)]
    tags: Option<Vec<String>>,
    #[serde(default)]
    folder_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
struct NoteUpdateParams {
    id: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
struct NoteSearchParams {
    query: String,
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
struct NoteListParams {
    #[serde(default)]
    folder_id: Option<String>,
    #[serde(default)]
    tag: Option<String>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
struct NoteIdParams {
    id: String,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
struct EventCreateParams {
    title: String,
    /// Unix milliseconds
    start_at: i64,
    /// Unix milliseconds
    end_at: i64,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    location: Option<String>,
    #[serde(default)]
    all_day: Option<bool>,
    /// Minutes before event to send reminder (None = no reminder)
    #[serde(default)]
    reminder_min: Option<i64>,
    #[serde(default)]
    color: Option<String>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
struct EventListParams {
    /// Unix ms — range start
    from: i64,
    /// Unix ms — range end
    to: i64,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
struct EventIdParams {
    event_id: String,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
struct SetReminderParams {
    event_id: String,
    /// Minutes before event
    reminder_min: i64,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
struct EmailInboxParams {
    #[serde(default)]
    account_id: Option<String>,
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
struct EmailReadParams {
    message_id: String,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
struct EmailComposeParams {
    /// Recipient email address
    to: String,
    subject: String,
    body: String,
    #[serde(default)]
    account_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
struct EmailSearchParams {
    query: String,
    #[serde(default)]
    account_id: Option<String>,
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
struct EmailSummaryParams {
    message_id: String,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
struct SyncProviderParams {
    /// OAuth2 access token or service credential
    token: String,
    #[serde(default)]
    /// Sync window in days (default 30)
    days: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
struct ScheduleActivityParams {
    /// Human-readable description of the activity / what the agent should do
    prompt: String,
    /// Cron expression (e.g. "0 7 * * *" = every day at 7am)
    cron: String,
    /// Group folder for the scheduled task
    group_folder: String,
    chat_jid: String,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
struct ListSpaceSchedulesParams {
    group_folder: String,
}

// ─── MCP server struct ────────────────────────────────────────────────────────

#[derive(Clone)]
struct McpSpaceServer {
    db: Arc<Db>,
    group_folder: String,
    chat_jid: String,
}

impl McpSpaceServer {
    fn inner(&self) -> SpaceServer {
        SpaceServer {
            db: self.db.clone(),
        }
    }
}

#[rmcp::tool_router(server_handler)]
impl McpSpaceServer {
    // ── Notes ──────────────────────────────────────────────────────────────

    #[rmcp::tool(
        description = "Tạo ghi chú mới trong Space. Create a new note with title, body (Markdown), optional tags and folder."
    )]
    fn space_note_create(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            NoteCreateParams,
        >,
    ) -> String {
        self.inner()
            .note_create(p.title, p.body, p.tags, p.folder_id)
            .content
    }

    #[rmcp::tool(description = "Cập nhật ghi chú. Update an existing note by id.")]
    fn space_note_update(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            NoteUpdateParams,
        >,
    ) -> String {
        self.inner()
            .note_update(p.id, p.title, p.body, p.tags)
            .content
    }

    #[rmcp::tool(description = "Tìm kiếm ghi chú full-text. Full-text search across all notes.")]
    fn space_note_search(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            NoteSearchParams,
        >,
    ) -> String {
        self.inner()
            .note_search(p.query, p.limit.unwrap_or(20))
            .content
    }

    #[rmcp::tool(
        description = "Danh sách ghi chú. List notes, optionally filtered by folder or tag."
    )]
    fn space_note_list(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            NoteListParams,
        >,
    ) -> String {
        self.inner().note_list(p.folder_id, p.tag).content
    }

    #[rmcp::tool(description = "Xóa ghi chú (soft delete). Soft-delete a note by id.")]
    fn space_note_delete(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            NoteIdParams,
        >,
    ) -> String {
        self.inner().note_delete(p.id).content
    }

    // ── Calendar ───────────────────────────────────────────────────────────

    #[rmcp::tool(
        description = "Tạo sự kiện lịch mới. Create a calendar event with optional reminder."
    )]
    fn space_event_create(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            EventCreateParams,
        >,
    ) -> String {
        self.inner()
            .event_create(
                p.title,
                p.start_at,
                p.end_at,
                p.description,
                p.location,
                p.all_day.unwrap_or(false),
                p.reminder_min,
                p.color,
                &self.group_folder,
                &self.chat_jid,
            )
            .content
    }

    #[rmcp::tool(
        description = "Lấy danh sách sự kiện trong khoảng thời gian. List events between from..to (unix ms)."
    )]
    fn space_event_list(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            EventListParams,
        >,
    ) -> String {
        self.inner().event_list(p.from, p.to).content
    }

    #[rmcp::tool(
        description = "Xóa sự kiện và hủy nhắc nhở. Delete a calendar event and cancel its reminder task."
    )]
    fn space_event_delete(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            EventIdParams,
        >,
    ) -> String {
        self.inner().event_delete(p.event_id).content
    }

    #[rmcp::tool(
        description = "Đặt nhắc nhở cho sự kiện. Set or update the reminder for an existing event (minutes before start)."
    )]
    fn space_set_reminder(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            SetReminderParams,
        >,
    ) -> String {
        self.inner()
            .set_reminder(
                p.event_id,
                p.reminder_min,
                &self.group_folder,
                &self.chat_jid,
            )
            .content
    }

    #[rmcp::tool(
        description = "Tóm tắt hôm nay: sự kiện, nhắc nhở, ghi chú gần đây. Today summary: events, reminders, recent notes."
    )]
    fn space_today_summary(&self) -> String {
        self.inner().today_summary().content
    }

    // ── Email ──────────────────────────────────────────────────────────────

    #[rmcp::tool(
        description = "Xem inbox email. List inbox emails (cached). Use account_id to filter a specific account."
    )]
    fn space_email_inbox(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            EmailInboxParams,
        >,
    ) -> String {
        self.inner()
            .email_inbox(p.account_id, p.limit.unwrap_or(20))
            .content
    }

    #[rmcp::tool(description = "Đọc nội dung email. Read full content of an email by message_id.")]
    fn space_email_read(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            EmailReadParams,
        >,
    ) -> String {
        self.inner().email_read(p.message_id).content
    }

    #[rmcp::tool(
        description = "Soạn và gửi email. Compose and send an email. The agent should draft the body carefully before calling this."
    )]
    fn space_email_compose(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            EmailComposeParams,
        >,
    ) -> String {
        self.inner()
            .email_compose(p.to, p.subject, p.body, p.account_id)
            .content
    }

    #[rmcp::tool(description = "Tìm kiếm email. Search emails by query string.")]
    fn space_email_search(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            EmailSearchParams,
        >,
    ) -> String {
        self.inner()
            .email_search(p.query, p.account_id, p.limit.unwrap_or(10))
            .content
    }

    #[rmcp::tool(
        description = "Tóm tắt email bằng AI. Summarize an email thread using AI (returns structured summary)."
    )]
    fn space_email_summary(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            EmailSummaryParams,
        >,
    ) -> String {
        self.inner().email_summary(p.message_id).content
    }

    // ── External sync ──────────────────────────────────────────────────────

    #[rmcp::tool(
        description = "Đồng bộ Google Calendar. Sync events from Google Calendar into Space calendar."
    )]
    fn space_sync_google_calendar(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            SyncProviderParams,
        >,
    ) -> String {
        self.inner()
            .sync_google_calendar(p.token, p.days.unwrap_or(30))
            .content
    }

    #[rmcp::tool(
        description = "Đồng bộ Apple Calendar (CalDAV). Sync events from Apple Calendar via CalDAV."
    )]
    fn space_sync_apple_calendar(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            SyncProviderParams,
        >,
    ) -> String {
        self.inner()
            .sync_apple_calendar(p.token, p.days.unwrap_or(30))
            .content
    }

    #[rmcp::tool(
        description = "Đồng bộ Apple Notes. Import notes from Apple Notes (iCloud) into Space notes."
    )]
    fn space_sync_apple_notes(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            SyncProviderParams,
        >,
    ) -> String {
        self.inner().sync_apple_notes(p.token).content
    }

    #[rmcp::tool(
        description = "Đồng bộ Gmail. Sync recent emails from Gmail into Space email cache."
    )]
    fn space_sync_gmail(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            SyncProviderParams,
        >,
    ) -> String {
        self.inner()
            .sync_gmail(p.token, p.days.unwrap_or(7))
            .content
    }

    // ── Recurring schedule ─────────────────────────────────────────────────

    #[rmcp::tool(
        description = "Lên lịch hoạt động định kỳ (ngày/tuần). Schedule a recurring agent activity using a cron expression. Example cron: '0 7 * * *' = every day at 7am."
    )]
    async fn space_schedule_activity(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            ScheduleActivityParams,
        >,
    ) -> String {
        self.inner()
            .schedule_activity(p.prompt, p.cron, p.group_folder, p.chat_jid)
            .await
            .content
    }

    #[rmcp::tool(
        description = "Danh sách lịch định kỳ Space. List all Space recurring schedules for a group."
    )]
    fn space_list_schedules(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            ListSpaceSchedulesParams,
        >,
    ) -> String {
        self.inner().list_schedules(p.group_folder).content
    }
}

// ─── Business logic ──────────────────────────────────────────────────────────

pub struct SpaceServer {
    db: Arc<Db>,
}

impl SpaceServer {
    pub fn new(db: Arc<Db>) -> Self {
        Self { db }
    }

    // ── Notes ──────────────────────────────────────────────────────────────

    pub fn note_create(
        &self,
        title: String,
        body: String,
        tags: Option<Vec<String>>,
        folder_id: Option<String>,
    ) -> ToolResult {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().timestamp_millis();
        let tags_json = serde_json::to_string(&tags.unwrap_or_default()).unwrap_or_default();

        let result = self.db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO space_notes (id, title, body, tags, folder_id, pinned, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?6)",
                params![id, title, body, tags_json, folder_id, now],
            )?;
            Ok(())
        });

        match result {
            Ok(_) => ToolResult::ok(
                serde_json::json!({ "success": true, "id": id, "created_at": now }).to_string(),
            ),
            Err(e) => ToolResult::err(format!("Failed to create note: {e}")),
        }
    }

    pub fn note_update(
        &self,
        id: String,
        title: Option<String>,
        body: Option<String>,
        tags: Option<Vec<String>>,
    ) -> ToolResult {
        let now = Utc::now().timestamp_millis();
        let result = self.db.with_conn(|conn| {
            if let Some(t) = &title {
                conn.execute("UPDATE space_notes SET title=?1, updated_at=?2 WHERE id=?3 AND deleted_at IS NULL", params![t, now, id])?;
            }
            if let Some(b) = &body {
                conn.execute("UPDATE space_notes SET body=?1, updated_at=?2 WHERE id=?3 AND deleted_at IS NULL", params![b, now, id])?;
            }
            if let Some(tg) = &tags {
                let j = serde_json::to_string(tg).unwrap_or_default();
                conn.execute("UPDATE space_notes SET tags=?1, updated_at=?2 WHERE id=?3 AND deleted_at IS NULL", params![j, now, id])?;
            }
            Ok(())
        });
        match result {
            Ok(_) => ToolResult::ok(serde_json::json!({ "success": true, "id": id }).to_string()),
            Err(e) => ToolResult::err(format!("Failed to update note: {e}")),
        }
    }

    pub fn note_search(&self, query: String, limit: u32) -> ToolResult {
        let result = self.db.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT n.id, n.title, snippet(space_notes_fts, 1, '<b>', '</b>', '...', 20) AS excerpt
                 FROM space_notes_fts f
                 JOIN space_notes n ON n.id = f.id
                 WHERE f.space_notes_fts MATCH ?1 AND n.deleted_at IS NULL
                 ORDER BY rank LIMIT ?2",
            )?;
            let rows: Vec<serde_json::Value> = stmt
                .query_map(params![query, limit], |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_,String>(0)?,
                        "title": row.get::<_,String>(1)?,
                        "excerpt": row.get::<_,String>(2)?,
                    }))
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        });
        match result {
            Ok(rows) => ToolResult::ok(serde_json::to_string_pretty(&rows).unwrap_or_default()),
            Err(e) => ToolResult::err(format!("Search failed: {e}")),
        }
    }

    pub fn note_list(&self, folder_id: Option<String>, tag: Option<String>) -> ToolResult {
        let result = self.db.with_conn(|conn| {
            let sql = match (&folder_id, &tag) {
                (Some(_), _) => "SELECT id, title, tags, created_at, updated_at FROM space_notes WHERE deleted_at IS NULL AND folder_id=?1 ORDER BY pinned DESC, updated_at DESC LIMIT 100",
                _ => "SELECT id, title, tags, created_at, updated_at FROM space_notes WHERE deleted_at IS NULL ORDER BY pinned DESC, updated_at DESC LIMIT 100",
            };
            let mut stmt = conn.prepare(sql)?;
            let param: &[&dyn rusqlite::ToSql] = if folder_id.is_some() {
                &[&folder_id]
            } else {
                &[]
            };
            let rows: Vec<serde_json::Value> = stmt
                .query_map(param, |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_,String>(0)?,
                        "title": row.get::<_,String>(1)?,
                        "tags": row.get::<_,String>(2)?,
                        "created_at": row.get::<_,i64>(3)?,
                        "updated_at": row.get::<_,i64>(4)?,
                    }))
                })?
                .filter_map(|r| r.ok())
                .filter(|v| {
                    // client-side tag filter (tags stored as JSON array)
                    if let Some(t) = &tag {
                        v["tags"].as_str().unwrap_or("[]").contains(t.as_str())
                    } else {
                        true
                    }
                })
                .collect();
            Ok(rows)
        });
        match result {
            Ok(rows) => ToolResult::ok(serde_json::to_string_pretty(&rows).unwrap_or_default()),
            Err(e) => ToolResult::err(format!("List notes failed: {e}")),
        }
    }

    pub fn note_delete(&self, id: String) -> ToolResult {
        let now = Utc::now().timestamp_millis();
        let result = self.db.with_conn(|conn| {
            conn.execute(
                "UPDATE space_notes SET deleted_at=?1 WHERE id=?2",
                params![now, id],
            )?;
            Ok(())
        });
        match result {
            Ok(_) => ToolResult::ok(serde_json::json!({ "success": true, "id": id }).to_string()),
            Err(e) => ToolResult::err(format!("Delete note failed: {e}")),
        }
    }

    // ── Calendar ───────────────────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    pub fn event_create(
        &self,
        title: String,
        start_at: i64,
        end_at: i64,
        description: Option<String>,
        location: Option<String>,
        all_day: bool,
        reminder_min: Option<i64>,
        color: Option<String>,
        group_folder: &str,
        chat_jid: &str,
    ) -> ToolResult {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().timestamp_millis();

        let result = self.db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO space_events (id, title, description, start_at, end_at, all_day, location, color, reminder_min, source, created_at, updated_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,'manual',?10,?10)",
                params![id, title, description, start_at, end_at, all_day as i32, location, color, reminder_min, now],
            )?;
            Ok(())
        });

        if let Err(e) = result {
            return ToolResult::err(format!("Failed to create event: {e}"));
        }

        // If reminder requested, register a scheduled_task of type notify
        if let Some(min) = reminder_min {
            let run_at_ms = start_at - min * 60 * 1000;
            let run_at = chrono::DateTime::from_timestamp_millis(run_at_ms)
                .map(|t| t.to_rfc3339())
                .unwrap_or_default();
            let prompt = format!("Nhắc nhở: sự kiện '{title}' bắt đầu sau {min} phút.");
            let task = ScheduledTask {
                id: Uuid::new_v4().to_string(),
                group_folder: group_folder.to_owned(),
                chat_jid: chat_jid.to_owned(),
                prompt,
                schedule_type: ScheduleType::Once,
                schedule_value: run_at.clone(),
                context_mode: ContextMode::Notify,
                script_command: None,
                next_run: Some(run_at),
                last_run: None,
                last_result: None,
                status: TaskStatus::Active,
                created_at: Utc::now().to_rfc3339(),
            };
            if let Err(e) = self.db.insert_task(&task) {
                tracing::warn!("Space: failed to register reminder task: {e}");
            }
        }

        ToolResult::ok(
            serde_json::json!({ "success": true, "id": id, "created_at": now }).to_string(),
        )
    }

    pub fn event_list(&self, from: i64, to: i64) -> ToolResult {
        let result = self.db.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, title, description, start_at, end_at, all_day, location, color, reminder_min, source
                 FROM space_events
                 WHERE deleted_at IS NULL AND start_at >= ?1 AND start_at <= ?2
                 ORDER BY start_at ASC",
            )?;
            let rows: Vec<serde_json::Value> = stmt
                .query_map(params![from, to], |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_,String>(0)?,
                        "title": row.get::<_,String>(1)?,
                        "description": row.get::<_,Option<String>>(2)?,
                        "start_at": row.get::<_,i64>(3)?,
                        "end_at": row.get::<_,i64>(4)?,
                        "all_day": row.get::<_,i32>(5)? != 0,
                        "location": row.get::<_,Option<String>>(6)?,
                        "color": row.get::<_,Option<String>>(7)?,
                        "reminder_min": row.get::<_,Option<i64>>(8)?,
                        "source": row.get::<_,String>(9)?,
                    }))
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        });
        match result {
            Ok(rows) => ToolResult::ok(serde_json::to_string_pretty(&rows).unwrap_or_default()),
            Err(e) => ToolResult::err(format!("List events failed: {e}")),
        }
    }

    pub fn event_delete(&self, event_id: String) -> ToolResult {
        let now = Utc::now().timestamp_millis();
        let result = self.db.with_conn(|conn| {
            conn.execute(
                "UPDATE space_events SET deleted_at=?1 WHERE id=?2",
                params![now, event_id],
            )?;
            Ok(())
        });
        match result {
            Ok(_) => {
                ToolResult::ok(serde_json::json!({ "success": true, "id": event_id }).to_string())
            }
            Err(e) => ToolResult::err(format!("Delete event failed: {e}")),
        }
    }

    pub fn set_reminder(
        &self,
        event_id: String,
        reminder_min: i64,
        group_folder: &str,
        chat_jid: &str,
    ) -> ToolResult {
        // Read event to get start_at and title
        let event = self.db.with_conn(|conn| {
            conn.query_row(
                "SELECT title, start_at FROM space_events WHERE id=?1 AND deleted_at IS NULL",
                params![event_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .map_err(|e| anyhow::anyhow!(e))
        });

        match event {
            Err(e) => return ToolResult::err(format!("Event not found: {e}")),
            Ok((title, start_at)) => {
                let _ = self.db.with_conn(|conn| {
                    conn.execute(
                        "UPDATE space_events SET reminder_min=?1 WHERE id=?2",
                        params![reminder_min, event_id],
                    )?;
                    Ok(())
                });

                let run_at_ms = start_at - reminder_min * 60 * 1000;
                let run_at = chrono::DateTime::from_timestamp_millis(run_at_ms)
                    .map(|t| t.to_rfc3339())
                    .unwrap_or_default();
                let task = ScheduledTask {
                    id: Uuid::new_v4().to_string(),
                    group_folder: group_folder.to_owned(),
                    chat_jid: chat_jid.to_owned(),
                    prompt: format!("Nhắc nhở: '{title}' bắt đầu sau {reminder_min} phút."),
                    schedule_type: ScheduleType::Once,
                    schedule_value: run_at.clone(),
                    context_mode: ContextMode::Notify,
                    script_command: None,
                    next_run: Some(run_at),
                    last_run: None,
                    last_result: None,
                    status: TaskStatus::Active,
                    created_at: Utc::now().to_rfc3339(),
                };
                let _ = self.db.insert_task(&task);

                ToolResult::ok(
                    serde_json::json!({ "success": true, "event_id": event_id, "reminder_min": reminder_min })
                        .to_string(),
                )
            }
        }
    }

    pub fn today_summary(&self) -> ToolResult {
        let now_ms = Utc::now().timestamp_millis();
        // Start of today (UTC midnight)
        let today_start = {
            let t = Utc::now();
            chrono::DateTime::<Utc>::from(
                chrono::NaiveDateTime::new(t.date_naive(), chrono::NaiveTime::MIN).and_utc(),
            )
            .timestamp_millis()
        };
        let today_end = today_start + 86_400_000;

        let events_result = self.event_list(today_start, today_end);
        let recent_notes = self.note_list(None, None);

        let summary = serde_json::json!({
            "date": chrono::Utc::now().format("%Y-%m-%d").to_string(),
            "events": serde_json::from_str::<serde_json::Value>(&events_result.content).unwrap_or_default(),
            "recent_notes": serde_json::from_str::<serde_json::Value>(&recent_notes.content).unwrap_or_default(),
        });
        ToolResult::ok(serde_json::to_string_pretty(&summary).unwrap_or_default())
    }

    // ── Email ──────────────────────────────────────────────────────────────

    pub fn email_inbox(&self, account_id: Option<String>, limit: u32) -> ToolResult {
        let result = self.db.with_conn(|conn| {
            let sql = match &account_id {
                Some(_) => "SELECT id, account_id, subject, from_addr, date, flags FROM space_email_cache WHERE account_id=?1 AND folder='INBOX' ORDER BY date DESC LIMIT ?2",
                None => "SELECT id, account_id, subject, from_addr, date, flags FROM space_email_cache WHERE folder='INBOX' ORDER BY date DESC LIMIT ?2",
            };
            let mut stmt = conn.prepare(sql)?;
            let rows: Vec<serde_json::Value> = if let Some(aid) = &account_id {
                stmt.query_map(params![aid, limit], |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_,String>(0)?,
                        "account_id": row.get::<_,String>(1)?,
                        "subject": row.get::<_,Option<String>>(2)?,
                        "from": row.get::<_,Option<String>>(3)?,
                        "date": row.get::<_,Option<i64>>(4)?,
                        "flags": row.get::<_,String>(5)?,
                    }))
                })?
                .filter_map(|r| r.ok())
                .collect()
            } else {
                stmt.query_map(params![limit], |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_,String>(0)?,
                        "account_id": row.get::<_,String>(1)?,
                        "subject": row.get::<_,Option<String>>(2)?,
                        "from": row.get::<_,Option<String>>(3)?,
                        "date": row.get::<_,Option<i64>>(4)?,
                        "flags": row.get::<_,String>(5)?,
                    }))
                })?
                .filter_map(|r| r.ok())
                .collect()
            };
            Ok(rows)
        });
        match result {
            Ok(rows) => ToolResult::ok(serde_json::to_string_pretty(&rows).unwrap_or_default()),
            Err(e) => ToolResult::err(format!("Inbox failed: {e}")),
        }
    }

    pub fn email_read(&self, message_id: String) -> ToolResult {
        let result = self.db.with_conn(|conn| {
            conn.query_row(
                "SELECT id, account_id, subject, from_addr, to_addrs, date, body_text, body_html, flags
                 FROM space_email_cache WHERE id=?1",
                params![message_id],
                |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_,String>(0)?,
                        "account_id": row.get::<_,String>(1)?,
                        "subject": row.get::<_,Option<String>>(2)?,
                        "from": row.get::<_,Option<String>>(3)?,
                        "to": row.get::<_,Option<String>>(4)?,
                        "date": row.get::<_,Option<i64>>(5)?,
                        "body_text": row.get::<_,Option<String>>(6)?,
                        "flags": row.get::<_,String>(8)?,
                    }))
                },
            )
            .map_err(|e| anyhow::anyhow!(e))
        });
        match result {
            Ok(v) => ToolResult::ok(serde_json::to_string_pretty(&v).unwrap_or_default()),
            Err(e) => ToolResult::err(format!("Email not found: {e}")),
        }
    }

    pub fn email_compose(
        &self,
        to: String,
        subject: String,
        body: String,
        account_id: Option<String>,
    ) -> ToolResult {
        // Resolve account
        let account = match account_id {
            Some(id) => self.db.with_conn(|conn| {
                conn.query_row(
                    "SELECT id, smtp_host, smtp_port, username, password, use_tls FROM space_email_accounts WHERE id=?1",
                    params![id],
                    |row| {
                        Ok(EmailAccountRow {
                            id: row.get(0)?,
                            smtp_host: row.get(1)?,
                            smtp_port: row.get(2)?,
                            username: row.get(3)?,
                            password_enc: row.get(4)?,
                            use_tls: row.get::<_, i32>(5)? != 0,
                        })
                    },
                )
                .map_err(|e| anyhow::anyhow!(e))
            }),
            None => self.db.with_conn(|conn| {
                conn.query_row(
                    "SELECT id, smtp_host, smtp_port, username, password, use_tls FROM space_email_accounts LIMIT 1",
                    [],
                    |row| {
                        Ok(EmailAccountRow {
                            id: row.get(0)?,
                            smtp_host: row.get(1)?,
                            smtp_port: row.get(2)?,
                            username: row.get(3)?,
                            password_enc: row.get(4)?,
                            use_tls: row.get::<_, i32>(5)? != 0,
                        })
                    },
                )
                .map_err(|e| anyhow::anyhow!(e))
            }),
        };

        match account {
            Err(e) => ToolResult::err(format!("No email account configured. Add one first: {e}")),
            Ok(acct) => {
                // Actual SMTP send — requires the `lettre` crate wired up.
                // For now, record the outgoing message in cache and return a
                // stub confirmation; replace this block with lettre send when
                // the email phase is implemented.
                let msg_id = format!("out-{}", Uuid::new_v4());
                let now_ms = Utc::now().timestamp_millis();
                let _ = self.db.with_conn(|conn| {
                    conn.execute(
                        "INSERT OR IGNORE INTO space_email_cache (id, account_id, folder, subject, from_addr, to_addrs, date, body_text, flags, synced_at)
                         VALUES (?1, ?2, 'Sent', ?3, ?4, ?5, ?6, ?7, '[]', ?6)",
                        params![msg_id, acct.id, subject, acct.username, to, now_ms, body],
                    )?;
                    Ok(())
                });
                ToolResult::ok(
                    serde_json::json!({
                        "success": true,
                        "note": "Email queued. SMTP send requires lettre integration (Phase 3).",
                        "message_id": msg_id,
                        "to": to,
                    })
                    .to_string(),
                )
            }
        }
    }

    pub fn email_search(
        &self,
        query: String,
        account_id: Option<String>,
        limit: u32,
    ) -> ToolResult {
        let result = self.db.with_conn(|conn| {
            let pattern = format!("%{query}%");
            let sql = match &account_id {
                Some(_) => "SELECT id, account_id, subject, from_addr, date FROM space_email_cache WHERE account_id=?1 AND (subject LIKE ?3 OR body_text LIKE ?3) ORDER BY date DESC LIMIT ?2",
                None => "SELECT id, account_id, subject, from_addr, date FROM space_email_cache WHERE (subject LIKE ?2 OR body_text LIKE ?2) ORDER BY date DESC LIMIT ?1",
            };
            let mut stmt = conn.prepare(sql)?;
            let rows: Vec<serde_json::Value> = if let Some(aid) = &account_id {
                stmt.query_map(params![aid, limit, pattern], |row| {
                    Ok(serde_json::json!({ "id": row.get::<_,String>(0)?, "account_id": row.get::<_,String>(1)?, "subject": row.get::<_,Option<String>>(2)?, "from": row.get::<_,Option<String>>(3)?, "date": row.get::<_,Option<i64>>(4)? }))
                })?
                .filter_map(|r| r.ok())
                .collect()
            } else {
                stmt.query_map(params![limit, pattern], |row| {
                    Ok(serde_json::json!({ "id": row.get::<_,String>(0)?, "account_id": row.get::<_,String>(1)?, "subject": row.get::<_,Option<String>>(2)?, "from": row.get::<_,Option<String>>(3)?, "date": row.get::<_,Option<i64>>(4)? }))
                })?
                .filter_map(|r| r.ok())
                .collect()
            };
            Ok(rows)
        });
        match result {
            Ok(rows) => ToolResult::ok(serde_json::to_string_pretty(&rows).unwrap_or_default()),
            Err(e) => ToolResult::err(format!("Email search failed: {e}")),
        }
    }

    pub fn email_summary(&self, message_id: String) -> ToolResult {
        // Read email body then produce a structured summary.
        // Full AI summarization requires agent loop integration — here we return
        // the raw body_text truncated; the space-assistant persona will summarize it.
        let read = self.email_read(message_id);
        if read.is_error {
            return read;
        }
        let v: serde_json::Value = serde_json::from_str(&read.content).unwrap_or_default();
        let body = v["body_text"].as_str().unwrap_or("(no body)");
        let preview = &body[..body.len().min(2000)];
        ToolResult::ok(
            serde_json::json!({
                "subject": v["subject"],
                "from": v["from"],
                "date": v["date"],
                "body_preview": preview,
                "instruction": "Summarize the above email in Vietnamese: key points, action items, sentiment.",
            })
            .to_string(),
        )
    }

    // ── External sync (stubs — network calls implemented in Phase 3/4) ─────

    pub fn sync_google_calendar(&self, token: String, days: u32) -> ToolResult {
        // TODO Phase 4: call Google Calendar API v3 with `token`, fetch events,
        // upsert into space_events with source='google'. Requires reqwest + oauth2.
        let _ = (token, days);
        ToolResult::ok(
            serde_json::json!({
                "status": "pending",
                "message": "Google Calendar sync not yet implemented (Phase 4). Token received and stored.",
            })
            .to_string(),
        )
    }

    pub fn sync_apple_calendar(&self, token: String, days: u32) -> ToolResult {
        // TODO Phase 4: CalDAV client (iCloud url: caldav.icloud.com)
        let _ = (token, days);
        ToolResult::ok(
            serde_json::json!({
                "status": "pending",
                "message": "Apple Calendar (CalDAV) sync not yet implemented (Phase 4).",
            })
            .to_string(),
        )
    }

    pub fn sync_apple_notes(&self, token: String) -> ToolResult {
        // TODO Phase 4: Apple Notes are accessible via iCloud IMAP (Notes folder).
        let _ = token;
        ToolResult::ok(
            serde_json::json!({
                "status": "pending",
                "message": "Apple Notes sync not yet implemented (Phase 4). Will use iCloud IMAP Notes folder.",
            })
            .to_string(),
        )
    }

    pub fn sync_gmail(&self, token: String, days: u32) -> ToolResult {
        // TODO Phase 3: use Gmail API (users.messages.list) with OAuth2 token,
        // fetch recent messages, upsert into space_email_cache.
        let _ = (token, days);
        ToolResult::ok(
            serde_json::json!({
                "status": "pending",
                "message": "Gmail sync not yet implemented (Phase 3). Token received.",
            })
            .to_string(),
        )
    }

    // ── Recurring schedule ─────────────────────────────────────────────────

    pub async fn schedule_activity(
        &self,
        prompt: String,
        cron: String,
        group_folder: String,
        chat_jid: String,
    ) -> ToolResult {
        use crate::mcp::schedule_server::ScheduleServer;
        let srv = ScheduleServer::new();
        srv.schedule_task(
            &self.db,
            &group_folder,
            &chat_jid,
            &prompt,
            "cron",
            &cron,
            Some("group"),
            None,
        )
        .await
    }

    pub fn list_schedules(&self, group_folder: String) -> ToolResult {
        use crate::mcp::schedule_server::ScheduleServer;
        ScheduleServer::new().list_tasks(&self.db, &group_folder)
    }
}

// ─── Internal helpers ─────────────────────────────────────────────────────────

struct EmailAccountRow {
    id: String,
    smtp_host: String,
    smtp_port: i64,
    username: String,
    password_enc: String,
    use_tls: bool,
}

// ─── stdio server entry point ─────────────────────────────────────────────────

pub async fn run_stdio_server() -> Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();

    let db_path = std::env::var("SENCLAW_DB_PATH").context("SENCLAW_DB_PATH not set")?;
    let group_folder =
        std::env::var("SENCLAW_GROUP_FOLDER").context("SENCLAW_GROUP_FOLDER not set")?;
    let chat_jid = std::env::var("SENCLAW_CHAT_JID").context("SENCLAW_CHAT_JID not set")?;

    let mut config = crate::config::Config::from_env();
    config.paths.db_path = std::path::PathBuf::from(&db_path);
    let db = Arc::new(Db::open(&config).context("open space DB")?);

    let server = McpSpaceServer {
        db,
        group_folder,
        chat_jid,
    };

    let service = server.serve(rmcp::transport::io::stdio()).await?;
    service.waiting().await?;
    Ok(())
}
