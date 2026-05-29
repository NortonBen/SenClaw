// Models mirroring the daemon's `/api/code/*` JSON shapes (see
// src/gateway/ui_server/code.rs). Field names match the Rust serialisation
// exactly (snake_case) so decoding over the relay tunnel is lossless.

class CodeSession {
  final String id;
  final String name;
  final String workspace;
  final String? language;
  final String status;
  final bool gitEnabled;
  final int createdAt;
  final int updatedAt;

  const CodeSession({
    required this.id,
    required this.name,
    required this.workspace,
    this.language,
    required this.status,
    required this.gitEnabled,
    required this.createdAt,
    required this.updatedAt,
  });

  factory CodeSession.fromJson(Map<String, dynamic> j) => CodeSession(
    id: (j['id'] ?? '').toString(),
    name: (j['name'] ?? '').toString(),
    workspace: (j['workspace'] ?? '').toString(),
    language: j['language'] as String?,
    status: (j['status'] ?? 'active').toString(),
    gitEnabled: j['git_enabled'] as bool? ?? false,
    createdAt: (j['created_at'] as num?)?.toInt() ?? 0,
    updatedAt: (j['updated_at'] as num?)?.toInt() ?? 0,
  );
}

class FileNode {
  final String name;
  final String path; // relative to workspace root
  final bool isDir;
  final List<FileNode> children;

  const FileNode({
    required this.name,
    required this.path,
    required this.isDir,
    this.children = const [],
  });

  factory FileNode.fromJson(Map<String, dynamic> j) => FileNode(
    name: (j['name'] ?? '').toString(),
    path: (j['path'] ?? '').toString(),
    isDir: (j['type'] ?? 'file') == 'dir',
    children: ((j['children'] as List?) ?? const [])
        .map((e) => FileNode.fromJson(e as Map<String, dynamic>))
        .toList(),
  );
}

class GitCommit {
  final String hash;
  final String message;
  final String date;

  const GitCommit({
    required this.hash,
    required this.message,
    required this.date,
  });

  factory GitCommit.fromJson(Map<String, dynamic> j) => GitCommit(
    hash: (j['hash'] ?? '').toString(),
    message: (j['message'] ?? '').toString(),
    date: (j['date'] ?? '').toString(),
  );

  String get shortHash => hash.length >= 7 ? hash.substring(0, 7) : hash;
}

class CodeChatGroup {
  final String id;
  final String projectId;
  final String name;
  final int createdAt;
  final int updatedAt;

  const CodeChatGroup({
    required this.id,
    required this.projectId,
    required this.name,
    required this.createdAt,
    required this.updatedAt,
  });

  factory CodeChatGroup.fromJson(Map<String, dynamic> j) => CodeChatGroup(
    id: (j['id'] ?? '').toString(),
    projectId: (j['project_id'] ?? '').toString(),
    name: (j['name'] ?? '').toString(),
    createdAt: (j['created_at'] as num?)?.toInt() ?? 0,
    updatedAt: (j['updated_at'] as num?)?.toInt() ?? 0,
  );
}

class CodeChatMessage {
  final String id;
  final String role; // 'user' | 'assistant' | 'system' | …
  final String content;
  final String status; // 'queued' | 'processing' | 'done' | 'error' | …
  final int? queuePosition;
  final String? dagPlan;
  final int createdAt;
  final int? processedAt;

  const CodeChatMessage({
    required this.id,
    required this.role,
    required this.content,
    required this.status,
    this.queuePosition,
    this.dagPlan,
    required this.createdAt,
    this.processedAt,
  });

  bool get isUser => role == 'user';
  bool get isPending => status == 'queued' || status == 'processing';

  factory CodeChatMessage.fromJson(Map<String, dynamic> j) => CodeChatMessage(
    id: (j['id'] ?? '').toString(),
    role: (j['role'] ?? 'assistant').toString(),
    content: (j['content'] ?? '').toString(),
    status: (j['status'] ?? '').toString(),
    queuePosition: (j['queue_position'] as num?)?.toInt(),
    dagPlan: j['dag_plan'] as String?,
    createdAt: (j['created_at'] as num?)?.toInt() ?? 0,
    processedAt: (j['processed_at'] as num?)?.toInt(),
  );
}

/// A directory listing entry from `/api/fs/ls` (folder picker).
class FsEntry {
  final String name;
  final String path;
  const FsEntry({required this.name, required this.path});

  factory FsEntry.fromJson(Map<String, dynamic> j) => FsEntry(
    name: (j['name'] ?? '').toString(),
    path: (j['path'] ?? '').toString(),
  );
}

class FsListing {
  final String current;
  final String? parent;
  final List<FsEntry> dirs;
  const FsListing({required this.current, this.parent, required this.dirs});

  factory FsListing.fromJson(Map<String, dynamic> j) => FsListing(
    current: (j['current'] ?? '').toString(),
    parent: j['parent'] as String?,
    dirs: ((j['dirs'] as List?) ?? const [])
        .map((e) => FsEntry.fromJson(e as Map<String, dynamic>))
        .toList(),
  );
}
