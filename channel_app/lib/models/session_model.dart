/// One chat session for this device, as reported by the daemon's
/// `SESSION_LIST_RESP` control frame. A session is a named conversation backed
/// by its own daemon group jid (`app:<cid>:user:<sender>` for the default, or
/// `app:<cid>:user:<sender>:s-<id>` for additional ones). Mirrors the
/// desktop app's `GroupInfo`, scoped to this device's sessions.
class SessionInfo {
  /// The daemon group jid — keys history, delta-sync and message routing.
  final String jid;

  /// Display name (user-editable).
  final String name;

  /// Bound agent/profile folder for this session.
  final String folder;

  /// 'chat' | 'code' | … (app sessions are 'chat').
  final String groupType;

  /// Last message/activity time, epoch ms. Null = no activity yet.
  final int? lastActivity;

  /// Whether this is the daemon's currently-active session for the device
  /// (i.e. where new messages are filed).
  final bool active;

  const SessionInfo({
    required this.jid,
    required this.name,
    this.folder = '',
    this.groupType = 'chat',
    this.lastActivity,
    this.active = false,
  });

  /// The default (non-deletable) session has no `:s-<id>` suffix.
  bool get isDefault => !jid.contains(':s-');

  String get title => name.isNotEmpty ? name : jid;

  factory SessionInfo.fromJson(Map<String, dynamic> json) => SessionInfo(
        jid: (json['jid'] ?? '').toString(),
        name: (json['name'] ?? '').toString(),
        folder: (json['folder'] ?? '').toString(),
        groupType: (json['groupType'] ?? 'chat').toString(),
        lastActivity: (json['lastActivity'] as num?)?.toInt(),
        active: json['active'] == true,
      );
}
