---
name: docx-open
description: >-
  Open, list, and read Word documents (.docx) inside the SenClaw DOCX Editor app.
  Use when the user wants to inspect an existing document — "mở tài liệu X",
  "đọc file docx", "xem nội dung tài liệu Y", "list my docs", "read the report".
  Also use as the first step of any editing task, to fetch the current text
  before rewriting. Do NOT use for creating a new blank document (use docx-edit
  instead).
---

# docx-open

Read documents in the **SenClaw DOCX Editor** app via the `docx-editor-mcp`
MCP server.

## When to use this skill

- The user asks to **open / read / xem** a specific document, or one whose
  title they name loosely ("mở tài liệu chiến lược Q3").
- The user asks to **list** their documents ("danh sách file word",
  "list my docs").
- You're about to edit or rewrite a document — always read it first so you
  don't clobber content you can't see.

## Steps

1. **Find it.** If the user gave a title, call
   `mcp__docx-editor-mcp__docx_open` with `title="<their words>"` — this
   matches on the exact stored title. If nothing matches, fall back to
   `mcp__docx-editor-mcp__docx_list` and pick the closest by title.
2. **Read the body.** `docx_open` returns the full plain text in the
   `content` field. For very long documents, use
   `mcp__docx-editor-mcp__docx_read` with `offset`/`limit` (characters) to
   page through.
3. **Report.** Give the user a short summary or the exact excerpt they
   asked for. Mention the document is open in the DOCX Editor app so they
   can see it too.

## Notes

- Paragraphs are separated by `\n` (single newlines). Blank lines mean an
  empty paragraph.
- The `id` from `docx_open` / `docx_list` is stable — hand it off to the
  `docx-edit` skill if the user asks for changes next.
- Reply in the user's language (Vietnamese or English).
