import '../models/workflow_models.dart';
import 'api_client.dart';
import 'local_cache.dart';

/// Typed wrapper over the Workflow API at `/api/workflows/*`, tunnelled
/// through the relay. Workflows are saved DAG routines of agent + script
/// steps executed by the daemon. List fetches feed the [LocalCache] domain
/// tables so screens can paint instantly before the relay answers.
class WorkflowApi {
  final _api = ApiClient();

  // ── Definitions (templates) ────────────────────────────────────────────
  Future<List<WorkflowDefSummary>> listDefs() async {
    final r = await _api.get('/api/workflows');
    final maps = jsonMaps(r is Map ? r['workflows'] : null);
    LocalCache().putDomainList('workflows', maps);
    return maps.map(WorkflowDefSummary.fromJson).toList();
  }

  Future<List<WorkflowDefSummary>> listDefsCached() async =>
      (await LocalCache().getDomainList('workflows'))
          .map(WorkflowDefSummary.fromJson)
          .toList();

  /// One-shot agent authors a draft definition from a description.
  /// Slow (30–120s) — nothing is saved; the UI reviews then POSTs to create.
  Future<String> draft(String description) async {
    final r = await _api.post('/api/workflows/draft',
        body: {'description': description},
        timeout: const Duration(seconds: 200));
    return r is Map ? '${r['content'] ?? ''}' : '';
  }

  /// Create (or with [overwrite] replace) a definition; returns its name.
  Future<String> create(String content, {bool overwrite = false}) async {
    final r = await _api.post('/api/workflows',
        body: {'content': content, 'overwrite': overwrite});
    return r is Map ? '${r['name'] ?? ''}' : '';
  }

  // ── Runs ───────────────────────────────────────────────────────────────
  Future<List<WorkflowRun>> listRuns() async {
    final r = await _api.get('/api/workflows/runs');
    final maps = jsonMaps(r is Map ? r['runs'] : null);
    LocalCache().putDomainList('workflow_runs', maps);
    return maps.map(WorkflowRun.fromJson).toList();
  }

  Future<List<WorkflowRun>> listRunsCached() async =>
      (await LocalCache().getDomainList('workflow_runs'))
          .map(WorkflowRun.fromJson)
          .toList();

  /// Trigger a run; returns the run id.
  Future<String> startRun(String name, Map<String, String> inputs) async {
    final r = await _api.post(
        '/api/workflows/${Uri.encodeComponent(name)}/run',
        body: {'inputs': inputs});
    return r is Map ? '${r['runId'] ?? ''}' : '';
  }

  Future<void> cancelRun(String id) =>
      _api.post('/api/workflows/runs/${Uri.encodeComponent(id)}/cancel');

  /// Rename (empty label clears back to the id).
  Future<void> renameRun(String id, String label) => _api.patch(
      '/api/workflows/runs/${Uri.encodeComponent(id)}',
      body: {'label': label});

  Future<void> deleteRun(String id) =>
      _api.delete('/api/workflows/runs/${Uri.encodeComponent(id)}');

  /// Live agent activity (think / tool / message entries).
  Future<List<Map<String, dynamic>>> runActivity(String id) async {
    final r = await _api
        .get('/api/workflows/runs/${Uri.encodeComponent(id)}/activity');
    final list = (r is Map ? r['entries'] : null) as List? ?? const [];
    return list.whereType<Map>().map((m) => m.cast<String, dynamic>()).toList();
  }
}
