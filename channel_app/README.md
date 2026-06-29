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

## Surfaces (bottom nav)

- **Chat** — interact with the bound agent (history, tools, permissions, plans).
- **Code** — drive remote coding sessions.
- **Space** — notes, calendar, schedules, email, apps.
- **Cowork** — DAG teams, tasks, members.
- **Khác** — quick dashboard, connection/theme/language controls, and the
  interaction surfaces (Wiki, Cognitive memory, Plugins).

## Run

```bash
flutter pub get
flutter run          # device/emulator
flutter analyze      # static analysis
flutter build apk    # or: flutter build ios / macos
```

Pair by scanning the QR shown in the daemon's Channels UI on first launch.
