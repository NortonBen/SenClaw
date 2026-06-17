---
name: google-workspace
description: Operate the Google Workspace Space App — sync Gmail/Calendar, read/send emails, create events, and upload files to Drive.
version: 2.0.0
when-to-use: "sync my Gmail", "import Google Calendar", "read my emails", "send an email", "upload a file to Drive", "create calendar event". Installed automatically with the Google Workspace Space App; removed when the app is uninstalled.
metadata:
  installed_by_app: google-workspace
---

# Google Workspace Skill

Bundled with the **Google Workspace** Space App. It documents the MCP tools the app exposes so the agent can interact with Gmail, Google Calendar, and Google Drive without the user opening the app UI.

> This skill is installed and removed with the app. It is read-only in the Skills panel — edit the app instead.

## Tools (MCP server `google-workspace-mcp`)

### Settings & Auth
| Tool | Purpose |
|------|---------|
| `gworkspace_get_settings` | Read saved settings and auth status. |
| `gworkspace_set_settings` | Set OAuth credentials (clientId, clientSecret). |

### Gmail
| Tool | Purpose |
|------|---------|
| `gworkspace_list_emails` | List recent emails from Gmail. |
| `gworkspace_read_email` | Read full content of a specific email by ID. |
| `gworkspace_send_email` | Send an email via Gmail. |

### Calendar
| Tool | Purpose |
|------|---------|
| `gworkspace_list_events` | List upcoming events from Google Calendar. |
| `gworkspace_create_event` | Create a new event in Google Calendar. |

### Drive
| Tool | Purpose |
|------|---------|
| `gworkspace_list_files` | List recently modified files from Google Drive. |
| `gworkspace_upload_file` | Upload a text file to Google Drive. |

## How to use
1. Check if the user needs to authenticate. If `gworkspace_list_emails` fails due to auth errors, ask the user to configure their Client ID and Secret in the app settings, and then authenticate via the App UI.
2. Use the respective tools to fulfill the user's request (e.g. read emails, add events).
