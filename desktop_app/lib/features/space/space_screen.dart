import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:intl/intl.dart';
import '../../core/i18n/l10n.dart';
import 'app_health_gate.dart';
import '../../core/transport/connection.dart';
import '../../models/space_models.dart';
import '../../theme/tokens.dart';
import '../../widgets/note_body.dart';
import '../../widgets/embedded_web.dart';
import '../../widgets/schedule_editor.dart';
import '../../widgets/section_scaffold.dart';
import 'event_link.dart';
import 'note_inline_editor.dart';
import 'note_tags.dart';
import 'space_providers.dart';

/// The host theme as the Space-app bridge string ('dark' | 'light'). Passed to
/// [embeddedWebView], which delivers it to the app via postMessage (the app's
/// URL stays stable, so a theme switch doesn't reload the frame).
String _embedTheme(BuildContext context) =>
    Theme.of(context).brightness == Brightness.dark ? 'dark' : 'light';

/// Top-level Notes screen (was the Space → Notes tab). Rail item `/notes`.
class NotesScreen extends StatelessWidget {
  const NotesScreen({super.key});

  @override
  Widget build(BuildContext context) =>
      SectionScaffold(title: context.tr('Notes'), body: const _NotesTab());
}

/// Top-level Calendar screen (was the Space → Calendar tab). Rail item
/// `/calendar`.
class CalendarScreen extends StatelessWidget {
  const CalendarScreen({super.key});

  @override
  Widget build(BuildContext context) =>
      SectionScaffold(title: context.tr('Calendar'), body: const _CalendarTab());
}

/// Schedules manager, surfaced as a Plugins section (was the Space →
/// Schedules tab). Cowork moved to Plugins too, so Space no longer exists.
class SchedulesPanel extends StatelessWidget {
  const SchedulesPanel({super.key});

  @override
  Widget build(BuildContext context) => SectionScaffold(
      title: context.tr('Schedules'), body: const _SchedulesTab());
}

// ── Notes ─────────────────────────────────────────────────────────────────
class _NotesTab extends ConsumerStatefulWidget {
  const _NotesTab();
  @override
  ConsumerState<_NotesTab> createState() => _NotesTabState();
}

class _NotesTabState extends ConsumerState<_NotesTab> {
  String? _selectedId;
  String _query = '';
  String? _tagFilter;

  bool _matches(SpaceNote n) {
    if (_tagFilter != null && !n.tags.contains(_tagFilter)) return false;
    if (_query.isEmpty) return true;
    return n.title.toLowerCase().contains(_query) ||
        n.body.toLowerCase().contains(_query) ||
        n.tags.any((t) => t.toLowerCase().contains(_query));
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final notesAsync = ref.watch(notesProvider);
    final allNotes = notesAsync.valueOrNull ?? const <SpaceNote>[];

    return Row(
      children: [
        SizedBox(
          width: 300,
          child: Container(
            color: c.sidebar,
            child: Column(
              children: [
                Padding(
                  padding: const EdgeInsets.all(AppTokens.s12),
                  child: Row(
                    children: [
                      Expanded(
                        child: TextField(
                          decoration: InputDecoration(
                            hintText: context.tr('Search notes…'),
                            prefixIcon: const Icon(Icons.search, size: 16),
                          ),
                          onChanged: (v) => setState(() => _query = v.toLowerCase()),
                        ),
                      ),
                      const SizedBox(width: AppTokens.s8),
                      IconButton.filled(
                        tooltip: context.tr('New note'),
                        icon: const Icon(Icons.add, size: 18),
                        onPressed: () => _editNote(context, ref, null),
                      ),
                    ],
                  ),
                ),
                _tagFilterBar(allNotes),
                Expanded(
                  child: notesAsync.when(
                    loading: () => const Center(child: CircularProgressIndicator()),
                    error: (e, _) => Center(child: Text('$e')),
                    data: (notes) {
                      final filtered = notes.where(_matches).toList()
                        ..sort((a, b) =>
                            (b.pinned ? 1 : 0).compareTo(a.pinned ? 1 : 0));
                      if (filtered.isEmpty) {
                        return Center(
                          child: Text(
                              _tagFilter != null
                                  ? context.trArgs('No notes tagged #{tag}',
                                      {'tag': _tagFilter})
                                  : context.tr('No notes'),
                              style: TextStyle(color: c.textMuted)),
                        );
                      }
                      return ListView.builder(
                        itemCount: filtered.length,
                        itemBuilder: (_, i) {
                          final n = filtered[i];
                          final sel = n.id == _selectedId;
                          return InkWell(
                            onTap: () => setState(() => _selectedId = n.id),
                            child: Container(
                              padding: const EdgeInsets.symmetric(
                                  horizontal: AppTokens.s12, vertical: AppTokens.s12),
                              margin: const EdgeInsets.symmetric(
                                  horizontal: AppTokens.s8, vertical: 2),
                              decoration: BoxDecoration(
                                color: sel ? c.accentSoft : Colors.transparent,
                                borderRadius: BorderRadius.circular(AppTokens.rMd),
                              ),
                              child: Column(
                                crossAxisAlignment: CrossAxisAlignment.start,
                                children: [
                                  Text(
                                      n.title.isEmpty
                                          ? context.tr('(untitled)')
                                          : n.title,
                                      maxLines: 1,
                                      overflow: TextOverflow.ellipsis,
                                      style: TextStyle(
                                        color: c.textPrimary,
                                        fontWeight: FontWeight.w600,
                                        fontSize: 14,
                                      )),
                                  if (n.tags.isNotEmpty)
                                    Padding(
                                      padding: const EdgeInsets.only(top: 2),
                                      child: Text(n.tags.map((t) => '#$t').join(' '),
                                          maxLines: 1,
                                          overflow: TextOverflow.ellipsis,
                                          style: TextStyle(
                                              color: c.textMuted, fontSize: 12)),
                                    ),
                                ],
                              ),
                            ),
                          );
                        },
                      );
                    },
                  ),
                ),
              ],
            ),
          ),
        ),
        Container(width: 1, color: c.border),
        Expanded(
          child: notesAsync.maybeWhen(
            data: (notes) {
              final note = notes.where((n) => n.id == _selectedId).firstOrNull;
              if (note == null) {
                return Center(
                  child: Text(context.tr('Select a note'),
                      style: TextStyle(color: c.textMuted)),
                );
              }
              return _NoteView(
                note: note,
                onPin: () =>
                    ref.read(spaceApiProvider).setPinned(note.id, !note.pinned),
                onDelete: () async {
                  await ref.read(spaceApiProvider).deleteNote(note.id);
                  setState(() => _selectedId = null);
                },
                onSave: (title, body, tags) => ref
                    .read(spaceApiProvider)
                    .updateNote(note.id, title, body, tags),
                onTagTap: (t) => setState(
                    () => _tagFilter = _tagFilter == t ? null : t),
              );
            },
            orElse: () => const SizedBox.shrink(),
          ),
        ),
      ],
    );
  }

  /// Horizontal, scrollable "All / #tag" chips derived from every note's tags.
  /// Clicking one filters the list; clicking the active one clears it.
  Widget _tagFilterBar(List<SpaceNote> notes) {
    final tags = <String>{};
    for (final n in notes) {
      tags.addAll(n.tags);
    }
    if (tags.isEmpty) return const SizedBox.shrink();
    final sorted = tags.toList()..sort();
    return SizedBox(
      height: 34,
      child: ListView(
        scrollDirection: Axis.horizontal,
        padding: const EdgeInsets.fromLTRB(
            AppTokens.s12, 0, AppTokens.s12, AppTokens.s4),
        children: [
          _FilterChip(
              label: context.tr('All'),
              active: _tagFilter == null,
              onTap: () => setState(() => _tagFilter = null)),
          for (final t in sorted) ...[
            const SizedBox(width: 6),
            _FilterChip(
                label: '#$t',
                active: _tagFilter == t,
                onTap: () =>
                    setState(() => _tagFilter = _tagFilter == t ? null : t)),
          ],
        ],
      ),
    );
  }

  Future<void> _editNote(BuildContext context, WidgetRef ref, SpaceNote? note) =>
      showDialog(context: context, builder: (_) => _NoteEditor(note: note));
}

class _NoteView extends StatelessWidget {
  const _NoteView(
      {required this.note,
      required this.onPin,
      required this.onDelete,
      required this.onSave,
      this.onTagTap});
  final SpaceNote note;
  final VoidCallback onPin;
  final VoidCallback onDelete;

  /// Debounced autosave from the inline editor: `(title, bodyMarkdown, tags)`.
  final void Function(String title, String body, List<String> tags) onSave;

  /// Tapping one of the note's tag chips (filters the list by that tag).
  final ValueChanged<String>? onTagTap;

  @override
  Widget build(BuildContext context) {
    // Pin/delete live inside the editor's toolbar row — no separate action
    // bar, the editor owns the whole pane. Key by note id so switching notes
    // rebuilds the editor state (document + autosave guards) cleanly — edits
    // never bleed across.
    return NoteInlineEditor(
      key: ValueKey(note.id),
      note: note,
      onSave: onSave,
      onTagTap: onTagTap,
      onPin: onPin,
      onDelete: onDelete,
    );
  }
}

class _NoteEditor extends ConsumerStatefulWidget {
  const _NoteEditor({this.note});
  final SpaceNote? note;
  @override
  ConsumerState<_NoteEditor> createState() => _NoteEditorState();
}

class _NoteEditorState extends ConsumerState<_NoteEditor> {
  late final TextEditingController _title =
      TextEditingController(text: widget.note?.title ?? '');
  late final TextEditingController _body =
      TextEditingController(text: widget.note?.body ?? '');
  final TextEditingController _tagInput = TextEditingController();
  late List<String> _tagList = normaliseTags(widget.note?.tags ?? const []);
  bool _preview = false;

  @override
  void dispose() {
    _title.dispose();
    _body.dispose();
    _tagInput.dispose();
    super.dispose();
  }

  /// Commit whatever is typed in the tag input (comma/space separated) as chips.
  void _commitTags() {
    final added = normaliseTags(_tagInput.text.split(RegExp(r'[,\s]+')));
    if (added.isNotEmpty) {
      setState(() => _tagList = normaliseTags([..._tagList, ...added]));
    }
    _tagInput.clear();
  }

  Future<void> _save() async {
    _commitTags(); // fold any half-typed tag into the list first
    // Explicit chips + `#hashtags` written in the body, normalised & deduped.
    final tags = normaliseTags([..._tagList, ...extractBodyTags(_body.text)]);
    final api = ref.read(spaceApiProvider);
    if (widget.note == null) {
      await api.createNote(_title.text, _body.text, tags);
    } else {
      await api.updateNote(widget.note!.id, _title.text, _body.text, tags);
    }
    if (mounted) Navigator.of(context).pop();
  }

  void _setBody(String text, TextSelection selection) {
    _body.value = TextEditingValue(text: text, selection: selection);
    setState(() {}); // keep the live preview / toolbar state in sync
  }

  /// Wrap the current selection (or a `text` placeholder) with [token], e.g.
  /// `**bold**`. Used for bold / italic.
  void _wrap(String token) {
    final text = _body.text;
    final sel = _body.selection;
    final start = sel.isValid ? sel.start : text.length;
    final end = sel.isValid ? sel.end : text.length;
    final selected = text.substring(start, end);
    final inner = selected.isEmpty ? 'text' : selected;
    final next = text.replaceRange(start, end, '$token$inner$token');
    _setBody(
      next,
      TextSelection(
          baseOffset: start + token.length,
          extentOffset: start + token.length + inner.length),
    );
  }

  /// Toggle a line-level [prefix] (`- [ ] `, `- `, `## `) on the caret's line.
  void _linePrefix(String prefix) {
    final text = _body.text;
    final sel = _body.selection;
    final caret = sel.isValid ? sel.start : text.length;
    final lineStart = caret <= 0 ? 0 : text.lastIndexOf('\n', caret - 1) + 1;
    var lineEnd = text.indexOf('\n', lineStart);
    if (lineEnd == -1) lineEnd = text.length;
    final line = text.substring(lineStart, lineEnd);
    final String newLine;
    final int delta;
    if (line.startsWith(prefix)) {
      newLine = line.substring(prefix.length);
      delta = -prefix.length;
    } else {
      newLine = '$prefix$line';
      delta = prefix.length;
    }
    final next = text.replaceRange(lineStart, lineEnd, newLine);
    final newCaret =
        (caret + delta).clamp(lineStart, lineStart + newLine.length);
    _setBody(next, TextSelection.collapsed(offset: newCaret));
  }

  Widget _toolBtn(IconData icon, String tip, VoidCallback onTap) {
    final c = context.colors;
    return Tooltip(
      message: tip,
      waitDuration: const Duration(milliseconds: 500),
      child: InkWell(
        onTap: onTap,
        borderRadius: BorderRadius.circular(AppTokens.rSm),
        child: Padding(
          padding: const EdgeInsets.all(6),
          child: Icon(icon, size: 17, color: c.textSecondary),
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Dialog(
      backgroundColor: c.surface,
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 640, maxHeight: 560),
        child: Padding(
          padding: const EdgeInsets.all(AppTokens.s20),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(
                  widget.note == null
                      ? context.tr('New note')
                      : context.tr('Edit note'),
                  style: TextStyle(
                      color: c.textPrimary,
                      fontSize: 16,
                      fontWeight: FontWeight.w700)),
              const SizedBox(height: AppTokens.s12),
              TextField(
                  controller: _title,
                  decoration:
                      InputDecoration(hintText: context.tr('Title'))),
              const SizedBox(height: AppTokens.s8),
              Align(
                alignment: Alignment.centerRight,
                child: SegmentedButton<bool>(
                  style:
                      const ButtonStyle(visualDensity: VisualDensity.compact),
                  segments: [
                    ButtonSegment(
                        value: false,
                        icon: const Icon(Icons.edit_outlined, size: 14),
                        label: Text(context.tr('Edit'))),
                    ButtonSegment(
                        value: true,
                        icon: const Icon(Icons.visibility_outlined, size: 14),
                        label: Text(context.tr('Preview'))),
                  ],
                  selected: {_preview},
                  onSelectionChanged: (s) =>
                      setState(() => _preview = s.first),
                ),
              ),
              const SizedBox(height: AppTokens.s8),
              // Keep-style quick-format toolbar (edit mode only).
              if (!_preview)
                Container(
                  margin: const EdgeInsets.only(bottom: AppTokens.s8),
                  padding: const EdgeInsets.symmetric(horizontal: 2),
                  decoration: BoxDecoration(
                    color: c.sidebar,
                    borderRadius: BorderRadius.circular(AppTokens.rSm),
                    border: Border.all(color: c.border),
                  ),
                  child: Row(
                    children: [
                      _toolBtn(Icons.check_box_outlined,
                          context.tr('Checklist item'),
                          () => _linePrefix('- [ ] ')),
                      _toolBtn(Icons.format_list_bulleted,
                          context.tr('Bullet list'), () => _linePrefix('- ')),
                      _toolBtn(Icons.title, context.tr('Heading'),
                          () => _linePrefix('## ')),
                      const SizedBox(width: 2),
                      Container(width: 1, height: 18, color: c.border),
                      const SizedBox(width: 2),
                      _toolBtn(Icons.format_bold, context.tr('Bold'),
                          () => _wrap('**')),
                      _toolBtn(Icons.format_italic, context.tr('Italic'),
                          () => _wrap('_')),
                    ],
                  ),
                ),
              Expanded(
                child: _preview
                    ? Container(
                        width: double.infinity,
                        padding: const EdgeInsets.all(AppTokens.s12),
                        decoration: BoxDecoration(
                          color: c.sidebar,
                          borderRadius: BorderRadius.circular(AppTokens.rMd),
                          border: Border.all(color: c.border),
                        ),
                        child: SingleChildScrollView(
                          child: _body.text.trim().isEmpty
                              ? Text(context.tr('(empty)'),
                                  style: TextStyle(
                                      color: c.textMuted,
                                      fontStyle: FontStyle.italic))
                              // Interactive preview: ticking a box here edits
                              // the body being saved.
                              : NoteBody(
                                  _body.text,
                                  style: TextStyle(
                                      color: c.textPrimary, height: 1.5),
                                  onChanged: (nb) => _setBody(nb,
                                      TextSelection.collapsed(offset: nb.length)),
                                ),
                        ),
                      )
                    : TextField(
                        controller: _body,
                        expands: true,
                        maxLines: null,
                        textAlignVertical: TextAlignVertical.top,
                        decoration: InputDecoration(
                            hintText: context.tr(
                                'Body — Markdown, or “- [ ] task” for a checklist…')),
                      ),
              ),
              const SizedBox(height: AppTokens.s8),
              _TagEditor(
                tags: _tagList,
                controller: _tagInput,
                onSubmit: _commitTags,
                onRemove: (t) =>
                    setState(() => _tagList = _tagList.where((x) => x != t).toList()),
              ),
              const SizedBox(height: 4),
              Text(context.tr('#hashtags in the body become labels on save.'),
                  style: TextStyle(color: c.textMuted, fontSize: 11)),
              const SizedBox(height: AppTokens.s16),
              Row(
                mainAxisAlignment: MainAxisAlignment.end,
                children: [
                  TextButton(
                      onPressed: () => Navigator.of(context).pop(),
                      child: Text(context.tr('Cancel'))),
                  const SizedBox(width: AppTokens.s8),
                  FilledButton(
                      onPressed: _save, child: Text(context.tr('Save'))),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// A selectable pill in the sidebar tag-filter bar.
class _FilterChip extends StatelessWidget {
  const _FilterChip(
      {required this.label, required this.active, required this.onTap});
  final String label;
  final bool active;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Center(
      child: InkWell(
        onTap: onTap,
        borderRadius: BorderRadius.circular(AppTokens.rFull),
        child: Container(
          padding: const EdgeInsets.symmetric(
              horizontal: AppTokens.s12, vertical: 5),
          decoration: BoxDecoration(
            color: active ? c.accent : c.surfaceAlt,
            borderRadius: BorderRadius.circular(AppTokens.rFull),
            border: Border.all(color: active ? c.accent : c.border),
          ),
          child: Text(label,
              style: TextStyle(
                color: active ? Colors.white : c.textSecondary,
                fontSize: 12,
                fontWeight: active ? FontWeight.w600 : FontWeight.w500,
              )),
        ),
      ),
    );
  }
}

/// Keep-style tag editor: existing tags as removable chips + an inline field
/// that commits on Enter (or comma / space). Sits in the note editor dialog.
class _TagEditor extends StatelessWidget {
  const _TagEditor(
      {required this.tags,
      required this.controller,
      required this.onSubmit,
      required this.onRemove});
  final List<String> tags;
  final TextEditingController controller;
  final VoidCallback onSubmit;
  final ValueChanged<String> onRemove;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Container(
      padding:
          const EdgeInsets.symmetric(horizontal: AppTokens.s8, vertical: 6),
      decoration: BoxDecoration(
        borderRadius: BorderRadius.circular(AppTokens.rMd),
        border: Border.all(color: c.border),
      ),
      child: Wrap(
        spacing: 6,
        runSpacing: 6,
        crossAxisAlignment: WrapCrossAlignment.center,
        children: [
          Icon(Icons.label_outline, size: 16, color: c.textMuted),
          for (final t in tags)
            Container(
              padding: const EdgeInsets.only(left: AppTokens.s8, right: 2),
              decoration: BoxDecoration(
                color: c.accentSoft,
                borderRadius: BorderRadius.circular(AppTokens.rFull),
              ),
              child: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Text('#$t',
                      style: TextStyle(
                          color: c.accent,
                          fontSize: 12,
                          fontWeight: FontWeight.w500)),
                  InkWell(
                    onTap: () => onRemove(t),
                    borderRadius: BorderRadius.circular(AppTokens.rFull),
                    child: Padding(
                      padding: const EdgeInsets.all(2),
                      child: Icon(Icons.close, size: 13, color: c.accent),
                    ),
                  ),
                ],
              ),
            ),
          ConstrainedBox(
            constraints: const BoxConstraints(minWidth: 90, maxWidth: 160),
            child: TextField(
              controller: controller,
              style: TextStyle(color: c.textPrimary, fontSize: 13),
              decoration: InputDecoration(
                isDense: true,
                border: InputBorder.none,
                hintText: tags.isEmpty
                    ? context.tr('Add label…')
                    : context.tr('Add…'),
                hintStyle: TextStyle(color: c.textMuted, fontSize: 13),
                contentPadding: EdgeInsets.zero,
              ),
              onChanged: (v) {
                // Commit as soon as a separator is typed (Keep behaviour).
                if (v.endsWith(',') || v.endsWith(' ')) onSubmit();
              },
              onSubmitted: (_) => onSubmit(),
            ),
          ),
        ],
      ),
    );
  }
}

// ── Calendar ────────────────────────────────────────────────────────────
class _CalendarTab extends ConsumerStatefulWidget {
  const _CalendarTab();
  @override
  ConsumerState<_CalendarTab> createState() => _CalendarTabState();
}

class _CalendarTabState extends ConsumerState<_CalendarTab> {
  bool _monthView = true;
  String _query = '';
  late DateTime _month = DateTime(DateTime.now().year, DateTime.now().month);

  bool _matches(SpaceEvent e) {
    if (_query.isEmpty) return true;
    final q = _query.toLowerCase();
    return e.title.toLowerCase().contains(q) ||
        (e.description ?? '').toLowerCase().contains(q) ||
        (e.location ?? '').toLowerCase().contains(q);
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final events = ref.watch(eventsProvider);
    final fmt = DateFormat('EEE d MMM · HH:mm');
    // Searching forces the list view (the grid can't show match context well).
    final searching = _query.isNotEmpty;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(
              AppTokens.s16, AppTokens.s12, AppTokens.s16, 0),
          child: Row(
            children: [
              if (_monthView && !searching) ...[
                IconButton(
                  tooltip: context.tr('Previous month'),
                  icon: const Icon(Icons.chevron_left, size: 20),
                  onPressed: () => setState(
                      () => _month = DateTime(_month.year, _month.month - 1)),
                ),
                Text(DateFormat('MMMM yyyy').format(_month),
                    style: TextStyle(
                        color: c.textPrimary,
                        fontSize: 15,
                        fontWeight: FontWeight.w700)),
                IconButton(
                  tooltip: context.tr('Next month'),
                  icon: const Icon(Icons.chevron_right, size: 20),
                  onPressed: () => setState(
                      () => _month = DateTime(_month.year, _month.month + 1)),
                ),
                TextButton(
                  onPressed: () => setState(() => _month = DateTime(
                      DateTime.now().year, DateTime.now().month)),
                  child: Text(context.tr('Today')),
                ),
              ],
              const SizedBox(width: AppTokens.s8),
              // Search events.
              SizedBox(
                width: 220,
                child: TextField(
                  onChanged: (v) => setState(() => _query = v.trim()),
                  decoration: InputDecoration(
                    hintText: context.tr('Search events…'),
                    prefixIcon: const Icon(Icons.search, size: 16),
                    isDense: true,
                    border: const OutlineInputBorder(),
                    suffixIcon: _query.isEmpty
                        ? null
                        : IconButton(
                            icon: const Icon(Icons.close, size: 14),
                            onPressed: () => setState(() => _query = ''),
                          ),
                  ),
                ),
              ),
              const Spacer(),
              SegmentedButton<bool>(
                style:
                    const ButtonStyle(visualDensity: VisualDensity.compact),
                segments: const [
                  ButtonSegment(
                      value: true, icon: Icon(Icons.grid_view, size: 16)),
                  ButtonSegment(
                      value: false, icon: Icon(Icons.view_list, size: 16)),
                ],
                selected: {_monthView},
                onSelectionChanged: (s) =>
                    setState(() => _monthView = s.first),
              ),
              const SizedBox(width: AppTokens.s8),
              IconButton(
                tooltip: context.tr('Reload events'),
                icon: const Icon(Icons.refresh, size: 18),
                onPressed: () => ref.invalidate(eventsProvider),
              ),
              const SizedBox(width: AppTokens.s8),
              FilledButton.icon(
                onPressed: () => showDialog(
                    context: context, builder: (_) => const _EventEditor()),
                icon: const Icon(Icons.add_rounded, size: 18),
                label: Text(context.tr('New event')),
                style: FilledButton.styleFrom(
                  backgroundColor: context.colors.accent,
                  foregroundColor: Colors.white,
                  elevation: 0,
                  padding: const EdgeInsets.symmetric(
                      horizontal: AppTokens.s16, vertical: AppTokens.s12),
                  shape: RoundedRectangleBorder(
                      borderRadius: BorderRadius.circular(AppTokens.rXl)),
                  textStyle: const TextStyle(
                      fontSize: 13, fontWeight: FontWeight.w600),
                ),
              ),
            ],
          ),
        ),
        Expanded(
          child: events.when(
            loading: () => const Center(child: CircularProgressIndicator()),
            error: (e, _) => Center(child: Text('$e')),
            data: (list) {
              final filtered = list.where(_matches).toList();
              if (_monthView && !searching) {
                return _MonthGrid(month: _month, events: filtered);
              }
              if (filtered.isEmpty) {
                return Center(
                  child: Text(
                      searching
                          ? context.tr('No matching events')
                          : context.tr('No upcoming events'),
                      style: TextStyle(color: c.textMuted)),
                );
              }
              final sorted = [...filtered]
                ..sort((a, b) => a.startAt.compareTo(b.startAt));
              return ListView.builder(
                padding: const EdgeInsets.all(AppTokens.s16),
                itemCount: sorted.length,
                itemBuilder: (_, i) {
                  final e = sorted[i];
                  return Container(
                    margin: const EdgeInsets.only(bottom: AppTokens.s8),
                    padding: const EdgeInsets.all(AppTokens.s12),
                    decoration: BoxDecoration(
                      color: c.surface,
                      border: Border.all(color: c.border),
                      borderRadius: BorderRadius.circular(AppTokens.rMd),
                    ),
                    child: Row(
                      children: [
                        Icon(Icons.event, size: 16, color: c.accent),
                        const SizedBox(width: AppTokens.s12),
                        Expanded(
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              Text(e.title,
                                  style: TextStyle(
                                      color: c.textPrimary,
                                      fontWeight: FontWeight.w600)),
                              Text(
                                  e.allDay
                                      ? context.tr('All day')
                                      : fmt.format(e.start),
                                  style: TextStyle(
                                      color: c.textMuted, fontSize: 12)),
                            ],
                          ),
                        ),
                        // An event that points at a Space-App screen (today's
                        // lesson, a board, a report) opens it directly.
                        if (isInternalAppLink(e.link))
                          IconButton(
                            tooltip: context.trArgs('Open {target}', {
                              'target': e.linkAppId ?? context.tr('content')
                            }),
                            icon: Icon(Icons.open_in_new,
                                size: 16, color: c.accent),
                            onPressed: () async {
                              final err =
                                  await openEventLink(context, ref, e.link);
                              if (err != null && context.mounted) {
                                ScaffoldMessenger.of(context)
                                    .showSnackBar(SnackBar(content: Text(err)));
                              }
                            },
                          ),
                        IconButton(
                          tooltip: context.tr('Edit'),
                          icon: Icon(Icons.edit_outlined,
                              size: 16, color: c.textSecondary),
                          onPressed: () => showDialog(
                              context: context,
                              builder: (_) => _EventEditor(existing: e)),
                        ),
                        IconButton(
                          tooltip: context.tr('Delete'),
                          icon: const Icon(Icons.delete_outline,
                              size: 16, color: AppTokens.danger),
                          onPressed: () =>
                              ref.read(spaceApiProvider).deleteEvent(e.id),
                        ),
                      ],
                    ),
                  );
                },
              );
            },
          ),
        ),
      ],
    );
  }
}

/// A month calendar grid (6 weeks × 7 days) with event chips per day. Week
/// starts on Sunday to match the web CalendarView.
class _MonthGrid extends StatelessWidget {
  const _MonthGrid({required this.month, required this.events});
  final DateTime month;
  final List<SpaceEvent> events;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final first = DateTime(month.year, month.month);
    // weekday(): Mon=1..Sun=7 → Sunday-first offset.
    final lead = first.weekday % 7;
    final gridStart = first.subtract(Duration(days: lead));
    final today = DateTime.now();

    // Bucket events by yyyy-mm-dd.
    final byDay = <String, List<SpaceEvent>>{};
    for (final e in events) {
      final d = e.start;
      byDay.putIfAbsent('${d.year}-${d.month}-${d.day}', () => []).add(e);
    }

    const dow = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];
    return Padding(
      padding: const EdgeInsets.all(AppTokens.s16),
      child: Column(
        children: [
          Row(
            children: [
              for (final d in dow)
                Expanded(
                  child: Center(
                    child: Text(context.tr(d),
                        style: TextStyle(
                            color: c.textMuted,
                            fontSize: 11,
                            fontWeight: FontWeight.w600)),
                  ),
                ),
            ],
          ),
          const SizedBox(height: AppTokens.s4),
          Expanded(
            child: Column(
              children: [
                for (var w = 0; w < 6; w++)
                  Expanded(
                    child: Row(
                      children: [
                        for (var d = 0; d < 7; d++)
                          Builder(builder: (_) {
                            final day =
                                gridStart.add(Duration(days: w * 7 + d));
                            final inMonth = day.month == month.month;
                            final isToday = day.year == today.year &&
                                day.month == today.month &&
                                day.day == today.day;
                            final dayEvents =
                                byDay['${day.year}-${day.month}-${day.day}'] ??
                                    const [];
                            return Expanded(
                              child: InkWell(
                                onTap: () => showDialog(
                                  context: context,
                                  builder: (_) => _DayEventsDialog(day: day),
                                ),
                                borderRadius:
                                    BorderRadius.circular(AppTokens.rSm),
                                child: Container(
                                margin: const EdgeInsets.all(1),
                                padding: const EdgeInsets.all(3),
                                decoration: BoxDecoration(
                                  color: inMonth
                                      ? c.surface
                                      : c.surface.withValues(alpha: 0.4),
                                  borderRadius:
                                      BorderRadius.circular(AppTokens.rSm),
                                  border: Border.all(
                                      color: isToday ? c.accent : c.border,
                                      width: isToday ? 1.5 : 1),
                                ),
                                child: Column(
                                  crossAxisAlignment:
                                      CrossAxisAlignment.stretch,
                                  children: [
                                    Text('${day.day}',
                                        style: TextStyle(
                                            color: inMonth
                                                ? (isToday
                                                    ? c.accent
                                                    : c.textSecondary)
                                                : c.textMuted,
                                            fontSize: 11,
                                            fontWeight: isToday
                                                ? FontWeight.w700
                                                : FontWeight.w400)),
                                    const SizedBox(height: 2),
                                    Expanded(
                                      child: ListView(
                                        padding: EdgeInsets.zero,
                                        physics:
                                            const ClampingScrollPhysics(),
                                        children: [
                                          for (final e in dayEvents.take(4))
                                            Container(
                                              margin: const EdgeInsets.only(
                                                  bottom: 1),
                                              padding:
                                                  const EdgeInsets.symmetric(
                                                      horizontal: 3,
                                                      vertical: 1),
                                              decoration: BoxDecoration(
                                                color: c.accentSoft,
                                                borderRadius:
                                                    BorderRadius.circular(3),
                                              ),
                                              child: Text(e.title,
                                                  maxLines: 1,
                                                  overflow:
                                                      TextOverflow.ellipsis,
                                                  style: TextStyle(
                                                      color: c.accent,
                                                      fontSize: 9.5)),
                                            ),
                                          if (dayEvents.length > 4)
                                            Text('+${dayEvents.length - 4}',
                                                style: TextStyle(
                                                    color: c.textMuted,
                                                    fontSize: 9)),
                                        ],
                                      ),
                                    ),
                                  ],
                                ),
                              ),
                              ),
                            );
                          }),
                      ],
                    ),
                  ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

/// Tapping a day in the month grid opens this — the events on that day with
/// edit / delete, plus a "New event" shortcut. Stays live off [eventsProvider].
class _DayEventsDialog extends ConsumerWidget {
  const _DayEventsDialog({required this.day});
  final DateTime day;

  bool _sameDay(DateTime a) =>
      a.year == day.year && a.month == day.month && a.day == day.day;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final all = ref.watch(eventsProvider).valueOrNull ?? const [];
    final events = all.where((e) => _sameDay(e.start)).toList()
      ..sort((a, b) => a.startAt.compareTo(b.startAt));
    final fmt = DateFormat('HH:mm');
    return AlertDialog(
      backgroundColor: c.surface,
      title: Row(children: [
        Expanded(
          child: Text(DateFormat('EEE, d MMM yyyy').format(day),
              style: const TextStyle(fontSize: 16)),
        ),
        IconButton(
          tooltip: context.tr('New event'),
          icon: const Icon(Icons.add, size: 18),
          onPressed: () => showDialog(
              context: context,
              builder: (_) => _EventEditor(initialDay: day)),
        ),
      ]),
      content: SizedBox(
        width: 380,
        child: events.isEmpty
            ? Padding(
                padding: const EdgeInsets.symmetric(vertical: AppTokens.s16),
                child: Text(context.tr('No events on this day.'),
                    style: TextStyle(color: c.textMuted)),
              )
            : Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  for (final e in events)
                    Container(
                      margin: const EdgeInsets.only(bottom: AppTokens.s8),
                      padding: const EdgeInsets.all(AppTokens.s12),
                      decoration: BoxDecoration(
                        border: Border.all(color: c.border),
                        borderRadius: BorderRadius.circular(AppTokens.rMd),
                      ),
                      child: Row(children: [
                        Container(
                            width: 4,
                            height: 36,
                            decoration: BoxDecoration(
                                color: c.accent,
                                borderRadius: BorderRadius.circular(2))),
                        const SizedBox(width: AppTokens.s12),
                        Expanded(
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              Text(e.title,
                                  maxLines: 2,
                                  overflow: TextOverflow.ellipsis,
                                  style: TextStyle(
                                      color: c.textPrimary,
                                      fontWeight: FontWeight.w600)),
                              Text(
                                  e.allDay
                                      ? context.tr('All day')
                                      : '${fmt.format(e.start)} – ${fmt.format(DateTime.fromMillisecondsSinceEpoch(e.endAt))}',
                                  style: TextStyle(
                                      color: c.textMuted, fontSize: 12)),
                              if (e.location != null &&
                                  e.location!.isNotEmpty)
                                Text(e.location!,
                                    maxLines: 1,
                                    overflow: TextOverflow.ellipsis,
                                    style: TextStyle(
                                        color: c.textMuted, fontSize: 12)),
                            ],
                          ),
                        ),
                        // An event that points at a Space-App screen (today's
                        // lesson, a board, a report) opens it directly.
                        if (isInternalAppLink(e.link))
                          IconButton(
                            tooltip: context.trArgs('Open {target}', {
                              'target': e.linkAppId ?? context.tr('content')
                            }),
                            icon: Icon(Icons.open_in_new,
                                size: 16, color: c.accent),
                            onPressed: () async {
                              final err =
                                  await openEventLink(context, ref, e.link);
                              if (err != null && context.mounted) {
                                ScaffoldMessenger.of(context)
                                    .showSnackBar(SnackBar(content: Text(err)));
                              }
                            },
                          ),
                        IconButton(
                          tooltip: context.tr('Edit'),
                          icon: Icon(Icons.edit_outlined,
                              size: 16, color: c.textSecondary),
                          onPressed: () => showDialog(
                              context: context,
                              builder: (_) => _EventEditor(existing: e)),
                        ),
                        IconButton(
                          tooltip: context.tr('Delete'),
                          icon: const Icon(Icons.delete_outline,
                              size: 16, color: AppTokens.danger),
                          onPressed: () =>
                              ref.read(spaceApiProvider).deleteEvent(e.id),
                        ),
                      ]),
                    ),
                ],
              ),
      ),
      actions: [
        TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: Text(context.tr('Close'))),
      ],
    );
  }
}

/// Public entry to the new-event editor (used by the Dashboard's events panel).
Future<void> showCreateEventDialog(BuildContext context) =>
    showDialog(context: context, builder: (_) => const _EventEditor());

/// Public entry to a day's events (used by the Dashboard mini-calendar).
Future<void> showDayEventsDialog(BuildContext context, DateTime day) =>
    showDialog(context: context, builder: (_) => _DayEventsDialog(day: day));

/// Public entry to the new-note editor (used by the Dashboard quick actions).
Future<void> showCreateNoteDialog(BuildContext context) =>
    showDialog(context: context, builder: (_) => const _NoteEditor());

class _EventEditor extends ConsumerStatefulWidget {
  const _EventEditor({this.existing, this.initialDay});
  /// When set, the dialog edits this event (PUT) instead of creating one.
  final SpaceEvent? existing;
  /// When creating, pre-seed the date to this day (from the calendar grid).
  final DateTime? initialDay;
  @override
  ConsumerState<_EventEditor> createState() => _EventEditorState();
}

class _EventEditorState extends ConsumerState<_EventEditor> {
  final _title = TextEditingController();
  final _description = TextEditingController();
  final _location = TextEditingController();
  late DateTime _start;
  late DateTime _end;
  bool _allDay = false;
  bool _saving = false;

  @override
  void initState() {
    super.initState();
    final e = widget.existing;
    if (e != null) {
      _title.text = e.title;
      _description.text = e.description ?? '';
      _location.text = e.location ?? '';
      _allDay = e.allDay;
      _start = DateTime.fromMillisecondsSinceEpoch(e.startAt);
      _end = e.endAt > e.startAt
          ? DateTime.fromMillisecondsSinceEpoch(e.endAt)
          : _start.add(const Duration(hours: 1));
    } else if (widget.initialDay != null) {
      // Creating for a specific calendar day → default 09:00–10:00 that day.
      final d = widget.initialDay!;
      _start = DateTime(d.year, d.month, d.day, 9);
      _end = _start.add(const Duration(hours: 1));
    } else {
      final now = DateTime.now();
      // Default to the next full hour, 1-hour duration.
      final base = DateTime(now.year, now.month, now.day, now.hour + 1);
      _start = base;
      _end = base.add(const Duration(hours: 1));
    }
  }

  @override
  void dispose() {
    _title.dispose();
    _description.dispose();
    _location.dispose();
    super.dispose();
  }

  Future<DateTime?> _pick(DateTime initial) async {
    final d = await showDatePicker(
      context: context,
      initialDate: initial,
      firstDate: DateTime(2020),
      lastDate: DateTime(2100),
    );
    if (d == null || !mounted) return null;
    if (_allDay) return DateTime(d.year, d.month, d.day);
    final t = await showTimePicker(
      context: context,
      initialTime: TimeOfDay.fromDateTime(initial),
      builder: (ctx, child) => MediaQuery(
        data: MediaQuery.of(ctx).copyWith(alwaysUse24HourFormat: true),
        child: child!,
      ),
    );
    return DateTime(d.year, d.month, d.day, t?.hour ?? initial.hour,
        t?.minute ?? initial.minute);
  }

  Future<void> _editStart() async {
    final picked = await _pick(_start);
    if (picked == null) return;
    setState(() {
      final dur = _end.difference(_start);
      _start = picked;
      // Keep the original duration; never let end fall before start.
      _end = _start.add(dur.isNegative ? const Duration(hours: 1) : dur);
    });
  }

  Future<void> _editEnd() async {
    final picked = await _pick(_end);
    if (picked == null) return;
    setState(() => _end =
        picked.isBefore(_start) ? _start.add(const Duration(hours: 1)) : picked);
  }

  Future<void> _save() async {
    if (_title.text.trim().isEmpty || _saving) return;
    setState(() => _saving = true);
    final start =
        _allDay ? DateTime(_start.year, _start.month, _start.day) : _start;
    final end = _allDay
        ? DateTime(_end.year, _end.month, _end.day).add(const Duration(days: 1))
        : _end;
    try {
      final api = ref.read(spaceApiProvider);
      final desc =
          _description.text.trim().isEmpty ? null : _description.text.trim();
      final loc = _location.text.trim().isEmpty ? null : _location.text.trim();
      if (widget.existing != null) {
        await api.updateEvent(
          id: widget.existing!.id,
          title: _title.text.trim(),
          startAt: start.millisecondsSinceEpoch,
          endAt: end.millisecondsSinceEpoch,
          allDay: _allDay,
          description: desc,
          location: loc,
        );
      } else {
        await api.createEvent(
          title: _title.text.trim(),
          startAt: start.millisecondsSinceEpoch,
          endAt: end.millisecondsSinceEpoch,
          allDay: _allDay,
          description: desc,
          location: loc,
        );
      }
      if (mounted) Navigator.of(context).pop();
    } catch (e) {
      if (mounted) {
        setState(() => _saving = false);
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
            content: Text(context.trArgs('Failed: {e}', {'e': e}))));
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final df = _allDay
        ? DateFormat('EEE d MMM yyyy')
        : DateFormat('EEE d MMM yyyy · HH:mm');
    final canSave = _title.text.trim().isNotEmpty && !_saving;

    return Dialog(
      backgroundColor: c.surface,
      shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(AppTokens.rXl)),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 460),
        child: SingleChildScrollView(
          padding: const EdgeInsets.all(AppTokens.s24),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Row(
                children: [
                  Container(
                    width: 36,
                    height: 36,
                    decoration: BoxDecoration(
                      color: c.accent.withValues(alpha: 0.14),
                      borderRadius: BorderRadius.circular(AppTokens.rMd),
                    ),
                    child: Icon(Icons.event, color: c.accent, size: 20),
                  ),
                  const SizedBox(width: AppTokens.s12),
                  Text(
                      widget.existing == null
                          ? context.tr('New event')
                          : context.tr('Edit event'),
                      style: TextStyle(
                          color: c.textPrimary,
                          fontSize: 18,
                          fontWeight: FontWeight.w700)),
                ],
              ),
              const SizedBox(height: AppTokens.s20),
              TextField(
                controller: _title,
                autofocus: true,
                textInputAction: TextInputAction.next,
                onChanged: (_) => setState(() {}),
                decoration: InputDecoration(
                  labelText: context.tr('Title'),
                  hintText: context.tr('What is it?'),
                  border: const OutlineInputBorder(),
                  isDense: true,
                ),
              ),
              const SizedBox(height: AppTokens.s12),
              TextField(
                controller: _description,
                minLines: 2,
                maxLines: 4,
                decoration: InputDecoration(
                  labelText: context.tr('Description'),
                  hintText: context.tr('Optional notes…'),
                  border: const OutlineInputBorder(),
                  alignLabelWithHint: true,
                ),
              ),
              const SizedBox(height: AppTokens.s12),
              TextField(
                controller: _location,
                decoration: InputDecoration(
                  labelText: context.tr('Location'),
                  hintText: context.tr('Optional'),
                  prefixIcon: const Icon(Icons.place_outlined, size: 18),
                  border: const OutlineInputBorder(),
                  isDense: true,
                ),
              ),
              const SizedBox(height: AppTokens.s16),
              // All-day toggle.
              Container(
                padding: const EdgeInsets.symmetric(
                    horizontal: AppTokens.s12, vertical: AppTokens.s4),
                decoration: BoxDecoration(
                  color: c.surfaceAlt,
                  borderRadius: BorderRadius.circular(AppTokens.rMd),
                  border: Border.all(color: c.border),
                ),
                child: Row(
                  children: [
                    Icon(Icons.today_outlined, size: 16, color: c.textSecondary),
                    const SizedBox(width: AppTokens.s8),
                    Text(context.tr('All day'),
                        style: TextStyle(color: c.textPrimary, fontSize: 13)),
                    const Spacer(),
                    Switch(
                      value: _allDay,
                      onChanged: (v) => setState(() => _allDay = v),
                    ),
                  ],
                ),
              ),
              const SizedBox(height: AppTokens.s12),
              _DateField(
                  label: context.tr('Starts'),
                  value: df.format(_start),
                  onTap: _editStart),
              const SizedBox(height: AppTokens.s8),
              _DateField(
                  label: context.tr('Ends'),
                  value: df.format(_end),
                  onTap: _editEnd),
              const SizedBox(height: AppTokens.s24),
              Row(
                mainAxisAlignment: MainAxisAlignment.end,
                children: [
                  TextButton(
                      onPressed: _saving
                          ? null
                          : () => Navigator.of(context).pop(),
                      child: Text(context.tr('Cancel'))),
                  const SizedBox(width: AppTokens.s8),
                  FilledButton.icon(
                    onPressed: canSave ? _save : null,
                    icon: _saving
                        ? const SizedBox(
                            width: 14,
                            height: 14,
                            child: CircularProgressIndicator(strokeWidth: 2))
                        : const Icon(Icons.check, size: 16),
                    label: Text(context.tr('Create')),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// A tappable date/time row used by the event editor.
class _DateField extends StatelessWidget {
  const _DateField(
      {required this.label, required this.value, required this.onTap});
  final String label;
  final String value;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return InkWell(
      onTap: onTap,
      borderRadius: BorderRadius.circular(AppTokens.rMd),
      child: Container(
        padding: const EdgeInsets.symmetric(
            horizontal: AppTokens.s12, vertical: AppTokens.s12),
        decoration: BoxDecoration(
          borderRadius: BorderRadius.circular(AppTokens.rMd),
          border: Border.all(color: c.border),
        ),
        child: Row(
          children: [
            SizedBox(
              width: 52,
              child: Text(label,
                  style: TextStyle(color: c.textMuted, fontSize: 13)),
            ),
            Icon(Icons.event, size: 16, color: c.accent),
            const SizedBox(width: AppTokens.s8),
            Expanded(
              child: Text(value,
                  style: TextStyle(
                      color: c.textPrimary,
                      fontSize: 13,
                      fontWeight: FontWeight.w500)),
            ),
            Icon(Icons.edit_outlined, size: 14, color: c.textMuted),
          ],
        ),
      ),
    );
  }
}

// ── Apps (embedded Space apps) ────────────────────────────────────────────
/// The Apps launcher: a grid of installed apps (home). Tapping one launches it
/// into [RunningAppsLayer], which the shell keeps mounted across navigation so
/// apps keep running in the background (Android task model).
class SpaceAppsScreen extends ConsumerWidget {
  const SpaceAppsScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final apps = ref.watch(spaceAppsProvider);
    final run = ref.watch(runningAppsProvider);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Container(
          padding: const EdgeInsets.fromLTRB(
              AppTokens.s24, AppTokens.s16, AppTokens.s16, AppTokens.s12),
          decoration:
              BoxDecoration(border: Border(bottom: BorderSide(color: c.border))),
          child: Row(children: [
            Text(context.tr('Apps'),
                style: TextStyle(
                    color: c.textPrimary,
                    fontSize: 16,
                    fontWeight: FontWeight.w700)),
            const SizedBox(width: AppTokens.s12),
            if (run.running.isNotEmpty)
              Container(
                padding: const EdgeInsets.symmetric(
                    horizontal: AppTokens.s8, vertical: 2),
                decoration: BoxDecoration(
                  color: c.accentSoft,
                  borderRadius: BorderRadius.circular(AppTokens.rFull),
                ),
                child: Text(
                    context.trArgs(
                        '{n} running', {'n': run.running.length}),
                    style: TextStyle(color: c.accent, fontSize: 11)),
              ),
            const Spacer(),
            IconButton(
              tooltip: context.tr('Reload apps'),
              icon: const Icon(Icons.refresh, size: 18),
              onPressed: () => ref.invalidate(spaceAppsProvider),
            ),
          ]),
        ),
        Expanded(
          child: apps.when(
            loading: () => const Center(child: CircularProgressIndicator()),
            error: (e, _) => Center(child: Text('$e')),
            data: (all) {
              final list =
                  all.where((a) => a.showInLauncher).toList(growable: false);
              return list.isEmpty
                ? Center(
                    child: Text(context.tr('No apps installed'),
                        style: TextStyle(color: c.textMuted)))
                : GridView.builder(
                    padding: const EdgeInsets.all(AppTokens.s24),
                    gridDelegate:
                        const SliverGridDelegateWithMaxCrossAxisExtent(
                      maxCrossAxisExtent: 200,
                      mainAxisExtent: 132,
                      crossAxisSpacing: AppTokens.s16,
                      mainAxisSpacing: AppTokens.s16,
                    ),
                    itemCount: list.length,
                    itemBuilder: (_, i) => _AppTile(app: list[i]),
                  );
            },
          ),
        ),
      ],
    );
  }
}

/// One launcher tile. Shows a running badge + an inline terminate button when
/// the app is currently mounted. Right-click (or long-press) opens a context
/// menu: start/stop by state, pin to dashboard, app info & details.
class _AppTile extends ConsumerWidget {
  const _AppTile({required this.app});
  final SpaceApp app;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final ctl = ref.read(runningAppsProvider.notifier);
    final running = ref.watch(runningAppsProvider).isRunning(app.id);
    final pinned = ref.watch(pinnedAppsProvider).contains(app.id);
    return GestureDetector(
      onSecondaryTapDown: (d) =>
          showAppContextMenu(context, ref, app, d.globalPosition),
      onLongPressStart: (d) =>
          showAppContextMenu(context, ref, app, d.globalPosition),
      child: InkWell(
        onTap: () => ctl.open(app),
        borderRadius: BorderRadius.circular(AppTokens.rXl),
        child: Container(
          decoration: BoxDecoration(
            color: c.surface,
            border: Border.all(
                color: running ? c.accent : c.border, width: running ? 1.5 : 1),
            borderRadius: BorderRadius.circular(AppTokens.rXl),
          ),
          padding: const EdgeInsets.all(AppTokens.s12),
          child: Stack(
            children: [
              Center(
                child: Column(
                  mainAxisAlignment: MainAxisAlignment.center,
                  children: [
                    Text(app.icon, style: const TextStyle(fontSize: 34)),
                    const SizedBox(height: AppTokens.s8),
                    Text(app.name,
                        maxLines: 2,
                        textAlign: TextAlign.center,
                        overflow: TextOverflow.ellipsis,
                        style: TextStyle(
                            color: c.textPrimary,
                            fontSize: 13,
                            fontWeight: FontWeight.w500)),
                  ],
                ),
              ),
              if (pinned)
                Positioned(
                  top: 0,
                  left: 0,
                  child: Icon(Icons.push_pin, size: 13, color: c.accent),
                ),
              if (running)
                Positioned(
                  top: 0,
                  right: 0,
                  child: Row(children: [
                    Container(
                      width: 8,
                      height: 8,
                      decoration: const BoxDecoration(
                          color: AppTokens.success, shape: BoxShape.circle),
                    ),
                    InkWell(
                      onTap: () => ctl.close(app.id),
                      borderRadius: BorderRadius.circular(AppTokens.rFull),
                      child: Padding(
                        padding: const EdgeInsets.all(2),
                        child: Icon(Icons.close, size: 15, color: c.textMuted),
                      ),
                    ),
                  ]),
                ),
            ],
          ),
        ),
      ),
    );
  }
}

/// Right-click / long-press menu for a Space app tile. Actions adapt to the
/// app's running state (Start vs. Stop/Restart).
Future<void> showAppContextMenu(
    BuildContext context, WidgetRef ref, SpaceApp app, Offset pos) async {
  final c = context.colors;
  final ctl = ref.read(runningAppsProvider.notifier);
  final running = ref.read(runningAppsProvider).isRunning(app.id);
  final pinned = ref.read(pinnedAppsProvider).contains(app.id);
  final overlay =
      Overlay.of(context).context.findRenderObject() as RenderBox;

  final choice = await showMenu<String>(
    context: context,
    color: c.surface,
    position: RelativeRect.fromRect(
        pos & const Size(40, 40), Offset.zero & overlay.size),
    items: [
      if (running) ...[
        PopupMenuItem(
            value: 'stop',
            child: _MenuRow(Icons.stop_circle_outlined, context.tr('Stop'),
                color: AppTokens.danger)),
        PopupMenuItem(
            value: 'restart',
            child: _MenuRow(Icons.restart_alt, context.tr('Restart'))),
      ] else
        PopupMenuItem(
            value: 'start',
            child: _MenuRow(Icons.play_arrow, context.tr('Start'))),
      const PopupMenuDivider(),
      PopupMenuItem(
        value: 'pin',
        child: _MenuRow(
            pinned ? Icons.push_pin : Icons.push_pin_outlined,
            pinned
                ? context.tr('Unpin from dashboard')
                : context.tr('Pin to dashboard')),
      ),
      const PopupMenuDivider(),
      PopupMenuItem(
          value: 'info',
          child: _MenuRow(Icons.info_outline, context.tr('App info'))),
      PopupMenuItem(
          value: 'details',
          child: _MenuRow(Icons.tune, context.tr('Details'))),
    ],
  );
  if (choice == null || !context.mounted) return;

  switch (choice) {
    case 'start':
      ctl.open(app);
    case 'stop':
      ctl.close(app.id);
    case 'restart':
      await ref
          .read(apiClientProvider)
          .post('/api/space/apps/${app.id}/restart');
      if (context.mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
            content: Text(context
                .trArgs('Restarting {name}…', {'name': app.name}))));
      }
    case 'pin':
      ref.read(pinnedAppsProvider.notifier).toggle(app.id);
    case 'info':
      showDialog(context: context, builder: (_) => _AppInfoDialog(app: app));
    case 'details':
      showDialog(context: context, builder: (_) => _AppDetailsDialog(app: app));
  }
}

class _MenuRow extends StatelessWidget {
  const _MenuRow(this.icon, this.label, {this.color});
  final IconData icon;
  final String label;
  final Color? color;
  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final fg = color ?? c.textPrimary;
    return Row(
      children: [
        Icon(icon, size: 16, color: color ?? c.textSecondary),
        const SizedBox(width: AppTokens.s12),
        Text(label, style: TextStyle(color: fg, fontSize: 13)),
      ],
    );
  }
}

/// Friendly "App info" card — icon, name, description, status.
class _AppInfoDialog extends ConsumerWidget {
  const _AppInfoDialog({required this.app});
  final SpaceApp app;
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final running = ref.watch(runningAppsProvider).isRunning(app.id);
    return Dialog(
      backgroundColor: c.surface,
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 420),
        child: Padding(
          padding: const EdgeInsets.all(AppTokens.s24),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  Text(app.icon, style: const TextStyle(fontSize: 40)),
                  const SizedBox(width: AppTokens.s16),
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(app.name,
                            style: TextStyle(
                                color: c.textPrimary,
                                fontSize: 18,
                                fontWeight: FontWeight.w700)),
                        const SizedBox(height: AppTokens.s4),
                        Row(children: [
                          _Badge(
                            running
                                ? context.tr('Running')
                                : context.tr('Stopped'),
                            running ? AppTokens.success : c.textMuted,
                          ),
                          const SizedBox(width: AppTokens.s8),
                          _Badge(
                            app.enabled
                                ? context.tr('Enabled')
                                : context.tr('Disabled'),
                            app.enabled ? AppTokens.brand : AppTokens.danger,
                          ),
                        ]),
                      ],
                    ),
                  ),
                ],
              ),
              const SizedBox(height: AppTokens.s16),
              Text(
                app.description.isEmpty
                    ? context.tr('No description provided.')
                    : app.description,
                style: TextStyle(color: c.textSecondary, height: 1.5),
              ),
              const SizedBox(height: AppTokens.s20),
              Align(
                alignment: Alignment.centerRight,
                child: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    TextButton(
                        onPressed: () => Navigator.of(context).pop(),
                        child: Text(context.tr('Close'))),
                    const SizedBox(width: AppTokens.s8),
                    FilledButton.icon(
                      onPressed: () {
                        ref.read(runningAppsProvider.notifier).open(app);
                        Navigator.of(context).pop();
                      },
                      icon: Icon(running ? Icons.open_in_full : Icons.play_arrow,
                          size: 16),
                      label: Text(
                          running ? context.tr('Open') : context.tr('Start')),
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

/// Technical "Details" dialog — id, URL, integration state.
class _AppDetailsDialog extends ConsumerWidget {
  const _AppDetailsDialog({required this.app});
  final SpaceApp app;
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final running = ref.watch(runningAppsProvider).isRunning(app.id);
    return Dialog(
      backgroundColor: c.surface,
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 520),
        child: Padding(
          padding: const EdgeInsets.all(AppTokens.s24),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(context.trArgs('{name} · details', {'name': app.name}),
                  style: TextStyle(
                      color: c.textPrimary,
                      fontSize: 16,
                      fontWeight: FontWeight.w700)),
              const SizedBox(height: AppTokens.s16),
              _DetailRow('ID', app.id),
              if (app.version.isNotEmpty)
                _DetailRow(context.tr('Version'), app.version),
              _DetailRow(
                  context.tr('Status'),
                  running ? context.tr('Running') : context.tr('Stopped')),
              _DetailRow(context.tr('Enabled'),
                  app.enabled ? context.tr('Yes') : context.tr('No')),
              _DetailRow('URL', app.url),
              if (app.permissions.isNotEmpty) ...[
                const SizedBox(height: AppTokens.s12),
                Text(context.tr('PERMISSIONS'),
                    style: TextStyle(
                        color: c.textMuted,
                        fontSize: 11,
                        fontWeight: FontWeight.w700,
                        letterSpacing: 0.5)),
                const SizedBox(height: AppTokens.s6),
                Wrap(spacing: AppTokens.s6, runSpacing: AppTokens.s6, children: [
                  for (final p in app.permissions) _AppTag(p),
                ]),
              ],
              if (app.mcpServers.isNotEmpty) ...[
                const SizedBox(height: AppTokens.s12),
                Text(context.tr('MCP SERVERS'),
                    style: TextStyle(
                        color: c.textMuted,
                        fontSize: 11,
                        fontWeight: FontWeight.w700,
                        letterSpacing: 0.5)),
                const SizedBox(height: AppTokens.s6),
                Wrap(spacing: AppTokens.s6, runSpacing: AppTokens.s6, children: [
                  for (final s in app.mcpServers) _AppTag(s),
                ]),
              ],
              const SizedBox(height: AppTokens.s20),
              Align(
                alignment: Alignment.centerRight,
                child: TextButton(
                    onPressed: () => Navigator.of(context).pop(),
                    child: Text(context.tr('Close'))),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// Small pill for a permission / MCP-server name in the app details dialog.
class _AppTag extends StatelessWidget {
  const _AppTag(this.text);
  final String text;
  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Container(
      padding:
          const EdgeInsets.symmetric(horizontal: AppTokens.s8, vertical: 3),
      decoration: BoxDecoration(
        color: c.surfaceAlt,
        borderRadius: BorderRadius.circular(AppTokens.rFull),
        border: Border.all(color: c.border),
      ),
      child: Text(text,
          style: TextStyle(
              color: c.textSecondary,
              fontSize: 11,
              fontFamily: AppTokens.fontMono)),
    );
  }
}

class _DetailRow extends StatelessWidget {
  const _DetailRow(this.label, this.value);
  final String label;
  final String value;
  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: AppTokens.s6),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: 88,
            child: Text(label,
                style: TextStyle(color: c.textMuted, fontSize: 13)),
          ),
          Expanded(
            child: SelectableText(value,
                style: TextStyle(
                    color: c.textPrimary,
                    fontSize: 13,
                    fontFamily: AppTokens.fontMono)),
          ),
        ],
      ),
    );
  }
}

class _Badge extends StatelessWidget {
  const _Badge(this.text, this.color);
  final String text;
  final Color color;
  @override
  Widget build(BuildContext context) {
    return Container(
      padding:
          const EdgeInsets.symmetric(horizontal: AppTokens.s8, vertical: 2),
      decoration: BoxDecoration(
        color: color.withValues(alpha: 0.14),
        borderRadius: BorderRadius.circular(AppTokens.rFull),
      ),
      child: Text(text,
          style: TextStyle(
              color: color, fontSize: 11, fontWeight: FontWeight.w600)),
    );
  }
}

/// The persistent layer of running apps, mounted by the shell. An
/// [IndexedStack] keeps every running app's web view alive; the shell makes
/// this visible only on /apps when an app is active, so apps keep running in
/// the background while the user is elsewhere.
class RunningAppsLayer extends ConsumerWidget {
  const RunningAppsLayer({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final run = ref.watch(runningAppsProvider);
    if (run.running.isEmpty) return const SizedBox.shrink();
    final idx = run.activeId == null
        ? 0
        : run.running.indexWhere((a) => a.id == run.activeId);
    return IndexedStack(
      index: idx < 0 ? 0 : idx,
      children: [for (final a in run.running) _RunningAppView(app: a)],
    );
  }
}

class _RunningAppView extends ConsumerWidget {
  const _RunningAppView({required this.app});
  final SpaceApp app;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final ctl = ref.read(runningAppsProvider.notifier);
    return Container(
      color: c.bg,
      child: Column(
        children: [
          Container(
            padding: const EdgeInsets.symmetric(
                horizontal: AppTokens.s12, vertical: AppTokens.s6),
            decoration: BoxDecoration(
                border: Border(bottom: BorderSide(color: c.border))),
            child: Row(
              children: [
                IconButton(
                  tooltip: context.tr('Back to apps (keep running)'),
                  icon: const Icon(Icons.grid_view_outlined, size: 18),
                  onPressed: ctl.background,
                ),
                const SizedBox(width: AppTokens.s4),
                Text(app.name,
                    style: TextStyle(
                        color: c.textPrimary, fontWeight: FontWeight.w600)),
                const Spacer(),
                TextButton.icon(
                  onPressed: () async {
                    await ref
                        .read(apiClientProvider)
                        .post('/api/space/apps/${app.id}/restart');
                    if (context.mounted) {
                      ScaffoldMessenger.of(context).showSnackBar(SnackBar(
                          content: Text(context.tr('Restarting…'))));
                    }
                  },
                  icon: const Icon(Icons.restart_alt, size: 16),
                  label: Text(context.tr('Restart')),
                ),
                IconButton(
                  tooltip: context.tr('Fullscreen'),
                  icon: const Icon(Icons.fullscreen, size: 18),
                  onPressed: () => showDialog(
                    context: context,
                    barrierColor: Colors.black87,
                    builder: (dctx) => _AppFullscreen(
                        appId: app.id,
                        icon: app.icon,
                        url: app.url,
                        name: app.name,
                        theme: _embedTheme(dctx)),
                  ),
                ),
                IconButton(
                  tooltip: context.tr('Run in background'),
                  icon: const Icon(Icons.minimize, size: 18),
                  onPressed: ctl.background,
                ),
                IconButton(
                  tooltip: context.tr('Close app (terminate)'),
                  icon: const Icon(Icons.power_settings_new, size: 18),
                  color: AppTokens.danger,
                  onPressed: () => ctl.close(app.id),
                ),
              ],
            ),
          ),
          Expanded(
            // Never point the web view at an app that is not answering: a
            // server app runs on its own port, so a dead one renders as a
            // blank white rectangle with no error in it.
            child: AppHealthGate(
              appId: app.id,
              appName: app.name,
              appIcon: app.icon,
              builder: (ctx) => embeddedWebView(app.url,
                  title: app.name, theme: _embedTheme(ctx)),
            ),
          ),
        ],
      ),
    );
  }
}

// ── Schedules ───────────────────────────────────────────────────────────
class _SchedulesTab extends ConsumerStatefulWidget {
  const _SchedulesTab();
  @override
  ConsumerState<_SchedulesTab> createState() => _SchedulesTabState();
}

class _SchedulesTabState extends ConsumerState<_SchedulesTab> {
  @override
  void initState() {
    super.initState();
    // Re-fetch every time the tab is opened so next-run/status are current.
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) ref.invalidate(schedulesProvider);
    });
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final schedules = ref.watch(schedulesProvider);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(
              AppTokens.s16, AppTokens.s12, AppTokens.s16, 0),
          child: Row(
            mainAxisAlignment: MainAxisAlignment.end,
            children: [
              OutlinedButton.icon(
                onPressed: () => ref.invalidate(schedulesProvider),
                icon: const Icon(Icons.refresh, size: 16),
                label: Text(context.tr('Refresh')),
                style: OutlinedButton.styleFrom(
                  foregroundColor: c.textSecondary,
                  side: BorderSide(color: c.border),
                  padding: const EdgeInsets.symmetric(
                      horizontal: AppTokens.s16, vertical: AppTokens.s12),
                  shape: RoundedRectangleBorder(
                      borderRadius: BorderRadius.circular(AppTokens.rXl)),
                ),
              ),
              const SizedBox(width: AppTokens.s8),
              FilledButton.icon(
                onPressed: () => showDialog(
                    context: context, builder: (_) => const ScheduleEditorDialog()),
                icon: const Icon(Icons.add_rounded, size: 18),
                label: Text(context.tr('New schedule')),
                style: FilledButton.styleFrom(
                  backgroundColor: c.accent,
                  foregroundColor: Colors.white,
                  elevation: 0,
                  padding: const EdgeInsets.symmetric(
                      horizontal: AppTokens.s16, vertical: AppTokens.s12),
                  shape: RoundedRectangleBorder(
                      borderRadius: BorderRadius.circular(AppTokens.rXl)),
                  textStyle: const TextStyle(
                      fontSize: 13, fontWeight: FontWeight.w600),
                ),
              ),
            ],
          ),
        ),
        Expanded(
          child: schedules.when(
            loading: () => const Center(child: CircularProgressIndicator()),
            error: (e, _) => Center(child: Text('$e')),
            data: (list) {
              if (list.isEmpty) {
                return Center(
                    child: Text(context.tr('No schedules'),
                        style: TextStyle(color: c.textMuted)));
              }
              final active = list.where((s) => s.status == 'active').toList()
                ..sort((a, b) {
                  final va = a.nextRun != null ? DateTime.tryParse(a.nextRun!)?.millisecondsSinceEpoch ?? 0 : 0;
                  final vb = b.nextRun != null ? DateTime.tryParse(b.nextRun!)?.millisecondsSinceEpoch ?? 0 : 0;
                  return va.compareTo(vb);
                });
              final paused = list.where((s) => s.status == 'paused').toList();
              final other = list.where((s) => s.status != 'active' && s.status != 'paused').toList();
              final sections = [
                if (active.isNotEmpty)
                  (
                    context.trArgs('Active ({n})', {'n': active.length}),
                    active
                  ),
                if (paused.isNotEmpty)
                  (
                    context.trArgs('Paused ({n})', {'n': paused.length}),
                    paused
                  ),
                if (other.isNotEmpty)
                  (context.trArgs('Other ({n})', {'n': other.length}), other),
              ];
              return ListView.builder(
                padding: const EdgeInsets.all(AppTokens.s16),
                itemCount: sections.fold<int>(0, (sum, s) => sum + 1 + s.$2.length),
                itemBuilder: (_, i) {
                  var idx = i;
                  for (final (title, items) in sections) {
                    if (idx == 0) {
                      return Padding(
                        padding: const EdgeInsets.only(
                            bottom: AppTokens.s8, top: AppTokens.s4),
                        child: Text(title,
                            style: TextStyle(
                                color: c.textMuted,
                                fontSize: 11,
                                fontWeight: FontWeight.w600,
                                letterSpacing: 0.5)),
                      );
                    }
                    idx--;
                    if (idx < items.length) {
                      return _ScheduleCard(schedule: items[idx]);
                    }
                    idx -= items.length;
                  }
                  return const SizedBox.shrink();
                },
              );
            },
          ),
        ),
      ],
    );
  }
}

/// Human label for a schedule's timing. One-shot schedules carry an ISO
/// datetime in scheduleValue; recurring ones carry a 5-field cron.
String? _describeSchedule(SpaceSchedule s) {
  if (s.scheduleType == 'once' || s.scheduleType == 'once_delete') {
    final suffix = s.scheduleType == 'once_delete'
        ? ' · ${L10n.global.t('auto-delete')}'
        : '';
    final dt = DateTime.tryParse(s.scheduleValue);
    if (dt == null) return '${L10n.global.t('Once')}$suffix';
    final l = dt.toLocal();
    final d = '${l.day.toString().padLeft(2, '0')}/${l.month.toString().padLeft(2, '0')}';
    final hhmm =
        '${l.hour.toString().padLeft(2, '0')}:${l.minute.toString().padLeft(2, '0')}';
    return '${L10n.global.tArgs('Once · {t}', {'t': '$d $hhmm'})}$suffix';
  }
  return s.scheduleValue.isNotEmpty ? _describeCron(s.scheduleValue) : null;
}

String _describeCron(String cron) {
  final parts = cron.trim().split(RegExp(r'\s+'));
  if (parts.length != 5) return cron;
  final m = parts[0], h = parts[1], dom = parts[2], dow = parts[4];
  final hhmm = '${h.padLeft(2, '0')}:${m.padLeft(2, '0')}';
  const dayLabels = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];
  if (dom == '*' && dow == '*') {
    return L10n.global.tArgs('Every day · {t}', {'t': hhmm});
  }
  if (dom == '*' && dow == '1-5') {
    return L10n.global.tArgs('Mon–Fri · {t}', {'t': hhmm});
  }
  if (dom == '*' && RegExp(r'^\d$').hasMatch(dow)) {
    final d = int.tryParse(dow) ?? 0;
    final day = d < dayLabels.length ? L10n.global.t(dayLabels[d]) : dow;
    return L10n.global.tArgs('Every {d} · {t}', {'d': day, 't': hhmm});
  }
  if (RegExp(r'^\d+$').hasMatch(dom) && dow == '*') {
    return L10n.global
        .tArgs('Day {d} of every month · {t}', {'d': dom, 't': hhmm});
  }
  return cron;
}

class _ScheduleCard extends ConsumerWidget {
  const _ScheduleCard({required this.schedule});
  final SpaceSchedule schedule;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final label = schedule.label.split('\n').first;
    final isActive = schedule.status == 'active';
    final isPaused = schedule.status == 'paused';
    final cronDesc = _describeSchedule(schedule);
    return Container(
      margin: const EdgeInsets.only(bottom: AppTokens.s8),
      padding: const EdgeInsets.all(AppTokens.s12),
      decoration: BoxDecoration(
        color: c.surface,
        border: Border.all(
            color: isActive ? c.accent.withValues(alpha: 0.4) : c.border),
        borderRadius: BorderRadius.circular(AppTokens.rMd),
      ),
      child: Row(
        children: [
          Icon(Icons.schedule,
              size: 16,
              color: isActive
                  ? AppTokens.brandAlt
                  : isPaused
                      ? AppTokens.warning
                      : c.textMuted),
          const SizedBox(width: AppTokens.s12),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    Container(
                      padding: const EdgeInsets.symmetric(
                          horizontal: 6, vertical: 1),
                      decoration: BoxDecoration(
                        color: isActive
                            ? AppTokens.success.withValues(alpha: 0.15)
                            : isPaused
                                ? AppTokens.warning.withValues(alpha: 0.15)
                                : c.surfaceAlt,
                        borderRadius: BorderRadius.circular(AppTokens.rSm),
                      ),
                      child: Text(
                          isActive
                              ? context.tr('Running')
                              : isPaused
                                  ? context.tr('Paused')
                                  : schedule.status,
                          style: TextStyle(
                              color: isActive
                                  ? AppTokens.success
                                  : isPaused
                                      ? AppTokens.warning
                                      : c.textMuted,
                              fontSize: 10,
                              fontWeight: FontWeight.w600)),
                    ),
                    if (cronDesc != null) ...[
                      const SizedBox(width: AppTokens.s8),
                      Text(cronDesc,
                          style: TextStyle(color: c.textMuted, fontSize: 11)),
                    ],
                    if (schedule.lastStatus == 'success') ...[
                      const SizedBox(width: AppTokens.s6),
                      Icon(Icons.check_circle, size: 12, color: AppTokens.success),
                    ] else if (schedule.lastStatus == 'error') ...[
                      const SizedBox(width: AppTokens.s6),
                      Icon(Icons.cancel, size: 12, color: AppTokens.danger),
                    ],
                  ],
                ),
                const SizedBox(height: 3),
                Text(label,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                        color: c.textPrimary, fontWeight: FontWeight.w600)),
                if (schedule.prompt.isNotEmpty &&
                    schedule.prompt != schedule.label)
                  Text(schedule.prompt,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(color: c.textMuted, fontSize: 12)),
                Row(
                  children: [
                    if (schedule.nextRun != null && isActive)
                      Text(
                          context.trArgs('Next run: {t}',
                              {'t': _fmtLocalTs(schedule.nextRun) ?? '—'}),
                          style: TextStyle(color: c.textMuted, fontSize: 11)),
                    if (schedule.lastRun != null) ...[
                      if (schedule.nextRun != null && isActive)
                        Text('  ·  ',
                            style:
                                TextStyle(color: c.textMuted, fontSize: 11)),
                      Text(
                          context.trArgs('Last run: {t}',
                              {'t': _fmtLocalTs(schedule.lastRun) ?? '—'}),
                          style: TextStyle(color: c.textMuted, fontSize: 11)),
                    ],
                  ],
                ),
              ],
            ),
          ),
          if (isActive)
            IconButton(
              tooltip: context.tr('Pause'),
              icon: const Icon(Icons.pause_circle_outline, size: 18),
              onPressed: () async {
                await ref
                    .read(spaceApiProvider)
                    .updateSchedule(schedule.id, {'status': 'paused'});
                ref.invalidate(schedulesProvider);
              },
            )
          else if (isPaused)
            IconButton(
              tooltip: context.tr('Activate'),
              icon: Icon(Icons.play_circle_outline,
                  size: 18, color: AppTokens.success),
              onPressed: () async {
                await ref
                    .read(spaceApiProvider)
                    .updateSchedule(schedule.id, {'status': 'active'});
                ref.invalidate(schedulesProvider);
              },
            ),
          if (isActive)
            IconButton(
              tooltip: context.tr('Run now'),
              icon: Icon(Icons.bolt, size: 18, color: c.accent),
              onPressed: () async {
                await ref.read(spaceApiProvider).runSchedule(schedule.id);
                ref.invalidate(schedulesProvider);
                if (context.mounted) {
                  ScaffoldMessenger.of(context).showSnackBar(
                    SnackBar(
                        content: Text(context
                            .tr('Queued — runs in a few seconds'))),
                  );
                }
              },
            ),
          IconButton(
            tooltip: context.tr('Edit'),
            icon: const Icon(Icons.edit_outlined, size: 16),
            onPressed: () async {
              await showDialog(
                  context: context,
                  builder: (_) => ScheduleEditorDialog(existing: schedule));
              ref.invalidate(schedulesProvider);
            },
          ),
          IconButton(
            tooltip: context.tr('Delete'),
            icon: const Icon(Icons.delete_outline,
                size: 16, color: AppTokens.danger),
            onPressed: () =>
                ref.read(spaceApiProvider).deleteSchedule(schedule.id),
          ),
        ],
      ),
    );
  }
}


/// Fullscreen overlay for a Space app's embedded view.
class _AppFullscreen extends StatelessWidget {
  const _AppFullscreen(
      {required this.appId,
      required this.url,
      required this.name,
      this.icon = '',
      this.theme = 'light'});
  final String appId;
  final String icon;
  final String url;
  final String name;
  final String theme;
  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Dialog(
      insetPadding: EdgeInsets.zero,
      backgroundColor: c.surface,
      shape: const RoundedRectangleBorder(),
      child: Column(
        children: [
          Container(
            padding: const EdgeInsets.symmetric(
                horizontal: AppTokens.s16, vertical: AppTokens.s8),
            decoration:
                BoxDecoration(border: Border(bottom: BorderSide(color: c.border))),
            child: Row(
              children: [
                Text(name,
                    style: TextStyle(
                        color: c.textPrimary, fontWeight: FontWeight.w700)),
                const Spacer(),
                IconButton(
                  tooltip: context.tr('Exit fullscreen'),
                  icon: const Icon(Icons.fullscreen_exit, size: 20),
                  onPressed: () => Navigator.of(context).pop(),
                ),
              ],
            ),
          ),
          Expanded(
            child: AppHealthGate(
              appId: appId,
              appName: name,
              appIcon: icon,
              builder: (_) => embeddedWebView(url, title: name, theme: theme),
            ),
          ),
        ],
      ),
    );
  }
}

/// ISO timestamp (UTC from the daemon, e.g. `2026-07-03T02:10:00+00:00`) →
/// the user's LOCAL time as `yyyy-MM-dd HH:mm`. Falls back to the raw string
/// when unparsable; null stays null so callers can show a placeholder.
String? _fmtLocalTs(String? iso) {
  if (iso == null || iso.isEmpty) return null;
  final dt = DateTime.tryParse(iso);
  if (dt == null) return iso;
  return DateFormat('yyyy-MM-dd HH:mm').format(dt.toLocal());
}
