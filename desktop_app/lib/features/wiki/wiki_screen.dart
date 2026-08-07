import 'dart:typed_data';
import 'dart:convert';
import 'dart:io' show File;
import 'package:file_picker/file_picker.dart';
import 'package:flutter/foundation.dart' show kIsWeb;
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:http/http.dart' as http;
import '../../core/i18n/l10n.dart';
import '../../core/transport/connection.dart';
import '../../theme/tokens.dart';
import '../../widgets/app_markdown.dart';

/// Upload a file into the wiki (multipart POST /api/wiki/upload, fields
/// `folder` + `file`).
Future<void> _uploadWikiFile(WidgetRef ref, {String folder = ''}) async {
  final res = await FilePicker.platform.pickFiles(withData: kIsWeb);
  final f = res?.files.firstOrNull;
  if (f == null) return;
  final cfg = ref.read(appConfigProvider);
  final uri = Uri.parse('http://${cfg.host}:${cfg.uiPort}/api/wiki/upload');
  final req = http.MultipartRequest('POST', uri)..fields['folder'] = folder;
  req.headers.addAll(cfg.authHeaders);
  if (kIsWeb && f.bytes != null) {
    req.files
        .add(http.MultipartFile.fromBytes('file', f.bytes!, filename: f.name));
  } else if (f.path != null) {
    req.files.add(http.MultipartFile.fromBytes(
        'file', await File(f.path!).readAsBytes(),
        filename: f.name));
  } else {
    return;
  }
  await req.send();
  ref.invalidate(wikiTreeProvider);
}

// ── Right-click context menu on a wiki tree node ──────────────────────────
Future<void> _showNodeMenu(
    BuildContext context, WidgetRef ref, WikiNode n, Offset pos) async {
  final overlay =
      Overlay.of(context).context.findRenderObject() as RenderBox;
  // Highlight the targeted row while the menu is open.
  ref.read(wikiContextTargetProvider.notifier).state = n.path;
  final sel = await showMenu<String>(
    context: context,
    position: RelativeRect.fromRect(
        pos & const Size(1, 1), Offset.zero & overlay.size),
    items: n.isDir
        ? [
            PopupMenuItem(
                value: 'newfile', child: Text(context.tr('New file…'))),
            PopupMenuItem(
                value: 'newfolder', child: Text(context.tr('New folder…'))),
            PopupMenuItem(
                value: 'upload', child: Text(context.tr('Upload file…'))),
            const PopupMenuDivider(),
            PopupMenuItem(
                value: 'deletedir',
                child: Text(context.tr('Delete folder'),
                    style: const TextStyle(color: AppTokens.danger))),
          ]
        : [
            PopupMenuItem(
                value: 'download', child: Text(context.tr('Download'))),
            PopupMenuItem(
                value: 'deletefile',
                child: Text(context.tr('Delete'),
                    style: const TextStyle(color: AppTokens.danger))),
          ],
  );
  ref.read(wikiContextTargetProvider.notifier).state = null;
  if (sel == null || !context.mounted) return;
  switch (sel) {
    case 'newfile':
      await _newWikiNode(context, ref, n.path, isDir: false);
    case 'newfolder':
      await _newWikiNode(context, ref, n.path, isDir: true);
    case 'upload':
      await _uploadWikiFile(ref, folder: n.path);
    case 'deletedir':
      await _deleteWikiNode(context, ref, n.path, isDir: true);
    case 'deletefile':
      await _deleteWikiNode(context, ref, n.path, isDir: false);
    case 'download':
      await _downloadWikiFile(context, ref, n);
  }
}

Future<String?> _promptName(
    BuildContext context, String title, String hint) async {
  final ctrl = TextEditingController();
  final ok = await showDialog<bool>(
    context: context,
    builder: (dctx) => AlertDialog(
      backgroundColor: dctx.colors.surface,
      title: Text(title),
      content: TextField(
        controller: ctrl,
        autofocus: true,
        decoration: InputDecoration(hintText: hint),
        onSubmitted: (_) => Navigator.pop(dctx, true),
      ),
      actions: [
        TextButton(
            onPressed: () => Navigator.pop(dctx, false),
            child: Text(dctx.tr('Cancel'))),
        FilledButton(
            onPressed: () => Navigator.pop(dctx, true),
            child: Text(dctx.tr('OK'))),
      ],
    ),
  );
  if (ok != true) return null;
  final v = ctrl.text.trim();
  return v.isEmpty ? null : v;
}

Future<void> _newWikiNode(BuildContext context, WidgetRef ref, String folder,
    {required bool isDir}) async {
  final name = await _promptName(
      context,
      isDir ? context.tr('New folder') : context.tr('New file'),
      isDir ? context.tr('name') : 'name.md');
  if (name == null) return;
  final path = folder.isEmpty ? name : '$folder/$name';
  final api = ref.read(apiClientProvider);
  if (isDir) {
    await api.post('/api/wiki/mkdir', body: {'path': path});
  } else {
    final p = path.endsWith('.md') ? path : '$path.md';
    final title = name.replaceAll('.md', '');
    await api.put('/api/wiki/file',
        body: {'path': p, 'content': '# $title\n', 'source': 'manual'});
  }
  ref.invalidate(wikiTreeProvider);
}

Future<void> _deleteWikiNode(BuildContext context, WidgetRef ref, String path,
    {required bool isDir}) async {
  final ok = await showDialog<bool>(
    context: context,
    builder: (dctx) => AlertDialog(
      backgroundColor: dctx.colors.surface,
      title: Text(
          dctx.tr(isDir ? 'Delete folder?' : 'Delete file?')),
      content: Text(isDir
          ? '${dctx.tr('Only empty folders can be removed.')}\n$path'
          : '${dctx.tr('This cannot be undone.')}\n$path'),
      actions: [
        TextButton(
            onPressed: () => Navigator.pop(dctx, false),
            child: Text(dctx.tr('Cancel'))),
        FilledButton(
            style: FilledButton.styleFrom(backgroundColor: AppTokens.danger),
            onPressed: () => Navigator.pop(dctx, true),
            child: Text(dctx.tr('Delete'))),
      ],
    ),
  );
  if (ok != true) return;
  final q = Uri.encodeQueryComponent(path);
  try {
    await ref
        .read(apiClientProvider)
        .delete('/api/wiki/${isDir ? 'dir' : 'file'}?path=$q');
    if (ref.read(wikiSelectedProvider) == path) {
      ref.read(wikiSelectedProvider.notifier).state = null;
    }
    ref.invalidate(wikiTreeProvider);
  } catch (e) {
    if (context.mounted) {
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(
          content: Text(context.trArgs('Delete failed: {err}', {'err': e}))));
    }
  }
}

Future<void> _downloadWikiFile(
    BuildContext context, WidgetRef ref, WikiNode n) async {
  // Resolved up front: the save-dialog title is needed after the fetch await,
  // where `context` may no longer be mounted.
  final dialogTitle = context.trArgs('Save {name}', {'name': n.name});
  final r = await ref
      .read(apiClientProvider)
      .get('/api/wiki/file', query: {'path': n.path});
  final content = (r is Map ? '${r['content'] ?? ''}' : '$r');
  final bytes = utf8.encode(content);
  final savePath = await FilePicker.platform.saveFile(
    dialogTitle: dialogTitle,
    fileName: n.name,
    bytes: kIsWeb ? Uint8List.fromList(bytes) : null,
  );
  if (!kIsWeb && savePath != null) {
    await File(savePath).writeAsBytes(bytes);
  }
  if (context.mounted && (savePath != null || kIsWeb)) {
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(
        content: Text(context.trArgs('Saved {name}', {'name': n.name}))));
  }
}

class WikiNode {
  final String name;
  final String path;
  final bool isDir;
  final List<WikiNode> children;
  const WikiNode(this.name, this.path, this.isDir, this.children);

  factory WikiNode.fromJson(Map<String, dynamic> j) => WikiNode(
    '${j['name'] ?? ''}',
    '${j['path'] ?? ''}',
    j['type'] == 'dir',
    ((j['children'] as List?) ?? const [])
        .whereType<Map>()
        .map((m) => WikiNode.fromJson(m.cast<String, dynamic>()))
        .toList(),
  );
}

final wikiTreeProvider = FutureProvider<List<WikiNode>>((ref) async {
  final r = await ref.read(apiClientProvider).get('/api/wiki/tree');
  final tree = (r is Map ? r['tree'] : r) as List? ?? const [];
  return tree
      .whereType<Map>()
      .map((m) => WikiNode.fromJson(m.cast<String, dynamic>()))
      .toList();
});

final wikiFileProvider = FutureProvider.family<String, String>((ref, path) async {
  final r = await ref
      .read(apiClientProvider)
      .get('/api/wiki/file', query: {'path': path});
  if (r is Map && r['content'] != null) return '${r['content']}';
  return r is String ? r : '';
});

final wikiSelectedProvider = StateProvider<String?>((ref) => null);

/// Path of the node a right-click context menu is currently targeting — so the
/// row visibly highlights which folder/file the menu will act on.
final wikiContextTargetProvider = StateProvider<String?>((ref) => null);

class WikiHit {
  final String path;
  final String title;
  final String snippet;
  const WikiHit(this.path, this.title, this.snippet);
  factory WikiHit.fromJson(Map<String, dynamic> j) => WikiHit(
    '${j['path'] ?? ''}',
    '${j['title'] ?? j['path'] ?? ''}',
    '${j['snippet'] ?? ''}',
  );
}

final wikiQueryProvider = StateProvider<String>((ref) => '');
// Active tag filter (null = none). Set by clicking a tag chip.
final wikiTagProvider = StateProvider<String?>((ref) => null);

/// All wiki tags for the tag cloud (`GET /api/wiki/tags`).
final wikiTagsProvider = FutureProvider<List<String>>((ref) async {
  final r = await ref.read(apiClientProvider).get('/api/wiki/tags');
  final tags = (r is Map ? r['tags'] : r) as List? ?? const [];
  return tags
      .map((t) => t is Map ? '${t['name'] ?? t['tag'] ?? ''}' : '$t')
      .where((s) => s.isNotEmpty)
      .toList();
});

/// Wiki knowledge stats ({totalFiles, totalDirs, …}) for the sidebar header.
final wikiStatsProvider = FutureProvider<Map<String, dynamic>>((ref) async {
  // Recompute whenever the tree changes (page/folder add/delete).
  ref.watch(wikiTreeProvider);
  final r = await ref.read(apiClientProvider).get('/api/wiki/stats');
  return r is Map ? r.cast<String, dynamic>() : <String, dynamic>{};
});

final wikiSearchProvider = FutureProvider<List<WikiHit>>((ref) async {
  final q = ref.watch(wikiQueryProvider).trim();
  final tag = ref.watch(wikiTagProvider);
  if (q.isEmpty && tag == null) return const [];
  final r = await ref.read(apiClientProvider).get('/api/wiki/search', query: {
    if (q.isNotEmpty) 'q': q,
    'tags': ?tag,
  });
  final results = (r is Map ? r['results'] : r) as List? ?? const [];
  return results
      .whereType<Map>()
      .map((m) => WikiHit.fromJson(m.cast<String, dynamic>()))
      .toList();
});

class WikiScreen extends ConsumerWidget {
  const WikiScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final tree = ref.watch(wikiTreeProvider);
    final selected = ref.watch(wikiSelectedProvider);
    final query = ref.watch(wikiQueryProvider);
    final tag = ref.watch(wikiTagProvider);
    final tags = ref.watch(wikiTagsProvider);
    final search = ref.watch(wikiSearchProvider);

    return Row(
      children: [
        SizedBox(
          width: 280,
          child: Container(
            color: c.sidebar,
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Padding(
                  padding: const EdgeInsets.fromLTRB(AppTokens.s16,
                      AppTokens.s16, AppTokens.s16, AppTokens.s8),
                  child: Row(
                    children: [
                      Text('Wiki',
                          style: TextStyle(
                              color: c.textPrimary,
                              fontSize: 16,
                              fontWeight: FontWeight.w700)),
                      const Spacer(),
                      IconButton(
                        tooltip: context.tr('New folder'),
                        icon: const Icon(Icons.create_new_folder_outlined,
                            size: 18),
                        onPressed: () async {
                          final ctrl = TextEditingController();
                          final ok = await showDialog<bool>(
                            context: context,
                            builder: (dctx) => AlertDialog(
                              backgroundColor: dctx.colors.surface,
                              title: Text(dctx.tr('New folder')),
                              content: TextField(
                                controller: ctrl,
                                autofocus: true,
                                decoration: InputDecoration(
                                    labelText: dctx.tr('Folder path'),
                                    hintText: 'category/subfolder'),
                              ),
                              actions: [
                                TextButton(
                                    onPressed: () =>
                                        Navigator.pop(dctx, false),
                                    child: Text(dctx.tr('Cancel'))),
                                FilledButton(
                                    onPressed: () =>
                                        Navigator.pop(dctx, true),
                                    child: Text(dctx.tr('Create'))),
                              ],
                            ),
                          );
                          if (ok == true && ctrl.text.trim().isNotEmpty) {
                            await ref.read(apiClientProvider).post(
                                '/api/wiki/mkdir',
                                body: {'path': ctrl.text.trim()});
                            ref.invalidate(wikiTreeProvider);
                          }
                        },
                      ),
                      IconButton(
                        tooltip: context.tr('New page'),
                        icon: const Icon(Icons.note_add_outlined, size: 18),
                        onPressed: () => showDialog(
                            context: context,
                            builder: (_) => const _NewPageDialog()),
                      ),
                      IconButton(
                        tooltip: context.tr('Upload file'),
                        icon: const Icon(Icons.upload_file_outlined, size: 18),
                        onPressed: () => _uploadWikiFile(ref),
                      ),
                      IconButton(
                        tooltip: context.tr('Reload'),
                        icon: const Icon(Icons.refresh, size: 18),
                        onPressed: () => ref.invalidate(wikiTreeProvider),
                      ),
                    ],
                  ),
                ),
                // Knowledge stats — pages · folders · tags (web WikiStats).
                ref.watch(wikiStatsProvider).maybeWhen(
                      orElse: () => const SizedBox.shrink(),
                      data: (s) {
                        final pages = s['totalFiles'] ?? 0;
                        final dirs = s['totalDirs'] ?? 0;
                        final tagCount = (s['byTag'] as List?)?.length ?? 0;
                        return Padding(
                          padding: const EdgeInsets.fromLTRB(AppTokens.s16, 0,
                              AppTokens.s16, AppTokens.s8),
                          child: DefaultTextStyle(
                            style: TextStyle(color: c.textMuted, fontSize: 11),
                            child: Wrap(spacing: AppTokens.s12, children: [
                              Text(context.trArgs('{n} pages', {'n': pages})),
                              Text(context.trArgs('{n} folders', {'n': dirs})),
                              if (tagCount > 0)
                                Text(context
                                    .trArgs('{n} tags', {'n': tagCount})),
                            ]),
                          ),
                        );
                      },
                    ),
                Padding(
                  padding: const EdgeInsets.fromLTRB(
                      AppTokens.s12, 0, AppTokens.s12, AppTokens.s8),
                  child: TextField(
                    decoration: InputDecoration(
                      hintText: context.tr('Search wiki…'),
                      prefixIcon: const Icon(Icons.search, size: 16),
                    ),
                    onChanged: (v) =>
                        ref.read(wikiQueryProvider.notifier).state = v,
                  ),
                ),
                // Tag cloud — click to filter by tag.
                tags.maybeWhen(
                  orElse: () => const SizedBox.shrink(),
                  data: (list) => list.isEmpty
                      ? const SizedBox.shrink()
                      : Padding(
                          padding: const EdgeInsets.fromLTRB(
                              AppTokens.s12, 0, AppTokens.s12, AppTokens.s8),
                          child: Wrap(
                            spacing: AppTokens.s6,
                            runSpacing: AppTokens.s6,
                            children: [
                              for (final tg in list)
                                GestureDetector(
                                  onTap: () => ref
                                      .read(wikiTagProvider.notifier)
                                      .state = tag == tg ? null : tg,
                                  child: Container(
                                    padding: const EdgeInsets.symmetric(
                                        horizontal: 8, vertical: 2),
                                    decoration: BoxDecoration(
                                      color: tag == tg
                                          ? c.accent
                                          : c.surfaceAlt,
                                      borderRadius:
                                          BorderRadius.circular(AppTokens.rXl),
                                      border: Border.all(color: c.border),
                                    ),
                                    child: Text('#$tg',
                                        style: TextStyle(
                                            color: tag == tg
                                                ? Colors.white
                                                : c.textSecondary,
                                            fontSize: 11)),
                                  ),
                                ),
                            ],
                          ),
                        ),
                ),
                Expanded(
                  child: (query.trim().isNotEmpty || tag != null)
                      ? search.when(
                          loading: () =>
                              const Center(child: CircularProgressIndicator()),
                          error: (e, _) => Center(child: Text('$e')),
                          data: (hits) => hits.isEmpty
                              ? Center(
                                  child: Text(context.tr('No results'),
                                      style: TextStyle(color: c.textMuted)))
                              : ListView.builder(
                                  itemCount: hits.length,
                                  itemBuilder: (_, i) {
                                    final h = hits[i];
                                    return InkWell(
                                      onTap: () => ref
                                          .read(wikiSelectedProvider.notifier)
                                          .state = h.path,
                                      child: Container(
                                        padding: const EdgeInsets.symmetric(
                                            horizontal: AppTokens.s12,
                                            vertical: AppTokens.s8),
                                        margin: const EdgeInsets.symmetric(
                                            horizontal: AppTokens.s8,
                                            vertical: 1),
                                        child: Column(
                                          crossAxisAlignment:
                                              CrossAxisAlignment.start,
                                          children: [
                                            Text(h.title,
                                                maxLines: 1,
                                                overflow: TextOverflow.ellipsis,
                                                style: TextStyle(
                                                    color: c.textPrimary,
                                                    fontWeight:
                                                        FontWeight.w600,
                                                    fontSize: 14)),
                                            if (h.snippet.isNotEmpty)
                                              Text(h.snippet,
                                                  maxLines: 2,
                                                  overflow:
                                                      TextOverflow.ellipsis,
                                                  style: TextStyle(
                                                      color: c.textMuted,
                                                      fontSize: 12)),
                                          ],
                                        ),
                                      ),
                                    );
                                  },
                                ),
                        )
                      : tree.when(
                          loading: () =>
                              const Center(child: CircularProgressIndicator()),
                          error: (e, _) => Center(child: Text('$e')),
                          data: (nodes) => ListView(
                            padding:
                                const EdgeInsets.only(bottom: AppTokens.s12),
                            children: [
                              for (final n in nodes)
                                _TreeTile(node: n, depth: 0),
                            ],
                          ),
                        ),
                ),
              ],
            ),
          ),
        ),
        Container(width: 1, color: c.border),
        Expanded(
          child: selected == null
              ? Center(
                  child: Text(context.tr('Select a document'),
                      style: TextStyle(color: c.textMuted)),
                )
              : _FileView(path: selected),
        ),
      ],
    );
  }
}

class _TreeTile extends ConsumerStatefulWidget {
  const _TreeTile({required this.node, required this.depth});
  final WikiNode node;
  final int depth;
  @override
  ConsumerState<_TreeTile> createState() => _TreeTileState();
}

class _TreeTileState extends ConsumerState<_TreeTile> {
  bool _open = false;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final n = widget.node;
    final selected = ref.watch(wikiSelectedProvider) == n.path ||
        ref.watch(wikiContextTargetProvider) == n.path;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        GestureDetector(
          onSecondaryTapDown: (d) =>
              _showNodeMenu(context, ref, n, d.globalPosition),
          child: InkWell(
          onTap: () {
            if (n.isDir) {
              setState(() => _open = !_open);
            } else {
              ref.read(wikiSelectedProvider.notifier).state = n.path;
            }
          },
          child: Container(
            padding: EdgeInsets.only(
                left: AppTokens.s12 + widget.depth * 14,
                right: AppTokens.s12,
                top: 6,
                bottom: 6),
            color: selected ? c.accentSoft : null,
            child: Row(
              children: [
                // Expand/collapse affordance (folders only); files get a
                // matching gap so names line up.
                SizedBox(
                  width: 16,
                  child: n.isDir
                      ? Icon(
                          _open ? Icons.expand_more : Icons.chevron_right,
                          size: 16,
                          color: c.textMuted,
                        )
                      : null,
                ),
                Icon(
                  n.isDir
                      ? (_open ? Icons.folder_open : Icons.folder_outlined)
                      : Icons.description_outlined,
                  size: 15,
                  color: selected ? c.accent : c.textMuted,
                ),
                const SizedBox(width: AppTokens.s8),
                Expanded(
                  child: Text(n.name,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(
                          color: selected ? c.accent : c.textPrimary,
                          fontSize: 14)),
                ),
              ],
            ),
          ),
        ),
        ),
        if (n.isDir && _open)
          for (final child in n.children)
            _TreeTile(node: child, depth: widget.depth + 1),
      ],
    );
  }
}

class _FileView extends ConsumerStatefulWidget {
  const _FileView({required this.path});
  final String path;
  @override
  ConsumerState<_FileView> createState() => _FileViewState();
}

class _FileViewState extends ConsumerState<_FileView> {
  bool _editing = false;
  bool _saving = false;
  final _ctrl = TextEditingController();

  @override
  void dispose() {
    _ctrl.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    setState(() => _saving = true);
    try {
      await ref.read(apiClientProvider).put('/api/wiki/file', body: {
        'path': widget.path,
        'content': _ctrl.text,
        'source': 'manual',
      });
      ref.invalidate(wikiFileProvider(widget.path));
      ref.invalidate(wikiTreeProvider);
      if (mounted) setState(() => _editing = false);
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final content = ref.watch(wikiFileProvider(widget.path));
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Container(
          padding: const EdgeInsets.symmetric(
              horizontal: AppTokens.s16, vertical: AppTokens.s8),
          decoration: BoxDecoration(
            border: Border(bottom: BorderSide(color: c.border)),
          ),
          child: Row(
            children: [
              Expanded(
                child: Text(widget.path,
                    style: TextStyle(
                        color: c.textSecondary,
                        fontSize: 14,
                        fontFamily: AppTokens.fontMono)),
              ),
              if (_editing) ...[
                TextButton(
                    onPressed: _saving
                        ? null
                        : () => setState(() => _editing = false),
                    child: Text(context.tr('Cancel'))),
                FilledButton(
                    onPressed: _saving ? null : _save,
                    child: Text(context.tr('Save'))),
              ] else ...[
                IconButton(
                  tooltip: context.tr('History'),
                  icon: const Icon(Icons.history, size: 18),
                  onPressed: () => showDialog(
                      context: context,
                      builder: (_) => _HistoryDialog(path: widget.path)),
                ),
                IconButton(
                  tooltip: context.tr('Edit'),
                  icon: const Icon(Icons.edit_outlined, size: 18),
                  onPressed: () {
                    _ctrl.text = content.valueOrNull ?? '';
                    setState(() => _editing = true);
                  },
                ),
              ],
            ],
          ),
        ),
        Expanded(
          child: content.when(
            loading: () => const Center(child: CircularProgressIndicator()),
            error: (e, _) => Center(child: Text('$e')),
            data: (text) => _editing
                ? Padding(
                    padding: const EdgeInsets.all(AppTokens.s16),
                    child: TextField(
                      controller: _ctrl,
                      expands: true,
                      maxLines: null,
                      textAlignVertical: TextAlignVertical.top,
                      style: TextStyle(
                          fontFamily: AppTokens.fontMono,
                          fontSize: 14,
                          color: c.textPrimary),
                      decoration: InputDecoration(
                          hintText:
                              context.tr('Markdown (with frontmatter)…')),
                    ),
                  )
                : SelectionArea(
                    child: SingleChildScrollView(
                      padding: const EdgeInsets.all(AppTokens.s24),
                      child: AppMarkdown(_stripFrontmatter(text),
                          style:
                              TextStyle(color: c.textSecondary, height: 1.6)),
                    ),
                  ),
          ),
        ),
      ],
    );
  }

  String _stripFrontmatter(String s) {
    if (s.startsWith('---')) {
      final end = s.indexOf('\n---', 3);
      if (end != -1) return s.substring(s.indexOf('\n', end + 1) + 1);
    }
    return s;
  }
}

/// Create a new wiki page: path + initial content → PUT /api/wiki/file.
class _NewPageDialog extends ConsumerStatefulWidget {
  const _NewPageDialog();
  @override
  ConsumerState<_NewPageDialog> createState() => _NewPageDialogState();
}

class _NewPageDialogState extends ConsumerState<_NewPageDialog> {
  final _path = TextEditingController(text: 'notes/new-page.md');
  final _content = TextEditingController(text: '# New page\n\n');
  bool _saving = false;
  String? _error;

  @override
  void dispose() {
    _path.dispose();
    _content.dispose();
    super.dispose();
  }

  Future<void> _create() async {
    var path = _path.text.trim();
    if (path.isEmpty) {
      setState(() => _error = context.tr('Path is required'));
      return;
    }
    if (!path.endsWith('.md')) path = '$path.md';
    setState(() {
      _saving = true;
      _error = null;
    });
    try {
      await ref.read(apiClientProvider).put('/api/wiki/file',
          body: {'path': path, 'content': _content.text});
      ref.invalidate(wikiTreeProvider);
      ref.read(wikiSelectedProvider.notifier).state = path;
      if (mounted) Navigator.pop(context);
    } catch (e) {
      setState(() => _error = '$e');
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return AlertDialog(
      backgroundColor: c.surface,
      title: Text(context.tr('New wiki page')),
      content: SizedBox(
        width: 520,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            TextField(
              controller: _path,
              decoration: InputDecoration(
                  labelText: context.tr('Path'),
                  hintText: 'category/page-name.md'),
            ),
            const SizedBox(height: AppTokens.s8),
            TextField(
              controller: _content,
              minLines: 5,
              maxLines: 10,
              style: TextStyle(fontFamily: AppTokens.fontMono, fontSize: 12),
              decoration: InputDecoration(
                  labelText: context.tr('Content (Markdown)'),
                  alignLabelWithHint: true),
            ),
            if (_error != null)
              Padding(
                padding: const EdgeInsets.only(top: AppTokens.s8),
                child: Text(_error!,
                    style: const TextStyle(color: AppTokens.danger)),
              ),
          ],
        ),
      ),
      actions: [
        TextButton(
            onPressed: () => Navigator.pop(context),
            child: Text(context.tr('Cancel'))),
        FilledButton(
          onPressed: _saving ? null : _create,
          child: _saving
              ? const SizedBox(
                  width: 14,
                  height: 14,
                  child: CircularProgressIndicator(strokeWidth: 2))
              : Text(context.tr('Create')),
        ),
      ],
    );
  }
}

/// Git history for a wiki page: GET /api/wiki/history?path= → {commits:[{hash,date,message}]}.
class _HistoryDialog extends ConsumerWidget {
  const _HistoryDialog({required this.path});
  final String path;
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    return Dialog(
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 560, maxHeight: 560),
        child: Padding(
          padding: const EdgeInsets.all(AppTokens.s20),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Row(children: [
                Icon(Icons.history, size: 18, color: c.accent),
                const SizedBox(width: AppTokens.s8),
                Expanded(
                  child: Text(
                      context.trArgs('History · {path}', {'path': path}),
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(
                          color: c.textPrimary,
                          fontSize: 15,
                          fontWeight: FontWeight.w700)),
                ),
                IconButton(
                    icon: const Icon(Icons.close, size: 18),
                    onPressed: () => Navigator.pop(context)),
              ]),
              const SizedBox(height: AppTokens.s12),
              Expanded(
                child: FutureBuilder(
                  future: ref.read(apiClientProvider).get('/api/wiki/history',
                      query: {'path': path, 'limit': '50'}),
                  builder: (_, snap) {
                    if (!snap.hasData) {
                      return const Center(child: CircularProgressIndicator());
                    }
                    final r = snap.data;
                    final commits =
                        (r is Map ? r['commits'] : null) as List? ?? const [];
                    if (commits.isEmpty) {
                      return Center(
                          child: Text(context.tr('No history'),
                              style: TextStyle(color: c.textMuted)));
                    }
                    return ListView.separated(
                      itemCount: commits.length,
                      separatorBuilder: (_, _) => Divider(color: c.border),
                      itemBuilder: (_, i) {
                        final m = (commits[i] as Map).cast<String, dynamic>();
                        return Padding(
                          padding: const EdgeInsets.symmetric(vertical: 4),
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              Text('${m['message'] ?? ''}',
                                  style: TextStyle(
                                      color: c.textPrimary, fontSize: 13)),
                              const SizedBox(height: 2),
                              Text(
                                  '${m['hash']?.toString().substring(0, 7) ?? ''} · ${m['date'] ?? ''}',
                                  style: TextStyle(
                                      color: c.textMuted,
                                      fontSize: 11,
                                      fontFamily: AppTokens.fontMono)),
                            ],
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
    );
  }
}
