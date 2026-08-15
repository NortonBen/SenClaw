import 'dart:convert';

import 'package:flutter/material.dart';

import '../../../core/i18n/l10n.dart';
import '../../../theme/tokens.dart';

/// Tool arguments, rendered as labelled rows instead of raw JSON.
///
/// The permission card is a decision prompt: the user has a second to judge
/// whether to let the agent do this. Braces, quotes and snake_case keys make
/// that harder than it needs to be — `{"start_local": "2026-08-15 19:23"}` is
/// the same information as `Bắt đầu · 2026-08-15 19:23`, just less legible.
///
/// Falls back to the original text whenever the payload is not a flat JSON
/// object, so a tool that sends prose, an array, or malformed JSON still shows
/// exactly what it sent rather than nothing. Mirrors
/// `web/src/components/chat-common/ToolParams.tsx`.
class ToolParams extends StatelessWidget {
  const ToolParams({super.key, required this.content});

  final String content;

  /// snake_case / camelCase key → human label. Unknown keys are humanised.
  static const _labels = <String, String>{
    'title': 'Tiêu đề',
    'name': 'Tên',
    'start_local': 'Bắt đầu',
    'end_local': 'Kết thúc',
    'start_at': 'Bắt đầu',
    'end_at': 'Kết thúc',
    'all_day': 'Cả ngày',
    'location': 'Địa điểm',
    'description': 'Mô tả',
    'content': 'Nội dung',
    'path': 'Đường dẫn',
    'file_path': 'Tệp',
    'command': 'Lệnh',
    'url': 'Đường dẫn',
    'query': 'Truy vấn',
    'reminder_min': 'Nhắc trước (phút)',
    'event_id': 'Mã sự kiện',
    'field': 'Trường',
    'value': 'Giá trị',
    'directive': 'Quy tắc',
    'tier': 'Phạm vi',
    'limit': 'Giới hạn',
    'timeout': 'Hết hạn (giây)',
  };

  static String _humanise(String key) {
    final known = _labels[key];
    if (known != null) return known;
    final spaced = key
        .replaceAll('_', ' ')
        .replaceAllMapped(RegExp(r'([a-z])([A-Z])'), (m) => '${m[1]} ${m[2]}');
    return spaced.isEmpty
        ? key
        : spaced[0].toUpperCase() + spaced.substring(1);
  }

  static String _renderValue(BuildContext context, Object? v) {
    if (v == null) return '—';
    if (v is bool) return v ? context.tr('Yes') : context.tr('No');
    if (v is String) return v.trim().isEmpty ? '—' : v;
    if (v is num) return '$v';
    // Nested objects/arrays have no flat form; keep them as JSON.
    return jsonEncode(v);
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final text = content.trim();
    if (text.isEmpty) return const SizedBox.shrink();

    Map<String, dynamic>? parsed;
    try {
      final decoded = jsonDecode(text);
      if (decoded is Map<String, dynamic>) parsed = decoded;
    } catch (_) {
      // Not JSON — fall through to the raw view.
    }

    if (parsed == null) {
      return Text(text, style: TextStyle(color: c.textSecondary, fontSize: 14));
    }
    // `{}` is what a no-argument tool sends. A card reading "{}" tells the user
    // nothing; showing no parameter block at all says the same thing quietly.
    if (parsed.isEmpty) return const SizedBox.shrink();

    return Container(
      width: double.infinity,
      padding: const EdgeInsets.all(AppTokens.s8),
      decoration: BoxDecoration(
        color: c.surfaceAlt,
        borderRadius: BorderRadius.circular(AppTokens.rMd),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          for (final e in parsed.entries)
            Padding(
              padding: const EdgeInsets.only(bottom: AppTokens.s4),
              child: Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  SizedBox(
                    width: 130,
                    child: Text(
                      _humanise(e.key),
                      style: TextStyle(color: c.textSecondary, fontSize: 12),
                    ),
                  ),
                  const SizedBox(width: AppTokens.s8),
                  Expanded(
                    child: Text(
                      _renderValue(context, e.value),
                      style: TextStyle(color: c.textPrimary, fontSize: 12),
                    ),
                  ),
                ],
              ),
            ),
        ],
      ),
    );
  }
}
