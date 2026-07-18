# SenClaw Mini Browser 🕶️

A real web browser packaged as a SenClaw **App Space** — Rust +
axum + `chromiumoxide` (CDP) driving a live Chromium, with **deep AI** (MCP,
skills, persona) and a live view the user and the AI **share**.

- **Real rendering** — drives an actual Chromium via the Chrome DevTools Protocol
  (not HTML scraping). The user watches a live JPEG stream and can click, scroll,
  type, and manage tabs.
- **Coherent identity** — drops Chrome's automation tells (`--enable-automation`,
  `AutomationControlled`) and otherwise lets the real browser be itself. It reads
  Chrome's genuine user-agent and client-hint metadata and republishes them
  unchanged, correcting only the `HeadlessChrome` branding when running without a
  window. There is deliberately **no** JS spoofing payload — a probe of a bare
  browser showed every property the old layer patched was already correct, and
  the patches contradicted each other. See [`src/stealth.rs`](src/stealth.rs) for
  the full reasoning, and [`src/input.rs`](src/input.rs) for the human-like mouse
  motion and per-key typing.
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
                                         ├ identity / UA override (stealth.rs)
                                         └ human-like input (input.rs)
                                                   │
                                            Chrome (headful by default)
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
  - `MB_HEADLESS=1` — force headless. The default is now a real window wherever
    the platform has a display, because headless is the only thing left that we
    have to misrepresent (Chrome brands itself `HeadlessChrome`). The UI streams
    screenshots either way, so a window costs nothing on a desktop.
  - `MB_HEADFUL=1` — force a window even without a detected display.
  - `MB_USER_AGENT` — override the user-agent. Rarely a good idea: the default is
    the browser's *real* UA, and a value that disagrees with the client-hint
    metadata is exactly the sort of contradiction that gets a browser flagged.
  - `MB_ACCEPT_LANGUAGE` — locale list (default `vi-VN,vi,en-US,en`). Plain
    locales only; Chrome appends the `q=` weights itself.
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

# Live tests launch Chrome against the shared profile, so run them serially:
cargo test -p mini-browser -- --ignored --test-threads=1
#   identity_smoke           — asserts the browser presents one coherent identity
#   google_serves_signin_form — asserts Google serves the sign-in form, not the
#                               "browser may not be secure" rejection
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
- Presenting a coherent identity is not a bypass: advanced anti-bot systems
  (Cloudflare Turnstile, DataDome) and Google's sign-in can still refuse a
  CDP-driven browser, and no amount of fingerprint tidying changes that.
- Use responsibly: the `web-operator` persona confirms before sensitive actions and
  respects site terms / rate limits.
