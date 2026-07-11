import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../../core/transport/connection.dart';
import '../../../models/chat_message.dart';
import '../../../theme/tokens.dart';
import '../../../widgets/app_markdown.dart';
import '../../dock/dispatch_provider.dart';
import '../audio_service.dart';
import 'form_card.dart';
import 'question_card.dart';

/// Dispatches a [ChatMessage] to the right bubble/card by kind.
class MessageItem extends StatelessWidget {
  const MessageItem({
    super.key,
    required this.message,
    required this.onPermission,
    required this.onQuestion,
    required this.onForm,
  });
  final ChatMessage message;
  final void Function(String requestId, String optionKey) onPermission;
  final void Function(
    String requestId,
    Map<int, dynamic> answers,
    Map<int, String> otherTexts,
  ) onQuestion;
  final void Function(
    String requestId,
    Map<String, dynamic> values,
    bool submitted,
  ) onForm;

  @override
  Widget build(BuildContext context) {
    switch (message.kind) {
      case MessageKind.tool:
        return _ToolCard(message: message);
      case MessageKind.permission:
        return _PermissionCard(message: message, onResolve: onPermission);
      case MessageKind.question:
        return QuestionCard(message: message, onSubmit: onQuestion);
      case MessageKind.form:
        return FormCard(message: message, onSubmit: onForm);
      case MessageKind.system:
        return _SystemNote(text: message.text ?? '');
      default:
        return _TextBubble(message: message);
    }
  }
}

class _TextBubble extends StatelessWidget {
  const _TextBubble({required this.message});
  final ChatMessage message;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    // Cross-channel user messages (e.g. from the mobile channel_app) arrive as
    // MessageKind.other. They're still the user's own input, just from another
    // client, so render them user-side (right-aligned, user bubble) while the
    // source label below keeps showing where they came from ("mobile-app").
    final isUser = message.kind == MessageKind.user;
    final isOther = message.kind == MessageKind.other;
    final userSide = isUser || isOther;
    final align = userSide ? Alignment.centerRight : Alignment.centerLeft;
    final bg = userSide ? c.bubbleUser : c.bubbleAgent;
    final parts = _splitThink(message.text ?? '');

    // Don't render an empty bubble (e.g. a reasoning-only message after the
    // tags were stripped) — that was the stray "pending" box at the top.
    if (parts.isEmpty &&
        message.attachments.isEmpty &&
        !message.streaming) {
      return const SizedBox.shrink();
    }

    return Container(
      alignment: align,
      padding: const EdgeInsets.symmetric(
        horizontal: AppTokens.s24,
        vertical: AppTokens.s4,
      ),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 720),
        child: Container(
          padding: const EdgeInsets.symmetric(
            horizontal: AppTokens.s16,
            vertical: AppTokens.s12,
          ),
          decoration: BoxDecoration(
            color: bg,
            border: Border.all(color: c.border),
            borderRadius: BorderRadius.circular(AppTokens.rLg),
          ),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              // Only label real incoming senders, never the agent itself.
              if (message.kind == MessageKind.other && message.sender != null)
                Padding(
                  padding: const EdgeInsets.only(bottom: AppTokens.s4),
                  child: Text(
                    message.sender!,
                    style: TextStyle(
                      color: c.accent,
                      fontSize: 12,
                      fontWeight: FontWeight.w600,
                    ),
                  ),
                ),
              for (final part in parts)
                part.isThink
                    ? _ReasoningTile(text: part.text)
                    : Padding(
                        padding: const EdgeInsets.symmetric(vertical: 1),
                        child: AppMarkdown(
                          part.text,
                          style: TextStyle(
                              color: c.textPrimary, height: 1.5, fontSize: 14),
                        ),
                      ),
              if (message.attachments.isNotEmpty)
                _Attachments(attachments: message.attachments),
              // Footer: tokens + actions (left) · timestamp (bottom-right).
              // Reasoning-only ("think") messages get no footer/time.
              Builder(builder: (_) {
                final hasContent = parts.any((p) => !p.isThink);
                final showTokens =
                    message.tokens != null || message.streaming;
                final showActions = message.kind == MessageKind.agent &&
                    !message.streaming &&
                    hasContent;
                final time = hasContent ? _fmtTime(message.ts) : '';
                if (!showTokens && !showActions && time.isEmpty) {
                  return const SizedBox.shrink();
                }
                return Padding(
                  padding: const EdgeInsets.only(top: AppTokens.s6),
                  child: Row(
                    // User bubbles hug their content; agent footers span so the
                    // timestamp sits flush right past the actions/tokens.
                    mainAxisSize:
                        userSide ? MainAxisSize.min : MainAxisSize.max,
                    crossAxisAlignment: CrossAxisAlignment.end,
                    children: [
                      if (showTokens)
                        Padding(
                          padding: const EdgeInsets.only(right: AppTokens.s8),
                          child: Text(
                            message.streaming ? '…' : '${message.tokens} tok',
                            style:
                                TextStyle(color: c.textMuted, fontSize: 12),
                          ),
                        ),
                      if (showActions)
                        _AgentActions(
                            text: parts
                                .where((p) => !p.isThink)
                                .map((p) => p.text)
                                .join('\n\n')),
                      if (!userSide) const Spacer(),
                      if (time.isNotEmpty)
                        Text(time,
                            style:
                                TextStyle(color: c.textMuted, fontSize: 11)),
                    ],
                  ),
                );
              }),
            ],
          ),
        ),
      ),
    );
  }
}

/// Format a message timestamp (ISO or "YYYY-MM-DD HH:MM:SS"). Within 30 minutes
/// it's relative ("now" / "Nm"); same-day → HH:mm; another day → "dd/MM HH:mm".
String _fmtTime(String? s) {
  if (s == null || s.isEmpty) return '';
  final dt = DateTime.tryParse(s) ??
      (s.contains(' ') ? DateTime.tryParse(s.replaceFirst(' ', 'T')) : null);
  if (dt == null) return '';
  final l = dt.toLocal();
  final now = DateTime.now();
  final diff = now.difference(l);
  if (diff.inSeconds >= 0 && diff.inSeconds < 60) return 'now';
  if (diff.inMinutes >= 0 && diff.inMinutes < 30) return '${diff.inMinutes}m';
  final hm =
      '${l.hour.toString().padLeft(2, '0')}:${l.minute.toString().padLeft(2, '0')}';
  final sameDay = l.year == now.year && l.month == now.month && l.day == now.day;
  if (sameDay) return hm;
  final dm = '${l.day.toString().padLeft(2, '0')}/${l.month.toString().padLeft(2, '0')}';
  return '$dm $hm';
}

/// Hover/persistent action row under an agent message: copy + save-to-notes
/// (mirrors the web MessageBubble actions).
class _AgentActions extends ConsumerStatefulWidget {
  const _AgentActions({required this.text});
  final String text;
  @override
  ConsumerState<_AgentActions> createState() => _AgentActionsState();
}

class _AgentActionsState extends ConsumerState<_AgentActions> {
  String? _flash; // transient "Copied" / "Saved"

  void _flashMsg(String m) {
    setState(() => _flash = m);
    Future.delayed(const Duration(seconds: 2), () {
      if (mounted) setState(() => _flash = null);
    });
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Padding(
      padding: const EdgeInsets.only(top: AppTokens.s6),
      child: Row(
        children: [
          _act(Icons.copy_outlined, 'Copy', () async {
            await Clipboard.setData(ClipboardData(text: widget.text));
            _flashMsg('Copied');
          }),
          const SizedBox(width: AppTokens.s4),
          _act(Icons.bookmark_add_outlined, 'Save note', () async {
            try {
              await ref
                  .read(apiClientProvider)
                  .post('/api/quicknotes', body: {'text': widget.text});
              _flashMsg('Saved');
            } catch (_) {
              _flashMsg('Failed');
            }
          }),
          const SizedBox(width: AppTokens.s4),
          _act(Icons.volume_up_outlined, 'Play (TTS)', () async {
            try {
              _flashMsg('Speaking…');
              await ref.read(audioServiceProvider).speak(widget.text);
            } catch (_) {
              _flashMsg('TTS failed');
            }
          }),
          if (_flash != null) ...[
            const SizedBox(width: AppTokens.s8),
            Text(_flash!,
                style: TextStyle(color: c.textMuted, fontSize: 12)),
          ],
        ],
      ),
    );
  }

  Widget _act(IconData icon, String tip, VoidCallback onTap) {
    final c = context.colors;
    return Tooltip(
      message: tip,
      child: InkWell(
        borderRadius: BorderRadius.circular(AppTokens.rSm),
        onTap: onTap,
        child: Padding(
          padding: const EdgeInsets.all(4),
          child: Icon(icon, size: 14, color: c.textMuted),
        ),
      ),
    );
  }
}

/// A segment of an agent message: either reasoning (`<think>…`) or content.
class _Part {
  final bool isThink;
  final String text;
  const _Part(this.isThink, this.text);
}

/// Split agent text into reasoning vs content parts. Handles multiple think
/// blocks, an unterminated `<think>` (still streaming), AND a dangling
/// `</think>` whose opener was lost (streamed/stored separately) — the latter
/// is why raw `</think>` used to leak into the UI.
// Reasoning wrappers emitted by different models (mirrors the web
// reasoningBlocks util): Qwen `think`, `thinking`, DeepSeek
// `redacted_reasoning` / `redacted_thinking`. Tags may carry attributes.
const _reasoningTags = 'think|thinking|redacted_reasoning|redacted_thinking';
final _reasoningOpenRe = RegExp('<(?:$_reasoningTags)\\b[^>]*>', caseSensitive: false);
final _reasoningCloseRe = RegExp('</(?:$_reasoningTags)>', caseSensitive: false);
final _reasoningBlockRe = RegExp(
    '<(?:$_reasoningTags)\\b[^>]*>([\\s\\S]*?)(?:</(?:$_reasoningTags)>|\$)',
    caseSensitive: false);

List<_Part> _splitThink(String text) {
  var norm = text;
  final firstOpen = _reasoningOpenRe.firstMatch(norm)?.start ?? -1;
  final firstClose = _reasoningCloseRe.firstMatch(norm)?.start ?? -1;
  // Reasoning that starts the message but lost its opening tag.
  if (firstClose != -1 && (firstOpen == -1 || firstClose < firstOpen)) {
    norm = '<think>$norm';
  }
  if (!_reasoningOpenRe.hasMatch(norm)) {
    final t = norm.trim();
    return t.isEmpty ? const [] : [_Part(false, t)];
  }
  final parts = <_Part>[];
  var cursor = 0;
  for (final m in _reasoningBlockRe.allMatches(norm)) {
    if (m.start > cursor) {
      final before = norm.substring(cursor, m.start).trim();
      if (before.isNotEmpty) parts.add(_Part(false, before));
    }
    final think = (m.group(1) ?? '').trim();
    if (think.isNotEmpty) parts.add(_Part(true, think));
    cursor = m.end;
  }
  if (cursor < norm.length) {
    final rest = norm.substring(cursor).trim();
    if (rest.isNotEmpty) parts.add(_Part(false, rest));
  }
  // Strip any stray reasoning tags that slipped through, drop empties.
  return parts
      .map((p) => _Part(
          p.isThink,
          p.text
              .replaceAll(_reasoningOpenRe, '')
              .replaceAll(_reasoningCloseRe, '')
              .trim()))
      .where((p) => p.text.isNotEmpty)
      .toList();
}

/// Collapsible reasoning block (collapsed by default), styled dim.
class _ReasoningTile extends StatefulWidget {
  const _ReasoningTile({required this.text});
  final String text;
  @override
  State<_ReasoningTile> createState() => _ReasoningTileState();
}

class _ReasoningTileState extends State<_ReasoningTile> {
  bool _open = false;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    // Compact inline "think ›" row (web ReasoningCollapsible), collapsed default.
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        InkWell(
          borderRadius: BorderRadius.circular(AppTokens.rSm),
          onTap: () => setState(() => _open = !_open),
          child: Padding(
            padding: const EdgeInsets.symmetric(vertical: AppTokens.s4),
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                // Filled bulb in the info accent — matches web BulbFilled.
                Icon(Icons.lightbulb, size: 13, color: c.accent),
                const SizedBox(width: AppTokens.s8),
                Text('think',
                    style: TextStyle(color: c.textSecondary, fontSize: 13)),
                const SizedBox(width: AppTokens.s6),
                Text(_open ? '▾' : '›',
                    style: TextStyle(color: c.textMuted, fontSize: 13)),
              ],
            ),
          ),
        ),
        if (_open)
          Container(
            margin: const EdgeInsets.only(left: 18, top: 2, bottom: AppTokens.s4),
            padding: const EdgeInsets.only(left: AppTokens.s12),
            decoration: BoxDecoration(
              border: Border(left: BorderSide(color: c.border, width: 2)),
            ),
            child: AppMarkdown(
              widget.text,
              style: TextStyle(
                  color: c.textSecondary,
                  fontSize: 12,
                  height: 1.5,
                  fontStyle: FontStyle.italic),
            ),
          ),
      ],
    );
  }
}

class _ToolCard extends StatelessWidget {
  const _ToolCard({required this.message});
  final ChatMessage message;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Padding(
      padding: const EdgeInsets.symmetric(
        horizontal: AppTokens.s24,
        vertical: AppTokens.s4,
      ),
      child: Container(
        padding: const EdgeInsets.symmetric(
          horizontal: AppTokens.s12,
          vertical: AppTokens.s8,
        ),
        decoration: BoxDecoration(
          color: c.surface,
          border: Border.all(color: c.border),
          borderRadius: BorderRadius.circular(AppTokens.rMd),
        ),
        child: Row(
          children: [
            Icon(
              message.toolOk ? Icons.check_circle_outline : Icons.error_outline,
              size: 16,
              color: message.toolOk ? AppTokens.cyan : AppTokens.danger,
            ),
            const SizedBox(width: AppTokens.s8),
            Flexible(
              child: Text(
                message.toolName,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(
                  color: c.textSecondary,
                  fontWeight: FontWeight.w600,
                  fontSize: 12,
                ),
              ),
            ),
            const SizedBox(width: AppTokens.s8),
            Expanded(
              flex: 2,
              child: Text(
                message.toolTitle,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(color: c.textMuted, fontSize: 12),
              ),
            ),
            if (message.toolSummary.isNotEmpty)
              Flexible(
                child: Padding(
                  padding: const EdgeInsets.only(left: AppTokens.s8),
                  child: Text(
                    message.toolSummary,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(color: c.textMuted, fontSize: 12),
                  ),
                ),
              ),
          ],
        ),
      ),
    );
  }
}

class _PermissionCard extends StatelessWidget {
  const _PermissionCard({required this.message, required this.onResolve});
  final ChatMessage message;
  final void Function(String requestId, String optionKey) onResolve;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final resolvedKey = message.resolvedKey;
    return Padding(
      padding: const EdgeInsets.symmetric(
        horizontal: AppTokens.s24,
        vertical: AppTokens.s8,
      ),
      child: Container(
        padding: const EdgeInsets.all(AppTokens.s16),
        decoration: BoxDecoration(
          color: c.surface,
          border: Border.all(color: AppTokens.warning.withValues(alpha: 0.5)),
          borderRadius: BorderRadius.circular(AppTokens.rLg),
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                const Icon(Icons.lock_outline,
                    size: 16, color: AppTokens.warning),
                const SizedBox(width: AppTokens.s8),
                Text(
                  message.permTitle.isNotEmpty
                      ? message.permTitle
                      : 'Permission required',
                  style: TextStyle(
                    color: c.textPrimary,
                    fontWeight: FontWeight.w700,
                  ),
                ),
              ],
            ),
            if (message.permContent.isNotEmpty) ...[
              const SizedBox(height: AppTokens.s8),
              Text(
                message.permContent,
                style: TextStyle(color: c.textSecondary, fontSize: 14),
              ),
            ],
            const SizedBox(height: AppTokens.s12),
            if (message.resolved)
              Text(
                'Resolved: ${resolvedKey ?? 'answered'}',
                style: TextStyle(color: c.textMuted, fontSize: 12),
              )
            else
              Wrap(
                spacing: AppTokens.s8,
                runSpacing: AppTokens.s8,
                children: [
                  for (final opt in message.permOptions)
                    FilledButton.tonal(
                      onPressed: () =>
                          onResolve(message.requestId, '${opt['key']}'),
                      child: Text('${opt['label'] ?? opt['key']}'),
                    ),
                ],
              ),
          ],
        ),
      ),
    );
  }
}

class _SystemNote extends StatelessWidget {
  const _SystemNote({required this.text});
  final String text;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Padding(
      padding: const EdgeInsets.symmetric(
        horizontal: AppTokens.s24,
        vertical: AppTokens.s8,
      ),
      child: Center(
        child: Text(
          text,
          style: TextStyle(color: c.textMuted, fontSize: 12),
        ),
      ),
    );
  }
}

/// Collapse any run of whitespace (incl. newlines from pretty-printed JSON
/// results) into single spaces so a Text with maxLines+ellipsis stays on one
/// tidy line instead of overflowing the card.
String _oneLine(String s) => s.replaceAll(RegExp(r'\s+'), ' ').trim();

/// Human verb for a tool name (claude-code style summary), ported from the web
/// ToolGroupCard.toolVerb.
String toolVerb(String raw) {
  // strip mcp__server__ prefix to the bare tool
  final name = raw.contains('__') ? raw.split('__').last : raw;
  switch (name) {
    case 'Read':
    case 'read_file':
      return 'Read a file';
    case 'Write':
    case 'write_file':
      return 'Created a file';
    case 'Edit':
    case 'NotebookEdit':
    case 'edit_file':
      return 'Edited a file';
    case 'Bash':
    case 'bash':
      return 'Ran a command';
    case 'Glob':
      return 'Searched files';
    case 'Grep':
      return 'Searched content';
    case 'WebFetch':
      return 'Fetched a URL';
    case 'WebSearch':
      return 'Searched the web';
    case 'ToolSearch':
      return 'Discovered a tool';
    case 'Skill':
      return 'Invoked a skill';
    case 'Task':
      return 'Spawned a subagent';
  }
  if (raw.startsWith('mcp__browser')) return 'Browser action';
  if (raw.startsWith('mcp__memory') || raw.startsWith('mcp__senclaw-memory')) {
    return 'Memory lookup';
  }
  if (raw.startsWith('mcp__wiki') || raw.startsWith('mcp__senclaw-wiki')) {
    return 'Wiki action';
  }
  return 'Used a tool';
}

/// Collapsible card that groups CONSECUTIVE tool calls (web ToolGroupCard):
/// collapsed shows a verb summary ("Read a file, Ran a command ›"), expanded
/// lists each tool row.
class ToolGroupCard extends StatefulWidget {
  const ToolGroupCard({super.key, required this.tools});
  final List<ChatMessage> tools;
  @override
  State<ToolGroupCard> createState() => _ToolGroupCardState();
}

class _ToolGroupCardState extends State<ToolGroupCard> {
  bool _open = false;

  String get _summary {
    // Count by verb, preserving first-seen order.
    final counts = <String, int>{};
    for (final t in widget.tools) {
      final v = toolVerb(t.toolName);
      counts[v] = (counts[v] ?? 0) + 1;
    }
    return counts.entries
        .map((e) => e.value > 1 ? '${e.key} ×${e.value}' : e.key)
        .join(', ');
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final anyErr = widget.tools.any((t) => !t.toolOk);
    // Left-aligned and capped to the same max width as message bubbles so the
    // tool timeline lines up with the chat instead of spanning full width.
    return Container(
      alignment: Alignment.centerLeft,
      padding: const EdgeInsets.symmetric(
          horizontal: AppTokens.s24, vertical: AppTokens.s4),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 720),
        child: Container(
        decoration: BoxDecoration(
          color: c.surface,
          border: Border.all(color: c.border),
          borderRadius: BorderRadius.circular(AppTokens.rMd),
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            InkWell(
              borderRadius: BorderRadius.circular(AppTokens.rMd),
              onTap: () => setState(() => _open = !_open),
              child: Padding(
                padding: const EdgeInsets.symmetric(
                    horizontal: AppTokens.s12, vertical: AppTokens.s8),
                child: Row(
                  children: [
                    Icon(
                      anyErr ? Icons.error_outline : Icons.build_circle_outlined,
                      size: 15,
                      color: anyErr ? AppTokens.danger : c.textMuted,
                    ),
                    const SizedBox(width: AppTokens.s8),
                    Expanded(
                      child: Text(_summary,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(
                              color: c.textSecondary, fontSize: 12)),
                    ),
                    if (widget.tools.length > 1)
                      Text('${widget.tools.length}',
                          style: TextStyle(color: c.textMuted, fontSize: 11)),
                    const SizedBox(width: AppTokens.s4),
                    Icon(_open ? Icons.expand_less : Icons.expand_more,
                        size: 16, color: c.textMuted),
                  ],
                ),
              ),
            ),
            if (_open)
              for (final t in widget.tools) _ToolDetailRow(tool: t),
          ],
        ),
      ),
      ),
    );
  }
}

/// One tool inside an expanded [ToolGroupCard]: a header row (status + name +
/// one-line title/summary) that, when it has full detail, taps to reveal the
/// tool's `content` (command + output, diff, matches…) in a mono block — the
/// per-tool detail the web tool/ components render.
class _ToolDetailRow extends StatefulWidget {
  const _ToolDetailRow({required this.tool});
  final ChatMessage tool;
  @override
  State<_ToolDetailRow> createState() => _ToolDetailRowState();
}

class _ToolDetailRowState extends State<_ToolDetailRow> {
  // File-mutation tools reveal their diff immediately (web `shouldAutoExpand`
  // parity) — the whole point of expanding the group is to read the change.
  late bool _open = _isFileMutationTool(widget.tool.toolName);

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final t = widget.tool;
    final hasDetail = t.toolContent.isNotEmpty;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        InkWell(
          onTap: hasDetail ? () => setState(() => _open = !_open) : null,
          child: Padding(
            padding: const EdgeInsets.fromLTRB(
                AppTokens.s12, 0, AppTokens.s12, AppTokens.s8),
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Padding(
                  padding: const EdgeInsets.only(top: 1),
                  child: Icon(
                      t.toolOk
                          ? Icons.check_circle_outline
                          : Icons.error_outline,
                      size: 13,
                      color: t.toolOk ? AppTokens.cyan : AppTokens.danger),
                ),
                const SizedBox(width: AppTokens.s8),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(t.toolName,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(
                              color: c.textSecondary,
                              fontSize: 12,
                              fontWeight: FontWeight.w600,
                              fontFamily: AppTokens.fontMono)),
                      if (t.toolTitle.isNotEmpty && t.toolTitle != t.toolName)
                        Text(_oneLine(t.toolTitle),
                            maxLines: 2,
                            overflow: TextOverflow.ellipsis,
                            style: TextStyle(color: c.textMuted, fontSize: 12)),
                      if (t.toolSummary.isNotEmpty)
                        Text(_oneLine(t.toolSummary),
                            maxLines: 2,
                            overflow: TextOverflow.ellipsis,
                            style: TextStyle(color: c.textMuted, fontSize: 11)),
                    ],
                  ),
                ),
                if (hasDetail)
                  Icon(_open ? Icons.expand_less : Icons.expand_more,
                      size: 15, color: c.textMuted),
              ],
            ),
          ),
        ),
        if (_open && hasDetail)
          Container(
            width: double.infinity,
            margin: const EdgeInsets.fromLTRB(
                AppTokens.s12, 0, AppTokens.s12, AppTokens.s8),
            padding: const EdgeInsets.all(AppTokens.s8),
            constraints: const BoxConstraints(maxHeight: 320),
            decoration: BoxDecoration(
              color: c.sidebar,
              borderRadius: BorderRadius.circular(AppTokens.rSm),
              border: Border.all(color: c.border),
            ),
            child: SingleChildScrollView(
              // Write/Edit tools carry a structured `{path, size, diff}` payload:
              // render it as a colored code diff (web EditDetail parity) instead
              // of a raw JSON dump. Falls back to the mono dump for other tools.
              child: _DiffView.forTool(t) ??
                  SelectableText(
                    t.toolContent,
                    style: const TextStyle(
                        fontSize: 11,
                        height: 1.4,
                        fontFamily: AppTokens.fontMono),
                  ),
            ),
          ),
      ],
    );
  }
}

/// True for the file-mutation tools (native + MCP `*_file` variants) whose
/// `content` payload should render as a code diff.
bool _isFileMutationTool(String raw) {
  final name = raw.contains('__') ? raw.split('__').last : raw;
  return name == 'Write' ||
      name == 'Edit' ||
      name == 'NotebookEdit' ||
      name == 'write_file' ||
      name == 'edit_file';
}

/// Colored unified-diff view for Write/Edit tool results — the Flutter parity of
/// the web `EditDetail`/`DiffBlock`. Renders `content.diff` line-by-line, coloring
/// `+`/`-`/`@@`/header lines; falls back to `oldString`/`newString` when a
/// structured diff isn't present. Returns null (via [forTool]) when the tool
/// isn't a file mutation or carries nothing diffable, so the caller keeps the
/// raw dump.
class _DiffView extends StatelessWidget {
  const _DiffView({required this.lines, this.path, this.plus, this.minus, this.size, this.newFile = false});

  final List<String> lines;
  final String? path;
  final int? plus;
  final int? minus;
  final int? size;
  final bool newFile;

  static const int _maxLines = 400;

  /// Build a diff view for [t] or null when not applicable.
  static Widget? forTool(ChatMessage t) {
    if (!_isFileMutationTool(t.toolName)) return null;
    final c = t.toolContentMap;
    if (c == null) return null;

    final path = (c['path'] ?? c['filePath'])?.toString();
    final size = c['size'] is num ? (c['size'] as num).toInt() : null;

    // Path 1: native unified-diff string.
    final diff = c['diff'];
    if (diff is String && diff.isNotEmpty) {
      final all = diff.split('\n');
      var plus = 0, minus = 0;
      for (final l in all) {
        if (l.startsWith('+++') || l.startsWith('---')) continue;
        if (l.startsWith('+')) {
          plus++;
        } else if (l.startsWith('-')) {
          minus++;
        }
      }
      return _DiffView(
          lines: all, path: path, plus: plus, minus: minus, size: size);
    }

    // Path 2: oldString/newString (less-structured MCP edit payloads).
    final oldStr = c['oldString'];
    final newStr = c['newString'];
    if (oldStr is String || newStr is String) {
      if (oldStr is! String && newStr is String) {
        // Write-style: preview of new content, no diff prefixes.
        return _DiffView(
            lines: newStr.split('\n'), path: path, size: size, newFile: true);
      }
      final oldLines = (oldStr is String ? oldStr : '').split('\n');
      final newLines = (newStr is String ? newStr : '').split('\n');
      return _DiffView(
        lines: [
          ...oldLines.map((l) => '-$l'),
          ...newLines.map((l) => '+$l'),
        ],
        path: path,
        plus: newLines.length,
        minus: oldLines.length,
        size: size,
      );
    }
    return null;
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final shown = lines.length > _maxLines ? lines.sublist(0, _maxLines) : lines;
    final extra = lines.length - shown.length;

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        // Header: path chip + +/- counts (or "New file").
        Wrap(
          spacing: AppTokens.s6,
          runSpacing: AppTokens.s4,
          crossAxisAlignment: WrapCrossAlignment.center,
          children: [
            if (path != null)
              Container(
                padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
                decoration: BoxDecoration(
                  color: c.surface,
                  borderRadius: BorderRadius.circular(3),
                ),
                child: Text(path!,
                    style: const TextStyle(
                        fontSize: 11, fontFamily: AppTokens.fontMono)),
              ),
            if (newFile)
              Text('New file',
                  style: TextStyle(fontSize: 11, color: c.textMuted)),
            if (plus != null)
              Text('+$plus',
                  style: const TextStyle(
                      fontSize: 11,
                      color: AppTokens.success,
                      fontWeight: FontWeight.w600)),
            if (minus != null)
              Text('−$minus',
                  style: const TextStyle(
                      fontSize: 11,
                      color: AppTokens.danger,
                      fontWeight: FontWeight.w600)),
            if (size != null)
              Text('$size bytes',
                  style: TextStyle(fontSize: 11, color: c.textMuted)),
          ],
        ),
        const SizedBox(height: AppTokens.s6),
        // Diff body: one colored row per line.
        for (final l in shown) _diffLine(context, l),
        if (extra > 0)
          Padding(
            padding: const EdgeInsets.only(top: 4),
            child: Text('… $extra more lines',
                style: TextStyle(fontSize: 11, color: c.textMuted)),
          ),
      ],
    );
  }

  Widget _diffLine(BuildContext context, String l) {
    final c = context.colors;
    Color? bg;
    Color fg = c.textSecondary;
    if (newFile) {
      // Plain new-file preview — no +/- coloring.
    } else if (l.startsWith('+++') || l.startsWith('---')) {
      fg = c.textMuted;
    } else if (l.startsWith('@@')) {
      fg = AppTokens.brand;
      bg = AppTokens.brand.withValues(alpha: 0.10);
    } else if (l.startsWith('+')) {
      fg = AppTokens.success;
      bg = AppTokens.success.withValues(alpha: 0.13);
    } else if (l.startsWith('-')) {
      fg = AppTokens.danger;
      bg = AppTokens.danger.withValues(alpha: 0.13);
    }
    return Container(
      width: double.infinity,
      color: bg,
      padding: const EdgeInsets.symmetric(horizontal: 4),
      child: SelectableText(
        l.isEmpty ? ' ' : l,
        style: TextStyle(
            fontSize: 11, height: 1.5, fontFamily: AppTokens.fontMono, color: fg),
      ),
    );
  }
}

/// Renders image attachments ({dataUrl, mimeType}) under a message bubble.
class _Attachments extends StatelessWidget {
  const _Attachments({required this.attachments});
  final List<Map<String, dynamic>> attachments;

  Uint8List? _decode(String dataUrl) {
    final comma = dataUrl.indexOf(',');
    final b64 = comma >= 0 ? dataUrl.substring(comma + 1) : dataUrl;
    try {
      return base64Decode(b64);
    } catch (_) {
      return null;
    }
  }

  /// Fullscreen zoomable preview (web ChatView image-preview modal).
  void _showFullImage(BuildContext context, ImageProvider provider) {
    showDialog(
      context: context,
      barrierColor: Colors.black87,
      builder: (ctx) => GestureDetector(
        onTap: () => Navigator.of(ctx).pop(),
        child: Stack(
          children: [
            InteractiveViewer(
              minScale: 0.5,
              maxScale: 5,
              child: Center(child: Image(image: provider, fit: BoxFit.contain)),
            ),
            Positioned(
              top: 24,
              right: 24,
              child: IconButton(
                icon: const Icon(Icons.close, color: Colors.white, size: 28),
                onPressed: () => Navigator.of(ctx).pop(),
              ),
            ),
          ],
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Padding(
      padding: const EdgeInsets.only(top: AppTokens.s6),
      child: Wrap(
        spacing: AppTokens.s8,
        runSpacing: AppTokens.s8,
        children: [
          for (final a in attachments)
            Builder(builder: (ctx) {
              final url = '${a['dataUrl'] ?? ''}';
              final box = const BoxConstraints(
                  maxWidth: 260, maxHeight: 260, minWidth: 40, minHeight: 40);
              ImageProvider? provider;
              if (url.startsWith('data:') || !url.startsWith('http')) {
                final bytes = _decode(url);
                if (bytes != null) provider = MemoryImage(bytes);
              } else {
                provider = NetworkImage(url);
              }
              if (provider == null) {
                return Icon(Icons.broken_image_outlined, color: c.textMuted);
              }
              return MouseRegion(
                cursor: SystemMouseCursors.click,
                child: GestureDetector(
                  onTap: () => _showFullImage(ctx, provider!),
                  child: ClipRRect(
                    borderRadius: BorderRadius.circular(AppTokens.rMd),
                    child: ConstrainedBox(
                      constraints: box,
                      child: Image(image: provider, fit: BoxFit.cover),
                    ),
                  ),
                ),
              );
            }),
        ],
      ),
    );
  }
}

/// A DAG dispatch parent rendered inline in the chat stream (web
/// InlineDispatchCard): goal + status + each sub-task's status/agent.
class InlineDispatchCard extends ConsumerWidget {
  const InlineDispatchCard({super.key, required this.parent});
  final DispatchParent parent;

  Color _statusColor(String s) => switch (s) {
        'done' || 'completed' => AppTokens.success,
        'error' || 'timeout' || 'failed' => AppTokens.danger,
        'active' || 'processing' || 'running' => AppTokens.warning,
        _ => AppTokens.brandAlt,
      };

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    // Live per-sub-task activity (web SubAgentActivityCard) — count + latest.
    final activity = ref.watch(dispatchProvider).activity;
    final done = parent.tasks
        .where((t) => t.status == 'done' || t.status == 'completed')
        .length;
    final total = parent.tasks.length;
    return Center(
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 720),
        child: Container(
          margin: const EdgeInsets.symmetric(
              horizontal: AppTokens.s16, vertical: AppTokens.s6),
          padding: const EdgeInsets.all(AppTokens.s12),
          decoration: BoxDecoration(
            color: c.surface,
            borderRadius: BorderRadius.circular(AppTokens.rMd),
            border: Border.all(color: c.border),
          ),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  Icon(Icons.account_tree_outlined,
                      size: 16, color: c.accent),
                  const SizedBox(width: AppTokens.s8),
                  Expanded(
                    child: Text(
                      parent.goal.isEmpty ? 'Dispatch' : parent.goal,
                      maxLines: 2,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(
                          color: c.textPrimary, fontWeight: FontWeight.w700),
                    ),
                  ),
                  const SizedBox(width: AppTokens.s8),
                  Container(
                    padding: const EdgeInsets.symmetric(
                        horizontal: AppTokens.s8, vertical: 2),
                    decoration: BoxDecoration(
                      color: _statusColor(parent.status).withValues(alpha: 0.15),
                      borderRadius: BorderRadius.circular(AppTokens.rSm),
                    ),
                    child: Text(
                      total > 0 ? '${parent.status} · $done/$total' : parent.status,
                      style: TextStyle(
                          color: _statusColor(parent.status),
                          fontSize: 11,
                          fontWeight: FontWeight.w600),
                    ),
                  ),
                ],
              ),
              if (parent.tasks.isNotEmpty) const SizedBox(height: AppTokens.s8),
              for (final t in parent.tasks)
                Builder(builder: (_) {
                  final acts =
                      activity.where((a) => a.taskId == t.id).toList();
                  return Padding(
                    padding: const EdgeInsets.symmetric(vertical: 3),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Row(
                          children: [
                            Container(
                              width: 8,
                              height: 8,
                              decoration: BoxDecoration(
                                  color: _statusColor(t.status),
                                  shape: BoxShape.circle),
                            ),
                            const SizedBox(width: AppTokens.s8),
                            Expanded(
                              child: Text(t.label,
                                  maxLines: 1,
                                  overflow: TextOverflow.ellipsis,
                                  style: TextStyle(
                                      color: c.textPrimary, fontSize: 13)),
                            ),
                            if (acts.isNotEmpty) ...[
                              Icon(Icons.bolt, size: 12, color: c.textMuted),
                              Text('${acts.length}',
                                  style: TextStyle(
                                      color: c.textMuted, fontSize: 11)),
                              const SizedBox(width: AppTokens.s6),
                            ],
                            if (t.agentId.isNotEmpty)
                              Text(t.agentId,
                                  style: TextStyle(
                                      color: c.textMuted, fontSize: 11)),
                          ],
                        ),
                        // Latest activity line for in-flight tasks.
                        if (acts.isNotEmpty &&
                            t.status != 'done' &&
                            t.status != 'completed')
                          Padding(
                            padding: const EdgeInsets.only(
                                left: 16, top: 1),
                            child: Text(_oneLine(acts.last.text),
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: TextStyle(
                                    color: c.textMuted,
                                    fontSize: 11,
                                    fontFamily: AppTokens.fontMono)),
                          ),
                      ],
                    ),
                  );
                }),
            ],
          ),
        ),
      ),
    );
  }
}
