import 'dart:typed_data';

import 'package:sqflite/sqflite.dart';

/// SQLite-backed cache of Space-app static assets fetched through the relay.
///
/// Every asset is keyed by `(appId, path)` and stamped with the app's
/// registration version (`installed_at` from the daemon). A version mismatch
/// invalidates the whole app's cache, so assets are re-downloaded only when
/// the app was actually redeployed — repeat opens render instantly from disk
/// instead of pulling every file through the relay tunnel again.
///
/// Only immutable-per-version GET responses belong here; the proxy skips the
/// app's own API calls (see [AppAssetCache.cacheable]).
class AppAssetCache {
  AppAssetCache._();
  static final AppAssetCache instance = AppAssetCache._();

  static const _dbName = 'senclaw_asset_cache.db';

  /// Per-asset ceiling — bigger blobs (videos…) aren't worth caching.
  static const maxBlobBytes = 8 * 1024 * 1024;

  Database? _db;

  Future<Database?> _open() async {
    if (_db != null) return _db;
    try {
      _db = await openDatabase(
        _dbName,
        version: 1,
        onCreate: (db, _) => db.execute('''
          CREATE TABLE assets (
            app_id       TEXT NOT NULL,
            path         TEXT NOT NULL,
            version      INTEGER NOT NULL,
            content_type TEXT,
            bytes        BLOB NOT NULL,
            ts           INTEGER NOT NULL,
            PRIMARY KEY (app_id, path)
          )
        '''),
      );
    } catch (_) {
      // Cache is best-effort — a broken DB must never break the webview.
      _db = null;
    }
    return _db;
  }

  /// Drop stale rows of [appId] whose version differs from [version].
  /// Call once when the proxy starts.
  Future<void> pruneVersion(String appId, int version) async {
    final db = await _open();
    if (db == null) return;
    try {
      await db.delete('assets',
          where: 'app_id = ? AND version != ?', whereArgs: [appId, version]);
    } catch (_) {/* best-effort */}
  }

  Future<CachedAsset?> get(String appId, int version, String path) async {
    final db = await _open();
    if (db == null) return null;
    try {
      final rows = await db.query('assets',
          columns: ['content_type', 'bytes'],
          where: 'app_id = ? AND path = ? AND version = ?',
          whereArgs: [appId, path, version],
          limit: 1);
      if (rows.isEmpty) return null;
      final bytes = rows.first['bytes'];
      if (bytes is! Uint8List) return null;
      return CachedAsset(rows.first['content_type'] as String?, bytes);
    } catch (_) {
      return null;
    }
  }

  Future<void> put(String appId, int version, String path,
      String? contentType, Uint8List bytes) async {
    if (bytes.length > maxBlobBytes) return;
    final db = await _open();
    if (db == null) return;
    try {
      await db.insert(
        'assets',
        {
          'app_id': appId,
          'path': path,
          'version': version,
          'content_type': contentType,
          'bytes': bytes,
          'ts': DateTime.now().millisecondsSinceEpoch,
        },
        conflictAlgorithm: ConflictAlgorithm.replace,
      );
    } catch (_) {/* best-effort */}
  }

  /// Forget everything cached for [appId] (e.g. explicit refresh).
  Future<void> clearApp(String appId) async {
    final db = await _open();
    if (db == null) return;
    try {
      await db.delete('assets', where: 'app_id = ?', whereArgs: [appId]);
    } catch (_) {/* best-effort */}
  }

  /// Whether a successful GET response should be cached: static UI assets
  /// (html/js/css/images/fonts/wasm) yes; the app's own API traffic no.
  static bool cacheable(String relPath, String? contentType) {
    if (relPath.startsWith('api/') || relPath == 'api') return false;
    final ct = (contentType ?? '').toLowerCase();
    if (ct.contains('json') || ct.contains('event-stream')) return false;
    return true;
  }
}

class CachedAsset {
  const CachedAsset(this.contentType, this.bytes);
  final String? contentType;
  final Uint8List bytes;
}
