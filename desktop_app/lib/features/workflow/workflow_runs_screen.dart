import 'dart:async';
import 'dart:io' show File;

import 'package:file_picker/file_picker.dart';
import 'package:flutter/foundation.dart' show kIsWeb;
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../models/workflow_models.dart';
import '../../theme/tokens.dart';
import '../../widgets/app_markdown.dart';
import '../plugins/plugins_screen.dart' show pluginsSectionProvider;
import 'workflow_providers.dart';
import 'workflow_run_dialog.dart';

/// The run the monitor opens with (set before navigating to /workflow-runs).
final openWorkflowRunProvider = StateProvider<String?>((ref) => null);

const _pageSize = 10;

enum _RunSort { recent, created, name }

enum _RunGroup { date, workflow, none }

Color _runStatusColor(String status, AppColors c) => switch (status) {
      'running' => AppTokens.brand,
      'done' => AppTokens.success,
      'partial-failed' => AppTokens.warning,
      'interrupted' => AppTokens.danger,
      _ => c.textSecondary,
    };

Color _stepStatusColor(String status, AppColors c) => switch (status) {
      'running' => AppTokens.brand,
      'done' => AppTokens.success,
      'failed' => AppTokens.danger,
      'skipped' => c.textSecondary,
      _ => c.textSecondary.withValues(alpha: 0.6),
    };

String _fmtTime(String? iso) {
  if (iso == null || iso.isEmpty) return '—';
  return iso.replaceFirst('T', ' ').split('.').first;
}

String _fmtDuration(String? a, String? b) {
  if (a == null || b == null) return '';
  final start = DateTime.tryParse(a);
  final end = DateTime.tryParse(b);
  if (start == null || end == null) return '';
  final ms = end.difference(start).inMilliseconds;
  if (ms < 0) return '';
  if (ms < 1000) return '${ms}ms';
  if (ms < 60000) return '${(ms / 1000).toStringAsFixed(1)}s';
  return '${ms ~/ 60000}m${((ms % 60000) / 1000).round()}s';
}

String _dateBucket(String iso) {
  final d = DateTime.tryParse(iso);
  if (d == null) return 'Older';
  final now = DateTime.now();
  bool sameDay(DateTime a, DateTime b) =>
      a.year == b.year && a.month == b.month && a.day == b.day;
  if (sameDay(d, now)) return 'Today';
  if (sameDay(d, now.subtract(const Duration(days: 1)))) return 'Yesterday';
  final diff = now.difference(d).inDays;
  if (diff <= 7) return 'Past 7 days';
  if (diff <= 30) return 'Past 30 days';
  return 'Older';
}

/// Shared rename dialog (sidebar section, run monitor, detail header).
Future<void> showRenameRunDialog(
    BuildContext context, WidgetRef ref, WorkflowRun r) async {
  final ctrl = TextEditingController(text: r.label ?? '');
  final ok = await showDialog<bool>(
    context: context,
    builder: (ctx) => AlertDialog(
      title: const Text('Rename run'),
      content: TextField(
        controller: ctrl,
        autofocus: true,
        decoration: InputDecoration(
          hintText: r.id,
          helperText: 'Empty resets back to the run id.',
          border: const OutlineInputBorder(),
          isDense: true,
        ),
        onSubmitted: (_) => Navigator.pop(ctx, true),
      ),
      actions: [
        TextButton(
            onPressed: () => Navigator.pop(ctx, false),
            child: const Text('Cancel')),
        FilledButton(
            onPressed: () => Navigator.pop(ctx, true),
            child: const Text('Save')),
      ],
    ),
  );
  if (ok != true) return;
  try {
    await renameWorkflowRun(ref, r.id, ctrl.text);
  } catch (e) {
    if (context.mounted) {
      ScaffoldMessenger.of(context)
          .showSnackBar(SnackBar(content: Text('Rename failed: $e')));
    }
  }
}

/// Shared delete-confirm dialog. Returns true when deleted.
Future<bool> showDeleteRunDialog(
    BuildContext context, WidgetRef ref, WorkflowRun r) async {
  final ok = await showDialog<bool>(
    context: context,
    builder: (ctx) => AlertDialog(
      title: Text('Delete run "${r.title}"?'),
      content:
          const Text('Only the history record is removed — workspace files are kept.'),
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
  if (ok != true) return false;
  try {
    await deleteWorkflowRun(ref, r.id);
    return true;
  } catch (e) {
    if (context.mounted) {
      ScaffoldMessenger.of(context)
          .showSnackBar(SnackBar(content: Text('Delete failed: $e')));
    }
    return false;
  }
}

/// Run monitor: grouped/sorted run list (left, 10-per-page) + full detail of
/// the selected run (right). Templates live in Plugins → Workflow.
class WorkflowRunsScreen extends ConsumerStatefulWidget {
  const WorkflowRunsScreen({super.key});
  @override
  ConsumerState<WorkflowRunsScreen> createState() => _WorkflowRunsScreenState();
}

class _WorkflowRunsScreenState extends ConsumerState<WorkflowRunsScreen> {
  Timer? _poll;
  _RunSort _sort = _RunSort.recent;
  _RunGroup _group = _RunGroup.date;
  int _limit = _pageSize;

  @override
  void initState() {
    super.initState();
    _poll = Timer.periodic(const Duration(seconds: 3), (_) {
      ref.invalidate(workflowRunsProvider);
    });
  }

  @override
  void dispose() {
    _poll?.cancel();
    super.dispose();
  }

  void _snack(String msg) {
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(msg)));
  }

  Future<void> _cancel(String id) async {
    try {
      await cancelWorkflowRun(ref, id);
      _snack('Cancel requested: $id');
    } catch (e) {
      _snack('Cancel failed: $e');
    }
  }

  void _rerun(WorkflowRun r) {
    final defs = ref.read(workflowsProvider).valueOrNull ?? [];
    WorkflowDefSummary? def;
    for (final d in defs) {
      if (d.name == r.workflowName) {
        def = d;
        break;
      }
    }
    if (def == null) {
      _snack('Definition "${r.workflowName}" no longer exists');
      return;
    }
    showWorkflowRunDialog(context, ref, def, preset: r.inputs, onStarted: (id) {
      ref.read(openWorkflowRunProvider.notifier).state = id;
      ref.invalidate(workflowRunsProvider);
    });
  }

  List<WorkflowRun> _sorted(List<WorkflowRun> runs) {
    final arr = [...runs];
    switch (_sort) {
      case _RunSort.name:
        arr.sort((a, b) => a.title.toLowerCase().compareTo(b.title.toLowerCase()));
      case _RunSort.created:
        arr.sort((a, b) => b.createdAt.compareTo(a.createdAt));
      case _RunSort.recent:
        String ts(WorkflowRun r) => r.completedAt ?? r.createdAt;
        arr.sort((a, b) => ts(b).compareTo(ts(a)));
    }
    return arr;
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final runsAsync = ref.watch(workflowRunsProvider);
    final all = _sorted(runsAsync.valueOrNull ?? const <WorkflowRun>[]);
    final visible = all.take(_limit).toList();
    final selectedId = ref.watch(openWorkflowRunProvider);
    final selected = all.isEmpty
        ? null
        : all.firstWhere((r) => r.id == selectedId, orElse: () => all.first);

    // Group the visible page.
    final groups = <String, List<WorkflowRun>>{};
    for (final r in visible) {
      final key = switch (_group) {
        _RunGroup.none => '',
        _RunGroup.workflow => r.workflowName,
        _RunGroup.date => _dateBucket(r.createdAt),
      };
      groups.putIfAbsent(key, () => []).add(r);
    }

    return Row(
      children: [
        // ── Left: run list ──
        SizedBox(
          width: 310,
          child: Container(
            color: c.sidebar,
            child: Column(
              children: [
                Padding(
                  padding: const EdgeInsets.fromLTRB(
                      AppTokens.s8, AppTokens.s8, AppTokens.s8, AppTokens.s4),
                  child: Row(
                    children: [
                      IconButton(
                        tooltip: 'Workflow templates',
                        icon: const Icon(Icons.arrow_back_rounded, size: 18),
                        onPressed: () {
                          ref.read(pluginsSectionProvider.notifier).state =
                              'workflow';
                          context.go('/plugins');
                        },
                      ),
                      Text('Workflow runs',
                          style: TextStyle(
                              color: c.textPrimary,
                              fontSize: 15,
                              fontWeight: FontWeight.w700)),
                      const Spacer(),
                      PopupMenuButton<String>(
                        tooltip: 'Group & sort',
                        icon: Icon(Icons.filter_list_rounded,
                            size: 18, color: c.textSecondary),
                        onSelected: (v) => setState(() {
                          switch (v) {
                            case 'g-date': _group = _RunGroup.date;
                            case 'g-wf': _group = _RunGroup.workflow;
                            case 'g-none': _group = _RunGroup.none;
                            case 's-recent': _sort = _RunSort.recent;
                            case 's-created': _sort = _RunSort.created;
                            case 's-name': _sort = _RunSort.name;
                          }
                        }),
                        itemBuilder: (ctx) => [
                          const PopupMenuItem(
                              enabled: false, height: 28, child: Text('Group by')),
                          CheckedPopupMenuItem(
                              value: 'g-date',
                              checked: _group == _RunGroup.date,
                              child: const Text('Date')),
                          CheckedPopupMenuItem(
                              value: 'g-wf',
                              checked: _group == _RunGroup.workflow,
                              child: const Text('Workflow')),
                          CheckedPopupMenuItem(
                              value: 'g-none',
                              checked: _group == _RunGroup.none,
                              child: const Text('No grouping')),
                          const PopupMenuDivider(),
                          const PopupMenuItem(
                              enabled: false, height: 28, child: Text('Sort by')),
                          CheckedPopupMenuItem(
                              value: 's-recent',
                              checked: _sort == _RunSort.recent,
                              child: const Text('Recent activity')),
                          CheckedPopupMenuItem(
                              value: 's-created',
                              checked: _sort == _RunSort.created,
                              child: const Text('Created')),
                          CheckedPopupMenuItem(
                              value: 's-name',
                              checked: _sort == _RunSort.name,
                              child: const Text('Name A–Z')),
                        ],
                      ),
                      IconButton(
                        tooltip: 'Refresh',
                        icon: const Icon(Icons.refresh_rounded, size: 18),
                        onPressed: () => ref.invalidate(workflowRunsProvider),
                      ),
                    ],
                  ),
                ),
                Expanded(
                  child: runsAsync.when(
                    loading: () =>
                        const Center(child: CircularProgressIndicator()),
                    error: (e, _) => Center(
                        child: Text('Cannot load runs: $e',
                            style: TextStyle(color: c.textSecondary))),
                    data: (_) => all.isEmpty
                        ? Center(
                            child: Text('No runs yet',
                                style: TextStyle(
                                    color: c.textSecondary, fontSize: 12)))
                        : ListView(
                            padding: const EdgeInsets.all(AppTokens.s8),
                            children: [
                              for (final e in groups.entries) ...[
                                if (e.key.isNotEmpty)
                                  Padding(
                                    padding: const EdgeInsets.fromLTRB(
                                        AppTokens.s4, AppTokens.s8,
                                        AppTokens.s4, AppTokens.s4),
                                    child: Text(e.key.toUpperCase(),
                                        style: TextStyle(
                                          color: c.textMuted,
                                          fontSize: 10,
                                          fontWeight: FontWeight.w700,
                                          letterSpacing: 1.1,
                                        )),
                                  ),
                                for (final r in e.value)
                                  _runCard(c, r, selected?.id == r.id),
                              ],
                              if (all.length > _limit)
                                Padding(
                                  padding:
                                      const EdgeInsets.only(top: AppTokens.s4),
                                  child: OutlinedButton(
                                    onPressed: () => setState(
                                        () => _limit += _pageSize),
                                    child: Text(
                                        'Show more (${all.length - _limit})'),
                                  ),
                                ),
                            ],
                          ),
                  ),
                ),
              ],
            ),
          ),
        ),
        Container(width: 1, color: c.border),
        // ── Right: run detail ──
        Expanded(
          child: selected == null
              ? Center(
                  child: Text('Select a run to see its details',
                      style: TextStyle(color: c.textSecondary)))
              : WorkflowRunDetail(
                  run: selected,
                  onCancel: () => _cancel(selected.id),
                  onRerun: () => _rerun(selected),
                  onDeleted: () =>
                      ref.read(openWorkflowRunProvider.notifier).state = null,
                ),
        ),
      ],
    );
  }

  Widget _runCard(AppColors c, WorkflowRun r, bool active) {
    return GestureDetector(
      onTap: () => ref.read(openWorkflowRunProvider.notifier).state = r.id,
      child: Container(
        margin: const EdgeInsets.only(bottom: AppTokens.s6),
        padding: const EdgeInsets.fromLTRB(
            AppTokens.s8, AppTokens.s6, 2, AppTokens.s6),
        decoration: BoxDecoration(
          color: active ? c.accent.withValues(alpha: 0.10) : c.surface,
          borderRadius: BorderRadius.circular(AppTokens.rLg),
          border: Border.all(color: active ? c.accent : c.border),
        ),
        child: Row(
          children: [
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(children: [
                    Expanded(
                      child: Text(r.title,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(
                              color: c.textPrimary,
                              fontSize: 12.5,
                              fontWeight: FontWeight.w600)),
                    ),
                    _chip(c, r.status, color: _runStatusColor(r.status, c)),
                  ]),
                  const SizedBox(height: 2),
                  Row(children: [
                    Expanded(
                      child: Text(r.workflowName,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(
                              color: c.textSecondary, fontSize: 10.5)),
                    ),
                    Text(_fmtTime(r.createdAt),
                        style:
                            TextStyle(color: c.textMuted, fontSize: 10)),
                  ]),
                ],
              ),
            ),
            PopupMenuButton<String>(
              tooltip: '',
              padding: EdgeInsets.zero,
              icon: Icon(Icons.more_vert_rounded,
                  size: 15, color: c.textMuted),
              onSelected: (v) async {
                switch (v) {
                  case 'rename':
                    await showRenameRunDialog(context, ref, r);
                  case 'cancel':
                    await _cancel(r.id);
                  case 'delete':
                    final gone = await showDeleteRunDialog(context, ref, r);
                    if (gone &&
                        ref.read(openWorkflowRunProvider) == r.id) {
                      ref.read(openWorkflowRunProvider.notifier).state = null;
                    }
                }
              },
              itemBuilder: (ctx) => [
                const PopupMenuItem(value: 'rename', child: Text('Rename')),
                if (r.isActive)
                  const PopupMenuItem(value: 'cancel', child: Text('Cancel run'))
                else
                  const PopupMenuItem(value: 'delete', child: Text('Delete')),
              ],
            ),
          ],
        ),
      ),
    );
  }
}

Widget _chip(AppColors c, String label, {Color? color}) {
  final fg = color ?? c.textSecondary;
  return Container(
    padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
    decoration: BoxDecoration(
      color: fg.withValues(alpha: 0.12),
      borderRadius: BorderRadius.circular(AppTokens.rFull),
    ),
    child: Text(label, style: TextStyle(color: fg, fontSize: 11)),
  );
}

/// Read-only detail of one workflow run: header actions (rename / download /
/// wiki / delete / cancel / re-run), info block, then per-step cards with
/// markdown-rendered observe + result and per-step export actions.
class WorkflowRunDetail extends ConsumerWidget {
  const WorkflowRunDetail({
    super.key,
    required this.run,
    required this.onCancel,
    required this.onRerun,
    this.onDeleted,
  });
  final WorkflowRun run;
  final VoidCallback onCancel;
  final VoidCallback onRerun;
  final VoidCallback? onDeleted;

  void _snack(BuildContext context, String msg) {
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(msg)));
  }

  Future<void> _download(
      BuildContext context, String fileName, String content) async {
    if (kIsWeb) {
      await Clipboard.setData(ClipboardData(text: content));
      if (context.mounted) _snack(context, 'Copied to clipboard');
      return;
    }
    final path = await FilePicker.platform.saveFile(
      dialogTitle: 'Save markdown',
      fileName: fileName,
      type: FileType.custom,
      allowedExtensions: ['md'],
    );
    if (path == null) return;
    await File(path).writeAsString(content);
    if (context.mounted) _snack(context, 'Saved to $path');
  }

  Future<void> _wiki(BuildContext context, WidgetRef ref, String path,
      String content) async {
    try {
      await saveRunToWiki(ref, path, content);
      if (context.mounted) _snack(context, 'Saved to wiki: $path');
    } catch (e) {
      if (context.mounted) _snack(context, 'Wiki save failed: $e');
    }
  }

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final wfSeg = sanitizeWikiSegment(run.workflowName);
    final runSeg = sanitizeWikiSegment(run.id);
    return SingleChildScrollView(
      padding: const EdgeInsets.all(AppTokens.s16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(children: [
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(run.title,
                      style: TextStyle(
                          color: c.textPrimary,
                          fontSize: 17,
                          fontWeight: FontWeight.w700)),
                  if (run.label != null)
                    Text(run.id,
                        style: TextStyle(
                            color: c.textMuted,
                            fontSize: 11,
                            fontFamily: 'monospace')),
                ],
              ),
            ),
            _chip(c, run.status, color: _runStatusColor(run.status, c)),
            const SizedBox(width: AppTokens.s6),
            IconButton(
              tooltip: 'Rename',
              icon: Icon(Icons.edit_outlined, size: 17, color: c.textSecondary),
              onPressed: () => showRenameRunDialog(context, ref, run),
            ),
            IconButton(
              tooltip: 'Download full result (.md)',
              icon: Icon(Icons.download_outlined,
                  size: 17, color: c.textSecondary),
              onPressed: () => _download(
                  context, '${sanitizeWikiSegment(run.title)}.md',
                  run.toMarkdown()),
            ),
            IconButton(
              tooltip: 'Save full result to wiki',
              icon: Icon(Icons.menu_book_outlined,
                  size: 17, color: c.textSecondary),
              onPressed: () => _wiki(context, ref,
                  'workflows/$wfSeg/$runSeg.md', run.toMarkdown()),
            ),
            IconButton(
              tooltip: run.isActive ? 'Cancel the run before deleting' : 'Delete run',
              icon: Icon(Icons.delete_outline,
                  size: 17,
                  color: run.isActive ? c.textMuted : AppTokens.danger),
              onPressed: run.isActive
                  ? null
                  : () async {
                      final gone =
                          await showDeleteRunDialog(context, ref, run);
                      if (gone) onDeleted?.call();
                    },
            ),
            const SizedBox(width: AppTokens.s4),
            if (run.isActive)
              OutlinedButton.icon(
                onPressed: onCancel,
                icon: const Icon(Icons.stop_circle_outlined, size: 16),
                label: const Text('Cancel'),
                style: OutlinedButton.styleFrom(
                    foregroundColor: AppTokens.danger),
              )
            else
              OutlinedButton.icon(
                onPressed: onRerun,
                icon: const Icon(Icons.replay_rounded, size: 16),
                label: const Text('Re-run'),
              ),
          ]),
          const SizedBox(height: AppTokens.s12),

          // Info block
          Container(
            padding: const EdgeInsets.all(AppTokens.s12),
            decoration: BoxDecoration(
              color: c.surface,
              borderRadius: BorderRadius.circular(AppTokens.rLg),
              border: Border.all(color: c.border),
            ),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                _infoRow(c, 'Workflow', run.workflowName),
                _infoRow(c, 'Trigger', run.trigger ?? '—'),
                _infoRow(c, 'Started', _fmtTime(run.createdAt)),
                _infoRow(
                    c,
                    'Finished',
                    run.completedAt == null
                        ? '—'
                        : '${_fmtTime(run.completedAt)} (${_fmtDuration(run.createdAt, run.completedAt)})'),
                if (run.inputs.isNotEmpty)
                  Padding(
                    padding: const EdgeInsets.only(top: 4),
                    child: Wrap(spacing: 4, runSpacing: 4, children: [
                      for (final e in run.inputs.entries)
                        _chip(c, '${e.key}=${e.value}'),
                    ]),
                  ),
                const SizedBox(height: 4),
                Text(run.runDir,
                    style: TextStyle(
                        color: c.textSecondary,
                        fontSize: 10,
                        fontFamily: 'monospace')),
              ],
            ),
          ),
          const SizedBox(height: AppTokens.s16),

          Text('Steps (${run.steps.length})',
              style: TextStyle(
                  color: c.textPrimary,
                  fontSize: 14,
                  fontWeight: FontWeight.w700)),
          const SizedBox(height: AppTokens.s8),
          for (final s in run.steps)
            _StepCard(
              step: s,
              onDownload: s.result.isEmpty
                  ? null
                  : () => _download(context, '$runSeg-${s.id}.md', s.result),
              onWiki: s.result.isEmpty
                  ? null
                  : () => _wiki(
                      context,
                      ref,
                      'workflows/$wfSeg/$runSeg-${sanitizeWikiSegment(s.id)}.md',
                      '# ${run.title} — ${s.id}\n\n${s.result}'),
            ),
        ],
      ),
    );
  }

  Widget _infoRow(AppColors c, String label, String value) => Padding(
        padding: const EdgeInsets.only(bottom: 2),
        child: Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
          SizedBox(
              width: 70,
              child: Text(label,
                  style: TextStyle(color: c.textSecondary, fontSize: 12))),
          Expanded(
              child: Text(value,
                  style: TextStyle(color: c.textPrimary, fontSize: 12))),
        ]),
      );
}

class _StepCard extends StatefulWidget {
  const _StepCard({required this.step, this.onDownload, this.onWiki});
  final WorkflowStepRun step;
  final VoidCallback? onDownload;
  final VoidCallback? onWiki;

  @override
  State<_StepCard> createState() => _StepCardState();
}

class _StepCardState extends State<_StepCard> {
  bool _open = true;

  WorkflowStepRun get step => widget.step;
  VoidCallback? get onDownload => widget.onDownload;
  VoidCallback? get onWiki => widget.onWiki;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Container(
      margin: const EdgeInsets.only(bottom: AppTokens.s8),
      padding: const EdgeInsets.all(AppTokens.s12),
      decoration: BoxDecoration(
        color: c.surface,
        borderRadius: BorderRadius.circular(AppTokens.rLg),
        border: Border.all(color: c.border),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          InkWell(
            onTap: () => setState(() => _open = !_open),
            child: Row(children: [
            Icon(
                _open
                    ? Icons.expand_more_rounded
                    : Icons.chevron_right_rounded,
                size: 16,
                color: c.textMuted),
            const SizedBox(width: 2),
            _chip(c, step.status, color: _stepStatusColor(step.status, c)),
            const SizedBox(width: AppTokens.s8),
            Text(step.id,
                style: TextStyle(
                    color: c.textPrimary,
                    fontWeight: FontWeight.w600,
                    fontSize: 13)),
            const SizedBox(width: AppTokens.s6),
            Text(step.kind,
                style: TextStyle(
                    color: step.kind == 'agent'
                        ? AppTokens.brand
                        : AppTokens.cyan,
                    fontSize: 11)),
            const Spacer(),
            if (onDownload != null)
              IconButton(
                tooltip: 'Download step result (.md)',
                icon: Icon(Icons.download_outlined,
                    size: 15, color: c.textMuted),
                visualDensity: VisualDensity.compact,
                onPressed: onDownload,
              ),
            if (onWiki != null)
              IconButton(
                tooltip: 'Save step result to wiki',
                icon: Icon(Icons.menu_book_outlined,
                    size: 15, color: c.textMuted),
                visualDensity: VisualDensity.compact,
                onPressed: onWiki,
              ),
            ]),
          ),
          if (_open && step.error != null) ...[
            const SizedBox(height: AppTokens.s6),
            Text(step.error!,
                style: TextStyle(color: AppTokens.danger, fontSize: 12)),
          ],
          if (_open && (step.observeContent ?? '').isNotEmpty) ...[
            const SizedBox(height: AppTokens.s8),
            Container(
              width: double.infinity,
              padding: const EdgeInsets.all(AppTokens.s8),
              decoration: BoxDecoration(
                color: c.surfaceAlt,
                borderRadius: BorderRadius.circular(AppTokens.rMd),
                border: Border(
                    left: BorderSide(color: c.accent, width: 3)),
              ),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text((step.observeLabel ?? 'observe').toUpperCase(),
                      style: TextStyle(
                          color: c.textSecondary,
                          fontSize: 10,
                          letterSpacing: 0.8)),
                  const SizedBox(height: 4),
                  AppMarkdown(step.observeContent!),
                ],
              ),
            ),
          ],
          if (_open && (step.observeArtifactPath ?? '').isNotEmpty) ...[
            const SizedBox(height: AppTokens.s6),
            Text('${step.observeLabel ?? 'artifact'}: ${step.observeArtifactPath}',
                style: TextStyle(
                    color: c.textSecondary,
                    fontSize: 11,
                    fontFamily: 'monospace')),
          ],
          if (_open && step.result.isNotEmpty)
            Theme(
              data: Theme.of(context)
                  .copyWith(dividerColor: Colors.transparent),
              child: ExpansionTile(
                tilePadding: EdgeInsets.zero,
                childrenPadding: EdgeInsets.zero,
                title: Text('Result (${step.result.length} chars)',
                    style: TextStyle(color: c.textSecondary, fontSize: 12)),
                children: [
                  Container(
                    width: double.infinity,
                    constraints: const BoxConstraints(maxHeight: 420),
                    padding: const EdgeInsets.all(AppTokens.s8),
                    decoration: BoxDecoration(
                      color: c.surfaceAlt,
                      borderRadius: BorderRadius.circular(AppTokens.rMd),
                    ),
                    child: SingleChildScrollView(
                      child: AppMarkdown(step.result),
                    ),
                  ),
                ],
              ),
            ),
        ],
      ),
    );
  }
}
