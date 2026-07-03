# SenClaw Desktop

Native **Flutter** desktop app for [SenClaw](../README.md) (macOS / Windows /
Linux / web). It replaces the former Tauri shell: instead of embedding a
WebView it talks to the daemon directly over HTTP/WebSocket and **supervises
the `senclaw` daemon as a child process** — spawns the bundled binary from
`Contents/Resources/senclaw`, streams its logs, and restarts it on demand
(see `lib/core/daemon/daemon_supervisor.dart`).

On launch a **startup gate** (`lib/core/daemon/startup_gate.dart`) decides the
path: a daemon already listening on the UI port is adopted and the main UI
opens immediately; otherwise the app spawns the bundled daemon, shows a
"Starting daemon" screen until the HTTP API answers, then switches to the
main screen. Failures land on a retryable error screen with the daemon log
tail.

## Build & run

```bash
# From the repo root — development (adopts a running daemon or spawns one)
make app-dev

# Production bundle: builds the daemon with the full Apple-Silicon feature
# set and bundles it into the .app (an Xcode build phase re-embeds the
# freshest target/release/senclaw on every build)
make app-build

# Install into /Applications and launch (macOS)
make app-install
```

Direct Flutter commands work too (`flutter run -d macos`,
`flutter build macos --release`) — the Xcode "Embed senclaw daemon" phase
copies `../../target/release/senclaw` into the bundle when it exists.

## Layout

- `lib/core/` — daemon supervisor + startup gate, HTTP/WS transport, config
- `lib/features/` — chat, dashboard, plugins, space, cowork, settings,
  cognitive memory, diagnostics, dock (terminal / files / todos)
- `lib/app/` — shell, router, tray + window management
- `macos/ windows/ linux/ web/` — platform runners

Ports default to `18788` (HTTP) / `18789` (WS); override with
`--dart-define=SENCLAW_UI_PORT=...` (see `lib/core/config/app_config.dart`).
