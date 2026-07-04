import 'package:flutter/foundation.dart' show kIsWeb;
import 'package:flutter/material.dart';
import 'package:webview_flutter/webview_flutter.dart';
import '../../core/app_proxy/app_proxy.dart';
import '../../services/language_service.dart';
import '../../theme/tokens.dart';

/// Renders a Space app's web UI remotely: a loopback [AppProxyServer] forwards
/// every asset/API request through the relay tunnel, and this webview points at
/// it. Mobile only — web shows an unsupported notice.
class AppWebViewScreen extends StatefulWidget {
  const AppWebViewScreen(
      {super.key, required this.appId, required this.title, this.version = 0});
  final String appId;
  final String title;

  /// App registration version (`installed_at`) — keys the local asset cache;
  /// 0 disables caching.
  final int version;

  @override
  State<AppWebViewScreen> createState() => _AppWebViewScreenState();
}

class _AppWebViewScreenState extends State<AppWebViewScreen> {
  AppProxyServer? _proxy;
  WebViewController? _controller;
  String? _error;
  bool _loading = true;

  @override
  void initState() {
    super.initState();
    if (!kIsWeb) _boot();
  }

  Future<void> _boot() async {
    try {
      final proxy = AppProxyServer(widget.appId, version: widget.version);
      final url = await proxy.start();
      final controller = WebViewController()
        ..setJavaScriptMode(JavaScriptMode.unrestricted)
        ..setNavigationDelegate(NavigationDelegate(
          onPageFinished: (_) {
            if (mounted) setState(() => _loading = false);
          },
          onWebResourceError: (_) {/* per-asset errors are non-fatal */},
        ))
        ..loadRequest(Uri.parse(url));
      if (!mounted) {
        await proxy.stop();
        return;
      }
      setState(() {
        _proxy = proxy;
        _controller = controller;
      });
    } catch (e) {
      if (mounted) {
        setState(() {
          _error = '$e';
          _loading = false;
        });
      }
    }
  }

  @override
  void dispose() {
    _proxy?.stop();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Scaffold(
      backgroundColor: c.bg,
      appBar: AppBar(
        backgroundColor: c.surface,
        elevation: 0,
        title: Text(widget.title, style: TextStyle(color: c.textPrimary)),
        actions: [
          if (_controller != null) ...[
            IconButton(
              tooltip: tr('Tải lại', 'Reload'),
              icon: Icon(Icons.refresh, color: c.textSecondary),
              onPressed: () => _controller!.reload(),
            ),
            IconButton(
              tooltip: tr('Tải mới hoàn toàn (xoá cache)',
                  'Full reload (clear cache)'),
              icon: Icon(Icons.cloud_sync_outlined, color: c.textSecondary),
              onPressed: () async {
                await AppProxyServer.clearAppCache(widget.appId);
                _controller!.reload();
              },
            ),
          ],
        ],
      ),
      body: kIsWeb
          ? Center(
              child: Text(
                  tr('Webview chỉ hỗ trợ trên thiết bị di động',
                      'Webview is only supported on mobile devices'),
                  style: TextStyle(color: c.textMuted)))
          : _error != null
              ? Center(
                  child: Padding(
                    padding: const EdgeInsets.all(24),
                    child: Text(
                        tr('Không mở được app:\n$_error',
                            'Could not open app:\n$_error'),
                        textAlign: TextAlign.center,
                        style: TextStyle(color: c.textSecondary)),
                  ),
                )
              : Stack(
                  children: [
                    if (_controller != null)
                      WebViewWidget(controller: _controller!),
                    if (_loading)
                      Center(
                          child: CircularProgressIndicator(color: c.accent)),
                  ],
                ),
    );
  }
}
