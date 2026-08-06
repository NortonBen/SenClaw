import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:url_launcher/url_launcher.dart';

import '../../core/i18n/l10n.dart';
import '../../core/transport/connection.dart';
import '../../theme/tokens.dart';

/// Live view of one Space App's process — the "is it actually working" half of
/// the details dialog.
///
/// Polls `/api/space/apps/<id>/runtime` while mounted. Mirrors
/// `web/src/components/space/AppRuntimePanel.tsx`: same endpoint, same numbers,
/// same copy. What it leads with is the unhappy case — an app that answers 500,
/// a launch counter climbing on its own (a crash loop looks like a healthy app
/// everywhere else), a proxy refusing the host the app actually needs.
class SpaceAppRuntimePanel extends ConsumerStatefulWidget {
  const SpaceAppRuntimePanel({super.key, required this.appId});

  final String appId;

  @override
  ConsumerState<SpaceAppRuntimePanel> createState() => _SpaceAppRuntimePanelState();
}

/// The same panel in a dialog, for screens that list apps rather than open one:
/// a row in Plugins → Sandbox is a fleet summary, and the way to go deeper on
/// one app should not be "leave here and find it in Space Apps".
class SpaceAppMonitorDialog extends StatelessWidget {
  const SpaceAppMonitorDialog({super.key, required this.appId, required this.appName});

  final String appId;
  final String appName;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Dialog(
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 760, maxHeight: 680),
        child: Padding(
          padding: const EdgeInsets.all(AppTokens.s20),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(children: [
                Icon(Icons.monitor_heart_outlined, size: 18, color: c.accent),
                const SizedBox(width: AppTokens.s8),
                Expanded(
                  child: Text(
                    context.trArgs('Process monitor — {name}', {'name': appName}),
                    style: TextStyle(
                        color: c.textPrimary,
                        fontSize: 15,
                        fontWeight: FontWeight.w700),
                  ),
                ),
                IconButton(
                  icon: const Icon(Icons.close, size: 18),
                  onPressed: () => Navigator.of(context).pop(),
                ),
              ]),
              const SizedBox(height: AppTokens.s8),
              Expanded(
                child: SingleChildScrollView(
                  child: SpaceAppRuntimePanel(appId: appId),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _SpaceAppRuntimePanelState extends ConsumerState<SpaceAppRuntimePanel> {
  Map<String, dynamic>? _snap;
  String? _error;
  bool _restarting = false;
  Timer? _timer;

  @override
  void initState() {
    super.initState();
    _load();
    _timer = Timer.periodic(const Duration(seconds: 3), (_) => _load());
  }

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }

  Future<void> _load() async {
    try {
      final d = await ref
          .read(apiClientProvider)
          .get('/api/space/apps/${widget.appId}/runtime') as Map<String, dynamic>;
      if (!mounted) return;
      setState(() {
        _snap = d;
        _error = null;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() => _error = '$e');
    }
  }

  Future<void> _restart() async {
    setState(() => _restarting = true);
    try {
      await ref.read(apiClientProvider).post('/api/space/apps/${widget.appId}/restart');
    } catch (_) {
      // The next poll shows what actually happened; a snackbar on top of a
      // panel that is already reporting the state would be noise.
    }
    if (!mounted) return;
    setState(() => _restarting = false);
    await _load();
  }

  String _uptime(int ms) {
    final s = ms ~/ 1000;
    if (s < 60) return '${s}s';
    if (s < 3600) return '${s ~/ 60}m ${s % 60}s';
    return '${s ~/ 3600}h ${(s % 3600) ~/ 60}m';
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    if (_error != null) {
      return Text(context.trArgs('Cannot read the state: {e}', {'e': _error!}),
          style: TextStyle(color: AppTokens.danger, fontSize: 11.5));
    }
    final snap = _snap;
    if (snap == null) {
      return Text(context.tr('Reading the state…'),
          style: TextStyle(color: c.textMuted, fontSize: 11.5));
    }

    final running = snap['running'] == true;
    final adopted = snap['adopted'] == true;
    final p = (snap['process'] as Map?)?.cast<String, dynamic>();
    final health = (snap['health'] as Map?)?.cast<String, dynamic>();
    final res = (snap['resources'] as Map?)?.cast<String, dynamic>();
    final net = (snap['network'] as Map?)?.cast<String, dynamic>() ?? const {};
    final launch = (snap['launch'] as Map?)?.cast<String, dynamic>() ?? const {};
    final log = (snap['log'] as Map?)?.cast<String, dynamic>() ?? const {};
    final launches = (snap['launches'] as num?)?.toInt() ?? 0;
    final healthy = health?['ok'] == true;

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        // ── the answer to "is it working", before any detail ────────────────
        Wrap(spacing: 6, runSpacing: 6, crossAxisAlignment: WrapCrossAlignment.center, children: [
          _chip(
            c,
            !running
                ? context.tr('not running')
                : !healthy
                    ? context.tr('running but not answering')
                    : adopted
                        ? context.tr('running (adopted)')
                        : context.tr('running'),
            !running
                ? AppTokens.danger
                : !healthy
                    ? AppTokens.warning
                    : adopted
                        ? c.accent
                        : AppTokens.success,
          ),
          if (p != null) _chip(c, 'pid ${p['pid']}', null),
          if (p != null) _chip(c, context.trArgs('port {p}', {'p': '${p['port']}'}), null),
          if (p?['uptimeMs'] != null)
            _chip(c, context.trArgs('up {t}', {'t': _uptime((p!['uptimeMs'] as num).toInt())}),
                null),
          // A launch count for a process this daemon did not start would be a
          // made-up number, so it simply is not shown.
          if (!adopted)
            Tooltip(
              message: context.tr(
                  'How many times the daemon has launched this app since it started. A number that keeps climbing on its own means the app keeps dying.'),
              child: _chip(c, context.trArgs('{n} launches', {'n': '$launches'}),
                  launches > 3 ? AppTokens.warning : null),
            ),
          if (p != null && '${p['isolation']}' != 'none')
            _chip(c, 'sandbox: ${p['isolation']}', c.accent),
          if (health != null)
            Text(
              healthy
                  ? 'health ${health['status']} · ${health['ms']}ms'
                  : '${health['error'] ?? 'health ${health['status']}'}',
              style: TextStyle(color: c.textMuted, fontSize: 11),
            ),
          OutlinedButton.icon(
            onPressed: _restarting ? null : _restart,
            icon: const Icon(Icons.refresh, size: 14),
            label: Text(context.tr('Restart')),
          ),
          if (p != null)
            OutlinedButton.icon(
              onPressed: () => launchUrl(Uri.parse('${p['url']}'),
                  mode: LaunchMode.externalApplication),
              icon: const Icon(Icons.open_in_new, size: 14),
              label: Text(context.tr('Open')),
            ),
          if (launch['cwd'] != null && (Platform.isMacOS || Platform.isWindows || Platform.isLinux))
            OutlinedButton.icon(
              onPressed: () => _openFolder('${launch['cwd']}'),
              icon: const Icon(Icons.folder_open, size: 14),
              label: Text(context.tr('Open folder')),
            ),
        ]),
        if (adopted)
          Padding(
            padding: const EdgeInsets.only(top: AppTokens.s8),
            child: Text(
                context.tr(
                    'This process is running but was NOT launched by the current daemon — it was already alive on the app\'s port, usually left over from a daemon restart. Whether the sandbox confines it is therefore unknown; restart the app if you need to be sure.'),
                style: TextStyle(color: c.accent, fontSize: 11.5)),
          ),
        if (running && !adopted && launches > 3)
          Padding(
            padding: const EdgeInsets.only(top: AppTokens.s8),
            child: Text(
                context.tr(
                    'This app has been launched many times — it is most likely dying and being restarted. The log below says why.'),
                style: TextStyle(color: AppTokens.warning, fontSize: 11.5)),
          ),

        // ── CPU / RAM ───────────────────────────────────────────────────────
        if (res != null) ...[
          const SizedBox(height: AppTokens.s12),
          Row(children: [
            Text('CPU ${(res['cpu'] as num).toStringAsFixed(1)}%',
                style: TextStyle(
                    color: c.textPrimary, fontSize: 12.5, fontWeight: FontWeight.w700)),
            const SizedBox(width: AppTokens.s16),
            Text('RAM ${(res['rssMb'] as num).toStringAsFixed(1)} MB',
                style: TextStyle(
                    color: c.textPrimary, fontSize: 12.5, fontWeight: FontWeight.w700)),
            const SizedBox(width: AppTokens.s12),
            Text(
                context.trArgs('{n} processes',
                    {'n': '${((res['processes'] as List?) ?? const []).length}'}),
                style: TextStyle(color: c.textMuted, fontSize: 11)),
          ]),
          if (res['note'] != null)
            Text('${res['note']}', style: TextStyle(color: AppTokens.warning, fontSize: 11)),
          const SizedBox(height: 4),
          for (final proc in ((res['processes'] as List?) ?? const []).whereType<Map>())
            _row(c, [
              '${proc['pid']}',
              (proc['cpu'] as num).toStringAsFixed(1),
              (proc['rssMb'] as num).toStringAsFixed(1),
              '${proc['elapsed']}',
              '${proc['command']}',
            ], flex: const [2, 2, 2, 2, 7]),
        ],

        // ── network ─────────────────────────────────────────────────────────
        const SizedBox(height: AppTokens.s12),
        Row(children: [
          Icon(Icons.lan_outlined, size: 14, color: c.accent),
          const SizedBox(width: 4),
          Text(context.tr('Network'),
              style: TextStyle(color: c.textPrimary, fontSize: 12.5, fontWeight: FontWeight.w700)),
          const SizedBox(width: AppTokens.s8),
          if (net['proxy'] != null)
            Expanded(
              child: Text(
                context.trArgs(
                    'allowlist proxy 127.0.0.1:{port} — {ok} allowed, {no} refused', {
                  'port': '${(net['proxy'] as Map)['port']}',
                  'ok': '${((net['proxy'] as Map)['stats'] as Map)['allowed']}',
                  'no': '${((net['proxy'] as Map)['stats'] as Map)['denied']}',
                }),
                style: TextStyle(color: c.textMuted, fontSize: 11),
                overflow: TextOverflow.ellipsis,
              ),
            ),
        ]),
        if (net['note'] != null)
          Text('${net['note']}', style: TextStyle(color: c.textMuted, fontSize: 11)),
        for (final conn in ((net['connections'] as List?) ?? const []).whereType<Map>())
          _row(c, [
            '${conn['proto']}',
            '${conn['local']}',
            conn['remote'] == null ? '—' : '${conn['remote']}',
            '${conn['state']}',
          ], flex: const [2, 6, 6, 4]),
        if (((net['connections'] as List?) ?? const []).isEmpty)
          Text(running ? context.tr('No sockets') : context.tr('The app is not running'),
              style: TextStyle(color: c.textMuted, fontSize: 11)),

        // ── everything needed to reproduce the launch by hand ───────────────
        const SizedBox(height: AppTokens.s12),
        _copyRow(c, context.tr('Folder'), '${launch['cwd'] ?? '—'}'),
        _copyRow(c, context.tr('Command'), '${launch['command'] ?? '—'}'),
        for (final e in ((launch['env'] as List?) ?? const []).whereType<List>())
          _copyRow(c, e.first == 'PORT' ? context.tr('Environment') : '', '${e.first}=${e.last}'),
        _copyRow(c, context.tr('Log file'),
            '${log['path'] ?? '—'}  (${(((log['bytes'] as num?) ?? 0) / 1024).toStringAsFixed(1)} KB)'),
      ],
    );
  }

  Future<void> _openFolder(String path) async {
    // The OS file manager, not a webview: this is the "let me look at it myself"
    // escape hatch.
    final cmd = Platform.isMacOS ? 'open' : (Platform.isWindows ? 'explorer' : 'xdg-open');
    try {
      await Process.run(cmd, [path]);
    } catch (_) {}
  }

  Widget _chip(AppColors c, String text, Color? color) => Container(
        padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
        decoration: BoxDecoration(
          color: (color ?? c.textMuted).withValues(alpha: 0.14),
          borderRadius: BorderRadius.circular(AppTokens.rSm),
        ),
        child: Text(text,
            style: TextStyle(color: color ?? c.textSecondary, fontSize: 11)),
      );

  Widget _row(AppColors c, List<String> cells, {required List<int> flex}) => Padding(
        padding: const EdgeInsets.symmetric(vertical: 2),
        child: Row(
          children: [
            for (var i = 0; i < cells.length; i++)
              Expanded(
                flex: flex[i],
                child: Text(cells[i],
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                        color: c.textSecondary, fontSize: 11, fontFamily: 'monospace')),
              ),
          ],
        ),
      );

  Widget _copyRow(AppColors c, String label, String value) => Padding(
        padding: const EdgeInsets.only(bottom: 3),
        child: Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
          SizedBox(
              width: 96,
              child: Text(label, style: TextStyle(color: c.textMuted, fontSize: 11.5))),
          Expanded(
            child: SelectableText(value,
                style: TextStyle(
                    color: c.textSecondary, fontSize: 11, fontFamily: 'monospace')),
          ),
          IconButton(
            tooltip: context.tr('Copy'),
            icon: const Icon(Icons.copy, size: 13),
            visualDensity: VisualDensity.compact,
            onPressed: () => Clipboard.setData(ClipboardData(text: value)),
          ),
        ]),
      );
}
