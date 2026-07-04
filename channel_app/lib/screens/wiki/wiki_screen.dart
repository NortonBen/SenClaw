import 'dart:async';
import 'package:flutter/material.dart';
import '../../models/wiki_models.dart';
import '../../services/language_service.dart';
import '../../services/relay_manager.dart';
import '../../services/wiki_api.dart';
import '../../theme/tokens.dart';
import '../../util/format.dart';
import '../../widgets/markdown_text.dart';
import '../../widgets/states.dart';

/// Wiki (knowledge base) over `/api/wiki/*`. Home (stats + recent + tags),
/// folder tree browse, full-text search, and a markdown doc viewer/editor.
class WikiScreen extends StatefulWidget {
  const WikiScreen({super.key});

  @override
  State<WikiScreen> createState() => _WikiScreenState();
}

class _WikiScreenState extends State<WikiScreen>
    with SingleTickerProviderStateMixin {
  final _api = WikiApi();
  late final TabController _tabs = TabController(length: 3, vsync: this);

  @override
  void dispose() {
    _tabs.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Scaffold(
      backgroundColor: c.bg,
      appBar: AppBar(
        backgroundColor: c.surface,
        elevation: 0,
        title: Row(
          children: [
            Text('Wiki', style: TextStyle(color: c.textPrimary)),
            const SizedBox(width: 8),
            AnimatedBuilder(
              animation: RelayManager(),
              builder: (_, _) =>
                  ConnectionDot(connected: RelayManager().connected),
            ),
          ],
        ),
        bottom: TabBar(
          controller: _tabs,
          indicatorColor: c.accent,
          labelColor: c.accent,
          unselectedLabelColor: c.textMuted,
          tabs: [
            Tab(
                icon: const Icon(Icons.home_outlined),
                text: tr('Tổng quan', 'Overview')),
            Tab(
                icon: const Icon(Icons.folder_outlined),
                text: tr('Thư mục', 'Folders')),
            Tab(icon: const Icon(Icons.search), text: tr('Tìm kiếm', 'Search')),
          ],
        ),
      ),
      body: Container(
        decoration: BoxDecoration(color: c.bg),
        child: TabBarView(
          controller: _tabs,
          children: [
            _WikiHomeTab(api: _api, onOpen: _openDoc),
            _WikiTreeTab(api: _api, onOpen: _openDoc),
            _WikiSearchTab(api: _api, onOpen: _openDoc),
          ],
        ),
      ),
    );
  }

  void _openDoc(String path) {
    Navigator.of(context).push(MaterialPageRoute(
      builder: (_) => WikiDocScreen(api: _api, path: path),
    ));
  }
}

// ─── Home ────────────────────────────────────────────────────────────────────

class _WikiHomeTab extends StatefulWidget {
  final WikiApi api;
  final void Function(String path) onOpen;
  const _WikiHomeTab({required this.api, required this.onOpen});

  @override
  State<_WikiHomeTab> createState() => _WikiHomeTabState();
}

class _WikiHomeTabState extends State<_WikiHomeTab>
    with AutomaticKeepAliveClientMixin {
  WikiStats? _stats;
  bool _loading = true;
  String? _error;

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
      final s = await widget.api.stats();
      if (!mounted) return;
      setState(() {
        _stats = s;
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

  @override
  Widget build(BuildContext context) {
    super.build(context);
    if (_loading) {
      return LoadingState(text: tr('Đang tải tổng quan…', 'Loading overview…'));
    }
    if (_error != null) return ErrorState(message: _error!, onRetry: _load);
    final s = _stats!;
    if (s.totalFiles == 0) {
      return EmptyState(
        icon: Icons.menu_book_outlined,
        message: tr('Kho tri thức trống', 'The knowledge base is empty'),
        hint: tr('Yêu cầu agent ghi lại kiến thức để xây dựng wiki',
            'Ask the agent to record knowledge to build the wiki'),
      );
    }
    final c = context.colors;
    return RefreshIndicator(
      onRefresh: _load,
      color: c.accent,
      backgroundColor: c.surface,
      child: ListView(
        padding: const EdgeInsets.fromLTRB(12, 12, 12, 24),
        children: [
          Row(
            children: [
              _statCard(Icons.description_outlined, '${s.totalFiles}',
                  tr('Trang', 'Pages')),
              const SizedBox(width: 10),
              _statCard(Icons.folder_outlined, '${s.totalDirs}',
                  tr('Thư mục', 'Folders')),
              const SizedBox(width: 10),
              _statCard(Icons.tag, '${s.byTag.length}', tr('Thẻ', 'Tags')),
            ],
          ),
          if (s.recentFiles.isNotEmpty) ...[
            const SizedBox(height: 18),
            _sectionLabel(tr('Cập nhật gần đây', 'Recently updated')),
            const SizedBox(height: 6),
            ...s.recentFiles.take(8).map((f) => _recentTile(f)),
          ],
          if (s.byTag.isNotEmpty) ...[
            const SizedBox(height: 18),
            _sectionLabel(tr('Thẻ phổ biến', 'Popular tags')),
            const SizedBox(height: 8),
            Wrap(
              spacing: 8,
              runSpacing: 8,
              children: s.byTag
                  .take(24)
                  .map((t) => Container(
                        padding: const EdgeInsets.symmetric(
                            horizontal: 10, vertical: 5),
                        decoration: BoxDecoration(
                          color: AppTokens.cyan.withValues(alpha: 0.12),
                          borderRadius: BorderRadius.circular(8),
                        ),
                        child: Text('#${t.name} · ${t.count}',
                            style: const TextStyle(
                                color: AppTokens.cyan, fontSize: 12)),
                      ))
                  .toList(),
            ),
          ],
          if (s.byCategory.isNotEmpty) ...[
            const SizedBox(height: 18),
            _sectionLabel(tr('Theo thư mục', 'By folder')),
            const SizedBox(height: 8),
            ...s.byCategory.map((c) => _categoryBar(c, s.byCategory.first.count)),
          ],
        ],
      ),
    );
  }

  Widget _statCard(IconData icon, String value, String label) {
    final c = context.colors;
    return Expanded(
      child: Container(
        padding: const EdgeInsets.symmetric(vertical: 16),
        decoration: BoxDecoration(
          color: c.surfaceAlt,
          borderRadius: BorderRadius.circular(14),
          border: Border.all(color: c.border),
        ),
        child: Column(
          children: [
            Icon(icon, color: c.accent, size: 22),
            const SizedBox(height: 8),
            Text(value,
                style: TextStyle(
                    color: c.textPrimary,
                    fontSize: 20,
                    fontWeight: FontWeight.bold)),
            Text(label,
                style: TextStyle(color: c.textMuted, fontSize: 11)),
          ],
        ),
      ),
    );
  }

  Widget _sectionLabel(String t) => Text(t,
      style: TextStyle(
          color: context.colors.textSecondary,
          fontSize: 13,
          fontWeight: FontWeight.w600));

  Widget _recentTile(WikiRecentFile f) {
    final c = context.colors;
    return Card(
      color: c.surfaceAlt,
      margin: const EdgeInsets.only(top: 8),
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(12),
        side: BorderSide(color: c.border),
      ),
      child: ListTile(
        dense: true,
        leading: Icon(Icons.article_outlined,
            color: c.textMuted, size: 20),
        title: Text(f.title.isEmpty ? titleFromPath(f.path) : f.title,
            style: TextStyle(color: c.textPrimary, fontSize: 14)),
        subtitle: Text(timeAgoIso(f.updated),
            style: TextStyle(color: c.textMuted, fontSize: 11)),
        onTap: () => widget.onOpen(f.path),
      ),
    );
  }

  Widget _categoryBar(WikiCategory cat, int max) {
    final c = context.colors;
    final frac = max == 0 ? 0.0 : (cat.count / max).clamp(0.05, 1.0);
    return Padding(
      padding: const EdgeInsets.only(bottom: 10),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Expanded(
                child: Text(cat.dir.isEmpty ? tr('(gốc)', '(root)') : cat.dir,
                    style:
                        TextStyle(color: c.textSecondary, fontSize: 12)),
              ),
              Text('${cat.count}',
                  style: TextStyle(color: c.textMuted, fontSize: 12)),
            ],
          ),
          const SizedBox(height: 4),
          ClipRRect(
            borderRadius: BorderRadius.circular(4),
            child: LinearProgressIndicator(
              value: frac,
              minHeight: 6,
              backgroundColor: c.surfaceAlt,
              valueColor:
                  AlwaysStoppedAnimation<Color>(c.accent),
            ),
          ),
        ],
      ),
    );
  }
}

// ─── Tree (folders) ──────────────────────────────────────────────────────────

class _WikiTreeTab extends StatefulWidget {
  final WikiApi api;
  final void Function(String path) onOpen;
  const _WikiTreeTab({required this.api, required this.onOpen});

  @override
  State<_WikiTreeTab> createState() => _WikiTreeTabState();
}

class _WikiTreeTabState extends State<_WikiTreeTab>
    with AutomaticKeepAliveClientMixin {
  List<WikiDirNode> _tree = [];
  bool _loading = true;
  String? _error;

  @override
  bool get wantKeepAlive => true;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() {
      _loading = _tree.isEmpty;
      _error = null;
    });
    var fresh = false;
    // Local-DB paint races the relay fetch in parallel — the relay result
    // always wins once it arrives.
    if (_tree.isEmpty) {
      unawaited(widget.api.treeCached().then((cached) {
        if (fresh || cached.isEmpty || !mounted || _tree.isNotEmpty) return;
        setState(() {
          _tree = cached;
          _loading = false;
          _error = null;
        });
      }));
    }
    try {
      final t = await widget.api.tree();
      fresh = true;
      if (!mounted) return;
      setState(() {
        _tree = t;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        // Keep the cached view usable when the refresh fails.
        _error = _tree.isEmpty ? '$e' : null;
        _loading = false;
      });
    }
  }

  Future<void> _newDoc() async {
    final c = context.colors;
    final pathCtrl = TextEditingController();
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: c.surface,
        title: Text(tr('Trang mới', 'New page'),
            style: TextStyle(color: c.textPrimary)),
        content: TextField(
          controller: pathCtrl,
          style: TextStyle(color: c.textPrimary, fontFamily: 'monospace'),
          decoration: InputDecoration(
            labelText: tr('Đường dẫn (vd: notes/ghi-chu.md)',
                'Path (e.g. notes/my-note.md)'),
            labelStyle: TextStyle(color: c.textSecondary),
          ),
        ),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(ctx, false),
              child: Text(tr('Huỷ', 'Cancel'))),
          TextButton(
              onPressed: () => Navigator.pop(ctx, true),
              child: Text(tr('Tạo', 'Create'),
                  style: TextStyle(color: c.accent))),
        ],
      ),
    );
    if (ok != true) return;
    var path = pathCtrl.text.trim();
    if (path.isEmpty) return;
    if (!path.endsWith('.md')) path = '$path.md';
    try {
      await widget.api.writeFile(
          path: path, content: '# ${titleFromPath(path)}\n\n', source: 'manual');
      if (!mounted) return;
      widget.onOpen(path);
      _load();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(
                content: Text(tr('Lỗi tạo: $e', 'Create failed: $e'))));
      }
    }
  }

  Future<void> _newFolder() async {
    final col = context.colors;
    final ctrl = TextEditingController();
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: col.surface,
        title: Text(tr('Thư mục mới', 'New folder'),
            style: TextStyle(color: col.textPrimary)),
        content: TextField(
          controller: ctrl,
          style: TextStyle(color: col.textPrimary, fontFamily: 'monospace'),
          decoration: InputDecoration(
            labelText:
                tr('Tên thư mục (kebab-case)', 'Folder name (kebab-case)'),
            labelStyle: TextStyle(color: col.textSecondary),
          ),
        ),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(ctx, false),
              child: Text(tr('Huỷ', 'Cancel'))),
          TextButton(
              onPressed: () => Navigator.pop(ctx, true),
              child: Text(tr('Tạo', 'Create'),
                  style: TextStyle(color: col.accent))),
        ],
      ),
    );
    if (ok != true) return;
    final p = ctrl.text.trim();
    if (p.isEmpty) return;
    try {
      await widget.api.mkdir(p);
      _load();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(
                SnackBar(content: Text(tr('Lỗi: $e', 'Error: $e'))));
      }
    }
  }

  Future<void> _deleteDir(WikiDirNode node) async {
    final col = context.colors;
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: col.surface,
        title: Text(tr('Xoá thư mục?', 'Delete folder?'),
            style: TextStyle(color: col.textPrimary)),
        content: Text(node.path,
            style: TextStyle(color: col.textSecondary)),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(ctx, false),
              child: Text(tr('Huỷ', 'Cancel'))),
          TextButton(
              onPressed: () => Navigator.pop(ctx, true),
              child: Text(tr('Xoá', 'Delete'),
                  style: TextStyle(color: AppTokens.danger))),
        ],
      ),
    );
    if (ok != true) return;
    try {
      await widget.api.deleteDir(node.path);
      _load();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(
                content: Text(tr('Lỗi xoá: $e', 'Delete failed: $e'))));
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    final c = context.colors;
    return Scaffold(
      backgroundColor: Colors.transparent,
      floatingActionButton: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          FloatingActionButton.small(
            heroTag: 'wiki-folder',
            onPressed: _newFolder,
            backgroundColor: c.surface,
            foregroundColor: c.accent,
            child: const Icon(Icons.create_new_folder_outlined),
          ),
          const SizedBox(height: 10),
          FloatingActionButton(
            heroTag: 'wiki-doc',
            onPressed: _newDoc,
            backgroundColor: c.accent,
            foregroundColor: Colors.white,
            child: const Icon(Icons.note_add),
          ),
        ],
      ),
      body: _buildBody(),
    );
  }

  Widget _buildBody() {
    final c = context.colors;
    if (_loading) {
      return LoadingState(text: tr('Đang tải thư mục…', 'Loading folders…'));
    }
    if (_error != null) return ErrorState(message: _error!, onRetry: _load);
    if (_tree.isEmpty) {
      return EmptyState(
        icon: Icons.folder_open_outlined,
        message: tr('Chưa có nội dung', 'No content yet'),
        hint: tr('Nhấn + để tạo trang hoặc thư mục',
            'Tap + to create a page or folder'),
      );
    }
    return RefreshIndicator(
      onRefresh: _load,
      color: c.accent,
      backgroundColor: c.surface,
      child: ListView(
        padding: const EdgeInsets.fromLTRB(8, 8, 8, 96),
        children: _tree.map((n) => _node(n, 0)).toList(),
      ),
    );
  }

  Widget _node(WikiDirNode n, int depth) {
    final c = context.colors;
    if (n.isDir) {
      return Theme(
        data: Theme.of(context).copyWith(dividerColor: Colors.transparent),
        child: ExpansionTile(
          tilePadding: EdgeInsets.only(left: 12.0 + depth * 14, right: 8),
          childrenPadding: EdgeInsets.zero,
          iconColor: c.textMuted,
          collapsedIconColor: c.textMuted,
          leading: Icon(Icons.folder_outlined,
              color: c.accent, size: 20),
          title: Text(n.name,
              style: TextStyle(color: c.textPrimary, fontSize: 14)),
          trailing: n.children.isEmpty
              ? IconButton(
                  icon: Icon(Icons.delete_outline,
                      color: c.textMuted, size: 18),
                  onPressed: () => _deleteDir(n),
                )
              : null,
          children: n.children.map((child) => _node(child, depth + 1)).toList(),
        ),
      );
    }
    return ListTile(
      contentPadding: EdgeInsets.only(left: 24.0 + depth * 14, right: 12),
      dense: true,
      leading: Icon(Icons.description_outlined,
          color: c.textMuted, size: 18),
      title: Text(
        n.name.replaceAll(RegExp(r'\.md$'), ''),
        style: TextStyle(color: c.textSecondary, fontSize: 13),
      ),
      onTap: () => widget.onOpen(n.path),
    );
  }
}

// ─── Search ──────────────────────────────────────────────────────────────────

class _WikiSearchTab extends StatefulWidget {
  final WikiApi api;
  final void Function(String path) onOpen;
  const _WikiSearchTab({required this.api, required this.onOpen});

  @override
  State<_WikiSearchTab> createState() => _WikiSearchTabState();
}

class _WikiSearchTabState extends State<_WikiSearchTab>
    with AutomaticKeepAliveClientMixin {
  final _ctrl = TextEditingController();
  List<WikiSearchResult> _results = [];
  bool _loading = false;
  bool _searched = false;
  String? _error;

  @override
  bool get wantKeepAlive => true;

  @override
  void dispose() {
    _ctrl.dispose();
    super.dispose();
  }

  Future<void> _search(String q) async {
    if (q.trim().isEmpty) return;
    setState(() {
      _loading = true;
      _searched = true;
      _error = null;
    });
    try {
      final r = await widget.api.search(q.trim());
      if (!mounted) return;
      setState(() {
        _results = r;
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

  @override
  Widget build(BuildContext context) {
    super.build(context);
    final c = context.colors;
    return Column(
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(12, 12, 12, 4),
          child: TextField(
            controller: _ctrl,
            style: TextStyle(color: c.textPrimary, fontSize: 14),
            textInputAction: TextInputAction.search,
            onSubmitted: _search,
            decoration: InputDecoration(
              hintText: tr('Tìm trong wiki…', 'Search the wiki…'),
              hintStyle: TextStyle(color: c.textMuted),
              prefixIcon:
                  Icon(Icons.search, color: c.textMuted, size: 20),
              suffixIcon: IconButton(
                icon: Icon(Icons.arrow_forward, color: c.accent),
                onPressed: () => _search(_ctrl.text),
              ),
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
        Expanded(child: _buildResults()),
      ],
    );
  }

  Widget _buildResults() {
    final c = context.colors;
    if (_loading) return const LoadingState();
    if (_error != null) return ErrorState(message: _error!);
    if (!_searched) {
      return EmptyState(
        icon: Icons.search,
        message: tr('Tìm kiếm tri thức', 'Search the knowledge base'),
        hint: tr('Nhập từ khoá rồi nhấn Enter',
            'Type a keyword and press Enter'),
      );
    }
    if (_results.isEmpty) {
      return EmptyState(
        icon: Icons.search_off,
        message: tr('Không có kết quả', 'No results'),
      );
    }
    return ListView.builder(
      padding: const EdgeInsets.fromLTRB(12, 4, 12, 24),
      itemCount: _results.length,
      itemBuilder: (ctx, i) {
        final r = _results[i];
        return Card(
          color: c.surfaceAlt,
          margin: const EdgeInsets.only(bottom: 8),
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(12),
            side: BorderSide(color: c.border),
          ),
          child: ListTile(
            title: Text(r.title.isEmpty ? titleFromPath(r.path) : r.title,
                style: TextStyle(
                    color: c.textPrimary, fontWeight: FontWeight.w600)),
            subtitle: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                if (r.snippet != null && r.snippet!.isNotEmpty) ...[
                  const SizedBox(height: 4),
                  Text(r.snippet!,
                      style: TextStyle(
                          color: c.textSecondary, fontSize: 12),
                      maxLines: 2,
                      overflow: TextOverflow.ellipsis),
                ],
                const SizedBox(height: 4),
                Text(r.path,
                    style: TextStyle(
                        color: c.textMuted,
                        fontSize: 11,
                        fontFamily: 'monospace')),
              ],
            ),
            onTap: () => widget.onOpen(r.path),
          ),
        );
      },
    );
  }
}

// ─── Doc viewer / editor ──────────────────────────────────────────────────────

class WikiDocScreen extends StatefulWidget {
  final WikiApi api;
  final String path;
  const WikiDocScreen({super.key, required this.api, required this.path});

  @override
  State<WikiDocScreen> createState() => _WikiDocScreenState();
}

class _WikiDocScreenState extends State<WikiDocScreen> {
  WikiDoc? _doc;
  bool _loading = true;
  bool _editing = false;
  bool _saving = false;
  String? _error;
  late final TextEditingController _editCtrl = TextEditingController();

  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  void dispose() {
    _editCtrl.dispose();
    super.dispose();
  }

  /// Strip a leading YAML frontmatter block for display.
  String _stripFrontmatter(String content) {
    if (content.startsWith('---')) {
      final end = content.indexOf('\n---', 3);
      if (end != -1) {
        final after = content.indexOf('\n', end + 1);
        if (after != -1) return content.substring(after + 1).trimLeft();
      }
    }
    return content;
  }

  Future<void> _load() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final d = await widget.api.file(widget.path);
      if (!mounted) return;
      setState(() {
        _doc = d;
        _editCtrl.text = d.content;
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

  Future<void> _save() async {
    setState(() => _saving = true);
    try {
      await widget.api.writeFile(
        path: widget.path,
        content: _editCtrl.text,
        commitMsg: 'wiki: edit ${widget.path}',
      );
      await _load();
      if (mounted) setState(() => _editing = false);
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(
                content: Text(tr('Lỗi lưu: $e', 'Save failed: $e'))));
      }
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final col = context.colors;
    final d = _doc;
    return Scaffold(
      backgroundColor: col.bg,
      appBar: AppBar(
        backgroundColor: col.surface,
        elevation: 0,
        title: Text(titleFromPath(widget.path),
            style: TextStyle(color: col.textPrimary, fontSize: 16)),
        actions: [
          if (d != null && !_editing)
            IconButton(
              icon: Icon(Icons.edit_outlined, color: col.textSecondary),
              onPressed: () => setState(() => _editing = true),
            ),
          if (_editing) ...[
            TextButton(
              onPressed: _saving
                  ? null
                  : () {
                      _editCtrl.text = d?.content ?? '';
                      setState(() => _editing = false);
                    },
              child: Text(tr('Huỷ', 'Cancel'),
                  style: TextStyle(color: col.textSecondary)),
            ),
            TextButton(
              onPressed: _saving ? null : _save,
              child: _saving
                  ? SizedBox(
                      width: 16,
                      height: 16,
                      child: CircularProgressIndicator(
                          strokeWidth: 2, color: col.accent))
                  : Text(tr('Lưu', 'Save'),
                      style: TextStyle(color: col.accent)),
            ),
          ],
        ],
      ),
      body: Container(
        decoration: BoxDecoration(color: col.bg),
        child: _buildBody(),
      ),
    );
  }

  Widget _buildBody() {
    final col = context.colors;
    if (_loading) return const LoadingState();
    if (_error != null) return ErrorState(message: _error!, onRetry: _load);
    final d = _doc!;
    if (_editing) {
      return Padding(
        padding: const EdgeInsets.all(12),
        child: TextField(
          controller: _editCtrl,
          maxLines: null,
          expands: true,
          textAlignVertical: TextAlignVertical.top,
          style: TextStyle(
              color: col.textPrimary, fontFamily: 'monospace', fontSize: 13),
          decoration: InputDecoration(
            border: InputBorder.none,
            hintText: tr('Nội dung markdown…', 'Markdown content…'),
            hintStyle: TextStyle(color: col.textMuted),
          ),
        ),
      );
    }
    return ListView(
      padding: const EdgeInsets.fromLTRB(16, 12, 16, 32),
      children: [
        Text(widget.path,
            style: TextStyle(
                color: col.textMuted,
                fontSize: 11,
                fontFamily: 'monospace')),
        if (d.frontmatter.tags.isNotEmpty) ...[
          const SizedBox(height: 8),
          Wrap(
            spacing: 6,
            runSpacing: 4,
            children: d.frontmatter.tags
                .map((t) => Container(
                      padding:
                          const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
                      decoration: BoxDecoration(
                        color: AppTokens.warning.withValues(alpha: 0.14),
                        borderRadius: BorderRadius.circular(6),
                      ),
                      child: Text('#$t',
                          style: const TextStyle(
                              color: AppTokens.warning, fontSize: 11)),
                    ))
                .toList(),
          ),
        ],
        if (d.frontmatter.updated != null) ...[
          const SizedBox(height: 6),
          Text(
              tr('Cập nhật ${timeAgoIso(d.frontmatter.updated)}',
                  'Updated ${timeAgoIso(d.frontmatter.updated)}'),
              style: TextStyle(color: col.textMuted, fontSize: 11)),
        ],
        const SizedBox(height: 14),
        MarkdownText(_stripFrontmatter(d.content)),
        if (d.gitLog.isNotEmpty) ...[
          const SizedBox(height: 24),
          Divider(color: col.border),
          const SizedBox(height: 8),
          Text(tr('Lịch sử', 'History'),
              style: TextStyle(
                  color: col.textSecondary,
                  fontSize: 13,
                  fontWeight: FontWeight.w600)),
          const SizedBox(height: 8),
          ...d.gitLog.map((c) => Padding(
                padding: const EdgeInsets.only(bottom: 8),
                child: Row(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                        c.hash.length > 7 ? c.hash.substring(0, 7) : c.hash,
                        style: const TextStyle(
                            color: AppTokens.cyan,
                            fontSize: 11,
                            fontFamily: 'monospace')),
                    const SizedBox(width: 10),
                    Expanded(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(c.message,
                              style: TextStyle(
                                  color: col.textSecondary, fontSize: 12)),
                          Text(timeAgoIso(c.date),
                              style: TextStyle(
                                  color: col.textMuted, fontSize: 10)),
                        ],
                      ),
                    ),
                  ],
                ),
              )),
        ],
      ],
    );
  }
}
