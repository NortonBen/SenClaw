import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:senclaw_desktop/core/config/app_config.dart';
import 'package:senclaw_desktop/core/transport/api_client.dart';
import 'package:senclaw_desktop/core/transport/connection.dart';
import 'package:senclaw_desktop/features/plugins/plugins_screen.dart';
import 'package:senclaw_desktop/theme/app_theme.dart';

/// Records mutating calls; GETs return empty envelopes (list providers are
/// overridden directly, so nothing actually fetches).
class _RecordingApi implements ApiClient {
  final calls = <String>[];

  @override
  void updateConfig(AppConfig config) {}

  @override
  void dispose() {}

  @override
  Future<dynamic> get(String path, {Map<String, dynamic>? query}) async => {};

  @override
  Future<dynamic> post(String path, {Object? body}) async {
    calls.add('POST $path ${jsonEncode(body ?? {})}');
    return {'ok': true};
  }

  @override
  Future<dynamic> put(String path, {Object? body}) async {
    calls.add('PUT $path ${jsonEncode(body ?? {})}');
    return {'ok': true};
  }

  @override
  Future<dynamic> patch(String path, {Object? body}) async => {'ok': true};

  @override
  Future<dynamic> delete(String path, {Object? body}) async {
    calls.add('DELETE $path');
    return {'ok': true};
  }
}

// App alias first so `.first` finders target the app row deterministically.
const _appAlias = ToolAlias(
  'mcp__ssh__run',
  'mcp__ssh-manager-mcp__ssh_execute_command',
  'Run a command on a saved host',
  false,
  'app:ssh-manager',
);
const _userAlias = ToolAlias(
  'mcp__browser__navigate',
  'mcp__mini-browser-mcp__mb_navigate',
  '',
  true,
  'user',
);

Future<_RecordingApi> _pump(WidgetTester tester) async {
  tester.view.physicalSize = const Size(1200, 900);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);

  final api = _RecordingApi();
  await tester.pumpWidget(ProviderScope(
    overrides: [
      apiClientProvider.overrideWithValue(api),
      pluginsSectionProvider.overrideWith((ref) => 'alias'),
      toolAliasesProvider.overrideWith((ref) async => [_appAlias, _userAlias]),
      // `mcp__browser__navigate` is a "known" tool → its alias row must show
      // the override badge; the ssh alias stays a rename ("new name").
      knownToolNamesProvider.overrideWith((ref) async => {
            'mcp__browser__navigate',
            'mcp__senclaw-browser__browser_navigate',
          }),
    ],
    child: MaterialApp(
      theme: AppTheme.light(),
      home: const Scaffold(body: PluginsScreen()),
    ),
  ));
  await tester.pumpAndSettle();
  return api;
}

void main() {
  testWidgets('alias section renders rows with kind + source badges',
      (tester) async {
    await _pump(tester);
    expect(tester.takeException(), isNull);

    // Info banner + both rows.
    expect(find.text('Rename or override MCP tools'), findsOneWidget);
    expect(find.text('mcp__ssh__run'), findsOneWidget);
    expect(find.text('mcp__browser__navigate'), findsOneWidget);
    expect(find.text('→ mcp__ssh-manager-mcp__ssh_execute_command'),
        findsOneWidget);

    // Rename vs override classification against the known-tool set.
    expect(find.text('new name'), findsOneWidget);
    expect(find.text('override'), findsOneWidget);

    // Source badges: app-owned + user.
    expect(find.text('app: ssh-manager'), findsOneWidget);
    expect(find.text('user'), findsOneWidget);

    // Only the user row is editable; both rows can be deleted.
    expect(find.byTooltip('Edit'), findsOneWidget);
    expect(find.byIcon(Icons.delete_outline), findsNWidgets(2));

    // Switch states mirror `enabled`: app row off, user row on.
    final switches = tester.widgetList<Switch>(find.byType(Switch)).toList();
    expect(switches[0].value, isFalse);
    expect(switches[1].value, isTrue);
  });

  testWidgets('enabling an app alias POSTs the approval-gate endpoint',
      (tester) async {
    final api = await _pump(tester);
    await tester.tap(find.byType(Switch).first);
    await tester.pumpAndSettle();
    expect(
      api.calls.single,
      'POST /api/tool-aliases/mcp__ssh__run/enabled {"enabled":true}',
    );
  });

  testWidgets('deleting an app alias confirms with the re-import note',
      (tester) async {
    final api = await _pump(tester);
    await tester.tap(find.byIcon(Icons.delete_outline).first);
    await tester.pumpAndSettle();

    expect(find.text('Delete alias "mcp__ssh__run"?'), findsOneWidget);
    expect(
      find.textContaining('re-imported (disabled)'),
      findsOneWidget,
    );

    await tester.tap(find.text('Delete'));
    await tester.pumpAndSettle();
    expect(api.calls.single, 'DELETE /api/tool-aliases/mcp__ssh__run');
  });

  testWidgets('cancelling the delete dialog issues no call', (tester) async {
    final api = await _pump(tester);
    await tester.tap(find.byIcon(Icons.delete_outline).first);
    await tester.pumpAndSettle();
    await tester.tap(find.text('Cancel'));
    await tester.pumpAndSettle();
    expect(api.calls, isEmpty);
  });
}
