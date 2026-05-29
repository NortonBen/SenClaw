// Models mirroring `/api/space/*` JSON shapes (src/gateway/ui_server/space.rs).
// Notes and events list endpoints return *bare JSON arrays* (not wrapped).

class SpaceNote {
  final String id;
  final String title;
  final String body;
  final List<String> tags;
  final String? folderId;
  final bool pinned;
  final int createdAt;
  final int updatedAt;

  const SpaceNote({
    required this.id,
    required this.title,
    required this.body,
    this.tags = const [],
    this.folderId,
    this.pinned = false,
    this.createdAt = 0,
    this.updatedAt = 0,
  });

  factory SpaceNote.fromJson(Map<String, dynamic> j) => SpaceNote(
    id: (j['id'] ?? '').toString(),
    title: (j['title'] ?? '').toString(),
    body: (j['body'] ?? '').toString(),
    tags: ((j['tags'] as List?) ?? const [])
        .map((e) => e.toString())
        .toList(),
    folderId: j['folder_id'] as String?,
    pinned: j['pinned'] as bool? ?? false,
    createdAt: (j['created_at'] as num?)?.toInt() ?? 0,
    updatedAt: (j['updated_at'] as num?)?.toInt() ?? 0,
  );
}

class SpaceEvent {
  final String id;
  final String title;
  final String? description;
  final int startAt; // epoch ms
  final int endAt; // epoch ms
  final bool allDay;
  final String? location;
  final String? color;
  final int? reminderMin;
  final String source;
  final String status;
  final int? renotifyMin;

  const SpaceEvent({
    required this.id,
    required this.title,
    this.description,
    required this.startAt,
    required this.endAt,
    this.allDay = false,
    this.location,
    this.color,
    this.reminderMin,
    this.source = '',
    this.status = 'upcoming',
    this.renotifyMin,
  });

  factory SpaceEvent.fromJson(Map<String, dynamic> j) => SpaceEvent(
    id: (j['id'] ?? '').toString(),
    title: (j['title'] ?? '').toString(),
    description: j['description'] as String?,
    startAt: (j['start_at'] as num?)?.toInt() ?? 0,
    endAt: (j['end_at'] as num?)?.toInt() ?? 0,
    allDay: j['all_day'] as bool? ?? false,
    location: j['location'] as String?,
    color: j['color'] as String?,
    reminderMin: (j['reminder_min'] as num?)?.toInt(),
    source: (j['source'] ?? '').toString(),
    status: (j['status'] ?? 'upcoming').toString(),
    renotifyMin: (j['renotify_min'] as num?)?.toInt(),
  );

  DateTime get start => DateTime.fromMillisecondsSinceEpoch(startAt);
  DateTime get end => DateTime.fromMillisecondsSinceEpoch(endAt);
}

/// A scheduled task (cron/interval/once). The Rust `ScheduledTask` struct has
/// NO serde rename, so keys are snake_case.
class SpaceSchedule {
  final String id;
  final String groupFolder;
  final String prompt;
  final String scheduleType; // 'cron' | 'interval' | 'once'
  final String scheduleValue;
  final String status;
  final String? nextRun;
  final String? lastRun;

  const SpaceSchedule({
    required this.id,
    required this.groupFolder,
    required this.prompt,
    required this.scheduleType,
    required this.scheduleValue,
    required this.status,
    this.nextRun,
    this.lastRun,
  });

  factory SpaceSchedule.fromJson(Map<String, dynamic> j) => SpaceSchedule(
    id: (j['id'] ?? '').toString(),
    groupFolder: (j['group_folder'] ?? '').toString(),
    prompt: (j['prompt'] ?? '').toString(),
    scheduleType: (j['schedule_type'] ?? '').toString(),
    scheduleValue: (j['schedule_value'] ?? '').toString(),
    status: (j['status'] ?? '').toString(),
    nextRun: j['next_run'] as String?,
    lastRun: j['last_run'] as String?,
  );
}
