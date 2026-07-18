/// Release version of this build, shown in the nav rail.
///
/// The git tag is the single source of version identity: `v0.3.0` → Cargo
/// `version = "0.3.0"` → this string. CI injects it in `.github/workflows/
/// desktop.yml` (`--dart-define=SENCLAW_VERSION=${TAG#v}`); anything built
/// outside a tag — including every local `flutter run` — stays `'dev'`.
///
/// Do NOT hard-code a number here. A literal that drifts above the real tag
/// makes the update check compare e.g. `1.0.0 > 0.2.0` and report "up to date"
/// forever, with no error to trace.
const String kAppVersion = String.fromEnvironment(
  'SENCLAW_VERSION',
  defaultValue: 'dev',
);

/// True for any build without a release version (local dev, CI branch builds).
/// The updater is disabled here — a dev build must never overwrite itself.
bool get kIsDevBuild => kAppVersion == 'dev';

/// Rust target triple this bundle was built for, e.g. `aarch64-apple-darwin`.
/// Keys the `assets` map in latest.json. Empty outside a CI release build.
///
/// Injected rather than detected at runtime: an arm64 build is the truth about
/// which asset fits, whereas probing the machine can disagree with it (an app
/// running under Rosetta sits on hardware that reports otherwise). CI already
/// knows the exact triple — it is `matrix.target`.
const String kBuildTarget = String.fromEnvironment(
  'SENCLAW_TARGET',
  defaultValue: '',
);

/// Connection configuration for the local SenClaw daemon.
///
/// The desktop/web build talks DIRECTLY to the daemon on localhost — no relay
/// hub, no encryption envelope (the daemon binds 127.0.0.1 only). Defaults
/// match `src/gateway/ui_server/core.rs` (UI 18788) and the WS gateway (18789).
///
/// Override at build time:
///   flutter run --dart-define=SENCLAW_HOST=127.0.0.1 \
///               --dart-define=SENCLAW_UI_PORT=18788
class AppConfig {
  final String host;
  final int uiPort;

  /// Discovered from `GET /api/config` ({ wsPort, token }). Falls back to
  /// [uiPort] + 1 (the daemon's default convention) until discovery runs.
  final int wsPort;

  /// Optional WS auth token from `/api/config`; sent as `{type:connect,token}`.
  final String? wsToken;

  /// Where the tray must write screen captures, from `/api/config`. Comes from
  /// the daemon rather than being derived here: `SENCLAW_SCREENSHOTS_DIR` can
  /// move it, and a tray writing elsewhere would 404 on every shot. Null until
  /// discovery runs, or against a daemon too old to report it.
  final String? screenshotsDir;

  const AppConfig({
    required this.host,
    required this.uiPort,
    required this.wsPort,
    this.wsToken,
    this.screenshotsDir,
  });

  /// Initial config from --dart-define (or sensible localhost defaults).
  factory AppConfig.fromEnvironment() {
    const host = String.fromEnvironment(
      'SENCLAW_HOST',
      defaultValue: '127.0.0.1',
    );
    const uiPort = int.fromEnvironment('SENCLAW_UI_PORT', defaultValue: 18788);
    const wsPort = int.fromEnvironment('SENCLAW_WS_PORT', defaultValue: 18789);
    return const AppConfig(host: host, uiPort: uiPort, wsPort: wsPort);
  }

  String get httpBase => 'http://$host:$uiPort';
  String get wsUrl => 'ws://$host:$wsPort/';

  AppConfig copyWith({int? wsPort, String? wsToken, String? screenshotsDir}) =>
      AppConfig(
        host: host,
        uiPort: uiPort,
        wsPort: wsPort ?? this.wsPort,
        wsToken: wsToken ?? this.wsToken,
        screenshotsDir: screenshotsDir ?? this.screenshotsDir,
      );
}
