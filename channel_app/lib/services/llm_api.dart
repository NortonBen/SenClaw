import 'api_client.dart';

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

  Future<LlmConfigList> list() async {
    final r = await _api.getObject('/api/llm-config');
    final configs = ((r['configs'] as List?) ?? const [])
        .whereType<Map>()
        .map((m) => LlmOption('${m['id']}', '${m['label'] ?? m['id']}'))
        .toList();
    return LlmConfigList(configs, r['activeId'] as String?);
  }

  /// Set the global active (main) model. `type` defaults to "main" server-side.
  Future<void> setActive(String id) =>
      _api.post('/api/llm-config/active', body: {'id': id});
}
