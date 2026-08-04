import 'dart:async';

import 'package:flutter/material.dart';

import '../../models/background_models.dart';
import '../../services/background_api.dart';
import '../../services/language_service.dart';
import '../../services/relay_manager.dart';
import '../../theme/tokens.dart';
import '../../widgets/states.dart';

/// Background tasks — autonomous daemon work (no chat session): list + stats,
/// quick-create via the daemon's LLM, run history and per-run transcripts.
///
/// The relay does not forward `bg:*` WS events, so this screen refreshes on a
/// timer while visible instead of following a live stream.
class BackgroundScreen extends StatefulWidget {
  const BackgroundScreen({super.key});

  @override
  State<BackgroundScreen> createState() => _BackgroundScreenState();
}

class _BackgroundScreenState extends State<BackgroundScreen> {
  final _api = BackgroundApi();

  List<BackgroundTask> _tasks = [];
  int _total = 0;
  BackgroundStats? _stats;
  bool _loading = true;
  bool _loadingMore = false;
  String? _error;

  String _window = '7d';
  String? _statusFilter;
  bool _showInternal = false;
  Timer? _pollTimer;

  static const _pageSize = 20;

  @override
  void initState() {
    super.initState();
    _load();
    // The daemon is the only thing that knows a background run started —
    // nothing pushes over the relay — so poll while the screen is open.
    _pollTimer = Timer.periodic(
        const Duration(seconds: 30), (_) => _load(silent: true));
  }

  @override
  void dispose() {
    _pollTimer?.cancel();
    super.dispose();
  }

  Future<void> _load({bool silent = false}) async {
    if (!silent) {
      setState(() {
        _loading = _tasks.isEmpty;
        _error = null;
      });
    }
    try {
      final results = await Future.wait([
        _api.listTasks(
          includeInternal: _showInternal,
          status: _statusFilter,
          limit: _pageSize,
        ),
        _api.stats(_window),
      ]);
      if (!mounted) return;
      final page = results[0] as BgTaskPage;
      setState(() {
        _tasks = page.tasks;
        _total = page.total;
        _stats = results[1] as BackgroundStats;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = _tasks.isEmpty ? '$e' : null;
        _loading = false;
      });
    }
  }

  Future<void> _loadMore() async {
    if (_loadingMore) return;
    setState(() => _loadingMore = true);
    try {
      final page = await _api.listTasks(
        includeInternal: _showInternal,
        status: _statusFilter,
        limit: _pageSize,
        offset: _tasks.length,
      );
      if (!mounted) return;
      setState(() {
        _tasks = [..._tasks, ...page.tasks];
        _total = page.total;
      });
    } catch (e) {
      if (mounted) _toast(tr('Lỗi: $e', 'Error: $e'));
    } finally {
      if (mounted) setState(() => _loadingMore = false);
    }
  }

  void _toast(String msg) => ScaffoldMessenger.of(context)
      .showSnackBar(SnackBar(content: Text(msg)));

  Future<void> _act(Future<void> Function() fn, String okMsg) async {
    try {
      await fn();
      if (mounted && okMsg.isNotEmpty) _toast(okMsg);
      _load(silent: true);
    } catch (e) {
      if (mounted) _toast(tr('Lỗi: $e', 'Error: $e'));
    }
  }

  Future<void> _runNow(BackgroundTask t) async {
    try {
      final runId = await _api.runNow(t.id);
      _load(silent: true);
      if (!mounted) return;
      if (runId.isNotEmpty) {
        Navigator.of(context).push(MaterialPageRoute(
            builder: (_) => _SessionScreen(runId: runId, title: t.title)));
      } else {
        _toast(tr('Đã chạy', 'Run started'));
      }
    } catch (e) {
      if (mounted) _toast(tr('Lỗi: $e', 'Error: $e'));
    }
  }

  Future<void> _confirmDelete(BackgroundTask t) async {
    final c = context.colors;
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: c.surface,
        title: Text(tr('Xoá task?', 'Delete task?'),
            style: TextStyle(color: c.textPrimary)),
        content: Text(t.title, style: TextStyle(color: c.textSecondary)),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(ctx, false),
              child: Text(tr('Huỷ', 'Cancel'))),
          TextButton(
              onPressed: () => Navigator.pop(ctx, true),
              child: Text(tr('Xoá', 'Delete'),
                  style: const TextStyle(color: AppTokens.danger))),
        ],
      ),
    );
    if (ok != true) return;
    await _act(() => _api.delete(t.id), tr('Đã xoá', 'Deleted'));
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
            Text(tr('Tác vụ nền', 'Background'),
                style: TextStyle(color: c.textPrimary)),
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
            tooltip: tr('Task hệ thống', 'System tasks'),
            icon: Icon(
              _showInternal ? Icons.visibility : Icons.visibility_off_outlined,
              color: _showInternal ? c.accent : c.textSecondary,
              size: 20,
            ),
            onPressed: () {
              setState(() => _showInternal = !_showInternal);
              _load();
            },
          ),
          PopupMenuButton<String>(
            tooltip: tr('Khoảng thống kê', 'Stats window'),
            color: c.surface,
            icon: Icon(Icons.history_toggle_off, color: c.textSecondary, size: 20),
            onSelected: (w) {
              setState(() => _window = w);
              _load();
            },
            itemBuilder: (_) => [
              for (final w in const ['24h', '7d', '30d'])
                PopupMenuItem(
                  value: w,
                  child: Text(w,
                      style: TextStyle(
                          color: w == _window ? c.accent : c.textPrimary)),
                ),
            ],
          ),
          IconButton(
            tooltip: tr('Tải lại', 'Reload'),
            icon: Icon(Icons.refresh, color: c.textSecondary),
            onPressed: _load,
          ),
        ],
      ),
      floatingActionButton: FloatingActionButton(
        onPressed: () => _showQuickDialog(context),
        backgroundColor: c.accent,
        foregroundColor: Colors.white,
        child: const Icon(Icons.bolt),
      ),
      body: _buildBody(),
    );
  }

  Widget _buildBody() {
    final c = context.colors;
    if (_loading) {
      return LoadingState(text: tr('Đang tải tác vụ…', 'Loading tasks…'));
    }
    if (_error != null) return ErrorState(message: _error!, onRetry: _load);

    final stats = _stats;
    return RefreshIndicator(
      onRefresh: _load,
      color: c.accent,
      backgroundColor: c.surface,
      child: ListView(
        padding: const EdgeInsets.fromLTRB(12, 12, 12, 88),
        children: [
          if (stats != null) _StatsCard(stats: stats, window: _window),
          if (stats != null && stats.attention.isNotEmpty)
            _AttentionBand(attention: stats.attention),
          const SizedBox(height: 8),
          _filterChips(),
          const SizedBox(height: 8),
          if (_tasks.isEmpty)
            Padding(
              padding: const EdgeInsets.only(top: 48),
              child: EmptyState(
                icon: Icons.motion_photos_auto_outlined,
                message: tr('Chưa có tác vụ nền', 'No background tasks yet'),
                hint: tr('Nhấn ⚡ để mô tả task bằng một câu',
                    'Tap ⚡ to describe a task in one line'),
              ),
            )
          else
            for (final t in _tasks) _taskCard(t),
          if (_tasks.length < _total)
            Padding(
              padding: const EdgeInsets.only(top: 4),
              child: Center(
                child: TextButton(
                  onPressed: _loadingMore ? null : _loadMore,
                  child: _loadingMore
                      ? const SizedBox(
                          width: 16,
                          height: 16,
                          child: CircularProgressIndicator(strokeWidth: 2))
                      : Text(
                          tr('Tải thêm (${_tasks.length}/$_total)',
                              'Load more (${_tasks.length}/$_total)'),
                          style: TextStyle(color: c.accent)),
                ),
              ),
            ),
        ],
      ),
    );
  }

  Widget _filterChips() {
    final c = context.colors;
    final filters = <(String?, String)>[
      (null, tr('Tất cả', 'All')),
      ('active', tr('Đang bật', 'Active')),
      ('paused', tr('Tạm dừng', 'Paused')),
      ('failed', tr('Lỗi', 'Failed')),
    ];
    return SingleChildScrollView(
      scrollDirection: Axis.horizontal,
      child: Row(
        children: [
          for (final (value, label) in filters)
            Padding(
              padding: const EdgeInsets.only(right: 6),
              child: ChoiceChip(
                label: Text(label, style: const TextStyle(fontSize: 12)),
                selected: _statusFilter == value,
                onSelected: (_) {
                  setState(() => _statusFilter = value);
                  _load();
                },
                selectedColor: c.accent.withValues(alpha: 0.3),
                backgroundColor: c.surfaceAlt,
                labelStyle: TextStyle(color: c.textPrimary),
              ),
            ),
        ],
      ),
    );
  }

  Widget _taskCard(BackgroundTask t) {
    final c = context.colors;
    final failing = t.consecutiveFailures > 0;
    return Card(
      color: c.surfaceAlt,
      margin: const EdgeInsets.only(bottom: 8),
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(12),
        side: BorderSide(color: c.border),
      ),
      child: InkWell(
        borderRadius: BorderRadius.circular(12),
        onTap: () => _showTaskDetail(t),
        child: Padding(
          padding: const EdgeInsets.fromLTRB(14, 12, 6, 12),
          child: Row(
            children: [
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Row(
                      children: [
                        _StatusDot(status: t.status),
                        const SizedBox(width: 8),
                        Expanded(
                          child: Text(
                            t.title,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: TextStyle(
                                color: t.status == 'active'
                                    ? c.textPrimary
                                    : c.textMuted,
                                fontSize: 14,
                                fontWeight: FontWeight.w600),
                          ),
                        ),
                      ],
                    ),
                    const SizedBox(height: 4),
                    Row(
                      children: [
                        _pill(context, t.ownerLabel),
                        if (t.notify) ...[
                          const SizedBox(width: 6),
                          Icon(Icons.notifications_active_outlined,
                              size: 12, color: c.textMuted),
                        ],
                        const SizedBox(width: 6),
                        Expanded(
                          child: Text(
                            '${t.triggerLabel} · ${fmtBgNextRun(t.nextRun, t.status)}',
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style:
                                TextStyle(color: c.textMuted, fontSize: 11.5),
                          ),
                        ),
                      ],
                    ),
                    if (failing)
                      Padding(
                        padding: const EdgeInsets.only(top: 4),
                        child: Text(
                          tr('${t.consecutiveFailures} lần lỗi liên tiếp',
                              '${t.consecutiveFailures} consecutive failures'),
                          style: const TextStyle(
                              color: AppTokens.danger, fontSize: 11.5),
                        ),
                      ),
                  ],
                ),
              ),
              PopupMenuButton<String>(
                color: c.surface,
                icon: Icon(Icons.more_vert, color: c.textSecondary, size: 20),
                onSelected: (v) {
                  if (v == 'run') _runNow(t);
                  if (v == 'pause') {
                    _act(() => _api.pause(t.id), tr('Đã tạm dừng', 'Paused'));
                  }
                  if (v == 'resume') {
                    _act(() => _api.resume(t.id), tr('Đã bật lại', 'Resumed'));
                  }
                  if (v == 'delete') _confirmDelete(t);
                },
                itemBuilder: (_) => [
                  PopupMenuItem(
                      value: 'run',
                      child: Text(tr('Chạy ngay', 'Run now'),
                          style: TextStyle(color: c.textPrimary))),
                  if (t.status == 'active')
                    PopupMenuItem(
                        value: 'pause',
                        child: Text(tr('Tạm dừng', 'Pause'),
                            style: TextStyle(color: c.textPrimary)))
                  else
                    PopupMenuItem(
                        value: 'resume',
                        child: Text(tr('Bật lại', 'Resume'),
                            style: TextStyle(color: c.textPrimary))),
                  if (t.isEditable)
                    PopupMenuItem(
                        value: 'delete',
                        child: Text(tr('Xoá', 'Delete'),
                            style: const TextStyle(color: AppTokens.danger))),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }

  void _showTaskDetail(BackgroundTask t) {
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
        child: _TaskDetailSheet(
          task: t,
          api: _api,
          onChanged: () => _load(silent: true),
        ),
      ),
    );
  }

  void _showQuickDialog(BuildContext context) async {
    final created = await showDialog<bool>(
      context: context,
      builder: (_) => const _QuickDialog(),
    );
    if (created == true) _load(silent: true);
  }
}

Widget _pill(BuildContext context, String text) {
  final c = context.colors;
  return Container(
    padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 1),
    decoration: BoxDecoration(
      color: c.surface,
      borderRadius: BorderRadius.circular(4),
      border: Border.all(color: c.border),
    ),
    child: Text(text, style: TextStyle(color: c.textMuted, fontSize: 10)),
  );
}

class _StatusDot extends StatelessWidget {
  final String status;
  const _StatusDot({required this.status});

  @override
  Widget build(BuildContext context) {
    final color = switch (status) {
      'active' => AppTokens.success,
      'paused' => AppTokens.warning,
      'failed' => AppTokens.danger,
      _ => context.colors.textMuted,
    };
    return Container(
      width: 8,
      height: 8,
      decoration: BoxDecoration(shape: BoxShape.circle, color: color),
    );
  }
}

// ─── Stats header ────────────────────────────────────────────────────────────

class _StatsCard extends StatelessWidget {
  final BackgroundStats stats;
  final String window;
  const _StatsCard({required this.stats, required this.window});

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final t = stats.totals;
    final rate = (t.successRate * 100).round();
    return Container(
      padding: const EdgeInsets.symmetric(vertical: 12, horizontal: 8),
      decoration: BoxDecoration(
        color: c.surfaceAlt,
        borderRadius: BorderRadius.circular(14),
        border: Border.all(color: c.border),
      ),
      child: Row(
        children: [
          _stat(context, window, '${t.runs}', tr('Lượt chạy', 'Runs')),
          _divider(c),
          _stat(context, '', '$rate%', tr('Thành công', 'Success')),
          _divider(c),
          _stat(context, '', fmtBgDuration(t.avgDurationMs),
              tr('TB mỗi lượt', 'Avg run')),
          _divider(c),
          _stat(context, '', '${t.running}', tr('Đang chạy', 'Running'),
              highlight: t.running > 0),
        ],
      ),
    );
  }

  Widget _divider(AppColors c) => Container(width: 1, height: 32, color: c.border);

  Widget _stat(BuildContext context, String tag, String value, String label,
      {bool highlight = false}) {
    final c = context.colors;
    return Expanded(
      child: Column(
        children: [
          Text(value,
              style: TextStyle(
                  color: highlight ? AppTokens.success : c.textPrimary,
                  fontSize: 16,
                  fontWeight: FontWeight.bold)),
          Text(tag.isEmpty ? label : '$label ($tag)',
              style: TextStyle(color: c.textMuted, fontSize: 10.5)),
        ],
      ),
    );
  }
}

/// Tasks failing repeatedly — surfaced above the list so quarantine-bound
/// tasks aren't invisible on a phone screen.
class _AttentionBand extends StatelessWidget {
  final List<BackgroundAttention> attention;
  const _AttentionBand({required this.attention});

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Container(
      margin: const EdgeInsets.only(top: 8),
      padding: const EdgeInsets.all(10),
      decoration: BoxDecoration(
        color: AppTokens.danger.withValues(alpha: 0.08),
        borderRadius: BorderRadius.circular(12),
        border: Border.all(color: AppTokens.danger.withValues(alpha: 0.35)),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              const Icon(Icons.warning_amber_rounded,
                  size: 14, color: AppTokens.danger),
              const SizedBox(width: 6),
              Text(tr('Cần chú ý', 'Needs attention'),
                  style: const TextStyle(
                      color: AppTokens.danger,
                      fontSize: 12,
                      fontWeight: FontWeight.w700)),
            ],
          ),
          const SizedBox(height: 4),
          for (final a in attention.take(3))
            Padding(
              padding: const EdgeInsets.only(top: 2),
              child: Text(
                '${a.title} — ${a.consecutiveFailures}× ${a.lastError ?? ''}',
                maxLines: 2,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(color: c.textSecondary, fontSize: 11.5),
              ),
            ),
        ],
      ),
    );
  }
}

// ─── Task detail sheet ───────────────────────────────────────────────────────

class _TaskDetailSheet extends StatefulWidget {
  final BackgroundTask task;
  final BackgroundApi api;
  final VoidCallback onChanged;
  const _TaskDetailSheet(
      {required this.task, required this.api, required this.onChanged});

  @override
  State<_TaskDetailSheet> createState() => _TaskDetailSheetState();
}

class _TaskDetailSheetState extends State<_TaskDetailSheet> {
  List<BackgroundRun>? _runs;
  String? _error;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    try {
      final r = await widget.api.runs(widget.task.id);
      if (mounted) setState(() => _runs = r);
    } catch (e) {
      if (mounted) setState(() => _error = '$e');
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final t = widget.task;
    return Padding(
      padding: const EdgeInsets.fromLTRB(16, 14, 16, 0),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              _StatusDot(status: t.status),
              const SizedBox(width: 8),
              Expanded(
                child: Text(t.title,
                    style: TextStyle(
                        color: c.textPrimary,
                        fontSize: 16,
                        fontWeight: FontWeight.bold)),
              ),
              _pill(context, t.ownerLabel),
            ],
          ),
          const SizedBox(height: 6),
          Text('${t.triggerLabel} · ${fmtBgNextRun(t.nextRun, t.status)}',
              style: TextStyle(color: c.textMuted, fontSize: 12)),
          if ((t.description ?? '').isNotEmpty) ...[
            const SizedBox(height: 8),
            Text(t.description!,
                style: TextStyle(color: c.textSecondary, fontSize: 13)),
          ],
          if ((t.prompt ?? '').isNotEmpty) ...[
            const SizedBox(height: 8),
            Container(
              width: double.infinity,
              padding: const EdgeInsets.all(10),
              decoration: BoxDecoration(
                color: c.surfaceAlt,
                borderRadius: BorderRadius.circular(10),
                border: Border.all(color: c.border),
              ),
              child: Text(
                t.prompt!,
                maxLines: 4,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(
                    color: c.textSecondary, fontSize: 12, height: 1.4),
              ),
            ),
          ],
          const SizedBox(height: 12),
          Text(tr('LỊCH SỬ CHẠY', 'RUN HISTORY'),
              style: TextStyle(
                  color: c.textMuted,
                  fontSize: 11,
                  fontWeight: FontWeight.w700,
                  letterSpacing: 0.6)),
          const SizedBox(height: 6),
          Expanded(child: _buildRuns()),
        ],
      ),
    );
  }

  Widget _buildRuns() {
    final c = context.colors;
    if (_error != null) return ErrorState(message: _error!, onRetry: _load);
    final runs = _runs;
    if (runs == null) return const LoadingState();
    if (runs.isEmpty) {
      return EmptyState(
        icon: Icons.history,
        message: tr('Chưa chạy lần nào', 'No runs yet'),
      );
    }
    return ListView.builder(
      padding: const EdgeInsets.only(bottom: 24),
      itemCount: runs.length,
      itemBuilder: (ctx, i) {
        final r = runs[i];
        return ListTile(
          dense: true,
          contentPadding: EdgeInsets.zero,
          leading: _runStatusIcon(r),
          title: Text(
            '${fmtBgTime(r.startedAt)} · ${fmtBgDuration(r.durationMs)}',
            style: TextStyle(color: c.textPrimary, fontSize: 13),
          ),
          subtitle: r.error != null
              ? Text(r.error!,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style:
                      const TextStyle(color: AppTokens.danger, fontSize: 11.5))
              : Text(r.triggerKind,
                  style: TextStyle(color: c.textMuted, fontSize: 11.5)),
          trailing: Icon(Icons.chevron_right, color: c.textMuted, size: 18),
          onTap: () => Navigator.of(context).push(MaterialPageRoute(
              builder: (_) =>
                  _SessionScreen(runId: r.id, title: widget.task.title))),
        );
      },
    );
  }

  Widget _runStatusIcon(BackgroundRun r) {
    return switch (r.status) {
      'running' => const SizedBox(
          width: 16,
          height: 16,
          child: CircularProgressIndicator(strokeWidth: 2)),
      'success' =>
        const Icon(Icons.check_circle_outline, color: AppTokens.success, size: 18),
      'error' || 'timeout' =>
        const Icon(Icons.error_outline, color: AppTokens.danger, size: 18),
      'skipped' =>
        Icon(Icons.redo, color: context.colors.textMuted, size: 18),
      _ => Icon(Icons.remove_circle_outline,
          color: context.colors.textMuted, size: 18),
    };
  }
}

// ─── Session (run transcript) ────────────────────────────────────────────────

class _SessionScreen extends StatefulWidget {
  final String runId;
  final String title;
  const _SessionScreen({required this.runId, required this.title});

  @override
  State<_SessionScreen> createState() => _SessionScreenState();
}

class _SessionScreenState extends State<_SessionScreen> {
  final _api = BackgroundApi();
  BackgroundSession? _session;
  String? _error;
  Timer? _pollTimer;

  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  void dispose() {
    _pollTimer?.cancel();
    super.dispose();
  }

  Future<void> _load() async {
    try {
      final s = await _api.session(widget.runId);
      if (!mounted) return;
      setState(() {
        _session = s;
        _error = null;
      });
      // Keep refreshing while the run is in flight so the transcript grows.
      if (s.run.isRunning) {
        _pollTimer ??= Timer.periodic(
            const Duration(seconds: 5), (_) => _load());
      } else {
        _pollTimer?.cancel();
        _pollTimer = null;
      }
    } catch (e) {
      if (mounted) setState(() => _error = '$e');
    }
  }

  Future<void> _cancel() async {
    try {
      await _api.cancelRun(widget.runId);
      _load();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text(tr('Lỗi: $e', 'Error: $e'))));
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final run = _session?.run;
    return Scaffold(
      backgroundColor: c.bg,
      appBar: AppBar(
        backgroundColor: c.surface,
        elevation: 0,
        title: Text(widget.title,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: TextStyle(color: c.textPrimary, fontSize: 16)),
        actions: [
          if (run != null && run.isRunning)
            IconButton(
              tooltip: tr('Huỷ lượt chạy', 'Cancel run'),
              icon: const Icon(Icons.stop_circle_outlined,
                  color: AppTokens.danger),
              onPressed: _cancel,
            ),
          IconButton(
            icon: Icon(Icons.refresh, color: c.textSecondary),
            onPressed: _load,
          ),
        ],
      ),
      body: _buildBody(),
    );
  }

  Widget _buildBody() {
    final c = context.colors;
    if (_error != null) return ErrorState(message: _error!, onRetry: _load);
    final s = _session;
    if (s == null) return const LoadingState();
    final run = s.run;

    return ListView(
      padding: const EdgeInsets.all(12),
      children: [
        // Run header: status + meters.
        Container(
          padding: const EdgeInsets.all(12),
          decoration: BoxDecoration(
            color: c.surfaceAlt,
            borderRadius: BorderRadius.circular(12),
            border: Border.all(color: c.border),
          ),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  _statusChip(run),
                  const SizedBox(width: 8),
                  Text(fmtBgTime(run.startedAt),
                      style: TextStyle(color: c.textMuted, fontSize: 12)),
                  const Spacer(),
                  Text(fmtBgDuration(run.durationMs),
                      style: TextStyle(color: c.textSecondary, fontSize: 12)),
                ],
              ),
              if (run.turnCount != null ||
                  run.tokensIn != null ||
                  run.tokensOut != null)
                Padding(
                  padding: const EdgeInsets.only(top: 6),
                  child: Text(
                    [
                      if (run.turnCount != null)
                        tr('${run.turnCount} lượt', '${run.turnCount} turns'),
                      if (run.tokensIn != null) '↓${run.tokensIn}',
                      if (run.tokensOut != null) '↑${run.tokensOut}',
                    ].join(' · '),
                    style: TextStyle(color: c.textMuted, fontSize: 11.5),
                  ),
                ),
            ],
          ),
        ),
        if ((run.error ?? '').isNotEmpty) ...[
          const SizedBox(height: 8),
          _section(context, tr('Lỗi', 'Error'), run.error!,
              color: AppTokens.danger),
        ],
        if ((run.result ?? '').isNotEmpty) ...[
          const SizedBox(height: 8),
          _section(context, tr('Kết quả', 'Result'), run.result!),
        ],
        const SizedBox(height: 12),
        Text(tr('DIỄN BIẾN', 'ACTIVITY'),
            style: TextStyle(
                color: c.textMuted,
                fontSize: 11,
                fontWeight: FontWeight.w700,
                letterSpacing: 0.6)),
        const SizedBox(height: 4),
        if (s.activity.isEmpty)
          Padding(
            padding: const EdgeInsets.symmetric(vertical: 16),
            child: Text(tr('(chưa có hoạt động)', '(no activity yet)'),
                style: TextStyle(color: c.textMuted, fontSize: 12)),
          )
        else
          for (final a in s.activity) _activityRow(context, a),
      ],
    );
  }

  Widget _statusChip(BackgroundRun run) {
    final (color, label) = switch (run.status) {
      'running' => (AppTokens.brand, tr('Đang chạy', 'Running')),
      'success' => (AppTokens.success, tr('Thành công', 'Success')),
      'error' => (AppTokens.danger, tr('Lỗi', 'Error')),
      'timeout' => (AppTokens.danger, 'Timeout'),
      'cancelled' => (AppTokens.warning, tr('Đã huỷ', 'Cancelled')),
      'skipped' => (AppTokens.warning, tr('Bỏ qua', 'Skipped')),
      _ => (AppTokens.warning, run.status),
    };
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
      decoration: BoxDecoration(
        color: color.withValues(alpha: 0.14),
        borderRadius: BorderRadius.circular(999),
      ),
      child: Text(label,
          style: TextStyle(
              color: color, fontSize: 11.5, fontWeight: FontWeight.w600)),
    );
  }

  Widget _section(BuildContext context, String title, String body,
      {Color? color}) {
    final c = context.colors;
    return Container(
      width: double.infinity,
      padding: const EdgeInsets.all(12),
      decoration: BoxDecoration(
        color: (color ?? c.accent).withValues(alpha: 0.06),
        borderRadius: BorderRadius.circular(12),
        border:
            Border.all(color: (color ?? c.border).withValues(alpha: 0.4)),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(title,
              style: TextStyle(
                  color: color ?? c.textMuted,
                  fontSize: 11,
                  fontWeight: FontWeight.w700)),
          const SizedBox(height: 4),
          SelectableText(body,
              style: TextStyle(
                  color: c.textSecondary, fontSize: 13, height: 1.4)),
        ],
      ),
    );
  }

  Widget _activityRow(BuildContext context, BackgroundActivity a) {
    final c = context.colors;
    final (icon, color) = switch (a.kind) {
      'tool' => (Icons.build_outlined, AppTokens.cyan),
      'tool_error' => (Icons.build_outlined, AppTokens.danger),
      'think' => (Icons.psychology_outlined, c.textMuted),
      'message' => (Icons.chat_bubble_outline, c.accent),
      _ => (Icons.notes, c.textSecondary),
    };
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Icon(icon, size: 14, color: color),
          const SizedBox(width: 8),
          Expanded(
            child: Text(a.detail,
                style: TextStyle(
                    color: c.textSecondary, fontSize: 12.5, height: 1.4)),
          ),
        ],
      ),
    );
  }
}

// ─── Quick task dialog (parse → review → create) ─────────────────────────────

/// "Quick task" — describe a background task in one line and let the daemon's
/// LLM fill the fields. Two steps on purpose: a background task runs
/// unattended, so a schedule the model got wrong must be visible before it is
/// committed, not after it silently fails to fire.
class _QuickDialog extends StatefulWidget {
  const _QuickDialog();

  @override
  State<_QuickDialog> createState() => _QuickDialogState();
}

class _QuickDialogState extends State<_QuickDialog> {
  final _api = BackgroundApi();
  final _text = TextEditingController();
  bool _parsing = false;
  bool _creating = false;
  String? _error;
  Map<String, dynamic>? _draft; // the parsed spec, awaiting confirmation

  @override
  void dispose() {
    _text.dispose();
    super.dispose();
  }

  Future<void> _parse() async {
    final text = _text.text.trim();
    if (text.isEmpty) return;
    setState(() {
      _parsing = true;
      _error = null;
    });
    try {
      final spec = await _api.parseQuick(text);
      if (mounted) setState(() => _draft = spec);
    } catch (e) {
      if (mounted) setState(() => _error = _msg(e));
    } finally {
      if (mounted) setState(() => _parsing = false);
    }
  }

  Future<void> _create() async {
    final d = _draft;
    if (d == null) return;
    setState(() {
      _creating = true;
      _error = null;
    });
    try {
      // Reuse the normal create path (server derives next_run).
      await _api.create({
        'title': d['title'],
        'prompt': d['prompt'],
        'trigger_type': d['trigger_type'],
        if (d['trigger_value'] != null) 'trigger_value': d['trigger_value'],
        'prompt_kind': d['prompt_kind'] ?? 'static',
        'continuity': d['continuity'] ?? 'fresh',
        if (d['notify'] == true) 'notify': true,
      });
      if (mounted) Navigator.pop(context, true);
    } catch (e) {
      if (mounted) setState(() => _error = _msg(e));
    } finally {
      if (mounted) setState(() => _creating = false);
    }
  }

  String _msg(Object e) {
    final s = e.toString();
    return s.startsWith('ApiException') && s.contains(':')
        ? s.substring(s.indexOf(':') + 1).trim()
        : s;
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return AlertDialog(
      backgroundColor: c.surface,
      insetPadding: const EdgeInsets.symmetric(horizontal: 16, vertical: 24),
      title: Row(
        children: [
          Icon(Icons.bolt, size: 18, color: c.accent),
          const SizedBox(width: 8),
          Text(tr('Task nhanh', 'Quick task'),
              style: TextStyle(
                  color: c.textPrimary,
                  fontSize: 16,
                  fontWeight: FontWeight.w700)),
        ],
      ),
      content: SizedBox(
        width: double.maxFinite,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                tr('Mô tả task bằng một câu — AI sẽ tự điền lịch chạy và nội dung.',
                    'Describe the task in one line — AI fills the schedule and prompt.'),
                style: TextStyle(color: c.textMuted, fontSize: 12),
              ),
              const SizedBox(height: 8),
              TextField(
                controller: _text,
                autofocus: true,
                maxLines: 3,
                enabled: !_parsing && !_creating,
                style: TextStyle(color: c.textPrimary, fontSize: 13),
                decoration: InputDecoration(
                  hintText: tr('vd: mỗi sáng 9h rà soát tri thức và dọn mâu thuẫn',
                      'e.g. every morning at 9 review knowledge and clean up'),
                  hintStyle: TextStyle(color: c.textMuted, fontSize: 12),
                  border: const OutlineInputBorder(),
                  isDense: true,
                ),
                onSubmitted: (_) => _parse(),
              ),
              const SizedBox(height: 8),
              Align(
                alignment: Alignment.centerRight,
                child: FilledButton.icon(
                  icon: _parsing
                      ? const SizedBox(
                          width: 14,
                          height: 14,
                          child: CircularProgressIndicator(strokeWidth: 2))
                      : const Icon(Icons.auto_awesome, size: 15),
                  label: Text(_parsing
                      ? tr('Đang phân tích…', 'Analyzing…')
                      : (_draft == null
                          ? tr('AI phân tích', 'AI analyze')
                          : tr('Phân tích lại', 'Re-analyze'))),
                  onPressed: (_parsing || _creating) ? null : _parse,
                ),
              ),
              if (_draft != null) ...[
                const SizedBox(height: 8),
                _DraftPreview(draft: _draft!),
              ],
              if (_error != null)
                Padding(
                  padding: const EdgeInsets.only(top: 8),
                  child: Text(_error!,
                      style: const TextStyle(
                          color: AppTokens.danger, fontSize: 12)),
                ),
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: Text(tr('Huỷ', 'Cancel'))),
        FilledButton(
          onPressed: (_draft == null || _creating) ? null : _create,
          child: Text(_creating
              ? tr('Đang tạo…', 'Creating…')
              : tr('Tạo task', 'Create task')),
        ),
      ],
    );
  }
}

/// Read-only preview of the parsed spec so the user can catch a wrong schedule
/// before committing.
class _DraftPreview extends StatelessWidget {
  const _DraftPreview({required this.draft});
  final Map<String, dynamic> draft;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    // Reuse the model's own trigger→prose formatting for consistency with the
    // task list.
    final t = BackgroundTask.fromJson({
      'id': '',
      'owner_kind': 'user',
      'title': draft['title'] ?? '',
      'trigger_type': draft['trigger_type'] ?? 'manual',
      'trigger_value': draft['trigger_value'],
      'continuity': draft['continuity'] ?? 'fresh',
    });

    Widget row(String k, String v) => Padding(
          padding: const EdgeInsets.symmetric(vertical: 2),
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              SizedBox(
                width: 76,
                child:
                    Text(k, style: TextStyle(color: c.textMuted, fontSize: 11)),
              ),
              Expanded(
                child: Text(v,
                    style: TextStyle(color: c.textSecondary, fontSize: 12)),
              ),
            ],
          ),
        );

    return Container(
      width: double.infinity,
      padding: const EdgeInsets.all(12),
      decoration: BoxDecoration(
        color: c.surfaceAlt,
        borderRadius: BorderRadius.circular(10),
        border: Border.all(color: c.border),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(tr('AI đề xuất', 'AI suggestion'),
              style: TextStyle(
                  color: c.textMuted,
                  fontSize: 10,
                  fontWeight: FontWeight.w600)),
          const SizedBox(height: 4),
          row(tr('Tiêu đề', 'Title'), '${draft['title'] ?? ''}'),
          row(tr('Lịch chạy', 'Schedule'), t.triggerLabel),
          if (draft['notify'] == true)
            row(tr('Kiểu', 'Kind'),
                tr('🔔 Thông báo (không chạy agent)', '🔔 Notify (no agent)')),
          if (t.continuity == 'thread')
            row(tr('Bộ nhớ', 'Memory'),
                tr('nhớ các lần trước', 'remembers prior runs')),
          const SizedBox(height: 4),
          Text(tr('Nội dung', 'Prompt'),
              style: TextStyle(color: c.textMuted, fontSize: 11)),
          const SizedBox(height: 2),
          Text('${draft['prompt'] ?? ''}',
              style: TextStyle(
                  color: c.textSecondary, fontSize: 12, height: 1.4)),
        ],
      ),
    );
  }
}
