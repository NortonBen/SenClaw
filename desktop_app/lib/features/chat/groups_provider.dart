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
                // Selecting/reading a chat re-emits group:updated with a bumped
                // lastActivity but the SAME lastMessage. Don't let that reorder
                // the Sort:Updated list — only a genuinely new message (changed
                // lastMessage) should move the chat's sort position.
                (info.lastMessage == x.lastMessage
                    ? info.copyWith(lastActivity: x.lastActivity)
                    : info)
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

// ── Organize / sort modes (persisted) ────────────────────────────────────
enum OrganizeMode { project, projectRecent, chronological, flat }
enum SortMode { updated, created }

class OrganizeNotifier extends StateNotifier<OrganizeMode> {
  OrganizeNotifier(this._ref)
    : super(_parse(_ref.read(prefsHelperProvider).string(kOrganizeKey, 'project')));
  final Ref _ref;
  static OrganizeMode _parse(String s) => switch (s) {
    'project-recent' => OrganizeMode.projectRecent,
    'chronological' => OrganizeMode.chronological,
    'flat' => OrganizeMode.flat,
    _ => OrganizeMode.project,
  };
  static String _str(OrganizeMode m) => switch (m) {
    OrganizeMode.projectRecent => 'project-recent',
    OrganizeMode.chronological => 'chronological',
    OrganizeMode.flat => 'flat',
    OrganizeMode.project => 'project',
  };
  void set(OrganizeMode m) {
    state = m;
    _ref.read(prefsHelperProvider).setString(kOrganizeKey, _str(m));
  }
}

final organizeProvider =
    StateNotifierProvider<OrganizeNotifier, OrganizeMode>(
      (ref) => OrganizeNotifier(ref),
    );

class SortNotifier extends StateNotifier<SortMode> {
  SortNotifier(this._ref)
    : super(_ref.read(prefsHelperProvider).string(kSortKey, 'updated') == 'created'
          ? SortMode.created
          : SortMode.updated);
  final Ref _ref;
  void set(SortMode m) {
    state = m;
    _ref
        .read(prefsHelperProvider)
        .setString(kSortKey, m == SortMode.created ? 'created' : 'updated');
  }
}

final sortProvider =
    StateNotifierProvider<SortNotifier, SortMode>((ref) => SortNotifier(ref));
