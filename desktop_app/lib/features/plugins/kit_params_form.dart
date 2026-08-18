// Tham số một kit hỏi trước khi cài (`params[]` trong manifest).
// Parity với web `KitParamsForm.tsx`.
//
// Mỗi loại ra một control: string → ô chữ (secret → ô ẩn), number → ô số,
// boolean → công tắc, select → dropdown, folder → ô đường dẫn + nút chọn thư
// mục bằng hộp thoại hệ điều hành.
//
// Giá trị giữ nguyên kiểu JSON (number là số, boolean là bool) rồi gửi thẳng
// cho `/api/kits/install`; daemon mới là nơi kiểm tra thật sự — form chỉ dựng
// giá trị mặc định và chặn lỗi thấy ngay.

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';

import '../../core/i18n/l10n.dart';
import '../../theme/tokens.dart';

// ── Models ──────────────────────────────────────────────────────────────────

class KitParamOption {
  final String value;
  final String label;
  const KitParamOption(this.value, this.label);

  factory KitParamOption.fromJson(Map<String, dynamic> j) {
    final value = '${j['value'] ?? ''}';
    final label = '${j['label'] ?? ''}';
    return KitParamOption(value, label.isEmpty ? value : label);
  }
}

class KitParam {
  final String key;
  final String label;

  /// string | number | boolean | select | folder
  final String type;
  final String description;
  final String placeholder;
  final Object? defaultValue;
  final bool required;

  /// Hiện dạng ẩn, và daemon không ghi vào sổ biên nhận.
  final bool secret;
  final List<KitParamOption> options;
  final double? min;
  final double? max;
  final double? step;

  const KitParam({
    required this.key,
    required this.label,
    required this.type,
    this.description = '',
    this.placeholder = '',
    this.defaultValue,
    this.required = false,
    this.secret = false,
    this.options = const [],
    this.min,
    this.max,
    this.step,
  });

  factory KitParam.fromJson(Map<String, dynamic> j) {
    final key = '${j['key'] ?? ''}';
    final label = '${j['label'] ?? ''}';
    return KitParam(
      key: key,
      label: label.isEmpty ? key : label,
      type: '${j['type'] ?? 'string'}',
      description: '${j['description'] ?? ''}',
      placeholder: '${j['placeholder'] ?? ''}',
      defaultValue: j['default'],
      required: j['required'] == true,
      secret: j['secret'] == true,
      options: [
        for (final o in (j['options'] as List? ?? const []))
          if (o is Map) KitParamOption.fromJson(o.cast<String, dynamic>()),
      ],
      min: (j['min'] as num?)?.toDouble(),
      max: (j['max'] as num?)?.toDouble(),
      step: (j['step'] as num?)?.toDouble(),
    );
  }
}

/// Giá trị khởi tạo lấy từ `default` của từng tham số.
Map<String, dynamic> initialAnswers(List<KitParam> params) {
  final out = <String, dynamic>{};
  for (final p in params) {
    if (p.defaultValue != null) {
      out[p.key] = p.defaultValue;
      continue;
    }
    // Không có default: công tắc vẫn phải có trạng thái; các loại khác để
    // trống để daemon phân biệt "chưa trả lời" với "trả lời rỗng".
    if (p.type == 'boolean') out[p.key] = false;
  }
  return out;
}

/// Tham số bắt buộc còn trống — để khoá nút Cài trước khi gọi mạng.
List<KitParam> missingRequired(
    List<KitParam> params, Map<String, dynamic> answers) {
  return [
    for (final p in params)
      if (p.required)
        if (answers[p.key] == null ||
            (answers[p.key] is String &&
                (answers[p.key] as String).trim().isEmpty))
          p,
  ];
}

// ── Form ────────────────────────────────────────────────────────────────────

class KitParamsForm extends StatelessWidget {
  const KitParamsForm({
    super.key,
    required this.params,
    required this.answers,
    required this.onChanged,
  });

  final List<KitParam> params;
  final Map<String, dynamic> answers;
  final ValueChanged<Map<String, dynamic>> onChanged;

  void _set(String key, Object? value) =>
      onChanged({...answers, key: value});

  @override
  Widget build(BuildContext context) {
    if (params.isEmpty) return const SizedBox.shrink();
    final c = context.colors;
    return Container(
      padding: const EdgeInsets.all(AppTokens.s12),
      decoration: BoxDecoration(
        color: c.surfaceAlt,
        border: Border.all(color: c.border),
        borderRadius: BorderRadius.circular(AppTokens.rMd),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(context.tr('Install parameters'),
              style: TextStyle(
                  color: c.textPrimary,
                  fontSize: 13,
                  fontWeight: FontWeight.w700)),
          Padding(
            padding: const EdgeInsets.only(top: 2, bottom: AppTokens.s8),
            child: Text(
              context.trArgs(
                  'This kit asks for {n} value(s) before installing. They are '
                  'substituted into {{param.<key>}} in the manifest.',
                  {'n': params.length}),
              style: TextStyle(color: c.textMuted, fontSize: 11),
            ),
          ),
          for (final p in params)
            Padding(
              padding: const EdgeInsets.only(bottom: AppTokens.s12),
              child: _ParamField(
                param: p,
                value: answers[p.key],
                onChanged: (v) => _set(p.key, v),
              ),
            ),
        ],
      ),
    );
  }
}

class _ParamField extends StatelessWidget {
  const _ParamField({
    required this.param,
    required this.value,
    required this.onChanged,
  });

  final KitParam param;
  final Object? value;
  final ValueChanged<Object?> onChanged;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Wrap(
          spacing: AppTokens.s6,
          runSpacing: AppTokens.s4,
          crossAxisAlignment: WrapCrossAlignment.center,
          children: [
            Text(param.label,
                style: TextStyle(color: c.textPrimary, fontSize: 12)),
            if (param.required)
              _Pill(label: context.tr('required'), color: AppTokens.danger),
            if (param.secret)
              _Pill(label: context.tr('secret'), color: AppTokens.brandAlt),
            Text('{{param.${param.key}}}',
                style: TextStyle(
                    color: c.textMuted,
                    fontSize: 10,
                    fontFamily: AppTokens.fontMono)),
          ],
        ),
        const SizedBox(height: AppTokens.s4),
        _control(context),
        if (param.description.isNotEmpty)
          Padding(
            padding: const EdgeInsets.only(top: 3),
            child: Text(param.description,
                style: TextStyle(color: c.textMuted, fontSize: 11)),
          ),
      ],
    );
  }

  Widget _control(BuildContext context) {
    switch (param.type) {
      case 'boolean':
        return Switch(
          value: value == true || value == 'true',
          onChanged: (v) => onChanged(v),
        );

      case 'number':
        return SizedBox(
          width: 200,
          child: _TextControl(
            key: ValueKey('num:${param.key}'),
            initial: value?.toString() ?? '',
            hint: param.placeholder,
            numeric: true,
            // Ô trống phải thành null, không phải 0 — daemon cần phân biệt
            // "chưa trả lời" để dùng default.
            onChanged: (t) {
              final trimmed = t.trim();
              if (trimmed.isEmpty) return onChanged(null);
              final n = num.tryParse(trimmed);
              onChanged(n ?? trimmed);
            },
          ),
        );

      case 'select':
        final current = value?.toString();
        final known = param.options.any((o) => o.value == current);
        return SizedBox(
          width: 260,
          child: DropdownButtonFormField<String>(
            initialValue: known ? current : null,
            isDense: true,
            decoration: _decoration(context, param.placeholder),
            items: [
              for (final o in param.options)
                DropdownMenuItem(value: o.value, child: Text(o.label)),
            ],
            onChanged: (v) => onChanged(v),
          ),
        );

      case 'folder':
        return Row(
          children: [
            SizedBox(
              width: 360,
              child: _TextControl(
                key: ValueKey('dir:${param.key}'),
                initial: value?.toString() ?? '',
                hint: param.placeholder.isEmpty
                    ? '~/Projects/…'
                    : param.placeholder,
                mono: true,
                onChanged: onChanged,
              ),
            ),
            const SizedBox(width: AppTokens.s8),
            OutlinedButton.icon(
              onPressed: () async {
                final picked = await FilePicker.platform.getDirectoryPath(
                  dialogTitle: context.trArgs(
                      'Choose a folder for "{name}"', {'name': param.label}),
                );
                if (picked != null) onChanged(picked);
              },
              icon: const Icon(Icons.folder_open, size: 15),
              label: Text(context.tr('Browse…'),
                  style: const TextStyle(fontSize: 12)),
            ),
          ],
        );

      default:
        return SizedBox(
          width: 360,
          child: _TextControl(
            key: ValueKey('str:${param.key}'),
            initial: value?.toString() ?? '',
            hint: param.placeholder,
            obscure: param.secret,
            onChanged: onChanged,
          ),
        );
    }
  }
}

InputDecoration _decoration(BuildContext context, String hint) {
  final c = context.colors;
  return InputDecoration(
    isDense: true,
    hintText: hint.isEmpty ? null : hint,
    hintStyle: TextStyle(color: c.textMuted, fontSize: 12),
    contentPadding: const EdgeInsets.symmetric(
        horizontal: AppTokens.s8, vertical: AppTokens.s8),
    border: OutlineInputBorder(
        borderRadius: BorderRadius.circular(AppTokens.rMd),
        borderSide: BorderSide(color: c.border)),
    enabledBorder: OutlineInputBorder(
        borderRadius: BorderRadius.circular(AppTokens.rMd),
        borderSide: BorderSide(color: c.border)),
  );
}

/// Ô chữ giữ con trỏ khi cha rebuild.
///
/// Dùng `TextEditingController` nội bộ thay vì `TextFormField(initialValue:)`:
/// mỗi lần gõ, form cha nhận giá trị mới và rebuild, và một `initialValue` mới
/// sẽ nhảy con trỏ về đầu dòng sau từng ký tự. Chỉ đồng bộ lại khi giá trị
/// ngoài thật sự khác (ví dụ vừa chọn thư mục xong).
class _TextControl extends StatefulWidget {
  const _TextControl({
    super.key,
    required this.initial,
    required this.onChanged,
    this.hint = '',
    this.obscure = false,
    this.numeric = false,
    this.mono = false,
  });

  final String initial;
  final ValueChanged<String> onChanged;
  final String hint;
  final bool obscure;
  final bool numeric;
  final bool mono;

  @override
  State<_TextControl> createState() => _TextControlState();
}

class _TextControlState extends State<_TextControl> {
  late final TextEditingController _controller =
      TextEditingController(text: widget.initial);

  @override
  void didUpdateWidget(covariant _TextControl old) {
    super.didUpdateWidget(old);
    if (widget.initial != _controller.text) {
      _controller.text = widget.initial;
      _controller.selection =
          TextSelection.collapsed(offset: _controller.text.length);
    }
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return TextField(
      controller: _controller,
      obscureText: widget.obscure,
      keyboardType: widget.numeric
          ? const TextInputType.numberWithOptions(decimal: true)
          : null,
      style: TextStyle(
        color: c.textPrimary,
        fontSize: 12,
        fontFamily: widget.mono ? AppTokens.fontMono : null,
      ),
      decoration: _decoration(context, widget.hint),
      onChanged: widget.onChanged,
    );
  }
}

class _Pill extends StatelessWidget {
  const _Pill({required this.label, required this.color});
  final String label;
  final Color color;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: AppTokens.s6, vertical: 1),
      decoration: BoxDecoration(
        color: color.withValues(alpha: 0.12),
        borderRadius: BorderRadius.circular(AppTokens.rSm),
        border: Border.all(color: color.withValues(alpha: 0.35)),
      ),
      child: Text(label,
          style: TextStyle(
              color: color, fontSize: 10, fontWeight: FontWeight.w500)),
    );
  }
}
