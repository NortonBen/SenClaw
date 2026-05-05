---
name: space
description: Personal productivity tools — manage notes, calendar events, email, and recurring schedules via the Space MCP server
version: 1.0.0
mcp_servers:
  - senclaw-space
  - senclaw-schedule
---

# Space — Personal Productivity Tools

The Space feature gives you a personal productivity layer: **notes**, **calendar events**, **email**, and **recurring schedules**. All tools are prefixed `space_*` and available through the `senclaw-space` MCP server.

---

## Notes

### Create a note
```
space_note_create(title, body, tags?, folder_id?)
```
- `tags`: array of strings, e.g. `["todo", "meeting"]`
- Use tag `"todo"` for tasks, `"idea"` for ideas, `"meeting"` for meeting notes

### Update a note
```
space_note_update(id, title?, body?, tags?)
```

### Search notes (full-text)
```
space_note_search(query, limit?)
```
Returns notes ranked by FTS5 relevance.

### List notes
```
space_note_list(folder_id?, tag?)
```
Filter by folder or tag; omit both to list all recent notes.

### Delete a note
```
space_note_delete(id)
```

---

## Current Time (call first!)

```
space_current_time()
```

Returns current local system time with pre-computed ranges. **Always call this before any time-relative query.**

Response includes:
- `now_ms` — current Unix timestamp (ms)
- `display` — formatted Vietnamese string, e.g. "Thứ ba, 05/05/2026 14:30"
- `timezone` — e.g. "UTC+7"
- `today.start_ms` / `today.end_ms` — start/end of today
- `this_week.start_ms` / `this_week.end_ms` — this week (Sun–Sat)
- `this_month.start_ms` / `this_month.end_ms` — this month
- `tomorrow.start_ms`, `yesterday.start_ms`

```
// Example: list all events today
const t = await space_current_time();
space_event_list({ from: t.today.start_ms, to: t.today.end_ms })

// Example: event at 3pm today
space_event_create({ start_at: t.today.start_ms + 15*3600*1000, ... })
```

## Calendar

### Create an event
```
space_event_create(title, start_at, end_at, description?, location?, all_day?, reminder_min?, color?)
```
- `start_at` / `end_at`: Unix milliseconds
- `reminder_min`: minutes before event to trigger a reminder notification (e.g. `15`)
- `color`: hex color code for the event chip

### List events in a range
```
space_event_list(from, to)
```
Both in Unix ms. Typically use start-of-day / end-of-day for the desired window.

### Delete an event
```
space_event_delete(event_id)
```

### Set or update a reminder
```
space_set_reminder(event_id, reminder_min)
```

### Today's summary
```
space_today_summary()
```
Returns today's events and recent notes as a compact briefing. Use this to answer "hôm nay có gì" / "what's on my schedule today".

---

## Email

### Inbox
```
space_email_inbox(account_id?, limit?)
```
Returns a list of recent messages with subject, sender, and flags.

### Read a message
```
space_email_read(message_id)
```
Returns full message with body text.

### Compose and send
```
space_email_compose(to, subject, body, account_id?)
```
**Always draft and show to user before calling this tool.**
Workflow:
1. Draft the email body yourself
2. Show it to the user: *"Đây là bản nháp email:"*
3. Wait for confirmation ("gửi đi" / "send it")
4. Only then call `space_email_compose`

### Search email
```
space_email_search(query, account_id?, limit?)
```

### Email summary
```
space_email_summary(message_id)
```
Returns a structured summary (subject, sender, key points, action items).

---

## Recurring Schedules

### Schedule a recurring activity
```
space_schedule_activity(prompt, cron, group_folder, chat_jid)
```
- `prompt`: what the agent should do when triggered (e.g. "Fetch gold price and report to user")
- `cron`: standard cron expression

**Common cron patterns:**
| Intent | Cron |
|--------|------|
| Every day at 7am | `0 7 * * *` |
| Every day at 9pm | `0 21 * * *` |
| Every Monday at 9am | `0 9 * * 1` |
| Every Friday at 5pm | `0 17 * * 5` |
| Every weekday at 8am | `0 8 * * 1-5` |
| Every hour | `0 * * * *` |

### List schedules
```
space_list_schedules(group_folder)
```

---

## External Sync (Phase 3+)

```
space_sync_google_calendar(token, days?)
space_sync_apple_calendar(token, days?)
space_sync_apple_notes(token)
space_sync_gmail(token, days?)
```
All sync tools require an OAuth2 token. They are stubs — return instructions for the user when tokens are unavailable.

---

## Time Parsing Reference

When user gives natural language times, convert to Unix ms:
- "lúc 7h sáng" → today at 07:00 local time
- "lúc 9 giờ tối" → today at 21:00 local time
- "ngày mai lúc 2pm" → tomorrow at 14:00
- "sau 30 phút" → now + 30 * 60 * 1000 ms
- "thứ 2 tuần sau" → next Monday at 09:00 (default start time)

Use `Date.now()` context for "now". Default event duration is 1 hour when end time is not specified.

---

## When to Use Space Tools

| User says | Tool |
|-----------|------|
| "nhắc tôi họp lúc 3pm" / "remind me at 3pm" | `space_event_create` with `reminder_min: 15` |
| "hôm nay có gì" / "today's schedule" | `space_today_summary` |
| "ghi chú lại" / "note this" / "#note" / "📝" | `space_note_create` |
| "việc cần làm" / "todo list" | `space_note_list(tag="todo")` |
| "kiểm tra mail" / "check email" | `space_email_inbox` |
| "viết email cho X" / "soạn email" | Draft → show → confirm → `space_email_compose` |
| "định kỳ mỗi ngày lúc 7h" / "every day at 7am" | `space_schedule_activity` with cron |
| "đồng bộ Google Calendar" | `space_sync_google_calendar` |
