import 'package:flutter_secure_storage/flutter_secure_storage.dart';

class ConfigService {
  static final ConfigService _instance = ConfigService._internal();
  factory ConfigService() => _instance;
  ConfigService._internal();

  final _storage = const FlutterSecureStorage();

  static const String _keyHubUrl = 'hub_url';
  static const String _keyGrpcUrl = 'grpc_url';
  static const String _keyChannelId = 'channel_id';
  static const String _keyEncryptionKey = 'encryption_key';
  static const String _keyAccessToken = 'access_token';
  static const String _keyLanguage = 'language_code';
  static const String _keySelectedAgentFolder = 'selected_agent_folder';
  static const String _keySelectedAgentName = 'selected_agent_name';

  Future<String?> get hubUrl => _storage.read(key: _keyHubUrl);
  Future<String?> get grpcUrl => _storage.read(key: _keyGrpcUrl);
  Future<String?> get channelId => _storage.read(key: _keyChannelId);
  Future<String?> get encryptionKey => _storage.read(key: _keyEncryptionKey);
  Future<String?> get accessToken => _storage.read(key: _keyAccessToken);
  Future<String> get languageCode async =>
      await _storage.read(key: _keyLanguage) ?? 'vi';
  Future<String?> get selectedAgentFolder =>
      _storage.read(key: _keySelectedAgentFolder);
  Future<String?> get selectedAgentName =>
      _storage.read(key: _keySelectedAgentName);

  Future<void> setHubUrl(String v) => _storage.write(key: _keyHubUrl, value: v);
  Future<void> setGrpcUrl(String v) => _storage.write(key: _keyGrpcUrl, value: v);
  Future<void> setChannelId(String v) => _storage.write(key: _keyChannelId, value: v);
  Future<void> setEncryptionKey(String v) => _storage.write(key: _keyEncryptionKey, value: v);
  Future<void> setAccessToken(String v) => _storage.write(key: _keyAccessToken, value: v);
  Future<void> setLanguageCode(String v) => _storage.write(key: _keyLanguage, value: v);
  Future<void> setSelectedAgentFolder(String v) =>
      _storage.write(key: _keySelectedAgentFolder, value: v);
  Future<void> setSelectedAgentName(String v) =>
      _storage.write(key: _keySelectedAgentName, value: v);

  Future<void> clearSelectedAgent() => Future.wait([
        _storage.delete(key: _keySelectedAgentFolder),
        _storage.delete(key: _keySelectedAgentName),
      ]);

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

  Future<void> clearAll() async {
    await Future.wait([
      _storage.delete(key: _keyHubUrl),
      _storage.delete(key: _keyGrpcUrl),
      _storage.delete(key: _keyChannelId),
      _storage.delete(key: _keyEncryptionKey),
      _storage.delete(key: _keyAccessToken),
      _storage.delete(key: _keySelectedAgentFolder),
      _storage.delete(key: _keySelectedAgentName),
    ]);
  }
}
