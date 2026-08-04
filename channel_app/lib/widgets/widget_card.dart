import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:fl_chart/fl_chart.dart';
import 'package:flutter/material.dart';

import '../services/language_service.dart';
import '../theme/tokens.dart';

/// Inline, one-way rich widget rendered in the chat stream (parity with the
/// web `WidgetCard.tsx` / desktop `widget_card.dart`). Reads a WidgetSpec map
/// — `{kind, title, data}` — and switches on [kind]: chart / image / clock /
/// weather. Display only, no response round-trip (unlike the form cards).
/// Unknown kind or malformed data degrades to a small inline error chip; it
/// never throws.
class WidgetCard extends StatelessWidget {
  const WidgetCard({super.key, required this.spec});

  /// The WidgetSpec: `{kind, title?, data}`.
  final Map<String, dynamic> spec;

  /// Fence languages that map to a widget. ```widget carries a full spec;
  /// the rest tag the body as that kind's data.
  static const widgetLangs = {
    'widget',
    'chart',
    'weather',
    'clock',
    'video',
    'audio',
    'image',
  };

  /// Try to parse a fenced block tagged as a widget into a WidgetSpec map.
  /// Returns null when the language isn't a widget tag or the body isn't valid
  /// JSON — the caller then renders a normal code block.
  static Map<String, dynamic>? tryParseFence(String? lang, String code) {
    final l = (lang ?? '').trim().toLowerCase();
    if (!widgetLangs.contains(l)) return null;
    try {
      final decoded = jsonDecode(code.trim());
      if (decoded is! Map) return null;
      final map = decoded.cast<String, dynamic>();
      if (l == 'widget') {
        // Full spec: {kind, title?, data}. Require a kind to be present.
        if (map['kind'] == null) return null;
        return map;
      }
      // Language-tagged kind: wrap the body as {kind: <lang>, data: <json>}.
      // Allow the body to optionally carry its own title.
      return {
        'kind': l,
        if (map['title'] != null) 'title': map['title'],
        'data': map.containsKey('data') ? map['data'] : map,
      };
    } catch (_) {
      return null;
    }
  }

  static const _palette = <Color>[
    Color(0xFF5B8FF9),
    Color(0xFF5AD8A6),
    Color(0xFF5D7092),
    Color(0xFFF6BD16),
    Color(0xFFE8684A),
    Color(0xFF6DC8EC),
    Color(0xFF9270CA),
    Color(0xFFFF9D4D),
    Color(0xFF269A99),
    Color(0xFFFF99C3),
  ];

  @override
  Widget build(BuildContext context) {
    final kind = '${spec['kind'] ?? ''}';
    final title = '${spec['title'] ?? ''}';
    final data = spec['data'];
    final dataMap =
        data is Map ? data.cast<String, dynamic>() : const <String, dynamic>{};

    Widget body;
    IconData icon;
    switch (kind) {
      case 'chart':
        icon = Icons.bar_chart;
        body = _ChartBody(data: dataMap, palette: _palette);
        break;
      case 'image':
        icon = Icons.image_outlined;
        body = _ImageBody(data: dataMap);
        break;
      case 'clock':
        icon = Icons.schedule;
        body = _ClockBody(data: dataMap);
        break;
      case 'weather':
        icon = Icons.wb_sunny_outlined;
        body = _WeatherBody(data: dataMap);
        break;
      // The phone talks to the daemon over the relay — a `127.0.0.1` media or
      // widget entry URL is unreachable from here. Render an informational
      // card (caption/fallback + URL as selectable text) instead of a player.
      case 'video':
        icon = Icons.play_circle_outline;
        body = _MediaInfoBody(data: dataMap, kindLabel: 'Video');
        break;
      case 'audio':
        icon = Icons.audiotrack_outlined;
        body = _MediaInfoBody(data: dataMap, kindLabel: 'Audio');
        break;
      case 'app':
        icon = Icons.widgets_outlined;
        body = _AppInfoBody(data: dataMap);
        break;
      default:
        return _errorChip(
            context, tr('Widget lạ: "$kind"', 'Unknown widget: "$kind"'));
    }

    final c = context.colors;
    return Container(
      margin: const EdgeInsets.symmetric(vertical: AppTokens.s6),
      padding: const EdgeInsets.all(AppTokens.s12),
      decoration: BoxDecoration(
        color: c.surface,
        border: Border.all(color: c.border),
        borderRadius: BorderRadius.circular(AppTokens.rXl),
      ),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          if (title.isNotEmpty) ...[
            Row(
              children: [
                Icon(icon, size: 16, color: c.accent),
                const SizedBox(width: AppTokens.s8),
                Expanded(
                  child: Text(
                    title,
                    maxLines: 2,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      color: c.textPrimary,
                      fontWeight: FontWeight.w700,
                    ),
                  ),
                ),
              ],
            ),
            const SizedBox(height: AppTokens.s12),
          ],
          body,
        ],
      ),
    );
  }

  /// Small inline error chip shown for an unknown kind or malformed data.
  static Widget _errorChip(BuildContext context, String msg) {
    final c = context.colors;
    return Container(
      margin: const EdgeInsets.symmetric(vertical: AppTokens.s4),
      padding: const EdgeInsets.symmetric(
        horizontal: AppTokens.s12,
        vertical: AppTokens.s8,
      ),
      decoration: BoxDecoration(
        color: AppTokens.danger.withValues(alpha: 0.10),
        border: Border.all(color: AppTokens.danger.withValues(alpha: 0.4)),
        borderRadius: BorderRadius.circular(AppTokens.rMd),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          const Icon(Icons.warning_amber_rounded,
              size: 14, color: AppTokens.danger),
          const SizedBox(width: AppTokens.s6),
          Flexible(
            child: Text(
              msg,
              style: TextStyle(color: c.textSecondary, fontSize: 12),
            ),
          ),
        ],
      ),
    );
  }
}

// ─────────────────────────────────────────────────────────────────────────
// Shared helpers
// ─────────────────────────────────────────────────────────────────────────

/// Parse a `#RRGGBB` / `#AARRGGBB` hex color, or null when absent/invalid.
Color? _parseHexColor(dynamic v) {
  if (v is! String) return null;
  var hex = v.trim().replaceFirst('#', '');
  if (hex.length == 6) hex = 'FF$hex';
  if (hex.length != 8) return null;
  final n = int.tryParse(hex, radix: 16);
  return n == null ? null : Color(n);
}

double? _asDouble(dynamic v) {
  if (v is num) return v.toDouble();
  if (v is String) {
    final s = v.trim();
    if (s.isEmpty) return null;
    final direct = double.tryParse(s);
    if (direct != null) return direct;
    // "33,5" → 33.5; never touch thousand-separated "1,234.5" (has a dot).
    if (s.contains(',') && !s.contains('.')) {
      return double.tryParse(s.replaceAll(',', '.'));
    }
  }
  return null;
}

/// A parsed chart series.
class _Series {
  _Series(this.name, this.color, this.points);
  final String name;
  final Color? color;
  final List<(dynamic x, double y)> points;
}

// ─────────────────────────────────────────────────────────────────────────
// Chart
// ─────────────────────────────────────────────────────────────────────────

class _ChartBody extends StatelessWidget {
  const _ChartBody({required this.data, required this.palette});
  final Map<String, dynamic> data;
  final List<Color> palette;

  /// Accepts the same shapes the daemon normalizer takes, so a fenced
  /// ```chart block written inline in the reply (which bypasses the daemon)
  /// renders identically to a tool-emitted chart: canonical `series` (points
  /// as {x,y}, [x,y] pairs or bare numbers), `rows` (one object per x — every
  /// numeric column becomes a series), or `labels` + `values`.
  List<_Series> get _series {
    final rawSeries = data['series'];
    if (rawSeries is List) {
      final out = <_Series>[];
      for (final s in rawSeries.whereType<Map>()) {
        final pts = <(dynamic, double)>[];
        final rawPts = (s['points'] as List?) ?? const [];
        for (var i = 0; i < rawPts.length; i++) {
          final p = rawPts[i];
          if (p is Map) {
            final y = _asDouble(p['y']);
            if (y != null) pts.add((p['x'] ?? i, y));
          } else if (p is List && p.length >= 2) {
            final y = _asDouble(p[1]);
            if (y != null) pts.add((p[0], y));
          } else {
            final y = _asDouble(p);
            if (y != null) pts.add((i, y));
          }
        }
        out.add(_Series(
            '${s['name'] ?? ''}', _parseHexColor(s['color']), pts));
      }
      return out;
    }
    final rows = data['rows'];
    if (rows is List) return _seriesFromRows(rows);
    final labels = data['labels'];
    final values = data['values'];
    if (labels is List && values is List) {
      final pts = <(dynamic, double)>[];
      for (var i = 0; i < labels.length && i < values.length; i++) {
        final y = _asDouble(values[i]);
        if (y != null) pts.add((labels[i], y));
      }
      return [_Series('${data['name'] ?? 'values'}', null, pts)];
    }
    return const [];
  }

  static const _xKeyHints = [
    'x', 'date', 'day', 'time', 'label', 'name', 'ngay', 'ngày',
  ];

  List<_Series> _seriesFromRows(List rows) {
    final maps = rows.whereType<Map>().toList();
    if (maps.isEmpty) return const [];
    final first = maps.first;
    String? xKey;
    final explicit = data['x'];
    if (explicit is String && first.containsKey(explicit)) xKey = explicit;
    if (xKey == null) {
      for (final h in _xKeyHints) {
        if (first.containsKey(h)) {
          xKey = h;
          break;
        }
      }
    }
    if (xKey == null) {
      for (final k in first.keys) {
        final v = first[k];
        if (v is String && _asDouble(v) == null) {
          xKey = '$k';
          break;
        }
      }
    }
    final keys = <String>[];
    for (final r in maps) {
      for (final k in r.keys) {
        final ks = '$k';
        if (ks == xKey || keys.contains(ks)) continue;
        if (_asDouble(r[k]) != null) keys.add(ks);
      }
    }
    return [
      for (final key in keys)
        _Series(key, null, [
          for (var i = 0; i < maps.length; i++)
            if (_asDouble(maps[i][key]) != null)
              ((xKey != null ? maps[i][xKey] : i), _asDouble(maps[i][key])!),
        ]),
    ];
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final chartType = '${data['chartType'] ?? 'bar'}';
    final series = _series;
    if (series.isEmpty || series.every((s) => s.points.isEmpty)) {
      return Text(tr('Không có dữ liệu', 'No chart data'),
          style: TextStyle(color: c.textMuted, fontSize: 12));
    }

    Widget chart;
    switch (chartType) {
      case 'pie':
        chart = _pie(context, series);
        break;
      case 'scatter':
        chart = _scatter(context, series);
        break;
      case 'line':
      case 'area':
        chart = _line(context, series, area: chartType == 'area');
        break;
      case 'bar':
      default:
        chart = _bar(context, series);
        break;
    }

    final xLabel = '${data['xLabel'] ?? ''}';
    final yLabel = '${data['yLabel'] ?? ''}';

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        SizedBox(height: 200, child: chart),
        if (chartType != 'pie' && (xLabel.isNotEmpty || yLabel.isNotEmpty))
          Padding(
            padding: const EdgeInsets.only(top: AppTokens.s6),
            child: Row(
              children: [
                if (yLabel.isNotEmpty)
                  Text('↕ $yLabel',
                      style: TextStyle(color: c.textMuted, fontSize: 11)),
                const Spacer(),
                if (xLabel.isNotEmpty)
                  Text('↔ $xLabel',
                      style: TextStyle(color: c.textMuted, fontSize: 11)),
              ],
            ),
          ),
        if (series.length > 1 || series.first.name.isNotEmpty)
          Padding(
            padding: const EdgeInsets.only(top: AppTokens.s8),
            child: _legend(context, series),
          ),
      ],
    );
  }

  Color _colorFor(_Series s, int i) => s.color ?? palette[i % palette.length];

  Widget _legend(BuildContext context, List<_Series> series) {
    final c = context.colors;
    return Wrap(
      spacing: AppTokens.s12,
      runSpacing: AppTokens.s6,
      children: [
        for (var i = 0; i < series.length; i++)
          if (series[i].name.isNotEmpty)
            Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                Container(
                  width: 10,
                  height: 10,
                  decoration: BoxDecoration(
                    color: _colorFor(series[i], i),
                    borderRadius: BorderRadius.circular(2),
                  ),
                ),
                const SizedBox(width: AppTokens.s6),
                Text(series[i].name,
                    style: TextStyle(color: c.textSecondary, fontSize: 12)),
              ],
            ),
      ],
    );
  }

  /// Ordered union of category labels across all series (bar/line/area x axis).
  List<String> _categories(List<_Series> series) {
    final seen = <String>[];
    for (final s in series) {
      for (final (x, _) in s.points) {
        final label = '$x';
        if (!seen.contains(label)) seen.add(label);
      }
    }
    return seen;
  }

  FlTitlesData _axisTitles(BuildContext context, List<String> categories) {
    final c = context.colors;
    final style = TextStyle(color: c.textMuted, fontSize: 10);
    // Thin out x labels when crowded so they don't overlap.
    final step = (categories.length / 6).ceil().clamp(1, 999);
    return FlTitlesData(
      show: true,
      topTitles: const AxisTitles(sideTitles: SideTitles(showTitles: false)),
      rightTitles: const AxisTitles(sideTitles: SideTitles(showTitles: false)),
      leftTitles: AxisTitles(
        sideTitles: SideTitles(
          showTitles: true,
          reservedSize: 38,
          getTitlesWidget: (v, meta) => SideTitleWidget(
            meta: meta,
            child: Text(_fmtNum(v), style: style),
          ),
        ),
      ),
      bottomTitles: AxisTitles(
        sideTitles: SideTitles(
          showTitles: true,
          reservedSize: 26,
          interval: 1,
          getTitlesWidget: (v, meta) {
            final i = v.round();
            if (i < 0 || i >= categories.length || i % step != 0) {
              return const SizedBox.shrink();
            }
            return SideTitleWidget(
              meta: meta,
              child: Text(
                categories[i],
                style: style,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
              ),
            );
          },
        ),
      ),
    );
  }

  FlGridData _grid(BuildContext context) => FlGridData(
        show: true,
        drawVerticalLine: false,
        getDrawingHorizontalLine: (_) =>
            FlLine(color: context.colors.border, strokeWidth: 1),
      );

  FlBorderData get _border => FlBorderData(show: false);

  // ── bar ────────────────────────────────────────────────────────────────
  Widget _bar(BuildContext context, List<_Series> series) {
    final categories = _categories(series);
    final stacked = data['stacked'] == true;

    final groups = <BarChartGroupData>[];
    for (var ci = 0; ci < categories.length; ci++) {
      final label = categories[ci];
      final rods = <BarChartRodData>[];
      if (stacked) {
        var from = 0.0;
        final stackItems = <BarChartRodStackItem>[];
        for (var si = 0; si < series.length; si++) {
          final y = _valueAt(series[si], label);
          if (y == null) continue;
          stackItems.add(
              BarChartRodStackItem(from, from + y, _colorFor(series[si], si)));
          from += y;
        }
        rods.add(BarChartRodData(
          toY: from,
          width: 14,
          rodStackItems: stackItems,
          borderRadius: BorderRadius.circular(2),
        ));
      } else {
        for (var si = 0; si < series.length; si++) {
          final y = _valueAt(series[si], label);
          if (y == null) continue;
          rods.add(BarChartRodData(
            toY: y,
            color: _colorFor(series[si], si),
            width: series.length > 1 ? 8 : 14,
            borderRadius: BorderRadius.circular(2),
          ));
        }
      }
      groups.add(BarChartGroupData(x: ci, barRods: rods, barsSpace: 2));
    }

    return BarChart(
      BarChartData(
        barGroups: groups,
        gridData: _grid(context),
        borderData: _border,
        titlesData: _axisTitles(context, categories),
        barTouchData: BarTouchData(
          touchTooltipData: BarTouchTooltipData(
            getTooltipColor: (_) => context.colors.surfaceAlt,
            getTooltipItem: (group, gi, rod, ri) => BarTooltipItem(
              _fmtNum(rod.toY),
              TextStyle(
                  color: context.colors.textPrimary,
                  fontSize: 11,
                  fontWeight: FontWeight.w600),
            ),
          ),
        ),
      ),
    );
  }

  double? _valueAt(_Series s, String label) {
    for (final (x, y) in s.points) {
      if ('$x' == label) return y;
    }
    return null;
  }

  // ── line / area ──────────────────────────────────────────────────────────
  Widget _line(BuildContext context, List<_Series> series,
      {required bool area}) {
    final categories = _categories(series);
    final bars = <LineChartBarData>[];
    for (var si = 0; si < series.length; si++) {
      final s = series[si];
      final color = _colorFor(s, si);
      final spots = <FlSpot>[];
      for (var ci = 0; ci < categories.length; ci++) {
        final y = _valueAt(s, categories[ci]);
        if (y != null) spots.add(FlSpot(ci.toDouble(), y));
      }
      bars.add(LineChartBarData(
        spots: spots,
        color: color,
        barWidth: 2.5,
        isCurved: true,
        curveSmoothness: 0.2,
        dotData: FlDotData(show: spots.length <= 24),
        belowBarData: BarAreaData(
          show: area,
          color: color.withValues(alpha: 0.18),
        ),
      ));
    }

    return LineChart(
      LineChartData(
        lineBarsData: bars,
        gridData: _grid(context),
        borderData: _border,
        titlesData: _axisTitles(context, categories),
        lineTouchData: LineTouchData(
          touchTooltipData: LineTouchTooltipData(
            getTooltipColor: (_) => context.colors.surfaceAlt,
            getTooltipItems: (spots) => [
              for (final s in spots)
                LineTooltipItem(
                  _fmtNum(s.y),
                  TextStyle(
                      color: context.colors.textPrimary,
                      fontSize: 11,
                      fontWeight: FontWeight.w600),
                ),
            ],
          ),
        ),
      ),
    );
  }

  // ── scatter ──────────────────────────────────────────────────────────────
  Widget _scatter(BuildContext context, List<_Series> series) {
    final spots = <ScatterSpot>[];
    for (var si = 0; si < series.length; si++) {
      final color = _colorFor(series[si], si);
      for (final (x, y) in series[si].points) {
        final xv = _asDouble(x);
        if (xv == null) continue;
        spots.add(ScatterSpot(
          xv,
          y,
          dotPainter: FlDotCirclePainter(radius: 5, color: color),
        ));
      }
    }
    final c = context.colors;
    final style = TextStyle(color: c.textMuted, fontSize: 10);
    return ScatterChart(
      ScatterChartData(
        scatterSpots: spots,
        gridData: _grid(context),
        borderData: _border,
        titlesData: FlTitlesData(
          show: true,
          topTitles:
              const AxisTitles(sideTitles: SideTitles(showTitles: false)),
          rightTitles:
              const AxisTitles(sideTitles: SideTitles(showTitles: false)),
          leftTitles: AxisTitles(
            sideTitles: SideTitles(
              showTitles: true,
              reservedSize: 38,
              getTitlesWidget: (v, meta) => SideTitleWidget(
                  meta: meta, child: Text(_fmtNum(v), style: style)),
            ),
          ),
          bottomTitles: AxisTitles(
            sideTitles: SideTitles(
              showTitles: true,
              reservedSize: 26,
              getTitlesWidget: (v, meta) => SideTitleWidget(
                  meta: meta, child: Text(_fmtNum(v), style: style)),
            ),
          ),
        ),
      ),
    );
  }

  // ── pie ──────────────────────────────────────────────────────────────────
  Widget _pie(BuildContext context, List<_Series> series) {
    final pts = series.first.points;
    final total = pts.fold<double>(0, (a, p) => a + p.$2);
    final sections = <PieChartSectionData>[];
    for (var i = 0; i < pts.length; i++) {
      final (_, y) = pts[i];
      final color = palette[i % palette.length];
      final pct = total > 0 ? (y / total * 100) : 0;
      sections.add(PieChartSectionData(
        value: y,
        color: color,
        title: pct >= 5 ? '${pct.round()}%' : '',
        radius: 62,
        titleStyle: const TextStyle(
            color: Colors.white, fontSize: 11, fontWeight: FontWeight.w700),
      ));
    }
    return Row(
      children: [
        Expanded(
          flex: 3,
          child: PieChart(
            PieChartData(
              sections: sections,
              sectionsSpace: 2,
              centerSpaceRadius: 26,
            ),
          ),
        ),
        const SizedBox(width: AppTokens.s12),
        Expanded(flex: 2, child: _pieLegend(context, pts)),
      ],
    );
  }

  Widget _pieLegend(BuildContext context, List<(dynamic, double)> pts) {
    final c = context.colors;
    return Column(
      mainAxisAlignment: MainAxisAlignment.center,
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        for (var i = 0; i < pts.length; i++)
          Padding(
            padding: const EdgeInsets.symmetric(vertical: 2),
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                Container(
                  width: 10,
                  height: 10,
                  decoration: BoxDecoration(
                    color: palette[i % palette.length],
                    borderRadius: BorderRadius.circular(2),
                  ),
                ),
                const SizedBox(width: AppTokens.s6),
                Flexible(
                  child: Text('${pts[i].$1}  ${_fmtNum(pts[i].$2)}',
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(color: c.textSecondary, fontSize: 12)),
                ),
              ],
            ),
          ),
      ],
    );
  }
}

/// Compact numeric label: drops a trailing `.0`, keeps up to 2 decimals.
String _fmtNum(double v) {
  if (v == v.roundToDouble()) return v.toInt().toString();
  return v
      .toStringAsFixed(2)
      .replaceFirst(RegExp(r'0+$'), '')
      .replaceFirst(RegExp(r'\.$'), '');
}

// ─────────────────────────────────────────────────────────────────────────
// Image
// ─────────────────────────────────────────────────────────────────────────

class _ImageBody extends StatelessWidget {
  const _ImageBody({required this.data});
  final Map<String, dynamic> data;

  Uint8List? _decode(String dataUrl) {
    final comma = dataUrl.indexOf(',');
    final b64 = comma >= 0 ? dataUrl.substring(comma + 1) : dataUrl;
    try {
      return base64Decode(b64);
    } catch (_) {
      return null;
    }
  }

  void _showFullImage(BuildContext context, ImageProvider provider) {
    showDialog(
      context: context,
      barrierColor: Colors.black87,
      builder: (ctx) => GestureDetector(
        onTap: () => Navigator.of(ctx).pop(),
        child: Stack(
          children: [
            InteractiveViewer(
              minScale: 0.5,
              maxScale: 5,
              child:
                  Center(child: Image(image: provider, fit: BoxFit.contain)),
            ),
            Positioned(
              top: 24,
              right: 24,
              child: IconButton(
                icon: const Icon(Icons.close, color: Colors.white, size: 28),
                onPressed: () => Navigator.of(ctx).pop(),
              ),
            ),
          ],
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final dataUrl = '${data['dataUrl'] ?? ''}';
    final url = '${data['url'] ?? ''}';
    final caption = '${data['caption'] ?? ''}';
    final alt = '${data['alt'] ?? ''}';

    ImageProvider? provider;
    if (dataUrl.isNotEmpty) {
      final bytes = _decode(dataUrl);
      if (bytes != null) provider = MemoryImage(bytes);
    } else if (url.startsWith('data:')) {
      final bytes = _decode(url);
      if (bytes != null) provider = MemoryImage(bytes);
    } else if (url.isNotEmpty) {
      provider = NetworkImage(url);
    }

    if (provider == null) {
      return Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(Icons.broken_image_outlined, size: 16, color: c.textMuted),
          const SizedBox(width: AppTokens.s6),
          Text(
              alt.isNotEmpty
                  ? alt
                  : tr('Không tải được ảnh', 'Image unavailable'),
              style: TextStyle(color: c.textMuted, fontSize: 12)),
        ],
      );
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        GestureDetector(
          onTap: () => _showFullImage(context, provider!),
          child: ClipRRect(
            borderRadius: BorderRadius.circular(AppTokens.rMd),
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxHeight: 280),
              child: Image(
                image: provider,
                fit: BoxFit.contain,
                semanticLabel: alt.isNotEmpty ? alt : null,
                errorBuilder: (_, _, _) => Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Icon(Icons.broken_image_outlined,
                        size: 16, color: c.textMuted),
                    const SizedBox(width: AppTokens.s6),
                    Text(tr('Lỗi tải ảnh', 'Failed to load image'),
                        style: TextStyle(color: c.textMuted, fontSize: 12)),
                  ],
                ),
              ),
            ),
          ),
        ),
        if (caption.isNotEmpty)
          Padding(
            padding: const EdgeInsets.only(top: AppTokens.s6),
            child: Text(caption,
                style: TextStyle(color: c.textMuted, fontSize: 12)),
          ),
      ],
    );
  }
}

// ─────────────────────────────────────────────────────────────────────────
// Clock
// ─────────────────────────────────────────────────────────────────────────

class _ClockBody extends StatefulWidget {
  const _ClockBody({required this.data});
  final Map<String, dynamic> data;
  @override
  State<_ClockBody> createState() => _ClockBodyState();
}

class _ClockBodyState extends State<_ClockBody> {
  Timer? _timer;
  DateTime _now = DateTime.now();

  @override
  void initState() {
    super.initState();
    _timer = Timer.periodic(const Duration(seconds: 1), (_) {
      if (mounted) setState(() => _now = DateTime.now());
    });
  }

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }

  static String _two(int n) => n.toString().padLeft(2, '0');

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final d = widget.data;
    final label = '${d['label'] ?? d['tz'] ?? ''}';
    final showSeconds = d['showSeconds'] != false;
    final showDate = d['showDate'] != false;
    final format24h = d['format24h'] != false;

    // Best-effort: local time. IANA tz conversion isn't done here, so the tz /
    // label is shown as-is for context.
    final now = _now;
    String timeStr;
    if (format24h) {
      timeStr = '${_two(now.hour)}:${_two(now.minute)}';
      if (showSeconds) timeStr += ':${_two(now.second)}';
    } else {
      final h12 = now.hour % 12 == 0 ? 12 : now.hour % 12;
      timeStr = '${_two(h12)}:${_two(now.minute)}';
      if (showSeconds) timeStr += ':${_two(now.second)}';
      timeStr += now.hour < 12 ? ' AM' : ' PM';
    }
    final dateStr = showDate
        ? '${_two(now.day)}/${_two(now.month)}/${now.year}'
        : '';

    return Row(
      children: [
        Icon(Icons.access_time_filled, size: 34, color: c.accent),
        const SizedBox(width: AppTokens.s16),
        Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(
              timeStr,
              style: TextStyle(
                color: c.textPrimary,
                fontSize: 30,
                fontWeight: FontWeight.w700,
                fontFeatures: const [FontFeature.tabularFigures()],
              ),
            ),
            if (label.isNotEmpty || dateStr.isNotEmpty)
              Padding(
                padding: const EdgeInsets.only(top: 2),
                child: Text(
                  [if (label.isNotEmpty) label, if (dateStr.isNotEmpty) dateStr]
                      .join('  ·  '),
                  style: TextStyle(color: c.textMuted, fontSize: 13),
                ),
              ),
          ],
        ),
      ],
    );
  }
}

// ─────────────────────────────────────────────────────────────────────────
// Weather
// ─────────────────────────────────────────────────────────────────────────

const Map<String, String> _weatherGlyphs = {
  'sunny': '☀️',
  'partly_cloudy': '⛅',
  'cloudy': '☁️',
  'rain': '🌧️',
  'thunderstorm': '⛈️',
  'snow': '❄️',
  'fog': '🌫️',
  'wind': '💨',
};

String _glyphFor(dynamic icon) => _weatherGlyphs['$icon'] ?? '🌡️';

class _WeatherBody extends StatelessWidget {
  const _WeatherBody({required this.data});
  final Map<String, dynamic> data;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final location = '${data['location'] ?? ''}';
    final unit = '${data['unit'] ?? 'C'}';
    final current = (data['current'] is Map)
        ? (data['current'] as Map).cast<String, dynamic>()
        : const <String, dynamic>{};
    final daily = ((data['daily'] as List?) ?? const [])
        .whereType<Map>()
        .map((e) => e.cast<String, dynamic>())
        .take(7)
        .toList();

    final temp = _asDouble(current['temp']);
    final condition = '${current['condition'] ?? ''}';
    final humidity = _asDouble(current['humidity']);
    final wind = _asDouble(current['wind']);

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        // Current conditions.
        Row(
          crossAxisAlignment: CrossAxisAlignment.center,
          children: [
            Text(_glyphFor(current['icon']),
                style: const TextStyle(fontSize: 44)),
            const SizedBox(width: AppTokens.s12),
            Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              mainAxisSize: MainAxisSize.min,
              children: [
                if (location.isNotEmpty)
                  Text(location,
                      style: TextStyle(
                          color: c.textSecondary,
                          fontSize: 13,
                          fontWeight: FontWeight.w600)),
                Text(
                  temp != null ? '${_fmtNum(temp)}°$unit' : '—',
                  style: TextStyle(
                      color: c.textPrimary,
                      fontSize: 28,
                      fontWeight: FontWeight.w700),
                ),
                if (condition.isNotEmpty)
                  Text(condition,
                      style: TextStyle(color: c.textMuted, fontSize: 13)),
              ],
            ),
            const Spacer(),
            Column(
              crossAxisAlignment: CrossAxisAlignment.end,
              mainAxisSize: MainAxisSize.min,
              children: [
                if (humidity != null)
                  _metric(context, Icons.water_drop_outlined,
                      '${_fmtNum(humidity)}%'),
                if (wind != null)
                  _metric(context, Icons.air, '${_fmtNum(wind)} km/h'),
              ],
            ),
          ],
        ),
        if (daily.isNotEmpty) ...[
          const SizedBox(height: AppTokens.s16),
          Divider(color: c.border, height: 1),
          const SizedBox(height: AppTokens.s12),
          SingleChildScrollView(
            scrollDirection: Axis.horizontal,
            child: Row(
              children: [
                for (final d in daily) _dayCol(context, d, unit),
              ],
            ),
          ),
        ],
      ],
    );
  }

  Widget _metric(BuildContext context, IconData icon, String text) {
    final c = context.colors;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 1),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(icon, size: 13, color: c.textMuted),
          const SizedBox(width: AppTokens.s4),
          Text(text, style: TextStyle(color: c.textSecondary, fontSize: 12)),
        ],
      ),
    );
  }

  Widget _dayCol(BuildContext context, Map<String, dynamic> d, String unit) {
    final c = context.colors;
    final day = '${d['day'] ?? ''}';
    final hi = _asDouble(d['hi']);
    final lo = _asDouble(d['lo']);
    return Container(
      width: 62,
      margin: const EdgeInsets.only(right: AppTokens.s8),
      padding: const EdgeInsets.symmetric(vertical: AppTokens.s8),
      decoration: BoxDecoration(
        color: c.surfaceAlt,
        borderRadius: BorderRadius.circular(AppTokens.rMd),
      ),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Text(day,
              style: TextStyle(
                  color: c.textSecondary,
                  fontSize: 12,
                  fontWeight: FontWeight.w600)),
          const SizedBox(height: 4),
          Text(_glyphFor(d['icon']), style: const TextStyle(fontSize: 20)),
          const SizedBox(height: 4),
          Text(
            '${hi != null ? _fmtNum(hi) : '–'}°',
            style: TextStyle(
                color: c.textPrimary,
                fontSize: 12,
                fontWeight: FontWeight.w600),
          ),
          Text(
            '${lo != null ? _fmtNum(lo) : '–'}°',
            style: TextStyle(color: c.textMuted, fontSize: 11),
          ),
        ],
      ),
    );
  }
}

/// Video/audio on mobile: the daemon's media URLs are usually loopback-local
/// and unreachable over the relay, so show the caption + a selectable URL the
/// user can copy into a browser on the same network, instead of a dead player.
class _MediaInfoBody extends StatelessWidget {
  const _MediaInfoBody({required this.data, required this.kindLabel});
  final Map<String, dynamic> data;
  final String kindLabel;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final url = '${data['url'] ?? ''}'.trim();
    final caption = '${data['caption'] ?? ''}';
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        if (caption.isNotEmpty)
          Text(caption,
              style: TextStyle(color: c.textPrimary, fontSize: 13),
              maxLines: 2,
              overflow: TextOverflow.ellipsis),
        if (url.isNotEmpty)
          SelectableText(url,
              maxLines: 2, style: TextStyle(color: c.accent, fontSize: 12)),
        Padding(
          padding: const EdgeInsets.only(top: AppTokens.s4),
          child: Text(
            tr('$kindLabel phát trên SenClaw Web/Desktop',
                '$kindLabel plays on SenClaw Web/Desktop'),
            style: TextStyle(color: c.textMuted, fontSize: 11),
          ),
        ),
      ],
    );
  }
}

/// Space-App widget on mobile: the embedded entry is desktop-only — show the
/// text fallback the daemon rendered (or a pointer at the app).
class _AppInfoBody extends StatelessWidget {
  const _AppInfoBody({required this.data});
  final Map<String, dynamic> data;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final id = '${data['id'] ?? data['widget'] ?? ''}';
    final fallback = '${data['textFallback'] ?? ''}';
    return Text(
      fallback.isNotEmpty
          ? fallback
          : tr('Widget $id — xem trên SenClaw Web/Desktop',
              'Widget $id — view on SenClaw Web/Desktop'),
      style: TextStyle(color: c.textSecondary, fontSize: 13),
    );
  }
}
