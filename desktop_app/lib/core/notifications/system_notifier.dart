import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:local_notifier/local_notifier.dart';
import 'package:window_manager/window_manager.dart';
import '../transport/connection.dart';
import '../transport/ws_client.dart';
import '../../features/chat/reminder_interaction.dart';

/// Surfaces daemon push events as native OS notifications when the window
/// isn't focused (so background-running SenClaw still pings the user). Desktop
/// only; a no-op on web.
///
/// Calendar reminders are interactive: clicking the toast (or its "Mở" action)
/// focuses the window and opens the reminder dialog; the "Xoá" action deletes
/// the event directly.
class SystemNotifier {
  SystemNotifier(this._ref);
  final Ref _ref;
  dynamic _sub;

  /// Reminder toasts must stay referenced or their click listeners (registered
  /// on the global `localNotifier`) get GC'd before the user can click. Capped.
  final List<LocalNotification> _live = [];

  void start() {
    if (kIsWeb) return;
    _sub ??= _ref.read(wsClientProvider).events.listen(_onEvent);
  }

  Future<void> _onEvent(WsEvent e) async {
    String? title;
    String body = '';
    ReminderTarget? target;
    switch (e['type']) {
      case 'notification':
        title = '${e['title'] ?? e['kind'] ?? 'SenClaw'}';
        body = '${e['message'] ?? e['text'] ?? ''}';
      case 'space:event:reminder':
        title = '⏰ ${e['title'] ?? 'Reminder'}';
        body = 'Calendar reminder';
        target = _target(e);
      case 'space:event:pending':
        title = '📅 ${e['title'] ?? 'Upcoming'}';
        body = 'Scheduled activity';
        target = _target(e, kind: 'pending');
      default:
        return;
    }
    // Don't notify while the app is in the foreground — the in-app bell covers that.
    try {
      if (await windowManager.isFocused()) return;
    } catch (_) {}
    try {
      final canDelete = target?.eventId != null && target!.eventId!.isNotEmpty;
      final notif = LocalNotification(
        title: title,
        body: body,
        actions: target != null
            ? [
                LocalNotificationAction(text: 'Mở'),
                if (canDelete) LocalNotificationAction(text: 'Xoá'),
              ]
            : null,
      );
      if (target != null) {
        final t = target; // promoted non-null; captured by the listeners
        notif.onClick = () => _openReminder(t);
        notif.onClickAction = (i) {
          if (i == 0) {
            _openReminder(t);
          } else if (i == 1) {
            _deleteEvent(t);
          }
        };
        _retain(notif);
      }
      await notif.show();
    } catch (_) {}
  }

  ReminderTarget _target(WsEvent e, {String? kind}) => ReminderTarget(
        eventId: e['eventId']?.toString(),
        title: '${e['title'] ?? 'Reminder'}',
        startAtMs: (e['startAt'] as num?)?.toInt(),
        kind: kind ?? '${e['kind'] ?? 'reminder'}',
        notificationId: e['id']?.toString(),
      );

  Future<void> _openReminder(ReminderTarget t) async {
    try {
      await windowManager.show();
      await windowManager.focus();
    } catch (_) {}
    _ref.read(pendingReminderProvider.notifier).state = t;
  }

  Future<void> _deleteEvent(ReminderTarget t) async {
    final id = t.eventId;
    if (id == null || id.isEmpty) return;
    try {
      await _ref
          .read(apiClientProvider)
          .delete('/api/space/calendar/events/$id');
    } catch (_) {}
  }

  void _retain(LocalNotification n) {
    _live.add(n);
    if (_live.length > 20) _live.removeAt(0);
  }

  void dispose() {
    _sub?.cancel();
    _sub = null;
    _live.clear();
  }
}

final systemNotifierProvider = Provider<SystemNotifier>((ref) {
  final n = SystemNotifier(ref);
  ref.onDispose(n.dispose);
  return n;
});
