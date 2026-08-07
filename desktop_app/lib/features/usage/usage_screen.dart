import 'dart:math' as math;

import 'package:fl_chart/fl_chart.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/i18n/l10n.dart';
import '../../core/transport/connection.dart';
import '../../theme/tokens.dart';
import '../../widgets/section_scaffold.dart';

/// Token accounting admin — mirrors the web `/usage` page: overview cards,
/// 30-day chart, per-model / per-app breakdowns, and the `model_pricing`
/// editor that turns raw token counts into $ figures.
///
/// Data comes from the daemon's `/api/usage/*`; a daemon older than the
/// token-accounting feature 404s into the SPA fallback, which the providers
/// surface as empty data (the UI then shows its zero/empty states).

// Chart series pair validated for the dark surface (CVD ΔE 19.9):
// cyan = tokens in, amber = tokens out. Mid-lightness → legible on light too.
const _cIn = Color(0xFF3D9AC7);
const _cOut = Color(0xFFBA7A35);

// ── data ───────────────────────────────────────────────────────────────────

class UsageTotals {
  final int calls;
  final int inputTokens;
  final int outputTokens;
  final int cacheCreationTokens;
  final int cacheReadTokens;
  final double estCostUsd;
  final int unpricedTokens;

  const UsageTotals({
    this.calls = 0,
    this.inputTokens = 0,
    this.outputTokens = 0,
    this.cacheCreationTokens = 0,
    this.cacheReadTokens = 0,
    this.estCostUsd = 0,
    this.unpricedTokens = 0,
  });

  factory UsageTotals.fromJson(Map<String, dynamic> j) => UsageTotals(
        calls: (j['calls'] as num?)?.toInt() ?? 0,
        inputTokens: (j['inputTokens'] as num?)?.toInt() ?? 0,
        outputTokens: (j['outputTokens'] as num?)?.toInt() ?? 0,
        cacheCreationTokens: (j['cacheCreationTokens'] as num?)?.toInt() ?? 0,
        cacheReadTokens: (j['cacheReadTokens'] as num?)?.toInt() ?? 0,
        estCostUsd: (j['estCostUsd'] as num?)?.toDouble() ?? 0,
        unpricedTokens: (j['unpricedTokens'] as num?)?.toInt() ?? 0,
      );

  /// Total billed input: prompt + cache writes + cache reads.
  int get totalIn => inputTokens + cacheCreationTokens + cacheReadTokens;
}

class PricingRow {
  final String model;
  final double inputPer1m;
  final double outputPer1m;
  final double? cacheReadPer1m;
  final double? cacheWritePer1m;

  const PricingRow({
    required this.model,
    required this.inputPer1m,
    required this.outputPer1m,
    this.cacheReadPer1m,
    this.cacheWritePer1m,
  });

  factory PricingRow.fromJson(Map<String, dynamic> j) => PricingRow(
        model: j['model'] as String? ?? '',
        inputPer1m: (j['inputPer1m'] as num?)?.toDouble() ?? 0,
        outputPer1m: (j['outputPer1m'] as num?)?.toDouble() ?? 0,
        cacheReadPer1m: (j['cacheReadPer1m'] as num?)?.toDouble(),
        cacheWritePer1m: (j['cacheWritePer1m'] as num?)?.toDouble(),
      );
}

Map<String, dynamic> _asMap(dynamic v) =>
    v is Map<String, dynamic> ? v : const {};

List<Map<String, dynamic>> _asRows(dynamic v) =>
    (v is Map ? v['rows'] : null) is List
        ? (v['rows'] as List).whereType<Map<String, dynamic>>().toList()
        : const [];

final usageOverviewProvider =
    FutureProvider.autoDispose<Map<String, UsageTotals>>((ref) async {
  final r = await ref.read(apiClientProvider).get('/api/usage/overview');
  final m = _asMap(r);
  return {
    for (final k in ['today', 'week', 'month'])
      k: UsageTotals.fromJson(_asMap(m[k])),
  };
});

/// Daily rollup rows, oldest first: `[{date, ...totals}]`.
final usageDailyProvider =
    FutureProvider.autoDispose<List<Map<String, dynamic>>>((ref) async {
  final r = await ref
      .read(apiClientProvider)
      .get('/api/usage/daily', query: {'days': 30});
  return _asRows(r);
});

/// Breakdown rows for one dimension (`model` / `app`), 7 days.
final usageBreakdownProvider = FutureProvider.autoDispose
    .family<List<Map<String, dynamic>>, String>((ref, by) async {
  final r = await ref
      .read(apiClientProvider)
      .get('/api/usage/breakdown', query: {'by': by, 'days': 7});
  return _asRows(r);
});

final usagePricingProvider =
    FutureProvider.autoDispose<List<PricingRow>>((ref) async {
  final r = await ref.read(apiClientProvider).get('/api/usage/pricing');
  return _asRows(r).map(PricingRow.fromJson).toList();
});

// ── formatting ─────────────────────────────────────────────────────────────

/// Compact token count. Drops a trailing `.0` so axis ticks read `500k`, not
/// `500.0k`, and never leaks a double's `.0` for small values (`0.0` → `0`).
String _fmtTokens(num n) {
  String trim(double v) {
    final s = v.toStringAsFixed(1);
    return s.endsWith('.0') ? s.substring(0, s.length - 2) : s;
  }

  if (n >= 1000000) return '${trim(n / 1000000)}M';
  if (n >= 1000) return '${trim(n / 1000)}k';
  return '${n.round()}';
}

/// Lining figures so digits stack in a column across table rows.
const _figures = [FontFeature.tabularFigures()];

/// Round `raw` up to the next 1/2/5 × 10ⁿ so grid lines land on readable
/// numbers (200k, 500k, 1M) instead of whatever the data happened to peak at.
double _niceStep(double raw) {
  if (!raw.isFinite || raw <= 0) return 1;
  final mag = math.pow(10, (math.log(raw) / math.ln10).floor()).toDouble();
  final norm = raw / mag;
  final mult = norm <= 1
      ? 1.0
      : norm <= 2
          ? 2.0
          : norm <= 5
              ? 5.0
              : 10.0;
  return mult * mag;
}

/// Cost label that never fakes $0: zero priced volume with some unpriced
/// volume reads "n/a".
String _fmtCost(UsageTotals? t) {
  if (t == null) return '—';
  if (t.estCostUsd == 0 && t.unpricedTokens > 0) return 'n/a';
  return '\$${t.estCostUsd.toStringAsFixed(t.estCostUsd >= 10 ? 2 : 3)}';
}

// ── screen ─────────────────────────────────────────────────────────────────

class UsageScreen extends ConsumerWidget {
  const UsageScreen({super.key});

  void _refresh(WidgetRef ref) {
    ref.invalidate(usageOverviewProvider);
    ref.invalidate(usageDailyProvider);
    ref.invalidate(usageBreakdownProvider);
    ref.invalidate(usagePricingProvider);
  }

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final overview = ref.watch(usageOverviewProvider).valueOrNull;
    final today = overview?['today'];
    final cacheShare = (today != null && today.totalIn > 0)
        ? (today.cacheReadTokens * 100 / today.totalIn).round()
        : 0;

    return SectionScaffold(
      title: context.tr('Token Usage'),
      subtitle: context.tr('Token in/out and estimated cost — agents, '
          'Space Apps, cognitive, embeddings'),
      actions: [
        OutlinedButton.icon(
          onPressed: () => _refresh(ref),
          icon: const Icon(Icons.refresh, size: 16),
          label: Text(context.tr('Refresh')),
        ),
      ],
      body: ListView(
        padding: const EdgeInsets.all(AppTokens.s24),
        children: [
          // Cap the measure: past ~1200px the tables turn into a thin ribbon of
          // text stranded between two oceans of empty surface.
          Center(
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: _maxContentWidth),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  _StatGrid(children: [
                    _UsageStat(
                      label: context.tr('Tokens in (today)'),
                      value: today == null ? '—' : _fmtTokens(today.totalIn),
                      icon: Icons.south_west,
                      accent: _cIn,
                    ),
                    _UsageStat(
                      label: context.tr('Tokens out (today)'),
                      value:
                          today == null ? '—' : _fmtTokens(today.outputTokens),
                      icon: Icons.north_east,
                      accent: _cOut,
                    ),
                    _UsageStat(
                      label: context.tr('Est. cost (today)'),
                      value: _fmtCost(today),
                      icon: Icons.attach_money,
                      accent: AppTokens.brand,
                      footnote: (today != null && today.unpricedTokens > 0)
                          ? context.trArgs('+{n} tokens unpriced',
                              {'n': _fmtTokens(today.unpricedTokens)})
                          : null,
                    ),
                    _UsageStat(
                      label: context.tr('Cache-read (today)'),
                      value: '$cacheShare%',
                      icon: Icons.storage_outlined,
                      accent: AppTokens.cyan,
                    ),
                  ]),
                  const SizedBox(height: AppTokens.s16),
                  const _DailyChartCard(),
                  const SizedBox(height: AppTokens.s16),
                  LayoutBuilder(builder: (context, cns) {
                    const gap =
                        SizedBox(width: AppTokens.s16, height: AppTokens.s16);
                    final model = _BreakdownCard(
                        title: context.tr('By model — 7 days'),
                        by: 'model',
                        keyLabel: context.tr('Model'),
                        mono: true);
                    final app = _BreakdownCard(
                        title: context.tr('By Space App — 7 days'),
                        by: 'app',
                        keyLabel: context.tr('App'));
                    if (cns.maxWidth < 760) {
                      return Column(children: [model, gap, app]);
                    }
                    // Stretch so the two cards share a baseline even when one
                    // has fewer rows than the other.
                    return IntrinsicHeight(
                      child: Row(
                        crossAxisAlignment: CrossAxisAlignment.stretch,
                        children: [
                          Expanded(child: model),
                          gap,
                          Expanded(child: app),
                        ],
                      ),
                    );
                  }),
                  const SizedBox(height: AppTokens.s16),
                  const _PricingCard(),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }
}

const double _maxContentWidth = 1200;

/// Equal-width, equal-height stat cards that reflow 4 → 2 → 1 across.
/// A `Wrap` of fixed-width cards left a wide window mostly empty and let the
/// card with a footnote grow taller than its neighbours.
class _StatGrid extends StatelessWidget {
  const _StatGrid({required this.children});
  final List<Widget> children;

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(builder: (context, cns) {
      final perRow = cns.maxWidth >= 940
          ? 4
          : cns.maxWidth >= 520
              ? 2
              : 1;
      final rows = <Widget>[];
      for (var i = 0; i < children.length; i += perRow) {
        final slice = children.skip(i).take(perRow).toList();
        rows.add(IntrinsicHeight(
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              for (var j = 0; j < perRow; j++) ...[
                if (j > 0) const SizedBox(width: AppTokens.s16),
                Expanded(
                    child: j < slice.length ? slice[j] : const SizedBox()),
              ],
            ],
          ),
        ));
      }
      return Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          for (var i = 0; i < rows.length; i++) ...[
            if (i > 0) const SizedBox(height: AppTokens.s16),
            rows[i],
          ],
        ],
      );
    });
  }
}

// ── stat card ──────────────────────────────────────────────────────────────

class _UsageStat extends StatelessWidget {
  const _UsageStat({
    required this.label,
    required this.value,
    required this.icon,
    required this.accent,
    this.footnote,
  });

  final String label;
  final String value;
  final IconData icon;
  final Color accent;
  final String? footnote;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Container(
      padding: const EdgeInsets.all(AppTokens.s16),
      decoration: BoxDecoration(
        color: c.surface,
        border: Border.all(color: c.border),
        borderRadius: BorderRadius.circular(AppTokens.rLg),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(children: [
            Container(
              padding: const EdgeInsets.all(AppTokens.s6),
              decoration: BoxDecoration(
                color: accent.withValues(alpha: 0.14),
                borderRadius: BorderRadius.circular(AppTokens.rMd),
              ),
              child: Icon(icon, color: accent, size: 16),
            ),
            const SizedBox(width: AppTokens.s8),
            Expanded(
              child: Text(label,
                  style: TextStyle(color: c.textMuted, fontSize: 12),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis),
            ),
          ]),
          const SizedBox(height: AppTokens.s12),
          FittedBox(
            fit: BoxFit.scaleDown,
            alignment: Alignment.centerLeft,
            child: Text(value,
                style: TextStyle(
                    color: c.textPrimary,
                    fontSize: 26,
                    height: 1.1,
                    letterSpacing: -0.5,
                    fontWeight: FontWeight.w600,
                    fontFeatures: _figures)),
          ),
          // Always reserve the footnote line: without it the one card that has
          // a footnote grows taller and the row of numbers goes ragged.
          const SizedBox(height: AppTokens.s4),
          SizedBox(
            height: 14,
            child: footnote == null
                ? null
                : Text(footnote!,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(color: c.textMuted, fontSize: 11)),
          ),
        ],
      ),
    );
  }
}

// ── daily chart ────────────────────────────────────────────────────────────

class _DailyChartCard extends ConsumerWidget {
  const _DailyChartCard();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final async = ref.watch(usageDailyProvider);
    final rows = async.valueOrNull ?? const [];

    Widget child;
    if (async.isLoading && rows.isEmpty) {
      child = const _PanelPlaceholder(height: 220, spinner: true);
    } else if (rows.isEmpty) {
      child = _PanelPlaceholder(
        height: 220,
        message: context.tr(
            'No data yet — numbers appear after the first LLM calls.'),
      );
    } else {
      final inSpots = <FlSpot>[];
      final outSpots = <FlSpot>[];
      var peak = 0.0;
      for (var i = 0; i < rows.length; i++) {
        final t = UsageTotals.fromJson(rows[i]);
        inSpots.add(FlSpot(i.toDouble(), t.totalIn.toDouble()));
        outSpots.add(FlSpot(i.toDouble(), t.outputTokens.toDouble()));
        peak = [peak, t.totalIn.toDouble(), t.outputTokens.toDouble()]
            .reduce((a, b) => a > b ? a : b);
      }
      String dateAt(int i) {
        final d = rows[i]['date'] as String? ?? '';
        return d.length >= 10 ? d.substring(5) : d; // MM-DD
      }

      // Anchor the axis at zero on a round step. Letting fl_chart pick both
      // ends put the baseline at the smallest sample, so the bottom tick read
      // a meaningless "23.4k" and the two series looked like they shared a
      // floor they don't share.
      final step = _niceStep(peak / 4);
      final maxY = peak <= 0 ? step * 4 : (peak / step).ceilToDouble() * step;

      LineChartBarData bar(List<FlSpot> spots, Color color) =>
          LineChartBarData(
            spots: spots,
            color: color,
            barWidth: 2,
            isCurved: true,
            curveSmoothness: 0.2,
            preventCurveOverShooting: true,
            dotData: FlDotData(show: spots.length <= 24),
            belowBarData: BarAreaData(
              show: true,
              gradient: LinearGradient(
                begin: Alignment.topCenter,
                end: Alignment.bottomCenter,
                colors: [
                  color.withValues(alpha: 0.18),
                  color.withValues(alpha: 0.01),
                ],
              ),
            ),
          );

      child = SizedBox(
        height: 240,
        child: LineChart(
          LineChartData(
            minY: 0,
            maxY: maxY,
            lineBarsData: [bar(inSpots, _cIn), bar(outSpots, _cOut)],
            gridData: FlGridData(
              show: true,
              drawVerticalLine: false,
              horizontalInterval: step,
              getDrawingHorizontalLine: (_) =>
                  FlLine(color: c.border, strokeWidth: 1),
            ),
            borderData: FlBorderData(show: false),
            titlesData: FlTitlesData(
              topTitles:
                  const AxisTitles(sideTitles: SideTitles(showTitles: false)),
              rightTitles:
                  const AxisTitles(sideTitles: SideTitles(showTitles: false)),
              leftTitles: AxisTitles(
                sideTitles: SideTitles(
                  showTitles: true,
                  reservedSize: 46,
                  interval: step,
                  getTitlesWidget: (v, _) => Padding(
                    padding: const EdgeInsets.only(right: AppTokens.s8),
                    child: Text(_fmtTokens(v),
                        textAlign: TextAlign.right,
                        style: TextStyle(
                            color: c.textMuted,
                            fontSize: 10,
                            fontFeatures: _figures)),
                  ),
                ),
              ),
              bottomTitles: AxisTitles(
                sideTitles: SideTitles(
                  showTitles: true,
                  reservedSize: 24,
                  interval: (rows.length / 6).ceilToDouble().clamp(1, 30),
                  getTitlesWidget: (v, _) {
                    final i = v.toInt();
                    if (i < 0 || i >= rows.length) return const SizedBox();
                    return Padding(
                      padding: const EdgeInsets.only(top: AppTokens.s8),
                      child: Text(dateAt(i),
                          style: TextStyle(
                              color: c.textMuted,
                              fontSize: 10,
                              fontFeatures: _figures)),
                    );
                  },
                ),
              ),
            ),
            lineTouchData: LineTouchData(
              getTouchedSpotIndicator: (bar, indexes) => [
                for (final _ in indexes)
                  TouchedSpotIndicatorData(
                    FlLine(color: c.borderStrong, strokeWidth: 1),
                    FlDotData(show: true),
                  ),
              ],
              touchTooltipData: LineTouchTooltipData(
                getTooltipColor: (_) => c.surfaceAlt,
                tooltipBorderRadius:
                    BorderRadius.circular(AppTokens.rMd),
                tooltipPadding: const EdgeInsets.symmetric(
                    horizontal: AppTokens.s12, vertical: AppTokens.s8),
                getTooltipItems: (spots) => [
                  for (var k = 0; k < spots.length; k++)
                    LineTooltipItem(
                      // Date once, on the first row of the tooltip.
                      k == 0 ? '${dateAt(spots[k].x.toInt())}\n' : '',
                      TextStyle(
                          color: c.textMuted,
                          fontSize: 10,
                          fontWeight: FontWeight.w400),
                      children: [
                        TextSpan(
                          text:
                              '${context.tr(spots[k].barIndex == 0 ? 'In' : 'Out')} '
                              '${_fmtTokens(spots[k].y)}',
                          style: TextStyle(
                              color: spots[k].barIndex == 0 ? _cIn : _cOut,
                              fontSize: 11,
                              fontWeight: FontWeight.w600),
                        ),
                      ],
                    ),
                ],
              ),
            ),
          ),
        ),
      );
    }

    return _Panel(
      title: context.tr('Tokens per day — 30 days'),
      trailing: Row(mainAxisSize: MainAxisSize.min, children: [
        _legendDot(_cIn, context.tr('Tokens in'), c.textMuted),
        const SizedBox(width: AppTokens.s12),
        _legendDot(_cOut, context.tr('Tokens out'), c.textMuted),
      ]),
      child: child,
    );
  }

  Widget _legendDot(Color color, String label, Color ink) =>
      Row(mainAxisSize: MainAxisSize.min, children: [
        Container(
            width: 8,
            height: 8,
            decoration: BoxDecoration(color: color, shape: BoxShape.circle)),
        const SizedBox(width: AppTokens.s4),
        Text(label, style: TextStyle(color: ink, fontSize: 11)),
      ]);
}

// ── breakdown card ─────────────────────────────────────────────────────────

class _BreakdownCard extends ConsumerWidget {
  const _BreakdownCard({
    required this.title,
    required this.by,
    required this.keyLabel,
    this.mono = false,
  });

  final String title;
  final String by;
  final String keyLabel;

  /// Model ids are machine strings — mono keeps the shared `mlx-community/`
  /// prefixes visually aligned so the differing tail stands out.
  final bool mono;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final async = ref.watch(usageBreakdownProvider(by));
    final rows = async.valueOrNull ?? const [];
    final visible =
        rows.where((r) => (r['key'] as String? ?? '') != '').take(6).toList();

    // Largest input volume in view — the share bars are relative to it, so the
    // top row always fills the track and the rest read as a fraction of it.
    final peak = visible.fold<int>(
        0, (m, r) => math.max(m, UsageTotals.fromJson(r).totalIn));

    final head = TextStyle(
        color: c.textMuted, fontSize: 11, fontWeight: FontWeight.w500);
    final cell =
        TextStyle(color: c.textPrimary, fontSize: 12, fontFeatures: _figures);
    final keyStyle = TextStyle(
        color: c.textPrimary,
        fontSize: 12,
        fontFamily: mono ? AppTokens.fontMono : null);

    Widget body;
    if (async.isLoading && visible.isEmpty) {
      body = const _PanelPlaceholder(height: 132, spinner: true);
    } else if (visible.isEmpty) {
      body = _PanelPlaceholder(
          height: 132, message: context.tr('No data yet'));
    } else {
      body = Table(
        columnWidths: const {
          0: FlexColumnWidth(),
          1: IntrinsicColumnWidth(),
          2: IntrinsicColumnWidth(),
          3: IntrinsicColumnWidth(),
          4: IntrinsicColumnWidth(),
        },
        border: TableBorder(
            horizontalInside: BorderSide(color: c.border, width: 1)),
        defaultVerticalAlignment: TableCellVerticalAlignment.middle,
        children: [
          TableRow(children: [
            _pad(Text(keyLabel, style: head)),
            _num(context.tr('Calls'), head),
            _num(context.tr('In'), head),
            _num(context.tr('Out'), head),
            _num(context.tr('Cost'), head),
          ]),
          for (final r in visible)
            TableRow(children: [
              _pad(_keyCell(context, r, keyStyle, peak)),
              _num('${(r['calls'] as num?) ?? 0}', cell),
              _num(_fmtTokens(UsageTotals.fromJson(r).totalIn), cell),
              _num(_fmtTokens((r['outputTokens'] as num?) ?? 0), cell),
              _num(_fmtCost(UsageTotals.fromJson(r)), cell),
            ]),
        ],
      );
    }
    return _Panel(title: title, child: body);
  }

  /// Name plus a hairline share bar — turns a column of numbers into a shape
  /// you can rank at a glance.
  Widget _keyCell(BuildContext context, Map<String, dynamic> r,
      TextStyle style, int peak) {
    final c = context.colors;
    final share =
        peak <= 0 ? 0.0 : UsageTotals.fromJson(r).totalIn / peak;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(r['key'] as String? ?? '',
            style: style, maxLines: 1, overflow: TextOverflow.ellipsis),
        const SizedBox(height: AppTokens.s4),
        ClipRRect(
          borderRadius: BorderRadius.circular(AppTokens.rFull),
          child: LinearProgressIndicator(
            value: share.clamp(0.0, 1.0),
            minHeight: 3,
            backgroundColor: c.border,
            valueColor: AlwaysStoppedAnimation(_cIn.withValues(alpha: 0.55)),
          ),
        ),
      ],
    );
  }

  Widget _pad(Widget child) => Padding(
        padding: const EdgeInsets.symmetric(vertical: AppTokens.s8),
        child: Padding(
            padding: const EdgeInsets.only(right: AppTokens.s12),
            child: child),
      );

  Widget _num(String s, TextStyle style) => Padding(
        padding: const EdgeInsets.only(left: AppTokens.s16, top: AppTokens.s8,
            bottom: AppTokens.s8),
        child: Text(s, style: style, textAlign: TextAlign.right),
      );
}

// ── pricing editor ─────────────────────────────────────────────────────────

class _PricingCard extends ConsumerWidget {
  const _PricingCard();

  Future<void> _delete(
      BuildContext context, WidgetRef ref, PricingRow row) async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text(ctx.tr('Delete model pricing')),
        content: Text(ctx.trArgs(
            'Drop the price list for "{model}"? Tokens for this model are '
            'then counted as "unpriced" (not \$0).',
            {'model': row.model})),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(ctx, false),
              child: Text(ctx.tr('Cancel'))),
          FilledButton(
              onPressed: () => Navigator.pop(ctx, true),
              child: Text(ctx.tr('Delete'))),
        ],
      ),
    );
    if (ok != true) return;
    try {
      await ref
          .read(apiClientProvider)
          .delete('/api/usage/pricing/${Uri.encodeComponent(row.model)}');
      ref.invalidate(usagePricingProvider);
      ref.invalidate(usageOverviewProvider);
    } catch (e) {
      if (context.mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
            content: Text(context.trArgs('Delete failed: {e}', {'e': e}))));
      }
    }
  }

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final async = ref.watch(usagePricingProvider);
    final rows = async.valueOrNull ?? const <PricingRow>[];

    final head = TextStyle(
        color: c.textMuted, fontSize: 11, fontWeight: FontWeight.w500);
    final cell =
        TextStyle(color: c.textPrimary, fontSize: 12, fontFeatures: _figures);
    final mono = TextStyle(
        color: c.textPrimary, fontSize: 12, fontFamily: AppTokens.fontMono);

    // Trim float artifacts (0.30000000000000004 → 0.3) and integer noise
    // (5.0 → 5) without forcing a fixed decimal count.
    String p(double? v) {
      if (v == null) return '—';
      var s = v.toStringAsFixed(6);
      s = s.replaceFirst(RegExp(r'0+$'), '').replaceFirst(RegExp(r'\.$'), '');
      return '\$$s';
    }

    return _Panel(
      title: context.tr('Model pricing (USD / 1M tokens)'),
      subtitle: context.tr('Exact id match first, then prefix. A model with '
          'no price is reported as "unpriced", never billed as \$0.'),
      trailing: OutlinedButton.icon(
        onPressed: () => showPricingDialog(context, ref),
        icon: const Icon(Icons.add, size: 14),
        label: Text(context.tr('Add model')),
      ),
      child: async.isLoading && rows.isEmpty
          ? const _PanelPlaceholder(height: 120, spinner: true)
          : rows.isEmpty
              ? _PanelPlaceholder(
                  height: 120, message: context.tr('No pricing rows yet'))
              : Table(
                  columnWidths: const {
                    0: FlexColumnWidth(),
                    1: IntrinsicColumnWidth(),
                    2: IntrinsicColumnWidth(),
                    3: IntrinsicColumnWidth(),
                    4: IntrinsicColumnWidth(),
                    5: IntrinsicColumnWidth(),
                  },
                  border: TableBorder(
                      horizontalInside: BorderSide(color: c.border, width: 1)),
                  defaultVerticalAlignment: TableCellVerticalAlignment.middle,
                  children: [
                    TableRow(children: [
                      _pad(Text(context.tr('Model (prefix match)'),
                          style: head)),
                      _num(context.tr('In'), head),
                      _num(context.tr('Out'), head),
                      _num(context.tr('Cache R'), head),
                      _num(context.tr('Cache W'), head),
                      const SizedBox(),
                    ]),
                    for (final r in rows)
                      TableRow(children: [
                        _pad(Text(r.model,
                            style: mono,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis)),
                        _num(p(r.inputPer1m), cell),
                        _num(p(r.outputPer1m), cell),
                        _num(p(r.cacheReadPer1m), cell),
                        _num(p(r.cacheWritePer1m), cell),
                        Padding(
                          padding:
                              const EdgeInsets.only(left: AppTokens.s16),
                          child: Row(mainAxisSize: MainAxisSize.min, children: [
                            _RowAction(
                              icon: Icons.edit_outlined,
                              tooltip: context.tr('Edit'),
                              color: c.textMuted,
                              onPressed: () =>
                                  showPricingDialog(context, ref, existing: r),
                            ),
                            const SizedBox(width: AppTokens.s4),
                            _RowAction(
                              icon: Icons.delete_outline,
                              tooltip: context.tr('Delete'),
                              color: AppTokens.danger,
                              onPressed: () => _delete(context, ref, r),
                            ),
                          ]),
                        ),
                      ]),
                  ],
                ),
    );
  }

  Widget _pad(Widget child) => Padding(
        padding: const EdgeInsets.only(
            top: AppTokens.s8, bottom: AppTokens.s8, right: AppTokens.s12),
        child: child,
      );

  Widget _num(String s, TextStyle style) => Padding(
        padding: const EdgeInsets.only(
            left: AppTokens.s16, top: AppTokens.s8, bottom: AppTokens.s8),
        child: Text(s, style: style, textAlign: TextAlign.right),
      );
}

/// Compact 24px row action. `IconButton`'s default 48px hit box added ~16px of
/// dead height to every pricing row.
class _RowAction extends StatelessWidget {
  const _RowAction({
    required this.icon,
    required this.tooltip,
    required this.color,
    required this.onPressed,
  });

  final IconData icon;
  final String tooltip;
  final Color color;
  final VoidCallback onPressed;

  @override
  Widget build(BuildContext context) => Tooltip(
        message: tooltip,
        child: InkWell(
          onTap: onPressed,
          borderRadius: BorderRadius.circular(AppTokens.rMd),
          child: Padding(
            padding: const EdgeInsets.all(AppTokens.s4),
            child: Icon(icon, size: 16, color: color),
          ),
        ),
      );
}

/// Shared empty / loading body for the cards, so the page keeps its shape
/// instead of collapsing and re-expanding as each provider resolves.
class _PanelPlaceholder extends StatelessWidget {
  const _PanelPlaceholder({
    required this.height,
    this.message,
    this.spinner = false,
  });

  final double height;
  final String? message;
  final bool spinner;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return SizedBox(
      height: height,
      child: Center(
        child: spinner
            ? SizedBox(
                width: 18,
                height: 18,
                child: CircularProgressIndicator(
                    strokeWidth: 2, color: c.textMuted),
              )
            : Text(message ?? '',
                textAlign: TextAlign.center,
                style: TextStyle(color: c.textMuted, fontSize: 12)),
      ),
    );
  }
}

/// Add/edit one pricing row. `existing` locks the model id (edit mode).
Future<void> showPricingDialog(BuildContext context, WidgetRef ref,
    {PricingRow? existing}) async {
  final model = TextEditingController(text: existing?.model ?? '');
  final input =
      TextEditingController(text: existing?.inputPer1m.toString() ?? '');
  final output =
      TextEditingController(text: existing?.outputPer1m.toString() ?? '');
  final cacheR =
      TextEditingController(text: existing?.cacheReadPer1m?.toString() ?? '');
  final cacheW =
      TextEditingController(text: existing?.cacheWritePer1m?.toString() ?? '');

  double? numOf(TextEditingController c) =>
      double.tryParse(c.text.trim().replaceAll(',', '.'));

  final api = ref.read(apiClientProvider);
  final saved = await showDialog<bool>(
    context: context,
    builder: (ctx) {
      String? error;
      return StatefulBuilder(builder: (ctx, setState) {
        InputDecoration dec(String label, {String? hint}) =>
            InputDecoration(labelText: label, hintText: hint, isDense: true);
        return AlertDialog(
          title: Text(ctx.tr(
              existing == null ? 'Add model pricing' : 'Edit model pricing')),
          content: SizedBox(
            width: 380,
            child: Column(mainAxisSize: MainAxisSize.min, children: [
              TextField(
                controller: model,
                enabled: existing == null,
                decoration: dec(ctx.tr('Model id (prefix match)'),
                    hint: ctx.tr('e.g. gpt-5.2')),
              ),
              const SizedBox(height: AppTokens.s12),
              Row(children: [
                Expanded(
                    child: TextField(
                        controller: input,
                        decoration: dec(ctx.tr('In \$/1M')),
                        keyboardType: TextInputType.number)),
                const SizedBox(width: AppTokens.s12),
                Expanded(
                    child: TextField(
                        controller: output,
                        decoration: dec(ctx.tr('Out \$/1M')),
                        keyboardType: TextInputType.number)),
              ]),
              const SizedBox(height: AppTokens.s12),
              Row(children: [
                Expanded(
                    child: TextField(
                        controller: cacheR,
                        decoration: dec(ctx.tr('Cache read \$/1M (optional)')),
                        keyboardType: TextInputType.number)),
                const SizedBox(width: AppTokens.s12),
                Expanded(
                    child: TextField(
                        controller: cacheW,
                        decoration: dec(ctx.tr('Cache write \$/1M (optional)')),
                        keyboardType: TextInputType.number)),
              ]),
              if (error != null) ...[
                const SizedBox(height: AppTokens.s12),
                Text(error!,
                    style: const TextStyle(
                        color: AppTokens.danger, fontSize: 12)),
              ],
            ]),
          ),
          actions: [
            TextButton(
                onPressed: () => Navigator.pop(ctx, false),
                child: Text(ctx.tr('Cancel'))),
            FilledButton(
              onPressed: () async {
                final m = model.text.trim();
                final inP = numOf(input);
                final outP = numOf(output);
                if (m.isEmpty || inP == null || outP == null) {
                  setState(() => error = ctx
                      .tr('Model id plus In and Out prices (numbers) are '
                          'required.'));
                  return;
                }
                try {
                  await api.put('/api/usage/pricing', body: {
                    'model': m,
                    'inputPer1m': inP,
                    'outputPer1m': outP,
                    'cacheReadPer1m': numOf(cacheR),
                    'cacheWritePer1m': numOf(cacheW),
                  });
                  if (ctx.mounted) Navigator.pop(ctx, true);
                } catch (e) {
                  setState(() =>
                      error = ctx.trArgs('Save failed: {e}', {'e': e}));
                }
              },
              child: Text(ctx.tr('Save')),
            ),
          ],
        );
      });
    },
  );
  if (saved == true) {
    ref.invalidate(usagePricingProvider);
    ref.invalidate(usageOverviewProvider);
    ref.invalidate(usageBreakdownProvider);
  }
}

// ── shared panel shell ─────────────────────────────────────────────────────

class _Panel extends StatelessWidget {
  const _Panel({
    required this.title,
    required this.child,
    this.subtitle,
    this.trailing,
  });

  final String title;
  final String? subtitle;
  final Widget? trailing;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Container(
      padding: const EdgeInsets.all(AppTokens.s16),
      decoration: BoxDecoration(
        color: c.surface,
        border: Border.all(color: c.border),
        borderRadius: BorderRadius.circular(AppTokens.rLg),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(children: [
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(title,
                      style: TextStyle(
                          color: c.textSecondary,
                          fontSize: 12,
                          fontWeight: FontWeight.w600)),
                  if (subtitle != null) ...[
                    const SizedBox(height: 2),
                    Text(subtitle!,
                        style:
                            TextStyle(color: c.textMuted, fontSize: 11)),
                  ],
                ],
              ),
            ),
            ?trailing,
          ]),
          const SizedBox(height: AppTokens.s12),
          child,
        ],
      ),
    );
  }
}
