---
name: google-workspace
description: Operate the Google Workspace Space App — read/send Gmail, list/create Calendar events, list/upload Drive files, and sync events into Space Calendar.
version: 3.0.0
when-to-use: "đọc email", "check mail", "gửi email", "send an email", "read my emails", "sự kiện sắp tới", "tạo sự kiện", "create calendar event", "google drive", "tải file lên drive", "sync google". Installed automatically with the Google Workspace Space App; removed when the app is uninstalled.
metadata:
  installed_by_app: google-workspace
---

# Google Workspace Skill

Bundled with the **Google Workspace** Space App (Rust, port 4310). It documents
the MCP tools the app exposes so the agent can interact with Gmail, Google
Calendar and Google Drive without the user opening the app UI.

> This skill is installed and removed with the app. It is read-only in the
> Skills panel — edit the app instead.

## Tools (MCP server `google-workspace-mcp`)

Full identifiers follow the Space-App pattern `mcp__google-workspace-mcp__<tool>`.

### Settings & Auth
| Tool | Purpose |
|------|---------|
| `gworkspace_get_settings` | Read saved settings (secrets masked), connection state, last sync run. |
| `gworkspace_set_settings` | Set OAuth client (clientId/clientSecret), sync window `days`, `services`, or connect directly with `accessToken`/`refreshToken`. |

### Gmail
| Tool | Purpose |
|------|---------|
| `gworkspace_list_emails` | List recent emails — `maxResults` (1-50), optional Gmail query `q` (`is:unread`, `from:x`, `newer_than:7d`…). |
| `gworkspace_read_email` | Read one email by `id`: headers, decoded text body, attachment list. |
| `gworkspace_send_email` | Send an email (`to`, `subject`, `body` — HTML or plain text; UTF-8 subjects handled). |

### Calendar
| Tool | Purpose |
|------|---------|
| `gworkspace_list_events` | Upcoming events on the primary calendar — `maxResults`, optional `days` horizon. |
| `gworkspace_create_event` | Create an event: `summary`, optional `description`, `startTime`, `endTime` (RFC3339, `YYYY-MM-DDTHH:MM` local, or `YYYY-MM-DD` all-day). |

### Drive
| Tool | Purpose |
|------|---------|
| `gworkspace_list_files` | List files, most recently modified first — `maxResults`, optional Drive query `q`. |
| `gworkspace_upload_file` | Upload a text file: `name`, optional `mimeType` (default text/plain), `textContent`. |

### Sync
| Tool | Purpose |
|------|---------|
| `gworkspace_sync` | Run a sync now: gmail/drive take a fresh snapshot; calendar pushes events into SenClaw Space Calendar. Optional `services`, `days` override saved settings. |

## How to use

1. **Check connection first**: call `gworkspace_get_settings`. If `connected: false`,
   ask the user to open the app UI (Space → Google Workspace) and either run the
   OAuth flow (needs Client ID/Secret in Settings) or paste an access token.
   A token can also be provided in chat and applied via
   `gworkspace_set_settings { accessToken }`.
2. Any tool answering with *"Chưa kết nối Google"* or a 401 message means the
   token is missing/expired — re-run step 1 rather than retrying blindly.
3. **Before sending email or creating events with agent-composed content,
   show the draft to the user and get confirmation.**
4. For inbox triage prefer `q` filters (`is:unread newer_than:2d`) over large
   `maxResults`.
