import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/transport/connection.dart';
import '../../core/transport/ws_client.dart';
import '../../theme/tokens.dart';
import 'reminder_interaction.dart';

class AppNotification {
  final String id;
  final String? rawId; // daemon event-notification id for `notification:read`
  final String title;
  final String detail;

  /// Non-null for calendar reminders/pending events — carries the context
  /// needed to open the interactive reminder dialog on tap.
  final ReminderTarget? target;
  bool read;
  AppNotification({
    required this.id,
    this.rawId,
    required this.title,
    required this.detail,
    this.target,
    this.read = false,
  });
}

/// Collects pushed notifications (`notification`, `space:event:reminder`,
/// `space:event:pending`) for the sidebar bell. Newest-first.
class NotificationsNotifier extends StateNotifier<List<AppNotification>> {
  NotificationsNotifier(this._ref) : super(const []) {
    _sub = _ref.read(wsClientProvider).events.listen(_onEvent);
  }
  final Ref _ref;
  late final dynamic _sub;
  int _seq = 0;

  void _onEvent(WsEvent e) {
    // Replay snapshots carry the persisted read state — honor it so items the
    // user already marked read don't come back as unread after a restart.
    final wasRead = e['read'] == true;
    switch (e['type']) {
      case 'notification':
        _add(
          '${e['id'] ?? 'n${_seq++}'}',
          '${e['title'] ?? e['kind'] ?? 'Notification'}',
          '${e['message'] ?? e['text'] ?? ''}',
          rawId: e['id']?.toString(),
          read: wasRead,
        );
      case 'space:event:reminder':
        _add('rem-${e['id'] ?? _seq++}', '⏰ ${e['title'] ?? 'Reminder'}',
            'Calendar reminder',
            rawId: e['id']?.toString(),
            read: wasRead,
            target: _reminderTarget(e));
      case 'space:event:pending':
        _add('pend-${e['id'] ?? _seq++}', '📅 ${e['title'] ?? 'Upcoming'}',
            'Scheduled',
            rawId: e['id']?.toString(),
            read: wasRead,
            target: _reminderTarget(e, kind: 'pending'));
    }
  }

  /// Build the interactive-reminder context from a `space:event:*` frame.
  ReminderTarget _reminderTarget(WsEvent e, {String? kind}) => ReminderTarget(
        eventId: e['eventId']?.toString(),
        title: '${e['title'] ?? 'Reminder'}',
        startAtMs: (e['startAt'] as num?)?.toInt(),
        kind: kind ?? '${e['kind'] ?? 'reminder'}',
        notificationId: e['id']?.toString(),
      );

  void _add(String id, String title, String detail,
      {String? rawId, bool read = false, ReminderTarget? target}) {
    // Dedup by id (replay snapshots can repeat).
    if (state.any((n) => n.id == id)) return;
    state = [
      AppNotification(
          id: id,
          rawId: rawId,
          title: title,
          detail: detail,
          target: target,
          read: read),
      ...state,
    ].take(50).toList();
  }

  /// Tell the daemon a notification was read so it doesn't replay on reconnect.
  void _persistRead(String? rawId) {
    if (rawId == null || rawId.isEmpty) return;
    _ref
        .read(wsClientProvider)
        .send({'type': 'notification:read', 'id': rawId});
  }

  void markRead(String id) {
    state = [
      for (final n in state)
        if (n.id == id)
          AppNotification(
              id: n.id,
              rawId: n.rawId,
              title: n.title,
              detail: n.detail,
              target: n.target,
              read: true)
        else
          n,
    ];
    _persistRead(state.firstWhere((n) => n.id == id,
        orElse: () => AppNotification(id: id, title: '', detail: '')).rawId);
  }

  void clearAll() {
    for (final n in state) {
      _persistRead(n.rawId);
    }
    state = const [];
  }

  @override
  void dispose() {
    _sub.cancel();
    super.dispose();
  }
}

final notificationsProvider =
    StateNotifierProvider<NotificationsNotifier, List<AppNotification>>(
      (ref) => NotificationsNotifier(ref),
    );

/// Bell with unread badge + popover list.
class NotificationsBell extends ConsumerWidget {
  const NotificationsBell({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final items = ref.watch(notificationsProvider);
    final unread = items.where((n) => !n.read).length;

    return PopupMenuButton<String>(
      tooltip: 'Notifications',
      offset: const Offset(0, 32),
      icon: Badge(
        isLabelVisible: unread > 0,
        label: Text('$unread'),
        child: Icon(
          unread > 0 ? Icons.notifications : Icons.notifications_none,
          size: 18,
          color: unread > 0 ? c.accent : c.textMuted,
        ),
      ),
      onSelected: (id) {
        if (id == '__clear__') {
          ref.read(notificationsProvider.notifier).clearAll();
          return;
        }
        AppNotification? item;
        for (final n in items) {
          if (n.id == id) {
            item = n;
            break;
          }
        }
        ref.read(notificationsProvider.notifier).markRead(id);
        // Calendar reminders open the interactive dialog; plain notifications
        // just mark read.
        if (item?.target != null) {
          ref.read(pendingReminderProvider.notifier).state = item!.target;
        }
      },
      itemBuilder: (_) {
        if (items.isEmpty) {
          return [
            PopupMenuItem(
              enabled: false,
              child: Text('No notifications',
                  style: TextStyle(color: c.textMuted)),
            ),
          ];
        }
        return [
          for (final n in items)
            PopupMenuItem(
              value: n.id,
              child: SizedBox(
                width: 280,
                child: Row(
                  children: [
                    if (!n.read)
                      Container(
                        width: 6,
                        height: 6,
                        margin: const EdgeInsets.only(right: AppTokens.s8),
                        decoration: BoxDecoration(
                          color: c.accent,
                          shape: BoxShape.circle,
                        ),
                      ),
                    Expanded(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(n.title,
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                              style: TextStyle(
                                color: c.textPrimary,
                                fontSize: 14,
                                fontWeight:
                                    n.read ? FontWeight.w400 : FontWeight.w600,
                              )),
                          if (n.detail.isNotEmpty)
                            Text(n.detail,
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: TextStyle(
                                    color: c.textMuted, fontSize: 12)),
                        ],
                      ),
                    ),
                  ],
                ),
              ),
            ),
          const PopupMenuDivider(),
          PopupMenuItem(
            value: '__clear__',
            child: Row(children: [
              Icon(Icons.clear_all, size: 16, color: c.textSecondary),
              const SizedBox(width: AppTokens.s8),
              const Text('Clear all'),
            ]),
          ),
        ];
      },
    );
  }
}
