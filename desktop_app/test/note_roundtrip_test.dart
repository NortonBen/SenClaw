import 'package:appflowy_editor/appflowy_editor.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:senclaw_desktop/features/space/note_markdown.dart';
import 'package:senclaw_desktop/features/space/note_tags.dart';

/// Guards the Markdown ↔ AppFlowy Document round-trip the inline editor relies
/// on. AppFlowy's own decoder is fussy (loose todo lists lose their text; block
/// images need blank lines), so [encodeNoteMarkdown] normalises the output and
/// [parseNoteMarkdown] normalises the *input* (bodies written by the web UI or
/// AI agents are routinely loose). These tests ensure real note constructs
/// survive AND that the result is idempotent — the property that lets the
/// editor persist only on real edits instead of corrupting a note each time it
/// is opened.
String rt(String md) => encodeNoteMarkdown(parseNoteMarkdown(md));

const _realNote = '''# Cấu trúc dự án

- src/App.tsx: Component chính
- src/components/TodoForm.tsx: Component thêm

## Việc cần làm

- [ ] viết TodoList
- [x] tạo hook useTodos
- [ ] test #todo

![screenshot](http://127.0.0.1:18788/api/space/screenshots/a.png)

_Chụp lúc 17/7/2026 23:37._''';

void main() {
  test('checkbox items survive with their checked state', () {
    final md = rt('- [ ] todo A\n- [x] done B');
    expect(md, contains('- [ ] todo A'));
    expect(md, contains('- [x] done B'));
  });

  test('a realistic note keeps headings, checkboxes, image and caption', () {
    final md = rt(_realNote);
    expect(md, contains('# Cấu trúc dự án'));
    expect(md, contains('- [ ] viết TodoList'));
    expect(md, contains('- [x] tạo hook useTodos'));
    expect(md, contains('](http://127.0.0.1:18788/api/space/screenshots/a.png)'));
    expect(md, contains('_Chụp lúc 17/7/2026 23:37._'));
    // The image must NOT be glued to its caption (that breaks reparse).
    expect(RegExp(r'\.png\)_').hasMatch(md), isFalse);
  });

  test('re-encoding is idempotent (no data loss on reload, no autosave loop)',
      () {
    final once = rt(_realNote);
    final twice = rt(once);
    expect(twice, once);
    // The checkbox text specifically must not vanish on the 2nd pass — the
    // exact bug appflowy exhibits with loose (blank-separated) todo lists.
    expect(twice, contains('- [ ] viết TodoList'));
    expect(twice, contains('](http://127.0.0.1:18788/api/space/screenshots/a.png)'));
  });

  test('LOOSE todo lists (web-UI/AI-written) keep their text on parse', () {
    // Blank lines between items — raw markdownToDocument turns each item into
    // an EMPTY todo block plus an orphan paragraph. parseNoteMarkdown must
    // tighten first so the text and checked state survive.
    const loose = '- [ ] set nổ hũ nhưng không nhận\n\n'
        '- [x] đề vs 3 càng\n\n'
        '- [ ] Fix napas Bin';
    final doc = parseNoteMarkdown(loose);
    final todos = <Node>[];
    void walk(Node n) {
      if (n.type == 'todo_list') todos.add(n);
      n.children.forEach(walk);
    }

    walk(doc.root);
    expect(todos, hasLength(3));
    for (final t in todos) {
      expect(t.delta?.toPlainText().trim(), isNotEmpty,
          reason: 'loose-list decode must not produce empty todo blocks');
    }
    final md = rt(loose);
    expect(md, contains('- [ ] set nổ hũ nhưng không nhận'));
    expect(md, contains('- [x] đề vs 3 càng'));
    expect(md, contains('- [ ] Fix napas Bin'));
  });

  test('CRLF line endings are normalised before parse', () {
    final md = rt('- [ ] a\r\n- [x] b\r\n\r\nplain');
    expect(md, contains('- [ ] a'));
    expect(md, contains('- [x] b'));
    expect(md, contains('plain'));
    expect(md.contains('\r'), isFalse);
  });

  test('inline #hashtags survive so tag extraction keeps working', () {
    expect(extractBodyTags(rt('mua sữa #shopping và #urgent')),
        containsAll(['shopping', 'urgent']));
  });

  test('normalizeNoteMarkdown is itself idempotent', () {
    const messy = 'a\n\n\n![x](u)\n_cap_\n\n- [ ] a\n\n- [x] b';
    final n = normalizeNoteMarkdown(messy);
    expect(normalizeNoteMarkdown(n), n);
  });
}
