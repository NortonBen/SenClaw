import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:senclaw_desktop/features/chat/widgets/slash_mention_input.dart';
import 'package:senclaw_desktop/features/plugins/plugins_screen.dart'
    show skillsProvider, SkillInfo;
import 'package:senclaw_desktop/theme/app_theme.dart';

/// The composer's `/ # @` triggers, exercised without a running daemon. The
/// daemon accepts `/name`, `#name` and `@path` alike, so a regression in any
/// one trigger silently drops that half of the feature.
void main() {
  const skills = [
    SkillInfo('agent-browser', 'Drive the connected browser', true, true, 'bundled'),
    SkillInfo('pdf', 'Work with PDF files', true, true, 'bundled'),
  ];
  const files = [
    MentionSuggestion('task-20', 'folder', null),
    MentionSuggestion('task-20/01-nghien-cuu.md', 'file', null),
    MentionSuggestion('README.md', 'file', null),
  ];

  Future<TextEditingController> pump(
    WidgetTester tester, {
    String fileScope = 'jid:web:test',
  }) async {
    tester.view.physicalSize = const Size(900, 1400);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);
    final ctrl = TextEditingController();
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          skillsProvider.overrideWith((ref) async => skills),
          mentionFilesProvider.overrideWith((ref, scope) async =>
              scope.isEmpty ? const <MentionSuggestion>[] : files),
        ],
        child: MaterialApp(
          theme: AppTheme.dark(),
          home: Scaffold(
            body: SlashMentionField(
              controller: ctrl,
              onSend: () {},
              fileScope: fileScope,
              decoration: const InputDecoration(hintText: 'Message…'),
            ),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    return ctrl;
  }

  /// Type into the field the way a user does, so the change listener runs.
  Future<void> type(WidgetTester tester, String text) async {
    await tester.tap(find.byType(TextField));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField), text);
    await tester.pumpAndSettle();
  }

  testWidgets('slash lists skills', (tester) async {
    await pump(tester);
    await type(tester, '/agent');
    expect(find.text('/agent-browser'), findsOneWidget);
    expect(find.text('/pdf'), findsNothing);
  });

  testWidgets('hash lists the same skills', (tester) async {
    await pump(tester);
    // Query stops short of the full name so the only `#pdf` on screen is the
    // popup row, not the field's own text.
    await type(tester, '#pd');
    expect(find.text('#pdf'), findsOneWidget);
    expect(find.text('#agent-browser'), findsNothing);
  });

  testWidgets('at lists workspace files', (tester) async {
    await pump(tester);
    await type(tester, '@nghien');
    expect(find.text('@task-20/01-nghien-cuu.md'), findsOneWidget);
  });

  testWidgets('no workspace means no file suggestions', (tester) async {
    await pump(tester, fileScope: '');
    await type(tester, '@nghien');
    expect(find.text('@task-20/01-nghien-cuu.md'), findsNothing);
  });

  testWidgets('picking a skill closes the popup and appends a space',
      (tester) async {
    final ctrl = await pump(tester);
    await type(tester, '/agent');
    await tester.tap(find.text('/agent-browser'));
    await tester.pumpAndSettle();
    expect(ctrl.text, '/agent-browser ');
    expect(find.text('/pdf'), findsNothing);
  });

  testWidgets('picking a folder keeps the popup open to drill deeper',
      (tester) async {
    final ctrl = await pump(tester);
    await type(tester, '@task');
    await tester.tap(find.text('@task-20'));
    await tester.pumpAndSettle();
    expect(ctrl.text, '@task-20');
    // Still filtering — the file under that folder is now the narrowed match.
    expect(find.text('@task-20/01-nghien-cuu.md'), findsOneWidget);
    expect(find.text('@README.md'), findsNothing);
  });

  testWidgets('a bare trigger mid-word does not open the popup',
      (tester) async {
    await pump(tester);
    await type(tester, 'xem https://x.dev/agent');
    expect(find.text('/agent-browser'), findsNothing);
  });
}
