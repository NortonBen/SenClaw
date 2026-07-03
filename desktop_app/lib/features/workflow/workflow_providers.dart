import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/transport/connection.dart';
import '../../models/workflow_models.dart';

/// Definition summaries (`GET /api/workflows` → `{workflows: [...]}`).
final workflowsProvider = FutureProvider<List<WorkflowDefSummary>>((ref) async {
  final r = await ref.read(apiClientProvider).get('/api/workflows');
  final list = (r is Map ? r['workflows'] : null) as List? ?? const [];
  return list
      .whereType<Map>()
      .map((m) => WorkflowDefSummary.fromJson(m.cast<String, dynamic>()))
      .toList();
});

/// Run history, newest first (`GET /api/workflows/runs` → `{runs: [...]}`).
final workflowRunsProvider = FutureProvider<List<WorkflowRun>>((ref) async {
  final r = await ref.read(apiClientProvider).get('/api/workflows/runs');
  final list = (r is Map ? r['runs'] : null) as List? ?? const [];
  return list
      .whereType<Map>()
      .map((m) => WorkflowRun.fromJson(m.cast<String, dynamic>()))
      .toList();
});

/// Raw markdown of one definition (edit/export).
Future<(String fileName, String content)> fetchWorkflowDefinition(
    WidgetRef ref, String name) async {
  final r = await ref
      .read(apiClientProvider)
      .get('/api/workflows/${Uri.encodeComponent(name)}/definition');
  final m = r as Map;
  return ('${m['fileName'] ?? '$name.md'}', '${m['content'] ?? ''}');
}

/// Trigger a run; returns the run id.
Future<String> startWorkflowRun(
    WidgetRef ref, String name, Map<String, String> inputs) async {
  final r = await ref
      .read(apiClientProvider)
      .post('/api/workflows/${Uri.encodeComponent(name)}/run',
          body: {'inputs': inputs});
  ref.invalidate(workflowRunsProvider);
  return r is Map ? '${r['runId'] ?? ''}' : '';
}

Future<void> cancelWorkflowRun(WidgetRef ref, String runId) async {
  await ref
      .read(apiClientProvider)
      .post('/api/workflows/runs/${Uri.encodeComponent(runId)}/cancel');
  ref.invalidate(workflowRunsProvider);
}

/// Create (also import). Returns the workflow name.
Future<String> createWorkflow(WidgetRef ref, String content,
    {bool overwrite = false}) async {
  final r = await ref.read(apiClientProvider).post('/api/workflows',
      body: {'content': content, 'overwrite': overwrite});
  ref.invalidate(workflowsProvider);
  return r is Map ? '${r['name'] ?? ''}' : '';
}

Future<void> updateWorkflow(WidgetRef ref, String name, String content) async {
  await ref.read(apiClientProvider).put(
      '/api/workflows/${Uri.encodeComponent(name)}/definition',
      body: {'content': content});
  ref.invalidate(workflowsProvider);
}

Future<void> deleteWorkflow(WidgetRef ref, String name) async {
  await ref
      .read(apiClientProvider)
      .delete('/api/workflows/${Uri.encodeComponent(name)}');
  ref.invalidate(workflowsProvider);
}

/// Ask a one-shot agent to author a draft definition from a description.
/// Returns (workflow name, markdown content); nothing is saved yet.
Future<(String, String)> draftWorkflow(WidgetRef ref, String description) async {
  final r = await ref
      .read(apiClientProvider)
      .post('/api/workflows/draft', body: {'description': description});
  final m = r as Map;
  return ('${m['name'] ?? ''}', '${m['content'] ?? ''}');
}

/// Targeted guidance/timeout edit (the tune form). `patch` shape:
/// { guidance?: String, steps: [{id, guidance?, timeout?}] }.
Future<void> patchWorkflowFields(
    WidgetRef ref, String name, Map<String, dynamic> patch) async {
  await ref.read(apiClientProvider).patch(
      '/api/workflows/${Uri.encodeComponent(name)}/definition',
      body: patch);
  ref.invalidate(workflowsProvider);
}

/// Runtime settings: LLM parallelism + no-result retries.
/// GET/PUT `/api/workflows/settings` — applied live daemon-side.
Future<(int llmParallel, int agentRetries)> fetchWorkflowSettings(
    WidgetRef ref) async {
  final r = await ref.read(apiClientProvider).get('/api/workflows/settings');
  final m = r as Map;
  return (
    (m['llmParallel'] as num?)?.toInt() ?? 1,
    (m['agentRetries'] as num?)?.toInt() ?? 1,
  );
}

Future<void> saveWorkflowSettings(
    WidgetRef ref, int llmParallel, int agentRetries) async {
  await ref.read(apiClientProvider).put('/api/workflows/settings', body: {
    'llmParallel': llmParallel,
    'agentRetries': agentRetries,
  });
}

/// Rename a run (empty label clears back to the id).
Future<void> renameWorkflowRun(WidgetRef ref, String id, String label) async {
  await ref.read(apiClientProvider).patch(
      '/api/workflows/runs/${Uri.encodeComponent(id)}',
      body: {'label': label});
  ref.invalidate(workflowRunsProvider);
}

/// Delete a run record (server rejects while running).
Future<void> deleteWorkflowRun(WidgetRef ref, String id) async {
  await ref
      .read(apiClientProvider)
      .delete('/api/workflows/runs/${Uri.encodeComponent(id)}');
  ref.invalidate(workflowRunsProvider);
}

/// Save markdown into the personal wiki under `workflows/…`.
Future<void> saveRunToWiki(WidgetRef ref, String path, String content) async {
  await ref.read(apiClientProvider).put('/api/wiki/file', body: {
    'path': path,
    'content': content,
    'source': 'workflow',
    'tags': ['workflow'],
    'commit_msg': 'workflow: save $path',
  });
}

String sanitizeWikiSegment(String s) =>
    s.replaceAll(RegExp(r'[^A-Za-z0-9._-]+'), '_');

/// Live agent activity of a run (think / tool / message entries).
Future<List<Map<String, dynamic>>> fetchRunActivity(
    WidgetRef ref, String runId) async {
  final r = await ref
      .read(apiClientProvider)
      .get('/api/workflows/runs/${Uri.encodeComponent(runId)}/activity');
  final list = (r is Map ? r['entries'] : null) as List? ?? const [];
  return list.whereType<Map>().map((m) => m.cast<String, dynamic>()).toList();
}
