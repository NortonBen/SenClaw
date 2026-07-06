# SenClaw Code (IDE)

A VSCode-style code editor packaged as a **SenClaw Space App**. Open any local
folder, edit real files in a Monaco editor, and pair with SenClaw's LLM in an
integrated chat panel — pin code selections by line as context, ask questions
grounded in your open files, and apply AI-suggested edits straight back to disk.

```
apps/code-ide/
├── Cargo.toml              # crate `code-ide` (workspace member)
├── senclaw-manifest.json   # Space-App manifest (server runtime, MCP, skills, personas)
├── src/                    # Rust axum backend
│   ├── main.rs             # serve web/dist + /api, restore last workspace
│   ├── api.rs              # REST: open/tree/file/save/create/rename/delete/search/chat/events
│   ├── workspace.rs        # file ops with path-traversal safety, binary/size guards
│   ├── llm.rs              # chat via SenClaw bridge (pins + open file → prompt)
│   ├── mcp.rs              # code-ide-mcp: ide_open/list_dir/read/write/create/rename/delete/search
│   ├── pty.rs              # integrated terminal: WebSocket ↔ PTY shell (portable-pty)
│   ├── watch.rs            # notify → SSE /api/events (external file changes)
│   └── db.rs               # tiny SQLite: last root + recents
├── web/                    # React 19 + Vite + Monaco (plain CSS, VSCode dark theme)
│   └── src/
│       ├── App.tsx         # layout, resizers, tabs, pins, chat, apply, SSE, search
│       ├── components/     # Explorer, EditorPane (Monaco), ChatPanel
│       └── main.tsx        # bundles Monaco + language workers locally (fully offline)
├── skills/                 # code-edit, explain-selection
├── personas/               # pair-programmer
└── scripts/pack.sh         # build + stage release/ + code-ide-app.zip
```

## Features

- **File explorer** — lazy tree, respects `.gitignore` + hard-ignores (`.git`,
  `node_modules`, `target`, `dist`); git status badges (M/A/U).
- **Monaco editor** — the real VSCode editor, bundled offline: tabs, dirty
  indicators, minimap, syntax highlighting for 30+ languages. `Cmd/Ctrl+S` saves.
- **Line pinning** — select code, press `Cmd/Ctrl+L` to pin `{file, lines, code}`
  as chips above the chat input; pins are woven into the LLM prompt. Add the whole
  open file with the **＋ Chat** button or `Cmd/Ctrl+Shift+L`.
- **Integrated terminal** — a real login shell (PTY) rooted at the workspace,
  streamed over a WebSocket to xterm.js. Toggle with the **⌘ Terminal** button or
  ``Ctrl+` ``; drag its top edge to resize.
- **AI chat** — grounded in pinned selections + the open file, powered by
  SenClaw's active model via the Space-App `llm.request` bridge. Cited answers.
  Right-click a selection for **📌 pin / 💬 ask AI / ➕ add file** menu actions.
- **Model + run-mode picker** — a dropdown in the chat lists the daemon's
  configured LLMs (including local `local-mlx` models) and switches the active
  one; a **Chat / Plan / Agent / DAG** toggle shapes how the AI responds.
- **DeepWiki tab** (📖) — DeepWiki (tree-sitter code index, call graph,
  source-grounded wiki + Q&A) is **vendored in-process**: its Axum router is
  nested at `/api/deepwiki`, its UI served at `/deepwiki` and embedded as a tab.
  Opening a folder auto-indexes it. (The standalone `apps/deepwiki` is untouched.)
- **Apply edits** — assistant code blocks get an **Apply** button; a
  `// file: path` header on the first line targets a specific file (else the
  active tab). Writes go straight to disk.
- **Workspace search** — case-insensitive text search across the workspace;
  click a hit to jump to that line.
- **Live sync** — a filesystem watcher pushes external changes over SSE so the
  explorer/git state stay current.
- **MCP server** (`code-ide-mcp`) — lets the SenClaw agent drive the editor:
  open a folder, read/write/create/rename/delete files, and search.

## Develop

```bash
# backend (serves API on :4340)
cargo run -p code-ide

# frontend (Vite dev server, proxies /api → :4340)
cd apps/code-ide/web && npm install && npm run dev
```

The backend calls the running SenClaw daemon for LLM completions
(`SENCLAW_BASE_URL`, default `http://127.0.0.1:18788`). File editing works
without a daemon; only the AI chat needs one.

## Build & package

```bash
apps/code-ide/scripts/pack.sh          # web build + release binary + zip
apps/code-ide/scripts/pack.sh --skip-build   # just re-stage/zip
```

Produces `apps/code-ide/code-ide-app.zip`, installable in SenClaw.
