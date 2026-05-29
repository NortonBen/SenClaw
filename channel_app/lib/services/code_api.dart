import '../models/code_models.dart';
import 'api_client.dart';

/// Typed wrapper over the `/api/code/*` and `/api/fs/*` endpoints, tunnelled
/// through the relay. Mirrors web/src/hooks/useCode.ts.
class CodeApi {
  final _api = ApiClient();

  Future<List<CodeSession>> listSessions({String status = 'active'}) async {
    final obj = await _api.getObject(
      ApiClient.withQuery('/api/code/sessions', {'status': status}),
    );
    return ((obj['sessions'] as List?) ?? const [])
        .map((e) => CodeSession.fromJson(e as Map<String, dynamic>))
        .toList();
  }

  Future<CodeSession> createSession({
    required String name,
    required String workspace,
    String? language,
    bool initGit = false,
  }) async {
    final r = await _api.post(
      '/api/code/sessions',
      body: {
        'name': name,
        'workspace': workspace,
        if (language != null && language.isNotEmpty) 'language': language,
        'init_git': initGit,
      },
    );
    return CodeSession.fromJson((r as Map).cast<String, dynamic>());
  }

  Future<void> archiveSession(String id) =>
      _api.delete('/api/code/sessions/$id');

  /// Returns (workspace, fileTree).
  Future<(String, List<FileNode>)> listFiles(String id) async {
    final obj = await _api.getObject('/api/code/sessions/$id/files');
    final tree = ((obj['tree'] as List?) ?? const [])
        .map((e) => FileNode.fromJson(e as Map<String, dynamic>))
        .toList();
    return ((obj['workspace'] ?? '').toString(), tree);
  }

  Future<String> fileContent(String id, String path) async {
    final obj = await _api.getObject(
      ApiClient.withQuery('/api/code/sessions/$id/file-content', {
        'path': path,
      }),
    );
    return (obj['content'] ?? '').toString();
  }

  Future<List<GitCommit>> gitLog(String id) async {
    final obj = await _api.getObject('/api/code/sessions/$id/git-log');
    return ((obj['log'] as List?) ?? const [])
        .map((e) => GitCommit.fromJson(e as Map<String, dynamic>))
        .toList();
  }

  Future<void> rollback(String id, int steps) =>
      _api.post('/api/code/sessions/$id/rollback', body: {'steps': steps});

  Future<List<CodeChatGroup>> listGroups(String projectId) async {
    final obj = await _api.getObject('/api/code/projects/$projectId/groups');
    return ((obj['groups'] as List?) ?? const [])
        .map((e) => CodeChatGroup.fromJson(e as Map<String, dynamic>))
        .toList();
  }

  Future<CodeChatGroup> createGroup(String projectId, String name) async {
    final r = await _api.post(
      '/api/code/projects/$projectId/groups',
      body: {'name': name},
    );
    return CodeChatGroup.fromJson((r as Map).cast<String, dynamic>());
  }

  /// Ensure the project has at least one chat group; returns it.
  Future<CodeChatGroup> ensureDefaultGroup(String projectId) async {
    final groups = await listGroups(projectId);
    if (groups.isNotEmpty) return groups.first;
    return createGroup(projectId, 'Mặc định');
  }

  Future<List<CodeChatMessage>> groupMessages(String groupId) async {
    final obj = await _api.getObject('/api/code/groups/$groupId/messages');
    return ((obj['messages'] as List?) ?? const [])
        .map((e) => CodeChatMessage.fromJson(e as Map<String, dynamic>))
        .toList();
  }

  /// Send a prompt; returns the raw response map (contains messages snapshot).
  Future<Map<String, dynamic>> sendChat({
    required String sessionId,
    required String groupId,
    required String prompt,
  }) async {
    final r = await _api.post(
      '/api/code/sessions/$sessionId/chat',
      body: {'prompt': prompt, 'group_id': groupId},
    );
    return (r as Map).cast<String, dynamic>();
  }

  Future<void> stopCurrent(String groupId) =>
      _api.post('/api/code/groups/$groupId/stop-current');

  Future<FsListing> fsLs({String? path}) async {
    final obj = await _api.getObject(
      ApiClient.withQuery('/api/fs/ls', {'path': path}),
    );
    return FsListing.fromJson(obj);
  }
}
