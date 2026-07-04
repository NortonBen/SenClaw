import 'dart:async';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/transport/connection.dart';
import '../../core/transport/ws_client.dart';
import '../../models/chat_message.dart';

/// Per-conversation state: the message list + the agent's run state + mode.
class ConversationState {
  final List<ChatMessage> messages;
  final String agentState; // 'idle' | 'processing' | ...
  final String agentMode; // 'Agent' | 'Plan' | 'Dag'
  const ConversationState({
    this.messages = const [],
    this.agentState = 'idle',
    this.agentMode = 'Agent',
  });

  bool get busy => agentState != 'idle' && agentState.isNotEmpty;

  ConversationState copyWith({
    List<ChatMessage>? messages,
    String? agentState,
    String? agentMode,
  }) => ConversationState(
    messages: messages ?? this.messages,
    agentState: agentState ?? this.agentState,
    agentMode: agentMode ?? this.agentMode,
  );
}

/// Owns one chat group's live state. Subscribes to the group on the WS gateway
/// and folds every relevant event into [ConversationState] — the Flutter analog
/// of the React `useWebSocket` per-jid reducer.
class ConversationNotifier extends StateNotifier<ConversationState> {
  ConversationNotifier(this._ref, this.jid)
    : super(const ConversationState()) {
    final ws = _ref.read(wsClientProvider);
    ws.subscribe(jid); // triggers history:load from the daemon
    _sub = ws.events.listen(_onEvent);
  }

  final Ref _ref;
  final String jid;
  late final StreamSubscription _sub;

  String get _streamId => 'agent-stream-$jid';

  void _onEvent(WsEvent e) {
    final type = e['type'];
    // Most events are group-scoped; ignore others' traffic.
    if (e['groupJid'] != null && e['groupJid'] != jid) return;

    switch (type) {
      case 'history:load':
        _onHistory(e);
      case 'incoming':
        if (e['isFromMe'] != true) {
          _add(ChatMessage(
            id: 'in-${e['timestamp'] ?? DateTime.now().microsecondsSinceEpoch}',
            kind: MessageKind.other,
            sender: e['senderName'] as String?,
            text: '${e['text'] ?? ''}',
            ts: e['timestamp'] as String?,
          ));
        }
      case 'agent:delta':
        _onDelta('${e['delta'] ?? ''}', e['ts'] as String?);
      case 'agent:reply':
        _onReply(e);
      case 'agent:state':
        state = state.copyWith(agentState: '${e['state'] ?? 'idle'}');
      case 'tool:execution':
        _add(ChatMessage(
          id: 'tool-$jid-${DateTime.now().microsecondsSinceEpoch}',
          kind: MessageKind.tool,
          ts: e['ts'] as String?,
          data: {
            'toolName': e['toolName'],
            'title': e['title'],
            'summary': e['summary'],
            'ok': e['ok'],
            'content': e['content'],
          },
        ));
      case 'permission:request':
        _add(ChatMessage(
          id: 'perm-${e['requestId']}',
          kind: MessageKind.permission,
          data: {
            'requestId': e['requestId'],
            'toolName': e['toolName'],
            'title': e['title'],
            'content': e['content'],
            'options': e['options'],
          },
        ));
      case 'permission:resolved':
        _resolve('perm-${e['requestId']}', e['optionKey'] as String?);
      case 'question:request':
        _add(ChatMessage(
          id: 'q-${e['requestId']}',
          kind: MessageKind.question,
          data: {
            'requestId': e['requestId'],
            'agentId': e['agentId'],
            'questions': e['questions'],
          },
        ));
      case 'question:resolved':
        _resolve('q-${e['requestId']}', '');
      case 'form:request':
        _add(ChatMessage(
          id: 'form-${e['requestId']}',
          kind: MessageKind.form,
          data: {
            'requestId': e['requestId'],
            'agentId': e['agentId'],
            'title': e['title'],
            'surface': e['surface'],
            'submitLabel': e['submitLabel'],
            'fields': e['fields'],
          },
        ));
      case 'form:resolved':
        _resolve('form-${e['requestId']}', '');
      case 'agent:mode:changed':
        final m = '${e['mode']}';
        if (m == 'Agent' || m == 'Plan' || m == 'Dag') {
          state = state.copyWith(agentMode: m);
        }
    }
  }

  void _onHistory(WsEvent e) {
    final raw = e['messages'];
    if (raw is! List) return;
    final hydrated = raw.whereType<Map>().map((m) {
      final role = m['role'];
      if (role == 'tool') {
        return ChatMessage(
          id: '${m['id']}',
          kind: MessageKind.tool,
          ts: m['timestamp'] as String?,
          data: {
            'toolName': m['toolName'],
            'title': m['title'],
            'summary': m['summary'],
            'ok': m['ok'],
            'content': m['content'],
          },
        );
      }
      // A non-empty senderName marks a message that came in from another
      // client/channel (e.g. the mobile channel_app "mobile-app") rather than
      // this desktop. Keep it user-side but tag it `other` so the source label
      // renders — matching the live `incoming` path.
      final senderName = m['senderName'] as String?;
      final fromOtherChannel = senderName != null && senderName.isNotEmpty;
      return ChatMessage(
        id: '${m['id']}',
        kind: role == 'agent'
            ? MessageKind.agent
            : fromOtherChannel
                ? MessageKind.other
                : MessageKind.user,
        sender: senderName,
        text: '${m['text'] ?? ''}',
        ts: m['timestamp'] as String?,
      );
    }).toList();
    // Don't clobber optimistic local bubbles with an empty history.
    if (hydrated.isEmpty && state.messages.isNotEmpty) return;
    state = state.copyWith(messages: hydrated);
  }

  /// Wipe the local message list NOW. Used by "Clear all messages" — the
  /// daemon's `stop_and_clear` pushes an empty `history:load`, but
  /// [_onHistory] deliberately ignores empty lists to protect optimistic
  /// bubbles, so the caller clears locally and the (now empty) DB keeps it
  /// clean across reloads.
  void clearLocal() {
    state = state.copyWith(messages: const []);
  }

  void _onDelta(String delta, String? ts) {
    final list = [...state.messages];
    final idx = list.indexWhere((m) => m.id == _streamId);
    if (idx >= 0) {
      list[idx] = list[idx].copyWith(text: (list[idx].text ?? '') + delta);
    } else {
      list.add(ChatMessage(
        id: _streamId,
        kind: MessageKind.agent,
        text: delta,
        ts: ts,
        streaming: true,
      ));
    }
    state = state.copyWith(messages: list);
  }

  void _onReply(WsEvent e) {
    final list = state.messages.where((m) => m.id != _streamId).toList();
    list.add(ChatMessage(
      id: 'agent-${DateTime.now().microsecondsSinceEpoch}',
      kind: MessageKind.agent,
      text: '${e['text'] ?? ''}',
      ts: e['ts'] as String?,
      tokens: (e['tokens'] as num?)?.toInt(),
    ));
    state = state.copyWith(messages: list);
  }

  void _add(ChatMessage m) =>
      state = state.copyWith(messages: [...state.messages, m]);

  void _resolve(String id, String? key) {
    final list = state.messages.map((m) {
      if (m.id != id || m.resolved) return m;
      return m.copyWith(data: {...m.data, 'resolved': true, 'resolvedKey': key});
    }).toList();
    state = state.copyWith(messages: list);
  }

  // ── User actions ───────────────────────────────────────────────────────

  /// Send a user message. [attachments] are `{dataUrl, mimeType}` maps
  /// (base64 data URLs), matching the React `ImageAttachment` shape.
  void sendText(String text, {List<Map<String, String>> attachments = const []}) {
    final trimmed = text.trim();
    if (trimmed.isEmpty && attachments.isEmpty) return;
    final ws = _ref.read(wsClientProvider);
    _add(ChatMessage(
      id: 'user-${DateTime.now().microsecondsSinceEpoch}',
      kind: MessageKind.user,
      text: trimmed,
      ts: DateTime.now().toIso8601String(),
      data: attachments.isEmpty ? const {} : {'attachments': attachments},
    ));
    ws.send({
      'type': 'message',
      'groupJid': jid,
      'text': trimmed,
      if (attachments.isNotEmpty) 'attachments': attachments,
    });
  }

  void resolvePermission(String requestId, String optionKey) {
    _ref.read(wsClientProvider).send({
      'type': 'permission:response',
      'requestId': requestId,
      'optionKey': optionKey,
    });
    _resolve('perm-$requestId', optionKey); // optimistic
  }

  /// answers: `{qIndex: optIndex | [optIndex...]}` (-1 = Other).
  void resolveQuestion(
    String requestId,
    Map<int, dynamic> answers, {
    Map<int, String>? otherTexts,
  }) {
    _ref.read(wsClientProvider).send({
      'type': 'question:response',
      'requestId': requestId,
      'answers': answers.map((k, v) => MapEntry('$k', v)),
      if (otherTexts != null && otherTexts.isNotEmpty)
        'otherTexts': otherTexts.map((k, v) => MapEntry('$k', v)),
    });
    _resolve('q-$requestId', ''); // optimistic
  }

  /// values: keyed by form field `key`; [submitted] = false means the user
  /// skipped the form (parity with web/channel_app FormCard).
  void resolveForm(
    String requestId,
    Map<String, dynamic> values, {
    bool submitted = true,
  }) {
    _ref.read(wsClientProvider).send({
      'type': 'form:response',
      'requestId': requestId,
      'values': values,
      'submitted': submitted,
    });
    _resolve('form-$requestId', ''); // optimistic
  }

  void setAgentMode(String mode) {
    state = state.copyWith(agentMode: mode); // optimistic; server echoes
    _ref.read(wsClientProvider).send({
      'type': 'agent:mode',
      'groupJid': jid,
      'mode': mode,
    });
  }

  void stop() => _ref.read(wsClientProvider).send({
    'type': 'agent:control',
    'groupJid': jid,
    'action': 'stop',
  });

  @override
  void dispose() {
    _sub.cancel();
    super.dispose();
  }
}

/// Keyed by group jid. `.autoDispose` so leaving a chat releases its listener.
final conversationProvider = StateNotifierProvider.autoDispose
    .family<ConversationNotifier, ConversationState, String>(
      (ref, jid) {
        ref.keepAlive(); // survive brief pane rebuilds; GC'd when truly unused
        return ConversationNotifier(ref, jid);
      },
    );
