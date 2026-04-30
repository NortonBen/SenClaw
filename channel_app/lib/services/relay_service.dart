import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';
import 'package:grpc/grpc.dart';
import 'package:fixnum/fixnum.dart';
import '../generated/channel_relay.pbgrpc.dart';
import '../generated/channel_relay.pbenum.dart';
import '../models/agent_model.dart';
import 'crypto_service.dart';
import 'logger_service.dart';

class RelayService {
  late ClientChannel _channel;
  late ChannelRelayClient _client;
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

  final _outboundController = StreamController<RelayMessage>.broadcast();

  StreamSubscription? _responseSubscription;
  StreamSubscription? _sessionOutboundSubscription;
  Timer? _reconnectTimer;
  Timer? _keepAliveTimer;
  StreamController<RelayMessage>? _sessionCtrl;
  bool _isDisposed = false;
  bool _isConnected = false;
  bool _isConnecting = false;
  int _reconnectDelaySecs = 2;

  RelayService({
    required String hubUrl,
    required this.channelId,
    required this.senderId,
    required this.accessToken,
    required List<int> encryptionKey,
  }) {
    String host = '';
    int port = 443;
    bool isSecure = false;

    try {
      if (!hubUrl.contains('://')) {
        final parts = hubUrl.split(':');
        host = parts[0];
        if (parts.length > 1) port = int.parse(parts[1]);
      } else {
        final uri = Uri.parse(hubUrl);
        host = uri.host;
        port = uri.port == 0 ? (uri.scheme == 'https' ? 443 : 80) : uri.port;
        isSecure = uri.scheme == 'https';
      }
    } catch (e) {
      Log.e('Error parsing hub URL: $hubUrl — $e');
      host = '127.0.0.1';
      port = 50051;
    }

    _channel = ClientChannel(
      host,
      port: port,
      options: ChannelOptions(
        credentials: isSecure
            ? const ChannelCredentials.secure()
            : const ChannelCredentials.insecure(),
      ),
    );
    _client = ChannelRelayClient(_channel);
    _crypto = CryptoService(encryptionKey);
    Log.i('RelayService: $host:$port (secure=$isSecure)');
  }

  void start() => _connect();

  void _connect() async {
    if (_isDisposed || _isConnected || _isConnecting) return;
    _isConnecting = true;

    _reconnectTimer?.cancel();
    _reconnectTimer = null;
    Log.i('RelayService: connecting...');

    try {
      final sessionCtrl = StreamController<RelayMessage>();
      _sessionCtrl = sessionCtrl;

      _sessionOutboundSubscription = _outboundController.stream.listen(
        sessionCtrl.add,
        onError: (e) => Log.e('Outbound error: $e'),
      );

      final responseStream = _client.stream(
        sessionCtrl.stream,
        options: CallOptions(metadata: {
          'channel_id': channelId,
          'access_token': accessToken,
        }),
      );

      // Handshake + agent list request sent directly to session (not via broadcast)
      sessionCtrl.add(_makeControl(ControlMessage_Type.PING, ''));
      sessionCtrl.add(_makeControl(ControlMessage_Type.AGENT_LIST_REQ, '{}'));

      _responseSubscription = responseStream.listen(
        (msg) async {
          if (!_isConnected) {
            _isConnected = true;
            _isConnecting = false;
            _reconnectDelaySecs = 2; // reset backoff on success
            Log.i('RelayService: connected');
            _startKeepAlive(sessionCtrl);
          }

          if (msg.channelId != channelId) return;

          if (msg.hasEncryptedData()) {
            final enc = msg.encryptedData;
            try {
              final text = await _crypto.decrypt(
                Uint8List.fromList(enc.nonce),
                Uint8List.fromList(enc.ciphertext),
                Uint8List.fromList(enc.tag),
              );
              _incomingController.add(text);
            } catch (e) {
              Log.e('Decrypt failed: $e');
            }
          } else if (msg.hasControl()) {
            _handleControl(msg.control);
          }
        },
        onDone: () {
          Log.i('Relay stream closed');
          _handleDisconnect(sessionCtrl);
        },
        onError: (e) {
          Log.e('Relay stream error: $e');
          _handleDisconnect(sessionCtrl);
        },
        cancelOnError: true,
      );
    } catch (e) {
      Log.e('Failed to connect: $e');
      _isConnecting = false;
      _scheduleReconnect();
    }
  }

  void _startKeepAlive(StreamController<RelayMessage> sessionCtrl) {
    _keepAliveTimer?.cancel();
    _keepAliveTimer = Timer.periodic(const Duration(seconds: 25), (_) {
      if (_isDisposed || !_isConnected) return;
      sessionCtrl.add(_makeControl(ControlMessage_Type.PING, ''));
    });
  }

  void _handleControl(ControlMessage ctrl) {
    final type = ctrl.type;
    final meta = ctrl.metadata;

    switch (type) {
      case ControlMessage_Type.TYPING_START:
        _typingController.add(true);
      case ControlMessage_Type.TYPING_STOP:
        _typingController.add(false);
      case ControlMessage_Type.AGENT_LIST_RESP:
        try {
          final raw = jsonDecode(meta) as List<dynamic>;
          final agents = raw
              .map((e) => AgentInfo.fromJson(e as Map<String, dynamic>))
              .toList();
          _agentListController.add(agents);
        } catch (e) {
          Log.e('AGENT_LIST_RESP parse error: $e');
        }
      case ControlMessage_Type.HISTORY_RESP:
        try {
          final raw = jsonDecode(meta) as List<dynamic>;
          final messages = raw
              .map((e) => HistoryMessage.fromJson(e as Map<String, dynamic>))
              .toList();
          _historyController.add(messages);
        } catch (e) {
          Log.e('HISTORY_RESP parse error: $e');
        }
      default:
        Log.d('Control type=${type.value} meta=$meta');
    }
  }

  /// Send a control frame to the server.
  void sendControl(ControlMessage_Type type, String metadata) {
    if (_isDisposed) return;
    _outboundController.add(_makeControl(type, metadata));
  }

  RelayMessage _makeControl(ControlMessage_Type type, String metadata) {
    return RelayMessage(
      messageId: DateTime.now().millisecondsSinceEpoch.toString(),
      channelId: channelId,
      senderId: senderId,
      timestamp: Int64(DateTime.now().millisecondsSinceEpoch),
      control: ControlMessage(type: type, metadata: metadata),
    );
  }

  Future<void> sendMessage(String text) async {
    try {
      final encrypted = await _crypto.encrypt(text);
      _outboundController.add(RelayMessage(
        messageId: DateTime.now().millisecondsSinceEpoch.toString(),
        channelId: channelId,
        senderId: senderId,
        timestamp: Int64(DateTime.now().millisecondsSinceEpoch),
        encryptedData: EncryptedData(
          nonce: Uint8List.fromList(encrypted.nonce),
          ciphertext: Uint8List.fromList(encrypted.cipherText),
          tag: Uint8List.fromList(encrypted.mac.bytes),
        ),
      ));
    } catch (e) {
      Log.e('Send failed: $e');
      rethrow;
    }
  }

  void _handleDisconnect(StreamController<RelayMessage> sessionCtrl) {
    if (!_isConnected && !_isConnecting) return; // already handled
    _isConnected = false;
    _isConnecting = false;
    _keepAliveTimer?.cancel();
    _keepAliveTimer = null;
    _sessionOutboundSubscription?.cancel();
    _responseSubscription?.cancel();
    if (!sessionCtrl.isClosed) sessionCtrl.close();
    _sessionCtrl = null;
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
    _responseSubscription?.cancel();
    if (_sessionCtrl != null && !_sessionCtrl!.isClosed) _sessionCtrl!.close();
    await Future.wait([
      _incomingController.close(),
      _outboundController.close(),
      _typingController.close(),
      _agentListController.close(),
      _historyController.close(),
    ]);
    await _channel.shutdown();
  }
}
