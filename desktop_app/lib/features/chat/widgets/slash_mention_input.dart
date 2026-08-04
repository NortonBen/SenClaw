import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../../core/transport/connection.dart';
import '../../../theme/tokens.dart';
import '../../plugins/plugins_screen.dart' show skillsProvider;

/// One autocomplete row: the text inserted after the trigger + a kind tag.
class MentionSuggestion {
  const MentionSuggestion(this.insert, this.kind, this.desc);
  final String insert;
  final String kind; // skill | file | folder
  final String? desc;
}

/// Workspace a `@` picker draws from, encoded so it can key a provider family.
/// `jid:<jid>` asks the daemon to resolve the chat's own workspace; `path:<abs>`
/// names one directly (the New Chat screen, which has no jid yet).
String mentionScopeForJid(String jid) => jid.isEmpty ? '' : 'jid:$jid';
String mentionScopeForPath(String? path) =>
    (path == null || path.isEmpty) ? '' : 'path:$path';

/// `@` file/folder mentions for one workspace scope. Paths come back relative to
/// the workspace root — the same form the daemon resolves them in, so what the
/// picker offers is exactly what the agent can open.
final mentionFilesProvider =
    FutureProvider.family<List<MentionSuggestion>, String>((ref, scope) async {
  if (scope.isEmpty) return const [];
  final sep = scope.indexOf(':');
  if (sep <= 0) return const [];
  final key = scope.substring(0, sep);
  final value = scope.substring(sep + 1);
  if (value.isEmpty) return const [];
  try {
    final r = await ref.read(apiClientProvider).get(
        '/api/chat/files?$key=${Uri.encodeQueryComponent(value)}');
    final entries = (r is Map ? r['entries'] : null) as List? ?? const [];
    final out = <MentionSuggestion>[];
    for (final e in entries) {
      if (e is! Map) continue;
      final rel = e['rel']?.toString() ?? '';
      if (rel.isEmpty) continue;
      out.add(MentionSuggestion(rel, e['is_dir'] == true ? 'folder' : 'file', null));
    }
    return out;
  } catch (_) {
    return const [];
  }
});

/// A chat input that pops a suggestion list when the caret follows a `/` or `#`
/// (skills) or `@` (workspace files & folders) trigger token. Tab/Enter or click
/// inserts; ↑/↓ navigate; Esc dismisses. Falls through to [onSend] when no popup
/// is open.
class SlashMentionField extends ConsumerStatefulWidget {
  const SlashMentionField({
    super.key,
    required this.controller,
    required this.onSend,
    required this.decoration,
    this.style,
    this.history = const [],
    this.fileScope = '',
    this.minLines = 1,
    this.autofocus = false,
  });
  final int minLines;
  final bool autofocus;
  final TextEditingController controller;
  final VoidCallback onSend;
  final InputDecoration decoration;
  final TextStyle? style;

  /// Built with [mentionScopeForJid] / [mentionScopeForPath]. Empty hides file
  /// suggestions — a chat with no workspace has nothing to offer.
  final String fileScope;

  /// Previously-sent messages in this conversation, chronological
  /// (oldest → newest). ↑ on the first line recalls the newest then walks
  /// backwards; ↓ on the last line walks forward and finally restores the
  /// in-progress draft — shell-history style. Empty disables recall.
  final List<String> history;

  @override
  ConsumerState<SlashMentionField> createState() => _SlashMentionFieldState();
}

class _SlashMentionFieldState extends ConsumerState<SlashMentionField> {
  String? _trigger; // '/', '#' or '@'
  String _query = '';
  int _active = 0;

  // Shell-style history recall. `_histIdx == null` means "editing the live
  // draft"; a number is the position within `widget.history` currently shown.
  int? _histIdx;
  String _draftBackup = '';
  String _lastText = '';
  // True while we programmatically rewrite the controller for a recall, so the
  // change listener doesn't mistake it for the user typing.
  bool _applyingHistory = false;

  @override
  void initState() {
    super.initState();
    widget.controller.addListener(_onChanged);
  }

  @override
  void dispose() {
    widget.controller.removeListener(_onChanged);
    super.dispose();
  }

  void _onChanged() {
    if (_applyingHistory) return;
    final text = widget.controller.text;
    // A real text edit (not a bare caret move) drops us out of history recall.
    if (text != _lastText) {
      _lastText = text;
      _histIdx = null;
    }
    final sel = widget.controller.selection;
    // Only trigger from the text up to the caret.
    final upto = (sel.baseOffset >= 0 && sel.baseOffset <= text.length)
        ? text.substring(0, sel.baseOffset)
        : text;
    final m = RegExp(r'(?:^|\s)([/@#])([^\s]*)$').firstMatch(upto);
    final t = m?.group(1);
    final q = (m?.group(2) ?? '').toLowerCase();
    if (t != _trigger || q != _query) {
      setState(() {
        _trigger = t;
        _query = q;
        _active = 0;
      });
    }
  }

  List<MentionSuggestion> _suggestions() {
    if (_trigger == null) return const [];
    final List<MentionSuggestion> source;
    if (_trigger == '@') {
      source =
          ref.watch(mentionFilesProvider(widget.fileScope)).valueOrNull ?? const [];
    } else {
      // `/` and `#` both pin a skill — the daemon accepts either form.
      source = (ref.watch(skillsProvider).valueOrNull ?? [])
          .map((s) => MentionSuggestion(s.name, 'skill', s.description))
          .toList();
    }
    final q = _query;
    final filtered = source
        .where((i) =>
            i.insert.toLowerCase().contains(q) ||
            (i.desc ?? '').toLowerCase().contains(q))
        .toList();
    return filtered.take(12).toList();
  }

  void _apply(MentionSuggestion s) {
    final ctrl = widget.controller;
    final text = ctrl.text;
    final caret = ctrl.selection.baseOffset.clamp(0, text.length);
    final before = text.substring(0, caret);
    final after = text.substring(caret);
    // Folders keep the popup open so the user can drill into a subpath; files
    // and skills get a trailing space to close it.
    final suffix = s.kind == 'folder' ? '' : ' ';
    final replacedBefore = before.replaceFirst(
        RegExp(r'([/@#])[^\s]*$'), '$_trigger${s.insert}$suffix');
    final newText = replacedBefore + after;
    // Assigning `value` notifies the change listener synchronously, which
    // recomputes the trigger from the new text — that is how a folder insert
    // keeps the popup open on the deeper path. Only force it shut for the
    // space-terminated kinds, where no trigger can survive anyway.
    ctrl.value = TextEditingValue(
      text: newText,
      selection: TextSelection.collapsed(offset: replacedBefore.length),
    );
    if (suffix.isEmpty) {
      setState(() => _active = 0);
      return;
    }
    setState(() {
      _trigger = null;
      _query = '';
      _active = 0;
    });
  }

  KeyEventResult _onKey(FocusNode node, KeyEvent e) {
    final sugg = _suggestions();
    final open = _trigger != null && sugg.isNotEmpty;
    if (!open) {
      // Enter (no Shift) sends; everything else is normal typing.
      if (e is KeyDownEvent &&
          e.logicalKey == LogicalKeyboardKey.enter &&
          !HardwareKeyboard.instance.isShiftPressed) {
        widget.onSend();
        return KeyEventResult.handled;
      }
      // ↑/↓ recall previously-sent messages, but only at the first/last line so
      // multi-line caret movement keeps working. No modifiers (Shift = select).
      final repeatable = e is KeyDownEvent || e is KeyRepeatEvent;
      if (repeatable && _noModifiers()) {
        if (e.logicalKey == LogicalKeyboardKey.arrowUp &&
            _atFirstLine() &&
            _recall(-1)) {
          return KeyEventResult.handled;
        }
        if (e.logicalKey == LogicalKeyboardKey.arrowDown &&
            _atLastLine() &&
            _recall(1)) {
          return KeyEventResult.handled;
        }
      }
      return KeyEventResult.ignored;
    }
    if (e is! KeyDownEvent && e is! KeyRepeatEvent) {
      return KeyEventResult.ignored;
    }
    final k = e.logicalKey;
    if (k == LogicalKeyboardKey.arrowDown) {
      setState(() => _active = (_active + 1) % sugg.length);
      return KeyEventResult.handled;
    }
    if (k == LogicalKeyboardKey.arrowUp) {
      setState(() => _active = (_active - 1 + sugg.length) % sugg.length);
      return KeyEventResult.handled;
    }
    if (k == LogicalKeyboardKey.enter || k == LogicalKeyboardKey.tab) {
      _apply(sugg[_active.clamp(0, sugg.length - 1)]);
      return KeyEventResult.handled;
    }
    if (k == LogicalKeyboardKey.escape) {
      setState(() => _trigger = null);
      return KeyEventResult.handled;
    }
    return KeyEventResult.ignored;
  }

  bool _noModifiers() {
    final k = HardwareKeyboard.instance;
    return !k.isShiftPressed &&
        !k.isControlPressed &&
        !k.isAltPressed &&
        !k.isMetaPressed;
  }

  /// Caret offset, clamped and defaulting to end-of-text when the selection is
  /// missing (e.g. never focused). Non-collapsed selections return null.
  int? _caretOffset() {
    final sel = widget.controller.selection;
    if (!sel.isValid || !sel.isCollapsed) return null;
    final len = widget.controller.text.length;
    final base = sel.baseOffset;
    return base < 0 ? len : base.clamp(0, len);
  }

  bool _atFirstLine() {
    final off = _caretOffset();
    return off != null && !widget.controller.text.substring(0, off).contains('\n');
  }

  bool _atLastLine() {
    final off = _caretOffset();
    return off != null && !widget.controller.text.substring(off).contains('\n');
  }

  /// Walk history: [dir] < 0 = older, > 0 = newer. Returns true when it moved.
  bool _recall(int dir) {
    final hist = widget.history;
    if (hist.isEmpty) return false;
    int? next;
    if (dir < 0) {
      if (_histIdx == null) {
        _draftBackup = widget.controller.text; // stash the live draft
        next = hist.length - 1;
      } else {
        next = (_histIdx! - 1).clamp(0, hist.length - 1);
      }
    } else {
      if (_histIdx == null) return false; // nothing newer than the draft
      next = _histIdx! >= hist.length - 1 ? null : _histIdx! + 1;
    }
    final text = next == null ? _draftBackup : hist[next];
    _histIdx = next;
    _lastText = text;
    _applyingHistory = true;
    widget.controller.value = TextEditingValue(
      text: text,
      selection: TextSelection.collapsed(offset: text.length),
    );
    _applyingHistory = false;
    // The guarded listener skipped trigger recompute; a recalled message has no
    // active trigger token, so clear any popup state.
    setState(() {
      _trigger = null;
      _query = '';
      _active = 0;
    });
    return true;
  }

  @override
  Widget build(BuildContext context) {
    final sugg = _suggestions();
    final open = _trigger != null && sugg.isNotEmpty;
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (open)
          Padding(
            padding: const EdgeInsets.only(bottom: AppTokens.s8),
            child: _popup(sugg),
          ),
        Focus(
          onKeyEvent: _onKey,
          child: TextField(
            controller: widget.controller,
            autofocus: widget.autofocus,
            minLines: widget.minLines,
            maxLines: 8,
            style: widget.style,
            decoration: widget.decoration,
            // Enter handling lives in _onKey so it can prefer the popup.
          ),
        ),
      ],
    );
  }

  Widget _popup(List<MentionSuggestion> sugg) {
    final c = context.colors;
    return Container(
      constraints: const BoxConstraints(maxHeight: 240),
      decoration: BoxDecoration(
        color: c.surface,
        border: Border.all(color: c.border),
        borderRadius: BorderRadius.circular(AppTokens.rXl),
        boxShadow: const [
          BoxShadow(
              color: Color(0x33000000), blurRadius: 24, offset: Offset(0, 8)),
        ],
      ),
      child: ListView.builder(
        shrinkWrap: true,
        padding: const EdgeInsets.symmetric(vertical: AppTokens.s4),
        itemCount: sugg.length,
        itemBuilder: (_, i) {
          final s = sugg[i];
          final active = i == _active;
          return InkWell(
            onTap: () => _apply(s),
            child: Container(
              color: active ? c.accentSoft : Colors.transparent,
              padding: const EdgeInsets.symmetric(
                  horizontal: AppTokens.s12, vertical: AppTokens.s8),
              child: Row(
                children: [
                  _kindTag(s.kind),
                  const SizedBox(width: AppTokens.s8),
                  Expanded(
                    child: Text('$_trigger${s.insert}',
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: TextStyle(
                            color: c.textPrimary,
                            fontSize: 13,
                            fontWeight: FontWeight.w500)),
                  ),
                  if (s.desc != null && s.desc!.isNotEmpty)
                    Flexible(
                      child: Text(s.desc!,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          textAlign: TextAlign.right,
                          style: TextStyle(color: c.textMuted, fontSize: 11)),
                    ),
                ],
              ),
            ),
          );
        },
      ),
    );
  }

  Widget _kindTag(String kind) {
    final c = context.colors;
    final (color, icon) = switch (kind) {
      'skill' => (AppTokens.brand, Icons.bolt_outlined),
      'folder' => (AppTokens.warning, Icons.folder_outlined),
      _ => (c.textMuted, Icons.insert_drive_file_outlined),
    };
    return Icon(icon, size: 14, color: color);
  }
}
