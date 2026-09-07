// The right dock's Console feed. Sub-agent activity arrives as markdown —
// headings, bullets, `code`, links — and it used to render as one unbroken
// monospace paragraph with no way to get the text back out. Covers the two
// things that fixed:
//
//  1. Markdown is rendered (default), and the toggle drops back to raw mono
//     text with the bullet prefix, persisting the choice.
//  2. Every line copies, and so does the whole feed.

import 'dart:async';

import 'package:flutter/gestures.dart' show PointerDeviceKind;
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:gpt_markdown/gpt_markdown.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:senclaw_desktop/core/config/app_config.dart';
import 'package:senclaw_desktop/core/prefs.dart';
import 'package:senclaw_desktop/core/transport/connection.dart';
import 'package:senclaw_desktop/core/transport/ws_client.dart';
import 'package:senclaw_desktop/features/dock/right_dock.dart';
import 'package:senclaw_desktop/theme/app_theme.dart';

/// A WsClient whose event stream the test drives — no real socket.
class _FakeWs extends WsClient {
  _FakeWs() : super(const AppConfig(host: 'x', uiPort: 0, wsPort: 0));
  final _ec = StreamController<WsEvent>.broadcast();
  @override
  Stream<WsEvent> get events => _ec.stream;
  @override
  Stream<WsStatus> get statusStream => const Stream<WsStatus>.empty();
  @override
  void send(Map<String, dynamic> msg) {}
  @override
  void dispose() => _ec.close();
  void emit(WsEvent e) => _ec.add(e);
}

const _md = '**Bold heading**\n\n* one bullet\n* two bullet';

/// One `dispatch:activity` frame, in the shape DispatchNotifier parses.
Map<String, dynamic> _activity(String text) => {
      'type': 'dispatch:activity',
      'taskId': 't1',
      'entry': {'entryType': 'message', 'text': text},
    };

Future<_FakeWs> _pump(WidgetTester tester,
    {Map<String, Object> prefs = const {}}) async {
  tester.view.physicalSize = const Size(1200, 900);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);

  // A dock wide enough for the tab bar: at the 320px minimum that Row
  // overflows on its own, which would mask everything asserted here.
  SharedPreferences.setMockInitialValues({'chat:dockWidth': '760', ...prefs});
  final sp = await SharedPreferences.getInstance();
  final ws = _FakeWs();

  await tester.pumpWidget(ProviderScope(
    overrides: [
      prefsProvider.overrideWithValue(sp),
      wsClientProvider.overrideWithValue(ws),
    ],
    child: MaterialApp(
      theme: AppTheme.light(),
      home: const Scaffold(body: RightDock(jid: 'web:main')),
    ),
  ));
  ws.emit(_activity(_md));
  await tester.pumpAndSettle();
  return ws;
}

void main() {
  testWidgets('activity renders as markdown by default', (tester) async {
    await _pump(tester);
    expect(tester.takeException(), isNull);

    expect(find.byType(GptMarkdown), findsOneWidget);
    // Raw mode's bullet-prefixed mono line must NOT be what is shown.
    expect(find.text('• $_md'), findsNothing);
  });

  testWidgets('the toggle switches to raw text and persists the choice',
      (tester) async {
    await _pump(tester);

    await tester.tap(find.byTooltip('View as plain text'));
    await tester.pumpAndSettle();

    expect(find.byType(GptMarkdown), findsNothing);
    expect(find.text('• $_md'), findsOneWidget);

    // Persisted, so a restart comes back in raw mode.
    final sp = await SharedPreferences.getInstance();
    expect(sp.getString('chat:consoleMarkdown'), '0');
  });

  testWidgets('a stored raw-mode preference is honoured on start',
      (tester) async {
    await _pump(tester, prefs: {'chat:consoleMarkdown': '0'});
    expect(find.byType(GptMarkdown), findsNothing);
    expect(find.byTooltip('View as markdown'), findsOneWidget);
  });

  testWidgets('one line and the whole feed both copy', (tester) async {
    String? copied;
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
        SystemChannels.platform, (call) async {
      if (call.method == 'Clipboard.setData') {
        copied = (call.arguments as Map)['text'] as String?;
      }
      return null;
    });
    addTearDown(() => tester.binding.defaultBinaryMessenger
        .setMockMethodCallHandler(SystemChannels.platform, null));

    final ws = await _pump(tester);
    ws.emit(_activity('second entry'));
    await tester.pumpAndSettle();

    // Per-line copy, on the newest line. The button is hidden until the
    // pointer is over that line.
    final copyBtn = find.byTooltip('Copy').last;
    double opacity() => tester
        .widget<AnimatedOpacity>(
            find.ancestor(of: copyBtn, matching: find.byType(AnimatedOpacity)))
        .opacity;
    expect(opacity(), 0);

    final gesture = await tester.createGesture(kind: PointerDeviceKind.mouse);
    await gesture.addPointer(location: Offset.zero);
    addTearDown(gesture.removePointer);
    await gesture.moveTo(tester.getCenter(copyBtn));
    await tester.pumpAndSettle();
    expect(opacity(), 1);

    // Copies that line's raw markdown source, not the rendered text.
    await tester.tap(copyBtn);
    await tester.pumpAndSettle();
    expect(copied, 'second entry');

    // Copy-all joins the feed in order.
    await tester.tap(find.byTooltip('Copy all activity'));
    await tester.pumpAndSettle();
    expect(copied, '$_md\n\nsecond entry');
  });
}
