import 'package:flutter/material.dart';
import '../theme/tokens.dart';
import 'markdown_text.dart';

/// Inline card for a pending tool-permission request (parity with the web
/// PermissionCard). `data` is the `permission:request` payload:
/// `{requestId, toolName, title, content, options:[{key,label}]}`.
class PermissionCard extends StatelessWidget {
  final Map<String, dynamic> data;
  final bool resolved;
  final String? resolvedText;
  final void Function(String optionKey, String optionLabel) onRespond;

  const PermissionCard({
    super.key,
    required this.data,
    required this.resolved,
    required this.onRespond,
    this.resolvedText,
  });

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final toolName = (data['toolName'] ?? 'tool').toString();
    final title = (data['title'] ?? '').toString();
    final content = (data['content'] ?? '').toString();
    final options =
        ((data['options'] as List?) ?? const []).cast<dynamic>();

    return Container(
      margin: const EdgeInsets.symmetric(vertical: 6, horizontal: 4),
      padding: const EdgeInsets.all(14),
      decoration: BoxDecoration(
        color: AppTokens.warning.withValues(alpha: 0.08),
        borderRadius: BorderRadius.circular(12),
        border: Border.all(color: AppTokens.warning.withValues(alpha: 0.35)),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              const Icon(Icons.shield_outlined,
                  color: AppTokens.warning, size: 16),
              const SizedBox(width: 6),
              Expanded(
                child: Text(
                  title.isNotEmpty ? title : 'Yêu cầu quyền: $toolName',
                  style: TextStyle(
                      color: c.textPrimary,
                      fontSize: 13,
                      fontWeight: FontWeight.w600),
                ),
              ),
            ],
          ),
          if (content.isNotEmpty) ...[
            const SizedBox(height: 8),
            ConstrainedBox(
              constraints: const BoxConstraints(maxHeight: 180),
              child: SingleChildScrollView(
                child: MarkdownText(content,
                    color: c.textSecondary, fontSize: 12),
              ),
            ),
          ],
          const SizedBox(height: 10),
          if (resolved)
            _ResolvedRow(label: 'Đã chọn: ${resolvedText ?? ''}')
          else
            Wrap(
              spacing: 8,
              runSpacing: 6,
              children: [
                for (final o in options)
                  _optionButton(
                    context,
                    (o as Map)['label']?.toString() ?? '',
                    () => onRespond(
                      o['key']?.toString() ?? '',
                      o['label']?.toString() ?? '',
                    ),
                  ),
              ],
            ),
        ],
      ),
    );
  }

  Widget _optionButton(BuildContext context, String label, VoidCallback onTap) {
    final isDeny = label.toLowerCase().contains('deny') ||
        label.toLowerCase().contains('từ chối') ||
        label.toLowerCase().contains('no');
    final color = isDeny ? AppTokens.danger : context.colors.accent;
    return OutlinedButton(
      onPressed: onTap,
      style: OutlinedButton.styleFrom(
        foregroundColor: color,
        side: BorderSide(color: color),
        padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 6),
        minimumSize: Size.zero,
      ),
      child: Text(label, style: TextStyle(color: color, fontSize: 12)),
    );
  }
}

/// Inline card for a pending ExitPlanMode request (parity with web
/// PlanExitDialog). `data` is the `plan:exit:request` payload:
/// `{groupJid, agentId, planFilePath, planContent, options:{startEditing, clearContextAndStart}}`.
class PlanCard extends StatelessWidget {
  final Map<String, dynamic> data;
  final bool resolved;
  final String? resolvedText;
  final void Function(String selected) onRespond;

  const PlanCard({
    super.key,
    required this.data,
    required this.resolved,
    required this.onRespond,
    this.resolvedText,
  });

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final planContent = (data['planContent'] ?? '').toString();
    final options = (data['options'] as Map?)?.cast<String, dynamic>() ?? const {};
    final startLabel =
        (options['startEditing'] ?? 'Bắt đầu thực thi').toString();
    final clearLabel =
        (options['clearContextAndStart'] ?? 'Xoá ngữ cảnh & bắt đầu').toString();

    return Container(
      margin: const EdgeInsets.symmetric(vertical: 6, horizontal: 4),
      padding: const EdgeInsets.all(14),
      decoration: BoxDecoration(
        color: c.accent.withValues(alpha: 0.08),
        borderRadius: BorderRadius.circular(12),
        border: Border.all(color: c.accent.withValues(alpha: 0.35)),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Icon(Icons.checklist_rtl, color: c.accent, size: 16),
              const SizedBox(width: 6),
              Text('Kế hoạch chờ duyệt',
                  style: TextStyle(
                      color: c.textPrimary,
                      fontSize: 13,
                      fontWeight: FontWeight.w600)),
            ],
          ),
          if (planContent.isNotEmpty) ...[
            const SizedBox(height: 8),
            ConstrainedBox(
              constraints: const BoxConstraints(maxHeight: 280),
              child: SingleChildScrollView(
                child: MarkdownText(planContent,
                    color: c.textSecondary, fontSize: 12),
              ),
            ),
          ],
          const SizedBox(height: 12),
          if (resolved)
            _ResolvedRow(label: 'Đã chọn: ${resolvedText ?? ''}')
          else
            Column(
              children: [
                SizedBox(
                  width: double.infinity,
                  child: FilledButton(
                    onPressed: () => onRespond('startEditing'),
                    style: FilledButton.styleFrom(
                      padding: const EdgeInsets.symmetric(vertical: 10),
                    ),
                    child: Text(startLabel),
                  ),
                ),
                const SizedBox(height: 8),
                SizedBox(
                  width: double.infinity,
                  child: OutlinedButton(
                    onPressed: () => onRespond('clearContextAndStart'),
                    style: OutlinedButton.styleFrom(
                      foregroundColor: c.accent,
                      side: BorderSide(color: c.accent),
                      padding: const EdgeInsets.symmetric(vertical: 10),
                    ),
                    child: Text(clearLabel),
                  ),
                ),
                const SizedBox(height: 4),
                TextButton(
                  onPressed: () => onRespond('cancelled'),
                  child: Text('Huỷ',
                      style: TextStyle(color: c.textMuted)),
                ),
              ],
            ),
        ],
      ),
    );
  }
}

/// Inline card for a pending ask-question batch (parity with web QuestionCard).
/// `data` is the `question:request` payload:
/// `{requestId, agentId, questions:[{header, question, options:[{label,description}], multiSelect}]}`.
class QuestionCard extends StatefulWidget {
  final Map<String, dynamic> data;
  final bool resolved;
  final void Function(Map<String, dynamic> answers) onSubmit;

  const QuestionCard({
    super.key,
    required this.data,
    required this.resolved,
    required this.onSubmit,
  });

  @override
  State<QuestionCard> createState() => _QuestionCardState();
}

class _QuestionCardState extends State<QuestionCard> {
  // questionIndex -> selected option index(es).
  final Map<int, Set<int>> _selected = {};

  List<dynamic> get _questions =>
      ((widget.data['questions'] as List?) ?? const []).cast<dynamic>();

  void _toggle(int qi, int oi, bool multi) {
    setState(() {
      final set = _selected.putIfAbsent(qi, () => <int>{});
      if (multi) {
        if (set.contains(oi)) {
          set.remove(oi);
        } else {
          set.add(oi);
        }
      } else {
        set
          ..clear()
          ..add(oi);
      }
    });
  }

  bool get _complete =>
      _questions.asMap().keys.every((qi) => (_selected[qi]?.isNotEmpty ?? false));

  void _submit() {
    final answers = <String, dynamic>{};
    for (final entry in _selected.entries) {
      final qi = entry.key;
      final sel = entry.value.toList()..sort();
      final multi = (_questions[qi] as Map)['multiSelect'] == true;
      answers['$qi'] = multi ? sel : (sel.isNotEmpty ? sel.first : 0);
    }
    widget.onSubmit(answers);
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Container(
      margin: const EdgeInsets.symmetric(vertical: 6, horizontal: 4),
      padding: const EdgeInsets.all(14),
      decoration: BoxDecoration(
        color: AppTokens.cyan.withValues(alpha: 0.08),
        borderRadius: BorderRadius.circular(12),
        border: Border.all(color: AppTokens.cyan.withValues(alpha: 0.35)),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              const Icon(Icons.help_outline, color: AppTokens.cyan, size: 16),
              const SizedBox(width: 6),
              Text('Agent đang hỏi',
                  style: TextStyle(
                      color: c.textPrimary,
                      fontSize: 13,
                      fontWeight: FontWeight.w600)),
            ],
          ),
          const SizedBox(height: 10),
          for (var qi = 0; qi < _questions.length; qi++)
            _buildQuestion(qi, _questions[qi] as Map),
          const SizedBox(height: 6),
          if (widget.resolved)
            _ResolvedRow(label: 'Đã trả lời')
          else
            SizedBox(
              width: double.infinity,
              child: FilledButton(
                onPressed: _complete ? _submit : null,
                style: FilledButton.styleFrom(
                  backgroundColor: AppTokens.cyan,
                  padding: const EdgeInsets.symmetric(vertical: 10),
                ),
                child: const Text('Gửi trả lời'),
              ),
            ),
        ],
      ),
    );
  }

  Widget _buildQuestion(int qi, Map q) {
    final c = context.colors;
    final header = (q['header'] ?? '').toString();
    final question = (q['question'] ?? '').toString();
    final multi = q['multiSelect'] == true;
    final options = ((q['options'] as List?) ?? const []).cast<dynamic>();
    return Padding(
      padding: const EdgeInsets.only(bottom: 12),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          if (header.isNotEmpty)
            Text(header.toUpperCase(),
                style: TextStyle(
                    color: c.textMuted,
                    fontSize: 10,
                    fontWeight: FontWeight.w600,
                    letterSpacing: 0.8)),
          if (question.isNotEmpty)
            Padding(
              padding: const EdgeInsets.only(top: 2, bottom: 6),
              child: Text(question,
                  style: TextStyle(color: c.textPrimary, fontSize: 13)),
            ),
          Wrap(
            spacing: 8,
            runSpacing: 6,
            children: [
              for (var oi = 0; oi < options.length; oi++)
                _optionChip(
                  (options[oi] as Map)['label']?.toString() ?? '',
                  _selected[qi]?.contains(oi) ?? false,
                  widget.resolved ? null : () => _toggle(qi, oi, multi),
                ),
            ],
          ),
        ],
      ),
    );
  }

  Widget _optionChip(String label, bool selected, VoidCallback? onTap) {
    final c = context.colors;
    return InkWell(
      onTap: onTap,
      borderRadius: BorderRadius.circular(8),
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 7),
        decoration: BoxDecoration(
          color: selected
              ? AppTokens.cyan.withValues(alpha: 0.18)
              : c.surfaceAlt,
          borderRadius: BorderRadius.circular(8),
          border: Border.all(
            color: selected ? AppTokens.cyan : c.border,
          ),
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            if (selected)
              const Padding(
                padding: EdgeInsets.only(right: 4),
                child: Icon(Icons.check, color: AppTokens.cyan, size: 14),
              ),
            Text(label,
                style: TextStyle(
                    color: selected ? AppTokens.cyan : c.textSecondary,
                    fontSize: 12)),
          ],
        ),
      ),
    );
  }
}

/// Shared "resolved" confirmation row.
class _ResolvedRow extends StatelessWidget {
  const _ResolvedRow({required this.label});
  final String label;

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        const Icon(Icons.check_circle, color: AppTokens.success, size: 15),
        const SizedBox(width: 6),
        Flexible(
          child: Text(label,
              style: const TextStyle(color: AppTokens.success, fontSize: 12)),
        ),
      ],
    );
  }
}
