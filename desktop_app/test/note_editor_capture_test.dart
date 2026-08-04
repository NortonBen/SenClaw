import 'dart:io';
import 'dart:ui' as ui;

import 'package:appflowy_editor/appflowy_editor.dart'
    show AppFlowyEditorLocalizations;
import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:senclaw_desktop/features/space/note_inline_editor.dart';
import 'package:senclaw_desktop/models/space_models.dart';
import 'package:senclaw_desktop/theme/app_theme.dart';

/// Offscreen screenshot harness for the rebuilt note editor — NOT a regular
/// test. Renders [NoteInlineEditor] with real fonts and dumps PNGs so the
/// editor can be visually verified without driving the live app.
///
/// Run with:
///   CAPTURE_NOTE_SHOTS=1 CAPTURE_OUT=/tmp flutter test test/note_editor_capture_test.dart
///
/// Without CAPTURE_NOTE_SHOTS=1 every test here is a no-op, so the normal
/// suite stays fast and font-independent.
void main() {
  final capture = Platform.environment['CAPTURE_NOTE_SHOTS'] == '1';
  final outDir = Platform.environment['CAPTURE_OUT'] ?? '/tmp';

  const fontDir = '/opt/homebrew/share/flutter/bin/cache/artifacts/material_fonts';

  Future<void> loadRealFonts() async {
    Future<void> loadAs(String family, List<String> files) async {
      final loader = FontLoader(family);
      for (final f in files) {
        final file = File('$fontDir/$f');
        if (!file.existsSync()) continue;
        final bytes = file.readAsBytesSync();
        loader.addFont(Future.value(ByteData.view(bytes.buffer)));
      }
      await loader.load();
    }

    const roboto = [
      'Roboto-Regular.ttf',
      'Roboto-Bold.ttf',
      'Roboto-Italic.ttf',
      'Roboto-Medium.ttf',
      'Roboto-Light.ttf',
    ];
    // Cover every family the widget tree can resolve to: explicit ones, the
    // app's UI font, and the platform defaults used when family is null.
    for (final family in [
      'Roboto',
      'AntUi',
      'SFMono-Regular',
      '.AppleSystemUIFont',
      '.SF NS',
      'CupertinoSystemText',
      'CupertinoSystemDisplay',
    ]) {
      await loadAs(family, roboto);
    }
    await loadAs('MaterialIcons', ['MaterialIcons-Regular.otf']);
  }

  Future<void> shoot(
    WidgetTester tester, {
    required String file,
    required ThemeData theme,
    required SpaceNote note,
  }) async {
    tester.view.physicalSize = const Size(2280, 1520);
    tester.view.devicePixelRatio = 2.0;
    addTearDown(tester.view.reset);

    final key = GlobalKey();
    await tester.runAsync(loadRealFonts);

    // Route every themed style to the loaded Roboto so glyphs are real (the
    // test binding otherwise renders the block-glyph FlutterTest font).
    final themed = theme.copyWith(
      textTheme: theme.textTheme.apply(fontFamily: 'Roboto'),
    );

    await tester.pumpWidget(RepaintBoundary(
      key: key,
      child: MaterialApp(
        debugShowCheckedModeBanner: false,
        localizationsDelegates: const [AppFlowyEditorLocalizations.delegate],
        theme: themed,
        home: Scaffold(
          body: NoteInlineEditor(
            note: note,
            onSave: (_, _, _) {},
            onPin: () {},
            onDelete: () {},
          ),
        ),
      ),
    ));
    await tester.pump(const Duration(milliseconds: 300));

    await tester.runAsync(() async {
      final boundary =
          key.currentContext!.findRenderObject()! as RenderRepaintBoundary;
      final image = await boundary.toImage(pixelRatio: 2.0);
      final bytes = await image.toByteData(format: ui.ImageByteFormat.png);
      File('$outDir/$file').writeAsBytesSync(bytes!.buffer.asUint8List());
    });

    // Unmount to dispose editor timers before teardown.
    await tester.pumpWidget(const SizedBox());
  }

  testWidgets('capture: user TODO note (light)', (tester) async {
    if (!capture) return;
    await shoot(
      tester,
      file: 'note_editor_light.png',
      theme: AppTheme.light(),
      note: const SpaceNote(
        id: 'shot1',
        title: 'TODO',
        tags: ['todo'],
        // Loose list exactly as other frontends write it — previously decoded
        // into empty "To-do" placeholders + orphan paragraphs.
        body: '- [ ] set nổ hũ nhưng không nhận\n\n'
            '- đề vs 3 càng không đánh được\n\n'
            '- [ ] Fix napas Bin',
      ),
    );
  });

  testWidgets('capture: showcase note (dark)', (tester) async {
    if (!capture) return;
    await shoot(
      tester,
      file: 'note_editor_dark.png',
      theme: AppTheme.dark(),
      note: const SpaceNote(
        id: 'shot2',
        title: 'Kế hoạch tuần',
        tags: ['work', 'todo'],
        body: '## Việc cần làm\n'
            '- [x] Chốt thiết kế editor mới\n'
            '- [ ] Viết test hồi quy cho ghi chú\n'
            '- [ ] Cập nhật tài liệu\n\n'
            '## Ưu tiên\n'
            '1. Sửa lỗi checkbox không hiển thị\n'
            '2. Placeholder tiếng Việt\n\n'
            '> Ghi chú: bản build cần daemon chạy ở cổng 18788\n\n'
            'Đoạn có chữ **đậm**, *nghiêng* và `mã inline`.\n\n'
            '---\n\n'
            'Hết.',
      ),
    );
  });
}
