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
  final nodes = ((m['nodes'] as List?) ?? const [])
      .whereType<Map>()
      .map((e) => GraphNode.fromJson(e.cast<String, dynamic>()))
      .toList();
  final edges = ((m['edges'] as List?) ?? const [])
      .whereType<Map>()
      .map((e) => GraphEdge.fromJson(e.cast<String, dynamic>()))
      .toList();
  return Subgraph(nodes, edges, m['truncated'] == true);
});

/// Whole-graph sample (merged BFS from top-degree seeds) — GET
/// /api/cognitive/sample. Powers the free-form Graph Explorer (web
/// GraphExplorerView), no node selection required.
final cogSampleProvider = FutureProvider<Subgraph>((ref) async {
  final r = await ref.read(apiClientProvider).get('/api/cognitive/sample',
      query: {'seed_count': '8', 'hops': '2', 'limit': '120'});
  final m = r is Map ? r : const {};
  final nodes = ((m['nodes'] as List?) ?? const [])
      .whereType<Map>()
      .map((e) => GraphNode.fromJson(e.cast<String, dynamic>()))
      .toList();
  final edges = ((m['edges'] as List?) ?? const [])
      .whereType<Map>()
      .map((e) => GraphEdge.fromJson(e.cast<String, dynamic>()))
      .toList();
  return Subgraph(nodes, edges, m['truncated'] == true);
});

const Map<String, Color> _kindColors = {
  'entity': Color(0xFF5BBFE8),
  'chunk': Color(0xFF9CA3AF),
  'summary': Color(0xFF10B981),
  'custom': Color(0xFFF59E0B),
};
Color _kindColor(String k) => _kindColors[k] ?? const Color(0xFF9CA3AF);

// ── Force-directed graph view (Fruchterman-Reingold) ────────────────────────
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
          : _GraphCanvas(graph: g, seedId: seedId, onNodeTap: onNodeTap),
    );
  }
}

/// Free-form whole-graph explorer (web GraphExplorerView): renders the sample
/// subgraph; tapping a node calls [onNodeTap]. Shown in a large dialog.
class CogGraphExplorer extends ConsumerWidget {
  const CogGraphExplorer({super.key, required this.onNodeTap});
  final void Function(GraphNode) onNodeTap;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final sub = ref.watch(cogSampleProvider);
    return sub.when(
      loading: () => const Center(child: CircularProgressIndicator()),
      error: (e, _) => Center(
          child: Text('Graph error: $e',
              style: const TextStyle(color: AppTokens.danger))),
      data: (g) => g.nodes.length < 2
          ? Center(
              child: Text('No graph data yet',
                  style: TextStyle(color: c.textMuted, fontSize: 12)))
          : _GraphCanvas(graph: g, seedId: '', onNodeTap: onNodeTap),
    );
  }
}

class _GraphCanvas extends StatefulWidget {
  const _GraphCanvas(
      {required this.graph, required this.seedId, required this.onNodeTap});
  final Subgraph graph;
  final String seedId;
  final void Function(GraphNode) onNodeTap;
  @override
  State<_GraphCanvas> createState() => _GraphCanvasState();
}

class _SimNode {
  double x, y, vx = 0, vy = 0;
  final GraphNode node;
  _SimNode(this.node, this.x, this.y);
}

class _GraphCanvasState extends State<_GraphCanvas> {
  static const double _w = 900, _h = 640;
  late List<_SimNode> _sim;
  late List<({int s, int t, double w, bool inferred})> _links;

  @override
  void initState() {
    super.initState();
    _layout();
  }

  @override
  void didUpdateWidget(covariant _GraphCanvas old) {
    super.didUpdateWidget(old);
    if (old.seedId != widget.seedId || old.graph != widget.graph) _layout();
  }

  /// Deterministic Fruchterman-Reingold (no RNG, so it survives rebuilds): seed
  /// positions on a circle by index, then relax with repulsion + spring + cool.
  void _layout() {
    final nodes = widget.graph.nodes;
    final n = nodes.length;
    final idx = <String, int>{};
    for (var i = 0; i < n; i++) {
      idx[nodes[i].id] = i;
    }
    _links = widget.graph.edges
        .map((e) => (
              s: idx[e.src] ?? -1,
              t: idx[e.dst] ?? -1,
              w: e.strength,
              inferred: e.inferred
            ))
        .where((l) => l.s >= 0 && l.t >= 0 && l.s != l.t)
        .toList();
    _sim = [
      for (var i = 0; i < n; i++)
        _SimNode(
          nodes[i],
          _w / 2 + (_w * 0.35) * math.cos(2 * math.pi * i / n),
          _h / 2 + (_h * 0.35) * math.sin(2 * math.pi * i / n),
        )
    ];
    final k = math.sqrt(_w * _h / n) * 0.55;
    var temp = _w / 10;
    const iters = 280;
    for (var it = 0; it < iters; it++) {
      // Repulsion (all pairs).
      for (var i = 0; i < n; i++) {
        for (var j = i + 1; j < n; j++) {
          var dx = _sim[i].x - _sim[j].x, dy = _sim[i].y - _sim[j].y;
          var d = math.sqrt(dx * dx + dy * dy);
          if (d < 0.01) {
            dx = (i - j) * 0.1 + 0.1;
            dy = 0.1;
            d = 0.15;
          }
          final f = (k * k) / d;
          final fx = (dx / d) * f, fy = (dy / d) * f;
          _sim[i].vx += fx;
          _sim[i].vy += fy;
          _sim[j].vx -= fx;
          _sim[j].vy -= fy;
        }
      }
      // Attraction (springs along edges).
      for (final l in _links) {
        final a = _sim[l.s], b = _sim[l.t];
        var dx = a.x - b.x, dy = a.y - b.y;
        var d = math.sqrt(dx * dx + dy * dy);
        if (d < 0.01) d = 0.01;
        final f = ((d * d) / k) * l.w.clamp(0.2, 1.5);
        final fx = (dx / d) * f, fy = (dy / d) * f;
        a.vx -= fx;
        a.vy -= fy;
        b.vx += fx;
        b.vy += fy;
      }
      // Apply with temperature cap + cooling.
      for (final s in _sim) {
        final disp = math.sqrt(s.vx * s.vx + s.vy * s.vy);
        if (disp > 0.01) {
          final capped = math.min(disp, temp);
          s.x = (s.x + (s.vx / disp) * capped).clamp(20.0, _w - 20);
          s.y = (s.y + (s.vy / disp) * capped).clamp(20.0, _h - 20);
        }
        s.vx = 0;
        s.vy = 0;
      }
      temp = math.max(temp * 0.96, 1.0);
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Stack(
      children: [
        Positioned.fill(
          child: InteractiveViewer(
            minScale: 0.4,
            maxScale: 4,
            boundaryMargin: const EdgeInsets.all(200),
            child: GestureDetector(
              behavior: HitTestBehavior.opaque,
              onTapUp: (d) {
                // Hit-test nodes (in canvas coords; InteractiveViewer applies
                // the transform, so local already maps when scale handled by it
                // — we approximate by nearest within 18px on the raw canvas).
                _SimNode? hit;
                var best = 18.0;
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
                painter: _GraphPainter(_sim, _links, widget.seedId,
                    c.border, c.textPrimary),
              ),
            ),
          ),
        ),
        if (widget.graph.truncated)
          Positioned(
            top: 8,
            right: 8,
            child: Container(
              padding:
                  const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
              decoration: BoxDecoration(
                  color: AppTokens.warning.withValues(alpha: 0.15),
                  borderRadius: BorderRadius.circular(AppTokens.rSm)),
              child: const Text('truncated',
                  style: TextStyle(color: AppTokens.warning, fontSize: 11)),
            ),
          ),
        // Legend.
        Positioned(
          left: 8,
          bottom: 8,
          child: Wrap(
            spacing: 10,
            children: [
              for (final e in _kindColors.entries)
                Row(mainAxisSize: MainAxisSize.min, children: [
                  Container(
                      width: 9,
                      height: 9,
                      decoration: BoxDecoration(
                          color: e.value, shape: BoxShape.circle)),
                  const SizedBox(width: 3),
                  Text(e.key,
                      style: TextStyle(color: c.textMuted, fontSize: 10)),
                ]),
            ],
          ),
        ),
      ],
    );
  }
}

class _GraphPainter extends CustomPainter {
  _GraphPainter(this.sim, this.links, this.seedId, this.borderColor,
      this.labelColor);
  final List<_SimNode> sim;
  final List<({int s, int t, double w, bool inferred})> links;
  final String seedId;
  final Color borderColor;
  final Color labelColor;

  @override
  void paint(Canvas canvas, Size size) {
    // Edges.
    for (final l in links) {
      final a = sim[l.s], b = sim[l.t];
      final paint = Paint()
        ..color = borderColor.withValues(alpha: l.inferred ? 0.35 : 0.6)
        ..strokeWidth = (0.5 + l.w.clamp(0.0, 3.0)).clamp(0.5, 3.5);
      if (l.inferred) {
        _dashedLine(canvas, Offset(a.x, a.y), Offset(b.x, b.y), paint);
      } else {
        canvas.drawLine(Offset(a.x, a.y), Offset(b.x, b.y), paint);
      }
    }
    // Nodes + labels.
    final tp = TextPainter(textDirection: TextDirection.ltr);
    for (final s in sim) {
      final isSeed = s.node.id == seedId;
      final r = isSeed ? 9.0 : 6.0;
      canvas.drawCircle(Offset(s.x, s.y), r,
          Paint()..color = _kindColor(s.node.kind));
      if (isSeed) {
        canvas.drawCircle(
            Offset(s.x, s.y),
            r + 2.5,
            Paint()
              ..style = PaintingStyle.stroke
              ..strokeWidth = 1.5
              ..color = _kindColor(s.node.kind));
      }
      final label = s.node.name.isEmpty ? s.node.kind : s.node.name;
      tp.text = TextSpan(
          text: label.length > 22 ? '${label.substring(0, 21)}…' : label,
          style: TextStyle(color: labelColor, fontSize: 10));
      tp.layout();
      tp.paint(canvas, Offset(s.x + r + 2, s.y - tp.height / 2));
    }
  }

  void _dashedLine(Canvas canvas, Offset a, Offset b, Paint p) {
    const dash = 5.0, gap = 4.0;
    final total = (b - a).distance;
    if (total == 0) return;
    final dir = (b - a) / total;
    var d = 0.0;
    while (d < total) {
      final start = a + dir * d;
      final end = a + dir * math.min(d + dash, total);
      canvas.drawLine(start, end, p);
      d += dash + gap;
    }
  }

  @override
  bool shouldRepaint(covariant _GraphPainter old) =>
      old.sim != sim || old.seedId != seedId;
}
