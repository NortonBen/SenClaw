import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:gpt_markdown/gpt_markdown.dart';
import '../features/chat/widgets/widget_card.dart';
import '../theme/tokens.dart';

/// GptMarkdown with a safe code-block builder: long/preformatted code scrolls
/// horizontally inside the bubble instead of overflowing the layout (the
/// "RIGHT OVERFLOWED BY N PIXELS" bug with wide JSON/tool output). Plain text
/// already wraps. Used everywhere markdown is rendered.
///
/// It also intercepts inline widget fenced blocks — ```widget (full WidgetSpec)
/// or ```chart / ```weather / ```clock (kind-data body) — and renders them via
/// [WidgetCard], so the chat-widget feature works purely from the LLM's text,
/// with no backend tool wired. Incomplete/invalid JSON (e.g. while streaming)
/// falls back to normal code rendering.
class AppMarkdown extends StatelessWidget {
  const AppMarkdown(this.data, {super.key, this.style});
  final String data;
  final TextStyle? style;

  static const _widgetLangs = {'widget', 'chart', 'weather', 'clock'};

  /// Try to parse a fenced block tagged as a widget into a WidgetSpec map.
  /// Returns null when the language isn't a widget tag or the body isn't valid
  /// JSON yet — the caller then renders a normal code block.
  static Map<String, dynamic>? _tryWidgetSpec(String? name, String code) {
    final lang = (name ?? '').trim().toLowerCase();
    if (!_widgetLangs.contains(lang)) return null;
    try {
      final decoded = jsonDecode(code.trim());
      if (decoded is! Map) return null;
      final map = decoded.cast<String, dynamic>();
      if (lang == 'widget') {
        // Full spec: {kind, title?, data}. Require a kind to be present.
        if (map['kind'] == null) return null;
        return map;
      }
      // Language-tagged kind: wrap the body as {kind: <lang>, data: <json>}.
      // Allow the body to optionally carry its own title.
      return {
        'kind': lang,
        if (map['title'] != null) 'title': map['title'],
        'data': map.containsKey('data') ? map['data'] : map,
      };
    } catch (_) {
      return null;
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return GptMarkdown(
      data,
      style: style,
      codeBuilder: (ctx, name, code, closed) {
        // Widget fenced block → native WidgetCard (only once the block has
        // closed and its JSON parses; otherwise fall through to code view).
        if (closed) {
          final spec = _tryWidgetSpec(name, code);
          if (spec != null) return WidgetCard(spec: spec);
        }
        return Container(
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
        );
      },
    );
  }
}
