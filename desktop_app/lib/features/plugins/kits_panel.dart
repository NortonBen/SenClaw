// Plugins → Kits (desktop) — cài/gỡ Zen Kit từ một manifest JSON duy nhất.
// Parity với web `KitsPanel.tsx`.
//
// Daemon sở hữu toàn bộ việc cài (thứ tự, luật không-ghi-đè, sổ biên nhận), nên
// panel chỉ POST manifest rồi hiển thị báo cáo. Xem docs/zen-kits.md.
//   GET    /api/kits          → kit đã cài (đọc từ sổ biên nhận)
//   POST   /api/kits/preview  → kiểm tra + đếm mục + cảnh báo, KHÔNG cài gì
//   POST   /api/kits/install  → cài, trả báo cáo từng mục
//   DELETE /api/kits/:id      → gỡ đúng những gì kit đã tạo
//
// Ba luật của daemon mà UI phải phản ánh trung thực:
//  1. Không ghi đè → `skipped` là "đã có sẵn, giữ nguyên", KHÔNG phải lỗi.
//  2. Không dừng giữa chừng → `ok:false` vẫn kèm báo cáo; luôn hiện nó.
//  3. Chỉ gỡ thứ đã tạo → mục `skipped` không vào sổ nên gỡ không đụng tới.


import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/i18n/l10n.dart';
import '../../core/transport/api_client.dart';
import '../../core/transport/connection.dart';
import '../../theme/tokens.dart';
import 'kit_install_dialog.dart';
import 'kit_params_form.dart';

// ── Models ──────────────────────────────────────────────────────────────────

/// Một mục kit đã tạo — hàng trong sổ biên nhận.
class KitItemRecord {
  final String type;
  final String name;

  /// Đường dẫn tuyệt đối cho mục dạng tệp (persona, thư mục skill, workflow).
  final String? path;

  /// Id engine cho mục nằm trong DB (background task).
  final String? engineRef;

  const KitItemRecord(this.type, this.name, this.path, this.engineRef);

  factory KitItemRecord.fromJson(Map<String, dynamic> j) => KitItemRecord(
        '${j['type'] ?? ''}',
        '${j['name'] ?? ''}',
        j['path'] as String?,
        j['engineRef'] as String?,
      );

  /// Nơi mục này nằm — dùng để đi kiểm chứng bằng tay.
  String get where {
    if (path != null && path!.isNotEmpty) return path!;
    if (engineRef != null && engineRef!.isNotEmpty) {
      return 'background_tasks: $engineRef';
    }
    return '—';
  }
}

class KitReceipt {
  final String id;
  final String name;
  final String version;

  /// Kit dùng để làm gì. Đọc từ sổ biên nhận vì đến lúc này manifest đã đi mất,
  /// mà một hàng chỉ có id với số hiệu bản thì không nói lên điều gì.
  final String description;

  /// RFC3339 UTC.
  final String installedAt;
  final List<KitItemRecord> items;

  /// Giá trị đã dùng khi cài. Tham số `secret` cố tình không có ở đây.
  final Map<String, String> params;

  const KitReceipt(this.id, this.name, this.version, this.description,
      this.installedAt, this.items, this.params);

  factory KitReceipt.fromJson(Map<String, dynamic> j) => KitReceipt(
        '${j['id'] ?? ''}',
        '${j['name'] ?? ''}',
        '${j['version'] ?? ''}',
        '${j['description'] ?? ''}',
        '${j['installedAt'] ?? ''}',
        [
          for (final i in (j['items'] as List? ?? const []))
            if (i is Map) KitItemRecord.fromJson(i.cast<String, dynamic>()),
        ],
        {
          for (final e in ((j['params'] as Map?) ?? const {}).entries)
            '${e.key}': '${e.value}',
        },
      );

  /// Đếm mục đã tạo theo loại — chip tóm tắt trên hàng kit.
  Map<String, int> get countsByKind {
    final out = <String, int>{};
    for (final i in items) {
      out[i.type] = (out[i.type] ?? 0) + 1;
    }
    return out;
  }
}

class KitWarning {
  final String kind;
  final String subject;
  final String detail;
  const KitWarning(this.kind, this.subject, this.detail);

  factory KitWarning.fromJson(Map<String, dynamic> j) => KitWarning(
        '${j['kind'] ?? ''}',
        '${j['subject'] ?? ''}',
        '${j['detail'] ?? ''}',
      );
}

/// Kết quả một mục, dùng chung cho cài (created|skipped|unsupported|failed) và
/// gỡ (removed|missing|failed) — hai bảng cùng hình dạng.
class KitOutcome {
  final String type;
  final String name;
  final String status;
  final String? detail;
  const KitOutcome(this.type, this.name, this.status, this.detail);

  factory KitOutcome.fromJson(Map<String, dynamic> j) => KitOutcome(
        '${j['type'] ?? ''}',
        '${j['name'] ?? ''}',
        '${j['status'] ?? ''}',
        j['detail'] as String?,
      );
}

class KitReport {
  final String kitId;
  final String version;
  final List<KitOutcome> items;
  final List<KitWarning> warnings;

  /// true = báo cáo gỡ (nhãn trạng thái khác).
  final bool isRemoval;

  const KitReport(
      this.kitId, this.version, this.items, this.warnings, this.isRemoval);

  factory KitReport.fromJson(Map<String, dynamic> j, {required bool removal}) =>
      KitReport(
        '${j['kitId'] ?? ''}',
        '${j['version'] ?? ''}',
        [
          for (final i in (j['items'] as List? ?? const []))
            if (i is Map) KitOutcome.fromJson(i.cast<String, dynamic>()),
        ],
        [
          for (final w in (j['warnings'] as List? ?? const []))
            if (w is Map) KitWarning.fromJson(w.cast<String, dynamic>()),
        ],
        removal,
      );
}

class KitPreview {
  final String id;
  final String name;
  final String version;

  /// Phiên bản schema daemon đọc được (1 = chỉ agents + jobs).
  final int manifest;
  final Map<String, int> counts;
  final List<KitWarning> warnings;

  /// Tham số kit hỏi trước khi cài. Rỗng = không hỏi gì.
  final List<KitParam> params;

  /// Khác null khi câu trả lời đã gửi sẽ bị từ chối — kiểm tra trực tiếp trên
  /// daemon, không phải đoán ở client.
  final String? paramError;

  final String description;

  /// Thứ đi kèm dạng tệp trong gói .zip, khác với thứ chỉ được khai báo trong
  /// manifest. Rỗng khi kit là manifest JSON thuần.
  final KitBundleSummary bundle;

  /// Khác null khi máy đã cài kit cùng id — nói trước, thay vì để người dùng
  /// phát hiện qua một loạt dòng "đã có sẵn" sau khi cài.
  final String? installedVersion;

  /// Từng mục sẽ cài, theo đúng thứ tự installer chạy.
  final List<KitPreviewItem> items;

  const KitPreview(this.id, this.name, this.version, this.manifest, this.counts,
      this.warnings, this.params, this.paramError,
      {this.description = '',
      this.bundle = const KitBundleSummary(),
      this.installedVersion,
      this.items = const []});

  factory KitPreview.fromJson(Map<String, dynamic> j) {
    final raw = (j['counts'] as Map?)?.cast<String, dynamic>() ?? const {};
    final installed = (j['installed'] as Map?)?.cast<String, dynamic>();
    return KitPreview(
      '${j['id'] ?? ''}',
      '${j['name'] ?? ''}',
      '${j['version'] ?? ''}',
      (j['manifest'] as num?)?.toInt() ?? 1,
      {
        for (final e in raw.entries) e.key: (e.value as num?)?.toInt() ?? 0,
      },
      [
        for (final w in (j['warnings'] as List? ?? const []))
          if (w is Map) KitWarning.fromJson(w.cast<String, dynamic>()),
      ],
      [
        for (final p in (j['params'] as List? ?? const []))
          if (p is Map) KitParam.fromJson(p.cast<String, dynamic>()),
      ],
      j['paramError'] as String?,
      description: '${j['description'] ?? ''}',
      bundle: KitBundleSummary.fromJson(
          (j['bundle'] as Map?)?.cast<String, dynamic>() ?? const {}),
      installedVersion: installed == null ? null : '${installed['version'] ?? ''}',
      items: [
        for (final i in (j['items'] as List? ?? const []))
          if (i is Map) KitPreviewItem.fromJson(i.cast<String, dynamic>()),
      ],
    );
  }

  /// Các loại daemon thật sự cài.
  static const installableKinds = ['agents', 'skills', 'workflows', 'hooks', 'jobs'];

  int get installableCount =>
      installableKinds.fold(0, (n, k) => n + (counts[k] ?? 0));

  /// mcpServers + apps: đọc được nhưng daemon không cài (có luồng đồng ý riêng).
  int get notInstalledCount => (counts['mcpServers'] ?? 0) + (counts['apps'] ?? 0);
}

/// Thân request cho preview/install: manifest + câu trả lời tham số.
///
/// Người dùng có thể đã dán sẵn dạng bọc (`{"manifest": {...}}`); bọc thêm lần
/// nữa thì trường `manifest` của kit hoá thành object và daemon parse hỏng.
/// Nhận ra dạng bọc rồi chỉ chèn `params` — đúng cách daemon phân biệt.
Object kitRequestBody(Object decoded, Map<String, dynamic> answers) {
  if (decoded is Map && (decoded['manifest'] is Map || decoded['kit'] is Map)) {
    return {...decoded, 'params': answers};
  }
  return {'manifest': decoded, 'params': answers};
}

/// Một mục kit sẽ cài, đủ để dựng danh sách chi tiết trước khi cài.
///
/// Trường có cấu trúc chứ không phải câu ghép sẵn: nhãn ("agent:", "tạm dừng")
/// phải theo ngôn ngữ của client.
class KitPreviewItem {
  final String type;
  final String name;
  final String description;

  /// skill/workflow: `bundle` = lấy từ tệp trong .zip, thắng bản inline.
  final String source;

  // job
  final String cron;
  final String agentRef;
  final bool enabled;

  // hook
  final String matcher;
  final String ifCondition;
  final bool blocking;

  // app
  final int bytes;

  /// mcpServer: daemon đọc được nhưng không cài.
  final bool unsupported;

  const KitPreviewItem({
    required this.type,
    required this.name,
    this.description = '',
    this.source = '',
    this.cron = '',
    this.agentRef = '',
    this.enabled = true,
    this.matcher = '',
    this.ifCondition = '',
    this.blocking = false,
    this.bytes = 0,
    this.unsupported = false,
  });

  factory KitPreviewItem.fromJson(Map<String, dynamic> j) => KitPreviewItem(
        type: '${j['type'] ?? ''}',
        name: '${j['name'] ?? ''}',
        description: j['description'] == null ? '' : '${j['description']}',
        source: j['source'] == null ? '' : '${j['source']}',
        cron: j['cron'] == null ? '' : '${j['cron']}',
        agentRef: j['agentRef'] == null ? '' : '${j['agentRef']}',
        enabled: j['enabled'] != false,
        matcher: j['matcher'] == null ? '' : '${j['matcher']}',
        ifCondition: j['if'] == null ? '' : '${j['if']}',
        blocking: j['blocking'] == true,
        bytes: (j['bytes'] as num?)?.toInt() ?? 0,
        unsupported: j['unsupported'] == true,
      );
}

/// Space App đi kèm trong gói, vẫn ở dạng .zip.
class BundleApp {
  final String id;
  final int bytes;
  const BundleApp(this.id, this.bytes);
}

/// Những gì đi kèm dạng tệp trong một gói .zip.
class KitBundleSummary {
  final bool hasFiles;
  final List<String> skills;
  final List<String> workflows;
  final List<BundleApp> apps;

  const KitBundleSummary({
    this.hasFiles = false,
    this.skills = const [],
    this.workflows = const [],
    this.apps = const [],
  });

  factory KitBundleSummary.fromJson(Map<String, dynamic> j) => KitBundleSummary(
        hasFiles: j['hasFiles'] == true,
        skills: [for (final v in (j['skills'] as List? ?? const [])) '$v'],
        workflows: [for (final v in (j['workflows'] as List? ?? const [])) '$v'],
        apps: [
          for (final a in (j['apps'] as List? ?? const []))
            if (a is Map)
              BundleApp('${a['id'] ?? ''}', (a['bytes'] as num?)?.toInt() ?? 0),
        ],
      );
}

/// Một kit do marketplace source chào (mảng `kits[]` trong catalog của nó).
class AvailableKit {
  final String sourceId;
  final String sourceName;
  final String name;
  final String description;
  final String version;

  /// False khi mục trong catalog không khai báo tệp để tải.
  final bool installable;

  /// Khác null khi máy đã cài kit này.
  final String? installedVersion;

  const AvailableKit(this.sourceId, this.sourceName, this.name,
      this.description, this.version, this.installable, this.installedVersion);

  factory AvailableKit.fromJson(Map<String, dynamic> j) => AvailableKit(
        '${j['sourceId'] ?? ''}',
        '${j['sourceName'] ?? ''}',
        '${j['name'] ?? ''}',
        '${j['description'] ?? ''}',
        '${j['version'] ?? ''}',
        j['installable'] != false,
        j['installedVersion'] == null ? null : '${j['installedVersion']}',
      );
}

// ── Providers ───────────────────────────────────────────────────────────────

/// Providers không mang BuildContext — dịch qua L10n toàn cục.
String get kitOldDaemonMsg => L10n.global.t(
    'This daemon does not serve /api/kits yet — rebuild and restart the daemon.');

/// Kit các marketplace source đang chào.
///
/// Trả rỗng thay vì ném lỗi khi daemon chưa có endpoint: marketplace là phần
/// thêm, hỏng nó không được làm hỏng danh sách kit đã cài.
final availableKitsProvider = FutureProvider<List<AvailableKit>>((ref) async {
  final r = await ref.read(apiClientProvider).get('/api/kits/available');
  if (r is! Map || r['kits'] is! List) return const [];
  return (r['kits'] as List)
      .whereType<Map>()
      .map((e) => AvailableKit.fromJson(e.cast<String, dynamic>()))
      .toList();
});

final kitsProvider = FutureProvider<List<KitReceipt>>((ref) async {
  final r = await ref.read(apiClientProvider).get('/api/kits');
  // Daemon cũ trả trang SPA cho /api lạ → ApiClient trả String, không phải Map.
  if (r is! Map || r['kits'] is! List) {
    throw Exception(kitOldDaemonMsg);
  }
  return (r['kits'] as List)
      .whereType<Map>()
      .map((e) => KitReceipt.fromJson(e.cast<String, dynamic>()))
      .toList();
});

// ── Nhãn ────────────────────────────────────────────────────────────────────

String kitKindLabel(BuildContext context, String kind) => switch (kind) {
      'agent' || 'agents' => context.tr('Persona'),
      'skill' || 'skills' => context.tr('Skill'),
      'workflow' || 'workflows' => context.tr('Workflow'),
      'hook' || 'hooks' => context.tr('Hook'),
      'job' || 'jobs' => context.tr('Scheduled job'),
      'mcpServer' || 'mcpServers' => context.tr('MCP server'),
      'app' || 'apps' => context.tr('Space App'),
      _ => kind,
    };

/// Nhãn + màu + giải thích cho một trạng thái. `skipped` và `missing` cố tình
/// KHÔNG phải màu lỗi: chúng là kết quả bình thường của luật không-ghi-đè.
({String label, Color color, String hint}) kitStatusMeta(
    BuildContext context, String status) {
  const neutral = Color(0xFF8A8A99);
  return switch (status) {
    'created' => (
        label: context.tr('created'),
        color: AppTokens.success,
        hint: context.tr('The kit created this item.')
      ),
    'skipped' => (
        label: context.tr('already there'),
        color: neutral,
        hint: context.tr(
            'That name already existed — the daemon left the original alone and '
            'did not overwrite it. Removing the kit will not touch it.')
      ),
    'unsupported' => (
        label: context.tr('not installed'),
        color: AppTokens.warning,
        hint: context.tr(
            'The daemon parses this but does not install it — that subsystem has '
            'its own consent flow.')
      ),
    'failed' => (
        label: context.tr('failed'),
        color: AppTokens.danger,
        hint: context.tr(
            'This item failed; the remaining items were still processed.')
      ),
    'removed' => (
        label: context.tr('removed'),
        color: AppTokens.success,
        hint: context.tr('Deleted from disk / database.')
      ),
    'missing' => (
        label: context.tr('already gone'),
        color: neutral,
        hint: context.tr('You had already deleted it by hand. Not an error.')
      ),
    _ => (label: status, color: neutral, hint: ''),
  };
}

// ── Panel ───────────────────────────────────────────────────────────────────

class KitsPanel extends ConsumerStatefulWidget {
  const KitsPanel({super.key});
  @override
  ConsumerState<KitsPanel> createState() => _KitsPanelState();
}

class _KitsPanelState extends ConsumerState<KitsPanel> {
  /// Báo cáo của lần gỡ gần nhất. Việc cài có báo cáo riêng trong hộp thoại;
  /// ở đây chỉ còn phần gỡ, vì nó chạy thẳng từ danh sách.
  KitReport? _report;

  /// Tab đang mở: 0 = đã cài, 1 = marketplace.
  int _tab = 0;

  String _friendly(Object e) {
    if (e is ApiException) return e.message;
    return '$e'.replaceFirst('Exception: ', '');
  }

  void _toast(String msg) {
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(msg)));
  }

  // ── Gỡ ────────────────────────────────────────────────────────────────────

  Future<void> _uninstall(KitReceipt kit) async {
    final label = kit.name.isNotEmpty ? kit.name : kit.id;
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text(ctx.trArgs('Remove kit "{name}"?', {'name': label})),
        content: Text(ctx.tr(
            'Only the items this kit created itself are deleted; anything you '
            'made yourself is left alone.')),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(ctx, false),
              child: Text(ctx.tr('Cancel'))),
          FilledButton(
            style: FilledButton.styleFrom(backgroundColor: AppTokens.danger),
            onPressed: () => Navigator.pop(ctx, true),
            child: Text(ctx.tr('Remove')),
          ),
        ],
      ),
    );
    if (confirmed != true) return;

    try {
      final r = await ref
          .read(apiClientProvider)
          .delete('/api/kits/${Uri.encodeComponent(kit.id)}');
      if (r is! Map) throw Exception(kitOldDaemonMsg);
      final map = r.cast<String, dynamic>();
      final ok = map['ok'] == true;
      final report = (map['report'] as Map?)?.cast<String, dynamic>();
      if (!mounted) return;
      setState(() {
        _report =
            report == null ? null : KitReport.fromJson(report, removal: true);
      });
      ref.invalidate(kitsProvider);
      ref.invalidate(availableKitsProvider);
      _toast(ok
          ? context.trArgs('Removed kit "{name}"', {'name': label})
          : context.tr('Removed with failures — the receipt was kept'));
    } catch (e) {
      if (mounted) {
        _toast(context.trArgs('Remove failed: {e}', {'e': _friendly(e)}));
      }
    }
  }

  Future<void> _openInstall({KitInstallSource? source}) async {
    await showKitInstallDialog(context,
        source: source ?? const KitInstallSource.local());
    if (!mounted) return;
    // Hộp thoại đã tự làm mới provider; xoá báo cáo gỡ cũ cho khỏi lẫn.
    setState(() => _report = null);
  }

  // ── Build ─────────────────────────────────────────────────────────────────

  @override
  Widget build(BuildContext context) {
    final c = context.colors;

    return Column(
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(
              AppTokens.s16, AppTokens.s12, AppTokens.s16, 0),
          child: Row(
            children: [
              Icon(Icons.card_giftcard_outlined, size: 18, color: c.accent),
              const SizedBox(width: AppTokens.s8),
              Text('Kits',
                  style: TextStyle(
                      color: c.textPrimary,
                      fontSize: 15,
                      fontWeight: FontWeight.w700)),
              const Spacer(),
              IconButton(
                tooltip: context.tr('Reload'),
                icon: const Icon(Icons.refresh, size: 18),
                onPressed: () {
                  ref.invalidate(kitsProvider);
                  ref.invalidate(availableKitsProvider);
                },
              ),
              const SizedBox(width: AppTokens.s8),
              FilledButton.icon(
                onPressed: () => _openInstall(),
                icon: const Icon(Icons.rocket_launch_outlined, size: 16),
                label: Text(context.tr('Install kit')),
              ),
            ],
          ),
        ),
        Padding(
          padding: const EdgeInsets.fromLTRB(
              AppTokens.s16, AppTokens.s12, AppTokens.s16, 0),
          child: Row(children: [
            _TabButton(
              label: context.tr('Installed'),
              active: _tab == 0,
              onTap: () => setState(() => _tab = 0),
            ),
            const SizedBox(width: AppTokens.s8),
            _TabButton(
              label: context.tr('Marketplace'),
              active: _tab == 1,
              onTap: () => setState(() => _tab = 1),
            ),
          ]),
        ),
        Expanded(
          child: ListView(
            padding: const EdgeInsets.all(AppTokens.s16),
            children: [
              Text(
                context.tr(
                    'A kit installs a whole setup in one go: personas, skills, workflows, '
                    'hooks and scheduled jobs. A .json carries only the declaration; a .zip '
                    'also carries the real files of its skills and workflows, and even a '
                    'Space App. The daemon keeps the never-overwrite rule (a name that is '
                    'taken is skipped) and a receipt, so removing a kit deletes only what '
                    'it created.'),
                style: TextStyle(color: c.textMuted, fontSize: 12),
              ),
              const SizedBox(height: AppTokens.s16),
              if (_tab == 0) _installedList(context) else _marketList(context),
              if (_report != null) ...[
                const SizedBox(height: AppTokens.s20),
                KitReportCard(
                  report: _report!,
                  onClose: () => setState(() => _report = null),
                ),
              ],
            ],
          ),
        ),
      ],
    );
  }

  Widget _installedList(BuildContext context) {
    final c = context.colors;
    final kits = ref.watch(kitsProvider);
    return kits.when(
      loading: () => const Padding(
        padding: EdgeInsets.all(AppTokens.s24),
        child: Center(child: CircularProgressIndicator()),
      ),
      error: (e, _) => KitWarningBox(message: _friendly(e)),
      data: (list) => list.isEmpty
          ? Padding(
              padding: const EdgeInsets.all(AppTokens.s16),
              child: Text(
                  context.tr(
                      'No kits installed — use “Install kit” to add one.'),
                  style: TextStyle(color: c.textMuted, fontSize: 12)),
            )
          : Column(
              children: [
                for (final k in list)
                  _KitRow(kit: k, onRemove: () => _uninstall(k)),
              ],
            ),
    );
  }

  Widget _marketList(BuildContext context) {
    final c = context.colors;
    final offered = ref.watch(availableKitsProvider);
    return offered.when(
      loading: () => const Padding(
        padding: EdgeInsets.all(AppTokens.s24),
        child: Center(child: CircularProgressIndicator()),
      ),
      error: (e, _) => KitWarningBox(message: _friendly(e)),
      data: (list) => list.isEmpty
          ? Padding(
              padding: const EdgeInsets.all(AppTokens.s16),
              child: Text(
                  context.tr(
                      'No marketplace source offers a kit yet. Kits are declared in the '
                      'kits[] array of a source’s marketplace.json — add a source on the '
                      'Marketplace page.'),
                  style: TextStyle(color: c.textMuted, fontSize: 12)),
            )
          : Column(
              children: [
                for (final k in list)
                  _MarketKitRow(
                    kit: k,
                    onInstall: () => _openInstall(
                      source: KitInstallSource.market(
                        sourceId: k.sourceId,
                        sourceName: k.sourceName,
                        name: k.name,
                      ),
                    ),
                  ),
              ],
            ),
    );
  }
}

// ── Tab + hàng marketplace ──────────────────────────────────────────────────

class _TabButton extends StatelessWidget {
  const _TabButton(
      {required this.label, required this.active, required this.onTap});
  final String label;
  final bool active;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return InkWell(
      onTap: onTap,
      borderRadius: BorderRadius.circular(AppTokens.rSm),
      child: Container(
        padding: const EdgeInsets.symmetric(
            horizontal: AppTokens.s12, vertical: AppTokens.s6),
        decoration: BoxDecoration(
          color: active ? c.accent.withValues(alpha: 0.12) : Colors.transparent,
          borderRadius: BorderRadius.circular(AppTokens.rSm),
        ),
        child: Text(label,
            style: TextStyle(
              fontSize: 12,
              fontWeight: active ? FontWeight.w700 : FontWeight.w500,
              color: active ? c.accent : c.textSecondary,
            )),
      ),
    );
  }
}

class _MarketKitRow extends StatelessWidget {
  const _MarketKitRow({required this.kit, required this.onInstall});
  final AvailableKit kit;
  final VoidCallback onInstall;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Container(
      margin: const EdgeInsets.only(bottom: AppTokens.s8),
      padding: const EdgeInsets.all(AppTokens.s12),
      decoration: BoxDecoration(
        color: c.surface,
        border: Border.all(color: c.border),
        borderRadius: BorderRadius.circular(AppTokens.rMd),
      ),
      child: Row(children: [
        Expanded(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(children: [
                Flexible(
                  child: Text(kit.name,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(
                          fontSize: 13,
                          fontWeight: FontWeight.w600,
                          color: c.textPrimary)),
                ),
                const SizedBox(width: AppTokens.s8),
                if (kit.installedVersion != null)
                  Container(
                    padding: const EdgeInsets.symmetric(
                        horizontal: AppTokens.s6, vertical: 1),
                    decoration: BoxDecoration(
                      color: AppTokens.success.withValues(alpha: 0.12),
                      borderRadius: BorderRadius.circular(AppTokens.rSm),
                    ),
                    child: Text(context.tr('installed'),
                        style: const TextStyle(
                            fontSize: 10,
                            color: AppTokens.success,
                            fontWeight: FontWeight.w600)),
                  ),
              ]),
              const SizedBox(height: 2),
              Text(
                [
                  kit.sourceName,
                  if (kit.version.isNotEmpty) 'v${kit.version}',
                ].join(' · '),
                style: TextStyle(fontSize: 11, color: c.textMuted),
              ),
              if (kit.description.isNotEmpty) ...[
                const SizedBox(height: 4),
                Text(kit.description,
                    style: TextStyle(fontSize: 11, color: c.textSecondary)),
              ],
            ],
          ),
        ),
        const SizedBox(width: AppTokens.s12),
        Tooltip(
          message: kit.installable
              ? ''
              : context
                  .tr('The catalog entry declares no file to download.'),
          child: OutlinedButton.icon(
            onPressed: kit.installable ? onInstall : null,
            icon: const Icon(Icons.cloud_download_outlined, size: 15),
            label: Text(kit.installedVersion != null
                ? context.tr('Reinstall')
                : context.tr('Install')),
          ),
        ),
      ]),
    );
  }
}


// ── Hàng kit đã cài ─────────────────────────────────────────────────────────

class _KitRow extends StatefulWidget {
  const _KitRow({required this.kit, required this.onRemove});
  final KitReceipt kit;
  final VoidCallback onRemove;
  @override
  State<_KitRow> createState() => _KitRowState();
}

class _KitRowState extends State<_KitRow> {
  bool _open = true;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final k = widget.kit;
    final counts = k.countsByKind;
    return Container(
      margin: const EdgeInsets.only(bottom: AppTokens.s8),
      decoration: BoxDecoration(
        color: c.surface,
        border: Border.all(color: c.border),
        borderRadius: BorderRadius.circular(AppTokens.rMd),
      ),
      child: Column(
        children: [
          Padding(
            padding: const EdgeInsets.symmetric(
                horizontal: AppTokens.s12, vertical: AppTokens.s8),
            child: Row(
              children: [
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(k.name.isNotEmpty ? k.name : k.id,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(
                              color: c.textPrimary,
                              fontSize: 13,
                              fontWeight: FontWeight.w600)),
                      Padding(
                        padding: const EdgeInsets.only(top: 2),
                        child: Text(
                          '${k.id} · v${k.version}'
                          '${k.installedAt.isEmpty ? '' : ' · ${_shortTime(k.installedAt)}'}',
                          style: TextStyle(color: c.textMuted, fontSize: 11),
                        ),
                      ),
                      if (k.description.isNotEmpty)
                        Padding(
                          padding: const EdgeInsets.only(top: AppTokens.s6),
                          child: Text(k.description,
                              style: TextStyle(
                                  color: c.textSecondary,
                                  fontSize: 11,
                                  height: 1.4)),
                        ),
                      if (k.params.isNotEmpty) ...[
                        const SizedBox(height: AppTokens.s6),
                        Wrap(
                          spacing: AppTokens.s4,
                          runSpacing: AppTokens.s4,
                          children: [
                            Text(context.tr('Installed with:'),
                                style: TextStyle(
                                    color: c.textMuted, fontSize: 11)),
                            for (final e in k.params.entries)
                              _Chip(label: '${e.key} = ${e.value}'),
                          ],
                        ),
                      ],
                      const SizedBox(height: AppTokens.s6),
                      if (counts.isEmpty)
                        Text(
                            context
                                .tr('nothing created (every item already existed)'),
                            style: TextStyle(color: c.textMuted, fontSize: 11))
                      else
                        Wrap(
                          spacing: AppTokens.s4,
                          runSpacing: AppTokens.s4,
                          children: [
                            for (final e in counts.entries)
                              _Chip(
                                  label:
                                      '${kitKindLabel(context, e.key)}: ${e.value}'),
                          ],
                        ),
                    ],
                  ),
                ),
                if (k.items.isNotEmpty)
                  IconButton(
                    tooltip: context.tr('What it created'),
                    icon: Icon(_open ? Icons.expand_less : Icons.expand_more,
                        size: 18),
                    onPressed: () => setState(() => _open = !_open),
                  ),
                IconButton(
                  tooltip: context.tr('Remove'),
                  icon: const Icon(Icons.delete_outline,
                      size: 18, color: AppTokens.danger),
                  onPressed: widget.onRemove,
                ),
              ],
            ),
          ),
          if (_open && k.items.isNotEmpty)
            Container(
              width: double.infinity,
              padding: const EdgeInsets.fromLTRB(AppTokens.s12, 0,
                  AppTokens.s12, AppTokens.s8),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Divider(color: c.border, height: AppTokens.s12),
                  for (final i in k.items)
                    Padding(
                      padding: const EdgeInsets.only(bottom: AppTokens.s6),
                      child: Row(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          SizedBox(
                            width: 96,
                            child: _Chip(label: kitKindLabel(context, i.type)),
                          ),
                          const SizedBox(width: AppTokens.s8),
                          Expanded(
                            child: Column(
                              crossAxisAlignment: CrossAxisAlignment.start,
                              children: [
                                Text(i.name,
                                    style: TextStyle(
                                        color: c.textSecondary, fontSize: 12)),
                                SelectableText(
                                  i.where,
                                  style: TextStyle(
                                      color: c.textMuted,
                                      fontSize: 11,
                                      fontFamily: AppTokens.fontMono),
                                ),
                              ],
                            ),
                          ),
                        ],
                      ),
                    ),
                ],
              ),
            ),
        ],
      ),
    );
  }

  /// RFC3339 → giờ địa phương, đủ ngắn để nằm cùng dòng với id.
  static String _shortTime(String rfc3339) {
    final t = DateTime.tryParse(rfc3339);
    if (t == null) return rfc3339;
    final l = t.toLocal();
    String two(int n) => n.toString().padLeft(2, '0');
    return '${l.year}-${two(l.month)}-${two(l.day)} ${two(l.hour)}:${two(l.minute)}';
  }
}

// ── Preview ─────────────────────────────────────────────────────────────────

// ── Báo cáo cài / gỡ ────────────────────────────────────────────────────────

class KitReportCard extends StatelessWidget {
  const KitReportCard({super.key, required this.report, required this.onClose});
  final KitReport report;
  final VoidCallback onClose;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final title = report.isRemoval
        ? context.trArgs('Removal result for "{id}"', {'id': report.kitId})
        : context.trArgs('Install result for "{id}" (v{v})',
            {'id': report.kitId, 'v': report.version});
    return Container(
      padding: const EdgeInsets.all(AppTokens.s12),
      decoration: BoxDecoration(
        color: c.surface,
        border: Border.all(color: c.border),
        borderRadius: BorderRadius.circular(AppTokens.rMd),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Expanded(
                child: Text(title,
                    style: TextStyle(
                        color: c.textPrimary,
                        fontSize: 13,
                        fontWeight: FontWeight.w700)),
              ),
              TextButton(
                  onPressed: onClose,
                  child:
                      Text(context.tr('Close'), style: const TextStyle(fontSize: 12))),
            ],
          ),
          for (final w in report.warnings)
            Padding(
              padding: const EdgeInsets.only(bottom: AppTokens.s6),
              child: _WarningLine(warning: w),
            ),
          const SizedBox(height: AppTokens.s4),
          for (final i in report.items)
            Padding(
              padding: const EdgeInsets.only(bottom: AppTokens.s8),
              child: _OutcomeRow(outcome: i),
            ),
        ],
      ),
    );
  }
}

class _OutcomeRow extends StatelessWidget {
  const _OutcomeRow({required this.outcome});
  final KitOutcome outcome;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final meta = kitStatusMeta(context, outcome.status);
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        SizedBox(width: 96, child: _Chip(label: kitKindLabel(context, outcome.type))),
        const SizedBox(width: AppTokens.s8),
        Expanded(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(outcome.name,
                  style: TextStyle(color: c.textPrimary, fontSize: 12)),
              if (outcome.detail != null && outcome.detail!.isNotEmpty)
                Text(outcome.detail!,
                    style: TextStyle(color: c.textMuted, fontSize: 11)),
            ],
          ),
        ),
        const SizedBox(width: AppTokens.s8),
        Tooltip(
          message: meta.hint,
          child: _Chip(label: meta.label, color: meta.color),
        ),
      ],
    );
  }
}

// ── Mảnh dùng chung ─────────────────────────────────────────────────────────

class _Chip extends StatelessWidget {
  const _Chip({required this.label, this.color});
  final String label;
  final Color? color;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final tone = color ?? c.textMuted;
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: AppTokens.s6, vertical: 1),
      decoration: BoxDecoration(
        color: tone.withValues(alpha: 0.12),
        borderRadius: BorderRadius.circular(AppTokens.rSm),
        border: Border.all(color: tone.withValues(alpha: 0.35)),
      ),
      child: Text(label,
          style: TextStyle(color: tone, fontSize: 11, fontWeight: FontWeight.w500)),
    );
  }
}

class _WarningLine extends StatelessWidget {
  const _WarningLine({required this.warning});
  final KitWarning warning;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        _Chip(label: warning.kind, color: AppTokens.warning),
        const SizedBox(width: AppTokens.s6),
        Expanded(
          child: Text('${warning.subject} — ${warning.detail}',
              style: TextStyle(color: c.textSecondary, fontSize: 11)),
        ),
      ],
    );
  }
}

class KitWarningBox extends StatelessWidget {
  const KitWarningBox({super.key, required this.message, this.danger = false});
  final String message;
  final bool danger;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final tone = danger ? AppTokens.danger : AppTokens.warning;
    return Container(
      padding: const EdgeInsets.all(AppTokens.s12),
      decoration: BoxDecoration(
        color: tone.withValues(alpha: 0.08),
        border: Border.all(color: tone.withValues(alpha: 0.4)),
        borderRadius: BorderRadius.circular(AppTokens.rMd),
      ),
      child: Row(
        children: [
          Icon(Icons.warning_amber_rounded, size: 16, color: tone),
          const SizedBox(width: AppTokens.s8),
          Expanded(
            child: Text(message.replaceFirst('Exception: ', ''),
                style: TextStyle(color: c.textSecondary, fontSize: 12)),
          ),
        ],
      ),
    );
  }
}
