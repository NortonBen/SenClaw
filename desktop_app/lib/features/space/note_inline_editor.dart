import 'dart:async';

import 'package:appflowy_editor/appflowy_editor.dart';
import 'package:flutter/material.dart';

import '../../models/space_models.dart';
import '../../theme/tokens.dart';
import 'note_markdown.dart';
import 'note_tags.dart';

/// Inline, WYSIWYG note editor (AppFlowy block editor) — edit the note directly
/// in the reading pane, no dialog. Title, tags and body are all editable here
/// and autosaved together.
///
/// Notes stay **Markdown on disk**: the body is loaded via [markdownToDocument]
/// and written back via [documentToMarkdown] (`lineBreak: '\n'` keeps blocks
/// like an image and its caption on separate lines). The round-trip is slightly
/// lossy (e.g. `-` bullets become `*`, image alt text is dropped), so we only
/// persist when the produced Markdown/title/tags actually differ from the last
/// saved value — otherwise merely opening a note would rewrite it.
///
/// Widget is keyed by note id upstream, so switching notes rebuilds the whole
/// state cleanly and edits never bleed across notes.
class NoteInlineEditor extends StatefulWidget {
  const NoteInlineEditor({
    super.key,
    required this.note,
    required this.onSave,
    this.onTagTap,
  });

  final SpaceNote note;

  /// Debounced autosave sink: `(title, bodyMarkdown, tags)`.
  final void Function(String title, String body, List<String> tags) onSave;

  /// Tapping a tag chip (filters the sidebar list).
  final ValueChanged<String>? onTagTap;

  @override
  State<NoteInlineEditor> createState() => _NoteInlineEditorState();
}

class _NoteInlineEditorState extends State<NoteInlineEditor> {
  late final EditorState _editorState;
  late final TextEditingController _title =
      TextEditingController(text: widget.note.title);
  final TextEditingController _tagInput = TextEditingController();
  late List<String> _tags = normaliseTags(widget.note.tags);

  Timer? _debounce;
  StreamSubscription? _sub;

  // Last values we handed to onSave — the guard against reformatting-on-view.
  late String _lastMd;
  late String _lastTitle = widget.note.title.trim();
  late List<String> _lastTags = List.of(_tags);

  @override
  void initState() {
    super.initState();
    final doc = markdownToDocument(widget.note.body);
    _editorState = doc.root.children.isEmpty
        ? EditorState.blank()
        : EditorState(document: doc);
    // Baseline is the *normalised* encoding, so simply opening a note (whose
    // stored body may differ cosmetically) doesn't count as an edit.
    _lastMd = encodeNoteMarkdown(_editorState.document);
    _sub = _editorState.transactionStream.listen((_) => _scheduleSave());
  }

  @override
  void dispose() {
    // Flush a pending edit before we tear down (e.g. switching notes fast).
    if (_debounce?.isActive ?? false) {
      _debounce!.cancel();
      _persist();
    }
    _sub?.cancel();
    _title.dispose();
    _tagInput.dispose();
    _editorState.dispose();
    super.dispose();
  }

  void _scheduleSave() {
    _debounce?.cancel();
    _debounce = Timer(const Duration(milliseconds: 700), _persist);
  }

  /// Compute the current (title, markdown, tags); persist only if something
  /// changed vs. the last save. `#hashtags` in the body are folded into tags.
  void _persist() {
    final md = encodeNoteMarkdown(_editorState.document);
    final title = _title.text.trim();
    final tags = normaliseTags([..._tags, ...extractBodyTags(md)]);

    final unchanged = md == _lastMd &&
        title == _lastTitle &&
        _sameTags(tags, _lastTags);
    if (unchanged) return;

    _lastMd = md;
    _lastTitle = title;
    _lastTags = List.of(tags);
    // Reflect body-extracted tags back into the chip row.
    if (mounted && !_sameTags(tags, _tags)) {
      setState(() => _tags = tags);
    }
    widget.onSave(title, md, tags);
  }

  static bool _sameTags(List<String> a, List<String> b) {
    if (a.length != b.length) return false;
    for (var i = 0; i < a.length; i++) {
      if (a[i] != b[i]) return false;
    }
    return true;
  }

  void _addTag() {
    final added = normaliseTags(_tagInput.text.split(RegExp(r'[,\s]+')));
    _tagInput.clear();
    if (added.isEmpty) return;
    setState(() => _tags = normaliseTags([..._tags, ...added]));
    _scheduleSave();
  }

  void _removeTag(String t) {
    setState(() => _tags = _tags.where((x) => x != t).toList());
    _scheduleSave();
  }

  // ── Toolbar actions ──────────────────────────────────────────────────────
  void _undo() => _editorState.undoManager.undo();
  void _redo() => _editorState.undoManager.redo();

  /// Toggle an inline attribute (bold / italic) over the current selection.
  void _toggle(String attr) {
    if (_editorState.selection == null) return;
    _editorState.toggleAttribute(attr);
  }

  /// Turn the current block into [type] (todo / bulleted / heading); toggling
  /// the same type again reverts it to a plain paragraph. Text is preserved.
  void _toBlock(String type) {
    final sel = _editorState.selection;
    if (sel == null) return;
    _editorState.formatNode(sel, (node) {
      final delta = node.delta ?? Delta();
      if (node.type == type) return paragraphNode(delta: delta);
      switch (type) {
        case TodoListBlockKeys.type:
          return todoListNode(checked: false, delta: delta);
        case BulletedListBlockKeys.type:
          return bulletedListNode(delta: delta);
        case HeadingBlockKeys.type:
          return headingNode(level: 2, delta: delta);
        default:
          return paragraphNode(delta: delta);
      }
    });
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final editorStyle = EditorStyle.desktop(
      padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 8),
      cursorColor: c.accent,
      selectionColor: c.accentSoft,
      textStyleConfiguration: TextStyleConfiguration(
        text: TextStyle(fontSize: 15.5, color: c.textSecondary, height: 1.6),
        code: TextStyle(
          fontFamily: AppTokens.fontMono,
          fontSize: 13.5,
          color: c.textPrimary,
          backgroundColor: c.surfaceAlt,
        ),
      ),
    );

    return Column(
      children: [
        _toolbar(context),
        Expanded(
          child: AppFlowyEditor(
            editorState: _editorState,
            editorStyle: editorStyle,
            shrinkWrap: false,
            header: _header(context),
            footer: const SizedBox(height: 120),
          ),
        ),
      ],
    );
  }

  /// Always-visible formatting menu for the inline editor.
  Widget _toolbar(BuildContext context) {
    final c = context.colors;
    Widget btn(IconData icon, String tip, VoidCallback onTap) => Tooltip(
          message: tip,
          waitDuration: const Duration(milliseconds: 500),
          child: InkWell(
            onTap: onTap,
            borderRadius: BorderRadius.circular(AppTokens.rSm),
            child: Padding(
              padding: const EdgeInsets.all(7),
              child: Icon(icon, size: 17, color: c.textSecondary),
            ),
          ),
        );
    Widget divider() => Container(
        width: 1,
        height: 18,
        margin: const EdgeInsets.symmetric(horizontal: 4),
        color: c.border);

    return Container(
      decoration: BoxDecoration(
        color: c.sidebar,
        border: Border(bottom: BorderSide(color: c.border)),
      ),
      padding: const EdgeInsets.symmetric(horizontal: AppTokens.s8, vertical: 3),
      child: Row(
        children: [
          btn(Icons.undo, 'Undo (previous)', _undo),
          btn(Icons.redo, 'Redo (next)', _redo),
          divider(),
          btn(Icons.check_box_outlined, 'Checklist',
              () => _toBlock(TodoListBlockKeys.type)),
          btn(Icons.format_list_bulleted, 'Bullet list',
              () => _toBlock(BulletedListBlockKeys.type)),
          btn(Icons.title, 'Heading', () => _toBlock(HeadingBlockKeys.type)),
          divider(),
          btn(Icons.format_bold, 'Bold',
              () => _toggle(AppFlowyRichTextKeys.bold)),
          btn(Icons.format_italic, 'Italic',
              () => _toggle(AppFlowyRichTextKeys.italic)),
        ],
      ),
    );
  }

  Widget _header(BuildContext context) {
    final c = context.colors;
    return Padding(
      padding: const EdgeInsets.fromLTRB(24, 20, 24, 8),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          TextField(
            controller: _title,
            onChanged: (_) => _scheduleSave(),
            style: TextStyle(
              color: c.textPrimary,
              fontSize: 24,
              fontWeight: FontWeight.w700,
              height: 1.25,
            ),
            maxLines: null,
            decoration: InputDecoration(
              isDense: true,
              border: InputBorder.none,
              contentPadding: EdgeInsets.zero,
              hintText: 'Tiêu đề',
              hintStyle: TextStyle(
                  color: c.textMuted, fontSize: 24, fontWeight: FontWeight.w700),
            ),
          ),
          const SizedBox(height: AppTokens.s8),
          _tagRow(context),
          const SizedBox(height: AppTokens.s4),
          Divider(height: 1, color: c.border),
        ],
      ),
    );
  }

  Widget _tagRow(BuildContext context) {
    final c = context.colors;
    return Wrap(
      spacing: 6,
      runSpacing: 6,
      crossAxisAlignment: WrapCrossAlignment.center,
      children: [
        for (final t in _tags)
          Container(
            padding: const EdgeInsets.only(left: AppTokens.s8, right: 2),
            decoration: BoxDecoration(
              color: c.accentSoft,
              borderRadius: BorderRadius.circular(AppTokens.rFull),
            ),
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                InkWell(
                  onTap: widget.onTagTap == null
                      ? null
                      : () => widget.onTagTap!(t),
                  child: Text('#$t',
                      style: TextStyle(
                          color: c.accent,
                          fontSize: 12,
                          fontWeight: FontWeight.w500)),
                ),
                InkWell(
                  onTap: () => _removeTag(t),
                  borderRadius: BorderRadius.circular(AppTokens.rFull),
                  child: Padding(
                    padding: const EdgeInsets.all(2),
                    child: Icon(Icons.close, size: 13, color: c.accent),
                  ),
                ),
              ],
            ),
          ),
        // Inline add-tag field.
        ConstrainedBox(
          constraints: const BoxConstraints(minWidth: 80, maxWidth: 150),
          child: TextField(
            controller: _tagInput,
            style: TextStyle(color: c.textPrimary, fontSize: 12.5),
            decoration: InputDecoration(
              isDense: true,
              border: InputBorder.none,
              contentPadding: EdgeInsets.zero,
              prefixIcon: Icon(Icons.label_outline, size: 15, color: c.textMuted),
              prefixIconConstraints:
                  const BoxConstraints(minWidth: 20, minHeight: 0),
              hintText: _tags.isEmpty ? 'Thêm nhãn…' : 'Thêm…',
              hintStyle: TextStyle(color: c.textMuted, fontSize: 12.5),
            ),
            onChanged: (v) {
              if (v.endsWith(',') || v.endsWith(' ')) _addTag();
            },
            onSubmitted: (_) => _addTag(),
          ),
        ),
      ],
    );
  }
}
