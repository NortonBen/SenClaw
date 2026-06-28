import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/transport/connection.dart';

/// Global jid→agentState map (for the SessionList active-state dots). Updated
/// from every `agent:state` event regardless of which chat is open.
class AgentStatesNotifier extends StateNotifier<Map<String, String>> {
  AgentStatesNotifier(this._ref) : super(const {}) {
    _sub = _ref.read(wsClientProvider).events.listen((e) {
      if (e['type'] == 'agent:state' && e['groupJid'] != null) {
        state = {...state, '${e['groupJid']}': '${e['state'] ?? 'idle'}'};
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

final agentStatesProvider =
    StateNotifierProvider<AgentStatesNotifier, Map<String, String>>(
      (ref) => AgentStatesNotifier(ref),
    );

const kActiveStates = {
  'thinking',
  'executing',
  'processing',
  'waiting_permission',
  'waiting_question',
};
