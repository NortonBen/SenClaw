import '../models/wiki_models.dart';
import 'api_client.dart';
import 'local_cache.dart';

/// Typed wrapper over `/api/wiki/*`, tunnelled through the relay.
/// Mirrors web/src/hooks/useWiki.ts. File upload (multipart) is intentionally
/// omitted — the relay tunnel only carries JSON bodies.
class WikiApi {
  final _api = ApiClient();

  Future<List<WikiDirNode>> tree() async {
    final obj = await _api.getObject('/api/wiki/tree');
    final maps = jsonMaps(obj['tree']);
    LocalCache().putDomainList('wiki_tree', maps);
    return maps.map(WikiDirNode.fromJson).toList();
  }

  Future<List<WikiDirNode>> treeCached() async =>
      (await LocalCache().getDomainList('wiki_tree'))
          .map(WikiDirNode.fromJson)
          .toList();

  Future<WikiDoc> file(String path) async {
    final obj = await _api
        .getObject(ApiClient.withQuery('/api/wiki/file', {'path': path}));
    return WikiDoc.fromJson(obj);
  }

  Future<String> writeFile({
    required String path,
    required String content,
    String? source,
    List<String>? tags,
    String? commitMsg,
  }) async {
    final r = await _api.put('/api/wiki/file', body: {
      'path': path,
      'content': content,
      'source': ?source,
      'tags': ?tags,
      'commitMsg': ?commitMsg,
    });
    if (r is Map && r['updated'] != null) return r['updated'].toString();
    return '';
  }

  Future<List<WikiSearchResult>> search(
    String q, {
    String? tags,
    int limit = 20,
  }) async {
    final obj = await _api.getObject(ApiClient.withQuery('/api/wiki/search', {
      'q': q,
      'tags': tags,
      'limit': limit,
    }));
    final list = (obj['results'] as List?) ?? const [];
    return list
        .map((e) => WikiSearchResult.fromJson(e as Map<String, dynamic>))
        .toList();
  }

  Future<WikiStats> stats() async {
    final obj = await _api.getObject('/api/wiki/stats');
    return WikiStats.fromJson(obj);
  }

  Future<List<WikiCommit>> history(String path, {int limit = 20}) async {
    final obj = await _api.getObject(ApiClient.withQuery('/api/wiki/history', {
      'path': path,
      'limit': limit,
    }));
    final list = (obj['commits'] as List?) ?? const [];
    return list
        .map((e) => WikiCommit.fromJson(e as Map<String, dynamic>))
        .toList();
  }

  Future<List<WikiTag>> tags() async {
    final obj = await _api.getObject('/api/wiki/tags');
    final list = (obj['tags'] as List?) ?? const [];
    return list.map((e) => WikiTag.fromJson(e as Map<String, dynamic>)).toList();
  }

  Future<void> mkdir(String path) =>
      _api.post('/api/wiki/mkdir', body: {'path': path});

  Future<void> deleteDir(String path) =>
      _api.delete(ApiClient.withQuery('/api/wiki/dir', {'path': path}));
}
