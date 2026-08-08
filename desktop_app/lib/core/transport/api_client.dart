import 'dart:async';
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

/// Ceiling on a single request. `package:http` has NO default timeout, and a
/// socket that connects but never answers therefore waits forever — which is
/// exactly how a half-dead daemon (or an unrelated process squatting on 18788)
/// used to leave the app on a blank white screen with nothing to click: the
/// startup gate was awaiting a future that could not complete. Every request
/// now ends, one way or the other.
const Duration kApiTimeout = Duration(seconds: 30);

/// Thrown when a request outlives its timeout. Distinct from [ApiException] so
/// callers can tell "the daemon said no" from "the daemon said nothing".
class ApiTimeout implements Exception {
  final String method;
  final String path;
  final Duration limit;
  ApiTimeout(this.method, this.path, this.limit);
  @override
  String toString() =>
      '$method $path did not answer within ${limit.inSeconds}s';
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

  Future<dynamic> get(String path,
          {Map<String, dynamic>? query, Duration? timeout}) =>
      _send('GET', path, query: query, timeout: timeout);

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
    Duration? timeout,
  }) async {
    final limit = timeout ?? kApiTimeout;
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
        // The whole exchange — connect, headers, body — shares one budget:
        // a server that sends headers and then stalls must not hang us either.
        final streamed = await _http.send(req).timeout(limit);
        res = await http.Response.fromStream(streamed).timeout(limit);
        break;
      } on TimeoutException {
        throw ApiTimeout(method, path, limit);
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
