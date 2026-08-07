import 'dart:convert';

import '../core/i18n/l10n.dart';

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

  /// Internal Space-App route this event opens, e.g.
  /// `/space/app/study?session=abc`. The daemon refuses to store anything that
  /// is not a `/space/app/…` path, so it is safe to open in the embedded view.
  final String? link;
  final String? appId;

  const SpaceEvent({
    required this.id,
    required this.title,
    this.description,
    this.startAt = 0,
    this.endAt = 0,
    this.allDay = false,
    this.location,
    this.link,
    this.appId,
  });

  factory SpaceEvent.fromJson(Map<String, dynamic> j) => SpaceEvent(
    id: '${j['id'] ?? ''}',
    title: '${j['title'] ?? ''}',
    description: j['description'] as String?,
    startAt: (j['start_at'] as num?)?.toInt() ?? 0,
    endAt: (j['end_at'] as num?)?.toInt() ?? 0,
    allDay: j['all_day'] as bool? ?? false,
    location: j['location'] as String?,
    link: (j['link'] as String?)?.trim().isEmpty ?? true
        ? null
        : j['link'] as String?,
    appId: j['app_id'] as String?,
  );

  /// App id the link points at, derived from the route when `app_id` is absent.
  String? get linkAppId {
    if (appId != null && appId!.isNotEmpty) return appId;
    final l = link;
    if (l == null || !l.startsWith('/space/app/')) return null;
    final rest = l.substring('/space/app/'.length);
    final id = rest.split(RegExp(r'[/?#]')).first;
    return id.isEmpty ? null : id;
  }

  DateTime get start => DateTime.fromMillisecondsSinceEpoch(startAt);
}

class SpaceSchedule {
  final String id;
  final String label;
  final String prompt;
  final String groupFolder;
  final String agentMode;
  /// Agent profile the schedule runs under. Null when it still runs under its
  /// own bare `schedule_<id>` folder (no profile picked).
  final String? agentFolder;

  /// LLM config the schedule runs under. Null = the active default.
  final String? modelId;
  final String scheduleType;
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
    this.agentFolder,
    this.modelId,
    this.scheduleType = 'cron',
    this.scheduleValue = '',
    this.status = 'active',
    this.nextRun,
    this.lastRun,
    this.lastStatus,
  });

  factory SpaceSchedule.fromJson(Map<String, dynamic> j) => SpaceSchedule(
    id: '${j['id'] ?? ''}',
    // Display-only fallback for a schedule the daemon stored with neither a
    // label nor a prompt.
    label: '${j['label'] ?? j['prompt'] ?? L10n.global.t('(schedule)')}',
    prompt: '${j['prompt'] ?? j['label'] ?? ''}',
    groupFolder: '${j['group_folder'] ?? ''}',
    agentMode: '${j['agent_mode'] ?? ''}',
    agentFolder: j['agent_folder'] as String?,
    modelId: j['model_id'] as String?,
    scheduleType: '${j['schedule_type'] ?? 'cron'}',
    scheduleValue: '${j['schedule_value'] ?? ''}',
    status: '${j['status'] ?? 'active'}',
    nextRun: j['next_run'] as String?,
    lastRun: j['last_run'] as String?,
    lastStatus: j['last_status'] as String?,
  );
}
