import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:local_notifier/local_notifier.dart';
import 'package:window_manager/window_manager.dart';
import '../transport/connection.dart';
import '../transport/ws_client.dart';

/// Surfaces daemon push events as native OS notifications when the window
/// isn't focused (so background-running SenClaw still pings the user). Desktop
/// only; a no-op on web.
class SystemNotifier {
  SystemNotifier(this._ref);
  final Ref _ref;
  dynamic _sub;

  void start() {
    if (kIsWeb) return;
    _sub ??= _ref.read(wsClientProvider).events.listen(_onEvent);
  }

  Future<void> _onEvent(WsEvent e) async {
    String? title;
    String body = '';
    switch (e['type']) {
      case 'notification':
        title = '${e['title'] ?? e['kind'] ?? 'SenClaw'}';
        body = '${e['message'] ?? e['text'] ?? ''}';
      case 'space:event:reminder':
        title = '⏰ ${e['title'] ?? 'Reminder'}';
        body = 'Calendar reminder';
      case 'space:event:pending':
        title = '📅 ${e['title'] ?? 'Upcoming'}';
        body = 'Scheduled activity';
      default:
        return;
    }
    // Don't notify while the app is in the foreground — the in-app bell covers that.
    try {
      if (await windowManager.isFocused()) return;
    } catch (_) {}
    try {
      await LocalNotification(title: title, body: body).show();
    } catch (_) {}
  }

  void dispose() {
    _sub?.cancel();
    _sub = null;
  }
}

final systemNotifierProvider = Provider<SystemNotifier>((ref) {
  final n = SystemNotifier(ref);
  ref.onDispose(n.dispose);
  return n;
});
