import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/i18n/l10n.dart';
import '../../core/transport/connection.dart';
import '../../models/kanban_models.dart';

/// Which board is open (null = board list).
final openBoardProvider = StateProvider<int?>((ref) => null);

/// Live updates: the daemon pushes `kanban:update {boardId}` over the WS
/// whenever ANY writer changes a board (REST, a dispatcher worker via the
/// kanban-server MCP, …). Watching this provider from the Kanban screen makes
/// the board re-fetch automatically — no manual Refresh.
final kanbanLiveUpdatesProvider = Provider<void>((ref) {
  final sub = ref.read(wsClientProvider).events.listen((e) {
    if (e['type'] != 'kanban:update') return;
    ref.invalidate(kanbanBoardsProvider);
    final id = (e['boardId'] as num?)?.toInt();
    if (id != null) {
      ref.invalidate(kanbanColumnsProvider(id));
      ref.invalidate(kanbanActivityProvider(id));
    } else {
      ref.invalidate(kanbanColumnsProvider);
      ref.invalidate(kanbanActivityProvider);
    }
    // Refresh any open card detail too (worker comments/status may have changed).
    ref.invalidate(kanbanCardDetailProvider);
  });
  ref.onDispose(sub.cancel);
});

/// Group the board's columns into worker lanes by assignee.
final workerLanesProvider = StateProvider<bool>((ref) => true);

/// Filter the board to one assignee (null = all).
final assigneeFilterProvider = StateProvider<String?>((ref) => null);

final kanbanBoardsProvider = FutureProvider<List<KanbanBoard>>((ref) async {
  final r = await ref.read(apiClientProvider).get('/api/kanban/boards');
  return (r is List ? r : const [])
      .whereType<Map>()
      .map((m) => KanbanBoard.fromJson(m.cast<String, dynamic>()))
      .toList();
});

/// Column templates (builtin + custom) for board creation & the Plugins manager.
final kanbanTemplatesProvider = FutureProvider<List<KanbanTemplate>>((ref) async {
  final r = await ref.read(apiClientProvider).get('/api/kanban/templates');
  return (r is List ? r : const [])
      .whereType<Map>()
      .map((m) => KanbanTemplate.fromJson(m.cast<String, dynamic>()))
      .toList();
});

/// Worker profiles (personas) — used for the assignee dropdown. Empty assignee
/// means the task runs on the default profile.
final workerProfilesProvider = FutureProvider<List<WorkerProfile>>((ref) async {
  final r = await ref.read(apiClientProvider).get('/api/cowork/personas');
  return (r is List ? r : const [])
      .whereType<Map>()
      .map((m) => WorkerProfile.fromJson(m.cast<String, dynamic>()))
      .toList();
});

/// Live activity for a board (running tasks + recent worker feed).
final kanbanActivityProvider =
    FutureProvider.family<KanbanActivity, int>((ref, boardId) async {
  final r = await ref
      .read(apiClientProvider)
      .get('/api/kanban/activity', query: {'board_id': '$boardId'});
  return KanbanActivity.fromJson((r as Map).cast<String, dynamic>());
});

/// Whether the board's right-hand activity drawer is open.
final activityDrawerProvider = StateProvider<bool>((ref) => false);

final kanbanColumnsProvider =
    FutureProvider.family<List<KanbanColumn>, int>((ref, boardId) async {
  final r =
      await ref.read(apiClientProvider).get('/api/kanban/board', query: {'id': '$boardId'});
  final cols = (r is Map ? r['columns'] : null) as List? ?? const [];
  return cols
      .whereType<Map>()
      .map((m) => KanbanColumn.fromJson(m.cast<String, dynamic>()))
      .toList();
});

final kanbanCardDetailProvider =
    FutureProvider.family<KanbanCardDetail, int>((ref, cardId) async {
  final r =
      await ref.read(apiClientProvider).get('/api/kanban/card', query: {'id': '$cardId'});
  return KanbanCardDetail.fromJson((r as Map).cast<String, dynamic>());
});

/// Mutations — thin wrappers that hit /api/kanban/* and invalidate the board.
class KanbanApi {
  KanbanApi(this._ref);
  final Ref _ref;

  dynamic get _client => _ref.read(apiClientProvider);

  void _refreshBoard(int boardId) {
    _ref.invalidate(kanbanColumnsProvider(boardId));
    _ref.invalidate(kanbanBoardsProvider);
  }

  Future<int?> createBoard(String title,
      {bool withDefaults = true, String? templateId, String? workspaceDir}) async {
    final r = await _client.post('/api/kanban/boards', body: {
      'title': title,
      'with_defaults': withDefaults,
      'template_id': ?templateId,
      if (workspaceDir != null && workspaceDir.isNotEmpty)
        'workspace_dir': workspaceDir,
    });
    _ref.invalidate(kanbanBoardsProvider);
    return (r is Map ? r['id'] as num? : null)?.toInt();
  }

  Future<String?> saveTemplate(
      String name, String description, List<KanbanTemplateColumn> columns) async {
    final r = await _client.post('/api/kanban/templates', body: {
      'name': name,
      'description': description,
      'columns': [for (final c in columns) c.toJson()],
    });
    _ref.invalidate(kanbanTemplatesProvider);
    return r is Map ? r['id'] as String? : null;
  }

  Future<void> deleteTemplate(String id) async {
    await _client.post('/api/kanban/templates/delete', body: {'id': id});
    _ref.invalidate(kanbanTemplatesProvider);
  }

  Future<void> deleteBoard(int id) async {
    await _client.post('/api/kanban/board/delete', body: {'id': id});
    _ref.invalidate(kanbanBoardsProvider);
  }

  Future<void> renameBoard(int id, String title, String description) async {
    await _client.post('/api/kanban/board/rename',
        body: {'id': id, 'title': title, 'description': description});
    _refreshBoard(id);
  }

  Future<void> addColumn(int boardId, String title,
      {String role = 'custom'}) async {
    await _client.post('/api/kanban/column/add', body: {
      'board_id': boardId,
      'title': title,
      'role': role,
    });
    _refreshBoard(boardId);
  }

  Future<void> deleteColumn(int boardId, int columnId) async {
    await _client.post('/api/kanban/column/delete', body: {'id': columnId});
    _refreshBoard(boardId);
  }

  Future<void> addCard(int boardId, int columnId, String title,
      {String? assignee, String? priority}) async {
    await _client.post('/api/kanban/card/add', body: {
      'column_id': columnId,
      'title': title,
      if (assignee != null && assignee.isNotEmpty) 'assignee': assignee,
      if (priority != null && priority.isNotEmpty) 'priority': priority,
    });
    _refreshBoard(boardId);
  }

  Future<void> moveCard(int boardId, int cardId, int columnId,
      {int index = 0}) async {
    await _client.post('/api/kanban/card/move',
        body: {'id': cardId, 'column_id': columnId, 'index': index});
    _refreshBoard(boardId);
  }

  Future<void> updateCard(int boardId, int cardId, Map<String, dynamic> patch,
      {bool refreshDetail = true}) async {
    await _client.post('/api/kanban/card/update', body: {'id': cardId, ...patch});
    _refreshBoard(boardId);
    if (refreshDetail) _ref.invalidate(kanbanCardDetailProvider(cardId));
  }

  Future<void> deleteCard(int boardId, int cardId) async {
    await _client.post('/api/kanban/card/delete', body: {'id': cardId});
    _refreshBoard(boardId);
  }

  Future<void> completeCard(int boardId, int cardId, String summary) async {
    await _client.post('/api/kanban/card/complete',
        body: {'card_id': cardId, 'summary': summary});
    _refreshBoard(boardId);
    _ref.invalidate(kanbanCardDetailProvider(cardId));
  }

  Future<void> blockCard(int boardId, int cardId, String reason) async {
    await _client
        .post('/api/kanban/card/block', body: {'card_id': cardId, 'reason': reason});
    _refreshBoard(boardId);
    _ref.invalidate(kanbanCardDetailProvider(cardId));
  }

  Future<void> unblockCard(int boardId, int cardId) async {
    await _client.post('/api/kanban/card/unblock', body: {'card_id': cardId});
    _refreshBoard(boardId);
    _ref.invalidate(kanbanCardDetailProvider(cardId));
  }

  Future<void> comment(int boardId, int cardId, String body) async {
    // The author is a display label stored alongside the comment (the daemon
    // never matches on it), so it follows the UI language.
    await _client.post('/api/kanban/card/comment',
        body: {'card_id': cardId, 'body': body, 'author': L10n.global.t('You')});
    _ref.invalidate(kanbanCardDetailProvider(cardId));
    _refreshBoard(boardId);
  }

  /// AI-plan a board from a goal. `templateId` null/"ai" = AI generates the
  /// columns too; otherwise columns come from the template and AI generates
  /// only the task cards.
  Future<int?> generateBoard(String goal,
      {int? boardId, String? templateId, String? workspaceDir}) async {
    final r = await _client.post('/api/kanban/generate', body: {
      'goal': goal,
      'board_id': ?boardId,
      'template_id': ?templateId,
      if (workspaceDir != null && workspaceDir.isNotEmpty)
        'workspace_dir': workspaceDir,
    });
    _ref.invalidate(kanbanBoardsProvider);
    final id = (r is Map ? r['boardId'] as num? : null)?.toInt();
    if (id != null) _ref.invalidate(kanbanColumnsProvider(id));
    return id;
  }

  /// AI-break a card into subtasks (inserted into the same column).
  Future<void> breakdownCard(int boardId, int cardId) async {
    await _client.post('/api/kanban/breakdown', body: {'card_id': cardId});
    _refreshBoard(boardId);
  }
}

final kanbanApiProvider = Provider<KanbanApi>((ref) => KanbanApi(ref));
