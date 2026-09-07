import 'package:flutter/material.dart';
import '../../models/pattern_models.dart';
import '../../services/language_service.dart';
import '../../services/patterns_api.dart';
import '../../theme/tokens.dart';
import '../../widgets/states.dart';
import 'patterns_screen.dart' show sourceSubtitle;

/// Where patterns come from: the installed sources, plus the catalog of
/// one-tap offers for a daemon that has none yet.
class PatternSourcesTab extends StatefulWidget {
  final PatternsApi api;
  const PatternSourcesTab({super.key, required this.api});

  @override
  State<PatternSourcesTab> createState() => _PatternSourcesTabState();
}

class _PatternSourcesTabState extends State<PatternSourcesTab> {
  List<PatternSource> _sources = const [];
  List<PatternCatalogEntry> _catalog = const [];
  String? _error;
  bool _loading = true;

  /// Ids with an install/sync in flight, so each row spins on its own.
  final _busy = <String>{};

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
      final sources = await widget.api.sources();
      // The catalog is a nice-to-have; a daemon that cannot build it must
      // still show the sources that exist.
      List<PatternCatalogEntry> catalog = const [];
      try {
        catalog = await widget.api.catalog();
      } catch (_) {}
      if (!mounted) return;
      setState(() {
        _sources = sources;
        _catalog = catalog;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = e.toString();
        _loading = false;
      });
    }
  }

  void _toast(String msg) {
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(msg)));
  }

  Future<void> _guard(String id, Future<void> Function() work) async {
    setState(() => _busy.add(id));
    try {
      await work();
    } catch (e) {
      _toast(e.toString());
    } finally {
      if (mounted) setState(() => _busy.remove(id));
    }
  }

  Future<void> _install(PatternCatalogEntry e) => _guard(e.id, () async {
        final r = await widget.api.installCatalog(e.id);
        final n = (r['installed'] as num?)?.toInt() ??
            (r['sync'] is Map
                ? ((r['sync'] as Map)['patterns'] as num?)?.toInt() ?? 0
                : 0);
        _toast(tr('Đã cài $n pattern', 'Installed $n patterns'));
        await _load();
      });

  Future<void> _sync(PatternSource s) => _guard(s.id, () async {
        final out = await widget.api.syncSource(s.id);
        final n = (out['patterns'] as num?)?.toInt() ?? 0;
        final pinned = out['pinned'] == true;
        _toast(pinned
            ? tr('Đồng bộ xong: $n pattern', 'Synced: $n patterns')
            : tr(
                'Đồng bộ xong: $n pattern — nguồn theo nhánh, bản cập nhật có thể đổi prompt',
                'Synced: $n patterns — this source tracks a branch, an update can rewrite prompts',
              ));
        await _load();
      });

  Future<void> _toggle(PatternSource s) => _guard(s.id, () async {
        await widget.api.toggleSource(s.id);
        await _load();
      });

  Future<void> _delete(PatternSource s) async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text(tr('Xoá nguồn?', 'Delete source?')),
        content: Text(tr(
          'Xoá "${s.id}" và toàn bộ pattern trong đó khỏi daemon.',
          'Removes "${s.id}" and every pattern in it from the daemon.',
        )),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(ctx, false),
            child: Text(tr('Huỷ', 'Cancel')),
          ),
          TextButton(
            onPressed: () => Navigator.pop(ctx, true),
            child: Text(tr('Xoá', 'Delete'),
                style: const TextStyle(color: AppTokens.danger)),
          ),
        ],
      ),
    );
    if (ok != true) return;
    await _guard(s.id, () async {
      await widget.api.deleteSource(s.id);
      await _load();
    });
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    if (_loading) return const LoadingState();
    if (_error != null) return ErrorState(message: _error!, onRetry: _load);

    final offers = _catalog.where((e) => !e.installed).toList();
    return RefreshIndicator(
      color: c.accent,
      onRefresh: _load,
      child: ListView(
        padding: const EdgeInsets.fromLTRB(12, 12, 12, 32),
        children: [
          if (_sources.isEmpty && offers.isEmpty)
            EmptyState(
              icon: Icons.source_outlined,
              message: tr('Chưa có nguồn nào', 'No sources yet'),
            ),
          if (_sources.isNotEmpty) ...[
            _heading(c, tr('Đã cài', 'Installed')),
            for (final s in _sources) _sourceCard(c, s),
          ],
          if (offers.isNotEmpty) ...[
            const SizedBox(height: 18),
            _heading(c, tr('Cài thêm', 'Add')),
            for (final e in offers) _offerCard(c, e),
          ],
          const SizedBox(height: 18),
          OutlinedButton.icon(
            style: OutlinedButton.styleFrom(
              foregroundColor: c.textSecondary,
              side: BorderSide(color: c.border),
              padding: const EdgeInsets.symmetric(vertical: 12),
            ),
            onPressed: _addSourceDialog,
            icon: const Icon(Icons.add, size: 18),
            label: Text(tr('Thêm nguồn git…', 'Add a git source…')),
          ),
        ],
      ),
    );
  }

  Widget _heading(AppColors c, String text) => Padding(
        padding: const EdgeInsets.only(bottom: 8),
        child: Text(
          text.toUpperCase(),
          style: TextStyle(
            color: c.textMuted,
            fontSize: 11,
            fontWeight: FontWeight.w600,
            letterSpacing: 0.6,
          ),
        ),
      );

  Widget _sourceCard(AppColors c, PatternSource s) {
    final busy = _busy.contains(s.id);
    return Card(
      margin: const EdgeInsets.only(bottom: 8),
      color: c.surface,
      elevation: 0,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(AppTokens.rMd),
        side: BorderSide(color: c.border),
      ),
      child: Padding(
        padding: const EdgeInsets.all(12),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Icon(s.isGit ? Icons.cloud_outlined : Icons.folder_outlined,
                    size: 18,
                    color: s.enabled ? c.accent : c.textMuted),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(
                    s.name.isEmpty ? s.id : s.name,
                    style: TextStyle(
                      color: s.enabled ? c.textPrimary : c.textMuted,
                      fontWeight: FontWeight.w600,
                      fontSize: 14,
                    ),
                  ),
                ),
                Text('${s.count}',
                    style: TextStyle(color: c.textMuted, fontSize: 12)),
                if (busy) ...[
                  const SizedBox(width: 10),
                  const SizedBox(
                    width: 14,
                    height: 14,
                    child: CircularProgressIndicator(strokeWidth: 2),
                  ),
                ] else
                  PopupMenuButton<String>(
                    icon: Icon(Icons.more_vert, size: 18, color: c.textMuted),
                    color: c.surface,
                    onSelected: (v) {
                      switch (v) {
                        case 'sync':
                          _sync(s);
                        case 'toggle':
                          _toggle(s);
                        case 'delete':
                          _delete(s);
                      }
                    },
                    itemBuilder: (_) => [
                      if (s.isGit)
                        PopupMenuItem(
                          value: 'sync',
                          child: Text(tr('Đồng bộ lại', 'Re-sync')),
                        ),
                      PopupMenuItem(
                        value: 'toggle',
                        child: Text(s.enabled
                            ? tr('Tắt', 'Disable')
                            : tr('Bật', 'Enable')),
                      ),
                      PopupMenuItem(
                        value: 'delete',
                        child: Text(tr('Xoá', 'Delete'),
                            style: const TextStyle(color: AppTokens.danger)),
                      ),
                    ],
                  ),
              ],
            ),
            const SizedBox(height: 6),
            Text(
              sourceSubtitle(s),
              style: TextStyle(
                color: (s.lastError ?? '').isNotEmpty
                    ? AppTokens.danger
                    : c.textMuted,
                fontSize: 11.5,
              ),
            ),
            // A pattern lands in the system-prompt position, so a source that
            // follows a moving branch lets an upstream commit rewrite an
            // instruction the agent obeys. Say so.
            if (s.isGit && !s.pinned) ...[
              const SizedBox(height: 6),
              Row(
                children: [
                  const Icon(Icons.warning_amber_rounded,
                      size: 13, color: AppTokens.warning),
                  const SizedBox(width: 5),
                  Expanded(
                    child: Text(
                      tr(
                        'Theo nhánh "${s.gitRef}" — không ghim, cập nhật có thể đổi prompt',
                        'Tracks branch "${s.gitRef}" — unpinned, an update can rewrite prompts',
                      ),
                      style: const TextStyle(
                          color: AppTokens.warning, fontSize: 11),
                    ),
                  ),
                ],
              ),
            ],
          ],
        ),
      ),
    );
  }

  Widget _offerCard(AppColors c, PatternCatalogEntry e) {
    final busy = _busy.contains(e.id);
    return Card(
      margin: const EdgeInsets.only(bottom: 8),
      color: c.surface,
      elevation: 0,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(AppTokens.rMd),
        side: BorderSide(color: c.border),
      ),
      child: Padding(
        padding: const EdgeInsets.all(12),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Expanded(
                  child: Text(
                    e.name,
                    style: TextStyle(
                      color: c.textPrimary,
                      fontWeight: FontWeight.w600,
                      fontSize: 14,
                    ),
                  ),
                ),
                if (e.kind == 'bundled')
                  Container(
                    padding:
                        const EdgeInsets.symmetric(horizontal: 7, vertical: 2),
                    decoration: BoxDecoration(
                      color: c.accentSoft,
                      borderRadius: BorderRadius.circular(4),
                    ),
                    child: Text(
                      tr('Không cần mạng', 'Offline'),
                      style: TextStyle(color: c.accent, fontSize: 10.5),
                    ),
                  ),
              ],
            ),
            const SizedBox(height: 6),
            Text(
              e.description,
              style: TextStyle(color: c.textSecondary, fontSize: 12.5),
            ),
            const SizedBox(height: 8),
            Row(
              children: [
                Text(
                  '${e.count} pattern · ${e.license}'
                  '${e.gitRef != null ? " · ${e.gitRef}" : ""}',
                  style: TextStyle(color: c.textMuted, fontSize: 11.5),
                ),
                const Spacer(),
                if (busy)
                  const SizedBox(
                    width: 16,
                    height: 16,
                    child: CircularProgressIndicator(strokeWidth: 2),
                  )
                else
                  FilledButton(
                    style: FilledButton.styleFrom(
                      backgroundColor: c.accent,
                      visualDensity: VisualDensity.compact,
                    ),
                    onPressed: () => _install(e),
                    child: Text(tr('Cài', 'Install')),
                  ),
              ],
            ),
          ],
        ),
      ),
    );
  }

  Future<void> _addSourceDialog() async {
    final url = TextEditingController();
    final ref = TextEditingController(text: 'main');
    final subdir = TextEditingController();
    final c = context.colors;

    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: c.surface,
        title: Text(tr('Thêm nguồn git', 'Add a git source')),
        content: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              TextField(
                controller: url,
                autofocus: true,
                decoration: const InputDecoration(
                  labelText: 'URL',
                  hintText: 'https://github.com/user/repo',
                ),
              ),
              TextField(
                controller: ref,
                decoration: InputDecoration(
                  labelText: tr('Nhánh hoặc tag', 'Branch or tag'),
                  helperText: tr(
                    'Nên ghim tag: pattern là system prompt.',
                    'Prefer a tag: a pattern is a system prompt.',
                  ),
                  helperMaxLines: 2,
                ),
              ),
              TextField(
                controller: subdir,
                decoration: InputDecoration(
                  labelText: tr('Thư mục con', 'Subdirectory'),
                  hintText: 'data/patterns',
                ),
              ),
            ],
          ),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(ctx, false),
            child: Text(tr('Huỷ', 'Cancel')),
          ),
          TextButton(
            onPressed: () => Navigator.pop(ctx, true),
            child: Text(tr('Thêm', 'Add')),
          ),
        ],
      ),
    );

    if (ok != true || url.text.trim().isEmpty) return;
    await _guard('__add__', () async {
      await widget.api.addSource(
        url: url.text.trim(),
        gitRef: ref.text.trim().isEmpty ? 'main' : ref.text.trim(),
        subdir: subdir.text.trim(),
      );
      await _load();
    });
  }
}
