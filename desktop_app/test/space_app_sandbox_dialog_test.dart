import 'dart:convert';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:senclaw_desktop/core/config/app_config.dart';
import 'package:senclaw_desktop/core/transport/api_client.dart';
import 'package:senclaw_desktop/core/transport/connection.dart';
import 'package:senclaw_desktop/features/plugins/space_app_sandbox_dialog.dart';
import 'package:senclaw_desktop/theme/app_theme.dart';

/// Answers `GET /api/space/apps/<id>/sandbox` and records mutations, so the
/// tests can assert on exactly what the dialog would send the daemon.
class _FakeApi implements ApiClient {
  _FakeApi({
    this.enabled = false,
    this.network = 'all',
    this.enforceable = true,
    this.networkEnforceable = true,
    this.note,
    this.proxy,
  });

  final bool enabled;
  final String network;
  final bool enforceable;
  final bool networkEnforceable;
  final String? note;
  final Map<String, dynamic>? proxy;
  final calls = <String>[];

  @override
  void updateConfig(AppConfig config) {}

  @override
  void dispose() {}

  @override
  Future<dynamic> get(String path, {Map<String, dynamic>? query, Duration? timeout}) async {
    calls.add('GET $path');
    return {
      'appId': 'demo',
      'config': {
        'enabled': enabled,
        'readMode': 'open',
        'folders': [
          {'path': '/Users/u/Documents/shared', 'readOnly': true},
        ],
        'network': network,
        'hosts': network == 'hosts' ? ['api.openai.com'] : <String>[],
        'daemonApi': true,
        'loopback': <int>[],
      },
      'effective': {
        'isolation': enforceable ? 'seatbelt' : 'unsupported',
        'enforceable': enforceable,
        'networkEnforceable': networkEnforceable,
        'note': note,
        'alwaysGranted': [
          '/Users/u/.senclaw/workspace/space-apps/demo',
          '/Users/u/.senclaw/apps/demo',
        ],
        'daemonPort': 18788,
      },
      'proxy': proxy,
    };
  }

  @override
  Future<dynamic> post(String path, {Object? body}) async {
    calls.add('POST $path');
    return {'ok': true};
  }

  @override
  Future<dynamic> put(String path, {Object? body}) async {
    calls.add('PUT $path ${jsonEncode(body ?? {})}');
    return {'ok': true, 'config': body, 'restartRequired': true};
  }

  @override
  Future<dynamic> patch(String path, {Object? body}) async => {'ok': true};

  @override
  Future<dynamic> delete(String path, {Object? body}) async {
    calls.add('DELETE $path');
    return {'ok': true};
  }
}

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

Future<void> _pump(WidgetTester tester, _FakeApi api) async {
  tester.view.physicalSize = const Size(1400, 2400);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(ProviderScope(
    overrides: [apiClientProvider.overrideWithValue(api)],
    child: MaterialApp(
      theme: AppTheme.light(),
      home: const Scaffold(
        body: SpaceAppSandboxDialog(appId: 'demo', appName: 'Demo App'),
      ),
    ),
  ));
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('shows the three questions and what is always granted',
      (tester) async {
    final api = _FakeApi(enabled: true);
    await _pump(tester, api);
    expect(tester.takeException(), isNull);
    expect(api.calls, ['GET /api/space/apps/demo/sandbox']);

    expect(find.text('Run this app inside the sandbox'), findsOneWidget);
    expect(find.text('Folders'), findsOneWidget);
    expect(find.text('Network'), findsOneWidget);
    // The app's own folders are shown as granted without being editable, so the
    // dialog never looks like the app was given nothing.
    expect(find.text('/Users/u/.senclaw/apps/demo'), findsOneWidget);
    expect(find.text('/Users/u/Documents/shared'), findsOneWidget);
    expect(find.text('Only these sites'), findsOneWidget);
  });

  testWidgets('a machine that cannot enforce says so before anything is saved',
      (tester) async {
    await _pump(tester, _FakeApi(enabled: true, enforceable: false));
    expect(
      find.textContaining('cannot confine a Space App'),
      findsOneWidget,
      reason: 'a stored-but-unenforced switch must not read as isolation',
    );
  });

  testWidgets('the platform note from the daemon is surfaced', (tester) async {
    await _pump(
        tester,
        _FakeApi(
          enabled: true,
          network: 'hosts',
          networkEnforceable: false,
          note: 'On Linux the network restriction is NOT enforced.',
        ));
    expect(find.textContaining('NOT enforced'), findsOneWidget);
    expect(find.textContaining('site list is not enforced'), findsOneWidget);
  });

  testWidgets('saving sends the whole config and can restart the app',
      (tester) async {
    final api = _FakeApi(enabled: true, network: 'hosts');
    await _pump(tester, api);

    await tester.tap(find.text('Save & restart app'));
    await tester.pumpAndSettle();

    final put = api.calls.firstWhere((c) => c.startsWith('PUT'));
    final body = jsonDecode(put.substring(put.indexOf('{')));
    expect(body['enabled'], true);
    expect(body['network'], 'hosts');
    expect(body['hosts'], ['api.openai.com']);
    expect(body['daemonApi'], true);
    expect(body['folders'], [
      {'path': '/Users/u/Documents/shared', 'readOnly': true}
    ]);
    // A profile is fixed at launch, so "restart" is a real part of saving.
    expect(api.calls, contains('POST /api/space/apps/demo/restart'));
  });

  testWidgets('a refused host can be added straight from what the proxy blocked',
      (tester) async {
    final api = _FakeApi(
      enabled: true,
      network: 'hosts',
      proxy: {
        'port': 51234,
        'stats': {
          'allowed': 3,
          'denied': 2,
          'recentDenied': ['telemetry.example'],
        },
      },
    );
    await _pump(tester, api);
    expect(find.textContaining('127.0.0.1:51234'), findsOneWidget);

    // The chip sits at the bottom of a scrolling body: a tap on an off-screen
    // widget silently misses, which is why this scrolls first.
    await tester.ensureVisible(find.text('+ telemetry.example'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('+ telemetry.example'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Save'));
    await tester.pumpAndSettle();

    final put = api.calls.firstWhere((c) => c.startsWith('PUT'));
    final body = jsonDecode(put.substring(put.indexOf('{')));
    expect(body['hosts'], ['api.openai.com', 'telemetry.example'],
        reason: 'one tap on what was blocked must be enough to allow it');
  });

  testWidgets('the folder picker adds a read+write grant', (tester) async {
    FilePicker.platform = _FakeDirPicker('/Users/u/Projects/data');
    addTearDown(() => FilePicker.platform = _FakeDirPicker(null));
    final api = _FakeApi(enabled: true);
    await _pump(tester, api);

    await tester.tap(find.text('Add folder'));
    await tester.pumpAndSettle();
    expect(find.text('/Users/u/Projects/data'), findsOneWidget);

    await tester.tap(find.text('Save'));
    await tester.pumpAndSettle();
    final put = api.calls.firstWhere((c) => c.startsWith('PUT'));
    final body = jsonDecode(put.substring(put.indexOf('{')));
    expect(body['folders'], [
      {'path': '/Users/u/Documents/shared', 'readOnly': true},
      {'path': '/Users/u/Projects/data', 'readOnly': false},
    ]);
  });

  testWidgets('with the sandbox off the controls are inert', (tester) async {
    // Asserted behaviourally: the network radios are dimmed *and* ignore input
    // while the app is not confined, so a tap must not change what gets saved.
    final api = _FakeApi(enabled: false);
    await _pump(tester, api);

    await tester.tap(find.text('Only these sites'), warnIfMissed: false);
    await tester.pumpAndSettle();
    await tester.tap(find.text('Save'));
    await tester.pumpAndSettle();

    final put = api.calls.firstWhere((c) => c.startsWith('PUT'));
    final body = jsonDecode(put.substring(put.indexOf('{')));
    expect(body['enabled'], false);
    expect(body['network'], 'all', reason: 'the tap must not have registered');
  });
}
