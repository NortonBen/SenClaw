import '../models/cognitive_models.dart';
import 'api_client.dart';

/// Typed wrapper over `/api/cognitive/*` (knowledge graph), tunnelled through
/// the relay. File upload (multipart) is omitted — the tunnel carries JSON only;
/// use [add] with plain text instead.
class CognitiveApi {
  final _api = ApiClient();

  Future<CogStats> stats() async {
    final obj = await _api.getObject('/api/cognitive/stats');
    return CogStats.fromJson(obj);
  }

  /// Registry of knowledge spaces. Only named custom scopes are switchable in
  /// the UI (global/group tags are filtered out, matching the desktop app).
  Future<List<CogSpace>> spaces() async {
    final obj = await _api.getObject('/api/cognitive/spaces');
    return ((obj['spaces'] as List?) ?? const [])
        .whereType<Map>()
        .map((e) => CogSpace.fromJson(e.cast<String, dynamic>()))
        .where((s) => s.scopeKind == 'custom' && s.scopeId.isNotEmpty)
        .toList();
  }

  Future<({int total, List<CogNode> nodes})> nodes({
    String? kind,
    int limit = 50,
    int offset = 0,
  }) async {
    final obj = await _api.getObject(ApiClient.withQuery('/api/cognitive/nodes', {
      'kind': kind,
      'limit': limit,
      'offset': offset,
    }));
    final list = ((obj['nodes'] as List?) ?? const [])
        .map((e) => CogNode.fromJson(e as Map<String, dynamic>))
        .toList();
    return (total: (obj['total'] as num?)?.toInt() ?? list.length, nodes: list);
  }

  Future<({CogNode node, List<CogEdge> edges})> node(String id) async {
    final obj = await _api.getObject('/api/cognitive/node/$id');
    return (
      node: CogNode.fromJson((obj['node'] as Map).cast<String, dynamic>()),
      edges: ((obj['edges'] as List?) ?? const [])
          .map((e) => CogEdge.fromJson(e as Map<String, dynamic>))
          .toList(),
    );
  }

  Future<CogSubgraph> sample({int seedCount = 5, int hops = 2, int limit = 150}) async {
    final obj = await _api.getObject(ApiClient.withQuery('/api/cognitive/sample', {
      'seed_count': seedCount,
      'hops': hops,
      'limit': limit,
    }));
    return CogSubgraph.fromJson(obj);
  }

  Future<CogSubgraph> subgraph(String seed, {int hops = 2, int limit = 100}) async {
    final obj = await _api.getObject(ApiClient.withQuery('/api/cognitive/subgraph', {
      'seed': seed,
      'hops': hops,
      'limit': limit,
    }));
    return CogSubgraph.fromJson(obj);
  }

  Future<List<CogHit>> search(
    String query, {
    String mode = 'graph',
    int limit = 20,
    bool rerank = false,
    String? space,
  }) async {
    final obj = await _api.post('/api/cognitive/search', body: {
      'query': query,
      'mode': mode,
      'limit': limit,
      'rerank': rerank,
      'space': ?space,
    });
    final list = ((obj is Map ? obj['hits'] : null) as List?) ?? const [];
    return list.map((e) => CogHit.fromJson(e as Map<String, dynamic>)).toList();
  }

  Future<CogRecall> recall(String query,
      {String mode = 'graph', int limit = 10, String? space}) async {
    final obj = await _api.post('/api/cognitive/recall', body: {
      'query': query,
      'mode': mode,
      'limit': limit,
      'space': ?space,
    });
    return CogRecall.fromJson((obj as Map).cast<String, dynamic>());
  }

  Future<CogAddResult> add(String text,
      {String? source, List<String> tags = const [], String? space}) async {
    final obj = await _api.post('/api/cognitive/add', body: {
      'text': text,
      'source': ?source,
      'tags': tags,
      'space': ?space,
    });
    return CogAddResult.fromJson((obj as Map).cast<String, dynamic>());
  }

  Future<void> reExtract(String id) =>
      _api.post('/api/cognitive/node/$id/re-extract');

  /// Bulk rescue: queues every chunk whose triplet extraction never ran OR
  /// whose edges have since decayed away through cognify again. Returns
  /// `{queued, reset}`; extraction happens in the background.
  Future<Map<String, dynamic>> reExtractPending() async {
    final obj = await _api.post('/api/cognitive/re-extract-pending');
    return obj is Map ? obj.cast<String, dynamic>() : {};
  }

  Future<Map<String, dynamic>> maintenance() async {
    final obj = await _api.post('/api/cognitive/maintenance');
    return obj is Map ? obj.cast<String, dynamic>() : {};
  }

  Future<void> deleteNode(String id) => _api.delete('/api/cognitive/node/$id');
}
