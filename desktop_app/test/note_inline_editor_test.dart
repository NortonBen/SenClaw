import 'package:appflowy_editor/appflowy_editor.dart'
    show AppFlowyEditorLocalizations;
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:senclaw_desktop/features/space/note_editor_blocks.dart';
import 'package:senclaw_desktop/features/space/note_inline_editor.dart';
import 'package:senclaw_desktop/models/space_models.dart';
import 'package:senclaw_desktop/theme/app_theme.dart';

/// Smoke tests: the inline AppFlowy editor mounts for a real note, shows the
/// rebuilt toolbar, renders custom checkboxes, and its debounced autosave
/// fires. (Deep markdown semantics are covered by the round-trip tests.)
void main() {
  Future<List<(String, String, List<String>)>> pumpEditor(
    WidgetTester tester, {
    String body = '- [ ] a\n- [x] b',
  }) async {
    tester.view.physicalSize = const Size(1200, 800);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);

    final saves = <(String, String, List<String>)>[];
    await tester.pumpWidget(MaterialApp(
      localizationsDelegates: const [AppFlowyEditorLocalizations.delegate],
      theme: AppTheme.dark(),
      home: Scaffold(
        body: NoteInlineEditor(
          note: SpaceNote(id: 'n1', title: 'Hello', body: body),
          onSave: (t, b, tags) => saves.add((t, b, tags)),
          onPin: () {},
          onDelete: () {},
        ),
      ),
    ));
    await tester.pump();
    return saves;
  }

  testWidgets('renders title, rebuilt toolbar and autosaves title edits',
      (tester) async {
    final saves = await pumpEditor(tester);

    // Title is shown in the header field.
    expect(find.text('Hello'), findsOneWidget);
    expect(tester.takeException(), isNull);

    // The fixed formatting toolbar: undo/redo, headings, block types, inline
    // formats and the word-count status chip.
    for (final icon in [
      Icons.undo,
      Icons.redo,
      Icons.check_box_outlined,
      Icons.format_list_bulleted,
      Icons.format_list_numbered,
      Icons.format_quote,
      Icons.horizontal_rule,
      Icons.format_bold,
      Icons.format_italic,
      Icons.format_underline,
      Icons.strikethrough_s,
      Icons.code,
      // Note actions merged into the toolbar (no separate action bar).
      Icons.push_pin_outlined,
      Icons.delete_outline,
    ]) {
      expect(find.byIcon(icon), findsOneWidget, reason: 'missing $icon');
    }
    for (final label in ['H1', 'H2', 'H3']) {
      expect(find.text(label), findsOneWidget);
    }
    expect(find.textContaining('từ'), findsOneWidget);

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

  testWidgets('todo items render tappable custom checkboxes that persist',
      (tester) async {
    final saves = await pumpEditor(tester);

    // Both todo blocks use the themed NoteCheckbox (not appflowy's SVG).
    expect(find.byType(NoteCheckbox), findsNWidgets(2));

    // Tick the first (unchecked) box → autosave flips it to `[x]`.
    await tester.tap(find.byType(NoteCheckbox).first);
    await tester.pump(const Duration(milliseconds: 800));

    expect(saves, isNotEmpty);
    expect(saves.last.$2, contains('- [x] a'));
    expect(saves.last.$2, contains('- [x] b'));

    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('loose-list note bodies mount without empty todo blocks',
      (tester) async {
    // Regression: bodies written loose by the web UI / AI previously decoded
    // into empty "To-do" placeholders + orphan paragraphs.
    await pumpEditor(
      tester,
      body: '- [ ] set nổ hũ nhưng không nhận\n\n- [ ] Fix napas Bin',
    );

    expect(find.byType(NoteCheckbox), findsNWidgets(2));
    expect(find.textContaining('set nổ hũ', findRichText: true), findsOneWidget);
    expect(find.textContaining('Fix napas Bin', findRichText: true),
        findsOneWidget);
    expect(tester.takeException(), isNull);

    await tester.pumpWidget(const SizedBox());
  });
}
