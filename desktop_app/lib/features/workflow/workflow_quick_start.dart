import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/i18n/l10n.dart';
import '../../models/workflow_models.dart';
import '../../theme/tokens.dart';
import '../chat/agents_provider.dart' show selectedJidProvider;
import 'workflow_providers.dart';
import 'workflow_session_pane.dart' show wfRunJid;

/// New-session Workflow tab: pick a saved workflow (fill inputs → run), or
/// describe a new routine and let a one-shot agent author + save it — the
/// fresh workflow is auto-selected, ready to run.
class WorkflowQuickStart extends ConsumerStatefulWidget {
  const WorkflowQuickStart({super.key});
  @override
  ConsumerState<WorkflowQuickStart> createState() => _WorkflowQuickStartState();
}

class _WorkflowQuickStartState extends ConsumerState<WorkflowQuickStart> {
  String? _selectedName;
  final Map<String, TextEditingController> _inputCtrls = {};
  final _desc = TextEditingController();
  bool _starting = false;
  bool _drafting = false;

  @override
  void dispose() {
    _desc.dispose();
    for (final c in _inputCtrls.values) {
      c.dispose();
    }
    super.dispose();
  }

  void _snack(String msg) {
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(msg)));
  }

  void _pick(String name, List<WorkflowDefSummary> defs) {
    WorkflowDefSummary? d;
    for (final x in defs) {
      if (x.name == name) {
        d = x;
        break;
      }
    }
    setState(() {
      _selectedName = name;
      for (final c in _inputCtrls.values) {
        c.dispose();
      }
      _inputCtrls.clear();
      for (final i in d?.inputs ?? const <WorkflowInputDef>[]) {
        _inputCtrls[i.name] =
            TextEditingController(text: i.defaultValue ?? '');
      }
    });
  }

  Future<void> _run(WorkflowDefSummary def) async {
    final missing = def.inputs
        .where((i) => i.required && (_inputCtrls[i.name]?.text.trim() ?? '').isEmpty)
        .map((i) => i.name)
        .toList();
    if (missing.isNotEmpty) {
      _snack(L10n.global.tArgs(
          'Missing required input(s): {names}', {'names': missing.join(', ')}));
      return;
    }
    setState(() => _starting = true);
    try {
      final inputs = <String, String>{
        for (final e in _inputCtrls.entries)
          if (e.value.text.trim().isNotEmpty) e.key: e.value.text,
      };
      final runId = await startWorkflowRun(ref, def.name, inputs);
      _snack(L10n.global.tArgs('Started: {id}', {'id': runId}));
      // Stay in Chat: the run appears as a "workflow session" whose pane
      // shows the live flow activity (selecting a jid dismisses New Session).
      ref.read(selectedJidProvider.notifier).state = wfRunJid(runId);
    } catch (e) {
      _snack(L10n.global.tArgs('Run failed: {e}', {'e': e}));
    } finally {
      if (mounted) setState(() => _starting = false);
    }
  }

  Future<void> _createWithAi() async {
    if (_desc.text.trim().isEmpty) {
      _snack(L10n.global.t('Describe the routine first'));
      return;
    }
    setState(() => _drafting = true);
    String content;
    try {
      // Agent authors a validated draft — nothing is saved yet.
      (_, content) = await draftWorkflow(ref, _desc.text);
    } catch (e) {
      _snack(L10n.global.tArgs('Draft failed: {e}', {'e': e}));
      if (mounted) setState(() => _drafting = false);
      return;
    }
    if (mounted) setState(() => _drafting = false);
    if (!mounted) return;
    await _reviewAndSave(content);
  }

  /// Review dialog: the user can edit the agent's draft, Save it (validated
  /// server-side), or Cancel to discard. On validation errors the editor
  /// stays open so the fix isn't lost.
  Future<void> _reviewAndSave(String content) async {
    final ctrl = TextEditingController(text: content);
    var busy = false;
    final c = context.colors;
    await showDialog<void>(
      context: context,
      barrierDismissible: false,
      builder: (ctx) => StatefulBuilder(
        builder: (ctx, setDlg) => AlertDialog(
          title: Text(ctx.tr('Review draft — edit if needed, then Save')),
          content: SizedBox(
            width: 640,
            height: 440,
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  ctx.tr(
                      'The content is validated (DAG, personas, cycles…) on save. '
                      'Cancel discards the draft.'),
                  style: TextStyle(color: c.textSecondary, fontSize: 12),
                ),
                const SizedBox(height: AppTokens.s8),
                Expanded(
                  child: TextField(
                    controller: ctrl,
                    maxLines: null,
                    expands: true,
                    enabled: !busy,
                    textAlignVertical: TextAlignVertical.top,
                    style: const TextStyle(
                        fontFamily: 'monospace', fontSize: 12),
                    decoration:
                        const InputDecoration(border: OutlineInputBorder()),
                  ),
                ),
              ],
            ),
          ),
          actions: [
            TextButton(
              onPressed: busy ? null : () => Navigator.pop(ctx),
              child: Text(ctx.tr('Cancel')),
            ),
            FilledButton(
              onPressed: busy
                  ? null
                  : () async {
                      setDlg(() => busy = true);
                      try {
                        final created =
                            await createWorkflow(ref, ctrl.text);
                        final defs =
                            await ref.refresh(workflowsProvider.future);
                        _pick(created, defs);
                        _desc.clear();
                        _snack(L10n.global.tArgs(
                            'Saved "{name}" — fill the inputs and press Run',
                            {'name': created}));
                        if (ctx.mounted) Navigator.pop(ctx);
                      } catch (e) {
                        setDlg(() => busy = false);
                        final msg = '$e';
                        if (msg.contains('already exists') && ctx.mounted) {
                          final overwrite = await showDialog<bool>(
                            context: ctx,
                            builder: (ctx2) => AlertDialog(
                              title: Text(ctx2.tr('Workflow already exists')),
                              content: Text(ctx2
                                  .tr('Overwrite the existing definition?')),
                              actions: [
                                TextButton(
                                    onPressed: () =>
                                        Navigator.pop(ctx2, false),
                                    child: Text(ctx2.tr('Cancel'))),
                                FilledButton(
                                    onPressed: () =>
                                        Navigator.pop(ctx2, true),
                                    child: Text(ctx2.tr('Overwrite'))),
                              ],
                            ),
                          );
                          if (overwrite == true) {
                            try {
                              final created = await createWorkflow(
                                  ref, ctrl.text,
                                  overwrite: true);
                              final defs = await ref
                                  .refresh(workflowsProvider.future);
                              _pick(created, defs);
                              _desc.clear();
                              _snack(L10n.global.tArgs(
                                  'Saved "{name}" — fill the inputs and press Run',
                                  {'name': created}));
                              if (ctx.mounted) Navigator.pop(ctx);
                              return;
                            } catch (e2) {
                              _snack(L10n.global
                                  .tArgs('Save failed: {e}', {'e': e2}));
                            }
                          }
                        } else {
                          // Validation error — keep the editor open.
                          _snack(
                              L10n.global.tArgs('Save failed: {e}', {'e': msg}));
                        }
                      }
                    },
              child: Text(
                  busy ? ctx.tr('Saving…') : ctx.tr('Save workflow')),
            ),
          ],
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final defsAsync = ref.watch(workflowsProvider);
    final defs = defsAsync.valueOrNull ?? const <WorkflowDefSummary>[];
    WorkflowDefSummary? selected;
    for (final d in defs) {
      if (d.name == _selectedName) {
        selected = d;
        break;
      }
    }

    BoxDecoration card() => BoxDecoration(
          color: c.surface,
          borderRadius: BorderRadius.circular(AppTokens.rXl),
          border: Border.all(color: c.border),
        );

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        // ── Pick & run an existing workflow ──
        Container(
          padding: const EdgeInsets.all(AppTokens.s16),
          decoration: card(),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(children: [
                Icon(Icons.account_tree_outlined, size: 16, color: c.accent),
                const SizedBox(width: AppTokens.s6),
                Text(context.tr('Run a saved workflow'),
                    style: TextStyle(
                        color: c.textPrimary,
                        fontWeight: FontWeight.w600,
                        fontSize: 14)),
              ]),
              const SizedBox(height: AppTokens.s8),
              DropdownButtonHideUnderline(
                child: Container(
                  padding: const EdgeInsets.symmetric(horizontal: AppTokens.s8),
                  decoration: BoxDecoration(
                    color: c.surfaceAlt,
                    borderRadius: BorderRadius.circular(AppTokens.rMd),
                    border: Border.all(color: c.border),
                  ),
                  child: DropdownButton<String>(
                    value: defs.any((d) => d.name == _selectedName)
                        ? _selectedName
                        : null,
                    isExpanded: true,
                    hint: Text(
                        defs.isEmpty
                            ? context.tr('No workflows yet — create one below')
                            : context.tr('Pick a workflow…'),
                        style:
                            TextStyle(color: c.textMuted, fontSize: 13)),
                    items: [
                      for (final d in defs)
                        DropdownMenuItem(
                          value: d.name,
                          child: Text(
                            '${d.name} · ${context.trArgs('{n} step', {'n': d.stepCount})}${(d.description ?? '').isEmpty ? '' : ' — ${d.description}'}',
                            overflow: TextOverflow.ellipsis,
                            style: TextStyle(
                                color: c.textPrimary, fontSize: 13),
                          ),
                        ),
                    ],
                    onChanged: (v) {
                      if (v != null) _pick(v, defs);
                    },
                  ),
                ),
              ),
              if (selected != null) ...[
                const SizedBox(height: AppTokens.s12),
                for (final i in selected.inputs)
                  Padding(
                    padding: const EdgeInsets.only(bottom: AppTokens.s8),
                    child: TextField(
                      controller: _inputCtrls[i.name],
                      decoration: InputDecoration(
                        labelText: i.required ? '${i.name} *' : i.name,
                        helperText: i.description,
                        border: const OutlineInputBorder(),
                        isDense: true,
                      ),
                      onSubmitted: (_) => _run(selected!),
                    ),
                  ),
                FilledButton.icon(
                  onPressed: _starting ? null : () => _run(selected!),
                  icon: _starting
                      ? const SizedBox(
                          width: 14,
                          height: 14,
                          child: CircularProgressIndicator(strokeWidth: 2))
                      : const Icon(Icons.play_arrow_rounded, size: 18),
                  label: Text(_starting
                      ? context.tr('Starting…')
                      : context.tr('Run workflow')),
                ),
              ],
            ],
          ),
        ),
        const SizedBox(height: AppTokens.s8),
        Center(
            child: Text(context.tr('or'),
                style: TextStyle(color: c.textMuted, fontSize: 12))),
        const SizedBox(height: AppTokens.s8),
        // ── Create a new one with the agent ──
        Container(
          padding: const EdgeInsets.all(AppTokens.s16),
          decoration: card(),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Row(children: [
                Icon(Icons.auto_awesome, size: 16, color: c.accent),
                const SizedBox(width: AppTokens.s6),
                Text(context.tr('Create a new workflow with the AI agent'),
                    style: TextStyle(
                        color: c.textPrimary,
                        fontWeight: FontWeight.w600,
                        fontSize: 14)),
              ]),
              const SizedBox(height: AppTokens.s8),
              TextField(
                controller: _desc,
                maxLines: 4,
                enabled: !_drafting,
                decoration: InputDecoration(
                  border: const OutlineInputBorder(),
                  hintText: context.tr(
                      'Describe the routine… e.g. Weekly: research a topic from 3 angles in parallel, fetch pricing with a script, then summarize into one report.'),
                ),
              ),
              const SizedBox(height: AppTokens.s8),
              OutlinedButton.icon(
                onPressed: _drafting ? null : _createWithAi,
                icon: _drafting
                    ? const SizedBox(
                        width: 14,
                        height: 14,
                        child: CircularProgressIndicator(strokeWidth: 2))
                    : const Icon(Icons.auto_awesome, size: 16),
                label: Text(_drafting
                    ? context.tr('Agent is drafting (30–120s)…')
                    : context.tr('Create workflow')),
              ),
              const SizedBox(height: AppTokens.s6),
              Text(
                context.tr(
                    'The draft opens in an editor for review — Save to keep it, Cancel to discard.'),
                textAlign: TextAlign.center,
                style: TextStyle(color: c.textMuted, fontSize: 11),
              ),
            ],
          ),
        ),
      ],
    );
  }
}
