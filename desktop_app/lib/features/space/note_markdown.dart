import 'package:appflowy_editor/appflowy_editor.dart';

/// Encodes an AppFlowy [Document] back into the Markdown we persist on disk.
///
/// AppFlowy's own Markdown round-trip is quirky, and its **decoder** must be
/// able to re-read whatever we save (notes reload through [parseNoteMarkdown]):
///   * With `lineBreak: ''` a block image is dropped on reparse (it needs blank
///     lines around it to be treated as a block).
///   * With `lineBreak: '\n'` blocks/images are correct, but *loose* todo lists
///     (blank line between items) lose their text and checked state on reparse.
///
/// So we encode with `lineBreak: '\n'` (correct blocks) and then [normalizeNoteMarkdown]
/// fixes the one remaining conflict: blank lines **around images**, but **tight**
/// list items. The result is idempotent — re-encoding it yields itself — which
/// is what lets the inline editor persist only on real change instead of
/// rewriting (or corrupting) a note every time it is opened.
String encodeNoteMarkdown(Document document) =>
    normalizeNoteMarkdown(documentToMarkdown(document, lineBreak: '\n'));

/// Decodes persisted Markdown into a [Document] for the editor.
///
/// The same [normalizeNoteMarkdown] used on encode runs first, because note
/// bodies are also written by the web UI and by AI agents, which routinely
/// produce *loose* lists (a blank line between items). AppFlowy's decoder
/// mangles those: a loose `- [ ] item` turns into an **empty** todo block
/// (rendering only its placeholder) plus an orphan paragraph with the text.
/// Tightening before parse feeds the decoder the only list shape it reads
/// correctly.
Document parseNoteMarkdown(String markdown) =>
    markdownToDocument(normalizeNoteMarkdown(markdown));

final RegExp _listItem = RegExp(r'^\s*([-*+]|\d+\.)\s');
final RegExp _imageBlock = RegExp(r'\n*(!\[[^\]]*\]\([^)]*\))\n*');

/// Normalise Markdown so AppFlowy's decoder can re-parse it unchanged.
String normalizeNoteMarkdown(String md) {
  // 0. Normalise line endings (bodies can come from any frontend/OS).
  var s = md.replaceAll('\r\n', '\n').replaceAll('\r', '\n');

  // 1. Force a blank line before and after every (block-level) image.
  s = s.replaceAllMapped(_imageBlock, (m) => '\n\n${m[1]}\n\n');

  // 2. Drop blank lines that sit *between two list items* (appflowy's decoder
  //    only reads tight todo/bullet lists correctly).
  final lines = s.split('\n');
  final out = <String>[];
  for (var i = 0; i < lines.length; i++) {
    final line = lines[i];
    if (line.trim().isEmpty) {
      final prev = out.isNotEmpty ? out.last : '';
      final next = (i + 1 < lines.length) ? lines[i + 1] : '';
      if (_listItem.hasMatch(prev) && _listItem.hasMatch(next)) continue;
    }
    out.add(line);
  }

  // 3. Collapse any 3+ newline runs and trim the edges.
  return out.join('\n').replaceAll(RegExp(r'\n{3,}'), '\n\n').trim();
}
