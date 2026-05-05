import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';
import 'package:fixnum/fixnum.dart';
import 'package:web_socket_channel/io.dart';
import '../models/agent_model.dart';
import 'crypto_service.dart';
import 'logger_service.dart';

class RelayControlType {
  static const int ping = 0;
  static const int pong = 1;
  static const int ack = 2;
  static const int typingStart = 3;
  static const int typingStop = 4;
  static const int disconnect = 5;
  static const int agentListReq = 6;
  static const int agentListResp = 7;
  static const int agentSelect = 8;
  static const int historyReq = 9;
  static const int historyResp = 10;
}

class RelayService {
  final String _hubUrl;
  late CryptoService _crypto;
  final String channelId;
  final String senderId;
  final String accessToken;

  final _incomingController = StreamController<String>.broadcast();
  Stream<String> get incomingMessages => _incomingController.stream;

  final _typingController = StreamController<bool>.broadcast();
  Stream<bool> get typingUpdates => _typingController.stream;

  final _agentListController = StreamController<List<AgentInfo>>.broadcast();
  Stream<List<AgentInfo>> get agentListUpdates => _agentListController.stream;

  final _historyController = StreamController<List<HistoryMessage>>.broadcast();
  Stream<List<HistoryMessage>> get historyUpdates => _historyController.stream;

  final _outboundController =
      StreamController<Map<String, dynamic>>.broadcast();
  IOWebSocketChannel? _ws;

  StreamSubscription? _wsSubscription;
  StreamSubscription? _sessionOutboundSubscription;
  Timer? _reconnectTimer;
  Timer? _keepAliveTimer;
  bool _isDisposed = false;
  bool _isConnected = false;
  bool _isConnecting = false;

  /// Any frame from hub (e.g. heartbeat PING) — proves TLS/stream works; agent list still needs Senclaw peer.
  bool _hasInboundData = false;
  int _reconnectDelaySecs = 2;

  bool get hasReceivedInboundHubData => _hasInboundData;

  RelayService({
    required String hubUrl,
    required this.channelId,
    required this.senderId,
    required this.accessToken,
    required List<int> encryptionKey,
  }) : _hubUrl = hubUrl {
    _crypto = CryptoService(encryptionKey);
    Log.i('RelayService WS endpoint source: ${hubUrl.trim()}');
  }

  void start() => _connect();

  void _connect() async {
    if (_isDisposed || _isConnected || _isConnecting) return;
    _isConnecting = true;
    _hasInboundData = false;

    _reconnectTimer?.cancel();
    _reconnectTimer = null;

    try {
      final wsUri = _buildWsUri(_hubUrl, channelId, accessToken);
      final ws = IOWebSocketChannel.connect(
        wsUri.toString(),
        pingInterval: const Duration(seconds: 25),
        connectTimeout: const Duration(seconds: 15),
      );
      _ws = ws;

      _sessionOutboundSubscription = _outboundController.stream.listen((msg) {
        _ws?.sink.add(jsonEncode(msg));
      }, onError: (e) => Log.e('Outbound error: $e'));

      _wsSubscription = ws.stream.listen(
        (dynamic raw) async {
          _hasInboundData = true;
          if (!_isConnected) {
            _isConnected = true;
            _reconnectDelaySecs = 2; // reset backoff on success
            Log.i('RelayService: connected');
            _startKeepAlive();
          }

          Map<String, dynamic> msg;
          try {
            if (raw is String) {
              msg = jsonDecode(raw) as Map<String, dynamic>;
            } else if (raw is List<int> || raw is Uint8List) {
              final text = utf8.decode((raw as List<int>));
              msg = jsonDecode(text) as Map<String, dynamic>;
            } else {
              Log.d('RelayService: ignoring unsupported ws frame');
              return;
            }
          } catch (e) {
            Log.e('RelayService decode error: $e');
            return;
          }

          final frameType = (msg['type'] ?? '') as String;
          final msgId = (msg['message_id'] ?? '') as String;
          final msgChannel = (msg['channel_id'] ?? '') as String;
          Log.i('Relay stream received frame: $frameType id=$msgId');
          if (msgChannel != channelId) return;

          if (frameType == 'encrypted') {
            try {
              final nonce = base64Decode((msg['nonce'] ?? '') as String);
              final cipher = base64Decode((msg['ciphertext'] ?? '') as String);
              final tag = base64Decode((msg['tag'] ?? '') as String);
              final text = await _crypto.decrypt(
                Uint8List.fromList(nonce),
                Uint8List.fromList(cipher),
                Uint8List.fromList(tag),
              );
              _incomingController.add(text);
            } catch (e) {
              Log.e('Decrypt failed: $e');
            }
          } else if (frameType == 'control') {
            _handleControl(
              (msg['control_type'] ?? 0) as int,
              (msg['metadata'] ?? '') as String,
            );
          } else if (frameType == 'ping') {
            _ws?.sink.add(jsonEncode(_makePingPongFrame('pong')));
          } else if (frameType == 'pong') {
            // keepalive acknowledgement; no app-level action needed
          }
        },
        onDone: () {
          Log.i('Relay stream closed');
          _handleDisconnect();
        },
        onError: (e) {
          Log.e('Relay stream error: $e');
          _handleDisconnect();
        },
        cancelOnError: true,
      );
      // Handshake + agent list request sent directly right after stream active.
      ws.sink.add(jsonEncode(_makePingPongFrame('ping')));
      ws.sink.add(
        jsonEncode(_makeControl(RelayControlType.agentListReq, '{}')),
      );
      // Stream subscription is active; hub may not send before first heartbeat (see hub StartHeartbeat).
      _isConnecting = false;
    } catch (e) {
      Log.e('Failed to connect: $e');
      _isConnecting = false;
      _scheduleReconnect();
    }
  }

  void _startKeepAlive() {
    _keepAliveTimer?.cancel();
    _keepAliveTimer = Timer.periodic(const Duration(seconds: 25), (_) {
      if (_isDisposed || !_isConnected) return;
      _ws?.sink.add(jsonEncode(_makePingPongFrame('ping')));
    });
  }

  void _handleControl(int type, String meta) {
    switch (type) {
      case RelayControlType.typingStart:
        _typingController.add(true);
      case RelayControlType.typingStop:
        _typingController.add(false);
      case RelayControlType.agentListResp:
        try {
          final raw = jsonDecode(meta) as List<dynamic>;
          final agents = raw
              .map((e) => AgentInfo.fromJson(e as Map<String, dynamic>))
              .toList();
          _agentListController.add(agents);
        } catch (e) {
          Log.e('AGENT_LIST_RESP parse error: $e');
        }
      case RelayControlType.historyResp:
        try {
          Log.d('HISTORY_RESP raw meta: $meta');
          final raw = jsonDecode(meta) as List<dynamic>;
          final messages = raw
              .map((e) => HistoryMessage.fromJson(e as Map<String, dynamic>))
              .toList();
          _historyController.add(messages);
        } catch (e) {
          Log.e('HISTORY_RESP parse error: $e');
        }
      default:
        Log.d('Control type=$type meta=$meta');
    }
  }

  /// Send a control frame to the server.
  void sendControl(int type, String metadata) {
    if (_isDisposed) return;
    _outboundController.add(_makeControl(type, metadata));
  }

  Map<String, dynamic> _makeControl(int type, String metadata) {
    return {
      'type': 'control',
      'message_id': DateTime.now().millisecondsSinceEpoch.toString(),
      'channel_id': channelId,
      'sender_id': senderId,
      'timestamp': Int64(DateTime.now().millisecondsSinceEpoch).toInt(),
      'control_type': type,
      'metadata': metadata,
    };
  }

  Map<String, dynamic> _makePingPongFrame(String frameType) {
    return {
      'type': frameType,
      'message_id': DateTime.now().millisecondsSinceEpoch.toString(),
      'channel_id': channelId,
      'sender_id': senderId,
      'timestamp': Int64(DateTime.now().millisecondsSinceEpoch).toInt(),
    };
  }

  Future<void> sendMessage(String text) async {
    try {
      final encrypted = await _crypto.encrypt(text);
      final tagBytes = encrypted.mac.bytes;
      _outboundController.add({
        'type': 'encrypted',
        'message_id': DateTime.now().millisecondsSinceEpoch.toString(),
        'channel_id': channelId,
        'sender_id': senderId,
        'timestamp': Int64(DateTime.now().millisecondsSinceEpoch).toInt(),
        'nonce': base64Encode(encrypted.nonce),
        'ciphertext': base64Encode(encrypted.cipherText),
        'tag': base64Encode(tagBytes),
      });
    } catch (e) {
      Log.e('Send failed: $e');
      rethrow;
    }
  }

  void _handleDisconnect() {
    if (!_isConnected && !_isConnecting && _wsSubscription == null) {
      return;
    }
    _isConnected = false;
    _isConnecting = false;
    _hasInboundData = false;
    _keepAliveTimer?.cancel();
    _keepAliveTimer = null;
    _sessionOutboundSubscription?.cancel();
    _wsSubscription?.cancel();
    _wsSubscription = null;
    _ws?.sink.close();
    _ws = null;
    if (!_isDisposed) _scheduleReconnect();
  }

  void _scheduleReconnect() {
    if (_isDisposed || _reconnectTimer != null) return;
    Log.i('RelayService: reconnecting in ${_reconnectDelaySecs}s...');
    _reconnectTimer = Timer(Duration(seconds: _reconnectDelaySecs), () {
      _reconnectTimer = null;
      _connect();
    });
    // Exponential backoff capped at 60 s
    _reconnectDelaySecs = (_reconnectDelaySecs * 2).clamp(2, 60);
  }

  Future<void> dispose() async {
    _isDisposed = true;
    _reconnectTimer?.cancel();
    _keepAliveTimer?.cancel();
    _sessionOutboundSubscription?.cancel();
    _wsSubscription?.cancel();
    await _ws?.sink.close();
    _ws = null;
    await Future.wait([
      _incomingController.close(),
      _outboundController.close(),
      _typingController.close(),
      _agentListController.close(),
      _historyController.close(),
    ]);
  }

  Uri _buildWsUri(String rawHub, String channelId, String accessToken) {
    final input = rawHub.trim();
    final withScheme = input.contains('://') ? input : 'https://$input';
    final parsed = Uri.parse(withScheme);

    final scheme = switch (parsed.scheme.toLowerCase()) {
      'http' => 'ws',
      'https' => 'wss',
      'ws' => 'ws',
      'wss' => 'wss',
      _ => 'wss',
    };
    // Keep relay endpoint fixed and consistent with SenClaw Rust client.
    const path = '/v1/relay/ws';

    return parsed.replace(
      scheme: scheme,
      path: path,
      queryParameters: {
        ...parsed.queryParameters,
        'channel_id': channelId,
        'access_token': accessToken,
      },
    );
  }
}
