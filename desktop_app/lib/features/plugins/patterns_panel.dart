// Plugins → Patterns (desktop) — parity với web `PatternsPanel.tsx`.
//
// Pattern là một system prompt đặt tên sẵn: chữ vào → chữ ra, một lượt model,
// không tool, không vòng lặp. Vì thế chúng KHÔNG phải skill (vài trăm skill sẽ
// nhấn chìm bộ đối sánh trigger) — xem docs/zen-patterns.md.
//   GET    /api/patterns                     → danh sách đã khử trùng tên + nguồn + strategy
//   GET    /api/patterns/:name               → nội dung system.md / user.md
//   POST   /api/patterns/run                 → render (dryRun) hoặc chạy thật
//   POST   /api/patterns                     → tạo trong nguồn ghi được
//   DELETE /api/patterns/:name?source=       → xoá
//   GET/POST /api/patterns/sources           → liệt kê / thêm nguồn git
//   POST   /api/patterns/sources/:id/sync    → clone hoặc pull
//   POST   /api/patterns/sources/:id/toggle  → bật/tắt mà không xoá
//   DELETE /api/patterns/sources/:id         → gỡ nguồn + xoá tệp
//
// Hai điều daemon quyết định mà UI phải phản ánh trung thực:
//  1. Nguồn `user` luôn được tra trước → một pattern cùng tên do người dùng
//     viết sẽ ĐÈ bản từ git. Dòng "đè lên N nguồn khác" nói ra điều đó.
//  2. Nguồn git là chỉ-đọc. Sửa = lưu bản riêng vào `user`, không sửa tại chỗ
//     (lần sync sau sẽ nuốt mất).

import 'dart:convert';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:http/http.dart' as http;

import '../../core/i18n/l10n.dart';
import '../../core/transport/connection.dart';
import '../../theme/tokens.dart';

const _userSource = 'user';

const patternsOldDaemonMsg =
    'Daemon này chưa phục vụ /api/patterns — cần build lại và khởi động daemon mới.';

// ── Model ───────────────────────────────────────────────────────────────────

class PatternRow {
  const PatternRow({
    required this.name,
    required this.source,
    required this.description,
    required this.shadowedIn,
    required this.writable,
  });

  final String name;
  final String source;
  final String description;

  /// Nguồn khác cũng có tên này nhưng bị đè. Hiện ra để "sửa rồi mà không đổi"
  /// luôn có nguyên nhân nhìn thấy được.
  final List<String> shadowedIn;
  final bool writable;

  factory PatternRow.fromJson(Map<String, dynamic> j) => PatternRow(
        name: j['name'] as String? ?? '',
        source: j['source'] as String? ?? '',
        description: j['description'] as String? ?? '',
        shadowedIn:
            (j['shadowedIn'] as List?)?.whereType<String>().toList() ?? const [],
        writable: j['writable'] as bool? ?? false,
      );
}

class PatternSourceRow {
  const PatternSourceRow({
    required this.id,
    required this.name,
    required this.kind,
    required this.url,
    required this.gitRef,
    required this.enabled,
    required this.count,
    required this.writable,
    required this.lastError,
  });

  final String id;
  final String name;
  final String kind; // 'local' | 'git'
  final String? url;
  final String gitRef;
  final bool enabled;
  final int count;
  final bool writable;
  final String? lastError;

  bool get isGit => kind == 'git';

  factory PatternSourceRow.fromJson(Map<String, dynamic> j) => PatternSourceRow(
        id: j['id'] as String? ?? '',
        name: j['name'] as String? ?? '',
        kind: j['kind'] as String? ?? 'local',
        url: j['url'] as String?,
        gitRef: j['gitRef'] as String? ?? 'main',
        enabled: j['enabled'] as bool? ?? true,
        count: (j['count'] as num?)?.toInt() ?? 0,
        writable: j['writable'] as bool? ?? false,
        lastError: j['lastError'] as String?,
      );
}

class StrategyRow {
  const StrategyRow({required this.name, required this.description});
  final String name;
  final String description;

  factory StrategyRow.fromJson(Map<String, dynamic> j) => StrategyRow(
        name: j['name'] as String? ?? '',
        description: j['description'] as String? ?? '',
      );
}

/// Một nguồn cài được bằng một cú bấm, không cần gõ URL.
///
/// `bundled` nằm sẵn trong bản cài nên cài offline và tức thì; `git` phải
/// clone. Hai chuyện đó khác nhau đủ để người dùng cần biết trước khi bấm.
class CatalogEntry {
  const CatalogEntry({
    required this.id,
    required this.name,
    required this.description,
    required this.kind,
    required this.count,
    required this.license,
    required this.gitRef,
    required this.installed,
    required this.pinned,
  });

  final String id;
  final String name;
  final String description;
  final String kind; // 'bundled' | 'git'
  final int count;
  final String license;
  final String? gitRef;
  final bool installed;
  final bool pinned;

  bool get isBundled => kind == 'bundled';

  factory CatalogEntry.fromJson(Map<String, dynamic> j) => CatalogEntry(
        id: j['id'] as String? ?? '',
        name: j['name'] as String? ?? '',
        description: j['description'] as String? ?? '',
        kind: j['kind'] as String? ?? 'git',
        count: (j['count'] as num?)?.toInt() ?? 0,
        license: j['license'] as String? ?? '',
        gitRef: j['gitRef'] as String?,
        installed: j['installed'] as bool? ?? false,
        pinned: j['pinned'] as bool? ?? false,
      );
}

class PatternsView {
  const PatternsView({
    required this.patterns,
    required this.sources,
    required this.strategies,
    required this.catalog,
  });

  final List<PatternRow> patterns;
  final List<PatternSourceRow> sources;
  final List<StrategyRow> strategies;
  final List<CatalogEntry> catalog;
}

// ── Provider ────────────────────────────────────────────────────────────────

final patternsProvider = FutureProvider<PatternsView>((ref) async {
  final api = ref.read(apiClientProvider);
  final r = await api.get('/api/patterns');
  // Daemon cũ trả trang SPA cho /api lạ → ApiClient trả String, không phải Map.
  if (r is! Map || r['patterns'] is! List) {
    throw Exception(patternsOldDaemonMsg);
  }
  List<Map<String, dynamic>> rows(String key) => (r[key] as List? ?? const [])
      .whereType<Map>()
      .map((e) => e.cast<String, dynamic>())
      .toList();

  // Catalog là phần thêm: hỏng nó không được làm hỏng danh sách pattern.
  List<CatalogEntry> catalog = const [];
  try {
    final c = await api.get('/api/patterns/catalog');
    if (c is Map && c['catalog'] is List) {
      catalog = (c['catalog'] as List)
          .whereType<Map>()
          .map((e) => CatalogEntry.fromJson(e.cast<String, dynamic>()))
          .toList();
    }
  } catch (_) {
    /* daemon cũ chưa có endpoint này */
  }

  return PatternsView(
    patterns: rows('patterns').map(PatternRow.fromJson).toList(),
    sources: rows('sources').map(PatternSourceRow.fromJson).toList(),
    strategies: rows('strategies').map(StrategyRow.fromJson).toList(),
    catalog: catalog,
  );
});

/// Lỗi daemon trả về là `{"error": "..."}`; đọc nó ra thay vì chỉ báo mã số.
String _errText(Object e) {
  final s = e.toString();
  return s.startsWith('Exception: ') ? s.substring(11) : s;
}

void _toast(BuildContext context, String msg, {bool error = false}) {
  if (!context.mounted) return;
  ScaffoldMessenger.of(context).showSnackBar(SnackBar(
    content: Text(msg),
    backgroundColor: error ? Colors.red.shade700 : null,
  ));
}

Future<bool> _confirm(BuildContext context, String title, String body) async {
  final r = await showDialog<bool>(
    context: context,
    builder: (ctx) => AlertDialog(
      title: Text(title),
      content: Text(body),
      actions: [
        TextButton(
            onPressed: () => Navigator.pop(ctx, false),
            child: Text(ctx.tr('Cancel'))),
        FilledButton(
            onPressed: () => Navigator.pop(ctx, true),
            child: Text(ctx.tr('Confirm'))),
      ],
    ),
  );
  return r == true;
}

// ── Chip nguồn ──────────────────────────────────────────────────────────────

/// Nguồn vừa là thông tin vừa là bộ lọc — gộp làm một chip bấm được, các nút
/// quản lý chỉ hiện trên chip đang chọn để dải không bị rối.
///
/// Thay cho lưới thẻ to trước đây: với 3-4 nguồn nó chiếm nguyên một khối màn
/// hình để nói ba con số, còn bộ lọc lại nằm ở một dropdown rời phía dưới.
class _SourceChip extends StatelessWidget {
  const _SourceChip({
    required this.label,
    required this.count,
    required this.active,
    required this.onTap,
    this.kind,
    this.dimmed = false,
    this.error,
    this.actions,
    this.subtitle,
  });

  final String label;
  final int count;
  final bool active;
  final VoidCallback onTap;
  final String? kind;
  final bool dimmed;
  final String? error;
  final Widget? actions;

  /// `url @ ref` cho nguồn git — hiện dạng tooltip vì chip không đủ chỗ.
  final String? subtitle;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final chip = Opacity(
      opacity: dimmed ? 0.5 : 1,
      child: InkWell(
        borderRadius: BorderRadius.circular(AppTokens.rMd),
        onTap: onTap,
        child: Container(
          padding: EdgeInsets.fromLTRB(AppTokens.s12, AppTokens.s4,
              actions == null ? AppTokens.s12 : AppTokens.s4, AppTokens.s4),
          decoration: BoxDecoration(
            borderRadius: BorderRadius.circular(AppTokens.rMd),
            color: active ? c.accentSoft : Colors.transparent,
            border: Border.all(color: active ? c.accent : c.border),
          ),
          child: Row(mainAxisSize: MainAxisSize.min, children: [
            Text(label, style: TextStyle(color: c.textPrimary, fontSize: 13)),
            if (kind == 'git') ...[
              const SizedBox(width: AppTokens.s4),
              Text('git', style: TextStyle(color: c.textMuted, fontSize: 10)),
            ],
            const SizedBox(width: AppTokens.s6),
            _Chip(label: '$count'),
            if (error != null) ...[
              const SizedBox(width: AppTokens.s4),
              Tooltip(
                message: error!,
                child: const Icon(Icons.warning_amber_rounded,
                    size: 14, color: Colors.orange),
              ),
            ],
            ?actions,
          ]),
        ),
      ),
    );
    return subtitle == null ? chip : Tooltip(message: subtitle!, child: chip);
  }
}

// ── Thẻ catalog ─────────────────────────────────────────────────────────────

/// Một nguồn cài được bằng một cú bấm.
///
/// `bundled` nằm sẵn trong bản cài nên cài offline và tức thì; `git` phải
/// clone nên nút phải nói trước là sẽ mất một lúc.
class _CatalogCard extends StatelessWidget {
  const _CatalogCard({
    required this.entry,
    required this.busy,
    required this.disabled,
    required this.onInstall,
  });

  final CatalogEntry entry;
  final bool busy;
  final bool disabled;
  final VoidCallback onInstall;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Container(
      width: 300,
      padding: const EdgeInsets.all(AppTokens.s12),
      decoration: BoxDecoration(
        color: c.surfaceAlt,
        borderRadius: BorderRadius.circular(AppTokens.rMd),
        border: Border.all(color: c.border),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(children: [
            Expanded(
              child: Text(entry.name,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                      color: c.textPrimary, fontWeight: FontWeight.w600)),
            ),
            _Chip(label: entry.isBundled ? context.tr('bundled') : 'git'),
            const SizedBox(width: AppTokens.s4),
            _Chip(label: '${entry.count}'),
          ]),
          const SizedBox(height: AppTokens.s6),
          Text(entry.description,
              maxLines: 4,
              overflow: TextOverflow.ellipsis,
              style: TextStyle(color: c.textSecondary, fontSize: 12)),
          const SizedBox(height: AppTokens.s6),
          Row(children: [
            Text(entry.license,
                style: TextStyle(color: c.textMuted, fontSize: 11)),
            if (entry.gitRef != null)
              Text(' · ${entry.gitRef}',
                  style: TextStyle(color: c.textMuted, fontSize: 11)),
            // Một pattern nằm ở vị trí system prompt, nên "bám nhánh" là rủi ro
            // cần nói ra, không phải chi tiết kỹ thuật giấu đi.
            if (!entry.pinned) ...[
              const SizedBox(width: AppTokens.s6),
              Tooltip(
                message: context.tr(
                    'This source tracks a moving branch: an upstream commit can silently rewrite instructions the agent will obey.'),
                child: const Row(mainAxisSize: MainAxisSize.min, children: [
                  Icon(Icons.warning_amber_rounded,
                      size: 12, color: Colors.orange),
                  Text(' chưa ghim tag',
                      style: TextStyle(color: Colors.orange, fontSize: 11)),
                ]),
              ),
            ],
          ]),
          const SizedBox(height: AppTokens.s8),
          if (entry.installed)
            OutlinedButton.icon(
              onPressed: null,
              icon: const Icon(Icons.check_circle_outline, size: 15),
              label: Text(context.tr('Installed')),
            )
          else
            FilledButton.icon(
              onPressed: (disabled && !busy) ? null : onInstall,
              icon: busy
                  ? const SizedBox(
                      width: 14,
                      height: 14,
                      child: CircularProgressIndicator(strokeWidth: 2))
                  : Icon(entry.isBundled ? Icons.bolt : Icons.cloud_download_outlined,
                      size: 15),
              label: Text(entry.isBundled
                  ? context.tr('Install now')
                  : context.tr('Download')),
            ),
        ],
      ),
    );
  }
}

// ── Panel ───────────────────────────────────────────────────────────────────

class PatternsPanel extends ConsumerStatefulWidget {
  const PatternsPanel({super.key});

  @override
  ConsumerState<PatternsPanel> createState() => _PatternsPanelState();
}

class _PatternsPanelState extends ConsumerState<PatternsPanel> {
  String _query = '';
  String? _sourceFilter;
  String? _busySource;
  String? _installing;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final view = ref.watch(patternsProvider);

    return view.when(
      loading: () => const Center(child: CircularProgressIndicator()),
      error: (e, _) => Center(
        child: Padding(
          padding: const EdgeInsets.all(AppTokens.s24),
          child: Text(_errText(e), style: TextStyle(color: c.textSecondary)),
        ),
      ),
      data: (data) {
        // Lọc phía client: danh sách đã nằm sẵn trong bộ nhớ, gọi lại API mỗi
        // lần gõ chỉ tốn round-trip mà không thêm kết quả nào.
        final q = _query.trim().toLowerCase();
        final visible = data.patterns.where((p) {
          if (_sourceFilter != null && p.source != _sourceFilter) return false;
          if (q.isEmpty) return true;
          return p.name.toLowerCase().contains(q) ||
              p.description.toLowerCase().contains(q);
        }).toList();

        return ListView(
          padding: const EdgeInsets.all(AppTokens.s20),
          children: [
            _header(context),
            const SizedBox(height: AppTokens.s16),
            // Màn hình rỗng phải trả lời "cài cái gì", không phải báo "0 pattern".
            if (data.patterns.isEmpty && data.catalog.isNotEmpty) ...[
              _catalogCard(context, data),
              const SizedBox(height: AppTokens.s16),
            ],
            _sourcesCard(context, data),
            const SizedBox(height: AppTokens.s16),
            _listCard(context, data, visible),
          ],
        );
      },
    );
  }

  Widget _header(BuildContext context) {
    final c = context.colors;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(context.tr('Patterns'),
            style: TextStyle(
                color: c.textPrimary,
                fontSize: 18,
                fontWeight: FontWeight.w700)),
        const SizedBox(height: AppTokens.s4),
        Text(
          context.tr(
              'A named prompt for one text transform: text in, text out, one model call, no tools. Agents reach them through pattern_run.'),
          style: TextStyle(color: c.textSecondary, fontSize: 12),
        ),
      ],
    );
  }

  Widget _catalogCard(BuildContext context, PatternsView data) {
    final c = context.colors;
    return _Card(
      title: context.tr('Get started'),
      child: Wrap(
        spacing: AppTokens.s12,
        runSpacing: AppTokens.s12,
        children: [
          for (final e in data.catalog)
            _CatalogCard(
              entry: e,
              busy: _installing == e.id,
              disabled: _installing != null,
              onInstall: () => _installFromCatalog(e),
            ),
          // Import .zip là một cách CÀI, nên nó thuộc về đây — để nó chỉ nằm
          // trong toolbar của bảng danh sách thì màn hình rỗng không có đường
          // nào tới nó.
          Container(
            width: 300,
            padding: const EdgeInsets.all(AppTokens.s12),
            decoration: BoxDecoration(
              color: c.surfaceAlt,
              borderRadius: BorderRadius.circular(AppTokens.rMd),
              border: Border.all(color: c.border),
            ),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(children: [
                  Expanded(
                    child: Text(context.tr('From a .zip file'),
                        style: TextStyle(
                            color: c.textPrimary, fontWeight: FontWeight.w600)),
                  ),
                  _Chip(label: context.tr('offline')),
                ]),
                const SizedBox(height: AppTokens.s6),
                Text(
                  context.tr(
                      'A zip whose sub-folders each hold a system.md. A GitHub download works too — the wrapping folder is stripped.'),
                  style: TextStyle(color: c.textSecondary, fontSize: 12),
                ),
                const SizedBox(height: AppTokens.s8),
                OutlinedButton.icon(
                  onPressed: _importZip,
                  icon: const Icon(Icons.inbox_outlined, size: 15),
                  label: Text(context.tr('Choose a .zip')),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }

  // ── Nguồn ─────────────────────────────────────────────────────────────────

  Widget _sourcesCard(BuildContext context, PatternsView data) {
    return _Card(
      title: context.tr('Sources'),
      actions: [
        TextButton.icon(
          icon: const Icon(Icons.refresh, size: 16),
          label: Text(context.tr('Reload')),
          onPressed: () => ref.invalidate(patternsProvider),
        ),
        FilledButton.icon(
          icon: const Icon(Icons.cloud_download_outlined, size: 16),
          label: Text(context.tr('Add git source')),
          onPressed: _addSource,
        ),
      ],
      child: Wrap(
        spacing: AppTokens.s8,
        runSpacing: AppTokens.s8,
        children: [
          _SourceChip(
            label: context.tr('All sources'),
            count: data.patterns.length,
            active: _sourceFilter == null,
            onTap: () => setState(() => _sourceFilter = null),
          ),
          for (final s in data.sources)
            _SourceChip(
              label: s.name.isEmpty ? s.id : s.name,
              count: s.count,
              kind: s.kind,
              dimmed: !s.enabled,
              error: s.lastError,
              subtitle: s.isGit ? '${s.url ?? ''} @ ${s.gitRef}' : null,
              active: _sourceFilter == s.id,
              onTap: () => setState(
                  () => _sourceFilter = _sourceFilter == s.id ? null : s.id),
              actions: _sourceFilter == s.id
                  ? Row(mainAxisSize: MainAxisSize.min, children: [
                      if (s.isGit)
                        IconButton(
                          tooltip: context.tr('Sync'),
                          visualDensity: VisualDensity.compact,
                          icon: _busySource == s.id
                              ? const SizedBox(
                                  width: 14,
                                  height: 14,
                                  child:
                                      CircularProgressIndicator(strokeWidth: 2))
                              : const Icon(Icons.sync, size: 16),
                          onPressed:
                              _busySource == null ? () => _syncSource(s.id) : null,
                        ),
                      IconButton(
                        tooltip: s.enabled
                            ? context.tr('Hide this source')
                            : context.tr('Show again'),
                        visualDensity: VisualDensity.compact,
                        icon: Icon(
                            s.enabled
                                ? Icons.visibility_outlined
                                : Icons.visibility_off_outlined,
                            size: 16),
                        onPressed:
                            _busySource == null ? () => _toggleSource(s.id) : null,
                      ),
                      if (s.id != _userSource)
                        IconButton(
                          tooltip: context.tr('Remove source'),
                          visualDensity: VisualDensity.compact,
                          icon: const Icon(Icons.delete_outline, size: 16),
                          color: Colors.red.shade400,
                          onPressed: () => _removeSource(s),
                        ),
                    ])
                  : null,
            ),
        ],
      ),
    );
  }

  // ── Danh sách ─────────────────────────────────────────────────────────────

  Widget _listCard(
      BuildContext context, PatternsView data, List<PatternRow> visible) {
    final c = context.colors;
    return _Card(
      title: '${visible.length} pattern',
      actions: [
        SizedBox(
          width: 200,
          child: TextField(
            decoration: InputDecoration(
              isDense: true,
              prefixIcon: const Icon(Icons.search, size: 16),
              hintText: context.tr('Search name or description'),
              border: const OutlineInputBorder(),
            ),
            onChanged: (v) => setState(() => _query = v),
          ),
        ),
        TextButton.icon(
          icon: const Icon(Icons.add, size: 16),
          label: Text(context.tr('New pattern')),
          onPressed: _newPattern,
        ),
        IconButton(
          tooltip: context.tr('Import a .zip of pattern folders'),
          icon: const Icon(Icons.inbox_outlined, size: 18),
          onPressed: _importZip,
        ),
      ],
      child: visible.isEmpty
          ? Padding(
              padding: const EdgeInsets.symmetric(vertical: AppTokens.s24),
              child: Center(
                child: Text(
                  data.patterns.isEmpty
                      ? context.tr(
                          'No patterns yet — add a git source (Fabric, for example) or write one')
                      : context.tr('Nothing matches the filter'),
                  style: TextStyle(color: c.textMuted),
                ),
              ),
            )
          : Column(
              children: [
                for (final p in visible)
                  _PatternTile(
                    row: p,
                    onOpen: () => _openPattern(p, data.strategies),
                    onDelete: p.writable ? () => _deletePattern(p) : null,
                  ),
              ],
            ),
    );
  }

  // ── Hành động ─────────────────────────────────────────────────────────────

  /// Cài một mục catalog. `bundled` không chạm mạng; `git` clone nên có thể
  /// mất một lúc — nút phải khoá lại trong lúc đó.
  Future<void> _installFromCatalog(CatalogEntry entry) async {
    setState(() => _installing = entry.id);
    try {
      final r = await ref
          .read(apiClientProvider)
          .post('/api/patterns/catalog/${entry.id}/install');
      final n = (r is Map)
          ? ((r['sync'] is Map
                  ? (r['sync'] as Map)['patterns'] as num?
                  : (r['installed'] as List?)?.length as num?)
              ?.toInt() ??
              0)
          : 0;
      if (mounted) {
        _toast(context,
            context.trArgs('Installed {n} pattern(s) from "{name}"',
                {'n': n, 'name': entry.name}));
      }
      ref.invalidate(patternsProvider);
    } catch (e) {
      if (mounted) _toast(context, _errText(e), error: true);
    } finally {
      if (mounted) setState(() => _installing = null);
    }
  }

  /// Import một zip các thư mục pattern. Đây là đường cài duy nhất không cần
  /// mạng lẫn không cần gõ gì.
  Future<void> _importZip() async {
    final FilePickerResult? res;
    try {
      res = await FilePicker.platform.pickFiles(
        type: FileType.custom,
        allowedExtensions: const ['zip'],
        withData: true,
      );
    } catch (e) {
      // file_picker ném trước cả khi mở bảng chọn khi thiếu entitlement macOS —
      // im lặng ở đây trông y hệt người dùng bấm Huỷ.
      if (mounted) _toast(context, _errText(e), error: true);
      return;
    }
    final bytes = res?.files.single.bytes;
    if (bytes == null) return;

    try {
      final cfg = ref.read(appConfigProvider);
      final uri = Uri.parse('${cfg.httpBase}/api/patterns/import');
      final req = http.MultipartRequest('POST', uri)
        ..headers.addAll(cfg.authHeaders)
        ..fields['source'] = _userSource
        ..files.add(http.MultipartFile.fromBytes('file', bytes,
            filename: res!.files.single.name));
      final streamed = await req.send();
      final body = await streamed.stream.bytesToString();
      final decoded = body.isEmpty ? null : jsonDecode(body);
      if (decoded is! Map) throw Exception(patternsOldDaemonMsg);
      final map = decoded.cast<String, dynamic>();
      if (streamed.statusCode >= 300) {
        throw Exception('${map['error'] ?? 'HTTP ${streamed.statusCode}'}');
      }
      if (mounted) {
        _toast(
            context,
            context.trArgs('Imported {n}/{found} pattern(s)', {
              'n': (map['imported'] as List?)?.length ?? 0,
              'found': map['found'] ?? 0,
            }));
      }
      ref.invalidate(patternsProvider);
    } catch (e) {
      if (mounted) _toast(context, _errText(e), error: true);
    }
  }

  Future<void> _syncSource(String id) async {
    setState(() => _busySource = id);
    try {
      final r =
          await ref.read(apiClientProvider).post('/api/patterns/sources/$id/sync');
      final n = (r is Map && r['sync'] is Map)
          ? ((r['sync'] as Map)['patterns'] as num?)?.toInt() ?? 0
          : 0;
      if (mounted) {
        _toast(context,
            context.trArgs('{n} pattern(s) from "{id}"', {'n': n, 'id': id}));
      }
      ref.invalidate(patternsProvider);
    } catch (e) {
      if (mounted) _toast(context, _errText(e), error: true);
    } finally {
      if (mounted) setState(() => _busySource = null);
    }
  }

  Future<void> _toggleSource(String id) async {
    setState(() => _busySource = id);
    try {
      await ref
          .read(apiClientProvider)
          .post('/api/patterns/sources/$id/toggle');
      ref.invalidate(patternsProvider);
    } catch (e) {
      if (mounted) _toast(context, _errText(e), error: true);
    } finally {
      if (mounted) setState(() => _busySource = null);
    }
  }

  Future<void> _removeSource(PatternSourceRow s) async {
    final ok = await _confirm(
      context,
      context.tr('Remove source'),
      context.trArgs('Remove "{id}" and delete its {n} pattern(s)?',
          {'id': s.id, 'n': s.count}),
    );
    if (!ok) return;
    try {
      await ref.read(apiClientProvider).delete('/api/patterns/sources/${s.id}');
      ref.invalidate(patternsProvider);
    } catch (e) {
      if (mounted) _toast(context, _errText(e), error: true);
    }
  }

  Future<void> _deletePattern(PatternRow p) async {
    final ok = await _confirm(context, context.tr('Delete pattern'),
        context.trArgs('Delete "{name}"?', {'name': p.name}));
    if (!ok) return;
    try {
      await ref
          .read(apiClientProvider)
          .delete('/api/patterns/${p.name}?source=${p.source}');
      ref.invalidate(patternsProvider);
    } catch (e) {
      if (mounted) _toast(context, _errText(e), error: true);
    }
  }

  Future<void> _openPattern(
      PatternRow row, List<StrategyRow> strategies) async {
    Map<String, dynamic> files;
    try {
      final r =
          await ref.read(apiClientProvider).get('/api/patterns/${row.name}');
      if (r is! Map || r['pattern'] is! Map) {
        throw Exception(patternsOldDaemonMsg);
      }
      files = (r['pattern'] as Map).cast<String, dynamic>();
    } catch (e) {
      if (mounted) _toast(context, _errText(e), error: true);
      return;
    }
    if (!mounted) return;
    await showDialog<void>(
      context: context,
      builder: (_) => _RunDialog(files: files, strategies: strategies),
    );
  }

  Future<void> _addSource() async {
    final added = await showDialog<bool>(
      context: context,
      builder: (_) => const _AddSourceDialog(),
    );
    if (added == true) ref.invalidate(patternsProvider);
  }

  Future<void> _newPattern() async {
    final saved = await showDialog<bool>(
      context: context,
      builder: (_) => const _NewPatternDialog(),
    );
    if (saved == true) ref.invalidate(patternsProvider);
  }
}

// ── Hàng trong danh sách ────────────────────────────────────────────────────

class _PatternTile extends StatelessWidget {
  const _PatternTile({required this.row, required this.onOpen, this.onDelete});

  final PatternRow row;
  final VoidCallback onOpen;
  final VoidCallback? onDelete;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return InkWell(
      onTap: onOpen,
      child: Padding(
        padding: const EdgeInsets.symmetric(
            vertical: AppTokens.s8, horizontal: AppTokens.s4),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Expanded(
              flex: 2,
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(row.name,
                      style: TextStyle(
                          color: c.accent, fontWeight: FontWeight.w600)),
                  if (row.shadowedIn.isNotEmpty)
                    Tooltip(
                      message: context.trArgs(
                          'Also in: {others} — the copy in "{winner}" is the one used',
                          {
                            'others': row.shadowedIn.join(', '),
                            'winner': row.source
                          }),
                      child: Text(
                          context.trArgs('shadows {n} other source(s)',
                              {'n': row.shadowedIn.length}),
                          style: TextStyle(color: c.textMuted, fontSize: 11)),
                    ),
                ],
              ),
            ),
            Expanded(
              flex: 5,
              child: Text(row.description,
                  maxLines: 2,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(color: c.textSecondary, fontSize: 12)),
            ),
            const SizedBox(width: AppTokens.s8),
            _Chip(label: row.source),
            SizedBox(
              width: 44,
              child: onDelete == null
                  ? Tooltip(
                      message: context.tr(
                          'Git source — to change it, save your own copy under the same name; it takes priority'),
                      child:
                          Icon(Icons.lock_outline, size: 16, color: c.textMuted),
                    )
                  : IconButton(
                      icon: const Icon(Icons.delete_outline, size: 16),
                      color: Colors.red.shade400,
                      onPressed: onDelete,
                    ),
            ),
          ],
        ),
      ),
    );
  }
}

// ── Xem + chạy ──────────────────────────────────────────────────────────────

class _RunDialog extends ConsumerStatefulWidget {
  const _RunDialog({required this.files, required this.strategies});

  final Map<String, dynamic> files;
  final List<StrategyRow> strategies;

  @override
  ConsumerState<_RunDialog> createState() => _RunDialogState();
}

class _RunDialogState extends ConsumerState<_RunDialog> {
  final _input = TextEditingController();
  String? _strategy;

  /// Mặc định "auto": thư viện Fabric viết bằng tiếng Anh và hầu hết pattern ép
  /// output tiếng Anh, nên input tiếng Việt sẽ nhận lại bản tóm tắt tiếng Anh.
  String _language = 'auto';
  bool _busy = false;
  String? _result;
  String _meta = '';
  List<String> _unresolved = const [];

  @override
  void dispose() {
    _input.dispose();
    super.dispose();
  }

  Future<void> _run({required bool dryRun}) async {
    setState(() {
      _busy = true;
      _result = null;
    });
    try {
      final r =
          await ref.read(apiClientProvider).post('/api/patterns/run', body: {
        'name': widget.files['name'],
        'input': _input.text,
        'strategy': _strategy,
        'language': _language == 'off' ? null : _language,
        'dryRun': dryRun,
      });
      if (r is! Map) throw Exception(patternsOldDaemonMsg);
      if (dryRun) {
        final rendered = (r['rendered'] as Map).cast<String, dynamic>();
        setState(() {
          _result =
              '# SYSTEM\n\n${rendered['system']}\n\n# USER\n\n${rendered['user']}';
          _unresolved =
              (rendered['unresolved'] as List?)?.whereType<String>().toList() ??
                  const [];
          _meta = context.tr('rendered only, no model call');
        });
      } else {
        setState(() {
          _result = r['text'] as String? ?? '';
          _unresolved =
              (r['unresolved'] as List?)?.whereType<String>().toList() ??
                  const [];
          _meta = '${r['model']} · ${r['latencyMs']}ms';
        });
      }
    } catch (e) {
      if (mounted) _toast(context, _errText(e), error: true);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Dialog(
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 760, maxHeight: 720),
        child: Padding(
          padding: const EdgeInsets.all(AppTokens.s20),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            mainAxisSize: MainAxisSize.min,
            children: [
              Row(children: [
                Expanded(
                  child: Text('${widget.files['name']}',
                      style: TextStyle(
                          color: c.textPrimary,
                          fontSize: 16,
                          fontWeight: FontWeight.w700)),
                ),
                _Chip(label: '${widget.files['source']}'),
                IconButton(
                    icon: const Icon(Icons.close),
                    onPressed: () => Navigator.pop(context)),
              ]),
              const SizedBox(height: AppTokens.s12),
              Expanded(
                child: ListView(
                  children: [
                    TextField(
                      controller: _input,
                      maxLines: 6,
                      decoration: InputDecoration(
                        border: const OutlineInputBorder(),
                        hintText: context.tr(
                            'Paste the text to transform (article, transcript, log, notes…)'),
                      ),
                    ),
                    const SizedBox(height: AppTokens.s12),
                    Wrap(
                      spacing: AppTokens.s8,
                      runSpacing: AppTokens.s8,
                      crossAxisAlignment: WrapCrossAlignment.center,
                      children: [
                        SizedBox(
                          width: 230,
                          child: DropdownButtonFormField<String?>(
                            initialValue: _strategy,
                            isDense: true,
                            isExpanded: true,
                            decoration: InputDecoration(
                              isDense: true,
                              border: const OutlineInputBorder(),
                              hintText: context.tr('Strategy (optional)'),
                            ),
                            items: [
                              DropdownMenuItem(
                                  value: null,
                                  child: Text(context.tr('No strategy'))),
                              for (final s in widget.strategies)
                                DropdownMenuItem(
                                    value: s.name,
                                    child: Text('${s.name} — ${s.description}',
                                        overflow: TextOverflow.ellipsis)),
                            ],
                            onChanged: (v) => setState(() => _strategy = v),
                          ),
                        ),
                        SizedBox(
                          width: 220,
                          child: DropdownButtonFormField<String>(
                            initialValue: _language,
                            isDense: true,
                            isExpanded: true,
                            decoration: const InputDecoration(
                                isDense: true, border: OutlineInputBorder()),
                            items: [
                              DropdownMenuItem(
                                  value: 'auto',
                                  child:
                                      Text(context.tr('Language: follow the input'))),
                              DropdownMenuItem(
                                  value: 'Vietnamese',
                                  child:
                                      Text(context.tr('Language: Vietnamese'))),
                              const DropdownMenuItem(
                                  value: 'English',
                                  child: Text('Ngôn ngữ: English')),
                              DropdownMenuItem(
                                  value: 'off',
                                  child: Text(
                                      context.tr('Language: let the pattern decide'))),
                            ],
                            onChanged: (v) =>
                                setState(() => _language = v ?? 'auto'),
                          ),
                        ),
                        FilledButton.icon(
                          icon: const Icon(Icons.play_arrow, size: 16),
                          label: Text(context.tr('Run')),
                          onPressed: _busy ? null : () => _run(dryRun: false),
                        ),
                        Tooltip(
                          message: context.tr(
                              'Assemble the prompt only — costs no model call'),
                          child: OutlinedButton.icon(
                            icon: const Icon(Icons.science_outlined, size: 16),
                            label: Text(context.tr('Preview prompt')),
                            onPressed: _busy ? null : () => _run(dryRun: true),
                          ),
                        ),
                        if (_busy)
                          const SizedBox(
                              width: 18,
                              height: 18,
                              child: CircularProgressIndicator(strokeWidth: 2)),
                      ],
                    ),
                    if (_unresolved.isNotEmpty) ...[
                      const SizedBox(height: AppTokens.s12),
                      Container(
                        padding: const EdgeInsets.all(AppTokens.s8),
                        decoration: BoxDecoration(
                          color: Colors.orange.withValues(alpha: 0.12),
                          borderRadius: BorderRadius.circular(AppTokens.rMd),
                        ),
                        child: Text(
                          context.trArgs(
                              'Unfilled variables: {vars} — the pattern keeps the placeholder rather than deleting it',
                              {'vars': _unresolved.join(', ')}),
                          style: const TextStyle(fontSize: 12),
                        ),
                      ),
                    ],
                    if (_result != null) ...[
                      const SizedBox(height: AppTokens.s12),
                      Text(_meta,
                          style: TextStyle(color: c.textMuted, fontSize: 11)),
                      const SizedBox(height: AppTokens.s4),
                      SelectableText(
                        _result!,
                        style: const TextStyle(
                            fontFamily: 'monospace', fontSize: 12),
                      ),
                    ],
                    const SizedBox(height: AppTokens.s8),
                    Theme(
                      data: Theme.of(context)
                          .copyWith(dividerColor: Colors.transparent),
                      child: ExpansionTile(
                        tilePadding: EdgeInsets.zero,
                        childrenPadding: EdgeInsets.zero,
                        title: Text('system.md',
                            style: TextStyle(
                                color: c.textSecondary, fontSize: 12)),
                        children: [
                          Container(
                            width: double.infinity,
                            padding: const EdgeInsets.all(AppTokens.s12),
                            constraints: const BoxConstraints(maxHeight: 360),
                            decoration: BoxDecoration(
                              border: Border.all(color: c.border),
                              borderRadius:
                                  BorderRadius.circular(AppTokens.rMd),
                            ),
                            child: SingleChildScrollView(
                              child: SelectableText(
                                '${widget.files['system']}',
                                style: const TextStyle(
                                    fontFamily: 'monospace', fontSize: 12),
                              ),
                            ),
                          ),
                          const SizedBox(height: AppTokens.s4),
                          Align(
                            alignment: Alignment.centerLeft,
                            child: Text('${widget.files['path']}',
                                style: TextStyle(
                                    color: c.textMuted, fontSize: 11)),
                          ),
                        ],
                      ),
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

// ── Thêm nguồn git ──────────────────────────────────────────────────────────

class _AddSourceDialog extends ConsumerStatefulWidget {
  const _AddSourceDialog();

  @override
  ConsumerState<_AddSourceDialog> createState() => _AddSourceDialogState();
}

class _AddSourceDialogState extends ConsumerState<_AddSourceDialog> {
  final _url = TextEditingController();
  final _id = TextEditingController();
  final _ref = TextEditingController(text: 'main');
  final _subdir = TextEditingController();
  final _strategies = TextEditingController();
  bool _busy = false;

  @override
  void dispose() {
    for (final c in [_url, _id, _ref, _subdir, _strategies]) {
      c.dispose();
    }
    super.dispose();
  }

  Future<void> _submit() async {
    if (_url.text.trim().isEmpty) {
      _toast(context, context.tr('A repository URL is required'), error: true);
      return;
    }
    setState(() => _busy = true);
    try {
      final r = await ref
          .read(apiClientProvider)
          .post('/api/patterns/sources', body: {
        'url': _url.text.trim(),
        'id': _id.text.trim(),
        'ref': _ref.text.trim(),
        'subdir': _subdir.text.trim(),
        'strategiesSubdir':
            _strategies.text.trim().isEmpty ? null : _strategies.text.trim(),
        'sync': true,
      });
      final n = (r is Map && r['sync'] is Map)
          ? ((r['sync'] as Map)['patterns'] as num?)?.toInt() ?? 0
          : 0;
      if (mounted) {
        _toast(context,
            context.trArgs('Downloaded {n} pattern(s)', {'n': n}));
        Navigator.pop(context, true);
      }
    } catch (e) {
      if (mounted) _toast(context, _errText(e), error: true);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: Text(context.tr('Add a pattern source from git')),
      content: SizedBox(
        width: 520,
        child: ListView(
          shrinkWrap: true,
          children: [
            Container(
              padding: const EdgeInsets.all(AppTokens.s12),
              decoration: BoxDecoration(
                color: Colors.orange.withValues(alpha: 0.12),
                borderRadius: BorderRadius.circular(AppTokens.rMd),
              ),
              child: Text(
                context.tr(
                    'Pin a tag, do not track a branch. A pattern is placed in the system-prompt position — following a moving branch lets an upstream commit silently rewrite instructions the agent will obey.'),
                style: const TextStyle(fontSize: 12),
              ),
            ),
            const SizedBox(height: AppTokens.s12),
            _field(_url, 'Git URL', 'https://github.com/danielmiessler/fabric'),
            _field(_id, context.tr('Source id (blank = after the repo name)'),
                'fabric'),
            _field(_ref, context.tr('Branch / tag / sha'), 'v1.4.470'),
            _field(_subdir, context.tr('Folder holding the patterns'), 'data/patterns'),
            _field(_strategies, context.tr('Strategies folder (optional)'),
                'data/strategies'),
          ],
        ),
      ),
      actions: [
        TextButton(
            onPressed: _busy ? null : () => Navigator.pop(context, false),
            child: Text(context.tr('Cancel'))),
        FilledButton(
          onPressed: _busy ? null : _submit,
          child: _busy
              ? const SizedBox(
                  width: 16,
                  height: 16,
                  child: CircularProgressIndicator(strokeWidth: 2))
              : Text(context.tr('Add and download')),
        ),
      ],
    );
  }

  Widget _field(TextEditingController c, String label, String hint) => Padding(
        padding: const EdgeInsets.only(bottom: AppTokens.s12),
        child: TextField(
          controller: c,
          decoration: InputDecoration(
            isDense: true,
            labelText: label,
            hintText: hint,
            border: const OutlineInputBorder(),
          ),
        ),
      );
}

// ── Pattern mới ─────────────────────────────────────────────────────────────

class _NewPatternDialog extends ConsumerStatefulWidget {
  const _NewPatternDialog();

  @override
  ConsumerState<_NewPatternDialog> createState() => _NewPatternDialogState();
}

class _NewPatternDialogState extends ConsumerState<_NewPatternDialog> {
  final _name = TextEditingController();
  final _system = TextEditingController();
  bool _busy = false;

  @override
  void dispose() {
    _name.dispose();
    _system.dispose();
    super.dispose();
  }

  Future<void> _submit() async {
    if (_name.text.trim().isEmpty || _system.text.trim().isEmpty) {
      _toast(context, context.tr('A name and a system prompt are required'), error: true);
      return;
    }
    setState(() => _busy = true);
    try {
      await ref.read(apiClientProvider).post('/api/patterns', body: {
        'name': _name.text.trim(),
        'system': _system.text,
        'source': _userSource,
      });
      if (mounted) Navigator.pop(context, true);
    } catch (e) {
      if (mounted) _toast(context, _errText(e), error: true);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: Text(context.tr('New pattern')),
      content: SizedBox(
        width: 640,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            TextField(
              controller: _name,
              decoration: InputDecoration(
                isDense: true,
                labelText: context.tr('Name'),
                hintText: 'tom_tat_hop',
                helperText: context.tr(
                    'Letters, digits, - and _ . Diacritics fold to a plain slug.'),
                border: const OutlineInputBorder(),
              ),
            ),
            const SizedBox(height: AppTokens.s16),
            TextField(
              controller: _system,
              maxLines: 14,
              style: const TextStyle(fontFamily: 'monospace', fontSize: 12),
              decoration: InputDecoration(
                labelText: 'system.md',
                helperText: context.tr(
                    'Fabric convention: # IDENTITY and PURPOSE → # STEPS → # OUTPUT INSTRUCTIONS → # INPUT. Use {{input}} to place the text mid-prompt; without it the text becomes the user message.'),
                helperMaxLines: 3,
                border: const OutlineInputBorder(),
              ),
            ),
          ],
        ),
      ),
      actions: [
        TextButton(
            onPressed: _busy ? null : () => Navigator.pop(context, false),
            child: Text(context.tr('Cancel'))),
        FilledButton(
            onPressed: _busy ? null : _submit,
            child: Text(context.tr('Save'))),
      ],
    );
  }
}

// ── Vụn ─────────────────────────────────────────────────────────────────────

class _Card extends StatelessWidget {
  const _Card({required this.title, required this.child, this.actions});

  final String title;
  final Widget child;
  final List<Widget>? actions;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Container(
      padding: const EdgeInsets.all(AppTokens.s16),
      decoration: BoxDecoration(
        color: c.surface,
        borderRadius: BorderRadius.circular(AppTokens.rLg),
        border: Border.all(color: c.border),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(children: [
            Expanded(
              child: Text(title,
                  style: TextStyle(
                      color: c.textPrimary, fontWeight: FontWeight.w600)),
            ),
            if (actions != null)
              Wrap(
                spacing: AppTokens.s8,
                crossAxisAlignment: WrapCrossAlignment.center,
                children: actions!,
              ),
          ]),
          const SizedBox(height: AppTokens.s12),
          child,
        ],
      ),
    );
  }
}

class _Chip extends StatelessWidget {
  const _Chip({required this.label});
  final String label;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Container(
      padding: const EdgeInsets.symmetric(
          horizontal: AppTokens.s6, vertical: AppTokens.s2),
      decoration: BoxDecoration(
        color: c.surfaceAlt,
        borderRadius: BorderRadius.circular(AppTokens.rSm),
        border: Border.all(color: c.border),
      ),
      child: Text(label, style: TextStyle(color: c.textSecondary, fontSize: 11)),
    );
  }
}
