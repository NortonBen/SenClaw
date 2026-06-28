import 'dart:convert';

/// Discriminated chat message. Mirrors the React union of text / tool /
/// permission / question bubbles; the renderer branches on [kind].
enum MessageKind { user, other, agent, tool, permission, question, system }

class ChatMessage {
  final String id;
  final MessageKind kind;
  final String? text;
  final String? sender;
  final String? ts;

  /// True while an agent reply is still streaming (built from `agent:delta`).
  final bool streaming;

  /// Output tokens this agent message cost (shown as a subtle badge).
  final int? tokens;

  /// Raw payload for non-text kinds (tool card, permission/question request).
  final Map<String, dynamic> data;

  const ChatMessage({
    required this.id,
    required this.kind,
    this.text,
    this.sender,
    this.ts,
    this.streaming = false,
    this.tokens,
    this.data = const {},
  });

  ChatMessage copyWith({
    String? text,
    bool? streaming,
    Map<String, dynamic>? data,
  }) => ChatMessage(
    id: id,
    kind: kind,
    text: text ?? this.text,
    sender: sender,
    ts: ts,
    streaming: streaming ?? this.streaming,
    tokens: tokens,
    data: data ?? this.data,
  );

  /// Image attachments ({dataUrl, mimeType}) on user/agent messages.
  List<Map<String, dynamic>> get attachments =>
      ((data['attachments'] as List?) ?? const [])
          .whereType<Map>()
          .map((e) => e.cast<String, dynamic>())
          .toList();

  // ── Tool accessors ─────────────────────────────────────────────────────
  String get toolName => '${data['toolName'] ?? ''}';
  String get toolTitle => '${data['title'] ?? toolName}';
  String get toolSummary => '${data['summary'] ?? ''}';
  bool get toolOk => data['ok'] != false;

  /// Full display-ready tool detail (command + output, diff, matches…). The
  /// daemon sends it as `content` — a string, or JSON we pretty-print.
  String get toolContent {
    final c = data['content'];
    if (c == null) return '';
    if (c is String) return c.trim();
    try {
      return const JsonEncoder.withIndent('  ').convert(c).trim();
    } catch (_) {
      return '$c';
    }
  }

  // ── Permission accessors ───────────────────────────────────────────────
  String get requestId => '${data['requestId'] ?? ''}';
  String get permTitle => '${data['title'] ?? ''}';
  String get permContent => '${data['content'] ?? ''}';
  List<Map<String, dynamic>> get permOptions =>
      ((data['options'] as List?) ?? const [])
          .whereType<Map>()
          .map((e) => e.cast<String, dynamic>())
          .toList();

  /// For permission/question: which option key was chosen (null = pending).
  String? get resolvedKey => data['resolvedKey'] as String?;
  bool get resolved => data['resolved'] == true || resolvedKey != null;

  static MessageKind kindFromRole(String? role) {
    switch (role) {
      case 'user':
        return MessageKind.user;
      case 'other':
        return MessageKind.other;
      case 'tool':
        return MessageKind.tool;
      case 'permission':
        return MessageKind.permission;
      case 'question':
        return MessageKind.question;
      case 'system':
        return MessageKind.system;
      default:
        return MessageKind.agent;
    }
  }
}
