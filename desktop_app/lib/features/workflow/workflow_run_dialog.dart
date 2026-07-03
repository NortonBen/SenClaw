import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../models/workflow_models.dart';
import '../../theme/tokens.dart';
import 'workflow_providers.dart';

/// Shared run-inputs dialog: collects the workflow's inputs (required marks,
/// defaults, optional preset from a previous run) and triggers the run.
/// Used by the template manager (Plugins → Workflow) and the run monitor
/// (re-run). `onStarted` fires with the new run id.
Future<void> showWorkflowRunDialog(
  BuildContext context,
  WidgetRef ref,
  WorkflowDefSummary def, {
  Map<String, String>? preset,
  void Function(String runId)? onStarted,
}) async {
  final controllers = {
    for (final i in def.inputs)
      i.name: TextEditingController(text: preset?[i.name] ?? i.defaultValue ?? ''),
  };
  final ok = await showDialog<bool>(
    context: context,
    builder: (ctx) => AlertDialog(
      title: Text('Run: ${def.name}'),
      content: SizedBox(
        width: 420,
        child: def.inputs.isEmpty
            ? const Text('This workflow takes no inputs.')
            : Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  for (final i in def.inputs)
                    Padding(
                      padding: const EdgeInsets.only(bottom: AppTokens.s8),
                      child: TextField(
                        controller: controllers[i.name],
                        decoration: InputDecoration(
                          labelText: i.required ? '${i.name} *' : i.name,
                          helperText: i.description,
                          border: const OutlineInputBorder(),
                          isDense: true,
                        ),
                      ),
                    ),
                ],
              ),
      ),
      actions: [
        TextButton(
            onPressed: () => Navigator.pop(ctx, false),
            child: const Text('Cancel')),
        FilledButton(
            onPressed: () => Navigator.pop(ctx, true),
            child: const Text('Run')),
      ],
    ),
  );
  if (ok != true || !context.mounted) return;

  final missing = def.inputs
      .where((i) => i.required && controllers[i.name]!.text.trim().isEmpty)
      .map((i) => i.name)
      .toList();
  if (missing.isNotEmpty) {
    ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Missing required input(s): ${missing.join(', ')}')));
    return;
  }
  try {
    final inputs = <String, String>{
      for (final e in controllers.entries)
        if (e.value.text.trim().isNotEmpty) e.key: e.value.text,
    };
    final runId = await startWorkflowRun(ref, def.name, inputs);
    if (context.mounted) {
      ScaffoldMessenger.of(context)
          .showSnackBar(SnackBar(content: Text('Started: $runId')));
    }
    onStarted?.call(runId);
  } catch (e) {
    if (context.mounted) {
      ScaffoldMessenger.of(context)
          .showSnackBar(SnackBar(content: Text('Run failed: $e')));
    }
  }
}
