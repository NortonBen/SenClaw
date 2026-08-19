// Hộp thoại "Cài app mới" của màn Apps — gom cả ba đường một Space App có thể
// đến vào cùng một chỗ:
//
//   • Cửa hàng — danh mục hub. Mục `kind: "app"` cài theo slug qua
//     POST /api/marketplace/hub/install: daemon giải version, tải artifact và
//     KIỂM SHA-512 hub công bố trước khi giao bytes cho installer. Chính phép
//     kiểm đó là lý do tab này không chỉ trỏ tới một link tải.
//   • Tệp ZIP — bundle trên máy, POST /api/space/apps/install-zip.
//   • Manifest URL — app tự phục vụ senclaw-manifest.json,
//     POST /api/space/apps/register.
//
// Cả ba đều qua bước quét bảo mật trước khi cài và cùng trả 422 khi bị chặn,
// nên chỉ cần một luồng hỏi-ghi-đè chung.
//
// Parity với web `web/src/components/space/AppInstallDialog.tsx`.

import 'dart:convert';
import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/foundation.dart' show kIsWeb;
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:http/http.dart' as http;

import '../../core/config/app_config.dart';
import '../../core/i18n/l10n.dart';
import '../../core/transport/api_client.dart' show ApiException;
import '../../core/transport/connection.dart';
import '../../theme/tokens.dart';
import 'app_search.dart';
import 'space_providers.dart';

/// Đẩy một bundle Space App lên daemon. Trả về hàng app đã cài.
///
/// Ném [ApiException] mang đúng lời daemon nói; status 422 nghĩa là quét bảo
/// mật đã CHẶN — chưa có gì được cài — nên người gọi có thể mời ghi đè.
Future<Map<String, dynamic>> uploadSpaceAppZip(
  AppConfig cfg, {
  required String filename,
  required List<int> bytes,
  bool force = false,
}) async {
  final uri = Uri.parse('${cfg.httpBase}/api/space/apps/install-zip');
  final req = http.MultipartRequest('POST', uri)
    ..headers.addAll(cfg.authHeaders)
    ..files.add(http.MultipartFile.fromBytes('file', bytes, filename: filename));
  // `runtime.start` của app chạy ngay lúc cài, nên ghi đè cảnh báo phải là
  // hành động cố ý — không bao giờ là một lần thử lại âm thầm.
  if (force) req.fields['force'] = 'true';

  final streamed = await req.send();
  final body = await streamed.stream.bytesToString();
  final decoded = body.isEmpty ? null : jsonDecode(body);
  final map =
      decoded is Map ? decoded.cast<String, dynamic>() : <String, dynamic>{};
  if (streamed.statusCode >= 300) {
    throw ApiException(streamed.statusCode,
        '${map['error'] ?? 'HTTP ${streamed.statusCode}'}');
  }
  return map;
}

/// Tệp .zip người dùng chọn, đã đọc sẵn bytes.
typedef PickedZip = ({String name, List<int> bytes});

/// Mở hộp chọn tệp .zip; null khi người dùng huỷ. Ném lỗi của file_picker (ví
/// dụ thiếu entitlement trên macOS) để người gọi hiện ra, thay vì im lặng.
Future<PickedZip?> pickSpaceAppZip() async {
  final res = await FilePicker.platform.pickFiles(
      type: FileType.custom, allowedExtensions: ['zip'], withData: kIsWeb);
  final f = res?.files.firstOrNull;
  if (f == null) return null;
  if (f.bytes != null) return (name: f.name, bytes: f.bytes!);
  if (f.path != null) {
    return (name: f.name, bytes: await File(f.path!).readAsBytes());
  }
  return null;
}

Future<void> showAppInstallDialog(BuildContext context) => showDialog<void>(
      context: context,
      builder: (_) => const AppInstallDialog(),
    );

/// Một mục app trong danh mục hub.
class StoreApp {
  final String name;
  final String description;
  final String? version;
  final String? author;
  final String slug;
  final int? downloads;
  final bool installed;
  final String? installedVersion;
  final bool updateAvailable;

  const StoreApp({
    required this.name,
    required this.description,
    required this.slug,
    this.version,
    this.author,
    this.downloads,
    this.installed = false,
    this.installedVersion,
    this.updateAvailable = false,
  });

  /// Null khi mục không phải app cài được: mục của `marketplace.json` không có
  /// slug và cài bằng git clone — endpoint khác, cũng không phải Space App.
  static StoreApp? fromJson(Map<String, dynamic> j) {
    final slug = j['slug'];
    if (j['kind'] != 'app' || slug is! String || slug.isEmpty) return null;
    return StoreApp(
      name: '${j['name'] ?? slug}',
      description: '${j['description'] ?? ''}',
      version: j['version'] as String?,
      author: j['author'] as String?,
      slug: slug,
      downloads: (j['downloads'] as num?)?.toInt(),
      installed: j['installed'] == true,
      installedVersion: j['installedVersion'] as String?,
      updateAvailable: j['updateAvailable'] == true,
    );
  }
}

class AppInstallDialog extends ConsumerStatefulWidget {
  const AppInstallDialog({super.key});

  @override
  ConsumerState<AppInstallDialog> createState() => _AppInstallDialogState();
}

class _AppInstallDialogState extends ConsumerState<AppInstallDialog> {
  List<StoreApp> _catalog = const [];
  bool _loadingCatalog = false;
  String? _catalogError;
  String _query = '';
  String? _busySlug;

  bool _installingZip = false;
  bool _registering = false;
  final _urlCtrl = TextEditingController();

  @override
  void initState() {
    super.initState();
    _loadCatalog();
  }

  @override
  void dispose() {
    _urlCtrl.dispose();
    super.dispose();
  }

  // ── Danh mục cửa hàng ──────────────────────────────────────────────────────

  Future<void> _loadCatalog() async {
    setState(() {
      _loadingCatalog = true;
      _catalogError = null;
    });
    final api = ref.read(apiClientProvider);
    try {
      final r = await api.get('/api/marketplace/sources');
      final sources = ((r is Map ? r['sources'] : null) as List? ?? const [])
          .whereType<Map>()
          .map((e) => e.cast<String, dynamic>())
          .where((s) => s['enabled'] != false)
          .toList();

      final bySlug = <String, StoreApp>{};
      for (final src in sources) {
        try {
          final d = await api.get('/api/marketplace/sources/${src['id']}');
          final apps = ((d is Map ? d['plugins'] : null) as List? ?? const [])
              .whereType<Map>()
              .map((e) => StoreApp.fromJson(e.cast<String, dynamic>()))
              .whereType<StoreApp>();
          for (final app in apps) {
            bySlug.putIfAbsent(app.slug, () => app);
          }
        } catch (_) {
          // Một nguồn hỏng không được làm rỗng cả cửa hàng.
        }
      }
      final list = bySlug.values.toList()
        ..sort((a, b) => a.name.toLowerCase().compareTo(b.name.toLowerCase()));
      if (!mounted) return;
      setState(() => _catalog = list);
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _catalog = const [];
        _catalogError = _friendly(e);
      });
    } finally {
      if (mounted) setState(() => _loadingCatalog = false);
    }
  }

  // ── Cài đặt ────────────────────────────────────────────────────────────────

  String _friendly(Object e) => e is ApiException ? e.message : '$e';

  void _snack(String msg) =>
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(msg)));

  /// Xong một lượt cài: làm mới lưới app rồi đóng hộp thoại.
  void _done(String appName) {
    ref.invalidate(spaceAppsProvider);
    if (!mounted) return;
    final messenger = ScaffoldMessenger.of(context);
    final text = context.trArgs('Installed {name}', {'name': appName});
    Navigator.of(context).pop();
    messenger.showSnackBar(SnackBar(content: Text(text)));
  }

  /// Hỏi trước khi cài đè bản đã bị quét bảo mật chặn.
  Future<bool> _confirmForce(String target, String reason) async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (dctx) => AlertDialog(
        backgroundColor: dctx.colors.surface,
        title: Text(
            dctx.trArgs('Security scan blocked {name}', {'name': target})),
        content: SizedBox(
          width: 420,
          child: Text(reason,
              style: TextStyle(color: dctx.colors.textSecondary, fontSize: 13)),
        ),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(dctx, false),
              child: Text(dctx.tr('Cancel'))),
          TextButton(
            onPressed: () => Navigator.pop(dctx, true),
            child: Text(dctx.tr('Install anyway'),
                style: const TextStyle(color: AppTokens.danger)),
          ),
        ],
      ),
    );
    return ok == true;
  }

  Future<void> _installFromStore(StoreApp app, {bool force = false}) async {
    setState(() => _busySlug = app.slug);
    try {
      final r = await ref.read(apiClientProvider).post(
          '/api/marketplace/hub/install',
          body: {'slug': app.slug, 'force': force});
      final m = (r is Map ? r['manifest'] : null) as Map?;
      _done('${m?['name'] ?? app.name}');
    } on ApiException catch (e) {
      if (!mounted) return;
      // 422 = quét bảo mật chặn; chưa có gì được cài.
      if (e.status == 422 &&
          !force &&
          await _confirmForce(app.name, e.message)) {
        await _installFromStore(app, force: true);
        return;
      }
      if (mounted) _snack(_friendly(e));
    } catch (e) {
      if (mounted) _snack(_friendly(e));
    } finally {
      if (mounted) setState(() => _busySlug = null);
    }
  }

  Future<void> _installZip() async {
    final PickedZip? picked;
    try {
      picked = await pickSpaceAppZip();
    } catch (e) {
      // file_picker hỏng TRƯỚC khi mở panel (ví dụ entitlement macOS) — hiện ra
      // thay vì để nút bấm không phản hồi.
      if (mounted) _snack(context.trArgs('File picker error: {e}', {'e': e}));
      return;
    }
    if (picked == null) return;
    final file = picked;

    setState(() => _installingZip = true);
    try {
      final cfg = ref.read(appConfigProvider);
      Map<String, dynamic> row;
      try {
        row = await uploadSpaceAppZip(cfg,
            filename: file.name, bytes: file.bytes);
      } on ApiException catch (e) {
        if (e.status != 422 || !mounted) rethrow;
        if (!await _confirmForce(file.name, e.message)) return;
        row = await uploadSpaceAppZip(cfg,
            filename: file.name, bytes: file.bytes, force: true);
      }
      final m = row['manifest'] as Map?;
      _done('${m?['name'] ?? row['id'] ?? file.name}');
    } catch (e) {
      if (mounted) _snack(_friendly(e));
    } finally {
      if (mounted) setState(() => _installingZip = false);
    }
  }

  Future<void> _register() async {
    final url = _urlCtrl.text.trim();
    if (url.isEmpty) return;
    setState(() => _registering = true);
    try {
      final r = await ref
          .read(apiClientProvider)
          .post('/api/space/apps/register', body: {'manifest_url': url});
      final m = (r is Map ? r['manifest'] : null) as Map?;
      _done('${m?['name'] ?? (r is Map ? r['id'] : null) ?? url}');
    } catch (e) {
      if (mounted) _snack(_friendly(e));
    } finally {
      if (mounted) setState(() => _registering = false);
    }
  }

  // ── Giao diện ──────────────────────────────────────────────────────────────

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return AlertDialog(
      backgroundColor: c.surface,
      title: Text(context.tr('Install a new app')),
      content: SizedBox(
        width: 640,
        height: 460,
        child: DefaultTabController(
          length: 3,
          child: Column(
            children: [
              TabBar(
                labelColor: c.accent,
                unselectedLabelColor: c.textMuted,
                indicatorColor: c.accent,
                labelStyle:
                    const TextStyle(fontSize: 13, fontWeight: FontWeight.w600),
                tabs: [
                  Tab(text: context.tr('Store')),
                  Tab(text: context.tr('ZIP file')),
                  Tab(text: context.tr('Manifest URL')),
                ],
              ),
              const SizedBox(height: AppTokens.s12),
              Expanded(
                child: TabBarView(children: [
                  _storeTab(context),
                  _zipTab(context),
                  _urlTab(context),
                ]),
              ),
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: Text(context.tr('Close'))),
      ],
    );
  }

  Widget _storeTab(BuildContext context) {
    final c = context.colors;
    final visible = _catalog
        .where((a) => searchMatches([a.name, a.description, a.slug], _query))
        .toList();

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(children: [
          Expanded(
            child: SizedBox(
              height: AppTokens.controlHeight,
              child: TextField(
                onChanged: (v) => setState(() => _query = v),
                style: TextStyle(color: c.textPrimary, fontSize: 13),
                decoration: InputDecoration(
                  isDense: true,
                  prefixIcon: Icon(Icons.search, size: 16, color: c.textMuted),
                  hintText: context.tr('Search apps in the store…'),
                  hintStyle: TextStyle(color: c.textMuted, fontSize: 13),
                  border: OutlineInputBorder(
                      borderRadius: BorderRadius.circular(AppTokens.rMd)),
                ),
              ),
            ),
          ),
          const SizedBox(width: AppTokens.s8),
          IconButton(
            tooltip: context.tr('Reload catalog'),
            icon: const Icon(Icons.refresh, size: 18),
            onPressed: _loadingCatalog ? null : _loadCatalog,
          ),
        ]),
        const SizedBox(height: AppTokens.s8),
        if (_catalogError != null)
          Padding(
            padding: const EdgeInsets.only(bottom: AppTokens.s8),
            child: Text(
                context.trArgs('Could not read the store catalog: {e}',
                    {'e': _catalogError!}),
                style: const TextStyle(color: AppTokens.warning, fontSize: 12)),
          ),
        Expanded(
          child: _loadingCatalog
              ? const Center(child: CircularProgressIndicator())
              : visible.isEmpty
                  ? Center(
                      child: Padding(
                        padding: const EdgeInsets.all(AppTokens.s16),
                        child: Text(
                          _catalog.isEmpty
                              ? context.tr(
                                  'The store has no apps — add a hub source in Plugins → Marketplace, then sync')
                              : context.tr('No app matches that search'),
                          textAlign: TextAlign.center,
                          style: TextStyle(color: c.textMuted, fontSize: 13),
                        ),
                      ),
                    )
                  : ListView.separated(
                      itemCount: visible.length,
                      separatorBuilder: (_, _) =>
                          Divider(height: 1, color: c.border),
                      itemBuilder: (_, i) => _storeRow(context, visible[i]),
                    ),
        ),
      ],
    );
  }

  Widget _storeRow(BuildContext context, StoreApp app) {
    final c = context.colors;
    final meta = [
      app.slug,
      if (app.author != null && app.author!.isNotEmpty) app.author!,
      if (app.downloads != null)
        context.trArgs('{n} downloads', {'n': app.downloads!}),
    ].join(' · ');

    return Padding(
      padding: const EdgeInsets.symmetric(vertical: AppTokens.s8),
      child: Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
        Expanded(
          child:
              Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
            Row(children: [
              Flexible(
                child: Text(app.name,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                        color: c.textPrimary,
                        fontSize: 13,
                        fontWeight: FontWeight.w600)),
              ),
              if (app.version != null) ...[
                const SizedBox(width: AppTokens.s8),
                _Pill(text: app.version!),
              ],
              if (app.updateAvailable && app.installedVersion != null) ...[
                const SizedBox(width: AppTokens.s4),
                _Pill(
                    text: context
                        .trArgs('using {v}', {'v': app.installedVersion!}),
                    color: AppTokens.warning),
              ],
            ]),
            if (app.description.isNotEmpty) ...[
              const SizedBox(height: 2),
              Text(app.description,
                  maxLines: 2,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(color: c.textSecondary, fontSize: 12)),
            ],
            const SizedBox(height: 2),
            Text(meta,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(color: c.textMuted, fontSize: 11)),
          ]),
        ),
        const SizedBox(width: AppTokens.s12),
        if (app.installed && !app.updateAvailable)
          _Pill(text: context.tr('Installed'), color: AppTokens.success)
        else
          FilledButton(
            onPressed: _busySlug == null ? () => _installFromStore(app) : null,
            child: _busySlug == app.slug
                ? const SizedBox(
                    width: 14,
                    height: 14,
                    child: CircularProgressIndicator(strokeWidth: 2))
                : Text(app.updateAvailable
                    ? context.tr('Update')
                    : context.tr('Install')),
          ),
      ]),
    );
  }

  Widget _zipTab(BuildContext context) {
    final c = context.colors;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          context.tr(
              'The app’s runtime.start command runs as soon as it is installed — only install a ZIP from a source you trust.'),
          style: const TextStyle(color: AppTokens.warning, fontSize: 12),
        ),
        const SizedBox(height: AppTokens.s16),
        Text(
          context
              .tr('A Space App bundle has senclaw-manifest.json at its root.'),
          style: TextStyle(color: c.textMuted, fontSize: 12),
        ),
        const SizedBox(height: AppTokens.s16),
        FilledButton.icon(
          onPressed: _installingZip ? null : _installZip,
          icon: _installingZip
              ? const SizedBox(
                  width: 14,
                  height: 14,
                  child: CircularProgressIndicator(strokeWidth: 2))
              : const Icon(Icons.folder_open, size: 16),
          label: Text(context.tr('Choose a .zip file')),
        ),
      ],
    );
  }

  Widget _urlTab(BuildContext context) {
    final c = context.colors;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          context.tr(
              'The app is embedded in an iframe — be sure you trust its origin before registering.'),
          style: const TextStyle(color: AppTokens.warning, fontSize: 12),
        ),
        const SizedBox(height: AppTokens.s16),
        TextField(
          controller: _urlCtrl,
          style: TextStyle(color: c.textPrimary, fontSize: 13),
          decoration: InputDecoration(
            isDense: true,
            labelText: context.tr('Manifest URL'),
            hintText: 'https://…/senclaw-manifest.json',
            border: OutlineInputBorder(
                borderRadius: BorderRadius.circular(AppTokens.rMd)),
          ),
          onSubmitted: (_) => _register(),
        ),
        const SizedBox(height: AppTokens.s8),
        Text(
          context.tr('The app must serve senclaw-manifest.json at this URL.'),
          style: TextStyle(color: c.textMuted, fontSize: 12),
        ),
        const SizedBox(height: AppTokens.s16),
        FilledButton(
          onPressed: _registering ? null : _register,
          child: _registering
              ? const SizedBox(
                  width: 14,
                  height: 14,
                  child: CircularProgressIndicator(strokeWidth: 2))
              : Text(context.tr('Register')),
        ),
      ],
    );
  }
}

/// Nhãn nhỏ bo tròn — version, "Đã cài", …
class _Pill extends StatelessWidget {
  const _Pill({required this.text, this.color});
  final String text;
  final Color? color;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final fg = color ?? c.textSecondary;
    return Container(
      padding:
          const EdgeInsets.symmetric(horizontal: AppTokens.s8, vertical: 2),
      decoration: BoxDecoration(
        color: fg.withValues(alpha: 0.12),
        borderRadius: BorderRadius.circular(AppTokens.rFull),
      ),
      child: Text(text, style: TextStyle(color: fg, fontSize: 11)),
    );
  }
}
