import '../models/api_models.dart';
import 'api_client.dart';

/// REST client for `/api/workbench/:jid/:id/*`.
///
/// Only four operations exist — there is deliberately no list endpoint, so
/// artifacts come from [WorkbenchStore], which is fed by relay events.
class WorkbenchApi {
  static final WorkbenchApi _instance = WorkbenchApi._internal();
  factory WorkbenchApi() => _instance;
  WorkbenchApi._internal();

  final _api = ApiClient();

  String _base(String jid, String id) =>
      '/api/workbench/${Uri.encodeComponent(jid)}/${Uri.encodeComponent(id)}';

  Future<bool> markViewed(String jid, String id) async {
    final m = await _api.post('${_base(jid, id)}/mark-viewed');
    return m is Map && m['ok'] == true;
  }

  /// Close on the daemon: stops a `backend` artifact's process and drops it
  /// from the engine's workbench.
  Future<bool> close(String jid, String id) async {
    final m = await _api.post('${_base(jid, id)}/close');
    return m is Map && m['ok'] == true;
  }

  /// Read one file out of the artifact's directory. Files arrive inline on the
  /// event when small; this is the path for the ones that do not.
  ///
  /// This endpoint reports failure as `{"error": …}` with **HTTP 200**, so
  /// [ApiClient] sees a success and the error has to be unpacked here — read
  /// only `content` and an unreadable file becomes an empty one.
  Future<String> readFile(String jid, String id, String path) async {
    final m = await _api.getObject(
      ApiClient.withQuery('${_base(jid, id)}/read-file', {'path': path}),
    );
    final error = m['error'];
    if (error != null && error.toString().isNotEmpty) {
      throw ApiException(200, error.toString());
    }
    return (m['content'] ?? '').toString();
  }

  /// Tail of a `backend` artifact's log — the only way to see why one crashed.
  Future<String> logs(String jid, String id, {int tail = 200}) async {
    final m = await _api.getObject(
      ApiClient.withQuery('${_base(jid, id)}/logs', {'tail': tail}),
    );
    return (m['logs'] ?? m['content'] ?? '').toString();
  }
}
