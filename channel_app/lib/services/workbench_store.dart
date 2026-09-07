import 'dart:async';

import 'package:flutter/foundation.dart';

import '../models/api_models.dart';
import '../models/workbench_models.dart';
import 'local_cache.dart';
import 'logger_service.dart';
import 'relay_manager.dart';

/// Per-chat workbench state, built from relay events.
///
/// There is no list endpoint on the daemon — artifacts exist only as
/// `workbench:new` events — so this store *is* the source of truth on the
/// device, and it persists to [LocalCache] the way the web UI persists to
/// localStorage. Without that, backgrounding the app loses every artifact.
///
/// The events reach mobile at all because `WsGateway::broadcast` mirrors any
/// `app:*` jid into the app-channel event sink, and all four
/// `notify_workbench_*` calls go through `broadcast`. Nothing had to be added
/// on the daemon side.
class WorkbenchStore extends ChangeNotifier {
  static final WorkbenchStore _instance = WorkbenchStore._internal();
  factory WorkbenchStore() => _instance;
  WorkbenchStore._internal();

  static const _domain = 'workbench';

  /// Cache scope suffix for the seen-id set. Kept beside the artifacts rather
  /// than in memory only: the artifacts survive a restart, so a purely
  /// in-memory seen set would light the badge for everything already read.
  static const _seenSuffix = ':seen';

  /// jid → artifacts, newest first.
  final Map<String, List<WorkbenchArtifact>> _byJid = {};

  /// Artifact ids the user has opened, so a badge can mean "new".
  final Set<String> _seen = {};

  StreamSubscription<ApiEvent>? _sub;
  bool _started = false;

  List<WorkbenchArtifact> artifactsFor(String jid) =>
      List.unmodifiable(_byJid[jid] ?? const []);

  WorkbenchArtifact? currentFor(String jid) {
    final list = _byJid[jid];
    return (list == null || list.isEmpty) ? null : list.first;
  }

  int unseenCount(String jid) =>
      (_byJid[jid] ?? const <WorkbenchArtifact>[])
          .where((a) => !_seen.contains(a.id))
          .length;

  bool isSeen(String id) => _seen.contains(id);

  /// Subscribe to the relay's event stream. Safe to call repeatedly; only the
  /// first call with a live relay attaches.
  void start() {
    if (_started) return;
    final relay = RelayManager().relay;
    if (relay == null) return;
    _sub = relay.apiEvents.listen(_onEvent);
    _started = true;
  }

  /// Re-attach after a reconnect handed the manager a new [RelayService].
  void restart() {
    _sub?.cancel();
    _sub = null;
    _started = false;
    start();
  }

  Future<void> loadCached(String jid) async {
    if (_byJid.containsKey(jid)) return;
    try {
      final seen =
          await LocalCache().getDomainList(_domain, scope: '$jid$_seenSuffix');
      for (final row in seen) {
        final id = row['id']?.toString();
        if (id != null && id.isNotEmpty) _seen.add(id);
      }
      final rows = await LocalCache().getDomainList(_domain, scope: jid);
      if (rows.isEmpty && seen.isEmpty) return;
      if (rows.isNotEmpty) {
        _byJid[jid] = rows.map(WorkbenchArtifact.fromJson).toList();
      }
      notifyListeners();
    } catch (e) {
      Log.e('[Workbench] loadCached($jid) failed: $e');
    }
  }

  void markSeen(String jid, String id) {
    if (!_seen.add(id)) return;
    _persistSeen(jid);
    notifyListeners();
  }

  /// Drop one artifact locally. The caller closes it on the daemon separately —
  /// a close that fails there should still clear it here, because the event
  /// that put it here will not be resent.
  void remove(String jid, String id) {
    final list = _byJid[jid];
    if (list == null) return;
    list.removeWhere((a) => a.id == id);
    _seen.remove(id);
    _persist(jid);
    _persistSeen(jid);
    notifyListeners();
  }

  void clear(String jid) {
    for (final a in _byJid[jid] ?? const <WorkbenchArtifact>[]) {
      _seen.remove(a.id);
    }
    _byJid.remove(jid);
    _persist(jid);
    _persistSeen(jid);
    notifyListeners();
  }

  void _onEvent(ApiEvent event) {
    final data = event.data;
    if (data is! Map) return;
    final msg = data.cast<String, dynamic>();
    final jid = (msg['groupJid'] ?? '').toString();
    if (jid.isEmpty) return;

    switch (event.topic) {
      case 'workbench:new':
        final raw = msg['artifact'];
        if (raw is! Map) return;
        final artifact =
            WorkbenchArtifact.fromJson(raw.cast<String, dynamic>());
        if (artifact.id.isEmpty) return;
        final list = _byJid.putIfAbsent(jid, () => []);
        // `replacesId` means a re-render of the same thing: drop the old row
        // rather than stacking two artifacts the user cannot tell apart.
        final replaces = msg['replacesId']?.toString();
        list.removeWhere(
            (a) => a.id == artifact.id || (replaces != null && a.id == replaces));
        list.insert(0, artifact);
        _persist(jid);
        notifyListeners();

      case 'workbench:service_ready':
      case 'workbench:service_crashed':
      case 'workbench:service_stopped':
        final id = (msg['artifactId'] ?? '').toString();
        final list = _byJid[jid];
        if (id.isEmpty || list == null) return;
        final status = switch (event.topic) {
          'workbench:service_ready' => 'ready',
          'workbench:service_crashed' => 'crashed',
          _ => 'stopped',
        };
        var changed = false;
        for (var i = 0; i < list.length; i++) {
          if (list[i].id == id && list[i].process != null) {
            list[i] = list[i].withProcessStatus(status);
            changed = true;
          }
        }
        if (!changed) return;
        _persist(jid);
        notifyListeners();
    }
  }

  /// The seen ids for this jid, stored as one-key rows so the list-shaped
  /// cache can hold them. Only ids still present are written — a set that grew
  /// forever would outlive every artifact it referred to.
  void _persistSeen(String jid) {
    final live = (_byJid[jid] ?? const <WorkbenchArtifact>[])
        .where((a) => _seen.contains(a.id))
        .map((a) => {'id': a.id})
        .toList();
    unawaited(
      LocalCache()
          .putDomainList(_domain, live, scope: '$jid$_seenSuffix')
          .catchError((Object e) => Log.e('[Workbench] persist seen failed: $e')),
    );
  }

  /// Cache write is fire-and-forget: a failed write must never drop the
  /// in-memory artifact the user is looking at.
  void _persist(String jid) {
    final list = _byJid[jid] ?? const <WorkbenchArtifact>[];
    unawaited(
      LocalCache()
          .putDomainList(
            _domain,
            list.map((a) => a.toJson()).toList(),
            scope: jid,
          )
          .catchError((Object e) => Log.e('[Workbench] persist failed: $e')),
    );
  }

  @override
  void dispose() {
    _sub?.cancel();
    super.dispose();
  }
}
