/// Web stub: the loopback proxy needs dart:io (mobile/desktop only).
class AppProxyServer {
  AppProxyServer(this.appId);
  final String appId;

  Future<String> start() async =>
      throw UnsupportedError('App webview is not supported on web');

  Future<void> stop() async {}
}
