/// Small shared formatting helpers used across feature screens.
library;

/// Relative "time ago" label from an ISO-8601 string (Vietnamese).
String timeAgoIso(String? iso) {
  if (iso == null || iso.isEmpty) return '';
  final dt = DateTime.tryParse(iso);
  if (dt == null) return '';
  return _ago(dt.toLocal());
}

/// Relative "time ago" label from unix seconds (Vietnamese).
String timeAgoEpochSecs(int? secs) {
  if (secs == null || secs == 0) return '';
  return _ago(DateTime.fromMillisecondsSinceEpoch(secs * 1000));
}

/// Relative "time ago" label from unix milliseconds (Vietnamese).
String timeAgoEpochMs(int? ms) {
  if (ms == null || ms == 0) return '';
  return _ago(DateTime.fromMillisecondsSinceEpoch(ms));
}

String _ago(DateTime dt) {
  final diff = DateTime.now().difference(dt);
  if (diff.inSeconds < 60) return 'vừa xong';
  if (diff.inMinutes < 60) return '${diff.inMinutes} phút trước';
  if (diff.inHours < 24) return '${diff.inHours} giờ trước';
  if (diff.inDays < 7) return '${diff.inDays} ngày trước';
  final d = dt;
  return '${d.day.toString().padLeft(2, '0')}/${d.month.toString().padLeft(2, '0')}/${d.year}';
}

/// Short title from a wiki/file path (basename without extension).
String titleFromPath(String path) {
  final base = path.split('/').last;
  return base.replaceAll(RegExp(r'\.md$'), '').replaceAll('-', ' ');
}
