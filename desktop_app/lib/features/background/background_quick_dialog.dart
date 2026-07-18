import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../models/background_models.dart';
import '../../theme/tokens.dart';
import 'background_providers.dart';

/// "Quick task" — describe a background task in one line and let the daemon's
/// LLM fill the fields, instead of the field-by-field New task form.
///
/// Two steps on purpose: parse → review → create. A background task runs
/// unattended, so a schedule the model got wrong must be visible before it is
/// committed, not after it silently fails to fire.
void showBackgroundQuickDialog(BuildContext context) {
  showDialog(context: context, builder: (_) => const _QuickDialog());
}

class _QuickDialog extends ConsumerStatefulWidget {
  const _QuickDialog();
  @override
  ConsumerState<_QuickDialog> createState() => _QuickDialogState();
}

class _QuickDialogState extends ConsumerState<_QuickDialog> {
  final _text = TextEditingController();
  bool _parsing = false;
  bool _creating = false;
  String? _error;
  Map<String, dynamic>? _draft; // the parsed spec, awaiting confirmation

  @override
  void dispose() {
    _text.dispose();
    super.dispose();
  }

  Future<void> _parse() async {
    final text = _text.text.trim();
    if (text.isEmpty) return;
    setState(() {
      _parsing = true;
      _error = null;
    });
    try {
      final spec = await ref.read(backgroundApiProvider).parseQuick(text);
      if (!mounted) return;
      setState(() => _draft = spec);
    } catch (e) {
      if (mounted) setState(() => _error = _msg(e));
    } finally {
      if (mounted) setState(() => _parsing = false);
    }
  }

  Future<void> _create() async {
    final d = _draft;
    if (d == null) return;
    setState(() {
      _creating = true;
      _error = null;
    });
    try {
      // Reuse the normal create path (server derives next_run, applies guard-3).
      await ref.read(backgroundApiProvider).create({
        'title': d['title'],
        'prompt': d['prompt'],
        'trigger_type': d['trigger_type'],
        if (d['trigger_value'] != null) 'trigger_value': d['trigger_value'],
        'prompt_kind': d['prompt_kind'] ?? 'static',
        'continuity': d['continuity'] ?? 'fresh',
        if (d['notify'] == true) 'notify': true,
      });
      if (mounted) Navigator.pop(context);
    } catch (e) {
      if (mounted) setState(() => _error = _msg(e));
    } finally {
      if (mounted) setState(() => _creating = false);
    }
  }

  String _msg(Object e) {
    final s = e.toString();
    return s.startsWith('ApiException') && s.contains(':')
        ? s.substring(s.indexOf(':') + 1).trim()
        : s;
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Dialog(
      backgroundColor: c.bg,
      child: SizedBox(
        width: 560,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Padding(
              padding: const EdgeInsets.all(AppTokens.s16),
              child: Row(
                children: [
                  const Icon(Icons.bolt, size: 18, color: AppTokens.brand),
                  const SizedBox(width: AppTokens.s8),
                  Expanded(
                    child: Text('Quick task',
                        style: TextStyle(
                            color: c.textPrimary,
                            fontSize: 15,
                            fontWeight: FontWeight.w700)),
                  ),
                  IconButton(
                    icon: const Icon(Icons.close, size: 16),
                    onPressed: () => Navigator.pop(context),
                  ),
                ],
              ),
            ),
            Divider(height: 1, color: c.border),
            Padding(
              padding: const EdgeInsets.all(AppTokens.s16),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    'Mô tả task bằng một câu — AI sẽ tự điền lịch chạy và nội dung.',
                    style: TextStyle(color: c.textMuted, fontSize: 12),
                  ),
                  const SizedBox(height: AppTokens.s8),
                  TextField(
                    controller: _text,
                    autofocus: true,
                    maxLines: 3,
                    enabled: !_parsing && !_creating,
                    style: TextStyle(color: c.textPrimary, fontSize: 13),
                    decoration: InputDecoration(
                      hintText:
                          'vd: mỗi sáng 9h rà soát tri thức và dọn mâu thuẫn',
                      hintStyle: TextStyle(color: c.textMuted, fontSize: 12),
                      border: const OutlineInputBorder(),
                      isDense: true,
                    ),
                    onSubmitted: (_) => _parse(),
                  ),
                  const SizedBox(height: AppTokens.s8),
                  Align(
                    alignment: Alignment.centerRight,
                    child: FilledButton.icon(
                      icon: _parsing
                          ? const SizedBox(
                              width: 14,
                              height: 14,
                              child: CircularProgressIndicator(strokeWidth: 2))
                          : const Icon(Icons.auto_awesome, size: 15),
                      label: Text(_parsing
                          ? 'Đang phân tích…'
                          : (_draft == null ? 'AI phân tích' : 'Phân tích lại')),
                      onPressed:
                          (_parsing || _creating) ? null : _parse,
                    ),
                  ),
                  if (_draft != null) ...[
                    const SizedBox(height: AppTokens.s8),
                    _DraftPreview(draft: _draft!),
                  ],
                  if (_error != null)
                    Padding(
                      padding: const EdgeInsets.only(top: AppTokens.s8),
                      child: Text(_error!,
                          style: const TextStyle(
                              color: AppTokens.danger, fontSize: 12)),
                    ),
                ],
              ),
            ),
            Divider(height: 1, color: c.border),
            Padding(
              padding: const EdgeInsets.all(AppTokens.s12),
              child: Row(
                mainAxisAlignment: MainAxisAlignment.end,
                children: [
                  TextButton(
                      onPressed: () => Navigator.pop(context),
                      child: const Text('Huỷ')),
                  const SizedBox(width: AppTokens.s8),
                  FilledButton(
                    onPressed:
                        (_draft == null || _creating) ? null : _create,
                    child: Text(_creating ? 'Đang tạo…' : 'Tạo task'),
                  ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}

/// Read-only preview of the parsed spec so the user can catch a wrong schedule
/// before committing.
class _DraftPreview extends StatelessWidget {
  const _DraftPreview({required this.draft});
  final Map<String, dynamic> draft;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    // Reuse the model's own trigger→prose formatting for consistency with the
    // task list.
    final t = BackgroundTask.fromJson({
      'id': '',
      'owner_kind': 'user',
      'title': draft['title'] ?? '',
      'trigger_type': draft['trigger_type'] ?? 'manual',
      'trigger_value': draft['trigger_value'],
      'continuity': draft['continuity'] ?? 'fresh',
    });

    Widget row(String k, String v) => Padding(
          padding: const EdgeInsets.symmetric(vertical: 2),
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              SizedBox(
                width: 80,
                child: Text(k,
                    style: TextStyle(color: c.textMuted, fontSize: 11)),
              ),
              Expanded(
                child: Text(v,
                    style: TextStyle(color: c.textSecondary, fontSize: 12)),
              ),
            ],
          ),
        );

    return Container(
      width: double.infinity,
      padding: const EdgeInsets.all(AppTokens.s12),
      decoration: BoxDecoration(
        color: c.surface,
        borderRadius: BorderRadius.circular(AppTokens.rMd),
        border: Border.all(color: c.border),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text('AI đề xuất',
              style: TextStyle(
                  color: c.textMuted,
                  fontSize: 10,
                  fontWeight: FontWeight.w600)),
          const SizedBox(height: AppTokens.s4),
          row('Tiêu đề', '${draft['title'] ?? ''}'),
          row('Lịch chạy', t.triggerLabel),
          if (draft['notify'] == true) row('Kiểu', '🔔 Thông báo (không chạy agent)'),
          if (t.continuity == 'thread') row('Bộ nhớ', 'nhớ các lần trước'),
          const SizedBox(height: AppTokens.s4),
          Text('Nội dung',
              style: TextStyle(color: c.textMuted, fontSize: 11)),
          const SizedBox(height: 2),
          Text('${draft['prompt'] ?? ''}',
              style: TextStyle(
                  color: c.textSecondary, fontSize: 12, height: 1.4)),
        ],
      ),
    );
  }
}
