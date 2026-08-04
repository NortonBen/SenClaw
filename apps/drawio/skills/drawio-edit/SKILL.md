---
name: drawio-edit
description: >-
  Modify an EXISTING diagram in the SenClaw Diagrams (draw.io) app with AI —
  add/remove/rename steps or components, change flow direction, restyle. Use
  for requests like "sửa sơ đồ", "thêm bước X vào sơ đồ", "đổi hướng flow sang
  ngang", "update the diagram", "add a retry branch to the flowchart". For
  creating a new diagram from scratch use drawio-generate instead.
---

# drawio-edit

## When to use this skill

The user refers to a diagram that already exists in the Diagrams app and wants
it changed: new steps or components, removed parts, renamed labels, layout or
style adjustments.

## Steps

1. Find the target diagram: call `mcp__drawio-mcp__drawio_list` and match by
   name/recency against what the user said (e.g. "sơ đồ đăng ký" → the diagram
   whose name mentions đăng ký; "sơ đồ vừa tạo" → the most recently updated).
   If several plausibly match, ask which one.
2. (Optional) Inspect it first with `mcp__drawio-mcp__drawio_get` when the
   instruction depends on current content (e.g. "xoá bước trùng lặp").
3. Call `mcp__drawio-mcp__drawio_edit_ai` with `id` and `instruction` — a
   precise, self-contained instruction in the user's language describing the
   change (name the exact steps/components involved).
4. The updated diagram is saved and pushed live to any open editor. Reply with
   a markdown link built from the returned **`path`** — e.g.
   `[Mở sơ đồ](/space/app/drawio?d=5)` — which opens it inside the SenClaw
   screen, plus a one-line summary of the change. Optionally show it inline:
   `mcp__drawio-mcp__drawio_export` `format: "svg"` → if not stale, call
   `emit_widget` with `kind: "image"`, `data: { "url": <svg_path> }`.
5. For surgical, deterministic changes (e.g. fixing one label), you may instead
   read `mcp__drawio-mcp__drawio_get_xml`, transform the XML yourself, and
   write it back with `mcp__drawio-mcp__drawio_set_xml` (it validates ids,
   vertex/edge structure before saving).

## Notes

- `drawio_edit_ai` refuses diagrams over ~60k characters of XML — tell the
  user very large diagrams must be edited manually or split.
- Renames of the diagram itself use `mcp__drawio-mcp__drawio_rename`, not the
  AI editor.
- Reply in the user's language (Vietnamese or English).
