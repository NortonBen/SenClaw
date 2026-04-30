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
    final secretBox = SecretBox(
      ciphertext,
      nonce: nonce,
      mac: Mac(tag),
    );

    final clearText = await algorithm.decrypt(
      secretBox,
      secretKey: secretKey,
    );

    return utf8.decode(clearText);
  }

  Future<SecretBox> encrypt(String text) async {
    final clearText = utf8.encode(text);
    Log.i("Encrypting message: $text with key: $secretKey");
    return await algorithm.encrypt(
      clearText,
      secretKey: secretKey,
    );
  }

  static Future<List<int>> deriveKey(String base64Key) async {
    final decoded = base64.decode(base64Key);
    
    // Always hash to ensure consistency and correct 32-byte length for AES-256
    final sha256 = Sha256();
    final hash = await sha256.hash(decoded);
    
    Log.i("Derived 32-byte key from input of length ${decoded.length}");
    return hash.bytes;
  }

  static List<int> parseBase64Key(String base64Key) {
    return base64.decode(base64Key);
  }
}
