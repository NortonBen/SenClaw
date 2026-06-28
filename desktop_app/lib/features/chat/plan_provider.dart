import 'dart:async';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/transport/connection.dart';
import '../../core/transport/ws_client.dart';

/// A pending plan-mode approval. Global (not per-conversation) — the agent has
/// prepared a plan and is waiting for the user to approve / restart / cancel.
class PlanExitRequest {
  final String groupJid;
  final String agentId;
  final String planContent;
  final String startEditingLabel;
  final String clearContextLabel;

  const PlanExitRequest({
    required this.groupJid,
    required this.agentId,
    required this.planContent,
    required this.startEditingLabel,
    required this.clearContextLabel,
  });
}

/// Holds the active plan-exit request, driven by WS `plan:exit:request`.
/// Cleared on `plan:exit:response` / `plan:implement` or when the user acts.
class PlanExitNotifier extends StateNotifier<PlanExitRequest?> {
  PlanExitNotifier(this._ref) : super(null) {
    _sub = _ref.read(wsClientProvider).events.listen(_onEvent);
  }

  final Ref _ref;
  late final StreamSubscription _sub;

  void _onEvent(WsEvent e) {
    switch (e['type']) {
      case 'plan:exit:request':
        final opts = (e['options'] as Map?)?.cast<String, dynamic>() ?? const {};
        state = PlanExitRequest(
          groupJid: '${e['groupJid'] ?? ''}',
          agentId: '${e['agentId'] ?? 'main'}',
          planContent: '${e['planContent'] ?? ''}',
          startEditingLabel:
              '${opts['startEditing'] ?? 'Approve plan and start editing'}',
          clearContextLabel:
              '${opts['clearContextAndStart'] ?? 'Clear context and start fresh'}',
        );
      case 'plan:exit:response':
      case 'plan:implement':
        state = null;
    }
  }

  /// selected ∈ 'startEditing' | 'clearContextAndStart' | 'cancelled'.
  void resolve(String selected) {
    final req = state;
    if (req == null) return;
    _ref.read(wsClientProvider).send({
      'type': 'plan:exit:response',
      'groupJid': req.groupJid,
      'agentId': req.agentId,
      'selected': selected,
    });
    state = null;
  }

  void dismiss() => state = null;

  @override
  void dispose() {
    _sub.cancel();
    super.dispose();
  }
}

final planExitProvider =
    StateNotifierProvider<PlanExitNotifier, PlanExitRequest?>(
      (ref) => PlanExitNotifier(ref),
    );
