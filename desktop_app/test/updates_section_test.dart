import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:senclaw_desktop/core/prefs.dart';
import 'package:senclaw_desktop/core/update/update_provider.dart';
import 'package:senclaw_desktop/core/update/update_service.dart';
import 'package:senclaw_desktop/features/settings/settings_screen.dart';
import 'package:senclaw_desktop/theme/app_theme.dart';
import 'package:shared_preferences/shared_preferences.dart';

const _target = 'aarch64-apple-darwin';

const _manifest = '''
{
  "version": "0.3.0",
  "notes": "- feat: the new thing",
  "assets": {
    "$_target": {"name": "SenClaw-$_target.app.zip", "size": 10, "sha256": "ab"}
  }
}
''';

Future<void> pumpSection(
  WidgetTester tester, {
  required String currentVersion,
  String body = _manifest,
  bool check = true,
  Map<String, Object> prefs = const {},
}) async {
  SharedPreferences.setMockInitialValues(prefs);
  final sp = await SharedPreferences.getInstance();
  final container = ProviderContainer(overrides: [
    prefsProvider.overrideWithValue(sp),
    updateServiceProvider.overrideWithValue(UpdateService(
      client: MockClient((_) async => http.Response(body, 200)),
      currentVersion: currentVersion,
      buildTarget: _target,
    )),
  ]);
  addTearDown(container.dispose);

  if (check) await container.read(updateProvider.notifier).check();

  await tester.pumpWidget(UncontrolledProviderScope(
    container: container,
    child: MaterialApp(
      theme: AppTheme.dark(),
      home: const Scaffold(body: UpdatesSection()),
    ),
  ));
  await tester.pump();
}

void main() {
  testWidgets('renders the current version and an update offer', (tester) async {
    await pumpSection(tester, currentVersion: '0.2.0');

    expect(find.textContaining('0.2.0'), findsWidgets, reason: 'shows what you run');
    expect(find.text('Version 0.3.0 is available.'), findsOneWidget);
    expect(find.widgetWithText(FilledButton, 'Download'), findsOneWidget);
    expect(find.textContaining("What's new in 0.3.0"), findsOneWidget);
  });

  testWidgets('up-to-date offers a plain re-check, not a download', (tester) async {
    await pumpSection(tester, currentVersion: '0.3.0');

    expect(find.text('You are on the latest version.'), findsOneWidget);
    expect(find.widgetWithText(FilledButton, 'Download'), findsNothing);
    expect(find.widgetWithText(OutlinedButton, 'Check now'), findsOneWidget);
  });

  // The updater must never offer to overwrite a developer's own build.
  testWidgets('a dev build explains itself instead of offering a download',
      (tester) async {
    await pumpSection(tester, currentVersion: 'dev', check: false);

    expect(find.text('Development build'), findsOneWidget);
    expect(find.widgetWithText(FilledButton, 'Download'), findsNothing);
    expect(find.widgetWithText(OutlinedButton, 'Check now'), findsNothing);
    expect(find.textContaining('senclaw install desktop'), findsOneWidget);
  });

  testWidgets('the auto-check toggle reflects and updates state', (tester) async {
    await pumpSection(tester, currentVersion: '0.2.0');

    final sw = find.byType(Switch);
    expect(sw, findsOneWidget);
    expect(tester.widget<Switch>(sw).value, isTrue);

    await tester.tap(sw);
    await tester.pumpAndSettle();
    expect(tester.widget<Switch>(find.byType(Switch)).value, isFalse,
        reason: 'the switch must reflect the new state, not snap back');
  });

  // "Skip this version" in the startup popup is otherwise a one-way door — this
  // row is the only way back.
  testWidgets('a silenced version can be un-silenced here', (tester) async {
    await pumpSection(tester,
        currentVersion: '0.2.0', prefs: {kUpdateSkippedKey: '0.3.0'});

    expect(find.textContaining('not to be notified about 0.3.0'), findsOneWidget);

    await tester.tap(find.widgetWithText(TextButton, 'Notify me again'));
    await tester.pumpAndSettle();

    expect(find.textContaining('not to be notified'), findsNothing);
    final ctx = tester.element(find.byType(UpdatesSection));
    final container = ProviderScope.containerOf(ctx);
    expect(container.read(updateProvider.notifier).shouldAnnounce(), isTrue);
  });

  testWidgets('a snoozed version says when it comes back', (tester) async {
    await pumpSection(tester, currentVersion: '0.2.0', prefs: {
      kUpdateSnoozeVersionKey: '0.3.0',
      kUpdateSnoozeUntilKey:
          DateTime.now().add(const Duration(hours: 6)).toIso8601String(),
    });

    expect(find.textContaining('paused until in 5h'), findsOneWidget);
  });

  testWidgets('nothing silenced means no undo row', (tester) async {
    await pumpSection(tester, currentVersion: '0.2.0');
    expect(find.widgetWithText(TextButton, 'Notify me again'), findsNothing);
  });

  testWidgets('install asks before killing running agents', (tester) async {
    await pumpSection(tester, currentVersion: '0.2.0');

    // Drive to `ready` the way a real download would leave it.
    final ctx = tester.element(find.byType(UpdatesSection));
    final container = ProviderScope.containerOf(ctx);
    container.read(updateProvider.notifier).state =
        container.read(updateProvider).copyWith(phase: UpdatePhase.ready);
    await tester.pump();

    expect(find.widgetWithText(FilledButton, 'Install & Restart'), findsOneWidget);
    await tester.tap(find.widgetWithText(FilledButton, 'Install & Restart'));
    await tester.pumpAndSettle();

    expect(find.text('Install update?'), findsOneWidget);
    expect(find.textContaining('will be stopped'), findsOneWidget);

    // Backing out must not do anything drastic.
    await tester.tap(find.text('Not now'));
    await tester.pumpAndSettle();
    expect(find.text('Install update?'), findsNothing);
  });
}
