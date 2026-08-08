import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:senclaw_desktop/core/config/app_config.dart';
import 'package:senclaw_desktop/core/transport/api_client.dart';
import 'package:senclaw_desktop/core/transport/connection.dart';
import 'package:senclaw_desktop/features/plugins/space_app_runtime_panel.dart';
import 'package:senclaw_desktop/theme/app_theme.dart';

/// Serves one `/runtime` snapshot and records what the panel asks for.
class _FakeApi implements ApiClient {
  _FakeApi({
    this.running = true,
    this.healthOk = true,
    this.launches = 1,
    this.connections = const [],
    this.proxy,
    this.fail = false,
  });

  final bool running;
  final bool healthOk;
  final int launches;
  final List<Map<String, dynamic>> connections;
  final Map<String, dynamic>? proxy;
  final bool fail;
  final calls = <String>[];

  @override
  void updateConfig(AppConfig config) {}

  @override
  void dispose() {}

  @override
  Future<dynamic> get(String path, {Map<String, dynamic>? query, Duration? timeout}) async {
    calls.add('GET $path');
    if (fail) throw Exception('daemon is down');
    return {
      'appId': 'ba',
      'running': running,
      'launches': launches,
      'process': running
          ? {
              'pid': 4242,
              'pgid': 4242,
              'port': 4740,
              'url': 'http://127.0.0.1:4740',
              'uptimeMs': 95000,
              'isolation': 'seatbelt',
            }
          : null,
      'health': running
          ? (healthOk
              ? {'url': 'http://127.0.0.1:4740/', 'ok': true, 'status': 200, 'ms': 7}
              : {'url': 'http://127.0.0.1:4740/', 'ok': false, 'status': 500, 'ms': 12})
          : null,
      'resources': running
          ? {
              'source': 'host',
              'cpu': 12.5,
              'rssMb': 88.25,
              'running': true,
              'note': null,
              'processes': [
                {
                  'pid': 4242,
                  'ppid': 1,
                  'cpu': 12.5,
                  'memPercent': 0.5,
                  'rssMb': 88.25,
                  'elapsed': '01:35',
                  'command': './ba',
                }
              ],
            }
          : null,
      'network': {'connections': connections, 'note': null, 'proxy': proxy},
      'sandbox': {'enabled': true, 'readMode': 'open', 'network': 'all', 'hosts': []},
      'log': {'path': '/tmp/app/.senclaw/runtime.log', 'bytes': 2048},
      'launch': {
        'cwd': '/tmp/app',
        'command': './ba',
        'env': [
          ['PORT', '4740'],
          ['SENCLAW_BASE_URL', 'http://127.0.0.1:18788'],
        ],
      },
    };
  }

  @override
  Future<dynamic> post(String path, {Object? body}) async {
    calls.add('POST $path');
    return {'ok': true};
  }

  @override
  Future<dynamic> put(String path, {Object? body}) async => {'ok': true};

  @override
  Future<dynamic> patch(String path, {Object? body}) async => {'ok': true};

  @override
  Future<dynamic> delete(String path, {Object? body}) async => {'ok': true};
}

Future<void> _pump(WidgetTester tester, _FakeApi api) async {
  tester.view.physicalSize = const Size(1400, 2000);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(ProviderScope(
    overrides: [apiClientProvider.overrideWithValue(api)],
    child: MaterialApp(
      theme: AppTheme.light(),
      home: const Scaffold(
        body: SingleChildScrollView(child: SpaceAppRuntimePanel(appId: 'ba')),
      ),
    ),
  ));
  await tester.pump(); // let the first load resolve
  await tester.pump(const Duration(milliseconds: 50));
}

void main() {
  testWidgets('reports process, cpu/ram and the launch details', (tester) async {
    final api = _FakeApi();
    await _pump(tester, api);
    expect(api.calls.first, 'GET /api/space/apps/ba/runtime');

    expect(find.text('running'), findsOneWidget);
    expect(find.text('pid 4242'), findsOneWidget);
    expect(find.text('port 4740'), findsOneWidget);
    expect(find.text('up 1m 35s'), findsOneWidget);
    expect(find.text('sandbox: seatbelt'), findsOneWidget);
    expect(find.textContaining('health 200'), findsOneWidget);
    expect(find.text('CPU 12.5%'), findsOneWidget);
    expect(find.text('RAM 88.3 MB'), findsOneWidget);
    // The launch is reproducible by hand from what is shown.
    expect(find.text('/tmp/app'), findsOneWidget);
    expect(find.text('PORT=4740'), findsOneWidget);
    expect(find.textContaining('runtime.log'), findsOneWidget);

    // Polling keeps it live rather than showing a frozen first sample.
    await tester.pump(const Duration(seconds: 3));
    await tester.pump();
    expect(api.calls.where((c) => c.startsWith('GET')).length, greaterThan(1));
  });

  testWidgets('an app that answers 500 does not read as healthy', (tester) async {
    await _pump(tester, _FakeApi(healthOk: false));
    expect(
      find.text('running but not answering'),
      findsOneWidget,
      reason: 'a live process that fails its health check is the case worth catching',
    );
  });

  testWidgets('a climbing launch count is called out as a crash loop', (tester) async {
    await _pump(tester, _FakeApi(launches: 12));
    expect(find.text('12 launches'), findsOneWidget);
    expect(find.textContaining('dying and being restarted'), findsOneWidget);
  });

  testWidgets('sockets are listed, with the listener distinguishable', (tester) async {
    await _pump(
      tester,
      _FakeApi(connections: [
        {
          'pid': 4242,
          'command': 'ba',
          'proto': 'TCP',
          'local': '127.0.0.1:4740',
          'remote': null,
          'state': 'LISTEN',
        },
        {
          'pid': 4242,
          'command': 'ba',
          'proto': 'TCP',
          'local': '192.168.1.5:5111',
          'remote': '142.250.1.1:443',
          'state': 'ESTABLISHED',
        },
      ]),
    );
    expect(find.text('LISTEN'), findsOneWidget);
    expect(find.text('142.250.1.1:443'), findsOneWidget);
    expect(find.text('—'), findsOneWidget, reason: 'a listener has no peer');
  });

  testWidgets('the allowlist proxy counters are shown when there is one',
      (tester) async {
    await _pump(
      tester,
      _FakeApi(proxy: {
        'port': 51234,
        'stats': {'allowed': 9, 'denied': 2, 'recentDenied': ['x.example']},
      }),
    );
    expect(
      find.textContaining('127.0.0.1:51234'),
      findsOneWidget,
      reason: 'the proxy is the only reason a sandboxed app can reach anything',
    );
  });

  testWidgets('a stopped app says so instead of showing stale numbers',
      (tester) async {
    await _pump(tester, _FakeApi(running: false));
    expect(find.text('not running'), findsOneWidget);
    expect(find.text('The app is not running'), findsOneWidget);
    expect(find.textContaining('CPU '), findsNothing);
  });

  testWidgets('a daemon that cannot answer surfaces the error', (tester) async {
    await _pump(tester, _FakeApi(fail: true));
    expect(find.textContaining('Cannot read the state'), findsOneWidget);
  });
}
