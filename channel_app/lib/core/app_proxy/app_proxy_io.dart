import 'dart:convert';
import 'dart:io';
import '../../services/relay_manager.dart';
import 'asset_cache.dart';

/// A loopback HTTP server that forwards a Space app's asset/API requests through
/// the relay tunnel, so a normal webview can render the app remotely.
///
/// Browser request `http://127.0.0.1:<port>/<path>` →
/// relay `GET /api/space/apps/<id>/proxy/<path>` → bytes (base64-safe) → served
/// back with the daemon's Content-Type.
///
/// Static assets are cached locally in SQLite keyed by [version] (the app's
/// `installed_at`, bumped on redeploy) — repeat opens are served from disk and
/// only go through the relay again when the app actually changed.
class AppProxyServer {
  AppProxyServer(this.appId, {this.version = 0});
  final String appId;

  /// App registration version; 0 = unknown → caching disabled.
  final int version;
  HttpServer? _server;

  Future<String> start() async {
    if (version > 0) {
      // Evict assets from older deploys of this app before serving.
      await AppAssetCache.instance.pruneVersion(appId, version);
    }
    final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    _server = server;
    server.listen(_handle);
    return 'http://127.0.0.1:${server.port}/';
  }

  Future<void> _handle(HttpRequest req) async {
    final res = req.response;
    try {
      final relay = RelayManager().relay;
      if (relay == null) {
        res.statusCode = 503;
        await res.close();
        return;
      }

      // Map the loopback path onto the daemon's app-proxy path. Handle both
      // relative refs (`app.js`) and absolute ones the app may emit
      // (`/api/space/apps/<id>/proxy/app.js`).
      final p = req.uri.path;
      const marker = '/proxy/';
      final String rel = p.contains(marker)
          ? p.substring(p.indexOf(marker) + marker.length)
          : (p.startsWith('/') ? p.substring(1) : p);
      final query = req.uri.hasQuery ? '?${req.uri.query}' : '';
      final apiPath = '/api/space/apps/$appId/proxy/$rel$query';
      final cacheKey = '$rel$query';

      // Serve static assets from the local cache (version-keyed) — no relay
      // round-trip unless the app was redeployed.
      final canCache = version > 0 && req.method == 'GET';
      if (canCache) {
        final hit =
            await AppAssetCache.instance.get(appId, version, cacheKey);
        if (hit != null) {
          res.statusCode = 200;
          if (hit.contentType != null && hit.contentType!.isNotEmpty) {
            res.headers.set(HttpHeaders.contentTypeHeader, hit.contentType!);
          }
          res.headers.set('access-control-allow-origin', '*');
          res.headers.set('x-senclaw-cache', 'hit');
          res.add(hit.bytes);
          await res.close();
          return;
        }
      }

      String? body;
      if (req.method != 'GET' && req.method != 'HEAD') {
        body = await utf8.decoder.bind(req).join();
      }

      final resp = await relay.apiRequestRaw(req.method, apiPath, rawBody: body);
      res.statusCode = resp.status;
      if (resp.contentType != null && resp.contentType!.isNotEmpty) {
        res.headers.set(HttpHeaders.contentTypeHeader, resp.contentType!);
      }
      // Allow the webview to read everything (loopback origin).
      res.headers.set('access-control-allow-origin', '*');
      res.add(resp.bytes);
      await res.close();

      // Store after responding — fire-and-forget, never delays the page.
      if (canCache &&
          resp.status == 200 &&
          AppAssetCache.cacheable(rel, resp.contentType)) {
        // ignore: unawaited_futures
        AppAssetCache.instance
            .put(appId, version, cacheKey, resp.contentType, resp.bytes);
      }
    } catch (e) {
      try {
        res.statusCode = 502;
        res.write('proxy error: $e');
        await res.close();
      } catch (_) {/* response already closed */}
    }
  }

  Future<void> stop() async {
    await _server?.close(force: true);
    _server = null;
  }

  /// Forget every cached asset of [appId] (explicit "hard refresh").
  static Future<void> clearAppCache(String appId) =>
      AppAssetCache.instance.clearApp(appId);
}
