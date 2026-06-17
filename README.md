<p align="center">
  <img src="docs/images/senclaw-logo.png" alt="SenClaw logo" width="200" />
</p>

<h1 align="center">SenClaw</h1>

<p align="center">
  <em>A general-purpose, open-source framework for personal AI agents.</em>
</p>

<p align="center">
  <a href="https://github.com/midea-ai/SenClaw/releases/latest"><img src="https://img.shields.io/github/v/release/midea-ai/SenClaw?label=release" alt="Latest Release" /></a>
  <a href="https://github.com/midea-ai/SenClaw/actions/workflows/desktop.yml"><img src="https://github.com/midea-ai/SenClaw/actions/workflows/desktop.yml/badge.svg" alt="Build" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-yellow.svg" alt="License: MIT" /></a>
  <a href="CONTRIBUTING.md"><img src="https://img.shields.io/badge/PRs-welcome-brightgreen.svg" alt="PRs Welcome" /></a>
</p>

<p align="center">
  <strong>English</strong> | <a href="./README.zh-CN.md">简体中文</a>
</p>

SenClaw is a general-purpose engineering harness for building personal AI agents. It provides the surrounding machinery — permissions, memory, scheduling, multi-agent orchestration, channel adapters, and a Web UI — that turns a raw LLM runtime into a usable personal AI system.

---

<p align="center">
  <img src="https://github.com/midea-ai/SenClaw/releases/download/v0.1.1-preview/SenClaw-demo.GIF" alt="SenClaw Demo" width="720" />
</p>

*SenClaw analyzed its own source code and generated the intro video above — powered by [frontend-slides](https://github.com/zarazhangrui/frontend-slides) and [remotion](https://github.com/remotion-dev/remotion) skills.* [Watch full demo video](https://midea-ai.github.io/SenClaw/assets/SenClaw-demo.mp4)

## Highlights

- **Three-layer context management** — Unifies working context, long-term memory retrieval, and per-agent persona partitioning into a single coherent model.
- **Human-in-the-Loop permissions** — `PermissionBridge` is a native harness primitive supporting both explicit user authorization for high-risk tool actions and agent-initiated clarification requests.
- **Four-layer plugin architecture** — MCP tools, subagents, skills, and hooks — each anchored to a distinct engineering concern, forming a principled extension surface.
- **DAG Teams** — A two-stage hybrid orchestration framework combining LLM-based dynamic task decomposition with deterministic DAG execution grounded in persistent agent personas.
- **Four-mode scheduled tasks** — Pure notification, pure script, pure agent, and hybrid script-plus-agent execution — matching mode to task complexity so token consumption stays proportional to reasoning work.
- **Agentic Wiki** — Transforms task outputs into structured, retrievable wiki entries indexed alongside agent memory, creating a compounding personal knowledge base that feeds back into future agent sessions.
- **Multi-channel & Web UI** — Telegram, Feishu (Lark), and QQ adapters out of the box, plus a WebSocket gateway and a React-based Web UI.
- **Space Apps** — Isolated micro-apps (SSH manager, Email, Google Workspace, Browser control) that register as MCP servers and appear as first-class tools inside the agent.
- **Local model support** — Native MLX inference (Gemma 4, Qwen, Mamba) and local TTS (ZipVoice / macOS) run entirely on-device.

---

## Quick Start

### Option A — Download pre-built binary

Download the latest release for your platform from [Releases](https://github.com/midea-ai/SenClaw/releases/latest), then:

```bash
# macOS / Linux
./senclaw

# Open Web UI
open http://127.0.0.1:18788
```

### Option B — Build from source (Rust)

```bash
# 1. Clone
git clone https://github.com/midea-ai/SenClaw.git
cd SenClaw

# 2. Build daemon + Web UI
cargo build --release
cd web && npm ci && npx vite build && cd ..

# 3. Configure (optional — channels)
cp .env.example .env
# Edit .env to enable Telegram / Feishu / QQ / WeChat.
# If left unset, SenClaw starts in Web UI–only mode.

# 4. Run
./target/release/senclaw
```

### Option C — Desktop app (Tauri)

Pre-built `.dmg` / `.exe` / `.AppImage` bundles are attached to each tagged release.  
To build locally:

```bash
cargo tauri build
```

---

> **Configure an LLM on first launch.** SenClaw starts without a built-in model. Open the Web UI → **Settings → LLM**, add a provider profile (OpenAI / Anthropic / DeepSeek / Qwen / …) with `baseURL`, `apiKey`, `modelName`. The profile is persisted to `~/.senclaw/config.json` — until at least one active profile exists, agent runs that call an LLM will fail.

---

## Documentation

| Document | Description |
|---|---|
| [Quick Start & Usage Guide](docs/QUICK_START.md) | Installation, configuration, CLI commands, runtime layout, MCP tools |
| [Architecture](docs/ARCHITECTURE.md) | Layer breakdown, startup sequence, data flow |
| [Space Apps](docs/workspace-feature-design.md) | How to build and register a Space App |
| [Memory](docs/memory.md) | FTS5 + vector hybrid memory, daily log, cognify pipeline |
| [Remote Access Guide](docs/REMOTE_ACCESS.md) | Expose the Web UI securely via reverse proxy (Nginx / Caddy) |
| [Contributing](CONTRIBUTING.md) | *Coming soon* |

---

## Project Structure

```
senclaw/
├── src/                    # Rust daemon (primary)
│   ├── agent/              # Agent lifecycle, bridges, permission routing
│   ├── channels/           # Telegram / Feishu / QQ adapters
│   ├── gateway/            # Group manager, message router, WebSocket + HTTP server
│   ├── mcp/                # MCP servers (admin, schedule, memory, dispatch, OCR, …)
│   ├── memory/             # FTS5 + vector hybrid search, cognify pipeline
│   ├── scheduler/          # Cron / interval / once task scheduler
│   ├── tts/                # Text-to-speech backends (ZipVoice, macOS)
│   ├── local_model/        # MLX inference (Gemma 4, Qwen, Mamba, OCR)
│   ├── wiki/               # Git-driven personal knowledge base
│   └── clawhub/            # ClaWHub skill marketplace integration
├── web/                    # React + Vite Web UI
├── src-tauri/              # Tauri desktop shell
├── apps/                   # Space Apps (ssh-manager, email, google-workspace, …)
├── skills/                 # Bundled skills
├── examples/               # Example Space Apps and SDK usage
└── docs/                   # Detailed documentation
```

---

## Contributing

Contributions are welcome. SenClaw exists to advance the shared engineering foundation for personal AI agents — issues, pull requests, and design discussions are all valuable. See [CONTRIBUTING.md](CONTRIBUTING.md) *(coming soon)* for guidelines.

---

## License

[MIT](LICENSE) © AIRC Sema Team

---

## About the Logo

The SenClaw logo depicts a horse with **claw-shaped wings** rising from its back. The imagery is inspired by the Chinese phrase *以梦为马* — *"to ride one's dreams as a horse"* — capturing the spirit of an AI harness that carries the user wherever their imagination leads. The name itself blends *Sema* (from *semantic*) and *Claw*.

---

## Acknowledgments

SenClaw integrates with the [ClaWHub](https://github.com/openclaw/clawhub) plugin marketplace and is inspired by [OpenClaw](https://github.com/openclaw/openclaw). Thanks to the broader open-source ecosystem this project depends on — including the [Model Context Protocol](https://modelcontextprotocol.io), [grammY](https://grammy.dev), and many others.

---

> SenClaw's ambition is not to define the final architecture of personal AI agents — it is to advance the shared engineering foundation on which better architectures can be built.
