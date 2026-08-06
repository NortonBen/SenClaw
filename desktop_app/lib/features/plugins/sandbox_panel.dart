import 'dart:async';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/i18n/l10n.dart';
import '../../core/i18n/locale_provider.dart';
import '../../core/transport/connection.dart';
import '../../theme/tokens.dart';
import 'space_app_runtime_panel.dart';
import 'space_app_sandbox_dialog.dart';

/// Plugins → Sandbox — quản lý OS sandbox tích hợp trong daemon
/// (`/api/sandbox/*`): luồng đang chạy, giám sát CPU/RAM + kill tiến trình,
/// lịch sử chạy, và các công tắc cưỡng chế exec/python/node/script.
///
/// Đối chiếu web: `web/src/components/plugins/SandboxPanel.tsx` — cùng REST,
/// cùng ngữ nghĩa. Ngôn ngữ lấy từ Settings → Language như mọi màn hình khác;
/// chuỗi tiếng Việt nằm trong `core/i18n/vi/plugins_misc.dart`.

// ── Models ──────────────────────────────────────────────────────────────────

class SandboxInfo {
  final String id;
  final String name;
  final String backend;
  final String workdir;
  final bool network;
  final double cpus;
  final int memoryMb;
  final int timeoutMs;
  final String fsMode;
  final String status;
  final String? lastError;
  final int createdAt;
  final int? lastUsedAt;

  const SandboxInfo(
      this.id,
      this.name,
      this.backend,
      this.workdir,
      this.network,
      this.cpus,
      this.memoryMb,
      this.timeoutMs,
      this.fsMode,
      this.status,
      this.lastError,
      this.createdAt,
      this.lastUsedAt);

  factory SandboxInfo.fromJson(Map<String, dynamic> j) => SandboxInfo(
        '${j['id'] ?? ''}',
        '${j['name'] ?? ''}',
        '${j['backend'] ?? 'direct'}',
        '${j['workdir'] ?? ''}',
        j['network'] == true,
        (j['cpus'] as num?)?.toDouble() ?? 1,
        (j['memoryMb'] as num?)?.toInt() ?? 512,
        (j['timeoutMs'] as num?)?.toInt() ?? 30000,
        '${j['fsMode'] ?? 'strict'}',
        '${j['status'] ?? ''}',
        j['lastError']?.toString(),
        (j['createdAt'] as num?)?.toInt() ?? 0,
        (j['lastUsedAt'] as num?)?.toInt(),
      );
}

class SandboxRun {
  final String kind;
  final String? language;
  final String source;
  final int? exitCode;
  final bool timedOut;
  final String isolation;
  final int durationMs;
  final int createdAt;

  const SandboxRun(this.kind, this.language, this.source, this.exitCode,
      this.timedOut, this.isolation, this.durationMs, this.createdAt);

  factory SandboxRun.fromJson(Map<String, dynamic> j) => SandboxRun(
        '${j['kind'] ?? ''}',
        j['language']?.toString(),
        '${j['source'] ?? ''}',
        (j['exitCode'] as num?)?.toInt(),
        j['timedOut'] == true,
        '${j['isolation'] ?? ''}',
        (j['durationMs'] as num?)?.toInt() ?? 0,
        (j['createdAt'] as num?)?.toInt() ?? 0,
      );
}

// ── Panel ───────────────────────────────────────────────────────────────────

class SandboxPanel extends ConsumerStatefulWidget {
  const SandboxPanel({super.key});

  @override
  ConsumerState<SandboxPanel> createState() => _SandboxPanelState();
}

class _SandboxPanelState extends ConsumerState<SandboxPanel> {
  Map<String, dynamic>? _caps;
  Map<String, dynamic>? _policy;
  Map<String, dynamic>? _defaults;
  List<SandboxInfo> _sandboxes = const [];
  List<SandboxRun> _runs = const [];
  /// Space Apps with a server process, and what the sandbox does to each. Kept
  /// separate from `_sandboxes`: those are the engine's throwaway sessions, an
  /// app is a long-lived process someone installed.
  List<Map<String, dynamic>> _apps = const [];
  Map<String, dynamic>? _appCaps;
  /// Page index for the apps card. 47 installed apps is a normal number and an
  /// unpaged list buries every other card on the screen.
  int _appPage = 0;
  /// `status` | `name` | `sandbox` | `cpu` | `ram` | `launches`.
  ///
  /// Status by default, not name: with 47 apps installed and two running, the
  /// alphabetical list opens on whatever happens to start with "A" and the rows
  /// worth looking at are three pages down.
  String _appSort = 'status';
  bool _appSortAsc = false;
  String? _error;
  bool _loading = true;
  final _manualPathCtl = TextEditingController();

  @override
  void initState() {
    super.initState();
    _refresh();
  }

  @override
  void dispose() {
    _manualPathCtl.dispose();
    super.dispose();
  }

  /// English string = key, rendered in the app language (Settings → Language).
  /// This screen used to carry its own EN/VI switch and its own stored
  /// preference; two places to set one thing is two places to disagree.
  String t(String en) => L10n(ref.watch(localeCodeProvider)).t(en);

  Future<void> _refresh() async {
    setState(() => _loading = true);
    final api = ref.read(apiClientProvider);
    try {
      final results = await Future.wait([
        api.get('/api/sandbox/caps'),
        api.get('/api/sandbox/exec-policy'),
        api.get('/api/sandbox/settings'),
        api.get('/api/sandbox/sandboxes'),
        api.get('/api/sandbox/runs', query: {'limit': 30}),
        // A different route family, so an older daemon that lacks it must not
        // take the whole panel down with it.
        api
            .get('/api/space/apps/sandbox-overview')
            .catchError((_) => <String, dynamic>{}),
      ]);
      // Daemon cũ trả trang SPA cho /api lạ → String chứ không phải Map.
      if (results[0] is! Map || results[1] is! Map) {
        throw Exception(t('This daemon does not serve /api/sandbox yet — '
            'rebuild and restart the daemon.'));
      }
      final sb = results[3];
      final rn = results[4];
      if (!mounted) return;
      setState(() {
        _caps = (results[0] as Map).cast<String, dynamic>();
        _policy = (results[1] as Map).cast<String, dynamic>();
        _defaults = results[2] is Map
            ? (results[2] as Map).cast<String, dynamic>()
            : null;
        _sandboxes = sb is Map && sb['sandboxes'] is List
            ? (sb['sandboxes'] as List)
                .whereType<Map>()
                .map((e) => SandboxInfo.fromJson(e.cast<String, dynamic>()))
                .toList()
            : const [];
        _runs = rn is Map && rn['runs'] is List
            ? (rn['runs'] as List)
                .whereType<Map>()
                .map((e) => SandboxRun.fromJson(e.cast<String, dynamic>()))
                .toList()
            : const [];
        final ov = results.length > 5 && results[5] is Map
            ? (results[5] as Map).cast<String, dynamic>()
            : const <String, dynamic>{};
        _apps = ((ov['apps'] as List?) ?? const [])
            .whereType<Map>()
            .map((e) => e.cast<String, dynamic>())
            .toList();
        _appCaps = (ov['caps'] as Map?)?.cast<String, dynamic>();
        _error = null;
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

  Future<void> _restartApp(String id) async {
    try {
      await ref.read(apiClientProvider).post('/api/space/apps/$id/restart');
      _snack(t('App restarted'));
    } catch (e) {
      _snack(t('Restart failed: {e}').replaceFirst('{e}', '$e'));
    }
    await _refresh();
  }

  void _snack(String msg) {
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(msg)));
  }

  /// PUT exec-policy — gửi CẢ object (PUT nhận full body; field thiếu sẽ bị
  /// reset về default phía daemon, nên không bao giờ gửi partial).
  Future<void> _savePolicy(Map<String, dynamic> patch) async {
    final next = {...?_policy, ...patch};
    setState(() => _policy = next);
    try {
      final saved = await ref
          .read(apiClientProvider)
          .put('/api/sandbox/exec-policy', body: next);
      if (!mounted) return;
      setState(() {
        if (saved is Map) _policy = saved.cast<String, dynamic>();
      });
      _snack(t('Enforcement saved'));
    } catch (e) {
      _snack(t('Save failed: {e}').replaceFirst('{e}', '$e'));
      _refresh();
    }
  }

  Future<void> _saveDefaults(Map<String, dynamic> patch,
      {List<String>? allowlist}) async {
    if (_defaults == null) return;
    final next = {
      ...?_defaults,
      ...patch,
      'allowlist': allowlist ?? (_defaults?['allowlist'] as List?) ?? const [],
    };
    try {
      final saved = await ref
          .read(apiClientProvider)
          .put('/api/sandbox/settings', body: next);
      if (!mounted) return;
      setState(() {
        if (saved is Map) _defaults = saved.cast<String, dynamic>();
      });
      _snack(t('Defaults saved'));
    } catch (e) {
      _snack(t('Save failed: {e}').replaceFirst('{e}', '$e'));
    }
  }

  Future<void> _killAll(SandboxInfo sb) async {
    try {
      await ref
          .read(apiClientProvider)
          .post('/api/sandbox/sandboxes/${sb.id}/kill', body: const {});
      _snack(t('Stopped all processes in "{name}"')
          .replaceFirst('{name}', sb.name));
    } catch (e) {
      _snack(t('Stop failed: {e}').replaceFirst('{e}', '$e'));
    }
  }

  Future<void> _delete(SandboxInfo sb, {required bool purge}) async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text(purge ? t('Delete with all files?') : t('Delete sandbox?')),
        content: Text((purge
                ? t('Deletes "{name}" AND every file in its working '
                    'directory. This cannot be undone.')
                : t('Removes "{name}" from the list. Files on disk are kept.'))
            .replaceFirst('{name}', sb.name)),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(ctx, false),
              child: Text(t('Cancel'))),
          FilledButton(
              onPressed: () => Navigator.pop(ctx, true),
              child: Text(purge ? t('Delete files') : t('Delete'))),
        ],
      ),
    );
    if (ok != true) return;
    try {
      await ref
          .read(apiClientProvider)
          .delete('/api/sandbox/sandboxes/${sb.id}?purge=$purge');
      _snack(t('Deleted'));
      _refresh();
    } catch (e) {
      _snack(t('Delete failed: {e}').replaceFirst('{e}', '$e'));
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    if (_loading && _caps == null) {
      return const Center(child: CircularProgressIndicator());
    }
    if (_error != null && _caps == null) {
      return Center(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(_error!,
                style: TextStyle(color: c.textSecondary, fontSize: 13),
                textAlign: TextAlign.center),
            const SizedBox(height: AppTokens.s12),
            OutlinedButton.icon(
                onPressed: _refresh,
                icon: const Icon(Icons.refresh, size: 16),
                label: Text(t('Retry'))),
          ],
        ),
      );
    }
    return ListView(
      padding: const EdgeInsets.all(AppTokens.s16),
      children: [
        Row(
          children: [
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text('Sandbox',
                      style: TextStyle(
                          color: c.textPrimary,
                          fontSize: 18,
                          fontWeight: FontWeight.w700)),
                  const SizedBox(height: 2),
                  Text(
                      t('Run commands and code isolated from the real machine '
                          '— sessions, CPU/RAM monitoring, and the '
                          'exec/python/node/script enforcement switches.'),
                      style: TextStyle(color: c.textMuted, fontSize: 12)),
                ],
              ),
            ),
            OutlinedButton.icon(
              onPressed: _loading ? null : _refresh,
              icon: const Icon(Icons.refresh, size: 16),
              label: Text(t('Refresh')),
            ),
          ],
        ),
        const SizedBox(height: AppTokens.s12),
        if (_caps != null) _capsCard(c),
        const SizedBox(height: AppTokens.s12),
        if (_policy != null) _policyCard(c),
        const SizedBox(height: AppTokens.s12),
        _appsCard(c),
        const SizedBox(height: AppTokens.s12),
        _sandboxesCard(c),
        const SizedBox(height: AppTokens.s12),
        _runsCard(c),
        const SizedBox(height: AppTokens.s12),
        if (_defaults != null) _defaultsCard(c),
      ],
    );
  }

  // ── Caps ──────────────────────────────────────────────────────────────────

  Widget _capsCard(AppColors c) {
    final direct = (_caps?['direct'] as Map?)?.cast<String, dynamic>();
    final docker = (_caps?['docker'] as Map?)?.cast<String, dynamic>();
    final anyOk = direct?['available'] == true || docker?['available'] == true;

    Widget capLine(String name,
        {required bool ok, required String tag, required String detail}) {
      return Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
              width: 48,
              child: Text(name,
                  style: TextStyle(color: c.textPrimary, fontSize: 12))),
          _Tag(tag, color: ok ? const Color(0x3322C55E) : c.surfaceAlt),
          const SizedBox(width: AppTokens.s8),
          Expanded(
            child: Text(detail,
                style: TextStyle(color: c.textMuted, fontSize: 11.5)),
          ),
        ],
      );
    }

    return Container(
      padding: const EdgeInsets.all(AppTokens.s12),
      decoration: BoxDecoration(
        color: anyOk
            ? c.accentSoft.withValues(alpha: 0.25)
            : const Color(0x22F59E0B),
        borderRadius: BorderRadius.circular(AppTokens.rMd),
        border: Border.all(color: c.border),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Icon(anyOk ? Icons.verified_user_outlined : Icons.warning_amber,
                  size: 15, color: anyOk ? c.accent : const Color(0xFFF59E0B)),
              const SizedBox(width: 6),
              Text(t('Available isolation'),
                  style: TextStyle(
                      color: c.textPrimary,
                      fontSize: 12.5,
                      fontWeight: FontWeight.w600)),
            ],
          ),
          const SizedBox(height: 6),
          capLine('direct',
              ok: direct?['available'] == true,
              tag: '${direct?['kind'] ?? '—'}',
              detail: '${direct?['detail'] ?? ''}'),
          const SizedBox(height: 4),
          capLine('docker',
              ok: docker?['available'] == true,
              tag: docker?['available'] == true
                  ? t('ready')
                  : t('no'),
              detail: '${docker?['detail'] ?? ''}'),
        ],
      ),
    );
  }

  // ── Cơ chế bảo mật ────────────────────────────────────────────────────────

  Widget _policyCard(AppColors c) {
    final p = _policy!;
    Widget toggle(String title, String desc, String key,
        {List<Widget> children = const []}) {
      return Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(t(title),
                        style: TextStyle(
                            color: c.textPrimary,
                            fontSize: 13,
                            fontWeight: FontWeight.w600)),
                    Text(t(desc),
                        style: TextStyle(color: c.textMuted, fontSize: 11.5)),
                  ],
                ),
              ),
              Switch(
                value: p[key] == true,
                onChanged: (v) => _savePolicy({key: v}),
              ),
            ],
          ),
          if (p[key] == true) ...children,
          const SizedBox(height: AppTokens.s8),
        ],
      );
    }

    return _Card(
      title: t('Security enforcement — run through the sandbox'),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          toggle(
            'Exec (agent Bash tool)',
            'Agent shell commands run inside the OS sandbox and can only '
                "write to the chat's working directory. Note: build caches "
                'outside the workspace (npm/cargo…) will be blocked from '
                'writing.',
            'execShell',
            children: [
              Padding(
                padding: const EdgeInsets.only(left: AppTokens.s12, top: 4),
                child: Row(
                  children: [
                    Text(t('Network'),
                        style: TextStyle(color: c.textSecondary, fontSize: 12)),
                    const SizedBox(width: 4),
                    Switch(
                      value: p['execNetwork'] == true,
                      onChanged: (v) => _savePolicy({'execNetwork': v}),
                    ),
                    const SizedBox(width: AppTokens.s16),
                    Text(t('Disk read'),
                        style: TextStyle(color: c.textSecondary, fontSize: 12)),
                    const SizedBox(width: AppTokens.s8),
                    _fsModeDropdown(
                      value: '${p['execFsMode'] ?? 'open'}',
                      onChanged: (v) => _savePolicy({'execFsMode': v}),
                    ),
                  ],
                ),
              ),
              Padding(
                padding: const EdgeInsets.only(left: AppTokens.s12, top: 6),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                        t("Local ports the agent's shell may call (e.g. the "
                            'dev server it is working on). Empty = none: '
                            "loopback is where SenClaw's own API and every "
                            'Space App live, and none of them ask for '
                            'credentials.'),
                        style: TextStyle(color: c.textMuted, fontSize: 11.5)),
                    const SizedBox(height: 4),
                    SizedBox(
                      height: 34,
                      child: TextFormField(
                        key: ValueKey('execLoopback:${_loopbackText(p)}'),
                        initialValue: _loopbackText(p),
                        style: TextStyle(
                            fontSize: 12,
                            color: c.textPrimary,
                            fontFamily: 'monospace'),
                        decoration: InputDecoration(
                          isDense: true,
                          labelText: t('Local ports'),
                          hintText: '3000, 5173',
                          border: const OutlineInputBorder(),
                          contentPadding: const EdgeInsets.symmetric(
                              vertical: 6, horizontal: 8),
                        ),
                        onFieldSubmitted: _saveLoopback,
                      ),
                    ),
                  ],
                ),
              ),
            ],
          ),
          toggle(
              'Run Python',
              'Allow real Python (REPL + sbx tools). Always sandboxed; '
                  'switching off refuses to run.',
              'runPython'),
          toggle(
              'Run Node.js',
              'Allow real Node.js. Always sandboxed; switching off refuses '
                  'to run.',
              'runNode'),
          toggle(
              'Network for Python/Node',
              'Off by default — enable when a snippet needs network access.',
              'codeNetwork'),
          toggle(
            'Scheduled scripts (scheduler)',
            'script / script-agent task commands run in a throwaway sandbox.',
            'schedulerScript',
            children: [
              Padding(
                padding: const EdgeInsets.only(left: AppTokens.s12, top: 4),
                child: Row(
                  children: [
                    Text(t('Network'),
                        style: TextStyle(color: c.textSecondary, fontSize: 12)),
                    const SizedBox(width: 4),
                    Switch(
                      value: p['schedulerNetwork'] == true,
                      onChanged: (v) => _savePolicy({'schedulerNetwork': v}),
                    ),
                  ],
                ),
              ),
            ],
          ),
        ],
      ),
    );
  }

  String _loopbackText(Map<String, dynamic> p) =>
      ((p['execLoopback'] as List?) ?? const []).join(', ');

  Future<void> _saveLoopback(String raw) async {
    final ports = raw
        .split(RegExp(r'[,\s]+'))
        .map((s) => int.tryParse(s.trim()))
        .whereType<int>()
        .where((n) => n > 0 && n < 65536)
        .toList();
    await _savePolicy({'execLoopback': ports});
  }

  Widget _fsModeDropdown(
      {required String value, required ValueChanged<String> onChanged}) {
    return DropdownButton<String>(
      value: const ['strict', 'allowlist', 'open'].contains(value)
          ? value
          : 'strict',
      isDense: true,
      style: TextStyle(fontSize: 12, color: context.colors.textPrimary),
      items: const [
        DropdownMenuItem(value: 'strict', child: Text('strict')),
        DropdownMenuItem(value: 'allowlist', child: Text('allowlist')),
        DropdownMenuItem(value: 'open', child: Text('open')),
      ],
      onChanged: (v) {
        if (v != null) onChanged(v);
      },
    );
  }

  // ── Luồng đang quản lý ────────────────────────────────────────────────────

  /// Space Apps with a server process: what each is configured to get, and what
  /// the process that is actually running was given.
  ///
  /// Those two differ the moment someone edits the settings without restarting —
  /// a profile is fixed at launch — and this is the only screen where the gap is
  /// visible, so it is called out with the restart that closes it.
  Widget _appsCard(AppColors c) {
    return _Card(
      title: '${t('Space Apps — per-app sandbox')} (${_apps.length})',
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          if (_appCaps != null && _appCaps!['enforceable'] != true)
            Padding(
              padding: const EdgeInsets.only(bottom: AppTokens.s8),
              child: Text(
                  t('This machine cannot confine a served app (isolation: {kind}) — the switches are stored but not enforced.')
                      .replaceFirst('{kind}', '${_appCaps!['isolation']}'),
                  style: TextStyle(color: AppTokens.warning, fontSize: 11.5)),
            ),
          if (_apps.isEmpty)
            Text(t('No server Space App installed'),
                style: TextStyle(color: c.textMuted, fontSize: 12)),
          if (_apps.length > 1) _appSortBar(c),
          for (final a in _pageOfApps) _appRow(c, a),
          if (_apps.length > _appsPerPage) _appPager(c),
        ],
      ),
    );
  }

  static const _appsPerPage = 10;

  /// Sort keys and their labels, in menu order.
  static const _appSortKeys = <String, String>{
    'status': 'Status',
    'name': 'Name',
    'sandbox': 'Sandbox',
    'cpu': 'CPU',
    'ram': 'RAM',
    'launches': 'Launches',
  };

  String _appName(Map<String, dynamic> a) =>
      '${a['name'] ?? a['id'] ?? ''}'.toLowerCase();

  List<Map<String, dynamic>> get _sortedApps {
    double num_(Map<String, dynamic> m, String k) =>
        (m[k] as num?)?.toDouble() ?? -1; // not running sorts below 0.0%
    int primary(Map<String, dynamic> a, Map<String, dynamic> b) {
      switch (_appSort) {
        case 'name':
          return _appName(a).compareTo(_appName(b));
        case 'cpu':
          return num_(a, 'cpu').compareTo(num_(b, 'cpu'));
        case 'ram':
          return num_(a, 'rssMb').compareTo(num_(b, 'rssMb'));
        case 'launches':
          return num_(a, 'launches').compareTo(num_(b, 'launches'));
        case 'sandbox':
          int on(Map<String, dynamic> m) =>
              ((m['config'] as Map?)?['enabled'] == true) ? 1 : 0;
          return on(a).compareTo(on(b));
        default:
          int run(Map<String, dynamic> m) => m['running'] == true ? 1 : 0;
          return run(a).compareTo(run(b));
      }
    }

    final list = [..._apps];
    list.sort((a, b) {
      final r = primary(a, b);
      // Direction applies to the chosen key only — ties stay A→Z, so flipping
      // the arrow never scrambles the alphabetical fallback.
      if (r != 0) return _appSortAsc ? r : -r;
      return _appName(a).compareTo(_appName(b));
    });
    return list;
  }

  Widget _appSortBar(AppColors c) => Padding(
        padding: const EdgeInsets.only(bottom: AppTokens.s6),
        child: Row(mainAxisAlignment: MainAxisAlignment.end, children: [
          Text(t('Sort by'),
              style: TextStyle(color: c.textMuted, fontSize: 11.5)),
          const SizedBox(width: AppTokens.s6),
          DropdownButton<String>(
            // Keyed: this panel has other string dropdowns (fs-mode), and
            // "the last one" is not a stable way to reach this one.
            key: const ValueKey('appSortDropdown'),
            value: _appSort,
            isDense: true,
            underline: const SizedBox.shrink(),
            style: TextStyle(color: c.textPrimary, fontSize: 12),
            onChanged: (v) => setState(() {
              _appSort = v ?? 'status';
              _appPage = 0; // a new order makes the old page number meaningless
            }),
            items: [
              for (final e in _appSortKeys.entries)
                DropdownMenuItem(
                    value: e.key,
                    child: Text(t(e.value),
                        style: const TextStyle(fontSize: 12))),
            ],
          ),
          IconButton(
            tooltip: _appSortAsc ? t('Ascending') : t('Descending'),
            icon: Icon(
                _appSortAsc ? Icons.arrow_upward : Icons.arrow_downward,
                size: 15),
            onPressed: () => setState(() {
              _appSortAsc = !_appSortAsc;
              _appPage = 0;
            }),
          ),
        ]),
      );

  /// Clamped on read, not on write: the list is reloaded every few seconds and
  /// an app can be uninstalled while you are on the last page.
  int get _appPageCount => ((_apps.length - 1) ~/ _appsPerPage) + 1;

  List<Map<String, dynamic>> get _pageOfApps {
    if (_apps.isEmpty) return const [];
    final page = _appPage.clamp(0, _appPageCount - 1);
    return _sortedApps.skip(page * _appsPerPage).take(_appsPerPage).toList();
  }

  Widget _appPager(AppColors c) {
    final page = _appPage.clamp(0, _appPageCount - 1);
    final from = page * _appsPerPage + 1;
    final to = (from + _appsPerPage - 1).clamp(1, _apps.length);
    return Padding(
      padding: const EdgeInsets.only(top: AppTokens.s8),
      child: Row(mainAxisAlignment: MainAxisAlignment.end, children: [
        Text('$from–$to / ${_apps.length}',
            style: TextStyle(color: c.textMuted, fontSize: 11.5)),
        IconButton(
          icon: const Icon(Icons.chevron_left, size: 18),
          tooltip: t('Previous page'),
          onPressed: page == 0 ? null : () => setState(() => _appPage = page - 1),
        ),
        IconButton(
          icon: const Icon(Icons.chevron_right, size: 18),
          tooltip: t('Next page'),
          onPressed: page >= _appPageCount - 1
              ? null
              : () => setState(() => _appPage = page + 1),
        ),
      ]),
    );
  }

  Widget _appRow(AppColors c, Map<String, dynamic> a) {
    final cfg = (a['config'] as Map?)?.cast<String, dynamic>() ?? const {};
    final on = cfg['enabled'] == true;
    final running = a['running'] == true;
    // Up, but this daemon did not launch it: `ensure_server_running` reuses a
    // healthy port instead of double-launching, so an app that outlived a daemon
    // restart is adopted — and used to be reported here as "not running".
    final adopted = a['adopted'] == true;
    final isolation = '${a['isolation'] ?? ''}';
    // Two ways a "confined" app can be running unconfined: the profile predates
    // the setting, or no profile was ever built because the process was adopted.
    final stale = running && on && (isolation == 'none' || adopted);
    final launches = (a['launches'] as num?)?.toInt() ?? 0;
    final cpu = (a['cpu'] as num?)?.toDouble();
    final rss = (a['rssMb'] as num?)?.toDouble();
    final proxy = (a['proxy'] as Map?)?.cast<String, dynamic>();
    final denied =
        ((proxy?['stats'] as Map?)?['denied'] as num?)?.toInt() ?? 0;

    String netLabel(String v) => switch (v) {
          'all' => t('everything'),
          'hosts' => t('only some sites'),
          'off' => t('no network'),
          _ => v,
        };

    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 5),
      child: Row(children: [
        Text('${a['icon'] ?? '🧩'} ', style: const TextStyle(fontSize: 13)),
        Expanded(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text('${a['name'] ?? a['id']}',
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                      color: c.textPrimary,
                      fontSize: 12.5,
                      fontWeight: FontWeight.w600)),
              Text(
                  !running
                      ? t('not running')
                      : adopted
                          // No launch count for a process we did not start, and
                          // no pretending we know one.
                          ? '${t('adopted')} · pid ${a['pid']} · ${_upFor(a)}'
                          : 'pid ${a['pid']} · ${_upFor(a)} · $launches×',
                  style: TextStyle(
                      color: adopted
                          ? c.accent
                          : (launches > 3 ? AppTokens.warning : c.textMuted),
                      fontSize: 11)),
            ],
          ),
        ),
        Wrap(spacing: 4, runSpacing: 4, children: [
          if (!on)
            _Tag(t('off'), color: c.textMuted)
          else ...[
            _Tag(
                !running
                    ? t('enabled')
                    : adopted
                        ? t('unknown')
                        : isolation,
                color: stale ? AppTokens.warning : AppTokens.success),
            if (stale) _Tag(t('restart needed'), color: AppTokens.warning),
            _Tag('${cfg['readMode']}', color: c.accent),
            _Tag(netLabel('${cfg['network']}'),
                color: cfg['network'] == 'all' ? c.textMuted : c.accent),
            if (denied > 0)
              _Tag(t('{n} refused').replaceFirst('{n}', '$denied'),
                  color: AppTokens.warning),
          ],
        ]),
        const SizedBox(width: AppTokens.s8),
        SizedBox(
          width: 92,
          child: Text(
              running && cpu != null
                  ? '${cpu.toStringAsFixed(1)}% · ${(rss ?? 0).toStringAsFixed(0)} MB'
                  : '—',
              textAlign: TextAlign.right,
              style: TextStyle(color: c.textSecondary, fontSize: 11)),
        ),
        IconButton(
          tooltip: t('Process monitor'),
          icon: const Icon(Icons.monitor_heart_outlined, size: 15),
          onPressed: () => showDialog(
            context: context,
            builder: (_) => SpaceAppMonitorDialog(
                appId: '${a['id']}', appName: '${a['name'] ?? a['id']}'),
          ),
        ),
        IconButton(
          tooltip: t('Sandbox settings'),
          icon: const Icon(Icons.science_outlined, size: 15),
          onPressed: () => showDialog(
            context: context,
            builder: (_) => SpaceAppSandboxDialog(
                appId: '${a['id']}', appName: '${a['name'] ?? a['id']}'),
          ).then((_) => _refresh()),
        ),
        IconButton(
          tooltip: t('Restart'),
          icon: const Icon(Icons.refresh, size: 15),
          onPressed: () => _restartApp('${a['id']}'),
        ),
      ]),
    );
  }

  String _upFor(Map<String, dynamic> a) {
    final raw = a['uptimeMs'] as num?;
    if (raw == null) return '?';
    final ms = raw.toInt();
    final s = ms ~/ 1000;
    if (s < 60) return '${s}s';
    if (s < 3600) return '${s ~/ 60}m';
    return '${s ~/ 3600}h ${(s % 3600) ~/ 60}m';
  }

  Widget _sandboxesCard(AppColors c) {
    return _Card(
      title: t('Managed sandboxes ({n})')
          .replaceFirst('{n}', '${_sandboxes.length}'),
      child: _sandboxes.isEmpty
          ? Padding(
              padding: const EdgeInsets.all(AppTokens.s12),
              child: Text(
                  t('No sandboxes yet — the agent creates one when it runs '
                      'code, or create one with sbx_create.'),
                  style: TextStyle(color: c.textMuted, fontSize: 12)),
            )
          : Column(
              children: [
                for (final sb in _sandboxes)
                  _SandboxRow(
                    key: ValueKey(sb.id),
                    sandbox: sb,
                    tr: t,
                    onKillAll: () => _killAll(sb),
                    onDelete: () => _delete(sb, purge: false),
                    onPurge: () => _delete(sb, purge: true),
                  ),
              ],
            ),
    );
  }

  // ── Lịch sử chạy ──────────────────────────────────────────────────────────

  Widget _runsCard(AppColors c) {
    return _Card(
      title: t('Recent runs'),
      child: _runs.isEmpty
          ? Padding(
              padding: const EdgeInsets.all(AppTokens.s12),
              child: Text(t('No runs yet.'),
                  style: TextStyle(color: c.textMuted, fontSize: 12)),
            )
          : Column(
              children: [
                for (final r in _runs)
                  Padding(
                    padding:
                        const EdgeInsets.symmetric(vertical: 3, horizontal: 4),
                    child: Row(
                      children: [
                        SizedBox(
                          width: 118,
                          child: Text(_fmtTime(r.createdAt),
                              style:
                                  TextStyle(color: c.textMuted, fontSize: 11)),
                        ),
                        _Tag(r.language ?? r.kind, color: c.accentSoft),
                        const SizedBox(width: AppTokens.s8),
                        Expanded(
                          child: Text(r.source.replaceAll('\n', ' '),
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                              style: TextStyle(
                                  color: c.textSecondary,
                                  fontSize: 11.5,
                                  fontFamily: 'monospace')),
                        ),
                        const SizedBox(width: AppTokens.s8),
                        _Tag(
                          r.timedOut
                              ? 'timeout'
                              : r.exitCode == 0
                                  ? 'exit 0'
                                  : 'exit ${r.exitCode ?? "?"}',
                          color: r.timedOut || r.exitCode != 0
                              ? const Color(0x33EF4444)
                              : const Color(0x3322C55E),
                        ),
                        const SizedBox(width: AppTokens.s8),
                        _Tag(r.isolation, color: c.surfaceAlt),
                        const SizedBox(width: AppTokens.s8),
                        SizedBox(
                          width: 56,
                          child: Text('${r.durationMs} ms',
                              textAlign: TextAlign.right,
                              style:
                                  TextStyle(color: c.textMuted, fontSize: 11)),
                        ),
                      ],
                    ),
                  ),
              ],
            ),
    );
  }

  // ── Mặc định cho sandbox mới ──────────────────────────────────────────────

  Widget _defaultsCard(AppColors c) {
    final d = _defaults!;
    Widget numField(String label, String key, {int? min, int? max}) {
      return Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Text(label, style: TextStyle(color: c.textSecondary, fontSize: 12)),
          const SizedBox(width: AppTokens.s8),
          SizedBox(
            width: 90,
            height: 30,
            child: TextFormField(
              key: ValueKey('$key:${d[key]}'),
              initialValue: '${d[key] ?? ''}',
              keyboardType: TextInputType.number,
              style: TextStyle(fontSize: 12, color: c.textPrimary),
              decoration: const InputDecoration(
                isDense: true,
                contentPadding:
                    EdgeInsets.symmetric(vertical: 6, horizontal: 8),
                border: OutlineInputBorder(),
              ),
              onFieldSubmitted: (v) {
                final n = num.tryParse(v);
                if (n == null) return;
                var clamped = n;
                if (min != null && clamped < min) clamped = min;
                if (max != null && clamped > max) clamped = max;
                _saveDefaults({key: clamped});
              },
            ),
          ),
        ],
      );
    }

    return _Card(
      title: t('Defaults for new sandboxes'),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Wrap(
            spacing: AppTokens.s16,
            runSpacing: AppTokens.s8,
            crossAxisAlignment: WrapCrossAlignment.center,
            children: [
              Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Text(t('Default disk read'),
                      style: TextStyle(color: c.textSecondary, fontSize: 12)),
                  const SizedBox(width: AppTokens.s8),
                  _fsModeDropdown(
                    value: '${d['defaultFsMode'] ?? 'strict'}',
                    onChanged: (v) => _saveDefaults({'defaultFsMode': v}),
                  ),
                ],
              ),
              Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Text(t('Default network'),
                      style: TextStyle(color: c.textSecondary, fontSize: 12)),
                  Switch(
                    value: d['defaultNetwork'] == true,
                    onChanged: (v) => _saveDefaults({'defaultNetwork': v}),
                  ),
                ],
              ),
              numField(t('RAM (MB)'), 'defaultMemoryMb', min: 64, max: 65536),
              numField(t('CPU'), 'defaultCpus', min: 1, max: 32),
              numField(t('Deadline (ms)'), 'defaultTimeoutMs',
                  min: 1000, max: 600000),
            ],
          ),
          const SizedBox(height: AppTokens.s12),
          Text(
              t('Allowlist — extra folders the sandbox may READ in allowlist '
                  'mode (writes stay blocked)'),
              style: TextStyle(color: c.textSecondary, fontSize: 12)),
          const SizedBox(height: 6),
          if (_allowlist.isEmpty)
            Padding(
              padding: const EdgeInsets.only(bottom: 6),
              child: Text(t('No folders yet.'),
                  style: TextStyle(color: c.textMuted, fontSize: 11.5)),
            )
          else
            Column(
              children: [
                for (final path in _allowlist)
                  Padding(
                    padding: const EdgeInsets.only(bottom: 2),
                    child: Row(
                      children: [
                        Icon(Icons.folder_outlined,
                            size: 15, color: c.textMuted),
                        const SizedBox(width: 6),
                        Expanded(
                          child: Text(path,
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                              style: TextStyle(
                                  color: c.textPrimary,
                                  fontSize: 12,
                                  fontFamily: 'monospace')),
                        ),
                        IconButton(
                          tooltip: t('Remove from allowlist'),
                          icon: const Icon(Icons.close, size: 15),
                          visualDensity: VisualDensity.compact,
                          onPressed: () => _removeAllowlistPath(path),
                        ),
                      ],
                    ),
                  ),
              ],
            ),
          Row(
            children: [
              OutlinedButton.icon(
                onPressed: _pickAllowlistFolder,
                icon: const Icon(Icons.create_new_folder_outlined, size: 16),
                label: Text(t('Choose folder…')),
              ),
              const SizedBox(width: AppTokens.s8),
              Expanded(
                child: SizedBox(
                  height: 30,
                  child: TextField(
                    controller: _manualPathCtl,
                    style: TextStyle(
                        fontSize: 12,
                        color: c.textPrimary,
                        fontFamily: 'monospace'),
                    decoration: InputDecoration(
                      isDense: true,
                      hintText: t('or type an absolute path: /Users/you/data'),
                      border: const OutlineInputBorder(),
                      contentPadding: const EdgeInsets.symmetric(
                          vertical: 6, horizontal: 8),
                    ),
                    onSubmitted: (v) => _addAllowlistPath(v),
                  ),
                ),
              ),
              const SizedBox(width: 4),
              IconButton(
                tooltip: t('Add path'),
                icon: const Icon(Icons.add, size: 18),
                onPressed: () => _addAllowlistPath(_manualPathCtl.text),
              ),
            ],
          ),
        ],
      ),
    );
  }

  List<String> get _allowlist =>
      ((_defaults?['allowlist'] as List?) ?? const [])
          .map((e) => '$e')
          .where((s) => s.isNotEmpty)
          .toList();

  Future<void> _pickAllowlistFolder() async {
    final dir = await FilePicker.platform
        .getDirectoryPath(dialogTitle: t('Choose a folder the sandbox may read'));
    if (dir == null || dir.isEmpty) return; // user bấm Huỷ
    await _addAllowlistPath(dir);
  }

  Future<void> _addAllowlistPath(String raw) async {
    final path = raw.trim();
    if (path.isEmpty) return;
    // Tuyệt đối mới có nghĩa với Seatbelt/bwrap: '/' (mac/Linux) hoặc 'C:\' (Windows).
    final absolute =
        path.startsWith('/') || RegExp(r'^[A-Za-z]:[\\/]').hasMatch(path);
    if (!absolute) {
      _snack(t('An absolute path is required (starts with / or C:\\)'));
      return;
    }
    final current = _allowlist;
    if (current.contains(path)) {
      _snack(t('Already in the allowlist'));
      return;
    }
    _manualPathCtl.clear();
    await _saveDefaults({}, allowlist: [...current, path]);
  }

  Future<void> _removeAllowlistPath(String path) async {
    await _saveDefaults({},
        allowlist: _allowlist.where((p) => p != path).toList());
  }
}

// ── Hàng sandbox: mở rộng xem CPU/RAM + tiến trình ──────────────────────────

class _SandboxRow extends ConsumerStatefulWidget {
  const _SandboxRow({
    super.key,
    required this.sandbox,
    required this.tr,
    required this.onKillAll,
    required this.onDelete,
    required this.onPurge,
  });

  final SandboxInfo sandbox;
  final String Function(String) tr;
  final VoidCallback onKillAll;
  final VoidCallback onDelete;
  final VoidCallback onPurge;

  @override
  ConsumerState<_SandboxRow> createState() => _SandboxRowState();
}

class _SandboxRowState extends ConsumerState<_SandboxRow> {
  bool _expanded = false;
  Map<String, dynamic>? _stats;
  Timer? _poll;

  @override
  void dispose() {
    _poll?.cancel();
    super.dispose();
  }

  Future<void> _fetchStats() async {
    try {
      final r = await ref
          .read(apiClientProvider)
          .get('/api/sandbox/sandboxes/${widget.sandbox.id}/stats');
      if (!mounted || r is! Map) return;
      setState(() => _stats = r.cast<String, dynamic>());
    } catch (_) {
      // Row có thể vừa bị xoá — bỏ qua, lần poll sau sẽ dừng theo expand.
    }
  }

  void _toggle() {
    setState(() => _expanded = !_expanded);
    _poll?.cancel();
    if (_expanded) {
      _fetchStats();
      _poll = Timer.periodic(const Duration(seconds: 3), (_) {
        if (_expanded) _fetchStats();
      });
    }
  }

  Future<void> _killPid(int pid) async {
    try {
      await ref.read(apiClientProvider).post(
          '/api/sandbox/sandboxes/${widget.sandbox.id}/kill',
          body: {'pid': pid});
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(
          content:
              Text(widget.tr('Stopped process {pid}').replaceFirst('{pid}', '$pid'))));
      _fetchStats();
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(
          content:
              Text(widget.tr('Stop failed: {e}').replaceFirst('{e}', '$e'))));
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final sb = widget.sandbox;
    final tr = widget.tr;
    return Container(
      margin: const EdgeInsets.symmetric(vertical: 3),
      decoration: BoxDecoration(
        border: Border.all(color: c.border),
        borderRadius: BorderRadius.circular(AppTokens.rMd),
      ),
      child: Column(
        children: [
          InkWell(
            onTap: _toggle,
            borderRadius: BorderRadius.circular(AppTokens.rMd),
            child: Padding(
              padding: const EdgeInsets.symmetric(
                  horizontal: AppTokens.s12, vertical: AppTokens.s8),
              child: Row(
                children: [
                  Icon(_expanded ? Icons.expand_more : Icons.chevron_right,
                      size: 16, color: c.textMuted),
                  const SizedBox(width: AppTokens.s8),
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(sb.name,
                            style: TextStyle(
                                color: c.textPrimary,
                                fontSize: 13,
                                fontWeight: FontWeight.w600)),
                        Text(sb.workdir,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: TextStyle(color: c.textMuted, fontSize: 11)),
                      ],
                    ),
                  ),
                  _Tag(sb.backend, color: c.accentSoft),
                  const SizedBox(width: 4),
                  _Tag(sb.fsMode, color: c.surfaceAlt),
                  const SizedBox(width: 4),
                  _Tag(sb.network ? tr('net: on') : tr('net: off'),
                      color: sb.network
                          ? const Color(0x33F59E0B)
                          : const Color(0x3322C55E)),
                  const SizedBox(width: AppTokens.s8),
                  Text(
                      '${sb.cpus.toStringAsFixed(sb.cpus % 1 == 0 ? 0 : 1)} CPU · ${sb.memoryMb} MB',
                      style: TextStyle(color: c.textMuted, fontSize: 11)),
                  const SizedBox(width: AppTokens.s8),
                  Tooltip(
                    message: sb.lastError ?? sb.status,
                    child: _Tag(sb.status,
                        color: sb.status == 'running'
                            ? const Color(0x3322C55E)
                            : sb.status == 'error'
                                ? const Color(0x33EF4444)
                                : c.surfaceAlt),
                  ),
                  const SizedBox(width: AppTokens.s8),
                  IconButton(
                    tooltip: tr('Stop all processes'),
                    icon: const Icon(Icons.stop_circle_outlined, size: 17),
                    onPressed: widget.onKillAll,
                  ),
                  IconButton(
                    tooltip: tr('Delete (keep files)'),
                    icon: const Icon(Icons.delete_outline, size: 17),
                    onPressed: widget.onDelete,
                  ),
                ],
              ),
            ),
          ),
          if (_expanded) _expandedBody(c),
        ],
      ),
    );
  }

  Widget _expandedBody(AppColors c) {
    final tr = widget.tr;
    final s = _stats;
    final procs = (s?['processes'] as List?)?.whereType<Map>().toList() ??
        const <Map>[];
    return Container(
      width: double.infinity,
      padding: const EdgeInsets.fromLTRB(
          AppTokens.s16, AppTokens.s8, AppTokens.s16, AppTokens.s12),
      decoration: BoxDecoration(
        border: Border(top: BorderSide(color: c.border)),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Text(
                s == null
                    ? tr('Measuring…')
                    : 'CPU ${((s['cpu'] as num?) ?? 0).toStringAsFixed(1)}%  ·  RAM ${((s['rssMb'] as num?) ?? 0).toStringAsFixed(0)} MB'
                        '${s['memoryLimitMb'] != null ? ' / ${s['memoryLimitMb']} MB' : ''}',
                style: TextStyle(
                    color: c.textPrimary,
                    fontSize: 12,
                    fontWeight: FontWeight.w600),
              ),
              if (s?['note'] != null) ...[
                const SizedBox(width: AppTokens.s8),
                Expanded(
                  child: Text('${s!['note']}',
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(color: c.textMuted, fontSize: 11)),
                ),
              ] else
                const Spacer(),
              TextButton(
                onPressed: widget.onPurge,
                style: TextButton.styleFrom(
                    foregroundColor: const Color(0xFFEF4444)),
                child: Text(tr('Delete with files'),
                    style: const TextStyle(fontSize: 12)),
              ),
            ],
          ),
          if (procs.isEmpty)
            Text(tr('No processes running.'),
                style: TextStyle(color: c.textMuted, fontSize: 11.5))
          else
            Column(
              children: [
                for (final p in procs)
                  Row(
                    children: [
                      SizedBox(
                        width: 64,
                        child: Text('${p['pid']}',
                            style: TextStyle(
                                color: c.textSecondary,
                                fontSize: 11,
                                fontFamily: 'monospace')),
                      ),
                      SizedBox(
                        width: 70,
                        child: Text(
                            '${((p['cpu'] as num?) ?? 0).toStringAsFixed(1)}%',
                            style: TextStyle(
                                color: c.textSecondary, fontSize: 11)),
                      ),
                      SizedBox(
                        width: 80,
                        child: Text(
                            '${((p['rssMb'] as num?) ?? 0).toStringAsFixed(0)} MB',
                            style: TextStyle(
                                color: c.textSecondary, fontSize: 11)),
                      ),
                      Expanded(
                        child: Text('${p['command'] ?? ''}',
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: TextStyle(
                                color: c.textMuted,
                                fontSize: 11,
                                fontFamily: 'monospace')),
                      ),
                      IconButton(
                        tooltip: tr('Stop this process'),
                        icon: const Icon(Icons.close, size: 14),
                        onPressed: () {
                          final pid = (p['pid'] as num?)?.toInt();
                          if (pid != null) _killPid(pid);
                        },
                      ),
                    ],
                  ),
              ],
            ),
        ],
      ),
    );
  }
}

// ── Small shared widgets ────────────────────────────────────────────────────

class _Card extends StatelessWidget {
  const _Card({required this.title, required this.child});
  final String title;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Container(
      decoration: BoxDecoration(
        color: c.surface,
        borderRadius: BorderRadius.circular(AppTokens.rMd),
        border: Border.all(color: c.border),
      ),
      padding: const EdgeInsets.all(AppTokens.s12),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(title,
              style: TextStyle(
                  color: c.textPrimary,
                  fontSize: 13,
                  fontWeight: FontWeight.w700)),
          const SizedBox(height: AppTokens.s8),
          child,
        ],
      ),
    );
  }
}

class _Tag extends StatelessWidget {
  const _Tag(this.text, {required this.color});
  final String text;
  final Color color;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 1.5),
      decoration: BoxDecoration(
        color: color,
        borderRadius: BorderRadius.circular(4),
      ),
      child: Text(text,
          style: TextStyle(
              color: c.textPrimary,
              fontSize: 10.5,
              fontWeight: FontWeight.w600)),
    );
  }
}

String _fmtTime(int ms) {
  if (ms <= 0) return '—';
  final d = DateTime.fromMillisecondsSinceEpoch(ms);
  String two(int n) => n.toString().padLeft(2, '0');
  return '${two(d.day)}/${two(d.month)} ${two(d.hour)}:${two(d.minute)}:${two(d.second)}';
}
