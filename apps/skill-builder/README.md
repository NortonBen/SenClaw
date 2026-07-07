# Skill Builder · Lò Rèn Kỹ Năng

A SenClaw **App Space** that turns a plain-language requirement into a ready-to-install
SenClaw skill. You describe *what the skill is for* and *when it should run*; the app reads
the daemon's live capability inventory (installed skills, sub-agents, MCP servers/tools),
asks the daemon's LLM to design a new skill that **reuses existing tools** and **avoids
duplication**, and writes it back — with **auto-load triggers** in the frontmatter — via the
daemon's own skill API.

*Bạn nhập kỹ năng dùng để làm gì và khi nào nên chạy; AI phân tích danh sách skill /
sub-agent / MCP hiện có rồi soạn một SKILL.md hoàn chỉnh kèm triggers để tự động nạp.*

## How it works

```
requirement ──▶ /api/generate ──▶ SpaceClient.llm_request (daemon LLM)
                    │                        ▲
                    ▼                        │ grounded in
              draft SKILL.md          GET /api/skills, /api/subagents, /api/mcp-servers
                    │
   user reviews / edits triggers & body (web UI)
                    │
                    ▼
              /api/install ──▶ POST /api/skills/create  (triggers → frontmatter, auto-load)
```

The app never talks to an LLM provider directly — it goes through the SenClaw Space-App
bridge (`app-space-sdk::SpaceClient`). It reaches the daemon over `SENCLAW_BASE_URL`, which
the daemon injects when it launches the app.

## Endpoints

| Method | Path | Purpose |
|---|---|---|
| GET | `/api/status` | health check |
| GET | `/api/inventory` | trimmed skills + sub-agents + MCP servers/tools |
| GET | `/api/skills` | installed skills (daemon shape) |
| POST | `/api/generate` | `{requirement, when_to_run}` → draft skill JSON |
| POST | `/api/install` | `{name, description, content, triggers, overwrite}` → install |
| DELETE | `/api/skills/:name` | uninstall a skill |
| GET/POST | `/api/mcp/sse`, `/api/mcp/message` | MCP server transport |

## MCP server — `skill-builder-mcp`

So the agent can build its own skills mid-conversation:

- `skill_inventory` — list existing skills / sub-agents / MCP tools (call first).
- `skill_draft` — design a skill from a requirement (no install).
- `skill_create` — design **and** install in one step (writes triggers → auto-load).
- `skill_create_exact` — install a skill from exact fields you provide.
- `skill_list` — list installed skills.
- `skill_remove` — uninstall a skill.

Bundled skill `forge-skill` + persona `skill-forge-master` teach the agent the workflow.

## Develop

```bash
# backend (repo root)
cargo run -p skill-builder            # serves on :4370, needs the daemon on SENCLAW_BASE_URL

# web UI
cd apps/skill-builder/web && npm install && npm run dev   # proxies /api → :4370
```

`SENCLAW_BASE_URL` defaults to `http://127.0.0.1:18788` (the daemon UI port). Set `PORT` to
change the app's own port (default `4370`).

## Package

```bash
apps/skill-builder/scripts/pack.sh    # → skill-builder-app.zip (install in SenClaw)
```

## Note

Auto-load triggers rely on a small daemon change: `POST /api/skills/create` now accepts a
`triggers` array (and an `overwrite` flag) and writes them into the SKILL.md frontmatter, so
the trigger matcher in `zen_core::engine` can surface the skill on matching prompts.
