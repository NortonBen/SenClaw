import 'dart:convert';

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:senclaw_desktop/core/prefs.dart';
import 'package:senclaw_desktop/core/update/update_provider.dart';
import 'package:senclaw_desktop/core/update/update_service.dart';
import 'package:shared_preferences/shared_preferences.dart';

const _target = 'aarch64-apple-darwin';

String manifestJson(String version) => '''
{
  "version": "$version",
  "notes": "- something",
  "assets": {
    "$_target": {"name": "SenClaw-$_target.app.zip", "size": 10, "sha256": "ab"}
  }
}
''';

/// Container wired with a fake GitHub and a fixed "current version", so the
/// real decision (is there an update?) is exercised — a test binary is always
/// version 'dev', which would otherwise short-circuit every path.
Future<ProviderContainer> makeContainer({
  required String currentVersion,
  required http.Client client,
  Map<String, Object> prefs = const {},
}) async {
  SharedPreferences.setMockInitialValues(prefs);
  final sp = await SharedPreferences.getInstance();
  return ProviderContainer(overrides: [
    prefsProvider.overrideWithValue(sp),
    updateServiceProvider.overrideWithValue(
      UpdateService(client: client, currentVersion: currentVersion, buildTarget: _target),
    ),
  ]);
}

MockClient okWith(String body) =>
    MockClient((_) async => http.Response(body, 200));

void main() {
  test('a newer release becomes available', () async {
    final c = await makeContainer(
      currentVersion: '0.2.0',
      client: okWith(manifestJson('0.3.0')),
    );
    await c.read(updateProvider.notifier).check();
    final s = c.read(updateProvider);
    expect(s.phase, UpdatePhase.available);
    expect(s.manifest!.version.toString(), '0.3.0');
    expect(s.hasUpdate, isTrue);
  });

  test('the same version reports up to date', () async {
    final c = await makeContainer(
      currentVersion: '0.3.0',
      client: okWith(manifestJson('0.3.0')),
    );
    await c.read(updateProvider.notifier).check();
    expect(c.read(updateProvider).phase, UpdatePhase.upToDate);
    expect(c.read(updateProvider).hasUpdate, isFalse);
  });

  // A local build that jumped ahead of the release must not be "updated" backwards.
  test('an older release is not offered as an update', () async {
    final c = await makeContainer(
      currentVersion: '0.4.0',
      client: okWith(manifestJson('0.3.0')),
    );
    await c.read(updateProvider.notifier).check();
    expect(c.read(updateProvider).phase, UpdatePhase.upToDate);
  });

  // The double-digit trap, end to end through the provider.
  test('0.10.0 is newer than 0.9.0', () async {
    final c = await makeContainer(
      currentVersion: '0.9.0',
      client: okWith(manifestJson('0.10.0')),
    );
    await c.read(updateProvider.notifier).check();
    expect(c.read(updateProvider).phase, UpdatePhase.available);
  });

  // GitHub serves latest.json as application/octet-stream with no charset, and
  // package:http then decodes `.body` as latin-1 — which mangled the Vietnamese
  // release notes on the Updates page ("chuẩn hoá" → "chuá°©n hoÃ¡").
  test('release notes survive UTF-8 served without a charset header', () async {
    final json = manifestJson('9.9.9')
        .replaceFirst('- something', '- chuẩn hoá bind host cho bài đăng');
    final svc = UpdateService(
      client: MockClient((_) async => http.Response.bytes(utf8.encode(json), 200)),
      currentVersion: '0.2.0',
      buildTarget: _target,
    );
    final m = await svc.fetchManifest();
    expect(m, isNotNull);
    expect(m!.notes, contains('chuẩn hoá bind host cho bài đăng'));
    expect(m.notes, isNot(contains('Ã')),
        reason: 'mojibake — the body was decoded as latin-1, not UTF-8');
  });

  test('a dev build never checks', () async {
    var called = false;
    final c = await makeContainer(
      currentVersion: 'dev',
      client: MockClient((_) async {
        called = true;
        return http.Response(manifestJson('9.9.9'), 200);
      }),
    );
    await c.read(updateProvider.notifier).maybeCheck();
    expect(called, isFalse, reason: 'a dev build must not even reach the network');
    expect(c.read(updateProvider).hasUpdate, isFalse);
  });

  group('failures stay quiet when the check was automatic', () {
    test('404 (a release older than the manifest step) is silent', () async {
      final c = await makeContainer(
        currentVersion: '0.2.0',
        client: MockClient((_) async => http.Response('Not Found', 404)),
      );
      await c.read(updateProvider.notifier).check(silent: true);
      final s = c.read(updateProvider);
      expect(s.phase, UpdatePhase.idle);
      expect(s.error, isNull, reason: 'a background check must not raise an error');
    });

    test('a network failure is silent', () async {
      final c = await makeContainer(
        currentVersion: '0.2.0',
        client: MockClient((_) async => throw Exception('offline')),
      );
      await c.read(updateProvider.notifier).check(silent: true);
      expect(c.read(updateProvider).phase, UpdatePhase.idle);
      expect(c.read(updateProvider).error, isNull);
    });

    test('but a check the user asked for does report the failure', () async {
      final c = await makeContainer(
        currentVersion: '0.2.0',
        client: MockClient((_) async => http.Response('Not Found', 404)),
      );
      await c.read(updateProvider.notifier).check();
      expect(c.read(updateProvider).phase, UpdatePhase.error);
      expect(c.read(updateProvider).error, isNotNull);
    });
  });

  group('maybeCheck scheduling', () {
    test('skips when checked recently', () async {
      var hits = 0;
      final c = await makeContainer(
        currentVersion: '0.2.0',
        prefs: {
          kUpdateLastCheckKey:
              DateTime.now().subtract(const Duration(hours: 1)).toIso8601String(),
        },
        client: MockClient((_) async {
          hits++;
          return http.Response(manifestJson('0.3.0'), 200);
        }),
      );
      await c.read(updateProvider.notifier).maybeCheck();
      expect(hits, 0, reason: 'the app is shown/hidden all day; do not re-check each time');
    });

    test('runs once the interval has passed', () async {
      var hits = 0;
      final c = await makeContainer(
        currentVersion: '0.2.0',
        prefs: {
          kUpdateLastCheckKey:
              DateTime.now().subtract(const Duration(days: 2)).toIso8601String(),
        },
        client: MockClient((_) async {
          hits++;
          return http.Response(manifestJson('0.3.0'), 200);
        }),
      );
      await c.read(updateProvider.notifier).maybeCheck();
      expect(hits, 1);
      expect(c.read(updateProvider).phase, UpdatePhase.available);
    });

    test('respects the auto-check toggle being off', () async {
      var hits = 0;
      final c = await makeContainer(
        currentVersion: '0.2.0',
        prefs: {kUpdateAutoCheckKey: false},
        client: MockClient((_) async {
          hits++;
          return http.Response(manifestJson('0.3.0'), 200);
        }),
      );
      await c.read(updateProvider.notifier).maybeCheck();
      expect(hits, 0);
    });

    test('a manual check ignores both the interval and the toggle', () async {
      final c = await makeContainer(
        currentVersion: '0.2.0',
        prefs: {
          kUpdateAutoCheckKey: false,
          kUpdateLastCheckKey: DateTime.now().toIso8601String(),
        },
        client: okWith(manifestJson('0.3.0')),
      );
      await c.read(updateProvider.notifier).check();
      expect(c.read(updateProvider).phase, UpdatePhase.available);
    });
  });

  group('announce', () {
    test('announces a new version once', () async {
      final c = await makeContainer(
        currentVersion: '0.2.0',
        client: okWith(manifestJson('0.3.0')),
      );
      final n = c.read(updateProvider.notifier);
      await n.check();
      expect(n.shouldAnnounce(), isTrue);
    });

    test('stays quiet about a version the user skipped', () async {
      final c = await makeContainer(
        currentVersion: '0.2.0',
        prefs: {kUpdateSkippedKey: '0.3.0'},
        client: okWith(manifestJson('0.3.0')),
      );
      final n = c.read(updateProvider.notifier);
      await n.check();
      expect(n.shouldAnnounce(), isFalse);
    });

    test('but speaks up again for the version after the skipped one', () async {
      final c = await makeContainer(
        currentVersion: '0.2.0',
        prefs: {kUpdateSkippedKey: '0.3.0'},
        client: okWith(manifestJson('0.4.0')),
      );
      final n = c.read(updateProvider.notifier);
      await n.check();
      expect(n.shouldAnnounce(), isTrue);
    });

    test('skipCurrent persists the version', () async {
      final c = await makeContainer(
        currentVersion: '0.2.0',
        client: okWith(manifestJson('0.3.0')),
      );
      final n = c.read(updateProvider.notifier);
      await n.check();
      await n.skipCurrent();
      expect(n.shouldAnnounce(), isFalse);
    });
  });

  test('autoCheck toggle round-trips through state and prefs', () async {
    final c = await makeContainer(
      currentVersion: '0.2.0',
      client: okWith(manifestJson('0.3.0')),
    );
    expect(c.read(updateProvider).autoCheck, isTrue);
    await c.read(updateProvider.notifier).setAutoCheck(false);
    expect(c.read(updateProvider).autoCheck, isFalse);
  });

  test('a release with no bundle for this platform refuses before downloading',
      () async {
    final c = await makeContainer(
      currentVersion: '0.2.0',
      client: okWith('''
        {"version":"0.3.0","assets":{"some-other-triple":{"name":"x.zip","sha256":"ab"}}}
      '''),
    );
    final n = c.read(updateProvider.notifier);
    await n.check();
    expect(c.read(updateProvider).phase, UpdatePhase.available);
    await n.download();
    final s = c.read(updateProvider);
    expect(s.phase, UpdatePhase.error);
    expect(s.error, contains('no bundle for this platform'));
  });
}
