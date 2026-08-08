import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../prefs.dart';
import '../transport/connection.dart';
import 'daemon_supervisor.dart';

/// The single daemon supervisor for the app lifetime.
///
/// No `ref.onDispose(sup.dispose)`: ChangeNotifierProvider disposes the
/// notifier itself, and disposing it twice trips a ChangeNotifier assert.
final daemonSupervisorProvider = ChangeNotifierProvider<DaemonSupervisor>((ref) {
  final cfg = ref.read(appConfigProvider);
  return DaemonSupervisor(
    host: cfg.host,
    uiPort: cfg.uiPort,
    wsPort: cfg.wsPort,
    bindHost: _storedBindHost(ref),
  );
});

/// Read once at creation rather than watched: the bind host is baked into the
/// daemon process at spawn time, so reacting to the pref live would only
/// promise a change the running daemon cannot honour. Settings writes the pref,
/// sets [DaemonSupervisor.bindHost], and offers a restart.
String _storedBindHost(Ref ref) {
  try {
    return ref.read(prefsProvider).getBool(kBindPublicKey) == true
        ? DaemonSupervisor.kPublicBindHost
        : DaemonSupervisor.kPrivateBindHost;
  } catch (_) {
    return DaemonSupervisor.kPrivateBindHost; // prefs not overridden (tests)
  }
}
