import 'dart:convert';
import 'dart:io' show File;

import 'package:file_picker/file_picker.dart';
import 'package:flutter/foundation.dart' show kIsWeb;
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../models/workflow_models.dart';
import '../../theme/tokens.dart';
import 'workflow_providers.dart';
import 'workflow_run_dialog.dart';
import 'workflow_runs_screen.dart' show openWorkflowRunProvider;

const _newWorkflowTemplate = '''---
name: my-workflow
description: What this routine does
inputs:
  - { name: topic, required: true }
steps:
  - id: fetch
    kind: script
    run: |
      echo "input: \$WF_INPUT_TOPIC"
  - id: analyze
    kind: agent
    persona: researcher
    prompt: |
      Analyze "{{input.topic}}". Raw data: {{steps.fetch.result}}
    observe: { label: "Result", from: result, as: inline }
---
(Notes for humans — the markdown body is not executed)
''';

/// Workflow TEMPLATE manager (Plugins → Workflow): author (by hand or via a
/// one-shot agent), import/export, tune guidance, and delete the `.md`
/// definitions. Runs are monitored on their own screen (/workflow-runs).
class WorkflowPanel extends ConsumerStatefulWidget {
  const WorkflowPanel({super.key});
  @override
  ConsumerState<WorkflowPanel> createState() => _WorkflowPanelState();
}

class _WorkflowPanelState extends ConsumerState<WorkflowPanel> {
  void _snack(String msg) {
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(msg)));
  }

  void _refreshAll() {
    ref.invalidate(workflowsProvider);
    ref.invalidate(workflowRunsProvider);
  }

  // ── Dialogs ──

  Future<void> _openEditor({String? name, String? initialContent}) async {
    String content = initialContent ?? _newWorkflowTemplate;
    if (name != null) {
      try {
        (_, content) = await fetchWorkflowDefinition(ref, name);
      } catch (e) {
        _snack('Cannot load definition: $e');
        return;
      }
    }
    if (!mounted) return;
    final controller = TextEditingController(text: content);
    final saved = await showDialog<bool>(
      context: context,
      builder: (ctx) => _EditorDialog(
        title: name == null ? 'New workflow' : 'Edit: $name',
        controller: controller,
      ),
    );
    if (saved != true) return;
    try {
      if (name == null) {
        final created = await createWorkflow(ref, controller.text);
        _snack('Created workflow "$created"');
      } else {
        await updateWorkflow(ref, name, controller.text);
        _snack('Saved "$name"');
      }
    } catch (e) {
      _snack('Save failed: $e');
    }
  }

  /// Runtime settings dialog: LLM parallel requests + no-result retries.
  Future<void> _openSettings() async {
    int llmParallel;
    int agentRetries;
    try {
      (llmParallel, agentRetries) = await fetchWorkflowSettings(ref);
    } catch (e) {
      _snack('Cannot load settings: $e');
      return;
    }
    if (!mounted) return;
    final c = context.colors;
    final llmCtrl = TextEditingController(text: '$llmParallel');
    final retryCtrl = TextEditingController(text: '$agentRetries');
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('Execution settings'),
        content: SizedBox(
          width: 400,
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              TextField(
                controller: llmCtrl,
                keyboardType: TextInputType.number,
                decoration: const InputDecoration(
                  labelText: 'Parallel LLM requests (1–16)',
                  helperText:
                      'Many providers allow only 1 request at a time. Agent steps beyond the budget WAIT as pending — their timeout only starts when they run.',
                  helperMaxLines: 4,
                  border: OutlineInputBorder(),
                ),
              ),
              const SizedBox(height: AppTokens.s12),
              TextField(
                controller: retryCtrl,
                keyboardType: TextInputType.number,
                decoration: const InputDecoration(
                  labelText: 'Retries when no result (0–5)',
                  helperText:
                      'Agent steps that hit a session error or return empty text are retried this many times before failing.',
                  helperMaxLines: 3,
                  border: OutlineInputBorder(),
                ),
              ),
              const SizedBox(height: AppTokens.s8),
              Text('Applied live — queued steps pick it up immediately.',
                  style: TextStyle(color: c.textMuted, fontSize: 11)),
            ],
          ),
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
      final p = (int.tryParse(llmCtrl.text.trim()) ?? 1).clamp(1, 16);
      final r = (int.tryParse(retryCtrl.text.trim()) ?? 1).clamp(0, 5);
      await saveWorkflowSettings(ref, p, r);
      _snack('Settings saved: $p parallel, $r retries');
    } catch (e) {
      _snack('Save failed: $e');
    }
  }

  /// "Draft with agent": describe the routine in plain language; a one-shot
  /// agent authors the .md (validated server-side), which lands in the
  /// editor for review — nothing is saved until the user confirms.
  Future<void> _draft() async {
    final descCtrl = TextEditingController();
    var busy = false;
    final content = await showDialog<String>(
      context: context,
      barrierDismissible: false,
      builder: (ctx) => StatefulBuilder(
        builder: (ctx, setDlg) => AlertDialog(
          title: const Text('✨ Draft with agent'),
          content: SizedBox(
            width: 520,
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  'Describe the routine — the agent picks matching personas, '
                  'builds the steps + guidance, and returns a draft for review. '
                  'Takes ~30–120s.',
                  style: TextStyle(
                      color: ctx.colors.textSecondary, fontSize: 12),
                ),
                const SizedBox(height: AppTokens.s8),
                TextField(
                  controller: descCtrl,
                  maxLines: 5,
                  enabled: !busy,
                  decoration: const InputDecoration(
                    border: OutlineInputBorder(),
                    hintText:
                        'e.g. Weekly: research a topic from 3 angles in parallel, fetch pricing with a script, then summarize into one report.',
                  ),
                ),
                if (busy) ...[
                  const SizedBox(height: AppTokens.s12),
                  const Center(child: CircularProgressIndicator()),
                ],
              ],
            ),
          ),
          actions: [
            TextButton(
              onPressed: busy ? null : () => Navigator.pop(ctx),
              child: const Text('Cancel'),
            ),
            FilledButton(
              onPressed: busy
                  ? null
                  : () async {
                      if (descCtrl.text.trim().isEmpty) return;
                      setDlg(() => busy = true);
                      try {
                        final (_, md) =
                            await draftWorkflow(ref, descCtrl.text);
                        if (ctx.mounted) Navigator.pop(ctx, md);
                      } catch (e) {
                        setDlg(() => busy = false);
                        if (ctx.mounted) {
                          ScaffoldMessenger.of(ctx).showSnackBar(
                              SnackBar(content: Text('Draft failed: $e')));
                        }
                      }
                    },
              child: Text(busy ? 'Drafting…' : 'Draft'),
            ),
          ],
        ),
      ),
    );
    if (content == null || content.isEmpty || !mounted) return;
    await _openEditor(initialContent: content);
  }

  /// Tune-guidance form: workflow-level guidance + per-agent-step guidance
  /// and per-step timeout, saved via targeted PATCH (never rewrites the DAG).
  Future<void> _openTune(WorkflowDefSummary def) async {
    final wfCtrl = TextEditingController(text: def.guidance ?? '');
    final stepGuidance = {
      for (final s in def.steps)
        if (s.kind == 'agent') s.id: TextEditingController(text: s.guidance ?? ''),
    };
    final stepTimeout = {
      for (final s in def.steps)
        s.id: TextEditingController(text: s.timeout?.toString() ?? ''),
    };
    final c = context.colors;
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text('Tune guidance: ${def.name}'),
        content: SizedBox(
          width: 620,
          height: 460,
          child: SingleChildScrollView(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  'Guidance is the RULES layer (persona = identity, prompt = task). '
                  'Editing here never touches the DAG structure; empty = remove.',
                  style: TextStyle(color: c.textSecondary, fontSize: 12),
                ),
                const SizedBox(height: AppTokens.s12),
                Text('Workflow guidance (applies to all agent steps)',
                    style: TextStyle(
                        color: c.textPrimary,
                        fontSize: 13,
                        fontWeight: FontWeight.w600)),
                const SizedBox(height: 4),
                TextField(
                  controller: wfCtrl,
                  maxLines: 3,
                  decoration:
                      const InputDecoration(border: OutlineInputBorder()),
                ),
                const SizedBox(height: AppTokens.s12),
                for (final s in def.steps) ...[
                  Container(
                    margin: const EdgeInsets.only(bottom: AppTokens.s8),
                    padding: const EdgeInsets.all(AppTokens.s8),
                    decoration: BoxDecoration(
                      borderRadius: BorderRadius.circular(AppTokens.rMd),
                      border: Border.all(color: c.border),
                    ),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Row(children: [
                          Text(s.id,
                              style: TextStyle(
                                  color: c.textPrimary,
                                  fontWeight: FontWeight.w600,
                                  fontSize: 13)),
                          const SizedBox(width: AppTokens.s6),
                          Text(
                              s.kind == 'agent'
                                  ? 'agent · ${s.persona ?? '?'}'
                                  : 'script',
                              style: TextStyle(
                                  color: s.kind == 'agent'
                                      ? AppTokens.brand
                                      : AppTokens.cyan,
                                  fontSize: 11)),
                          const Spacer(),
                          Text('timeout (s): ',
                              style: TextStyle(
                                  color: c.textSecondary, fontSize: 11)),
                          SizedBox(
                            width: 70,
                            child: TextField(
                              controller: stepTimeout[s.id],
                              keyboardType: TextInputType.number,
                              decoration: const InputDecoration(
                                border: OutlineInputBorder(),
                                isDense: true,
                                hintText: '600',
                              ),
                              style: const TextStyle(fontSize: 12),
                            ),
                          ),
                        ]),
                        if (s.kind == 'agent') ...[
                          const SizedBox(height: AppTokens.s6),
                          TextField(
                            controller: stepGuidance[s.id],
                            maxLines: 3,
                            decoration: const InputDecoration(
                              border: OutlineInputBorder(),
                              hintText:
                                  'Rules for this step: output format, scope, tone…',
                            ),
                            style: const TextStyle(fontSize: 13),
                          ),
                        ],
                      ],
                    ),
                  ),
                ],
              ],
            ),
          ),
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
      final steps = <Map<String, dynamic>>[];
      for (final s in def.steps) {
        final entry = <String, dynamic>{'id': s.id};
        if (s.kind == 'agent') entry['guidance'] = stepGuidance[s.id]!.text;
        final t = int.tryParse(stepTimeout[s.id]!.text.trim());
        if (t != null && t > 0) entry['timeout'] = t;
        steps.add(entry);
      }
      await patchWorkflowFields(ref, def.name, {
        'guidance': wfCtrl.text,
        'steps': steps,
      });
      _snack('Saved guidance for "${def.name}"');
    } catch (e) {
      _snack('Save failed: $e');
    }
  }

  Future<void> _import() async {
    String? content;
    // Native: real file dialog. Web build (or cancel): fall back to paste.
    if (!kIsWeb) {
      final picked = await FilePicker.platform.pickFiles(
        type: FileType.custom,
        allowedExtensions: ['md', 'markdown', 'txt'],
        withData: true,
      );
      if (picked == null) return;
      final f = picked.files.first;
      content = f.bytes != null
          ? utf8.decode(f.bytes!, allowMalformed: true)
          : (f.path != null ? await File(f.path!).readAsString() : null);
    } else {
      final controller = TextEditingController();
      final ok = await showDialog<bool>(
        context: context,
        builder: (ctx) => _EditorDialog(
          title: 'Import workflow (paste .md content)',
          controller: controller,
          okLabel: 'Import',
        ),
      );
      if (ok != true) return;
      content = controller.text;
    }
    if (content == null || content.trim().isEmpty) return;
    try {
      final created = await createWorkflow(ref, content);
      _snack('Imported workflow "$created"');
    } catch (e) {
      final msg = '$e';
      if (msg.contains('already exists') && mounted) {
        final overwrite = await showDialog<bool>(
          context: context,
          builder: (ctx) => AlertDialog(
            title: const Text('Workflow already exists'),
            content: const Text('Overwrite the existing definition?'),
            actions: [
              TextButton(
                  onPressed: () => Navigator.pop(ctx, false),
                  child: const Text('Cancel')),
              FilledButton(
                  onPressed: () => Navigator.pop(ctx, true),
                  child: const Text('Overwrite')),
            ],
          ),
        );
        if (overwrite == true) {
          try {
            final created =
                await createWorkflow(ref, content, overwrite: true);
            _snack('Imported workflow "$created" (overwritten)');
          } catch (e2) {
            _snack('Import failed: $e2');
          }
        }
      } else {
        _snack('Import failed: $msg');
      }
    }
  }

  Future<void> _export(String name) async {
    try {
      final (fileName, content) = await fetchWorkflowDefinition(ref, name);
      if (!mounted) return;
      if (!kIsWeb) {
        // Native: save-file dialog straight to disk.
        final path = await FilePicker.platform.saveFile(
          dialogTitle: 'Export workflow',
          fileName: fileName,
          type: FileType.custom,
          allowedExtensions: ['md'],
        );
        if (path == null) return;
        await File(path).writeAsString(content);
        _snack('Exported to $path');
        return;
      }
      // Web build: show + copy.
      await showDialog<void>(
        context: context,
        builder: (ctx) => AlertDialog(
          title: Text('Export: $fileName'),
          content: SizedBox(
            width: 560,
            child: SingleChildScrollView(
              child: SelectableText(content,
                  style: const TextStyle(fontFamily: 'monospace', fontSize: 12)),
            ),
          ),
          actions: [
            TextButton(
              onPressed: () async {
                await Clipboard.setData(ClipboardData(text: content));
                if (ctx.mounted) Navigator.pop(ctx);
              },
              child: const Text('Copy to clipboard'),
            ),
            FilledButton(
                onPressed: () => Navigator.pop(ctx),
                child: const Text('Close')),
          ],
        ),
      );
    } catch (e) {
      _snack('Export failed: $e');
    }
  }

  Future<void> _delete(String name) async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text('Delete workflow "$name"?'),
        content:
            const Text('Run history and the workspace directory are kept.'),
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
    if (ok != true) return;
    try {
      await deleteWorkflow(ref, name);
      _snack('Deleted "$name"');
    } catch (e) {
      _snack('Delete failed: $e');
    }
  }

  /// Trigger a run, then jump to the run monitor with the new run selected.
  Future<void> _run(WorkflowDefSummary def) async {
    await showWorkflowRunDialog(context, ref, def, onStarted: (runId) {
      ref.read(openWorkflowRunProvider.notifier).state = runId;
      if (mounted) context.go('/workflow-runs');
    });
  }

  void _showDetail(WorkflowDefSummary def) {
    showDialog<void>(
      context: context,
      builder: (ctx) => _DetailDialog(
        def: def,
        onRun: () {
          Navigator.pop(ctx);
          _run(def);
        },
        onEdit: () {
          Navigator.pop(ctx);
          _openEditor(name: def.name);
        },
      ),
    );
  }

  // ── Build ──

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final defs = ref.watch(workflowsProvider);

    return Padding(
      padding: const EdgeInsets.all(AppTokens.s16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Icon(Icons.account_tree_outlined, color: c.accent, size: 20),
              const SizedBox(width: AppTokens.s8),
              Text('Workflow templates',
                  style: TextStyle(
                      color: c.textPrimary,
                      fontSize: 16,
                      fontWeight: FontWeight.w700)),
              const SizedBox(width: AppTokens.s8),
              Expanded(
                child: Text('multi-step routine definitions (agent + script)',
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(color: c.textSecondary, fontSize: 12)),
              ),
              OutlinedButton.icon(
                onPressed: () => context.go('/workflow-runs'),
                icon: const Icon(Icons.history_rounded, size: 16),
                label: const Text('Run history'),
              ),
              const SizedBox(width: AppTokens.s8),
              OutlinedButton.icon(
                onPressed: _draft,
                icon: const Icon(Icons.auto_awesome, size: 16),
                label: const Text('Draft with agent'),
              ),
              const SizedBox(width: AppTokens.s8),
              OutlinedButton.icon(
                onPressed: _import,
                icon: const Icon(Icons.upload_outlined, size: 16),
                label: const Text('Import'),
              ),
              const SizedBox(width: AppTokens.s8),
              FilledButton.icon(
                onPressed: () => _openEditor(),
                icon: const Icon(Icons.add_rounded, size: 16),
                label: const Text('New workflow'),
              ),
              const SizedBox(width: AppTokens.s8),
              IconButton(
                onPressed: _openSettings,
                icon: const Icon(Icons.settings_outlined, size: 18),
                tooltip: 'Execution settings (LLM parallel, retries)',
              ),
              IconButton(
                onPressed: _refreshAll,
                icon: const Icon(Icons.refresh_rounded, size: 18),
                tooltip: 'Refresh',
              ),
            ],
          ),
          const SizedBox(height: AppTokens.s12),
          Expanded(child: _buildDefs(c, defs)),
        ],
      ),
    );
  }

  Widget _buildDefs(AppColors c, AsyncValue<List<WorkflowDefSummary>> defs) {
    return defs.when(
      loading: () => const Center(child: CircularProgressIndicator()),
      error: (e, _) => Center(
          child: Text('Cannot load workflows: $e',
              style: TextStyle(color: c.textSecondary))),
      data: (list) {
        if (list.isEmpty) {
          return Center(
            child: Text(
                'No workflows yet — click "New workflow" or "Import" to add one.\nDefinitions live in ~/senclaw/workflows/*.md',
                textAlign: TextAlign.center,
                style: TextStyle(color: c.textSecondary, fontSize: 13)),
          );
        }
        return ListView.separated(
          itemCount: list.length,
          separatorBuilder: (_, _) => const SizedBox(height: AppTokens.s8),
          itemBuilder: (ctx, i) {
            final d = list[i];
            return Container(
              padding: const EdgeInsets.all(AppTokens.s12),
              decoration: BoxDecoration(
                color: c.surface,
                borderRadius: BorderRadius.circular(AppTokens.rLg),
                border: Border.all(color: c.border),
              ),
              child: Row(
                children: [
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Row(children: [
                          Text(d.name,
                              style: TextStyle(
                                  color: c.textPrimary,
                                  fontWeight: FontWeight.w600)),
                          const SizedBox(width: AppTokens.s8),
                          _chip(c, '${d.stepCount} step'),
                          for (final inp in d.inputs.take(4)) ...[
                            const SizedBox(width: 4),
                            _chip(c, inp.name,
                                color: inp.required ? AppTokens.brand : null),
                          ],
                        ]),
                        if ((d.description ?? '').isNotEmpty)
                          Padding(
                            padding: const EdgeInsets.only(top: 2),
                            child: Text(d.description!,
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: TextStyle(
                                    color: c.textSecondary, fontSize: 12)),
                          ),
                      ],
                    ),
                  ),
                  IconButton(
                      tooltip: 'Run',
                      onPressed: () => _run(d),
                      icon: Icon(Icons.play_arrow_rounded,
                          color: AppTokens.success, size: 20)),
                  IconButton(
                      tooltip: 'Tune guidance',
                      onPressed: () => _openTune(d),
                      icon: Icon(Icons.tune_rounded,
                          color: c.textSecondary, size: 18)),
                  IconButton(
                      tooltip: 'Details',
                      onPressed: () => _showDetail(d),
                      icon: Icon(Icons.visibility_outlined,
                          color: c.textSecondary, size: 18)),
                  IconButton(
                      tooltip: 'Edit',
                      onPressed: () => _openEditor(name: d.name),
                      icon: Icon(Icons.edit_outlined,
                          color: c.textSecondary, size: 18)),
                  IconButton(
                      tooltip: 'Export',
                      onPressed: () => _export(d.name),
                      icon: Icon(Icons.download_outlined,
                          color: c.textSecondary, size: 18)),
                  IconButton(
                      tooltip: 'Delete',
                      onPressed: () => _delete(d.name),
                      icon: Icon(Icons.delete_outline,
                          color: AppTokens.danger, size: 18)),
                ],
              ),
            );
          },
        );
      },
    );
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
}

// ── Editor dialog (create / edit / import paste) ────────────────────────────

class _EditorDialog extends StatelessWidget {
  const _EditorDialog({
    required this.title,
    required this.controller,
    this.okLabel = 'Save',
  });
  final String title;
  final TextEditingController controller;
  final String okLabel;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return AlertDialog(
      title: Text(title),
      content: SizedBox(
        width: 640,
        height: 440,
        child: TextField(
          controller: controller,
          maxLines: null,
          expands: true,
          textAlignVertical: TextAlignVertical.top,
          style: const TextStyle(fontFamily: 'monospace', fontSize: 12),
          decoration: InputDecoration(
            border: const OutlineInputBorder(),
            hintText: 'Markdown with YAML frontmatter…',
            hintStyle: TextStyle(color: c.textSecondary),
          ),
        ),
      ),
      actions: [
        TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('Cancel')),
        FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: Text(okLabel)),
      ],
    );
  }
}

// ── Detail dialog ────────────────────────────────────────────────────────────

class _DetailDialog extends StatelessWidget {
  const _DetailDialog(
      {required this.def, required this.onRun, required this.onEdit});
  final WorkflowDefSummary def;
  final VoidCallback onRun;
  final VoidCallback onEdit;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return AlertDialog(
      title: Text('Workflow: ${def.name}'),
      content: SizedBox(
        width: 560,
        child: SingleChildScrollView(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              if ((def.description ?? '').isNotEmpty) ...[
                Text(def.description!,
                    style: TextStyle(color: c.textSecondary, fontSize: 13)),
                const SizedBox(height: AppTokens.s12),
              ],
              if ((def.workspace ?? '').isNotEmpty) ...[
                Text('Workspace: ${def.workspace}',
                    style: TextStyle(
                        color: c.textSecondary,
                        fontSize: 12,
                        fontFamily: 'monospace')),
                const SizedBox(height: AppTokens.s12),
              ],
              if (def.inputs.isNotEmpty) ...[
                Text('Inputs',
                    style: TextStyle(
                        color: c.textPrimary, fontWeight: FontWeight.w600)),
                const SizedBox(height: AppTokens.s6),
                for (final i in def.inputs)
                  Padding(
                    padding: const EdgeInsets.only(bottom: 4),
                    child: Text(
                      '• ${i.name}'
                      '${i.required ? ' (required)' : ''}'
                      '${i.defaultValue != null ? ' [default: ${i.defaultValue}]' : ''}'
                      '${i.description != null ? ' — ${i.description}' : ''}',
                      style: TextStyle(color: c.textSecondary, fontSize: 12),
                    ),
                  ),
                const SizedBox(height: AppTokens.s12),
              ],
              Text('Steps (${def.steps.length})',
                  style: TextStyle(
                      color: c.textPrimary, fontWeight: FontWeight.w600)),
              const SizedBox(height: AppTokens.s6),
              for (final s in def.steps)
                Container(
                  margin: const EdgeInsets.only(bottom: AppTokens.s6),
                  padding: const EdgeInsets.all(AppTokens.s8),
                  decoration: BoxDecoration(
                    borderRadius: BorderRadius.circular(AppTokens.rMd),
                    border: Border.all(color: c.border),
                  ),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Row(children: [
                        Text(s.id,
                            style: TextStyle(
                                color: c.textPrimary,
                                fontWeight: FontWeight.w600,
                                fontSize: 13)),
                        const SizedBox(width: AppTokens.s6),
                        Text(
                            s.kind == 'agent'
                                ? 'agent · ${s.persona ?? '?'}'
                                : 'script',
                            style: TextStyle(
                                color: s.kind == 'agent'
                                    ? AppTokens.brand
                                    : AppTokens.cyan,
                                fontSize: 11)),
                        if (s.timeout != null) ...[
                          const SizedBox(width: AppTokens.s6),
                          Text('${s.timeout}s',
                              style: TextStyle(
                                  color: c.textSecondary, fontSize: 11)),
                        ],
                      ]),
                      if (s.dependsOn.isNotEmpty)
                        Text('← waits for: ${s.dependsOn.join(', ')}',
                            style: TextStyle(
                                color: c.textSecondary, fontSize: 11)),
                    ],
                  ),
                ),
            ],
          ),
        ),
      ),
      actions: [
        TextButton(onPressed: onEdit, child: const Text('Edit')),
        FilledButton.icon(
          onPressed: onRun,
          icon: const Icon(Icons.play_arrow_rounded, size: 16),
          label: const Text('Run'),
        ),
      ],
    );
  }
}
