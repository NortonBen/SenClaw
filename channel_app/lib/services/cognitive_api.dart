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
  }) async {
    final obj = await _api.post('/api/cognitive/search', body: {
      'query': query,
      'mode': mode,
      'limit': limit,
      'rerank': rerank,
    });
    final list = ((obj is Map ? obj['hits'] : null) as List?) ?? const [];
    return list.map((e) => CogHit.fromJson(e as Map<String, dynamic>)).toList();
  }

  Future<CogRecall> recall(String query, {String mode = 'graph', int limit = 10}) async {
    final obj = await _api.post('/api/cognitive/recall', body: {
      'query': query,
      'mode': mode,
      'limit': limit,
    });
    return CogRecall.fromJson((obj as Map).cast<String, dynamic>());
  }

  Future<CogAddResult> add(String text, {String? source, List<String> tags = const []}) async {
    final obj = await _api.post('/api/cognitive/add', body: {
      'text': text,
      'source': ?source,
      'tags': tags,
    });
    return CogAddResult.fromJson((obj as Map).cast<String, dynamic>());
  }

  Future<void> reExtract(String id) =>
      _api.post('/api/cognitive/node/$id/re-extract');

  Future<Map<String, dynamic>> maintenance() async {
    final obj = await _api.post('/api/cognitive/maintenance');
    return obj is Map ? obj.cast<String, dynamic>() : {};
  }

  Future<void> deleteNode(String id) => _api.delete('/api/cognitive/node/$id');
}
