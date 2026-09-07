import '../models/kit_models.dart';
import 'api_client.dart';

/// REST client for `/api/kits*` — Zen Kits, one bundle that installs agents,
/// skills, workflows, hooks, jobs, Space Apps and pattern sources together.
///
/// Uploading a `.zip`/`kit.json` from the device is deliberately absent: those
/// two routes take a raw body or multipart, and the relay bridge carries JSON.
/// Everything installable from a source — including the kits compiled into the
/// daemon — goes through `available/{preview,install}`.
class KitsApi {
  static final KitsApi _instance = KitsApi._internal();
  factory KitsApi() => _instance;
  KitsApi._internal();

  final _api = ApiClient();

  List<Map<String, dynamic>> _maps(dynamic raw) => (raw is List ? raw : const [])
      .whereType<Map>()
      .map((e) => e.cast<String, dynamic>())
      .toList();

  Future<List<InstalledKit>> installed() async {
    final m = await _api.getObject('/api/kits');
    return _maps(m['kits']).map(InstalledKit.fromJson).toList();
  }

  /// Built-in kits plus whatever the marketplace offers. Built-ins are listed
  /// even with no marketplace configured — a kit that ships in the binary must
  /// stay installable on a fresh machine.
  Future<List<AvailableKit>> available() async {
    final m = await _api.getObject('/api/kits/available');
    return _maps(m['kits']).map(AvailableKit.fromJson).toList();
  }

  /// What an install would do. [params] is echoed back through validation, so
  /// re-previewing after filling a field is how the dialog clears
  /// [KitPreview.paramError].
  Future<KitPreview> preview({
    required String sourceId,
    required String name,
    Map<String, dynamic> params = const {},
  }) async {
    final m = await _api.post(
      '/api/kits/available/preview',
      body: {'sourceId': sourceId, 'name': name, 'params': params},
      timeout: const Duration(minutes: 2),
    );
    return KitPreview.fromJson(
      m is Map ? m.cast<String, dynamic>() : const {},
    );
  }

  /// Install. Long timeout on purpose: a kit carrying a Space App runs the
  /// security scan plus that app's one-off dependency prepare step, and a kit
  /// carrying a git pattern source clones it.
  Future<KitReport> install({
    required String sourceId,
    required String name,
    Map<String, dynamic> params = const {},
    bool force = false,
  }) async {
    final m = await _api.post(
      '/api/kits/available/install',
      body: {
        'sourceId': sourceId,
        'name': name,
        'params': params,
        if (force) 'force': true,
      },
      timeout: const Duration(minutes: 10),
    );
    return KitReport.fromJson(m is Map ? m.cast<String, dynamic>() : const {});
  }

  /// Remove everything the receipt says the kit created.
  ///
  /// The daemon keeps the receipt when anything failed — it is the only record
  /// of what is still out there — so a failed uninstall leaves the kit listed.
  Future<KitReport> uninstall(String id) async {
    final m = await _api.delete('/api/kits/${Uri.encodeComponent(id)}');
    return KitReport.fromJson(m is Map ? m.cast<String, dynamic>() : const {});
  }
}
