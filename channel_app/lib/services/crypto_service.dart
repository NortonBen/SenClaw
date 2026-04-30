import 'dart:convert';
import 'dart:typed_data';
import 'package:cryptography/cryptography.dart';

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
    return await algorithm.encrypt(
      clearText,
      secretKey: secretKey,
    );
  }

  static List<int> parseBase64Key(String base64Key) {
    return base64.decode(base64Key);
  }
}
