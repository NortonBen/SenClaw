import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../models/background_models.dart';
import '../../theme/tokens.dart';
import '../../widgets/section_scaffold.dart';
import 'background_providers.dart';
import 'background_quick_dialog.dart';
import 'background_session_dialog.dart';
import 'background_task_editor.dart';

/// Background — autonomous work the daemon runs by itself.
///
/// Separate from Calendar's schedules on purpose: a schedule runs in a chat and
/// replies to you, a background task runs unattended and writes to a run record.
/// This screen is the only place those runs are visible, so it leads with
/// "is anything broken" rather than with a task list.
class BackgroundScreen extends ConsumerWidget {
  const BackgroundScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    // Keep the live WS listener mounted for as long as this screen is.
    ref.watch(bgLiveProvider);
    final tasks = ref.watch(bgTasksProvider);
    final selected = ref.watch(bgSelectedTaskProvider);

    return SectionScaffold(
      title: 'Background',
      subtitle: 'Tasks SenClaw runs by itself — no chat, no reply',
      actions: [
        const _WindowPicker(),
        const SizedBox(width: AppTokens.s8),
        _InternalToggle(),
        const SizedBox(width: AppTokens.s8),
        IconButton(
          tooltip: 'Refresh',
          icon: const Icon(Icons.refresh, size: 18),
          onPressed: () => ref.read(bgRevProvider.notifier).state++,
        ),
        const SizedBox(width: AppTokens.s8),
        // Describe a task in one line; AI fills the fields. The New task form
        // beside it is the manual, field-by-field path.
        OutlinedButton.icon(
          icon: const Icon(Icons.bolt, size: 16),
          label: const Text('Quick task'),
          onPressed: () => showBackgroundQuickDialog(context),
        ),
        const SizedBox(width: AppTokens.s8),
        FilledButton.icon(
          icon: const Icon(Icons.add, size: 16),
          label: const Text('New task'),
          onPressed: () => showBackgroundTaskEditor(context, ref),
        ),
      ],
      body: tasks.when(
        loading: () => const Center(child: CircularProgressIndicator()),
        error: (e, _) => _ErrorPane(message: '$e'),
        data: (page) => Row(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Expanded(
              flex: 3,
              child: _LeftPane(page: page),
            ),
            if (selected != null) ...[
              Container(width: 1, color: c.border),
              Expanded(
                flex: 4,
                child: _DetailPane(
                  taskId: selected,
                  // A task can vanish under us (deleted, or an app uninstalled).
                  task: page.tasks.where((t) => t.id == selected).firstOrNull,
                ),
              ),
            ],
          ],
        ),
      ),
    );
  }
}

// ─── left: stats + attention + task list ─────────────────────────────────────

class _LeftPane extends ConsumerWidget {
  const _LeftPane({required this.page});
  final BgTaskPage page;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final status = ref.watch(bgStatusFilterProvider);
    final tasks = page.tasks;
    return ListView(
      padding: const EdgeInsets.all(AppTokens.s16),
      children: [
        const _StatsRow(),
        const SizedBox(height: AppTokens.s12),
        const _AttentionBand(),
        const _StatusFilterBar(),
        const SizedBox(height: AppTokens.s8),
        if (tasks.isEmpty)
          // A filtered-empty result is different from having no tasks at all —
          // don't show the "create your first task" onboarding over a filter.
          status != null
              ? _FilterEmpty(status: status)
              : const _EmptyState()
        else
          ...tasks.map((t) => _TaskRow(task: t)),
        const _Pager(),
      ],
    );
  }
}

/// Status filter chips + count. Sits above the list.
class _StatusFilterBar extends ConsumerWidget {
  const _StatusFilterBar();

  static const _options = <String?, String>{
    null: 'Tất cả',
    'active': 'Active',
    'paused': 'Paused',
    'failed': 'Failed',
    'completed': 'Completed',
    'cancelled': 'Cancelled',
  };

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final sel = ref.watch(bgStatusFilterProvider);
    final total = ref.watch(bgTasksProvider).valueOrNull?.total;
    return Row(
      children: [
        Expanded(
          child: Wrap(
            spacing: AppTokens.s6,
            runSpacing: AppTokens.s6,
            children: _options.entries.map((e) {
              final active = sel == e.key;
              return InkWell(
                borderRadius: BorderRadius.circular(AppTokens.rSm),
                onTap: () {
                  ref.read(bgStatusFilterProvider.notifier).state = e.key;
                  ref.read(bgPageProvider.notifier).state = 0; // filter resets the pager
                },
                child: Container(
                  padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 4),
                  decoration: BoxDecoration(
                    color: active ? c.accent.withValues(alpha: 0.16) : c.surface,
                    borderRadius: BorderRadius.circular(AppTokens.rSm),
                    border: Border.all(
                        color: active ? c.accent : c.border),
                  ),
                  child: Text(
                    e.value,
                    style: TextStyle(
                      color: active ? c.accent : c.textSecondary,
                      fontSize: 11,
                      fontWeight: active ? FontWeight.w600 : FontWeight.w400,
                    ),
                  ),
                ),
              );
            }).toList(),
          ),
        ),
        if (total != null)
          Padding(
            padding: const EdgeInsets.only(left: AppTokens.s8),
            child: Text('$total task',
                style: TextStyle(color: c.textMuted, fontSize: 11)),
          ),
      ],
    );
  }
}

/// Prev/next pager, shown only when the total exceeds one page.
class _Pager extends ConsumerWidget {
  const _Pager();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final total = ref.watch(bgTasksProvider).valueOrNull?.total ?? 0;
    if (total <= bgPageSize) return const SizedBox.shrink();
    final page = ref.watch(bgPageProvider);
    final pages = (total + bgPageSize - 1) ~/ bgPageSize;
    return Padding(
      padding: const EdgeInsets.only(top: AppTokens.s12),
      child: Row(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          IconButton(
            icon: const Icon(Icons.chevron_left, size: 18),
            onPressed: page > 0
                ? () => ref.read(bgPageProvider.notifier).state = page - 1
                : null,
          ),
          Text('${page + 1} / $pages',
              style: TextStyle(color: c.textSecondary, fontSize: 12)),
          IconButton(
            icon: const Icon(Icons.chevron_right, size: 18),
            onPressed: page + 1 < pages
                ? () => ref.read(bgPageProvider.notifier).state = page + 1
                : null,
          ),
        ],
      ),
    );
  }
}

class _FilterEmpty extends StatelessWidget {
  const _FilterEmpty({required this.status});
  final String status;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: AppTokens.s32),
      child: Center(
        child: Text('Không có task nào ở trạng thái "$status".',
            style: TextStyle(color: c.textMuted, fontSize: 12)),
      ),
    );
  }
}

class _StatsRow extends ConsumerWidget {
  const _StatsRow();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final stats = ref.watch(bgStatsProvider);
    return stats.when(
      loading: () => const SizedBox(height: 72),
      error: (_, _) => const SizedBox.shrink(),
      data: (s) {
        final t = s.totals;
        return Row(
          children: [
            _StatCard(
              label: 'Runs',
              value: '${t.runs}',
              hint: t.running > 0 ? '${t.running} in flight' : null,
            ),
            _StatCard(
              label: 'Success',
              value: t.runs == 0 ? '—' : '${(t.successRate * 100).round()}%',
              // Skips are excluded from the rate on purpose: a template task
              // with nothing to do is healthy, not a failure.
              hint: t.skipped > 0 ? '${t.skipped} skipped' : null,
              color: t.runs == 0
                  ? null
                  : t.successRate >= 0.9
                      ? AppTokens.success
                      : t.successRate >= 0.6
                          ? AppTokens.warning
                          : AppTokens.danger,
            ),
            _StatCard(label: 'Avg', value: fmtBgDuration(t.avgDurationMs)),
            _StatCard(
              label: 'Tokens',
              value: _compact(t.tokensIn + t.tokensOut),
            ),
          ],
        );
      },
    );
  }

  static String _compact(int n) {
    if (n < 1000) return '$n';
    if (n < 1000000) return '${(n / 1000).toStringAsFixed(1)}k';
    return '${(n / 1000000).toStringAsFixed(1)}M';
  }
}

class _StatCard extends StatelessWidget {
  const _StatCard({
    required this.label,
    required this.value,
    this.hint,
    this.color,
  });
  final String label;
  final String value;
  final String? hint;
  final Color? color;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Expanded(
      child: Container(
        margin: const EdgeInsets.only(right: AppTokens.s8),
        padding: const EdgeInsets.all(AppTokens.s12),
        decoration: BoxDecoration(
          color: c.surface,
          borderRadius: BorderRadius.circular(AppTokens.rMd),
          border: Border.all(color: c.border),
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(label, style: TextStyle(color: c.textMuted, fontSize: 11)),
            const SizedBox(height: AppTokens.s4),
            Text(
              value,
              style: TextStyle(
                color: color ?? c.textPrimary,
                fontSize: 20,
                fontWeight: FontWeight.w700,
              ),
            ),
            if (hint != null)
              Text(hint!, style: TextStyle(color: c.textMuted, fontSize: 10)),
          ],
        ),
      ),
    );
  }
}

/// Auto-quarantined and currently-failing tasks. Nobody is watching a
/// background task, so this is the thing the screen exists to show.
class _AttentionBand extends ConsumerWidget {
  const _AttentionBand();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final stats = ref.watch(bgStatsProvider);
    final items = stats.valueOrNull?.attention ?? const <BackgroundAttention>[];
    if (items.isEmpty) return const SizedBox.shrink();

    return Container(
      margin: const EdgeInsets.only(bottom: AppTokens.s12),
      padding: const EdgeInsets.all(AppTokens.s12),
      decoration: BoxDecoration(
        color: AppTokens.danger.withValues(alpha: 0.08),
        borderRadius: BorderRadius.circular(AppTokens.rMd),
        border: Border.all(color: AppTokens.danger.withValues(alpha: 0.35)),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              const Icon(Icons.warning_amber_rounded,
                  size: 15, color: AppTokens.danger),
              const SizedBox(width: AppTokens.s6),
              Text(
                'Needs attention (${items.length})',
                style: const TextStyle(
                  color: AppTokens.danger,
                  fontSize: 12,
                  fontWeight: FontWeight.w700,
                ),
              ),
            ],
          ),
          const SizedBox(height: AppTokens.s6),
          ...items.map(
            (a) => Padding(
              padding: const EdgeInsets.symmetric(vertical: 2),
              child: InkWell(
                onTap: () =>
                    ref.read(bgSelectedTaskProvider.notifier).state = a.taskId,
                child: Row(
                  children: [
                    Expanded(
                      child: Text(
                        a.title,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: TextStyle(color: c.textPrimary, fontSize: 12),
                      ),
                    ),
                    if (a.status == 'failed')
                      const _Pill(text: 'auto-paused', color: AppTokens.danger),
                    const SizedBox(width: AppTokens.s6),
                    Text(
                      '${a.consecutiveFailures}× failed',
                      style: TextStyle(color: c.textMuted, fontSize: 11),
                    ),
                  ],
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _TaskRow extends ConsumerWidget {
  const _TaskRow({required this.task});
  final BackgroundTask task;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final selected = ref.watch(bgSelectedTaskProvider) == task.id;
    final live = ref.watch(bgLiveProvider).contains(task.id);
    // Keeps the "Next in …" label honest between refetches.
    ref.watch(bgClockProvider);
    final api = ref.read(backgroundApiProvider);

    return Container(
      margin: const EdgeInsets.only(bottom: AppTokens.s8),
      decoration: BoxDecoration(
        color: selected ? c.surfaceAlt : c.surface,
        borderRadius: BorderRadius.circular(AppTokens.rMd),
        border: Border.all(color: selected ? c.accent : c.border),
      ),
      child: InkWell(
        borderRadius: BorderRadius.circular(AppTokens.rMd),
        onTap: () => ref.read(bgSelectedTaskProvider.notifier).state =
            selected ? null : task.id,
        child: Padding(
          padding: const EdgeInsets.all(AppTokens.s12),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  if (live)
                    const Padding(
                      padding: EdgeInsets.only(right: AppTokens.s6),
                      child: SizedBox(
                        width: 11,
                        height: 11,
                        child: CircularProgressIndicator(strokeWidth: 2),
                      ),
                    ),
                  Expanded(
                    child: Text(
                      task.title,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(
                        color: c.textPrimary,
                        fontSize: 13,
                        fontWeight: FontWeight.w600,
                      ),
                    ),
                  ),
                  _StatusPill(task.status),
                ],
              ),
              const SizedBox(height: AppTokens.s6),
              Row(
                children: [
                  _Pill(
                    text: task.ownerLabel,
                    color: switch (task.ownerKind) {
                      'system' => c.textMuted,
                      'app' => AppTokens.brandAlt,
                      _ => AppTokens.brand,
                    },
                  ),
                  const SizedBox(width: AppTokens.s6),
                  if (task.isNative) ...[
                    const _Pill(text: 'native', color: AppTokens.cyan),
                    const SizedBox(width: AppTokens.s6),
                  ],
                  Expanded(
                    child: Text(
                      task.triggerLabel,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(color: c.textSecondary, fontSize: 11),
                    ),
                  ),
                ],
              ),
              const SizedBox(height: AppTokens.s6),
              Row(
                children: [
                  Icon(Icons.schedule, size: 12, color: c.textMuted),
                  const SizedBox(width: AppTokens.s4),
                  Text(
                    fmtBgNextRun(task.nextRun, task.status),
                    style: TextStyle(color: c.textMuted, fontSize: 11),
                  ),
                  const Spacer(),
                  _IconAction(
                    icon: task.status == 'active'
                        ? Icons.pause
                        : Icons.play_arrow,
                    tooltip: task.status == 'active' ? 'Pause' : 'Resume',
                    onTap: () => _guard(
                      context,
                      () => task.status == 'active'
                          ? api.pause(task.id)
                          : api.resume(task.id),
                    ),
                  ),
                  _IconAction(
                    icon: Icons.bolt,
                    tooltip: 'Run now',
                    onTap: () async {
                      final runId = await _guard(context, () => api.runNow(task.id));
                      if (runId != null && runId.isNotEmpty && context.mounted) {
                        showBackgroundSessionDialog(context, runId);
                      }
                    },
                  ),
                  if (task.isEditable)
                    _IconAction(
                      icon: Icons.edit_outlined,
                      tooltip: 'Edit',
                      onTap: () =>
                          showBackgroundTaskEditor(context, ref, task: task),
                    ),
                  _IconAction(
                    icon: Icons.delete_outline,
                    tooltip: task.isEditable
                        ? 'Delete'
                        : task.ownerKind == 'app'
                            ? 'Owned by ${task.ownerId} — uninstall the app to remove'
                            : 'Core upkeep — pause it instead',
                    enabled: task.isEditable,
                    onTap: () => _confirmDelete(context, ref, task),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }
}

Future<void> _confirmDelete(
    BuildContext context, WidgetRef ref, BackgroundTask t) async {
  final ok = await showDialog<bool>(
    context: context,
    builder: (ctx) => AlertDialog(
      title: Text('Delete "${t.title}"?'),
      content: const Text(
        'The task stops firing and is removed. Its run history is kept.',
      ),
      actions: [
        TextButton(
            onPressed: () => Navigator.pop(ctx, false),
            child: const Text('Cancel')),
        FilledButton(
          style: FilledButton.styleFrom(backgroundColor: AppTokens.danger),
          onPressed: () => Navigator.pop(ctx, true),
          child: const Text('Delete'),
        ),
      ],
    ),
  );
  if (ok != true || !context.mounted) return;
  await _guard(context, () => ref.read(backgroundApiProvider).delete(t.id));
  if (ref.read(bgSelectedTaskProvider) == t.id) {
    ref.read(bgSelectedTaskProvider.notifier).state = null;
  }
}

// ─── right: detail + run history ─────────────────────────────────────────────

class _DetailPane extends ConsumerWidget {
  const _DetailPane({required this.taskId, required this.task});
  final String taskId;
  final BackgroundTask? task;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final t = task;
    if (t == null) {
      return Center(
        child: Text('Task no longer exists',
            style: TextStyle(color: c.textMuted, fontSize: 12)),
      );
    }
    final runs = ref.watch(bgRunsProvider(taskId));

    return ListView(
      padding: const EdgeInsets.all(AppTokens.s16),
      children: [
        Row(
          children: [
            Expanded(
              child: Text(
                t.title,
                style: TextStyle(
                  color: c.textPrimary,
                  fontSize: 15,
                  fontWeight: FontWeight.w700,
                ),
              ),
            ),
            IconButton(
              icon: const Icon(Icons.close, size: 16),
              tooltip: 'Close',
              onPressed: () =>
                  ref.read(bgSelectedTaskProvider.notifier).state = null,
            ),
          ],
        ),
        if (t.description != null && t.description!.isNotEmpty)
          Padding(
            padding: const EdgeInsets.only(bottom: AppTokens.s8),
            child: Text(t.description!,
                style: TextStyle(color: c.textSecondary, fontSize: 12)),
          ),
        if (!t.isEditable)
          Container(
            margin: const EdgeInsets.only(bottom: AppTokens.s12),
            padding: const EdgeInsets.all(AppTokens.s8),
            decoration: BoxDecoration(
              color: c.surfaceAlt,
              borderRadius: BorderRadius.circular(AppTokens.rSm),
            ),
            child: Text(
              t.ownerKind == 'app'
                  ? 'Declared by the "${t.ownerId}" app. Its configuration lives in '
                      'the app manifest — an edit here would be reverted on reinstall. '
                      'You can still pause it or run it now.'
                  : 'Core upkeep. Its body is Rust, not a prompt. You can pause it '
                      'or run it now.',
              style: TextStyle(color: c.textMuted, fontSize: 11, height: 1.4),
            ),
          ),
        _kv(context, 'Trigger', t.triggerLabel),
        _kv(
          context,
          'Next run',
          t.status == 'active'
              ? '${fmtBgTime(t.nextRun)} · ${fmtBgNextRun(t.nextRun, t.status)}'
              : '—',
        ),
        _kv(context, 'Last run', fmtBgTime(t.lastRun)),
        _kv(context, 'Prompt kind', t.promptKind),
        if (t.contextUrl != null) _kv(context, 'Context URL', t.contextUrl!),
        if (t.persona != null) _kv(context, 'Persona', t.persona!),
        if (t.nativeJob != null) _kv(context, 'Native job', t.nativeJob!),
        _kv(context, 'Continuity',
            t.continuity == 'thread' ? 'thread (remembers prior runs)' : 'fresh'),
        _kv(context, 'On overlap', t.overlapPolicy),
        if (t.useTools.isNotEmpty) _kv(context, 'Tools', t.useTools.join(', ')),
        if (t.consecutiveFailures > 0)
          _kv(context, 'Consecutive failures',
              '${t.consecutiveFailures} / ${t.maxFailures == 0 ? '∞' : t.maxFailures}'),
        if (t.prompt != null && t.prompt!.isNotEmpty) ...[
          const SizedBox(height: AppTokens.s12),
          Text('Prompt',
              style: TextStyle(
                  color: c.textMuted, fontSize: 11, fontWeight: FontWeight.w600)),
          const SizedBox(height: AppTokens.s4),
          Container(
            padding: const EdgeInsets.all(AppTokens.s8),
            decoration: BoxDecoration(
              color: c.surface,
              borderRadius: BorderRadius.circular(AppTokens.rSm),
              border: Border.all(color: c.border),
            ),
            child: SelectableText(
              t.prompt!,
              style: TextStyle(
                  color: c.textSecondary, fontSize: 11, fontFamily: 'monospace'),
            ),
          ),
        ],
        const SizedBox(height: AppTokens.s16),
        Text('Run history',
            style: TextStyle(
                color: c.textPrimary, fontSize: 13, fontWeight: FontWeight.w700)),
        const SizedBox(height: AppTokens.s8),
        runs.when(
          loading: () => const Padding(
            padding: EdgeInsets.all(AppTokens.s16),
            child: Center(child: CircularProgressIndicator()),
          ),
          error: (e, _) => _ErrorPane(message: '$e'),
          data: (list) => list.isEmpty
              ? Text('Has not run yet.',
                  style: TextStyle(color: c.textMuted, fontSize: 12))
              : Column(children: list.map((r) => _RunRow(run: r)).toList()),
        ),
      ],
    );
  }

  Widget _kv(BuildContext context, String k, String v) {
    final c = context.colors;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 3),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: 128,
            child: Text(k, style: TextStyle(color: c.textMuted, fontSize: 11)),
          ),
          Expanded(
            child: SelectableText(v,
                style: TextStyle(color: c.textSecondary, fontSize: 11)),
          ),
        ],
      ),
    );
  }
}

class _RunRow extends ConsumerWidget {
  const _RunRow({required this.run});
  final BackgroundRun run;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    return InkWell(
      onTap: () => showBackgroundSessionDialog(context, run.id),
      child: Container(
        margin: const EdgeInsets.only(bottom: AppTokens.s4),
        padding: const EdgeInsets.symmetric(
            horizontal: AppTokens.s8, vertical: AppTokens.s6),
        decoration: BoxDecoration(
          color: c.surface,
          borderRadius: BorderRadius.circular(AppTokens.rSm),
          border: Border.all(color: c.border),
        ),
        child: Row(
          children: [
            _StatusPill(run.status),
            const SizedBox(width: AppTokens.s8),
            Text(fmtBgTime(run.startedAt),
                style: TextStyle(color: c.textSecondary, fontSize: 11)),
            const SizedBox(width: AppTokens.s8),
            Text(fmtBgDuration(run.durationMs),
                style: TextStyle(color: c.textMuted, fontSize: 11)),
            if (run.triggerKind != 'schedule') ...[
              const SizedBox(width: AppTokens.s6),
              _Pill(text: run.triggerKind, color: c.textMuted),
            ],
            const Spacer(),
            if (run.isRunning)
              _IconAction(
                icon: Icons.stop_circle_outlined,
                tooltip: 'Cancel run',
                onTap: () => _guard(
                    context, () => ref.read(backgroundApiProvider).cancelRun(run.id)),
              ),
            Expanded(
              flex: 2,
              child: Text(
                run.error ?? run.result ?? '',
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                textAlign: TextAlign.right,
                style: TextStyle(
                  color: run.isFailure ? AppTokens.danger : c.textMuted,
                  fontSize: 11,
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

// ─── shared bits ─────────────────────────────────────────────────────────────

Color bgStatusColor(String status, AppColors c) => switch (status) {
      'running' => AppTokens.brand,
      'success' || 'active' => AppTokens.success,
      'error' || 'timeout' || 'failed' => AppTokens.danger,
      'skipped' => c.textMuted,
      'paused' || 'cancelled' => AppTokens.warning,
      'completed' => c.textSecondary,
      _ => c.textSecondary,
    };

class _StatusPill extends StatelessWidget {
  const _StatusPill(this.status);
  final String status;

  @override
  Widget build(BuildContext context) {
    return _Pill(text: status, color: bgStatusColor(status, context.colors));
  }
}

class _Pill extends StatelessWidget {
  const _Pill({required this.text, required this.color});
  final String text;
  final Color color;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 1),
      decoration: BoxDecoration(
        color: color.withValues(alpha: 0.14),
        borderRadius: BorderRadius.circular(AppTokens.rSm),
        border: Border.all(color: color.withValues(alpha: 0.4)),
      ),
      child: Text(
        text,
        style: TextStyle(color: color, fontSize: 10, fontWeight: FontWeight.w600),
      ),
    );
  }
}

class _IconAction extends StatelessWidget {
  const _IconAction({
    required this.icon,
    required this.tooltip,
    required this.onTap,
    this.enabled = true,
  });
  final IconData icon;
  final String tooltip;
  final VoidCallback onTap;
  final bool enabled;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Tooltip(
      message: tooltip,
      child: IconButton(
        icon: Icon(icon, size: 15),
        color: enabled ? c.textSecondary : c.textMuted.withValues(alpha: 0.4),
        visualDensity: VisualDensity.compact,
        constraints: const BoxConstraints(minWidth: 28, minHeight: 28),
        padding: EdgeInsets.zero,
        onPressed: enabled ? onTap : null,
      ),
    );
  }
}

class _WindowPicker extends ConsumerWidget {
  const _WindowPicker();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final w = ref.watch(bgWindowProvider);
    return SegmentedButton<String>(
      segments: const [
        ButtonSegment(value: '24h', label: Text('24h')),
        ButtonSegment(value: '7d', label: Text('7d')),
        ButtonSegment(value: '30d', label: Text('30d')),
      ],
      selected: {w},
      showSelectedIcon: false,
      style: const ButtonStyle(
        visualDensity: VisualDensity.compact,
        textStyle: WidgetStatePropertyAll(TextStyle(fontSize: 11)),
      ),
      onSelectionChanged: (s) =>
          ref.read(bgWindowProvider.notifier).state = s.first,
    );
  }
}

class _InternalToggle extends ConsumerWidget {
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final on = ref.watch(bgShowInternalProvider);
    return Tooltip(
      message: 'Show core upkeep jobs (cognitive decay, maintenance, …)',
      child: TextButton.icon(
        icon: Icon(on ? Icons.visibility : Icons.visibility_off, size: 15),
        label: const Text('System', style: TextStyle(fontSize: 11)),
        onPressed: () =>
            ref.read(bgShowInternalProvider.notifier).state = !on,
      ),
    );
  }
}

class _EmptyState extends StatelessWidget {
  const _EmptyState();

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: AppTokens.s48),
      child: Column(
        children: [
          // Matches the nav rail — see the icon note in `app/nav.dart`.
          Icon(Icons.pending_actions, size: 32, color: c.textMuted),
          const SizedBox(height: AppTokens.s8),
          Text('No background tasks',
              style: TextStyle(color: c.textSecondary, fontSize: 13)),
          const SizedBox(height: AppTokens.s4),
          SizedBox(
            width: 320,
            child: Text(
              'Background tasks run on a schedule with nobody watching — periodic '
              'upkeep, unattended follow-up, an app\'s standing duties. Unlike a '
              'calendar schedule, they never reply to you; their output lands here.',
              textAlign: TextAlign.center,
              style: TextStyle(color: c.textMuted, fontSize: 11, height: 1.5),
            ),
          ),
        ],
      ),
    );
  }
}

class _ErrorPane extends StatelessWidget {
  const _ErrorPane({required this.message});
  final String message;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.all(AppTokens.s16),
      child: Text(message,
          style: const TextStyle(color: AppTokens.danger, fontSize: 12)),
    );
  }
}

/// Run an action, surfacing the daemon's own error text.
///
/// The daemon says useful things here — quota exceeded, an app-owned task can't
/// be deleted, a run already in flight — so show its message rather than a
/// generic failure.
Future<T?> _guard<T>(BuildContext context, Future<T> Function() action) async {
  try {
    return await action();
  } catch (e) {
    if (context.mounted) {
      final msg = e is Exception ? '$e'.replaceFirst(RegExp(r'^\w+Exception\(\d+\): '), '') : '$e';
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(msg)));
    }
    return null;
  }
}
