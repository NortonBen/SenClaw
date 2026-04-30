import 'dart:async';
import 'dart:typed_data';
import 'package:grpc/grpc.dart';
import '../generated/channel_relay.pbgrpc.dart';
import 'package:fixnum/fixnum.dart';
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

  final _outboundController = StreamController<RelayMessage>.broadcast();
  
  StreamSubscription? _responseSubscription;
  StreamSubscription? _sessionOutboundSubscription;
  Timer? _reconnectTimer;
  bool _isDisposed = false;
  bool _isConnected = false;

  RelayService({
    required String hubUrl,
    required this.channelId,
    required this.senderId,
    required this.accessToken,
    required List<int> encryptionKey,
  }) {
    // Handle hubUrl that might not have a scheme
    String host = "";
    int port = 443;
    bool isSecure = false;

    try {
      if (!hubUrl.contains("://")) {
        // Assume raw host:port
        final parts = hubUrl.split(':');
        host = parts[0];
        if (parts.length > 1) {
          port = int.parse(parts[1]);
        }
      } else {
        final uri = Uri.parse(hubUrl);
        host = uri.host;
        port = uri.port == 0 ? (uri.scheme == 'https' ? 443 : 80) : uri.port;
        isSecure = uri.scheme == 'https';
      }
    } catch (e) {
      Log.e("Error parsing hub URL: $hubUrl. Error: $e");
      // Fallback to defaults or throw
      host = "127.0.0.1";
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
    Log.i("RelayService initialized for $host:$port (secure: $isSecure)");
    Log.i("RelayService: Using encryption key of length ${encryptionKey.length} bytes");
  }

  void start() {
    _connect();
  }

  void _connect() async {
    if (_isDisposed || _isConnected) return;

    _reconnectTimer?.cancel();
    _reconnectTimer = null;

    Log.i("RelayService: Attempting to connect...");

    try {
      // Create a session-specific request controller
      final sessionRequestController = StreamController<RelayMessage>();
      
      // Pipe messages from our main outbound controller to the session controller
      _sessionOutboundSubscription = _outboundController.stream.listen(
        (msg) => sessionRequestController.add(msg),
        onError: (e) => Log.e("Outbound stream error: $e"),
      );

      final responseStream = _client.stream(
        sessionRequestController.stream,
        options: CallOptions(metadata: {
          'channel_id': channelId,
          'access_token': accessToken,
        }),
      );
      
      // Send initial handshake message to unblock the stream
      sessionRequestController.add(RelayMessage(
        channelId: channelId,
        senderId: senderId,
        timestamp: Int64(DateTime.now().millisecondsSinceEpoch),
        messageId: "handshake-${DateTime.now().millisecondsSinceEpoch}",
        control: ControlMessage(
          type: ControlMessage_Type.PING,
        ),
      ));
      
      _responseSubscription = responseStream.listen(
        (msg) async {
          if (!_isConnected) {
            _isConnected = true;
            Log.i("RelayService: Connected and receiving messages");
          }

          if (msg.channelId != channelId) {
            Log.w("Received message for different channel: ${msg.channelId}");
            return;
          }

          Log.d("Received message: ${msg.messageId}");

          if (msg.hasEncryptedData()) {
            final enc = msg.encryptedData;
            try {
              final decrypted = await _crypto.decrypt(
                Uint8List.fromList(enc.nonce),
                Uint8List.fromList(enc.ciphertext),
                Uint8List.fromList(enc.tag),
              );go
              Log.d("Decrypted message: $decrypted");
              _incomingController.add(decrypted);
            } catch (e) {
              Log.e("Decryption failed", error: e);
            }
          }
        },
        onDone: () {
          Log.i("Relay stream closed by server");
          _handleDisconnect(sessionRequestController);
        },
        onError: (e) {
          Log.e("Relay stream error", error: e);
          _handleDisconnect(sessionRequestController);
        },
        cancelOnError: true,
      );
    } catch (e) {
      Log.e("Failed to establish relay connection", error: e);
      _scheduleReconnect();
    }
  }

  void _handleDisconnect(StreamController<RelayMessage> sessionController) {
    _isConnected = false;
    _sessionOutboundSubscription?.cancel();
    _responseSubscription?.cancel();
    sessionController.close();
    
    if (!_isDisposed) {
      _scheduleReconnect();
    }
  }

  void _scheduleReconnect() {
    if (_isDisposed || _reconnectTimer != null) return;
    
    Log.i("RelayService: Scheduling reconnect in 5 seconds...");
    _reconnectTimer = Timer(const Duration(seconds: 5), _connect);
  }

  Future<void> sendMessage(String text) async {
    try {
      Log.t("Sending message: $text");
      final encrypted = await _crypto.encrypt(text);
      Log.t("Sending encrypted message: ${encrypted.cipherText}");
      final msg = RelayMessage(
        messageId: DateTime.now().millisecondsSinceEpoch.toString(),
        channelId: channelId,
        senderId: senderId,
        encryptedData: EncryptedData(
          nonce: Uint8List.fromList(encrypted.nonce),
          ciphertext: Uint8List.fromList(encrypted.cipherText),
          tag: Uint8List.fromList(encrypted.mac.bytes),
        ),
      );
      _outboundController.add(msg);
      Log.d("Encrypted message added to outbound queue");
    } catch (e) {
      Log.e("Failed to encrypt/send message", error: e);
      rethrow;
    }
  }

  Future<void> dispose() async {
    _isDisposed = true;
    _reconnectTimer?.cancel();
    _sessionOutboundSubscription?.cancel();
    _responseSubscription?.cancel();
    await _incomingController.close();
    await _outboundController.close();
    await _channel.shutdown();
    Log.i("RelayService disposed");
  }
}
