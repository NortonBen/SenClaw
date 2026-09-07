import '../models/dispatch_models.dart';
import 'api_client.dart';

/// Read-only client for `GET /api/dispatch`.
///
/// The daemon's live view is the `dispatch:update` WebSocket event, which goes
/// to admin clients only — a relay client cannot receive it. So this is a
/// snapshot the caller polls while a screen is open, exactly as the background
/// tasks screen does with `bg:*`.
class DispatchApi {
  static final DispatchApi _instance = DispatchApi._internal();
  factory DispatchApi() => _instance;
  DispatchApi._internal();

  final _api = ApiClient();

  Future<List<DispatchParent>> parents() async {
    final m = await _api.getObject('/api/dispatch');
    return ((m['parents'] as List?) ?? const [])
        .whereType<Map>()
        .map((e) => DispatchParent.fromJson(e.cast<String, dynamic>()))
        .toList();
  }

  /// Re-run one failed subtask.
  ///
  /// The daemon refuses with a reason written for a person ("already running",
  /// "finished successfully"). That reason travels out of here as the thrown
  /// message so the screen can show it verbatim — a generic failure would just
  /// make the user tap again.
  Future<void> retryTask(String taskId) =>
      _api.post('/api/dispatch/tasks/$taskId/retry');

  /// Re-run every failed subtask of one dispatch.
  Future<void> retryParent(String parentId) =>
      _api.post('/api/dispatch/parents/$parentId/retry');
}
