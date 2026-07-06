# SenClaw Mindmap 🧠

A visual mind-mapping **Space App** for SenClaw. Create maps, edit branches on an
interactive two-sided canvas, and let the configured LLM flesh out any topic — either
from the UI (the ✨ AI button and the chat panel) or programmatically from an agent via
the `mindmap-mcp` MCP server.

## Features

- **Four layout styles** (switch live from the topbar) — `mindmap` (two-sided radial),
  `org` (top-down org chart), `outline` (indented list with elbow connectors), and
  `right` (horizontal tree). Inspired by MindMeister's mind-map / org-chart / list modes.
- **Interactive canvas** — a hand-rolled, dependency-free SVG renderer with pan/zoom,
  inline rename, collapse/expand, and colored branches.
- **Node customization** — a per-node style panel (🎨) with a 12-color theme palette,
  **fill** vs. outline, five **shapes** (line / rectangle / rounded / pill / ellipse),
  and an **emoji/icon** picker.
- **Template gallery** — one-click starter maps across categories: SWOT, empathy map,
  project plan, exam revision, macroeconomics (org chart), weekly team sync (list),
  campaign brainstorm.
- **AI generation** — the **✨ AI** button on any node asks the LLM for a structured
  hierarchy of sub-topics and inserts it directly into the map. "New map" can seed a
  whole tree from just a title.
- **AI chat with history & sessions** — an integrated assistant panel, grounded in the
  current map's outline. Chats are **persisted per map** with **multiple named sessions**
  (switcher + new/rename/delete). Turn any AI answer into nodes with **🧠 Tạo sơ đồ**.
- **File attach → OCR → mind map** — the **📎** button imports a file; images are run
  through SenClaw's OCR (`/api/ocr/recognize`), text files are read directly, then the
  content is structured by the LLM into a brand-new map.
- **Theme follows SenClaw** — light/dark syncs with the SenClaw desktop/host via
  `senclaw:init` / `senclaw:theme` postMessages (with a manual toggle fallback).
- **Free-drag positioning** — 🔒 unlocks drag mode: move any node (its subtree follows)
  to a custom position that is **saved**; ↻ auto-sorts back to the algorithmic layout.
- **Import / Export** — read & write **JSON**, **Markdown**, **OPML**, and
  **FreeMind/Freeplane (.mm)** — the standard mind-map interchange formats.
- **Display settings** — ⚙️ sets the default layout for new maps, **full labels**
  (no truncation), and a **child-count** badge on each node.
- **Responsive** — a collapsible sidebar drawer + overlay chat make it usable on phones
  and small screens; custom-styled scrollbars throughout.
- **Undo / Redo** — ↶ / ↷ (Ctrl/⌘+Z, Ctrl/⌘+Shift+Z) restore and replay every change —
  edits, add/delete, AI generation, styling, drag, and layout — via snapshot history.
- **MCP server** (`mindmap-mcp`) — maps, nodes, layout, templates, styling, and
  `mindmap_generate` (see the table below).
- **Skills** — `mindmap-generate` (start a map / use a template) and `mindmap-expand`
  (deepen, restyle, or re-layout an existing map), with rich triggers.
- **Persona** — `mindmap-architect`, an AI that structures ideas into balanced maps.

Everything routes through the SenClaw daemon's Space-App bridge (`app-space-sdk`); the
app never talks to an LLM provider directly.

## Architecture

- **Backend** (`src/`, Rust + axum, port `4350`)
  - `db.rs` — SQLite store: `maps` + a normalized `nodes` adjacency-list tree; tree
    assembly, subtree delete/move, bulk insert for AI generation.
  - `llm.rs` — chat + structured-tree generation via `app-space-sdk`; tolerant JSON
    parsing of model output.
  - `api.rs` — REST endpoints (`/api/maps`, `/api/node/*`, `/api/generate`, `/api/chat`,
    `/api/models`, `/api/llm-info`).
  - `mcp.rs` — JSON-RPC-over-SSE MCP server (`/api/mcp/sse`), auto-registered from the
    manifest.
- **Frontend** (`web/`, React 19 + Vite + TypeScript)
  - `lib.ts` — the two-sided tidy-tree layout algorithm.
  - `components/MindmapCanvas.tsx` — the SVG canvas, node boxes, toolbar, pan/zoom.
  - `components/ChatPanel.tsx` — the AI chat.
  - `App.tsx` — sidebar, topbar (theme, model picker), state, keyboard shortcuts.

## Develop

```bash
# backend (from repo root)
cargo run -p mindmap                 # serves http://127.0.0.1:4350

# frontend (separate terminal)
cd apps/mindmap/web && npm install && npm run dev   # Vite proxies /api → :4350
```

Keyboard on the canvas: **Tab** add child · **Enter** add sibling · **F2** / double-click
rename · **Del** delete · **Esc** deselect · scroll to zoom · drag to pan.

## Package for install

```bash
apps/mindmap/scripts/pack.sh         # → apps/mindmap/mindmap-app.zip
```

Install the zip in SenClaw (Space Apps → install). The daemon launches the binary with an
assigned `PORT`, serves the UI in an iframe, and auto-registers `mindmap-mcp`.

## MCP tools

| Tool | Purpose |
|---|---|
| `mindmap_list` | List all maps |
| `mindmap_create` | Create a map (optional `layout`); returns `id` + `rootId` |
| `mindmap_templates` | List built-in starter templates |
| `mindmap_from_template` | Create a map pre-filled from a template |
| `mindmap_set_layout` | Change a map's layout (`mindmap`/`org`/`outline`/`right`) |
| `mindmap_get` | Get a map's full node tree |
| `mindmap_delete` | Delete a map |
| `mindmap_add_node` | Add a child under a node |
| `mindmap_update_node` | Edit a node's text/note/color/shape/fill/icon |
| `mindmap_delete_node` | Delete a node + subtree |
| `mindmap_generate` | AI-generate a hierarchy of sub-topics under a node |
