import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/transport/connection.dart';

/// Token/context usage for one agent (from `agent:usage` events).
class AgentUsage {
  final int useTokens;
  final int maxTokens;
  final int promptTokens;
  const AgentUsage(this.useTokens, this.maxTokens, this.promptTokens);

  double get pct =>
      maxTokens > 0 ? (useTokens / maxTokens).clamp(0.0, 1.0) : 0.0;
  int get remaining => (maxTokens - useTokens).clamp(0, maxTokens);
}

/// Global jid→usage map, updated from every `agent:usage` event so the chat
/// header can show a live context-window meter (web ChatView usage indicator).
class AgentUsageNotifier extends StateNotifier<Map<String, AgentUsage>> {
  AgentUsageNotifier(this._ref) : super(const {}) {
    _sub = _ref.read(wsClientProvider).events.listen((e) {
      if (e['type'] == 'agent:usage' && e['agentJid'] != null) {
        final u = (e['usage'] as Map?)?.cast<String, dynamic>() ?? const {};
        state = {
          ...state,
          '${e['agentJid']}': AgentUsage(
            (u['useTokens'] as num?)?.toInt() ?? 0,
            (u['maxTokens'] as num?)?.toInt() ?? 0,
            (u['promptTokens'] as num?)?.toInt() ?? 0,
          ),
        };
      }
    });
  }
  final Ref _ref;
  late final dynamic _sub;

  @override
  void dispose() {
    _sub.cancel();
    super.dispose();
  }
}

final agentUsageProvider =
    StateNotifierProvider<AgentUsageNotifier, Map<String, AgentUsage>>(
  (ref) => AgentUsageNotifier(ref),
);
