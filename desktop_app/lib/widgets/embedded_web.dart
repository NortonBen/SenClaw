// Cross-platform embedded web content.
//
//  • Web build  → a real <iframe> via HtmlElementView (embedded_web_web.dart).
//  • Desktop    → an "Open in browser" card (embedded_web_stub.dart), since a
//    native desktop webview would mean a heavy CEF/WKWebView dependency.
export 'embedded_web_stub.dart'
    if (dart.library.js_interop) 'embedded_web_web.dart';
