# channel_app — Senclaw Connect (mobile remote)

A Flutter **remote-control client** for a Senclaw daemon. It pairs to the daemon
over the encrypted **relay hub** (QR pairing) and tunnels every `/api/*` call and
chat event through that single relay connection — it never talks to the daemon
directly.

This app is **for controlling and interacting** with agents, not for
administering them. There is **no daemon configuration** here (LLM / embedding /
cognitive / permission settings live in the desktop/web admin UI). The only
client-side controls are connection (re-pair / disconnect), theme, and language.

## Design system

Shares the desktop app's **Ant Design (v5)** token system, adapted for mobile:

- `lib/theme/tokens.dart` — `AppTokens` (brand/semantic colors, spacing, radii)
  and the `AppColors` `ThemeExtension`, accessed via `context.colors`.
- `lib/theme/app_theme.dart` — `AppTheme.light()` / `AppTheme.dark()`.
- `lib/theme/theme_mode_provider.dart` — persisted light/dark/system toggle.

The app supports **light and dark** themes (toggle in the *Khác* tab). State is
managed with **Riverpod**; the relay transport (`RelayManager` / `RelayService`)
is exposed to the widget tree via `lib/core/relay_providers.dart`.

## Surfaces

Bottom nav:

- **Chat** — interact with the bound agent (history, tools, permissions, plans,
  and the **Workbench** button when the agent has built something).
- **Code** — drive remote coding sessions.
- **Space** — notes, calendar, schedules, email, apps.
- **Cowork** — DAG teams: tasks, chat transcript, members, settings.
- **Khác** — quick dashboard, connection/theme/language controls.

Drawer: Wiki, Knowledge, Plugins, **Patterns**, **Kits**, **Dispatch**,
**Usage**, Workflows, Background tasks.

### What is read-only, and why

Three surfaces here cannot be driven the way the desktop UI drives them, for
reasons in the daemon rather than in this app:

| Surface | Limit | Cause |
|---|---|---|
| **Dispatch** | polls, no live stream | `dispatch:update` goes to WebSocket *admin* clients only (`broadcast_to_admins`). This app polls the read-only `GET /api/dispatch` snapshot while the screen is open — the same pattern the Background-tasks screen uses for `bg:*`. |
| **Cowork team chat** | read-only | Reading works: a team is chat group `cowork:<id>` and `GET /api/chat/history` takes any jid. *Sending* is routed by `Channel::owns_jid`, which for the app channel matches only `app:<channelId>:user:<sender>`; making it claim `cowork:*` would change channel ownership daemon-wide, since every outbound loop takes the first channel that answers. Send from web/desktop. |
| **Patterns / Kits** | no file upload | `POST /api/patterns/import` and `POST /api/kits/install` take multipart or a raw body; the relay bridge carries JSON. Everything installable from a *source* — including the kits and pattern library compiled into the daemon — works. |

**Workbench** is not in that table: `WsGateway::broadcast` mirrors every `app:*`
jid into the app-channel event sink, and all four `notify_workbench_*` calls go
through it, so artifacts arrive live with no daemon change. But the daemon has
no endpoint that *lists* artifacts — they exist only as `workbench:new` events —
so `WorkbenchStore` is the source of truth on the device and caches to
`LocalCache`. A device shows the artifacts it received; it cannot backfill.

## Run

```bash
flutter pub get
flutter run          # device/emulator
flutter analyze      # static analysis
flutter test         # model/parse tests (no daemon needed)
flutter build apk    # or: flutter build ios / macos
```

Pair by scanning the QR shown in the daemon's Channels UI on first launch.
