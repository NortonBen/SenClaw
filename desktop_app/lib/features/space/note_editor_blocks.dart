import 'package:appflowy_editor/appflowy_editor.dart';
import 'package:flutter/material.dart';

import '../../theme/tokens.dart';

/// SenClaw-themed AppFlowy block builders for the note editor.
///
/// Replaces the stock visuals where they fight the app theme: the SVG todo
/// checkbox (a 1 px #BDBDBD outline — effectively invisible on our grey
/// panes), the blue quote bar, the grey divider, and the English block
/// placeholders ("To-do", …) that leak through regardless of app language.
Map<String, BlockComponentBuilder> noteBlockBuilders(AppColors c) {
  final config = BlockComponentConfiguration(
    placeholderTextStyle: (node, {textSpan}) =>
        TextStyle(color: c.textMuted, fontSize: 15.5, height: 1.6),
  );

  return {
    PageBlockKeys.type: PageBlockComponentBuilder(),
    ParagraphBlockKeys.type: ParagraphBlockComponentBuilder(
      // Empty placeholder: an empty line should look empty, not advertise
      // slash-commands on every blank paragraph.
      configuration: config.copyWith(placeholderText: (_) => ''),
    ),
    TodoListBlockKeys.type: TodoListBlockComponentBuilder(
      configuration: config.copyWith(placeholderText: (_) => 'Việc cần làm'),
      iconBuilder: (context, node, onCheck) => NoteCheckbox(
        checked: node.attributes[TodoListBlockKeys.checked] == true,
        colors: c,
        onTap: onCheck,
      ),
      textStyleBuilder: (checked) => checked
          ? TextStyle(
              decoration: TextDecoration.lineThrough,
              decorationColor: c.textMuted,
              color: c.textMuted,
            )
          : const TextStyle(),
    ),
    BulletedListBlockKeys.type: BulletedListBlockComponentBuilder(
      configuration: config.copyWith(placeholderText: (_) => 'Mục danh sách'),
      iconBuilder: (context, node) => NoteBulletDot(node: node, colors: c),
    ),
    NumberedListBlockKeys.type: NumberedListBlockComponentBuilder(
      // Stock icon renders "N." with the editor text style — already themed.
      configuration: config.copyWith(placeholderText: (_) => 'Mục danh sách'),
    ),
    QuoteBlockKeys.type: QuoteBlockComponentBuilder(
      configuration: config.copyWith(placeholderText: (_) => 'Trích dẫn'),
      iconBuilder: (context, node) => NoteQuoteBar(colors: c),
    ),
    HeadingBlockKeys.type: HeadingBlockComponentBuilder(
      configuration: config.copyWith(
        placeholderText: (node) =>
            'Tiêu đề ${node.attributes[HeadingBlockKeys.level] ?? ''}',
        padding: (_) => const EdgeInsets.only(top: 12, bottom: 2),
      ),
      textStyleBuilder: (level) => switch (level) {
        1 => TextStyle(
            fontSize: 23,
            fontWeight: FontWeight.w700,
            color: c.textPrimary,
            height: 1.4),
        2 => TextStyle(
            fontSize: 19,
            fontWeight: FontWeight.w700,
            color: c.textPrimary,
            height: 1.4),
        3 => TextStyle(
            fontSize: 16.5,
            fontWeight: FontWeight.w600,
            color: c.textPrimary,
            height: 1.4),
        _ => TextStyle(
            fontSize: 15.5,
            fontWeight: FontWeight.w600,
            color: c.textPrimary,
            height: 1.4),
      },
    ),
    ImageBlockKeys.type: ImageBlockComponentBuilder(),
    DividerBlockKeys.type: DividerBlockComponentBuilder(
      configuration: config.copyWith(
        padding: (_) => const EdgeInsets.symmetric(vertical: 8),
      ),
      lineColor: c.borderStrong,
      height: 12,
    ),
    TableBlockKeys.type: TableBlockComponentBuilder(),
    TableCellBlockKeys.type: TableCellBlockComponentBuilder(),
  };
}

/// Material-style animated todo checkbox: outlined rounded square that fills
/// with the accent color and shows a white check when done. Sized to sit on
/// the first text line of the block (15.5 px text at 1.6 line-height ≈ 25 px).
class NoteCheckbox extends StatelessWidget {
  const NoteCheckbox({
    super.key,
    required this.checked,
    required this.colors,
    required this.onTap,
  });

  final bool checked;
  final AppColors colors;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    return MouseRegion(
      cursor: SystemMouseCursors.click,
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: onTap,
        child: Container(
          width: 26,
          height: 25,
          padding: const EdgeInsets.only(right: 8),
          alignment: Alignment.center,
          child: AnimatedContainer(
            duration: const Duration(milliseconds: 120),
            curve: Curves.easeOut,
            width: 17,
            height: 17,
            decoration: BoxDecoration(
              color: checked ? colors.accent : Colors.transparent,
              borderRadius: BorderRadius.circular(5),
              // textMuted (45% fg) outline — borderStrong is too faint on the
              // grey pane, which is how the old SVG checkbox went invisible.
              border: checked
                  ? null
                  : Border.all(color: colors.textMuted, width: 1.5),
            ),
            child: checked
                ? const Icon(Icons.check, size: 13, color: Colors.white)
                : null,
          ),
        ),
      ),
    );
  }
}

/// Bullet marker with Notion-style depth shapes: ● → ○ → ▪ as lists nest.
class NoteBulletDot extends StatelessWidget {
  const NoteBulletDot({super.key, required this.node, required this.colors});

  final Node node;
  final AppColors colors;

  int get _depth {
    var d = 0;
    var p = node.parent;
    while (p != null) {
      if (p.type == BulletedListBlockKeys.type) d++;
      p = p.parent;
    }
    return d;
  }

  @override
  Widget build(BuildContext context) {
    final color = colors.textSecondary;
    final shape = switch (_depth % 3) {
      1 => BoxDecoration(
          shape: BoxShape.circle, border: Border.all(color: color, width: 1.2)),
      2 => BoxDecoration(color: color),
      _ => BoxDecoration(color: color, shape: BoxShape.circle),
    };
    return Container(
      width: 26,
      height: 25,
      padding: const EdgeInsets.only(right: 8),
      alignment: Alignment.center,
      child: Container(width: 6, height: 6, decoration: shape),
    );
  }
}

/// Accent vertical bar for quote blocks (stretches with the block height).
class NoteQuoteBar extends StatelessWidget {
  const NoteQuoteBar({super.key, required this.colors});

  final AppColors colors;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(left: 2, right: 12, top: 2, bottom: 2),
      child: Container(
        width: 3,
        decoration: BoxDecoration(
          color: colors.accent,
          borderRadius: BorderRadius.circular(AppTokens.rFull),
        ),
      ),
    );
  }
}
