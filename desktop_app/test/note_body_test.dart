import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:senclaw_desktop/widgets/note_body.dart';
import 'package:senclaw_desktop/theme/app_theme.dart';

/// Exercises the interactive Google-Keep-style checklist rendering in
/// [NoteBody]: parsing task lines, toggling in place, the completed section,
/// and inline add-item — all without a running daemon.
void main() {
  Future<String?> pump(WidgetTester tester, String body,
      {bool showAddItem = false, ValueChanged<String>? onChanged}) async {
    tester.view.physicalSize = const Size(900, 1400);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);
    String? captured;
    await tester.pumpWidget(
      MaterialApp(
        theme: AppTheme.dark(),
        home: Scaffold(
          body: SingleChildScrollView(
            child: NoteBody(
              body,
              showAddItem: showAddItem,
              onChanged: (nb) {
                captured = nb;
                onChanged?.call(nb);
              },
            ),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    return captured;
  }

  const mixed = '''
- [ ] alpha
- [ ] beta
- [x] gamma''';

  testWidgets('active items render; completed are bucketed & hidden',
      (tester) async {
    await pump(tester, mixed);
    // Two active checkboxes visible, one completed hidden behind the divider.
    expect(find.byType(Checkbox), findsNWidgets(2));
    expect(find.text('alpha'), findsOneWidget);
    expect(find.text('beta'), findsOneWidget);
    expect(find.text('gamma'), findsNothing);
    expect(find.text('1 completed'), findsOneWidget);
  });

  testWidgets('tapping an active checkbox toggles that source line to [x]',
      (tester) async {
    late String captured;
    await pump(tester, mixed, onChanged: (nb) => captured = nb);
    await tester.tap(find.byType(Checkbox).first); // "alpha"
    await tester.pumpAndSettle();
    expect(captured, contains('- [x] alpha'));
    // The others are untouched.
    expect(captured, contains('- [ ] beta'));
    expect(captured, contains('- [x] gamma'));
  });

  testWidgets('expanding completed lets you un-check a done item',
      (tester) async {
    late String captured;
    await pump(tester, mixed, onChanged: (nb) => captured = nb);
    await tester.tap(find.text('1 completed'));
    await tester.pumpAndSettle();
    expect(find.text('gamma'), findsOneWidget); // now visible
    // gamma's checkbox is the last one; ticking it flips [x] -> [ ].
    await tester.tap(find.byType(Checkbox).last);
    await tester.pumpAndSettle();
    expect(captured, contains('- [ ] gamma'));
  });

  testWidgets('inline add-item appends a new unchecked task', (tester) async {
    late String captured;
    await pump(tester, mixed, showAddItem: true, onChanged: (nb) => captured = nb);
    await tester.tap(find.text('List item'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField), 'delta');
    await tester.testTextInput.receiveAction(TextInputAction.done);
    await tester.pumpAndSettle();
    expect(captured, contains('- [ ] delta'));
    // Appended after the block, not replacing anything.
    expect(captured, contains('- [ ] alpha'));
  });

  testWidgets('read-only mode (no onChanged) renders static checkboxes',
      (tester) async {
    await tester.pumpWidget(
      MaterialApp(
        theme: AppTheme.dark(),
        home: const Scaffold(body: NoteBody(mixed)),
      ),
    );
    await tester.pumpAndSettle();
    final cb = tester.widget<Checkbox>(find.byType(Checkbox).first);
    expect(cb.onChanged, isNull); // disabled / not interactive
  });

  testWidgets('a note with no checklist still renders (markdown fallback)',
      (tester) async {
    await pump(tester, 'Just some **prose**, no boxes here.');
    expect(find.byType(Checkbox), findsNothing);
    expect(tester.takeException(), isNull);
  });
}
