import 'package:flutter/material.dart';
import '../../../models/chat_message.dart';
import '../../../theme/tokens.dart';

/// Inline card for a FormUI declarative form (parity with the web/channel_app
/// FormCard). `message.data` is the `form:request` payload:
/// `{requestId, agentId, title, surface, submitLabel, fields:[…]}` where each
/// field is one of the closed catalog: text, textarea, number, slider, select,
/// radio, multiselect, checkbox, date, static_text, editable_table.
class FormCard extends StatefulWidget {
  const FormCard({super.key, required this.message, required this.onSubmit});
  final ChatMessage message;
  final void Function(
    String requestId,
    Map<String, dynamic> values,
    bool submitted,
  ) onSubmit;

  @override
  State<FormCard> createState() => _FormCardState();
}

class _FormCardState extends State<FormCard> {
  final Map<String, dynamic> _values = {};

  List<Map<String, dynamic>> get _fields =>
      ((widget.message.data['fields'] as List?) ?? const [])
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
    final resolved = widget.message.resolved;
    final title = '${widget.message.data['title'] ?? ''}';
    final submitLabel = '${widget.message.data['submitLabel'] ?? 'Submit'}';
    final missing = _missingCount;
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
                Icon(Icons.edit_note, size: 16, color: c.accent),
                const SizedBox(width: AppTokens.s8),
                Expanded(
                  child: Text(
                    title.isNotEmpty ? title : 'Form',
                    style: TextStyle(
                      color: c.textPrimary,
                      fontWeight: FontWeight.w700,
                    ),
                  ),
                ),
              ],
            ),
            const SizedBox(height: AppTokens.s12),
            for (final f in _fields) _buildField(f, resolved),
            const SizedBox(height: AppTokens.s4),
            if (resolved)
              Text('Submitted',
                  style: TextStyle(color: c.textMuted, fontSize: 12))
            else
              Row(
                children: [
                  Expanded(
                    child: FilledButton(
                      onPressed: missing == 0
                          ? () => widget.onSubmit(
                              widget.message.requestId, Map.of(_values), true)
                          : null,
                      child: Text(missing == 0
                          ? submitLabel
                          : '$missing required field${missing == 1 ? '' : 's'} left'),
                    ),
                  ),
                  const SizedBox(width: AppTokens.s8),
                  OutlinedButton(
                    onPressed: () => widget.onSubmit(
                        widget.message.requestId, const {}, false),
                    child: Text('Skip',
                        style:
                            TextStyle(color: c.textSecondary, fontSize: 12)),
                  ),
                ],
              ),
          ],
        ),
      ),
    );
  }

  Widget _buildField(Map<String, dynamic> f, bool resolved) {
    final c = context.colors;
    final type = (f['type'] ?? '').toString();
    if (type == 'static_text') {
      final variant = (f['variant'] ?? 'body').toString();
      final text = (f['text'] ?? '').toString();
      if (variant == 'divider') {
        return Divider(color: c.border, height: 20);
      }
      return Padding(
        padding: const EdgeInsets.only(bottom: AppTokens.s8),
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
    final disabled = resolved;

    final labelRow = Padding(
      padding: const EdgeInsets.only(bottom: AppTokens.s4),
      child: Text.rich(
        TextSpan(
          text: label.toUpperCase(),
          style: TextStyle(
              color: c.accent,
              fontSize: 12,
              fontWeight: FontWeight.w700,
              letterSpacing: 0.5),
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
              _set(key, num.tryParse(v));
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
                activeColor: c.accent,
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
          decoration: _inputDecoration('Select…'),
          dropdownColor: c.surface,
        );
        break;
      }
      case 'radio':
      case 'multiselect': {
        final multi = type == 'multiselect';
        final options = _options(f);
        control = Wrap(
          spacing: AppTokens.s8,
          runSpacing: AppTokens.s6,
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
          onTap: disabled ? null : () => _set(key, !(_values[key] == true)),
          borderRadius: BorderRadius.circular(AppTokens.rLg),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(
                _values[key] == true
                    ? Icons.check_box
                    : Icons.check_box_outline_blank,
                color: _values[key] == true ? c.accent : c.textMuted,
                size: 18,
              ),
              const SizedBox(width: AppTokens.s6),
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
                    firstDate:
                        DateTime.tryParse((f['min'] ?? '').toString()) ??
                            DateTime(now.year - 10),
                    lastDate:
                        DateTime.tryParse((f['max'] ?? '').toString()) ??
                            DateTime(now.year + 10),
                  );
                  if (picked != null) {
                    _set(key, picked.toIso8601String().substring(0, 10));
                  }
                },
          borderRadius: BorderRadius.circular(AppTokens.rLg),
          child: Container(
            padding:
                const EdgeInsets.symmetric(horizontal: 10, vertical: 9),
            decoration: BoxDecoration(
              color: c.surfaceAlt,
              borderRadius: BorderRadius.circular(AppTokens.rLg),
              border: Border.all(color: c.border),
            ),
            child: Row(
              children: [
                Icon(Icons.calendar_today_outlined,
                    size: 14, color: c.textMuted),
                const SizedBox(width: AppTokens.s6),
                Text(current.isEmpty ? 'Pick a date…' : current,
                    style: TextStyle(
                        color: current.isEmpty ? c.textMuted : c.textPrimary,
                        fontSize: 13)),
              ],
            ),
          ),
        );
        break;
      }
      case 'editable_table': {
        final columns =
            ((f['columns'] as List?) ?? const []).whereType<Map>().toList();
        final rows =
            (_values[key] as List?)?.whereType<Map>().toList() ?? const [];
        final allowAdd = f['allowAddRow'] != false;
        control = Container(
          decoration: BoxDecoration(
            borderRadius: BorderRadius.circular(AppTokens.rLg),
            border: Border.all(color: c.border),
          ),
          child: Column(
            children: [
              for (var ri = 0; ri < rows.length; ri++)
                Padding(
                  padding: const EdgeInsets.symmetric(
                      horizontal: AppTokens.s8, vertical: AppTokens.s4),
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
                        if (col != columns.last)
                          const SizedBox(width: AppTokens.s6),
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
                  icon: Icon(Icons.add, size: 14, color: c.accent),
                  label: Text('Add row',
                      style: TextStyle(color: c.accent, fontSize: 12)),
                ),
            ],
          ),
        );
        break;
      }
      default:
        control = Text('(unsupported field: $type)',
            style: TextStyle(color: c.textMuted, fontSize: 12));
    }

    return Padding(
      padding: const EdgeInsets.only(bottom: AppTokens.s12),
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
        borderRadius: BorderRadius.circular(AppTokens.rLg),
        borderSide: BorderSide(color: c.border),
      ),
      focusedBorder: OutlineInputBorder(
        borderRadius: BorderRadius.circular(AppTokens.rLg),
        borderSide: BorderSide(color: c.accent),
      ),
      disabledBorder: OutlineInputBorder(
        borderRadius: BorderRadius.circular(AppTokens.rLg),
        borderSide: BorderSide(color: c.border),
      ),
    );
  }

  Widget _valueChip(String label, bool selected, VoidCallback? onTap) {
    final c = context.colors;
    return InkWell(
      onTap: onTap,
      borderRadius: BorderRadius.circular(AppTokens.rFull),
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
            if (selected)
              Padding(
                padding: const EdgeInsets.only(right: AppTokens.s4),
                child: Icon(Icons.check, color: c.accent, size: 14),
              ),
            Text(label,
                style: TextStyle(
                    color: selected ? c.textPrimary : c.textSecondary,
                    fontSize: 13)),
          ],
        ),
      ),
    );
  }
}
