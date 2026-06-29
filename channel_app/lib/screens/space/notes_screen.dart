import 'package:flutter/material.dart';
import '../../models/space_models.dart';
import '../../services/space_api.dart';
import '../../theme/tokens.dart';
import '../../widgets/states.dart';
import 'space_page.dart';

/// Standalone Space surfaces. Space is no longer a tabbed container — each
/// feature is its own titled screen, opened from the drawer.
class NotesScreen extends StatelessWidget {
  const NotesScreen({super.key});
  @override
  Widget build(BuildContext context) =>
      const SpacePage(title: 'Notes', child: _NotesTab());
}

// ─── Notes ───────────────────────────────────────────────────────────────────

class _NotesTab extends StatefulWidget {
  const _NotesTab();

  @override
  State<_NotesTab> createState() => _NotesTabState();
}

class _NotesTabState extends State<_NotesTab>
    with AutomaticKeepAliveClientMixin {
  final _api = SpaceApi();
  List<SpaceNote> _notes = [];
  bool _loading = true;
  String? _error;
  String _query = '';

  @override
  bool get wantKeepAlive => true;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final notes = _query.trim().isEmpty
          ? await _api.listNotes()
          : await _api.searchNotes(_query.trim());
      if (!mounted) return;
      setState(() {
        _notes = notes;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = '$e';
        _loading = false;
      });
    }
  }

  Future<void> _edit({SpaceNote? note}) async {
    final saved = await showModalBottomSheet<bool>(
      context: context,
      isScrollControlled: true,
      backgroundColor: context.colors.surface,
      shape: const RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(top: Radius.circular(18)),
      ),
      builder: (_) => _NoteEditor(note: note),
    );
    if (saved == true) _load();
  }

  Future<void> _delete(SpaceNote note) async {
    final c = context.colors;
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: c.surface,
        title: Text('Xoá ghi chú?',
            style: TextStyle(color: c.textPrimary)),
        content: Text(note.title,
            style: TextStyle(color: c.textSecondary)),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(ctx, false),
              child: const Text('Huỷ')),
          TextButton(
              onPressed: () => Navigator.pop(ctx, true),
              child: const Text('Xoá',
                  style: TextStyle(color: AppTokens.danger))),
        ],
      ),
    );
    if (ok != true) return;
    try {
      await _api.deleteNote(note.id);
      _load();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Lỗi xoá: $e')));
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    final c = context.colors;
    return Scaffold(
      backgroundColor: Colors.transparent,
      floatingActionButton: FloatingActionButton(
        onPressed: () => _edit(),
        backgroundColor: c.accent,
        foregroundColor: Colors.white,
        child: const Icon(Icons.add),
      ),
      body: Column(
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(12, 12, 12, 4),
            child: TextField(
              style: TextStyle(color: c.textPrimary, fontSize: 14),
              onSubmitted: (v) {
                _query = v;
                _load();
              },
              decoration: InputDecoration(
                hintText: 'Tìm ghi chú…',
                hintStyle: TextStyle(color: c.textMuted),
                prefixIcon:
                    Icon(Icons.search, color: c.textMuted, size: 20),
                isDense: true,
                filled: true,
                fillColor: c.surfaceAlt,
                border: OutlineInputBorder(
                  borderRadius: BorderRadius.circular(10),
                  borderSide: BorderSide(color: c.border),
                ),
                enabledBorder: OutlineInputBorder(
                  borderRadius: BorderRadius.circular(10),
                  borderSide: BorderSide(color: c.border),
                ),
              ),
            ),
          ),
          Expanded(child: _buildList()),
        ],
      ),
    );
  }

  Widget _buildList() {
    final c = context.colors;
    if (_loading) return const LoadingState(text: 'Đang tải ghi chú…');
    if (_error != null) return ErrorState(message: _error!, onRetry: _load);
    if (_notes.isEmpty) {
      return const EmptyState(
        icon: Icons.sticky_note_2_outlined,
        message: 'Chưa có ghi chú',
        hint: 'Nhấn + để tạo ghi chú mới',
      );
    }
    return RefreshIndicator(
      onRefresh: _load,
      color: c.accent,
      backgroundColor: c.surface,
      child: ListView.builder(
        padding: const EdgeInsets.fromLTRB(12, 4, 12, 88),
        itemCount: _notes.length,
        itemBuilder: (ctx, i) => _noteCard(_notes[i]),
      ),
    );
  }

  Widget _noteCard(SpaceNote n) {
    final c = context.colors;
    return Card(
      color: c.surfaceAlt,
      margin: const EdgeInsets.only(bottom: 10),
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(14),
        side: BorderSide(color: c.border),
      ),
      child: ListTile(
        contentPadding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
        title: Row(
          children: [
            if (n.pinned)
              Padding(
                padding: const EdgeInsets.only(right: 6),
                child: Icon(Icons.push_pin, color: c.accent, size: 14),
              ),
            Expanded(
              child: Text(
                n.title.isEmpty ? '(không tiêu đề)' : n.title,
                style: TextStyle(
                    color: c.textPrimary, fontWeight: FontWeight.w600),
              ),
            ),
          ],
        ),
        subtitle: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            if (n.body.isNotEmpty) ...[
              const SizedBox(height: 4),
              Text(
                n.body,
                style: TextStyle(color: c.textMuted, fontSize: 12),
                maxLines: 2,
                overflow: TextOverflow.ellipsis,
              ),
            ],
            if (n.tags.isNotEmpty) ...[
              const SizedBox(height: 6),
              Wrap(
                spacing: 6,
                runSpacing: 4,
                children: n.tags
                    .map((t) => Container(
                          padding: const EdgeInsets.symmetric(
                              horizontal: 7, vertical: 2),
                          decoration: BoxDecoration(
                            color: AppTokens.cyan.withValues(alpha: 0.12),
                            borderRadius: BorderRadius.circular(6),
                          ),
                          child: Text('#$t',
                              style: const TextStyle(
                                  color: AppTokens.cyan, fontSize: 10)),
                        ))
                    .toList(),
              ),
            ],
          ],
        ),
        onTap: () => _edit(note: n),
        trailing: IconButton(
          icon: Icon(Icons.delete_outline,
              color: c.textMuted, size: 20),
          onPressed: () => _delete(n),
        ),
      ),
    );
  }
}

class _NoteEditor extends StatefulWidget {
  final SpaceNote? note;
  const _NoteEditor({this.note});

  @override
  State<_NoteEditor> createState() => _NoteEditorState();
}

class _NoteEditorState extends State<_NoteEditor> {
  final _api = SpaceApi();
  late final _titleCtrl = TextEditingController(text: widget.note?.title ?? '');
  late final _bodyCtrl = TextEditingController(text: widget.note?.body ?? '');
  late final _tagsCtrl =
      TextEditingController(text: widget.note?.tags.join(', ') ?? '');
  bool _saving = false;
  String? _error;

  @override
  void dispose() {
    _titleCtrl.dispose();
    _bodyCtrl.dispose();
    _tagsCtrl.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    final title = _titleCtrl.text.trim();
    if (title.isEmpty) {
      setState(() => _error = 'Cần tiêu đề');
      return;
    }
    final tags = _tagsCtrl.text
        .split(',')
        .map((s) => s.trim())
        .where((s) => s.isNotEmpty)
        .toList();
    setState(() {
      _saving = true;
      _error = null;
    });
    try {
      if (widget.note == null) {
        await _api.createNote(title: title, body: _bodyCtrl.text, tags: tags);
      } else {
        await _api.updateNote(widget.note!.id,
            title: title, body: _bodyCtrl.text, tags: tags);
      }
      if (mounted) Navigator.pop(context, true);
    } catch (e) {
      if (mounted) {
        setState(() {
          _error = '$e';
          _saving = false;
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Padding(
      padding: EdgeInsets.fromLTRB(
          20, 20, 20, MediaQuery.of(context).viewInsets.bottom + 20),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(widget.note == null ? 'Ghi chú mới' : 'Sửa ghi chú',
              style: TextStyle(
                  color: c.textPrimary,
                  fontSize: 18,
                  fontWeight: FontWeight.bold)),
          const SizedBox(height: 16),
          _field(_titleCtrl, 'Tiêu đề'),
          const SizedBox(height: 10),
          _field(_bodyCtrl, 'Nội dung', maxLines: 6),
          const SizedBox(height: 10),
          _field(_tagsCtrl, 'Tags (phân tách bằng dấu phẩy)'),
          if (_error != null) ...[
            const SizedBox(height: 8),
            Text(_error!,
                style: const TextStyle(color: AppTokens.danger, fontSize: 12)),
          ],
          const SizedBox(height: 16),
          SizedBox(
            width: double.infinity,
            child: ElevatedButton(
              onPressed: _saving ? null : _save,
              style: ElevatedButton.styleFrom(
                backgroundColor: c.accent,
                foregroundColor: Colors.white,
                padding: const EdgeInsets.symmetric(vertical: 14),
              ),
              child: _saving
                  ? const SizedBox(
                      width: 18,
                      height: 18,
                      child: CircularProgressIndicator(
                          strokeWidth: 2, color: Colors.white))
                  : const Text('Lưu'),
            ),
          ),
        ],
      ),
    );
  }

  Widget _field(TextEditingController c, String hint, {int maxLines = 1}) {
    final col = context.colors;
    return TextField(
      controller: c,
      maxLines: maxLines,
      style: TextStyle(color: col.textPrimary),
      decoration: InputDecoration(
        hintText: hint,
        hintStyle: TextStyle(color: col.textMuted),
        filled: true,
        fillColor: col.surfaceAlt,
        border: OutlineInputBorder(
          borderRadius: BorderRadius.circular(10),
          borderSide: BorderSide(color: col.border),
        ),
        enabledBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(10),
          borderSide: BorderSide(color: col.border),
        ),
      ),
    );
  }
}
