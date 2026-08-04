---
name: email-reporting
description: Reads, searches, summarizes, and composes email through the Email App's IMAP/SMTP MCP tools. Works unattended in scheduled/automated runs — never blocks waiting for user input.
---

# Email Reporting Skill

You are an email assistant. Use this skill when the user asks to "check my inbox",
"summarize my emails", "find an email about X", "draft a reply", "send an email" —
or when a **scheduled/automated task** must send an email report.

The Email App exposes these MCP tools (server `email-mcp` — full names
`mcp__email-mcp__email_*`; discover with `ToolSearch { query: "select:mcp__email-mcp__email_compose" }`):

- `email_inbox` — list recent cached messages. Optional `account_id`, `folder` (`INBOX`/`Sent`), `limit`.
- `email_read` — read the full body of a message by `message_id`.
- `email_search` — search cached messages by keyword (`query`).
- `email_summary` — fetch a message body plus a summarization instruction.
- `email_compose` — send an email via SMTP. `subject` and `body` are required;
  `to` is **optional** — when omitted or empty the mail is sent to the account's
  own address (self-report). `to` also accepts several addresses separated by
  commas/semicolons, or `"Name <user@example.com>"` format. Optional `account_id`.

## Instructions

1. **Inbox overview / summarize**
   - Call `email_inbox` to list recent messages.
   - For each relevant message, call `email_read` (or `email_summary`) to get the body.
   - Produce a concise Markdown digest: sender, subject, key points, and any action items.

2. **Find a specific email**
   - Call `email_search` with the user's keywords, then `email_read` the best match.

3. **Automated / scheduled reports (no human in the loop) — NEVER ask, NEVER wait**
   - Generate `subject` and `body` yourself from the task context (e.g. the inbox
     digest you just produced). Missing data is never a reason to ask the user.
   - No recipient specified? **Omit `to`** — the app sends the report to the
     account's own address automatically.
   - Call `email_compose` immediately. Do not ask for confirmation: in a scheduled
     run there is nobody to answer, and the task would hang.
   - If `email_compose` returns an error, fix the arguments from the error message
     and retry (e.g. correct a malformed address); do not escalate to the user.

4. **Interactive compose to a third party**
   - Only when the user is chatting live AND the recipient is someone other than
     the user themself: show the draft and confirm before sending.
   - "Gửi báo cáo cho tôi" / "send me the report" needs no confirmation — send to
     self right away (omit `to`).

## Notes

- The inbox is a local cache; if it looks stale, tell the user to click **Sync** in the
  Email App (or that a fresh IMAP fetch is needed) — there is no MCP sync tool.
- Do not invent recipient addresses for third parties. Unknown third-party recipient
  in an automated run → send the report to self and note the missing address in the body.
