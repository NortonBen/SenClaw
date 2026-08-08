import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';

import 'package:senclaw_desktop/core/daemon/daemon_provider.dart';
import 'package:senclaw_desktop/core/daemon/daemon_supervisor.dart';
import 'package:senclaw_desktop/core/i18n/l10n.dart';
import 'package:senclaw_desktop/core/prefs.dart';
import 'package:senclaw_desktop/features/settings/settings_screen.dart';
import 'package:senclaw_desktop/theme/app_theme.dart';

/// Settings → General → Network access: the daemon's bind host, chosen from
/// the UI instead of an env var no desktop user ever sees.
Future<SharedPreferences> _pump(
  WidgetTester tester, {
  bool startPublic = false,
  DaemonSupervisor? supervisor,
}) async {
  SharedPreferences.setMockInitialValues(
      startPublic ? {kBindPublicKey: true} : {});
  final prefs = await SharedPreferences.getInstance();
  await tester.pumpWidget(ProviderScope(
    overrides: [
      prefsProvider.overrideWithValue(prefs),
      if (supervisor != null)
        daemonSupervisorProvider.overrideWith((_) => supervisor),
    ],
    child: MaterialApp(
      theme: AppTheme.light(),
      localizationsDelegates: const [L10nDelegate()],
      home: const Scaffold(
        body: SingleChildScrollView(child: NetworkBindField()),
      ),
    ),
  ));
  await tester.pump();
  return prefs;
}

void main() {
  testWidgets('defaults to private, with no exposure warning', (tester) async {
    await _pump(tester);

    expect(find.text('127.0.0.1'), findsOneWidget);
    expect(find.text('0.0.0.0'), findsOneWidget);
    // Nothing is exposed, so nothing is warned about.
    expect(find.textContaining('Anyone on your network'), findsNothing);
    // Nothing is running either, so there is nothing to restart — the next
    // start picks the choice up on its own.
    expect(find.text('Restart daemon'), findsNothing);
  });

  testWidgets('choosing public warns and persists', (tester) async {
    final prefs = await _pump(tester);

    await tester.tap(find.text('Public'));
    await tester.pump();

    expect(find.textContaining('Anyone on your network'), findsOneWidget,
        reason: 'exposing the daemon to the LAN must say so');
    expect(prefs.getBool(kBindPublicKey), isTrue);
  });

  testWidgets('switching back to private clears the warning', (tester) async {
    final prefs = await _pump(tester, startPublic: true);
    expect(find.textContaining('Anyone on your network'), findsOneWidget);

    await tester.tap(find.text('Private'));
    await tester.pump();

    expect(find.textContaining('Anyone on your network'), findsNothing);
    expect(prefs.getBool(kBindPublicKey), isFalse);
  });

  testWidgets('a live daemon on the other setting is offered a restart',
      (tester) async {
    // A daemon that answers → adopted, i.e. started outside the app, so its
    // bind host came from its own environment and no setting here reached it.
    // Real sockets and real delays need runAsync: inside a widget test the
    // clock is fake, and `start()` would wait on timers that never fire.
    late final DaemonSupervisor sup;
    await tester.runAsync(() async {
      final fake = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
      fake.listen((req) => req.response.close());
      addTearDown(() => fake.close(force: true));

      sup = DaemonSupervisor(
        uiPort: fake.port,
        adoptProbeBudget: const Duration(seconds: 2),
      );
      await sup.start();
    });
    expect(sup.phase, DaemonPhase.adopted);

    await _pump(tester, supervisor: sup);

    expect(find.text('Restart daemon'), findsOneWidget);
    expect(find.textContaining('started outside the app'), findsOneWidget);
  });

  test('the stored choice reaches the supervisor as its bind host', () async {
    SharedPreferences.setMockInitialValues({kBindPublicKey: true});
    final prefs = await SharedPreferences.getInstance();
    final container = ProviderContainer(
      overrides: [prefsProvider.overrideWithValue(prefs)],
    );
    addTearDown(container.dispose);

    expect(container.read(daemonSupervisorProvider).bindHost,
        DaemonSupervisor.kPublicBindHost);
  });

  test('a changed choice is pending only while a daemon is actually up', () {
    final sup = DaemonSupervisor();
    // Nothing running: the change applies at the next start, no restart owed.
    sup.bindHost = DaemonSupervisor.kPublicBindHost;
    expect(sup.bindHostPending, isFalse);
    expect(sup.activeBindHost, isNull);
  });
}
