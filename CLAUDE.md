# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project overview

SemaClaw is a general-purpose framework for personal AI agents — multi-channel messaging gateway, agent orchestration, memory, scheduling, wiki, and Web UI. It runs on the [sema-code-core](https://github.com/midea-ai/sema-code-core) agent runtime.

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
