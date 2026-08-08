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
| js | `senclaw-js` | `js_` | `mcp__senclaw-js__js_eval` |
| **cognitive** | `senclaw-cognitive` | **`cog_`** (not `cognitive_`) | `mcp__senclaw-cognitive__cog_search` |
| **sandbox** | `senclaw-sandbox` | **`sbx_`** (not `sandbox_`) | `mcp__senclaw-sandbox__sbx_run` |
| usage | `senclaw-usage` | `usage_` | `mcp__senclaw-usage__usage_overview` |
| admin | `senclaw-admin` | (varies) | — |

Source of truth: server names live in [`src/mcp/helper.rs`](src/mcp/helper.rs) `*_mcp_config()` builders; tool names are the `#[rmcp::tool] async fn <name>` definitions in `src/mcp/*_server.rs`.

### Space-App MCP servers

Space Apps register their own MCP servers with a different pattern: `mcp__<mcp.name>__<tool>`, where `<mcp.name>` is the `mcp.name` field of the app's `senclaw-manifest.json` (usually `<app-id>-mcp`, e.g. `ssh-manager-mcp` → `mcp__ssh-manager-mcp__ssh_execute_command`, but not always — luna-calendar registers `luna-mcp`). Never derive the server name from the app id; read the manifest. Tool names live in the app's `apps/<app>/src/mcp.rs` `tools/list`. Runtime check: `GET http://127.0.0.1:18788/api/mcp-servers` lists every registered server with its status. Full lookup + troubleshooting guide (including the `groups.allowed_tools` whitelist trap that empties a session's tool roster): [docs/tool-skill-name-lookup.md](docs/tool-skill-name-lookup.md).

### MCP tool aliases (Plugins → Alias)

Users (and Space Apps via `mcp.toolAliases` in `senclaw-manifest.json`) can rename an
MCP tool or override it with another tool. Mapping `alias → target` lives in the
`mcp_tool_aliases` table, resolves at stage 0 of `resolve_tool_by_name`
([src/tools/tool_search.rs](src/tools/tool_search.rs)) and decorates the roster via
[src/tools/tool_alias.rs](src/tools/tool_alias.rs). App-declared aliases import
**disabled** — the user must enable them in Plugins → Alias. When a tool name doesn't
behave as documented, check this table first (`GET /api/tool-aliases`). Full guide:
[docs/mcp-tool-alias.md](docs/mcp-tool-alias.md).

### Space-App external links

Links in a Space App UI must open in the **system browser**, never navigate the embedded desktop webview. Flow (JS hook `openExternal` → webview safety net → daemon `POST /api/ui/open-url`), canonical helper (`apps/zeach/web/src/openExternal.ts`), and per-app adoption checklist: [docs/space-app-open-external.md](docs/space-app-open-external.md).

### Rules for Claude

- **Never invent a "short" tool name.** There is no `mcp__browser__*` resolver in plain Claude Code. The form `mcp__senclaw-<domain>__<prefix>_<verb>` is the only one that resolves.
- **Never substitute another MCP server** (e.g. Playwright plugin, `Claude_in_Chrome`) when a SenClaw skill references a SenClaw server. SenClaw skills assume their own server semantics, return shapes, and side effects — substituting a different browser MCP silently breaks the skill's contract.
- **Verify the server is registered before suggesting the user run it.** Check `.mcp.json` at project root and the Claude Code MCP list. If absent, the fix is to register the server in `.mcp.json` (stdio command pointing to the `senclaw` binary with the matching `<domain>-server` subcommand), not to rewrite the skill.
- **Match the registry above when writing or updating a SKILL.md.** When in doubt, run `grep -n '#\[rmcp::tool' src/mcp/<domain>_server.rs -A 2` to confirm the exact `async fn <name>` and use that verbatim.
- **Two prefix exceptions** — server `senclaw-cognitive` has tool prefix `cog_*`, and server `senclaw-sandbox` has tool prefix `sbx_*`. Do not "normalize" them to `cognitive_*` / `sandbox_*`.
- **`senclaw-sandbox` is the built-in OS-sandbox engine** (`src/sandbox`, ported from `apps/sandbox` which still exists as a standalone Space App with server `sandbox-mcp`). The built-in engine's data lives in `~/.senclaw/sandbox/`; the Space App keeps its own under `~/.senclaw/space-app-data/sandbox/`. Enforcement switches (agent Bash exec / Python / Node.js / scheduler scripts through the sandbox) live at `/api/sandbox/exec-policy` and the Plugins → Sandbox Web UI page.

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

## Space App lifecycle: background vs session

Every server Space App is one of two things, declared as `runtime.mode`:
**`background`** (started with the daemon, supervised, restarted when it dies)
or **`session`** — the **default** — started when the app is opened or one of
its MCP tools is called, and stopped once idle for `runtime.idleTimeoutSecs`
(60s default, 15s floor). Session is the default because the old behaviour
launched all ~50 installed apps at boot and kept them forever.

The mechanism that makes on-demand MCP work: a session app's MCP server is
registered against **the daemon's app proxy**
(`/api/space/apps/<id>/proxy<mcp.path>`), not the app's own port, and its tool
list comes from `<app>/.senclaw/mcp-tools.json` cached at the last successful
connection. So its tools stay in every agent's roster while it is stopped, and
the first call connects → proxy → spawn → answer. Without both halves nothing
would ever call a stopped app and it would never start.

Rules for Claude:

- **Never assume "not running" is a fault.** For a session app it is the resting
  state; only background apps are supervised.
- **A misspelled `mode` is silent** — it falls back to `session`, so an app that
  must poll a channel quietly stops.
  [`tests/space_app_lifecycle_manifests.rs`](tests/space_app_lifecycle_manifests.rs)
  enforces the spelling *and* scans each app's Rust for autonomous-startup
  markers (`extbridge::serve_ws`, `spawn_heartbeat`, `spawn_scheduler`,
  `spawn_poller`, `run_supervisor`, `spawn_janitor`); an app that gains one must
  be declared `background` or `cargo test` goes red.
- **Adding a background loop to an app means editing its manifest too.**

Manifest also carries `requires` (what the machine must have: `node`, `python`,
`bin`, `env`, `os` — checked at install *and* before every launch, a hard miss
refuses the launch with the reason) and `sandbox` (the confinement the app asks
for itself; `force: true` means the settings dialog cannot turn it off, and a
non-forced declaration never overrides a choice the user already saved).

`runtime.runner` (`binary` | `node` | `python` | `shell`, inferred from `start`)
drives a one-off prepare step: `npm ci`/`npm install` for Node, and for Python a
**virtualenv at `<app>/.venv`** plus `pip install -r requirements.txt` into it —
never the user's system Python. The stamp hashes file *content*, not mtimes, so
extracting an update does not reinstall.

Model in [`src/apps/`](src/apps/) (`manifest.rs`, `requirements.rs`,
`prepare.rs`, `sandbox_decl.rs`); process lifecycle in
[`src/gateway/ui_server/space_mcp.rs`](src/gateway/ui_server/space_mcp.rs).
Endpoints: `POST /api/space/apps/:id/{stop,start}`,
`GET /api/space/apps/:id/requirements`, plus `lifecycle` + `requirements` blocks
on `/runtime`. Knobs: `SENCLAW_SPACE_SUPERVISE_SECS` (20),
`SENCLAW_SPACE_IDLE_SWEEP_SECS` (10). Full guide:
[docs/space-app-lifecycle.md](docs/space-app-lifecycle.md).

### Space App SDKs

Four, one per language an app can be written in — all documented against the
same manifest:

| | |
|---|---|
| Rust | [`app-space-sdk/`](app-space-sdk/) |
| Node / TypeScript | [`senclaw-sdk/senclaw-app-sdk/`](senclaw-sdk/senclaw-app-sdk/) — `@senclaw/space-sdk` on npm, subpaths `/mcp` and `/lifecycle` |
| Python | [`senclaw-sdk/senclaw-app-sdk-python/`](senclaw-sdk/senclaw-app-sdk-python/) — `senclaw-space-sdk` on PyPI, `senclaw_space`, standard library only |
| Go | [`senclaw-sdk/senclaw-app-sdk-go/`](senclaw-sdk/senclaw-app-sdk-go/) — `go get github.com/NortonBen/SenClaw/senclaw-sdk/senclaw-app-sdk-go`, package `senclaw` + subpackages `manifest` / `dispatch`, standard library only |

Each SDK carries its own runnable minimal app under `examples/`, and each
exposes a manifest validator that catches the silent-failure spellings
(`python -m senclaw_space.manifest <file>` / `validateManifest()` /
`go run …/cmd/senclaw-manifest <file>`).

**A Go app has no install step.** `runtime.install` runs for the `node` and
`python` runners only ([`src/apps/prepare.rs`](src/apps/prepare.rs) returns
early for `binary` and `shell`), so a Go app either ships a built binary
(`start: "./app"`, runner inferred `binary`) or compiles in `start`
(`go run .`, runner `shell`, `requires.bin: ["go"]`, within the daemon's 30s
health budget). Declaring `install: "go build …"` is silently skipped — the Go
SDK's `manifest.Validate` is the only thing that flags it.

## Per-app Space App sandbox

Each Space App can be run inside the OS sandbox from Plugins → Space Apps → the
**Sandbox** button (web + desktop): write-jail to its own folders, a read mode
(`open` / `strict`), extra folders, and a network mode — everything / nothing /
**only these sites**. Per-site egress cannot be an OS rule (Seatbelt accepts only
`*` or `localhost` as a remote host), so it is an allowlisting proxy on loopback
with the sandbox given no direct egress: a client that ignores `HTTP_PROXY`
reaches nothing rather than everything. Config in
[`src/sandbox/app_policy.rs`](src/sandbox/app_policy.rs), launch wrapping in
[`src/sandbox/app_launch.rs`](src/sandbox/app_launch.rs), proxy in
[`src/sandbox/proxy.rs`](src/sandbox/proxy.rs), REST at
`/api/space/apps/:id/sandbox`. Enforcement differs by platform (macOS full, Linux
folders only, Windows none) and the UI says so up front. Traps — `strict` breaks
apps whose runtime lives under `$HOME` (nvm), granting a folder is not enough when
its *parent* is denied (SQLite dies with `SQLITE_CANTOPEN`), `npm start` wants
`registry.npmjs.org`, paths are never remapped — plus the measured before/after
table: [docs/space-app-sandbox.md](docs/space-app-sandbox.md). Process lifecycle
(daemon must catch SIGTERM, shutdown signals all apps at once, a healthy port is
*reclaimed* rather than adopted — only when the process's cwd proves it is that
app's): [docs/sandbox-app-design.md](docs/sandbox-app-design.md).

## Space App runtime monitor

Plugins → Space Apps → **Details & logs** carries a live panel for any
`runtime.kind == "server"` app: running/answering state (a health check, not just
"tracked"), pid, port, uptime, **launch count** (a number climbing on its own is
the only visible signature of a crash loop), CPU/RAM by *process group* (`npm →
node`), open sockets via `lsof`, the allowlist proxy's allowed/denied counters,
and the cwd + start command + env needed to rerun the app by hand. One endpoint,
`GET /api/space/apps/:id/runtime`
([src/gateway/ui_server/space_runtime.rs](src/gateway/ui_server/space_runtime.rs)),
best-effort everywhere — a missing `lsof` or a timed-out health check becomes a
note in the payload, never a failed request. Plugins → Sandbox additionally
carries the fleet view (`GET /api/space/apps/sandbox-overview`, one `ps` for the
whole list), whose first column is *what the running process actually got* — the
only place the "configured confined, running unconfined" gap is visible, since a
profile is fixed at launch. Guide:
[docs/space-app-monitor.md](docs/space-app-monitor.md).

## Space App network binding

Space Apps under `apps/*` have **no authentication of their own**. Their REST API
and their MCP endpoint are wide open to anyone who can reach the port: the trust
boundary is the loopback interface, not the app. The daemon reaches every app at
`http://127.0.0.1:<port>` — health checks (`src/gateway/ui_server/space_mcp.rs`
`health_url`), the MCP origin, and the UI proxy all hardcode loopback — so
binding loopback costs nothing operationally.

Every app bootstrap MUST therefore resolve its bind host from the env, never
hardcode one:

```rust
// Loopback by default. A Space App authenticates nothing of its own — the
// daemon reaches it over 127.0.0.1 and the UI is same-origin — so binding
// 0.0.0.0 hands the whole REST + MCP surface to anyone on the LAN. Set
// SENCLAW_BIND_HOST=0.0.0.0 to opt in to that explicitly.
let host = std::env::var("SENCLAW_BIND_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
let listener = tokio::net::TcpListener::bind(format!("{host}:{port}")).await.unwrap();
```

Rules:

- **Never write `bind("0.0.0.0:...")`.** The bootstrap is copy-pasted between apps,
  so one bad copy re-exposes the fleet. This was a real exposure (fixed 2026-07-31):
  nearly every app was serving customer data on `*:PORT`.
- **Same knob for extension-bridge WebSockets** (`apps/*/src/extbridge.rs`). The
  Chrome extensions all dial `ws://127.0.0.1:<port>`, so loopback is correct there too.
- **Node apps** read `process.env.SENCLAW_BIND_HOST || '127.0.0.1'` and pass it as
  the `app.listen(PORT, HOST, ...)` host argument.
- **Next.js apps** must pass `-H ${SENCLAW_BIND_HOST:-127.0.0.1}`; bare `next start`
  binds `0.0.0.0`.
- `apps/rule-engine` keeps its older `RULE_ENGINE_BIND` override, checked before
  `SENCLAW_BIND_HOST`.

[`tests/space_app_bind_loopback.rs`](tests/space_app_bind_loopback.rs) enforces all
of the above on every `cargo test`.

## Space App access token & API version

Loopback is a boundary around the *machine*, not around an *app*: knowing an
app's id (public) used to be enough for any local process — including another
Space App — to drive its `/bridge` (a full tool-enabled agent), read its
`/config` (API keys) and query its SQLite. So the daemon now mints **one access
token per installed app** (`sca_<64 hex>`, table `space_app_tokens`), hands it to
the app's process in **`SENCLAW_TOKEN_ACCESS_APP`**, and treats it as the app's
name: a token presented against another id is **403 in every mode**.

- **`SENCLAW_APP_TOKEN_MODE`** = `off` (default — tokenless calls served exactly
  as before, so the installed fleet keeps working) | `warn` (served + one log
  line per app) | `strict` (refused unless the caller is the daemon's own UI).
  Strict only gates the app's *data* routes (`/bridge`, `/config`, `/sqlite/query`,
  `/mcp/register`, `/env`, `/token`) — never management routes or `/proxy`,
  `/static`.
- **`SENCLAW_API_VERSION`** — Space-App contract version (now **2**). Injected
  into every app, stamped on every app-scoped response, sent by every SDK. Older
  contracts are served; a newer one gets **426**.
- **Inbound**: the proxy stamps the token on everything it forwards (and strips
  a client's copy), and MCP configs carry it in `headers`/`env` so a *background*
  app — whose MCP client dials its port directly — still authenticates. Each SDK
  ships an opt-in guard (`RequireAppToken` / `require_app_token` /
  `requireAppToken`) that closes the app's own port to everything but the daemon.

Rules for Claude:

- **Strict mode is not a boundary against local malware.** Anything that can read
  `~/.senclaw/senclaw.db` reads every token in it. The feature is app-vs-app
  isolation, and it is only a real boundary combined with the per-app sandbox.
- **Never turn the relay's `TrustedOperator` marker into a header.** It is a
  request extension precisely because nothing on the network can forge one.
- **Never drop the `sca_` prefix check** in `presented_token` — the daemon's own
  API token arrives through the same `Authorization: Bearer` header.
- `/env` must never carry the token: it feeds the app's *browser* UI.

Model in [`src/apps/token.rs`](src/apps/token.rs), enforcement in
[`src/gateway/ui_server/app_auth.rs`](src/gateway/ui_server/app_auth.rs). Full
guide: [docs/space-app-api-token.md](docs/space-app-api-token.md).

## Daemon network binding & API token

The daemon's own surface (UI HTTP 18788 + WS gateway 18789) binds `127.0.0.1`
by default via `SENCLAW_UI_BIND_HOST` — a knob deliberately **separate** from
the Space-App `SENCLAW_BIND_HOST` (apps have no auth; the env would propagate
to them). Desktop users flip it at **Settings → General → Network access**
(Private `127.0.0.1` / Public `0.0.0.0`); the choice is persisted in prefs and
handed to the daemon at spawn time, so it needs a daemon restart to take
effect. Binding the daemon to a non-loopback host auto-enables token auth
(`src/gateway/ui_server/auth.rs`): every non-loopback peer must present the
API token (`SENCLAW_API_TOKEN` env, else auto-generated `~/.senclaw/api_token`,
0600) via `Authorization: Bearer`, `X-SenClaw-Token`, `?token=`, or the
`senclaw_token` cookie minted by `POST /api/auth/login`. Loopback peers are
always exempt — local desktop/apps need zero config. `/api/auth/status` and
`/api/auth/login` are the only open API paths; CORS is loopback-origin-only
(never reintroduce `CorsLayer::permissive()` — it leaked `/api/llm-config`
keys to any website). Full guide: [docs/remote-access-security.md](docs/remote-access-security.md).
