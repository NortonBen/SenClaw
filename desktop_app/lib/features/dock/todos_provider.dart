import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/transport/connection.dart';
import '../../core/transport/ws_client.dart';

/// A single agent todo (TodoWrite item).
class AgentTodo {
  final String content;
  final String status; // pending | in_progress | completed
  final String activeForm;
  const AgentTodo(this.content, this.status, this.activeForm);

  factory AgentTodo.fromJson(Map<String, dynamic> j) => AgentTodo(
        '${j['content'] ?? ''}',
        '${j['status'] ?? 'pending'}',
        '${j['activeForm'] ?? ''}',
      );
}

/// Live per-agent todo lists from the daemon's `agent:todos` WS event
/// (mirrors the web AgentTodoPanel). Keyed by agent jid.
class AgentTodosNotifier extends StateNotifier<Map<String, List<AgentTodo>>> {
  AgentTodosNotifier(this._ref) : super(const {}) {
    _sub = _ref.read(wsClientProvider).events.listen(_onEvent);
  }
  final Ref _ref;
  late final dynamic _sub;

  void _onEvent(WsEvent e) {
    if (e['type'] != 'agent:todos') return;
    final jid = '${e['agentJid'] ?? e['jid'] ?? 'agent'}';
    final todos = ((e['todos'] as List?) ?? const [])
        .whereType<Map>()
        .map((m) => AgentTodo.fromJson(m.cast<String, dynamic>()))
        .toList();
    final next = {...state};
    if (todos.isEmpty) {
      next.remove(jid);
    } else {
      next[jid] = todos;
    }
    state = next;
  }

  /// Dismiss one agent's todo list from the console (local only — the daemon
  /// will re-push if that agent emits new todos).
  void remove(String jid) {
    if (!state.containsKey(jid)) return;
    final next = {...state}..remove(jid);
    state = next;
  }

  /// Clear every agent's todo list from the console.
  void clear() => state = const {};

  @override
  void dispose() {
    _sub.cancel();
    super.dispose();
  }
}

final agentTodosProvider =
    StateNotifierProvider<AgentTodosNotifier, Map<String, List<AgentTodo>>>(
  (ref) => AgentTodosNotifier(ref),
);
