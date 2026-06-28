import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../../core/prefs.dart';
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

const _kRecentDirs = 'senclaw:recent-workdirs';

/// `@` file/folder mentions sourced from the most-recent project workdir
/// (the same set the New Chat picker persists). Empty if no project is known.
final mentionFilesProvider =
    FutureProvider<List<MentionSuggestion>>((ref) async {
  final dirs = Prefs(ref.read(prefsProvider)).stringSet(_kRecentDirs).toList()
    ..sort();
  if (dirs.isEmpty) return const [];
  final root = dirs.first;
  try {
    final r = await ref.read(apiClientProvider).get(
        '/api/workspace/files?path=${Uri.encodeQueryComponent(root)}&depth=2');
    final entries = (r is Map ? r['entries'] : null) as List? ?? const [];
    final rootStr = (r is Map ? r['root']?.toString() : null) ?? root;
    final out = <MentionSuggestion>[];
    for (final e in entries) {
      if (e is! Map) continue;
      final path = e['path']?.toString() ?? '';
      final isDir = e['is_dir'] == true;
      var rel = path.startsWith(rootStr) ? path.substring(rootStr.length) : path;
      if (rel.startsWith('/')) rel = rel.substring(1);
      if (rel.isEmpty) continue;
      out.add(MentionSuggestion(rel, isDir ? 'folder' : 'file', null));
    }
    return out;
  } catch (_) {
    return const [];
  }
});

/// A chat input that pops a suggestion list when the caret follows a `/`
/// (skills/commands) or `@` (project files & folders) trigger token. Tab/Enter
/// or click inserts; ↑/↓ navigate; Esc dismisses. Falls through to [onSend]
/// when no popup is open.
class SlashMentionField extends ConsumerStatefulWidget {
  const SlashMentionField({
    super.key,
    required this.controller,
    required this.onSend,
    required this.decoration,
    this.style,
  });
  final TextEditingController controller;
  final VoidCallback onSend;
  final InputDecoration decoration;
  final TextStyle? style;

  @override
  ConsumerState<SlashMentionField> createState() => _SlashMentionFieldState();
}

class _SlashMentionFieldState extends ConsumerState<SlashMentionField> {
  String? _trigger; // '/' or '@'
  String _query = '';
  int _active = 0;

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
    final text = widget.controller.text;
    final sel = widget.controller.selection;
    // Only trigger from the text up to the caret.
    final upto = (sel.baseOffset >= 0 && sel.baseOffset <= text.length)
        ? text.substring(0, sel.baseOffset)
        : text;
    final m = RegExp(r'(?:^|\s)([/@])([^\s]*)$').firstMatch(upto);
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
    if (_trigger == '/') {
      source = (ref.watch(skillsProvider).valueOrNull ?? [])
          .map((s) => MentionSuggestion(s.name, 'skill', s.description))
          .toList();
    } else {
      source = ref.watch(mentionFilesProvider).valueOrNull ?? const [];
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
    final replacedBefore =
        before.replaceFirst(RegExp(r'([/@])[^\s]*$'), '$_trigger${s.insert} ');
    final newText = replacedBefore + after;
    ctrl.value = TextEditingValue(
      text: newText,
      selection: TextSelection.collapsed(offset: replacedBefore.length),
    );
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
            minLines: 1,
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
