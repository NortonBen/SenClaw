import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/transport/connection.dart';
import '../../core/transport/ws_client.dart';

class PlanSummary {
  final String id;
  final String title;
  final String status;
  final int? createdAt;
  const PlanSummary(this.id, this.title, this.status, this.createdAt);
  factory PlanSummary.fromJson(Map<String, dynamic> j) => PlanSummary(
        '${j['id'] ?? ''}',
        '${j['title'] ?? 'Untitled plan'}',
        '${j['status'] ?? ''}',
        (j['createdAt'] as num?)?.toInt(),
      );
}

class PlanHistoryState {
  final List<PlanSummary> summaries;
  final Map<String, String> contentById; // id → markdown
  const PlanHistoryState({this.summaries = const [], this.contentById = const {}});
}

/// Plan history for a group (web PlanHistoryPanel): plans live in SQLite, so
/// this replays across restarts. `plan:list`→`plans:list`, `plan:get`→`plans:get`.
class PlanHistoryNotifier extends StateNotifier<PlanHistoryState> {
  PlanHistoryNotifier(this._ref) : super(const PlanHistoryState()) {
    _sub = _ref.read(wsClientProvider).events.listen(_onEvent);
  }
  final Ref _ref;
  late final dynamic _sub;

  void requestList(String jid) {
    _ref.read(wsClientProvider).send({'type': 'plan:list', 'groupJid': jid});
  }

  void requestGet(String id) {
    _ref.read(wsClientProvider).send({'type': 'plan:get', 'id': id});
  }

  void _onEvent(WsEvent e) {
    switch (e['type']) {
      case 'plans:list':
        final plans = ((e['plans'] as List?) ?? const [])
            .whereType<Map>()
            .map((m) => PlanSummary.fromJson(m.cast<String, dynamic>()))
            .toList();
        state = PlanHistoryState(
            summaries: plans, contentById: state.contentById);
      case 'plans:get':
        final p = (e['plan'] as Map?)?.cast<String, dynamic>();
        if (p == null) return;
        state = PlanHistoryState(
          summaries: state.summaries,
          contentById: {
            ...state.contentById,
            '${p['id']}': '${p['contentMd'] ?? p['content'] ?? ''}',
          },
        );
    }
  }

  @override
  void dispose() {
    _sub.cancel();
    super.dispose();
  }
}

final planHistoryProvider =
    StateNotifierProvider<PlanHistoryNotifier, PlanHistoryState>(
  (ref) => PlanHistoryNotifier(ref),
);
