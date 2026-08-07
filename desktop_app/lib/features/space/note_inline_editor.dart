import 'dart:async';

import 'package:appflowy_editor/appflowy_editor.dart';
import 'package:flutter/material.dart';

import '../../core/i18n/l10n.dart';
import '../../models/space_models.dart';
import '../../theme/tokens.dart';
import 'note_editor_blocks.dart';
import 'note_markdown.dart';
import 'note_tags.dart';

/// Inline, WYSIWYG note editor (AppFlowy block editor) — edit the note directly
/// in the reading pane, no dialog. Title, tags and body are all editable here
/// and autosaved together.
///
/// Notes stay **Markdown on disk**: the body is loaded via [parseNoteMarkdown]
/// (which pre-normalises loose lists written by the web UI / AI agents so the
/// decoder doesn't shred them) and written back via [encodeNoteMarkdown]. The
/// round-trip is slightly lossy (e.g. `-` bullets become `*`, image alt text is
/// dropped), so we only persist when the produced Markdown/title/tags actually
/// differ from the last saved value — otherwise merely opening a note would
/// rewrite it.
///
/// Widget is keyed by note id upstream, so switching notes rebuilds the whole
/// state cleanly and edits never bleed across notes.
class NoteInlineEditor extends StatefulWidget {
  const NoteInlineEditor({
    super.key,
    required this.note,
    required this.onSave,
    this.onTagTap,
    this.onPin,
    this.onDelete,
  });

  final SpaceNote note;

  /// Debounced autosave sink: `(title, bodyMarkdown, tags)`.
  final void Function(String title, String body, List<String> tags) onSave;

  /// Tapping a tag chip (filters the sidebar list).
  final ValueChanged<String>? onTagTap;

  /// Note actions surfaced on the right of the toolbar (hidden when null).
  final VoidCallback? onPin;
  final VoidCallback? onDelete;

  @override
  State<NoteInlineEditor> createState() => _NoteInlineEditorState();
}

enum _SaveStatus { clean, dirty, saved }

class _NoteInlineEditorState extends State<NoteInlineEditor> {
  late final EditorState _editorState;
  late final EditorScrollController _scrollController;
  late final TextEditingController _title =
      TextEditingController(text: widget.note.title);
  final TextEditingController _tagInput = TextEditingController();
  late List<String> _tags = normaliseTags(widget.note.tags);

  Timer? _debounce;
  StreamSubscription? _sub;
  final ValueNotifier<_SaveStatus> _saveStatus =
      ValueNotifier(_SaveStatus.clean);

  // Last values we handed to onSave — the guard against reformatting-on-view.
  late String _lastMd;
  late String _lastTitle = widget.note.title.trim();
  late List<String> _lastTags = List.of(_tags);

  @override
  void initState() {
    super.initState();
    final doc = parseNoteMarkdown(widget.note.body);
    _editorState = doc.root.children.isEmpty
        ? EditorState.blank()
        : EditorState(document: doc);
    _scrollController = EditorScrollController(editorState: _editorState);
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
      _persist(flushing: true);
    }
    _sub?.cancel();
    _saveStatus.dispose();
    _title.dispose();
    _tagInput.dispose();
    _scrollController.dispose();
    _editorState.dispose();
    super.dispose();
  }

  void _scheduleSave() {
    _saveStatus.value = _SaveStatus.dirty;
    _debounce?.cancel();
    _debounce = Timer(const Duration(milliseconds: 700), _persist);
  }

  /// Compute the current (title, markdown, tags); persist only if something
  /// changed vs. the last save. `#hashtags` in the body are folded into tags.
  /// [flushing] marks the final flush from dispose, where setState is illegal.
  void _persist({bool flushing = false}) {
    final md = encodeNoteMarkdown(_editorState.document);
    final title = _title.text.trim();
    final tags = normaliseTags([..._tags, ...extractBodyTags(md)]);

    final unchanged = md == _lastMd &&
        title == _lastTitle &&
        _sameTags(tags, _lastTags);
    if (unchanged) {
      _saveStatus.value = _SaveStatus.clean;
      return;
    }

    _lastMd = md;
    _lastTitle = title;
    _lastTags = List.of(tags);
    // Reflect body-extracted tags back into the chip row.
    if (!flushing && mounted && !_sameTags(tags, _tags)) {
      setState(() => _tags = tags);
    }
    widget.onSave(title, md, tags);
    _saveStatus.value = _SaveStatus.saved;
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

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    // AppFlowy's RichText doesn't read the theme's DefaultTextStyle, so the
    // app's UI font (family + fallback stack) must be threaded in explicitly
    // or note text renders in the raw platform default.
    final ambient = Theme.of(context).textTheme.bodyMedium;
    TextStyle themed(TextStyle s) => s.copyWith(
          fontFamily: ambient?.fontFamily,
          fontFamilyFallback: ambient?.fontFamilyFallback,
        );
    final editorStyle = EditorStyle.desktop(
      padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 8),
      cursorColor: c.accent,
      selectionColor: c.accentSoft,
      textStyleConfiguration: TextStyleConfiguration(
        text: themed(
            TextStyle(fontSize: 15.5, color: c.textPrimary, height: 1.6)),
        code: TextStyle(
          fontFamily: AppTokens.fontMono,
          fontSize: 13.5,
          color: c.textPrimary,
          backgroundColor: c.surfaceAlt,
        ),
        href: themed(TextStyle(
          color: c.accent,
          decoration: TextDecoration.underline,
          decorationColor: c.accent,
        )),
      ),
    );

    return Column(
      children: [
        _NoteToolbar(
          editorState: _editorState,
          saveStatus: _saveStatus,
          pinned: widget.note.pinned,
          onPin: widget.onPin,
          onDelete: widget.onDelete,
        ),
        Expanded(
          child: FloatingToolbar(
            items: [
              paragraphItem,
              ...headingItems,
              ...markdownFormatItems,
              quoteItem,
              bulletedListItem,
              numberedListItem,
              linkItem,
            ],
            editorState: _editorState,
            editorScrollController: _scrollController,
            textDirection: TextDirection.ltr,
            style: const FloatingToolbarStyle(
              backgroundColor: Color(0xFF23272F),
              toolbarIconColor: Color(0xFFE8EAED),
              toolbarActiveColor: AppTokens.brand,
              toolbarShadowColor: Colors.black26,
              toolbarElevation: 6,
            ),
            tooltipBuilder: (context, id, message, child) => Tooltip(
              message: _toolbarTooltips[id] == null
                  ? message
                  : context.tr(_toolbarTooltips[id]!),
              waitDuration: const Duration(milliseconds: 400),
              child: child,
            ),
            child: AppFlowyEditor(
              editorState: _editorState,
              editorScrollController: _scrollController,
              editorStyle: editorStyle,
              blockComponentBuilders: noteBlockBuilders(c),
              shrinkWrap: false,
              header: _header(context),
              footer: _footer(),
            ),
          ),
        ),
      ],
    );
  }

  /// Our own tooltips for the floating (selection) toolbar's built-ins, keyed
  /// by AppFlowy item id. Values are English keys translated at the call site.
  static const Map<String, String> _toolbarTooltips = {
    'editor.paragraph': 'Plain text',
    'editor.h1': 'Heading 1',
    'editor.h2': 'Heading 2',
    'editor.h3': 'Heading 3',
    'editor.bold': 'Bold (⌘B)',
    'editor.italic': 'Italic (⌘I)',
    'editor.underline': 'Underline (⌘U)',
    'editor.strikethrough': 'Strikethrough',
    'editor.code': 'Inline code (⌘E)',
    'editor.quote': 'Quote',
    'editor.bulleted_list': 'Bulleted list',
    'editor.numbered_list': 'Numbered list',
    'editor.link': 'Insert link (⌘K)',
  };

  /// Clicking the empty space under the note appends/focuses a trailing
  /// paragraph — so the whole pane feels editable, not just existing lines.
  Widget _footer() => GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: _focusTrailingParagraph,
        child: const SizedBox(height: 160, width: double.infinity),
      );

  void _focusTrailingParagraph() {
    final root = _editorState.document.root;
    final last = root.children.isEmpty ? null : root.children.last;
    if (last != null &&
        last.type == ParagraphBlockKeys.type &&
        (last.delta?.isEmpty ?? true)) {
      _editorState.selection = Selection.collapsed(Position(path: last.path));
      return;
    }
    final path = [root.children.length];
    final tx = _editorState.transaction
      ..insertNode(path, paragraphNode())
      ..afterSelection = Selection.collapsed(Position(path: path));
    _editorState.apply(tx);
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
              fontSize: 26,
              fontWeight: FontWeight.w700,
              height: 1.25,
            ),
            maxLines: null,
            // Explicit none-borders: the app's InputDecorationTheme outlines
            // every field otherwise (the boxy title of the old editor).
            decoration: InputDecoration(
              isDense: true,
              filled: false,
              border: InputBorder.none,
              enabledBorder: InputBorder.none,
              focusedBorder: InputBorder.none,
              contentPadding: EdgeInsets.zero,
              hintText: context.tr('Title'),
              hintStyle: TextStyle(
                  color: c.textMuted, fontSize: 26, fontWeight: FontWeight.w700),
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
              filled: false,
              border: InputBorder.none,
              enabledBorder: InputBorder.none,
              focusedBorder: InputBorder.none,
              contentPadding: EdgeInsets.zero,
              prefixIcon: Icon(Icons.label_outline, size: 15, color: c.textMuted),
              prefixIconConstraints:
                  const BoxConstraints(minWidth: 20, minHeight: 0),
              hintText: _tags.isEmpty
                  ? context.tr('Add label…')
                  : context.tr('Add…'),
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

/// Fixed formatting toolbar with live active-state: buttons highlight to show
/// the block type / inline formats at the caret. Rebuilds itself on selection
/// and document changes — the editor widget itself is untouched.
class _NoteToolbar extends StatefulWidget {
  const _NoteToolbar({
    required this.editorState,
    required this.saveStatus,
    this.pinned = false,
    this.onPin,
    this.onDelete,
  });

  final EditorState editorState;
  final ValueNotifier<_SaveStatus> saveStatus;
  final bool pinned;
  final VoidCallback? onPin;
  final VoidCallback? onDelete;

  @override
  State<_NoteToolbar> createState() => _NoteToolbarState();
}

class _NoteToolbarState extends State<_NoteToolbar> {
  StreamSubscription? _txSub;

  EditorState get editorState => widget.editorState;

  @override
  void initState() {
    super.initState();
    editorState.selectionNotifier.addListener(_refresh);
    _txSub = editorState.transactionStream.listen((_) => _refresh());
  }

  @override
  void dispose() {
    editorState.selectionNotifier.removeListener(_refresh);
    _txSub?.cancel();
    super.dispose();
  }

  void _refresh() {
    if (mounted) setState(() {});
  }

  // ── Queries ──────────────────────────────────────────────────────────────

  Node? get _startNode {
    final sel = editorState.selection;
    if (sel == null) return null;
    return editorState.getNodeAtPath(sel.normalized.start.path);
  }

  bool _blockActive(String type, {int? level}) {
    final node = _startNode;
    if (node == null || node.type != type) return false;
    if (level == null) return true;
    return node.attributes[HeadingBlockKeys.level] == level;
  }

  /// Is inline attribute [name] active at the caret / over the selection?
  bool _inlineActive(String name) {
    final sel = editorState.selection;
    if (sel == null) return false;
    if (!sel.isCollapsed) {
      final nodes = editorState.getNodesInSelection(sel);
      if (nodes.isEmpty) return false;
      return nodes.allSatisfyInSelection(
        sel,
        (delta) =>
            delta.isNotEmpty &&
            delta.everyAttributes((attr) => attr[name] == true),
      );
    }
    // Collapsed caret: a just-toggled style wins, else the char before it.
    final toggled = editorState.toggledStyle[name];
    if (toggled is bool) return toggled;
    final delta = _startNode?.delta;
    final offset = sel.start.offset;
    if (delta == null || offset == 0) return false;
    final slice = delta.slice(offset - 1, offset);
    return slice.isNotEmpty &&
        slice.everyAttributes((attr) => attr[name] == true);
  }

  int get _wordCount {
    var words = 0;
    void walk(Node node) {
      final text = node.delta?.toPlainText();
      if (text != null && text.trim().isNotEmpty) {
        words += text.trim().split(RegExp(r'\s+')).length;
      }
      for (final child in node.children) {
        walk(child);
      }
    }

    walk(editorState.document.root);
    return words;
  }

  // ── Actions ──────────────────────────────────────────────────────────────

  void _toggleInline(String attr) {
    if (editorState.selection == null) return;
    editorState.toggleAttribute(attr);
  }

  /// Turn the current block into [type] (with optional heading [level]);
  /// invoking on an already-matching block reverts it to a plain paragraph.
  void _toBlock(String type, {int? level}) {
    final sel = editorState.selection;
    if (sel == null) return;
    editorState.formatNode(sel, (node) {
      final delta = node.delta ?? Delta();
      final same = node.type == type &&
          (type != HeadingBlockKeys.type ||
              node.attributes[HeadingBlockKeys.level] == level);
      if (same) return paragraphNode(delta: delta);
      return switch (type) {
        TodoListBlockKeys.type => todoListNode(checked: false, delta: delta),
        BulletedListBlockKeys.type => bulletedListNode(delta: delta),
        NumberedListBlockKeys.type => numberedListNode(delta: delta),
        QuoteBlockKeys.type => quoteNode(delta: delta),
        HeadingBlockKeys.type => headingNode(level: level ?? 2, delta: delta),
        _ => paragraphNode(delta: delta),
      };
    });
  }

  /// Insert a horizontal rule below the current block, caret on a fresh
  /// paragraph after it.
  void _insertDivider() {
    final sel = editorState.selection;
    if (sel == null) return;
    final path = sel.normalized.end.path;
    final tx = editorState.transaction
      ..insertNode(path.next, dividerNode())
      ..insertNode(path.next.next, paragraphNode())
      ..afterSelection = Selection.collapsed(Position(path: path.next.next));
    editorState.apply(tx);
  }

  // ── UI ───────────────────────────────────────────────────────────────────

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    Widget divider() => Container(
        width: 1,
        height: 18,
        margin: const EdgeInsets.symmetric(horizontal: 5),
        color: c.border);

    return Container(
      decoration: BoxDecoration(
        color: c.sidebar,
        border: Border(bottom: BorderSide(color: c.border)),
      ),
      padding: const EdgeInsets.symmetric(horizontal: AppTokens.s8, vertical: 4),
      child: Row(
        children: [
          _TbButton(
            icon: Icons.undo,
            tip: context.tr('Undo (⌘Z)'),
            onTap: () => editorState.undoManager.undo(),
          ),
          _TbButton(
            icon: Icons.redo,
            tip: context.tr('Redo (⇧⌘Z)'),
            onTap: () => editorState.undoManager.redo(),
          ),
          divider(),
          for (final level in [1, 2, 3])
            _TbButton(
              label: 'H$level',
              tip: context.trArgs('Heading {n}', {'n': level}),
              active: _blockActive(HeadingBlockKeys.type, level: level),
              onTap: () => _toBlock(HeadingBlockKeys.type, level: level),
            ),
          divider(),
          _TbButton(
            icon: Icons.check_box_outlined,
            tip: context.tr('To-do'),
            active: _blockActive(TodoListBlockKeys.type),
            onTap: () => _toBlock(TodoListBlockKeys.type),
          ),
          _TbButton(
            icon: Icons.format_list_bulleted,
            tip: context.tr('Bulleted list'),
            active: _blockActive(BulletedListBlockKeys.type),
            onTap: () => _toBlock(BulletedListBlockKeys.type),
          ),
          _TbButton(
            icon: Icons.format_list_numbered,
            tip: context.tr('Numbered list'),
            active: _blockActive(NumberedListBlockKeys.type),
            onTap: () => _toBlock(NumberedListBlockKeys.type),
          ),
          _TbButton(
            icon: Icons.format_quote,
            tip: context.tr('Quote'),
            active: _blockActive(QuoteBlockKeys.type),
            onTap: () => _toBlock(QuoteBlockKeys.type),
          ),
          _TbButton(
            icon: Icons.horizontal_rule,
            tip: context.tr('Divider'),
            onTap: _insertDivider,
          ),
          divider(),
          _TbButton(
            icon: Icons.format_bold,
            tip: context.tr('Bold (⌘B)'),
            active: _inlineActive(AppFlowyRichTextKeys.bold),
            onTap: () => _toggleInline(AppFlowyRichTextKeys.bold),
          ),
          _TbButton(
            icon: Icons.format_italic,
            tip: context.tr('Italic (⌘I)'),
            active: _inlineActive(AppFlowyRichTextKeys.italic),
            onTap: () => _toggleInline(AppFlowyRichTextKeys.italic),
          ),
          _TbButton(
            icon: Icons.format_underline,
            tip: context.tr('Underline (⌘U)'),
            active: _inlineActive(AppFlowyRichTextKeys.underline),
            onTap: () => _toggleInline(AppFlowyRichTextKeys.underline),
          ),
          _TbButton(
            icon: Icons.strikethrough_s,
            tip: context.tr('Strikethrough'),
            active: _inlineActive(AppFlowyRichTextKeys.strikethrough),
            onTap: () => _toggleInline(AppFlowyRichTextKeys.strikethrough),
          ),
          _TbButton(
            icon: Icons.code,
            tip: context.tr('Inline code (⌘E)'),
            active: _inlineActive(AppFlowyRichTextKeys.code),
            onTap: () => _toggleInline(AppFlowyRichTextKeys.code),
          ),
          const Spacer(),
          _StatusChip(saveStatus: widget.saveStatus, wordCount: _wordCount),
          if (widget.onPin != null || widget.onDelete != null) divider(),
          if (widget.onPin != null)
            _TbButton(
              icon: widget.pinned ? Icons.push_pin : Icons.push_pin_outlined,
              tip: widget.pinned
                  ? context.tr('Unpin')
                  : context.tr('Pin note'),
              active: widget.pinned,
              onTap: widget.onPin!,
            ),
          if (widget.onDelete != null)
            _TbButton(
              icon: Icons.delete_outline,
              tip: context.tr('Delete note'),
              color: AppTokens.danger,
              onTap: widget.onDelete!,
            ),
        ],
      ),
    );
  }
}

/// One toolbar button: icon or short text label, hover feedback, accent
/// highlight when [active].
class _TbButton extends StatelessWidget {
  const _TbButton({
    this.icon,
    this.label,
    required this.tip,
    required this.onTap,
    this.active = false,
    this.color,
  }) : assert(icon != null || label != null);

  final IconData? icon;
  final String? label;
  final String tip;
  final VoidCallback onTap;
  final bool active;

  /// Overrides the idle foreground color (e.g. danger red for delete).
  final Color? color;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final color = active ? c.accent : (this.color ?? c.textSecondary);
    return Tooltip(
      message: tip,
      waitDuration: const Duration(milliseconds: 500),
      child: Material(
        color: active ? c.accentSoft : Colors.transparent,
        borderRadius: BorderRadius.circular(AppTokens.rSm),
        child: InkWell(
          onTap: onTap,
          borderRadius: BorderRadius.circular(AppTokens.rSm),
          child: Container(
            constraints: const BoxConstraints(minWidth: 30, minHeight: 30),
            padding: const EdgeInsets.symmetric(horizontal: 5),
            alignment: Alignment.center,
            child: icon != null
                ? Icon(icon, size: 17, color: color)
                : Text(label!,
                    style: TextStyle(
                        fontSize: 12.5,
                        fontWeight: FontWeight.w700,
                        color: color)),
          ),
        ),
      ),
    );
  }
}

/// Right side of the toolbar: word count + autosave state.
class _StatusChip extends StatelessWidget {
  const _StatusChip({required this.saveStatus, required this.wordCount});

  final ValueNotifier<_SaveStatus> saveStatus;
  final int wordCount;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return ValueListenableBuilder<_SaveStatus>(
      valueListenable: saveStatus,
      builder: (context, status, _) {
        final (label, color) = switch (status) {
          _SaveStatus.dirty => (context.tr('Saving…'), c.textMuted),
          _SaveStatus.saved => (context.tr('Saved'), AppTokens.success),
          _SaveStatus.clean => (null, c.textMuted),
        };
        return Padding(
          padding: const EdgeInsets.only(right: AppTokens.s4),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              if (label != null) ...[
                if (status == _SaveStatus.saved)
                  Padding(
                    padding: const EdgeInsets.only(right: 3),
                    child: Icon(Icons.check_circle,
                        size: 12, color: AppTokens.success),
                  ),
                Text(label, style: TextStyle(fontSize: 11.5, color: color)),
                Text('  ·  ',
                    style: TextStyle(fontSize: 11.5, color: c.textMuted)),
              ],
              Text(
                  context.trPlural(wordCount, '{n} word', '{n} words'),
                  style: TextStyle(fontSize: 11.5, color: c.textMuted)),
            ],
          ),
        );
      },
    );
  }
}
