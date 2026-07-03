import 'dart:math';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/prefs.dart';
import '../../core/transport/connection.dart';
import '../../core/transport/ws_client.dart';
import '../../models/group.dart';

/// The sidebar group list, driven by WS `groups` (full list) plus incremental
/// `group:registered` / `group:unregistered` / `group:updated` events. Exposes
/// create/rename/delete helpers (register/update/unregister over WS).
class GroupsNotifier extends StateNotifier<List<GroupInfo>> {
  GroupsNotifier(this._ref) : super(const []) {
    final ws = _ref.read(wsClientProvider);
    // The status stream only emits on transitions; if the socket is ALREADY
    // connected when this provider is first read (lazy), fetch right now —
    // otherwise the list would stay empty until the next reconnect.
    if (ws.status == WsStatus.connected) ws.send({'type': 'list:groups'});
    _statusSub = ws.statusStream.listen((s) {
      if (s == WsStatus.connected) ws.send({'type': 'list:groups'});
    });
    _eventSub = ws.events.listen(_onEvent);
  }

  final Ref _ref;
  late final dynamic _statusSub;
  late final dynamic _eventSub;

  void _onEvent(WsEvent e) {
    switch (e['type']) {
      case 'groups':
        if (e['groups'] is List) {
          state = (e['groups'] as List)
              .whereType<Map>()
              .map((m) => GroupInfo.fromJson(m.cast<String, dynamic>()))
              .toList();
        }
      case 'group:registered':
        final g = e['group'];
        if (g is Map) {
          final info = GroupInfo.fromJson(g.cast<String, dynamic>());
          if (!state.any((x) => x.jid == info.jid)) {
            state = [...state, info];
          }
        }
      case 'group:unregistered':
        final jid = '${e['jid']}';
        state = state.where((g) => g.jid != jid).toList();
      case 'group:updated':
        final g = e['group'];
        if (g is Map) {
          final info = GroupInfo.fromJson(g.cast<String, dynamic>());
          state = [
            for (final x in state)
              if (x.jid == info.jid)
                // group:updated carries settings changes, not activity — keep
                // the lastActivity we already know unless the server sent one.
                (info.lastActivity == null
                    ? info.copyWith(lastActivity: x.lastActivity)
                    : info)
              else
                x,
          ];
        }
      case 'group:activity':
        // Lightweight tick broadcast on every new message/agent response:
        // bump that chat's lastActivity so the "recent activity" sort
        // reorders live.
        final jid = '${e['jid']}';
        final ts = (e['ts'] as num?)?.toInt();
        if (ts != null) {
          state = [
            for (final x in state)
              if (x.jid == jid && ts > (x.lastActivity ?? 0))
                x.copyWith(lastActivity: ts)
              else
                x,
          ];
        }
    }
  }

  void refresh() => _ref.read(wsClientProvider).send({'type': 'list:groups'});

  /// Register a new web-only chat; returns its jid. Mirrors ChatPage.handleStartChat.
  String createChat({
    required String folder,
    required String name,
    required bool isCode,
    String? workDir,
    String? modelId,
  }) {
    final rand = _rand(6);
    var jid = 'web:$folder:${_b36(DateTime.now().millisecondsSinceEpoch)}-$rand';
    while (state.any((g) => g.jid == jid)) {
      jid = 'web:$folder:${_b36(DateTime.now().millisecondsSinceEpoch)}-${_rand(6)}';
    }
    _ref.read(wsClientProvider).send({
      'type': 'register:group',
      'jid': jid,
      'folder': folder,
      'name': name,
      'groupType': isCode ? 'code' : 'chat',
      'requiresTrigger': false,
      'allowedWorkDirs': isCode && workDir != null ? [workDir] : null,
      'modelId': modelId,
    });
    return jid;
  }

  void rename(String jid, String name) => _ref
      .read(wsClientProvider)
      .send({'type': 'update:group', 'jid': jid, 'name': name});

  /// Set the per-chat model override (empty/null → global default).
  /// Mirrors the web `updateGroup(jid, { modelId })`.
  void setModel(String jid, String? modelId) => _ref.read(wsClientProvider).send(
      {'type': 'update:group', 'jid': jid, 'modelId': modelId});

  void delete(String jid) => _ref
      .read(wsClientProvider)
      .send({'type': 'unregister:group', 'jid': jid});

  static String _b36(int n) => n.toRadixString(36);
  static String _rand(int len) {
    const chars = 'abcdefghijklmnopqrstuvwxyz0123456789';
    final r = Random();
    return String.fromCharCodes(
      List.generate(len, (_) => chars.codeUnitAt(r.nextInt(chars.length))),
    );
  }

  @override
  void dispose() {
    _statusSub.cancel();
    _eventSub.cancel();
    super.dispose();
  }
}

final groupsProvider =
    StateNotifierProvider<GroupsNotifier, List<GroupInfo>>(
      (ref) => GroupsNotifier(ref),
    );

// ── Pinned jids (persisted) ──────────────────────────────────────────────
class PinnedNotifier extends StateNotifier<Set<String>> {
  PinnedNotifier(this._ref) : super({}) {
    state = _ref.read(prefsHelperProvider).stringSet(kPinnedKey);
  }
  final Ref _ref;

  void toggle(String jid) {
    final next = {...state};
    next.contains(jid) ? next.remove(jid) : next.add(jid);
    state = next;
    _ref.read(prefsHelperProvider).setStringSet(kPinnedKey, next);
  }
}

final pinnedProvider =
    StateNotifierProvider<PinnedNotifier, Set<String>>(
      (ref) => PinnedNotifier(ref),
    );

// ── Group / sort modes (persisted) ───────────────────────────────────────
/// "Group by" axis: bucket sessions by project folder, by date, or not at all.
enum GroupMode { project, date, none }

/// "Sort by" axis: order sessions (and project buckets) by last activity,
/// creation time, or name A–Z.
enum SortMode { updated, created, name }

class GroupModeNotifier extends StateNotifier<GroupMode> {
  GroupModeNotifier(this._ref)
    : super(_parse(_ref.read(prefsHelperProvider).string(kOrganizeKey, 'project')));
  final Ref _ref;
  // Legacy 4-way organize values map onto the 3-way split: 'project-recent'
  // collapses into 'project' (bucket order now follows the sort mode).
  static GroupMode _parse(String s) => switch (s) {
    'chronological' || 'date' => GroupMode.date,
    'flat' || 'none' => GroupMode.none,
    _ => GroupMode.project,
  };
  void set(GroupMode m) {
    state = m;
    _ref.read(prefsHelperProvider).setString(kOrganizeKey, m.name);
  }
}

final groupModeProvider =
    StateNotifierProvider<GroupModeNotifier, GroupMode>(
      (ref) => GroupModeNotifier(ref),
    );

class SortNotifier extends StateNotifier<SortMode> {
  SortNotifier(this._ref)
    : super(switch (_ref.read(prefsHelperProvider).string(kSortKey, 'updated')) {
        'created' => SortMode.created,
        'name' => SortMode.name,
        _ => SortMode.updated,
      });
  final Ref _ref;
  void set(SortMode m) {
    state = m;
    _ref.read(prefsHelperProvider).setString(kSortKey, m.name);
  }
}

final sortProvider =
    StateNotifierProvider<SortNotifier, SortMode>((ref) => SortNotifier(ref));
