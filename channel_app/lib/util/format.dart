/// Small shared formatting helpers used across feature screens.
library;

import '../services/language_service.dart';

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
  if (diff.inSeconds < 60) return tr('vừa xong', 'just now');
  if (diff.inMinutes < 60) {
    return tr('${diff.inMinutes} phút trước', '${diff.inMinutes}m ago');
  }
  if (diff.inHours < 24) {
    return tr('${diff.inHours} giờ trước', '${diff.inHours}h ago');
  }
  if (diff.inDays < 7) {
    return tr('${diff.inDays} ngày trước', '${diff.inDays}d ago');
  }
  final d = dt;
  return '${d.day.toString().padLeft(2, '0')}/${d.month.toString().padLeft(2, '0')}/${d.year}';
}

/// Short title from a wiki/file path (basename without extension).
String titleFromPath(String path) {
  final base = path.split('/').last;
  return base.replaceAll(RegExp(r'\.md$'), '').replaceAll('-', ' ');
}
