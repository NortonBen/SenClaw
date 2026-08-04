# SenClaw Mini Browser 🕶️

A real web browser packaged as a SenClaw **App Space** — Rust + axum +
`chromiumoxide` (CDP) driving a live Chromium, with **deep AI** (MCP, skills,
persona) and a live view the user and the AI **share**.

- **Runs out of sight** — Chromium starts with no window; the page appears only in
  the app's live view, streamed over CDP screencast. The user can click, scroll,
  type, and manage tabs there. `MB_HEADFUL=1` puts the real window back.
- **Real rendering** — an actual Chromium via the Chrome DevTools Protocol, not
  HTML scraping.
- **Coherent identity** — drops Chrome's automation tells and otherwise lets the
  real browser be itself. There is deliberately **no** JS spoofing payload; see
  [Identity](#identity) for what that means and where the limits are.
- **User ≡ AI** — both the live-view input and the AI's MCP actions flow into the
  **same page / same CDP session** via `Input.*` events, so a site cannot tell an
  AI action from a person's.
- **The AI sees what is actually there** — the page is handed to the model as
  Chrome's own accessibility tree, including inside iframes and shadow DOM.
- **Deep AI** — 35 MCP tools including `browser_act` (autonomous, self-verifying
  observe→decide→act loop) and `browser_extract` (page-grounded Q&A / structured
  extraction), plus a chat + Act side panel. All LLM calls go through the SenClaw
  daemon via `app-space-sdk` (no provider keys in the app).
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
                                         ├ accessibility snapshot (snapshot.rs)
                                         ├ console / network / dialogs (events.rs)
                                         ├ identity / UA override (stealth.rs)
                                         └ human-like input (input.rs)
                                                   │
                                            Chrome (headful by default)
```

Modules: [`main.rs`](src/main.rs) (launch + serve), [`session.rs`](src/session.rs)
(the shared browsing surface, tabs, actions, frame stitching),
[`snapshot.rs`](src/snapshot.rs) (accessibility tree → model-facing text + refs),
[`events.rs`](src/events.rs) (console, network, dialogs, downloads),
[`stealth.rs`](src/stealth.rs), [`input.rs`](src/input.rs),
[`mcp.rs`](src/mcp.rs), [`llm.rs`](src/llm.rs) (chat / act / extract),
[`api.rs`](src/api.rs), [`db.rs`](src/db.rs) (history + bookmarks).

## How the AI sees a page

`browser_snapshot` returns Chrome's own accessibility tree, rendered in the shape
Playwright's MCP server uses — the page format current models have seen most of:

```text
- heading "Probe" [level=1] [ref=e2]
- textbox "Username" [ref=e4]
- checkbox "Remember me" [checked] [ref=e6]
- combobox [ref=e9]:
  - option "One" [ref=e10]
  - option "Two" [selected] [ref=e11]
- button "Sign in" [ref=e12]
- button "Nope" [disabled] [ref=e13]
- link "Learn more" [ref=e18]:
  - /url: https://example.com/x
- iframe [ref=e19]:
  - button "Inside frame" [ref=e20]
```

Three properties of this are load-bearing:

- **Nothing is written to the page.** The previous implementation stamped
  `data-mb-idx` attributes onto every interactive element — visible to the site's
  own scripts, and capable of tripping attribute selectors and mutation
  observers on a profile the user is signed into. Reading the accessibility tree
  touches nothing.
- **Refs are stable.** A ref is bound to the element's CDP `backendNodeId`, so the
  same button keeps `e12` across re-renders and a ref from three turns ago still
  resolves. Naive 1..n numbering shifts when anything re-renders, which turns a
  stale ref into a *wrong click* rather than a failed one. Navigation invalidates
  every ref, deliberately and loudly.
- **`*` marks what is new.** After an action, the elements that appeared since the
  last snapshot carry a leading `*` — a free diff, so the model can see what its
  click actually did.

Two things an accessibility tree structurally cannot tell you are filled in from
CDP, still without touching the page:

- **Elements the page styles as clickable.** A `<div onclick>` with no role and no
  ARIA is not an accessibility object — Chrome reports it as `generic`, or ignores
  it — yet a great deal of application UI is built exactly that way. The agent
  would read the label and never learn it was a target.
  `DOM.getNodesForSubtreeByStyle` asks Chrome which elements compute to an
  interactive cursor (the same cue a sighted person acts on, piercing shadow
  roots), and those get promoted to `clickable` with a label taken from their own
  text. The in-page libraries that pioneered this heuristic have to inject a
  script to get the same answer.
- **Where the viewport sits.** The tree describes the whole document with no hint
  of what is on screen, so a long page reads exactly like a short one and the
  agent either stops early or scrolls forever. Every snapshot now carries
  `Viewport 713px of a 5210px page — 0.0 pages above, 6.3 below, at 0%` and
  brackets the tree with `[start of page]` / `[more below — scroll down]`.

Cost of both, measured: about 110–140 ms per snapshot on a heavy real-world page
(vnexpress.net, 5210px tall), ~10 ms on a light one.

Clicks resolve through `DOM.getContentQuads`, which reports coordinates the
browser computed itself. That is what makes clicking an element **inside an
iframe** land correctly; the old `getBoundingClientRect` maths returned
frame-relative coordinates and missed.

## MCP tools (`mini-browser-mcp`)

**Observing** — `browser_snapshot`, `browser_find`, `browser_screenshot`,
`browser_get_info`, `browser_extract_text`, `browser_extract_links`

**Navigating** — `browser_navigate`, `browser_back`, `browser_forward`,
`browser_reload`

**Acting** — `browser_click`, `browser_type`, `browser_fill_form`,
`browser_select_option`, `browser_hover`, `browser_drag`, `browser_press_key`,
`browser_scroll`, `browser_scroll_to`, `browser_highlight`

**Modals & files** — `browser_handle_dialog`, `browser_file_upload`,
`browser_downloads`

**Waiting** — `browser_wait_for`

**Diagnostics** — `browser_console_messages`, `browser_network_requests`

**Tabs & environment** — `browser_new_tab`, `browser_list_tabs`,
`browser_switch_tab`, `browser_close_tab`, `browser_resize`, `browser_execute_js`

**Handing over** — `browser_request_login`

**AI** — `browser_act` (plan → execute → verify → replan, see below),
`browser_extract`

Every action tool answers with the state it produced — url and title, any blocking
dialog, new console errors, the tab list when there is more than one, and a fresh
snapshot. Without that the model either snapshots after every action (doubling the
round-trips) or acts on a stale picture of the page.

Two behaviours are worth knowing about:

- **A JavaScript dialog gates everything.** `alert`/`confirm`/`prompt` suspends the
  renderer, so every other tool refuses with an explanation until
  `browser_handle_dialog` clears it. An unanswered dialog is auto-dismissed after
  30 seconds rather than leaving the browser wedged.
- **`browser_act` runs to completion, or says it did not.** See below.

## The preview

With no window, the live view is the browser as far as the user is concerned, so
it streams over CDP **screencast** rather than a screenshot timer. Chrome pushes a
frame when the page actually composites something: measurably smoother while
things move (~10 fps vs ~3) and free while they do not, where a timer pays to
encode a JPEG several times a second to redraw a page that has not changed.
Metadata — url, title, viewport, any blocking dialog — rides a slower ticker,
because reading the title is a call into the renderer and it does not change ten
times a second. If screencast cannot start, the pump falls back to polling.

A screencast only emits on compositor commits, which makes a static page look
like a broken stream — worth knowing before debugging one. `browser_screenshot`
still takes ordinary on-demand captures.

## How a request gets done

A request is not a sequence of clicks, it is a goal, and the engine is built
around that distinction:

1. **Plan** — a short ordered list of steps in plain language ("open the first
   article", "read the price from the table"), not individual clicks.
2. **Execute** — each step gets its own small observe-decide-act loop, which can
   batch several actions per turn and stops as soon as *that step* is done. What
   the step read is carried forward; it is the only thing later steps see.
3. **Verify** — a separate model call re-reads the page and decides whether the
   goal was met. Agents routinely declare success on a search-results page, or
   having opened one item when asked for four, so the acting model does not get
   the final word.
4. **Replan** — if the check fails, plan again knowing what was tried and why it
   was rejected. Up to `max_plans` (default 10, hard-capped at 10, settable in
   the Act panel).

The budget is a safety rail, not a tuning knob: without it a goal the page cannot
satisfy becomes an unbounded spend of model calls and clicks on the user's real
logged-in browser.

Chat, the Act panel and `browser_act` are all the same engine, the same replan
budget and the same transcript. **Chat decides**: a question is answered from the
page, a request to *do* something becomes a run, and the reply reports what
actually happened. It never answers with an action to be run later — that was the
original bug, where asking it to open four pages produced
`{"action":"click","element_id":"e73"}` printed as a chat message, twice.

Every run is recorded: the plan, each step, each action, the check's verdict.
The assistant message that reports a run carries that run's id, which is the link
the Act panel follows.

## Signing in — the user does it, not the AI

Some things must not be automated, and signing in is the clearest. The agent
never types a username, password, one-time code or recovery code, and never asks
for one in chat. When a task needs an account it calls `browser_request_login`
and stops.

That opens the **real Chrome window** and puts the person in control. It has to
be a real window rather than the live view: a password manager, a passkey prompt
and a hardware key all live in browser and OS chrome that no screencast can show
and no synthetic `Input` event can reach. Chrome decides at launch whether it has
a window and no CDP command changes its mind, so this relaunches against the same
profile — which is also what carries the sign-in into every later automated run.

While the user holds control, **the agent can neither act on the page nor read
it** — clicking, typing, screenshots, `execute_js` and text extraction all refuse;
only "what URL are we on" stays available, because the UI needs it. The
**preview stream stops** as well, so the page you type your password into is not
being encoded and broadcast. All of that is enforced in `session.rs`, not asked
for in a prompt: a model that decides to try anyway gets an error.

The first cut of this got it wrong in a way worth recording — it gated clicking
and typing but left `execute_js` open, so an agent that had just handed over
could have read the password field straight out of the DOM. "The AI never sees
your credentials" has to cover reading, or it is not a claim worth making.

**A takeover ends by itself if you forget it.** It carries a fifteen-minute
deadline that the app refreshes while the banner is on screen, so a slow sign-in
— finding a phone, waiting for an SMS — is never cut short, but closing the app
mid-handover does not leave the agent locked out of its own browser until you
restart it. A watchdog puts the browser back and says so.

**What gets typed into a credential field is never written down.** The run log
lives in SQLite, is shown in the Act panel and is fed back into later prompts, so
anything recorded there long outlives its usefulness. Fields are recognised as
credential-shaped by more than `type=password`: a one-time code, a CVV and a PIN
are all plain text inputs, and a "show password" toggle turns a password field
into one. Those report `typed (6 chars, hidden)` and nothing else. Ordinary
typing is still logged in full, because masking everything would make the log
useless.

Two things it deliberately does *not* do. It does not hide the window instead of
relaunching: measured on macOS, a minimised window yields 1 screencast frame
against 32 for a visible one — the window server stops compositing and the
preview goes black, so the tidier-looking design would leave the user watching
nothing. And it does not try to work around Google's block; that is
[published policy about automation control](#identity), not a fingerprint check.

### Risks, per platform

Say these out loud before signing anything in; they are not equal.

- **X / Twitter — the one where you can lose the account.** X's automation rules
  prohibit scripting the website and give permanent suspension as the penalty.
  Treat this as opt-in with your eyes open, or don't use it here.
- **Facebook / Instagram / Threads.** Meta's terms prohibit automated collection
  *explicitly including while logged in* — the case is named, not an oversight.
  The realistic outcome is checkpoints that stall the agent; the tail is a
  disabled account needing ID verification to recover.
- **Google.** Signing in *through* automation is blocked by published policy,
  which is exactly why you do it yourself in a real window. Afterwards it mostly
  works, with occasional CAPTCHA walls and sessions that expire early. Account
  termination is not the documented response and there is no known case of it for
  personal automation — but the blast radius if it happened is large and the
  appeal process is automated.
- **The profile directory is a full credential.** Anyone who copies
  `~/.senclaw/space-apps/mini-browser/profile` is signed in as you, with no
  password and no 2FA prompt. This app deliberately does *not* pass Chrome's
  `--use-mock-keychain` / `--password-store=basic` flags, which the usual
  automation flag set includes and which would encrypt those cookies with a
  publicly known key.
- **A secondary account is the cheapest mitigation there is**, and costs nothing.

A human-initiated login is materially safer than an automated one — no credential
passes through the model and the hardest gate is cleared by a real person — but
it buys detection risk, not permission. Post-login behavioural analysis does not
care who typed the password.

## What it remembers

After a run passes its check, the agent writes down what would have made that run
shorter had it known it at the start — *"on this site pressing Enter in the search
box does not submit; click the Search button"* — filed under the **host**. Those
notes go into the planner the next time it works on that site.

Four things keep this from becoming a liability:

- **Only verified runs teach.** A run that did not work is precisely the wrong
  thing to learn from: its steps are unproven at best and the reason it failed at
  worst. (Replications of the published systems found roughly half their "learned"
  workflows had been induced from failed trajectories.)
- **Notes earn their place.** Each is credited when a run that was shown it
  succeeds and debited when one fails; advice that keeps losing stops being
  offered, and can earn its way back. That is the whole staleness mechanism — a
  note about a stable site should never expire, and one about a redesigned page
  should die immediately, and a timer knows neither.
- **Notes are filtered mechanically, not by asking the model nicely.** A note is
  distilled from a transcript derived from page content and then injected as
  trusted guidance on every later visit — a laundering path straight from a
  hostile page into a browser holding your logged-in sessions. Notes naming
  another host, mentioning credentials or money movement, or addressing the agent
  imperatively ("ignore…", "your real task is…") are refused outright.
- **You can see and delete every one of them**, in Settings.

Retrieval is by host and nothing else, deliberately: the sites recur, the corpus
is small, and the strongest published result in this area (Agent Workflow Memory
on WebArena) does exactly this with no similarity search at all.

## Identity

The browser presents itself accurately rather than pretending to be something
else. It removes Chrome's automation tells (`--enable-automation`,
`AutomationControlled`), reads Chrome's genuine user-agent and client-hint
metadata and republishes them unchanged, and corrects only the `HeadlessChrome`
branding when running without a window. `--accept-lang` sets the HTTP header and
`navigator.languages` from one source so the two cannot drift.

There is **no** JS spoofing payload, and adding one would make things worse: a
probe of a bare browser showed every property the old layer patched was already
correct, and the patches contradicted each other — a fabricated Windows/Direct3D
GPU behind a macOS user-agent, among others. See [`src/stealth.rs`](src/stealth.rs)
for the full reasoning.

The browser also runs with **no viewport emulation**. Passing a viewport makes
chromiumoxide issue `Emulation.setTouchEmulationEnabled(true)` — hardcoded,
regardless of the `has_touch` you ask for — which had this browser reporting a
touch-capable Mac, and pinned `screenOrientation` to an angle no desktop reports.
Those cross-attribute impossibilities are the strongest published signal for
spotting an evasive browser, so the fix is to stop emulating rather than to
emulate more carefully. `identity_smoke` asserts all of this against a real Chrome.

Input is human-like where the evidence says it matters: real mouse movement from
the pointer's actual previous position (not a teleport), wheel events as a
decaying train rather than one jump, and typing with measured dwell times and
key overlap — real typists press the next key before releasing the last, so about
a quarter of keystrokes have a negative flight time, and a strictly serialized
key stream has none. The mouse path is a plain eased curve on purpose: the
controlled study on this found elaborate Bezier "humanizers" made no significant
difference (p = 0.57) and may score slightly worse, because a mathematically ideal
curve is itself a signature.

**Known limitation.** chromiumoxide enables the CDP `Runtime` domain during
startup, which a page can detect. Avoiding it means forking the crate; the other
signals here are genuine (real TLS, real HTTP/2, real GPU, real profile, a real
human in the session), so expect occasional friction rather than a wall.

## Requirements

- **Google Chrome / Chromium installed** on the host (driven via CDP). Override the
  executable with `MB_CHROME=/path/to/chrome`.
- Environment:
  - `PORT` — HTTP port (default `4360`).
  - `MB_HEADFUL=1` — show the real Chrome window. The default is no window: the
    browser belongs inside the app, and the live view already shows the page. The
    cost is the one thing that then has to be corrected rather than passed through
    — Chrome brands a windowless build `HeadlessChrome` in its user-agent and
    client hints — which `stealth.rs` rewrites and `identity_smoke` guards.
  - `MB_HEADLESS=0` — equivalent to `MB_HEADFUL=1`.
  - `MB_USER_AGENT` — override the user-agent. Rarely a good idea: the default is
    the browser's *real* UA, and a value that disagrees with the client-hint
    metadata is exactly the sort of contradiction that gets a browser flagged.
  - `MB_ACCEPT_LANGUAGE` — locale list (default `vi-VN,vi,en-US,en`). Plain
    locales only; Chrome appends the `q=` weights itself.
  - `SENCLAW_BASE_URL` / `SENCLAW_SPACE_APP_ID` — injected by the daemon for LLM calls.

Downloads are accepted automatically and written to
`~/.senclaw/mini-browser/downloads`; `browser_downloads` lists them.

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
```

The live tests are the ones that matter, because the parts they cover were built
on assumptions about what Chrome returns:

- `snapshot_sees_the_whole_page` — roles, states, link targets, **iframe content
  and shadow DOM** all survive into the snapshot, and hidden text does not.
- `clicks_land_on_the_right_element` — including an element nested inside an
  iframe.
- `a_ref_from_a_previous_page_is_refused` — the safety property the ref design
  exists for. This failure was observed live: after a click navigated, stale refs
  still resolved onto backend node ids Chrome had recycled, so the agent clicked an
  unrelated element and reported success.
- `typing_fires_key_events` — text arrives *and* real `keydown`/`keyup` fire, which
  is what search-as-you-type and form validation listen for.
- `a_dialog_blocks_and_can_be_answered` — a `confirm()` is noticed, gates the other
  tools, and can be cleared.
- `preview_streams_with_no_window` — frames really do flow in the mode the app
  ships in.
- `the_user_can_take_the_browser_over` — the handover opens a real window, the
  agent can neither act nor *read* while it is up, and the profile survives the
  relaunch, which is what carries the sign-in forward.
- `an_abandoned_takeover_gives_the_browser_back` — the deadline lapses and the
  watchdog restores normal service.
- `secrets_do_not_reach_the_transcript` — a password and a one-time code are
  masked; an ordinary field is not.
- `can_a_window_be_hidden_instead` — keeps the measurement that says minimising
  kills the preview, because the idea is attractive enough to be re-proposed.
- `styled_divs_are_actionable` — a `<div onclick style="cursor:pointer">` is
  surfaced with a ref, labelled from its text, and actually clicks.
- `scroll_position_reaches_the_model` — a long page says so, and says so
  differently once scrolled.
- `identity_smoke` — one coherent identity: no headless tells, client hints present
  and agreeing with the UA, a GPU belonging to the claimed platform, no phantom
  touchscreen, and `navigator.languages` matching the `Accept-Language` header.
- `google_serves_signin_form` — Google serves the sign-in form rather than the
  "browser may not be secure" rejection.

For a manual check, navigate to `bot-detector.rebrowser.net` (the only public test
of CDP-attach leaks) or `abrahamjuliot.github.io/creepjs` — on CreepJS the target is
`lies: 0`, i.e. a canary against anyone re-introducing JS patches, not a score to
optimize. `bot.sannysoft.com` tests pre-2022 headless artifacts and an all-green
result there means very little.

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
  (Cloudflare Turnstile, DataDome) can still refuse a CDP-driven browser, and no
  amount of fingerprint tidying changes that. Two things are unfixable from a
  CDP client at all: synthetic mouse events report `screenX == clientX`, and
  `Page.navigate` cannot produce the `Sec-Fetch-User: ?1` that a real user gesture
  does.
- Cross-origin iframes live in another process and answer accessibility queries
  separately; they appear in the snapshot as a leaf rather than with their
  contents. Same-origin and `srcdoc` frames are fully traversed.
- Use responsibly: the `web-operator` persona confirms before sensitive actions and
  respects site terms / rate limits.
