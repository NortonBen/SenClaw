import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../core/i18n/l10n.dart';
import '../../models/workflow_models.dart';
import '../../theme/tokens.dart';
import '../chat/agents_provider.dart' show selectedJidProvider;
import 'workflow_providers.dart';
import 'workflow_run_dialog.dart';
import 'workflow_runs_screen.dart'
    show
        WorkflowRunDetail,
        openWorkflowRunProvider,
        showDeleteRunDialog,
        showRenameRunDialog;

/// Sentinel jid prefix marking a "workflow session" in the chat sidebar.
/// Never a real chat group — the chat screen renders [WorkflowSessionPane]
/// instead of a ConversationPane for these.
const wfRunJidPrefix = 'wfrun:';

String wfRunJid(String runId) => '$wfRunJidPrefix$runId';

Color _runStatusColor(String status, AppColors c) => switch (status) {
      'running' => AppTokens.brand,
      'done' => AppTokens.success,
      'partial-failed' => AppTokens.warning,
      'interrupted' => AppTokens.danger,
      _ => c.textSecondary,
    };

// ─── Sidebar section ─────────────────────────────────────────────────────────

/// "Workflows" section in the chat SessionList: recent runs shown as
/// sessions. Selecting one swaps the conversation pane for the run's flow
/// view (activity only — no chat composer).
class WorkflowSessionSection extends ConsumerStatefulWidget {
  const WorkflowSessionSection({super.key});
  @override
  ConsumerState<WorkflowSessionSection> createState() =>
      _WorkflowSessionSectionState();
}

class _WorkflowSessionSectionState
    extends ConsumerState<WorkflowSessionSection> {
  Timer? _poll;
  bool _collapsed = false;

  @override
  void initState() {
    super.initState();
    // Light poll so live runs tick in the sidebar; cheap no-op otherwise.
    _poll = Timer.periodic(const Duration(seconds: 5), (_) {
      final runs = ref.read(workflowRunsProvider).valueOrNull;
      if (runs != null && runs.any((r) => r.isActive)) {
        ref.invalidate(workflowRunsProvider);
      }
    });
  }

  @override
  void dispose() {
    _poll?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final runs = ref.watch(workflowRunsProvider).valueOrNull ??
        const <WorkflowRun>[];
    if (runs.isEmpty) return const SizedBox.shrink();
    final selected = ref.watch(selectedJidProvider);
    // EVERY running run + the most recent finished ones, 5 rows total.
    final running = runs.where((r) => r.isActive).toList();
    final finished = runs.where((r) => !r.isActive).toList();
    final visible = [
      ...running,
      ...finished.take((5 - running.length).clamp(0, 5)),
    ];

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(
              AppTokens.s16, AppTokens.s8, AppTokens.s8, AppTokens.s4),
          child: Row(
            children: [
              Expanded(
                child: Text(context.tr('WORKFLOWS'),
                    style: TextStyle(
                      color: c.textMuted,
                      fontSize: 11,
                      fontWeight: FontWeight.w700,
                      letterSpacing: 1.2,
                    )),
              ),
              InkWell(
                onTap: () => setState(() => _collapsed = !_collapsed),
                child: Icon(
                  _collapsed
                      ? Icons.expand_more_rounded
                      : Icons.expand_less_rounded,
                  size: 16,
                  color: c.textMuted,
                ),
              ),
            ],
          ),
        ),
        if (!_collapsed) ...[
          for (final r in visible) _runItem(c, r, selected),
          if (runs.length > visible.length)
            Padding(
              padding: const EdgeInsets.fromLTRB(
                  AppTokens.s12, 0, AppTokens.s12, AppTokens.s4),
              child: InkWell(
                onTap: () {
                  ref.read(openWorkflowRunProvider.notifier).state = null;
                  context.go('/workflow-runs');
                },
                child: Padding(
                  padding: const EdgeInsets.symmetric(vertical: 4),
                  child: Text(
                    context.trArgs('More {n} workflows →',
                        {'n': runs.length - visible.length}),
                    style: TextStyle(color: c.accent, fontSize: 11),
                  ),
                ),
              ),
            ),
        ],
      ],
    );
  }

  Widget _runItem(AppColors c, WorkflowRun r, String? selected) {
    final jid = wfRunJid(r.id);
    final active = selected == jid;
    return InkWell(
      onTap: () => ref.read(selectedJidProvider.notifier).state = jid,
      child: Container(
        margin: const EdgeInsets.symmetric(
            horizontal: AppTokens.s8, vertical: 1),
        padding: const EdgeInsets.symmetric(
            horizontal: AppTokens.s8, vertical: 3),
        decoration: BoxDecoration(
          color: active ? c.accent.withValues(alpha: 0.10) : null,
          borderRadius: BorderRadius.circular(AppTokens.rMd),
        ),
        child: Row(
          children: [
            r.isActive
                ? SizedBox(
                    width: 14,
                    height: 14,
                    child: CircularProgressIndicator(
                        strokeWidth: 2, color: AppTokens.brand),
                  )
                : Icon(Icons.account_tree_outlined,
                    size: 14, color: _runStatusColor(r.status, c)),
            const SizedBox(width: AppTokens.s8),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(r.title,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(
                          color: active ? c.accent : c.textPrimary,
                          fontSize: 12.5)),
                  Text(
                    context.trArgs('{status} · {done}/{total} steps', {
                      'status': context.tr(r.status),
                      'done':
                          r.steps.where((s) => s.status == 'done').length,
                      'total': r.steps.length,
                    }),
                    style: TextStyle(color: c.textMuted, fontSize: 10.5),
                  ),
                ],
              ),
            ),
            PopupMenuButton<String>(
              tooltip: '',
              padding: EdgeInsets.zero,
              iconSize: 15,
              // Kill the 48x48 default tap target so the row stays compact.
              constraints: const BoxConstraints(),
              style: const ButtonStyle(
                minimumSize: WidgetStatePropertyAll(Size(24, 24)),
                fixedSize: WidgetStatePropertyAll(Size(24, 24)),
                padding: WidgetStatePropertyAll(EdgeInsets.zero),
                tapTargetSize: MaterialTapTargetSize.shrinkWrap,
              ),
              icon: Icon(Icons.more_vert_rounded,
                  size: 15, color: c.textMuted),
              onSelected: (v) async {
                switch (v) {
                  case 'rename':
                    await showRenameRunDialog(context, ref, r);
                  case 'cancel':
                    try {
                      await cancelWorkflowRun(ref, r.id);
                    } catch (_) {}
                  case 'delete':
                    final gone = await showDeleteRunDialog(context, ref, r);
                    if (gone &&
                        ref.read(selectedJidProvider) == wfRunJid(r.id)) {
                      ref.read(selectedJidProvider.notifier).state = null;
                    }
                }
              },
              itemBuilder: (ctx) => [
                PopupMenuItem(value: 'rename', child: Text(ctx.tr('Rename'))),
                if (r.isActive)
                  PopupMenuItem(
                      value: 'cancel', child: Text(ctx.tr('Cancel run')))
                else
                  PopupMenuItem(value: 'delete', child: Text(ctx.tr('Delete'))),
              ],
            ),
          ],
        ),
      ),
    );
  }
}

// ─── Conversation-area pane ──────────────────────────────────────────────────

/// Read-only "workflow session" view shown in place of the chat conversation
/// pane: header with status/actions + live step-by-step flow activity.
class WorkflowSessionPane extends ConsumerStatefulWidget {
  const WorkflowSessionPane({super.key, required this.runId});
  final String runId;

  @override
  ConsumerState<WorkflowSessionPane> createState() =>
      _WorkflowSessionPaneState();
}

class _WorkflowSessionPaneState extends ConsumerState<WorkflowSessionPane> {
  Timer? _poll;
  List<Map<String, dynamic>> _activity = const [];
  final _feedScroll = ScrollController();
  /// Expanded entry indexes — everything starts collapsed (chat-box style).
  final Set<int> _expandedActivity = {};

  /// Whole activity panel: default collapsed to a slim strip with the count.
  bool _feedOpen = false;

  Future<void> _loadActivity() async {
    try {
      final entries = await fetchRunActivity(ref, widget.runId);
      if (!mounted) return;
      final grew = entries.length != _activity.length;
      setState(() => _activity = entries);
      // Follow the tail while new entries stream in.
      if (grew && _feedScroll.hasClients) {
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (_feedScroll.hasClients) {
            _feedScroll.jumpTo(_feedScroll.position.maxScrollExtent);
          }
        });
      }
    } catch (_) {/* transient */}
  }

  @override
  void initState() {
    super.initState();
    _loadActivity();
    _poll = Timer.periodic(const Duration(seconds: 3), (_) {
      final runs = ref.read(workflowRunsProvider).valueOrNull;
      final run = runs?.where((r) => r.id == widget.runId).firstOrNull;
      // Keep polling while live (or unknown); stop churning once terminal.
      if (run == null || run.isActive) {
        ref.invalidate(workflowRunsProvider);
        _loadActivity();
      }
    });
  }

  @override
  void dispose() {
    _poll?.cancel();
    _feedScroll.dispose();
    super.dispose();
  }

  void _snack(String msg) {
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(msg)));
  }

  /// Collapsed activity panel: slim vertical strip — chevron, live spinner
  /// (or tool icon), and the action count. Click anywhere to expand.
  Widget _collapsedFeedStrip(AppColors c, bool active) {
    return InkWell(
      onTap: () => setState(() => _feedOpen = true),
      child: Tooltip(
        message: context.trArgs('Activity ({n}) — click to expand',
            {'n': _activity.length}),
        child: Column(
          children: [
            const SizedBox(height: AppTokens.s12),
            Icon(Icons.chevron_right_rounded, size: 16, color: c.textMuted),
            const SizedBox(height: AppTokens.s8),
            if (active)
              SizedBox(
                width: 12,
                height: 12,
                child: CircularProgressIndicator(
                    strokeWidth: 1.6, color: AppTokens.brand),
              )
            else
              Icon(Icons.build_outlined, size: 13, color: c.textMuted),
            const SizedBox(height: AppTokens.s8),
            Text('${_activity.length}',
                style: TextStyle(
                    color: c.textMuted,
                    fontSize: 11,
                    fontWeight: FontWeight.w700)),
          ],
        ),
      ),
    );
  }

  Widget _activityFeed(AppColors c, bool active) {
    IconData icon(String k) => switch (k) {
          'think' => Icons.psychology_outlined,
          'tool' => Icons.build_outlined,
          'tool_error' => Icons.warning_amber_rounded,
          'status' => Icons.sync_rounded,
          _ => Icons.chat_bubble_outline_rounded,
        };
    Color color(String k) => switch (k) {
          'tool' => AppTokens.brand,
          'tool_error' => AppTokens.danger,
          'status' => AppTokens.warning,
          _ => c.textMuted,
        };
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        InkWell(
          onTap: () => setState(() => _feedOpen = false),
          child: Padding(
            padding: const EdgeInsets.fromLTRB(
                AppTokens.s12, AppTokens.s8, AppTokens.s12, AppTokens.s4),
            child: Row(children: [
              Icon(Icons.expand_more_rounded, size: 14, color: c.textMuted),
              const SizedBox(width: AppTokens.s4),
              if (active)
                SizedBox(
                  width: 12,
                  height: 12,
                  child: CircularProgressIndicator(
                      strokeWidth: 1.6, color: AppTokens.brand),
                ),
              if (active) const SizedBox(width: AppTokens.s6),
              Text(context.tr('ACTIVITY'),
                  style: TextStyle(
                      color: c.textMuted,
                      fontSize: 10,
                      fontWeight: FontWeight.w700,
                      letterSpacing: 1.1)),
              const SizedBox(width: AppTokens.s4),
              Text('(${_activity.length})',
                  style: TextStyle(color: c.textMuted, fontSize: 10)),
            ]),
          ),
        ),
        Expanded(
          child: _activity.isEmpty
              ? Center(
                  child: Text(
                      active
                          ? context.tr('Waiting for the agent…')
                          : context.tr('No activity recorded'),
                      style: TextStyle(color: c.textMuted, fontSize: 11)))
              : ListView.builder(
                  controller: _feedScroll,
                  padding: const EdgeInsets.fromLTRB(
                      AppTokens.s8, 0, AppTokens.s8, AppTokens.s8),
                  itemCount: _activity.length,
                  itemBuilder: (ctx, i) {
                    final e = _activity[i];
                    final kind = '${e['kind'] ?? 'text'}';
                    final text = '${e['text'] ?? ''}';
                    final isToolish = kind == 'tool' || kind == 'tool_error';
                    // Status lines stay plain — everything else collapses to
                    // a one-line header (chat-box style, closed by default).
                    if (kind == 'status') {
                      return Padding(
                        padding: const EdgeInsets.symmetric(vertical: 3),
                        child: Row(children: [
                          Icon(icon(kind), size: 12, color: color(kind)),
                          const SizedBox(width: 4),
                          Expanded(
                            child: Text(text,
                                style: TextStyle(
                                    color: AppTokens.warning, fontSize: 11)),
                          ),
                        ]),
                      );
                    }
                    final firstLine = text.split('\n').first;
                    final title = switch (kind) {
                      'tool' || 'tool_error' => firstLine.contains(' — ')
                          ? firstLine.substring(0, firstLine.indexOf(' — '))
                          : (firstLine.length > 48
                              ? '${firstLine.substring(0, 48)}…'
                              : firstLine),
                      'think' => ctx.trArgs(
                          'Thinking… ({n} chars)', {'n': text.length}),
                      'text' => ctx.trArgs(
                          'Writing… ({n} chars)', {'n': text.length}),
                      _ => firstLine.length > 48
                          ? '${firstLine.substring(0, 48)}…'
                          : firstLine,
                    };
                    final open = _expandedActivity.contains(i);
                    return Container(
                      margin: const EdgeInsets.only(bottom: AppTokens.s4),
                      decoration: BoxDecoration(
                        color: c.surface,
                        borderRadius: BorderRadius.circular(AppTokens.rMd),
                        border: Border.all(color: c.border),
                      ),
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.stretch,
                        children: [
                          InkWell(
                            onTap: () => setState(() {
                              open
                                  ? _expandedActivity.remove(i)
                                  : _expandedActivity.add(i);
                            }),
                            child: Padding(
                              padding: const EdgeInsets.symmetric(
                                  horizontal: 6, vertical: 4),
                              child: Row(children: [
                                Icon(
                                    open
                                        ? Icons.expand_more_rounded
                                        : Icons.chevron_right_rounded,
                                    size: 13,
                                    color: c.textMuted),
                                Icon(icon(kind),
                                    size: 12, color: color(kind)),
                                const SizedBox(width: 4),
                                Expanded(
                                  child: Text(title,
                                      overflow: TextOverflow.ellipsis,
                                      style: TextStyle(
                                        color: kind == 'think'
                                            ? c.textMuted
                                            : c.textPrimary,
                                        fontSize: 11,
                                        fontStyle: kind == 'think'
                                            ? FontStyle.italic
                                            : null,
                                      )),
                                ),
                                const SizedBox(width: 4),
                                Text('${e['stepId'] ?? ''}',
                                    style: TextStyle(
                                        color: c.textMuted, fontSize: 9)),
                              ]),
                            ),
                          ),
                          if (open)
                            Container(
                              width: double.infinity,
                              constraints:
                                  const BoxConstraints(maxHeight: 280),
                              padding: const EdgeInsets.fromLTRB(
                                  AppTokens.s8, AppTokens.s4,
                                  AppTokens.s8, AppTokens.s6),
                              decoration: BoxDecoration(
                                color: c.surfaceAlt,
                                border: Border(
                                    top: BorderSide(color: c.border)),
                              ),
                              child: SingleChildScrollView(
                                child: SelectableText(
                                  text,
                                  style: TextStyle(
                                    color: kind == 'think'
                                        ? c.textMuted
                                        : c.textSecondary,
                                    fontSize: 11,
                                    fontFamily:
                                        isToolish ? 'monospace' : null,
                                    fontStyle: kind == 'think'
                                        ? FontStyle.italic
                                        : null,
                                  ),
                                ),
                              ),
                            ),
                        ],
                      ),
                    );
                  },
                ),
        ),
      ],
    );
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final runs = ref.watch(workflowRunsProvider).valueOrNull ??
        const <WorkflowRun>[];
    final run = runs.where((r) => r.id == widget.runId).firstOrNull;

    if (run == null) {
      return Center(
        child: Text(
            context.trArgs('Run "{id}" not found (history may have rotated)',
                {'id': widget.runId}),
            style: TextStyle(color: c.textSecondary, fontSize: 13)),
      );
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        // Header — mirrors the chat pane header but flow-flavored.
        Container(
          padding: const EdgeInsets.symmetric(
              horizontal: AppTokens.s16, vertical: AppTokens.s8),
          decoration: BoxDecoration(
            border: Border(bottom: BorderSide(color: c.border)),
          ),
          child: Row(
            children: [
              Icon(Icons.account_tree_outlined, size: 18, color: c.accent),
              const SizedBox(width: AppTokens.s8),
              Expanded(
                child: Text(run.title,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                        color: c.textPrimary,
                        fontSize: 14,
                        fontWeight: FontWeight.w600)),
              ),
              Text(context.tr('workflow session'),
                  style: TextStyle(color: c.textMuted, fontSize: 11)),
              const SizedBox(width: AppTokens.s8),
              IconButton(
                tooltip: context.tr('Open in run monitor'),
                icon: const Icon(Icons.open_in_new_rounded, size: 16),
                onPressed: () {
                  ref.read(openWorkflowRunProvider.notifier).state = run.id;
                  context.go('/workflow-runs');
                },
              ),
            ],
          ),
        ),
        // Body — left: live agent activity (think / tool calls / messages);
        // right: the shared read-only flow view.
        Expanded(
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              AnimatedContainer(
                duration: const Duration(milliseconds: 150),
                width: _feedOpen ? 300 : 40,
                decoration: BoxDecoration(
                  color: c.sidebar,
                  border: Border(right: BorderSide(color: c.border)),
                ),
                // Clip so mid-animation widths never overflow the row content.
                child: ClipRect(
                  child: _feedOpen
                      ? _activityFeed(c, run.isActive)
                      : _collapsedFeedStrip(c, run.isActive),
                ),
              ),
              Expanded(
                child: WorkflowRunDetail(
                  run: run,
            onCancel: () async {
              try {
                await cancelWorkflowRun(ref, run.id);
                _snack(L10n.global
                    .tArgs('Cancel requested: {id}', {'id': run.id}));
              } catch (e) {
                _snack(L10n.global.tArgs('Cancel failed: {e}', {'e': e}));
              }
            },
            onRerun: () {
              final defs = ref.read(workflowsProvider).valueOrNull ?? [];
              WorkflowDefSummary? def;
              for (final d in defs) {
                if (d.name == run.workflowName) {
                  def = d;
                  break;
                }
              }
              if (def == null) {
                _snack(context.trArgs('Definition "{name}" no longer exists',
                    {'name': run.workflowName}));
                return;
              }
              showWorkflowRunDialog(context, ref, def, preset: run.inputs,
                  onStarted: (id) {
                ref.read(selectedJidProvider.notifier).state = wfRunJid(id);
                ref.invalidate(workflowRunsProvider);
              });
            },
                  onDeleted: () =>
                      ref.read(selectedJidProvider.notifier).state = null,
                ),
              ),
            ],
          ),
        ),
      ],
    );
  }
}
