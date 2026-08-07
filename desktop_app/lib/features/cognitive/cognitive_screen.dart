import 'package:http/http.dart' as http;
import 'package:flutter/foundation.dart' show kIsWeb;
import 'package:file_picker/file_picker.dart';
import 'dart:io' show File;
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/i18n/l10n.dart';
import '../../core/transport/connection.dart';
import '../../theme/tokens.dart';
import 'cognitive_graph.dart';

class CogNode {
  final String id;
  final String kind;
  final String name;
  final String summary;
  const CogNode(this.id, this.kind, this.name, this.summary);

  factory CogNode.fromJson(Map<String, dynamic> j) => CogNode(
    '${j['id'] ?? ''}',
    '${j['kind'] ?? ''}',
    '${j['name'] ?? ''}',
    '${j['summary'] ?? ''}',
  );

  String get label => name.isNotEmpty
      ? name
      : (summary.isNotEmpty ? summary : id).split('\n').first;
}

final cogStatsProvider = FutureProvider<Map<String, dynamic>>((ref) async {
  final r = await ref.read(apiClientProvider).get('/api/cognitive/stats');
  return r is Map ? r.cast<String, dynamic>() : {};
});

final cogQueryProvider = StateProvider<String>((ref) => '');

/// Selected knowledge space (custom scope id). null = all knowledge.
final cogSpaceProvider = StateProvider<String?>((ref) => null);

class CogSpace {
  final String scopeKind;
  final String scopeId;
  final String tag;
  final int nodes;
  const CogSpace(this.scopeKind, this.scopeId, this.tag, this.nodes);

  /// Display label, e.g. `ai-office:nghien-cuu (12)`.
  String get label => scopeId.isEmpty ? tag : scopeId;
}

/// Registry of knowledge spaces (custom scopes) — e.g. one private space per
/// AI-Office staff member. Global/group tags are filtered out; only named
/// spaces are switchable here.
final cogSpacesProvider = FutureProvider<List<CogSpace>>((ref) async {
  final r = await ref.read(apiClientProvider).get('/api/cognitive/spaces');
  final spaces = (r is Map ? r['spaces'] : r) as List? ?? const [];
  return spaces
      .whereType<Map>()
      .map((m) => CogSpace('${m['scopeKind'] ?? ''}', '${m['scopeId'] ?? ''}',
          '${m['tag'] ?? ''}', (m['nodes'] as num?)?.toInt() ?? 0))
      .where((s) => s.scopeKind == 'custom' && s.scopeId.isNotEmpty)
      .toList();
});

/// Top nodes when the query is empty; semantic search results otherwise.
/// Both respect the selected knowledge space.
final cogNodesProvider = FutureProvider<List<CogNode>>((ref) async {
  final q = ref.watch(cogQueryProvider).trim();
  final space = ref.watch(cogSpaceProvider);
  final api = ref.read(apiClientProvider);
  if (q.isEmpty) {
    final r = await api.get('/api/cognitive/top-nodes', query: {
      'limit': 60,
      if (space != null && space.isNotEmpty) 'space': space,
    });
    final nodes = (r is Map ? r['nodes'] : r) as List? ?? const [];
    return nodes.whereType<Map>().map((m) {
      final n = (m['node'] ?? m) as Map;
      return CogNode.fromJson(n.cast<String, dynamic>());
    }).toList();
  }
  final r = await api.post('/api/cognitive/search', body: {
    'query': q,
    'limit': 40,
    if (space != null && space.isNotEmpty) 'space': space,
  });
  final hits = (r is Map ? r['hits'] : r) as List? ?? const [];
  return hits.whereType<Map>().map((m) {
    final n = (m['node'] ?? m) as Map;
    return CogNode.fromJson(n.cast<String, dynamic>());
  }).toList();
});

class CognitiveScreen extends ConsumerStatefulWidget {
  const CognitiveScreen({super.key});
  @override
  ConsumerState<CognitiveScreen> createState() => _CognitiveScreenState();
}

class _CognitiveScreenState extends ConsumerState<CognitiveScreen> {
  CogNode? _selected;
  bool _showGraph = false;
  String _tab = 'graph'; // graph | data — default to the graph view

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final stats = ref.watch(cogStatsProvider);
    final nodes = ref.watch(cogNodesProvider);

    return Column(
      children: [
        // Stats header
        Container(
          padding: const EdgeInsets.all(AppTokens.s16),
          decoration: BoxDecoration(
            border: Border(bottom: BorderSide(color: c.border)),
          ),
          child: Row(
            children: [
              Tooltip(
                message: context.tr(
                    'User info aggregated from chats · extended with '
                    'uploaded documents · Recall researches it for detailed, '
                    'grounded answers'),
                child: Text(context.tr('Knowledge'),
                    style: TextStyle(
                        color: c.textPrimary,
                        fontSize: 16,
                        fontWeight: FontWeight.w700)),
              ),
              const SizedBox(width: AppTokens.s24),
              stats.when(
                loading: () => const SizedBox.shrink(),
                error: (e, st) => const SizedBox.shrink(),
                data: (s) => Row(children: [
                  _stat('${s['nodes_total'] ?? 0}', context.tr('nodes'),
                      AppTokens.brand),
                  const SizedBox(width: AppTokens.s16),
                  _stat('${s['edges'] ?? 0}', context.tr('edges'),
                      AppTokens.brandAlt),
                  const SizedBox(width: AppTokens.s16),
                  for (final kv in (s['nodes_by_kind'] as List? ?? const []))
                    if (kv is List && kv.length == 2)
                      Padding(
                        padding: const EdgeInsets.only(right: AppTokens.s12),
                        child: _stat('${kv[1]}', '${kv[0]}', AppTokens.cyan),
                      ),
                ]),
              ),
              const SizedBox(width: AppTokens.s16),
              SegmentedButton<String>(
                style:
                    const ButtonStyle(visualDensity: VisualDensity.compact),
                segments: [
                  ButtonSegment(
                      value: 'graph',
                      icon: const Icon(Icons.hub_outlined, size: 15),
                      label: Text(context.tr('Graph'))),
                  ButtonSegment(
                      value: 'data',
                      icon: const Icon(Icons.list_alt_outlined, size: 15),
                      label: Text(context.tr('Data'))),
                ],
                selected: {_tab},
                onSelectionChanged: (s) => setState(() => _tab = s.first),
              ),
              const SizedBox(width: AppTokens.s12),
              // Knowledge-space switcher: All ↔ one isolated space (e.g. a
              // single AI-Office staff member's private memory).
              ref.watch(cogSpacesProvider).maybeWhen(
                    data: (spaces) => spaces.isEmpty
                        ? const SizedBox.shrink()
                        : DropdownButton<String?>(
                            value: ref.watch(cogSpaceProvider),
                            underline: const SizedBox.shrink(),
                            isDense: true,
                            style: TextStyle(
                                color: c.textPrimary, fontSize: 12.5),
                            items: [
                              DropdownMenuItem<String?>(
                                  value: null,
                                  child:
                                      Text(context.tr('🌐 All knowledge'))),
                              for (final s in spaces)
                                DropdownMenuItem<String?>(
                                    value: s.scopeId,
                                    child: Text(
                                        '🔒 ${s.label} (${s.nodes})')),
                            ],
                            onChanged: (v) => ref
                                .read(cogSpaceProvider.notifier)
                                .state = v,
                          ),
                    orElse: () => const SizedBox.shrink(),
                  ),
              const Spacer(),
              // Primary actions stand out (tonal); maintenance ops fold into ⋯.
              FilledButton.tonalIcon(
                onPressed: () => showDialog(
                    context: context, builder: (_) => const _RecallDialog()),
                icon: const Icon(Icons.lightbulb_outline, size: 16),
                label: const Text('Recall'),
              ),
              const SizedBox(width: AppTokens.s8),
              FilledButton.tonalIcon(
                onPressed: () => showDialog(
                    context: context, builder: (_) => const _AddMemoryDialog()),
                icon: const Icon(Icons.cloud_upload_outlined, size: 16),
                label: Text(context.tr('Add knowledge')),
              ),
              const SizedBox(width: AppTokens.s4),
              PopupMenuButton<String>(
                tooltip: context.tr('Maintenance'),
                icon: Icon(Icons.more_horiz, color: c.textMuted),
                position: PopupMenuPosition.under,
                onSelected: (v) {
                  switch (v) {
                    case 'maintenance':
                      _runOp(context, ref, 'maintenance',
                          context.tr('Maintenance run'));
                    case 'cleanup':
                      _runOp(context, ref, 'cleanup', context.tr('Cleanup done'));
                    case 'backfill':
                      _runOp(context, ref, 're-extract-pending',
                          context.tr('Backfill started'));
                    case 'decay':
                      showDialog(
                          context: context,
                          builder: (_) => const _DecayLogDialog());
                  }
                },
                itemBuilder: (_) => [
                  PopupMenuItem(
                    value: 'maintenance',
                    child: Row(children: [
                      const Icon(Icons.cleaning_services_outlined, size: 16),
                      const SizedBox(width: AppTokens.s8),
                      Text(context.tr('Maintain')),
                    ]),
                  ),
                  PopupMenuItem(
                    value: 'backfill',
                    child: Row(children: [
                      const Icon(Icons.replay_outlined, size: 16),
                      const SizedBox(width: AppTokens.s8),
                      Text(context.tr('Re-extract pending')),
                    ]),
                  ),
                  PopupMenuItem(
                    value: 'cleanup',
                    child: Row(children: [
                      const Icon(Icons.auto_delete_outlined, size: 16),
                      const SizedBox(width: AppTokens.s8),
                      Text(context.tr('Cleanup')),
                    ]),
                  ),
                  PopupMenuItem(
                    value: 'decay',
                    child: Row(children: [
                      const Icon(Icons.history, size: 16),
                      const SizedBox(width: AppTokens.s8),
                      Text(context.tr('Decay log')),
                    ]),
                  ),
                ],
              ),
              IconButton(
                tooltip: context.tr('Reload'),
                icon: const Icon(Icons.refresh, size: 18),
                onPressed: () {
                  ref.invalidate(cogStatsProvider);
                  ref.invalidate(cogNodesProvider);
                },
              ),
            ],
          ),
        ),
        Expanded(
          child: _tab == 'graph'
              ? CogGraphExplorer(
                  onNodeTap: (gn) => setState(() {
                    _selected =
                        CogNode(gn.id, gn.kind, gn.name, gn.summary);
                    _tab = 'data';
                  }),
                )
              : Row(
            children: [
              SizedBox(
                width: 360,
                child: Container(
                  color: c.sidebar,
                  child: Column(
                    children: [
                      Padding(
                        padding: const EdgeInsets.all(AppTokens.s12),
                        child: TextField(
                          decoration: InputDecoration(
                            hintText: context.tr('Search knowledge…'),
                            prefixIcon: const Icon(Icons.search, size: 16),
                          ),
                          onSubmitted: (v) => ref
                              .read(cogQueryProvider.notifier)
                              .state = v,
                        ),
                      ),
                      Expanded(
                        child: nodes.when(
                          loading: () => const Center(
                              child: CircularProgressIndicator()),
                          error: (e, _) => Center(child: Text('$e')),
                          data: (list) => list.isEmpty
                              ? Center(
                                  child: Text(context.tr('No nodes'),
                                      style:
                                          TextStyle(color: c.textMuted)))
                              : ListView.builder(
                                  itemCount: list.length,
                                  itemBuilder: (_, i) {
                                    final n = list[i];
                                    final sel = n.id == _selected?.id;
                                    return InkWell(
                                      onTap: () =>
                                          setState(() => _selected = n),
                                      child: Container(
                                        padding: const EdgeInsets.symmetric(
                                            horizontal: AppTokens.s12,
                                            vertical: AppTokens.s8),
                                        margin: const EdgeInsets.symmetric(
                                            horizontal: AppTokens.s8,
                                            vertical: 1),
                                        decoration: BoxDecoration(
                                          color: sel
                                              ? c.accentSoft
                                              : Colors.transparent,
                                          borderRadius: BorderRadius.circular(
                                              AppTokens.rMd),
                                        ),
                                        child: Row(
                                          children: [
                                            _kindChip(n.kind),
                                            const SizedBox(
                                                width: AppTokens.s8),
                                            Expanded(
                                              child: Text(n.label,
                                                  maxLines: 1,
                                                  overflow:
                                                      TextOverflow.ellipsis,
                                                  style: TextStyle(
                                                      color: c.textPrimary,
                                                      fontSize: 14)),
                                            ),
                                          ],
                                        ),
                                      ),
                                    );
                                  },
                                ),
                        ),
                      ),
                    ],
                  ),
                ),
              ),
              Container(width: 1, color: c.border),
              Expanded(
                child: _selected == null
                    ? Center(
                        child: Text(context.tr('Select a node'),
                            style: TextStyle(color: c.textMuted)))
                    : SingleChildScrollView(
                        padding: const EdgeInsets.all(AppTokens.s24),
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            // Header: kind + concise title on the left, the
                            // action buttons grouped on the right (top-aligned
                            // so a long title doesn't push them to the middle).
                            Row(
                              crossAxisAlignment: CrossAxisAlignment.start,
                              children: [
                                _kindChip(_selected!.kind),
                                const SizedBox(width: AppTokens.s8),
                                Expanded(
                                  child: Padding(
                                    padding:
                                        const EdgeInsets.only(top: 2),
                                    child: Text(
                                      _selected!.label,
                                      maxLines: 2,
                                      overflow: TextOverflow.ellipsis,
                                      style: TextStyle(
                                          color: c.textPrimary,
                                          fontSize: 14,
                                          fontWeight: FontWeight.w700,
                                          height: 1.35),
                                    ),
                                  ),
                                ),
                                const SizedBox(width: AppTokens.s8),
                                _NodeAction(
                                  icon: Icons.hub_outlined,
                                  label: context.tr('Graph'),
                                  color: _showGraph ? c.accent : null,
                                  onTap: () => setState(
                                      () => _showGraph = !_showGraph),
                                ),
                                _NodeAction(
                                  icon: Icons.refresh,
                                  label: context.tr('Re-extract'),
                                  onTap: () async {
                                    await ref.read(apiClientProvider).post(
                                        '/api/cognitive/node/${_selected!.id}/re-extract');
                                    ref.invalidate(cogNodesProvider);
                                  },
                                ),
                                _NodeAction(
                                  icon: Icons.delete_outline,
                                  label: context.tr('Forget'),
                                  color: AppTokens.danger,
                                  onTap: () async {
                                    await ref.read(apiClientProvider).delete(
                                        '/api/cognitive/node/${_selected!.id}');
                                    ref.invalidate(cogStatsProvider);
                                    ref.invalidate(cogNodesProvider);
                                    setState(() => _selected = null);
                                  },
                                ),
                              ],
                            ),
                            const SizedBox(height: AppTokens.s12),
                            Divider(height: 1, color: c.border),
                            const SizedBox(height: AppTokens.s16),
                            // Full content, shown once (summary, else label).
                            SelectableText(
                              _selected!.summary.trim().isNotEmpty
                                  ? _selected!.summary
                                  : (_selected!.label.trim().isEmpty
                                      ? context.tr('(no content)')
                                      : _selected!.label),
                              style: TextStyle(
                                  color: c.textSecondary,
                                  fontSize: 13,
                                  height: 1.6),
                            ),
                            if (_showGraph) ...[
                              const SizedBox(height: AppTokens.s16),
                              Container(
                                height: 440,
                                decoration: BoxDecoration(
                                  color: c.sidebar,
                                  borderRadius:
                                      BorderRadius.circular(AppTokens.rMd),
                                  border: Border.all(color: c.border),
                                ),
                                clipBehavior: Clip.antiAlias,
                                child: CogGraphView(
                                  seedId: _selected!.id,
                                  onNodeTap: (gn) => setState(() => _selected =
                                      CogNode(gn.id, gn.kind, gn.name,
                                          gn.summary)),
                                ),
                              ),
                            ],
                          ],
                        ),
                      ),
              ),
            ],
          ),
        ),
      ],
    );
  }

  Widget _stat(String value, String label, Color color) => Row(
        children: [
          Text(value,
              style: TextStyle(
                  color: color, fontWeight: FontWeight.w700, fontSize: 16)),
          const SizedBox(width: 4),
          Text(label,
              style: TextStyle(color: context.colors.textMuted, fontSize: 12)),
        ],
      );

  Future<void> _runOp(
      BuildContext context, WidgetRef ref, String op, String msg) async {
    final r = await ref.read(apiClientProvider).post('/api/cognitive/$op');
    ref.invalidate(cogStatsProvider);
    ref.invalidate(cogNodesProvider);
    if (!context.mounted) return;
    // Cleanup reports how much junk it swept; backfill how many chunks it
    // queued — surface whichever count came back.
    var text = msg;
    if (r is Map && r['total_removed'] is num) {
      text = '$msg — '
          '${context.trArgs('removed {n} node(s)', {'n': r['total_removed']})}';
    } else if (r is Map && r['queued'] is num) {
      text = '$msg — '
          '${context.trArgs('{n} chunk(s) queued', {'n': r['queued']})}';
    }
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(text)));
  }

  Widget _kindChip(String kind) {
    final c = context.colors;
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
      decoration: BoxDecoration(
        color: c.surfaceAlt,
        borderRadius: BorderRadius.circular(AppTokens.rSm),
        border: Border.all(color: c.border),
      ),
      child: Text(kind,
          style: TextStyle(color: c.textMuted, fontSize: 11)),
    );
  }
}

/// Add a free-text memory (POST /api/cognitive/add {text, tags}).
class _AddMemoryDialog extends ConsumerStatefulWidget {
  const _AddMemoryDialog();
  @override
  ConsumerState<_AddMemoryDialog> createState() => _AddMemoryDialogState();
}

class _AddMemoryDialogState extends ConsumerState<_AddMemoryDialog> {
  final _text = TextEditingController();
  final _tags = TextEditingController();
  bool _busy = false;

  @override
  void dispose() {
    _text.dispose();
    _tags.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    if (_text.text.trim().isEmpty) return;
    setState(() => _busy = true);
    final tags = _tags.text
        .split(',')
        .map((t) => t.trim())
        .where((t) => t.isNotEmpty)
        .toList();
    try {
      final space = ref.read(cogSpaceProvider);
      await ref.read(apiClientProvider).post('/api/cognitive/add', body: {
        'text': _text.text.trim(),
        'tags': tags,
        if (space != null && space.isNotEmpty) 'space': space,
      });
      ref.invalidate(cogStatsProvider);
      ref.invalidate(cogNodesProvider);
      if (mounted) Navigator.of(context).pop();
    } catch (_) {
      if (mounted) setState(() => _busy = false);
    }
  }

  /// Upload a file → the daemon extracts text, chunks + ingests it into the
  /// graph (multipart POST /api/cognitive/upload, field `file`).
  Future<void> _uploadFile() async {
    final res = await FilePicker.platform.pickFiles(withData: kIsWeb);
    final f = res?.files.firstOrNull;
    if (f == null) return;
    setState(() => _busy = true);
    try {
      final cfg = ref.read(appConfigProvider);
      final uri =
          Uri.parse('http://${cfg.host}:${cfg.uiPort}/api/cognitive/upload');
      final req = http.MultipartRequest('POST', uri);
      req.headers.addAll(cfg.authHeaders);
      if (kIsWeb && f.bytes != null) {
        req.files.add(
            http.MultipartFile.fromBytes('file', f.bytes!, filename: f.name));
      } else if (f.path != null) {
        req.files.add(http.MultipartFile.fromBytes(
            'file', await File(f.path!).readAsBytes(),
            filename: f.name));
      } else {
        if (mounted) setState(() => _busy = false);
        return;
      }
      final resp = await req.send();
      final body = await resp.stream.bytesToString();
      ref.invalidate(cogStatsProvider);
      ref.invalidate(cogNodesProvider);
      if (!mounted) return;
      Navigator.of(context).pop();
      final ok = resp.statusCode >= 200 && resp.statusCode < 300;
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(
          content: Text(ok
              ? context.trArgs('{name}: added to knowledge', {'name': f.name})
              : context.trArgs('Upload failed: {err}', {'err': body}))));
    } catch (e) {
      if (mounted) {
        setState(() => _busy = false);
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
            content: Text(context.trArgs('Upload failed: {err}', {'err': e}))));
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return AlertDialog(
      backgroundColor: c.surface,
      title: Text(context.tr('Add knowledge')),
      content: SizedBox(
        width: 460,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            TextField(
              controller: _text,
              autofocus: true,
              minLines: 3,
              maxLines: 8,
              decoration:
                  InputDecoration(hintText: context.tr('What to remember…')),
            ),
            const SizedBox(height: AppTokens.s8),
            TextField(
                controller: _tags,
                decoration: InputDecoration(
                    hintText: context.tr('tags, comma, separated'))),
            const SizedBox(height: AppTokens.s12),
            Row(children: [
              Expanded(child: Divider(color: c.border)),
              Padding(
                padding:
                    const EdgeInsets.symmetric(horizontal: AppTokens.s8),
                child: Text(context.tr('or'),
                    style: TextStyle(color: c.textMuted, fontSize: 12)),
              ),
              Expanded(child: Divider(color: c.border)),
            ]),
            const SizedBox(height: AppTokens.s8),
            OutlinedButton.icon(
              onPressed: _busy ? null : _uploadFile,
              icon: const Icon(Icons.upload_file_outlined, size: 16),
              label: Text(context.tr('Upload a file')),
            ),
          ],
        ),
      ),
      actions: [
        TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: Text(context.tr('Cancel'))),
        FilledButton(
            onPressed: _busy ? null : _save,
            child: Text(context.tr('Add'))),
      ],
    );
  }
}

/// Recent cognitive decay/maintenance runs (GET /api/cognitive/decay-log).
class _DecayLogDialog extends ConsumerWidget {
  const _DecayLogDialog();
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final future = ref.read(apiClientProvider).get('/api/cognitive/decay-log');
    return Dialog(
      backgroundColor: c.surface,
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 560, maxHeight: 520),
        child: Padding(
          padding: const EdgeInsets.all(AppTokens.s20),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(context.tr('Decay log'),
                  style: TextStyle(
                      color: c.textPrimary,
                      fontSize: 16,
                      fontWeight: FontWeight.w700)),
              const SizedBox(height: AppTokens.s12),
              Expanded(
                child: FutureBuilder(
                  future: future,
                  builder: (_, snap) {
                    if (!snap.hasData) {
                      return const Center(child: CircularProgressIndicator());
                    }
                    final r = snap.data;
                    final runs = ((r is Map ? r['runs'] : r) as List? ?? const [])
                        .whereType<Map>()
                        .toList();
                    if (runs.isEmpty) {
                      return Center(
                          child: Text(context.tr('No runs yet'),
                              style: TextStyle(color: c.textMuted)));
                    }
                    return ListView.builder(
                      itemCount: runs.length,
                      itemBuilder: (_, i) {
                        final run = runs[i];
                        final at = (run['run_at'] as num?)?.toInt() ?? 0;
                        final when = at > 0
                            ? DateTime.fromMillisecondsSinceEpoch(at * 1000)
                                .toLocal()
                                .toString()
                                .substring(0, 19)
                            : '—';
                        return Padding(
                          padding: const EdgeInsets.symmetric(vertical: 4),
                          child: Row(
                            children: [
                              Expanded(
                                child: Text(when,
                                    style: TextStyle(
                                        color: c.textSecondary, fontSize: 12)),
                              ),
                              Text(
                                  context.trArgs(
                                      'scanned {scanned} · pruned {pruned} · '
                                      'promoted {promoted}',
                                      {
                                        'scanned': run['edges_scanned'] ?? 0,
                                        'pruned': run['edges_pruned'] ?? 0,
                                        'promoted': run['edges_promoted'] ?? 0,
                                      }),
                                  style: TextStyle(
                                      color: c.textMuted,
                                      fontSize: 12,
                                      fontFamily: AppTokens.fontMono)),
                            ],
                          ),
                        );
                      },
                    );
                  },
                ),
              ),
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

/// Compact icon+label action used in the node-detail header toolbar.
class _NodeAction extends StatelessWidget {
  const _NodeAction(
      {required this.icon,
      required this.label,
      required this.onTap,
      this.color});
  final IconData icon;
  final String label;
  final VoidCallback onTap;
  final Color? color;
  @override
  Widget build(BuildContext context) {
    final c = color ?? context.colors.textMuted;
    return Tooltip(
      message: label,
      child: InkWell(
        onTap: onTap,
        borderRadius: BorderRadius.circular(AppTokens.rMd),
        child: Padding(
          padding: const EdgeInsets.symmetric(
              horizontal: AppTokens.s8, vertical: AppTokens.s6),
          child: Row(mainAxisSize: MainAxisSize.min, children: [
            Icon(icon, size: 14, color: c),
            const SizedBox(width: AppTokens.s4),
            Text(label, style: TextStyle(color: c, fontSize: 12)),
          ]),
        ),
      ),
    );
  }
}

/// "Recall" — ask a question and synthesize a grounded answer from the memory
/// graph (POST /api/cognitive/recall → {answer, sources[]}). Mirrors the web
/// Cognitive "Recall (answer)" view.
class _RecallDialog extends ConsumerStatefulWidget {
  const _RecallDialog();
  @override
  ConsumerState<_RecallDialog> createState() => _RecallDialogState();
}

class _RecallDialogState extends ConsumerState<_RecallDialog> {
  final _query = TextEditingController();
  String _mode = 'hybrid';
  bool _loading = false;
  String _answer = '';
  List<dynamic> _sources = const [];
  String? _error;

  @override
  void dispose() {
    _query.dispose();
    super.dispose();
  }

  Future<void> _run() async {
    final q = _query.text.trim();
    if (q.isEmpty || _loading) return;
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final space = ref.read(cogSpaceProvider);
      final r = await ref.read(apiClientProvider).post('/api/cognitive/recall',
          body: {
            'query': q,
            'mode': _mode,
            'limit': 6,
            'hops': 2,
            if (space != null && space.isNotEmpty) 'space': space,
          });
      if (r is Map) {
        _answer = '${r['answer'] ?? ''}';
        _sources = (r['sources'] as List?) ?? const [];
      }
    } catch (e) {
      _error = '$e';
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return AlertDialog(
      backgroundColor: c.surface,
      title: Row(children: [
        const Icon(Icons.lightbulb_outline, size: 20),
        const SizedBox(width: AppTokens.s8),
        Text(context.tr('Recall — grounded answer')),
      ]),
      content: SizedBox(
        width: 540,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Row(children: [
                Expanded(
                  child: TextField(
                    controller: _query,
                    autofocus: true,
                    onSubmitted: (_) => _run(),
                    decoration: InputDecoration(
                        hintText: context.tr('Ask a question…'),
                        border: const OutlineInputBorder(),
                        isDense: true),
                  ),
                ),
                const SizedBox(width: AppTokens.s8),
                DropdownButton<String>(
                  value: _mode,
                  items: [
                    DropdownMenuItem(
                        value: 'hybrid',
                        child: Text(context.tr('Hybrid (vec+FTS)'))),
                    const DropdownMenuItem(
                        value: 'graph', child: Text('GraphCompletion')),
                    DropdownMenuItem(
                        value: 'fts', child: Text(context.tr('Keyword (FTS)'))),
                  ],
                  onChanged: (v) => setState(() => _mode = v ?? 'hybrid'),
                ),
                const SizedBox(width: AppTokens.s8),
                FilledButton(
                    onPressed: _loading ? null : _run,
                    child: Text(context.tr('Ask'))),
              ]),
              if (_loading) ...[
                const SizedBox(height: AppTokens.s12),
                const LinearProgressIndicator(),
              ],
              if (_error != null) ...[
                const SizedBox(height: AppTokens.s12),
                Text(_error!,
                    style: const TextStyle(color: AppTokens.danger)),
              ],
              if (_answer.isNotEmpty) ...[
                const SizedBox(height: AppTokens.s16),
                Container(
                  width: double.infinity,
                  padding: const EdgeInsets.all(AppTokens.s12),
                  decoration: BoxDecoration(
                    color: c.sidebar,
                    borderRadius: BorderRadius.circular(AppTokens.rMd),
                    border: Border.all(color: c.border),
                  ),
                  child: SelectableText(_answer,
                      style: TextStyle(color: c.textPrimary, height: 1.5)),
                ),
              ],
              if (_sources.isNotEmpty) ...[
                const SizedBox(height: AppTokens.s12),
                Text(
                    context
                        .trArgs('SOURCES ({n})', {'n': _sources.length}),
                    style: TextStyle(
                        color: c.textMuted,
                        fontSize: 11,
                        fontWeight: FontWeight.w700,
                        letterSpacing: 0.5)),
                const SizedBox(height: AppTokens.s4),
                for (final s in _sources)
                  if (s is Map)
                    Padding(
                      padding: const EdgeInsets.symmetric(vertical: 2),
                      child: Row(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text('${s['index'] ?? '•'}. ',
                              style: TextStyle(
                                  color: c.accent,
                                  fontSize: 12,
                                  fontWeight: FontWeight.w600)),
                          Expanded(
                            child: Text('${s['label'] ?? s['name'] ?? ''}',
                                maxLines: 2,
                                overflow: TextOverflow.ellipsis,
                                style: TextStyle(
                                    color: c.textSecondary, fontSize: 12)),
                          ),
                        ],
                      ),
                    ),
              ],
            ],
          ),
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
