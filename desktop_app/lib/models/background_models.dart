/// Background tasks — autonomous work the daemon runs by itself.
///
/// Distinct from [SpaceSchedule] in `space_models.dart`: a schedule runs inside
/// a chat session and replies to the user, a background task runs unattended
/// and writes to a run record nobody has to read. See
/// `docs/background-tasks-design.md`.
///
/// The daemon's REST surface is snake_case (WS is camelCase — see
/// `background_providers.dart` for the live-event side).
library;

class BackgroundTask {
  final String id;
  final String ownerKind; // 'system' | 'app' | 'user'
  final String ownerId;
  final String ownerKey;
  final String title;
  final String? description;
  final String jobKind; // 'prompt' | 'native'
  final String? nativeJob;
  final String promptKind; // 'static' | 'template' | 'generator'
  final String? prompt;
  final String? contextUrl;
  final String? persona;
  final List<String> useTools;
  final String? modelId;
  final int? maxTurns;
  final int? timeoutSecs;
  final String continuity; // 'fresh' | 'thread'
  final String triggerType; // 'cron' | 'interval' | 'once' | 'on_install' | 'manual'
  final String? triggerValue;
  final String? nextRun;
  final String? lastRun;
  final String overlapPolicy;
  final bool catchUp;
  final int maxFailures;
  final int consecutiveFailures;
  final String visibility; // 'normal' | 'internal'
  /// Deliver an OS notification instead of running an agent.
  final bool notify;
  final String status; // active | paused | completed | failed | cancelled

  const BackgroundTask({
    required this.id,
    required this.ownerKind,
    required this.ownerId,
    required this.ownerKey,
    required this.title,
    this.description,
    this.jobKind = 'prompt',
    this.nativeJob,
    this.promptKind = 'static',
    this.prompt,
    this.contextUrl,
    this.persona,
    this.useTools = const [],
    this.modelId,
    this.maxTurns,
    this.timeoutSecs,
    this.continuity = 'fresh',
    this.triggerType = 'cron',
    this.triggerValue,
    this.nextRun,
    this.lastRun,
    this.overlapPolicy = 'skip',
    this.catchUp = false,
    this.maxFailures = 5,
    this.consecutiveFailures = 0,
    this.visibility = 'normal',
    this.notify = false,
    this.status = 'active',
  });

  factory BackgroundTask.fromJson(Map<String, dynamic> j) => BackgroundTask(
        id: '${j['id'] ?? ''}',
        ownerKind: '${j['owner_kind'] ?? 'user'}',
        ownerId: '${j['owner_id'] ?? ''}',
        ownerKey: '${j['owner_key'] ?? ''}',
        title: '${j['title'] ?? ''}',
        description: j['description'] as String?,
        jobKind: '${j['job_kind'] ?? 'prompt'}',
        nativeJob: j['native_job'] as String?,
        promptKind: '${j['prompt_kind'] ?? 'static'}',
        prompt: j['prompt'] as String?,
        contextUrl: j['context_url'] as String?,
        persona: j['persona'] as String?,
        useTools:
            (j['use_tools'] as List?)?.map((e) => '$e').toList() ?? const [],
        modelId: j['model_id'] as String?,
        maxTurns: (j['max_turns'] as num?)?.toInt(),
        timeoutSecs: (j['timeout_secs'] as num?)?.toInt(),
        continuity: '${j['continuity'] ?? 'fresh'}',
        triggerType: '${j['trigger_type'] ?? 'cron'}',
        triggerValue: j['trigger_value'] as String?,
        nextRun: j['next_run'] as String?,
        lastRun: j['last_run'] as String?,
        overlapPolicy: '${j['overlap_policy'] ?? 'skip'}',
        catchUp: j['catch_up'] as bool? ?? false,
        maxFailures: (j['max_failures'] as num?)?.toInt() ?? 5,
        consecutiveFailures: (j['consecutive_failures'] as num?)?.toInt() ?? 0,
        visibility: '${j['visibility'] ?? 'normal'}',
        notify: j['notify'] as bool? ?? false,
        status: '${j['status'] ?? 'active'}',
      );

  /// Only user-owned tasks are editable. An app's config lives in its manifest
  /// (an edit would be reverted by a reinstall) and a native job's body is Rust
  /// — both can still be paused.
  bool get isEditable => ownerKind == 'user';

  bool get isNative => jobKind == 'native';

  /// Human label for the owner badge.
  String get ownerLabel => switch (ownerKind) {
        'system' => 'System',
        'app' => ownerId,
        _ => 'You',
      };

  /// The trigger in prose. A cron expression tells the user nothing at a glance,
  /// and a mis-set schedule is invisible until it fails to fire.
  String get triggerLabel => switch (triggerType) {
        'manual' => 'Manual only',
        'on_install' => 'Once, on install',
        'once' => 'Once at ${_fmtTime(triggerValue)}',
        'interval' => 'Every ${_fmtInterval(triggerValue)}',
        'cron' => _cronLabel(triggerValue),
        _ => triggerValue ?? '—',
      };
}

class BackgroundRun {
  final String id;
  final String taskId;
  final String sessionId;
  final String triggerKind;
  final String status; // running|success|error|timeout|cancelled|skipped
  final String startedAt;
  final String? finishedAt;
  final int? durationMs;
  final int? turnCount;
  final int? tokensIn;
  final int? tokensOut;
  final String? prompt;
  final String? result;
  final String? error;

  const BackgroundRun({
    required this.id,
    required this.taskId,
    this.sessionId = '',
    this.triggerKind = 'schedule',
    this.status = 'success',
    this.startedAt = '',
    this.finishedAt,
    this.durationMs,
    this.turnCount,
    this.tokensIn,
    this.tokensOut,
    this.prompt,
    this.result,
    this.error,
  });

  factory BackgroundRun.fromJson(Map<String, dynamic> j) => BackgroundRun(
        id: '${j['id'] ?? ''}',
        taskId: '${j['task_id'] ?? ''}',
        sessionId: '${j['session_id'] ?? ''}',
        triggerKind: '${j['trigger_kind'] ?? 'schedule'}',
        status: '${j['status'] ?? 'success'}',
        startedAt: '${j['started_at'] ?? ''}',
        finishedAt: j['finished_at'] as String?,
        durationMs: (j['duration_ms'] as num?)?.toInt(),
        turnCount: (j['turn_count'] as num?)?.toInt(),
        tokensIn: (j['tokens_in'] as num?)?.toInt(),
        tokensOut: (j['tokens_out'] as num?)?.toInt(),
        prompt: j['prompt'] as String?,
        result: j['result'] as String?,
        error: j['error'] as String?,
      );

  bool get isRunning => status == 'running';

  /// A skip is an outcome, not a fault — a template task with nothing to do is
  /// healthy. Keep it out of anything that reads as an error.
  bool get isFailure => status == 'error' || status == 'timeout';
}

/// One line of a background session's transcript.
class BackgroundActivity {
  final int id;
  final String runId;
  final String ts;
  final String kind; // think | text | tool | tool_error | message
  final String detail;

  const BackgroundActivity({
    required this.id,
    required this.runId,
    this.ts = '',
    this.kind = 'text',
    this.detail = '',
  });

  factory BackgroundActivity.fromJson(Map<String, dynamic> j) =>
      BackgroundActivity(
        id: (j['id'] as num?)?.toInt() ?? 0,
        runId: '${j['run_id'] ?? ''}',
        ts: '${j['ts'] ?? ''}',
        kind: '${j['kind'] ?? 'text'}',
        detail: '${j['detail'] ?? ''}',
      );
}

class BackgroundTotals {
  final int runs;
  final int success;
  final int error;
  final int timeout;
  final int cancelled;
  final int skipped;
  final int running;
  final double successRate;
  final int avgDurationMs;
  final int p95DurationMs;
  final int tokensIn;
  final int tokensOut;

  const BackgroundTotals({
    this.runs = 0,
    this.success = 0,
    this.error = 0,
    this.timeout = 0,
    this.cancelled = 0,
    this.skipped = 0,
    this.running = 0,
    this.successRate = 1.0,
    this.avgDurationMs = 0,
    this.p95DurationMs = 0,
    this.tokensIn = 0,
    this.tokensOut = 0,
  });

  factory BackgroundTotals.fromJson(Map<String, dynamic> j) => BackgroundTotals(
        runs: (j['runs'] as num?)?.toInt() ?? 0,
        success: (j['success'] as num?)?.toInt() ?? 0,
        error: (j['error'] as num?)?.toInt() ?? 0,
        timeout: (j['timeout'] as num?)?.toInt() ?? 0,
        cancelled: (j['cancelled'] as num?)?.toInt() ?? 0,
        skipped: (j['skipped'] as num?)?.toInt() ?? 0,
        running: (j['running'] as num?)?.toInt() ?? 0,
        successRate: (j['success_rate'] as num?)?.toDouble() ?? 1.0,
        avgDurationMs: (j['avg_duration_ms'] as num?)?.toInt() ?? 0,
        p95DurationMs: (j['p95_duration_ms'] as num?)?.toInt() ?? 0,
        tokensIn: (j['tokens_in'] as num?)?.toInt() ?? 0,
        tokensOut: (j['tokens_out'] as num?)?.toInt() ?? 0,
      );
}

class BackgroundTaskStat {
  final String taskId;
  final String title;
  final String ownerKind;
  final String ownerId;
  final String status;
  final String? nextRun;
  final int consecutiveFailures;
  final int runs;
  final int success;
  final int skipped;
  final int failures;
  final double successRate;
  final int avgDurationMs;

  const BackgroundTaskStat({
    required this.taskId,
    required this.title,
    this.ownerKind = 'user',
    this.ownerId = '',
    this.status = 'active',
    this.nextRun,
    this.consecutiveFailures = 0,
    this.runs = 0,
    this.success = 0,
    this.skipped = 0,
    this.failures = 0,
    this.successRate = 1.0,
    this.avgDurationMs = 0,
  });

  factory BackgroundTaskStat.fromJson(Map<String, dynamic> j) =>
      BackgroundTaskStat(
        taskId: '${j['task_id'] ?? ''}',
        title: '${j['title'] ?? ''}',
        ownerKind: '${j['owner_kind'] ?? 'user'}',
        ownerId: '${j['owner_id'] ?? ''}',
        status: '${j['status'] ?? 'active'}',
        nextRun: j['next_run'] as String?,
        consecutiveFailures: (j['consecutive_failures'] as num?)?.toInt() ?? 0,
        runs: (j['runs'] as num?)?.toInt() ?? 0,
        success: (j['success'] as num?)?.toInt() ?? 0,
        skipped: (j['skipped'] as num?)?.toInt() ?? 0,
        failures: (j['failures'] as num?)?.toInt() ?? 0,
        successRate: (j['success_rate'] as num?)?.toDouble() ?? 1.0,
        avgDurationMs: (j['avg_duration_ms'] as num?)?.toInt() ?? 0,
      );
}

class BackgroundAttention {
  final String taskId;
  final String title;
  final String status;
  final int consecutiveFailures;
  final String? lastError;

  const BackgroundAttention({
    required this.taskId,
    required this.title,
    this.status = 'active',
    this.consecutiveFailures = 0,
    this.lastError,
  });

  factory BackgroundAttention.fromJson(Map<String, dynamic> j) =>
      BackgroundAttention(
        taskId: '${j['task_id'] ?? ''}',
        title: '${j['title'] ?? ''}',
        status: '${j['status'] ?? 'active'}',
        consecutiveFailures: (j['consecutive_failures'] as num?)?.toInt() ?? 0,
        lastError: j['last_error'] as String?,
      );
}

class BackgroundStats {
  final String window;
  final BackgroundTotals totals;
  final List<BackgroundTaskStat> byTask;
  final List<BackgroundAttention> attention;

  const BackgroundStats({
    this.window = '7d',
    this.totals = const BackgroundTotals(),
    this.byTask = const [],
    this.attention = const [],
  });

  factory BackgroundStats.fromJson(Map<String, dynamic> j) => BackgroundStats(
        window: '${j['window'] ?? '7d'}',
        totals: BackgroundTotals.fromJson(
            (j['totals'] as Map?)?.cast<String, dynamic>() ?? const {}),
        byTask: ((j['by_task'] as List?) ?? const [])
            .whereType<Map>()
            .map((e) => BackgroundTaskStat.fromJson(e.cast<String, dynamic>()))
            .toList(),
        attention: ((j['attention'] as List?) ?? const [])
            .whereType<Map>()
            .map((e) => BackgroundAttention.fromJson(e.cast<String, dynamic>()))
            .toList(),
      );
}

// ─── formatting helpers ───────────────────────────────────────────────────────

String _fmtTime(String? iso) {
  if (iso == null || iso.isEmpty) return '—';
  final d = DateTime.tryParse(iso);
  if (d == null) return iso;
  final l = d.toLocal();
  String two(int n) => n.toString().padLeft(2, '0');
  return '${l.year}-${two(l.month)}-${two(l.day)} ${two(l.hour)}:${two(l.minute)}';
}

String _fmtInterval(String? ms) {
  final v = int.tryParse(ms ?? '');
  if (v == null || v <= 0) return '?';
  if (v < 60000) return '${(v / 1000).round()}s';
  if (v < 3600000) return '${(v / 60000).round()}m';
  if (v < 86400000) return '${(v / 3600000).round()}h';
  return '${(v / 86400000).round()}d';
}

/// Render the common cron shapes in words. Falls back to the raw expression —
/// better an honest expression than a confidently wrong sentence.
String _cronLabel(String? expr) {
  if (expr == null || expr.trim().isEmpty) return '—';
  final f = expr.trim().split(RegExp(r'\s+'));
  // Accept the 6-field form (leading seconds) the daemon also takes.
  final p = f.length == 6 ? f.sublist(1) : f;
  if (p.length != 5) return expr;
  final [min, hour, dom, mon, dow] = p;

  String at() {
    final h = int.tryParse(hour), m = int.tryParse(min);
    if (h == null || m == null) return '';
    return ' at ${h.toString().padLeft(2, '0')}:${m.toString().padLeft(2, '0')}';
  }

  const days = {
    '0': 'Sunday', '1': 'Monday', '2': 'Tuesday', '3': 'Wednesday',
    '4': 'Thursday', '5': 'Friday', '6': 'Saturday', '7': 'Sunday',
  };

  if (dom == '*' && mon == '*' && dow == '*') {
    if (hour == '*') return min == '*' ? 'Every minute' : 'Hourly at :$min';
    return 'Daily${at()}';
  }
  if (dom == '*' && mon == '*' && days.containsKey(dow)) {
    return 'Weekly on ${days[dow]}${at()}';
  }
  if (dom == '*' && mon == '*' && dow == '1-5') return 'Weekdays${at()}';
  if (mon == '*' && dow == '*' && int.tryParse(dom) != null) {
    return 'Monthly on day $dom${at()}';
  }
  return expr;
}

String fmtBgTime(String? iso) => _fmtTime(iso);

String fmtBgDuration(int? ms) {
  if (ms == null) return '—';
  if (ms < 1000) return '${ms}ms';
  if (ms < 60000) return '${(ms / 1000).toStringAsFixed(1)}s';
  return '${ms ~/ 60000}m${((ms % 60000) / 1000).round()}s';
}

/// "in 4h", "2m ago" — a next-run timestamp means little without the delta.
String fmtBgRelative(String? iso) {
  if (iso == null || iso.isEmpty) return '—';
  final d = DateTime.tryParse(iso);
  if (d == null) return iso;
  final diff = d.toLocal().difference(DateTime.now());
  final ahead = !diff.isNegative;
  final s = diff.abs();
  String unit;
  if (s.inSeconds < 60) {
    unit = '${s.inSeconds}s';
  } else if (s.inMinutes < 60) {
    unit = '${s.inMinutes}m';
  } else if (s.inHours < 24) {
    unit = '${s.inHours}h';
  } else {
    unit = '${s.inDays}d';
  }
  return ahead ? 'in $unit' : '$unit ago';
}

/// The next-run line for a task row.
///
/// A `next_run` in the past does NOT mean the task is broken — the scheduler
/// polls on an interval, so a window can sit a few seconds overdue, and the
/// list itself only refetches when a run finishes. Say "Due now" rather than
/// composing "Next " with a past tense, which reads as gibberish
/// ("Next 24s ago") and looks like a bug when nothing is wrong.
String fmtBgNextRun(String? iso, String status) {
  if (status != 'active') return 'Not scheduled';
  if (iso == null || iso.isEmpty) return 'Not scheduled';
  final d = DateTime.tryParse(iso);
  if (d == null) return 'Not scheduled';
  if (d.toLocal().isBefore(DateTime.now())) return 'Due now';
  return 'Next ${fmtBgRelative(iso)}';
}
