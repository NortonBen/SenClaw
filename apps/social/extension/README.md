# SenClaw Social — Chrome extension (WXT + TypeScript)

Shared multi-host bridge that lets the `social` Space App operate your
Facebook / TikTok / X / Instagram / YouTube accounts through **your own logged-in
Chrome session** (not a headless browser — that's the point: the fingerprint is a
real Chrome, so it's the least-divergent option, though nothing guarantees you
won't be flagged).

Built with [WXT](https://wxt.dev) (Vite MV3) — headless (no popup UI): everything
runs in the background service worker + content scripts.

## Build & install (unpacked, dev)

```bash
cd apps/social/extension
npm install
npm run build        # → dist/chrome-mv3/   (npm run dev for HMR)
```

1. Build/run the `social` app so the bridge is listening (default `ws://127.0.0.1:9224`).
2. `chrome://extensions` → enable **Developer mode** → **Load unpacked** → select `dist/chrome-mv3/`.
3. Log into the platforms you want to use, in the same Chrome profile.
4. In the app, `social_ext_status` / the Settings page should show `connected: true`
   and the platforms with a live session under `hosts_ready`.

## How it works

- `src/entrypoints/background.ts` — WS client to the app bridge; token capture via
  `webRequest.onBeforeSendHeaders`; command handler for `ReplayApi` / `OpenLogin`
  / `WhoAmI` / `Ping`; caches page-VM tokens forwarded from the MAIN-world script;
  15s heartbeat reporting hosts with a session **+ each adapter's capability map**.
  **Captured tokens stay in the extension — only presence is reported to the app.**
  It derives all site-specific data from the adapter registry.
- `src/adapters/*.ts` — the **PlatformAdapter registry** (`base.ts` + `types.ts` +
  one file per platform: x, facebook, instagram, threads, tiktok, youtube;
  aggregated in `index.ts`). Each declares `hosts`, `sessionCookie`,
  `captureHeaders`, `sign` (`none|meta|tiktok`), `loginUrl`, an async `whoami`,
  and a per-capability **strategy** (`official|replay|page-sign|dom|none`).
- `src/entrypoints/metasign.content.ts` — **MAIN-world** reader for Facebook. Reads
  the page's own module system (`require('CurrentUserInitialData')` → real name +
  id, `require('DTSGInitialData')`/`require('LSD')` → `fb_dtsg`/`lsd`, derives
  `jazoest`) and posts them to the window.
- `src/entrypoints/relay.content.ts` — ISOLATED-world relay; forwards those tokens
  to the service worker, which caches them so `WhoAmI` returns the real FB name/id
  and signed replays can carry `fb_dtsg`.

## Ports

- App bridge WS: `9224` (override in the extension via `chrome.storage.local.ws_url`).
- App HTTP: `4520` (callback fallback `POST /api/ext/callback` with the handshake secret).

## What is / isn't wired

| Piece | State |
|---|---|
| WS connect + reconnect + heartbeat + hosts_ready | ✅ done |
| Token capture (Authorization / CSRF headers) | ✅ done |
| `ReplayApi` with an explicit `url` | ✅ done (credentialed fetch) |
| `OpenLogin` — open the platform login tab in Chrome | ✅ done (per-adapter `loginUrl`) |
| `WhoAmI` — confirm session + best-effort identity | ✅ done (real name+id on Facebook via page tokens; real handle on x/ig; id-only on threads; session-only on tiktok/youtube) |
| Facebook page tokens (fb_dtsg / lsd / jazoest + identity) | ✅ done (MAIN-world `metasign` → relay → cache) |
| TikTok / X signed endpoints (search/feed/DM) | ⏳ signer TODO (page-VM RE, rotating) |

The **Tài khoản** page drives the login flow: pick a platform → *Mở đăng nhập*
(`POST /api/ext/login` → `OpenLogin`) opens the real login page; the app then
polls *Lấy thông tin* (`POST /api/ext/whoami` → `WhoAmI`) until the session is
live and prefills handle/name for the operator to confirm and save. The app
never sees credentials — only which host is logged in and the public identity.

Follow the build order in `docs/social-app-extension-design.md`.
