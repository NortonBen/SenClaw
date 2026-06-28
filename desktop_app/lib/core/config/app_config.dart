/// App version shown in the nav rail. Keep in sync with `pubspec.yaml`.
const String kAppVersion = '1.0.0';

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

  const AppConfig({
    required this.host,
    required this.uiPort,
    required this.wsPort,
    this.wsToken,
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

  AppConfig copyWith({int? wsPort, String? wsToken}) => AppConfig(
    host: host,
    uiPort: uiPort,
    wsPort: wsPort ?? this.wsPort,
    wsToken: wsToken ?? this.wsToken,
  );
}
