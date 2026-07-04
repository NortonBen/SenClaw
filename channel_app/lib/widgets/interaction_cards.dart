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

/// Inline card for a FormUI declarative form (parity with the web FormCard).
/// `data` is the `form:request` payload:
/// `{requestId, agentId, title, surface, submitLabel, fields:[…]}` where each
/// field is one of the closed catalog: text, textarea, number, slider, select,
/// radio, multiselect, checkbox, date, static_text, editable_table.
class FormCard extends StatefulWidget {
  final Map<String, dynamic> data;
  final bool resolved;
  final void Function(Map<String, dynamic> values, bool submitted) onSubmit;

  const FormCard({
    super.key,
    required this.data,
    required this.resolved,
    required this.onSubmit,
  });

  @override
  State<FormCard> createState() => _FormCardState();
}

class _FormCardState extends State<FormCard> {
  final Map<String, dynamic> _values = {};

  List<Map<String, dynamic>> get _fields =>
      ((widget.data['fields'] as List?) ?? const [])
          .whereType<Map>()
          .map((f) => f.cast<String, dynamic>())
          .toList();

  @override
  void initState() {
    super.initState();
    // Seed values from each field's declared default (rows for tables).
    for (final f in _fields) {
      final key = (f['key'] ?? '').toString();
      if (key.isEmpty || f['type'] == 'static_text') continue;
      if (f['type'] == 'editable_table') {
        _values[key] = ((f['rows'] as List?) ?? const [])
            .whereType<Map>()
            .map((r) => Map<String, dynamic>.from(r))
            .toList();
      } else if (f['default'] != null) {
        _values[key] = f['default'];
      }
    }
  }

  bool _isEmpty(dynamic v) =>
      v == null || (v is String && v.isEmpty) || (v is List && v.isEmpty);

  int get _missingCount => _fields
      .where((f) =>
          f['type'] != 'static_text' &&
          f['required'] == true &&
          _isEmpty(_values[(f['key'] ?? '').toString()]))
      .length;

  void _set(String key, dynamic v) => setState(() => _values[key] = v);

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final title = (widget.data['title'] ?? '').toString();
    final submitLabel = (widget.data['submitLabel'] ?? 'Gửi').toString();
    final missing = _missingCount;
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
              const Icon(Icons.edit_note, color: AppTokens.cyan, size: 16),
              const SizedBox(width: 6),
              Expanded(
                child: Text(title,
                    style: TextStyle(
                        color: c.textPrimary,
                        fontSize: 13,
                        fontWeight: FontWeight.w600)),
              ),
            ],
          ),
          const SizedBox(height: 10),
          for (final f in _fields) _buildField(f),
          const SizedBox(height: 6),
          if (widget.resolved)
            _ResolvedRow(label: 'Đã gửi biểu mẫu')
          else
            Row(
              children: [
                Expanded(
                  child: FilledButton(
                    onPressed: missing == 0
                        ? () => widget.onSubmit(Map.of(_values), true)
                        : null,
                    style: FilledButton.styleFrom(
                      backgroundColor: AppTokens.cyan,
                      padding: const EdgeInsets.symmetric(vertical: 10),
                    ),
                    child: Text(missing == 0
                        ? submitLabel
                        : 'Còn thiếu $missing trường'),
                  ),
                ),
                const SizedBox(width: 8),
                OutlinedButton(
                  onPressed: () => widget.onSubmit(const {}, false),
                  style: OutlinedButton.styleFrom(
                    padding: const EdgeInsets.symmetric(
                        vertical: 10, horizontal: 14),
                    side: BorderSide(color: c.border),
                  ),
                  child: Text('Bỏ qua',
                      style: TextStyle(color: c.textSecondary, fontSize: 12)),
                ),
              ],
            ),
        ],
      ),
    );
  }

  Widget _buildField(Map<String, dynamic> f) {
    final c = context.colors;
    final type = (f['type'] ?? '').toString();
    if (type == 'static_text') {
      final variant = (f['variant'] ?? 'body').toString();
      final text = (f['text'] ?? '').toString();
      if (variant == 'divider') {
        return Divider(color: c.border, height: 20);
      }
      return Padding(
        padding: const EdgeInsets.only(bottom: 8),
        child: Text(text,
            style: variant == 'heading'
                ? TextStyle(
                    color: c.textPrimary,
                    fontSize: 13,
                    fontWeight: FontWeight.w600)
                : TextStyle(color: c.textMuted, fontSize: 12)),
      );
    }

    final key = (f['key'] ?? '').toString();
    final label = (f['label'] ?? '').toString();
    final required = f['required'] == true;
    final help = (f['help'] ?? '').toString();
    final disabled = widget.resolved;

    final labelRow = Padding(
      padding: const EdgeInsets.only(bottom: 4),
      child: Text.rich(
        TextSpan(
          text: label.toUpperCase(),
          style: TextStyle(
              color: c.textMuted,
              fontSize: 10,
              fontWeight: FontWeight.w600,
              letterSpacing: 0.8),
          children: [
            if (required)
              const TextSpan(
                  text: ' *', style: TextStyle(color: AppTokens.danger)),
          ],
        ),
      ),
    );

    Widget control;
    switch (type) {
      case 'text':
      case 'textarea':
      case 'number':
        control = TextFormField(
          enabled: !disabled,
          initialValue: (_values[key] ?? '').toString(),
          maxLines: type == 'textarea' ? ((f['rows'] as num?)?.toInt() ?? 4) : 1,
          maxLength: (f['maxLength'] as num?)?.toInt(),
          keyboardType:
              type == 'number' ? TextInputType.number : TextInputType.text,
          style: TextStyle(color: c.textPrimary, fontSize: 13),
          decoration: _inputDecoration(f['placeholder']?.toString()),
          onChanged: (v) {
            if (type == 'number') {
              final n = num.tryParse(v);
              _set(key, n);
            } else {
              _set(key, v);
            }
          },
        );
        break;
      case 'slider': {
        final min = (f['min'] as num?)?.toDouble() ?? 0;
        final max = (f['max'] as num?)?.toDouble() ?? 100;
        final step = (f['step'] as num?)?.toDouble() ?? 1;
        final value =
            ((_values[key] as num?)?.toDouble() ?? min).clamp(min, max);
        final divisions =
            step > 0 && max > min ? ((max - min) / step).round() : null;
        control = Row(
          children: [
            Expanded(
              child: Slider(
                value: value,
                min: min,
                max: max,
                divisions: divisions,
                activeColor: AppTokens.cyan,
                onChanged: disabled ? null : (v) => _set(key, v),
              ),
            ),
            SizedBox(
              width: 44,
              child: Text(
                value == value.roundToDouble()
                    ? value.round().toString()
                    : value.toStringAsFixed(1),
                textAlign: TextAlign.right,
                style: TextStyle(color: c.textPrimary, fontSize: 12),
              ),
            ),
          ],
        );
        break;
      }
      case 'select': {
        final options = _options(f);
        final current = (_values[key] ?? '').toString();
        control = DropdownButtonFormField<String>(
          initialValue:
              options.any((o) => o.$1 == current) ? current : null,
          items: [
            for (final (value, label) in options)
              DropdownMenuItem(
                  value: value,
                  child: Text(label,
                      style: TextStyle(color: c.textPrimary, fontSize: 13))),
          ],
          onChanged: disabled ? null : (v) => _set(key, v),
          decoration: _inputDecoration('Chọn…'),
          dropdownColor: c.surface,
        );
        break;
      }
      case 'radio':
      case 'multiselect': {
        final multi = type == 'multiselect';
        final options = _options(f);
        control = Wrap(
          spacing: 8,
          runSpacing: 6,
          children: [
            for (final (value, label) in options)
              _valueChip(
                label,
                multi
                    ? ((_values[key] as List?)?.contains(value) ?? false)
                    : _values[key] == value,
                disabled
                    ? null
                    : () {
                        if (multi) {
                          final list = List<dynamic>.of(
                              (_values[key] as List?) ?? const []);
                          list.contains(value)
                              ? list.remove(value)
                              : list.add(value);
                          _set(key, list);
                        } else {
                          _set(key, value);
                        }
                      },
              ),
          ],
        );
        break;
      }
      case 'checkbox':
        control = InkWell(
          onTap: disabled
              ? null
              : () => _set(key, !(_values[key] == true)),
          borderRadius: BorderRadius.circular(8),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(
                _values[key] == true
                    ? Icons.check_box
                    : Icons.check_box_outline_blank,
                color:
                    _values[key] == true ? AppTokens.cyan : c.textMuted,
                size: 18,
              ),
              const SizedBox(width: 6),
              Flexible(
                child: Text(label,
                    style: TextStyle(color: c.textPrimary, fontSize: 13)),
              ),
            ],
          ),
        );
        break;
      case 'date': {
        final current = (_values[key] ?? '').toString();
        control = InkWell(
          onTap: disabled
              ? null
              : () async {
                  final now = DateTime.now();
                  final picked = await showDatePicker(
                    context: context,
                    initialDate: DateTime.tryParse(current) ?? now,
                    firstDate: DateTime.tryParse(
                            (f['min'] ?? '').toString()) ??
                        DateTime(now.year - 10),
                    lastDate: DateTime.tryParse(
                            (f['max'] ?? '').toString()) ??
                        DateTime(now.year + 10),
                  );
                  if (picked != null) {
                    _set(key,
                        picked.toIso8601String().substring(0, 10));
                  }
                },
          borderRadius: BorderRadius.circular(8),
          child: Container(
            padding:
                const EdgeInsets.symmetric(horizontal: 10, vertical: 9),
            decoration: BoxDecoration(
              color: c.surfaceAlt,
              borderRadius: BorderRadius.circular(8),
              border: Border.all(color: c.border),
            ),
            child: Row(
              children: [
                Icon(Icons.calendar_today_outlined,
                    size: 14, color: c.textMuted),
                const SizedBox(width: 6),
                Text(current.isEmpty ? 'Chọn ngày…' : current,
                    style: TextStyle(
                        color: current.isEmpty
                            ? c.textMuted
                            : c.textPrimary,
                        fontSize: 13)),
              ],
            ),
          ),
        );
        break;
      }
      case 'editable_table': {
        final columns = ((f['columns'] as List?) ?? const [])
            .whereType<Map>()
            .toList();
        final rows =
            (_values[key] as List?)?.whereType<Map>().toList() ?? const [];
        final allowAdd = f['allowAddRow'] != false;
        control = Container(
          decoration: BoxDecoration(
            borderRadius: BorderRadius.circular(8),
            border: Border.all(color: c.border),
          ),
          child: Column(
            children: [
              for (var ri = 0; ri < rows.length; ri++)
                Padding(
                  padding:
                      const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
                  child: Row(
                    children: [
                      for (final col in columns) ...[
                        Expanded(
                          child: TextFormField(
                            enabled: !disabled,
                            initialValue:
                                (rows[ri][col['key']] ?? '').toString(),
                            keyboardType: col['type'] == 'number'
                                ? TextInputType.number
                                : TextInputType.text,
                            style: TextStyle(
                                color: c.textPrimary, fontSize: 12),
                            decoration: _inputDecoration(
                                (col['label'] ?? '').toString(),
                                dense: true),
                            onChanged: (v) {
                              final list = List<dynamic>.of(
                                  (_values[key] as List?) ?? const []);
                              final row = Map<String, dynamic>.from(
                                  list[ri] as Map);
                              row[(col['key'] ?? '').toString()] =
                                  col['type'] == 'number'
                                      ? (num.tryParse(v) ?? 0)
                                      : v;
                              list[ri] = row;
                              _set(key, list);
                            },
                          ),
                        ),
                        if (col != columns.last) const SizedBox(width: 6),
                      ],
                    ],
                  ),
                ),
              if (allowAdd && !disabled)
                TextButton.icon(
                  onPressed: () {
                    final list = List<dynamic>.of(
                        (_values[key] as List?) ?? const []);
                    list.add({
                      for (final col in columns)
                        (col['key'] ?? '').toString():
                            col['type'] == 'number' ? 0 : '',
                    });
                    _set(key, list);
                  },
                  icon: const Icon(Icons.add,
                      size: 14, color: AppTokens.cyan),
                  label: const Text('Thêm dòng',
                      style:
                          TextStyle(color: AppTokens.cyan, fontSize: 12)),
                ),
            ],
          ),
        );
        break;
      }
      default:
        control = Text('(trường không hỗ trợ: $type)',
            style: TextStyle(color: c.textMuted, fontSize: 12));
    }

    return Padding(
      padding: const EdgeInsets.only(bottom: 12),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          if (type != 'checkbox') labelRow,
          control,
          if (help.isNotEmpty)
            Padding(
              padding: const EdgeInsets.only(top: 3),
              child: Text(help,
                  style: TextStyle(color: c.textMuted, fontSize: 11)),
            ),
        ],
      ),
    );
  }

  List<(String, String)> _options(Map<String, dynamic> f) =>
      ((f['options'] as List?) ?? const [])
          .whereType<Map>()
          .map((o) => (
                (o['value'] ?? '').toString(),
                (o['label'] ?? '').toString(),
              ))
          .toList();

  InputDecoration _inputDecoration(String? hint, {bool dense = false}) {
    final c = context.colors;
    return InputDecoration(
      hintText: hint,
      hintStyle: TextStyle(color: c.textMuted, fontSize: dense ? 11 : 13),
      isDense: true,
      counterText: '',
      filled: true,
      fillColor: c.surfaceAlt,
      contentPadding:
          EdgeInsets.symmetric(horizontal: 10, vertical: dense ? 7 : 9),
      enabledBorder: OutlineInputBorder(
        borderRadius: BorderRadius.circular(8),
        borderSide: BorderSide(color: c.border),
      ),
      focusedBorder: OutlineInputBorder(
        borderRadius: BorderRadius.circular(8),
        borderSide: const BorderSide(color: AppTokens.cyan),
      ),
      disabledBorder: OutlineInputBorder(
        borderRadius: BorderRadius.circular(8),
        borderSide: BorderSide(color: c.border),
      ),
    );
  }

  Widget _valueChip(String label, bool selected, VoidCallback? onTap) {
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
          border: Border.all(color: selected ? AppTokens.cyan : c.border),
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
