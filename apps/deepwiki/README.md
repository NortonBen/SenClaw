# DeepWiki App Space

**AI code intelligence for any local codebase** — a unified app combining a tree-sitter
**symbol/call graph** (à la [colbymchenry/codegraph](https://github.com/colbymchenry/codegraph))
with an **AI-generated, source-grounded wiki** (à la [Cognition/Devin's DeepWiki](https://deepwiki.com)).
One index powers both: the graph gives agents surgical structural context, the wiki gives humans
browsable docs + conversational Q&A.

> CodeGraph and DeepWiki were merged into this single self-contained app. The former
> `codegraph` app and the shared `codeindex-core` crate were both folded in and removed —
> the code-intelligence core now lives in this crate (`src/{db,index,lang,model,parse,query}.rs`).

- **Parsing/index:** tree-sitter (Rust, Python, JavaScript, TypeScript/TSX, Go) →
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

- **Wiki** (`WikiView`) — page tree + react-markdown + grounded Ask box, plus a **STRUCTURE
  file tree** built from `/api/files` (expandable folders → files with line counts); clicking a
  file shows its **outline + syntax-highlighted source** in the content area.
- **Code** (`CodeView`) — symbol search + call-graph/blast-radius explorer, with an inline
  **syntax-highlighted source viewer** (`CodeBlock`, via `/api/snippet`) and a "Graph" button.
- **Graph** (`GraphView`) — an interactive SVG **call-graph**: the focused symbol in the
  centre, callers feeding in from the left and callees fanning out to the right (internal nodes
  coloured/clickable, external nodes muted). Click any node to re-centre; filter callers/callees.

The typed API client is `web/src/api.ts`.
