// Hộp thoại cài kit: chọn nguồn → xem trước → cài → báo cáo.
//
// Xem trước là bước đồng ý, không phải trang trí. Một kit có thể mang theo Space
// App — tức kéo hẳn một chương trình về máy — nên người dùng phải thấy chính xác
// nó chứa gì TRƯỚC khi bấm cài. Vì thế mọi đường vào (chọn tệp, dán manifest,
// lấy từ marketplace) đều đi qua đúng hộp thoại này.
//
//   POST /api/kits/preview            multipart .zip/.json, hoặc JSON manifest
//   POST /api/kits/install            như trên, trả báo cáo từng mục
//   POST /api/kits/available/preview  {sourceId, name} — kit trong catalog
//   POST /api/kits/available/install  như trên
//
// Parity với web `KitInstallDialog.tsx`.

import 'dart:convert';
import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/foundation.dart' show kIsWeb;
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:http/http.dart' as http;

import '../../core/i18n/l10n.dart';
import '../../core/transport/api_client.dart' show ApiException;
import '../../core/transport/connection.dart';
import '../../theme/tokens.dart';
import 'kit_params_form.dart';
import 'kits_panel.dart';

/// Kit đến từ đâu. Từ marketplace thì đã biết tên nên bỏ qua bước chọn tệp.
class KitInstallSource {
  final String? sourceId;
  final String? sourceName;
  final String? name;

  const KitInstallSource.local()
      : sourceId = null,
        sourceName = null,
        name = null;

  const KitInstallSource.market({
    required String this.sourceId,
    required String this.sourceName,
    required String this.name,
  });

  bool get isMarket => sourceId != null;
}

Future<void> showKitInstallDialog(
  BuildContext context, {
  KitInstallSource source = const KitInstallSource.local(),
}) =>
    showDialog<void>(
      context: context,
      barrierDismissible: false,
      builder: (_) => _KitInstallDialog(source: source),
    );

class _KitInstallDialog extends ConsumerStatefulWidget {
  const _KitInstallDialog({required this.source});
  final KitInstallSource source;

  @override
  ConsumerState<_KitInstallDialog> createState() => _KitInstallDialogState();
}

class _KitInstallDialogState extends ConsumerState<_KitInstallDialog> {
  final _controller = TextEditingController();

  /// Tệp đã chọn, giữ nguyên bytes để lần cài gửi lại đúng thứ đã xem trước.
  String? _fileName;
  List<int>? _fileBytes;

  KitPreview? _preview;
  String? _error;
  bool _previewing = false;
  bool _installing = false;
  KitReport? _report;

  Map<String, dynamic> _answers = {};
  String _seededSig = '';

  @override
  void initState() {
    super.initState();
    // Kit từ marketplace đã đủ thông tin để xem trước ngay khi mở.
    if (widget.source.isMarket) {
      WidgetsBinding.instance.addPostFrameCallback((_) => _runPreview());
    }
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  // ── Gửi request ───────────────────────────────────────────────────────────

  /// Gửi kit đi theo đúng dạng nó đến: tệp thì multipart, dán tay thì JSON.
  Future<Map<String, dynamic>> _send(String path, {bool force = false}) async {
    final api = ref.read(apiClientProvider);

    if (widget.source.isMarket) {
      final r = await api.post('/api/kits/available/$path', body: {
        'sourceId': widget.source.sourceId,
        'name': widget.source.name,
        'params': _answers,
        'force': force,
      });
      if (r is! Map) throw Exception(kitOldDaemonMsg);
      return r.cast<String, dynamic>();
    }

    if (_fileBytes != null) {
      final cfg = ref.read(appConfigProvider);
      final uri = Uri.parse('${cfg.httpBase}/api/kits/$path');
      final req = http.MultipartRequest('POST', uri)
        ..headers.addAll(cfg.authHeaders)
        ..fields['params'] = jsonEncode(_answers)
        ..fields['force'] = '$force'
        ..files.add(http.MultipartFile.fromBytes('file', _fileBytes!,
            filename: _fileName ?? 'kit.zip'));
      final streamed = await req.send();
      final body = await streamed.stream.bytesToString();
      final decoded = body.isEmpty ? null : jsonDecode(body);
      if (decoded is! Map) throw Exception(kitOldDaemonMsg);
      final map = decoded.cast<String, dynamic>();
      // Lỗi của daemon là `{"error": "..."}`; đọc nó ra thay vì chỉ báo mã số.
      if (streamed.statusCode >= 300) {
        throw Exception('${map['error'] ?? 'HTTP ${streamed.statusCode}'}');
      }
      return map;
    }

    final text = _controller.text.trim();
    // Parse tại chỗ trước: JSON hỏng không đáng gọi mạng, và lỗi của Dart chỉ
    // rõ vị trí khi tự parse.
    final decoded = jsonDecode(text);
    final body = kitRequestBody(decoded, _answers);
    final r = await api.post('/api/kits/$path',
        body: {...(body as Map).cast<String, dynamic>(), 'force': force});
    if (r is! Map) throw Exception(kitOldDaemonMsg);
    return r.cast<String, dynamic>();
  }

  String _friendly(Object e) {
    if (e is ApiException) return e.message;
    return '$e'.replaceFirst('Exception: ', '');
  }

  // ── Xem trước ─────────────────────────────────────────────────────────────

  Future<void> _runPreview() async {
    if (!_hasKit) return;
    setState(() {
      _previewing = true;
      _error = null;
    });
    try {
      final map = await _send('preview');
      if (!mounted) return;
      final preview = KitPreview.fromJson(map);
      setState(() {
        _preview = preview;
        _error = null;
        // Bộ tham số đổi (chọn kit khác) → nạp lại mặc định. Cùng một bộ thì
        // giữ nguyên những gì người dùng đã điền.
        final sig = preview.params.map((p) => '${p.key}:${p.type}').join(',');
        if (sig != _seededSig) {
          _seededSig = sig;
          _answers = initialAnswers(preview.params);
        }
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _preview = null;
        _error = _friendly(e);
      });
    } finally {
      if (mounted) setState(() => _previewing = false);
    }
  }

  Future<void> _pickFile() async {
    final FilePickerResult? res;
    try {
      res = await FilePicker.platform.pickFiles(
        type: FileType.custom,
        allowedExtensions: const ['zip', 'json'],
        withData: true,
      );
    } catch (e) {
      // file_picker lỗi trước cả khi mở bảng chọn (ví dụ thiếu entitlement
      // macOS) — im lặng thì người dùng tưởng bấm hụt.
      if (mounted) {
        setState(
            () => _error = context.trArgs('File picker error: {e}', {'e': e}));
      }
      return;
    }
    final f = res?.files.firstOrNull;
    if (f == null) return;

    List<int>? bytes = f.bytes;
    if (bytes == null && !kIsWeb && f.path != null) {
      bytes = await File(f.path!).readAsBytes();
    }
    if (bytes == null) return;

    setState(() {
      _fileName = f.name;
      _fileBytes = bytes;
      _controller.clear();
      _preview = null;
      _seededSig = '';
      _answers = {};
    });
    // Chọn tệp chính là ý định "xem cái này".
    await _runPreview();
  }

  // ── Cài ───────────────────────────────────────────────────────────────────

  Future<void> _install({bool force = false}) async {
    setState(() => _installing = true);
    try {
      final map = await _send('install', force: force);
      final report = (map['report'] as Map?)?.cast<String, dynamic>();
      if (!mounted) return;
      setState(() {
        // `ok:false` vẫn kèm report: kit cài dở còn xem được vẫn hơn lỗi mù.
        _report =
            report == null ? null : KitReport.fromJson(report, removal: false);
      });
      // Kể cả cài một phần, danh sách vẫn phải làm mới.
      ref.invalidate(kitsProvider);
      ref.invalidate(availableKitsProvider);
    } catch (e) {
      if (!mounted) return;
      setState(() => _error = _friendly(e));
    } finally {
      if (mounted) setState(() => _installing = false);
    }
  }

  // ── Trạng thái ────────────────────────────────────────────────────────────

  bool get _hasKit =>
      widget.source.isMarket ||
      _fileBytes != null ||
      _controller.text.trim().isNotEmpty;

  bool get _canInstall {
    final p = _preview;
    if (p == null) return false;
    return missingRequired(p.params, _answers).isEmpty && p.paramError == null;
  }

  /// App bị bước quét bảo mật chặn — cho phép cài lại có chủ đích, đúng cách
  /// trang Space Apps làm.
  List<KitOutcome> get _blockedApps => [
        for (final i in _report?.items ?? const <KitOutcome>[])
          if (i.type == 'app' &&
              i.status == 'failed' &&
              (i.detail ?? '').contains('security scan'))
            i,
      ];

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final report = _report;

    return AlertDialog(
      backgroundColor: c.surface,
      title: Row(children: [
        Icon(
            report == null
                ? Icons.rocket_launch_outlined
                : Icons.receipt_long_outlined,
            size: 18,
            color: c.textPrimary),
        const SizedBox(width: AppTokens.s8),
        Expanded(
          child: Text(
            report == null
                ? context.tr('Install kit')
                : context.tr('Install result'),
            style: TextStyle(
                fontSize: 16, fontWeight: FontWeight.w700, color: c.textPrimary),
          ),
        ),
        if (widget.source.isMarket && report == null)
          _Pill(text: widget.source.sourceName!, color: AppTokens.brand),
      ]),
      content: SizedBox(
        width: 620,
        child: SingleChildScrollView(
          child: report != null
              ? KitReportCard(
                  report: report,
                  onClose: () => Navigator.of(context).pop(),
                )
              : Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    if (!widget.source.isMarket) ...[
                      _picker(context),
                      const SizedBox(height: AppTokens.s12),
                    ],
                    if (_error != null) ...[
                      _ErrorBox(message: _error!),
                      const SizedBox(height: AppTokens.s12),
                    ],
                    if (_previewing)
                      const Padding(
                        padding: EdgeInsets.symmetric(vertical: AppTokens.s16),
                        child: Center(
                            child: SizedBox(
                                width: 20,
                                height: 20,
                                child:
                                    CircularProgressIndicator(strokeWidth: 2))),
                      )
                    else if (_preview != null)
                      _KitSummary(
                        preview: _preview!,
                        answers: _answers,
                        onChanged: (next) {
                          setState(() => _answers = next);
                          _runPreview();
                        },
                      )
                    else if (!_hasKit)
                      Text(
                        context.tr(
                            'Choose a .zip / .json file, or paste a manifest, to see what the kit contains'),
                        style: TextStyle(fontSize: 12, color: c.textMuted),
                      ),
                  ],
                ),
        ),
      ),
      actions: report != null
          ? [
              if (_blockedApps.isNotEmpty)
                TextButton(
                  onPressed: _installing
                      ? null
                      : () {
                          setState(() => _report = null);
                          _install(force: true);
                        },
                  child: Text(
                    context.tr('Install again, ignoring the security warning'),
                    style: const TextStyle(color: AppTokens.danger),
                  ),
                ),
              FilledButton(
                onPressed: () => Navigator.of(context).pop(),
                child: Text(context.tr('Close')),
              ),
            ]
          : [
              TextButton(
                onPressed:
                    _installing ? null : () => Navigator.of(context).pop(),
                child: Text(context.tr('Cancel')),
              ),
              FilledButton.icon(
                onPressed: _canInstall && !_installing ? () => _install() : null,
                icon: _installing
                    ? const SizedBox(
                        width: 14,
                        height: 14,
                        child: CircularProgressIndicator(strokeWidth: 2))
                    : const Icon(Icons.rocket_launch_outlined, size: 16),
                label: Text(context.tr('Install kit')),
              ),
            ],
    );
  }

  Widget _picker(BuildContext context) {
    final c = context.colors;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        OutlinedButton.icon(
          onPressed: _previewing ? null : _pickFile,
          icon: const Icon(Icons.folder_open_outlined, size: 16),
          label: Text(_fileName ?? context.tr('Choose .zip or .json file…')),
        ),
        const SizedBox(height: AppTokens.s6),
        Text(
          context.tr(
              '.zip — the whole kit: kit.json declares it, with skills/, workflows/ and apps/ '
              'carrying the actual files. .json — the manifest only, which cannot carry an app.'),
          style: TextStyle(fontSize: 11, color: c.textMuted, height: 1.4),
        ),
        if (_fileBytes == null) ...[
          const SizedBox(height: AppTokens.s12),
          Text(context.tr('or paste a manifest'),
              style: TextStyle(fontSize: 11, color: c.textMuted)),
          const SizedBox(height: AppTokens.s6),
          TextField(
            controller: _controller,
            maxLines: 6,
            minLines: 4,
            style: TextStyle(
                fontFamily: 'monospace', fontSize: 12, color: c.textPrimary),
            decoration: InputDecoration(
              isDense: true,
              hintText: '{\n  "manifest": 2,\n  "id": "daily-report"\n}',
              hintStyle: TextStyle(
                  fontFamily: 'monospace', fontSize: 12, color: c.textMuted),
              border: OutlineInputBorder(
                  borderRadius: BorderRadius.circular(AppTokens.rMd),
                  borderSide: BorderSide(color: c.border)),
            ),
            onChanged: (_) => setState(() {}),
            onEditingComplete: _runPreview,
          ),
          const SizedBox(height: AppTokens.s6),
          TextButton(
            onPressed: _controller.text.trim().isEmpty || _previewing
                ? null
                : _runPreview,
            child: Text(context.tr('Preview')),
          ),
        ],
      ],
    );
  }
}

// ── Tóm tắt kit ─────────────────────────────────────────────────────────────

class _KitSummary extends StatelessWidget {
  const _KitSummary({
    required this.preview,
    required this.answers,
    required this.onChanged,
  });

  final KitPreview preview;
  final Map<String, dynamic> answers;
  final ValueChanged<Map<String, dynamic>> onChanged;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final apps = preview.bundle.apps;

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        Text(
            '${preview.name.isEmpty ? preview.id : preview.name}  v${preview.version}',
            style: TextStyle(
                fontSize: 15,
                fontWeight: FontWeight.w700,
                color: c.textPrimary)),
        const SizedBox(height: 2),
        Text(
          '${preview.id} · ${preview.bundle.hasFiles ? context.tr('.zip bundle') : context.tr('JSON manifest')}',
          style: TextStyle(fontSize: 11, color: c.textMuted),
        ),
        if (preview.description.isNotEmpty) ...[
          const SizedBox(height: AppTokens.s6),
          Text(preview.description,
              style:
                  TextStyle(fontSize: 12, color: c.textSecondary, height: 1.4)),
        ],
        if (preview.installedVersion != null) ...[
          const SizedBox(height: AppTokens.s12),
          _Notice(
            color: AppTokens.brand,
            icon: Icons.info_outline,
            title: context.trArgs('This kit is already installed (v{v})',
                {'v': preview.installedVersion!}),
            body: context.tr(
                'Installing again will not overwrite: anything with a matching name is left '
                'as it is and reported as “already there”.'),
          ),
        ],
        const SizedBox(height: AppTokens.s12),
        _ItemList(
          items: preview.items,
          summary: [
            for (final kind in KitPreview.installableKinds)
              if ((preview.counts[kind] ?? 0) > 0)
                '${preview.counts[kind]} ${kitKindLabel(context, kind)}',
          ].join(', '),
        ),

        // App là thứ đáng nhìn kỹ nhất: cài kit đồng nghĩa kéo cả chương trình
        // về máy, nên nó tách riêng chứ không lẫn vào hàng thẻ đếm.
        if (apps.isNotEmpty) ...[
          const SizedBox(height: AppTokens.s12),
          _Notice(
            color: AppTokens.warning,
            icon: Icons.apps_outlined,
            title: context.trArgs(
                'This kit installs {n} Space App(s) on your machine',
                {'n': apps.length}),
            body: [
              for (final a in apps) '${a.id} · ${_bytes(a.bytes)}',
              context.tr(
                  'Each app goes through the pre-install security scan; a blocked app is '
                  'reported on its own and the rest of the kit still installs.'),
            ].join('\n'),
          ),
        ],

        if (preview.bundle.skills.isNotEmpty ||
            preview.bundle.workflows.isNotEmpty) ...[
          const SizedBox(height: AppTokens.s12),
          if (preview.bundle.skills.isNotEmpty)
            _FileLine(
                label: context.tr('Skills in the bundle'),
                value: preview.bundle.skills.join(', ')),
          if (preview.bundle.workflows.isNotEmpty)
            _FileLine(
                label: context.tr('Workflows in the bundle'),
                value: preview.bundle.workflows.join(', ')),
        ],

        if (preview.warnings.isNotEmpty) ...[
          const SizedBox(height: AppTokens.s12),
          KitWarningBox(
            message: [
              for (final w in preview.warnings) '${w.subject}: ${w.detail}',
            ].join('\n'),
          ),
        ],

        if (preview.params.isNotEmpty) ...[
          const SizedBox(height: AppTokens.s16),
          Text(context.tr('The kit needs you to fill in'),
              style: TextStyle(
                  fontSize: 13,
                  fontWeight: FontWeight.w600,
                  color: c.textPrimary)),
          const SizedBox(height: AppTokens.s8),
          KitParamsForm(
              params: preview.params, answers: answers, onChanged: onChanged),
          if (preview.paramError != null) ...[
            const SizedBox(height: AppTokens.s8),
            _ErrorBox(message: preview.paramError!),
          ],
        ],
      ],
    );
  }

  static String _bytes(int n) {
    if (n < 1024) return '$n B';
    if (n < 1024 * 1024) return '${(n / 1024).toStringAsFixed(0)} KB';
    return '${(n / 1024 / 1024).toStringAsFixed(1)} MB';
  }
}

// ── Danh sách từng mục ──────────────────────────────────────────────────────

IconData _itemIcon(String type) => switch (type) {
      'agent' => Icons.person_outline,
      'skill' => Icons.bolt_outlined,
      'workflow' => Icons.account_tree_outlined,
      'hook' => Icons.webhook_outlined,
      'job' => Icons.schedule_outlined,
      'app' => Icons.apps_outlined,
      'mcpServer' => Icons.dns_outlined,
      _ => Icons.circle_outlined,
    };

/// Dòng phụ của một mục — ghép ở client vì nhãn phải theo ngôn ngữ.
String _itemSubtitle(BuildContext context, KitPreviewItem item) {
  final parts = <String>[];
  switch (item.type) {
    case 'job':
      if (item.cron.isNotEmpty) parts.add(item.cron);
      if (item.agentRef.isNotEmpty) parts.add('agent: ${item.agentRef}');
      // Cài ở trạng thái tạm dừng là điều đáng nói trước: người dùng sẽ đi tìm
      // xem vì sao lịch không chạy.
      if (!item.enabled) parts.add(context.tr('installed paused'));
    case 'hook':
      if (item.matcher.isNotEmpty) {
        parts.add('${context.tr('matches')}: ${item.matcher}');
      }
      if (item.ifCondition.isNotEmpty) {
        parts.add('${context.tr('if')}: ${item.ifCondition}');
      }
      if (item.blocking) parts.add(context.tr('can block the agent loop'));
    case 'app':
      if (item.bytes > 0) parts.add(_KitSummary._bytes(item.bytes));
    case 'mcpServer':
      parts.add(context.tr('the daemon does not install this — use the MCP servers page'));
    default:
      if (item.description.isNotEmpty) parts.add(item.description);
      if (item.source == 'bundle') parts.add(context.tr('file from the .zip bundle'));
  }
  return parts.join(' · ');
}

class _ItemList extends StatefulWidget {
  const _ItemList({required this.items, required this.summary});
  final List<KitPreviewItem> items;
  final String summary;

  @override
  State<_ItemList> createState() => _ItemListState();
}

class _ItemListState extends State<_ItemList> {
  bool _open = true;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    if (widget.items.isEmpty) {
      return Text(context.tr('The kit declares nothing to install.'),
          style: TextStyle(fontSize: 12, color: c.textMuted));
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        InkWell(
          onTap: () => setState(() => _open = !_open),
          child: Padding(
            padding: const EdgeInsets.symmetric(vertical: 2),
            child: Row(mainAxisSize: MainAxisSize.min, children: [
              Text(
                '${context.tr('Will install')}: ${widget.summary.isEmpty ? '${widget.items.length}' : widget.summary}',
                style: TextStyle(
                    fontSize: 13,
                    fontWeight: FontWeight.w600,
                    color: c.textPrimary),
              ),
              const SizedBox(width: AppTokens.s6),
              Icon(_open ? Icons.expand_less : Icons.expand_more,
                  size: 16, color: c.textMuted),
            ]),
          ),
        ),
        if (_open) ...[
          const SizedBox(height: AppTokens.s6),
          for (final item in widget.items) _row(context, item),
        ],
      ],
    );
  }

  Widget _row(BuildContext context, KitPreviewItem item) {
    final c = context.colors;
    final subtitle = _itemSubtitle(context, item);
    return Opacity(
      opacity: item.unsupported ? 0.6 : 1,
      child: Container(
        margin: const EdgeInsets.only(bottom: AppTokens.s6),
        padding: const EdgeInsets.symmetric(
            horizontal: AppTokens.s12, vertical: AppTokens.s8),
        decoration: BoxDecoration(
          border: Border.all(color: c.border),
          borderRadius: BorderRadius.circular(AppTokens.rMd),
        ),
        child: Row(children: [
          Container(
            width: 28,
            height: 28,
            decoration: BoxDecoration(
              color: c.sidebar,
              borderRadius: BorderRadius.circular(AppTokens.rSm),
            ),
            child: Icon(_itemIcon(item.type), size: 15, color: c.textSecondary),
          ),
          const SizedBox(width: AppTokens.s12),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(item.name,
                    style: TextStyle(
                        fontSize: 13,
                        fontWeight: FontWeight.w600,
                        color: c.textPrimary)),
                if (subtitle.isNotEmpty) ...[
                  const SizedBox(height: 2),
                  Text(subtitle,
                      style: TextStyle(fontSize: 11, color: c.textMuted)),
                ],
              ],
            ),
          ),
          const SizedBox(width: AppTokens.s8),
          _Pill(
              text: kitKindLabel(context, item.type),
              color: item.unsupported ? AppTokens.warning : AppTokens.brand),
        ]),
      ),
    );
  }
}

// ── Mảnh dùng lại ───────────────────────────────────────────────────────────

class _Pill extends StatelessWidget {
  const _Pill({required this.text, required this.color});
  final String text;
  final Color color;

  @override
  Widget build(BuildContext context) => Container(
        padding:
            const EdgeInsets.symmetric(horizontal: AppTokens.s8, vertical: 2),
        decoration: BoxDecoration(
          color: color.withValues(alpha: 0.12),
          borderRadius: BorderRadius.circular(AppTokens.rSm),
        ),
        child: Text(text,
            style: TextStyle(
                fontSize: 11, color: color, fontWeight: FontWeight.w600)),
      );
}

class _Notice extends StatelessWidget {
  const _Notice({
    required this.color,
    required this.icon,
    required this.title,
    required this.body,
  });
  final Color color;
  final IconData icon;
  final String title;
  final String body;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Container(
      width: double.infinity,
      padding: const EdgeInsets.all(AppTokens.s12),
      decoration: BoxDecoration(
        color: color.withValues(alpha: 0.08),
        borderRadius: BorderRadius.circular(AppTokens.rMd),
        border: Border.all(color: color.withValues(alpha: 0.35)),
      ),
      child: Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
        Icon(icon, size: 15, color: color),
        const SizedBox(width: AppTokens.s8),
        Expanded(
          child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
            Text(title,
                style: TextStyle(
                    fontSize: 12,
                    fontWeight: FontWeight.w600,
                    color: c.textPrimary)),
            const SizedBox(height: 2),
            Text(body,
                style: TextStyle(
                    fontSize: 11, color: c.textSecondary, height: 1.45)),
          ]),
        ),
      ]),
    );
  }
}

class _FileLine extends StatelessWidget {
  const _FileLine({required this.label, required this.value});
  final String label;
  final String value;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Padding(
      padding: const EdgeInsets.only(bottom: 4),
      child: RichText(
        text: TextSpan(children: [
          TextSpan(
              text: '$label: ',
              style: TextStyle(
                  fontSize: 11,
                  color: c.textMuted,
                  fontWeight: FontWeight.w600)),
          TextSpan(
              text: value,
              style: TextStyle(fontSize: 11, color: c.textSecondary)),
        ]),
      ),
    );
  }
}

class _ErrorBox extends StatelessWidget {
  const _ErrorBox({required this.message});
  final String message;

  @override
  Widget build(BuildContext context) => Container(
        width: double.infinity,
        padding: const EdgeInsets.all(AppTokens.s12),
        decoration: BoxDecoration(
          color: AppTokens.danger.withValues(alpha: 0.08),
          borderRadius: BorderRadius.circular(AppTokens.rMd),
          border: Border.all(color: AppTokens.danger.withValues(alpha: 0.35)),
        ),
        child: Text(message,
            style: const TextStyle(
                fontSize: 12, color: AppTokens.danger, height: 1.4)),
      );
}
