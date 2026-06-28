/// A chat/group entry shown in the sidebar (mirrors WS `groups` payload).
class GroupInfo {
  final String jid;
  final String name;
  final String? folder;
  final String? lastMessage;
  final int? lastActivity;
  final int unread;
  final String? modelId;
  final String? groupType;
  /// The chat's project workspace dir (first of `allowedWorkDirs`), if any —
  /// drives the per-session Files browser. Null for non-code chats.
  final String? workDir;

  const GroupInfo({
    required this.jid,
    required this.name,
    this.folder,
    this.lastMessage,
    this.lastActivity,
    this.unread = 0,
    this.modelId,
    this.groupType,
    this.workDir,
  });

  factory GroupInfo.fromJson(Map<String, dynamic> j) => GroupInfo(
    jid: (j['jid'] ?? j['groupJid'] ?? '').toString(),
    name: (j['name'] ?? j['title'] ?? j['jid'] ?? 'Untitled').toString(),
    folder: j['folder']?.toString(),
    lastMessage: j['lastMessage']?.toString(),
    lastActivity: (j['lastActivity'] as num?)?.toInt(),
    unread: (j['unread'] as num?)?.toInt() ?? 0,
    modelId: j['modelId']?.toString(),
    groupType: j['groupType']?.toString(),
    workDir: () {
      final dirs = j['allowedWorkDirs'];
      if (dirs is List && dirs.isNotEmpty) return '${dirs.first}';
      return null;
    }(),
  );

  GroupInfo copyWith({int? lastActivity}) => GroupInfo(
        jid: jid,
        name: name,
        folder: folder,
        lastMessage: lastMessage,
        lastActivity: lastActivity ?? this.lastActivity,
        unread: unread,
        modelId: modelId,
        groupType: groupType,
        workDir: workDir,
      );
}
