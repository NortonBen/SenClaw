import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:audioplayers/audioplayers.dart';
import 'package:fl_chart/fl_chart.dart';
import 'package:flutter/material.dart';
import 'package:intl/intl.dart';
import 'package:url_launcher/url_launcher.dart';

import '../../../core/i18n/l10n.dart';
import '../../../theme/tokens.dart';
import '../../../widgets/embedded_web.dart';
import '../flow_defaults.dart';

/// Inline, one-way rich widget rendered in the chat stream (parity with the web
/// `WidgetCard.tsx`). Reads a WidgetSpec map — `{kind, title, data}` — and
/// switches on [kind]: chart / image / clock / weather / video. Display only, no
/// response round-trip (unlike FormCard). Unknown kind or malformed data
/// degrades to a small inline error chip; it never throws.
class WidgetCard extends StatelessWidget {
  const WidgetCard({super.key, required this.spec});

  /// The WidgetSpec: `{kind, title?, data}`.
  final Map<String, dynamic> spec;

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
    final dataMap = data is Map ? data.cast<String, dynamic>() : const <String, dynamic>{};

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
      case 'video':
        icon = Icons.play_circle_outline;
        body = _VideoBody(data: dataMap);
        break;
      case 'audio':
        icon = Icons.audiotrack_outlined;
        body = _AudioBody(data: dataMap);
        break;
      case 'app':
        icon = Icons.widgets_outlined;
        body = _AppWidgetBody(data: dataMap);
        break;
      case 'clock':
        icon = Icons.schedule;
        body = _ClockBody(data: dataMap);
        break;
      case 'weather':
        icon = Icons.wb_sunny_outlined;
        body = _WeatherBody(data: dataMap);
        break;
      default:
        return _errorChip(
            context, context.trArgs('Unknown widget: "{kind}"', {'kind': kind}));
    }

    final c = context.colors;
    return Container(
      alignment: Alignment.centerLeft,
      padding: const EdgeInsets.symmetric(
        horizontal: AppTokens.s24,
        vertical: AppTokens.s6,
      ),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 720),
        child: Container(
          padding: const EdgeInsets.all(AppTokens.s16),
          decoration: BoxDecoration(
            color: c.surface,
            border: Border.all(color: c.border),
            borderRadius: BorderRadius.circular(AppTokens.rLg),
          ),
          child: Column(
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
        ),
      ),
    );
  }

  /// Small inline error chip shown for an unknown kind or malformed data.
  static Widget _errorChip(BuildContext context, String msg) {
    final c = context.colors;
    return Padding(
      padding: const EdgeInsets.symmetric(
        horizontal: AppTokens.s24,
        vertical: AppTokens.s4,
      ),
      child: Container(
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
    // x column: explicit `x` field → well-known name → first non-numeric
    // string column → row index.
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
      return Text(context.tr('No chart data'),
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
        SizedBox(height: 240, child: chart),
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
    final step = (categories.length / 8).ceil().clamp(1, 999);
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
    final idxOf = {for (var i = 0; i < categories.length; i++) categories[i]: i};

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
          stackItems.add(BarChartRodStackItem(from, from + y, _colorFor(series[si], si)));
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
      // suppress unused idxOf lints if categories empty
      idxOf[label];
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
  Widget _line(BuildContext context, List<_Series> series, {required bool area}) {
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
          topTitles: const AxisTitles(sideTitles: SideTitles(showTitles: false)),
          rightTitles:
              const AxisTitles(sideTitles: SideTitles(showTitles: false)),
          leftTitles: AxisTitles(
            sideTitles: SideTitles(
              showTitles: true,
              reservedSize: 38,
              getTitlesWidget: (v, meta) =>
                  SideTitleWidget(meta: meta, child: Text(_fmtNum(v), style: style)),
            ),
          ),
          bottomTitles: AxisTitles(
            sideTitles: SideTitles(
              showTitles: true,
              reservedSize: 26,
              getTitlesWidget: (v, meta) =>
                  SideTitleWidget(meta: meta, child: Text(_fmtNum(v), style: style)),
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
      final (x, y) = pts[i];
      final color = palette[i % palette.length];
      final pct = total > 0 ? (y / total * 100) : 0;
      sections.add(PieChartSectionData(
        value: y,
        color: color,
        title: pct >= 5 ? '${pct.round()}%' : '',
        radius: 82,
        titleStyle: const TextStyle(
            color: Colors.white, fontSize: 11, fontWeight: FontWeight.w700),
      ));
      // keep label for legend below via _pieLegend
      x;
    }
    return Row(
      children: [
        Expanded(
          flex: 3,
          child: PieChart(
            PieChartData(
              sections: sections,
              sectionsSpace: 2,
              centerSpaceRadius: 32,
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
  return v.toStringAsFixed(2).replaceFirst(RegExp(r'0+$'), '').replaceFirst(RegExp(r'\.$'), '');
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
              child: Center(child: Image(image: provider, fit: BoxFit.contain)),
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
          Text(alt.isNotEmpty ? alt : context.tr('Image unavailable'),
              style: TextStyle(color: c.textMuted, fontSize: 12)),
        ],
      );
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        MouseRegion(
          cursor: SystemMouseCursors.click,
          child: GestureDetector(
            onTap: () => _showFullImage(context, provider!),
            child: ClipRRect(
              borderRadius: BorderRadius.circular(AppTokens.rMd),
              child: ConstrainedBox(
                constraints: const BoxConstraints(maxHeight: 360),
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
                      Text(context.tr('Failed to load image'),
                          style: TextStyle(color: c.textMuted, fontSize: 12)),
                    ],
                  ),
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
// Video
// ─────────────────────────────────────────────────────────────────────────

/// Inline video player. There is no native Flutter player wired into this app,
/// so playback runs in the same embedded web surface the Space Apps use — a
/// WKWebView/WebView2 on macOS & Windows, a real <iframe> on the Flutter-web
/// build, and an "open in browser" card on Linux (no embedded webview there).
///
/// Click-to-play on purpose: a chat scrollback can hold many video cards, and
/// spinning up a webview per card on first paint would be brutal. Until the
/// user presses play this is just a poster image.
class _VideoBody extends StatefulWidget {
  const _VideoBody({required this.data});
  final Map<String, dynamic> data;
  @override
  State<_VideoBody> createState() => _VideoBodyState();
}

class _VideoBodyState extends State<_VideoBody> {
  bool _playing = false;

  /// Only http(s) can be handed to the webview — a bare filesystem path or a
  /// file:// URL is the common mistake and would render a blank frame.
  String get _url {
    final raw = '${widget.data['url'] ?? ''}'.trim();
    final lower = raw.toLowerCase();
    return (lower.startsWith('http://') || lower.startsWith('https://'))
        ? raw
        : '';
  }

  @override
  void initState() {
    super.initState();
    _playing = widget.data['autoplay'] == true;
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final url = _url;
    final caption = '${widget.data['caption'] ?? ''}';

    if (url.isEmpty) {
      return Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(Icons.videocam_off_outlined, size: 16, color: c.textMuted),
          const SizedBox(width: AppTokens.s6),
          Flexible(
            child: Text(
              context.tr('Video needs a playable http(s) URL (not a file path).'),
              style: TextStyle(color: c.textMuted, fontSize: 12),
            ),
          ),
        ],
      );
    }

    final isDark = Theme.of(context).brightness == Brightness.dark;

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        ClipRRect(
          borderRadius: BorderRadius.circular(AppTokens.rMd),
          child: SizedBox(
            height: 380,
            width: double.infinity,
            child: _playing
                ? embeddedWebView(
                    url,
                    title: caption.isNotEmpty ? caption : context.tr('Video'),
                    theme: isDark ? 'dark' : 'light',
                    instanceKey: 'chat-video',
                  )
                : _poster(context, url),
          ),
        ),
        Padding(
          padding: const EdgeInsets.only(top: AppTokens.s6),
          child: Row(
            children: [
              if (caption.isNotEmpty)
                Flexible(
                  child: Text(
                    caption,
                    maxLines: 2,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(color: c.textMuted, fontSize: 12),
                  ),
                ),
              const Spacer(),
              TextButton.icon(
                onPressed: () => launchUrl(Uri.parse(url),
                    mode: LaunchMode.externalApplication),
                icon: const Icon(Icons.open_in_new, size: 14),
                label: Text(context.tr('Open externally')),
                style: TextButton.styleFrom(
                  foregroundColor: c.textMuted,
                  textStyle: const TextStyle(fontSize: 12),
                  padding: const EdgeInsets.symmetric(
                      horizontal: AppTokens.s8, vertical: 0),
                  minimumSize: Size.zero,
                  tapTargetSize: MaterialTapTargetSize.shrinkWrap,
                ),
              ),
            ],
          ),
        ),
      ],
    );
  }

  /// Pre-playback surface: the poster frame (when supplied) under a play button.
  Widget _poster(BuildContext context, String url) {
    final poster = '${widget.data['poster'] ?? ''}'.trim();
    return Stack(
      fit: StackFit.expand,
      children: [
        Container(color: Colors.black),
        if (poster.isNotEmpty)
          Image.network(
            poster,
            fit: BoxFit.contain,
            errorBuilder: (_, _, _) => const SizedBox.shrink(),
          ),
        Material(
          color: Colors.transparent,
          child: InkWell(
            onTap: () => setState(() => _playing = true),
            child: Center(
              child: Container(
                width: 64,
                height: 64,
                decoration: BoxDecoration(
                  color: Colors.black.withValues(alpha: 0.55),
                  shape: BoxShape.circle,
                  border: Border.all(color: Colors.white24),
                ),
                child: const Icon(Icons.play_arrow_rounded,
                    size: 40, color: Colors.white),
              ),
            ),
          ),
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
    final timePattern = format24h
        ? (showSeconds ? 'HH:mm:ss' : 'HH:mm')
        : (showSeconds ? 'hh:mm:ss a' : 'hh:mm a');
    final timeStr = DateFormat(timePattern).format(now);
    final dateStr = showDate ? DateFormat('EEE, d MMM yyyy').format(now) : '';

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

/// Inline audio player (kind `audio`). Uses the same `audioplayers` package as
/// the TTS AudioService — no new dependency. Streams from the URL; nothing is
/// downloaded until the user presses play.
class _AudioBody extends StatefulWidget {
  const _AudioBody({required this.data});
  final Map<String, dynamic> data;
  @override
  State<_AudioBody> createState() => _AudioBodyState();
}

class _AudioBodyState extends State<_AudioBody> {
  AudioPlayer? _player;
  PlayerState _state = PlayerState.stopped;
  StreamSubscription<PlayerState>? _sub;

  String get _url {
    final raw = '${widget.data['url'] ?? ''}'.trim();
    final lower = raw.toLowerCase();
    return (lower.startsWith('http://') || lower.startsWith('https://'))
        ? raw
        : '';
  }

  Future<void> _toggle() async {
    final url = _url;
    if (url.isEmpty) return;
    final p = _player ??= AudioPlayer();
    _sub ??= p.onPlayerStateChanged.listen((s) {
      if (mounted) setState(() => _state = s);
    });
    try {
      if (_state == PlayerState.playing) {
        await p.pause();
      } else if (_state == PlayerState.paused) {
        await p.resume();
      } else {
        await p.play(UrlSource(url));
      }
    } catch (_) {
      // Undecodable stream — leave the "Open externally" escape hatch.
    }
  }

  @override
  void dispose() {
    _sub?.cancel();
    _player?.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final url = _url;
    final caption = '${widget.data['caption'] ?? ''}';

    if (url.isEmpty) {
      return Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(Icons.music_off_outlined, size: 16, color: c.textMuted),
          const SizedBox(width: AppTokens.s6),
          Flexible(
            child: Text(
              context.tr('Audio needs a playable http(s) URL (not a file path).'),
              style: TextStyle(color: c.textMuted, fontSize: 12),
            ),
          ),
        ],
      );
    }

    final playing = _state == PlayerState.playing;
    return Row(
      children: [
        IconButton(
          onPressed: _toggle,
          icon: Icon(
            playing ? Icons.pause_circle_filled : Icons.play_circle_fill,
            size: 34,
            color: c.accent,
          ),
          padding: EdgeInsets.zero,
          visualDensity: VisualDensity.compact,
          tooltip: playing ? context.tr('Pause') : context.tr('Play'),
        ),
        const SizedBox(width: AppTokens.s8),
        Expanded(
          child: Text(
            caption.isNotEmpty ? caption : Uri.parse(url).pathSegments.lastOrNull ?? url,
            maxLines: 2,
            overflow: TextOverflow.ellipsis,
            style: TextStyle(color: c.textPrimary, fontSize: 13),
          ),
        ),
        TextButton.icon(
          onPressed: () =>
              launchUrl(Uri.parse(url), mode: LaunchMode.externalApplication),
          icon: const Icon(Icons.open_in_new, size: 14),
          label: Text(context.tr('Open externally')),
          style: TextButton.styleFrom(
            foregroundColor: c.textMuted,
            textStyle: const TextStyle(fontSize: 12),
            padding:
                const EdgeInsets.symmetric(horizontal: AppTokens.s8, vertical: 0),
            minimumSize: Size.zero,
          ),
        ),
      ],
    );
  }
}

/// Space-App widget (kind `app`) — the entry resolved by the daemon registry
/// rendered in the shared embedded web surface. Click-to-load like
/// [_VideoBody]: a long scrollback must not mount one webview per card.
class _AppWidgetBody extends StatefulWidget {
  const _AppWidgetBody({required this.data});
  final Map<String, dynamic> data;
  @override
  State<_AppWidgetBody> createState() => _AppWidgetBodyState();
}

class _AppWidgetBodyState extends State<_AppWidgetBody> {
  bool _loaded = false;

  static const _heights = {
    'small': 180.0,
    'medium': 320.0,
    'large': 480.0,
    'tall': 560.0,
  };

  /// The daemon resolves `entry` at emit time. A proxy-path entry
  /// (`/api/space/apps/...`) is daemon-relative — absolutize it against the
  /// configured HTTP base so the webview can load it.
  String get _entry {
    final raw = '${widget.data['entry'] ?? ''}'.trim();
    if (raw.isEmpty) return '';
    if (raw.startsWith('http://') || raw.startsWith('https://')) return raw;
    if (raw.startsWith('/')) return '${ChatLinkFlow.httpBase}$raw';
    return '';
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final entry = _entry;
    final id = '${widget.data['id'] ?? widget.data['widget'] ?? ''}';
    final fallback = '${widget.data['textFallback'] ?? ''}';

    if (entry.isEmpty) {
      // No resolvable entry (fence-emitted spec or app gone) — degrade to the
      // text fallback, never a blank frame.
      return Text(
        fallback.isNotEmpty
            ? fallback
            : context.trArgs(
                'Widget {id} is unavailable — open the app in Apps to view it.',
                {'id': id}),
        style: TextStyle(color: c.textSecondary, fontSize: 13),
      );
    }

    final size = '${widget.data['size'] ?? 'medium'}';
    final height = _heights[size] ?? _heights['medium']!;
    final isDark = Theme.of(context).brightness == Brightness.dark;

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        ClipRRect(
          borderRadius: BorderRadius.circular(AppTokens.rMd),
          child: SizedBox(
            height: height,
            width: double.infinity,
            child: _loaded
                ? embeddedWebView(
                    entry,
                    title: id,
                    theme: isDark ? 'dark' : 'light',
                    instanceKey: 'chat-widget-$id',
                  )
                : Material(
                    color: c.surfaceAlt,
                    child: InkWell(
                      onTap: () => setState(() => _loaded = true),
                      child: Center(
                        child: Column(
                          mainAxisSize: MainAxisSize.min,
                          children: [
                            Icon(Icons.widgets_outlined,
                                size: 28, color: c.accent),
                            const SizedBox(height: AppTokens.s8),
                            Text(id,
                                style: TextStyle(
                                    color: c.textPrimary,
                                    fontSize: 13,
                                    fontWeight: FontWeight.w600)),
                            const SizedBox(height: AppTokens.s4),
                            Text(context.tr('Click to load widget'),
                                style: TextStyle(
                                    color: c.textMuted, fontSize: 12)),
                          ],
                        ),
                      ),
                    ),
                  ),
          ),
        ),
        if (fallback.isNotEmpty)
          Padding(
            padding: const EdgeInsets.only(top: AppTokens.s6),
            child: Text(
              fallback,
              maxLines: 2,
              overflow: TextOverflow.ellipsis,
              style: TextStyle(color: c.textMuted, fontSize: 12),
            ),
          ),
      ],
    );
  }
}
