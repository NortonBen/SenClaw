import 'package:flutter/material.dart';
import '../../../core/i18n/l10n.dart';
import '../../../models/chat_message.dart';
import '../../../theme/tokens.dart';

/// Interactive AskUserQuestion card: one or more questions, each single- or
/// multi-select, with an "Other" free-text option. Submits
/// `answers {qIndex: optIndex | [optIndex...]}` (-1 = Other) + `otherTexts`.
class QuestionCard extends StatefulWidget {
  const QuestionCard({super.key, required this.message, required this.onSubmit});
  final ChatMessage message;
  final void Function(
    String requestId,
    Map<int, dynamic> answers,
    Map<int, String> otherTexts,
  ) onSubmit;

  @override
  State<QuestionCard> createState() => _QuestionCardState();
}

class _QuestionCardState extends State<QuestionCard> {
  // qIndex → selected option indices (Other == options.length).
  final Map<int, Set<int>> _selected = {};
  final Map<int, TextEditingController> _other = {};

  List<Map<String, dynamic>> get _questions =>
      ((widget.message.data['questions'] as List?) ?? const [])
          .whereType<Map>()
          .map((e) => e.cast<String, dynamic>())
          .toList();

  @override
  void dispose() {
    for (final c in _other.values) {
      c.dispose();
    }
    super.dispose();
  }

  bool get _complete {
    for (var qi = 0; qi < _questions.length; qi++) {
      if ((_selected[qi] ?? const {}).isEmpty) return false;
    }
    return true;
  }

  void _toggle(int qi, int oi, bool multi) {
    setState(() {
      final set = _selected.putIfAbsent(qi, () => <int>{});
      if (multi) {
        set.contains(oi) ? set.remove(oi) : set.add(oi);
      } else {
        set
          ..clear()
          ..add(oi);
      }
    });
  }

  void _submit() {
    final answers = <int, dynamic>{};
    final otherTexts = <int, String>{};
    for (var qi = 0; qi < _questions.length; qi++) {
      final q = _questions[qi];
      final multi = q['multiSelect'] == true;
      final opts = (q['options'] as List?) ?? const [];
      final otherIdx = opts.length;
      final sel = (_selected[qi] ?? const <int>{})
          .map((oi) => oi == otherIdx ? -1 : oi)
          .toList();
      if (sel.contains(-1)) {
        otherTexts[qi] = _other[qi]?.text.trim() ?? '';
      }
      answers[qi] = multi ? sel : sel.first;
    }
    widget.onSubmit(widget.message.requestId, answers, otherTexts);
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final resolved = widget.message.resolved;
    return Padding(
      padding: const EdgeInsets.symmetric(
        horizontal: AppTokens.s24,
        vertical: AppTokens.s8,
      ),
      child: Container(
        padding: const EdgeInsets.all(AppTokens.s16),
        decoration: BoxDecoration(
          color: c.surface,
          border: Border.all(color: c.accent.withValues(alpha: 0.5)),
          borderRadius: BorderRadius.circular(AppTokens.rLg),
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Icon(Icons.help_outline, size: 16, color: c.accent),
                const SizedBox(width: AppTokens.s8),
                Text(
                  context.tr('Question'),
                  style: TextStyle(
                    color: c.textPrimary,
                    fontWeight: FontWeight.w700,
                  ),
                ),
              ],
            ),
            const SizedBox(height: AppTokens.s12),
            for (var qi = 0; qi < _questions.length; qi++)
              _buildQuestion(context, qi, _questions[qi], resolved),
            const SizedBox(height: AppTokens.s4),
            if (resolved)
              Text(context.tr('Answered'),
                  style: TextStyle(color: c.textMuted, fontSize: 12))
            else
              Align(
                alignment: Alignment.centerRight,
                child: FilledButton(
                  onPressed: _complete ? _submit : null,
                  child: Text(context.tr('Submit')),
                ),
              ),
          ],
        ),
      ),
    );
  }

  Widget _buildQuestion(
      BuildContext context, int qi, Map<String, dynamic> q, bool resolved) {
    final c = context.colors;
    final multi = q['multiSelect'] == true;
    final opts = ((q['options'] as List?) ?? const [])
        .whereType<Map>()
        .map((e) => e.cast<String, dynamic>())
        .toList();
    final otherIdx = opts.length;
    final sel = _selected[qi] ?? const <int>{};
    final otherSelected = sel.contains(otherIdx);

    return Padding(
      padding: const EdgeInsets.only(bottom: AppTokens.s16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          if ('${q['header'] ?? ''}'.isNotEmpty)
            Text(
              '${q['header']}'.toUpperCase(),
              style: TextStyle(
                color: c.accent,
                fontSize: 12,
                fontWeight: FontWeight.w700,
                letterSpacing: 0.5,
              ),
            ),
          const SizedBox(height: AppTokens.s4),
          Text(
            '${q['question'] ?? ''}',
            style: TextStyle(color: c.textPrimary, fontSize: 14),
          ),
          const SizedBox(height: AppTokens.s8),
          Wrap(
            spacing: AppTokens.s8,
            runSpacing: AppTokens.s8,
            children: [
              for (var oi = 0; oi < opts.length; oi++)
                _OptionChip(
                  label: '${opts[oi]['label'] ?? ''}',
                  selected: sel.contains(oi),
                  multi: multi,
                  enabled: !resolved,
                  onTap: () => _toggle(qi, oi, multi),
                ),
              _OptionChip(
                label: context.tr('Other'),
                selected: otherSelected,
                multi: multi,
                enabled: !resolved,
                onTap: () => _toggle(qi, otherIdx, multi),
              ),
            ],
          ),
          if (otherSelected && !resolved) ...[
            const SizedBox(height: AppTokens.s8),
            TextField(
              controller: _other.putIfAbsent(qi, () => TextEditingController()),
              decoration:
                  InputDecoration(hintText: context.tr('Your answer…')),
            ),
          ],
        ],
      ),
    );
  }
}

class _OptionChip extends StatelessWidget {
  const _OptionChip({
    required this.label,
    required this.selected,
    required this.multi,
    required this.enabled,
    required this.onTap,
  });
  final String label;
  final bool selected;
  final bool multi;
  final bool enabled;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return InkWell(
      borderRadius: BorderRadius.circular(AppTokens.rFull),
      onTap: enabled ? onTap : null,
      child: Container(
        padding: const EdgeInsets.symmetric(
          horizontal: AppTokens.s12,
          vertical: AppTokens.s8,
        ),
        decoration: BoxDecoration(
          color: selected ? c.accentSoft : c.surfaceAlt,
          border: Border.all(
            color: selected ? c.accent : c.border,
            width: selected ? 1.5 : 1,
          ),
          borderRadius: BorderRadius.circular(AppTokens.rFull),
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(
              selected
                  ? (multi ? Icons.check_box : Icons.radio_button_checked)
                  : (multi
                      ? Icons.check_box_outline_blank
                      : Icons.radio_button_unchecked),
              size: 14,
              color: selected ? c.accent : c.textMuted,
            ),
            const SizedBox(width: AppTokens.s6),
            Text(
              label,
              style: TextStyle(
                color: selected ? c.textPrimary : c.textSecondary,
                fontSize: 14,
              ),
            ),
          ],
        ),
      ),
    );
  }
}
