import 'dart:convert';
import '../models/api_models.dart';
import 'relay_manager.dart';

/// REST client that tunnels every call through the shared relay
/// (see [RelayManager] + the daemon's API_REQ/API_RESP bridge).
///
/// Mirrors the `/api/*` surface the React web UI talks to, but over the
/// encrypted relay instead of a direct localhost HTTP connection.
class ApiClient {
  static final ApiClient _instance = ApiClient._internal();
  factory ApiClient() => _instance;
  ApiClient._internal();

  final _manager = RelayManager();

  Future<dynamic> _send(String method, String path, {Object? body}) async {
    await _manager.ensureStarted();
    final relay = _manager.relay;
    if (relay == null) {
      throw const ApiException(0, 'no relay connection');
    }
    final resp = await relay.apiRequest(method, path, body: body);
    if (!resp.isOk) {
      throw ApiException(resp.status, _extractError(resp.body));
    }
    if (resp.body.isEmpty) return null;
    try {
      return jsonDecode(resp.body);
    } catch (_) {
      return resp.body; // non-JSON body (rare)
    }
  }

  String _extractError(String body) {
    try {
      final decoded = jsonDecode(body);
      if (decoded is Map && decoded['error'] != null) {
        return decoded['error'].toString();
      }
    } catch (_) {}
    return body.isEmpty ? 'request failed' : body;
  }

  Future<dynamic> get(String path) => _send('GET', path);
  Future<dynamic> post(String path, {Object? body}) =>
      _send('POST', path, body: body);
  Future<dynamic> put(String path, {Object? body}) =>
      _send('PUT', path, body: body);
  Future<dynamic> patch(String path, {Object? body}) =>
      _send('PATCH', path, body: body);
  Future<dynamic> delete(String path, {Object? body}) =>
      _send('DELETE', path, body: body);

  /// GET returning a JSON object.
  Future<Map<String, dynamic>> getObject(String path) async {
    final r = await get(path);
    if (r is Map) return r.cast<String, dynamic>();
    return <String, dynamic>{};
  }

  /// GET returning a JSON array.
  Future<List<dynamic>> getList(String path) async {
    final r = await get(path);
    if (r is List) return r;
    return const [];
  }

  /// Append a query string to [base], skipping null/empty values.
  static String withQuery(String base, Map<String, dynamic> params) {
    final entries = params.entries
        .where((e) => e.value != null && e.value.toString().isNotEmpty)
        .map(
          (e) =>
              '${Uri.encodeQueryComponent(e.key)}=${Uri.encodeQueryComponent(e.value.toString())}',
        )
        .toList();
    if (entries.isEmpty) return base;
    return '$base?${entries.join('&')}';
  }
}
