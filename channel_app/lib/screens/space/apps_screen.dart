import 'package:flutter/material.dart';
import '../../models/space_models.dart';
import '../../services/space_api.dart';
import '../../theme/tokens.dart';
import '../../widgets/states.dart';
import 'space_page.dart';
import 'app_webview_screen.dart';

class AppsScreen extends StatelessWidget {
  const AppsScreen({super.key});
  @override
  Widget build(BuildContext context) =>
      const SpacePage(title: 'Apps', child: _AppsTab());
}

// ─── Apps ─────────────────────────────────────────────────────────────────────

class _AppsTab extends StatefulWidget {
  const _AppsTab();

  @override
  State<_AppsTab> createState() => _AppsTabState();
}

class _AppsTabState extends State<_AppsTab>
    with AutomaticKeepAliveClientMixin {
  final _api = SpaceApi();
  List<SpaceApp> _apps = [];
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
      final a = await _api.listApps();
      if (!mounted) return;
      setState(() {
        _apps = a;
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

  Future<void> _register() async {
    final ctrl = TextEditingController();
    final c = context.colors;
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: c.surface,
        title: Text('Thêm app từ manifest',
            style: TextStyle(color: c.textPrimary)),
        content: TextField(
          controller: ctrl,
          autofocus: true,
          style: TextStyle(color: c.textPrimary),
          decoration: InputDecoration(
            labelText: 'URL manifest',
            labelStyle: TextStyle(color: c.textMuted),
          ),
        ),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(ctx, false),
              child: const Text('Huỷ')),
          TextButton(
              onPressed: () => Navigator.pop(ctx, true),
              child: Text('Thêm',
                  style: TextStyle(color: c.accent))),
        ],
      ),
    );
    if (ok != true || ctrl.text.trim().isEmpty) return;
    try {
      await _api.registerApp(ctrl.text.trim());
      _load();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Lỗi: $e')));
      }
    }
  }

  void _openApp(SpaceApp a) {
    Navigator.of(context).push(MaterialPageRoute(
      builder: (_) => AppWebViewScreen(appId: a.id, title: a.name),
    ));
  }

  Future<void> _restart(SpaceApp a) async {
    try {
      await _api.restartApp(a.id);
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: Text('Đã khởi động lại ${a.name}')));
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Lỗi: $e')));
      }
    }
  }

  Future<void> _delete(SpaceApp a) async {
    final c = context.colors;
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: c.surface,
        title: Text('Gỡ app?', style: TextStyle(color: c.textPrimary)),
        content: Text(a.name, style: TextStyle(color: c.textSecondary)),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(ctx, false),
              child: const Text('Huỷ')),
          TextButton(
              onPressed: () => Navigator.pop(ctx, true),
              child: const Text('Gỡ',
                  style: TextStyle(color: AppTokens.danger))),
        ],
      ),
    );
    if (ok != true) return;
    try {
      await _api.deleteApp(a.id);
      _load();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Lỗi: $e')));
      }
    }
  }

  Future<void> _logs(SpaceApp a) async {
    try {
      final logs = await _api.appLogs(a.id);
      if (!mounted) return;
      final c = context.colors;
      showModalBottomSheet(
        context: context,
        isScrollControlled: true,
        backgroundColor: c.surface,
        shape: const RoundedRectangleBorder(
          borderRadius: BorderRadius.vertical(top: Radius.circular(18)),
        ),
        builder: (_) => FractionallySizedBox(
          heightFactor: 0.85,
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text('Log · ${a.name}',
                    style: TextStyle(
                        color: c.textPrimary,
                        fontSize: 16,
                        fontWeight: FontWeight.bold)),
                const SizedBox(height: 10),
                Expanded(
                  child: SingleChildScrollView(
                    child: SelectableText(
                      logs.isEmpty ? '(trống)' : logs,
                      style: TextStyle(
                          color: c.textSecondary,
                          fontFamily: 'monospace',
                          fontSize: 12),
                    ),
                  ),
                ),
              ],
            ),
          ),
        ),
      );
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Lỗi: $e')));
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
        onPressed: _register,
        backgroundColor: c.accent,
        foregroundColor: Colors.white,
        child: const Icon(Icons.add),
      ),
      body: _buildBody(),
    );
  }

  Widget _buildBody() {
    final c = context.colors;
    if (_loading) return const LoadingState(text: 'Đang tải apps…');
    if (_error != null) return ErrorState(message: _error!, onRetry: _load);
    if (_apps.isEmpty) {
      return const EmptyState(
        icon: Icons.apps_outlined,
        message: 'Chưa có app',
        hint: 'Nhấn + để thêm app từ URL manifest',
      );
    }
    return RefreshIndicator(
      onRefresh: _load,
      color: c.accent,
      backgroundColor: c.surface,
      child: ListView.builder(
        padding: const EdgeInsets.fromLTRB(12, 12, 12, 88),
        itemCount: _apps.length,
        itemBuilder: (ctx, i) {
          final a = _apps[i];
          return Card(
            color: c.surfaceAlt,
            margin: const EdgeInsets.only(bottom: 8),
            shape: RoundedRectangleBorder(
              borderRadius: BorderRadius.circular(12),
              side: BorderSide(color: c.border),
            ),
            child: ListTile(
              leading: Text(a.icon, style: const TextStyle(fontSize: 24)),
              title: Text(a.name,
                  style: TextStyle(
                      color: a.enabled ? c.textPrimary : c.textMuted,
                      fontWeight: FontWeight.w600)),
              subtitle: a.description.isEmpty
                  ? null
                  : Text(a.description,
                      style: TextStyle(
                          color: c.textMuted, fontSize: 12),
                      maxLines: 2,
                      overflow: TextOverflow.ellipsis),
              trailing: PopupMenuButton<String>(
                color: c.surface,
                icon: Icon(Icons.more_vert, color: c.textSecondary),
                onSelected: (v) {
                  if (v == 'open') _openApp(a);
                  if (v == 'restart') _restart(a);
                  if (v == 'logs') _logs(a);
                  if (v == 'delete') _delete(a);
                },
                itemBuilder: (_) => [
                  PopupMenuItem(
                      value: 'open',
                      child: Text('Mở',
                          style: TextStyle(color: c.textPrimary))),
                  PopupMenuItem(
                      value: 'restart',
                      child: Text('Khởi động lại',
                          style: TextStyle(color: c.textPrimary))),
                  PopupMenuItem(
                      value: 'logs',
                      child: Text('Xem log',
                          style: TextStyle(color: c.textPrimary))),
                  const PopupMenuItem(
                      value: 'delete',
                      child: Text('Gỡ',
                          style: TextStyle(color: AppTokens.danger))),
                ],
              ),
              onTap: () => _openApp(a),
            ),
          );
        },
      ),
    );
  }
}
