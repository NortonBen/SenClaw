import '../models/cowork_models.dart';
import 'api_client.dart';

/// Typed wrapper over `/api/cowork/*`, tunnelled through the relay.
/// Mirrors the cowork slice of the web UI.
class CoworkApi {
  final _api = ApiClient();

  Future<List<CoworkWorkspace>> listWorkspaces() async {
    final obj = await _api.getObject('/api/cowork/workspaces');
    return ((obj['workspaces'] as List?) ?? const [])
        .map((e) => CoworkWorkspace.fromJson(e as Map<String, dynamic>))
        .toList();
  }

  Future<CoworkWorkspace> createWorkspace({
    required String name,
    String? description,
    String? workingDir,
  }) async {
    final r = await _api.post('/api/cowork/workspaces', body: {
      'name': name,
      if (description != null && description.isNotEmpty)
        'description': description,
      if (workingDir != null && workingDir.isNotEmpty) 'workingDir': workingDir,
    });
    return CoworkWorkspace.fromJson((r as Map).cast<String, dynamic>());
  }

  Future<void> deleteWorkspace(String id) =>
      _api.delete('/api/cowork/workspaces/$id');

  Future<List<CoworkTask>> listTasks(String wsId, {String? status}) async {
    final obj = await _api.getObject(
      ApiClient.withQuery('/api/cowork/workspaces/$wsId/tasks', {
        'status': status,
      }),
    );
    return ((obj['tasks'] as List?) ?? const [])
        .map((e) => CoworkTask.fromJson(e as Map<String, dynamic>))
        .toList();
  }

  Future<void> createTask(
    String wsId, {
    required String title,
    String? description,
    String? assignee,
    String? priority,
  }) =>
      _api.post('/api/cowork/workspaces/$wsId/tasks', body: {
        'title': title,
        if (description != null && description.isNotEmpty)
          'description': description,
        if (assignee != null && assignee.isNotEmpty) 'assignee': assignee,
        if (priority != null && priority.isNotEmpty) 'priority': priority,
        'createdBy': 'mobile',
      });

  Future<void> updateTaskStatus(String wsId, String taskId, String status) =>
      _api.patch('/api/cowork/workspaces/$wsId/tasks/$taskId',
          body: {'status': status});

  Future<void> deleteTask(String wsId, String taskId) =>
      _api.delete('/api/cowork/workspaces/$wsId/tasks/$taskId');

  Future<List<CoworkMessage>> listMessages(String wsId, {int limit = 100}) async {
    final obj = await _api.getObject(
      ApiClient.withQuery('/api/cowork/workspaces/$wsId/messages', {
        'limit': limit,
      }),
    );
    return ((obj['messages'] as List?) ?? const [])
        .map((e) => CoworkMessage.fromJson(e as Map<String, dynamic>))
        .toList();
  }

  /// Note: the send-message body uses snake_case keys (from_member,
  /// message_type) — these fields are NOT renamed on the Rust side.
  Future<void> sendMessage(
    String wsId, {
    required String content,
    String fromMember = 'mobile',
    String messageType = 'status',
  }) =>
      _api.post('/api/cowork/workspaces/$wsId/messages', body: {
        'from_member': fromMember,
        'content': content,
        'message_type': messageType,
      });

  Future<List<CoworkMember>> listMembers(String wsId) async {
    final obj = await _api.getObject('/api/cowork/workspaces/$wsId/members');
    return ((obj['members'] as List?) ?? const [])
        .map((e) => CoworkMember.fromJson(e as Map<String, dynamic>))
        .toList();
  }

  Future<List<CoworkBoardEntry>> getBoard(String wsId) async {
    final obj = await _api.getObject('/api/cowork/workspaces/$wsId/board');
    return ((obj['entries'] as List?) ?? const [])
        .map((e) => CoworkBoardEntry.fromJson(e as Map<String, dynamic>))
        .toList();
  }

  Future<void> updateBoardSection(
    String wsId,
    String section, {
    required String content,
    String? title,
  }) =>
      _api.patch('/api/cowork/workspaces/$wsId/board/$section', body: {
        'content': content,
        if (title != null) 'title': title,
        'author': 'mobile',
      });
}
