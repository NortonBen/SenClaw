import 'package:flutter/material.dart';
import '../../models/kit_models.dart';
import '../../services/kits_api.dart';
import '../../services/language_service.dart';
import '../../services/relay_manager.dart';
import '../../theme/tokens.dart';
import '../../util/format.dart';
import '../../widgets/states.dart';
import 'kit_install_sheet.dart';

/// Zen Kits over `/api/kits*` — installed bundles and what is on offer.
class KitsScreen extends StatefulWidget {
  const KitsScreen({super.key});

  @override
  State<KitsScreen> createState() => _KitsScreenState();
}

class _KitsScreenState extends State<KitsScreen>
    with SingleTickerProviderStateMixin {
  final _api = KitsApi();
  late final TabController _tabs = TabController(length: 2, vsync: this);

  List<InstalledKit> _installed = const [];
  List<AvailableKit> _available = const [];
  String? _error;
  bool _loading = true;

  final _busy = <String>{};

  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  void dispose() {
    _tabs.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final installed = await _api.installed();
      // A daemon with no marketplace still lists its built-in kits, but a
      // hard failure here must not hide the installed list.
      List<AvailableKit> available = const [];
      try {
        available = await _api.available();
      } catch (_) {}
      if (!mounted) return;
      setState(() {
        _installed = installed;
        _available = available;
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

  Future<void> _openInstall(AvailableKit kit) async {
    final report = await showModalBottomSheet<KitReport>(
      context: context,
      isScrollControlled: true,
      backgroundColor: Colors.transparent,
      builder: (_) => KitInstallSheet(api: _api, kit: kit),
    );
    if (report == null) return;
    _toast(report.ok
        ? tr('Đã cài ${report.changed} mục', 'Installed ${report.changed} items')
        : tr('Cài xong, ${report.failed} mục lỗi',
            'Installed with ${report.failed} failures'));
    await _load();
  }

  Future<void> _uninstall(InstalledKit kit) async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text(tr('Gỡ kit?', 'Uninstall kit?')),
        content: Text(tr(
          'Gỡ mọi thứ "${kit.title}" đã cài: agent, skill, workflow, hook, lịch, app.',
          'Removes everything "${kit.title}" installed: agents, skills, workflows, hooks, jobs, apps.',
        )),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(ctx, false),
            child: Text(tr('Huỷ', 'Cancel')),
          ),
          TextButton(
            onPressed: () => Navigator.pop(ctx, true),
            child: Text(tr('Gỡ', 'Uninstall'),
                style: const TextStyle(color: AppTokens.danger)),
          ),
        ],
      ),
    );
    if (ok != true) return;

    setState(() => _busy.add(kit.id));
    try {
      final report = await _api.uninstall(kit.id);
      _toast(report.ok
          ? tr('Đã gỡ ${report.changed} mục', 'Removed ${report.changed} items')
          : tr('Gỡ chưa xong, ${report.failed} mục lỗi — kit vẫn còn trong danh sách',
              'Uninstall incomplete, ${report.failed} failed — the kit stays listed'));
      await _load();
    } catch (e) {
      _toast(e.toString());
    } finally {
      if (mounted) setState(() => _busy.remove(kit.id));
    }
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
            Text('Kit', style: TextStyle(color: c.textPrimary)),
            const SizedBox(width: 8),
            AnimatedBuilder(
              animation: RelayManager(),
              builder: (_, _) =>
                  ConnectionDot(connected: RelayManager().connected),
            ),
          ],
        ),
        actions: [
          IconButton(
            tooltip: tr('Tải lại', 'Reload'),
            icon: Icon(Icons.refresh, color: c.textSecondary),
            onPressed: _loading ? null : _load,
          ),
        ],
        bottom: TabBar(
          controller: _tabs,
          indicatorColor: c.accent,
          labelColor: c.accent,
          unselectedLabelColor: c.textMuted,
          tabs: [
            Tab(
                icon: const Icon(Icons.inventory_2_outlined),
                text: tr('Đã cài (${_installed.length})',
                    'Installed (${_installed.length})')),
            Tab(
                icon: const Icon(Icons.storefront_outlined),
                text: tr('Có sẵn', 'Available')),
          ],
        ),
      ),
      body: _loading
          ? const LoadingState()
          : _error != null
              ? ErrorState(message: _error!, onRetry: _load)
              : TabBarView(
                  controller: _tabs,
                  children: [_installedTab(c), _availableTab(c)],
                ),
    );
  }

  Widget _installedTab(AppColors c) {
    if (_installed.isEmpty) {
      return EmptyState(
        icon: Icons.inventory_2_outlined,
        message: tr('Chưa cài kit nào', 'No kits installed'),
        hint: tr('Mở tab "Có sẵn" để cài.', 'Open the Available tab to install one.'),
      );
    }
    return RefreshIndicator(
      color: c.accent,
      onRefresh: _load,
      child: ListView.builder(
        padding: const EdgeInsets.fromLTRB(12, 12, 12, 32),
        itemCount: _installed.length,
        itemBuilder: (_, i) {
          final k = _installed[i];
          final busy = _busy.contains(k.id);
          return _card(
            c,
            title: k.title,
            subtitle: k.description,
            meta: tr(
              'v${k.version} · ${k.itemCount} mục · cài ${timeAgoIso(k.installedAt)}',
              'v${k.version} · ${k.itemCount} items · installed ${timeAgoIso(k.installedAt)}',
            ),
            trailing: busy
                ? const SizedBox(
                    width: 16,
                    height: 16,
                    child: CircularProgressIndicator(strokeWidth: 2),
                  )
                : IconButton(
                    tooltip: tr('Gỡ', 'Uninstall'),
                    icon: const Icon(Icons.delete_outline,
                        size: 20, color: AppTokens.danger),
                    onPressed: () => _uninstall(k),
                  ),
          );
        },
      ),
    );
  }

  Widget _availableTab(AppColors c) {
    if (_available.isEmpty) {
      return EmptyState(
        icon: Icons.storefront_outlined,
        message: tr('Không có kit nào', 'No kits on offer'),
        hint: tr(
          'Thêm một nguồn marketplace ở giao diện quản trị của daemon.',
          'Add a marketplace source in the daemon admin UI.',
        ),
      );
    }
    return RefreshIndicator(
      color: c.accent,
      onRefresh: _load,
      child: ListView.builder(
        padding: const EdgeInsets.fromLTRB(12, 12, 12, 32),
        itemCount: _available.length,
        itemBuilder: (_, i) {
          final k = _available[i];
          return _card(
            c,
            title: k.name,
            subtitle: k.description,
            meta: [
              if (k.version.isNotEmpty) 'v${k.version}',
              if (k.sourceName.isNotEmpty) k.sourceName,
              if (k.category.isNotEmpty) k.category,
            ].join(' · '),
            badge: k.hasUpdate
                ? _badge(c, tr('Cập nhật → v${k.version}', 'Update → v${k.version}'),
                    AppTokens.warning)
                : k.installed
                    ? _badge(c, tr('Đã cài', 'Installed'), AppTokens.success)
                    : null,
            trailing: !k.installable
                ? Text(tr('Không tải được', 'No artifact'),
                    style: TextStyle(color: c.textMuted, fontSize: 11))
                : FilledButton(
                    style: FilledButton.styleFrom(
                      backgroundColor: k.installed ? c.surfaceAlt : c.accent,
                      foregroundColor: k.installed ? c.textSecondary : null,
                      visualDensity: VisualDensity.compact,
                    ),
                    onPressed: () => _openInstall(k),
                    child: Text(k.hasUpdate
                        ? tr('Cập nhật', 'Update')
                        : k.installed
                            ? tr('Cài lại', 'Reinstall')
                            : tr('Cài', 'Install')),
                  ),
          );
        },
      ),
    );
  }

  Widget _badge(AppColors c, String text, Color color) => Container(
        padding: const EdgeInsets.symmetric(horizontal: 7, vertical: 2),
        decoration: BoxDecoration(
          color: color.withValues(alpha: 0.14),
          borderRadius: BorderRadius.circular(4),
        ),
        child: Text(text, style: TextStyle(color: color, fontSize: 10.5)),
      );

  Widget _card(
    AppColors c, {
    required String title,
    required String subtitle,
    required String meta,
    Widget? badge,
    Widget? trailing,
  }) =>
      Card(
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
                      title,
                      style: TextStyle(
                        color: c.textPrimary,
                        fontWeight: FontWeight.w600,
                        fontSize: 14,
                      ),
                    ),
                  ),
                  ?badge,
                ],
              ),
              if (subtitle.isNotEmpty) ...[
                const SizedBox(height: 6),
                Text(
                  subtitle,
                  maxLines: 3,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(color: c.textSecondary, fontSize: 12.5),
                ),
              ],
              const SizedBox(height: 8),
              Row(
                children: [
                  Expanded(
                    child: Text(meta,
                        style: TextStyle(color: c.textMuted, fontSize: 11.5)),
                  ),
                  ?trailing,
                ],
              ),
            ],
          ),
        ),
      );
}
