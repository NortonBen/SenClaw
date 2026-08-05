import 'dart:math' as math;
import 'package:flutter/material.dart';
import 'package:flutter_graph_view/flutter_graph_view.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/transport/connection.dart';
import '../../theme/tokens.dart';
import 'cognitive_screen.dart' show cogSpaceProvider;

// ── Models ────────────────────────────────────────────────────────────────
class GraphNode {
  final String id;
  final String kind;
  final String name;
  final String summary;
  const GraphNode(this.id, this.kind, this.name, this.summary);
  factory GraphNode.fromJson(Map<String, dynamic> j) => GraphNode(
        '${j['id'] ?? ''}',
        '${j['kind'] ?? ''}',
        '${j['name'] ?? j['label'] ?? ''}',
        '${j['summary'] ?? ''}',
      );
}

class GraphEdge {
  final String src;
  final String dst;
  final String predicate;
  final double strength;
  final int tier;
  final bool inferred;
  const GraphEdge(
      this.src, this.dst, this.predicate, this.strength, this.tier, this.inferred);
  factory GraphEdge.fromJson(Map<String, dynamic> j) => GraphEdge(
        '${j['src'] ?? ''}',
        '${j['dst'] ?? ''}',
        '${j['predicate'] ?? ''}',
        (j['strength'] as num?)?.toDouble() ?? 1.0,
        (j['tier'] as num?)?.toInt() ?? 0,
        j['inferred'] == true,
      );
}

class Subgraph {
  final List<GraphNode> nodes;
  final List<GraphEdge> edges;
  final bool truncated;
  const Subgraph(this.nodes, this.edges, this.truncated);
}

/// BFS subgraph around a seed node (GET /api/cognitive/subgraph).
final cogSubgraphProvider =
    FutureProvider.family<Subgraph, String>((ref, seed) async {
  final r = await ref.read(apiClientProvider).get('/api/cognitive/subgraph',
      query: {'seed': seed, 'hops': '2', 'limit': '60'});
  final m = r is Map ? r : const {};
  return _parseSubgraph(m);
});

/// Full-graph provider — GET /api/cognitive/full-graph with connected_only
/// and include_chunks toggles. Parameterized by a record.
typedef FullGraphParams = ({bool connectedOnly, bool includeChunks});

final cogFullGraphProvider =
    FutureProvider.family<Subgraph, FullGraphParams>((ref, params) async {
  // The graph view honors the same knowledge-space switcher as the Data
  // list — watching here refetches automatically when the space changes.
  final space = ref.watch(cogSpaceProvider);
  final r = await ref.read(apiClientProvider).get('/api/cognitive/full-graph',
      query: {
        'node_limit': '2000',
        'edge_limit': '5000',
        'include_chunks': params.includeChunks ? 'true' : 'false',
        'connected_only': params.connectedOnly ? 'true' : 'false',
        if (space != null && space.isNotEmpty) 'space': space,
      });
  final m = r is Map ? r : const {};
  return _parseSubgraph(m);
});

Subgraph _parseSubgraph(Map m) {
  final nodes = ((m['nodes'] as List?) ?? const [])
      .whereType<Map>()
      .map((e) => GraphNode.fromJson(e.cast<String, dynamic>()))
      .toList();
  final edges = ((m['edges'] as List?) ?? const [])
      .whereType<Map>()
      .map((e) => GraphEdge.fromJson(e.cast<String, dynamic>()))
      .toList();
  return Subgraph(nodes, edges, m['truncated'] == true);
}

const Map<String, Color> _kindColors = {
  'entity': Color(0xFF5BBFE8),
  'chunk': Color(0xFF9CA3AF),
  'summary': Color(0xFF10B981),
  'custom': Color(0xFFF59E0B),
};
Color _kindColor(String k) => _kindColors[k] ?? const Color(0xFF9CA3AF);

// ── CogGraphView — seed-based subgraph (used in data tab) ────────────────
class CogGraphView extends ConsumerWidget {
  const CogGraphView(
      {super.key, required this.seedId, required this.onNodeTap});
  final String seedId;
  final void Function(GraphNode) onNodeTap;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final sub = ref.watch(cogSubgraphProvider(seedId));
    return sub.when(
      loading: () => const Center(child: CircularProgressIndicator()),
      error: (e, _) => Center(
          child: Text('Graph error: $e',
              style: const TextStyle(color: AppTokens.danger))),
      data: (g) => g.nodes.length < 2
          ? Center(
              child: Text('Not enough connected nodes to graph',
                  style: TextStyle(color: c.textMuted, fontSize: 12)))
          : _GraphCanvas(
              graph: g,
              focusId: seedId,
              onNodeTap: onNodeTap,
              searchText: '',
            ),
    );
  }
}

// ── CogGraphExplorer — full graph with controls ──────────────────────────
class CogGraphExplorer extends ConsumerStatefulWidget {
  const CogGraphExplorer({super.key, required this.onNodeTap});
  final void Function(GraphNode) onNodeTap;
  @override
  ConsumerState<CogGraphExplorer> createState() => _CogGraphExplorerState();
}

class _CogGraphExplorerState extends ConsumerState<CogGraphExplorer> {
  bool _connectedOnly = true;
  bool _includeChunks = false;
  String _search = '';
  GraphNode? _focusNode;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final params = (connectedOnly: _connectedOnly, includeChunks: _includeChunks);
    final sub = ref.watch(cogFullGraphProvider(params));

    return sub.when(
      loading: () => const Center(child: CircularProgressIndicator()),
      error: (e, _) => Center(
          child: Text('Graph error: $e',
              style: const TextStyle(color: AppTokens.danger))),
      data: (g) {
        final kindCounts = <String, int>{};
        for (final n in g.nodes) {
          kindCounts[n.kind] = (kindCounts[n.kind] ?? 0) + 1;
        }

        final focusEdges = _focusNode == null
            ? <GraphEdge>[]
            : g.edges
                .where((e) =>
                    e.src == _focusNode!.id || e.dst == _focusNode!.id)
                .toList();

        return Column(
          children: [
            // ── Header bar ──
            Container(
              padding:
                  const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
              decoration: BoxDecoration(
                  border: Border(
                      bottom: BorderSide(color: c.border, width: 0.5))),
              child: Row(
                children: [
                  Icon(Icons.hub_outlined, size: 16, color: c.accent),
                  const SizedBox(width: 6),
                  Text('Knowledge',
                      style: TextStyle(
                          fontWeight: FontWeight.w600,
                          fontSize: 13,
                          color: c.textPrimary)),
                  const SizedBox(width: 12),
                  _chip('${g.nodes.length} nodes', AppTokens.brand),
                  const SizedBox(width: 6),
                  _chip('${g.edges.length} edges', AppTokens.cyan),
                  const SizedBox(width: 6),
                  for (final kv in kindCounts.entries) ...[
                    Text('${kv.value} ${kv.key}',
                        style: TextStyle(
                            fontSize: 10,
                            color: c.textMuted)),
                    const SizedBox(width: 8),
                  ],
                  const Spacer(),
                  // Search
                  SizedBox(
                    width: 160,
                    height: 28,
                    child: TextField(
                      style: TextStyle(fontSize: 12, color: c.textPrimary),
                      decoration: InputDecoration(
                        hintText: 'Search nodes…',
                        hintStyle: TextStyle(
                            fontSize: 11, color: c.textMuted),
                        prefixIcon: Icon(Icons.search,
                            size: 14, color: c.textMuted),
                        prefixIconConstraints:
                            const BoxConstraints(minWidth: 28),
                        isDense: true,
                        contentPadding: const EdgeInsets.symmetric(
                            horizontal: 8, vertical: 4),
                        border: OutlineInputBorder(
                            borderRadius: BorderRadius.circular(6),
                            borderSide: BorderSide(color: c.border)),
                        enabledBorder: OutlineInputBorder(
                            borderRadius: BorderRadius.circular(6),
                            borderSide: BorderSide(color: c.border)),
                      ),
                      onChanged: (v) => setState(() => _search = v),
                    ),
                  ),
                  const SizedBox(width: 12),
                  // Connected toggle
                  _toggle('Connected', _connectedOnly, (v) {
                    setState(() => _connectedOnly = v);
                  }, c),
                  const SizedBox(width: 10),
                  // Chunks toggle
                  _toggle('Chunks', _includeChunks, (v) {
                    setState(() => _includeChunks = v);
                  }, c),
                  const SizedBox(width: 8),
                  IconButton(
                    tooltip: 'Refresh',
                    icon: const Icon(Icons.refresh, size: 16),
                    onPressed: () =>
                        ref.invalidate(cogFullGraphProvider(params)),
                    visualDensity: VisualDensity.compact,
                    iconSize: 16,
                  ),
                ],
              ),
            ),

            // ── Graph + detail panel ──
            Expanded(
              child: g.nodes.length < 2
                  ? Center(
                      child: Text('No graph data yet',
                          style:
                              TextStyle(color: c.textMuted, fontSize: 12)))
                  : Row(
                      children: [
                        Expanded(
                          child: _GraphCanvas(
                            graph: g,
                            focusId: _focusNode?.id ?? '',
                            onNodeTap: (n) {
                              setState(() {
                                if (_focusNode?.id == n.id) {
                                  _focusNode = null;
                                } else {
                                  _focusNode = n;
                                }
                              });
                            },
                            searchText: _search,
                          ),
                        ),
                        if (_focusNode != null)
                          _DetailPanel(
                            node: _focusNode!,
                            edges: focusEdges,
                            allNodes: g.nodes,
                            onClose: () =>
                                setState(() => _focusNode = null),
                            onNodeTap: (n) =>
                                setState(() => _focusNode = n),
                            onOpenData: widget.onNodeTap,
                          ),
                      ],
                    ),
            ),

            // ── Footer ──
            Container(
              padding:
                  const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
              child: Row(children: [
                Text('Drag node = move · Scroll = zoom · Tap node = focus',
                    style:
                        TextStyle(fontSize: 10, color: c.textMuted)),
                if (g.truncated) ...[
                  const Spacer(),
                  Text('Graph truncated',
                      style: TextStyle(
                          fontSize: 10, color: AppTokens.warning)),
                ],
              ]),
            ),
          ],
        );
      },
    );
  }

  Widget _chip(String text, Color color) => Container(
        padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 1),
        decoration: BoxDecoration(
            color: color.withValues(alpha: 0.12),
            borderRadius: BorderRadius.circular(4)),
        child: Text(text,
            style: TextStyle(fontSize: 10, color: color, fontWeight: FontWeight.w500)),
      );

  Widget _toggle(
      String label, bool value, ValueChanged<bool> onChanged, dynamic c) {
    return Row(mainAxisSize: MainAxisSize.min, children: [
      Text(label,
          style: TextStyle(fontSize: 10, color: c.textMuted)),
      const SizedBox(width: 4),
      SizedBox(
        height: 20,
        child: Switch(
          value: value,
          onChanged: onChanged,
          materialTapTargetSize: MaterialTapTargetSize.shrinkWrap,
        ),
      ),
    ]);
  }
}

// ── Detail panel (right side) ────────────────────────────────────────────
class _DetailPanel extends StatelessWidget {
  const _DetailPanel({
    required this.node,
    required this.edges,
    required this.allNodes,
    required this.onClose,
    required this.onNodeTap,
    required this.onOpenData,
  });
  final GraphNode node;
  final List<GraphEdge> edges;
  final List<GraphNode> allNodes;
  final VoidCallback onClose;
  final void Function(GraphNode) onNodeTap;
  final void Function(GraphNode) onOpenData;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final nodeMap = {for (final n in allNodes) n.id: n};

    return Container(
      width: 260,
      decoration: BoxDecoration(
          border: Border(left: BorderSide(color: c.border, width: 0.5))),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          // Header
          Padding(
            padding: const EdgeInsets.fromLTRB(12, 10, 4, 0),
            child: Row(
              children: [
                Container(
                  padding:
                      const EdgeInsets.symmetric(horizontal: 6, vertical: 1),
                  decoration: BoxDecoration(
                      color: _kindColor(node.kind).withValues(alpha: 0.15),
                      borderRadius: BorderRadius.circular(4)),
                  child: Text(node.kind,
                      style: TextStyle(
                          fontSize: 10,
                          color: _kindColor(node.kind),
                          fontWeight: FontWeight.w500)),
                ),
                const Spacer(),
                IconButton(
                  icon: const Icon(Icons.open_in_new, size: 14),
                  tooltip: 'Open in data view',
                  onPressed: () => onOpenData(node),
                  visualDensity: VisualDensity.compact,
                  iconSize: 14,
                ),
                IconButton(
                  icon: const Icon(Icons.close, size: 14),
                  onPressed: onClose,
                  visualDensity: VisualDensity.compact,
                  iconSize: 14,
                ),
              ],
            ),
          ),
          Padding(
            padding: const EdgeInsets.fromLTRB(12, 4, 12, 0),
            child: Text(
                node.name.isEmpty ? node.id.substring(0, 16) : node.name,
                style: TextStyle(
                    fontWeight: FontWeight.w600,
                    fontSize: 14,
                    color: c.textPrimary)),
          ),
          if (node.summary.isNotEmpty)
            Padding(
              padding: const EdgeInsets.fromLTRB(12, 4, 12, 0),
              child: Text(
                  node.summary.length > 200
                      ? '${node.summary.substring(0, 200)}…'
                      : node.summary,
                  style: TextStyle(
                      fontSize: 11,
                      color: c.textMuted,
                      height: 1.4)),
            ),
          Padding(
            padding: const EdgeInsets.fromLTRB(12, 8, 12, 4),
            child: Text('${edges.length} connections',
                style: TextStyle(fontSize: 10, color: c.textMuted)),
          ),
          const Divider(height: 1),
          // Edge list
          Expanded(
            child: edges.isEmpty
                ? Center(
                    child: Text('No connections',
                        style:
                            TextStyle(fontSize: 11, color: c.textMuted)))
                : ListView.builder(
                    padding: const EdgeInsets.symmetric(vertical: 4),
                    itemCount: edges.length > 30 ? 31 : edges.length,
                    itemBuilder: (_, i) {
                      if (i >= 30) {
                        return Padding(
                          padding: const EdgeInsets.all(8),
                          child: Text('+${edges.length - 30} more',
                              textAlign: TextAlign.center,
                              style: TextStyle(
                                  fontSize: 10, color: c.textMuted)),
                        );
                      }
                      final e = edges[i];
                      final isOut = e.src == node.id;
                      final otherId = isOut ? e.dst : e.src;
                      final other = nodeMap[otherId];
                      return InkWell(
                        onTap: other != null
                            ? () => onNodeTap(other)
                            : null,
                        child: Padding(
                          padding: const EdgeInsets.symmetric(
                              horizontal: 12, vertical: 4),
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              Row(children: [
                                Text(isOut ? '→ ' : '← ',
                                    style: TextStyle(
                                        fontSize: 10,
                                        color: c.textMuted)),
                                Expanded(
                                  child: Text(e.predicate,
                                      style: const TextStyle(
                                          fontSize: 11,
                                          color: Color(0xFF5BBFE8),
                                          fontWeight: FontWeight.w500)),
                                ),
                              ]),
                              Padding(
                                padding:
                                    const EdgeInsets.only(left: 14),
                                child: Text(
                                    other?.name ??
                                        otherId.substring(
                                            0, math.min(12, otherId.length)),
                                    style: TextStyle(
                                        fontSize: 10,
                                        color: c.textMuted)),
                              ),
                            ],
                          ),
                        ),
                      );
                    },
                  ),
          ),
        ],
      ),
    );
  }
}

// ── Graph canvas — powered by flutter_graph_view ────────────────────────
class _GraphCanvas extends StatelessWidget {
  const _GraphCanvas({
    required this.graph,
    required this.focusId,
    required this.onNodeTap,
    required this.searchText,
  });
  final Subgraph graph;
  final String focusId;
  final void Function(GraphNode) onNodeTap;
  final String searchText;

  @override
  Widget build(BuildContext context) {
    final nodeMap = {for (final n in graph.nodes) n.id: n};

    final vertexes = <Map<String, dynamic>>{};
    for (final n in graph.nodes) {
      vertexes.add({
        'id': n.id,
        'tag': n.kind,
        'tags': [n.kind],
      });
    }
    final edges = <Map<String, dynamic>>{};
    for (final e in graph.edges) {
      edges.add({
        'srcId': e.src,
        'dstId': e.dst,
        'edgeName': e.predicate,
        'ranking': (e.strength * 100).round(),
      });
    }

    final data = {'vertexes': vertexes, 'edges': edges};

    final opts = Options();
    opts.enableHit = true;
    opts.panelDelay = const Duration(milliseconds: 300);
    opts.showText = true;
    opts.textGetter = (v) {
      final n = nodeMap[v.id];
      if (n == null) return '${v.id}'.substring(0, math.min(12, '${v.id}'.length));
      final label = n.name.isEmpty ? n.kind : n.name;
      return label.length > 24 ? '${label.substring(0, 22)}…' : label;
    };
    opts.onVertexTapUp = (v, _) {
      final n = nodeMap[v.id];
      if (n != null) onNodeTap(n);
    };
    opts.graphStyle = GraphStyle()
      ..tagColor = {
        'entity': const Color(0xFF5BBFE8),
        'chunk': const Color(0xFF9CA3AF),
        'summary': const Color(0xFF10B981),
        'custom': const Color(0xFFF59E0B),
      }
      ..tagColorByIndex = [
        const Color(0xFF5BBFE8),
        const Color(0xFF9CA3AF),
        const Color(0xFF10B981),
        const Color(0xFFF59E0B),
        const Color(0xFF3B82F6),
        const Color(0xFF8B5CF6),
      ];
    opts.backgroundBuilder = (_) => Container(color: Colors.transparent);
    opts.vertexPanelBuilder = (v) {
      final n = nodeMap[v.id];
      if (n == null) return const SizedBox.shrink();
      return Container(
        width: 220,
        padding: const EdgeInsets.all(10),
        decoration: BoxDecoration(
          color: Colors.grey.shade900.withAlpha(230),
          borderRadius: BorderRadius.circular(8),
          border: Border(
            left: BorderSide(color: _kindColor(n.kind), width: 3),
          ),
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(
              n.name.isEmpty ? n.id.substring(0, 12) : n.name,
              style: const TextStyle(
                color: Colors.white, fontSize: 13, fontWeight: FontWeight.w600,
              ),
            ),
            const SizedBox(height: 2),
            Text(
              '${n.kind} · ${v.degree} connections',
              style: TextStyle(
                color: Colors.white.withValues(alpha: 0.6), fontSize: 10,
              ),
            ),
            if (n.summary.isNotEmpty) ...[
              const SizedBox(height: 4),
              Text(
                n.summary.length > 150
                    ? '${n.summary.substring(0, 150)}…'
                    : n.summary,
                style: TextStyle(
                  color: Colors.white.withValues(alpha: 0.8),
                  fontSize: 11, height: 1.4,
                ),
              ),
            ],
          ],
        ),
      );
    };
    opts.edgePanelBuilder = (e) {
      return Container(
        padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
        decoration: BoxDecoration(
          color: Colors.grey.shade900.withAlpha(220),
          borderRadius: BorderRadius.circular(6),
        ),
        child: Text(
          e.edgeName,
          style: const TextStyle(color: Colors.white, fontSize: 11),
        ),
      );
    };
    opts.edgeShape = EdgeLineShape();
    opts.vertexShape = VertexCircleShape();

    // flutter_graph_view paints the graph via a `CustomPaint(size:
    // Size.infinite)` inside a `Positioned.fill` in a `Stack`. Because the
    // fill fits the Stack exactly, `RenderStack` sees no overflow and installs
    // no clip, so nodes the force layout pushes off-canvas paint OUTSIDE the
    // widget bounds (spilling into the sidebar). ClipRect is the Flutter analog
    // of the web `overflow: hidden` fix — it clips the render to our bounds.
    return ClipRect(
      child: FlutterGraphWidget(
        data: data,
        algorithm: ForceDirected(
          decorators: [
            CoulombDecorator(),
            HookeDecorator(),
            CoulombCenterDecorator(),
            HookeCenterDecorator(),
            HookeBorderDecorator(),
            ForceDecorator(),
            ForceMotionDecorator(),
            TimeCounterDecorator(),
          ],
        ),
        convertor: MapConvertor(),
        options: opts,
      ),
    );
  }
}
