// Plugins → Widget (desktop) — quản lý widget chat/dashboard + luồng mặc định.
// Parity với web `WidgetsPanel.tsx`: danh mục từ GET /api/widgets (bật/tắt
// từng widget qua PUT /api/widgets/:id) + luồng mặc định GET/PUT /api/defaults
// (mở link / media / search / note). Lưu default xong thì làm mới luôn cache
// ChatLinkFlow để hành vi click link trong chat đổi ngay, không chờ restart.

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/i18n/l10n.dart';
import '../../core/transport/connection.dart';
import '../../theme/tokens.dart';
import '../chat/flow_defaults.dart';

// ── Models ──────────────────────────────────────────────────────────────────

class WidgetInfo {
  final String id;
  final String source; // builtin | app:<id> | plugin:<name>
  final String kind; // template | url
  final String name;
  final String description;
  final List<String> surfaces;
  final bool enabled;

  const WidgetInfo(this.id, this.source, this.kind, this.name,
      this.description, this.surfaces, this.enabled);

  factory WidgetInfo.fromJson(Map<String, dynamic> j) => WidgetInfo(
        '${j['id'] ?? ''}',
        '${j['source'] ?? ''}',
        '${j['kind'] ?? ''}',
        '${j['name'] ?? ''}',
        '${j['description'] ?? ''}',
        [
          for (final s in (j['surfaces'] as List? ?? const [])) '$s',
        ],
        j['enabled'] != false,
      );
}

/// Providers carry no BuildContext — translate through the global L10n.
String get _oldDaemonMsg => L10n.global.t(
    'This daemon does not serve /api/widgets yet — rebuild and restart the daemon.');

// ── Providers ───────────────────────────────────────────────────────────────

final widgetCatalogProvider = FutureProvider<List<WidgetInfo>>((ref) async {
  final r = await ref.read(apiClientProvider).get('/api/widgets');
  // Daemon cũ trả trang SPA cho /api lạ → ApiClient trả String, không phải Map.
  if (r is! Map || r['widgets'] is! List) {
    throw Exception(_oldDaemonMsg);
  }
  return (r['widgets'] as List)
      .whereType<Map>()
      .map((e) => WidgetInfo.fromJson(e.cast<String, dynamic>()))
      .toList();
});

final flowDefaultsProvider = FutureProvider<Map<String, dynamic>>((ref) async {
  final r = await ref.read(apiClientProvider).get('/api/defaults');
  if (r is! Map || r['openLink'] is! String) {
    throw Exception(_oldDaemonMsg);
  }
  return r.cast<String, dynamic>();
});

/// Id các Space App đang cài & bật — quyết định option mini-browser/search-app
/// có chọn được không (app cài rồi mới có nghĩa).
final installedAppIdsProvider = FutureProvider<Set<String>>((ref) async {
  final r = await ref.read(apiClientProvider).get('/api/space/apps');
  if (r is! List) return const {};
  return {
    for (final a in r.whereType<Map>())
      if (a['enabled'] != false) '${a['id'] ?? ''}',
  };
});

// ── Panel ───────────────────────────────────────────────────────────────────

class WidgetsManagePanel extends ConsumerWidget {
  const WidgetsManagePanel({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final catalog = ref.watch(widgetCatalogProvider);
    return Column(
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(
              AppTokens.s16, AppTokens.s12, AppTokens.s16, 0),
          child: Row(
            children: [
              Icon(Icons.widgets_outlined, size: 18, color: c.accent),
              const SizedBox(width: AppTokens.s8),
              Text('Widget',
                  style: TextStyle(
                      color: c.textPrimary,
                      fontSize: 15,
                      fontWeight: FontWeight.w700)),
              const Spacer(),
              IconButton(
                tooltip: context.tr('Reload'),
                icon: const Icon(Icons.refresh, size: 18),
                onPressed: () {
                  ref.invalidate(widgetCatalogProvider);
                  ref.invalidate(flowDefaultsProvider);
                  ref.invalidate(installedAppIdsProvider);
                },
              ),
            ],
          ),
        ),
        Expanded(
          child: ListView(
            padding: const EdgeInsets.all(AppTokens.s16),
            children: [
              Text(
                context.tr(
                    'Widgets render in the chat pane (emit_widget) and on the '
                    'Dashboard. Space Apps declare them in senclaw-manifest.json '
                    '→ widgets[]; plugins in widgets/widgets.json.'),
                style: TextStyle(color: c.textMuted, fontSize: 12),
              ),
              const SizedBox(height: AppTokens.s12),
              const _DefaultsCard(),
              const SizedBox(height: AppTokens.s16),
              Text(context.tr('Widget catalog'),
                  style: TextStyle(
                      color: c.textPrimary,
                      fontSize: 13,
                      fontWeight: FontWeight.w700)),
              const SizedBox(height: AppTokens.s8),
              catalog.when(
                loading: () => const Padding(
                  padding: EdgeInsets.all(AppTokens.s24),
                  child: Center(child: CircularProgressIndicator()),
                ),
                error: (e, _) => _errorBox(context, '$e'),
                data: (list) => Column(
                  children: [
                    for (final w in list) _WidgetRow(info: w),
                    if (list.isEmpty)
                      Padding(
                        padding: const EdgeInsets.all(AppTokens.s16),
                        child: Text(context.tr('No widgets yet.'),
                            style:
                                TextStyle(color: c.textMuted, fontSize: 12)),
                      ),
                  ],
                ),
              ),
            ],
          ),
        ),
      ],
    );
  }

  static Widget _errorBox(BuildContext context, String msg) {
    final c = context.colors;
    return Container(
      padding: const EdgeInsets.all(AppTokens.s12),
      decoration: BoxDecoration(
        color: AppTokens.warning.withValues(alpha: 0.08),
        border: Border.all(color: AppTokens.warning.withValues(alpha: 0.4)),
        borderRadius: BorderRadius.circular(AppTokens.rMd),
      ),
      child: Row(
        children: [
          const Icon(Icons.warning_amber_rounded,
              size: 16, color: AppTokens.warning),
          const SizedBox(width: AppTokens.s8),
          Expanded(
            child: Text(msg.replaceFirst('Exception: ', ''),
                style: TextStyle(color: c.textSecondary, fontSize: 12)),
          ),
        ],
      ),
    );
  }
}

// ── Catalog row ─────────────────────────────────────────────────────────────

({String label, Color color}) _widgetSource(String source) {
  if (source == 'builtin') return (label: 'Builtin', color: AppTokens.brandAlt);
  if (source.startsWith('app:')) {
    return (label: 'App: ${source.substring(4)}', color: AppTokens.cyan);
  }
  if (source.startsWith('plugin:')) {
    return (label: 'Plugin: ${source.substring(7)}', color: AppTokens.brand);
  }
  return (label: source, color: const Color(0xFF8A8A99));
}

class _WidgetRow extends ConsumerStatefulWidget {
  const _WidgetRow({required this.info});
  final WidgetInfo info;
  @override
  ConsumerState<_WidgetRow> createState() => _WidgetRowState();
}

class _WidgetRowState extends ConsumerState<_WidgetRow> {
  bool _busy = false;

  Future<void> _toggle(bool enabled) async {
    setState(() => _busy = true);
    try {
      await ref.read(apiClientProvider).put(
            '/api/widgets/${Uri.encodeComponent(widget.info.id)}',
            body: {'enabled': enabled},
          );
      ref.invalidate(widgetCatalogProvider);
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
            content: Text(context.trArgs('Save failed: {e}', {'e': e}))));
      }
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final w = widget.info;
    final src = _widgetSource(w.source);
    return Container(
      margin: const EdgeInsets.only(bottom: AppTokens.s8),
      padding: const EdgeInsets.symmetric(
          horizontal: AppTokens.s12, vertical: AppTokens.s8),
      decoration: BoxDecoration(
        color: c.surface,
        border: Border.all(color: c.border),
        borderRadius: BorderRadius.circular(AppTokens.rMd),
      ),
      child: Row(
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    Flexible(
                      child: Text(w.name.isNotEmpty ? w.name : w.id,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(
                              color: c.textPrimary,
                              fontSize: 13,
                              fontWeight: FontWeight.w600)),
                    ),
                    const SizedBox(width: AppTokens.s8),
                    Text(w.id,
                        style: TextStyle(color: c.textMuted, fontSize: 11)),
                  ],
                ),
                if (w.description.isNotEmpty)
                  Padding(
                    padding: const EdgeInsets.only(top: 2),
                    child: Text(w.description,
                        maxLines: 2,
                        overflow: TextOverflow.ellipsis,
                        style:
                            TextStyle(color: c.textSecondary, fontSize: 12)),
                  ),
                Padding(
                  padding: const EdgeInsets.only(top: AppTokens.s6),
                  child: Wrap(
                    spacing: AppTokens.s6,
                    children: [
                      _chip(src.label, src.color),
                      for (final s in w.surfaces)
                        _chip(s, s == 'chat' ? AppTokens.cyan : c.textMuted),
                    ],
                  ),
                ),
              ],
            ),
          ),
          const SizedBox(width: AppTokens.s12),
          _busy
              ? const SizedBox(
                  width: 32,
                  height: 20,
                  child: Center(
                      child: SizedBox(
                          width: 14,
                          height: 14,
                          child: CircularProgressIndicator(strokeWidth: 2))))
              : Switch(
                  value: w.enabled,
                  onChanged: (v) => _toggle(v),
                ),
        ],
      ),
    );
  }

  Widget _chip(String label, Color color) => Container(
        padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 1),
        decoration: BoxDecoration(
          color: color.withValues(alpha: 0.12),
          borderRadius: BorderRadius.circular(AppTokens.rSm),
        ),
        child: Text(label, style: TextStyle(color: color, fontSize: 10)),
      );
}

// ── Defaults card ───────────────────────────────────────────────────────────

class _DefaultsCard extends ConsumerWidget {
  const _DefaultsCard();

  Future<void> _save(
      BuildContext context, WidgetRef ref, String key, String value) async {
    try {
      await ref
          .read(apiClientProvider)
          .put('/api/defaults', body: {key: value});
      ref.invalidate(flowDefaultsProvider);
      // Chat link taps read the ChatLinkFlow static cache — refresh it now so
      // the new "Open link" default applies without restarting the app.
      await ChatLinkFlow.prefetch(ref.read(appConfigProvider).httpBase,
          force: true,
          authHeaders: ref.read(appConfigProvider).authHeaders);
    } catch (e) {
      if (context.mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
            content: Text(context.trArgs('Save failed: {e}', {'e': e}))));
      }
    }
  }

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final defaults = ref.watch(flowDefaultsProvider);
    final apps = ref.watch(installedAppIdsProvider).valueOrNull ?? const {};
    final hasMiniBrowser = apps.contains('mini-browser');
    final hasSearchApp = apps.contains('search');

    return Container(
      padding: const EdgeInsets.all(AppTokens.s12),
      decoration: BoxDecoration(
        color: c.surface,
        border: Border.all(color: c.border),
        borderRadius: BorderRadius.circular(AppTokens.rMd),
      ),
      child: defaults.when(
        loading: () => const Padding(
          padding: EdgeInsets.all(AppTokens.s16),
          child: Center(child: CircularProgressIndicator()),
        ),
        error: (e, _) => Text(
          '$e'.replaceFirst('Exception: ', ''),
          style: TextStyle(color: c.textMuted, fontSize: 12),
        ),
        data: (d) => Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Icon(Icons.tune, size: 15, color: c.accent),
                const SizedBox(width: AppTokens.s6),
                Text(context.tr('Default flows'),
                    style: TextStyle(
                        color: c.textPrimary,
                        fontSize: 13,
                        fontWeight: FontWeight.w700)),
              ],
            ),
            const SizedBox(height: AppTokens.s8),
            _row(
              context,
              context.tr('Open link'),
              _dropdown(context, '${d['openLink']}', [
                ('system-browser', context.tr('System browser'), true),
                ('new-tab', context.tr('New tab (web UI)'), true),
                ('mini-browser', context.tr('Mini Browser (inside SenClaw)'),
                    hasMiniBrowser),
              ], (v) => _save(context, ref, 'openLink', v)),
              hint: hasMiniBrowser
                  ? null
                  : context.tr('install the mini-browser app to open '
                      'inside SenClaw'),
            ),
            _row(
              context,
              context.tr('Media'),
              _dropdown(context, '${d['media']}', [
                ('inline-widget', context.tr('Play inline in chat (widget)'),
                    true),
                ('mini-browser', 'Mini Browser', hasMiniBrowser),
                ('system-browser', context.tr('System browser'), true),
              ], (v) => _save(context, ref, 'media', v)),
            ),
            _row(
              context,
              context.tr('Search'),
              Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  _dropdown(context, '${d['search']}', [
                    ('browser', 'browser_search (SERP)', true),
                    ('search-app', context.tr('App Search (federated)'),
                        hasSearchApp),
                  ], (v) => _save(context, ref, 'search', v)),
                  const SizedBox(width: AppTokens.s8),
                  _dropdown(context, '${d['searchEngine']}', [
                    ('google', 'Google', true),
                    ('bing', 'Bing', true),
                  ], (v) => _save(context, ref, 'searchEngine', v)),
                ],
              ),
              hint: hasSearchApp
                  ? null
                  : context.tr('install the search app for federated search'),
            ),
            _row(
              context,
              context.tr('Note'),
              _dropdown(context, '${d['note']}', [
                ('space-notes', 'Space Notes', true),
                ('wiki', 'Wiki (wiki_write)', true),
                ('memory', 'Memory (memory_save)', true),
              ], (v) => _save(context, ref, 'note', v)),
            ),
            const SizedBox(height: AppTokens.s6),
            Text(
              context.tr(
                  'These defaults go into the agent system prompt ("User '
                  'defaults") and drive what a link tap does. Messaging '
                  'channels always get a text summary instead of the widget.'),
              style: TextStyle(color: c.textMuted, fontSize: 11),
            ),
          ],
        ),
      ),
    );
  }

  Widget _row(BuildContext context, String label, Widget control,
      {String? hint}) {
    final c = context.colors;
    return Padding(
      padding: const EdgeInsets.only(bottom: AppTokens.s8),
      child: Row(
        children: [
          SizedBox(
            width: 90,
            child: Text(label,
                style: TextStyle(color: c.textSecondary, fontSize: 12)),
          ),
          control,
          if (hint != null) ...[
            const SizedBox(width: AppTokens.s8),
            Flexible(
              child: Text('($hint)',
                  style: TextStyle(color: c.textMuted, fontSize: 11)),
            ),
          ],
        ],
      ),
    );
  }

  Widget _dropdown(
    BuildContext context,
    String value,
    List<(String, String, bool)> options,
    void Function(String) onChanged,
  ) {
    final c = context.colors;
    // A stored value whose option is unavailable (app uninstalled later) must
    // still render — add it as a disabled-looking entry instead of crashing
    // the DropdownButton value assertion.
    final known = options.any((o) => o.$1 == value);
    return DropdownButton<String>(
      value: known ? value : null,
      hint: known ? null : Text(value),
      isDense: true,
      style: TextStyle(color: c.textPrimary, fontSize: 12),
      items: [
        for (final (v, label, enabled) in options)
          DropdownMenuItem(
            value: v,
            enabled: enabled,
            child: Text(label,
                style: TextStyle(
                    fontSize: 12,
                    color: enabled ? c.textPrimary : c.textMuted)),
          ),
      ],
      onChanged: (v) {
        if (v != null && v != value) onChanged(v);
      },
    );
  }
}
