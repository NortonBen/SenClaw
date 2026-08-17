import 'dart:io' show File, Platform;
import 'package:file_picker/file_picker.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:http/http.dart' as http;
import 'package:path_provider/path_provider.dart';
import 'package:record/record.dart';
import '../../core/i18n/l10n.dart';
import '../../core/transport/connection.dart';
import '../../models/chat_message.dart';
import '../../theme/tokens.dart';
import '../cowork/cowork_providers.dart';
import '../../models/cowork_models.dart' show CoworkTask;
import '../dock/right_dock.dart';
import '../dock/dispatch_provider.dart';
import '../dock/todos_provider.dart';
import 'agent_usage_provider.dart';
import 'audio_service.dart';
import 'conversation_provider.dart';
import 'plan_history_provider.dart';
import '../../models/space_models.dart' show SpaceSchedule;
import '../../widgets/app_markdown.dart';
import '../../widgets/schedule_editor.dart';
import 'groups_provider.dart';
import 'image_attachment.dart';
import 'mini_chat_screen.dart' show subWindowIdProvider;
import 'new_chat_dialog.dart' show llmConfigsProvider, LlmConfig;
import 'voice_chat_overlay.dart';
import 'widgets/message_widgets.dart';
import 'widgets/slash_mention_input.dart';

/// Parse a daemon timestamp to epoch ms — ISO 8601, or "YYYY-MM-DD HH:MM:SS"
/// (space separator, which `DateTime.parse` rejects). Null if unparseable.
/// Mirrors the web ChatView `toMs` helper used for DAG interleaving.
int? _toMs(String? s) {
  if (s == null || s.isEmpty) return null;
  var dt = DateTime.tryParse(s);
  if (dt == null && s.contains(' ')) {
    dt = DateTime.tryParse(s.replaceFirst(' ', 'T'));
  }
  return dt?.millisecondsSinceEpoch;
}

/// The live conversation surface for a single chat group: header (agent mode +
/// busy/stop), scrolling message list, and a composer with attachments.
class ConversationPane extends ConsumerStatefulWidget {
  const ConversationPane({super.key, required this.jid, required this.title});
  final String jid;
  final String title;

  @override
  ConsumerState<ConversationPane> createState() => _ConversationPaneState();
}

class _ConversationPaneState extends ConsumerState<ConversationPane> {
  final _input = TextEditingController();
  final _scroll = ScrollController();
  // Pending image attachments as {dataUrl, mimeType} (React ImageAttachment).
  final List<Map<String, String>> _attachments = [];
  final _recorder = AudioRecorder();
  bool _recording = false;
  bool _transcribing = false;

  @override
  void initState() {
    super.initState();
    // Open at the bottom (newest message) like the web chat — scroll up for
    // history. Jump after layout, then again once late content (markdown /
    // images) has expanded the scroll extent.
    _jumpToBottomSoon();
  }

  /// Jump to the bottom after the next frame, then re-jump shortly after so
  /// asynchronously-sized content (rendered markdown, images) doesn't leave the
  /// view stranded above the newest message.
  void _jumpToBottomSoon() {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      _scrollToBottom(animate: false);
      for (final ms in const [120, 350]) {
        Future.delayed(Duration(milliseconds: ms), () {
          if (mounted) _scrollToBottom(animate: false);
        });
      }
    });
  }

  @override
  void dispose() {
    _input.dispose();
    _scroll.dispose();
    _recorder.dispose();
    super.dispose();
  }

  /// Toggle mic recording. On stop, send the audio to Whisper and append the
  /// recognized text to the composer.
  Future<void> _toggleMic() async {
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
        if (text.isNotEmpty) {
          final prefix = _input.text.trimRight();
          _input.text = prefix.isEmpty ? text : '$prefix $text';
        }
      } catch (e) {
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(
                content: Text(
                    context.trArgs('Transcription failed: {e}', {'e': e}))),
          );
        }
      } finally {
        if (mounted) setState(() => _transcribing = false);
      }
      return;
    }
    if (!await _recorder.hasPermission()) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text(context.tr('Microphone permission denied'))),
        );
      }
      return;
    }
    String path = '';
    if (!kIsWeb) {
      final dir = await getTemporaryDirectory();
      path =
          '${dir.path}${Platform.pathSeparator}senclaw_rec_${DateTime.now().millisecondsSinceEpoch}.m4a';
    }
    await _recorder.start(const RecordConfig(), path: path);
    if (mounted) setState(() => _recording = true);
  }

  /// The user's own prior messages, chronological, with consecutive duplicates
  /// collapsed — feeds the composer's ↑/↓ shell-style recall.
  List<String> _inputHistory(List<ChatMessage> messages) {
    final out = <String>[];
    for (final m in messages) {
      if (m.kind != MessageKind.user) continue;
      final t = m.text?.trim();
      if (t == null || t.isEmpty) continue;
      if (out.isNotEmpty && out.last == t) continue;
      out.add(t);
    }
    return out;
  }

  void _send() {
    final text = _input.text;
    if (text.trim().isEmpty && _attachments.isEmpty) return;
    ref
        .read(conversationProvider(widget.jid).notifier)
        .sendText(text, attachments: List.of(_attachments));
    _input.clear();
    setState(_attachments.clear);
    WidgetsBinding.instance.addPostFrameCallback((_) => _scrollToBottom());
  }

  Future<void> _attach() async {
    // Any file: images take the vision/OCR route, everything else is saved by
    // the daemon and its text extracted into the prompt.
    final res = await FilePicker.platform.pickFiles(
      allowMultiple: true,
      withData: true,
    );
    if (res == null) return;
    for (final f in res.files) {
      final bytes = f.bytes;
      if (bytes == null) continue;
      final Map<String, String> att;
      if (isImageExtension(f.extension)) {
        att = await buildImageAttachment(bytes, mimeForExtension(f.extension));
      } else {
        if (bytes.length > kMaxDocBytes) {
          if (!mounted) return;
          ScaffoldMessenger.of(context).showSnackBar(SnackBar(
            content: Text('${f.name} > ${kMaxDocBytes ~/ (1024 * 1024)}MB'),
          ));
          continue;
        }
        att = buildDocumentAttachment(
            bytes, documentMimeForExtension(f.extension), f.name);
      }
      if (!mounted) return;
      setState(() => _attachments.add(att));
    }
  }

  void _scrollToBottom({bool animate = true}) {
    if (!_scroll.hasClients) return;
    final max = _scroll.position.maxScrollExtent;
    if (animate) {
      _scroll.animateTo(max,
          duration: const Duration(milliseconds: 200), curve: Curves.easeOut);
    } else {
      _scroll.jumpTo(max);
    }
  }

  /// True when the view is parked near the newest message — used to decide
  /// whether a freshly-arrived message should auto-follow. If the user has
  /// scrolled up to read history, we DON'T yank them back down.
  bool _isNearBottom() {
    if (!_scroll.hasClients) return true;
    final pos = _scroll.position;
    return pos.maxScrollExtent - pos.pixels < 240;
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final convo = ref.watch(conversationProvider(widget.jid));
    final notifier = ref.read(conversationProvider(widget.jid).notifier);
    final kind = widget.jid.startsWith('cowork:')
        ? 'cowork'
        : widget.jid.startsWith('schedule:')
            ? 'schedule'
            : (ref
                    .watch(groupsProvider)
                    .where((g) => g.jid == widget.jid)
                    .firstOrNull
                    ?.groupType ??
                '');
    final hasBadge =
        kind == 'cowork' || kind == 'code' || kind == 'schedule';

    ref.listen(conversationProvider(widget.jid), (prev, next) {
      final hadMessages = (prev?.messages.length ?? 0) > 0;
      if (!hadMessages && next.messages.isNotEmpty) {
        // First async load → hard-jump (with the late re-jumps) to the bottom.
        _jumpToBottomSoon();
      } else if (_isNearBottom()) {
        // New message while already at the bottom → follow it down. If the user
        // scrolled up to read history, leave their position alone.
        WidgetsBinding.instance
            .addPostFrameCallback((_) => _scrollToBottom(animate: true));
      }
    });

    return Column(
      children: [
        // Header
        Container(
          height: 56,
          padding: const EdgeInsets.symmetric(horizontal: AppTokens.s24),
          decoration: BoxDecoration(
            border: Border(bottom: BorderSide(color: c.border)),
          ),
          child: Row(
            children: [
              Expanded(
                child: Row(
                  children: [
                    Flexible(
                      child: Text(
                        widget.title,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: TextStyle(
                          color: c.textPrimary,
                          fontWeight: FontWeight.w700,
                          fontSize: 16,
                        ),
                      ),
                    ),
                    if (hasBadge) ...[
                      const SizedBox(width: AppTokens.s8),
                      _KindBadge(kind: kind),
                    ],
                  ],
                ),
              ),
              _ContextUsageMeter(jid: widget.jid),
              IconButton(
                tooltip: context.tr('Voice chat'),
                icon: const Icon(Icons.graphic_eq, size: 18),
                onPressed: () =>
                    showVoiceChat(context, widget.jid, widget.title),
              ),
              IconButton(
                tooltip: context.tr('Chat info'),
                icon: const Icon(Icons.info_outline, size: 18),
                onPressed: () => showDialog(
                  context: context,
                  builder: (_) =>
                      _ChatInfoDialog(jid: widget.jid, title: widget.title),
                ),
              ),
              if (widget.jid.startsWith('cowork:')) ...[
                IconButton(
                  tooltip: context.tr('Team tasks'),
                  icon: const Icon(Icons.view_kanban_outlined, size: 18),
                  onPressed: () => showDialog(
                    context: context,
                    builder: (_) => _CoworkTasksDialog(
                        teamId: widget.jid.substring('cowork:'.length)),
                  ),
                ),
                IconButton(
                  tooltip: context.tr('Open Cowork board'),
                  icon: const Icon(Icons.dashboard_outlined, size: 18),
                  onPressed: () {
                    ref.read(openTeamProvider.notifier).state =
                        widget.jid.substring('cowork:'.length);
                    context.go('/cowork');
                  },
                ),
              ],
              if (kind == 'schedule')
                IconButton(
                  tooltip: context.tr('Schedule'),
                  icon: const Icon(Icons.event_repeat_outlined, size: 18),
                  onPressed: () => showDialog(
                    context: context,
                    builder: (_) => _ScheduleInfoDialog(
                        scheduleId:
                            widget.jid.substring('schedule:'.length)),
                  ),
                )
              else if (kind != 'cowork')
                IconButton(
                  tooltip: context.tr('Plan history'),
                  icon: const Icon(Icons.history_edu_outlined, size: 18),
                  onPressed: () {
                    ref
                        .read(planHistoryProvider.notifier)
                        .requestList(widget.jid);
                    showDialog(
                        context: context,
                        builder: (_) => _PlanHistoryDialog(jid: widget.jid));
                  },
                ),
              // The right dock/console doesn't exist in the mini window, so the
              // toggle is a no-op there — hide it.
              if (ref.watch(subWindowIdProvider) == null)
                Builder(builder: (_) {
                  // Status dot (web DockBadges): green = live dispatch, blue =
                  // pending agent todos.
                  final live = ref.watch(dispatchProvider).parents.any((d) =>
                      d.status == 'active' ||
                      d.status == 'queued' ||
                      d.status == 'running' ||
                      d.status == 'processing');
                  final info = ref.watch(agentTodosProvider).isNotEmpty;
                  final dot = live
                      ? AppTokens.success
                      : (info ? context.colors.accent : null);
                  return Stack(clipBehavior: Clip.none, children: [
                    IconButton(
                      tooltip: context.tr('Toggle console / workbench'),
                      icon: const Icon(Icons.view_sidebar_outlined, size: 18),
                      onPressed: () => ref
                          .read(dockVisibleProvider.notifier)
                          .update((v) => !v),
                    ),
                    if (dot != null)
                      Positioned(
                        right: 6,
                        top: 6,
                        child: Container(
                          width: 8,
                          height: 8,
                          decoration: BoxDecoration(
                            color: dot,
                            shape: BoxShape.circle,
                            border: Border.all(
                                color: context.colors.surface, width: 1.5),
                          ),
                        ),
                      ),
                  ]);
                }),
              if (convo.busy) ...[
                const SizedBox(width: AppTokens.s12),
                const SizedBox(
                  width: 14,
                  height: 14,
                  child: CircularProgressIndicator(strokeWidth: 2),
                ),
                const SizedBox(width: AppTokens.s8),
                TextButton.icon(
                  onPressed: notifier.stop,
                  icon: const Icon(Icons.stop_circle_outlined, size: 16),
                  label: Text(context.tr('Stop')),
                ),
              ],
            ],
          ),
        ),
        // Messages — wrapped in SelectionArea so the user can drag-select and
        // copy text across bubbles (Cmd/Ctrl+C, or right-click → Copy).
        Expanded(
          child: convo.messages.isEmpty
              ? Center(
                  child: Text(
                    context.tr('No messages yet. Say hello 👋'),
                    style: TextStyle(color: c.textMuted),
                  ),
                )
              : Builder(builder: (_) {
                  // Order by timestamp so think / tool-calls / response land in
                  // the right place (the daemon's history isn't always sorted,
                  // and streamed parts can arrive out of order). Stable: a null
                  // timestamp inherits the previous message's time, ties break
                  // on original index — so live streaming order is preserved.
                  final indexed =
                      <({int ms, int idx, ChatMessage m})>[];
                  var lastMs = 0;
                  for (var k = 0; k < convo.messages.length; k++) {
                    final m = convo.messages[k];
                    final ms = _toMs(m.ts) ?? lastMs;
                    lastMs = ms;
                    indexed.add((ms: ms, idx: k, m: m));
                  }
                  indexed.sort((a, b) {
                    final c = a.ms.compareTo(b.ms);
                    return c != 0 ? c : a.idx.compareTo(b.idx);
                  });
                  final msgs = indexed.map((e) => e.m).toList();
                  // Tool-group a slice of messages into ToolGroupCard/MessageItem
                  // widgets (web ChatView behavior).
                  List<Widget> renderMsgs(List<ChatMessage> slice) {
                    final out = <Widget>[];
                    var i = 0;
                    while (i < slice.length) {
                      if (slice[i].kind == MessageKind.tool) {
                        final group = <ChatMessage>[];
                        while (i < slice.length &&
                            slice[i].kind == MessageKind.tool) {
                          group.add(slice[i]);
                          i++;
                        }
                        out.add(ToolGroupCard(tools: group));
                      } else {
                        out.add(MessageItem(
                          message: slice[i],
                          onPermission: notifier.resolvePermission,
                          onQuestion: (rid, answers, otherTexts) => notifier
                              .resolveQuestion(rid, answers,
                                  otherTexts: otherTexts),
                          onForm: (rid, values, submitted) => notifier
                              .resolveForm(rid, values, submitted: submitted),
                        ));
                        i++;
                      }
                    }
                    return out;
                  }

                  // DAG dispatch cards for this chat, interleaved chronologically
                  // (web InlineDispatchCard). Parents whose adminFolder matches
                  // this chat's folder, split by createdAt vs message timestamps.
                  final folder = ref
                      .watch(groupsProvider)
                      .where((g) => g.jid == widget.jid)
                      .firstOrNull
                      ?.folder;
                  final myParents = (folder != null && folder.isNotEmpty)
                      ? ref
                          .watch(dispatchProvider)
                          .parents
                          .where((p) => p.adminFolder == folder)
                          .toList()
                      : <DispatchParent>[];
                  final items = <Widget>[];
                  if (myParents.isEmpty) {
                    items.addAll(renderMsgs(msgs));
                  } else {
                    // Parseable parents interleave; unparseable ones drop to
                    // the bottom (safe fallback).
                    final timed = myParents
                        .map((p) => (p: p, ms: _toMs(p.createdAt)))
                        .toList();
                    final placed = timed.where((e) => e.ms != null).toList()
                      ..sort((a, b) => a.ms!.compareTo(b.ms!));
                    final tail = timed.where((e) => e.ms == null).toList();
                    var cursor = 0;
                    for (final e in placed) {
                      final before = <ChatMessage>[];
                      while (cursor < msgs.length &&
                          (_toMs(msgs[cursor].ts) ?? 0) <= e.ms!) {
                        before.add(msgs[cursor]);
                        cursor++;
                      }
                      items.addAll(renderMsgs(before));
                      items.add(InlineDispatchCard(parent: e.p));
                    }
                    if (cursor < msgs.length) {
                      items.addAll(renderMsgs(msgs.sublist(cursor)));
                    }
                    for (final e in tail) {
                      items.add(InlineDispatchCard(parent: e.p));
                    }
                  }
                  // Typing indicator while the agent works (channel_app
                  // parity): shown between send and the first streamed delta,
                  // and while tools run — hidden once a live streaming bubble
                  // is already animating.
                  final streamingNow =
                      msgs.isNotEmpty && msgs.last.streaming;
                  if (convo.busy && !streamingNow) {
                    items.add(const _TypingIndicatorRow());
                  }
                  return SelectionArea(
                    child: ListView.builder(
                      controller: _scroll,
                      padding:
                          const EdgeInsets.symmetric(vertical: AppTokens.s16),
                      itemCount: items.length,
                      itemBuilder: (_, i) => items[i],
                    ),
                  );
                }),
        ),
        // Composer: input on top, action row (mode/model/attach/mic/send)
        // underneath — Claude-style.
        _Composer(
          jid: widget.jid,
          mode: convo.agentMode,
          onMode: notifier.setAgentMode,
          controller: _input,
          history: _inputHistory(convo.messages),
          attachments: _attachments,
          recording: _recording,
          transcribing: _transcribing,
          onSend: _send,
          onAttach: _attach,
          onMic: _toggleMic,
          onRemoveAttachment: (i) => setState(() => _attachments.removeAt(i)),
        ),
      ],
    );
  }
}

class _ModeToggle extends StatelessWidget {
  const _ModeToggle(
      {required this.mode,
      required this.onChanged,
      this.locked = false,
      this.modes = const ['Agent', 'Plan', 'Dag']});
  final String mode;
  final void Function(String) onChanged;

  /// When locked (cowork chats are always DAG), only the current mode shows
  /// and it can't be changed.
  final bool locked;

  /// Available modes (schedule chats omit 'Plan').
  final List<String> modes;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Container(
      decoration: BoxDecoration(
        color: c.surfaceAlt,
        borderRadius: BorderRadius.circular(AppTokens.rFull),
        border: Border.all(color: c.border),
      ),
      padding: const EdgeInsets.all(2),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          for (final m in (locked ? [mode] : modes))
            GestureDetector(
              onTap: locked ? null : () => onChanged(m),
              child: AnimatedContainer(
                duration: const Duration(milliseconds: 150),
                padding: const EdgeInsets.symmetric(
                  horizontal: AppTokens.s12,
                  vertical: AppTokens.s6,
                ),
                decoration: BoxDecoration(
                  color: m == mode ? c.accent : Colors.transparent,
                  borderRadius: BorderRadius.circular(AppTokens.rFull),
                ),
                child: Text(
                  context.tr(m),
                  style: TextStyle(
                    color: m == mode ? Colors.white : c.textMuted,
                    fontSize: 12,
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ),
            ),
        ],
      ),
    );
  }
}

class _Composer extends ConsumerWidget {
  const _Composer({
    required this.jid,
    required this.mode,
    required this.onMode,
    required this.controller,
    required this.history,
    required this.attachments,
    required this.recording,
    required this.transcribing,
    required this.onSend,
    required this.onAttach,
    required this.onMic,
    required this.onRemoveAttachment,
  });
  final String jid;
  final String mode;
  final void Function(String) onMode;
  final TextEditingController controller;
  final List<String> history;
  final List<Map<String, String>> attachments;
  final bool recording;
  final bool transcribing;
  final VoidCallback onSend;
  final VoidCallback onAttach;
  final VoidCallback onMic;
  final void Function(int) onRemoveAttachment;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final group =
        ref.watch(groupsProvider).where((g) => g.jid == jid).firstOrNull;
    final isCowork = group?.groupType == 'cowork';
    final isSchedule = jid.startsWith('schedule:');
    final configs = ref.watch(llmConfigsProvider).valueOrNull?.configs ?? const [];
    final modelId = group?.modelId;
    final selected = configs.where((m) => m.id == modelId).firstOrNull;
    final modelLabel =
        selected?.label ?? (modelId ?? context.tr('Default model'));

    return Container(
      padding: const EdgeInsets.fromLTRB(
          AppTokens.s16, AppTokens.s12, AppTokens.s16, AppTokens.s16),
      decoration: BoxDecoration(
        border: Border(top: BorderSide(color: c.border)),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (attachments.isNotEmpty)
            Padding(
              padding: const EdgeInsets.only(bottom: AppTokens.s8),
              child: Wrap(
                spacing: AppTokens.s8,
                runSpacing: AppTokens.s8,
                children: [
                  for (var i = 0; i < attachments.length; i++)
                    // Documents show their filename; a pasted/picked image has
                    // none, so it stays numbered.
                    Chip(
                      label: Text(
                        attachments[i]['name'] ??
                            context.trArgs('image {n}', {'n': i + 1}),
                        style: const TextStyle(fontSize: 12),
                      ),
                      avatar: Icon(
                        (attachments[i]['mimeType'] ?? '').startsWith('image/')
                            ? Icons.image_outlined
                            : Icons.description_outlined,
                        size: 14,
                      ),
                      onDeleted: () => onRemoveAttachment(i),
                    ),
                ],
              ),
            ),
          // ── Input on top (with / # skill and @ file·folder hints) ──
          SlashMentionField(
            controller: controller,
            onSend: onSend,
            history: history,
            fileScope: mentionScopeForJid(jid),
            style: TextStyle(color: c.textPrimary, fontSize: 14),
            decoration: InputDecoration(
              hintText: context.tr('Message the agent…   / # skill · @ file'),
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
              suffixIcon: Padding(
                padding: const EdgeInsets.only(right: AppTokens.s4),
                child: IconButton(
                  tooltip: context.tr('Send (Enter)'),
                  icon: Icon(Icons.keyboard_return,
                      size: 18, color: c.textMuted),
                  onPressed: onSend,
                ),
              ),
            ),
          ),
          const SizedBox(height: AppTokens.s8),
          // ── Action row(s) underneath. Layout adapts to the AVAILABLE WIDTH
          //    (not just the mini window): a narrow pane splits into two rows
          //    (mode+model, then attach/mic+send); a wide one uses one row. ──
          LayoutBuilder(builder: (context, constraints) {
            final compact = constraints.maxWidth < 480;
            final modeToggle = _ModeToggle(
              mode: isCowork ? 'Dag' : mode,
              onChanged: onMode,
              locked: isCowork,
              modes: isSchedule
                  ? const ['Agent', 'Dag']
                  : const ['Agent', 'Plan', 'Dag'],
            );
            final modelChip =
                _ModelPickerChip(jid: jid, label: modelLabel, configs: configs);
            final attachBtn = IconButton(
              tooltip: context.tr('Attach images'),
              icon: const Icon(Icons.attach_file, size: 18),
              onPressed: onAttach,
            );
            final micBtn = IconButton(
              tooltip: recording
                  ? context.tr('Stop & transcribe')
                  : context.tr('Voice input'),
              icon: transcribing
                  ? const SizedBox(
                      width: 16,
                      height: 16,
                      child: CircularProgressIndicator(strokeWidth: 2))
                  : Icon(recording ? Icons.stop : Icons.mic_none,
                      size: 18, color: recording ? AppTokens.danger : null),
              onPressed: transcribing ? null : onMic,
            );
            final sendBtn = FilledButton(
              onPressed: onSend,
              style: FilledButton.styleFrom(
                minimumSize: const Size(40, 36),
                padding:
                    const EdgeInsets.symmetric(horizontal: AppTokens.s12),
              ),
              child: const Icon(Icons.arrow_upward, size: 18),
            );

            if (compact) {
              // One compact row: mode · model · attach · mic. No send button —
              // Enter (or the ↵ inside the input) sends.
              return Row(children: [
                modeToggle,
                const SizedBox(width: AppTokens.s8),
                Flexible(child: modelChip),
                const SizedBox(width: AppTokens.s4),
                attachBtn,
                micBtn,
              ]);
            }
            return Row(children: [
              modeToggle,
              const SizedBox(width: AppTokens.s8),
              modelChip,
              const Spacer(),
              attachBtn,
              micBtn,
              const SizedBox(width: AppTokens.s8),
              sendBtn,
            ]);
          }),
        ],
      ),
    );
  }
}

/// Compact model-picker chip used in the composer action row.
class _ModelPickerChip extends ConsumerWidget {
  const _ModelPickerChip(
      {required this.jid, required this.label, required this.configs});
  final String jid;
  final String label;
  final List<LlmConfig> configs;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    return Material(
      color: Colors.transparent,
      child: PopupMenuButton<String>(
        tooltip: context.tr('Model'),
        position: PopupMenuPosition.under,
        onSelected: (id) => ref
            .read(groupsProvider.notifier)
            .setModel(jid, id.isEmpty ? null : id),
        itemBuilder: (_) => [
          PopupMenuItem(value: '', child: Text(context.tr('Default model'))),
          for (final m in configs)
            PopupMenuItem(value: m.id, child: Text(m.label)),
        ],
        child: Container(
          padding: const EdgeInsets.symmetric(
              horizontal: AppTokens.s12, vertical: AppTokens.s6),
          decoration: BoxDecoration(
            border: Border.all(color: c.border),
            borderRadius: BorderRadius.circular(AppTokens.rXl),
          ),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(Icons.smart_toy_outlined, size: 14, color: c.textMuted),
              const SizedBox(width: AppTokens.s6),
              ConstrainedBox(
                constraints: const BoxConstraints(maxWidth: 180),
                child: Text(label,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(color: c.textSecondary, fontSize: 12)),
              ),
              Icon(Icons.expand_more, size: 16, color: c.textMuted),
            ],
          ),
        ),
      ),
    );
  }
}

/// Browse past plans for a group (web PlanHistoryPanel). Persistent in SQLite.
class _PlanHistoryDialog extends ConsumerStatefulWidget {
  const _PlanHistoryDialog({required this.jid});
  final String jid;
  @override
  ConsumerState<_PlanHistoryDialog> createState() =>
      _PlanHistoryDialogState();
}

class _PlanHistoryDialogState extends ConsumerState<_PlanHistoryDialog> {
  String? _selected;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final st = ref.watch(planHistoryProvider);
    final content = _selected == null ? null : st.contentById[_selected];
    return Dialog(
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 820, maxHeight: 620),
        child: Padding(
          padding: const EdgeInsets.all(AppTokens.s16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(children: [
                Icon(Icons.history_edu_outlined, size: 18, color: c.accent),
                const SizedBox(width: AppTokens.s8),
                Text(context.tr('Plan history'),
                    style: TextStyle(
                        color: c.textPrimary,
                        fontSize: 16,
                        fontWeight: FontWeight.w700)),
                const Spacer(),
                IconButton(
                    icon: const Icon(Icons.close, size: 18),
                    onPressed: () => Navigator.pop(context)),
              ]),
              const SizedBox(height: AppTokens.s8),
              Expanded(
                child: st.summaries.isEmpty
                    ? Center(
                        child: Text(context.tr('No plans yet'),
                            style: TextStyle(color: c.textMuted)))
                    : Row(
                        crossAxisAlignment: CrossAxisAlignment.stretch,
                        children: [
                          // List
                          SizedBox(
                            width: 280,
                            child: ListView.builder(
                              itemCount: st.summaries.length,
                              itemBuilder: (_, i) {
                                final p = st.summaries[i];
                                final sel = p.id == _selected;
                                return InkWell(
                                  onTap: () {
                                    ref
                                        .read(planHistoryProvider.notifier)
                                        .requestGet(p.id);
                                    setState(() => _selected = p.id);
                                  },
                                  child: Container(
                                    padding: const EdgeInsets.symmetric(
                                        horizontal: AppTokens.s12,
                                        vertical: AppTokens.s8),
                                    margin: const EdgeInsets.symmetric(
                                        vertical: 1),
                                    decoration: BoxDecoration(
                                      color:
                                          sel ? c.accentSoft : Colors.transparent,
                                      borderRadius:
                                          BorderRadius.circular(AppTokens.rMd),
                                    ),
                                    child: Column(
                                      crossAxisAlignment:
                                          CrossAxisAlignment.start,
                                      children: [
                                        Text(p.title,
                                            maxLines: 2,
                                            overflow: TextOverflow.ellipsis,
                                            style: TextStyle(
                                                color: c.textPrimary,
                                                fontSize: 13,
                                                fontWeight: FontWeight.w600)),
                                        if (p.status.isNotEmpty)
                                          Text(p.status,
                                              style: TextStyle(
                                                  color: p.status == 'pending'
                                                      ? AppTokens.warning
                                                      : c.textMuted,
                                                  fontSize: 11)),
                                      ],
                                    ),
                                  ),
                                );
                              },
                            ),
                          ),
                          Container(width: 1, color: c.border),
                          // Content
                          Expanded(
                            child: _selected == null
                                ? Center(
                                    child: Text(context.tr('Select a plan'),
                                        style:
                                            TextStyle(color: c.textMuted)))
                                : content == null
                                    ? const Center(
                                        child: CircularProgressIndicator())
                                    : SingleChildScrollView(
                                        padding: const EdgeInsets.all(
                                            AppTokens.s12),
                                        child: AppMarkdown(content),
                                      ),
                          ),
                        ],
                      ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// Live context-window meter for the chat header (web ChatView usage
/// indicator): a small bar + percentage fed by `agent:usage` events.
class _ContextUsageMeter extends ConsumerWidget {
  const _ContextUsageMeter({required this.jid});
  final String jid;
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final u = ref.watch(agentUsageProvider)[jid];
    if (u == null || u.maxTokens <= 0) return const SizedBox.shrink();
    final pctInt = (u.pct * 100).round();
    final color = u.pct > 0.9
        ? AppTokens.danger
        : (u.pct > 0.7 ? AppTokens.warning : c.accent);
    return Tooltip(
      message: context.trArgs(
          'Context: {use} / {max} tokens · {remaining} left', {
        'use': u.useTokens,
        'max': u.maxTokens,
        'remaining': u.remaining,
      }),
      child: Padding(
        padding: const EdgeInsets.only(right: AppTokens.s8),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            SizedBox(
              width: 56,
              child: ClipRRect(
                borderRadius: BorderRadius.circular(4),
                child: LinearProgressIndicator(
                  value: u.pct,
                  minHeight: 6,
                  backgroundColor: c.border,
                  valueColor: AlwaysStoppedAnimation(color),
                ),
              ),
            ),
            const SizedBox(width: AppTokens.s6),
            Text('$pctInt%',
                style: TextStyle(
                    color: c.textMuted,
                    fontSize: 12,
                    fontWeight: FontWeight.w600)),
          ],
        ),
      ),
    );
  }
}

/// Tasks panel for a cowork team chat — shows the team's tasks grouped by
/// status (the Kanban data) + live dispatch activity for that team.
class _CoworkTasksDialog extends ConsumerWidget {
  const _CoworkTasksDialog({required this.teamId});
  final String teamId;

  static const _statusOrder = [
    ('in_progress', 'In progress', AppTokens.warning),
    ('review', 'Review', AppTokens.brandAlt),
    ('todo', 'To do', AppTokens.brand),
    ('blocked', 'Blocked', AppTokens.danger),
    ('done', 'Done', AppTokens.success),
  ];

  Future<void> _patch(WidgetRef ref, String taskId, Map<String, dynamic> body) async {
    await ref
        .read(apiClientProvider)
        .patch('/api/cowork/teams/$teamId/tasks/$taskId', body: body);
    ref.invalidate(teamTasksProvider(teamId));
  }

  Future<void> _delete(WidgetRef ref, String taskId) async {
    await ref
        .read(apiClientProvider)
        .delete('/api/cowork/teams/$teamId/tasks/$taskId');
    ref.invalidate(teamTasksProvider(teamId));
  }

  static const _editStatuses = [
    'todo',
    'in_progress',
    'review',
    'done',
    'blocked',
  ];

  Future<void> _editTask(
      BuildContext context, WidgetRef ref, CoworkTask task) async {
    final title = TextEditingController(text: task.title);
    final content = TextEditingController(text: task.description ?? '');
    var status = _editStatuses.contains(task.status) ? task.status : 'todo';
    final ok = await showDialog<bool>(
      context: context,
      builder: (dctx) => StatefulBuilder(
        builder: (dctx, setLocal) => AlertDialog(
          title: Text(dctx.tr('Edit task')),
          content: SizedBox(
            width: 460,
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                TextField(
                  controller: title,
                  decoration: InputDecoration(
                      labelText: dctx.tr('Title'),
                      border: const OutlineInputBorder()),
                ),
                const SizedBox(height: AppTokens.s12),
                TextField(
                  controller: content,
                  minLines: 3,
                  maxLines: 8,
                  decoration: InputDecoration(
                      labelText: dctx.tr('Content (prompt to run)'),
                      alignLabelWithHint: true,
                      border: const OutlineInputBorder()),
                ),
                const SizedBox(height: AppTokens.s12),
                DropdownButtonFormField<String>(
                  initialValue: status,
                  decoration: InputDecoration(
                      labelText: dctx.tr('Status'),
                      border: const OutlineInputBorder()),
                  items: [
                    for (final s in _editStatuses)
                      DropdownMenuItem(value: s, child: Text(s)),
                  ],
                  onChanged: (v) => setLocal(() => status = v ?? status),
                ),
              ],
            ),
          ),
          actions: [
            TextButton(
                onPressed: () => Navigator.of(dctx).pop(false),
                child: Text(dctx.tr('Cancel'))),
            FilledButton(
                onPressed: () => Navigator.of(dctx).pop(true),
                child: Text(dctx.tr('Save'))),
          ],
        ),
      ),
    );
    if (ok != true) return;
    await _patch(ref, task.id, {
      'title': title.text.trim(),
      'description': content.text.trim(),
      'status': status,
    });
  }

  Future<void> _newTask(BuildContext context, WidgetRef ref) async {
    final title = TextEditingController();
    final content = TextEditingController();
    final ok = await showDialog<bool>(
      context: context,
      builder: (dctx) => AlertDialog(
        title: Text(dctx.tr('New task')),
        content: SizedBox(
          width: 460,
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              TextField(
                controller: title,
                autofocus: true,
                decoration: InputDecoration(
                    labelText: dctx.tr('Title'),
                    border: const OutlineInputBorder()),
              ),
              const SizedBox(height: AppTokens.s12),
              TextField(
                controller: content,
                minLines: 4,
                maxLines: 10,
                decoration: InputDecoration(
                    labelText: dctx.tr('Content (prompt to run)'),
                    alignLabelWithHint: true,
                    border: const OutlineInputBorder()),
              ),
            ],
          ),
        ),
        actions: [
          TextButton(
              onPressed: () => Navigator.of(dctx).pop(false),
              child: Text(dctx.tr('Cancel'))),
          FilledButton(
              onPressed: () => Navigator.of(dctx).pop(true),
              child: Text(dctx.tr('Create'))),
        ],
      ),
    );
    if (ok != true || title.text.trim().isEmpty) return;
    await ref.read(apiClientProvider).post('/api/cowork/teams/$teamId/tasks',
        body: {
          'title': title.text.trim(),
          if (content.text.trim().isNotEmpty) 'description': content.text.trim(),
          'status': 'todo',
        });
    ref.invalidate(teamTasksProvider(teamId));
  }

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final tasks = ref.watch(teamTasksProvider(teamId));
    return Dialog(
      backgroundColor: c.surface,
      child: SizedBox(
        width: 560,
        height: 560,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(
                  AppTokens.s16, AppTokens.s12, AppTokens.s8, AppTokens.s8),
              child: Row(
                children: [
                  Icon(Icons.view_kanban_outlined, size: 18, color: c.accent),
                  const SizedBox(width: AppTokens.s8),
                  Text(context.tr('Team tasks'),
                      style: TextStyle(
                          color: c.textPrimary, fontWeight: FontWeight.w700)),
                  const Spacer(),
                  TextButton.icon(
                    onPressed: () => _newTask(context, ref),
                    icon: const Icon(Icons.add, size: 16),
                    label: Text(context.tr('New task')),
                  ),
                  IconButton(
                    tooltip: context.tr('Reload'),
                    icon: const Icon(Icons.refresh, size: 16),
                    onPressed: () =>
                        ref.invalidate(teamTasksProvider(teamId)),
                  ),
                  IconButton(
                    icon: const Icon(Icons.close, size: 18),
                    onPressed: () => Navigator.of(context).pop(),
                  ),
                ],
              ),
            ),
            const Divider(height: 1),
            Expanded(
              child: tasks.when(
                loading: () =>
                    const Center(child: CircularProgressIndicator()),
                error: (e, _) => Center(
                    child: Text('$e',
                        style: const TextStyle(color: AppTokens.danger))),
                data: (list) {
                  if (list.isEmpty) {
                    return Center(
                      child: Text(context.tr('No tasks yet'),
                          style: TextStyle(color: c.textMuted, fontSize: 12)),
                    );
                  }
                  return ListView(
                    padding: const EdgeInsets.all(AppTokens.s12),
                    children: [
                      for (final (key, label, color) in _statusOrder)
                        ...(() {
                          final group =
                              list.where((t) => t.status == key).toList();
                          if (group.isEmpty) return <Widget>[];
                          return [
                            Padding(
                              padding: const EdgeInsets.fromLTRB(
                                  AppTokens.s4, AppTokens.s8, 0, AppTokens.s4),
                              child: Row(
                                children: [
                                  Container(
                                      width: 8,
                                      height: 8,
                                      decoration: BoxDecoration(
                                          color: color,
                                          shape: BoxShape.circle)),
                                  const SizedBox(width: AppTokens.s8),
                                  Text(
                                      '${context.tr(label)} · ${group.length}',
                                      style: TextStyle(
                                          color: c.textMuted,
                                          fontSize: 11,
                                          fontWeight: FontWeight.w700,
                                          letterSpacing: 0.5)),
                                ],
                              ),
                            ),
                            for (final t in group)
                              Container(
                                margin: const EdgeInsets.only(
                                    bottom: AppTokens.s6),
                                padding: const EdgeInsets.all(AppTokens.s12),
                                decoration: BoxDecoration(
                                  color: c.sidebar,
                                  borderRadius:
                                      BorderRadius.circular(AppTokens.rMd),
                                  border: Border(
                                      left: BorderSide(color: color, width: 3),
                                      top: BorderSide(color: c.border),
                                      right: BorderSide(color: c.border),
                                      bottom: BorderSide(color: c.border)),
                                ),
                                child: Row(
                                  crossAxisAlignment:
                                      CrossAxisAlignment.start,
                                  children: [
                                    Expanded(
                                      child: Column(
                                        crossAxisAlignment:
                                            CrossAxisAlignment.start,
                                        children: [
                                          Text(
                                              t.title.trim().isEmpty
                                                  ? context.tr('(untitled)')
                                                  : t.title,
                                              style: TextStyle(
                                                  color: t.title.trim().isEmpty
                                                      ? c.textMuted
                                                      : c.textPrimary,
                                                  fontWeight:
                                                      FontWeight.w600)),
                                          if (t.description != null &&
                                              t.description!.trim()
                                                  .isNotEmpty) ...[
                                            const SizedBox(height: 3),
                                            Text(t.description!,
                                                maxLines: 3,
                                                overflow:
                                                    TextOverflow.ellipsis,
                                                style: TextStyle(
                                                    color: c.textSecondary,
                                                    fontSize: 12,
                                                    height: 1.4)),
                                          ],
                                          if (t.assignee != null &&
                                              t.assignee!.isNotEmpty) ...[
                                            const SizedBox(height: 2),
                                            Row(children: [
                                              Icon(Icons.smart_toy_outlined,
                                                  size: 12, color: c.textMuted),
                                              const SizedBox(width: 4),
                                              Text(t.assignee!,
                                                  style: TextStyle(
                                                      color: c.textMuted,
                                                      fontSize: 12)),
                                            ]),
                                          ],
                                        ],
                                      ),
                                    ),
                                    IconButton(
                                      tooltip: context.tr('Edit'),
                                      visualDensity: VisualDensity.compact,
                                      icon: const Icon(Icons.edit_outlined,
                                          size: 16),
                                      onPressed: () =>
                                          _editTask(context, ref, t),
                                    ),
                                    if (key == 'in_progress' ||
                                        key == 'todo' ||
                                        key == 'review')
                                      IconButton(
                                        tooltip: context.tr('Stop'),
                                        visualDensity: VisualDensity.compact,
                                        icon: const Icon(Icons.stop_circle_outlined,
                                            size: 16),
                                        onPressed: () => _patch(
                                            ref, t.id, {'status': 'blocked'}),
                                      ),
                                    IconButton(
                                      tooltip: context.tr('Delete'),
                                      visualDensity: VisualDensity.compact,
                                      icon: const Icon(Icons.delete_outline,
                                          size: 16, color: AppTokens.danger),
                                      onPressed: () => _delete(ref, t.id),
                                    ),
                                  ],
                                ),
                              ),
                          ];
                        })(),
                    ],
                  );
                },
              ),
            ),
          ],
        ),
      ),
    );
  }
}

/// Chat session info: model/agent meta + context-window usage + the agent's
/// live MEMORY.md (the "memory context" the agent carries into the chat).
class _ChatInfoDialog extends ConsumerStatefulWidget {
  const _ChatInfoDialog({required this.jid, required this.title});
  final String jid;
  final String title;
  @override
  ConsumerState<_ChatInfoDialog> createState() => _ChatInfoDialogState();
}

class _ChatInfoDialogState extends ConsumerState<_ChatInfoDialog> {
  String? _folder;
  String? _modelId;
  final _memCtrl = TextEditingController();
  bool _loadingMem = true;
  bool _savingMem = false;
  bool _memDirty = false;

  @override
  void initState() {
    super.initState();
    final g = ref
        .read(groupsProvider)
        .where((g) => g.jid == widget.jid)
        .firstOrNull;
    _folder = g?.folder;
    _modelId = g?.modelId;
    _loadMemory();
  }

  @override
  void dispose() {
    _memCtrl.dispose();
    super.dispose();
  }

  Future<void> _loadMemory() async {
    if (_folder == null || _folder!.isEmpty) {
      if (mounted) setState(() => _loadingMem = false);
      return;
    }
    try {
      final r =
          await ref.read(apiClientProvider).get('/api/agents/$_folder/files');
      if (mounted && r is Map) {
        _memCtrl.text = '${r['memory'] ?? ''}';
      }
    } catch (_) {}
    if (mounted) setState(() => _loadingMem = false);
  }

  /// Persist MEMORY.md for this session's agent folder (SOUL.md untouched).
  Future<void> _saveMemory() async {
    if (_folder == null || _folder!.isEmpty) return;
    setState(() => _savingMem = true);
    try {
      await ref.read(apiClientProvider).put('/api/agents/$_folder/files',
          body: {'memory': _memCtrl.text});
      if (mounted) {
        setState(() => _memDirty = false);
        ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: Text(context.tr('Memory saved'))));
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
            content: Text('${context.tr('Save failed')}: $e')));
      }
    } finally {
      if (mounted) setState(() => _savingMem = false);
    }
  }

  Widget _row(String k, String v, Color labelColor) => Padding(
        padding: const EdgeInsets.symmetric(vertical: 3),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            SizedBox(
                width: 110,
                child: Text(context.tr(k),
                    style: TextStyle(color: labelColor, fontSize: 12))),
            Expanded(
                child: SelectableText(v,
                    style: TextStyle(
                        color: context.colors.textPrimary, fontSize: 13))),
          ],
        ),
      );

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final u = ref.watch(agentUsageProvider)[widget.jid];
    return Dialog(
      backgroundColor: c.surface,
      child: SizedBox(
        width: 560,
        height: 560,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(
                  AppTokens.s16, AppTokens.s12, AppTokens.s8, AppTokens.s8),
              child: Row(
                children: [
                  Icon(Icons.info_outline, size: 18, color: c.accent),
                  const SizedBox(width: AppTokens.s8),
                  Expanded(
                    child: Text(context.tr('Chat info'),
                        style: TextStyle(
                            color: c.textPrimary,
                            fontWeight: FontWeight.w700)),
                  ),
                  IconButton(
                    icon: const Icon(Icons.close, size: 18),
                    onPressed: () => Navigator.of(context).pop(),
                  ),
                ],
              ),
            ),
            const Divider(height: 1),
            Expanded(
              child: ListView(
                padding: const EdgeInsets.all(AppTokens.s16),
                children: [
                  // Session meta.
                  _row('Name', widget.title, c.textMuted),
                  _row('Agent', _folder?.isEmpty ?? true ? '—' : _folder!,
                      c.textMuted),
                  _row(
                      'Model',
                      (_modelId?.isEmpty ?? true)
                          ? context.tr('Active default')
                          : _modelId!,
                      c.textMuted),
                  _row('JID', widget.jid, c.textMuted),
                  const SizedBox(height: AppTokens.s16),
                  // Context length.
                  Text(context.tr('CONTEXT LENGTH'),
                      style: TextStyle(
                          color: c.textMuted,
                          fontSize: 11,
                          fontWeight: FontWeight.w700,
                          letterSpacing: 0.5)),
                  const SizedBox(height: AppTokens.s8),
                  if (u != null && u.maxTokens > 0) ...[
                    ClipRRect(
                      borderRadius: BorderRadius.circular(4),
                      child: LinearProgressIndicator(
                        value: u.pct,
                        minHeight: 8,
                        backgroundColor: c.border,
                        valueColor: AlwaysStoppedAnimation(u.pct > 0.9
                            ? AppTokens.danger
                            : (u.pct > 0.7 ? AppTokens.warning : c.accent)),
                      ),
                    ),
                    const SizedBox(height: AppTokens.s8),
                    Text(
                        context.trArgs(
                            '{use} / {max} tokens ({pct}%) · {remaining} left{promptPart}',
                            {
                              'use': u.useTokens,
                              'max': u.maxTokens,
                              'pct': (u.pct * 100).round(),
                              'remaining': u.remaining,
                              'promptPart': u.promptTokens > 0
                                  ? context.trArgs(' · prompt {p}',
                                      {'p': u.promptTokens})
                                  : '',
                            },
                        ),
                        style: TextStyle(color: c.textSecondary, fontSize: 12)),
                  ] else
                    Text(context.tr('No usage reported yet (send a message).'),
                        style: TextStyle(color: c.textMuted, fontSize: 12)),
                  const SizedBox(height: AppTokens.s12),
                  Align(
                    alignment: Alignment.centerLeft,
                    child: OutlinedButton.icon(
                      onPressed: () async {
                        await ref
                            .read(apiClientProvider)
                            .post('/api/groups/${widget.jid}/compact');
                        if (context.mounted) {
                          ScaffoldMessenger.of(context).showSnackBar(
                            SnackBar(
                                content:
                                    Text(context.tr('Compacting context…'))),
                          );
                          Navigator.of(context).pop();
                        }
                      },
                      icon: const Icon(Icons.compress, size: 16),
                      label: Text(context.tr('Compact context')),
                    ),
                  ),
                  const SizedBox(height: AppTokens.s20),
                  // Memory context — editable MEMORY.md for this session's agent.
                  Row(
                    children: [
                      Text(context.tr('MEMORY CONTEXT (MEMORY.md)'),
                          style: TextStyle(
                              color: c.textMuted,
                              fontSize: 11,
                              fontWeight: FontWeight.w700,
                              letterSpacing: 0.5)),
                      const Spacer(),
                      if (_memDirty && !_savingMem)
                        TextButton.icon(
                          onPressed: _saveMemory,
                          icon: const Icon(Icons.save_outlined, size: 14),
                          label: Text(context.tr('Save')),
                          style: TextButton.styleFrom(
                              visualDensity: VisualDensity.compact),
                        ),
                      if (_savingMem)
                        const Padding(
                          padding: EdgeInsets.only(right: AppTokens.s8),
                          child: SizedBox(
                              width: 14,
                              height: 14,
                              child:
                                  CircularProgressIndicator(strokeWidth: 2)),
                        ),
                    ],
                  ),
                  const SizedBox(height: AppTokens.s8),
                  if (_loadingMem)
                    const Center(child: CircularProgressIndicator())
                  else if (_folder == null || _folder!.isEmpty)
                    Text(context.tr('No agent folder bound to this chat.'),
                        style: TextStyle(color: c.textMuted, fontSize: 12))
                  else
                    TextField(
                      controller: _memCtrl,
                      minLines: 4,
                      maxLines: 12,
                      onChanged: (_) {
                        if (!_memDirty) setState(() => _memDirty = true);
                      },
                      style: TextStyle(
                          color: c.textSecondary,
                          fontSize: 12,
                          height: 1.5,
                          fontFamily: AppTokens.fontMono),
                      decoration: InputDecoration(
                        hintText: context.tr('No memory yet — type to add '
                            'notes the agent should remember…'),
                        hintStyle:
                            TextStyle(color: c.textMuted, fontSize: 12),
                        filled: true,
                        fillColor: c.sidebar,
                        contentPadding: const EdgeInsets.all(AppTokens.s12),
                        enabledBorder: OutlineInputBorder(
                          borderRadius:
                              BorderRadius.circular(AppTokens.rMd),
                          borderSide: BorderSide(color: c.border),
                        ),
                        focusedBorder: OutlineInputBorder(
                          borderRadius:
                              BorderRadius.circular(AppTokens.rMd),
                          borderSide: BorderSide(color: c.accent),
                        ),
                      ),
                    ),
                  const SizedBox(height: AppTokens.s20),
                  // Danger zone — wipe this session's entire chat history.
                  Text(context.tr('DANGER ZONE'),
                      style: TextStyle(
                          color: c.textMuted,
                          fontSize: 11,
                          fontWeight: FontWeight.w700,
                          letterSpacing: 0.5)),
                  const SizedBox(height: AppTokens.s8),
                  Align(
                    alignment: Alignment.centerLeft,
                    child: OutlinedButton.icon(
                      style: OutlinedButton.styleFrom(
                        foregroundColor: AppTokens.danger,
                        side: const BorderSide(color: AppTokens.danger),
                      ),
                      onPressed: _clearAllMessages,
                      icon: const Icon(Icons.delete_sweep_outlined, size: 16),
                      label: Text(context.tr('Clear all messages')),
                    ),
                  ),
                  const SizedBox(height: AppTokens.s4),
                  Text(
                      context.tr('Stops the agent and permanently deletes '
                          'every message, tool log, and chat event of this '
                          'session.'),
                      style: TextStyle(color: c.textMuted, fontSize: 11)),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }

  /// Confirm, then ask the daemon to stop the agent and permanently delete
  /// this session's persisted history (`agent:control stop_and_clear` wipes
  /// group_messages + tool_executions + chat_events). Local list is cleared
  /// directly because the provider ignores empty history:load pushes.
  Future<void> _clearAllMessages() async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text(ctx.tr('Clear all messages?')),
        content: Text(ctx.tr('This stops the agent and permanently deletes '
            'the entire chat history of this session. This cannot be '
            'undone.')),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(ctx).pop(false),
            child: Text(ctx.tr('Cancel')),
          ),
          FilledButton(
            style: FilledButton.styleFrom(backgroundColor: AppTokens.danger),
            onPressed: () => Navigator.of(ctx).pop(true),
            child: Text(ctx.tr('Delete')),
          ),
        ],
      ),
    );
    if (confirmed != true || !mounted) return;
    ref.read(wsClientProvider).send({
      'type': 'agent:control',
      'groupJid': widget.jid,
      'action': 'stop_and_clear',
    });
    ref.read(conversationProvider(widget.jid).notifier).clearLocal();
    if (mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text(context.tr('Chat history deleted'))),
      );
      Navigator.of(context).pop();
    }
  }
}

/// Small chat-kind badge shown next to the chat title (Cowork / Code).
class _KindBadge extends StatelessWidget {
  const _KindBadge({required this.kind});
  final String kind;
  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    // 'Cowork' is a brand name and is never translated; 'Schedule' / 'Code'
    // are plain UI labels.
    final (label, color, icon) = switch (kind) {
      'cowork' => ('Cowork', c.accent, Icons.groups_outlined),
      'schedule' => (context.tr('Schedule'), AppTokens.warning, Icons.schedule),
      _ => (context.tr('Code'), AppTokens.cyan, Icons.code),
    };
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: AppTokens.s8, vertical: 3),
      decoration: BoxDecoration(
        color: color.withValues(alpha: 0.14),
        borderRadius: BorderRadius.circular(AppTokens.rSm),
      ),
      child: Row(mainAxisSize: MainAxisSize.min, children: [
        Icon(icon, size: 12, color: color),
        const SizedBox(width: 4),
        Text(label,
            style: TextStyle(
                color: color, fontSize: 11, fontWeight: FontWeight.w600)),
      ]),
    );
  }
}

/// Schedule info + run history for a `schedule:<id>` chat. Replaces Plan
/// history on schedule chats; includes Edit (PATCH /api/space/schedules/:id).
class _ScheduleInfoDialog extends ConsumerStatefulWidget {
  const _ScheduleInfoDialog({required this.scheduleId});
  final String scheduleId;
  @override
  ConsumerState<_ScheduleInfoDialog> createState() =>
      _ScheduleInfoDialogState();
}

class _ScheduleInfoDialogState extends ConsumerState<_ScheduleInfoDialog> {
  Map<String, dynamic>? _data;
  bool _loading = true;
  bool _running = false;
  String? _error;

  @override
  void initState() {
    super.initState();
    _load();
  }

  /// Fire the schedule immediately (POST .../run-now), then refresh.
  Future<void> _runNow() async {
    setState(() => _running = true);
    try {
      await ref
          .read(apiClientProvider)
          .post('/api/space/schedules/${widget.scheduleId}/run-now');
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: Text(context.tr('Queued to run now'))));
        Navigator.of(context).pop();
      }
    } catch (e) {
      if (mounted) {
        setState(() => _running = false);
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
            content: Text('${context.tr('Run failed')}: $e')));
      }
    }
  }

  Future<void> _load() async {
    setState(() => _loading = true);
    try {
      final r = await ref
          .read(apiClientProvider)
          .get('/api/space/schedules/${widget.scheduleId}');
      _data = r is Map ? r.cast<String, dynamic>() : null;
      _error = null;
    } catch (e) {
      _error = '$e';
    }
    if (mounted) setState(() => _loading = false);
  }

  String _s(String k) => '${_data?[k] ?? ''}';

  /// ISO timestamp (UTC from the daemon) → local `yyyy-MM-dd HH:mm` for
  /// display; falls back to the raw string when unparsable.
  static String _fmtTs(String raw) {
    final dt = DateTime.tryParse(raw);
    if (dt == null) return raw;
    final l = dt.toLocal();
    String two(int n) => n.toString().padLeft(2, '0');
    return '${l.year}-${two(l.month)}-${two(l.day)} ${two(l.hour)}:${two(l.minute)}';
  }

  String _localTs(String k) => _fmtTs(_s(k));

  Future<void> _edit() async {
    final schedule = SpaceSchedule.fromJson({
      'id': widget.scheduleId,
      ..._data ?? {},
    });
    await showDialog(
      context: context,
      builder: (_) =>
          ScheduleEditorDialog(existing: schedule, showStatus: true),
    );
    await _load();
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final runs = (_data?['runs'] as List?) ?? const [];
    return Dialog(
      backgroundColor: c.surface,
      child: SizedBox(
        width: 560,
        height: 560,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(
                  AppTokens.s16, AppTokens.s12, AppTokens.s8, AppTokens.s8),
              child: Row(
                children: [
                  Icon(Icons.event_repeat_outlined,
                      size: 18, color: c.accent),
                  const SizedBox(width: AppTokens.s8),
                  Text(context.tr('Schedule'),
                      style: TextStyle(
                          color: c.textPrimary, fontWeight: FontWeight.w700)),
                  const Spacer(),
                  if (_data != null)
                    FilledButton.tonalIcon(
                      onPressed: _running ? null : _runNow,
                      icon: _running
                          ? const SizedBox(
                              width: 14,
                              height: 14,
                              child:
                                  CircularProgressIndicator(strokeWidth: 2))
                          : const Icon(Icons.play_arrow_rounded, size: 18),
                      label: Text(context.tr('Run now')),
                    ),
                  const SizedBox(width: AppTokens.s8),
                  if (_data != null)
                    TextButton.icon(
                      onPressed: _edit,
                      icon: const Icon(Icons.edit_outlined, size: 16),
                      label: Text(context.tr('Edit')),
                    ),
                  IconButton(
                    tooltip: context.tr('Reload'),
                    icon: const Icon(Icons.refresh, size: 16),
                    onPressed: _load,
                  ),
                  IconButton(
                    icon: const Icon(Icons.close, size: 18),
                    onPressed: () => Navigator.of(context).pop(),
                  ),
                ],
              ),
            ),
            const Divider(height: 1),
            Expanded(
              child: _loading
                  ? const Center(child: CircularProgressIndicator())
                  : _error != null
                      ? Center(
                          child: Text(_error!,
                              style:
                                  const TextStyle(color: AppTokens.danger)))
                      : ListView(
                          padding: const EdgeInsets.all(AppTokens.s16),
                          children: [
                            // Prompt in its own box (it's usually multi-line).
                            Text(context.tr('PROMPT'),
                                style: TextStyle(
                                    color: c.textMuted,
                                    fontSize: 11,
                                    fontWeight: FontWeight.w700,
                                    letterSpacing: 0.5)),
                            const SizedBox(height: AppTokens.s4),
                            Container(
                              width: double.infinity,
                              padding: const EdgeInsets.all(AppTokens.s12),
                              decoration: BoxDecoration(
                                color: c.sidebar,
                                borderRadius:
                                    BorderRadius.circular(AppTokens.rMd),
                                border: Border.all(color: c.border),
                              ),
                              child: SelectableText(
                                  _s('prompt').isEmpty ? '—' : _s('prompt'),
                                  style: TextStyle(
                                      color: c.textPrimary,
                                      fontSize: 13,
                                      height: 1.45)),
                            ),
                            const SizedBox(height: AppTokens.s12),
                            // Status pill + schedule meta.
                            Row(children: [
                              _statusPill(c, _s('status')),
                              const Spacer(),
                              Icon(Icons.schedule,
                                  size: 14, color: c.textMuted),
                              const SizedBox(width: AppTokens.s4),
                              SelectableText(
                                _s('cron').isNotEmpty
                                    ? _s('cron')
                                    : _s('schedule_value'),
                                style: TextStyle(
                                    color: c.textSecondary,
                                    fontSize: 12,
                                    fontFamily: AppTokens.fontMono),
                              ),
                            ]),
                            const SizedBox(height: AppTokens.s8),
                            if (_s('next_run').isNotEmpty)
                              _kv(c, 'Next run', _localTs('next_run')),
                            if (_s('last_run').isNotEmpty)
                              _kv(c, 'Last run', _localTs('last_run')),
                            const SizedBox(height: AppTokens.s16),
                            Text(context.tr('HISTORY'),
                                style: TextStyle(
                                    color: c.textMuted,
                                    fontSize: 11,
                                    fontWeight: FontWeight.w700,
                                    letterSpacing: 0.5)),
                            const SizedBox(height: AppTokens.s8),
                            if (runs.isEmpty)
                              Text(context.tr('No runs yet'),
                                  style: TextStyle(
                                      color: c.textMuted, fontSize: 12))
                            else
                              for (final r in runs.whereType<Map>())
                                Padding(
                                  padding:
                                      const EdgeInsets.symmetric(vertical: 3),
                                  child: Row(children: [
                                    Container(
                                      width: 8,
                                      height: 8,
                                      decoration: BoxDecoration(
                                          shape: BoxShape.circle,
                                          color: '${r['status']}' == 'ok' ||
                                                  '${r['status']}' == 'done'
                                              ? AppTokens.success
                                              : '${r['status']}' == 'error'
                                                  ? AppTokens.danger
                                                  : c.textMuted),
                                    ),
                                    const SizedBox(width: AppTokens.s8),
                                    Expanded(
                                      child: Text(
                                          '${_fmtTs('${r['ranAt'] ?? r['ran_at'] ?? r['created_at'] ?? ''}')} · ${r['status'] ?? ''}',
                                          maxLines: 1,
                                          overflow: TextOverflow.ellipsis,
                                          style: TextStyle(
                                              color: c.textSecondary,
                                              fontSize: 12)),
                                    ),
                                  ]),
                                ),
                          ],
                        ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _kv(dynamic c, String k, String v) => Padding(
        padding: const EdgeInsets.symmetric(vertical: 3),
        child: Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
          SizedBox(
              width: 90,
              child: Text(context.tr(k),
                  style: TextStyle(color: c.textMuted, fontSize: 12))),
          Expanded(
              child: SelectableText(v.isEmpty ? '—' : v,
                  style: TextStyle(color: c.textPrimary, fontSize: 13))),
        ]),
      );

  Widget _statusPill(dynamic c, String status) {
    final s = status.isEmpty ? 'unknown' : status;
    final color = s == 'active'
        ? AppTokens.success
        : s == 'paused'
            ? AppTokens.warning
            : c.textMuted;
    return Container(
      padding:
          const EdgeInsets.symmetric(horizontal: AppTokens.s8, vertical: 3),
      decoration: BoxDecoration(
        color: color.withValues(alpha: 0.14),
        borderRadius: BorderRadius.circular(AppTokens.rFull),
      ),
      child: Row(mainAxisSize: MainAxisSize.min, children: [
        Container(
            width: 7,
            height: 7,
            decoration: BoxDecoration(color: color, shape: BoxShape.circle)),
        const SizedBox(width: AppTokens.s6),
        Text(s,
            style: TextStyle(
                color: color, fontSize: 12, fontWeight: FontWeight.w600)),
      ]),
    );
  }
}

/// "Agent is typing" row appended under the last message while the agent is
/// busy (channel_app parity): a small agent-side bubble with three bouncing
/// dots. Hidden as soon as a streaming reply bubble takes over.
class _TypingIndicatorRow extends StatelessWidget {
  const _TypingIndicatorRow();

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Padding(
      padding: const EdgeInsets.fromLTRB(
          AppTokens.s16, AppTokens.s8, AppTokens.s16, AppTokens.s8),
      child: Align(
        alignment: Alignment.centerLeft,
        child: Container(
          padding: const EdgeInsets.symmetric(
              horizontal: AppTokens.s12, vertical: AppTokens.s12),
          decoration: BoxDecoration(
            color: c.bubbleAgent,
            borderRadius: const BorderRadius.only(
              topLeft: Radius.circular(4),
              topRight: Radius.circular(16),
              bottomLeft: Radius.circular(16),
              bottomRight: Radius.circular(16),
            ),
            border: Border.all(color: c.border),
          ),
          child: const _TypingDots(),
        ),
      ),
    );
  }
}

/// Three dots pulsing in a staggered wave (ported from channel_app).
class _TypingDots extends StatefulWidget {
  const _TypingDots();

  @override
  State<_TypingDots> createState() => _TypingDotsState();
}

class _TypingDotsState extends State<_TypingDots>
    with SingleTickerProviderStateMixin {
  late final AnimationController _ctrl = AnimationController(
      vsync: this, duration: const Duration(milliseconds: 1000))
    ..repeat();

  @override
  void dispose() {
    _ctrl.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return AnimatedBuilder(
      animation: _ctrl,
      builder: (context, _) {
        return Row(
          mainAxisSize: MainAxisSize.min,
          children: List.generate(3, (i) {
            final t = (_ctrl.value - i * 0.15) % 1.0;
            final scale = t < 0.5 ? 0.6 + t * 0.8 : 1.0 - (t - 0.5) * 0.8;
            return Padding(
              padding: const EdgeInsets.symmetric(horizontal: 2),
              child: Transform.scale(
                scale: scale.clamp(0.6, 1.0),
                child: Container(
                  width: 6,
                  height: 6,
                  decoration: BoxDecoration(
                    shape: BoxShape.circle,
                    color: c.accent.withValues(alpha: 0.6),
                  ),
                ),
              ),
            );
          }),
        );
      },
    );
  }
}
