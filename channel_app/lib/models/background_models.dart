/// Background tasks — autonomous work the daemon runs by itself.
///
/// Distinct from schedules in `space_models.dart`: a schedule runs inside a
/// chat session and replies to the user, a background task runs unattended and
/// writes to a run record nobody has to read. Mirrors the desktop app's
/// `background_models.dart`; the daemon's REST surface is snake_case.
library;

import '../services/language_service.dart';

class BackgroundTask {
  final String id;
  final String ownerKind; // 'system' | 'app' | 'user'
  final String ownerId;
  final String title;
  final String? description;
  final String jobKind; // 'prompt' | 'native'
  final String promptKind; // 'static' | 'template' | 'generator'
  final String? prompt;
  final String? persona;
  final String continuity; // 'fresh' | 'thread'
  final String triggerType; // 'cron' | 'interval' | 'once' | 'on_install' | 'manual'
  final String? triggerValue;
  final String? nextRun;
  final String? lastRun;
  final int consecutiveFailures;
  final String visibility; // 'normal' | 'internal'
  /// Deliver an OS notification instead of running an agent.
  final bool notify;
  final String status; // active | paused | completed | failed | cancelled

  const BackgroundTask({
    required this.id,
    required this.ownerKind,
    required this.ownerId,
    required this.title,
    this.description,
    this.jobKind = 'prompt',
    this.promptKind = 'static',
    this.prompt,
    this.persona,
    this.continuity = 'fresh',
    this.triggerType = 'cron',
    this.triggerValue,
    this.nextRun,
    this.lastRun,
    this.consecutiveFailures = 0,
    this.visibility = 'normal',
    this.notify = false,
    this.status = 'active',
  });

  factory BackgroundTask.fromJson(Map<String, dynamic> j) => BackgroundTask(
        id: '${j['id'] ?? ''}',
        ownerKind: '${j['owner_kind'] ?? 'user'}',
        ownerId: '${j['owner_id'] ?? ''}',
        title: '${j['title'] ?? ''}',
        description: j['description'] as String?,
        jobKind: '${j['job_kind'] ?? 'prompt'}',
        promptKind: '${j['prompt_kind'] ?? 'static'}',
        prompt: j['prompt'] as String?,
        persona: j['persona'] as String?,
        continuity: '${j['continuity'] ?? 'fresh'}',
        triggerType: '${j['trigger_type'] ?? 'cron'}',
        triggerValue: j['trigger_value'] as String?,
        nextRun: j['next_run'] as String?,
        lastRun: j['last_run'] as String?,
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
        'system' => tr('Hệ thống', 'System'),
        'app' => ownerId,
        _ => tr('Bạn', 'You'),
      };

  /// The trigger in prose. A cron expression tells the user nothing at a
  /// glance, and a mis-set schedule is invisible until it fails to fire.
  String get triggerLabel => switch (triggerType) {
        'manual' => tr('Chạy tay', 'Manual only'),
        'on_install' => tr('Một lần, khi cài', 'Once, on install'),
        'once' => tr('Một lần lúc ${fmtBgTime(triggerValue)}',
            'Once at ${fmtBgTime(triggerValue)}'),
        'interval' => tr('Mỗi ${_fmtInterval(triggerValue)}',
            'Every ${_fmtInterval(triggerValue)}'),
        'cron' => _cronLabel(triggerValue),
        _ => triggerValue ?? '—',
      };
}

class BackgroundRun {
  final String id;
  final String taskId;
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

/// One run plus its transcript, from `GET /api/background/runs/:id`.
class BackgroundSession {
  final BackgroundRun run;
  final List<BackgroundActivity> activity;
  const BackgroundSession(this.run, this.activity);
}

/// One page of tasks plus the unpaged total, so the UI can render a pager.
class BgTaskPage {
  final List<BackgroundTask> tasks;
  final int total;
  const BgTaskPage(this.tasks, this.total);
}

class BackgroundTotals {
  final int runs;
  final int success;
  final int error;
  final int timeout;
  final int skipped;
  final int running;
  final double successRate;
  final int avgDurationMs;
  final int tokensIn;
  final int tokensOut;

  const BackgroundTotals({
    this.runs = 0,
    this.success = 0,
    this.error = 0,
    this.timeout = 0,
    this.skipped = 0,
    this.running = 0,
    this.successRate = 1.0,
    this.avgDurationMs = 0,
    this.tokensIn = 0,
    this.tokensOut = 0,
  });

  factory BackgroundTotals.fromJson(Map<String, dynamic> j) => BackgroundTotals(
        runs: (j['runs'] as num?)?.toInt() ?? 0,
        success: (j['success'] as num?)?.toInt() ?? 0,
        error: (j['error'] as num?)?.toInt() ?? 0,
        timeout: (j['timeout'] as num?)?.toInt() ?? 0,
        skipped: (j['skipped'] as num?)?.toInt() ?? 0,
        running: (j['running'] as num?)?.toInt() ?? 0,
        successRate: (j['success_rate'] as num?)?.toDouble() ?? 1.0,
        avgDurationMs: (j['avg_duration_ms'] as num?)?.toInt() ?? 0,
        tokensIn: (j['tokens_in'] as num?)?.toInt() ?? 0,
        tokensOut: (j['tokens_out'] as num?)?.toInt() ?? 0,
      );
}

/// A task the stats endpoint flags as needing attention (failing repeatedly).
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
  final List<BackgroundAttention> attention;

  const BackgroundStats({
    this.window = '7d',
    this.totals = const BackgroundTotals(),
    this.attention = const [],
  });

  factory BackgroundStats.fromJson(Map<String, dynamic> j) => BackgroundStats(
        window: '${j['window'] ?? '7d'}',
        totals: BackgroundTotals.fromJson(
            (j['totals'] as Map?)?.cast<String, dynamic>() ?? const {}),
        attention: ((j['attention'] as List?) ?? const [])
            .whereType<Map>()
            .map((e) => BackgroundAttention.fromJson(e.cast<String, dynamic>()))
            .toList(),
      );
}

// ─── formatting helpers ───────────────────────────────────────────────────────

String fmtBgTime(String? iso) {
  if (iso == null || iso.isEmpty) return '—';
  final d = DateTime.tryParse(iso);
  if (d == null) return iso;
  final l = d.toLocal();
  String two(int n) => n.toString().padLeft(2, '0');
  return '${two(l.day)}/${two(l.month)} ${two(l.hour)}:${two(l.minute)}';
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
    final hm = '${h.toString().padLeft(2, '0')}:${m.toString().padLeft(2, '0')}';
    return tr(' lúc $hm', ' at $hm');
  }

  final days = {
    '0': tr('Chủ nhật', 'Sunday'),
    '1': tr('Thứ hai', 'Monday'),
    '2': tr('Thứ ba', 'Tuesday'),
    '3': tr('Thứ tư', 'Wednesday'),
    '4': tr('Thứ năm', 'Thursday'),
    '5': tr('Thứ sáu', 'Friday'),
    '6': tr('Thứ bảy', 'Saturday'),
    '7': tr('Chủ nhật', 'Sunday'),
  };

  if (dom == '*' && mon == '*' && dow == '*') {
    if (hour == '*') {
      return min == '*'
          ? tr('Mỗi phút', 'Every minute')
          : tr('Mỗi giờ phút :$min', 'Hourly at :$min');
    }
    return tr('Hàng ngày${at()}', 'Daily${at()}');
  }
  if (dom == '*' && mon == '*' && days.containsKey(dow)) {
    return tr('Hàng tuần ${days[dow]}${at()}', 'Weekly on ${days[dow]}${at()}');
  }
  if (dom == '*' && mon == '*' && dow == '1-5') {
    return tr('Ngày làm việc${at()}', 'Weekdays${at()}');
  }
  if (mon == '*' && dow == '*' && int.tryParse(dom) != null) {
    return tr('Hàng tháng ngày $dom${at()}', 'Monthly on day $dom${at()}');
  }
  return expr;
}

String fmtBgDuration(int? ms) {
  if (ms == null) return '—';
  if (ms < 1000) return '${ms}ms';
  if (ms < 60000) return '${(ms / 1000).toStringAsFixed(1)}s';
  return '${ms ~/ 60000}m${((ms % 60000) / 1000).round()}s';
}

/// "in 4h" / "2m ago" — a next-run timestamp means little without the delta.
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
  return ahead ? tr('sau $unit', 'in $unit') : tr('$unit trước', '$unit ago');
}

/// The next-run line for a task row.
///
/// A `next_run` in the past does NOT mean the task is broken — the scheduler
/// polls on an interval, so a window can sit a few seconds overdue. Say "Due
/// now" rather than composing "Next " with a past tense, which reads as
/// gibberish and looks like a bug when nothing is wrong.
String fmtBgNextRun(String? iso, String status) {
  if (status != 'active') return tr('Không hẹn giờ', 'Not scheduled');
  if (iso == null || iso.isEmpty) return tr('Không hẹn giờ', 'Not scheduled');
  final d = DateTime.tryParse(iso);
  if (d == null) return tr('Không hẹn giờ', 'Not scheduled');
  if (d.toLocal().isBefore(DateTime.now())) return tr('Đến hạn', 'Due now');
  return tr('Chạy tiếp ${fmtBgRelative(iso)}', 'Next ${fmtBgRelative(iso)}');
}
