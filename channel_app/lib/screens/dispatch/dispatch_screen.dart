import 'dart:async';

import 'package:flutter/material.dart';

import '../../models/dispatch_models.dart';
import '../../services/dispatch_api.dart';
import '../../services/language_service.dart';
import '../../services/relay_manager.dart';
import '../../theme/tokens.dart';
import '../../util/format.dart';
import '../../widgets/states.dart';

/// DAG sub-agent dispatch: what the orchestrator fanned out and how far each
/// subtask got.
///
/// Polls while open. The live `dispatch:update` event reaches WebSocket admin
/// clients only, so there is nothing to subscribe to over the relay — and even
/// a forwarded event would be lost across the relay's reconnect cycle, leaving
/// a half-built tree with no way to reconcile.
class DispatchScreen extends StatefulWidget {
  const DispatchScreen({super.key});

  @override
  State<DispatchScreen> createState() => _DispatchScreenState();
}

class _DispatchScreenState extends State<DispatchScreen> {
  final _api = DispatchApi();

  List<DispatchParent> _parents = const [];
  String? _error;
  bool _retrying = false;
  bool _loading = true;
  Timer? _poll;

  /// Faster while something is running, idle otherwise — a finished tree does
  /// not change, and each poll is a relay round-trip on a phone.
  static const _activeInterval = Duration(seconds: 5);
  static const _idleInterval = Duration(seconds: 30);

  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  void dispose() {
    _poll?.cancel();
    super.dispose();
  }

  void _schedule() {
    _poll?.cancel();
    final active = _parents.any((p) => p.status != 'done');
    _poll = Timer(active ? _activeInterval : _idleInterval, _load);
  }

  Future<void> _load() async {
    if (!mounted) return;
    try {
      final parents = await _api.parents();
      if (!mounted) return;
      setState(() {
        _parents = parents;
        _error = null;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = e.toString();
        _loading = false;
      });
    }
    _schedule();
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
            Text(tr('Điều phối', 'Dispatch'),
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
            tooltip: tr('Tải lại', 'Reload'),
            icon: Icon(Icons.refresh, color: c.textSecondary),
            onPressed: _load,
          ),
        ],
      ),
      body: _loading
          ? const LoadingState()
          : _error != null && _parents.isEmpty
              ? ErrorState(message: _error!, onRetry: _load)
              : _parents.isEmpty
                  ? EmptyState(
                      icon: Icons.account_tree_outlined,
                      message: tr('Chưa có phiên điều phối nào',
                          'No dispatches yet'),
                      hint: tr(
                        'Khi agent chia việc cho các agent con, tiến trình sẽ hiện ở đây.',
                        'When the agent fans work out to sub-agents, progress shows up here.',
                      ),
                    )
                  : RefreshIndicator(
                      color: c.accent,
                      onRefresh: _load,
                      child: ListView.builder(
                        padding: const EdgeInsets.fromLTRB(12, 12, 12, 32),
                        itemCount: _parents.length,
                        itemBuilder: (_, i) => _parentCard(c, _parents[i]),
                      ),
                    ),
    );
  }

  Widget _parentCard(AppColors c, DispatchParent p) {
    final total = p.tasks.length;
    final done = p.doneCount;
    return Card(
      margin: const EdgeInsets.only(bottom: 8),
      color: c.surface,
      elevation: 0,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(AppTokens.rMd),
        side: BorderSide(color: c.border),
      ),
      child: Theme(
        // The default ExpansionTile divider fights the card border.
        data: Theme.of(context).copyWith(dividerColor: Colors.transparent),
        child: ExpansionTile(
          initiallyExpanded: p.status != 'done',
          tilePadding: const EdgeInsets.symmetric(horizontal: 12),
          childrenPadding: const EdgeInsets.fromLTRB(12, 0, 12, 8),
          title: Text(
            p.goal.isEmpty ? p.id : p.goal,
            maxLines: 2,
            overflow: TextOverflow.ellipsis,
            style: TextStyle(
              color: c.textPrimary,
              fontWeight: FontWeight.w600,
              fontSize: 14,
            ),
          ),
          subtitle: Padding(
            padding: const EdgeInsets.only(top: 5),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    _statusPill(c, p.status),
                    const SizedBox(width: 8),
                    Text(
                      '$done/$total'
                      '${p.failedCount > 0 ? tr(" · ${p.failedCount} lỗi", " · ${p.failedCount} failed") : ""}',
                      style: TextStyle(color: c.textMuted, fontSize: 11.5),
                    ),
                    const Spacer(),
                    if (p.failedCount > 0)
                      TextButton.icon(
                        onPressed: _retrying
                            ? null
                            : () => _runRetry(
                                  () => DispatchApi().retryParent(p.id),
                                  tr('Đã xếp lại ${p.failedCount} việc lỗi',
                                      'Re-queued ${p.failedCount} failed task(s)'),
                                ),
                        icon: const Icon(Icons.refresh, size: 15),
                        label: Text(tr('Thử lại', 'Retry'),
                            style: const TextStyle(fontSize: 11.5)),
                        style: TextButton.styleFrom(
                          foregroundColor: AppTokens.danger,
                          padding: const EdgeInsets.symmetric(horizontal: 6),
                          minimumSize: const Size(0, 28),
                          tapTargetSize: MaterialTapTargetSize.shrinkWrap,
                        ),
                      ),
                    if (p.createdAt.isNotEmpty)
                      Text(timeAgoIso(p.createdAt),
                          style: TextStyle(color: c.textMuted, fontSize: 11)),
                  ],
                ),
                const SizedBox(height: 6),
                ClipRRect(
                  borderRadius: BorderRadius.circular(3),
                  child: LinearProgressIndicator(
                    value: total == 0 ? 0 : done / total,
                    minHeight: 4,
                    backgroundColor: c.surfaceAlt,
                    valueColor: AlwaysStoppedAnimation<Color>(
                      p.failedCount > 0 ? AppTokens.warning : c.accent,
                    ),
                  ),
                ),
              ],
            ),
          ),
          children: [for (final t in p.tasks) _taskRow(c, t)],
        ),
      ),
    );
  }

  Widget _taskRow(AppColors c, DispatchTask t) => InkWell(
        borderRadius: BorderRadius.circular(AppTokens.rSm),
        onTap: () => _showTask(t),
        child: Padding(
          padding: const EdgeInsets.symmetric(vertical: 7),
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              _statusDot(t.status),
              const SizedBox(width: 9),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      t.label.isEmpty ? t.id : t.label,
                      style: TextStyle(color: c.textPrimary, fontSize: 13),
                    ),
                    const SizedBox(height: 2),
                    Text(
                      [
                        if (t.worker.isNotEmpty) t.worker,
                        if (t.isVirtual) tr('ảo', 'virtual'),
                        if (t.dependsOn.isNotEmpty)
                          tr('sau ${t.dependsOn.length}',
                              'after ${t.dependsOn.length}'),
                        if (t.retryCount > 0)
                          tr('thử lại ${t.retryCount}', '${t.retryCount} retries'),
                      ].join(' · '),
                      style: TextStyle(color: c.textMuted, fontSize: 11),
                    ),
                  ],
                ),
              ),
              Icon(Icons.chevron_right, size: 16, color: c.textMuted),
            ],
          ),
        ),
      );

  Widget _statusDot(String status) => Container(
        width: 8,
        height: 8,
        margin: const EdgeInsets.only(top: 5),
        decoration: BoxDecoration(
          color: _statusColor(status),
          shape: BoxShape.circle,
        ),
      );

  Widget _statusPill(AppColors c, String status) => Container(
        padding: const EdgeInsets.symmetric(horizontal: 7, vertical: 2),
        decoration: BoxDecoration(
          color: _statusColor(status).withValues(alpha: 0.14),
          borderRadius: BorderRadius.circular(4),
        ),
        child: Text(
          status,
          style: TextStyle(color: _statusColor(status), fontSize: 10.5),
        ),
      );

  Color _statusColor(String status) => switch (status) {
        'done' => AppTokens.success,
        'processing' || 'active' => AppTokens.cyan,
        'error' || 'timeout' => AppTokens.danger,
        _ => AppTokens.warning,
      };

  /// Run a retry and report the outcome. Refusals from the daemon are shown
  /// verbatim: they explain *why* ("already running", "finished successfully"),
  /// which a generic "failed" would hide and the user would just tap again.
  Future<void> _runRetry(Future<void> Function() action, String okMsg) async {
    setState(() => _retrying = true);
    String? failure;
    try {
      await action();
    } catch (e) {
      failure = e.toString();
    }
    if (!mounted) return;
    setState(() => _retrying = false);
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(content: Text(failure ?? okMsg)),
    );
    // The snapshot is polled, not pushed — refresh so the new state shows now.
    await _load();
  }

  void _showTask(DispatchTask t) {
    final c = context.colors;
    showModalBottomSheet(
      context: context,
      backgroundColor: c.surface,
      isScrollControlled: true,
      shape: const RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(top: Radius.circular(14)),
      ),
      builder: (_) => DraggableScrollableSheet(
        expand: false,
        initialChildSize: 0.75,
        builder: (_, controller) => ListView(
          controller: controller,
          padding: const EdgeInsets.all(16),
          children: [
            Text(
              t.label.isEmpty ? t.id : t.label,
              style: TextStyle(
                color: c.textPrimary,
                fontWeight: FontWeight.w700,
                fontSize: 15,
              ),
            ),
            const SizedBox(height: 8),
            Wrap(
              spacing: 8,
              runSpacing: 6,
              children: [
                _statusPill(c, t.status),
                if (t.worker.isNotEmpty) _meta(c, t.worker),
                if (t.timeoutSeconds > 0)
                  _meta(c, tr('hạn ${t.timeoutSeconds}s',
                      '${t.timeoutSeconds}s budget')),
                if (t.startedAt != null)
                  _meta(c, tr('bắt đầu ${timeAgoIso(t.startedAt)}',
                      'started ${timeAgoIso(t.startedAt)}')),
                if (t.completedAt != null)
                  _meta(c, tr('xong ${timeAgoIso(t.completedAt)}',
                      'finished ${timeAgoIso(t.completedAt)}')),
              ],
            ),
            if (t.status == 'error' || t.status == 'timeout') ...[
              const SizedBox(height: 12),
              SizedBox(
                width: double.infinity,
                child: FilledButton.icon(
                  onPressed: _retrying
                      ? null
                      : () {
                          // Close the sheet first: the list underneath reloads
                          // and a stale sheet would still show the old failure.
                          Navigator.of(context).pop();
                          _runRetry(
                            () => DispatchApi().retryTask(t.id),
                            tr('Đã xếp lại công việc', 'Task re-queued'),
                          );
                        },
                  icon: const Icon(Icons.refresh, size: 18),
                  label: Text(tr('Chạy lại công việc này', 'Re-run this task')),
                  style: FilledButton.styleFrom(
                    backgroundColor: AppTokens.danger,
                  ),
                ),
              ),
            ],
            if (t.checklist.isNotEmpty) ...[
              const SizedBox(height: 16),
              Row(
                children: [
                  Text(tr('Danh mục kiểm', 'Checklist'),
                      style: TextStyle(
                          color: c.textPrimary, fontWeight: FontWeight.w600)),
                  const SizedBox(width: 8),
                  // An auto checklist was inferred from the prompt and only
                  // downgrades to warnings; presenting it as a verdict would
                  // overstate what a red item means.
                  if (t.checklistAuto)
                    Text(tr('(tự suy ra — chỉ tham khảo)',
                        '(inferred — advisory)'),
                        style: TextStyle(color: c.textMuted, fontSize: 11)),
                ],
              ),
              const SizedBox(height: 6),
              for (final item in t.checklist)
                Padding(
                  padding: const EdgeInsets.symmetric(vertical: 3),
                  child: Row(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Icon(
                        item.done
                            ? Icons.check_circle_outline
                            : Icons.radio_button_unchecked,
                        size: 15,
                        color: item.done ? AppTokens.success : c.textMuted,
                      ),
                      const SizedBox(width: 8),
                      Expanded(
                        child: Text(item.text,
                            style: TextStyle(
                                color: c.textSecondary, fontSize: 12.5)),
                      ),
                    ],
                  ),
                ),
            ],
            if (t.prompt.isNotEmpty) ...[
              const SizedBox(height: 16),
              Text(tr('Yêu cầu', 'Prompt'),
                  style: TextStyle(
                      color: c.textPrimary, fontWeight: FontWeight.w600)),
              const SizedBox(height: 6),
              SelectableText(t.prompt,
                  style: TextStyle(color: c.textSecondary, fontSize: 12.5)),
            ],
            if ((t.result ?? '').isNotEmpty) ...[
              const SizedBox(height: 16),
              Text(tr('Kết quả', 'Result'),
                  style: TextStyle(
                      color: c.textPrimary, fontWeight: FontWeight.w600)),
              const SizedBox(height: 6),
              SelectableText(t.result!,
                  style: TextStyle(color: c.textSecondary, fontSize: 12.5)),
            ],
          ],
        ),
      ),
    );
  }

  Widget _meta(AppColors c, String text) => Container(
        padding: const EdgeInsets.symmetric(horizontal: 7, vertical: 2),
        decoration: BoxDecoration(
          color: c.surfaceAlt,
          borderRadius: BorderRadius.circular(4),
          border: Border.all(color: c.border),
        ),
        child: Text(text, style: TextStyle(color: c.textMuted, fontSize: 11)),
      );
}
