import 'dart:convert';
import 'dart:typed_data';
import 'package:cryptography/cryptography.dart';
import 'logger_service.dart';

class CryptoService {
  final AesGcm algorithm = AesGcm.with256bits();
  final SecretKey secretKey;

  CryptoService(List<int> keyBytes)
      : secretKey = SecretKey(keyBytes);

  Future<String> decrypt(Uint8List nonce, Uint8List ciphertext, Uint8List tag) async {
    final secretBox = SecretBox(ciphertext, nonce: nonce, mac: Mac(tag));
    final clearText = await algorithm.decrypt(secretBox, secretKey: secretKey);
    return utf8.decode(clearText);
  }

  Future<SecretBox> encrypt(String text) async {
    final clearText = utf8.encode(text);
    return await algorithm.encrypt(clearText, secretKey: secretKey);
  }

  /// Derive a 32-byte AES key from a base64-encoded string.
  /// If the decoded bytes are already 32 bytes, use them directly (no hashing).
  /// Otherwise apply SHA-256 to produce a 32-byte key.
  /// Must match server-side Crypto::new_from_b64 in src/util/crypto.rs.
  static Future<List<int>> deriveKey(String base64Key) async {
    final decoded = base64.decode(base64Key);
    if (decoded.length == 32) {
      Log.i('CryptoService: using raw 32-byte key');
      return decoded;
    }
    final sha256 = Sha256();
    final hash = await sha256.hash(decoded);
    Log.i('CryptoService: derived 32-byte key via SHA-256 from ${decoded.length}-byte input');
    return hash.bytes;
  }

  static List<int> parseBase64Key(String base64Key) => base64.decode(base64Key);
}
