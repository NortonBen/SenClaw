import 'dart:js_interop';
import 'dart:js_interop_unsafe';
import 'dart:ui_web' as ui_web;
import 'package:flutter/widgets.dart';
import 'package:web/web.dart' as web;

final Map<String, web.HTMLIFrameElement> _iframes = {};
final Set<String> _registered = {};

/// Web build: embed the URL as a real <iframe> and hand the app its theme via
/// `postMessage` (the SenClaw bridge: `senclaw:init` on load, `senclaw:theme`
/// on change) — the same protocol the React web UI uses. The URL itself stays
/// stable so a theme change never reloads the frame.
Widget embeddedWebView(String url,
        {String? title, String theme = 'light', String? instanceKey}) =>
    _IframeView(url: url, theme: theme, instanceKey: instanceKey);

class _IframeView extends StatefulWidget {
  const _IframeView(
      {required this.url, required this.theme, this.instanceKey});
  final String url;
  final String theme;
  final String? instanceKey;
  @override
  State<_IframeView> createState() => _IframeViewState();
}

class _IframeViewState extends State<_IframeView> {
  // Distinct instances (e.g. the same app shown in the dock vs the main Apps
  // surface) need separate view types, or they'd share/clobber one iframe.
  late final String _viewType =
      'embedded-iframe-${widget.instanceKey ?? ''}-${widget.url.hashCode}';
  JSFunction? _msgListener;

  void _post(String type) {
    final win = _iframes[_viewType]?.contentWindow;
    if (win == null) return;
    final msg = <String, String>{'type': type, 'theme': widget.theme}.jsify();
    try {
      (win as JSObject).callMethod('postMessage'.toJS, msg, '*'.toJS);
    } catch (_) {}
  }

  // The embedded app posts `senclaw:ready` once it has mounted its listener —
  // reply with the current theme. This handshake avoids the race where the
  // iframe `load` event (our `senclaw:init`) fires before the app is listening.
  void _onMessage(web.Event e) {
    String? type;
    try {
      final data = (e as web.MessageEvent).data;
      type = (data as JSObject).getProperty('type'.toJS).dartify() as String?;
    } catch (_) {
      return;
    }
    if (type == 'senclaw:ready') _post('senclaw:init');
  }

  @override
  void initState() {
    super.initState();
    _msgListener = _onMessage.toJS;
    web.window.addEventListener('message', _msgListener);
    if (_registered.add(_viewType)) {
      ui_web.platformViewRegistry.registerViewFactory(_viewType, (int _) {
        final iframe = web.HTMLIFrameElement()
          ..src = widget.url
          ..style.border = 'none'
          ..style.width = '100%'
          ..style.height = '100%'
          ..allow = 'clipboard-read; clipboard-write';
        iframe.onLoad.listen((_) => _post('senclaw:init'));
        _iframes[_viewType] = iframe;
        return iframe;
      });
    } else {
      // Frame already exists (kept alive) — push the current theme now.
      WidgetsBinding.instance
          .addPostFrameCallback((_) => _post('senclaw:theme'));
    }
  }

  @override
  void didUpdateWidget(covariant _IframeView old) {
    super.didUpdateWidget(old);
    if (old.theme != widget.theme) _post('senclaw:theme');
  }

  @override
  void dispose() {
    if (_msgListener != null) {
      web.window.removeEventListener('message', _msgListener);
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => HtmlElementView(viewType: _viewType);
}
