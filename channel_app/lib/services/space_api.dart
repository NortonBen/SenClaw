import '../models/space_models.dart';
import 'api_client.dart';
import 'local_cache.dart';

/// Typed wrapper over `/api/space/*` (Notes + Calendar), tunnelled through the
/// relay. Mirrors web/src/hooks/useSpace.ts. List fetches feed the
/// [LocalCache] domain tables for instant cache-first rendering.
class SpaceApi {
  final _api = ApiClient();

  // ── Notes ──────────────────────────────────────────────────────────────
  Future<List<SpaceNote>> listNotes({String? tag}) async {
    final list = await _api.getList(
      ApiClient.withQuery('/api/space/notes', {'tag': tag}),
    );
    final maps = jsonMaps(list);
    // Only the unfiltered list is cached — tag views are cheap derivatives.
    if (tag == null || tag.isEmpty) {
      LocalCache().putDomainList('notes', maps);
    }
    return maps.map(SpaceNote.fromJson).toList();
  }

  Future<List<SpaceNote>> listNotesCached() async =>
      (await LocalCache().getDomainList('notes'))
          .map(SpaceNote.fromJson)
          .toList();

  Future<List<SpaceNote>> searchNotes(String q) async {
    final list = await _api.getList(
      ApiClient.withQuery('/api/space/notes/search', {'q': q}),
    );
    return list
        .map((e) => SpaceNote.fromJson(e as Map<String, dynamic>))
        .toList();
  }

  Future<void> createNote({
    required String title,
    required String body,
    List<String> tags = const [],
  }) =>
      _api.post('/api/space/notes',
          body: {'title': title, 'body': body, 'tags': tags});

  Future<void> updateNote(
    String id, {
    String? title,
    String? body,
    List<String>? tags,
  }) =>
      _api.put('/api/space/notes/$id', body: {
        'title': ?title,
        'body': ?body,
        'tags': ?tags,
      });

  Future<void> deleteNote(String id) => _api.delete('/api/space/notes/$id');

  // ── Calendar ───────────────────────────────────────────────────────────
  /// [cache] should be set ONLY by the wide-window (±1y) fetch — the cache is
  /// full-replace, so a narrow-window fetch would clobber it with a subset.
  Future<List<SpaceEvent>> listEvents({
    required int from,
    required int to,
    bool cache = false,
  }) async {
    final list = await _api.getList(
      ApiClient.withQuery('/api/space/calendar/events', {
        'from': from,
        'to': to,
      }),
    );
    final maps = jsonMaps(list);
    if (cache) {
      LocalCache().putDomainList('calendar_events', maps);
    }
    return maps.map(SpaceEvent.fromJson).toList();
  }

  Future<List<SpaceEvent>> listEventsCached() async =>
      (await LocalCache().getDomainList('calendar_events'))
          .map(SpaceEvent.fromJson)
          .toList();

  Future<void> createEvent({
    required String title,
    required int startAt,
    required int endAt,
    bool allDay = false,
    String? description,
    String? location,
    int? reminderMin,
  }) =>
      _api.post('/api/space/calendar/events', body: {
        'title': title,
        'start_at': startAt,
        'end_at': endAt,
        'all_day': allDay,
        if (description != null && description.isNotEmpty)
          'description': description,
        if (location != null && location.isNotEmpty) 'location': location,
        'reminder_min': ?reminderMin,
      });

  Future<void> deleteEvent(String id) =>
      _api.delete('/api/space/calendar/events/$id');

  // ── Schedules ──────────────────────────────────────────────────────────
  Future<List<SpaceSchedule>> listSchedules(String groupFolder) async {
    final list = await _api.getList(
      ApiClient.withQuery('/api/space/schedules', {'group': groupFolder}),
    );
    final maps = jsonMaps(list);
    LocalCache().putDomainList('schedules', maps, scope: groupFolder);
    return maps.map(SpaceSchedule.fromJson).toList();
  }

  Future<List<SpaceSchedule>> listSchedulesCached(String groupFolder) async =>
      (await LocalCache().getDomainList('schedules', scope: groupFolder))
          .map(SpaceSchedule.fromJson)
          .toList();

  Future<void> createSchedule({
    required String prompt,
    required String cron,
    required String groupFolder,
    required String chatJid,
  }) =>
      _api.post('/api/space/schedules', body: {
        'prompt': prompt,
        'cron': cron,
        'group_folder': groupFolder,
        'chat_jid': chatJid,
      });

  /// Cancel needs the owning group_folder in the request body.
  Future<void> cancelSchedule(String id, String groupFolder) =>
      _api.delete('/api/space/schedules/$id',
          body: {'group_folder': groupFolder});

  // ── Email ──────────────────────────────────────────────────────────────
  Future<List<SpaceEmailAccount>> listEmailAccounts() async {
    final list = await _api.getList('/api/space/email/accounts');
    return list
        .map((e) => SpaceEmailAccount.fromJson(e as Map<String, dynamic>))
        .toList();
  }

  Future<void> addEmailAccount(Map<String, dynamic> account) =>
      _api.post('/api/space/email/accounts', body: account);

  Future<void> deleteEmailAccount(String id) =>
      _api.delete('/api/space/email/accounts/$id');

  Future<List<SpaceEmail>> inbox({String? accountId}) async {
    final list = await _api.getList(
      ApiClient.withQuery('/api/space/email/inbox', {'account_id': accountId}),
    );
    return list
        .map((e) => SpaceEmail.fromJson(e as Map<String, dynamic>))
        .toList();
  }

  Future<SpaceEmail> readEmail(String id) async {
    final obj = await _api.getObject('/api/space/email/messages/$id');
    return SpaceEmail.fromJson(obj);
  }

  Future<void> sendEmail({
    required String to,
    required String subject,
    required String body,
    String? accountId,
  }) =>
      _api.post('/api/space/email/send', body: {
        'to': to,
        'subject': subject,
        'body': body,
        if (accountId != null && accountId.isNotEmpty) 'account_id': accountId,
      });

  // ── Apps ─────────────────────────────────────────────────────────────────
  Future<List<SpaceApp>> listApps() async {
    final maps = jsonMaps(await _api.get('/api/space/apps'));
    LocalCache().putDomainList('space_apps', maps);
    return maps.map(SpaceApp.fromJson).toList();
  }

  Future<List<SpaceApp>> listAppsCached() async =>
      (await LocalCache().getDomainList('space_apps'))
          .map(SpaceApp.fromJson)
          .toList();

  Future<void> registerApp(String manifestUrl) =>
      _api.post('/api/space/apps/register', body: {'manifest_url': manifestUrl});

  Future<void> deleteApp(String id) => _api.delete('/api/space/apps/$id');

  Future<void> restartApp(String id) =>
      _api.post('/api/space/apps/$id/restart');

  Future<String> appLogs(String id) async {
    final obj = await _api.getObject('/api/space/apps/$id/logs');
    return (obj['content'] ?? '').toString();
  }
}
