import '../models/background_models.dart';
import 'api_client.dart';

/// REST client for `/api/background/*` — the daemon's autonomous background
/// tasks (see the desktop app's `background_providers.dart`). All calls tunnel
/// through the relay via [ApiClient].
///
/// Unlike the desktop, the relay does NOT forward the `bg:*` WS events (they
/// broadcast to admin WS clients only), so screens poll/refresh instead of
/// following a live stream.
class BackgroundApi {
  static final BackgroundApi _instance = BackgroundApi._internal();
  factory BackgroundApi() => _instance;
  BackgroundApi._internal();

  final _api = ApiClient();

  List<Map<String, dynamic>> _maps(dynamic raw) =>
      (raw is List ? raw : const [])
          .whereType<Map>()
          .map((e) => e.cast<String, dynamic>())
          .toList();

  Future<BgTaskPage> listTasks({
    bool includeInternal = false,
    String? status,
    int limit = 20,
    int offset = 0,
  }) async {
    final path = ApiClient.withQuery('/api/background/tasks', {
      if (includeInternal) 'include_internal': 'true',
      'status': status,
      'limit': limit,
      'offset': offset,
    });
    final m = await _api.getObject(path);
    return BgTaskPage(
      _maps(m['tasks']).map(BackgroundTask.fromJson).toList(),
      (m['total'] as num?)?.toInt() ?? 0,
    );
  }

  Future<BackgroundStats> stats(String window) async {
    final m = await _api
        .getObject(ApiClient.withQuery('/api/background/stats', {'window': window}));
    return BackgroundStats.fromJson(m);
  }

  /// Run history for one task (most recent first).
  Future<List<BackgroundRun>> runs(String taskId) async {
    final m = await _api.getObject(
        ApiClient.withQuery('/api/background/tasks/$taskId/runs', {'limit': 50}));
    return _maps(m['runs']).map(BackgroundRun.fromJson).toList();
  }

  /// One run plus its transcript.
  Future<BackgroundSession> session(String runId) async {
    final m = await _api.getObject('/api/background/runs/$runId');
    return BackgroundSession(
      BackgroundRun.fromJson(
          (m['run'] as Map?)?.cast<String, dynamic>() ?? const {}),
      _maps(m['activity']).map(BackgroundActivity.fromJson).toList(),
    );
  }

  /// Turn one line of natural language into a draft task spec via the daemon's
  /// LLM. Does not create anything — the caller reviews and then calls
  /// [create].
  Future<Map<String, dynamic>> parseQuick(String text) async {
    final r = await _api.post('/api/background/parse', body: {'text': text});
    return (r as Map?)?.cast<String, dynamic>() ?? const {};
  }

  Future<void> create(Map<String, dynamic> body) =>
      _api.post('/api/background/tasks', body: body);

  Future<void> update(String id, Map<String, dynamic> body) =>
      _api.patch('/api/background/tasks/$id', body: body);

  Future<void> pause(String id) => update(id, {'status': 'paused'});

  /// Resuming also clears the failure counter daemon-side — otherwise a task
  /// the user deliberately un-paused would re-quarantine on its next failure.
  Future<void> resume(String id) => update(id, {'status': 'active'});

  Future<void> delete(String id) => _api.delete('/api/background/tasks/$id');

  /// Runs inline daemon-side and returns the run id, so the caller can open
  /// the session straight away.
  Future<String> runNow(String id) async {
    final r = await _api.post('/api/background/tasks/$id/run-now');
    return '${(r as Map?)?['run_id'] ?? ''}';
  }

  Future<void> cancelRun(String runId) =>
      _api.post('/api/background/runs/$runId/cancel');
}
