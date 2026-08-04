---
name: drawio-generate
description: >-
  Draw a NEW diagram (flowchart, sequence, system architecture, ER, state,
  class, org chart, network, BPMN) from a plain-language description using the
  SenClaw Diagrams (draw.io) app. Use when the user asks to draw or create a
  diagram — e.g. "vẽ sơ đồ / lưu đồ / flowchart về X", "vẽ sơ đồ kiến trúc hệ
  thống Y", "draw a diagram of X", "create a flowchart for Y". Do NOT use this
  to change a diagram that already exists — use drawio-edit instead.
---

# drawio-generate

## When to use this skill

The user wants a brand-new visual diagram from a description: a process
(flowchart/BPMN), an interaction (sequence), a system (architecture/network),
a data model (ER/class), states, or an org chart. The result is a real,
editable draw.io diagram saved in the SenClaw Diagrams app — not ASCII art and
not an image.

## Steps

1. Pick the diagram family from the request: `flowchart` (default), `sequence`,
   `architecture`, `er`, `state`, `class`, `org`, `network`, `bpmn`.
2. Call `mcp__drawio-mcp__drawio_generate` with:
   - `prompt` — the user's description, enriched with any concrete steps,
     actors, components or relationships they mentioned (keep their language);
   - `kind` — the family from step 1;
   - `name` — a short title for the diagram (optional; defaults to the prompt).
   Keep the scope under ~40 shapes: if the request is bigger, generate the core
   first and offer to add detail with drawio-edit afterwards.
3. The result contains `id`, `path`, `url` and `cells`. Reply with a markdown
   link built from **`path`** — e.g. `[Mở sơ đồ](/space/app/drawio?d=5)` — it
   opens the diagram fully editable inside the SenClaw screen (Space → Diagrams
   frame). Use `url` (absolute) only for contexts outside the SenClaw UI. Add a
   one-line summary of what was drawn.
4. To also show the picture INLINE in the chat, call
   `mcp__drawio-mcp__drawio_export` with `format: "svg"` and, if it succeeds,
   call the `emit_widget` tool with `kind: "image"`,
   `data: { "url": <svg_path from the result> }`, `title`: the diagram name.
   If export answers that no snapshot exists yet or `stale: true`, skip the
   widget — the editor renders the snapshot the first time the user opens the
   diagram (only the editor can render SVG); the `path` link still works.

## Notes

- Overwrite an existing diagram by passing `diagram_id` — only when the user
  explicitly asks to redo it.
- If generation fails with a "truncated" error, retry with a simpler prompt
  (fewer steps/components) or tell the user the diagram needs splitting.
- Reply in the user's language (Vietnamese or English).
