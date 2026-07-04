import 'dart:math' as math;
import 'package:flutter/material.dart';
import '../../models/cognitive_models.dart';
import '../../services/cognitive_api.dart';
import '../../services/language_service.dart';
import '../../services/relay_manager.dart';
import '../../theme/tokens.dart';
import '../../util/format.dart';
import '../../widgets/markdown_text.dart';
import '../../widgets/states.dart';

/// Color for a node kind, matching the web graph legend roughly.
/// Theme-aware: the `entity` accent resolves from [context] when provided.
Color kindColor(String kind, [BuildContext? context]) {
  switch (kind) {
    case 'entity':
      return context?.colors.accent ?? AppTokens.brand;
    case 'chunk':
      return AppTokens.cyan;
    case 'summary':
      return AppTokens.warning;
    default:
      return AppTokens.success;
  }
}

/// Cognitive (knowledge graph) over `/api/cognitive/*`. Recall, graph explorer,
/// add knowledge, and a paginated data-point list.
class CognitiveScreen extends StatefulWidget {
  const CognitiveScreen({super.key});

  @override
  State<CognitiveScreen> createState() => _CognitiveScreenState();
}

class _CognitiveScreenState extends State<CognitiveScreen>
    with SingleTickerProviderStateMixin {
  final _api = CognitiveApi();
  late final TabController _tabs = TabController(length: 4, vsync: this);

  @override
  void dispose() {
    _tabs.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Scaffold(
      backgroundColor: c.bg,
      appBar: AppBar(
        backgroundColor: c.surface,
        elevation: 0,
        title: Row(
          children: [
            Text(tr('Tri thức', 'Knowledge'),
                style: TextStyle(color: c.textPrimary)),
            const SizedBox(width: 8),
            AnimatedBuilder(
              animation: RelayManager(),
              builder: (_, _) =>
                  ConnectionDot(connected: RelayManager().connected),
            ),
          ],
        ),
        bottom: TabBar(
          controller: _tabs,
          isScrollable: true,
          tabAlignment: TabAlignment.start,
          indicatorColor: c.accent,
          labelColor: c.accent,
          unselectedLabelColor: c.textMuted,
          tabs: [
            Tab(
                icon: const Icon(Icons.psychology_outlined),
                text: tr('Gợi nhớ', 'Recall')),
            Tab(icon: const Icon(Icons.hub_outlined), text: tr('Đồ thị', 'Graph')),
            Tab(
                icon: const Icon(Icons.add_circle_outline),
                text: tr('Thêm', 'Add')),
            Tab(
                icon: const Icon(Icons.storage_outlined),
                text: tr('Dữ liệu', 'Data')),
          ],
        ),
      ),
      body: Container(
        decoration: BoxDecoration(color: c.bg),
        child: TabBarView(
          controller: _tabs,
          children: [
            _RecallTab(api: _api),
            _GraphTab(api: _api),
            _AddTab(api: _api),
            _DataTab(api: _api),
          ],
        ),
      ),
    );
  }
}

// ─── Recall ──────────────────────────────────────────────────────────────────

class _RecallTab extends StatefulWidget {
  final CognitiveApi api;
  const _RecallTab({required this.api});

  @override
  State<_RecallTab> createState() => _RecallTabState();
}

class _RecallTabState extends State<_RecallTab>
    with AutomaticKeepAliveClientMixin {
  final _ctrl = TextEditingController();
  CogRecall? _result;
  bool _loading = false;
  String? _error;

  @override
  bool get wantKeepAlive => true;

  @override
  void dispose() {
    _ctrl.dispose();
    super.dispose();
  }

  Future<void> _ask() async {
    final q = _ctrl.text.trim();
    if (q.isEmpty) return;
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final r = await widget.api.recall(q);
      if (!mounted) return;
      setState(() {
        _result = r;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = '$e';
        _loading = false;
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    final c = context.colors;
    return ListView(
      padding: const EdgeInsets.fromLTRB(12, 12, 12, 24),
      children: [
        TextField(
          controller: _ctrl,
          style: TextStyle(color: c.textPrimary),
          textInputAction: TextInputAction.search,
          onSubmitted: (_) => _ask(),
          decoration:
              _inputDec(context, tr('Hỏi từ trí nhớ…', 'Ask from memory…')),
        ),
        const SizedBox(height: 10),
        SizedBox(
          width: double.infinity,
          child: ElevatedButton.icon(
            onPressed: _loading ? null : _ask,
            icon: _loading
                ? const SizedBox(
                    width: 16,
                    height: 16,
                    child: CircularProgressIndicator(
                        strokeWidth: 2, color: Colors.white))
                : const Icon(Icons.auto_awesome),
            label: Text(tr('Hỏi', 'Ask')),
            style: ElevatedButton.styleFrom(
              backgroundColor: c.accent,
              foregroundColor: Colors.white,
              padding: const EdgeInsets.symmetric(vertical: 14),
            ),
          ),
        ),
        if (_error != null) ...[
          const SizedBox(height: 14),
          Text(_error!,
              style: const TextStyle(color: AppTokens.danger, fontSize: 13)),
        ],
        if (_result != null) ...[
          const SizedBox(height: 18),
          Container(
            padding: const EdgeInsets.all(14),
            decoration: BoxDecoration(
              color: c.surfaceAlt,
              borderRadius: BorderRadius.circular(12),
              border: Border.all(color: c.border),
            ),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    Icon(
                      _result!.grounded ? Icons.verified : Icons.info_outline,
                      size: 16,
                      color: _result!.grounded
                          ? AppTokens.success
                          : c.textMuted,
                    ),
                    const SizedBox(width: 6),
                    Text(
                      _result!.grounded
                          ? tr('Có dẫn chứng', 'Grounded')
                          : tr('Chưa tổng hợp', 'Not synthesized'),
                      style: TextStyle(
                          color: _result!.grounded
                              ? AppTokens.success
                              : c.textMuted,
                          fontSize: 11),
                    ),
                  ],
                ),
                const SizedBox(height: 8),
                MarkdownText(_result!.answer),
                if (_result!.note != null && _result!.note!.isNotEmpty) ...[
                  const SizedBox(height: 8),
                  Text(_result!.note!,
                      style: TextStyle(
                          color: c.textMuted, fontSize: 11)),
                ],
              ],
            ),
          ),
          if (_result!.sources.isNotEmpty) ...[
            const SizedBox(height: 14),
            Text(tr('Nguồn', 'Sources'),
                style: TextStyle(
                    color: c.textSecondary,
                    fontSize: 13,
                    fontWeight: FontWeight.w600)),
            const SizedBox(height: 6),
            ..._result!.sources.map((s) => Padding(
                  padding: const EdgeInsets.only(bottom: 8),
                  child: Row(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text('[${s.index}]',
                          style: const TextStyle(
                              color: AppTokens.cyan, fontSize: 12)),
                      const SizedBox(width: 8),
                      Expanded(
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Text(s.name,
                                style: TextStyle(
                                    color: c.textPrimary, fontSize: 13)),
                            Text(s.summary,
                                style: TextStyle(
                                    color: c.textSecondary, fontSize: 11),
                                maxLines: 3,
                                overflow: TextOverflow.ellipsis),
                          ],
                        ),
                      ),
                    ],
                  ),
                )),
          ],
        ],
      ],
    );
  }
}

// ─── Graph ───────────────────────────────────────────────────────────────────

class _GraphTab extends StatefulWidget {
  final CognitiveApi api;
  const _GraphTab({required this.api});

  @override
  State<_GraphTab> createState() => _GraphTabState();
}

class _GraphTabState extends State<_GraphTab>
    with AutomaticKeepAliveClientMixin {
  CogSubgraph? _graph;
  bool _loading = true;
  String? _error;
  CogNode? _selected;

  @override
  bool get wantKeepAlive => true;

  @override
  void initState() {
    super.initState();
    _loadSample();
  }

  Future<void> _loadSample() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final g = await widget.api.sample();
      if (!mounted) return;
      setState(() {
        _graph = g;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = '$e';
        _loading = false;
      });
    }
  }

  Future<void> _reseed(CogNode n) async {
    setState(() {
      _loading = true;
      _selected = n;
    });
    try {
      final g = await widget.api.subgraph(n.id, hops: 2);
      if (!mounted) return;
      setState(() {
        _graph = g;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = '$e';
        _loading = false;
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    final c = context.colors;
    if (_loading) {
      return LoadingState(text: tr('Đang dựng đồ thị…', 'Building graph…'));
    }
    if (_error != null) return ErrorState(message: _error!, onRetry: _loadSample);
    final g = _graph;
    if (g == null || g.nodes.isEmpty) {
      return EmptyState(
        icon: Icons.hub_outlined,
        message: tr('Đồ thị trống', 'Graph is empty'),
        hint: tr('Thêm tri thức để xây dựng mạng liên kết',
            'Add knowledge to build the link network'),
        action: OutlinedButton.icon(
          onPressed: _loadSample,
          icon: Icon(Icons.refresh, color: c.accent, size: 18),
          label: Text(tr('Tải lại', 'Reload'),
              style: TextStyle(color: c.accent)),
          style: OutlinedButton.styleFrom(
              side: BorderSide(color: c.accent)),
        ),
      );
    }
    return Column(
      children: [
        if (_selected != null)
          Container(
            width: double.infinity,
            color: c.surface,
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
            child: Row(
              children: [
                Expanded(
                  child: Text(
                      tr('Tâm: ${_selected!.name}',
                          'Center: ${_selected!.name}'),
                      style: TextStyle(
                          color: c.textSecondary, fontSize: 12),
                      overflow: TextOverflow.ellipsis),
                ),
                TextButton(
                  onPressed: _loadSample,
                  child: Text(tr('Đặt lại', 'Reset'),
                      style: TextStyle(color: c.accent, fontSize: 12)),
                ),
              ],
            ),
          ),
        if (g.truncated)
          Padding(
            padding: const EdgeInsets.all(6),
            child: Text(
                tr('Đồ thị đã bị cắt bớt (đạt giới hạn nút)',
                    'Graph was truncated (node limit reached)'),
                style: const TextStyle(color: AppTokens.warning, fontSize: 11)),
          ),
        Expanded(
          child: InteractiveViewer(
            minScale: 0.4,
            maxScale: 3,
            child: GraphCanvas(
              graph: g,
              onTapNode: _reseed,
              seedId: _selected?.id,
              edgeColor: c.border,
              edgeAccent: c.accent,
              labelColor: c.textSecondary,
              seedRingColor: c.textPrimary,
              accent: c.accent,
            ),
          ),
        ),
      ],
    );
  }
}

/// Force-directed graph rendered with CustomPaint. Layout is computed once per
/// graph; tap re-seeds via [onTapNode].
class GraphCanvas extends StatefulWidget {
  final CogSubgraph graph;
  final String? seedId;
  final void Function(CogNode) onTapNode;

  /// Theme-resolved colors passed in from [context.colors]/[AppTokens] so the
  /// painter never hardcodes white/purple and works in light + dark.
  final Color edgeColor; // inferred / default edge
  final Color edgeAccent; // asserted edge
  final Color labelColor; // node labels
  final Color seedRingColor; // ring around the seed node
  final Color accent; // entity-kind node accent

  const GraphCanvas({
    super.key,
    required this.graph,
    required this.onTapNode,
    required this.edgeColor,
    required this.edgeAccent,
    required this.labelColor,
    required this.seedRingColor,
    required this.accent,
    this.seedId,
  });

  @override
  State<GraphCanvas> createState() => _GraphCanvasState();
}

class _GraphCanvasState extends State<GraphCanvas> {
  static const _size = 1000.0;
  final Map<String, Offset> _pos = {};

  @override
  void initState() {
    super.initState();
    _layout();
  }

  @override
  void didUpdateWidget(covariant GraphCanvas old) {
    super.didUpdateWidget(old);
    if (old.graph != widget.graph) _layout();
  }

  /// Simple Fruchterman-Reingold layout over a fixed virtual canvas.
  void _layout() {
    _pos.clear();
    final nodes = widget.graph.nodes;
    if (nodes.isEmpty) return;
    final rng = math.Random(7);
    const center = _size / 2;
    final radius = _size * 0.4;
    for (var i = 0; i < nodes.length; i++) {
      final a = (i / nodes.length) * 2 * math.pi + rng.nextDouble() * 0.3;
      _pos[nodes[i].id] =
          Offset(center + radius * math.cos(a), center + radius * math.sin(a));
    }
    final k = _size / math.sqrt(nodes.length + 1) * 0.6;
    final idIndex = {for (var i = 0; i < nodes.length; i++) nodes[i].id: i};
    for (var iter = 0; iter < 120; iter++) {
      final disp = {for (final n in nodes) n.id: Offset.zero};
      // repulsion
      for (var i = 0; i < nodes.length; i++) {
        for (var j = i + 1; j < nodes.length; j++) {
          final pi = _pos[nodes[i].id]!;
          final pj = _pos[nodes[j].id]!;
          var delta = pi - pj;
          var dist = delta.distance;
          if (dist < 0.01) {
            delta = Offset(rng.nextDouble(), rng.nextDouble());
            dist = delta.distance;
          }
          final force = (k * k) / dist;
          final push = delta / dist * force;
          disp[nodes[i].id] = disp[nodes[i].id]! + push;
          disp[nodes[j].id] = disp[nodes[j].id]! - push;
        }
      }
      // attraction along edges
      for (final e in widget.graph.edges) {
        if (!idIndex.containsKey(e.src) || !idIndex.containsKey(e.dst)) continue;
        final ps = _pos[e.src]!;
        final pd = _pos[e.dst]!;
        var delta = ps - pd;
        final dist = delta.distance.clamp(0.01, double.infinity);
        final force = (dist * dist) / k;
        final pull = delta / dist * force;
        disp[e.src] = disp[e.src]! - pull;
        disp[e.dst] = disp[e.dst]! + pull;
      }
      final temp = (1 - iter / 120) * _size * 0.05;
      for (final n in nodes) {
        final d = disp[n.id]!;
        final dist = d.distance.clamp(0.01, double.infinity);
        var np = _pos[n.id]! + d / dist * math.min(dist, temp);
        np = Offset(
          np.dx.clamp(20.0, _size - 20),
          np.dy.clamp(20.0, _size - 20),
        );
        _pos[n.id] = np;
      }
    }
  }

  void _handleTap(TapUpDetails d, BoxConstraints c) {
    final scale = math.min(c.maxWidth, c.maxHeight) / _size;
    final p = d.localPosition / scale;
    CogNode? hit;
    var best = 40.0;
    for (final n in widget.graph.nodes) {
      final pos = _pos[n.id];
      if (pos == null) continue;
      final dist = (pos - p).distance;
      if (dist < best) {
        best = dist;
        hit = n;
      }
    }
    if (hit != null) widget.onTapNode(hit);
  }

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, c) {
        final side = math.min(c.maxWidth, c.maxHeight);
        return Center(
          child: GestureDetector(
            onTapUp: (d) => _handleTap(d, BoxConstraints.tight(Size(side, side))),
            child: CustomPaint(
              size: Size(side, side),
              painter: _GraphPainter(
                graph: widget.graph,
                pos: _pos,
                virtualSize: _size,
                seedId: widget.seedId,
                edgeColor: widget.edgeColor,
                edgeAccent: widget.edgeAccent,
                labelColor: widget.labelColor,
                seedRingColor: widget.seedRingColor,
                accent: widget.accent,
              ),
            ),
          ),
        );
      },
    );
  }
}

class _GraphPainter extends CustomPainter {
  final CogSubgraph graph;
  final Map<String, Offset> pos;
  final double virtualSize;
  final String? seedId;

  /// Theme-resolved colors (see [GraphCanvas]) — no hardcoded white/purple.
  final Color edgeColor;
  final Color edgeAccent;
  final Color labelColor;
  final Color seedRingColor;
  final Color accent;

  _GraphPainter({
    required this.graph,
    required this.pos,
    required this.virtualSize,
    required this.edgeColor,
    required this.edgeAccent,
    required this.labelColor,
    required this.seedRingColor,
    required this.accent,
    this.seedId,
  });

  /// Node fill per kind, using the theme [accent] for entities.
  Color _nodeColor(String kind) {
    switch (kind) {
      case 'entity':
        return accent;
      case 'chunk':
        return AppTokens.cyan;
      case 'summary':
        return AppTokens.warning;
      default:
        return AppTokens.success;
    }
  }

  @override
  void paint(Canvas canvas, Size size) {
    final scale = size.width / virtualSize;
    Offset map(Offset o) => o * scale;

    for (final e in graph.edges) {
      final s = pos[e.src];
      final d = pos[e.dst];
      if (s == null || d == null) continue;
      final paint = Paint()
        ..color = (e.inferred ? edgeColor : edgeAccent)
            .withValues(alpha: 0.25 + e.strength * 0.5)
        ..strokeWidth = 0.5 + e.strength * 2;
      canvas.drawLine(map(s), map(d), paint);
    }

    final labelStyle = TextStyle(color: labelColor, fontSize: 9);
    for (final n in graph.nodes) {
      final p = pos[n.id];
      if (p == null) continue;
      final c = map(p);
      final isSeed = n.id == seedId;
      final r = (isSeed ? 9.0 : 5.0) + n.salience * 4;
      canvas.drawCircle(
          c, r, Paint()..color = _nodeColor(n.kind).withValues(alpha: 0.9));
      if (isSeed) {
        canvas.drawCircle(
            c,
            r + 3,
            Paint()
              ..style = PaintingStyle.stroke
              ..strokeWidth = 1.5
              ..color = seedRingColor);
      }
      final name = n.name.length > 18 ? '${n.name.substring(0, 18)}…' : n.name;
      final tp = TextPainter(
        text: TextSpan(text: name, style: labelStyle),
        textDirection: TextDirection.ltr,
      )..layout(maxWidth: 90);
      tp.paint(canvas, c + Offset(r + 2, -tp.height / 2));
    }
  }

  @override
  bool shouldRepaint(covariant _GraphPainter old) =>
      old.graph != graph ||
      old.seedId != seedId ||
      old.edgeColor != edgeColor ||
      old.edgeAccent != edgeAccent ||
      old.labelColor != labelColor ||
      old.seedRingColor != seedRingColor ||
      old.accent != accent;
}

// ─── Add ─────────────────────────────────────────────────────────────────────

class _AddTab extends StatefulWidget {
  final CognitiveApi api;
  const _AddTab({required this.api});

  @override
  State<_AddTab> createState() => _AddTabState();
}

class _AddTabState extends State<_AddTab> with AutomaticKeepAliveClientMixin {
  final _text = TextEditingController();
  final _tags = TextEditingController();
  bool _saving = false;
  CogAddResult? _result;
  String? _error;

  @override
  bool get wantKeepAlive => true;

  @override
  void dispose() {
    _text.dispose();
    _tags.dispose();
    super.dispose();
  }

  Future<void> _add() async {
    if (_text.text.trim().isEmpty) return;
    setState(() {
      _saving = true;
      _error = null;
      _result = null;
    });
    try {
      final tags = _tags.text
          .split(',')
          .map((s) => s.trim())
          .where((s) => s.isNotEmpty)
          .toList();
      final r = await widget.api.add(_text.text, source: 'mobile', tags: tags);
      if (!mounted) return;
      setState(() {
        _result = r;
        _saving = false;
        _text.clear();
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = '$e';
        _saving = false;
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    final c = context.colors;
    return ListView(
      padding: const EdgeInsets.fromLTRB(12, 12, 12, 24),
      children: [
        TextField(
          controller: _tags,
          style: TextStyle(color: c.textPrimary),
          decoration: _inputDec(
              context,
              tr('Thẻ (phân tách bằng dấu phẩy, tuỳ chọn)',
                  'Tags (comma-separated, optional)')),
        ),
        const SizedBox(height: 10),
        TextField(
          controller: _text,
          maxLines: 8,
          style: TextStyle(color: c.textPrimary),
          decoration: _inputDec(
              context, tr('Nội dung cần ghi nhớ…', 'Content to remember…')),
        ),
        const SizedBox(height: 12),
        SizedBox(
          width: double.infinity,
          child: ElevatedButton.icon(
            onPressed: _saving ? null : _add,
            icon: _saving
                ? const SizedBox(
                    width: 16,
                    height: 16,
                    child: CircularProgressIndicator(
                        strokeWidth: 2, color: Colors.white))
                : const Icon(Icons.add),
            label: Text(tr('Thêm vào trí nhớ', 'Add to memory')),
            style: ElevatedButton.styleFrom(
              backgroundColor: c.accent,
              foregroundColor: Colors.white,
              padding: const EdgeInsets.symmetric(vertical: 14),
            ),
          ),
        ),
        if (_error != null) ...[
          const SizedBox(height: 12),
          Text(_error!,
              style: const TextStyle(color: AppTokens.danger, fontSize: 13)),
        ],
        if (_result != null) ...[
          const SizedBox(height: 16),
          Container(
            padding: const EdgeInsets.all(12),
            decoration: BoxDecoration(
              color: AppTokens.success.withValues(alpha: 0.08),
              borderRadius: BorderRadius.circular(12),
              border: Border.all(
                  color: AppTokens.success.withValues(alpha: 0.3)),
            ),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  tr('+${_result!.chunksAdded} đoạn · +${_result!.entitiesAdded} thực thể · +${_result!.edgesAdded} liên kết',
                      '+${_result!.chunksAdded} chunks · +${_result!.entitiesAdded} entities · +${_result!.edgesAdded} links'),
                  style: TextStyle(color: c.textPrimary, fontSize: 13),
                ),
                if (_result!.llmSkipped)
                  Padding(
                    padding: const EdgeInsets.only(top: 6),
                    child: Text(
                        tr('LLM đang ngủ — chỉ lưu thô',
                            'LLM is asleep — raw save only'),
                        style: const TextStyle(
                            color: AppTokens.warning, fontSize: 11)),
                  ),
              ],
            ),
          ),
        ],
      ],
    );
  }
}

// ─── Data points ─────────────────────────────────────────────────────────────

class _DataTab extends StatefulWidget {
  final CognitiveApi api;
  const _DataTab({required this.api});

  @override
  State<_DataTab> createState() => _DataTabState();
}

class _DataTabState extends State<_DataTab>
    with AutomaticKeepAliveClientMixin {
  List<CogNode> _nodes = [];
  bool _loading = true;
  String? _error;
  String? _kind;

  @override
  bool get wantKeepAlive => true;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final r = await widget.api.nodes(kind: _kind, limit: 100);
      if (!mounted) return;
      setState(() {
        _nodes = r.nodes;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = '$e';
        _loading = false;
      });
    }
  }

  Future<void> _forget(CogNode n) async {
    try {
      await widget.api.deleteNode(n.id);
      _load();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(
                SnackBar(content: Text(tr('Lỗi: $e', 'Error: $e'))));
      }
    }
  }

  Future<void> _maintenance() async {
    try {
      final r = await widget.api.maintenance();
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
            content: Text(tr(
                'Dọn dẹp: gộp ${r['entities_merged'] ?? 0} thực thể, bỏ ${r['orphan_entities_removed'] ?? 0} mồ côi',
                'Cleanup: merged ${r['entities_merged'] ?? 0} entities, removed ${r['orphan_entities_removed'] ?? 0} orphans'))));
      }
      _load();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(
                SnackBar(content: Text(tr('Lỗi: $e', 'Error: $e'))));
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    final c = context.colors;
    return Column(
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(12, 8, 12, 4),
          child: Row(
            children: [
              Expanded(
                child: SingleChildScrollView(
                  scrollDirection: Axis.horizontal,
                  child: Row(
                    children: [
                      for (final k in [null, 'entity', 'chunk', 'summary'])
                        Padding(
                          padding: const EdgeInsets.only(right: 6),
                          child: ChoiceChip(
                            label: Text(k ?? tr('Tất cả', 'All'),
                                style: const TextStyle(fontSize: 12)),
                            selected: _kind == k,
                            onSelected: (_) {
                              setState(() => _kind = k);
                              _load();
                            },
                            selectedColor:
                                c.accent.withValues(alpha: 0.3),
                            backgroundColor: c.surfaceAlt,
                            labelStyle: TextStyle(color: c.textPrimary),
                          ),
                        ),
                    ],
                  ),
                ),
              ),
              IconButton(
                tooltip: tr('Dọn dẹp', 'Cleanup'),
                icon: Icon(Icons.cleaning_services_outlined,
                    color: c.textSecondary, size: 20),
                onPressed: _maintenance,
              ),
            ],
          ),
        ),
        Expanded(child: _buildList()),
      ],
    );
  }

  Widget _buildList() {
    final c = context.colors;
    if (_loading) return const LoadingState();
    if (_error != null) return ErrorState(message: _error!, onRetry: _load);
    if (_nodes.isEmpty) {
      return EmptyState(
        icon: Icons.storage_outlined,
        message: tr('Chưa có dữ liệu', 'No data yet'),
      );
    }
    return RefreshIndicator(
      onRefresh: _load,
      color: c.accent,
      backgroundColor: c.surface,
      child: ListView.builder(
        padding: const EdgeInsets.fromLTRB(12, 4, 12, 24),
        itemCount: _nodes.length,
        itemBuilder: (ctx, i) {
          final n = _nodes[i];
          return Card(
            color: c.surfaceAlt,
            margin: const EdgeInsets.only(bottom: 8),
            shape: RoundedRectangleBorder(
              borderRadius: BorderRadius.circular(12),
              side: BorderSide(color: c.border),
            ),
            child: ListTile(
              leading: CircleAvatar(
                radius: 6,
                backgroundColor: kindColor(n.kind, context),
              ),
              title: Text(n.name,
                  style: TextStyle(color: c.textPrimary, fontSize: 14),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis),
              subtitle: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  if (n.summary.isNotEmpty)
                    Text(n.summary,
                        style: TextStyle(
                            color: c.textSecondary, fontSize: 12),
                        maxLines: 2,
                        overflow: TextOverflow.ellipsis),
                  Text(
                      tr('${n.kind} · ${n.mentionCount} lần · ${timeAgoEpochSecs(n.lastSeenAt)}',
                          '${n.kind} · ${n.mentionCount} mentions · ${timeAgoEpochSecs(n.lastSeenAt)}'),
                      style: TextStyle(
                          color: c.textMuted, fontSize: 10)),
                ],
              ),
              trailing: IconButton(
                icon: Icon(Icons.delete_outline,
                    color: c.textMuted, size: 20),
                onPressed: () => _forget(n),
              ),
            ),
          );
        },
      ),
    );
  }
}

// ─── shared ──────────────────────────────────────────────────────────────────

InputDecoration _inputDec(BuildContext context, String hint) {
  final c = context.colors;
  return InputDecoration(
    hintText: hint,
    hintStyle: TextStyle(color: c.textMuted),
    filled: true,
    fillColor: c.surfaceAlt,
    border: OutlineInputBorder(
      borderRadius: BorderRadius.circular(10),
      borderSide: BorderSide(color: c.border),
    ),
    enabledBorder: OutlineInputBorder(
      borderRadius: BorderRadius.circular(10),
      borderSide: BorderSide(color: c.border),
    ),
  );
}
