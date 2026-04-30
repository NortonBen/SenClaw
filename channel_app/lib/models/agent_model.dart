class AgentInfo {
  final String jid;
  final String folder;
  final String name;
  final String channel;
  final bool isAdmin;

  const AgentInfo({
    required this.jid,
    required this.folder,
    required this.name,
    required this.channel,
    this.isAdmin = false,
  });

  factory AgentInfo.fromJson(Map<String, dynamic> json) => AgentInfo(
        jid: json['jid'] as String? ?? '',
        folder: json['folder'] as String? ?? '',
        name: json['name'] as String? ?? '',
        channel: json['channel'] as String? ?? '',
        isAdmin: json['isAdmin'] as bool? ?? false,
      );

  @override
  String toString() => 'AgentInfo(name=$name, folder=$folder)';
}

class HistoryMessage {
  final String id;
  final String sender;
  final String content;
  final String timestamp;
  final bool isFromMe;
  final bool isBotReply;
  final String role;

  const HistoryMessage({
    required this.id,
    required this.sender,
    required this.content,
    required this.timestamp,
    required this.isFromMe,
    required this.isBotReply,
    this.role = 'user',
  });

  factory HistoryMessage.fromJson(Map<String, dynamic> json) => HistoryMessage(
        id: (json['id'] ?? '').toString(),
        sender: (json['sender'] ?? '').toString(),
        content: (json['content'] ?? '').toString(),
        timestamp: (json['timestamp'] ?? '').toString(),
        isFromMe: json['isFromMe'] as bool? ?? false,
        isBotReply: json['isBotReply'] as bool? ?? false,
        role: (json['role'] ?? (json['isBotReply'] == true ? 'agent' : 'user'))
            .toString(),
      );
}
