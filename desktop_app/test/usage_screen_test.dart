import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:senclaw_desktop/features/usage/usage_screen.dart';
import 'package:senclaw_desktop/theme/app_theme.dart';

/// Two days of traffic shaped like the real payload — a large `in`, a small
/// `out`, and unpriced tokens so the cost card renders its "n/a" + footnote
/// branch (the case that used to make that card taller than its neighbours).
Map<String, dynamic> _row(String key, int inTok, int outTok) => {
      'date': '2026-08-0$key',
      'key': key,
      'calls': 12,
      'inputTokens': inTok,
      'outputTokens': outTok,
      'cacheCreationTokens': 0,
      'cacheReadTokens': 0,
      'estCostUsd': 0,
      'unpricedTokens': inTok,
    };

List<Override> _overrides() => [
      usageOverviewProvider.overrideWith((ref) async => {
            'today': UsageTotals.fromJson(_row('1', 2400000, 27200)),
            'week': const UsageTotals(),
            'month': const UsageTotals(),
          }),
      usageDailyProvider
          .overrideWith((ref) async => [_row('1', 1700000, 23400),
                _row('2', 1900000, 27200)]),
      usageBreakdownProvider.overrideWith(
          (ref, by) async => [_row('ag/gemini-pro-agent', 3700000, 86400),
                _row('mlx-community/Qwen2.5-0.5B-Instruct-4bit', 240400, 64)]),
      usagePricingProvider.overrideWith((ref) async => const [
            PricingRow(
                model: 'claude-fable-5',
                inputPer1m: 10,
                outputPer1m: 50,
                cacheReadPer1m: 1,
                cacheWritePer1m: 12.5),
            PricingRow(
                model: 'claude-haiku-4-5', inputPer1m: 1, outputPer1m: 5),
          ]),
    ];

Future<void> _pump(WidgetTester tester, Size size) async {
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);

  await tester.pumpWidget(ProviderScope(
    overrides: _overrides(),
    child: MaterialApp(
      theme: AppTheme.light(),
      home: const Scaffold(body: UsageScreen()),
    ),
  ));
  await tester.pumpAndSettle();
}

void main() {
  // The page has to survive both a wide desktop window and a narrow one: the
  // stat grid reflows 4 → 2 → 1 and the breakdown pair stacks under 760px.
  for (final size in const [
    Size(1600, 1000), // ultrawide — content is capped and centered
    Size(1100, 900), // typical window
    Size(700, 900), // narrow — breakdowns stack
    Size(460, 900), // very narrow — stats go single column
  ]) {
    testWidgets('renders without overflow at ${size.width.toInt()}px',
        (tester) async {
      await _pump(tester, size);
      expect(tester.takeException(), isNull);
      expect(find.text('Token Usage'), findsOneWidget);
    });
  }

  testWidgets('stat cards in a row share one height', (tester) async {
    await _pump(tester, const Size(1600, 1000));
    // The cost card carries a footnote; the others do not. They must still
    // agree on height, or the row of headline numbers goes ragged.
    // `n/a` also appears in the breakdown Cost column — the stat card is the
    // first one in tree order.
    final tokensIn = tester.getRect(find.text('2.4M').first);
    final cost = tester.getRect(find.text('n/a').first);
    expect(cost.top, closeTo(tokensIn.top, 0.5),
        reason: 'headline numbers must share a baseline across the row');

    final cacheShare = tester.getRect(find.text('0%'));
    expect(cacheShare.top, closeTo(tokensIn.top, 0.5));
  });

  testWidgets('token axis labels drop the trailing .0', (tester) async {
    await _pump(tester, const Size(1600, 1000));
    // `500.0k` / `1.0M` were the old formatter's output on the y axis.
    expect(find.textContaining('.0k'), findsNothing);
    expect(find.textContaining('.0M'), findsNothing);
  });

  testWidgets('cost with unpriced volume reads n/a, never \$0',
      (tester) async {
    await _pump(tester, const Size(1600, 1000));
    expect(find.text('n/a'), findsWidgets);
    expect(find.textContaining('+2.4M tokens'), findsOneWidget);
  });
}
