import 'dart:async';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/transport/connection.dart';
import '../../core/transport/ws_client.dart';

/// A registered agent profile (subset of the React `AgentInfo`).
class AgentInfo {
  final int id;
  final String folder;
  final String name;
  final String corePrompt;
  final String? modelId;
  final bool requiresTrigger;
  const AgentInfo({
    required this.id,
    required this.folder,
    required this.name,
    this.corePrompt = '',
    this.modelId,
    this.requiresTrigger = false,
  });

  factory AgentInfo.fromJson(Map<String, dynamic> j) => AgentInfo(
    id: (j['id'] as num?)?.toInt() ?? 0,
    folder: '${j['folder'] ?? 'main'}',
    name: '${j['name'] ?? j['folder'] ?? 'main'}',
    corePrompt: '${j['corePrompt'] ?? ''}',
    modelId: j['modelId'] as String?,
    requiresTrigger: j['requiresTrigger'] == true,
  );

  bool get isSchedule => folder.startsWith('schedule_');
}

/// Agent profiles, driven by the WS `agents` event (requested on connect).
class AgentsNotifier extends StateNotifier<List<AgentInfo>> {
  AgentsNotifier(this._ref) : super(const []) {
    final ws = _ref.read(wsClientProvider);
    _statusSub = ws.statusStream.listen((s) {
      if (s == WsStatus.connected) ws.send({'type': 'list:agents'});
    });
    _eventSub = ws.events.listen((e) {
      if (e['type'] == 'agents' && e['agents'] is List) {
        state = (e['agents'] as List)
            .whereType<Map>()
            .map((m) => AgentInfo.fromJson(m.cast<String, dynamic>()))
            .toList();
      } else if (e['type'] == 'agent:registered' && e['agent'] is Map) {
        final a = AgentInfo.fromJson(
            (e['agent'] as Map).cast<String, dynamic>());
        state = [for (final x in state) if (x.id != a.id) x, a];
        _pending.remove(a.folder)?.complete(a.id);
      }
    });
  }

  final Ref _ref;
  late final dynamic _statusSub;
  late final dynamic _eventSub;
  final Map<String, Completer<int?>> _pending = {};

  void refresh() => _ref.read(wsClientProvider).send({'type': 'list:agents'});

  /// Create a new agent profile (`register:agent {...}`). Resolves to the new
  /// agent id once the daemon echoes `agent:registered`, or null on timeout.
  Future<int?> registerAgent({
    required String folder,
    required String name,
    bool requiresTrigger = true,
    String corePrompt = '',
    String? modelId,
  }) {
    final completer = Completer<int?>();
    _pending[folder] = completer;
    _ref.read(wsClientProvider).send({
      'type': 'register:agent',
      'folder': folder,
      'name': name,
      'requiresTrigger': requiresTrigger,
      'corePrompt': corePrompt,
      if (modelId != null && modelId.isNotEmpty) 'modelId': modelId,
    });
    return completer.future.timeout(
      const Duration(seconds: 6),
      onTimeout: () {
        _pending.remove(folder);
        return null;
      },
    );
  }

  /// Delete an agent profile (`unregister:agent {id}`).
  void deleteAgent(int id) {
    _ref.read(wsClientProvider).send({'type': 'unregister:agent', 'id': id});
    state = state.where((a) => a.id != id).toList();
  }

  /// Update an agent profile (`update:agent {id, ...}`).
  /// Pass `modelId: ''` to clear the per-agent override (→ global model).
  void updateAgent(int id, {String? name, String? corePrompt, String? modelId}) {
    _ref.read(wsClientProvider).send({
      'type': 'update:agent',
      'id': id,
      'name': ?name,
      'corePrompt': ?corePrompt,
      if (modelId != null) 'modelId': modelId.isEmpty ? null : modelId,
    });
  }

  /// First non-schedule agent (the implicit default for a fresh chat).
  AgentInfo? get defaultAgent {
    for (final a in state) {
      if (!a.isSchedule) return a;
    }
    return state.isNotEmpty ? state.first : null;
  }

  @override
  void dispose() {
    _statusSub.cancel();
    _eventSub.cancel();
    super.dispose();
  }
}

final agentsProvider =
    StateNotifierProvider<AgentsNotifier, List<AgentInfo>>(
      (ref) => AgentsNotifier(ref),
    );

/// The currently-selected chat jid (lifted out of ChatScreen so New Chat and
/// the SessionList can drive it).
final selectedJidProvider = StateProvider<String?>((ref) => null);
