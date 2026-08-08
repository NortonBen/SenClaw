# `senclaw-sdk/` — publishable SDKs for writing Space Apps

Each directory here is an **independent package published to a public
registry**, so someone writing a Space App outside the monorepo runs
`npm install` / `pip install` / `go get` instead of cloning all of SenClaw.

| Directory | Package | Registry |
|---|---|---|
| [`senclaw-app-sdk/`](senclaw-app-sdk) | `@senclaw/space-sdk` | npm |
| [`senclaw-app-sdk-python/`](senclaw-app-sdk-python) | `senclaw-space-sdk` | PyPI |
| [`senclaw-app-sdk-go/`](senclaw-app-sdk-go) | `github.com/NortonBen/SenClaw/senclaw-sdk/senclaw-app-sdk-go` | Go modules (the repo itself) |

The **Rust** SDK stays at [`../app-space-sdk`](../app-space-sdk): it is a
workspace member of this repo, and outside repos point at it with a git
dependency rather than through crates.io — see
[docs/space-app-sdk-publish-guide.md](../docs/space-app-sdk-publish-guide.md).

All four speak the same manifest contract: lifecycle mode (`background` /
`session`), `requires`, `sandbox`, `runner` —
[docs/space-app-lifecycle.md](../docs/space-app-lifecycle.md).

> Tiếng Việt: [README-vn.md](README-vn.md).

## Parity with the Rust SDK

The Rust column is the reference — `app-space-sdk` is the most complete, because
most apps in `apps/*` are written in Rust. The others follow it.

| | Rust | Node | Python | Go |
|---|:--:|:--:|:--:|:--:|
| `llm.request` (system/prompt/maxTokens/**profile**) | ✅ | ✅ | ✅ | ✅ |
| Full `text` + `model` + `finish` + `usage` | `llm_request_usage` | `llmDetailed` | `llm_detailed` | `LLMDetailed` |
| `agent.run` (an agent with tools, multiple steps) | — ¹ | ✅ | ✅ | ✅ |
| `knowledge.save` / `.search` / `.recall` | ✅ | ✅ | ✅ | ✅ |
| `usage.report` (app holds its own provider key) | ✅ | ✅ | ✅ | ✅ |
| List / switch the active model | ✅ | ✅ | ✅ | ✅ |
| `capabilities` — ask the daemon what it can do | — ¹ | ✅ | ✅ | ✅ |
| The app's own config + SQLite | — ¹ | ✅ | ✅ | ✅ |
| Register an MCP server | — ¹ | ✅ | ✅ | ✅ |
| Built-in MCP server | — ² | `/mcp` | `McpServer` | `MCPServer` |
| Dispatch (poll/heartbeat/reclaim/finalize) | ✅ | `/dispatch` | `dispatch.py` | `/dispatch` |
| Manifest: types + validation + CLI | — ³ | `/lifecycle` + `senclaw-manifest` | `manifest.py` + `-m` | `manifest` + `cmd/senclaw-manifest` |
| `bind_host` / `PORT` / graceful stop | manual | `/lifecycle` | `serve()` | `Serve()` |
| Access token sent on every daemon call | ✅ | ✅ | ✅ | ✅ |
| Guard closing the app's own port to all but the daemon | `auth::require_app_token` | `requireAppToken` | `require_app_token=True` | `RequireAppToken` |

¹ A Rust app calls `POST /api/space/apps/<id>/bridge` directly with `reqwest` —
`SpaceClient::bridge_action` is private, so there is no public wrapper yet. Not
a missing capability, just an unwrapped one.
² A Rust app uses `rmcp` directly and needs no wrapper.
³ A Rust app's manifest is hand-written; the test
[`space_app_lifecycle_manifests.rs`](../tests/space_app_lifecycle_manifests.rs)
is what catches typos across the whole repo.

The Rust SDK's `events` / `fs` / `net` modules have **no equivalent and need
none**: they only reproduce Node's `EventEmitter`, `fs.readFile` and
`net.createServer` for Rust. Node, Python and Go already have them.

Every SDK carries its own runnable example app under `examples/` — installing it
with `register-local` gives a working app in the daemon, with nothing else to
build.

## The app's access token

Every SDK reads `SENCLAW_TOKEN_ACCESS_APP` — the token the daemon mints per
installed app — and sends it, plus `SENCLAW_API_VERSION`, on every daemon call.
That token is the app's identity: it is bound to one app id, and using it against
another is refused, which is what keeps one app out of another's settings,
database and AI bridge.

The reverse direction is opt-in per app: each SDK ships a guard that refuses any
request to the *app's own* port that did not come through the daemon. See the
per-SDK README, and
[docs/space-app-api-token.md](../docs/space-app-api-token.md) for the whole
picture including `SENCLAW_APP_TOKEN_MODE=strict`.

## Which one to pick

| | |
|---|---|
| **Rust** | The app lives in this monorepo under `apps/*`, or needs the fastest start and smallest footprint |
| **Node** | The app is mostly a web UI, or leans on an npm library |
| **Python** | The app leans on the Python ecosystem (ML, scraping, data). No dependencies means no install step at all |
| **Go** | A single static binary, no runtime to install on the user's machine — but there is **no install step** for a Go app, so it ships built or compiles in `start` ([details](senclaw-app-sdk-go#read-this-first-a-go-app-has-no-install-step)) |

## Publishing

```bash
# npm
cd senclaw-app-sdk && npm publish        # `prepare` rebuilds dist/ before packing

# PyPI
cd senclaw-app-sdk-python && python -m build && python -m twine upload dist/*

# Go — no registry step: a git tag is the release.
git tag senclaw-sdk/senclaw-app-sdk-go/v0.1.0 && git push origin --tags
```

npm and PyPI bump their version by hand in `package.json` / `pyproject.toml` —
there is no version-generation step, and a published version cannot be changed.
Go modules take the version from the tag, and the tag must carry the module's
subdirectory prefix or `go get` will not see it.
