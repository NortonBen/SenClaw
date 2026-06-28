import 'package:flutter/material.dart';
import 'package:gpt_markdown/gpt_markdown.dart';
import '../theme/tokens.dart';

/// GptMarkdown with a safe code-block builder: long/preformatted code scrolls
/// horizontally inside the bubble instead of overflowing the layout (the
/// "RIGHT OVERFLOWED BY N PIXELS" bug with wide JSON/tool output). Plain text
/// already wraps. Used everywhere markdown is rendered.
class AppMarkdown extends StatelessWidget {
  const AppMarkdown(this.data, {super.key, this.style});
  final String data;
  final TextStyle? style;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return GptMarkdown(
      data,
      style: style,
      codeBuilder: (ctx, name, code, closed) => Container(
        width: double.infinity,
        margin: const EdgeInsets.symmetric(vertical: AppTokens.s6),
        decoration: BoxDecoration(
          color: c.surfaceAlt,
          borderRadius: BorderRadius.circular(AppTokens.rSm),
          border: Border.all(color: c.border),
        ),
        child: SingleChildScrollView(
          scrollDirection: Axis.horizontal,
          padding: const EdgeInsets.all(AppTokens.s12),
          child: Text(
            code,
            style: TextStyle(
              fontFamily: AppTokens.fontMono,
              fontSize: 12,
              height: 1.45,
              color: c.textPrimary,
            ),
          ),
        ),
      ),
    );
  }
}
