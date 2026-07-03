import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../config/app_config.dart';
import '../daemon/daemon_provider.dart';
import 'api_client.dart';
import 'ws_client.dart';

/// Holds the live [AppConfig] (mutable after `/api/config` discovery).
final appConfigProvider = StateProvider<AppConfig>(
  (ref) => AppConfig.fromEnvironment(),
);

/// REST client. Recreated only if host/uiPort change (rare).
final apiClientProvider = Provider<ApiClient>((ref) {
  final cfg = ref.watch(appConfigProvider);
  final client = ApiClient(cfg);
  ref.onDispose(client.dispose);
  return client;
});

/// Persistent WebSocket. Kept alive for the app lifetime.
final wsClientProvider = Provider<WsClient>((ref) {
  final cfg = ref.read(appConfigProvider);
  final ws = WsClient(cfg);
  ref.onDispose(ws.dispose);
  return ws;
});

/// Live connection status for the WS gateway.
final wsStatusProvider = StreamProvider<WsStatus>((ref) async* {
  final ws = ref.watch(wsClientProvider);
  // The status stream is a broadcast controller that only emits transitions —
  // it does NOT replay. Seed subscribers with the CURRENT status, otherwise a
  // late watcher (e.g. the nav-rail connection dot) shows "Offline" even
  // though the socket connected long before it was built.
  yield ws.status;
  yield* ws.statusStream;
});

/// Broadcast of every decoded server event. Feature providers do
/// `ref.watch(wsEventsProvider).whereType(...)`.
final wsEventsProvider = StreamProvider<WsEvent>((ref) {
  final ws = ref.watch(wsClientProvider);
  return ws.events;
});

/// One-shot bootstrap: (desktop) spawn/adopt the daemon, then discover
/// wsPort/token via `/api/config`, then open WS. Call once at app start.
final connectionBootstrapProvider = FutureProvider<void>((ref) async {
  // Desktop hosts the daemon as a child process; web attaches to an external
  // one. `start()` adopts an already-running daemon and waits for the port.
  await ref.read(daemonSupervisorProvider).start();

  final api = ref.read(apiClientProvider);
  try {
    final cfg = await api.get('/api/config');
    if (cfg is Map) {
      final current = ref.read(appConfigProvider);
      final updated = current.copyWith(
        wsPort: (cfg['wsPort'] as num?)?.toInt(),
        wsToken: cfg['token'] as String?,
      );
      ref.read(appConfigProvider.notifier).state = updated;
      ref.read(wsClientProvider).updateConfig(updated);
      api.updateConfig(updated);
    }
  } catch (e) {
    // Daemon not reachable yet — WS layer will retry with defaults.
    if (kDebugMode) debugPrint('connection bootstrap: $e');
  }
  ref.read(wsClientProvider).connect();
});
