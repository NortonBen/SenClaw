import 'package:appflowy_editor/appflowy_editor.dart'
    show AppFlowyEditorLocalizations;
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:senclaw_desktop/features/space/note_inline_editor.dart';
import 'package:senclaw_desktop/models/space_models.dart';
import 'package:senclaw_desktop/theme/app_theme.dart';

/// Smoke test: the inline AppFlowy editor mounts for a real note and its
/// debounced autosave fires with the edited title. (Deep block interactions —
/// checkbox toggles etc. — are covered by the pure round-trip tests.)
void main() {
  testWidgets('renders the note title and autosaves title edits',
      (tester) async {
    tester.view.physicalSize = const Size(1000, 800);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);

    final saves = <(String, String, List<String>)>[];
    await tester.pumpWidget(MaterialApp(
      localizationsDelegates: const [AppFlowyEditorLocalizations.delegate],
      theme: AppTheme.dark(),
      home: Scaffold(
        body: NoteInlineEditor(
          note: const SpaceNote(
              id: 'n1', title: 'Hello', body: '- [ ] a\n- [x] b'),
          onSave: (t, b, tags) => saves.add((t, b, tags)),
        ),
      ),
    ));
    await tester.pump();

    // Title is shown in the header field.
    expect(find.text('Hello'), findsOneWidget);
    expect(tester.takeException(), isNull);

    // The fixed formatting toolbar (undo/redo + block/inline formats) is shown.
    for (final icon in [
      Icons.undo,
      Icons.redo,
      Icons.check_box_outlined,
      Icons.format_list_bulleted,
      Icons.title,
      Icons.format_bold,
      Icons.format_italic,
    ]) {
      expect(find.byIcon(icon), findsOneWidget);
    }
    // Undo with an empty history is a safe no-op.
    await tester.tap(find.byIcon(Icons.undo));
    await tester.pump();
    expect(tester.takeException(), isNull);

    // Edit the title → after the debounce, onSave carries the new title and
    // the (round-tripped) body.
    await tester.enterText(find.byType(TextField).first, 'Hello world');
    await tester.pump(const Duration(milliseconds: 800));

    expect(saves, isNotEmpty);
    expect(saves.last.$1, 'Hello world');
    expect(saves.last.$2, contains('- [ ] a'));
    expect(saves.last.$2, contains('- [x] b'));

    // Unmount to dispose the editor (cancels its timers) before teardown.
    await tester.pumpWidget(const SizedBox());
  });
}
