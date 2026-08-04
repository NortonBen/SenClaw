// Cached view of the daemon's default-flow settings (`GET /api/defaults`) +
// the registration point for the in-app link opener.
//
// Chat markdown link taps fire synchronously deep inside stateless widget
// trees, so the defaults live in a static cache the chat screen warms on
// build, and the mini-browser opener is a callback the screen registers with
// its (context, ref) captured — `openEventLink` needs both.

import 'dart:convert';

import 'package:http/http.dart' as http;
import 'package:url_launcher/url_launcher.dart';

class ChatLinkFlow {
  ChatLinkFlow._();

  static String _openLink = 'system-browser';
  static String _httpBase = 'http://127.0.0.1:18788';
  static bool _fetched = false;

  /// Daemon base (`AppConfig.httpBase`) — also used to absolutize relative
  /// widget entries (`/api/space/apps/<id>/proxy/...`) for the webview.
  static String get httpBase => _httpBase;

  /// `system-browser` | `mini-browser` | `new-tab` (new-tab is a web-UI
  /// concept; on desktop it behaves like system-browser).
  static String get openLink => _openLink;

  /// Registered by the chat screen with (context, ref) captured; opens an
  /// internal `/space/app/<id>?…` route (the mini-browser). Returns a
  /// human-readable failure reason like `openEventLink`, or null on success.
  static Future<String?> Function(String route)? openInternal;

  /// Warm the cache. Cheap and idempotent; re-fetches only when [force].
  static Future<void> prefetch(String httpBase, {bool force = false}) async {
    _httpBase = httpBase;
    if (_fetched && !force) return;
    _fetched = true;
    try {
      final res = await http
          .get(Uri.parse('$httpBase/api/defaults'))
          .timeout(const Duration(seconds: 4));
      if (res.statusCode != 200) return;
      final body = jsonDecode(res.body);
      // An old daemon answers unknown /api routes with the SPA index page —
      // guard on the shape, not just the status code.
      if (body is Map && body['openLink'] is String) {
        _openLink = body['openLink'] as String;
      }
    } catch (_) {
      // Offline daemon / old daemon → keep the system-browser default.
    }
  }

  /// Handle a plain (unmodified) chat-link tap per the user's default:
  /// mini-browser default + registered opener → in-app; everything else →
  /// system browser. This is also what un-breaks the previously dead plain
  /// click (only shift+click opened links before).
  static Future<void> handleChatLink(String url) async {
    final u = url.trim();
    final isHttp = u.startsWith('http://') || u.startsWith('https://');
    if (isHttp && _openLink == 'mini-browser' && openInternal != null) {
      final err = await openInternal!(
        '/space/app/mini-browser?url=${Uri.encodeComponent(u)}',
      );
      if (err == null) return;
      // Mini-browser missing/disabled → fall through to the system browser
      // rather than silently doing nothing.
    }
    var uri = Uri.tryParse(u);
    if (uri == null) return;
    if (!uri.hasScheme) uri = Uri.tryParse('https://$u');
    if (uri == null) return;
    if (!const {'http', 'https', 'mailto'}.contains(uri.scheme)) return;
    try {
      await launchUrl(uri, mode: LaunchMode.externalApplication);
    } catch (_) {
      // No handler registered — a failed tap must never crash the app.
    }
  }
}
