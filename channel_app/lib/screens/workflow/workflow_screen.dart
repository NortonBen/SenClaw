import 'dart:async';

import 'package:flutter/material.dart';

import '../../models/workflow_models.dart';
import '../../services/workflow_api.dart';
import '../../theme/tokens.dart';
import '../../widgets/markdown_text.dart';

Color _runColor(String status, AppColors c) => switch (status) {
      'running' => AppTokens.brand,
      'done' => AppTokens.success,
      'partial-failed' => AppTokens.warning,
      'interrupted' => AppTokens.danger,
      _ => c.textSecondary,
    };

Color _stepColor(String status, AppColors c) => switch (status) {
      'running' => AppTokens.brand,
      'done' => AppTokens.success,
      'failed' => AppTokens.danger,
      _ => c.textSecondary,
    };

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

String _fmtTime(String? iso) =>
    (iso == null || iso.isEmpty) ? '—' : iso.replaceFirst('T', ' ').split('.').first;

/// Run-inputs sheet → starts the run and returns the run id (null = cancel).
Future<String?> _runSheet(BuildContext context, WorkflowDefSummary def,
    {Map<String, String>? preset}) async {
  final api = WorkflowApi();
  final ctrls = {
    for (final i in def.inputs)
      i.name: TextEditingController(text: preset?[i.name] ?? i.defaultValue ?? ''),
  };
  return showModalBottomSheet<String>(
    context: context,
    isScrollControlled: true,
    builder: (ctx) => Padding(
      padding: EdgeInsets.only(
          left: 16, right: 16, top: 16,
          bottom: MediaQuery.of(ctx).viewInsets.bottom + 16),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text('Run: ${def.name}',
              style: const TextStyle(fontSize: 15, fontWeight: FontWeight.w700)),
          const SizedBox(height: 12),
          for (final i in def.inputs) ...[
            TextField(
              controller: ctrls[i.name],
              decoration: InputDecoration(
                labelText: i.required ? '${i.name} *' : i.name,
                helperText: i.description,
                border: const OutlineInputBorder(),
                isDense: true,
              ),
            ),
            const SizedBox(height: 10),
          ],
          FilledButton.icon(
            icon: const Icon(Icons.play_arrow_rounded),
            label: const Text('Run workflow'),
            onPressed: () async {
              final missing = def.inputs
                  .where((i) => i.required && ctrls[i.name]!.text.trim().isEmpty)
                  .map((i) => i.name);
              if (missing.isNotEmpty) {
                ScaffoldMessenger.of(ctx).showSnackBar(SnackBar(
                    content: Text('Missing: ${missing.join(', ')}')));
                return;
              }
              try {
                final id = await api.startRun(def.name, {
                  for (final e in ctrls.entries)
                    if (e.value.text.trim().isNotEmpty) e.key: e.value.text,
                });
                if (ctx.mounted) Navigator.pop(ctx, id);
              } catch (e) {
                if (ctx.mounted) {
                  ScaffoldMessenger.of(ctx)
                      .showSnackBar(SnackBar(content: Text('Run failed: $e')));
                }
              }
            },
          ),
        ],
      ),
    ),
  );
}

/// Workflow hub: Runs (history + live) and Templates (run with inputs).
class WorkflowScreen extends StatefulWidget {
  const WorkflowScreen({super.key});
  @override
  State<WorkflowScreen> createState() => _WorkflowScreenState();
}

class _WorkflowScreenState extends State<WorkflowScreen> {
  final _api = WorkflowApi();
  List<WorkflowRun> _runs = const [];
  List<WorkflowDefSummary> _defs = const [];
  bool _loading = true;
  Timer? _poll;

  Future<void> _refresh() async {
    try {
      final runs = await _api.listRuns();
      final defs = await _api.listDefs();
      if (mounted) setState(() { _runs = runs; _defs = defs; _loading = false; });
    } catch (_) {
      if (mounted) setState(() => _loading = false);
    }
  }

  /// Instant paint from the local cache while the relay answers.
  Future<void> _paintFromCache() async {
    final runs = await _api.listRunsCached();
    final defs = await _api.listDefsCached();
    if (!mounted || !_loading) return;
    if (runs.isNotEmpty || defs.isNotEmpty) {
      setState(() {
        _runs = runs;
        _defs = defs;
        _loading = false;
      });
    }
  }

  @override
  void initState() {
    super.initState();
    _paintFromCache();
    _refresh();
    _poll = Timer.periodic(const Duration(seconds: 5), (_) {
      if (_runs.any((r) => r.isActive)) _refresh();
    });
  }

  @override
  void dispose() {
    _poll?.cancel();
    super.dispose();
  }

  void _openDetail(String runId) {
    Navigator.of(context)
        .push(MaterialPageRoute(
            builder: (_) => WorkflowRunDetailScreen(runId: runId)))
        .then((_) => _refresh());
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return DefaultTabController(
      length: 2,
      child: Scaffold(
        backgroundColor: c.bg,
        appBar: AppBar(
          backgroundColor: c.surface,
          elevation: 0,
          title: Text('Workflow', style: TextStyle(color: c.textPrimary)),
          iconTheme: IconThemeData(color: c.textPrimary),
          bottom: TabBar(tabs: [
            Tab(text: 'Runs (${_runs.length})'),
            Tab(text: 'Templates (${_defs.length})'),
          ]),
        ),
        body: _loading
            ? const Center(child: CircularProgressIndicator())
            : TabBarView(children: [_runsTab(c), _defsTab(c)]),
      ),
    );
  }

  Widget _runsTab(AppColors c) => RefreshIndicator(
        onRefresh: _refresh,
        child: _runs.isEmpty
            ? ListView(children: [
                const SizedBox(height: 80),
                Center(child: Text('No runs yet',
                    style: TextStyle(color: c.textMuted))),
              ])
            : ListView.builder(
                itemCount: _runs.length,
                itemBuilder: (ctx, i) {
                  final r = _runs[i];
                  final done =
                      r.steps.where((s) => s.status == 'done').length;
                  return ListTile(
                    onTap: () => _openDetail(r.id),
                    leading: r.isActive
                        ? const SizedBox(width: 18, height: 18,
                            child: CircularProgressIndicator(strokeWidth: 2))
                        : Icon(Icons.account_tree_outlined,
                            color: _runColor(r.status, c), size: 20),
                    title: Text(r.title,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: TextStyle(color: c.textPrimary, fontSize: 14)),
                    subtitle: Text(
                        '${r.status} · $done/${r.steps.length} steps · ${_fmtTime(r.createdAt)}',
                        style: TextStyle(color: c.textMuted, fontSize: 11)),
                    trailing: PopupMenuButton<String>(
                      onSelected: (v) async {
                        try {
                          switch (v) {
                            case 'cancel':
                              await _api.cancelRun(r.id);
                            case 'rename':
                              final ctrl =
                                  TextEditingController(text: r.label ?? '');
                              final ok = await showDialog<bool>(
                                context: context,
                                builder: (dctx) => AlertDialog(
                                  title: const Text('Rename run'),
                                  content: TextField(
                                      controller: ctrl,
                                      decoration:
                                          InputDecoration(hintText: r.id)),
                                  actions: [
                                    TextButton(
                                        onPressed: () =>
                                            Navigator.pop(dctx, false),
                                        child: const Text('Cancel')),
                                    FilledButton(
                                        onPressed: () =>
                                            Navigator.pop(dctx, true),
                                        child: const Text('Save')),
                                  ],
                                ),
                              );
                              if (ok == true) {
                                await _api.renameRun(r.id, ctrl.text);
                              }
                            case 'delete':
                              await _api.deleteRun(r.id);
                          }
                        } catch (e) {
                          if (mounted) {
                            ScaffoldMessenger.of(context).showSnackBar(
                                SnackBar(content: Text('$e')));
                          }
                        }
                        _refresh();
                      },
                      itemBuilder: (_) => [
                        const PopupMenuItem(
                            value: 'rename', child: Text('Rename')),
                        if (r.isActive)
                          const PopupMenuItem(
                              value: 'cancel', child: Text('Cancel run'))
                        else
                          const PopupMenuItem(
                              value: 'delete', child: Text('Delete')),
                      ],
                    ),
                  );
                },
              ),
      );

  Widget _defsTab(AppColors c) => RefreshIndicator(
        onRefresh: _refresh,
        child: _defs.isEmpty
            ? ListView(children: [
                const SizedBox(height: 80),
                Center(child: Text('No workflows defined',
                    style: TextStyle(color: c.textMuted))),
              ])
            : ListView.builder(
                itemCount: _defs.length,
                itemBuilder: (ctx, i) {
                  final d = _defs[i];
                  return ListTile(
                    leading: Icon(Icons.account_tree_outlined,
                        color: c.accent, size: 20),
                    title: Text(d.name,
                        style: TextStyle(color: c.textPrimary, fontSize: 14)),
                    subtitle: Text(
                        '${d.stepCount} steps${(d.description ?? '').isNotEmpty ? ' — ${d.description}' : ''}',
                        maxLines: 2,
                        overflow: TextOverflow.ellipsis,
                        style: TextStyle(color: c.textMuted, fontSize: 11)),
                    trailing: IconButton(
                      icon: Icon(Icons.play_arrow_rounded,
                          color: AppTokens.success),
                      onPressed: () async {
                        final id = await _runSheet(context, d);
                        if (id != null && id.isNotEmpty && mounted) {
                          _openDetail(id);
                        }
                      },
                    ),
                  );
                },
              ),
      );
}

/// Run detail: status header, live activity feed (collapsed entries), and
/// step cards with markdown results.
class WorkflowRunDetailScreen extends StatefulWidget {
  const WorkflowRunDetailScreen({super.key, required this.runId});
  final String runId;

  @override
  State<WorkflowRunDetailScreen> createState() =>
      _WorkflowRunDetailScreenState();
}

class _WorkflowRunDetailScreenState extends State<WorkflowRunDetailScreen> {
  final _api = WorkflowApi();
  WorkflowRun? _run;
  List<Map<String, dynamic>> _activity = const [];
  final Set<int> _openActivity = {};

  /// Whole ACTIVITY section: collapsed by default, header shows the count.
  bool _activityOpen = false;
  final Set<String> _openSteps = {};
  Timer? _poll;

  Future<void> _refresh() async {
    try {
      final runs = await _api.listRuns();
      final acts = await _api.runActivity(widget.runId);
      if (!mounted) return;
      setState(() {
        _run = runs.where((r) => r.id == widget.runId).firstOrNull;
        _activity = acts;
      });
    } catch (_) {}
  }

  @override
  void initState() {
    super.initState();
    _refresh();
    _poll = Timer.periodic(const Duration(seconds: 3), (_) {
      if (_run == null || _run!.isActive) _refresh();
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
    final r = _run;
    return Scaffold(
      backgroundColor: c.bg,
      appBar: AppBar(
        backgroundColor: c.surface,
        elevation: 0,
        iconTheme: IconThemeData(color: c.textPrimary),
        title: Text(r?.title ?? widget.runId,
            style: TextStyle(color: c.textPrimary, fontSize: 15)),
        actions: [
          if (r != null && r.isActive)
            IconButton(
              tooltip: 'Cancel run',
              icon: Icon(Icons.stop_circle_outlined, color: AppTokens.danger),
              onPressed: () async {
                try { await _api.cancelRun(r.id); } catch (_) {}
                _refresh();
              },
            ),
        ],
      ),
      body: r == null
          ? const Center(child: CircularProgressIndicator())
          : ListView(
              padding: const EdgeInsets.all(12),
              children: [
                Row(children: [
                  _chip(c, r.status, color: _runColor(r.status, c)),
                  const SizedBox(width: 8),
                  Expanded(
                    child: Text(r.workflowName,
                        overflow: TextOverflow.ellipsis,
                        style:
                            TextStyle(color: c.textSecondary, fontSize: 12)),
                  ),
                  Text(_fmtTime(r.createdAt),
                      style: TextStyle(color: c.textMuted, fontSize: 11)),
                ]),
                if (r.inputs.isNotEmpty) ...[
                  const SizedBox(height: 8),
                  Wrap(spacing: 4, runSpacing: 4, children: [
                    for (final e in r.inputs.entries)
                      _chip(c, '${e.key}=${e.value}'),
                  ]),
                ],
                const SizedBox(height: 14),

                // ── Activity: whole section collapsible, closed by default —
                //    header always shows the action count. ──
                if (_activity.isNotEmpty) ...[
                  InkWell(
                    onTap: () =>
                        setState(() => _activityOpen = !_activityOpen),
                    child: Padding(
                      padding: const EdgeInsets.symmetric(vertical: 2),
                      child: Row(children: [
                        Icon(
                            _activityOpen
                                ? Icons.expand_more_rounded
                                : Icons.chevron_right_rounded,
                            size: 14,
                            color: c.textMuted),
                        const SizedBox(width: 2),
                        Text('ACTIVITY (${_activity.length})',
                            style: TextStyle(
                                color: c.textMuted,
                                fontSize: 10,
                                fontWeight: FontWeight.w700,
                                letterSpacing: 1.1)),
                      ]),
                    ),
                  ),
                  const SizedBox(height: 6),
                  if (_activityOpen)
                    for (var i = 0; i < _activity.length; i++)
                      _activityTile(c, i, _activity[i]),
                  const SizedBox(height: 14),
                ],

                Text('STEPS (${r.steps.length})',
                    style: TextStyle(
                        color: c.textMuted,
                        fontSize: 10,
                        fontWeight: FontWeight.w700,
                        letterSpacing: 1.1)),
                const SizedBox(height: 6),
                for (final s in r.steps) _stepCard(c, s),
              ],
            ),
    );
  }

  Widget _activityTile(AppColors c, int i, Map<String, dynamic> e) {
    final kind = '${e['kind'] ?? 'text'}';
    final text = '${e['text'] ?? ''}';
    if (kind == 'status') {
      return Padding(
        padding: const EdgeInsets.symmetric(vertical: 2),
        child: Text('↻ $text',
            style: TextStyle(color: AppTokens.warning, fontSize: 11)),
      );
    }
    final first = text.split('\n').first;
    final title = switch (kind) {
      'tool' || 'tool_error' =>
        first.contains(' — ') ? first.substring(0, first.indexOf(' — ')) : first,
      'think' => 'Thinking… (${text.length} chars)',
      _ => first,
    };
    final open = _openActivity.contains(i);
    final icon = switch (kind) {
      'think' => Icons.psychology_outlined,
      'tool' => Icons.build_outlined,
      'tool_error' => Icons.warning_amber_rounded,
      _ => Icons.chat_bubble_outline_rounded,
    };
    return Container(
      margin: const EdgeInsets.only(bottom: 4),
      decoration: BoxDecoration(
        color: c.surface,
        borderRadius: BorderRadius.circular(AppTokens.rMd),
        border: Border.all(color: c.border),
      ),
      child: Column(children: [
        InkWell(
          onTap: () => setState(() =>
              open ? _openActivity.remove(i) : _openActivity.add(i)),
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 6),
            child: Row(children: [
              Icon(open ? Icons.expand_more : Icons.chevron_right,
                  size: 14, color: c.textMuted),
              Icon(icon, size: 13,
                  color: kind == 'tool_error'
                      ? AppTokens.danger
                      : kind == 'tool'
                          ? AppTokens.brand
                          : c.textMuted),
              const SizedBox(width: 6),
              Expanded(
                child: Text(title,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                        color: kind == 'think' ? c.textMuted : c.textPrimary,
                        fontSize: 11,
                        fontStyle:
                            kind == 'think' ? FontStyle.italic : null)),
              ),
              Text('${e['stepId'] ?? ''}',
                  style: TextStyle(color: c.textMuted, fontSize: 9)),
            ]),
          ),
        ),
        if (open)
          Container(
            width: double.infinity,
            constraints: const BoxConstraints(maxHeight: 240),
            padding: const EdgeInsets.all(8),
            decoration: BoxDecoration(
              color: c.surfaceAlt,
              border: Border(top: BorderSide(color: c.border)),
            ),
            child: SingleChildScrollView(
              child: SelectableText(text,
                  style: TextStyle(color: c.textSecondary, fontSize: 11)),
            ),
          ),
      ]),
    );
  }

  Widget _stepCard(AppColors c, WorkflowStepRun s) {
    // `_openSteps` holds COLLAPSED step ids — cards default to expanded.
    final open = !_openSteps.contains(s.id);
    void toggle() => setState(() {
          open ? _openSteps.add(s.id) : _openSteps.remove(s.id);
        });
    return Container(
      margin: const EdgeInsets.only(bottom: 8),
      padding: const EdgeInsets.all(10),
      decoration: BoxDecoration(
        color: c.surface,
        borderRadius: BorderRadius.circular(AppTokens.rLg),
        border: Border.all(color: c.border),
      ),
      child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
        InkWell(
          onTap: toggle,
          child: Row(children: [
            Icon(open ? Icons.expand_more : Icons.chevron_right,
                size: 15, color: c.textMuted),
            _chip(c, s.status, color: _stepColor(s.status, c)),
            const SizedBox(width: 6),
            Expanded(
              child: Text(s.id,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                      color: c.textPrimary,
                      fontWeight: FontWeight.w600,
                      fontSize: 13)),
            ),
            Text(s.kind, style: TextStyle(color: c.textMuted, fontSize: 10)),
          ]),
        ),
        if (open) ...[
          if (s.error != null)
            Padding(
              padding: const EdgeInsets.only(top: 6),
              child: Text(s.error!,
                  style: TextStyle(color: AppTokens.danger, fontSize: 12)),
            ),
          if ((s.observeContent ?? '').isNotEmpty)
            Container(
              width: double.infinity,
              margin: const EdgeInsets.only(top: 8),
              padding: const EdgeInsets.all(8),
              decoration: BoxDecoration(
                color: c.surfaceAlt,
                borderRadius: BorderRadius.circular(AppTokens.rMd),
                border: Border(left: BorderSide(color: c.accent, width: 3)),
              ),
              child: MarkdownText(s.observeContent!, fontSize: 12),
            ),
          if (s.result.isNotEmpty)
            Container(
              width: double.infinity,
              margin: const EdgeInsets.only(top: 8),
              padding: const EdgeInsets.all(8),
              decoration: BoxDecoration(
                color: c.surfaceAlt,
                borderRadius: BorderRadius.circular(AppTokens.rMd),
              ),
              child: MarkdownText(s.result, fontSize: 12),
            ),
        ],
      ]),
    );
  }
}
