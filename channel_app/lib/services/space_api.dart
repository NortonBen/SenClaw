import '../models/space_models.dart';
import 'api_client.dart';

/// Typed wrapper over `/api/space/*` (Notes + Calendar), tunnelled through the
/// relay. Mirrors web/src/hooks/useSpace.ts.
class SpaceApi {
  final _api = ApiClient();

  // ── Notes ──────────────────────────────────────────────────────────────
  Future<List<SpaceNote>> listNotes({String? tag}) async {
    final list = await _api.getList(
      ApiClient.withQuery('/api/space/notes', {'tag': tag}),
    );
    return list
        .map((e) => SpaceNote.fromJson(e as Map<String, dynamic>))
        .toList();
  }

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
        if (title != null) 'title': title,
        if (body != null) 'body': body,
        if (tags != null) 'tags': tags,
      });

  Future<void> deleteNote(String id) => _api.delete('/api/space/notes/$id');

  // ── Calendar ───────────────────────────────────────────────────────────
  Future<List<SpaceEvent>> listEvents({
    required int from,
    required int to,
  }) async {
    final list = await _api.getList(
      ApiClient.withQuery('/api/space/calendar/events', {
        'from': from,
        'to': to,
      }),
    );
    return list
        .map((e) => SpaceEvent.fromJson(e as Map<String, dynamic>))
        .toList();
  }

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
        if (reminderMin != null) 'reminder_min': reminderMin,
      });

  Future<void> deleteEvent(String id) =>
      _api.delete('/api/space/calendar/events/$id');

  // ── Schedules ──────────────────────────────────────────────────────────
  Future<List<SpaceSchedule>> listSchedules(String groupFolder) async {
    final list = await _api.getList(
      ApiClient.withQuery('/api/space/schedules', {'group': groupFolder}),
    );
    return list
        .map((e) => SpaceSchedule.fromJson(e as Map<String, dynamic>))
        .toList();
  }

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
}
