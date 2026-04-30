import 'package:flutter_secure_storage/flutter_secure_storage.dart';

class ConfigService {
  static final ConfigService _instance = ConfigService._internal();
  factory ConfigService() => _instance;
  ConfigService._internal();

  final _storage = const FlutterSecureStorage();

  // Storage keys
  static const String _keyHubUrl = 'hub_url';
  static const String _keyGrpcUrl = 'grpc_url';
  static const String _keyChannelId = 'channel_id';
  static const String _keyEncryptionKey = 'encryption_key';
  static const String _keyAccessToken = 'access_token';
  static const String _keyLanguage = 'language_code';

  // Getters
  Future<String?> get hubUrl => _storage.read(key: _keyHubUrl);
  Future<String?> get grpcUrl => _storage.read(key: _keyGrpcUrl);
  Future<String?> get channelId => _storage.read(key: _keyChannelId);
  Future<String?> get encryptionKey => _storage.read(key: _keyEncryptionKey);
  Future<String?> get accessToken => _storage.read(key: _keyAccessToken);
  Future<String> get languageCode async => await _storage.read(key: _keyLanguage) ?? 'vi';

  // Setters
  Future<void> setHubUrl(String value) => _storage.write(key: _keyHubUrl, value: value);
  Future<void> setGrpcUrl(String value) => _storage.write(key: _keyGrpcUrl, value: value);
  Future<void> setChannelId(String value) => _storage.write(key: _keyChannelId, value: value);
  Future<void> setEncryptionKey(String value) => _storage.write(key: _keyEncryptionKey, value: value);
  Future<void> setAccessToken(String value) => _storage.write(key: _keyAccessToken, value: value);
  Future<void> setLanguageCode(String value) => _storage.write(key: _keyLanguage, value: value);

  // Helper for all pairing data
  Future<void> savePairingData({
    required String hubUrl,
    required String grpcUrl,
    required String channelId,
    required String encryptionKey,
    required String accessToken,
  }) async {
    await Future.wait([
      setHubUrl(hubUrl),
      setGrpcUrl(grpcUrl),
      setChannelId(channelId),
      setEncryptionKey(encryptionKey),
      setAccessToken(accessToken),
    ]);
  }

  // Clear connection data (logout) but preserve settings like language
  Future<void> clearAll() async {
    await Future.wait([
      _storage.delete(key: _keyHubUrl),
      _storage.delete(key: _keyGrpcUrl),
      _storage.delete(key: _keyChannelId),
      _storage.delete(key: _keyEncryptionKey),
      _storage.delete(key: _keyAccessToken),
    ]);
  }
}
