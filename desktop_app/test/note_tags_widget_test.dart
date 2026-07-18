import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:senclaw_desktop/features/space/space_screen.dart';
import 'package:senclaw_desktop/features/space/space_providers.dart';
import 'package:senclaw_desktop/models/space_models.dart';
import 'package:senclaw_desktop/theme/app_theme.dart';
import 'package:senclaw_desktop/core/transport/api_client.dart';
import 'package:senclaw_desktop/core/transport/connection.dart';
import 'package:senclaw_desktop/core/config/app_config.dart';

/// Captures the note-create POST instead of hitting the daemon.
class _RecordApi extends ApiClient {
  _RecordApi() : super(const AppConfig(host: '127.0.0.1', uiPort: 1, wsPort: 2));
  Object? lastBody;
  @override
  Future<dynamic> post(String path, {Object? body}) async {
    lastBody = body;
    return <String, dynamic>{'id': 'note-new'};
  }
}

void main() {
  Object? nonOverflow(WidgetTester tester) {
    final e = tester.takeException();
    if (e == null) return null;
    return e.toString().contains('overflowed') ? null : e;
  }

  testWidgets('Save merges chip tags + body #hashtags, normalised & deduped',
      (tester) async {
    tester.view.physicalSize = const Size(1400, 1400);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);

    final api = _RecordApi();
    await tester.pumpWidget(ProviderScope(
      overrides: [apiClientProvider.overrideWithValue(api)],
      child: MaterialApp(
        theme: AppTheme.dark(),
        home: Scaffold(
          body: Builder(
            builder: (ctx) => Center(
              child: ElevatedButton(
                onPressed: () => showCreateNoteDialog(ctx),
                child: const Text('open'),
              ),
            ),
          ),
        ),
      ),
    ));
    await tester.tap(find.text('open'));
    await tester.pumpAndSettle();

    // Editor field order: [0] title, [1] body, [2] tag input.
    final fields = find.byType(TextField);
    await tester.enterText(fields.at(0), 'My note');
    await tester.enterText(fields.at(1), 'buy milk #Shopping and #urgent #shopping');
    // Add an explicit chip via the tag input.
    await tester.enterText(fields.at(2), 'work');
    await tester.testTextInput.receiveAction(TextInputAction.done);
    await tester.pumpAndSettle();
    expect(nonOverflow(tester), isNull);
    // The chip is now shown.
    expect(find.text('#work'), findsOneWidget);

    await tester.tap(find.text('Save'));
    await tester.pumpAndSettle();

    final body = api.lastBody as Map<String, dynamic>;
    final tags = (body['tags'] as List).cast<String>();
    // work (chip) + shopping/urgent (body), lower-cased & deduped.
    expect(tags, ['work', 'shopping', 'urgent']);
    expect(body['title'], 'My note');
  });

  testWidgets('tag filter bar narrows the note list', (tester) async {
    tester.view.physicalSize = const Size(1400, 1400);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);

    final seed = [
      const SpaceNote(id: 'a', title: 'Alpha', tags: ['work']),
      const SpaceNote(id: 'b', title: 'Bravo', tags: ['home']),
    ];
    await tester.pumpWidget(ProviderScope(
      overrides: [notesProvider.overrideWith((ref) async => seed)],
      child: MaterialApp(
        theme: AppTheme.dark(),
        home: const Scaffold(body: NotesScreen()),
      ),
    ));
    await tester.pumpAndSettle();

    // Both notes visible, and the filter bar offers All / #home / #work.
    expect(find.text('Alpha'), findsOneWidget);
    expect(find.text('Bravo'), findsOneWidget);
    expect(find.text('#work'), findsWidgets); // filter chip + list subtitle

    // The filter bar is rendered before the list, so `.first` is its chip.
    await tester.tap(find.text('#work').first);
    await tester.pumpAndSettle();

    // Only the #work note remains.
    expect(find.text('Alpha'), findsOneWidget);
    expect(find.text('Bravo'), findsNothing);
  });
}
