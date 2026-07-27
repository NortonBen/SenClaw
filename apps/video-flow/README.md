# Video Flow — SenClaw Space App

**Flow Kit ported as a SenClaw Space App.** An AI video production factory: a
multi-agent DAG turns a raw concept or a finished screenplay into a complete
video — driving Google Flow (Veo3 video / Imagen images) through a Chrome
extension bridge. Rust + axum backend, React dashboard, all LLM calls routed
through the SenClaw daemon's shared LLM bridge (no provider keys of its own).

Projects → videos → scenes → characters, with reference images, per-scene
frame images, per-scene Veo3 clips (vertical + horizontal state fully
independent), 4K upscales and ffmpeg concat. Every sub-agent (director,
screenwriter, shot design, image/video gen, critic, …) has its own editable
prompt in `souls/`.

## Architecture

```
┌────────────────────┐   WS ws://127.0.0.1:9222   ┌───────────────────────┐
│  Chrome            │◄──────────────────────────►│  video-flow app :4460 │
│  extension/        │                            │  (axum)               │
│  (Video Flow bridge)│─ HTTP /api/ext/callback ─►│   ├─ DAG engine       │
│  labs.google tab:  │                            │   ├─ request worker   │
│  token + reCAPTCHA │                            │   ├─ REST /api/*      │
└────────────────────┘                            │   ├─ MCP /api/mcp/sse │
        ▲ calls aisandbox-pa.googleapis.com       │   └─ React UI (/)     │
        │ (Veo3 / Imagen)                         └──────────┬────────────┘
                                                             │ LLM bridge
                                                             ▼
                                                  ┌───────────────────────┐
                                                  │ SenClaw daemon :18788 │
                                                  │ (shared LLM + agents) │
                                                  └───────────────────────┘
```

- The **extension** runs in the browser because Google Flow needs the user's
  `ya29.*` OAuth token, per-call reCAPTCHA Enterprise solving, and requests
  originating from `labs.google`.
- The **app** never talks to an LLM provider directly — `src/llm.rs` bridges
  to the SenClaw daemon (`SENCLAW_BASE_URL`, default `http://127.0.0.1:18788`).
- Two pollers over SQLite: the **DAG engine** (multi-agent pipeline, 500ms
  tick, max 5 concurrent) and the **request worker** (video/upscale queue via
  the extension).

## Dev run

```bash
# backend (from the SemaClaw repo root; serves UI + API on :4460, ext WS :9222)
cargo run -p video-flow

# web UI dev server (hot reload)
cd apps/video-flow/web && npm install && npm run dev
```

The SenClaw daemon must be running (LLM bridge on :18788). Health check:
`curl http://127.0.0.1:4460/api/status`.

## Install as a Space App

- **Dev / from source:** register the app dir directly with the daemon
  (register-local) so it starts `./video-flow` per `senclaw-manifest.json`.
- **Packaged:** `apps/video-flow/scripts/pack.sh` builds web + release binary
  and stages a flat `release/` (binary, manifest, skills/, personas/, souls/,
  playbooks/, extension/, web_dist/), zipped as `video-flow-app.zip` — install
  that zip via SenClaw's install-zip flow. The manifest auto-registers the MCP
  server `video-flow-mcp` at `/api/mcp/sse`.

## Chrome extension setup (required for any image/video generation)

1. `chrome://extensions` → Developer mode → **Load unpacked** → select the
   app's `extension/` folder.
2. Open **labs.google** (Google Flow) and sign in — the extension captures the
   Bearer token and connects to the app WS on `:9222`.
3. Verify: the popup shows three green dots (app WS / app HTTP / Flow token),
   and `vf_status` (or `GET /api/status`) shows `extension_connected: true`.

If SenClaw assigns the app a port other than 4460, set it in the popup under
**Kết nối · cổng của app** — the ports live in `chrome.storage.local`, so no
rebuild is needed. See [EXTENSION_SETUP.md](EXTENSION_SETUP.md).

Planning/parsing stages (pure LLM) work without the extension; image, video
and upscale stages require it. Upscale additionally needs a Flow TIER_TWO
account.

## Environment variables

| Var | Default | Meaning |
|---|---|---|
| `PORT` | `4460` | App HTTP port (injected by the daemon) |
| `FLOWKIT_WS_PORT` | `9222` | Extension WebSocket port |
| `FLOWKIT_WORKER` | `1` | Enable the video/upscale request worker |
| `FLOWKIT_WORKER_POLL_SEC` | `5` | Worker queue poll interval |
| `FLOWKIT_WORKER_GEN_TIMEOUT_SEC` | `300` | Per-generation timeout |
| `FLOWKIT_WORKER_VIDEO_POLL_SEC` | `10` | Video render poll interval |
| `FLOWKIT_WORKER_VIDEO_POLL_TIMEOUT_SEC` | `420` | Video render poll timeout |
| `GOOGLE_FLOW_API` | `https://aisandbox-pa.googleapis.com` | Flow API base |
| `GOOGLE_API_KEY` | *(empty)* | Optional key; normally the extension token is used |
| `FLOWKIT_ORIENTATION` | `VERTICAL` | Default orientation when unspecified |
| `FLOWKIT_EXEC_ALLOWLIST` | `ffmpeg,ffprobe` | Commands the exec tool may run |
| `FLOWKIT_EXEC_TIMEOUT_SEC` | `300` | Exec tool timeout |
| `FLOWKIT_TOOL_HTTP_ALLOW_PRIVATE` | `0` | Allow http tools to hit private IPs |
| `FLOWKIT_DATA_DIR` | `~/.senclaw/space-app-data/<app_id>` | App data root — **outside** the install dir, since installing a zip wipes it |
| `FLOWKIT_DB_PATH` | `<data>/app.sqlite` | SQLite path |
| `FLOWKIT_MEDIA_DIR` | `<data>/media` | Downloaded/uploaded media |
| `FLOWKIT_SOULS_DIR` | `souls/` | Sub-agent prompt files |
| `FLOWKIT_SKILLS_DIR` | `playbooks/` | Internal prompt playbooks |
| `SENCLAW_BASE_URL` | `http://127.0.0.1:18788` | SenClaw daemon LLM bridge |
| `SENCLAW_SPACE_APP_ID` | `video-flow` | App id sent to the bridge |

Chi tiết cài extension + đổi cổng: [EXTENSION_SETUP.md](EXTENSION_SETUP.md).

## MCP tools (`video-flow-mcp`, full names `mcp__video-flow-mcp__vf_*`)

| Tool | What it does |
|---|---|
| `vf_project_create/list/get/update/delete` | Project CRUD; `get` includes videos (+scene counts) and linked characters with reference-image readiness |
| `vf_character_create/list/update` | Characters/locations/assets (single-outfit base look), optional project link |
| `vf_video_create/list` | Videos — where orientation (VERTICAL/HORIZONTAL) is fixed |
| `vf_scene_list/get/create/update/delete` | Scenes — compact progress list, full row, prompt edits (cascade-aware) |
| `vf_pipeline_create` | Create + start the DAG (`production` / `full` / `custom` modes) |
| `vf_pipeline_status` | Parent + per-task status, by pipeline_id or project_id |
| `vf_pipeline_control` | `start` / `pause` / `cancel` / `retry_task` |
| `vf_generate_image` | Reference image (character_id), scene frame (scene_id), or all missing refs (`all_refs`); spawns, returns immediately |
| `vf_generate_video` | Veo3 clip for a scene+orientation (needs completed image); async |
| `vf_upscale_video` | 4K upscale (needs completed video; TIER_TWO); async |
| `vf_generate_narration` | TTS voice-over per scene via SenClaw's TTS (no extension needed); async |
| `vf_media_localize` | Download still-remote Flow assets into local media and repoint the DB (Flow URLs expire) |
| `vf_tts_status` | Installed TTS models + active model/voice/language/speed |
| `vf_requests_status` | Generation queue: counts by status/type + recent rows with errors |
| `vf_agents_list` | The 17 built-ins + skill agents, with soul excerpts |
| `vf_soul_get` / `vf_soul_set` | Read / overwrite one sub-agent's system prompt |
| `vf_status` | Health: extension connected, worker, LLM profile, entity counts |

## Data location (survives reinstall)

Installing a Space App zip runs `remove_dir_all(<app_dir>)` before extracting,
so anything stored next to the binary is destroyed on every update. The DB and
downloaded media therefore live in **`~/.senclaw/space-app-data/<app_id>/`**
(override with `FLOWKIT_DATA_DIR`). On first boot the app migrates a DB left in
the old in-app-dir location — including the `-wal`/`-shm` sidecars and `media/`
— so upgrading from an older build keeps existing projects.

## Local media & request lifecycle

Google Flow serves generated assets from short-lived signed URLs, so every
successful generation is mirrored into `media/` immediately and the DB stores
`/api/media/{id}/file`. Downloads dedupe on `original_url`, and a download
failure never loses the asset — the remote URL is kept and the generation still
counts as done.

- Repair an older project whose thumbnails went blank:
  `POST /api/media/localize {"project_id": "..."}` or MCP `vf_media_localize`.
- A `request` row in PROCESSING cannot survive a restart, so at boot the app
  reconciles leftovers: if the asset it was producing exists the row becomes
  COMPLETED, otherwise FAILED with an explicit reason. Image requests are closed
  out by the image agent (video/upscale by the worker) — nothing stays spinning.

## Souls editing

Each pipeline stage's behaviour is its **soul** — a markdown file in `souls/`
(e.g. `director.md`, `image-gen.md`, `critic.md`), frontmatter stripped at
load. Edit via the UI, `PUT /api/agents/{type}/soul`, or the MCP pair
`vf_soul_get` / `vf_soul_set`; changes apply on the agent's next execution.
`playbooks/` holds the internal prompt playbooks used by DB-backed skill
agents (ReAct tool loop) — not to be confused with the SenClaw skills in
`skills/`.

## Credits

Port of [Flow Agent Video ("Flow Kit")](../../../flow-agent-video) — the
original Go backend, React dashboard and Chrome extension — onto the SenClaw
Space App runtime (shared daemon LLM, MCP, skills and personas).
