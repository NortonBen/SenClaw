import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import '../theme/app_colors.dart';

/// Lightweight, dependency-free Markdown renderer for agent replies.
///
/// Handles the constructs LLM output uses most: fenced code blocks (```),
/// inline `code`, **bold**, *italic*, `# headings`, and `-`/`*` bullet lists.
/// Anything it doesn't recognise renders as plain paragraph text, so passing
/// arbitrary plain text is always safe.
class MarkdownText extends StatelessWidget {
  final String text;
  final Color color;
  final double fontSize;

  const MarkdownText(
    this.text, {
    super.key,
    this.color = Colors.white,
    this.fontSize = 14,
  });

  @override
  Widget build(BuildContext context) {
    final blocks = _parseBlocks(text);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        for (final b in blocks) _buildBlock(context, b),
      ],
    );
  }

  Widget _buildBlock(BuildContext context, _Block b) {
    switch (b.kind) {
      case _BlockKind.code:
        return _CodeBlock(code: b.text, fontSize: fontSize);
      case _BlockKind.heading:
        return Padding(
          padding: const EdgeInsets.only(top: 8, bottom: 2),
          child: RichText(
            text: _inlineSpans(
              b.text,
              base: TextStyle(
                color: color,
                fontSize: fontSize + (b.level == 1 ? 5 : b.level == 2 ? 3 : 1),
                fontWeight: FontWeight.bold,
                height: 1.3,
              ),
            ),
          ),
        );
      case _BlockKind.bullet:
        return Padding(
          padding: const EdgeInsets.symmetric(vertical: 2),
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text('•  ',
                  style: TextStyle(color: color, fontSize: fontSize, height: 1.4)),
              Expanded(
                child: RichText(
                  text: _inlineSpans(b.text,
                      base: TextStyle(
                          color: color, fontSize: fontSize, height: 1.4)),
                ),
              ),
            ],
          ),
        );
      case _BlockKind.paragraph:
        return Padding(
          padding: const EdgeInsets.symmetric(vertical: 2),
          child: RichText(
            text: _inlineSpans(b.text,
                base: TextStyle(color: color, fontSize: fontSize, height: 1.4)),
          ),
        );
    }
  }

  // ── Block parsing ────────────────────────────────────────────────────────
  List<_Block> _parseBlocks(String src) {
    final lines = src.replaceAll('\r\n', '\n').split('\n');
    final blocks = <_Block>[];
    final para = <String>[];

    void flushPara() {
      if (para.isNotEmpty) {
        blocks.add(_Block(_BlockKind.paragraph, para.join('\n').trim()));
        para.clear();
      }
    }

    var i = 0;
    while (i < lines.length) {
      final line = lines[i];
      final trimmed = line.trimLeft();

      if (trimmed.startsWith('```')) {
        flushPara();
        final buf = <String>[];
        i++;
        while (i < lines.length && !lines[i].trimLeft().startsWith('```')) {
          buf.add(lines[i]);
          i++;
        }
        i++; // skip closing fence
        blocks.add(_Block(_BlockKind.code, buf.join('\n')));
        continue;
      }

      final heading = RegExp(r'^(#{1,6})\s+(.*)$').firstMatch(trimmed);
      if (heading != null) {
        flushPara();
        blocks.add(_Block(_BlockKind.heading, heading.group(2)!,
            level: heading.group(1)!.length));
        i++;
        continue;
      }

      final bullet = RegExp(r'^[-*]\s+(.*)$').firstMatch(trimmed);
      if (bullet != null) {
        flushPara();
        blocks.add(_Block(_BlockKind.bullet, bullet.group(1)!));
        i++;
        continue;
      }

      if (trimmed.isEmpty) {
        flushPara();
      } else {
        para.add(line);
      }
      i++;
    }
    flushPara();
    if (blocks.isEmpty) blocks.add(_Block(_BlockKind.paragraph, src.trim()));
    return blocks;
  }

  // ── Inline parsing (`code`, **bold**, *italic*) ────────────────────────────
  TextSpan _inlineSpans(String text, {required TextStyle base}) {
    final spans = <TextSpan>[];
    final pattern = RegExp(r'(`[^`]+`)|(\*\*[^*]+\*\*)|(\*[^*]+\*)');
    var last = 0;
    for (final m in pattern.allMatches(text)) {
      if (m.start > last) {
        spans.add(TextSpan(text: text.substring(last, m.start), style: base));
      }
      final tok = m.group(0)!;
      if (tok.startsWith('`')) {
        spans.add(TextSpan(
          text: tok.substring(1, tok.length - 1),
          style: base.copyWith(
            fontFamily: 'monospace',
            backgroundColor: Colors.white.withValues(alpha: 0.08),
            color: AppColors.cyan,
          ),
        ));
      } else if (tok.startsWith('**')) {
        spans.add(TextSpan(
          text: tok.substring(2, tok.length - 2),
          style: base.copyWith(fontWeight: FontWeight.bold),
        ));
      } else {
        spans.add(TextSpan(
          text: tok.substring(1, tok.length - 1),
          style: base.copyWith(fontStyle: FontStyle.italic),
        ));
      }
      last = m.end;
    }
    if (last < text.length) {
      spans.add(TextSpan(text: text.substring(last), style: base));
    }
    return TextSpan(children: spans, style: base);
  }
}

enum _BlockKind { paragraph, heading, bullet, code }

class _Block {
  final _BlockKind kind;
  final String text;
  final int level;
  _Block(this.kind, this.text, {this.level = 1});
}

class _CodeBlock extends StatelessWidget {
  final String code;
  final double fontSize;
  const _CodeBlock({required this.code, required this.fontSize});

  @override
  Widget build(BuildContext context) {
    return Container(
      width: double.infinity,
      margin: const EdgeInsets.symmetric(vertical: 6),
      padding: const EdgeInsets.fromLTRB(12, 10, 8, 10),
      decoration: BoxDecoration(
        color: Colors.black.withValues(alpha: 0.35),
        borderRadius: BorderRadius.circular(8),
        border: Border.all(color: AppColors.cardBorder),
      ),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Expanded(
            child: SingleChildScrollView(
              scrollDirection: Axis.horizontal,
              child: SelectableText(
                code,
                style: TextStyle(
                  color: Colors.white70,
                  fontFamily: 'monospace',
                  fontSize: fontSize - 1,
                  height: 1.4,
                ),
              ),
            ),
          ),
          InkWell(
            onTap: () => Clipboard.setData(ClipboardData(text: code)),
            child: const Padding(
              padding: EdgeInsets.only(left: 4, top: 2),
              child: Icon(Icons.copy, size: 14, color: Colors.white38),
            ),
          ),
        ],
      ),
    );
  }
}
