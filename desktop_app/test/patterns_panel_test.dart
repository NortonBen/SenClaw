// Plugins → Patterns (desktop). Covers the three things the panel exists to
// communicate, which are all daemon decisions the UI must not soften:
//
//  1. The `user` source is resolved first, so a pattern it holds SHADOWS the
//     git copy of the same name. If the row does not say so, "I edited it and
//     nothing changed" has no visible cause.
//  2. A git source is read-only — no delete button, and a tooltip saying where
//     to put the edit instead.
//  3. `dryRun` renders without spending a model call, and the language default
//     is "follow the input" (Fabric pins English output otherwise).

import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:senclaw_desktop/core/config/app_config.dart';
import 'package:senclaw_desktop/core/transport/api_client.dart';
import 'package:senclaw_desktop/core/transport/connection.dart';
import 'package:senclaw_desktop/features/plugins/patterns_panel.dart';
import 'package:senclaw_desktop/features/plugins/plugins_screen.dart';
import 'package:senclaw_desktop/theme/app_theme.dart';

/// Serves the one pattern the dialog opens and records every mutation.
class _FakeApi implements ApiClient {
  final calls = <String>[];

  @override
  void updateConfig(AppConfig config) {}

  @override
  void dispose() {}

  @override
  Future<dynamic> get(String path,
      {Map<String, dynamic>? query, Duration? timeout}) async {
    calls.add('GET $path');
    if (path.startsWith('/api/patterns/')) {
      return {
        'pattern': {
          'name': 'summarize',
          'source': 'user',
          'system': '# IDENTITY and PURPOSE\n\nSummarise content.\n\n# INPUT:',
          'path': '/home/u/.senclaw/patterns/user/summarize',
          'writable': true,
        },
        'source': {'id': 'user'},
      };
    }
    return {};
  }

  @override
  Future<dynamic> post(String path, {Object? body}) async {
    calls.add('POST $path ${jsonEncode(body ?? {})}');
    if (path == '/api/patterns/run') {
      return {
        'ok': true,
        'dryRun': true,
        'rendered': {
          'system': '# IDENTITY\n\n# LANGUAGE\n\nsame language as the INPUT',
          'user': 'xin chào',
          'unresolved': <String>[],
        },
      };
    }
    return {
      'ok': true,
      'sync': {'patterns': 255},
    };
  }

  @override
  Future<dynamic> put(String path, {Object? body}) async => {'ok': true};

  @override
  Future<dynamic> patch(String path, {Object? body}) async => {'ok': true};

  @override
  Future<dynamic> delete(String path, {Object? body}) async {
    calls.add('DELETE $path');
    return {'ok': true};
  }
}

const _view = PatternsView(
  patterns: [
    // Shadowing case: the same name exists in `fabric`, but `user` wins.
    PatternRow(
      name: 'summarize',
      source: 'user',
      description: 'Summarise content.',
      shadowedIn: ['fabric'],
      writable: true,
    ),
    // Read-only case: straight from the checkout.
    PatternRow(
      name: 'extract_wisdom',
      source: 'fabric',
      description: 'Extract surprising insights.',
      shadowedIn: [],
      writable: false,
    ),
  ],
  sources: [
    PatternSourceRow(
      id: 'user',
      name: 'My patterns',
      kind: 'local',
      url: null,
      gitRef: 'main',
      enabled: true,
      count: 1,
      writable: true,
      lastError: null,
    ),
    PatternSourceRow(
      id: 'fabric',
      name: 'Fabric',
      kind: 'git',
      url: 'https://github.com/danielmiessler/fabric',
      gitRef: 'v1.4.470',
      enabled: true,
      count: 255,
      writable: false,
      lastError: null,
    ),
  ],
  strategies: [
    StrategyRow(name: 'cot', description: 'Chain-of-Thought (CoT) Prompting'),
  ],
  // Already installed, so the catalog contributes no "install me" card here —
  // the fixture is the populated case, not the fresh-daemon one.
  catalog: [
    CatalogEntry(
      id: 'senclaw',
      name: 'Thư viện đi kèm',
      description: '261 pattern, cài offline',
      kind: 'bundled',
      count: 261,
      license: 'MIT',
      gitRef: null,
      installed: true,
      pinned: true,
    ),
  ],
);

Future<_FakeApi> _pump(WidgetTester tester, {PatternsView view = _view}) async {
  tester.view.physicalSize = const Size(1400, 1000);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);

  final api = _FakeApi();
  await tester.pumpWidget(ProviderScope(
    overrides: [
      apiClientProvider.overrideWithValue(api),
      pluginsSectionProvider.overrideWith((ref) => 'patterns'),
      patternsProvider.overrideWith((ref) async => view),
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
  testWidgets('rows show the source, and a shadowed name says so',
      (tester) async {
    await _pump(tester);
    expect(tester.takeException(), isNull);

    expect(find.text('summarize'), findsOneWidget);
    expect(find.text('extract_wisdom'), findsOneWidget);
    // The whole reason the field exists: an edit that appears to do nothing
    // must have a visible cause.
    expect(find.text('shadows 1 other source(s)'), findsOneWidget);
  });

  testWidgets('a git-sourced pattern offers no delete button', (tester) async {
    await _pump(tester);
    // The read-only row shows a lock instead of a delete: removing from a
    // checkout would be undone by the next sync.
    expect(find.byIcon(Icons.lock_outline), findsOneWidget);
  });

  testWidgets('the source strip shows each source with its count',
      (tester) async {
    await _pump(tester);
    expect(find.text('Fabric'), findsOneWidget);
    expect(find.text('255'), findsOneWidget);
    // The chip has no room for the URL, so where it points and which ref it
    // pins live in the tooltip rather than being dropped.
    expect(
      find.byTooltip('https://github.com/danielmiessler/fabric @ v1.4.470'),
      findsOneWidget,
    );
  });

  testWidgets('syncing a git source posts to that source only', (tester) async {
    final api = await _pump(tester);
    // Management buttons only appear on the selected chip — that is what keeps
    // the strip readable with several sources.
    expect(find.byIcon(Icons.sync), findsNothing);
    await tester.tap(find.text('Fabric'));
    await tester.pumpAndSettle();
    await tester.tap(find.byIcon(Icons.sync));
    await tester.pumpAndSettle();

    expect(
      api.calls
          .where((c) => c.startsWith('POST /api/patterns/sources/fabric/sync')),
      hasLength(1),
    );
  });

  testWidgets('preview renders without spending a model call', (tester) async {
    final api = await _pump(tester);
    await tester.tap(find.text('summarize'));
    await tester.pumpAndSettle();

    await tester.enterText(find.byType(TextField).first, 'xin chào');
    await tester.tap(find.text('Preview prompt'));
    await tester.pumpAndSettle();

    final run =
        api.calls.firstWhere((c) => c.startsWith('POST /api/patterns/run'));
    final body = jsonDecode(run.substring('POST /api/patterns/run '.length))
        as Map<String, dynamic>;
    expect(body['dryRun'], isTrue);
    expect(body['name'], 'summarize');
    // Fabric patterns pin English output in their own instructions, so the
    // default must be to follow the input's language.
    expect(body['language'], 'auto');
  });

  testWidgets('an empty daemon points at adding a source', (tester) async {
    await _pump(
      tester,
      view: const PatternsView(
          patterns: [], sources: [], strategies: [], catalog: []),
    );
    expect(find.textContaining('add a git source'), findsOneWidget);
  });
}
