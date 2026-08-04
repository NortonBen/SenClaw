/// Models mirroring the web Cognitive (knowledge graph) feature
/// (`/api/cognitive/*`). JSON is snake_case; timestamps are unix seconds.
library;

class CogStats {
  final int edges;
  final int nodesTotal;
  final List<MapEntry<String, int>> nodesByKind;

  CogStats({
    required this.edges,
    required this.nodesTotal,
    this.nodesByKind = const [],
  });

  factory CogStats.fromJson(Map<String, dynamic> j) => CogStats(
        edges: (j['edges'] as num?)?.toInt() ?? 0,
        nodesTotal: (j['nodes_total'] as num?)?.toInt() ?? 0,
        nodesByKind: ((j['nodes_by_kind'] as List?) ?? const [])
            .map((e) {
              final pair = e as List;
              return MapEntry(
                  pair[0].toString(), (pair[1] as num).toInt());
            })
            .toList(),
      );
}

/// One knowledge space (custom NodeSet scope), e.g. an AI-Office staff
/// member's private space. From `GET /api/cognitive/spaces` (camelCase).
class CogSpace {
  final String scopeKind;
  final String scopeId;
  final String tag;
  final int nodes;

  const CogSpace({
    this.scopeKind = '',
    this.scopeId = '',
    this.tag = '',
    this.nodes = 0,
  });

  factory CogSpace.fromJson(Map<String, dynamic> j) => CogSpace(
        scopeKind: '${j['scopeKind'] ?? ''}',
        scopeId: '${j['scopeId'] ?? ''}',
        tag: '${j['tag'] ?? ''}',
        nodes: (j['nodes'] as num?)?.toInt() ?? 0,
      );

  /// Display label, e.g. `ai-office:nghien-cuu`.
  String get label => scopeId.isEmpty ? tag : scopeId;
}

class CogNode {
  final String id;
  final String kind;
  final String name;
  final String summary;
  final double salience;
  final int mentionCount;
  final int createdAt;
  final int lastSeenAt;

  CogNode({
    required this.id,
    required this.kind,
    required this.name,
    required this.summary,
    this.salience = 0,
    this.mentionCount = 0,
    this.createdAt = 0,
    this.lastSeenAt = 0,
  });

  factory CogNode.fromJson(Map<String, dynamic> j) => CogNode(
        id: j['id'] as String? ?? '',
        kind: j['kind'] as String? ?? 'custom',
        name: j['name'] as String? ?? '',
        summary: j['summary'] as String? ?? '',
        salience: (j['salience'] as num?)?.toDouble() ?? 0,
        mentionCount: (j['mention_count'] as num?)?.toInt() ?? 0,
        createdAt: (j['created_at'] as num?)?.toInt() ?? 0,
        lastSeenAt: (j['last_seen_at'] as num?)?.toInt() ?? 0,
      );
}

class CogEdge {
  final String src;
  final String dst;
  final String predicate;
  final double strength;
  final int tier;
  final bool inferred;

  CogEdge({
    required this.src,
    required this.dst,
    required this.predicate,
    this.strength = 0,
    this.tier = 0,
    this.inferred = false,
  });

  factory CogEdge.fromJson(Map<String, dynamic> j) => CogEdge(
        src: j['src'] as String? ?? '',
        dst: j['dst'] as String? ?? '',
        predicate: j['predicate'] as String? ?? '',
        strength: (j['strength'] as num?)?.toDouble() ?? 0,
        tier: (j['tier'] as num?)?.toInt() ?? 0,
        inferred: j['inferred'] as bool? ?? false,
      );
}

class CogHit {
  final CogNode node;
  final double score;
  final int pathLen;
  CogHit({required this.node, this.score = 0, this.pathLen = 0});
  factory CogHit.fromJson(Map<String, dynamic> j) => CogHit(
        node: CogNode.fromJson((j['node'] as Map).cast<String, dynamic>()),
        score: (j['score'] as num?)?.toDouble() ?? 0,
        pathLen: (j['path_len'] as num?)?.toInt() ?? 0,
      );
}

class CogSubgraph {
  final List<CogNode> nodes;
  final List<CogEdge> edges;
  final bool truncated;
  CogSubgraph({this.nodes = const [], this.edges = const [], this.truncated = false});
  factory CogSubgraph.fromJson(Map<String, dynamic> j) => CogSubgraph(
        nodes: ((j['nodes'] as List?) ?? const [])
            .map((e) => CogNode.fromJson(e as Map<String, dynamic>))
            .toList(),
        edges: ((j['edges'] as List?) ?? const [])
            .map((e) => CogEdge.fromJson(e as Map<String, dynamic>))
            .toList(),
        truncated: j['truncated'] as bool? ?? false,
      );
}

class CogRecallSource {
  final int index;
  final String id;
  final String kind;
  final String name;
  final String summary;
  final double score;
  CogRecallSource({
    required this.index,
    required this.id,
    required this.kind,
    required this.name,
    required this.summary,
    this.score = 0,
  });
  factory CogRecallSource.fromJson(Map<String, dynamic> j) => CogRecallSource(
        index: (j['index'] as num?)?.toInt() ?? 0,
        id: j['id'] as String? ?? '',
        kind: j['kind'] as String? ?? '',
        name: j['name'] as String? ?? '',
        summary: j['summary'] as String? ?? '',
        score: (j['score'] as num?)?.toDouble() ?? 0,
      );
}

class CogRecall {
  final String answer;
  final bool grounded;
  final String? note;
  final List<CogRecallSource> sources;
  CogRecall({
    required this.answer,
    this.grounded = false,
    this.note,
    this.sources = const [],
  });
  factory CogRecall.fromJson(Map<String, dynamic> j) => CogRecall(
        answer: j['answer'] as String? ?? '',
        grounded: j['grounded'] as bool? ?? false,
        note: j['note'] as String?,
        sources: ((j['sources'] as List?) ?? const [])
            .map((e) => CogRecallSource.fromJson(e as Map<String, dynamic>))
            .toList(),
      );
}

class CogAddResult {
  final String? filename;
  final int chunksAdded;
  final int entitiesAdded;
  final int edgesAdded;
  final bool llmSkipped;
  CogAddResult({
    this.filename,
    this.chunksAdded = 0,
    this.entitiesAdded = 0,
    this.edgesAdded = 0,
    this.llmSkipped = false,
  });
  factory CogAddResult.fromJson(Map<String, dynamic> j) => CogAddResult(
        filename: j['filename'] as String?,
        chunksAdded: (j['chunks_added'] as num?)?.toInt() ?? 0,
        entitiesAdded: (j['entities_added'] as num?)?.toInt() ?? 0,
        edgesAdded: (j['edges_added'] as num?)?.toInt() ?? 0,
        llmSkipped: j['llm_skipped'] as bool? ?? false,
      );
}
