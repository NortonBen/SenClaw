import 'dart:convert';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/prefs.dart';
import '../../core/transport/connection.dart';
import '../../models/space_models.dart';

List<Map<String, dynamic>> _asMaps(dynamic r) =>
    (r is List ? r : const []).whereType<Map>().map((e) => e.cast<String, dynamic>()).toList();

/// A widget definition declared in a Space App's manifest.
class AppWidgetDef {
  final String id;
  final String name;
  final String description;
  final String size; // 'small' | 'medium' | 'large'
  final String entryUrl;
  const AppWidgetDef({
    required this.id,
    required this.name,
    this.description = '',
    this.size = 'small',
    this.entryUrl = '/',
  });
}

/// A widget placed on the dashboard (persisted in prefs).
class PlacedWidget {
  final String appId;
  final String widgetId;

  /// User size override ('small' | 'medium' | 'large'); null = use the manifest
  /// default declared by the widget.
  final String? sizeOverride;
  const PlacedWidget(this.appId, this.widgetId, {this.sizeOverride});

  String get key => '$appId:$widgetId';

  PlacedWidget copyWith({String? sizeOverride}) =>
      PlacedWidget(appId, widgetId, sizeOverride: sizeOverride ?? this.sizeOverride);

  Map<String, dynamic> toJson() => {
        'appId': appId,
        'widgetId': widgetId,
        if (sizeOverride != null) 'size': sizeOverride,
      };
  factory PlacedWidget.fromJson(Map<String, dynamic> j) => PlacedWidget(
        j['appId'] as String? ?? '',
        j['widgetId'] as String? ?? '',
        sizeOverride: j['size'] as String?,
      );
}

const _kDashboardWidgetsKey = 'senclaw:dashboard-widgets';

class DashboardWidgetsNotifier extends StateNotifier<List<PlacedWidget>> {
  DashboardWidgetsNotifier(this._ref) : super(_load(_ref));
  final Ref _ref;

  static List<PlacedWidget> _load(Ref ref) {
    final raw = ref.read(prefsHelperProvider).string(_kDashboardWidgetsKey, '');
    if (raw.isEmpty) return [];
    try {
      final list = (jsonDecode(raw) as List).cast<Map<String, dynamic>>();
      return list.map(PlacedWidget.fromJson).toList();
    } catch (_) {
      return [];
    }
  }

  void _save() {
    _ref.read(prefsHelperProvider).setString(
      _kDashboardWidgetsKey,
      jsonEncode(state.map((w) => w.toJson()).toList()),
    );
  }

  void add(PlacedWidget w) {
    state = [...state, w];
    _save();
  }

  void remove(int index) {
    final next = [...state];
    if (index < next.length) next.removeAt(index);
    state = next;
    _save();
  }

  /// Move the widget at [oldIndex] to [newIndex] (ReorderableListView semantics).
  void reorder(int oldIndex, int newIndex) {
    final next = [...state];
    if (oldIndex < 0 || oldIndex >= next.length) return;
    if (newIndex > oldIndex) newIndex -= 1;
    newIndex = newIndex.clamp(0, next.length - 1);
    final item = next.removeAt(oldIndex);
    next.insert(newIndex, item);
    state = next;
    _save();
  }

  /// Override the size of the widget at [index].
  void setSize(int index, String size) {
    final next = [...state];
    if (index < 0 || index >= next.length) return;
    next[index] = PlacedWidget(next[index].appId, next[index].widgetId,
        sizeOverride: size);
    state = next;
    _save();
  }
}

final dashboardWidgetsProvider =
    StateNotifierProvider<DashboardWidgetsNotifier, List<PlacedWidget>>(
        (ref) => DashboardWidgetsNotifier(ref));

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

  /// Widget definitions declared in the manifest.
  List<AppWidgetDef> get widgets {
    final raw = manifest['widgets'];
    if (raw is! List) return const [];
    return raw.whereType<Map>().map((w) {
      return AppWidgetDef(
        id: '${w['id'] ?? ''}',
        name: '${w['name'] ?? ''}',
        description: '${w['description'] ?? ''}',
        size: '${w['size'] ?? 'small'}',
        entryUrl: '${w['entryUrl'] ?? '/'}',
      );
    }).toList();
  }
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

  /// Launch an app at a specific internal route — the calendar's "open this
  /// event" path (`/space/app/<id>?session=…`).
  ///
  /// An app already running at a different route is torn down and relaunched:
  /// the web view is keyed by app id, so mutating the URL in place would leave
  /// the old page mounted and the user staring at yesterday's lesson.
  void openAt(SpaceApp a, String route) {
    final query = _queryOf(route);
    final url = query.isEmpty
        ? a.url
        : '${a.url}${a.url.contains('?') ? '&' : '?'}$query';
    final relaunched = SpaceApp(a.id, a.name, a.icon, a.description, url,
        a.enabled,
        manifest: a.manifest);
    final list = [
      ...state.running.where((x) => x.id != a.id),
      relaunched,
    ];
    state = RunningAppsState(list, a.id);
  }

  /// Query string of an internal `/space/app/<id>?…` route, without the `?`.
  static String _queryOf(String route) {
    final i = route.indexOf('?');
    if (i < 0) return '';
    return route.substring(i + 1).split('#').first;
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
  final cfg = ref.read(appConfigProvider);
  final base = cfg.httpBase;
  final r = await ref.read(apiClientProvider).get('/api/space/apps');
  return _asMaps(r).map((row) {
    final m = (row['manifest'] as Map?)?.cast<String, dynamic>() ?? const {};
    final integ = (m['integration'] as Map?)?.cast<String, dynamic>() ??
        {'type': 'iframe', 'url': m['url'] ?? '/'};
    final runtime = (m['runtime'] as Map?)?.cast<String, dynamic>();
    final runtimeUrl = runtime?['url'] as String?;
    final runtimePort = (runtime?['port'] as num?)?.toInt();
    final isServer = runtime?['kind'] == 'server';
    final integUrl = '${integ['url'] ?? '/'}';
    final path = integUrl.startsWith('/') ? integUrl : '/$integUrl';
    var url = runtimeUrl != null
        ? '${runtimeUrl.replaceAll(RegExp(r'/$'), '')}$integUrl'
        : integUrl;
    if (runtimeUrl == null && isServer && runtimePort != null) {
      // Server apps run as a local process on their OWN port. The desktop
      // always talks to the daemon on localhost, so reach the app DIRECTLY at
      // its origin — NOT the daemon root ('$base/', which is SenClaw's own UI)
      // and NOT the daemon's HTTP reverse-proxy. Direct access is also the only
      // thing that makes an app's own WebSocket work (e.g. mini-browser's live
      // view): the reqwest-based /proxy/ endpoint can't tunnel a WS upgrade.
      url = 'http://${cfg.host}:$runtimePort$path';
    } else if (url.startsWith('/')) {
      // Static/esm apps served by the daemon; '/' means the app root → proxy.
      url = url == '/'
          ? '$base/api/space/apps/${row['id']}/proxy/'
          : '$base$url';
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

  /// Partial pin update — sends only `pinned` (the backend PUT is a partial
  /// update). Safe to call while the inline editor has unsaved body edits,
  /// since it never re-sends the (possibly stale) body.
  Future<void> setPinned(String id, bool pinned) async {
    await _ref
        .read(apiClientProvider)
        .put('/api/space/notes/$id', body: {'pinned': pinned});
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
