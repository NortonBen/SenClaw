import 'dart:convert';

import 'version.dart';

/// One downloadable bundle in `latest.json`.
class UpdateAsset {
  const UpdateAsset({required this.name, required this.size, required this.sha256});

  final String name;

  /// Bytes. 0 when the manifest omitted it — only used to render progress.
  final int size;

  /// Hex SHA-256, empty if absent. The bundle is unsigned, so this is the only
  /// integrity check between GitHub and the user's disk; `apply-update` refuses
  /// to install on a mismatch.
  final String sha256;

  static UpdateAsset? _tryParse(Object? raw) {
    if (raw is! Map) return null;
    final name = raw['name'];
    if (name is! String || name.isEmpty) return null;
    return UpdateAsset(
      name: name,
      size: (raw['size'] as num?)?.toInt() ?? 0,
      sha256: raw['sha256'] is String ? raw['sha256'] as String : '',
    );
  }
}

/// Parsed `latest.json` — the release manifest published as an asset by the
/// `Generate update manifest` step in .github/workflows/desktop.yml.
class UpdateManifest {
  const UpdateManifest({
    required this.version,
    required this.assets,
    this.notes,
    this.publishedAt,
    this.minVersion,
  });

  final Version version;

  /// Keyed by Rust target triple, e.g. `aarch64-apple-darwin`.
  final Map<String, UpdateAsset> assets;

  final String? notes;
  final DateTime? publishedAt;

  /// Builds older than this cannot update straight to [version] (reserved for
  /// releases with a migration that needs a stop along the way). Unset today.
  final Version? minVersion;

  /// Tolerant on purpose: a malformed or half-understood manifest yields null
  /// so the caller can stay quiet, rather than throwing into the user's face
  /// during a background check they never asked for.
  static UpdateManifest? tryParse(String body) {
    Object? raw;
    try {
      raw = jsonDecode(body);
    } catch (_) {
      return null;
    }
    if (raw is! Map) return null;

    final v = raw['version'];
    if (v is! String) return null;
    final version = Version.tryParse(v);
    if (version == null) return null;

    final assets = <String, UpdateAsset>{};
    final rawAssets = raw['assets'];
    if (rawAssets is Map) {
      rawAssets.forEach((k, value) {
        final a = UpdateAsset._tryParse(value);
        if (a != null) assets['$k'] = a;
      });
    }

    return UpdateManifest(
      version: version,
      assets: assets,
      notes: raw['notes'] is String ? raw['notes'] as String : null,
      publishedAt:
          raw['publishedAt'] is String ? DateTime.tryParse(raw['publishedAt'] as String) : null,
      minVersion: raw['minVersion'] is String
          ? Version.tryParse(raw['minVersion'] as String)
          : null,
    );
  }

  UpdateAsset? assetFor(String triple) => assets[triple];
}
