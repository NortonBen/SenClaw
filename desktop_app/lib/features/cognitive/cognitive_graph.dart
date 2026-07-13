import 'dart:math' as math;
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/transport/connection.dart';
import '../../theme/tokens.dart';

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
  final r = await ref.read(apiClientProvider).get('/api/cognitive/full-graph',
      query: {
        'node_limit': '2000',
        'edge_limit': '5000',
        'include_chunks': params.includeChunks ? 'true' : 'false',
        'connected_only': params.connectedOnly ? 'true' : 'false',
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

const _tierColors = [Color(0xFF6B7280), Color(0xFF3B82F6), Color(0xFF10B981)];

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
                Text('Drag = pan · Pinch = zoom · Tap node = focus',
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

// ── Graph canvas — shared by CogGraphView & CogGraphExplorer ─────────────
class _GraphCanvas extends StatefulWidget {
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
  State<_GraphCanvas> createState() => _GraphCanvasState();
}

class _SimNode {
  double x, y, vx = 0, vy = 0;
  int degree;
  final GraphNode node;
  _SimNode(this.node, this.x, this.y, this.degree);
}

class _GraphCanvasState extends State<_GraphCanvas> {
  static const double _w = 1200, _h = 800;
  late List<_SimNode> _sim;
  late List<({int s, int t, double w, bool inferred, int tier})> _links;
  late Map<String, Set<String>> _neighborMap;

  @override
  void initState() {
    super.initState();
    _layout();
  }

  @override
  void didUpdateWidget(covariant _GraphCanvas old) {
    super.didUpdateWidget(old);
    if (old.graph != widget.graph) _layout();
  }

  void _layout() {
    final nodes = widget.graph.nodes;
    final n = nodes.length;
    if (n == 0) {
      _sim = [];
      _links = [];
      _neighborMap = {};
      return;
    }

    final degreeMap = <String, int>{};
    for (final e in widget.graph.edges) {
      degreeMap[e.src] = (degreeMap[e.src] ?? 0) + 1;
      degreeMap[e.dst] = (degreeMap[e.dst] ?? 0) + 1;
    }

    _neighborMap = {};
    for (final e in widget.graph.edges) {
      (_neighborMap[e.src] ??= {}).add(e.dst);
      (_neighborMap[e.dst] ??= {}).add(e.src);
    }

    final idx = <String, int>{};
    for (var i = 0; i < n; i++) {
      idx[nodes[i].id] = i;
    }
    _links = widget.graph.edges
        .map((e) => (
              s: idx[e.src] ?? -1,
              t: idx[e.dst] ?? -1,
              w: e.strength,
              inferred: e.inferred,
              tier: e.tier,
            ))
        .where((l) => l.s >= 0 && l.t >= 0 && l.s != l.t)
        .toList();
    _sim = [
      for (var i = 0; i < n; i++)
        _SimNode(
          nodes[i],
          _w / 2 + (_w * 0.35) * math.cos(2 * math.pi * i / n),
          _h / 2 + (_h * 0.35) * math.sin(2 * math.pi * i / n),
          degreeMap[nodes[i].id] ?? 0,
        )
    ];
    final k = math.sqrt(_w * _h / n) * 0.85;
    var temp = math.min(_w, _h) * 0.15;
    final iters = math.min(300, 100 + n * 2);
    final cooling = temp / iters;
    for (var it = 0; it < iters; it++) {
      for (var i = 0; i < n; i++) {
        _sim[i].vx = 0;
        _sim[i].vy = 0;
        for (var j = i + 1; j < n; j++) {
          var dx = _sim[i].x - _sim[j].x;
          var dy = _sim[i].y - _sim[j].y;
          var d = math.sqrt(dx * dx + dy * dy) + 0.1;
          final f = (k * k) / d;
          final fx = (dx / d) * f, fy = (dy / d) * f;
          _sim[i].vx += fx;
          _sim[i].vy += fy;
          _sim[j].vx -= fx;
          _sim[j].vy -= fy;
        }
      }
      for (final l in _links) {
        final a = _sim[l.s], b = _sim[l.t];
        var dx = a.x - b.x, dy = a.y - b.y;
        var d = math.sqrt(dx * dx + dy * dy) + 0.1;
        final f = ((d * d) / k) * l.w.clamp(0.3, 2.0);
        final fx = (dx / d) * f, fy = (dy / d) * f;
        a.vx -= fx;
        a.vy -= fy;
        b.vx += fx;
        b.vy += fy;
      }
      final cx = _w / 2, cy = _h / 2;
      for (final s in _sim) {
        final grav = s.degree == 0 ? 0.05 : 0.01;
        s.vx += (cx - s.x) * grav;
        s.vy += (cy - s.y) * grav;
      }
      for (final s in _sim) {
        final disp = math.sqrt(s.vx * s.vx + s.vy * s.vy) + 0.01;
        s.x = (s.x + (s.vx / disp) * math.min(disp, temp)).clamp(30.0, _w - 30);
        s.y = (s.y + (s.vy / disp) * math.min(disp, temp)).clamp(30.0, _h - 30);
      }
      temp -= cooling;
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Stack(
      children: [
        Positioned.fill(
          child: InteractiveViewer(
            minScale: 0.3,
            maxScale: 5,
            boundaryMargin: const EdgeInsets.all(300),
            child: GestureDetector(
              behavior: HitTestBehavior.opaque,
              onTapUp: (d) {
                _SimNode? hit;
                var best = 22.0;
                for (final s in _sim) {
                  final dd = (Offset(s.x, s.y) - d.localPosition).distance;
                  if (dd < best) {
                    best = dd;
                    hit = s;
                  }
                }
                if (hit != null) widget.onNodeTap(hit.node);
              },
              child: CustomPaint(
                size: const Size(_w, _h),
                painter: _GraphPainter(
                  _sim,
                  _links,
                  widget.focusId,
                  widget.searchText,
                  _neighborMap,
                  c.border,
                  c.textPrimary,
                ),
              ),
            ),
          ),
        ),
        // Legend
        Positioned(
          left: 8,
          bottom: 8,
          child: Container(
            padding: const EdgeInsets.all(8),
            decoration: BoxDecoration(
              color: c.sidebar.withValues(alpha: 0.85),
              borderRadius: BorderRadius.circular(8),
            ),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              mainAxisSize: MainAxisSize.min,
              children: [
                for (final e in _kindColors.entries)
                  Padding(
                    padding: const EdgeInsets.only(bottom: 2),
                    child: Row(mainAxisSize: MainAxisSize.min, children: [
                      Container(
                        width: 9,
                        height: 9,
                        decoration: BoxDecoration(
                          shape: BoxShape.circle,
                          color: e.value,
                          boxShadow: [
                            BoxShadow(
                                color: e.value.withValues(alpha: 0.5),
                                blurRadius: 3),
                          ],
                        ),
                      ),
                      const SizedBox(width: 5),
                      Text(e.key,
                          style:
                              TextStyle(color: c.textMuted, fontSize: 10)),
                    ]),
                  ),
                Divider(height: 8, color: c.border),
                for (var i = 0; i < _tierColors.length; i++)
                  Padding(
                    padding: const EdgeInsets.only(bottom: 2),
                    child: Row(mainAxisSize: MainAxisSize.min, children: [
                      Container(width: 14, height: 2, color: _tierColors[i]),
                      const SizedBox(width: 5),
                      Text(['L1 working', 'L2 episodic', 'L3 semantic'][i],
                          style:
                              TextStyle(color: c.textMuted, fontSize: 10)),
                    ]),
                  ),
              ],
            ),
          ),
        ),
      ],
    );
  }
}

// ── CustomPainter — draws edges + nodes with focus/search styling ─────────
class _GraphPainter extends CustomPainter {
  _GraphPainter(this.sim, this.links, this.focusId, this.searchText,
      this.neighborMap, this.borderColor, this.labelColor);
  final List<_SimNode> sim;
  final List<({int s, int t, double w, bool inferred, int tier})> links;
  final String focusId;
  final String searchText;
  final Map<String, Set<String>> neighborMap;
  final Color borderColor;
  final Color labelColor;

  @override
  void paint(Canvas canvas, Size size) {
    final hasFocus = focusId.isNotEmpty;
    final focusNeighbors = hasFocus ? (neighborMap[focusId] ?? {}) : <String>{};
    final searchLower = searchText.toLowerCase().trim();
    final hasSearch = searchLower.isNotEmpty;

    // Edges
    for (final l in links) {
      final a = sim[l.s], b = sim[l.t];
      final isFocusEdge = hasFocus &&
          (a.node.id == focusId || b.node.id == focusId);
      final dimmed = hasFocus && !isFocusEdge;

      final color = _tierColors.length > l.tier
          ? _tierColors[l.tier]
          : borderColor;
      final paint = Paint()
        ..color = color.withValues(alpha: dimmed ? 0.04 : (l.inferred ? 0.25 : 0.45))
        ..strokeWidth =
            isFocusEdge ? 2.5 : (0.5 + l.w.clamp(0.0, 3.0)).clamp(0.5, 3.0);

      // Curved edge
      final mx = (a.x + b.x) / 2;
      final my = (a.y + b.y) / 2;
      final dx = b.x - a.x;
      final dy = b.y - a.y;
      final len = math.sqrt(dx * dx + dy * dy) + 0.1;
      final curveOff = math.min(20.0, len * 0.08);
      final nx = -dy / len;
      final ny = dx / len;
      final cx = mx + nx * curveOff;
      final cy = my + ny * curveOff;

      final path = Path()
        ..moveTo(a.x, a.y)
        ..quadraticBezierTo(cx, cy, b.x, b.y);

      if (l.inferred) {
        _dashedPath(canvas, path, paint);
      } else {
        canvas.drawPath(path, paint..style = PaintingStyle.stroke);
      }
    }

    // Nodes
    for (final s in sim) {
      final isFocused = s.node.id == focusId;
      final isNeighbor = hasFocus && focusNeighbors.contains(s.node.id);
      final dimmed = hasFocus && !isFocused && !isNeighbor;
      final isSearchMatch =
          hasSearch && s.node.name.toLowerCase().contains(searchLower);

      final color = _kindColor(s.node.kind);
      final r = _nodeR(s.degree, isFocused);
      final opacity = dimmed ? 0.08 : 1.0;

      // Glow for focused/search nodes
      if (isFocused || isSearchMatch) {
        final glowColor = isSearchMatch ? const Color(0xFFFFD700) : color;
        canvas.drawCircle(
          Offset(s.x, s.y),
          r + 6,
          Paint()
            ..color = glowColor.withValues(alpha: 0.3 * opacity)
            ..maskFilter = const MaskFilter.blur(BlurStyle.normal, 4),
        );
      }

      // Background circle
      canvas.drawCircle(
        Offset(s.x, s.y),
        r,
        Paint()..color = color.withValues(alpha: 0.12 * opacity),
      );
      // Ring
      canvas.drawCircle(
        Offset(s.x, s.y),
        r - 1,
        Paint()
          ..style = PaintingStyle.stroke
          ..strokeWidth = isFocused ? 2.5 : 1.5
          ..color = color.withValues(alpha: opacity),
      );
      // Inner dot
      canvas.drawCircle(
        Offset(s.x, s.y),
        math.max(2, r * 0.35),
        Paint()..color = color.withValues(alpha: opacity),
      );

      // Label
      if (!dimmed || isFocused || isSearchMatch) {
        final label = s.node.name.isEmpty ? s.node.kind : s.node.name;
        final tp = TextPainter(
          text: TextSpan(
            text: label.length > 24 ? '${label.substring(0, 22)}…' : label,
            style: TextStyle(
              color: labelColor.withValues(alpha: opacity),
              fontSize: isFocused ? 11 : 9,
              fontWeight: isFocused ? FontWeight.w600 : FontWeight.w400,
            ),
          ),
          textDirection: TextDirection.ltr,
        );
        tp.layout();
        tp.paint(canvas, Offset(s.x - tp.width / 2, s.y + r + 4));
      }
    }
  }

  double _nodeR(int degree, bool focused) {
    final base = math.max(4.0, math.min(20.0, 4.0 + math.sqrt(degree) * 2.5));
    return focused ? base + 3 : base;
  }

  void _dashedPath(Canvas canvas, Path path, Paint paint) {
    for (final metric in path.computeMetrics()) {
      var d = 0.0;
      const dash = 5.0, gap = 4.0;
      while (d < metric.length) {
        final end = math.min(d + dash, metric.length);
        final seg = metric.extractPath(d, end);
        canvas.drawPath(seg, paint..style = PaintingStyle.stroke);
        d += dash + gap;
      }
    }
  }

  @override
  bool shouldRepaint(covariant _GraphPainter old) =>
      old.sim != sim || old.focusId != focusId || old.searchText != searchText;
}
