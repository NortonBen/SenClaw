import 'dart:async';
import 'dart:convert';
import 'package:web_socket_channel/web_socket_channel.dart';
import '../config/app_config.dart';

enum WsStatus { connecting, connected, disconnected }

/// A single decoded server→client event ({type, ...payload}).
typedef WsEvent = Map<String, dynamic>;

/// Manages the persistent WebSocket to the daemon gateway (ws://host:wsPort/).
///
/// Mirrors the React `useWebSocket` lifecycle: connect → `{type:connect,token}`
/// → re-subscribe known groups on reconnect, exponential backoff (max 15s).
/// Exposes a broadcast [events] stream that feature providers filter by `type`.
class WsClient {
  WsClient(this._config);

  AppConfig _config;
  WebSocketChannel? _channel;
  StreamSubscription? _sub;
  Timer? _retry;
  int _backoffMs = 500;
  bool _disposed = false;

  final _statusCtrl = StreamController<WsStatus>.broadcast();
  final _eventsCtrl = StreamController<WsEvent>.broadcast();
  final Set<String> _subscriptions = {};

  WsStatus _status = WsStatus.disconnected;
  WsStatus get status => _status;
  Stream<WsStatus> get statusStream => _statusCtrl.stream;
  Stream<WsEvent> get events => _eventsCtrl.stream;

  void updateConfig(AppConfig config) => _config = config;

  void connect() {
    if (_disposed) return;
    _setStatus(WsStatus.connecting);
    try {
      // A daemon bound beyond loopback gates the WS upgrade itself; the API
      // token rides as a query param because WebSocketChannel.connect cannot
      // set headers on all platforms. Loopback daemons ignore it.
      var uri = Uri.parse(_config.wsUrl);
      final apiToken = _config.apiToken;
      if (apiToken != null && apiToken.isNotEmpty) {
        uri = uri.replace(queryParameters: {'token': apiToken});
      }
      final ch = WebSocketChannel.connect(uri);
      _channel = ch;
      _sub = ch.stream.listen(
        _onMessage,
        onDone: _onDisconnect,
        onError: (_) => _onDisconnect(),
        cancelOnError: true,
      );
      // Auth handshake — token is optional on localhost. Fall back to the
      // API token so a gateway configured with SENCLAW_WS_TOKEN=<api token>
      // still authenticates.
      final tok = (_config.wsToken?.isNotEmpty ?? false) ? _config.wsToken : apiToken;
      send({'type': 'connect', if (tok != null && tok.isNotEmpty) 'token': tok});
    } catch (_) {
      _scheduleReconnect();
    }
  }

  void _onMessage(dynamic raw) {
    _backoffMs = 500; // healthy traffic resets backoff
    WsEvent? evt;
    try {
      final decoded = jsonDecode(raw as String);
      if (decoded is Map<String, dynamic>) evt = decoded;
    } catch (_) {
      return;
    }
    if (evt == null) return;
    if (evt['type'] == 'auth:ok') {
      _setStatus(WsStatus.connected);
      _resubscribe();
    }
    _eventsCtrl.add(evt);
  }

  void _onDisconnect() {
    _setStatus(WsStatus.disconnected);
    _scheduleReconnect();
  }

  void _scheduleReconnect() {
    _sub?.cancel();
    _channel = null;
    if (_disposed) return;
    _retry?.cancel();
    _retry = Timer(Duration(milliseconds: _backoffMs), connect);
    _backoffMs = (_backoffMs * 2).clamp(500, 15000);
  }

  void _resubscribe() {
    for (final jid in _subscriptions) {
      send({'type': 'subscribe', 'groupJid': jid});
    }
  }

  /// Subscribe to a chat group's event stream (survives reconnect). Always
  /// (re-)sends when connected so re-opening a chat reloads its `history:load`
  /// — the per-chat provider is autoDispose, so its messages are dropped on
  /// leave and must be re-fetched on return. The daemon treats re-subscribe as
  /// idempotent and replays history.
  void subscribe(String groupJid) {
    _subscriptions.add(groupJid);
    if (_status == WsStatus.connected) {
      send({'type': 'subscribe', 'groupJid': groupJid});
    }
  }

  void unsubscribe(String groupJid) {
    if (_subscriptions.remove(groupJid)) {
      send({'type': 'unsubscribe', 'groupJid': groupJid});
    }
  }

  void send(Map<String, dynamic> msg) {
    _channel?.sink.add(jsonEncode(msg));
  }

  void _setStatus(WsStatus s) {
    if (_status == s) return;
    _status = s;
    if (!_statusCtrl.isClosed) _statusCtrl.add(s);
  }

  void dispose() {
    _disposed = true;
    _retry?.cancel();
    _sub?.cancel();
    _channel?.sink.close();
    _statusCtrl.close();
    _eventsCtrl.close();
  }
}
