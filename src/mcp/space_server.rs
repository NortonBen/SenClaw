//! Space MCP server — personal productivity tools for the SenClaw Space feature.
//!
//! Tools cover: Notes (CRUD + FTS), Calendar (events + reminders),
//! external sync (Google Calendar/Apple Calendar/Apple Notes), and recurring
//! schedule helpers that wrap the TaskScheduler.
//!
//! Tool namespace: `space:*`

use anyhow::{Context, Result};
use chrono::{TimeZone, Utc};
use rmcp::ServiceExt;
use rusqlite::params;
use serde::Deserialize;
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
    /// Repeat reminder every N minutes while event is ongoing (None = no re-notification)
    #[serde(default)]
    renotify_min: Option<i64>,
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
struct EventUpdateParams {
    /// ID of the event to update
    event_id: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    description: Option<String>,
    /// Unix milliseconds
    #[serde(default)]
    start_at: Option<i64>,
    /// Unix milliseconds
    #[serde(default)]
    end_at: Option<i64>,
    #[serde(default)]
    location: Option<String>,
    #[serde(default)]
    all_day: Option<bool>,
    #[serde(default)]
    color: Option<String>,
    /// Minutes before event to send reminder
    #[serde(default)]
    reminder_min: Option<i64>,
    /// Repeat reminder every N minutes while event is ongoing
    #[serde(default)]
    renotify_min: Option<i64>,
    /// Force-reset the reminder so it fires again (e.g. after changing reminder_min)
    #[serde(default)]
    reset_reminder: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
struct EventSearchParams {
    /// Keyword to search in title, description, location (leave empty to search by date only)
    #[serde(default)]
    query: Option<String>,
    /// Natural-language or ISO date string for a specific day, e.g. "today", "tomorrow",
    /// "2026-05-10", "next Monday". If provided, only events on that day are returned.
    #[serde(default)]
    date: Option<String>,
    /// Unix ms — search window start (overrides `date` if both given)
    #[serde(default)]
    from: Option<i64>,
    /// Unix ms — search window end (overrides `date` if both given)
    #[serde(default)]
    to: Option<i64>,
    /// Max results (default 50)
    #[serde(default)]
    limit: Option<u32>,
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

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
struct RecurringCreateParams {
    /// Yêu cầu gửi cho agent vào mỗi lần lịch chạy.
    prompt: String,
    /// Tên hiển thị cho lịch và chat session đi kèm.
    label: Option<String>,
    /// Giờ chạy theo giờ máy ("HH:MM", 24h). Bắt buộc khi không dùng cron_advanced.
    time_local: Option<String>,
    /// "daily" | "weekdays" | "weekly" | "monthly". Mặc định "daily".
    frequency: Option<String>,
    /// 0=Chủ nhật .. 6=Thứ Bảy, dùng khi frequency = "weekly".
    weekday: Option<u32>,
    /// 1..28, dùng khi frequency = "monthly".
    day_of_month: Option<u32>,
    /// Cron 5 trường (phút giờ ngày tháng thứ). Ghi đè time_local/frequency.
    cron_advanced: Option<String>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
struct RecurringIdParams {
    /// ID của lịch định kỳ.
    id: String,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
struct RecurringUpdateParams {
    id: String,
    prompt: Option<String>,
    label: Option<String>,
    /// "active" | "paused" | "completed"
    status: Option<String>,
    time_local: Option<String>,
    frequency: Option<String>,
    weekday: Option<u32>,
    day_of_month: Option<u32>,
    cron_advanced: Option<String>,
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
                p.renotify_min,
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
        description = "Cập nhật sự kiện lịch. Update any field of an existing calendar event by id. \
                       Only provided fields are changed — omit fields you don't want to modify. \
                       start_at and end_at are Unix milliseconds."
    )]
    fn space_event_update(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            EventUpdateParams,
        >,
    ) -> String {
        self.inner()
            .event_update(
                p.event_id,
                p.title,
                p.description,
                p.start_at,
                p.end_at,
                p.location,
                p.all_day,
                p.color,
                p.reminder_min,
                p.renotify_min,
                p.reset_reminder.unwrap_or(false),
            )
            .content
    }

    #[rmcp::tool(description = "Tìm kiếm sự kiện theo từ khóa và/hoặc ngày. \
                       Search events by keyword (title/description/location) and/or date. \
                       `date` accepts natural language: 'today', 'tomorrow', 'yesterday', \
                       'hôm nay', 'ngày mai', or ISO format 'YYYY-MM-DD'. \
                       `query` filters by keyword within the matched date range. \
                       Examples: {date:'today'}, {query:'họp'}, {query:'react', date:'2026-05-10'}.")]
    fn space_event_search(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            EventSearchParams,
        >,
    ) -> String {
        self.inner()
            .event_search(p.query, p.date, p.from, p.to, p.limit.unwrap_or(50))
            .content
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
        description = "Lấy giờ hệ thống local hiện tại. Get the current local system time with full context: \
                       unix timestamp (ms), ISO datetime, Vietnamese formatted string, timezone offset, \
                       day-of-week, and pre-computed start/end ms for today, this week, and this month. \
                       ALWAYS call this first before any query that involves relative time \
                       (hôm nay, tuần này, ngày mai, lúc mấy giờ, etc.)."
    )]
    fn space_current_time(&self) -> String {
        use chrono::{Datelike, Duration, Local, Timelike};
        let now = Local::now();
        let today_start = now
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .and_then(|dt| Local.from_local_datetime(&dt).single())
            .map(|dt| dt.timestamp_millis())
            .unwrap_or(now.timestamp_millis());
        let today_end = now
            .date_naive()
            .and_hms_opt(23, 59, 59)
            .and_then(|dt| Local.from_local_datetime(&dt).single())
            .map(|dt| dt.timestamp_millis())
            .unwrap_or(now.timestamp_millis());

        // Start of week (Sunday = 0)
        let days_from_sun = now.weekday().num_days_from_sunday() as i64;
        let week_start_date = now.date_naive() - Duration::days(days_from_sun);
        let week_end_date = week_start_date + Duration::days(6);
        let week_start_ms = week_start_date
            .and_hms_opt(0, 0, 0)
            .and_then(|dt| Local.from_local_datetime(&dt).single())
            .map(|dt| dt.timestamp_millis())
            .unwrap_or(today_start);
        let week_end_ms = week_end_date
            .and_hms_opt(23, 59, 59)
            .and_then(|dt| Local.from_local_datetime(&dt).single())
            .map(|dt| dt.timestamp_millis())
            .unwrap_or(today_end);

        // Start/end of month
        let month_start = now.date_naive().with_day(1).unwrap_or(now.date_naive());
        let month_end = if now.month() == 12 {
            chrono::NaiveDate::from_ymd_opt(now.year() + 1, 1, 1)
        } else {
            chrono::NaiveDate::from_ymd_opt(now.year(), now.month() + 1, 1)
        }
        .map(|d| d.pred_opt().unwrap_or(d))
        .unwrap_or(now.date_naive());
        let month_start_ms = month_start
            .and_hms_opt(0, 0, 0)
            .and_then(|dt| Local.from_local_datetime(&dt).single())
            .map(|dt| dt.timestamp_millis())
            .unwrap_or(today_start);
        let month_end_ms = month_end
            .and_hms_opt(23, 59, 59)
            .and_then(|dt| Local.from_local_datetime(&dt).single())
            .map(|dt| dt.timestamp_millis())
            .unwrap_or(today_end);

        let day_names_vi = [
            "Chủ nhật",
            "Thứ hai",
            "Thứ ba",
            "Thứ tư",
            "Thứ năm",
            "Thứ sáu",
            "Thứ bảy",
        ];
        let dow_vi = day_names_vi[now.weekday().num_days_from_sunday() as usize];
        let tz_offset = now.offset().local_minus_utc() / 3600;
        let tz_sign = if tz_offset >= 0 { "+" } else { "" };

        let result = serde_json::json!({
            "now_ms": now.timestamp_millis(),
            "iso": now.format("%Y-%m-%dT%H:%M:%S").to_string(),
            "display": format!("{}, {:02}/{:02}/{} {:02}:{:02}",
                dow_vi, now.day(), now.month(), now.year(),
                now.hour(), now.minute()),
            "timezone": format!("UTC{tz_sign}{tz_offset}"),
            "year": now.year(),
            "month": now.month(),
            "day": now.day(),
            "hour": now.hour(),
            "minute": now.minute(),
            "day_of_week": now.weekday().num_days_from_sunday(),
            "day_of_week_vi": dow_vi,
            "today": {
                "start_ms": today_start,
                "end_ms": today_end,
                "iso_date": now.format("%Y-%m-%d").to_string(),
            },
            "this_week": {
                "start_ms": week_start_ms,
                "end_ms": week_end_ms,
                "start_date": week_start_date.format("%Y-%m-%d").to_string(),
                "end_date": week_end_date.format("%Y-%m-%d").to_string(),
            },
            "this_month": {
                "start_ms": month_start_ms,
                "end_ms": month_end_ms,
                "start_date": month_start.format("%Y-%m-%d").to_string(),
                "end_date": month_end.format("%Y-%m-%d").to_string(),
            },
            "tomorrow": {
                "start_ms": today_start + 86_400_000,
                "end_ms": today_end + 86_400_000,
                "iso_date": (now.date_naive() + Duration::days(1)).format("%Y-%m-%d").to_string(),
            },
            "yesterday": {
                "start_ms": today_start - 86_400_000,
                "end_ms": today_end - 86_400_000,
                "iso_date": (now.date_naive() - Duration::days(1)).format("%Y-%m-%d").to_string(),
            },
        });
        result.to_string()
    }

    #[rmcp::tool(
        description = "Tóm tắt hôm nay: sự kiện, nhắc nhở, ghi chú gần đây. Today summary: events, reminders, recent notes."
    )]
    fn space_today_summary(&self) -> String {
        self.inner().today_summary().content
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

    // ── Recurring schedules (new model: each schedule owns a chat session) ─

    #[rmcp::tool(
        description = "Tạo lịch định kỳ tự động cho agent. \
Mỗi lịch sẽ tự tạo một chat session riêng và mỗi lần đến giờ agent sẽ chạy prompt trong chat đó. \
Dùng `time_local` (HH:MM, giờ máy) + `frequency` (daily/weekdays/weekly/monthly), hoặc `cron_advanced` (5 trường). \
VD: prompt='Tìm giá vàng SJC hôm nay', time_local='07:00', frequency='daily'."
    )]
    async fn space_recurring_create(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            RecurringCreateParams,
        >,
    ) -> String {
        self.inner()
            .recurring_create(
                p.prompt,
                p.label,
                p.time_local,
                p.frequency,
                p.weekday,
                p.day_of_month,
                p.cron_advanced,
            )
            .await
            .content
    }

    #[rmcp::tool(
        description = "Liệt kê toàn bộ lịch định kỳ tự động (mỗi mục có id, label, prompt, chat_jid, schedule_value, status, next_run, last_run, last_status)."
    )]
    fn space_recurring_list(&self) -> String {
        self.inner().recurring_list().content
    }

    #[rmcp::tool(
        description = "Lấy chi tiết một lịch định kỳ kèm lịch sử 20 lần chạy gần nhất."
    )]
    fn space_recurring_get(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            RecurringIdParams,
        >,
    ) -> String {
        self.inner().recurring_get(&p.id).content
    }

    #[rmcp::tool(
        description = "Cập nhật lịch định kỳ. Có thể đổi prompt, label, lịch (time_local+frequency hoặc cron_advanced), và status (active/paused/completed)."
    )]
    fn space_recurring_update(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            RecurringUpdateParams,
        >,
    ) -> String {
        self.inner()
            .recurring_update(
                &p.id,
                p.prompt,
                p.label,
                p.status,
                p.time_local,
                p.frequency,
                p.weekday,
                p.day_of_month,
                p.cron_advanced,
            )
            .content
    }

    #[rmcp::tool(
        description = "Xoá lịch định kỳ và chat session đi kèm. Hành động này không thể hoàn tác."
    )]
    fn space_recurring_delete(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            RecurringIdParams,
        >,
    ) -> String {
        self.inner().recurring_delete(&p.id).content
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
        renotify_min: Option<i64>,
        color: Option<String>,
        group_folder: &str,
        chat_jid: &str,
    ) -> ToolResult {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().timestamp_millis();

        let result = self.db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO space_events (id, title, description, start_at, end_at, all_day, location, color, reminder_min, renotify_min, source, created_at, updated_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'manual',?11,?11)",
                params![id, title, description, start_at, end_at, all_day as i32, location, color, reminder_min, renotify_min, now],
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
                "SELECT id, title, description, start_at, end_at, all_day, location, color,
                        reminder_min, source, status, renotify_min
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
                        "status": row.get::<_,Option<String>>(10)?.unwrap_or_else(|| "upcoming".into()),
                        "renotify_min": row.get::<_,Option<i64>>(11)?,
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

    pub fn event_update(
        &self,
        event_id: String,
        title: Option<String>,
        description: Option<String>,
        start_at: Option<i64>,
        end_at: Option<i64>,
        location: Option<String>,
        all_day: Option<bool>,
        color: Option<String>,
        reminder_min: Option<i64>,
        renotify_min: Option<i64>,
        reset_reminder: bool,
    ) -> ToolResult {
        let result = self.db.with_conn(|conn| {
            let now_ms = chrono::Utc::now().timestamp_millis();
            if let Some(v) = &title {
                conn.execute("UPDATE space_events SET title=?1 WHERE id=?2 AND deleted_at IS NULL", params![v, event_id])?;
            }
            if description.is_some() {
                conn.execute("UPDATE space_events SET description=?1 WHERE id=?2 AND deleted_at IS NULL", params![description, event_id])?;
            }
            if let Some(v) = start_at {
                // Re-arm reminder + start notifications when the event moves.
                conn.execute(
                    "UPDATE space_events SET start_at=?1, reminder_sent_at=NULL, start_sent_at=NULL WHERE id=?2 AND deleted_at IS NULL",
                    params![v, event_id],
                )?;
            }
            if let Some(v) = end_at {
                conn.execute("UPDATE space_events SET end_at=?1 WHERE id=?2 AND deleted_at IS NULL", params![v, event_id])?;
            }
            if location.is_some() {
                conn.execute("UPDATE space_events SET location=?1 WHERE id=?2 AND deleted_at IS NULL", params![location, event_id])?;
            }
            if let Some(v) = all_day {
                conn.execute("UPDATE space_events SET all_day=?1 WHERE id=?2 AND deleted_at IS NULL", params![v as i32, event_id])?;
            }
            if color.is_some() {
                conn.execute("UPDATE space_events SET color=?1 WHERE id=?2 AND deleted_at IS NULL", params![color, event_id])?;
            }
            if let Some(v) = reminder_min {
                conn.execute("UPDATE space_events SET reminder_min=?1 WHERE id=?2 AND deleted_at IS NULL", params![v, event_id])?;
            }
            if let Some(v) = renotify_min {
                conn.execute("UPDATE space_events SET renotify_min=?1 WHERE id=?2 AND deleted_at IS NULL", params![v, event_id])?;
            }
            if reset_reminder {
                // Clear sent flags so EventNotifier fires the reminder again.
                conn.execute(
                    "UPDATE space_events SET reminder_sent_at=NULL, renotify_sent_at=NULL, start_sent_at=NULL WHERE id=?1 AND deleted_at IS NULL",
                    params![event_id],
                )?;
            }
            conn.execute(
                "UPDATE space_events SET updated_at=?1 WHERE id=?2 AND deleted_at IS NULL",
                params![now_ms, event_id],
            )?;
            Ok(())
        });
        match result {
            Ok(_) => {
                ToolResult::ok(serde_json::json!({ "success": true, "id": event_id }).to_string())
            }
            Err(e) => ToolResult::err(format!("Update event failed: {e}")),
        }
    }

    /// Search events by keyword and/or date.
    /// `date` accepts: "today", "tomorrow", "yesterday", ISO date "YYYY-MM-DD".
    /// Returns events sorted by start_at ascending.
    pub fn event_search(
        &self,
        query: Option<String>,
        date: Option<String>,
        from_ms: Option<i64>,
        to_ms: Option<i64>,
        limit: u32,
    ) -> ToolResult {
        // Resolve time window
        let (range_from, range_to) = if let (Some(f), Some(t)) = (from_ms, to_ms) {
            (f, t)
        } else if let Some(ref d) = date {
            match resolve_date(d) {
                Some((f, t)) => (f, t),
                None => {
                    return ToolResult::err(format!(
                        "Không nhận dạng được ngày: '{d}'. \
                         Dùng 'today', 'tomorrow', 'yesterday' hoặc định dạng YYYY-MM-DD."
                    ));
                }
            }
        } else {
            // Default: next 30 days
            let now = Utc::now().timestamp_millis();
            (now, now + 30 * 24 * 3600 * 1000)
        };

        let kw = query.as_deref().unwrap_or("").trim().to_lowercase();
        let result = self.db.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, title, description, start_at, end_at, all_day, location, color,
                        reminder_min, source, status, renotify_min
                 FROM space_events
                 WHERE deleted_at IS NULL
                   AND start_at >= ?1 AND start_at <= ?2
                 ORDER BY start_at ASC
                 LIMIT ?3",
            )?;
            let rows: Vec<serde_json::Value> = stmt
                .query_map(params![range_from, range_to, limit as i64], |row| {
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
                .filter(|ev| {
                    if kw.is_empty() {
                        return true;
                    }
                    let title = ev["title"].as_str().unwrap_or("").to_lowercase();
                    let desc = ev["description"].as_str().unwrap_or("").to_lowercase();
                    let loc = ev["location"].as_str().unwrap_or("").to_lowercase();
                    title.contains(&kw) || desc.contains(&kw) || loc.contains(&kw)
                })
                .collect();
            Ok(rows)
        });
        match result {
            Ok(rows) => ToolResult::ok(serde_json::to_string_pretty(&rows).unwrap_or_default()),
            Err(e) => ToolResult::err(format!("Search events failed: {e}")),
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
        let _now_ms = Utc::now().timestamp_millis();
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

    // ── Recurring schedule (legacy, group-bound) ──────────────────────────

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

    // ── Recurring schedule (redesigned: each schedule owns a chat session) ─
    //
    // The new model auto-creates a dedicated `groups` row per schedule (jid
    // `schedule:<id>`, folder `schedule_<id>`). Agent output streams into that
    // chat session. Used by the Space UI and the `space_recurring_*` MCP tools.

    pub async fn recurring_create(
        &self,
        prompt: String,
        label: Option<String>,
        time_local: Option<String>,
        frequency: Option<String>,
        weekday: Option<u32>,
        day_of_month: Option<u32>,
        cron_advanced: Option<String>,
    ) -> ToolResult {
        if prompt.trim().is_empty() {
            return ToolResult::err("prompt is required".into());
        }
        let cron = match build_schedule_cron(
            cron_advanced.as_deref(),
            time_local.as_deref(),
            frequency.as_deref(),
            weekday,
            day_of_month,
        ) {
            Ok(c) => c,
            Err(e) => return ToolResult::err(e),
        };
        let id = Uuid::new_v4().to_string();
        let chat_jid = format!("{SCHEDULE_JID_PREFIX}{id}");
        let group_folder = format!("{SCHEDULE_FOLDER_PREFIX}{id}");
        let label = label
            .as_deref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_owned())
            .unwrap_or_else(|| truncate_label(&prompt, 60));

        let now = Utc::now().to_rfc3339();
        if let Err(e) = self.db.upsert_group(&crate::types::GroupBinding {
            jid: chat_jid.clone(),
            folder: group_folder.clone(),
            name: label.clone(),
            channel: String::new(),
            group_type: "chat".into(),
            is_admin: true,
            requires_trigger: false,
            allowed_tools: None,
            allowed_paths: None,
            allowed_work_dirs: None,
            bot_token: None,
            max_messages: None,
            last_active: Some(now.clone()),
            added_at: now,
        }) {
            return ToolResult::err(format!("create chat session: {e}"));
        }

        let srv = crate::mcp::schedule_server::ScheduleServer::new();
        let result = srv
            .schedule_task(
                &self.db,
                &group_folder,
                &chat_jid,
                &prompt,
                "cron",
                &cron,
                Some("group"),
                None,
            )
            .await;
        if result.is_error {
            let _ = self.db.delete_group(&chat_jid);
            return result;
        }
        let info: serde_json::Value = serde_json::from_str(&result.content).unwrap_or_default();
        let task_id = info
            .get("taskId")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned();
        let tasks = match self.db.get_tasks_by_group(&group_folder) {
            Ok(t) => t,
            Err(e) => return ToolResult::err(format!("lookup task: {e}")),
        };
        let task = match tasks.into_iter().find(|t| t.id == task_id) {
            Some(t) => t,
            None => return ToolResult::err("task not found after insert".into()),
        };
        ToolResult::ok(self.serialize_schedule(&task).to_string())
    }

    pub fn recurring_list(&self) -> ToolResult {
        let tasks = match self.db.list_all_tasks() {
            Ok(t) => t,
            Err(e) => return ToolResult::err(format!("list tasks: {e}")),
        };
        let items: Vec<serde_json::Value> = tasks
            .iter()
            .filter(|t| t.group_folder.starts_with(SCHEDULE_FOLDER_PREFIX))
            .map(|t| self.serialize_schedule(t))
            .collect();
        ToolResult::ok(serde_json::Value::Array(items).to_string())
    }

    pub fn recurring_get(&self, id: &str) -> ToolResult {
        let tasks = match self.db.list_all_tasks() {
            Ok(t) => t,
            Err(e) => return ToolResult::err(format!("list tasks: {e}")),
        };
        let task = match tasks
            .into_iter()
            .find(|t| t.id == id && t.group_folder.starts_with(SCHEDULE_FOLDER_PREFIX))
        {
            Some(t) => t,
            None => return ToolResult::err(format!("schedule not found: {id}")),
        };
        let runs = self.db.get_task_run_logs(id, 20).unwrap_or_default();
        let mut item = self.serialize_schedule(&task);
        item["runs"] = serde_json::json!(
            runs.iter()
                .map(|l| serde_json::json!({
                    "id":          l.id,
                    "run_at":      l.run_at,
                    "duration_ms": l.duration_ms,
                    "status":      l.status.as_str(),
                    "result":      l.result,
                    "error":       l.error,
                }))
                .collect::<Vec<_>>()
        );
        ToolResult::ok(item.to_string())
    }

    pub fn recurring_update(
        &self,
        id: &str,
        prompt: Option<String>,
        label: Option<String>,
        status: Option<String>,
        time_local: Option<String>,
        frequency: Option<String>,
        weekday: Option<u32>,
        day_of_month: Option<u32>,
        cron_advanced: Option<String>,
    ) -> ToolResult {
        let tasks = match self.db.list_all_tasks() {
            Ok(t) => t,
            Err(e) => return ToolResult::err(format!("list tasks: {e}")),
        };
        let task = match tasks
            .into_iter()
            .find(|t| t.id == id && t.group_folder.starts_with(SCHEDULE_FOLDER_PREFIX))
        {
            Some(t) => t,
            None => return ToolResult::err(format!("schedule not found: {id}")),
        };

        let touches_schedule = cron_advanced.is_some()
            || time_local.is_some()
            || frequency.is_some()
            || weekday.is_some()
            || day_of_month.is_some();
        let new_cron = if touches_schedule {
            match build_schedule_cron(
                cron_advanced.as_deref(),
                time_local.as_deref(),
                frequency.as_deref(),
                weekday,
                day_of_month,
            ) {
                Ok(c) => Some(c),
                Err(e) => return ToolResult::err(e),
            }
        } else {
            None
        };

        if let Some(p) = prompt
            .as_deref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            if let Err(e) = self.db.with_conn(|c| {
                c.execute(
                    "UPDATE scheduled_tasks SET prompt = ?1 WHERE id = ?2",
                    rusqlite::params![p, id],
                )?;
                Ok(())
            }) {
                return ToolResult::err(format!("update prompt: {e}"));
            }
        }

        if let Some(cron) = &new_cron {
            let mut tmp = task.clone();
            tmp.schedule_value = cron.clone();
            tmp.next_run = None;
            let next = crate::scheduler::task_scheduler::compute_next_run(&tmp);
            if let Err(e) = self.db.with_conn(|c| {
                c.execute(
                    "UPDATE scheduled_tasks SET schedule_value=?1, next_run=?2 WHERE id=?3",
                    rusqlite::params![cron, next, id],
                )?;
                Ok(())
            }) {
                return ToolResult::err(format!("update cron: {e}"));
            }
        }

        if let Some(st) = status.as_deref() {
            let parsed = match st {
                "active" => Some(crate::types::TaskStatus::Active),
                "paused" => Some(crate::types::TaskStatus::Paused),
                "completed" => Some(crate::types::TaskStatus::Completed),
                other => return ToolResult::err(format!("unknown status: {other}")),
            };
            if let Some(st) = parsed {
                if let Err(e) = self.db.update_task_status(id, st) {
                    return ToolResult::err(format!("update status: {e}"));
                }
            }
        }

        if let Some(new_label) = label
            .as_deref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            if let Err(e) = self.db.with_conn(|c| {
                c.execute(
                    "UPDATE groups SET name = ?1 WHERE jid = ?2",
                    rusqlite::params![new_label, task.chat_jid],
                )?;
                Ok(())
            }) {
                return ToolResult::err(format!("update label: {e}"));
            }
        }

        // Re-fetch and return.
        let tasks = match self.db.list_all_tasks() {
            Ok(t) => t,
            Err(e) => return ToolResult::err(format!("list tasks: {e}")),
        };
        let task = match tasks.into_iter().find(|t| t.id == id) {
            Some(t) => t,
            None => return ToolResult::err("schedule disappeared after update".into()),
        };
        ToolResult::ok(self.serialize_schedule(&task).to_string())
    }

    pub fn recurring_delete(&self, id: &str) -> ToolResult {
        let tasks = match self.db.list_all_tasks() {
            Ok(t) => t,
            Err(e) => return ToolResult::err(format!("list tasks: {e}")),
        };
        let task = match tasks
            .into_iter()
            .find(|t| t.id == id && t.group_folder.starts_with(SCHEDULE_FOLDER_PREFIX))
        {
            Some(t) => t,
            None => return ToolResult::err(format!("schedule not found: {id}")),
        };
        if let Err(e) = self.db.delete_task(id) {
            return ToolResult::err(format!("delete task: {e}"));
        }
        let _ = self.db.delete_group(&task.chat_jid);
        let _ = self.db.delete_group_by_folder(&task.group_folder);
        ToolResult::ok(serde_json::json!({ "success": true, "id": id }).to_string())
    }

    fn serialize_schedule(&self, task: &crate::types::ScheduledTask) -> serde_json::Value {
        let label = self
            .db
            .get_group(&task.chat_jid)
            .ok()
            .flatten()
            .map(|g| g.name)
            .unwrap_or_else(|| truncate_label(&task.prompt, 40));
        let logs = self.db.get_task_run_logs(&task.id, 1).unwrap_or_default();
        let last_status = logs.first().map(|l| l.status.as_str().to_owned());
        serde_json::json!({
            "id":              task.id,
            "label":           label,
            "prompt":          task.prompt,
            "chat_jid":        task.chat_jid,
            "group_folder":    task.group_folder,
            "schedule_type":   task.schedule_type.as_str(),
            "schedule_value":  task.schedule_value,
            "status":          task.status.as_str(),
            "next_run":        task.next_run,
            "last_run":        task.last_run,
            "last_status":     last_status,
            "created_at":      task.created_at,
        })
    }
}

// ─── Recurring schedule helpers ──────────────────────────────────────────────

pub(crate) const SCHEDULE_FOLDER_PREFIX: &str = "schedule_";
pub(crate) const SCHEDULE_JID_PREFIX: &str = "schedule:";

pub(crate) fn build_schedule_cron(
    advanced: Option<&str>,
    time_local: Option<&str>,
    frequency: Option<&str>,
    weekday: Option<u32>,
    day_of_month: Option<u32>,
) -> std::result::Result<String, String> {
    if let Some(raw) = advanced.map(|s| s.trim()).filter(|s| !s.is_empty()) {
        if raw.split_whitespace().count() != 5 {
            return Err("cron_advanced must be a 5-field expression".into());
        }
        return Ok(raw.to_owned());
    }
    let time = time_local.unwrap_or("").trim();
    let (h, m) = parse_hhmm(time).ok_or_else(|| "time_local must be HH:MM (24h)".to_owned())?;
    let freq = frequency.unwrap_or("daily");
    Ok(match freq {
        "daily" => format!("{m} {h} * * *"),
        "weekdays" => format!("{m} {h} * * 1-5"),
        "weekly" => {
            let dow = weekday.unwrap_or(1).min(6);
            format!("{m} {h} * * {dow}")
        }
        "monthly" => {
            let dom = day_of_month.unwrap_or(1).clamp(1, 28);
            format!("{m} {h} {dom} * *")
        }
        other => return Err(format!("Unknown frequency: {other}")),
    })
}

fn parse_hhmm(s: &str) -> Option<(u32, u32)> {
    let (h, m) = s.split_once(':')?;
    let h: u32 = h.parse().ok()?;
    let m: u32 = m.parse().ok()?;
    if h > 23 || m > 59 {
        return None;
    }
    Some((h, m))
}

pub(crate) fn truncate_label(s: &str, max: usize) -> String {
    let trimmed = s.trim();
    if trimmed.chars().count() <= max {
        trimmed.to_owned()
    } else {
        let head: String = trimmed.chars().take(max - 1).collect();
        format!("{head}…")
    }
}

// ─── Date resolution helper ───────────────────────────────────────────────────

/// Parse a natural-language or ISO date string into a (start_ms, end_ms) day range.
/// Returns None when the string is not recognized.
fn resolve_date(s: &str) -> Option<(i64, i64)> {
    use chrono::{Datelike, Duration, Local, NaiveDate, TimeZone};

    let s = s.trim().to_lowercase();
    let today = Local::now().date_naive();

    let date: NaiveDate = match s.as_str() {
        "today" | "hôm nay" | "hom nay" => today,
        "tomorrow" | "ngày mai" | "ngay mai" => today + Duration::days(1),
        "yesterday" | "hôm qua" | "hom qua" => today - Duration::days(1),
        "next monday" | "thứ 2 tuần sau" => {
            let days = (7 - today.weekday().num_days_from_monday() as i64 + 7) % 7;
            today + Duration::days(if days == 0 { 7 } else { days })
        }
        "this week" | "tuần này" => today, // treat as "from today through end of week" below
        _ => {
            // Try ISO date YYYY-MM-DD or DD/MM/YYYY
            if let Ok(d) = NaiveDate::parse_from_str(&s, "%Y-%m-%d") {
                d
            } else if let Ok(d) = NaiveDate::parse_from_str(&s, "%d/%m/%Y") {
                d
            } else if let Ok(d) = NaiveDate::parse_from_str(&s, "%d-%m-%Y") {
                d
            } else {
                return None;
            }
        }
    };

    let start = Local
        .from_local_datetime(&date.and_hms_opt(0, 0, 0)?)
        .single()?
        .timestamp_millis();
    let end = Local
        .from_local_datetime(&date.and_hms_opt(23, 59, 59)?)
        .single()?
        .timestamp_millis();
    Some((start, end))
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
