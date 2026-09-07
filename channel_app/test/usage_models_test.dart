import 'package:channel_app/models/usage_models.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  group('UsageTotals', () {
    test('parses camelCase and sums every token bucket', () {
      final t = UsageTotals.fromJson({
        'calls': 12,
        'inputTokens': 1000,
        'outputTokens': 500,
        'cacheCreationTokens': 200,
        'cacheReadTokens': 300,
        'estCostUsd': 0.0421,
        'unpricedTokens': 50,
      });
      expect(t.calls, 12);
      expect(t.totalTokens, 2000);
      expect(t.estCostUsd, closeTo(0.0421, 1e-9));
      expect(t.unpricedTokens, 50);
    });

    test('an empty object is zeroed, not null', () {
      final t = UsageTotals.fromJson(const {});
      expect(t.calls, 0);
      expect(t.totalTokens, 0);
      expect(t.estCostUsd, 0);
    });
  });

  test('UsageOverview picks all three windows', () {
    final o = UsageOverview.fromJson({
      'today': {'calls': 1},
      'week': {'calls': 7},
      'month': {'calls': 30},
    });
    expect(o.today.calls, 1);
    expect(o.week.calls, 7);
    expect(o.month.calls, 30);
  });

  test('a missing window degrades to zeros instead of throwing', () {
    final o = UsageOverview.fromJson({'today': {'calls': 1}});
    expect(o.today.calls, 1);
    expect(o.month.calls, 0);
  });

  test('daily and breakdown rows read totals flattened beside their key', () {
    // The daemon `#[serde(flatten)]`s UsageTotals, so `date`/`key` sit in the
    // same object as `inputTokens` — not under a nested `totals`.
    final d = UsageDailyRow.fromJson({'date': '2026-08-21', 'inputTokens': 10});
    expect(d.date, '2026-08-21');
    expect(d.totals.inputTokens, 10);

    final b = UsageBreakdownRow.fromJson({'key': 'claude-opus-5', 'calls': 3});
    expect(b.key, 'claude-opus-5');
    expect(b.totals.calls, 3);
  });
}
