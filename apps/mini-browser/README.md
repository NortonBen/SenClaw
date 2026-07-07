# SenClaw Mini Browser 🕶️

A real, **stealth** web browser packaged as a SenClaw **App Space** — Rust +
axum + `chromiumoxide` (CDP) driving a live Chromium, with **deep AI** (MCP,
skills, persona) and a live view the user and the AI **share**.

- **Real rendering** — drives an actual Chromium via the Chrome DevTools Protocol
  (not HTML scraping). The user watches a live JPEG stream and can click, scroll,
  type, and manage tabs.
- **Stealth / anti-bot** — drops Chrome's automation tells (`--enable-automation`,
  `AutomationControlled`, `IdleDetection`) and injects JS before every page to
  neutralize `navigator.webdriver`, fix `languages`/`plugins`/`permissions`, add a
  real `window.chrome`, and make the patches un-introspectable. Human-like mouse
  motion and per-key typing. See [`src/stealth.rs`](src/stealth.rs),
  [`src/input.rs`](src/input.rs).
- **User ≡ AI** — both the live-view input and the AI's MCP actions flow into the
  **same page / same CDP session** via `Input.*` events, so a site cannot tell an
  AI action from a person's.
- **Deep AI** — 19 MCP tools including `browser_act` (autonomous observe→decide→act
  loop) and `browser_extract` (page-grounded Q&A / structured extraction), plus a
  chat + Act side panel. All LLM calls go through the SenClaw daemon via
  `app-space-sdk` (no provider keys in the app).
- **Skills** — three trigger-rich skills route requests to the browser:
  `browse-web` (open / search / read / summarize / translate), `web-extract`
  (tables / lists / prices / contacts / links → JSON, compare pages), and
  `web-task` (log in, fill forms, buy, book, download, post — multi-step actions),
  plus the `web-operator` persona.

## Architecture

```
web (React live view + AI panel)  ─┐
MCP (mini-browser-mcp, /api/mcp/sse)─┤→ BrowserSession (one shared page)
REST (/api/*) + live-view WebSocket ─┘   ├ chromiumoxide (CDP)
                                         ├ stealth injector (stealth.rs)
                                         └ human-like input (input.rs)
                                                   │
                                            Chromium (headless=new / headful)
```

Modules: [`main.rs`](src/main.rs) (launch + serve), [`session.rs`](src/session.rs)
(the shared browsing surface + DOM extractor), [`stealth.rs`](src/stealth.rs),
[`input.rs`](src/input.rs), [`mcp.rs`](src/mcp.rs), [`llm.rs`](src/llm.rs)
(chat / act / extract), [`api.rs`](src/api.rs), [`db.rs`](src/db.rs)
(history + bookmarks).

## MCP tools (`mini-browser-mcp`)

`browser_navigate`, `browser_snapshot`, `browser_click`, `browser_type`,
`browser_press_key`, `browser_scroll`, `browser_back`, `browser_forward`,
`browser_reload`, `browser_get_info`, `browser_extract_text`,
`browser_extract_links`, `browser_execute_js`, `browser_new_tab`,
`browser_list_tabs`, `browser_switch_tab`, `browser_close_tab`,
`browser_act`, `browser_extract`.

## Requirements

- **Google Chrome / Chromium installed** on the host (driven via CDP). Override the
  executable with `MB_CHROME=/path/to/chrome`.
- Environment:
  - `PORT` — HTTP port (default `4360`).
  - `MB_HEADFUL=1` — run a headful window (least detectable; needs a display).
    Default is the new headless mode.
  - `MB_USER_AGENT` — override the spoofed user-agent.
  - `SENCLAW_BASE_URL` / `SENCLAW_SPACE_APP_ID` — injected by the daemon for LLM calls.

## Develop

```bash
# backend (from repo root)
cargo run -p mini-browser              # http://0.0.0.0:4360

# web UI (separate terminal, proxies /api → 4360)
cd apps/mini-browser/web && npm install && npm run dev
```

## Test

```bash
cargo test -p mini-browser                                  # pure-logic unit tests
cargo test -p mini-browser -- --ignored stealth_smoke       # live: launches Chrome,
                                                            # asserts bot signals are gone
```

For a manual anti-bot check, navigate to `bot.sannysoft.com`,
`arh.antoinevastel.com/bots/areyouheadless`, or `abrahamjuliot.github.io/creepjs`
and confirm no headless/webdriver flags.

## Package & install

```bash
apps/mini-browser/scripts/pack.sh      # builds web + release binary → mini-browser-app.zip
```

Install the resulting `mini-browser-app.zip` in SenClaw. The daemon launches the
binary with a `PORT`, health-checks `/api/status`, serves it in an iframe, and
auto-registers the MCP server at `/api/mcp/sse`.

## Limitations

- Depends on a Chromium binary (Rust has no native web engine) — not pure-Rust for
  rendering.
- Stealth greatly reduces detection but does not defeat advanced anti-bot systems
  (Cloudflare Turnstile, DataDome).
- Use responsibly: the `web-operator` persona confirms before sensitive actions and
  respects site terms / rate limits.
