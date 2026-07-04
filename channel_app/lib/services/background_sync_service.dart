import 'dart:async';
import 'package:flutter/widgets.dart';
import 'chat_api.dart';
import 'language_service.dart';
import 'logger_service.dart';
import 'notification_service.dart';
import 'relay_manager.dart';

/// Periodic, in-app background sync. While the app is running (foreground or a
/// brief background window before the OS suspends the isolate) a timer pulls
/// fresh agent/session lists and new messages for the active session, and — when
/// notifications are enabled and the app is not in the foreground — raises an OS
/// notification for new agent replies.
///
/// This is intentionally NOT a killed-app background worker (no workmanager);
/// timers pause once the OS suspends the app.
class BackgroundSyncService with WidgetsBindingObserver {
  static final BackgroundSyncService _i = BackgroundSyncService._();
  factory BackgroundSyncService() => _i;
  BackgroundSyncService._();

  final _relayManager = RelayManager();
  final _chatApi = ChatApi();

  Timer? _timer;
  bool _configured = false;
  bool _enabled = false;
  int _intervalMin = 15;
  bool _notify = false;
  bool _observerAdded = false;
  bool _ticking = false;
  AppLifecycleState _lifecycle = AppLifecycleState.resumed;

  /// Per-jid "last seen" timestamp (epoch ms). In-memory only: a fresh launch
  /// re-baselines so we never notify for history the user already had.
  final Map<String, int> _cursor = {};

  bool get _foreground => _lifecycle == AppLifecycleState.resumed;

  /// Apply the latest settings. Idempotent — a no-op if nothing changed.
  void configure({
    required bool enabled,
    required int intervalMinutes,
    required bool notify,
  }) {
    if (_configured &&
        _enabled == enabled &&
        _intervalMin == intervalMinutes &&
        _notify == notify) {
      return;
    }
    _configured = true;
    _enabled = enabled;
    _intervalMin = intervalMinutes;
    _notify = notify;
    if (!_observerAdded) {
      WidgetsBinding.instance.addObserver(this);
      _observerAdded = true;
    }
    _restart();
  }

  void _restart() {
    _timer?.cancel();
    _timer = null;
    if (!_enabled) return;
    final mins = _intervalMin.clamp(1, 1440);
    Log.i('[BgSync] enabled — every $mins min (notify=$_notify)');
    _timer = Timer.periodic(Duration(minutes: mins), (_) => unawaited(_tick()));
    // Kick one promptly so lists refresh right after the user enables it.
    unawaited(_tick());
  }

  void stop() {
    _timer?.cancel();
    _timer = null;
    _enabled = false;
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    _lifecycle = state;
  }

  Future<void> _tick() async {
    if (!_enabled || _ticking) return;
    _ticking = true;
    try {
      final started = await _relayManager.ensureStarted();
      if (!started) return;
      // Keep the sidebar lists warm.
      _relayManager.requestAgentList();
      _relayManager.requestSessionList();

      final jid = _activeJid();
      if (jid == null) return;

      final baseline = !_cursor.containsKey(jid);
      final after = _cursor[jid] ?? 0;
      // `after - 1` re-fetches the boundary ms so same-ms rows aren't skipped.
      final rows =
          await _chatApi.fetchHistoryAfter(jid, after > 0 ? after - 1 : 0);

      var maxTs = after;
      final fresh = <ChatHistoryEntry>[];
      for (final r in rows) {
        if (r.ts > maxTs) maxTs = r.ts;
        if (r.ts > after && (r.role == 'agent' || r.isBotReply)) fresh.add(r);
      }
      _cursor[jid] = maxTs;

      // First observation of this session: set the baseline, don't notify.
      if (baseline) return;
      if (_notify && fresh.isNotEmpty && !_foreground) {
        await _notifyNew(fresh);
      }
    } catch (e) {
      Log.w('[BgSync] tick failed: $e');
    } finally {
      _ticking = false;
    }
  }

  /// The device's active session jid (falls back to the default session).
  String? _activeJid() {
    final active = _relayManager.sessions.where((s) => s.active).toList();
    if (active.isNotEmpty) return active.first.jid;
    return _relayManager.defaultSessionJid;
  }

  Future<void> _notifyNew(List<ChatHistoryEntry> rows) async {
    final n = rows.length;
    final title = n == 1
        ? tr('Tin nhắn mới', 'New message')
        : tr('$n tin nhắn mới', '$n new messages');
    var body = rows.last.content.trim();
    if (body.length > 120) body = '${body.substring(0, 120)}…';
    if (body.isEmpty) body = tr('Bạn có tin nhắn mới', 'You have new messages');
    await NotificationService().show(title, body);
  }
}
