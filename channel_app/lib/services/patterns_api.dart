import '../models/pattern_models.dart';
import 'api_client.dart';

/// REST client for `/api/patterns/*` — Zen Patterns, the daemon's named
/// one-shot prompt transforms. Tunnels through the relay like every other
/// surface here.
///
/// Import (`POST /api/patterns/import`) is deliberately absent: it is a
/// multipart zip upload, and the relay bridge carries a JSON body. Zips are
/// managed from the desktop/web admin UI.
class PatternsApi {
  static final PatternsApi _instance = PatternsApi._internal();
  factory PatternsApi() => _instance;
  PatternsApi._internal();

  final _api = ApiClient();

  List<Map<String, dynamic>> _maps(dynamic raw) => (raw is List ? raw : const [])
      .whereType<Map>()
      .map((e) => e.cast<String, dynamic>())
      .toList();

  /// The list, its sources and the strategy picker in one call — the daemon
  /// sends all three together because the UI groups patterns by source.
  Future<PatternCatalogPage> list({String query = '', String? source}) async {
    final path = ApiClient.withQuery('/api/patterns', {
      'q': query,
      'source': source,
    });
    final m = await _api.getObject(path);
    return PatternCatalogPage(
      patterns: _maps(m['patterns']).map(PatternEntry.fromJson).toList(),
      sources: _maps(m['sources']).map(PatternSource.fromJson).toList(),
      strategies:
          _maps(m['strategies']).map(PatternStrategy.fromJson).toList(),
    );
  }

  /// Full body of one pattern. `writable` comes off the *source*, so it is
  /// merged in here rather than read off the pattern object.
  Future<PatternFiles> get(String name) async {
    final m = await _api.getObject('/api/patterns/${Uri.encodeComponent(name)}');
    final p = (m['pattern'] as Map?)?.cast<String, dynamic>() ?? const {};
    final src = (m['source'] as Map?)?.cast<String, dynamic>();
    return PatternFiles.fromJson({
      ...p,
      if (src != null && p['writable'] == null)
        'writable': PatternSource.fromJson(src).kind == 'local',
    });
  }

  /// Run a pattern. A real run is a full LLM call, so it gets a long timeout —
  /// the daemon's own ceiling is 32k output tokens and it will not answer fast.
  ///
  /// [language] `"auto"` follows the input's language; a language name pins it;
  /// absent leaves the pattern's own wording in charge. Fabric patterns pin
  /// **English** in their own `# OUTPUT INSTRUCTIONS`, so Vietnamese input
  /// needs `auto` to come back in Vietnamese.
  Future<PatternRunResult> run(
    String name, {
    String input = '',
    String? strategy,
    Map<String, String> variables = const {},
    String? language,
    String? profile,
    int? maxTokens,
    bool dryRun = false,
  }) async {
    final m = await _api.post(
      '/api/patterns/run',
      body: {
        'name': name,
        'input': input,
        if (strategy != null && strategy.isNotEmpty) 'strategy': strategy,
        if (variables.isNotEmpty) 'variables': variables,
        if (language != null && language.isNotEmpty) 'language': language,
        if (profile != null && profile.isNotEmpty) 'profile': profile,
        'maxTokens': ?maxTokens,
        if (dryRun) 'dryRun': true,
      },
      timeout: const Duration(minutes: 5),
    );
    return PatternRunResult.fromJson(
      m is Map ? m.cast<String, dynamic>() : const {},
    );
  }

  /// Create or overwrite a pattern in a writable source (defaults to `user`).
  Future<PatternFiles> save({
    required String name,
    required String system,
    String? user,
    String? source,
    bool overwrite = false,
  }) async {
    final m = await _api.post(
      '/api/patterns',
      body: {
        'name': name,
        'system': system,
        if (user != null && user.isNotEmpty) 'user': user,
        if (source != null && source.isNotEmpty) 'source': source,
        if (overwrite) 'overwrite': true,
      },
    );
    final obj = m is Map ? m.cast<String, dynamic>() : const {};
    return PatternFiles.fromJson(
      (obj['pattern'] as Map?)?.cast<String, dynamic>() ?? const {},
    );
  }

  /// Delete. Without [source] the daemon deletes the one the name actually
  /// resolves to, not "whatever the user source happens to hold".
  Future<void> delete(String name, {String? source}) => _api.delete(
        ApiClient.withQuery(
          '/api/patterns/${Uri.encodeComponent(name)}',
          {'source': source},
        ),
      );

  // ===== catalog =====

  /// One-tap offers: the bundled library (installs offline) plus git presets.
  Future<List<PatternCatalogEntry>> catalog() async {
    final m = await _api.getObject('/api/patterns/catalog');
    return _maps(m['catalog']).map(PatternCatalogEntry.fromJson).toList();
  }

  /// Install a catalog offer. The bundled one needs no network; a git one
  /// clones, so this gets the long timeout.
  Future<Map<String, dynamic>> installCatalog(String id) async {
    final m = await _api.post(
      '/api/patterns/catalog/${Uri.encodeComponent(id)}/install',
      timeout: const Duration(minutes: 5),
    );
    return m is Map ? m.cast<String, dynamic>() : <String, dynamic>{};
  }

  // ===== sources =====

  Future<List<PatternSource>> sources() async {
    final m = await _api.getObject('/api/patterns/sources');
    return _maps(m['sources']).map(PatternSource.fromJson).toList();
  }

  Future<Map<String, dynamic>> addSource({
    required String url,
    String? id,
    String? name,
    String gitRef = 'main',
    String subdir = '',
    String? strategiesSubdir,
    bool sync = true,
  }) async {
    final m = await _api.post(
      '/api/patterns/sources',
      body: {
        'url': url,
        if (id != null && id.isNotEmpty) 'id': id,
        if (name != null && name.isNotEmpty) 'name': name,
        'gitRef': gitRef,
        'subdir': subdir,
        if (strategiesSubdir != null && strategiesSubdir.isNotEmpty)
          'strategiesSubdir': strategiesSubdir,
        'sync': sync,
      },
      timeout: const Duration(minutes: 5),
    );
    return m is Map ? m.cast<String, dynamic>() : <String, dynamic>{};
  }

  /// Re-clone a source. Checkouts are shallow, but Fabric is still ~250
  /// patterns over the wire, so the same long timeout applies.
  Future<Map<String, dynamic>> syncSource(String id) async {
    final m = await _api.post(
      '/api/patterns/sources/${Uri.encodeComponent(id)}/sync',
      timeout: const Duration(minutes: 5),
    );
    final obj = m is Map ? m.cast<String, dynamic>() : const {};
    return (obj['sync'] as Map?)?.cast<String, dynamic>() ?? <String, dynamic>{};
  }

  /// Flip enabled/disabled. Returns the new state.
  Future<bool> toggleSource(String id) async {
    final m = await _api
        .post('/api/patterns/sources/${Uri.encodeComponent(id)}/toggle');
    return m is Map && m['enabled'] == true;
  }

  Future<void> deleteSource(String id) =>
      _api.delete('/api/patterns/sources/${Uri.encodeComponent(id)}');
}
