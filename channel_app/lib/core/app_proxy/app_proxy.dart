/// Loopback proxy for viewing a Space app's webview remotely.
///
/// The phone can only reach the daemon through the relay tunnel, so a webview
/// can't load `http://daemon/api/space/apps/:id/proxy/...` directly. This spins
/// up a tiny `127.0.0.1` HTTP server that forwards every request through the
/// relay (binary-safe via base64) and points the webview at it.
///
/// dart:io is unavailable on Flutter web, so the real impl is conditionally
/// imported; the web build gets a stub that throws on `start()`.
library;

export 'app_proxy_stub.dart' if (dart.library.io) 'app_proxy_io.dart';
