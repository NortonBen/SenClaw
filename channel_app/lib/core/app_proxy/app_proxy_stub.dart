/// Web stub: the loopback proxy needs dart:io (mobile/desktop only).
class AppProxyServer {
  AppProxyServer(this.appId, {this.version = 0});
  final String appId;
  final int version;

  Future<String> start() async =>
      throw UnsupportedError('App webview is not supported on web');

  Future<void> stop() async {}

  static Future<void> clearAppCache(String appId) async {}
}
