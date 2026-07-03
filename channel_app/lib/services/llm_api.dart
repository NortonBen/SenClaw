import 'api_client.dart';
import 'local_cache.dart';

class LlmOption {
  final String id;
  final String label;
  const LlmOption(this.id, this.label);
}

class LlmConfigList {
  final List<LlmOption> configs;
  final String? activeId;
  const LlmConfigList(this.configs, this.activeId);
}

/// Read LLM configs + set the active model, over the relay `/api/*` tunnel.
/// Note: setting the active model is GLOBAL (per the daemon), not per-session.
class LlmApi {
  final _api = ApiClient();

  static LlmConfigList _parse(Map<String, dynamic> r) {
    final configs = ((r['configs'] as List?) ?? const [])
        .whereType<Map>()
        .map((m) => LlmOption('${m['id']}', '${m['label'] ?? m['id']}'))
        .toList();
    return LlmConfigList(configs, r['activeId'] as String?);
  }

  Future<LlmConfigList> list() async {
    final r = await _api.getObject('/api/llm-config');
    // Single-row domain: the whole response object is the cached item.
    LocalCache().putDomainList('llm_config', [r]);
    return _parse(r);
  }

  Future<LlmConfigList> listCached() async {
    final rows = await LocalCache().getDomainList('llm_config');
    if (rows.isEmpty) return const LlmConfigList([], null);
    return _parse(rows.first);
  }

  /// Set the global active (main) model. `type` defaults to "main" server-side.
  Future<void> setActive(String id) =>
      _api.post('/api/llm-config/active', body: {'id': id});
}
