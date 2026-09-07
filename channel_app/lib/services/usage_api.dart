import '../models/usage_models.dart';
import 'api_client.dart';

/// REST client for `/api/usage/*` — token/cost accounting over the relay.
///
/// The pricing table (`/api/usage/pricing`) is deliberately absent: editing
/// per-model prices is daemon administration, which this remote-control app
/// does not do.
class UsageApi {
  static final UsageApi _instance = UsageApi._internal();
  factory UsageApi() => _instance;
  UsageApi._internal();

  final _api = ApiClient();

  List<Map<String, dynamic>> _maps(dynamic raw) => (raw is List ? raw : const [])
      .whereType<Map>()
      .map((e) => e.cast<String, dynamic>())
      .toList();

  Future<UsageOverview> overview() async =>
      UsageOverview.fromJson(await _api.getObject('/api/usage/overview'));

  /// Per-day rollup, oldest first. The daemon clamps `days` to 1..365.
  Future<List<UsageDailyRow>> daily({int days = 30}) async {
    final m = await _api
        .getObject(ApiClient.withQuery('/api/usage/daily', {'days': days}));
    return _maps(m['rows']).map(UsageDailyRow.fromJson).toList();
  }

  /// [by] is one of `model`, `source`, `jid`, `app`. The daemon rejects
  /// anything else with a 400, and clamps `days` to 1..90.
  Future<List<UsageBreakdownRow>> breakdown({
    String by = 'model',
    int days = 7,
  }) async {
    final m = await _api.getObject(
      ApiClient.withQuery('/api/usage/breakdown', {'by': by, 'days': days}),
    );
    return _maps(m['rows']).map(UsageBreakdownRow.fromJson).toList();
  }
}
