import 'dart:convert';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/prefs.dart';
import '../../core/transport/connection.dart';
import '../../core/transport/ws_client.dart';

// ── Channels ───────────────────────────────────────────────────────────────
class ChannelInfo {
  final int id;
  final String platformType;
  final String name;
  final String connectionState;
  final bool enabled;
  final String credentialsJson;
  const ChannelInfo({
    required this.id,
    required this.platformType,
    required this.name,
    required this.connectionState,
    required this.enabled,
    this.credentialsJson = '',
  });

  factory ChannelInfo.fromJson(Map<String, dynamic> j) => ChannelInfo(
    id: (j['id'] as num?)?.toInt() ?? 0,
    platformType: '${j['platformType'] ?? ''}',
    name: '${j['name'] ?? ''}',
    connectionState: '${j['connectionState'] ?? 'unknown'}',
    enabled: j['enabled'] != false,
    credentialsJson: '${j['credentialsJson'] ?? ''}',
  );

  /// Parsed credentials map (or empty if none/invalid).
  Map<String, dynamic> get credentials {
    if (credentialsJson.trim().isEmpty) return const {};
    try {
      final d = jsonDecode(credentialsJson);
      return d is Map ? d.cast<String, dynamic>() : const {};
    } catch (_) {
      return const {};
    }
  }
}

class ChannelsNotifier extends StateNotifier<List<ChannelInfo>> {
  ChannelsNotifier(this._ref) : super(const []) {
    final ws = _ref.read(wsClientProvider);
    // The status stream only emits on transitions; if the socket is ALREADY
    // connected when this provider is first read (lazy), fetch right now —
    // otherwise the list would stay empty until the next reconnect.
    if (ws.status == WsStatus.connected) ws.send({'type': 'list:channels'});
    _statusSub = ws.statusStream.listen((s) {
      if (s == WsStatus.connected) ws.send({'type': 'list:channels'});
    });
    _eventSub = ws.events.listen((e) {
      if (e['type'] == 'channels' && e['channels'] is List) {
        state = (e['channels'] as List)
            .whereType<Map>()
            .map((m) => ChannelInfo.fromJson(m.cast<String, dynamic>()))
            .toList();
      } else if (e['type'] == 'channel:unregistered') {
        final id = (e['id'] as num?)?.toInt();
        state = state.where((c) => c.id != id).toList();
      }
    });
  }
  final Ref _ref;
  late final dynamic _statusSub;
  late final dynamic _eventSub;

  void refresh() => _ref.read(wsClientProvider).send({'type': 'list:channels'});

  /// Register a new channel (`register:channel {platformType,name,credentials}`).
  void register({
    required String platformType,
    required String name,
    required Map<String, dynamic> credentials,
  }) {
    _ref.read(wsClientProvider).send({
      'type': 'register:channel',
      'platformType': platformType,
      'name': name,
      'credentials': credentials,
    });
  }

  /// Edit a channel (`update:channel {id, name?, credentials?}`). The daemon
  /// REPLACES credentials, so callers must pass the full (merged) set.
  void update(int id, {String? name, Map<String, dynamic>? credentials}) {
    _ref.read(wsClientProvider).send({
      'type': 'update:channel',
      'id': id,
      'name': ?name,
      'credentials': ?credentials,
    });
  }

  void setEnabled(int id, bool enabled) {
    _ref.read(wsClientProvider)
        .send({'type': 'update:channel', 'id': id, 'enabled': enabled});
    state = [
      for (final c in state)
        if (c.id == id)
          ChannelInfo(
            id: c.id,
            platformType: c.platformType,
            name: c.name,
            connectionState: c.connectionState,
            enabled: enabled,
          )
        else
          c,
    ];
  }

  void delete(int id) {
    _ref.read(wsClientProvider).send({'type': 'unregister:channel', 'id': id});
    state = state.where((c) => c.id != id).toList();
  }

  @override
  void dispose() {
    _statusSub.cancel();
    _eventSub.cancel();
    super.dispose();
  }
}

final channelsProvider =
    StateNotifierProvider<ChannelsNotifier, List<ChannelInfo>>(
      (ref) => ChannelsNotifier(ref),
    );

// ── Agent ↔ channel bindings ──────────────────────────────────────────────
class Binding {
  final int id;
  final int agentId;
  final int channelId;
  const Binding(
      {required this.id, required this.agentId, required this.channelId});

  factory Binding.fromJson(Map<String, dynamic> j) => Binding(
        id: (j['id'] as num?)?.toInt() ?? 0,
        agentId: (j['agentId'] as num?)?.toInt() ?? 0,
        channelId: (j['channelId'] as num?)?.toInt() ?? 0,
      );
}

class BindingsNotifier extends StateNotifier<List<Binding>> {
  BindingsNotifier(this._ref) : super(const []) {
    final ws = _ref.read(wsClientProvider);
    _statusSub = ws.statusStream.listen((s) {
      if (s == WsStatus.connected) ws.send({'type': 'list:bindings'});
    });
    _eventSub = ws.events.listen((e) {
      final t = e['type'];
      if (t == 'bindings' && e['bindings'] is List) {
        state = (e['bindings'] as List)
            .whereType<Map>()
            .map((m) => Binding.fromJson(m.cast<String, dynamic>()))
            .toList();
      } else if (t == 'binding:registered' || t == 'binding:unregistered') {
        // Re-sync after any binding mutation we (or another client) made.
        ws.send({'type': 'list:bindings'});
      }
    });
    if (ws.status == WsStatus.connected) ws.send({'type': 'list:bindings'});
  }
  final Ref _ref;
  late final dynamic _statusSub;
  late final dynamic _eventSub;

  void refresh() => _ref.read(wsClientProvider).send({'type': 'list:bindings'});

  /// Bind an agent to a channel (`register:binding {agentId, channelId}`).
  void bind(int agentId, int channelId) {
    _ref.read(wsClientProvider).send({
      'type': 'register:binding',
      'agentId': agentId,
      'channelId': channelId,
    });
    state = [...state, Binding(id: -1, agentId: agentId, channelId: channelId)];
    refresh();
  }

  /// Remove a binding (`unregister:binding {id}`).
  void unbind(int bindingId) {
    _ref.read(wsClientProvider)
        .send({'type': 'unregister:binding', 'id': bindingId});
    state = state.where((b) => b.id != bindingId).toList();
  }

  /// The agent id a channel is bound to, or null if free.
  int? boundAgentOf(int channelId) {
    for (final b in state) {
      if (b.channelId == channelId) return b.agentId;
    }
    return null;
  }

  @override
  void dispose() {
    _statusSub.cancel();
    _eventSub.cancel();
    super.dispose();
  }
}

final bindingsProvider =
    StateNotifierProvider<BindingsNotifier, List<Binding>>(
      (ref) => BindingsNotifier(ref),
    );

// ── Tool auto-accept rules ───────────────────────────────────────────────
class ToolRule {
  final String id;
  final String action;
  final bool enabled;
  final String description;
  final Map<String, dynamic> matcher;
  const ToolRule({
    required this.id,
    required this.action,
    required this.enabled,
    required this.description,
    required this.matcher,
  });

  factory ToolRule.fromJson(Map<String, dynamic> j) => ToolRule(
    id: '${j['id'] ?? ''}',
    action: '${j['action'] ?? ''}',
    enabled: j['enabled'] != false,
    description: '${j['description'] ?? ''}',
    matcher: (j['matcher'] as Map?)?.cast<String, dynamic>() ?? const {},
  );

  Map<String, dynamic> toJson() => {
    'id': id,
    'action': action,
    'enabled': enabled,
    if (description.isNotEmpty) 'description': description,
    'matcher': matcher,
  };

  /// Human summary of the matcher (e.g. "bash_glob: git *").
  String get matcherLabel {
    final type = '${matcher['type'] ?? ''}';
    final detail = matcher['pattern'] ??
        matcher['tool_name'] ??
        matcher['skill_name'] ??
        matcher['server'] ??
        matcher['category'] ??
        '';
    return detail.toString().isEmpty ? type : '$type: $detail';
  }

  ToolRule copyWith({bool? enabled}) => ToolRule(
    id: id,
    action: action,
    enabled: enabled ?? this.enabled,
    description: description,
    matcher: matcher,
  );
}

const _kToolRulesKey = 'senclaw:tool-rules';

class ToolRulesNotifier extends StateNotifier<List<ToolRule>> {
  ToolRulesNotifier(this._ref) : super(const []) {
    // Tool rules are persisted CLIENT-SIDE (mirrors the web localStorage), so
    // they display immediately in Settings even before any chat is opened. The
    // daemon's permission bridge is in-memory, so we (re-)push our rules to it
    // on every (re)connect; the daemon only emits `permission:rules` on group
    // subscribe, which we then merge in (ignoring an empty list so a freshly
    // restarted daemon doesn't wipe our local copy before the re-push lands).
    state = _load();
    final ws = _ref.read(wsClientProvider);
    _sub = ws.events.listen((e) {
      final t = e['type'];
      if (t == 'permission:rules' && e['rules'] is List) {
        final rules = (e['rules'] as List)
            .whereType<Map>()
            .map((m) => ToolRule.fromJson(m.cast<String, dynamic>()))
            .toList();
        if (rules.isEmpty && state.isNotEmpty) return; // don't wipe on restart
        state = rules;
        _save();
      }
    });
    _statusSub = ws.statusStream.listen((s) {
      if (s == WsStatus.connected) _pushAll();
    });
    if (ws.status == WsStatus.connected) _pushAll();
  }
  final Ref _ref;
  late final dynamic _sub;
  late final dynamic _statusSub;

  List<ToolRule> _load() {
    final raw = _ref.read(prefsHelperProvider).string(_kToolRulesKey, '');
    if (raw.isEmpty) return const [];
    try {
      return (jsonDecode(raw) as List)
          .whereType<Map>()
          .map((m) => ToolRule.fromJson(m.cast<String, dynamic>()))
          .toList();
    } catch (_) {
      return const [];
    }
  }

  void _save() => _ref
      .read(prefsHelperProvider)
      .setString(_kToolRulesKey, jsonEncode(state.map((r) => r.toJson()).toList()));

  void _pushAll() {
    final ws = _ref.read(wsClientProvider);
    for (final r in state) {
      ws.send({'type': 'permission:rule:update', 'rule': r.toJson()});
    }
  }

  void setEnabled(ToolRule rule, bool enabled) {
    final updated = rule.copyWith(enabled: enabled);
    _ref.read(wsClientProvider)
        .send({'type': 'permission:rule:update', 'rule': updated.toJson()});
    state = [for (final r in state) r.id == rule.id ? updated : r];
    _save();
  }

  /// Upsert a rule (used for per-tool MCP auto-accept toggles).
  void add(ToolRule rule) {
    _ref.read(wsClientProvider)
        .send({'type': 'permission:rule:update', 'rule': rule.toJson()});
    state = [
      for (final r in state)
        if (r.id != rule.id) r,
      rule,
    ];
    _save();
  }

  void remove(String id) {
    _ref.read(wsClientProvider)
        .send({'type': 'permission:rule:remove', 'ruleId': id});
    state = state.where((r) => r.id != id).toList();
    _save();
  }

  @override
  void dispose() {
    _sub.cancel();
    _statusSub.cancel();
    super.dispose();
  }
}

final toolRulesProvider =
    StateNotifierProvider<ToolRulesNotifier, List<ToolRule>>(
      (ref) => ToolRulesNotifier(ref),
    );

// ── Dangerously-accept-all (persisted) ────────────────────────────────────
const _kAcceptAllKey = 'senclaw:dangerously-accept-all';

class AcceptAllNotifier extends StateNotifier<bool> {
  AcceptAllNotifier(this._ref)
    : super(_ref.read(prefsHelperProvider).string(_kAcceptAllKey, 'false') ==
          'true') {
    // Push the persisted choice to the daemon on (re)connect. If the socket
    // is ALREADY connected when this provider is first read, push right away —
    // the status stream only emits on transitions.
    final ws = _ref.read(wsClientProvider);
    if (ws.status == WsStatus.connected && state) {
      ws.send({'type': 'permission:accept-all', 'enabled': true});
    }
    _sub = ws.statusStream.listen((s) {
      if (s == WsStatus.connected && state) {
        ws.send({'type': 'permission:accept-all', 'enabled': true});
      }
    });
  }
  final Ref _ref;
  late final dynamic _sub;

  void set(bool enabled) {
    state = enabled;
    _ref.read(prefsHelperProvider).setString(_kAcceptAllKey, '$enabled');
    _ref.read(wsClientProvider)
        .send({'type': 'permission:accept-all', 'enabled': enabled});
  }

  @override
  void dispose() {
    _sub.cancel();
    super.dispose();
  }
}

final acceptAllProvider =
    StateNotifierProvider<AcceptAllNotifier, bool>(
      (ref) => AcceptAllNotifier(ref),
    );
