---
name: docx-edit
description: >-
  Create, write, edit, and save Word documents (.docx) in the SenClaw DOCX
  Editor app. Use when the user asks to draft, write, rewrite, extend,
  fix-and-replace, or save a Word document — "soạn tài liệu về X",
  "viết cho tôi báo cáo", "sửa đoạn 2", "thay thế 'foo' bằng 'bar'",
  "lưu tài liệu", "tải tài liệu về máy", "write a proposal", "draft a docx",
  "find and replace in the doc", "save/export the docx". If the user wants
  to change an existing document, start by opening it with **docx-open** so
  you know what's already there.
---

# docx-edit

Edit and save documents in the **SenClaw DOCX Editor** app via the
`docx-editor-mcp` MCP server. The user watches your changes appear in the
editor as you make them, and can hit "Tải .docx" at any time to download a
standards-compliant Word file.

## When to use this skill

- User asks you to **draft / write / soạn** a new Word document.
- User asks you to **rewrite / restructure / chỉnh sửa** an existing doc.
- User asks for a **find-and-replace / edit paragraph N / append a
  section**.
- User asks to **save / export / tải về** the current document.

For pure reading, use **docx-open** instead.

## Steps

### Creating a new doc

1. Call `mcp__docx-editor-mcp__docx_create` with a descriptive
   `title` (e.g. "Q3 Strategy Memo") and optionally an initial `content`
   block. It returns the new `id`.
2. If you need multi-pass drafting, use `docx_write` (whole-body replace)
   for the first pass and `docx_append` for follow-on sections.

### Editing an existing doc

1. **Read first.** Call `mcp__docx-editor-mcp__docx_open` (by `id` or
   `title`) to fetch the current text.
2. **Choose the smallest edit that works:**
   - **Whole-body rewrite** → `docx_write` with new `content`.
   - **Add a section at the end** → `docx_append` with `text`.
   - **Targeted change** → `docx_replace` with `find` / `replace`
     (`replace_all` defaults to true). Case-sensitive.
3. **Verify.** Call `docx_open` again to confirm the change landed, then
   report a short before/after or the paragraph you changed.

### Saving / exporting

- Every `docx_create`, `docx_write`, `docx_append`, and `docx_replace` call
  auto-regenerates and stores the .docx blob. There is no manual "save"
  step to make.
- To hand the user a downloadable .docx, call
  `mcp__docx-editor-mcp__docx_export_url` — it returns a URL they can
  click to download the file directly.

## Tool cheat sheet

| Tool | Use for |
| --- | --- |
| `docx_list` | Enumerate documents (id, title, excerpt). |
| `docx_create` | New blank / seeded document. |
| `docx_open` | Full read (returns `content`). |
| `docx_read` | Paged read (`offset`/`limit` in characters). |
| `docx_write` | Overwrite the whole body. |
| `docx_append` | Add text as a new paragraph at the end. |
| `docx_replace` | Find & replace substring. |
| `docx_rename` | Change the title / filename base. |
| `docx_delete` | Delete permanently. |
| `docx_export_url` | Get a URL to download the .docx. |

## Notes

- Paragraphs are separated by `\n`. Use `\n\n` for a blank line between
  paragraphs.
- The editor also autosaves on the user's side — if you race with a
  user's live edits, `docx_open` will show whichever version landed last.
- Reply in the user's language (Vietnamese or English).
