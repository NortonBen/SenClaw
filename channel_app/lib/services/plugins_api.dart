import 'dart:convert';
import '../models/plugin_models.dart';
import 'api_client.dart';

/// Typed wrapper over the plugin-management endpoints (Skills / Subagents /
/// MCP / Marketplace / Hooks), tunnelled through the relay.
class PluginsApi {
  final _api = ApiClient();

  // ── Skills ─────────────────────────────────────────────────────────────
  Future<List<LocalSkill>> listSkills() async {
    final obj = await _api.getObject('/api/skills');
    return ((obj['skills'] as List?) ?? const [])
        .map((e) => LocalSkill.fromJson(e as Map<String, dynamic>))
        .toList();
  }

  Future<List<RemoteSkill>> searchSkills(String q) async {
    final obj = await _api
        .getObject(ApiClient.withQuery('/api/skills/remote-search', {'q': q}));
    return ((obj['results'] as List?) ?? const [])
        .map((e) => RemoteSkill.fromJson(e as Map<String, dynamic>))
        .toList();
  }

  Future<void> installSkill(String slug) =>
      _api.post('/api/skills/install', body: {'slug': slug});

  Future<void> deleteSkill(String name) => _api.delete('/api/skills/$name');

  Future<void> toggleSkill(String name, bool enable) =>
      _api.post('/api/skills/$name/${enable ? 'enable' : 'disable'}');

  // ── Subagents ──────────────────────────────────────────────────────────
  Future<List<Subagent>> listSubagents() async {
    final obj = await _api.getObject('/api/subagents');
    return ((obj['subagents'] as List?) ?? const [])
        .map((e) => Subagent.fromJson(e as Map<String, dynamic>))
        .toList();
  }

  Future<void> createSubagent(String name, String content) =>
      _api.post('/api/subagents/create', body: {'name': name, 'content': content});

  Future<void> toggleSubagent(String name, bool enable) =>
      _api.post('/api/subagents/$name/${enable ? 'enable' : 'disable'}');

  // ── Plugins (packages) ─────────────────────────────────────────────────
  Future<List<Plugin>> listPlugins() async {
    final obj = await _api.getObject('/api/plugins');
    return ((obj['plugins'] as List?) ?? const [])
        .map((e) => Plugin.fromJson(e as Map<String, dynamic>))
        .toList();
  }

  Future<List<RemoteSkill>> searchPlugins(String q) async {
    final obj = await _api
        .getObject(ApiClient.withQuery('/api/plugins/remote-search', {'q': q}));
    return ((obj['results'] as List?) ?? const [])
        .map((e) => RemoteSkill.fromJson(e as Map<String, dynamic>))
        .toList();
  }

  Future<void> installPlugin(String slug) =>
      _api.post('/api/plugins/install', body: {'slug': slug});

  Future<void> deletePlugin(String slug) => _api.delete('/api/plugins/$slug');

  Future<void> togglePlugin(String slug, bool enable) =>
      _api.post('/api/plugins/$slug/${enable ? 'enable' : 'disable'}');

  // ── MCP ────────────────────────────────────────────────────────────────
  Future<List<McpServer>> listMcp() async {
    final obj = await _api.getObject('/api/mcp-servers');
    return ((obj['servers'] as List?) ?? const [])
        .map((e) => McpServer.fromJson(e as Map<String, dynamic>))
        .toList();
  }

  Future<void> addMcp(Map<String, dynamic> body) =>
      _api.post('/api/mcp-servers', body: body);

  Future<void> deleteMcp(String name, {String scope = 'user'}) =>
      _api.delete(ApiClient.withQuery('/api/mcp-servers/$name', {'scope': scope}));

  Future<void> connectMcp(String name) =>
      _api.post('/api/mcp-servers/$name/connect');

  Future<void> disconnectMcp(String name) =>
      _api.post('/api/mcp-servers/$name/disconnect');

  Future<void> setMcpEnabled(String name, bool enabled, {String scope = 'user'}) =>
      _api.post('/api/mcp-servers/$name/enabled',
          body: {'enabled': enabled, 'scope': scope});

  // ── Marketplace ────────────────────────────────────────────────────────
  Future<List<MarketplaceSource>> listMarketplace() async {
    final obj = await _api.getObject('/api/marketplace/sources');
    return ((obj['sources'] as List?) ?? const [])
        .map((e) => MarketplaceSource.fromJson(e as Map<String, dynamic>))
        .toList();
  }

  Future<void> addMarketplace(Map<String, dynamic> body) =>
      _api.post('/api/marketplace/sources', body: body);

  Future<void> deleteMarketplace(String id) =>
      _api.delete('/api/marketplace/sources/$id');

  Future<void> syncMarketplace(String id) =>
      _api.post('/api/marketplace/sources/$id/sync');

  // ── Hooks (raw JSON) ───────────────────────────────────────────────────
  Future<String> getHooksJson() async {
    final obj = await _api.getObject('/api/hooks');
    final hooks = obj['hooks'] ?? obj;
    return const JsonEncoder.withIndent('  ').convert(hooks);
  }

  Future<void> saveHooksJson(String json) async {
    final parsed = jsonDecode(json);
    await _api.put('/api/hooks', body: {'hooks': parsed});
  }
}
