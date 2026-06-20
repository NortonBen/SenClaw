# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project overview

SenClaw is a general-purpose framework for personal AI agents — multi-channel messaging gateway, agent orchestration, memory, scheduling, wiki, and Web UI. It runs on the [sema-code-core](https://github.com/midea-ai/sema-code-core) agent runtime.

The repo is mid-rewrite: the original TypeScript codebase (`src-old/`) is being ported to Rust (`src/`). The TypeScript source is still the functional reference. The Rust port renames the binary to **senclaw** and uses the `SENCLAW_*` env-var prefix (vs. `SEMACLAW_*`).

## Build & run

### Rust (in-progress port)

```bash
cargo build              # compile
cargo run                # start the daemon (stub — most modules not yet wired)
cargo test               # run all tests
cargo test -p senclaw     # run crate tests (single binary crate, same as above)
cargo test -- db          # run tests matching "db"
```

### TypeScript (reference implementation)

```bash
npm install
npm run build            # tsc → dist/
npm start                # node dist/index.js
npm run dev              # tsx src/index.ts (watchless dev)
npm run cli              # tsx src/cli.ts <subcommand>
```

### Web UI (React + Vite + Tailwind)

```bash
npm run build:web        # cd web && npm install && npm run build
npm run dev:web          # cd web && npm run dev (Vite dev server)
```

## Architecture

### Startup sequence (daemon)

The TS `src-old/index.ts` defines the canonical boot order, which `src/lib.rs::run_daemon()` will replicate:

1. SQLite init (WAL, schema, memory tables)
2. GroupManager — load group bindings from DB + config.json
3. Channel adapters connect (Telegram → Feishu → QQ → WeChat), each graceful on failure
4. AgentPool + GroupQueue created, wired with sendReply callback
5. MessageRouter starts — routes incoming messages to AgentPool via GroupQueue (per-group FIFO)
6. TaskScheduler starts — polls for due cron/interval/once tasks
7. DispatchBridge, PersonaRegistry, VirtualWorkerPool — DAG team orchestration
8. WebSocketGateway + UIServer (axum) — serves React Web UI + WS events
9. WikiManager — git-driven knowledge base
10. Graceful shutdown on SIGINT/SIGTERM

### Key layers

- **`agent/`** — Agent lifecycle, multi-agent pool with per-group concurrency limits, permission bridging (human-in-the-loop), persona registry, DAG-based virtual worker dispatch
- **`gateway/`** — Message routing, group binding management, trigger/mention detection, command dispatch, WebSocket push events, HTTP/WS UI server
- **`channels/`** — Telegram (teloxide), Feishu/Lark (REST SDK), QQ, WeChat adapters
- **`mcp/`** — MCP servers exposed to agents: admin, dispatch, memory, schedule, send, virtual worker, workspace, local Wiki (git)
- **`memory/`** — FTS5 full-text search + vector similarity (sqlite-vec, not yet wired in Rust). Chunking, embedding cache, query rewrite, daily log indexing. Providers: OpenAI, OpenRouter, Ollama, local (Xenova/transformers.js in TS)
- **`scheduler/`** — Cron/interval/once task execution with five context modes: `isolated` (fresh session), `group` (shared chat context), `notify` (push-only), `script` (shell), `script-agent` (shell output fed to agent)
- **`db/`** — rusqlite wrapper (Mutex-protected connection). Tables: `groups`, `channel_messages` (FIFO), `scheduled_tasks`, `task_run_logs`, `router_state`. Memory tables in `memory::schema`
- **`wiki/`** — Git-backed knowledge base that converts agent outputs into structured, searchable entries
- **`clawhub/`** — ClawHub skill marketplace (auth, lockfile, signal protocol)
- **`skills/`** — Bundled skill definitions (bot-channels, clawhub, wiki)
- **`cli/`** — Subcommands: `skills`, `clawhub`, `wiki`, `channel`
- **`config.rs`** — Single `Config::from_env()` read at startup. All paths default under `~/.senclaw/`

### Web UI

React 18 + Vite 6 + Tailwind 3. Served by the Rust axum server embedded in the daemon. Source in `web/src/` with two entry points: `main.tsx` (main UI) and `wiki-main.tsx` (wiki viewer).

## Testing

- Rust: `cargo test` — unit tests co-located in `#[cfg(test)]` modules at the bottom of each source file
- The old TS code has three test files at the repo root: `test-comprehensive.ts`, `test-multi-model.ts`, `test-regression.ts`

## Porting conventions

When porting from `src-old/` to `src/`:
- Filenames: `camelCase.ts` → `snake_case.rs`. Module declarations in `mod.rs` files
- The TS `IChannel` interface becomes a trait (not yet defined)
- `anyhow::Result` for fallible functions, `thiserror` for library error types
- SQLite access through `Db::with_conn()` / `Db::with_conn_mut()` closures (Mutex guard)
- Config is read once via `Config::from_env()` — do not call `env::var()` directly in library code

## SenClaw MCP naming convention

All SenClaw-bundled MCP servers follow a strict three-level naming pattern. Skills, docs, and any code referencing MCP tools by name MUST use the canonical form below — never invent shortened or "stripped" variants.

### Pattern

```
mcp__senclaw-<domain>__<tool-prefix>_<verb>[_<modifier>]
└────┬─────┘└────┬───┘└────────┬──────────┘
   protocol  server name      tool name
   prefix    (kebab-case)     (snake_case)
```

1. **Server name** — `senclaw-<domain>` (kebab-case). The string passed as the first arg of `McpServerConfig::new(...)` in [`src/mcp/helper.rs`](src/mcp/helper.rs). One server per domain.
2. **Tool name** — `<tool-prefix>_<verb>[_<modifier>]` (snake_case). The Rust method name under `#[rmcp::tool]` inside `src/mcp/<domain>_server.rs`. The `<tool-prefix>` is usually the same word as `<domain>`, with a few historical exceptions (see table).
3. **Full identifier from Claude Code** — concatenate: `mcp__` + server name + `__` + tool name. This is what `ToolSearch select:...` and direct tool calls expect.

### Canonical registry

| Domain | Server name | Tool prefix | Example full tool |
|---|---|---|---|
| browser | `senclaw-browser` | `browser_` | `mcp__senclaw-browser__browser_navigate` |
| memory | `senclaw-memory` | `memory_` | `mcp__senclaw-memory__memory_search` |
| schedule | `senclaw-schedule` | `schedule_` | `mcp__senclaw-schedule__schedule_task` |
| wiki | `senclaw-wiki` | `wiki_` | `mcp__senclaw-wiki__wiki_write` |
| dispatch | `senclaw-dispatch` | `dispatch_` | `mcp__senclaw-dispatch__dispatch_task` |
| send | `senclaw-send` | `send_` | `mcp__senclaw-send__send_message` |
| workspace | `senclaw-workspace` | `workspace_` | `mcp__senclaw-workspace__workspace_*` |
| virtual | `senclaw-virtual` | `run_` / `virtual_` | `mcp__senclaw-virtual__run_persona` |
| space | `senclaw-space` | `space_` | `mcp__senclaw-space__space_schedule_activity` |
| ocr | `senclaw-ocr` | `ocr_` | `mcp__senclaw-ocr__ocr_*` |
| litho | `senclaw-litho` | `litho_` | `mcp__senclaw-litho__litho_generate` |
| **cognitive** | `senclaw-cognitive` | **`cog_`** (not `cognitive_`) | `mcp__senclaw-cognitive__cog_search` |
| admin | `senclaw-admin` | (varies) | — |

Source of truth: server names live in [`src/mcp/helper.rs`](src/mcp/helper.rs) `*_mcp_config()` builders; tool names are the `#[rmcp::tool] async fn <name>` definitions in `src/mcp/*_server.rs`.

### Rules for Claude

- **Never invent a "short" tool name.** There is no `mcp__browser__*` resolver in plain Claude Code. The form `mcp__senclaw-<domain>__<prefix>_<verb>` is the only one that resolves.
- **Never substitute another MCP server** (e.g. Playwright plugin, `Claude_in_Chrome`) when a SenClaw skill references a SenClaw server. SenClaw skills assume their own server semantics, return shapes, and side effects — substituting a different browser MCP silently breaks the skill's contract.
- **Verify the server is registered before suggesting the user run it.** Check `.mcp.json` at project root and the Claude Code MCP list. If absent, the fix is to register the server in `.mcp.json` (stdio command pointing to the `senclaw` binary with the matching `<domain>-server` subcommand), not to rewrite the skill.
- **Match the registry above when writing or updating a SKILL.md.** When in doubt, run `grep -n '#\[rmcp::tool' src/mcp/<domain>_server.rs -A 2` to confirm the exact `async fn <name>` and use that verbatim.
- **Cognitive is the one exception** — server `senclaw-cognitive` but tool prefix `cog_*`. Do not "normalize" it to `cognitive_*`.

### Registering a SenClaw MCP server for Claude Code

Project-level [`.mcp.json`](.mcp.json) template (one entry per server needed):

```json
{
  "mcpServers": {
    "senclaw-<domain>": {
      "type": "stdio",
      "command": "/absolute/path/to/target/release/senclaw",
      "args": ["<domain>-server"],
      "env": { "SENCLAW_WS_PORT": "18789" }
    }
  }
}
```

The `<domain>-server` subcommand list is in `src/main.rs` (e.g. `browser-server`, `memory-server`, `schedule-server`, ...). After editing `.mcp.json`, the user must restart Claude Code and approve the server in the prompt.
