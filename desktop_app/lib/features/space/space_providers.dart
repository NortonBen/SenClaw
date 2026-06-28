import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/prefs.dart';
import '../../core/transport/connection.dart';
import '../../models/space_models.dart';

List<Map<String, dynamic>> _asMaps(dynamic r) =>
    (r is List ? r : const []).whereType<Map>().map((e) => e.cast<String, dynamic>()).toList();

/// Bumped to force note/event/schedule lists to refetch after a mutation.
final spaceRevProvider = StateProvider<int>((ref) => 0);

final notesProvider = FutureProvider<List<SpaceNote>>((ref) async {
  ref.watch(spaceRevProvider);
  final r = await ref.read(apiClientProvider).get('/api/space/notes');
  return _asMaps(r).map(SpaceNote.fromJson).toList();
});

final eventsProvider = FutureProvider<List<SpaceEvent>>((ref) async {
  ref.watch(spaceRevProvider);
  // The handler requires a [from,to] epoch-ms window.
  final now = DateTime.now();
  final from = now.subtract(const Duration(days: 90)).millisecondsSinceEpoch;
  final to = now.add(const Duration(days: 365)).millisecondsSinceEpoch;
  final r = await ref
      .read(apiClientProvider)
      .get('/api/space/calendar/events', query: {'from': from, 'to': to});
  return _asMaps(r).map(SpaceEvent.fromJson).toList();
});

final schedulesProvider = FutureProvider<List<SpaceSchedule>>((ref) async {
  ref.watch(spaceRevProvider);
  final r = await ref.read(apiClientProvider).get('/api/space/schedules');
  return _asMaps(r).map(SpaceSchedule.fromJson).toList();
});

class SpaceApp {
  final String id;
  final String name;
  final String icon;
  final String description;
  final String url;
  final bool enabled;
  /// Raw manifest ({permissions, mcp_servers, version, …}) for the details view.
  final Map<String, dynamic> manifest;
  const SpaceApp(this.id, this.name, this.icon, this.description, this.url,
      this.enabled,
      {this.manifest = const {}});

  /// Declared permission strings from the manifest, if any.
  List<String> get permissions =>
      ((manifest['permissions'] as List?) ?? const [])
          .map((e) => '$e')
          .where((s) => s.isNotEmpty)
          .toList();

  /// Declared MCP server names from the manifest, if any.
  List<String> get mcpServers {
    final raw = manifest['mcp_servers'] ?? manifest['mcpServers'];
    if (raw is List) return raw.map((e) => '$e').toList();
    if (raw is Map) return raw.keys.map((e) => '$e').toList();
    return const [];
  }

  String get version => '${manifest['version'] ?? ''}';
}

/// Which Space apps are "running" (their web view stays mounted, Android-style)
/// and which one is currently shown. Held globally so apps keep running while
/// the user navigates to Chat and back.
class RunningAppsState {
  const RunningAppsState(this.running, this.activeId);
  final List<SpaceApp> running;
  final String? activeId;
  bool isRunning(String id) => running.any((a) => a.id == id);
  SpaceApp? get active =>
      activeId == null ? null : running.where((a) => a.id == activeId).firstOrNull;
}

class RunningAppsController extends StateNotifier<RunningAppsState> {
  RunningAppsController() : super(const RunningAppsState([], null));

  /// Launch (or focus) an app and show it.
  void open(SpaceApp a) {
    final list = state.isRunning(a.id) ? state.running : [...state.running, a];
    state = RunningAppsState(list, a.id);
  }

  /// Minimize: keep every app running, show the launcher.
  void background() => state = RunningAppsState(state.running, null);

  /// Terminate one app (unmount its web view).
  void close(String id) {
    state = RunningAppsState(
      state.running.where((a) => a.id != id).toList(),
      state.activeId == id ? null : state.activeId,
    );
  }
}

final runningAppsProvider =
    StateNotifierProvider<RunningAppsController, RunningAppsState>(
        (ref) => RunningAppsController());

/// App ids the user pinned to the Dashboard for quick launch (persisted).
class PinnedAppsNotifier extends StateNotifier<Set<String>> {
  PinnedAppsNotifier(this._ref) : super({}) {
    state = _ref.read(prefsHelperProvider).stringSet(kPinnedAppsKey);
  }
  final Ref _ref;

  void toggle(String id) {
    final next = {...state};
    next.contains(id) ? next.remove(id) : next.add(id);
    state = next;
    _ref.read(prefsHelperProvider).setStringSet(kPinnedAppsKey, next);
  }
}

final pinnedAppsProvider =
    StateNotifierProvider<PinnedAppsNotifier, Set<String>>(
        (ref) => PinnedAppsNotifier(ref));

/// Installed Space apps with their resolved iframe URL (mirrors web SpacePage).
final spaceAppsProvider = FutureProvider<List<SpaceApp>>((ref) async {
  ref.watch(spaceRevProvider);
  final base = ref.read(appConfigProvider).httpBase;
  final r = await ref.read(apiClientProvider).get('/api/space/apps');
  return _asMaps(r).map((row) {
    final m = (row['manifest'] as Map?)?.cast<String, dynamic>() ?? const {};
    final integ = (m['integration'] as Map?)?.cast<String, dynamic>() ??
        {'type': 'iframe', 'url': m['url'] ?? '/'};
    final runtimeUrl =
        ((m['runtime'] as Map?)?.cast<String, dynamic>())?['url'] as String?;
    final integUrl = '${integ['url'] ?? '/'}';
    var url = runtimeUrl != null
        ? '${runtimeUrl.replaceAll(RegExp(r'/$'), '')}$integUrl'
        : integUrl;
    // Resolve a relative URL against the daemon, with a proxy fallback.
    if (url.startsWith('/')) {
      url = '$base$url';
    } else if (!url.startsWith('http')) {
      url = '$base/api/space/apps/${row['id']}/proxy/';
    }
    return SpaceApp(
      '${row['id']}',
      '${m['name'] ?? row['id']}',
      '${m['icon'] ?? '🧩'}',
      '${m['description'] ?? ''}',
      url,
      row['enabled'] != false,
      manifest: m,
    );
  }).toList();
});

/// Space mutations (notes CRUD). Bumps [spaceRevProvider] to refresh lists.
class SpaceApi {
  SpaceApi(this._ref);
  final Ref _ref;

  void _bump() =>
      _ref.read(spaceRevProvider.notifier).state++;

  Future<void> createNote(String title, String body, List<String> tags) async {
    await _ref.read(apiClientProvider).post('/api/space/notes',
        body: {'title': title, 'body': body, 'tags': tags});
    _bump();
  }

  Future<void> updateNote(
      String id, String title, String body, List<String> tags) async {
    await _ref.read(apiClientProvider).put('/api/space/notes/$id',
        body: {'title': title, 'body': body, 'tags': tags});
    _bump();
  }

  Future<void> deleteNote(String id) async {
    await _ref.read(apiClientProvider).delete('/api/space/notes/$id');
    _bump();
  }

  Future<void> togglePin(SpaceNote n) async {
    await _ref.read(apiClientProvider).put('/api/space/notes/${n.id}', body: {
      'title': n.title,
      'body': n.body,
      'tags': n.tags,
      'pinned': !n.pinned,
    });
    _bump();
  }

  Future<void> createEvent({
    required String title,
    required int startAt,
    required int endAt,
    bool allDay = false,
    String? description,
    String? location,
  }) async {
    await _ref.read(apiClientProvider).post('/api/space/calendar/events', body: {
      'title': title,
      'start_at': startAt,
      'end_at': endAt,
      'all_day': allDay,
      if (description != null && description.isNotEmpty)
        'description': description,
      if (location != null && location.isNotEmpty) 'location': location,
    });
    _bump();
  }

  Future<void> updateEvent({
    required String id,
    required String title,
    required int startAt,
    required int endAt,
    bool allDay = false,
    String? description,
    String? location,
  }) async {
    await _ref
        .read(apiClientProvider)
        .put('/api/space/calendar/events/$id', body: {
      'title': title,
      'start_at': startAt,
      'end_at': endAt,
      'all_day': allDay,
      'description': description ?? '',
      'location': location ?? '',
    });
    _bump();
  }

  Future<void> deleteEvent(String id) async {
    await _ref
        .read(apiClientProvider)
        .delete('/api/space/calendar/events/$id');
    _bump();
  }

  Future<void> runSchedule(String id) =>
      _ref.read(apiClientProvider).post('/api/space/schedules/$id/run-now');

  Future<void> createSchedule(Map<String, dynamic> body) async {
    await _ref.read(apiClientProvider).post('/api/space/schedules', body: body);
    _bump();
  }

  Future<void> updateSchedule(String id, Map<String, dynamic> body) async {
    await _ref
        .read(apiClientProvider)
        .patch('/api/space/schedules/$id', body: body);
    _bump();
  }

  Future<void> deleteSchedule(String id) async {
    await _ref.read(apiClientProvider).delete('/api/space/schedules/$id');
    _bump();
  }
}

final spaceApiProvider = Provider<SpaceApi>((ref) => SpaceApi(ref));
