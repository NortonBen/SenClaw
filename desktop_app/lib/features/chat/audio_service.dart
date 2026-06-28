import 'dart:convert';
import 'dart:typed_data';
import 'package:audioplayers/audioplayers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:http/http.dart' as http;
import '../../core/transport/connection.dart';

/// Voice round-trips to the daemon: Whisper transcription (mic → text) and TTS
/// synthesis + playback (text → audio). Network base comes from AppConfig.
class AudioService {
  AudioService(this._ref);
  final Ref _ref;
  final AudioPlayer _player = AudioPlayer();

  String get _base => _ref.read(appConfigProvider).httpBase;

  /// Upload recorded audio bytes to Whisper; returns the recognized text.
  Future<String> transcribe(Uint8List bytes, String filename) async {
    final req = http.MultipartRequest(
        'POST', Uri.parse('$_base/api/whisper/transcribe'));
    req.files.add(http.MultipartFile.fromBytes('audio', bytes, filename: filename));
    final res = await http.Response.fromStream(await req.send());
    if (res.statusCode < 200 || res.statusCode >= 300) {
      throw Exception(res.body);
    }
    try {
      final m = jsonDecode(res.body);
      if (m is Map && m['text'] != null) return '${m['text']}'.trim();
    } catch (_) {}
    return res.body.trim();
  }

  /// Synthesize [text] to speech and play it.
  Future<void> speak(String text) async {
    final res = await http.post(
      Uri.parse('$_base/api/tts/synthesize'),
      headers: {'content-type': 'application/json'},
      body: jsonEncode({'text': text}),
    );
    if (res.statusCode < 200 || res.statusCode >= 300) {
      throw Exception('TTS failed (${res.statusCode}): ${res.body}');
    }
    await _player.stop();
    // The WAV bytes need an explicit mime type or macOS audioplayers can't
    // pick a decoder and playback fails silently.
    await _player.play(BytesSource(res.bodyBytes, mimeType: 'audio/wav'));
  }

  Future<void> stop() => _player.stop();

  void dispose() => _player.dispose();
}

final audioServiceProvider = Provider<AudioService>((ref) {
  final s = AudioService(ref);
  ref.onDispose(s.dispose);
  return s;
});
