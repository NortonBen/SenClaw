import 'dart:io' show File, Platform;

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:http/http.dart' as http;
import 'package:intl/intl.dart';
import 'package:path_provider/path_provider.dart';
import 'package:record/record.dart';

import '../../core/transport/connection.dart';
import '../../models/chat_message.dart';
import '../../theme/tokens.dart';
import 'audio_service.dart';
import 'conversation_provider.dart';
import 'groups_provider.dart';
import 'widgets/message_widgets.dart';

/// A calendar reminder the user tapped to act on — reschedule, delete, or ask
/// SenClaw to do something else. Carries just enough context (from the
/// `space:event:reminder` WS frame) for the agent to resolve which event.
class ReminderTarget {
  final String? eventId;
  final String title;
  final int? startAtMs;
  final String kind; // 'reminder' | 'renotify' | 'pending'
  final String? notificationId;
  const ReminderTarget({
    this.eventId,
    required this.title,
    this.startAtMs,
    this.kind = 'reminder',
    this.notificationId,
  });
}

/// Set to open the reminder interaction dialog; null closes it. Written by the
/// in-app bell ([NotificationsBell]) and the OS-notification click handler
/// ([SystemNotifier]); watched by [ReminderInteractionOverlay].
final pendingReminderProvider = StateProvider<ReminderTarget?>((ref) => null);

/// Stable jid for the single persistent "Reminders" assistant chat. Every
/// reminder conversation routes here so reminder chatter stays out of the
/// user's other chats while the agent builds up reminder-handling context.
const kRemindersJid = 'web:reminders:main';

/// Ensure the dedicated Reminders chat exists, returning its jid. Registers it
/// once (mirrors [GroupsNotifier.createChat]'s frame) then waits — best-effort,
/// with a timeout — for it to appear in [groupsProvider]. Even if the UI list
/// hasn't caught up, the WS channel is ordered so a following `message` frame
/// to this jid is processed after the registration.
Future<String> ensureRemindersChat(WidgetRef ref) async {
  bool exists() => ref.read(groupsProvider).any((g) => g.jid == kRemindersJid);
  if (exists()) return kRemindersJid;
  ref.read(wsClientProvider).send({
    'type': 'register:group',
    'jid': kRemindersJid,
    'folder': 'reminders',
    'name': 'Reminders',
    'groupType': 'chat',
    'requiresTrigger': false,
  });
  final deadline = DateTime.now().add(const Duration(seconds: 5));
  while (DateTime.now().isBefore(deadline)) {
    await Future.delayed(const Duration(milliseconds: 100));
    if (exists()) return kRemindersJid;
  }
  return kRemindersJid; // daemon almost certainly registered it; proceed.
}

/// Global modal for interacting with a reminder — mounted once over the whole
/// app (via `MaterialApp.builder`, next to [PlanExitOverlay]). Renders nothing
/// until [pendingReminderProvider] is set.
class ReminderInteractionOverlay extends ConsumerWidget {
  const ReminderInteractionOverlay({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final target = ref.watch(pendingReminderProvider);
    if (target == null) return const SizedBox.shrink();
    // Key by the target so reopening a DIFFERENT reminder resets dialog state
    // (fresh context preamble, cleared composer).
    return _ReminderDialog(
      key: ValueKey(
          target.notificationId ?? target.eventId ?? target.title),
      target: target,
    );
  }
}

class _ReminderDialog extends ConsumerStatefulWidget {
  const _ReminderDialog({super.key, required this.target});
  final ReminderTarget target;

  @override
  ConsumerState<_ReminderDialog> createState() => _ReminderDialogState();
}

class _ReminderDialogState extends ConsumerState<_ReminderDialog> {
  final _input = TextEditingController();
  final _scroll = ScrollController();
  final _recorder = AudioRecorder();

  String? _jid;
  ProviderSubscription<ConversationState>? _convoSub;

  bool _recording = false;
  bool _transcribing = false;

  /// The reminder context is injected only on the FIRST turn of this dialog —
  /// after that the agent already has it in history.
  bool _needsPreamble = true;

  /// True after a voice turn until its reply is spoken — makes the reply
  /// modality match the input modality (push-to-talk → spoken answer).
  bool _awaitingVoiceReply = false;
  String? _spokenMsgId;

  @override
  void initState() {
    super.initState();
    _resolveChat();
  }

  Future<void> _resolveChat() async {
    final jid = await ensureRemindersChat(ref);
    if (!mounted) return;
    setState(() => _jid = jid);
    // React to new replies for autoscroll + modality-matched TTS.
    _convoSub =
        ref.listenManual(conversationProvider(jid), (prev, next) {
      _scrollSoon();
      if (!_awaitingVoiceReply) return;
      final reply = _lastCompletedAgent(next);
      if (reply != null &&
          reply.id != _spokenMsgId &&
          (reply.text ?? '').trim().isNotEmpty) {
        _spokenMsgId = reply.id;
        _awaitingVoiceReply = false;
        _speak(reply.text!.trim());
      }
    });
  }

  @override
  void dispose() {
    _convoSub?.close();
    _input.dispose();
    _scroll.dispose();
    _recorder.dispose();
    super.dispose();
  }

  void _close() => ref.read(pendingReminderProvider.notifier).state = null;

  // ── Sending ────────────────────────────────────────────────────────────
  ChatMessage? _lastCompletedAgent(ConversationState s) {
    for (final m in s.messages.reversed) {
      if (m.kind == MessageKind.agent && !m.streaming) return m;
    }
    return null;
  }

  void _sendTurn(String raw, {required bool voice}) {
    final text = raw.trim();
    final jid = _jid;
    if (text.isEmpty || jid == null) return;
    final preamble = _needsPreamble ? _buildPreamble(widget.target) : null;
    _needsPreamble = false;
    if (voice) {
      // Anchor "new reply" detection so we don't re-speak an existing message.
      _spokenMsgId = _lastCompletedAgent(ref.read(conversationProvider(jid)))?.id;
      _awaitingVoiceReply = true;
    } else {
      _awaitingVoiceReply = false; // a typed turn cancels any pending speak.
    }
    ref
        .read(conversationProvider(jid).notifier)
        .sendText(text, contextPreamble: preamble);
    _input.clear();
    _scrollSoon();
  }

  Future<void> _speak(String text) async {
    try {
      await ref.read(audioServiceProvider).speak(text);
    } catch (_) {/* TTS is best-effort */}
  }

  String _buildPreamble(ReminderTarget t) {
    final when =
        t.startAtMs != null ? _fmtWhen(t.startAtMs!) : 'không rõ thời gian';
    final id = t.eventId != null && t.eventId!.isNotEmpty
        ? ' (eventId=${t.eventId})'
        : '';
    return '[Ngữ cảnh nhắc nhở] Người dùng vừa mở nhắc nhở lịch: '
        '"${t.title}"$id, bắt đầu lúc $when. Yêu cầu bên dưới có thể là: '
        'dời thời gian, xoá nhắc nhở/sự kiện, hoặc một việc khác. Hãy dùng '
        'công cụ MCP senclaw-space (event_update / event_delete / '
        'event_reminder_set / event_list) để thực hiện, rồi trả lời ngắn gọn '
        'bằng tiếng Việt.';
  }

  // ── Mic (push-to-talk, auto-send) ────────────────────────────────────────
  Future<void> _toggleMic() async {
    if (_jid == null) return;
    if (_recording) {
      setState(() {
        _recording = false;
        _transcribing = true;
      });
      try {
        final out = await _recorder.stop();
        if (out == null) return;
        Uint8List bytes;
        String filename;
        if (kIsWeb) {
          bytes = (await http.get(Uri.parse(out))).bodyBytes;
          filename = 'recording.webm';
        } else {
          bytes = await File(out).readAsBytes();
          filename = out.split(Platform.pathSeparator).last;
        }
        final text =
            await ref.read(audioServiceProvider).transcribe(bytes, filename);
        if (text.trim().isNotEmpty) _sendTurn(text, voice: true);
      } catch (e) {
        _snack('Nhận dạng giọng nói thất bại: $e');
      } finally {
        if (mounted) setState(() => _transcribing = false);
      }
      return;
    }
    if (!await _recorder.hasPermission()) {
      _snack('Không có quyền truy cập micro');
      return;
    }
    String path = '';
    if (!kIsWeb) {
      final dir = await getTemporaryDirectory();
      path =
          '${dir.path}${Platform.pathSeparator}senclaw_rem_${DateTime.now().millisecondsSinceEpoch}.m4a';
    }
    await _recorder.start(const RecordConfig(), path: path);
    if (mounted) setState(() => _recording = true);
  }

  void _snack(String msg) {
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(msg)));
  }

  void _scrollSoon() {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (_scroll.hasClients) {
        _scroll.jumpTo(_scroll.position.maxScrollExtent);
      }
    });
  }

  static String _fmtWhen(int ms) =>
      DateFormat('EEE, d MMM • HH:mm').format(
          DateTime.fromMillisecondsSinceEpoch(ms));

  // ── Build ────────────────────────────────────────────────────────────────
  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final t = widget.target;
    final isLate = (t.startAtMs != null &&
            t.startAtMs! < DateTime.now().millisecondsSinceEpoch) ||
        t.kind == 'renotify';

    return Positioned.fill(
      child: GestureDetector(
        onTap: _close, // tap the dim barrier to dismiss
        child: Container(
          color: Colors.black.withValues(alpha: 0.55),
          alignment: Alignment.center,
          child: GestureDetector(
            onTap: () {}, // absorb taps inside the card
            child: ConstrainedBox(
              constraints:
                  const BoxConstraints(maxWidth: 640, maxHeight: 620),
              child: Container(
                margin: const EdgeInsets.all(AppTokens.s24),
                decoration: BoxDecoration(
                  color: c.surface,
                  border: Border.all(color: c.border),
                  borderRadius: BorderRadius.circular(AppTokens.rXl),
                ),
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    _header(c, t, isLate),
                    Divider(height: 1, color: c.border),
                    Flexible(child: _conversation(c)),
                    _quickActions(c),
                    Divider(height: 1, color: c.border),
                    _composer(c),
                  ],
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }

  Widget _header(AppColors c, ReminderTarget t, bool isLate) => Padding(
        padding: const EdgeInsets.fromLTRB(
            AppTokens.s20, AppTokens.s16, AppTokens.s8, AppTokens.s12),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            const Padding(
              padding: EdgeInsets.only(top: 2),
              child: Text('⏰', style: TextStyle(fontSize: 18)),
            ),
            const SizedBox(width: AppTokens.s8),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(
                    children: [
                      Flexible(
                        child: Text(
                          t.title,
                          maxLines: 2,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(
                            color: c.textPrimary,
                            fontWeight: FontWeight.w700,
                            fontSize: 16,
                          ),
                        ),
                      ),
                      if (isLate) ...[
                        const SizedBox(width: AppTokens.s8),
                        Container(
                          padding: const EdgeInsets.symmetric(
                              horizontal: 6, vertical: 1),
                          decoration: BoxDecoration(
                            color: AppTokens.warning.withValues(alpha: 0.15),
                            borderRadius: BorderRadius.circular(AppTokens.rFull),
                          ),
                          child: const Text('Trễ',
                              style: TextStyle(
                                  color: AppTokens.warning,
                                  fontSize: 11,
                                  fontWeight: FontWeight.w700)),
                        ),
                      ],
                    ],
                  ),
                  const SizedBox(height: 2),
                  Text(
                    t.startAtMs != null
                        ? _fmtWhen(t.startAtMs!)
                        : 'Nhắc nhở lịch',
                    style: TextStyle(color: c.textMuted, fontSize: 12),
                  ),
                ],
              ),
            ),
            IconButton(
              tooltip: 'Đóng',
              icon: const Icon(Icons.close, size: 18),
              onPressed: _close,
            ),
          ],
        ),
      );

  Widget _conversation(AppColors c) {
    final jid = _jid;
    if (jid == null) {
      return const Center(
        child: Padding(
          padding: EdgeInsets.all(AppTokens.s24),
          child: CircularProgressIndicator(),
        ),
      );
    }
    final convo = ref.watch(conversationProvider(jid));
    final n = ref.read(conversationProvider(jid).notifier);
    final items = _renderMessages(convo.messages, n);
    if (convo.busy) {
      items.add(Padding(
        padding: const EdgeInsets.symmetric(
            horizontal: AppTokens.s16, vertical: AppTokens.s8),
        child: Row(children: [
          const SizedBox(
              width: 12,
              height: 12,
              child: CircularProgressIndicator(strokeWidth: 2)),
          const SizedBox(width: AppTokens.s8),
          Text('SenClaw đang xử lý…',
              style: TextStyle(color: c.textMuted, fontSize: 12)),
        ]),
      ));
    }
    if (items.isEmpty) {
      return Center(
        child: Padding(
          padding: const EdgeInsets.all(AppTokens.s24),
          child: Text(
            'Nhắn hoặc nói để dời lịch, xoá, hay nhờ SenClaw việc khác 👋',
            textAlign: TextAlign.center,
            style: TextStyle(color: c.textMuted),
          ),
        ),
      );
    }
    return SelectionArea(
      child: ListView(
        controller: _scroll,
        padding: const EdgeInsets.symmetric(vertical: AppTokens.s12),
        children: items,
      ),
    );
  }

  List<Widget> _renderMessages(
      List<ChatMessage> msgs, ConversationNotifier n) {
    final out = <Widget>[];
    var i = 0;
    while (i < msgs.length) {
      if (msgs[i].kind == MessageKind.tool) {
        final group = <ChatMessage>[];
        while (i < msgs.length && msgs[i].kind == MessageKind.tool) {
          group.add(msgs[i]);
          i++;
        }
        out.add(ToolGroupCard(tools: group));
      } else {
        out.add(MessageItem(
          message: msgs[i],
          onPermission: n.resolvePermission,
          onQuestion: (rid, answers, otherTexts) =>
              n.resolveQuestion(rid, answers, otherTexts: otherTexts),
          onForm: (rid, values, submitted) =>
              n.resolveForm(rid, values, submitted: submitted),
        ));
        i++;
      }
    }
    return out;
  }

  Widget _quickActions(AppColors c) {
    const chips = <(String, String)>[
      ('Nhắc lại sau 10 phút', 'Nhắc lại nhắc nhở này sau 10 phút nữa.'),
      ('Dời sang tối nay 20:00', 'Dời sự kiện này sang 20:00 tối nay.'),
      ('Xoá nhắc nhở', 'Xoá nhắc nhở và sự kiện này giúp tôi.'),
    ];
    final disabled = _jid == null || _transcribing;
    return Padding(
      padding: const EdgeInsets.fromLTRB(
          AppTokens.s12, AppTokens.s4, AppTokens.s12, AppTokens.s8),
      child: Wrap(
        spacing: AppTokens.s8,
        runSpacing: AppTokens.s6,
        children: [
          for (final (label, prompt) in chips)
            ActionChip(
              label: Text(label, style: const TextStyle(fontSize: 12)),
              onPressed:
                  disabled ? null : () => _sendTurn(prompt, voice: false),
            ),
        ],
      ),
    );
  }

  Widget _composer(AppColors c) {
    final canSend = _jid != null && !_transcribing;
    return Padding(
      padding: const EdgeInsets.fromLTRB(
          AppTokens.s12, AppTokens.s8, AppTokens.s12, AppTokens.s12),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.end,
        children: [
          Expanded(
            child: TextField(
              controller: _input,
              minLines: 1,
              maxLines: 4,
              textInputAction: TextInputAction.send,
              onSubmitted:
                  canSend ? (v) => _sendTurn(v, voice: false) : null,
              style: TextStyle(color: c.textPrimary, fontSize: 14),
              decoration: InputDecoration(
                hintText: 'Nhắn cho SenClaw…',
                hintStyle: TextStyle(color: c.textMuted, fontSize: 14),
                filled: true,
                fillColor: c.surfaceAlt,
                isDense: true,
                contentPadding: const EdgeInsets.symmetric(
                    horizontal: AppTokens.s16, vertical: AppTokens.s12),
                enabledBorder: OutlineInputBorder(
                  borderRadius: BorderRadius.circular(AppTokens.rXl),
                  borderSide: BorderSide(color: c.border),
                ),
                focusedBorder: OutlineInputBorder(
                  borderRadius: BorderRadius.circular(AppTokens.rXl),
                  borderSide: BorderSide(color: c.accent, width: 1.5),
                ),
              ),
            ),
          ),
          const SizedBox(width: AppTokens.s4),
          IconButton(
            tooltip: _recording ? 'Dừng & gửi' : 'Nói (giọng nói)',
            icon: _transcribing
                ? const SizedBox(
                    width: 16,
                    height: 16,
                    child: CircularProgressIndicator(strokeWidth: 2))
                : Icon(_recording ? Icons.stop : Icons.mic_none,
                    size: 20, color: _recording ? AppTokens.danger : null),
            onPressed: (canSend || _recording) ? _toggleMic : null,
          ),
          const SizedBox(width: AppTokens.s4),
          FilledButton(
            onPressed:
                canSend ? () => _sendTurn(_input.text, voice: false) : null,
            style: FilledButton.styleFrom(
              minimumSize: const Size(44, 40),
              padding: const EdgeInsets.symmetric(horizontal: AppTokens.s12),
            ),
            child: const Icon(Icons.arrow_upward, size: 18),
          ),
        ],
      ),
    );
  }
}
