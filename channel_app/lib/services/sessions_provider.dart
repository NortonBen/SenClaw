import 'dart:async';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../core/prefs.dart';
import '../models/session_model.dart';
import 'relay_manager.dart';

/// The device's session list, fed by the daemon's `SESSION_LIST_RESP` frames
/// (relayed through [RelayManager.sessionUpdates]). Exposes create / rename /
/// rebind / delete / select helpers that send the matching control frames.
class SessionsNotifier extends StateNotifier<List<SessionInfo>> {
  SessionsNotifier() : super(const []) {
    final rm = RelayManager();
    state = rm.sessions; // seed with whatever arrived before this mounted
    _sub = rm.sessionUpdates.listen((list) {
      if (mounted) state = list;
    });
    if (rm.connected) rm.requestSessionList();
  }

  late final StreamSubscription _sub;

  void refresh() => RelayManager().requestSessionList();

  void create({required String folder, required String name, String? mode}) =>
      RelayManager().createSession(folder: folder, name: name, mode: mode);

  void rename(String jid, String name) =>
      RelayManager().updateSession(jid, name: name);

  void rebind(String jid, String folder) =>
      RelayManager().updateSession(jid, folder: folder);

  void delete(String jid) => RelayManager().deleteSession(jid);

  void select(String jid, {String? folder, String? mode}) =>
      RelayManager().selectSession(jid, folder: folder, mode: mode);

  @override
  void dispose() {
    _sub.cancel();
    super.dispose();
  }
}

final sessionsProvider =
    StateNotifierProvider<SessionsNotifier, List<SessionInfo>>(
  (ref) => SessionsNotifier(),
);

/// The session currently open in the chat UI (its jid). Driven by the drawer /
/// sessions screen; ChatScreen watches it to switch conversations.
final selectedSessionJidProvider = StateProvider<String?>((ref) => null);

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
/// "Group by" axis: bucket sessions by agent folder, by date, or not at all.
enum GroupMode { agent, date, none }

/// "Sort by" axis: order sessions by last activity or name A–Z.
enum SortMode { updated, name }

class GroupModeNotifier extends StateNotifier<GroupMode> {
  GroupModeNotifier(this._ref)
      : super(_parse(
            _ref.read(prefsHelperProvider).string(kSessOrganizeKey, 'date')));
  final Ref _ref;
  static GroupMode _parse(String s) => switch (s) {
        'agent' || 'project' => GroupMode.agent,
        'none' || 'flat' => GroupMode.none,
        _ => GroupMode.date,
      };
  void set(GroupMode m) {
    state = m;
    _ref.read(prefsHelperProvider).setString(kSessOrganizeKey, m.name);
  }
}

final groupModeProvider =
    StateNotifierProvider<GroupModeNotifier, GroupMode>(
  (ref) => GroupModeNotifier(ref),
);

class SortNotifier extends StateNotifier<SortMode> {
  SortNotifier(this._ref)
      : super(_ref.read(prefsHelperProvider).string(kSessSortKey, 'updated') ==
                'name'
            ? SortMode.name
            : SortMode.updated);
  final Ref _ref;
  void set(SortMode m) {
    state = m;
    _ref.read(prefsHelperProvider).setString(kSessSortKey, m.name);
  }
}

final sortProvider =
    StateNotifierProvider<SortNotifier, SortMode>((ref) => SortNotifier(ref));
