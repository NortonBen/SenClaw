/// Zen Kits — one bundle that installs agents, skills, workflows, hooks, jobs,
/// Space Apps and pattern sources together.
///
/// The daemon serialises these camelCase. Two catalogs meet in one list:
/// kits compiled into the binary (`sourceId == "builtin"`, installable with no
/// marketplace at all) and kits offered by a configured marketplace source.
library;

/// A kit already installed on this daemon (`GET /api/kits`).
class InstalledKit {
  final String id;
  final String version;
  final String name;
  final String description;

  /// RFC3339 UTC.
  final String installedAt;
  final int itemCount;

  const InstalledKit({
    required this.id,
    this.version = '',
    this.name = '',
    this.description = '',
    this.installedAt = '',
    this.itemCount = 0,
  });

  String get title => name.isEmpty ? id : name;

  factory InstalledKit.fromJson(Map<String, dynamic> j) => InstalledKit(
        id: (j['id'] ?? '').toString(),
        version: (j['version'] ?? '').toString(),
        name: (j['name'] ?? '').toString(),
        description: (j['description'] ?? '').toString(),
        installedAt: (j['installedAt'] ?? '').toString(),
        itemCount: (j['items'] as List?)?.length ?? 0,
      );
}

/// A kit on offer (`GET /api/kits/available`). Installing one needs both
/// [sourceId] and [name] — the daemon looks the artifact up by that pair.
class AvailableKit {
  final String sourceId;
  final String sourceName;
  final String name;

  /// The manifest's declared id. Absent for a catalog entry that omits it, in
  /// which case the daemon matches the installed badge on [name] instead.
  final String? id;
  final String description;
  final String version;
  final String author;
  final String category;
  final String? homepage;

  /// False when the catalog entry carries no artifact URL — nothing to fetch.
  final bool installable;

  /// Non-null when this daemon already has it; the row shows an update instead
  /// of offering the same thing twice.
  final String? installedVersion;

  const AvailableKit({
    required this.sourceId,
    required this.name,
    this.sourceName = '',
    this.id,
    this.description = '',
    this.version = '',
    this.author = '',
    this.category = '',
    this.homepage,
    this.installable = true,
    this.installedVersion,
  });

  bool get installed => installedVersion != null;

  /// True only when both versions are known and differ — an unknown version on
  /// either side must not render as "update available".
  bool get hasUpdate =>
      installed && version.isNotEmpty && installedVersion != version;

  factory AvailableKit.fromJson(Map<String, dynamic> j) => AvailableKit(
        sourceId: (j['sourceId'] ?? '').toString(),
        sourceName: (j['sourceName'] ?? '').toString(),
        name: (j['name'] ?? '').toString(),
        id: j['id']?.toString(),
        description: (j['description'] ?? '').toString(),
        version: (j['version'] ?? '').toString(),
        author: (j['author'] ?? '').toString(),
        category: (j['category'] ?? '').toString(),
        homepage: j['homepage']?.toString(),
        installable: j['installable'] != false,
        installedVersion: j['installedVersion']?.toString(),
      );
}

/// One declared install parameter, substituted into the manifest as
/// `{{param.<key>}}` **before** anything reaches disk.
class KitParam {
  final String key;
  final String label;

  /// `string` | `number` | `boolean` | `select` | `folder`.
  final String type;
  final String description;
  final String placeholder;
  final String? defaultValue;
  final bool required;

  /// Render masked and keep out of the receipt.
  final bool secret;

  /// `select` only: `value` → `label`.
  final List<MapEntry<String, String>> options;

  const KitParam({
    required this.key,
    this.label = '',
    this.type = 'string',
    this.description = '',
    this.placeholder = '',
    this.defaultValue,
    this.required = false,
    this.secret = false,
    this.options = const [],
  });

  String get title => label.isEmpty ? key : label;

  factory KitParam.fromJson(Map<String, dynamic> j) => KitParam(
        key: (j['key'] ?? '').toString(),
        label: (j['label'] ?? '').toString(),
        type: (j['type'] ?? 'string').toString(),
        description: (j['description'] ?? '').toString(),
        placeholder: (j['placeholder'] ?? '').toString(),
        defaultValue: j['default']?.toString(),
        required: j['required'] == true,
        secret: j['secret'] == true,
        options: ((j['options'] as List?) ?? const [])
            .whereType<Map>()
            .map((o) => MapEntry(
                  (o['value'] ?? '').toString(),
                  (o['label'] ?? o['value'] ?? '').toString(),
                ))
            .toList(),
      );
}

/// One thing a kit would install, from the preview.
class KitPreviewItem {
  /// `agent` | `skill` | `workflow` | `hook` | `job` | `app` | `mcpServer`.
  final String type;
  final String name;
  final String? description;

  /// `job` only — the schedule it installs.
  final String? cron;

  /// `job` only. False means it installs **paused**; worth saying, because a
  /// schedule that starts running on install spends tokens before anyone reads
  /// what it does.
  final bool? enabled;

  /// `mcpServer` — the daemon marks these as not installable from a kit.
  final bool unsupported;

  const KitPreviewItem({
    required this.type,
    required this.name,
    this.description,
    this.cron,
    this.enabled,
    this.unsupported = false,
  });

  factory KitPreviewItem.fromJson(Map<String, dynamic> j) => KitPreviewItem(
        type: (j['type'] ?? '').toString(),
        name: (j['name'] ?? '').toString(),
        description: j['description']?.toString(),
        cron: j['cron']?.toString(),
        enabled: j['enabled'] is bool ? j['enabled'] as bool : null,
        unsupported: j['unsupported'] == true,
      );
}

/// `POST /api/kits/available/preview` — what an install would do, before doing it.
class KitPreview {
  final String id;
  final String name;
  final String version;
  final String description;
  final List<KitParam> params;

  /// Set when the supplied params do not satisfy the manifest. Install would
  /// fail with the same message, so the dialog blocks on it.
  final String? paramError;
  final List<KitPreviewItem> items;
  final String? installedVersion;

  const KitPreview({
    required this.id,
    this.name = '',
    this.version = '',
    this.description = '',
    this.params = const [],
    this.paramError,
    this.items = const [],
    this.installedVersion,
  });

  factory KitPreview.fromJson(Map<String, dynamic> j) {
    final installed = j['installed'];
    return KitPreview(
      id: (j['id'] ?? '').toString(),
      name: (j['name'] ?? '').toString(),
      version: (j['version'] ?? '').toString(),
      description: (j['description'] ?? '').toString(),
      params: ((j['params'] as List?) ?? const [])
          .whereType<Map>()
          .map((m) => KitParam.fromJson(m.cast<String, dynamic>()))
          .toList(),
      paramError: j['paramError']?.toString(),
      items: ((j['items'] as List?) ?? const [])
          .whereType<Map>()
          .map((m) => KitPreviewItem.fromJson(m.cast<String, dynamic>()))
          .toList(),
      installedVersion:
          installed is Map ? installed['version']?.toString() : null,
    );
  }
}

/// Outcome of an install or uninstall. Every item reports its own status, so a
/// partly-failed install is visible rather than reported as a flat failure.
class KitReport {
  final bool ok;
  final List<KitReportItem> items;
  final List<String> warnings;

  const KitReport({
    this.ok = false,
    this.items = const [],
    this.warnings = const [],
  });

  int get failed => items.where((i) => i.status == 'failed').length;

  /// Install reports use `created`; uninstall reports use `removed`. Counting
  /// both keeps one summary line correct for either direction.
  int get changed => items
      .where((i) => i.status == 'created' || i.status == 'removed')
      .length;

  factory KitReport.fromJson(Map<String, dynamic> j) {
    final report = (j['report'] as Map?)?.cast<String, dynamic>() ?? const {};
    return KitReport(
      ok: j['ok'] == true,
      items: ((report['items'] as List?) ?? const [])
          .whereType<Map>()
          .map((m) => KitReportItem.fromJson(m.cast<String, dynamic>()))
          .toList(),
      // A warning is `{kind, subject, detail}`; the detail alone is the part
      // worth reading, with the subject only when it names something.
      warnings: ((report['warnings'] as List?) ?? const []).map((w) {
        if (w is! Map) return w.toString();
        final subject = (w['subject'] ?? '').toString();
        final detail = (w['detail'] ?? '').toString();
        return subject.isEmpty ? detail : '$subject: $detail';
      }).toList(),
    );
  }
}

class KitReportItem {
  /// `agent` | `skill` | `workflow` | `hook` | `job` | `app` | `patternSource`…
  ///
  /// Arrives on the wire as `type` — the Rust field is `kind` but carries
  /// `#[serde(rename = "type")]`.
  final String kind;
  final String name;

  /// Install: `created` | `skipped` | `unsupported` | `failed`.
  /// Uninstall: `removed` | `missing` | `failed`.
  final String status;
  final String? detail;

  const KitReportItem({
    required this.kind,
    required this.name,
    this.status = '',
    this.detail,
  });

  factory KitReportItem.fromJson(Map<String, dynamic> j) => KitReportItem(
        kind: (j['type'] ?? j['kind'] ?? '').toString(),
        name: (j['name'] ?? '').toString(),
        status: (j['status'] ?? '').toString(),
        detail: j['detail']?.toString(),
      );
}
