import 'package:fl_chart/fl_chart.dart';
import 'package:flutter/material.dart';
import '../../models/usage_models.dart';
import '../../services/language_service.dart';
import '../../services/relay_manager.dart';
import '../../services/usage_api.dart';
import '../../theme/tokens.dart';
import '../../widgets/states.dart';

/// Token usage over `/api/usage/*`: what the daemon's LLM calls cost.
///
/// Read-only. Editing the per-model pricing table is daemon administration and
/// lives in the desktop/web UI, not here.
class UsageScreen extends StatefulWidget {
  const UsageScreen({super.key});

  @override
  State<UsageScreen> createState() => _UsageScreenState();
}

class _UsageScreenState extends State<UsageScreen> {
  final _api = UsageApi();

  UsageOverview? _overview;
  List<UsageDailyRow> _daily = const [];
  List<UsageBreakdownRow> _breakdown = const [];
  String? _error;
  bool _loading = true;

  int _days = 30;
  String _by = 'model';

  static const _byOptions = ['model', 'source', 'jid', 'app'];

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
      // One await each rather than a Future.wait: the relay serialises
      // requests anyway, and a partial failure should name the call that broke.
      final overview = await _api.overview();
      final daily = await _api.daily(days: _days);
      // The daemon clamps breakdown days to 1..90, so a 365-day chart window
      // must not be forwarded verbatim.
      final breakdown =
          await _api.breakdown(by: _by, days: _days > 90 ? 90 : _days);
      if (!mounted) return;
      setState(() {
        _overview = overview;
        _daily = daily;
        _breakdown = breakdown;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = e.toString();
        _loading = false;
      });
    }
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
            Text(tr('Mức dùng', 'Usage'),
                style: TextStyle(color: c.textPrimary)),
            const SizedBox(width: 8),
            AnimatedBuilder(
              animation: RelayManager(),
              builder: (_, _) =>
                  ConnectionDot(connected: RelayManager().connected),
            ),
          ],
        ),
        actions: [
          IconButton(
            tooltip: tr('Tải lại', 'Reload'),
            icon: Icon(Icons.refresh, color: c.textSecondary),
            onPressed: _loading ? null : _load,
          ),
        ],
      ),
      body: _loading
          ? const LoadingState()
          : _error != null
              ? ErrorState(message: _error!, onRetry: _load)
              : RefreshIndicator(
                  color: c.accent,
                  onRefresh: _load,
                  child: ListView(
                    padding: const EdgeInsets.fromLTRB(12, 12, 12, 32),
                    children: [
                      _overviewRow(c),
                      const SizedBox(height: 18),
                      _sectionHeader(
                        c,
                        tr('Theo ngày', 'Daily'),
                        trailing: _daysPicker(c),
                      ),
                      const SizedBox(height: 8),
                      _dailyChart(c),
                      const SizedBox(height: 22),
                      _sectionHeader(
                        c,
                        tr('Phân bổ', 'Breakdown'),
                        trailing: _byPicker(c),
                      ),
                      const SizedBox(height: 8),
                      _breakdownList(c),
                    ],
                  ),
                ),
    );
  }

  // ── overview ────────────────────────────────────────────────────────────

  Widget _overviewRow(AppColors c) {
    final o = _overview ?? const UsageOverview();
    return Row(
      children: [
        Expanded(child: _statCard(c, tr('Hôm nay', 'Today'), o.today)),
        const SizedBox(width: 8),
        Expanded(child: _statCard(c, tr('7 ngày', '7 days'), o.week)),
        const SizedBox(width: 8),
        Expanded(child: _statCard(c, tr('30 ngày', '30 days'), o.month)),
      ],
    );
  }

  Widget _statCard(AppColors c, String label, UsageTotals t) => Container(
        padding: const EdgeInsets.all(10),
        decoration: BoxDecoration(
          color: c.surface,
          borderRadius: BorderRadius.circular(AppTokens.rMd),
          border: Border.all(color: c.border),
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(label, style: TextStyle(color: c.textMuted, fontSize: 11)),
            const SizedBox(height: 6),
            Text(
              _money(t.estCostUsd),
              style: TextStyle(
                color: c.textPrimary,
                fontSize: 16,
                fontWeight: FontWeight.w700,
              ),
            ),
            const SizedBox(height: 2),
            Text(
              '${_compact(t.totalTokens)} tok · ${t.calls}',
              style: TextStyle(color: c.textSecondary, fontSize: 11),
            ),
            // Tokens on an unpriced model are real spend the cost line does
            // not include; without this, a small total reads as "cheap".
            if (t.unpricedTokens > 0) ...[
              const SizedBox(height: 4),
              Text(
                tr(
                  '${_compact(t.unpricedTokens)} chưa có giá',
                  '${_compact(t.unpricedTokens)} unpriced',
                ),
                style: const TextStyle(
                    color: AppTokens.warning, fontSize: 10),
              ),
            ],
          ],
        ),
      );

  // ── daily chart ─────────────────────────────────────────────────────────

  Widget _dailyChart(AppColors c) {
    if (_daily.isEmpty) {
      return _panel(
        c,
        SizedBox(
          height: 120,
          child: Center(
            child: Text(
              tr('Chưa có dữ liệu', 'No data yet'),
              style: TextStyle(color: c.textMuted, fontSize: 12.5),
            ),
          ),
        ),
      );
    }

    final spots = <FlSpot>[
      for (var i = 0; i < _daily.length; i++)
        FlSpot(i.toDouble(), _daily[i].totals.estCostUsd),
    ];
    final maxY = spots.fold<double>(0, (m, s) => s.y > m ? s.y : m);
    // Label every Nth day so a 90-day window does not overprint its own axis.
    final step = (_daily.length / 6).ceil().clamp(1, 999);

    return _panel(
      c,
      SizedBox(
        height: 180,
        child: LineChart(
          LineChartData(
            minY: 0,
            maxY: maxY <= 0 ? 1 : maxY * 1.15,
            lineBarsData: [
              LineChartBarData(
                spots: spots,
                color: c.accent,
                barWidth: 2.5,
                isCurved: true,
                curveSmoothness: 0.2,
                dotData: FlDotData(show: spots.length <= 24),
                belowBarData: BarAreaData(
                  show: true,
                  color: c.accent.withValues(alpha: 0.18),
                ),
              ),
            ],
            gridData: FlGridData(
              show: true,
              drawVerticalLine: false,
              getDrawingHorizontalLine: (_) =>
                  FlLine(color: c.border, strokeWidth: 1),
            ),
            borderData: FlBorderData(show: false),
            titlesData: FlTitlesData(
              show: true,
              topTitles: const AxisTitles(
                  sideTitles: SideTitles(showTitles: false)),
              rightTitles: const AxisTitles(
                  sideTitles: SideTitles(showTitles: false)),
              leftTitles: AxisTitles(
                sideTitles: SideTitles(
                  showTitles: true,
                  reservedSize: 44,
                  getTitlesWidget: (v, meta) => SideTitleWidget(
                    meta: meta,
                    child: Text(
                      _money(v),
                      style: TextStyle(color: c.textMuted, fontSize: 9.5),
                    ),
                  ),
                ),
              ),
              bottomTitles: AxisTitles(
                sideTitles: SideTitles(
                  showTitles: true,
                  reservedSize: 24,
                  interval: 1,
                  getTitlesWidget: (v, meta) {
                    final i = v.round();
                    if (i < 0 || i >= _daily.length || i % step != 0) {
                      return const SizedBox.shrink();
                    }
                    // "2026-08-21" → "08-21"; the year never varies inside one
                    // window and costs a third of the label width.
                    final d = _daily[i].date;
                    return SideTitleWidget(
                      meta: meta,
                      child: Text(
                        d.length >= 10 ? d.substring(5) : d,
                        style: TextStyle(color: c.textMuted, fontSize: 9.5),
                      ),
                    );
                  },
                ),
              ),
            ),
            lineTouchData: LineTouchData(
              touchTooltipData: LineTouchTooltipData(
                getTooltipColor: (_) => c.surfaceAlt,
                getTooltipItems: (touched) => [
                  for (final s in touched)
                    LineTooltipItem(
                      '${_daily[s.x.round().clamp(0, _daily.length - 1)].date}\n'
                      '${_money(s.y)}',
                      TextStyle(
                        color: c.textPrimary,
                        fontSize: 11,
                        fontWeight: FontWeight.w600,
                      ),
                    ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }

  // ── breakdown ───────────────────────────────────────────────────────────

  Widget _breakdownList(AppColors c) {
    if (_breakdown.isEmpty) {
      return _panel(
        c,
        Padding(
          padding: const EdgeInsets.symmetric(vertical: 18),
          child: Center(
            child: Text(
              tr('Chưa có dữ liệu', 'No data yet'),
              style: TextStyle(color: c.textMuted, fontSize: 12.5),
            ),
          ),
        ),
      );
    }
    final max = _breakdown.fold<double>(
        0, (m, r) => r.totals.estCostUsd > m ? r.totals.estCostUsd : m);
    return _panel(
      c,
      Column(
        children: [
          for (final r in _breakdown) ...[
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 7),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(
                    children: [
                      Expanded(
                        child: Text(
                          r.key.isEmpty ? '—' : r.key,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style:
                              TextStyle(color: c.textPrimary, fontSize: 12.5),
                        ),
                      ),
                      const SizedBox(width: 8),
                      Text(
                        _money(r.totals.estCostUsd),
                        style: TextStyle(
                          color: c.textPrimary,
                          fontSize: 12.5,
                          fontWeight: FontWeight.w600,
                        ),
                      ),
                    ],
                  ),
                  const SizedBox(height: 5),
                  ClipRRect(
                    borderRadius: BorderRadius.circular(3),
                    child: LinearProgressIndicator(
                      value: max <= 0 ? 0 : r.totals.estCostUsd / max,
                      minHeight: 4,
                      backgroundColor: c.surfaceAlt,
                      valueColor: AlwaysStoppedAnimation<Color>(c.accent),
                    ),
                  ),
                  const SizedBox(height: 4),
                  Text(
                    '${_compact(r.totals.totalTokens)} tok · '
                    '${r.totals.calls} ${tr("lượt", "calls")}',
                    style: TextStyle(color: c.textMuted, fontSize: 11),
                  ),
                ],
              ),
            ),
          ],
        ],
      ),
    );
  }

  // ── chrome ──────────────────────────────────────────────────────────────

  Widget _panel(AppColors c, Widget child) => Container(
        padding: const EdgeInsets.all(12),
        decoration: BoxDecoration(
          color: c.surface,
          borderRadius: BorderRadius.circular(AppTokens.rMd),
          border: Border.all(color: c.border),
        ),
        child: child,
      );

  Widget _sectionHeader(AppColors c, String title, {Widget? trailing}) => Row(
        children: [
          Text(
            title.toUpperCase(),
            style: TextStyle(
              color: c.textMuted,
              fontSize: 11,
              fontWeight: FontWeight.w600,
              letterSpacing: 0.6,
            ),
          ),
          const Spacer(),
          ?trailing,
        ],
      );

  Widget _daysPicker(AppColors c) => _segmented(
        c,
        options: const [7, 30, 90],
        labelOf: (d) => '${d}d',
        selected: _days,
        onSelect: (d) {
          setState(() => _days = d);
          _load();
        },
      );

  Widget _byPicker(AppColors c) => _segmented(
        c,
        options: _byOptions,
        labelOf: (v) => switch (v) {
          'model' => tr('Mô hình', 'Model'),
          'source' => tr('Nguồn', 'Source'),
          'jid' => tr('Nhóm', 'Chat'),
          _ => 'App',
        },
        selected: _by,
        onSelect: (v) {
          setState(() => _by = v);
          _load();
        },
      );

  Widget _segmented<T>(
    AppColors c, {
    required List<T> options,
    required String Function(T) labelOf,
    required T selected,
    required ValueChanged<T> onSelect,
  }) =>
      Container(
        decoration: BoxDecoration(
          color: c.surfaceAlt,
          borderRadius: BorderRadius.circular(AppTokens.rMd),
          border: Border.all(color: c.border),
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            for (final o in options)
              GestureDetector(
                onTap: () => onSelect(o),
                child: Container(
                  padding:
                      const EdgeInsets.symmetric(horizontal: 10, vertical: 5),
                  decoration: BoxDecoration(
                    color: o == selected ? c.accentSoft : Colors.transparent,
                    borderRadius: BorderRadius.circular(AppTokens.rSm),
                  ),
                  child: Text(
                    labelOf(o),
                    style: TextStyle(
                      color: o == selected ? c.accent : c.textMuted,
                      fontSize: 11.5,
                      fontWeight:
                          o == selected ? FontWeight.w600 : FontWeight.w400,
                    ),
                  ),
                ),
              ),
          ],
        ),
      );
}

/// `$0.0421` under a cent, `$4.21` above — a four-decimal figure is unreadable
/// at scale and a two-decimal one rounds a real day of usage to `$0.00`.
String _money(double usd) {
  if (usd <= 0) return r'$0';
  if (usd < 0.01) return '\$${usd.toStringAsFixed(4)}';
  if (usd < 1) return '\$${usd.toStringAsFixed(3)}';
  return '\$${usd.toStringAsFixed(2)}';
}

String _compact(int n) {
  if (n >= 1000000) return '${(n / 1000000).toStringAsFixed(1)}M';
  if (n >= 1000) return '${(n / 1000).toStringAsFixed(1)}K';
  return '$n';
}
