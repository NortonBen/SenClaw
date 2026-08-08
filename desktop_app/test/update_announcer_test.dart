import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:senclaw_desktop/core/prefs.dart';
import 'package:senclaw_desktop/core/update/update_announcer.dart';
import 'package:senclaw_desktop/core/update/update_provider.dart';
import 'package:senclaw_desktop/core/update/update_service.dart';
import 'package:senclaw_desktop/theme/app_theme.dart';
import 'package:shared_preferences/shared_preferences.dart';

const _target = 'aarch64-apple-darwin';

String manifestJson(String version) => '''
{
  "version": "$version",
  "notes": "- feat: the new thing",
  "assets": {
    "$_target": {"name": "SenClaw-$_target.app.zip", "size": 10, "sha256": "ab"}
  }
}
''';

/// Mounts the announcer over a trivial page, with a fake GitHub and a fixed
/// current version — a test binary is always 'dev', which never announces.
Future<(ProviderContainer, List<String>)> pumpAnnouncer(
  WidgetTester tester, {
  String currentVersion = '0.2.0',
  String release = '0.3.0',
  Map<String, Object> prefs = const {},
}) async {
  SharedPreferences.setMockInitialValues(prefs);
  final sp = await SharedPreferences.getInstance();
  final container = ProviderContainer(overrides: [
    prefsProvider.overrideWithValue(sp),
    updateServiceProvider.overrideWithValue(UpdateService(
      client: MockClient((_) async => http.Response(manifestJson(release), 200)),
      currentVersion: currentVersion,
      buildTarget: _target,
    )),
  ]);
  addTearDown(container.dispose);

  final opened = <String>[];
  await tester.pumpWidget(UncontrolledProviderScope(
    container: container,
    child: MaterialApp(
      theme: AppTheme.dark(),
      home: UpdateAnnouncer(
        onOpenUpdates: () => opened.add('updates'),
        child: const Scaffold(body: Text('home')),
      ),
    ),
  ));
  await tester.pump();
  return (container, opened);
}

Future<void> check(ProviderContainer c, WidgetTester tester) async {
  await c.read(updateProvider.notifier).check();
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('a new release pops up on its own', (tester) async {
    final (c, _) = await pumpAnnouncer(tester);
    expect(find.text('A new version of SenClaw is available'), findsNothing);

    await check(c, tester);

    expect(find.text('A new version of SenClaw is available'), findsOneWidget);
    expect(find.textContaining('0.3.0'), findsWidgets);
    expect(find.textContaining('the new thing'), findsWidgets,
        reason: 'release notes belong in the popup, not just on the page');
  });

  testWidgets('"View update" routes to the Updates page', (tester) async {
    final (c, opened) = await pumpAnnouncer(tester);
    await check(c, tester);

    await tester.tap(find.widgetWithText(FilledButton, 'View update'));
    await tester.pumpAndSettle();

    expect(opened, ['updates']);
    expect(find.text('A new version of SenClaw is available'), findsNothing);
    // Neither escape hatch was taken, so nothing is silenced.
    expect(c.read(updateProvider).announcementSilenced, isFalse);
  });

  testWidgets('"Remind me later" silences this version for a day',
      (tester) async {
    final (c, _) = await pumpAnnouncer(tester);
    await check(c, tester);

    await tester.tap(find.widgetWithText(TextButton, 'Remind me later'));
    await tester.pumpAndSettle();

    expect(find.text('A new version of SenClaw is available'), findsNothing);
    final s = c.read(updateProvider);
    expect(s.announcementSilenced, isTrue);
    expect(s.snoozeVersion, '0.3.0');
    expect(s.snoozeUntil!.isAfter(DateTime.now().add(const Duration(hours: 23))),
        isTrue);
    expect(c.read(updateProvider.notifier).shouldAnnounce(), isFalse);
  });

  testWidgets('"Skip this version" silences it for good', (tester) async {
    final (c, _) = await pumpAnnouncer(tester);
    await check(c, tester);

    await tester.tap(find.widgetWithText(TextButton, 'Skip this version'));
    await tester.pumpAndSettle();

    expect(c.read(updateProvider).skippedVersion, '0.3.0');
    expect(c.read(updateProvider.notifier).shouldAnnounce(), isFalse);
  });

  // The popup is the whole point of the startup check; a repeat check within
  // the same run must not stack a second dialog on the first.
  testWidgets('does not re-open for a version already announced',
      (tester) async {
    final (c, _) = await pumpAnnouncer(tester);
    await check(c, tester);
    expect(find.byType(AlertDialog), findsOneWidget);

    await tester.tap(find.widgetWithText(TextButton, 'Remind me later'));
    await tester.pumpAndSettle();
    await check(c, tester);

    expect(find.byType(AlertDialog), findsNothing);
  });

  testWidgets('a snoozed version stays quiet on the next launch',
      (tester) async {
    final (c, _) = await pumpAnnouncer(tester, prefs: {
      kUpdateSnoozeVersionKey: '0.3.0',
      kUpdateSnoozeUntilKey:
          DateTime.now().add(const Duration(hours: 5)).toIso8601String(),
    });
    await check(c, tester);
    expect(find.byType(AlertDialog), findsNothing);
  });

  testWidgets('but speaks up once the snooze has run out', (tester) async {
    final (c, _) = await pumpAnnouncer(tester, prefs: {
      kUpdateSnoozeVersionKey: '0.3.0',
      kUpdateSnoozeUntilKey:
          DateTime.now().subtract(const Duration(minutes: 1)).toIso8601String(),
    });
    await check(c, tester);
    expect(find.byType(AlertDialog), findsOneWidget);
  });

  // A snooze is per-version: postponing 0.3.0 must not hide 0.4.0 shipping the
  // same afternoon.
  testWidgets('a snooze on an older version does not hide a newer one',
      (tester) async {
    final (c, _) = await pumpAnnouncer(tester, release: '0.4.0', prefs: {
      kUpdateSnoozeVersionKey: '0.3.0',
      kUpdateSnoozeUntilKey:
          DateTime.now().add(const Duration(hours: 20)).toIso8601String(),
    });
    await check(c, tester);
    expect(find.byType(AlertDialog), findsOneWidget);
  });

  testWidgets('a skipped version stays quiet, a later one does not',
      (tester) async {
    final (skipped, _) = await pumpAnnouncer(tester,
        prefs: {kUpdateSkippedKey: '0.3.0'});
    await check(skipped, tester);
    expect(find.byType(AlertDialog), findsNothing);

    final (next, _) =
        await pumpAnnouncer(tester, release: '0.4.0', prefs: {kUpdateSkippedKey: '0.3.0'});
    await check(next, tester);
    expect(find.byType(AlertDialog), findsOneWidget);
  });

  // Closing with Esc is not an answer: nothing is persisted, so the next launch
  // asks again.
  testWidgets('dismissing without choosing persists nothing', (tester) async {
    final (c, _) = await pumpAnnouncer(tester);
    await check(c, tester);

    Navigator.of(tester.element(find.byType(AlertDialog))).pop();
    await tester.pumpAndSettle();

    expect(find.byType(AlertDialog), findsNothing);
    expect(c.read(updateProvider).announcementSilenced, isFalse);
    expect(c.read(updateProvider.notifier).shouldAnnounce(), isTrue);
  });

  testWidgets('an update found before the announcer mounted still shows',
      (tester) async {
    SharedPreferences.setMockInitialValues({});
    final sp = await SharedPreferences.getInstance();
    final container = ProviderContainer(overrides: [
      prefsProvider.overrideWithValue(sp),
      updateServiceProvider.overrideWithValue(UpdateService(
        client:
            MockClient((_) async => http.Response(manifestJson('0.3.0'), 200)),
        currentVersion: '0.2.0',
        buildTarget: _target,
      )),
    ]);
    addTearDown(container.dispose);
    await container.read(updateProvider.notifier).check();

    await tester.pumpWidget(UncontrolledProviderScope(
      container: container,
      child: MaterialApp(
        theme: AppTheme.dark(),
        home: UpdateAnnouncer(
          onOpenUpdates: () {},
          child: const Scaffold(body: Text('home')),
        ),
      ),
    ));
    await tester.pumpAndSettle();

    expect(find.byType(AlertDialog), findsOneWidget);
  });
}
