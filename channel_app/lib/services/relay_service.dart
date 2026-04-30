import 'dart:async';
import 'dart:typed_data';
import 'package:grpc/grpc.dart';
import '../generated/channel_relay.pbgrpc.dart';
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

  RelayService({
    required String hubUrl,
    required this.channelId,
    required this.senderId,
    required this.accessToken,
    required List<int> encryptionKey,
  }) {
    final uri = Uri.parse(hubUrl);
    _channel = ClientChannel(
      uri.host,
      port: uri.port == 0 ? 443 : uri.port,
      options: ChannelOptions(
        credentials: uri.scheme == 'https' 
            ? const ChannelCredentials.secure() 
            : const ChannelCredentials.insecure(),
      ),
    );
    _client = ChannelRelayClient(_channel);
    _crypto = CryptoService(encryptionKey);
    Log.i("RelayService initialized for channel: $channelId");
  }

  void start() {
    final outboundStream = StreamController<RelayMessage>();
    
    // Initial control message or authentication could go here
    
    final responseStream = _client.stream(
      outboundStream.stream,
      options: CallOptions(metadata: {
        'channel_id': channelId,
        'access_token': accessToken,
      }),
    );
    
    responseStream.listen((msg) async {
      Log.t("Received message from relay: ${msg.channelId}");
      if (msg.channelId != channelId) {
        Log.w("Received message for different channel: ${msg.channelId}");
        return;
      }

      if (msg.hasEncryptedData()) {
        final enc = msg.encryptedData;
        try {
          final decrypted = await _crypto.decrypt(
            Uint8List.fromList(enc.nonce),
            Uint8List.fromList(enc.ciphertext),
            Uint8List.fromList(enc.tag),
          );
          Log.d("Successfully decrypted message");
          _incomingController.add(decrypted);
        } catch (e) {
          Log.e("Decryption failed", error: e);
        }
      }
    }, onDone: () {
      Log.i("Relay stream closed");
    }, onError: (e) {
      Log.e("Relay stream error", error: e);
    });
  }

  Future<void> dispose() async {
    await _incomingController.close();
    await _channel.shutdown();
  }
}
