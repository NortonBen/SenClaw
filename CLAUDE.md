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

## Chat attachments: images (vision, else OCR) and documents

Everything attached to a chat message — from the web composer, the desktop
picker, a paste, or a channel adapter that downloaded media — travels as
`attachments: [{dataUrl, mimeType, name?}]` and is one type end to end:
[`types::MessageAttachment`](src/types.rs). `is_image()` splits the two routes in
[`AgentPool::prepare_turn_input`](src/agent/agent_pool/pool.rs); image turns
bypass the engine's text-only mid-turn pending queue because the per-group queue
is what serializes them ([`src/lib.rs`](src/lib.rs) `enqueue_and_process`).

**Documents** ([`src/agent/documents.rs`](src/agent/documents.rs)) are saved
under `~/.senclaw/uploads/<sanitized jid>/<stamp>-<name>` and their text pulled
out (`text/*` and code by MIME *or* extension, `.docx` by unzipping
`word/document.xml`; no PDF extractor is built in). `append_document_context`
inlines up to 20k characters **and always states the saved path**, so the agent
can Read/grep the rest — or the whole file when the format is one we can't parse.
An unreadable file is reported as such, never silently dropped, and the block
tells the model not to invent contents.

**Images** go through `build_agent_input`, which resolves every source (local
path, http(s) download, `data:` URL) to base64 and returns interleaved blocks.
`split_input` then separates the text from the images, and
`AgentPool::dispatch_user_input` picks one of two routes:

- **Vision model** → the images travel as real `ContentBlock::Image` blocks,
  placed *ahead* of the prompt text in the user turn
  ([`ZenEngine::start_query`](src/zen_core/engine.rs)), and serialize as
  `image_url` data URLs (OpenAI) or `source.data` raw base64 (Anthropic).
- **Text-only model** → each image is transcribed by the built-in OCR engine and
  `append_ocr_context` folds the result into the prompt, labelled as a
  transcription. When OCR yields nothing the prompt tells the model to say so
  and **not** guess — an unanswerable "describe this image" is otherwise
  answered with an invented one.

Rules for Claude:

- **The capability check must go through
  [`ZenEngine::model_accepts_images`](src/zen_core/engine.rs).** It wraps the
  *same* `resolve_model_profile_at` the turn itself uses, including its
  fallbacks (unknown per-group override → active config → first config). A
  second lookup with its own resolution disagrees exactly on those edges and
  routes a vision model's images through OCR.
- **Never send image blocks on a maybe.** No config resolved → treat as
  vision-less. A text-only endpoint answers an image block with a hard 400 that
  fails the whole turn; OCR only degrades it.
- **[`src/zen_core/vision.rs`](src/zen_core/vision.rs) patterns are
  load-bearing, and the web copy in `LLMSettings.tsx` must match.** They were
  pinned to the model generations that existed when written, which silently
  demoted each new release to the OCR path. Generation digits are open-ended
  (`claude-[3-9]`, `gpt-[5-9]`, `gemini-[2-9]`) for that reason. The explicit
  `vision` toggle in Settings → Models always wins over inference.
- **Save the document before extracting from it.** The path is the fallback for
  every format we can't parse; an extractor that returns `Err` without a saved
  file leaves the agent with nothing to open.
- **Only Telegram downloads channel media** (photos and image-typed documents,
  in [`src/channels/telegram.rs`](src/channels/telegram.rs) `download_media`).
  Feishu/QQ/WeChat construct `attachments: Vec::new()` — adding media there means
  filling that field, not just parsing the event. A channel turn is rebuilt from
  DB history, so an adapter's attachments must reach
  `StoredMessage::attachments` or `run_agent` can never see them.
- Clients cap an image's long edge at 1568px before upload (`MAX_IMAGE_EDGE` in
  `ChatView.tsx`, `kMaxImageEdge` in
  `desktop_app/lib/features/chat/image_attachment.dart`) — a phone photo
  otherwise base64s past Anthropic's 5 MB per-image limit. Documents are capped
  at 32 MB on both ends (`MAX_DOC_BYTES`).

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

### Built-in MCP servers run in ONE process (`senclaw-core`)

`AgentPool` used to spawn fourteen MCP subprocesses per chat session — one per
built-in server. It now spawns a single `senclaw core-server` that hosts them
all in-process and merges their tool tables, so `wiki_*`, `workspace_*`,
`memory_*` … all arrive over one stdio connection. Each server keeps its own
subcommand (`senclaw wiki-server`, …) for debugging one in isolation, and
`mcp.bundled = false` (env `SENCLAW_MCP_BUNDLED`) restores the per-server spawn.
Adding a server means giving it `from_env() -> Result<Option<Self>>` plus
`vis = "pub"` on its `#[rmcp::tool_router]` — the aggregator never re-declares a
tool. Full design, limits, and a verified stdio transcript:
[docs/mcp-core-bundled.md](docs/mcp-core-bundled.md).

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

## Local models left the daemon

`src/local_model/` went from 30 121 lines to an OCR module. Everything model-
shaped now lives in one of two places, and the distinction is deliberate:

| | what | why this shape |
|---|---|---|
| [`apps/mlx-lm`](apps/mlx-lm) | LLM on Apple Silicon (`mlx-rs`) | **Space App** — optional, installed by choice, in the model picker only when present |
| [`apps/candle`](apps/candle) | LLM, cross-platform pure Rust | **Space App** — same |
| [`apps/local-model-core`](apps/local-model-core) | shared model root, HF downloader, `settings.json`, `/api/local-models/*` REST | library both engine apps mount |
| [`crates/senclaw-media`](crates/senclaw-media) | Whisper ASR (MLX) | **sidecar** — ships beside the daemon, spawned on demand, never in any app list |

**App vs sidecar is about absence.** A Space App can be uninstalled, and the
daemon must treat that as normal. Speech-to-text backs voice chat and the
transcribe endpoint — a missing binary there is a broken build, not an
uninstalled feature — so `senclaw-media` is a plain binary next to the daemon,
supervised by [`src/media_sidecar.rs`](src/media_sidecar.rs): fixed port 18790,
spawned on the first transcription, adopted if already running, killed on
daemon shutdown. `SENCLAW_MEDIA_BIN` points it at `target/` during development.

**It reaches a machine two ways, and both are enforced.** A desktop install
gets it inside the app bundle, so `swap_bundle` **refuses** any downloaded
bundle missing the daemon, the sidecar, or (macOS) `mlx.metallib` — checked on
the extracted `.new` copy before the live bundle moves, in *both* copies of the
updater ([`distrib.rs`](src/cli/commands/distrib.rs) and
[`update_desktop/src/apply.rs`](update_desktop/src/apply.rs)). A CLI install
gets nothing from `install.sh`, so `senclaw web` downloads the standalone
`senclaw-media-<triple>` asset into `~/.senclaw/bin` — the third and last
candidate in `binary_path`, after `SENCLAW_MEDIA_BIN` and next-to-`current_exe`.
The bundle copy must keep winning over the download: after a desktop update
those are two different versions.

**What stayed in `src/` stayed because it has no MLX in it.** TTS is VieNeu
(ONNX on the CPU) plus the macOS `say` presets — the MLX voices are gone, not
relocated: ZipVoice never synthesized anything, and MMS-VITS was the last thing
keeping `mlx-rs` on the TTS path for a voice no machine had selected. OCR is
MNN. Neither justifies a process hop.

The daemon **compiles no MLX at all**. `DAEMON_FEATURES` is
`local-embed-metal,local-embed,ocr-paddle-metal,tts-vieneu`; the `local-mlx*`,
`local-candle*`, `whisper-audio` and `cognitive-mlx-embed` features no longer
exist. `make app-build` builds the daemon without touching `mlx-sys`, then
builds and bundles `senclaw-media` (with `mlx.metallib` beside it — MLX
resolves the metallib relative to the executable, and a missing copy is a
hard error in CI, not a warning).

Rules for Claude:

- **Do not reintroduce an MLX dependency into the daemon.** The measurement
  that unlocked this split: two MLX processes generating concurrently on the
  same `Device(gpu, 0)` run clean — Metal isolates command queues per process.
  In-process concurrency is still unsafe; each MLX binary keeps its own
  process-wide serial lock (`mlx_serial`).
- **A removed voice must degrade, not 400.** `select_backend_for`'s catch-all
  is an `Unsupported` backend returning `NotImplemented`, which the fallback
  chain turns into the macOS voice plus a `fallback_reason`. Machines still
  have `facebook/mms-tts-vie` selected in config; `None` would hard-fail their
  next play button.
- **Symphonia probes audio by file extension.** The sidecar's transcribe
  endpoint takes a `filename` query param and the temp file keeps that
  extension — writing `.audio` breaks decoding of perfectly good files.
- **An app-provided config has an empty `api_key` on purpose** (the app proxy
  needs no credential). Anything that skips a config for an empty key must
  exempt `llm_provider::is_app_config` first, or the local model silently
  becomes unusable — `memory::cognitive::llm_openai` had exactly that bug.
- **Everything model-storing shares `~/.senclaw/local-models/`**, injected as
  `SENCLAW_LOCAL_MODELS_DIR`. Never point an engine at `space-app-data/`: the
  directory is tens of gigabytes and relocating it means everyone re-downloads.
- **`settings.json` is snake_case, and apps and daemon read the same file.**
  A `rename_all` on `local_model_core::settings::Settings` would parse every
  existing file into all-`None` silently. A test pins the exact daemon file.
- **Weights load on the first request, never in `main`.** Health gates are
  30 s with 5 s probes; both the apps and the sidecar answer `/health` before
  reading a byte of weights.
- **`idleTimeoutSecs` is 300 for the engine apps** (not the Space-App default
  60); the sidecar is not reaped at all — it drops weights after each use and
  an idle process costs a few megabytes.
- `ModelCard::vision` is **required and comes from the checkpoint's config**. A
  local id like `mlx-community/Qwen3.5-2B-OptiQ-4bit` matches no pattern in
  `src/zen_core/vision.rs`, so a name-based guess is right or wrong by accident
  — and the wrong direction is a hard 400 that fails the whole turn.
- **Engine apps declare `integration.launcher: false`**: they have a UI (model
  management), but it is reached from Settings — the launcher grid and Space
  sidebar filter them out. Absent means visible, so older apps keep their tile.
- **Workspace:** `default-members = [".", "app-space-sdk"]` keeps bare
  `cargo build`/`test` off the engines. `[workspace.dependencies]` pins shared
  versions — above all `mlx-rs`/`mlx-sys`, which `apps/mlx-lm` and
  `crates/senclaw-media` both build: on the same tag they share every compiled
  artifact in `target/`; drifted, the workspace pays for two full MLX builds.

## Space Apps that serve models (`llm` manifest block)

An app declaring an `llm` block becomes an LLM provider: its models appear in
the same picker as OpenAI and Anthropic, and turns route to its own
`/v1/chat/completions` over loopback.

```json
"llm": { "autoRegister": true, "path": "/v1", "adapt": "openai", "displayName": "MLX" }
```

The app speaks **OpenAI** — `GET /v1/models`, `POST /v1/chat/completions` (SSE
and JSON) — so the daemon reuses `adapt: "openai"` and needs **no new adapter**.
`app_space_sdk::llm::openai_router` renders the wire from a semantic
`LlmProvider` trait, so an app emits `Chunk::{Text, Reasoning, ToolCall, Usage}`
and never hand-writes the JSON.

Registration mirrors `mcp.autoRegister` exactly: session apps are addressed
through `/api/space/apps/<id>/proxy/v1`, and the model list is cached at
`<app>/.senclaw/llm-models.json` so a **stopped** app still has models in the
picker — without which nobody would select one, so nothing would call the app,
so it would never start.

Rules for Claude:

- **`LlmDecl::parse` returns `Result`, unlike every other parser in
  `src/apps/manifest.rs`.** The others fall back to a default because the
  failure is survivable. Here it is not: an `adapt` the daemon does not route
  means every turn gets an OpenAI body and fails upstream with an error naming
  neither the app nor the field, and `adapt: "local-mlx"` routes the turn to an
  in-process engine so the app is registered and *never called*.
- **`APP_DECLARABLE_ADAPTERS` is narrower than `ROUTED_ADAPTERS`** — `openai`
  and `anthropic` only. Do not widen it to whatever `query_llm` happens to
  dispatch.
- **App configs are never written to `config.json`.** They are rebuilt from
  `space_app_llm_providers` on every `load_llm_configs`. `save_llm_config`
  refuses an `app:` id; a frozen copy would outlive the app.
- **Merging happens inside `load_llm_configs`, not at the HTTP layer.** That
  function is the single seam the picker, `resolve_model_profile_at` and
  `model_accepts_images` all go through.
- **`REQUEST_TIMEOUT` no longer applies to a loopback endpoint.**
  `DEFAULT_MAX_NEW_TOKENS` is 8192, which at ~60 tok/s is over two minutes of
  legitimate output — a total deadline would cut it mid-sentence. Stalls are
  caught by the client's `read_timeout`, which resets on every byte. Never
  reintroduce a total timeout for a local provider.

Model in [`src/apps/llm_provider.rs`](src/apps/llm_provider.rs) and
[`src/apps/manifest.rs`](src/apps/manifest.rs) `LlmDecl`; SDK in
[`app-space-sdk/src/llm.rs`](app-space-sdk/src/llm.rs); registration in
[`src/gateway/ui_server/space_mcp.rs`](src/gateway/ui_server/space_mcp.rs)
`register_llm`. Design record:
[docs/space-app-llm-provider-sdk.md](docs/space-app-llm-provider-sdk.md).

## Gemma 4 on the native MLX path

Sliding-window layers keep a decode-time KV **ring**
([`cache.rs`](src/local_model/mlx_lm/cache.rs) `ring_head`): decode writes one
row via `slice_update` instead of evicting with a tail slice and re-growing,
which the pre-ring path did on every token past the window. Measured on both
Gemma-4 E2B and E4B it is **neutral, not an optimization** (<1% decode, no CPU
or GPU change), so treat it as a simpler eviction path, not a fast one.
Rotation is safe because
attention is permutation-invariant along the key axis — but *only* while these
layers pass **no mask** on decode, which `Gemma4TextModel::forward` does at
`seq <= 1`. Three paths need chronological order back and call `unrotate` first:
a multi-token write on a cache that already decoded, `trim_by`, and
`snapshot_clone` (the prefix cache replays a snapshot as a positional prefix).
Reordering the key axis changes floating-point accumulation order, so the
contract is **token parity, not bit identity**.

Sampling goes through [`sampling.rs`](src/local_model/mlx_lm/sampling.rs)
`sample_with` (top-k then nucleus on the survivors). Defaults come from the
**checkpoint's own `generation_config.json`**, never a per-architecture table:
precedence is user setting → checkpoint → off, where off is the historical
untruncated full-vocabulary draw. This moves sampled output for **every** local
checkpoint shipping those fields, not just Gemma — Qwen3 ships `top_k: 20`,
Qwen3.5 ships `20 / 0.80`. Greedy is untouched (`argmax` short-circuits before
either filter), so prefix-cache determinism is unaffected.

Rules for Claude:

- **`MLX_BENCH_EXT_DETERMINISM=1` is only meaningful with `temperature: 0`
  pinned in the bench cell's own `settings.json`.** At the Gemma default of 0.65
  it reports "OUTPUTS DIFFER" for every build including unmodified ones — that
  is the sampler being stochastic, not a determinism regression.
- **Never claim a decode win from a single ordered pair of runs, and generate
  enough tokens to see past the noise.** At 400-token generations the KV-ring
  A/B spread 2–6% and read as inconclusive; at 1500 tokens the spread collapsed
  to ~1% and the answer appeared — flat.
- **Measure RAM as well as tok/s, and on more than one model.** The KV ring
  looked neutral on throughput, then appeared to cost ~68 MiB of peak RSS on
  E2B — consistently, across six runs. On E4B that gap did not reproduce at all,
  which is what demoted it from "a real cost" to "an E2B artifact". A finding
  from one checkpoint is a hypothesis. Use
  [`scripts/mlx_resource_bench.py`](scripts/mlx_resource_bench.py) (CPU + GPU +
  RAM, no root needed); the record is
  [docs/mlx-resource-benchmark.md](docs/mlx-resource-benchmark.md).
- **E4B needs no code of its own.** `gemma-4-e4b-it-4bit` is the same dense
  Gemma-4 path as E2B (42 layers, hidden 2560, 2 KV heads, 18 KV-shared) and
  loads with zero unmatched keys — ~1.7× slower than E2B for ~1.5 GB more peak
  MLX memory.
- **TurboQuant 4-bit KV for Gemma-4 is rejected, not missing.** The `Exception`
  in `gemma4.rs` is a decision: measured against a windowed FP16 cache it is
  slower, saves ~82 MiB at 4 K, grows *larger* at long context, and fails
  quality (top-1 agreement −5.08 pp).
- **`gemma-4-26b-a4b` is implemented but never run.** Config parsing is tested;
  the forward pass, loader key matching and expert matmul shapes are unverified
  on real tensors (~14.3 GB, not downloaded).

Full record, including what transfers from
[drumih/turbo-fieldfare](https://github.com/drumih/turbo-fieldfare) and what
does not: [docs/gemma4-local-optimizations.md](docs/gemma4-local-optimizations.md).

## Scaffolding: `senclaw create`

`senclaw create app|skill|sub-agent <name>` renders a working project from a
template. Templates live in a git repo (`NortonBen/senclaw-templates`, cloned to
`~/.senclaw/templates/repo`) **and** are compiled into the binary from
`assets/templates/` — git wins when reachable, the bundled copy is what keeps the
command working offline. Four app languages: `rust` (default), `go`, `node`,
`python`. Engine in [`src/scaffold/`](src/scaffold/), CLI in
[`src/cli/commands/create.rs`](src/cli/commands/create.rs).

Rules for Claude:

- **The rendered project is validated before anything is written**, and the
  checks are the silent-failure ones: a misspelled `runtime.mode` (falls back to
  `session`, so a background poller quietly stops), a `runtime.kind` that is not
  exactly `"server"` or a missing `start` (the app installs and never launches),
  a wrong-typed field (`as_str`/`as_u64` read a wrong type exactly like an absent
  one, so `"port": "4800"` would reach the daemon as port 0), and a port above
  65535 (cast `as u16`, so 70000 becomes 4464). Adding a template means those
  checks must still pass — `cargo test` renders every bundled template and runs
  them ([`src/scaffold/create.rs`](src/scaffold/create.rs) `validate`).
- **The wildcard-bind rule runs on the template's own source, not the rendered
  output** (`check_bind_host`). After substitution a user's `--desc "never binds
  0.0.0.0"` is indistinguishable from code. It catches the literal *and* the
  hostless forms that contain no literal — `server.listen(PORT)`, `":"+port`,
  `(("", PORT))`, bare `next start` — and strips comments first, since every
  template documents the rule. This is now the only in-repo enforcement of the
  bind-loopback rule for Space App code, since `apps/` moved to its own repo.
- **Adding a bundled template is dropping a directory into `assets/templates/`.**
  `build.rs` walks it into an `include_bytes!` table; there is no list to update.
- **The render syntax is `{{lower_snake}}` only.** Everything else in braces
  (`{{.Name}}`, `{{ item.title }}`, `{{#each}}`) passes through untouched, so a
  template may ship Go/Vue/handlebars syntax. An unknown `{{placeholder}}` is
  left verbatim and warned about, never blanked. **Substituted values are
  escaped for their destination** — JSON, a markdown file's YAML frontmatter
  (but not its prose), or HTML — because `--desc`/`--icon`/`--var` are arbitrary
  user text: unescaped, one can inject a second `id` key that serde prefers, or
  a second `name:` that the persona registry prefers.
- **A template that calls `/bridge` must send `action`, not `capability`**, and
  must treat a `200` carrying `{"status":"error"}` as a failure. Both are
  enforced by tests over the bundled templates
  ([`src/scaffold/bundled.rs`](src/scaffold/bundled.rs)) because both fail in a
  way that looks exactly like the app degrading gracefully with no daemon.
- **Only the id folds diacritics.** `"Quản lý Kho"` → id `quan-ly-kho`, display
  name still `Quản lý Kho`. Folding shares the table in
  [`src/security/replication.rs`](src/security/replication.rs) `fold`.
- Ports auto-pick from **4800** (bundled apps own 4300–4799), skipping ports
  declared by installed manifests *and* ports currently listening.
- `postCreate` steps are **printed, never executed**.

Full guide, including how to author a template for the repo:
[docs/senclaw-create.md](docs/senclaw-create.md).

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

### Managing Space Apps from chat (`space_app_*`)

`senclaw-space` carries five tools so an agent can do from a conversation what
Settings → Space Apps does: `space_app_list` (installed apps + running state,
filterable, `probe` for a real health check), `space_app_start`,
`space_app_stop`, `space_app_restart`, and `space_app_mcp_list` (which MCP
server each app registers, its status and tool count — the way to look up the
full `mcp__<mcpName>__<tool>` name).

They live in [`src/mcp/space_apps.rs`](src/mcp/space_apps.rs) and are **loopback
HTTP calls back into the daemon**, unlike the notes/calendar half of the same
server which opens the DB directly. The reason is not style: an app's process
lives in the daemon's `SpaceMcpLauncher` — a child-process map, a user-stopped
set, a launch counter, all in memory in *another* process. A second launcher in
the MCP subprocess would fight the first one for ports. `space_mcp_config` sets
`SENCLAW_SPACE_API_URL`; loopback peers are exempt from the daemon's API token,
and the app-token gate covers only an app's *data* routes, never `/start` and
`/stop` — so the local case needs no credential.

Rules for Claude:

- **`GET /api/space/apps/status` is a literal sibling of `:id`.** Adding a route
  like it means adding the segment to `app_auth::split_app_path`'s literal list,
  or it is parsed as an app named "status".
- **Never report a stopped `session` app as broken.** It is the resting state;
  only `background` apps are supervised. The tool descriptions say so because an
  agent that "fixes" it restarts something working as designed.
- **Stopping a `background` app stops whatever it was watching** (channel polls,
  schedules) until someone starts it again — confirm with the user first.
- **A client timeout on `space_app_start` is not a failure.** A first start can
  be minutes (`npm ci`, venv); the daemon keeps going after the client gives up,
  so the answer is "check again with `space_app_list`", never a retry.
- **The MCP registry is enrichment, not the answer.** `space_app_mcp_list` reads
  `mcpName` from the manifest and only *decorates* it with live status; when the
  registry is unreadable it degrades (`registered: null` + `degraded` note)
  rather than failing or claiming `registered: false`.

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

- **`SENCLAW_APP_TOKEN_MODE`** = `strict` (**default** — a tokenless call to an
  app's data route is refused unless the caller is the daemon's own UI) | `warn`
  (served + one log line per app, the way to find what would break) | `off`
  (served as before; the escape hatch for an app with a hand-rolled HTTP
  client). Strict only gates the app's *data* routes (`/bridge`, `/config`,
  `/sqlite/query`, `/mcp/register`, `/env`, `/token`) — never management routes
  or `/proxy`, `/static`. An unrecognised value falls back to `strict`, never to
  `off`: a typo must not silently disable app isolation.
- **Changeable from the UI** at Settings → Space Apps (web + desktop), via
  `GET`/`PUT /api/space/app-token-mode`. The choice lives in `router_state`
  (`space:appTokenMode`), **overrides** the env var, and needs no daemon restart
  — the middleware reads it per request. That route is deliberately not under
  `/api/space/apps/`: `app_auth` gates everything there per app id, which under
  strict would lock the operator out of the switch that turns strict off.
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
