import 'dart:convert';

/// Parse tags that may arrive as a JSON array OR a JSON-encoded string
/// (the daemon returns both shapes across endpoints).
List<String> _parseTags(dynamic raw) {
  if (raw is List) return raw.map((e) => '$e').toList();
  if (raw is String && raw.isNotEmpty) {
    try {
      final d = jsonDecode(raw);
      if (d is List) return d.map((e) => '$e').toList();
    } catch (_) {}
  }
  return const [];
}

class SpaceNote {
  final String id;
  final String title;
  final String body;
  final List<String> tags;
  final bool pinned;
  final int createdAt;
  final int updatedAt;

  const SpaceNote({
    required this.id,
    required this.title,
    this.body = '',
    this.tags = const [],
    this.pinned = false,
    this.createdAt = 0,
    this.updatedAt = 0,
  });

  factory SpaceNote.fromJson(Map<String, dynamic> j) => SpaceNote(
    id: '${j['id'] ?? ''}',
    title: '${j['title'] ?? ''}',
    body: '${j['body'] ?? ''}',
    tags: _parseTags(j['tags']),
    pinned: j['pinned'] as bool? ?? false,
    createdAt: (j['created_at'] as num?)?.toInt() ?? 0,
    updatedAt: (j['updated_at'] as num?)?.toInt() ?? 0,
  );
}

class SpaceEvent {
  final String id;
  final String title;
  final String? description;
  final int startAt;
  final int endAt;
  final bool allDay;
  final String? location;

  const SpaceEvent({
    required this.id,
    required this.title,
    this.description,
    this.startAt = 0,
    this.endAt = 0,
    this.allDay = false,
    this.location,
  });

  factory SpaceEvent.fromJson(Map<String, dynamic> j) => SpaceEvent(
    id: '${j['id'] ?? ''}',
    title: '${j['title'] ?? ''}',
    description: j['description'] as String?,
    startAt: (j['start_at'] as num?)?.toInt() ?? 0,
    endAt: (j['end_at'] as num?)?.toInt() ?? 0,
    allDay: j['all_day'] as bool? ?? false,
    location: j['location'] as String?,
  );

  DateTime get start => DateTime.fromMillisecondsSinceEpoch(startAt);
}

class SpaceSchedule {
  final String id;
  final String label;
  final String prompt;
  final String groupFolder;
  final String agentMode;
  final String scheduleValue;
  final String status;
  final String? nextRun;
  final String? lastRun;
  final String? lastStatus;

  const SpaceSchedule({
    required this.id,
    required this.label,
    this.prompt = '',
    this.groupFolder = '',
    this.agentMode = '',
    this.scheduleValue = '',
    this.status = 'active',
    this.nextRun,
    this.lastRun,
    this.lastStatus,
  });

  factory SpaceSchedule.fromJson(Map<String, dynamic> j) => SpaceSchedule(
    id: '${j['id'] ?? ''}',
    label: '${j['label'] ?? j['prompt'] ?? '(schedule)'}',
    prompt: '${j['prompt'] ?? j['label'] ?? ''}',
    groupFolder: '${j['group_folder'] ?? ''}',
    agentMode: '${j['agent_mode'] ?? ''}',
    scheduleValue: '${j['schedule_value'] ?? ''}',
    status: '${j['status'] ?? 'active'}',
    nextRun: j['next_run'] as String?,
    lastRun: j['last_run'] as String?,
    lastStatus: j['last_status'] as String?,
  );
}
