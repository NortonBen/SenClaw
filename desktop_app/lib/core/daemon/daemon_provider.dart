import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../transport/connection.dart';
import 'daemon_supervisor.dart';

/// The single daemon supervisor for the app lifetime.
final daemonSupervisorProvider = ChangeNotifierProvider<DaemonSupervisor>((ref) {
  final cfg = ref.read(appConfigProvider);
  final sup = DaemonSupervisor(
    host: cfg.host,
    uiPort: cfg.uiPort,
    wsPort: cfg.wsPort,
  );
  ref.onDispose(sup.dispose);
  return sup;
});
