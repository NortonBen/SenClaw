import 'dart:convert';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:senclaw_desktop/core/config/app_config.dart';
import 'package:senclaw_desktop/core/transport/api_client.dart';
import 'package:senclaw_desktop/core/transport/connection.dart';
import 'package:senclaw_desktop/features/plugins/sandbox_panel.dart';
import 'package:senclaw_desktop/core/prefs.dart';
import 'package:senclaw_desktop/theme/app_theme.dart';
import 'package:shared_preferences/shared_preferences.dart';

/// Answers the five GETs the panel fires on mount with realistic payloads and
/// records every mutating call.
class _FakeSandboxApi implements ApiClient {
  _FakeSandboxApi({this.apps});

  /// Rows for `/api/space/apps/sandbox-overview`. Null = the default single
  /// confined, running app.
  final List<Map<String, dynamic>>? apps;
  final calls = <String>[];

  @override
  void updateConfig(AppConfig config) {}

  @override
  void dispose() {}

  @override
  Future<dynamic> get(String path, {Map<String, dynamic>? query}) async {
    calls.add('GET $path');
    switch (path) {
      case '/api/space/apps/sandbox-overview':
        return {
          'apps': apps ??
              [
                {
                  'id': 'ba',
                  'name': 'BA Studio',
                  'icon': '📐',
                  'config': {
                    'enabled': true,
                    'readMode': 'strict',
                    'network': 'hosts',
                    'hosts': ['api.openai.com'],
                    'daemonApi': true,
                    'folders': 0,
                  },
                  'running': true,
                  'isolation': 'seatbelt',
                  'pid': 4242,
                  'port': 4740,
                  'uptimeMs': 125000,
                  'launches': 1,
                  'cpu': 3.5,
                  'rssMb': 42.0,
                  'processes': 1,
                  'proxy': {
                    'port': 51234,
                    'stats': {
                      'allowed': 5,
                      'denied': 2,
                      'recentDenied': ['telemetry.example'],
                    },
                  },
                }
              ],
          'caps': {
            'isolation': 'seatbelt',
            'enforceable': true,
            'networkEnforceable': true,
          },
        };
      case '/api/sandbox/caps':
        return {
          'backends': ['direct'],
          'direct': {
            'available': true,
            'kind': 'seatbelt',
            'detail': 'macOS Seatbelt',
          },
          'docker': {'available': false, 'detail': 'daemon not running'},
        };
      case '/api/sandbox/exec-policy':
        return {
          'execShell': false,
          'execNetwork': true,
          'execFsMode': 'open',
          'runPython': true,
          'runNode': true,
          'codeNetwork': false,
          'schedulerScript': false,
          'schedulerNetwork': true,
        };
      case '/api/sandbox/settings':
        return {
          'defaultFsMode': 'strict',
          'allowlist': ['/tmp/du-lieu'],
          'defaultNetwork': false,
          'defaultMemoryMb': 512,
          'defaultCpus': 1.0,
          'defaultTimeoutMs': 30000,
        };
      case '/api/sandbox/sandboxes':
        return {
          'sandboxes': [
            {
              'id': 'sb1',
              'name': 'demo-sandbox',
              'backend': 'direct',
              'workdir': '/tmp/sb1',
              'network': false,
              'cpus': 1.0,
              'memoryMb': 512,
              'timeoutMs': 30000,
              'fsMode': 'strict',
              'traceEnabled': false,
              'status': 'stopped',
              'createdAt': 1700000000000,
            }
          ],
        };
      case '/api/sandbox/runs':
        return {
          'runs': [
            {
              'id': 'r1',
              'sandboxId': 'sb1',
              'kind': 'run',
              'language': 'python',
              'source': 'print(42)',
              'exitCode': 0,
              'timedOut': false,
              'isolation': 'seatbelt',
              'network': false,
              'durationMs': 31,
              'createdAt': 1700000000000,
            }
          ],
        };
      case '/api/sandbox/sandboxes/sb1/stats':
        return {
          'running': false,
          'source': 'host',
          'cpu': 1.5,
          'rssMb': 12.0,
          'memoryLimitMb': null,
          'processes': [
            {
              'pid': 123,
              'ppid': 1,
              'cpu': 1.5,
              'memPercent': 0.1,
              'rssMb': 12.0,
              'elapsed': '00:01',
              'command': 'python3 main.py',
            }
          ],
        };
    }
    return {};
  }

  @override
  Future<dynamic> post(String path, {Object? body}) async {
    calls.add('POST $path ${jsonEncode(body ?? {})}');
    return {'ok': true};
  }

  @override
  Future<dynamic> put(String path, {Object? body}) async {
    calls.add('PUT $path ${jsonEncode(body ?? {})}');
    // Echo the body back — the panel adopts the server's saved copy.
    return body;
  }

  @override
  Future<dynamic> patch(String path, {Object? body}) async => {'ok': true};

  @override
  Future<dynamic> delete(String path, {Object? body}) async {
    calls.add('DELETE $path');
    return {'ok': true};
  }
}

/// Fake native folder picker — returns a fixed path (or null = user cancelled).
class _FakeDirPicker extends FilePicker {
  _FakeDirPicker(this.result);
  final String? result;

  @override
  Future<String?> getDirectoryPath({
    String? dialogTitle,
    bool lockParentWindow = false,
    String? initialDirectory,
  }) async =>
      result;
}

Future<_FakeSandboxApi> _pump(
  WidgetTester tester, [
  _FakeSandboxApi? given,
  String? language,
]) async {
  tester.view.physicalSize = const Size(1400, 2600);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);

  // The panel renders in the app language now (it no longer carries its own
  // EN/VI switch), so the language chain — and therefore prefs — has to be real.
  SharedPreferences.setMockInitialValues(
      language == null ? {} : {'flutter.senclaw:app-language': language});
  final prefs = await SharedPreferences.getInstance();

  final api = given ?? _FakeSandboxApi();
  await tester.pumpWidget(ProviderScope(
    overrides: [
      apiClientProvider.overrideWithValue(api),
      prefsProvider.overrideWithValue(prefs),
    ],
    child: MaterialApp(
      theme: AppTheme.light(),
      home: const Scaffold(body: SandboxPanel()),
    ),
  ));
  await tester.pumpAndSettle();
  api.calls.clear(); // drop the mount-time GETs; tests assert mutations
  return api;
}


/// A handful of apps that differ in every sortable way, so an order can be
/// asserted without ambiguity.
List<Map<String, dynamic>> _mixedApps() => [
      _appJson('zeta', 'Zeta', running: true, cpu: 1.0, rss: 10, launches: 1, on: true),
      _appJson('alpha', 'Alpha', running: false),
      _appJson('mid', 'Mid', running: true, cpu: 55.0, rss: 500, launches: 9, on: true),
      _appJson('beta', 'Beta', running: false, on: true),
    ];

Map<String, dynamic> _appJson(
  String id,
  String name, {
  bool running = false,
  double? cpu,
  double? rss,
  int launches = 0,
  bool on = false,
}) =>
    {
      'id': id,
      'name': name,
      'icon': null,
      'config': {
        'enabled': on,
        'readMode': 'open',
        'network': 'all',
        'hosts': [],
        'daemonApi': true,
        'folders': 0,
      },
      'running': running,
      'isolation': running ? (on ? 'seatbelt' : 'none') : null,
      'pid': running ? 1 : null,
      'port': running ? 4000 : null,
      'uptimeMs': running ? 1000 : null,
      'launches': launches,
      'cpu': cpu,
      'rssMb': rss,
      'processes': running ? 1 : null,
      'proxy': null,
    };

/// Row order as rendered, top to bottom.
List<String> _appOrder(WidgetTester tester) => tester
    .widgetList<Text>(find.byType(Text))
    .map((t) => t.data ?? '')
    .where((s) => ['Alpha', 'Beta', 'Mid', 'Zeta'].contains(s))
    .toList();

void main() {
  testWidgets('renders caps, policy switches, sandbox row and run history',
      (tester) async {
    await _pump(tester);
    expect(tester.takeException(), isNull);

    // Caps banner.
    expect(find.text('Available isolation'), findsOneWidget);
    expect(find.text('seatbelt'), findsWidgets); // caps tag + run history tag

    // The five enforcement toggles.
    expect(find.text('Exec (agent Bash tool)'), findsOneWidget);
    expect(find.text('Run Python'), findsOneWidget);
    expect(find.text('Run Node.js'), findsOneWidget);
    expect(find.text('Network for Python/Node'), findsOneWidget);
    expect(find.text('Scheduled scripts (scheduler)'), findsOneWidget);

    // Switch values mirror the policy payload:
    // execShell OFF, runPython ON, runNode ON, codeNetwork OFF,
    // schedulerScript OFF, then defaults' network OFF.
    final switches = tester.widgetList<Switch>(find.byType(Switch)).toList();
    expect(switches.map((s) => s.value).toList(),
        [false, true, true, false, false, false]);

    // Sandbox row + run history.
    expect(find.text('Managed sandboxes (1)'), findsOneWidget);
    expect(find.text('demo-sandbox'), findsOneWidget);
    expect(find.text('print(42)'), findsOneWidget);
    expect(find.text('exit 0'), findsOneWidget);

    // Defaults card lists the allowlist entry as a removable row and offers
    // the native folder picker.
    expect(find.text('/tmp/du-lieu'), findsOneWidget);
    expect(find.byTooltip('Remove from allowlist'), findsOneWidget);
    expect(find.text('Choose folder…'), findsOneWidget);
  });

  testWidgets('the panel renders in the app language, with no switch of its own',
      (tester) async {
    // Settings → Language owns this. The screen used to carry a second EN/VI
    // switch and its own stored preference, which is how the two disagreed.
    await _pump(tester);
    expect(find.text('Managed sandboxes (1)'), findsOneWidget);
    expect(find.text('EN'), findsNothing);
    expect(find.text('VI'), findsNothing);
  });

  testWidgets('a Vietnamese app language translates every label', (tester) async {
    await _pump(tester, null, 'vi');

    expect(find.text('Luồng đang quản lý (1)'), findsOneWidget);
    expect(find.text('Exec (tool Bash của agent)'), findsOneWidget);
    expect(find.text('Lịch sử chạy gần nhất'), findsOneWidget);
    expect(find.text('Managed sandboxes (1)'), findsNothing);
  });

  testWidgets('removing an allowlist entry saves the shrunken list',
      (tester) async {
    final api = await _pump(tester);
    await tester.ensureVisible(find.byTooltip('Remove from allowlist'));
    await tester.tap(find.byTooltip('Remove from allowlist'));
    await tester.pumpAndSettle();

    final call = api.calls.single;
    expect(call, startsWith('PUT /api/sandbox/settings '));
    final body = jsonDecode(call.substring('PUT /api/sandbox/settings '.length))
        as Map<String, dynamic>;
    expect(body['allowlist'], isEmpty);
    expect(find.text('/tmp/du-lieu'), findsNothing);
  });

  testWidgets('typing an absolute path adds it; a relative one is refused',
      (tester) async {
    final api = await _pump(tester);
    final field = find.widgetWithText(
        TextField, 'or type an absolute path: /Users/you/data');
    await tester.ensureVisible(field);

    // Relative → refused with a snack, no API call.
    await tester.enterText(field, 'du-lieu/tuong-doi');
    await tester.testTextInput.receiveAction(TextInputAction.done);
    await tester.pumpAndSettle();
    expect(api.calls, isEmpty);
    expect(find.textContaining('An absolute path is required'), findsOneWidget);

    // Absolute → saved alongside the existing entry.
    await tester.enterText(field, '/Users/ban/tai-lieu');
    await tester.testTextInput.receiveAction(TextInputAction.done);
    await tester.pumpAndSettle();
    final call = api.calls.single;
    final body = jsonDecode(call.substring('PUT /api/sandbox/settings '.length))
        as Map<String, dynamic>;
    expect(body['allowlist'], ['/tmp/du-lieu', '/Users/ban/tai-lieu']);
    expect(find.text('/Users/ban/tai-lieu'), findsOneWidget);
  });

  testWidgets('the folder picker appends the chosen directory',
      (tester) async {
    final api = await _pump(tester);
    FilePicker.platform = _FakeDirPicker('/Users/ban/da-chon');
    await tester.ensureVisible(find.text('Choose folder…'));
    await tester.tap(find.text('Choose folder…'));
    await tester.pumpAndSettle();

    final call = api.calls.single;
    final body = jsonDecode(call.substring('PUT /api/sandbox/settings '.length))
        as Map<String, dynamic>;
    expect(body['allowlist'], ['/tmp/du-lieu', '/Users/ban/da-chon']);
  });

  testWidgets('cancelling the folder picker changes nothing', (tester) async {
    final api = await _pump(tester);
    FilePicker.platform = _FakeDirPicker(null);
    await tester.ensureVisible(find.text('Choose folder…'));
    await tester.tap(find.text('Choose folder…'));
    await tester.pumpAndSettle();
    expect(api.calls, isEmpty);
  });

  testWidgets('toggling Run Python PUTs the full policy object',
      (tester) async {
    final api = await _pump(tester);
    await tester.tap(find.byType(Switch).at(1)); // runPython
    await tester.pumpAndSettle();

    expect(api.calls, hasLength(1));
    final call = api.calls.single;
    expect(call, startsWith('PUT /api/sandbox/exec-policy '));
    final body =
        jsonDecode(call.substring('PUT /api/sandbox/exec-policy '.length))
            as Map<String, dynamic>;
    expect(body['runPython'], isFalse);
    // Full object, not a partial patch — a missing field would be reset to
    // its default by the daemon's serde(default).
    expect(body.keys.toSet(), {
      'execShell',
      'execNetwork',
      'execFsMode',
      'runPython',
      'runNode',
      'codeNetwork',
      'schedulerScript',
      'schedulerNetwork',
    });
  });

  testWidgets('delete asks first, then DELETEs without purge', (tester) async {
    final api = await _pump(tester);
    await tester.tap(find.byIcon(Icons.delete_outline).first);
    await tester.pumpAndSettle();

    expect(find.text('Delete sandbox?'), findsOneWidget);
    await tester.tap(find.text('Delete'));
    await tester.pumpAndSettle();

    expect(api.calls.first, 'DELETE /api/sandbox/sandboxes/sb1?purge=false');
  });

  testWidgets('cancelling the delete dialog issues no call', (tester) async {
    final api = await _pump(tester);
    await tester.tap(find.byIcon(Icons.delete_outline).first);
    await tester.pumpAndSettle();
    await tester.tap(find.text('Cancel'));
    await tester.pumpAndSettle();
    expect(api.calls, isEmpty);
  });

  testWidgets('expanding a sandbox row loads stats and can kill one pid',
      (tester) async {
    final api = await _pump(tester);
    await tester.tap(find.text('demo-sandbox'));
    await tester.pumpAndSettle();

    // Stats fetched and rendered: header line + the process row.
    expect(api.calls, contains('GET /api/sandbox/sandboxes/sb1/stats'));
    expect(find.textContaining('CPU 1.5%'), findsOneWidget);
    expect(find.text('python3 main.py'), findsOneWidget);

    // Kill the single process.
    await tester.tap(find.byTooltip('Stop this process'));
    await tester.pumpAndSettle();
    expect(
      api.calls,
      contains('POST /api/sandbox/sandboxes/sb1/kill {"pid":123}'),
    );

    // Collapse again so the 3s stats poller is cancelled deterministically.
    await tester.tap(find.text('demo-sandbox'));
    await tester.pumpAndSettle();
  });

  testWidgets('kill-all button posts the kill endpoint without a pid',
      (tester) async {
    final api = await _pump(tester);
    await tester.tap(find.byTooltip('Stop all processes'));
    await tester.pumpAndSettle();
    expect(api.calls.single, 'POST /api/sandbox/sandboxes/sb1/kill {}');
  });

  testWidgets('the Space Apps card reports what each app actually got',
      (tester) async {
    await _pump(tester);

    expect(find.text('Space Apps — per-app sandbox (1)'), findsOneWidget);
    expect(find.text('BA Studio'), findsOneWidget);
    // What the running process got, not just what is configured.
    expect(find.text('seatbelt'), findsWidgets);
    expect(find.text('strict'), findsWidgets);
    expect(find.text('only some sites'), findsOneWidget);
    expect(find.text('pid 4242 · 2m · 1×'), findsOneWidget);
    expect(find.text('3.5% · 42 MB'), findsOneWidget);
    // The proxy's refusals are the usual reason a confined app misbehaves.
    expect(find.text('2 refused'), findsOneWidget);
  });

  testWidgets('an app configured on but running unconfined says restart needed',
      (tester) async {
    await _pump(
      tester,
      _FakeSandboxApi(apps: [
        {
          'id': 'x',
          'name': 'Stale App',
          'icon': null,
          'config': {
            'enabled': true,
            'readMode': 'open',
            'network': 'all',
            'hosts': [],
            'daemonApi': true,
            'folders': 0,
          },
          'running': true,
          // A profile is fixed at launch: the settings changed after this
          // process started, so it is running with no confinement at all.
          'isolation': 'none',
          'pid': 7,
          'port': 4000,
          'uptimeMs': 1000,
          'launches': 1,
          'cpu': 0.0,
          'rssMb': 1.0,
          'processes': 1,
          'proxy': null,
        }
      ]),
    );
    expect(find.text('restart needed'), findsOneWidget);
  });

  testWidgets('restarting an app from the card hits the app restart endpoint',
      (tester) async {
    final api = await _pump(tester);
    api.calls.clear();

    await tester.tap(find.widgetWithIcon(IconButton, Icons.refresh).first);
    await tester.pumpAndSettle();

    expect(api.calls, contains('POST /api/space/apps/ba/restart {}'));
  });

  testWidgets('no server apps reads as such, not as an empty box',
      (tester) async {
    await _pump(tester, _FakeSandboxApi(apps: const []));
    expect(find.text('No server Space App installed'), findsOneWidget);
  });

  testWidgets('the apps list pages at ten and the pager walks it',
      (tester) async {
    // 47 installed apps is what this looked like on a real machine; unpaged, the
    // card buried every other section of the screen.
    List<Map<String, dynamic>> many(int n) => [
          for (var i = 0; i < n; i++)
            {
              'id': 'app$i',
              // Zero-padded: the default order is alphabetical, and "App 10"
              // sorts before "App 9" in every string comparison there is.
              'name': 'App ${i.toString().padLeft(2, '0')}',
              'icon': null,
              'config': {
                'enabled': false,
                'readMode': 'open',
                'network': 'all',
                'hosts': [],
                'daemonApi': true,
                'folders': 0,
              },
              'running': false,
              'isolation': null,
              'pid': null,
              'port': null,
              'uptimeMs': null,
              'launches': 0,
              'cpu': null,
              'rssMb': null,
              'processes': null,
              'proxy': null,
            }
        ];
    await _pump(tester, _FakeSandboxApi(apps: many(23)));

    expect(find.text('Space Apps — per-app sandbox (23)'), findsOneWidget);
    expect(find.text('App 00'), findsOneWidget);
    expect(find.text('App 09'), findsOneWidget);
    expect(find.text('App 10'), findsNothing, reason: 'page one holds ten rows');
    expect(find.text('1–10 / 23'), findsOneWidget);

    await tester.tap(find.byTooltip('Next page'));
    await tester.pumpAndSettle();
    expect(find.text('App 10'), findsOneWidget);
    expect(find.text('App 00'), findsNothing);
    expect(find.text('11–20 / 23'), findsOneWidget);

    // The last page is short, and the range must say so rather than claim 30.
    await tester.tap(find.byTooltip('Next page'));
    await tester.pumpAndSettle();
    expect(find.text('21–23 / 23'), findsOneWidget);
    expect(find.text('App 22'), findsOneWidget);

    await tester.tap(find.byTooltip('Previous page'));
    await tester.pumpAndSettle();
    expect(find.text('11–20 / 23'), findsOneWidget);
  });

  testWidgets('ten or fewer apps show no pager at all', (tester) async {
    await _pump(tester);
    expect(find.byTooltip('Next page'), findsNothing);
  });

  testWidgets('the monitor button opens the detail dialog for that app',
      (tester) async {
    final api = await _pump(tester);
    await tester.tap(find.byTooltip('Process monitor').first);
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 50));

    expect(find.text('Process monitor — BA Studio'), findsOneWidget);
    // The dialog loads the per-app snapshot, not the fleet summary again.
    expect(api.calls, contains('GET /api/space/apps/ba/runtime'));
  });

  testWidgets('the default order puts running apps on top, then A→Z',
      (tester) async {
    await _pump(tester, _FakeSandboxApi(apps: _mixedApps()));
    expect(_appOrder(tester), ['Mid', 'Zeta', 'Alpha', 'Beta'],
        reason: 'two running (A→Z), then two stopped (A→Z)');
  });

  testWidgets('sorting by name ignores whether an app is running',
      (tester) async {
    await _pump(tester, _FakeSandboxApi(apps: _mixedApps()));
    await tester.tap(find.byKey(const ValueKey('appSortDropdown')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Name').last);
    await tester.pumpAndSettle();
    // Descending is the default direction.
    expect(_appOrder(tester), ['Zeta', 'Mid', 'Beta', 'Alpha']);

    await tester.tap(find.byTooltip('Descending'));
    await tester.pumpAndSettle();
    expect(_appOrder(tester), ['Alpha', 'Beta', 'Mid', 'Zeta']);
  });

  testWidgets('sorting by CPU puts the heaviest first and the idle last',
      (tester) async {
    await _pump(tester, _FakeSandboxApi(apps: _mixedApps()));
    await tester.tap(find.byKey(const ValueKey('appSortDropdown')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('CPU').last);
    await tester.pumpAndSettle();
    // Mid 55% > Zeta 1% > the two that are not running (no number at all),
    // which fall to the bottom in A→Z rather than reading as 0%.
    expect(_appOrder(tester), ['Mid', 'Zeta', 'Alpha', 'Beta']);
  });

  testWidgets('changing the sort returns to the first page', (tester) async {
    final many = [
      for (var i = 0; i < 23; i++)
        _appJson('app$i', 'App ${i.toString().padLeft(2, '0')}'),
    ];
    await _pump(tester, _FakeSandboxApi(apps: many));
    await tester.tap(find.byTooltip('Next page'));
    await tester.pumpAndSettle();
    expect(find.text('11–20 / 23'), findsOneWidget);

    await tester.tap(find.byTooltip('Descending'));
    await tester.pumpAndSettle();
    expect(find.text('1–10 / 23'), findsOneWidget,
        reason: 'page 2 of the old order means nothing in the new one');
  });

  testWidgets('an app the daemon adopted reads as running, not as stopped',
      (tester) async {
    // The bug this exists to prevent: `ensure_server_running` reuses a healthy
    // port instead of double-launching, so an app that outlived a daemon restart
    // has no child record — and the card used to call it "not running" while it
    // was serving requests.
    await _pump(
      tester,
      _FakeSandboxApi(apps: [
        {
          'id': 'deepwiki',
          'name': 'DeepWiki',
          'icon': null,
          'config': {
            'enabled': true,
            'readMode': 'open',
            'network': 'all',
            'hosts': [],
            'daemonApi': true,
            'folders': 0,
          },
          'running': true,
          'adopted': true,
          'isolation': 'unknown',
          'pid': 18274,
          'port': 4491,
          'uptimeMs': 3600000,
          'launches': 0,
          'cpu': 0.2,
          'rssMb': 4.8,
          'processes': 1,
          'proxy': null,
        }
      ]),
    );

    expect(find.text('not running'), findsNothing);
    expect(find.textContaining('pid 18274'), findsOneWidget);
    expect(find.textContaining('adopted'), findsOneWidget);
    // No launch count is invented for a process this daemon did not start.
    expect(find.textContaining('0×'), findsNothing);
    // And the sandbox column must not claim confinement it cannot verify.
    expect(find.text('unknown'), findsOneWidget);
    expect(find.text('restart needed'), findsOneWidget);
  });
}
