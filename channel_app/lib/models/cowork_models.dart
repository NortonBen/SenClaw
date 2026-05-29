// Models mirroring `/api/cowork/*` JSON shapes. The Rust structs serialise
// with `#[serde(rename_all = "camelCase")]` (src/types.rs) and timestamps are
// ISO-8601 *strings* (unlike the int epochs used by code/space).

class CoworkWorkspace {
  final String id;
  final String name;
  final String? description;
  final String status;
  final String rootDir;
  final String? workingDir;
  final String createdAt;
  final String updatedAt;

  const CoworkWorkspace({
    required this.id,
    required this.name,
    this.description,
    required this.status,
    this.rootDir = '',
    this.workingDir,
    this.createdAt = '',
    this.updatedAt = '',
  });

  factory CoworkWorkspace.fromJson(Map<String, dynamic> j) => CoworkWorkspace(
    id: (j['id'] ?? '').toString(),
    name: (j['name'] ?? '').toString(),
    description: j['description'] as String?,
    status: (j['status'] ?? '').toString(),
    rootDir: (j['rootDir'] ?? '').toString(),
    workingDir: j['workingDir'] as String?,
    createdAt: (j['createdAt'] ?? '').toString(),
    updatedAt: (j['updatedAt'] ?? '').toString(),
  );
}

class CoworkTask {
  final String id;
  final String workspaceId;
  final String title;
  final String? description;
  final String status;
  final String? assignee;
  final String? reviewer;
  final String priority;
  final String createdBy;
  final String createdAt;
  final String? resultOutput;

  const CoworkTask({
    required this.id,
    required this.workspaceId,
    required this.title,
    this.description,
    required this.status,
    this.assignee,
    this.reviewer,
    this.priority = 'normal',
    this.createdBy = '',
    this.createdAt = '',
    this.resultOutput,
  });

  factory CoworkTask.fromJson(Map<String, dynamic> j) => CoworkTask(
    id: (j['id'] ?? '').toString(),
    workspaceId: (j['workspaceId'] ?? '').toString(),
    title: (j['title'] ?? '').toString(),
    description: j['description'] as String?,
    status: (j['status'] ?? 'todo').toString(),
    assignee: j['assignee'] as String?,
    reviewer: j['reviewer'] as String?,
    priority: (j['priority'] ?? 'normal').toString(),
    createdBy: (j['createdBy'] ?? '').toString(),
    createdAt: (j['createdAt'] ?? '').toString(),
    resultOutput: j['resultOutput'] as String?,
  );
}

class CoworkMessage {
  final String id;
  final String workspaceId;
  final String fromMember;
  final String? toMember;
  final String messageType;
  final String content;
  final String? taskId;
  final bool isRead;
  final String createdAt;

  const CoworkMessage({
    required this.id,
    required this.workspaceId,
    required this.fromMember,
    this.toMember,
    this.messageType = 'status',
    required this.content,
    this.taskId,
    this.isRead = false,
    this.createdAt = '',
  });

  factory CoworkMessage.fromJson(Map<String, dynamic> j) => CoworkMessage(
    id: (j['id'] ?? '').toString(),
    workspaceId: (j['workspaceId'] ?? '').toString(),
    fromMember: (j['fromMember'] ?? '').toString(),
    toMember: j['toMember'] as String?,
    messageType: (j['messageType'] ?? 'status').toString(),
    content: (j['content'] ?? '').toString(),
    taskId: j['taskId'] as String?,
    isRead: j['isRead'] as bool? ?? false,
    createdAt: (j['createdAt'] ?? '').toString(),
  );
}

class CoworkFile {
  final String name;
  final String path; // relative to workspace root
  final bool isDir;
  final int size;

  const CoworkFile({
    required this.name,
    required this.path,
    required this.isDir,
    this.size = 0,
  });

  factory CoworkFile.fromJson(Map<String, dynamic> j) => CoworkFile(
    name: (j['name'] ?? '').toString(),
    path: (j['path'] ?? '').toString(),
    isDir: j['isDir'] as bool? ?? false,
    size: (j['size'] as num?)?.toInt() ?? 0,
  );

  int get depth => path.split('/').where((s) => s.isNotEmpty).length - 1;
}

class CoworkMember {
  final String workspaceId;
  final String memberId;
  final String role;
  final String? persona;
  final String? responsibilities;

  const CoworkMember({
    required this.workspaceId,
    required this.memberId,
    required this.role,
    this.persona,
    this.responsibilities,
  });

  factory CoworkMember.fromJson(Map<String, dynamic> j) => CoworkMember(
    workspaceId: (j['workspaceId'] ?? '').toString(),
    memberId: (j['memberId'] ?? '').toString(),
    role: (j['role'] ?? '').toString(),
    persona: j['persona'] as String?,
    responsibilities: j['responsibilities'] as String?,
  );
}

class CoworkBoardEntry {
  final String id;
  final String workspaceId;
  final String section;
  final String? title;
  final String content;
  final String author;
  final bool pinned;
  final String createdAt;
  final String updatedAt;

  const CoworkBoardEntry({
    required this.id,
    required this.workspaceId,
    required this.section,
    this.title,
    required this.content,
    this.author = '',
    this.pinned = false,
    this.createdAt = '',
    this.updatedAt = '',
  });

  factory CoworkBoardEntry.fromJson(Map<String, dynamic> j) => CoworkBoardEntry(
    id: (j['id'] ?? '').toString(),
    workspaceId: (j['workspaceId'] ?? '').toString(),
    section: (j['section'] ?? '').toString(),
    title: j['title'] as String?,
    content: (j['content'] ?? '').toString(),
    author: (j['author'] ?? '').toString(),
    pinned: j['pinned'] as bool? ?? false,
    createdAt: (j['createdAt'] ?? '').toString(),
    updatedAt: (j['updatedAt'] ?? '').toString(),
  );
}
