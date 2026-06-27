# DeepWiki App Space

**AI code intelligence for any local codebase** — a unified app combining a tree-sitter
**symbol/call graph** (à la [colbymchenry/codegraph](https://github.com/colbymchenry/codegraph))
with an **AI-generated, source-grounded wiki** (à la [Cognition/Devin's DeepWiki](https://deepwiki.com)).
One index powers both: the graph gives agents surgical structural context, the wiki gives humans
browsable docs + conversational Q&A.

> CodeGraph and DeepWiki were merged into this single self-contained app. The former
> `codegraph` app and the shared `codeindex-core` crate were both folded in and removed —
> the code-intelligence core now lives in this crate (`src/{db,index,lang,model,parse,query}.rs`).

- **Parsing/index:** tree-sitter — **17 languages**: Rust, Python, JavaScript, TypeScript/TSX, Go,
  Java, C, C++, C#, Ruby, PHP, Scala, Bash, Julia, Haskell, OCaml →
  SQLite + FTS5 graph of symbols/calls/imports. Wiki pages live in the same DB at
  `~/.senclaw/space-apps-data/deepwiki/index.db`.
- **Live:** a file watcher incrementally re-indexes on change (debounced).
- **Grounding:** every wiki page / answer is built from retrieved evidence with `path:line`
  citations — no hallucinated APIs.

## MCP tools (`deepwiki-mcp`)

**Index**

| Tool | Purpose |
|---|---|
| `deepwiki_index` | Index/re-index a repo by absolute path |
| `deepwiki_status` | Indexed root + file/symbol/edge counts + page count |

**Code graph**

| Tool | Purpose |
|---|---|
| `deepwiki_explore` | **Preferred.** Symbol matches + callers + callees + blast radius in one shot |
| `deepwiki_search` | Full-text search over names/signatures/docs |
| `deepwiki_symbol` | Exact-name lookup: definitions + direct callers + callees |
| `deepwiki_impact` | Transitive callers (blast radius) of a symbol |
| `deepwiki_file_outline` | All symbols in a file + its imports |
| `deepwiki_list_files` | Indexed file inventory |
| `deepwiki_snippet` | Read the real source of a symbol (by name) or a file line range |

**Wiki**

| Tool | Purpose |
|---|---|
| `deepwiki_outline` | Repo structural map (dirs, largest files, architectural types, hot symbols) — to PLAN |
| `deepwiki_context` | Source-grounded evidence for a topic/question — to WRITE / ANSWER |
| `deepwiki_save_page` | Create/update a Markdown page (`slug`, `title`, `content`, `parent`, `ord`) |
| `deepwiki_list_pages` / `deepwiki_get_page` / `deepwiki_delete_page` | Manage pages |

## Skills

- `deepwiki-generate` — plan from the outline, write each page from grounded context, save.
- `deepwiki-ask` — answer codebase questions, grounded in real `path:line` sources.
- `deepwiki-explore` — understand structure via the call graph (who calls what, outlines).
- `deepwiki-review` — impact/risk review of a change (callers, blast radius, tests at risk).

## Sub-agents (personas)

Installed into the SenClaw virtual-agents dir on app install; dispatchable via `run_persona` /
the dispatch DAG:

- `wiki-author` — plans and writes the wiki from grounded evidence.
- `codebase-guide` — answers questions about the codebase, grounded in source.
- `code-explorer` — navigates and explains a codebase via the call graph.
- `impact-analyst` — assesses the blast radius and risk of a change end-to-end.

## REST API (used by the Web UI)

`GET /api/status` · `GET /api/recents` (previously-indexed roots) · `POST /api/index {path}` · `GET /api/outline` ·
`GET /api/context?q=&depth=` (alias `/api/ask`) · `GET /api/pages` ·
`GET/POST/DELETE /api/page?slug=` · `GET /api/search?q=` · `GET /api/symbol?name=` ·
`GET /api/explore?q=&depth=` · `GET /api/file?path=` · `GET /api/files` ·
`GET /api/snippet?name=|path=&start=&end=` · MCP at `/api/mcp/sse` + `/api/mcp/message`

## Build & run

```bash
# Web UI — React + TypeScript + Ant Design + Vite (same stack as the email app)
cd web && npm install && npm run build   # → web/dist (served statically by the binary)
cd ..

cargo build -p deepwiki --release        # binary at target/release/deepwiki
PORT=4330 ./target/release/deepwiki       # serves Web UI + REST + MCP on :4330
# dev server with HMR: cd web && npm run dev
```

The header's repo field is an **AutoComplete**: once you've indexed folders, clicking it lists
your previously-indexed roots (with file/symbol counts + relative time) for one-click
re-indexing, and typing filters that list. History is kept in the `indexed_roots` table and
served by `GET /api/recents`.

The Web UI (`web/`, React 19 + AntD 6, theme-synced with the SenClaw host via postMessage)
has three tabs:

- **Wiki** (`WikiView`) — page tree + react-markdown, a **STRUCTURE file tree** (`/api/files`,
  expandable folders → files; click a file → outline + syntax-highlighted source), and a
  dual-mode query box:
  - **Tìm kiếm** — returns grounded symbol evidence (`/api/context`).
  - **Hỏi AI** (Devin-style) — `POST /api/ask` **investigates the question multi-hop through the
    call graph** (callers + callees, both directions, `query::investigate`), sends that subgraph +
    source excerpts to **SenClaw's configured LLM** (Space-App **bridge** `llm.request`), and
    renders: a cited Markdown answer, the model, a **"Graph tổng quan — luồng điều tra"**
    (`OverviewGraph` — the investigation subgraph laid out by depth), a **"Xem luồng"** button
    (full Graph tab on the focus symbol), and the evidence. Every Q&A is **saved to history**
    (`ask_history` table) — the **Lịch sử** panel lists past questions and reopens them (answer +
    graph) or deletes them (`/api/ask-history`, `/api/ask-history/:id`).

> **Hỏi AI requires:** (1) the SenClaw **daemon built with this repo** (the bridge `llm.request`
> handler lives in the main crate — see `src/gateway/ui_server/{space.rs,llm_config.rs}`), and
> (2) an **active LLM configured** in SenClaw (Settings → Models). The app calls the daemon at
> `SENCLAW_BASE_URL` (injected when the daemon launches the app; defaults to
> `http://127.0.0.1:18788`). FTS uses Porter stemming so NL queries match identifiers
> (`indexing` → `index_repo`).
- **Code** (`CodeView`) — symbol search + call-graph/blast-radius explorer, with an inline
  **syntax-highlighted source viewer** (`CodeBlock`, via `/api/snippet`) and a "Graph" button.
- **Graph** (`GraphView` + `OverviewGraph`) — an interactive **multi-hop call-graph**
  (`/api/investigate`): columns by relative depth (**callers N … focus … callees N**). Each node
  shows **name + kind badge + file:line** so a coder reads real logic, not bare names. Controls:
  Cả hai/Callers/Callees and **Sâu 1/2/3** (depth). Click a node to re-centre.

**Path filtering + Settings.** Indexing skips build artifacts by default (`node_modules`, `target`,
`dist`, `build`, `release`, `web_dist`, `*.min.js`, `*.map`, …) and auto-detects minified/generated
files, so the graph reflects real source (no flood of 1-char symbols from vendored bundles). The
header **"Loại trừ path"** field adds custom globs; the **⚙️ Settings drawer** (`SettingsDrawer`)
edits the **full default-excludes list** (with "Khôi phục mặc định"), custom excludes, and the
minified line-length threshold — all persisted in the `settings` meta JSON and applied on re-index
(`/api/settings`, `index::Settings`/`load_settings`/`save_settings`). `IndexReport.excluded` counts
skipped files.

**LLM (Hỏi AI) uses SenClaw's Main model.** The bridge `chat_completion` reads SenClaw's **active
(Main) LLM config** — same model the daemon uses. The Settings drawer shows it live via
`GET /api/llm-info` (e.g. `deepseek-v4-flash`), proxied from the daemon's `/api/llm-config`, so you
can confirm a real model (not a mock) is wired. Requires the daemon built from this repo (bridge
enabled) + an active model in Settings → Models.

The typed API client is `web/src/api.ts`.
