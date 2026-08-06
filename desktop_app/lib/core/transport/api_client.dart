import 'dart:convert';
import 'package:http/http.dart' as http;
import '../config/app_config.dart';

/// Error thrown for non-2xx responses; carries the daemon's `{error}` body.
class ApiException implements Exception {
  final int status;
  final String message;
  ApiException(this.status, this.message);
  @override
  String toString() => 'ApiException($status): $message';
}

/// Thin REST wrapper over the daemon's `/api/*` surface.
///
/// This is the single seam that differs between the desktop build (direct
/// localhost) and a future mobile build (relay tunnel): swap this class for a
/// relay-backed implementation and every feature keeps working unchanged.
class ApiClient {
  ApiClient(this._config, {http.Client? client})
    : _http = client ?? http.Client();

  AppConfig _config;
  final http.Client _http;

  /// Updated after `/api/config` discovery (wsPort/token may change).
  void updateConfig(AppConfig config) => _config = config;

  Uri _uri(String path, [Map<String, dynamic>? query]) {
    final p = path.startsWith('/') ? path : '/$path';
    return Uri.parse('${_config.httpBase}$p').replace(
      queryParameters: query?.map((k, v) => MapEntry(k, '$v')),
    );
  }

  Future<dynamic> get(String path, {Map<String, dynamic>? query}) =>
      _send('GET', path, query: query);

  Future<dynamic> post(String path, {Object? body}) =>
      _send('POST', path, body: body);

  Future<dynamic> put(String path, {Object? body}) =>
      _send('PUT', path, body: body);

  Future<dynamic> patch(String path, {Object? body}) =>
      _send('PATCH', path, body: body);

  Future<dynamic> delete(String path, {Object? body}) =>
      _send('DELETE', path, body: body);

  Future<dynamic> _send(
    String method,
    String path, {
    Object? body,
    Map<String, dynamic>? query,
  }) async {
    // The Dart `http` keep-alive pool can hand back a socket the daemon has
    // already half-closed, surfacing as "Connection closed before full header
    // was received". The request never reached the server, so retrying on a
    // fresh socket is safe — do so a couple of times with a short backoff.
    http.Response? res;
    for (var attempt = 0; ; attempt++) {
      final req = http.Request(method, _uri(path, query));
      req.headers['accept'] = 'application/json';
      req.headers.addAll(_config.authHeaders);
      if (body != null) {
        req.headers['content-type'] = 'application/json';
        req.body = jsonEncode(body);
      }
      try {
        final streamed = await _http.send(req);
        res = await http.Response.fromStream(streamed);
        break;
      } on http.ClientException catch (e) {
        final transient = e.message.contains('Connection closed') ||
            e.message.contains('Connection reset') ||
            e.message.contains('Connection refused');
        if (!transient || attempt >= 2) rethrow;
        await Future.delayed(Duration(milliseconds: 80 * (attempt + 1)));
      }
    }
    if (res.statusCode < 200 || res.statusCode >= 300) {
      String msg = res.body;
      try {
        final decoded = jsonDecode(res.body);
        if (decoded is Map && decoded['error'] != null) {
          msg = decoded['error'].toString();
        }
      } catch (_) {}
      throw ApiException(res.statusCode, msg);
    }
    if (res.body.isEmpty) return null;
    try {
      return jsonDecode(res.body);
    } catch (_) {
      return res.body; // non-JSON (e.g. raw text/markdown)
    }
  }

  void dispose() => _http.close();
}
