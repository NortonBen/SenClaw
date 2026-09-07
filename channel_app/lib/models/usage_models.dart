/// Token usage accounting — what the daemon's LLM calls actually cost.
///
/// Backed by `llm_usage_log` / `llm_usage_daily` and a pricing table. The
/// daemon serialises these camelCase, and both the daily and the breakdown row
/// **flatten** their totals, so `date`/`key` sit beside `inputTokens` in the
/// same object rather than under a nested `totals`.
library;

class UsageTotals {
  final int calls;
  final int inputTokens;
  final int outputTokens;
  final int cacheCreationTokens;
  final int cacheReadTokens;
  final double estCostUsd;

  /// Tokens spent on a model with no pricing row. They are real spend the cost
  /// figure does **not** include — shown so an implausibly small total has a
  /// visible cause.
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

  int get totalTokens =>
      inputTokens + outputTokens + cacheCreationTokens + cacheReadTokens;

  factory UsageTotals.fromJson(Map<String, dynamic> j) => UsageTotals(
        calls: (j['calls'] as num?)?.toInt() ?? 0,
        inputTokens: (j['inputTokens'] as num?)?.toInt() ?? 0,
        outputTokens: (j['outputTokens'] as num?)?.toInt() ?? 0,
        cacheCreationTokens:
            (j['cacheCreationTokens'] as num?)?.toInt() ?? 0,
        cacheReadTokens: (j['cacheReadTokens'] as num?)?.toInt() ?? 0,
        estCostUsd: (j['estCostUsd'] as num?)?.toDouble() ?? 0,
        unpricedTokens: (j['unpricedTokens'] as num?)?.toInt() ?? 0,
      );
}

/// `GET /api/usage/overview` — three windows, all ending now.
class UsageOverview {
  final UsageTotals today;
  final UsageTotals week;
  final UsageTotals month;

  const UsageOverview({
    this.today = const UsageTotals(),
    this.week = const UsageTotals(),
    this.month = const UsageTotals(),
  });

  factory UsageOverview.fromJson(Map<String, dynamic> j) {
    UsageTotals pick(String k) => UsageTotals.fromJson(
          (j[k] as Map?)?.cast<String, dynamic>() ?? const {},
        );
    return UsageOverview(
      today: pick('today'),
      week: pick('week'),
      month: pick('month'),
    );
  }
}

/// One day of the rollup, oldest first. `date` is flattened alongside totals.
class UsageDailyRow {
  final String date;
  final UsageTotals totals;

  const UsageDailyRow({required this.date, required this.totals});

  factory UsageDailyRow.fromJson(Map<String, dynamic> j) => UsageDailyRow(
        date: (j['date'] ?? '').toString(),
        totals: UsageTotals.fromJson(j),
      );
}

/// One bucket of `GET /api/usage/breakdown?by=model|source|jid|app`.
class UsageBreakdownRow {
  final String key;
  final UsageTotals totals;

  const UsageBreakdownRow({required this.key, required this.totals});

  factory UsageBreakdownRow.fromJson(Map<String, dynamic> j) =>
      UsageBreakdownRow(
        key: (j['key'] ?? '').toString(),
        totals: UsageTotals.fromJson(j),
      );
}
