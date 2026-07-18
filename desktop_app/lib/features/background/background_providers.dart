import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/transport/connection.dart';
import '../../models/background_models.dart';

/// Bumped to force the task/stats lists to refetch after a mutation.
final bgRevProvider = StateProvider<int>((ref) => 0);

/// Show `visibility = 'internal'` tasks (core upkeep: cognitive decay,
/// maintenance, the SOUL watcher). Off by default so system jobs don't bury the
/// user's own tasks.
final bgShowInternalProvider = StateProvider<bool>((ref) => false);

/// Stats window: 24h | 7d | 30d.
final bgWindowProvider = StateProvider<String>((ref) => '7d');

/// Task selected in the list (drives the detail pane).
final bgSelectedTaskProvider = StateProvider<String?>((ref) => null);

/// Status filter for the list. Null = all.
final bgStatusFilterProvider = StateProvider<String?>((ref) => null);

/// Zero-based page for the list pager.
final bgPageProvider = StateProvider<int>((ref) => 0);

/// Page size for the task list.
const bgPageSize = 20;

/// Ticks so relative times ("Next in 40s") recompute instead of freezing at
/// whatever they read when the list was last fetched.
///
/// Refetching is driven by `bg:run:finished` over WS, which for a slow task can
/// be hours apart — long enough for a frozen countdown to drift into the past
/// and render as nonsense. This only rebuilds the label; it costs no requests.
final bgClockProvider = StreamProvider<int>((ref) async* {
  var i = 0;
  yield i;
  while (true) {
    await Future<void>.delayed(const Duration(seconds: 10));
    yield ++i;
  }
});

List<Map<String, dynamic>> _maps(dynamic raw) => (raw is List ? raw : const [])
    .whereType<Map>()
    .map((e) => e.cast<String, dynamic>())
    .toList();

/// One page of tasks plus the unpaged total, so the UI can render a pager.
class BgTaskPage {
  final List<BackgroundTask> tasks;
  final int total;
  const BgTaskPage(this.tasks, this.total);
}

final bgTasksProvider = FutureProvider<BgTaskPage>((ref) async {
  ref.watch(bgRevProvider);
  final internal = ref.watch(bgShowInternalProvider);
  final status = ref.watch(bgStatusFilterProvider);
  final page = ref.watch(bgPageProvider);
  final r = await ref.read(apiClientProvider).get(
    '/api/background/tasks',
    query: {
      if (internal) 'include_internal': 'true',
      'status': ?status,
      'limit': bgPageSize,
      'offset': page * bgPageSize,
    },
  );
  final m = (r as Map?)?.cast<String, dynamic>() ?? const {};
  return BgTaskPage(
    _maps(m['tasks']).map(BackgroundTask.fromJson).toList(),
    (m['total'] as num?)?.toInt() ?? 0,
  );
});

final bgStatsProvider = FutureProvider<BackgroundStats>((ref) async {
  ref.watch(bgRevProvider);
  final window = ref.watch(bgWindowProvider);
  final r = await ref
      .read(apiClientProvider)
      .get('/api/background/stats', query: {'window': window});
  return BackgroundStats.fromJson((r as Map).cast<String, dynamic>());
});

/// Run history for one task.
final bgRunsProvider =
    FutureProvider.family<List<BackgroundRun>, String>((ref, taskId) async {
  ref.watch(bgRevProvider);
  final r = await ref
      .read(apiClientProvider)
      .get('/api/background/tasks/$taskId/runs', query: {'limit': 50});
  return _maps((r as Map?)?['runs']).map(BackgroundRun.fromJson).toList();
});

/// One background session: the run plus its transcript.
class BackgroundSession {
  final BackgroundRun run;
  final List<BackgroundActivity> activity;
  const BackgroundSession(this.run, this.activity);
}

final bgSessionProvider =
    FutureProvider.family<BackgroundSession, String>((ref, runId) async {
  ref.watch(bgRevProvider);
  final r = await ref.read(apiClientProvider).get('/api/background/runs/$runId');
  final m = (r as Map).cast<String, dynamic>();
  return BackgroundSession(
    BackgroundRun.fromJson((m['run'] as Map).cast<String, dynamic>()),
    _maps(m['activity']).map(BackgroundActivity.fromJson).toList(),
  );
});

/// Task ids with a run in flight right now, kept live off the WS stream.
///
/// The daemon is the only thing that knows a background run started — nothing
/// polls for it — so without this the list would sit still while work happens.
/// WS payloads are camelCase (REST is snake_case).
class BgLiveNotifier extends StateNotifier<Set<String>> {
  BgLiveNotifier(this._ref) : super({}) {
    _sub = _ref.read(wsClientProvider).events.listen(_onEvent);
  }
  final Ref _ref;
  Object? _sub;

  void _onEvent(Map<String, dynamic> e) {
    final type = '${e['type'] ?? ''}';
    if (!type.startsWith('bg:')) return;
    final taskId = '${e['taskId'] ?? ''}';
    switch (type) {
      case 'bg:run:started':
        if (taskId.isNotEmpty) state = {...state, taskId};
        break;
      case 'bg:run:finished':
        if (taskId.isNotEmpty) state = {...state}..remove(taskId);
        // A finished run changes history, stats and the attention band.
        _bump();
        break;
      case 'bg:task:changed':
        _bump();
        break;
    }
  }

  void _bump() => _ref.read(bgRevProvider.notifier).state++;

  @override
  void dispose() {
    (_sub as dynamic)?.cancel();
    super.dispose();
  }
}

final bgLiveProvider =
    StateNotifierProvider<BgLiveNotifier, Set<String>>((ref) => BgLiveNotifier(ref));

/// Live activity lines for the session currently open, so an in-flight run
/// streams instead of needing a refetch.
class BgLiveActivityNotifier extends StateNotifier<List<BackgroundActivity>> {
  BgLiveActivityNotifier(this._ref, this.runId) : super([]) {
    _sub = _ref.read(wsClientProvider).events.listen((e) {
      if ('${e['type'] ?? ''}' != 'bg:run:activity') return;
      if ('${e['runId'] ?? ''}' != runId) return;
      state = [
        ...state,
        BackgroundActivity(
          id: state.length,
          runId: runId,
          ts: DateTime.now().toIso8601String(),
          kind: '${e['kind'] ?? 'text'}',
          detail: '${e['detail'] ?? ''}',
        ),
      ];
    });
  }
  final Ref _ref;
  final String runId;
  Object? _sub;

  @override
  void dispose() {
    (_sub as dynamic)?.cancel();
    super.dispose();
  }
}

final bgLiveActivityProvider = StateNotifierProvider.family<
    BgLiveActivityNotifier, List<BackgroundActivity>, String>(
  (ref, runId) => BgLiveActivityNotifier(ref, runId),
);

/// Background task mutations. Bumps [bgRevProvider] to refresh lists.
class BackgroundApi {
  BackgroundApi(this._ref);
  final Ref _ref;

  void _bump() => _ref.read(bgRevProvider.notifier).state++;

  Future<Map<String, dynamic>> create(Map<String, dynamic> body) async {
    final r =
        await _ref.read(apiClientProvider).post('/api/background/tasks', body: body);
    _bump();
    return (r as Map?)?.cast<String, dynamic>() ?? const {};
  }

  /// Turn one line of natural language into a draft task spec via the daemon's
  /// LLM. Does not create anything — the caller reviews and then calls [create].
  Future<Map<String, dynamic>> parseQuick(String text) async {
    final r = await _ref
        .read(apiClientProvider)
        .post('/api/background/parse', body: {'text': text});
    return (r as Map?)?.cast<String, dynamic>() ?? const {};
  }

  Future<void> update(String id, Map<String, dynamic> body) async {
    await _ref
        .read(apiClientProvider)
        .patch('/api/background/tasks/$id', body: body);
    _bump();
  }

  Future<void> pause(String id) => update(id, {'status': 'paused'});

  /// Resuming also clears the failure counter daemon-side — otherwise a task
  /// the user deliberately un-paused would re-quarantine on its next failure.
  Future<void> resume(String id) => update(id, {'status': 'active'});

  Future<void> delete(String id) async {
    await _ref.read(apiClientProvider).delete('/api/background/tasks/$id');
    _bump();
  }

  /// Runs inline daemon-side and returns the run id, so the caller can open the
  /// session straight away.
  Future<String> runNow(String id) async {
    final r = await _ref
        .read(apiClientProvider)
        .post('/api/background/tasks/$id/run-now');
    _bump();
    return '${(r as Map?)?['run_id'] ?? ''}';
  }

  Future<void> cancelRun(String runId) async {
    await _ref
        .read(apiClientProvider)
        .post('/api/background/runs/$runId/cancel');
    _bump();
  }
}

final backgroundApiProvider = Provider<BackgroundApi>((ref) => BackgroundApi(ref));
