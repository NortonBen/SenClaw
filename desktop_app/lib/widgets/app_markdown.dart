import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart' show HardwareKeyboard;
import 'package:gpt_markdown/gpt_markdown.dart';
import 'package:url_launcher/url_launcher.dart';
import '../features/chat/flow_defaults.dart';
import '../features/chat/widgets/widget_card.dart';
import '../theme/tokens.dart';

/// GptMarkdown with a safe code-block builder: long/preformatted code scrolls
/// horizontally inside the bubble instead of overflowing the layout (the
/// "RIGHT OVERFLOWED BY N PIXELS" bug with wide JSON/tool output). Plain text
/// already wraps. Used everywhere markdown is rendered.
///
/// It also intercepts inline widget fenced blocks — ```widget (full WidgetSpec)
/// or ```chart / ```weather / ```clock / ```video (kind-data body) — renders via
/// [WidgetCard], so the chat-widget feature works purely from the LLM's text,
/// with no backend tool wired. Incomplete/invalid JSON (e.g. while streaming)
/// falls back to normal code rendering.
class AppMarkdown extends StatelessWidget {
  const AppMarkdown(this.data, {super.key, this.style, this.onLinkTap});
  final String data;
  final TextStyle? style;

  /// Optional handler for a plain click on a link. Shift+click always opens
  /// the link in the system browser regardless of this handler.
  final void Function(String url, String title)? onLinkTap;

  /// Open [url] in the system browser. Scheme-less links ("www.example.com")
  /// get https:// prepended; only web/mail links are launched.
  static Future<void> openExternal(String url) async {
    var uri = Uri.tryParse(url.trim());
    if (uri == null) return;
    if (!uri.hasScheme) uri = Uri.tryParse('https://${url.trim()}');
    if (uri == null) return;
    if (!const {'http', 'https', 'mailto'}.contains(uri.scheme)) return;
    try {
      await launchUrl(uri, mode: LaunchMode.externalApplication);
    } catch (_) {
      // Swallow launcher errors (e.g. no handler registered) — a failed
      // shift+click should never crash the app.
    }
  }

  static const _widgetLangs = {
    'widget',
    'chart',
    'weather',
    'clock',
    'video',
    'audio',
    'image',
  };

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
      onLinkTap: (url, title) {
        if (HardwareKeyboard.instance.isShiftPressed) {
          openExternal(url);
        } else if (onLinkTap != null) {
          onLinkTap!.call(url, title);
        } else {
          // No explicit handler (the chat bubbles): honor the user's
          // "Mở link" default — mini-browser in-app, else system browser.
          // Before this, a plain click here did nothing at all.
          ChatLinkFlow.handleChatLink(url);
        }
      },
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
