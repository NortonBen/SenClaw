import 'api_client.dart';

/// One row of `GET /api/chat/history` — a server-authoritative message with
/// the daemon-parsed epoch-ms [ts] used as the incremental-sync cursor.
class ChatHistoryEntry {
  final String id;
  final String sender;
  final String content;
  final int ts;
  final bool isFromMe;
  final bool isBotReply;
  final String role; // 'user' | 'agent'

  const ChatHistoryEntry({
    required this.id,
    required this.sender,
    required this.content,
    required this.ts,
    required this.isFromMe,
    required this.isBotReply,
    required this.role,
  });

  factory ChatHistoryEntry.fromJson(Map<String, dynamic> json) =>
      ChatHistoryEntry(
        id: (json['id'] ?? '').toString(),
        sender: (json['sender'] ?? '').toString(),
        content: (json['content'] ?? '').toString(),
        ts: (json['ts'] as num?)?.toInt() ?? 0,
        isFromMe: json['isFromMe'] == true,
        isBotReply: json['isBotReply'] == true,
        role: (json['role'] ?? '').toString() == 'agent' ? 'agent' : 'user',
      );
}

/// Resolves pending agent interactions (tool-permission requests, ask-question
/// batches) over the relay tunnel — parity with the web WS `permission:response`
/// / `question:response` — plus the chat sync endpoints (delta history +
/// agent-state snapshot).
class ChatApi {
  final _api = ApiClient();

  /// Messages strictly newer than [afterTs] (epoch ms) for [jid], oldest →
  /// newest. Pass 0 for a full (capped) fetch.
  Future<List<ChatHistoryEntry>> fetchHistoryAfter(
    String jid,
    int afterTs, {
    int limit = 200,
  }) async {
    final path = ApiClient.withQuery('/api/chat/history', {
      'jid': jid,
      'after_ts': afterTs,
      'limit': limit,
    });
    final obj = await _api.getObject(path);
    return ((obj['messages'] as List?) ?? const [])
        .whereType<Map>()
        .map((e) => ChatHistoryEntry.fromJson(e.cast<String, dynamic>()))
        .toList();
  }

  /// Authoritative per-group agent states (`jid → 'processing' | 'idle' | …`).
  /// Called on relay (re)connect to reconcile a possibly-stale typing
  /// indicator whose `agent:state` events were lost while the socket was down.
  Future<Map<String, String>> fetchAgentStates() async {
    final obj = await _api.getObject('/api/chat/states');
    final states = obj['states'];
    if (states is! Map) return const {};
    return states.map((k, v) => MapEntry(k.toString(), (v ?? '').toString()));
  }

  Future<void> respondPermission(String requestId, String optionKey) =>
      _api.post('/api/chat/permission/respond',
          body: {'requestId': requestId, 'optionKey': optionKey});

  /// [answers] is `{ "<questionIndex>": optionIndex | [optionIndex, …] }`.
  Future<void> respondQuestion(
    String requestId,
    Map<String, dynamic> answers, {
    Map<String, dynamic>? otherTexts,
  }) =>
      _api.post('/api/chat/question/respond', body: {
        'requestId': requestId,
        'answers': answers,
        'otherTexts': ?otherTexts,
      });

  /// [selected] = 'startEditing' | 'clearContextAndStart' | 'cancelled'.
  Future<void> respondPlan(
    String groupJid,
    String agentId,
    String selected,
  ) =>
      _api.post('/api/chat/plan/respond', body: {
        'groupJid': groupJid,
        'agentId': agentId,
        'selected': selected,
      });
}
