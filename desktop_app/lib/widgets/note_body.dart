import 'package:flutter/material.dart';
import '../theme/tokens.dart';
import 'app_markdown.dart';

/// Renders a note body the way Google Keep does: GFM task-list lines
/// (`- [ ] …` / `- [x] …`) become **interactive** checkbox rows — tap to
/// toggle, done items strike through and sink into a collapsible "completed"
/// section — while everything else (prose, images, the capture caption,
/// headings, tables) still renders through [AppMarkdown].
///
/// The body stays plain Markdown on disk, so notes remain compatible with the
/// web UI, AI-generated notes and the screenshot-capture flow. Toggling a box
/// just flips `[ ]`↔`[x]` on that source line and reports the new body via
/// [onChanged]; the widget updates optimistically so the tap feels instant.
class NoteBody extends StatefulWidget {
  const NoteBody(
    this.body, {
    super.key,
    this.style,
    this.onChanged,
    this.showAddItem = false,
  });

  /// The raw Markdown body.
  final String body;

  /// Base text style for rendered prose / checklist labels.
  final TextStyle? style;

  /// Called with the rewritten body whenever a checkbox is toggled or an item
  /// is added. When null, checkboxes render read-only (no interaction).
  final ValueChanged<String>? onChanged;

  /// Show a "+ List item" affordance at the end of each checklist (view mode).
  final bool showAddItem;

  /// True when [body] contains at least one GFM task-list line.
  static bool hasChecklist(String body) =>
      _taskLine.hasMatch(body);

  @override
  State<NoteBody> createState() => _NoteBodyState();
}

/// A single source line that is a GFM task-list item.
class _Task {
  _Task({
    required this.lineIndex,
    required this.depth,
    required this.checked,
    required this.label,
  });

  /// Index into the source body's lines — the anchor for toggling.
  final int lineIndex;

  /// Nesting depth (every 2 leading spaces / 1 tab = one level).
  final int depth;
  final bool checked;
  final String label;
}

sealed class _Seg {
  const _Seg();
}

class _MdSeg extends _Seg {
  const _MdSeg(this.text);
  final String text;
}

class _ListSeg extends _Seg {
  const _ListSeg(this.tasks, this.anchor);
  final List<_Task> tasks;

  /// Stable identity (first task's source line) so the collapse state and the
  /// add-item field survive rebuilds after a toggle.
  final int anchor;
}

// `- [ ] text`, `* [x] text`, `+ [X] text`, with optional leading indentation.
final RegExp _taskLine =
    RegExp(r'^([ \t]*)([-*+])[ \t]+\[([ xX])\][ \t]?(.*)$', multiLine: true);

// Same shape, but split so the state char can be swapped in place.
final RegExp _taskToggle = RegExp(r'^([ \t]*[-*+][ \t]+\[)([ xX])(\].*)$');

class _NoteBodyState extends State<NoteBody> {
  late String _body = widget.body;

  @override
  void didUpdateWidget(NoteBody old) {
    super.didUpdateWidget(old);
    // Resync when the note is replaced or edited elsewhere (e.g. after the
    // provider refetches). An echo of our own optimistic write is a no-op.
    if (widget.body != old.body && widget.body != _body) {
      _body = widget.body;
    }
  }

  List<_Seg> _parse(String body) {
    final lines = body.split('\n');
    final segs = <_Seg>[];
    final md = StringBuffer();
    var tasks = <_Task>[];

    void flushMd() {
      if (md.isNotEmpty) {
        segs.add(_MdSeg(md.toString()));
        md.clear();
      }
    }

    void flushList() {
      if (tasks.isNotEmpty) {
        segs.add(_ListSeg(tasks, tasks.first.lineIndex));
        tasks = [];
      }
    }

    final re = RegExp(r'^([ \t]*)([-*+])[ \t]+\[([ xX])\][ \t]?(.*)$');
    for (var i = 0; i < lines.length; i++) {
      final m = re.firstMatch(lines[i]);
      if (m != null) {
        flushMd();
        final indent = m.group(1)!.replaceAll('\t', '  ');
        tasks.add(_Task(
          lineIndex: i,
          depth: (indent.length ~/ 2).clamp(0, 6),
          checked: m.group(3)!.toLowerCase() == 'x',
          label: m.group(4)!.trim(),
        ));
      } else {
        flushList();
        if (md.isNotEmpty) md.write('\n');
        md.write(lines[i]);
      }
    }
    flushMd();
    flushList();
    return segs;
  }

  void _apply(String next) {
    setState(() => _body = next);
    widget.onChanged?.call(next);
  }

  void _toggle(int lineIndex) {
    final lines = _body.split('\n');
    if (lineIndex < 0 || lineIndex >= lines.length) return;
    final m = _taskToggle.firstMatch(lines[lineIndex]);
    if (m == null) return;
    final now = m.group(2)!.toLowerCase() == 'x' ? ' ' : 'x';
    lines[lineIndex] = '${m.group(1)}$now${m.group(3)}';
    _apply(lines.join('\n'));
  }

  /// Append a new unchecked item to the checklist block that ends at
  /// [afterLineIndex], matching that block's marker + indentation.
  void _addItem(int afterLineIndex, String text) {
    final trimmed = text.trim();
    if (trimmed.isEmpty) return;
    final lines = _body.split('\n');
    if (afterLineIndex < 0 || afterLineIndex >= lines.length) return;
    final m = RegExp(r'^([ \t]*)([-*+])[ \t]').firstMatch(lines[afterLineIndex]);
    final indent = m?.group(1) ?? '';
    final marker = m?.group(2) ?? '-';
    lines.insert(afterLineIndex + 1, '$indent$marker [ ] $trimmed');
    _apply(lines.join('\n'));
  }

  @override
  Widget build(BuildContext context) {
    final segs = _parse(_body);
    final interactive = widget.onChanged != null;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      mainAxisSize: MainAxisSize.min,
      children: [
        for (final seg in segs)
          if (seg is _MdSeg)
            AppMarkdown(seg.text, style: widget.style)
          else if (seg is _ListSeg)
            _Checklist(
              key: ValueKey('cl-${seg.anchor}'),
              tasks: seg.tasks,
              style: widget.style,
              onToggle: interactive ? _toggle : null,
              onAdd: interactive && widget.showAddItem
                  ? (text) => _addItem(seg.tasks.last.lineIndex, text)
                  : null,
            ),
      ],
    );
  }
}

/// One contiguous run of task items. Active items on top; completed ones live
/// under a tappable "✓ N completed" divider (collapsed by default, like Keep).
class _Checklist extends StatefulWidget {
  const _Checklist({
    super.key,
    required this.tasks,
    required this.style,
    required this.onToggle,
    required this.onAdd,
  });

  final List<_Task> tasks;
  final TextStyle? style;
  final ValueChanged<int>? onToggle;
  final ValueChanged<String>? onAdd;

  @override
  State<_Checklist> createState() => _ChecklistState();
}

class _ChecklistState extends State<_Checklist> {
  bool _showDone = false;
  bool _adding = false;
  final _addCtrl = TextEditingController();
  final _addFocus = FocusNode();

  @override
  void dispose() {
    _addCtrl.dispose();
    _addFocus.dispose();
    super.dispose();
  }

  void _submitAdd() {
    final text = _addCtrl.text;
    _addCtrl.clear();
    widget.onAdd?.call(text);
    if (text.trim().isEmpty) {
      setState(() => _adding = false);
    } else {
      // Keep the field open for rapid multi-item entry (Keep behaviour).
      _addFocus.requestFocus();
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final active = widget.tasks.where((t) => !t.checked).toList();
    final done = widget.tasks.where((t) => t.checked).toList();

    return Padding(
      padding: const EdgeInsets.symmetric(vertical: AppTokens.s4),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        mainAxisSize: MainAxisSize.min,
        children: [
          for (final t in active) _row(context, t),
          if (widget.onAdd != null) _addRow(context),
          if (done.isNotEmpty) ...[
            const SizedBox(height: AppTokens.s4),
            InkWell(
              onTap: () => setState(() => _showDone = !_showDone),
              borderRadius: BorderRadius.circular(AppTokens.rSm),
              child: Padding(
                padding: const EdgeInsets.symmetric(
                    horizontal: AppTokens.s4, vertical: AppTokens.s6),
                child: Row(
                  children: [
                    Icon(_showDone ? Icons.expand_more : Icons.chevron_right,
                        size: 18, color: c.textMuted),
                    const SizedBox(width: AppTokens.s4),
                    Icon(Icons.check, size: 14, color: c.textMuted),
                    const SizedBox(width: AppTokens.s6),
                    Text('${done.length} completed',
                        style: TextStyle(
                            color: c.textMuted,
                            fontSize: 12.5,
                            fontWeight: FontWeight.w500)),
                  ],
                ),
              ),
            ),
            if (_showDone)
              for (final t in done) _row(context, t),
          ],
        ],
      ),
    );
  }

  Widget _row(BuildContext context, _Task t) {
    final c = context.colors;
    final base = widget.style ?? TextStyle(color: c.textSecondary);
    final enabled = widget.onToggle != null;
    return Padding(
      padding: EdgeInsets.only(left: t.depth * 22.0),
      child: InkWell(
        onTap: enabled ? () => widget.onToggle!(t.lineIndex) : null,
        borderRadius: BorderRadius.circular(AppTokens.rSm),
        child: Padding(
          padding: const EdgeInsets.symmetric(vertical: 1),
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              SizedBox(
                width: 34,
                height: 32,
                child: Checkbox(
                  value: t.checked,
                  onChanged:
                      enabled ? (_) => widget.onToggle!(t.lineIndex) : null,
                  visualDensity: VisualDensity.compact,
                  materialTapTargetSize: MaterialTapTargetSize.shrinkWrap,
                  side: BorderSide(color: c.textMuted, width: 1.5),
                  activeColor: c.accent,
                  shape: RoundedRectangleBorder(
                      borderRadius: BorderRadius.circular(3)),
                ),
              ),
              const SizedBox(width: AppTokens.s4),
              Expanded(
                child: Padding(
                  padding: const EdgeInsets.only(top: 6, bottom: 6),
                  child: Text(
                    t.label.isEmpty ? ' ' : t.label,
                    style: base.copyWith(
                      color: t.checked ? c.textMuted : base.color,
                      decoration:
                          t.checked ? TextDecoration.lineThrough : null,
                      decorationColor: c.textMuted,
                      height: 1.4,
                    ),
                  ),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }

  Widget _addRow(BuildContext context) {
    final c = context.colors;
    if (!_adding) {
      return InkWell(
        onTap: () {
          setState(() => _adding = true);
          _addFocus.requestFocus();
        },
        borderRadius: BorderRadius.circular(AppTokens.rSm),
        child: Padding(
          padding: const EdgeInsets.symmetric(vertical: 6),
          child: Row(
            children: [
              SizedBox(
                width: 34,
                child: Icon(Icons.add, size: 18, color: c.textMuted),
              ),
              const SizedBox(width: AppTokens.s4),
              Text('List item',
                  style: TextStyle(color: c.textMuted, fontSize: 13.5)),
            ],
          ),
        ),
      );
    }
    return Row(
      crossAxisAlignment: CrossAxisAlignment.center,
      children: [
        SizedBox(
          width: 34,
          child: Icon(Icons.add, size: 18, color: c.textMuted),
        ),
        const SizedBox(width: AppTokens.s4),
        Expanded(
          child: TextField(
            controller: _addCtrl,
            focusNode: _addFocus,
            autofocus: true,
            style: TextStyle(color: c.textPrimary, fontSize: 13.5),
            decoration: const InputDecoration(
              isDense: true,
              hintText: 'List item',
              border: InputBorder.none,
              contentPadding: EdgeInsets.symmetric(vertical: 6),
            ),
            textInputAction: TextInputAction.done,
            onSubmitted: (_) => _submitAdd(),
            onTapOutside: (_) {
              if (_addCtrl.text.trim().isEmpty && _adding) {
                setState(() => _adding = false);
              }
            },
          ),
        ),
      ],
    );
  }
}
