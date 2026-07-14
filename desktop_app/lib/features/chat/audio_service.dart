import 'dart:convert';
import 'package:audioplayers/audioplayers.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:http/http.dart' as http;
import '../../core/transport/connection.dart';

/// Voice round-trips to the daemon: Whisper transcription (mic → text) and TTS
/// synthesis + playback (text → audio). Network base comes from AppConfig.
class AudioService {
  AudioService(this._ref);
  // NOTE: do NOT clear `speaking` from a global onPlayerStateChanged listener.
  // Long text is pipelined into several sentence clips, so PlayerState.completed
  // fires once per clip — clearing on the first clip's completion flipped the UI
  // back to Play after one segment while the pipeline kept playing the rest.
  // `speak()` owns `speaking`: it clears it in finally when the whole pipeline
  // ends, and `stop()` clears it explicitly.
  final Ref _ref;
  final AudioPlayer _player = AudioPlayer();

  /// Key of the utterance currently being synthesized/played (the text passed
  /// to [speak]), or `null` when idle. UI widgets listen to this to render a
  /// Stop control while their message is speaking.
  final ValueNotifier<String?> speaking = ValueNotifier(null);

  /// Bumped on every speak()/stop(); a stale synthesis response (the HTTP
  /// round-trip can take a moment) must not start playing after a stop.
  int _gen = 0;

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

  /// Synthesize [text] to speech and play it. Supersedes any utterance that is
  /// still synthesizing or playing. Call [stop] to interrupt.
  ///
  /// Long texts are split into sentences and pipelined: the first sentence
  /// plays as soon as it is synthesized (fast time-to-first-audio) while the
  /// next one synthesizes in the background during playback.
  Future<void> speak(String text) async {
    final gen = ++_gen;
    await _player.stop();
    speaking.value = text;
    var spoke = false;
    try {
      // Chat text is raw markdown — strip formatting to speakable prose
      // first (no "sao sao" for **bold**), then drop fragments with nothing
      // speakable ("---", "```", lone bullets).
      final parts = splitSentences(stripMarkdownForSpeech(text))
          .where(hasSpeakableContent)
          .toList();
      if (parts.isEmpty) return;
      // Eager future = prefetch: sentence i+1 synthesizes while i plays.
      // _synthesize never throws (returns null) so a failed or skipped
      // sentence can NEVER end the whole read-aloud — we move on.
      Future<Uint8List?>? next = _synthesize(parts.first);
      for (var i = 0; i < parts.length; i++) {
        final bytes = await next!;
        if (gen != _gen) return; // stopped/superseded during synthesis
        next = (i + 1 < parts.length) ? _synthesize(parts[i + 1]) : null;
        if (bytes == null) continue; // this sentence failed — skip it
        if (await _playClip(bytes, gen)) {
          spoke = true;
        }
        if (gen != _gen) return;
      }
      if (!spoke) {
        throw Exception('TTS failed for every sentence');
      }
    } catch (_) {
      if (!spoke) {
        if (gen == _gen) speaking.value = null;
        rethrow;
      }
    } finally {
      if (gen == _gen) speaking.value = null;
    }
  }

  /// Play one WAV clip and wait for it to finish. Retries once — audioplayers
  /// can reject a play() issued in the same tick its previous source finishes
  /// (native player still finalizing), which used to kill the whole pipeline
  /// after the first segment.
  Future<bool> _playClip(Uint8List bytes, int gen) async {
    for (var attempt = 0; attempt < 2; attempt++) {
      try {
        // The WAV bytes need an explicit mime type or macOS audioplayers
        // can't pick a decoder and playback fails silently.
        await _player.play(BytesSource(bytes, mimeType: 'audio/wav'));
        // Wait until this clip finishes (or is stopped). The timeout guards a
        // missed state event on very short clips — never hang the pipeline.
        await _player.onPlayerStateChanged
            .firstWhere(
                (s) => s == PlayerState.completed || s == PlayerState.stopped)
            .timeout(Duration(milliseconds: _wavMs(bytes) + 2000),
                onTimeout: () => PlayerState.completed);
        return true;
      } catch (_) {
        if (gen != _gen) return false;
        await Future.delayed(const Duration(milliseconds: 150));
        try {
          await _player.stop(); // reset native state before the retry
        } catch (_) {}
      }
    }
    return false;
  }

  /// Never throws: returns the WAV bytes or `null` on any failure, so one bad
  /// sentence can't abort the sentence pipeline.
  Future<Uint8List?> _synthesize(String text) async {
    try {
      final res = await http.post(
        Uri.parse('$_base/api/tts/synthesize'),
        headers: {'content-type': 'application/json'},
        body: jsonEncode({'text': text}),
      );
      if (res.statusCode < 200 || res.statusCode >= 300) return null;
      return res.bodyBytes;
    } catch (_) {
      return null;
    }
  }

  /// Playback length of a 16-bit PCM WAV in ms (from its header byte rate).
  static int _wavMs(Uint8List b) {
    if (b.length < 44) return 0;
    final bd = ByteData.sublistView(b);
    final byteRate = bd.getUint32(28, Endian.little);
    if (byteRate == 0) return 0;
    return ((b.length - 44) * 1000 / byteRate).round();
  }

  /// Stop playback (or cancel an in-flight synthesis before it plays).
  Future<void> stop() async {
    _gen++;
    speaking.value = null;
    await _player.stop();
  }

  void dispose() => _player.dispose();
}

final audioServiceProvider = Provider<AudioService>((ref) {
  final s = AudioService(ref);
  ref.onDispose(s.dispose);
  return s;
});

/// Split [text] into sentence-sized speech chunks (≤ [maxChars] each).
///
/// Cuts at sentence enders (. ! ? … ; newline), falls back to a space cut when
/// a sentence runs past [maxChars], and merges fragments shorter than
/// [minChars] (e.g. list numbers like "1.") into their neighbor so the TTS
/// pipeline doesn't fire tiny requests. Top-level so it's unit-testable.
List<String> splitSentences(String text, {int maxChars = 220, int minChars = 8}) {
  final pieces = <String>[];
  final cur = StringBuffer();
  var curLen = 0;
  void flush() {
    final s = cur.toString().trim();
    if (s.isNotEmpty) pieces.add(s);
    cur.clear();
    curLen = 0;
  }

  final runes = text.runes.toList();
  bool isDigit(int r) => r >= 0x30 && r <= 0x39;
  for (var i = 0; i < runes.length; i++) {
    final ch = String.fromCharCode(runes[i]);
    cur.write(ch);
    curLen++;
    if ('.!?…;\n'.contains(ch)) {
      // A '.' BETWEEN digits is a decimal/version separator ("0.08",
      // "6.6.56") — cutting there produced tiny nonsense clips.
      final decimalDot = ch == '.' &&
          i > 0 &&
          i + 1 < runes.length &&
          isDigit(runes[i - 1]) &&
          isDigit(runes[i + 1]);
      if (!decimalDot) {
        flush();
      }
    } else if (curLen >= maxChars && ch == ' ') {
      flush();
    }
  }
  flush();

  // Merge fragments that are not sentences of their own: a too-short leading
  // piece (a list number like "1.") pulls the next piece in, and a too-short
  // trailing piece merges backward unless it is a complete sentence itself
  // ("Câu ba?" stays separate). Always respects maxChars.
  final out = <String>[];
  for (final p in pieces) {
    final isSentence = '.!?…;'.contains(p[p.length - 1]);
    final canMergePrev = out.isNotEmpty &&
        (out.last.length < minChars || (p.length < minChars && !isSentence)) &&
        out.last.length + 1 + p.length <= maxChars;
    if (canMergePrev) {
      out[out.length - 1] = '${out.last} $p';
    } else {
      out.add(p);
    }
  }
  return out;
}

/// True when a fragment contains at least one letter or digit — markdown
/// leftovers like "---", "```" or "**" have nothing to speak.
bool hasSpeakableContent(String s) =>
    RegExp(r'[\p{L}\p{N}]', unicode: true).hasMatch(s);

/// Normalize markdown to speakable prose: keep the CONTENT, drop the syntax.
///
/// TTS otherwise reads the markers literally ("sao sao" for `**`, "gạch gạch
/// gạch" for `---`). Order matters: fences/rules go first so emphasis
/// stripping can't turn `***` into a stray `*`. Newlines are preserved — the
/// sentence splitter uses them as boundaries.
String stripMarkdownForSpeech(String text) {
  var t = text;
  // Fenced code markers (``` / ```lang) — keep the code lines themselves.
  t = t.replaceAll(RegExp(r'^\s*```[^\n]*$', multiLine: true), '');
  // Horizontal rules (---, ***, ___) on their own line.
  t = t.replaceAll(RegExp(r'^\s*([-*_]\s*){3,}$', multiLine: true), '');
  // Images ![alt](url) → alt, links [text](url) → text.
  t = t.replaceAllMapped(
      RegExp(r'!\[([^\]]*)\]\([^)]*\)'), (m) => m[1] ?? '');
  t = t.replaceAllMapped(
      RegExp(r'\[([^\]]+)\]\([^)]*\)'), (m) => m[1] ?? '');
  // Table separator rows (|---|:---:|), then pipes → spaces.
  t = t.replaceAll(
      RegExp(r'^\s*\|?\s*:?-{2,}[-:|\s]*$', multiLine: true), '');
  t = t.replaceAll('|', ' ');
  // Emphasis / strikethrough / inline-code markers (content stays).
  t = t.replaceAll(RegExp(r'\*\*|__|~~|[*`]'), '');
  // Headings (#…) and blockquotes (>) at line start.
  t = t.replaceAll(RegExp(r'^\s{0,3}#{1,6}\s+', multiLine: true), '');
  t = t.replaceAll(RegExp(r'^\s{0,3}>\s?', multiLine: true), '');
  // List bullets at line start (- + •); numbers like "1." stay.
  t = t.replaceAll(RegExp(r'^\s*[-+•]\s+', multiLine: true), '');
  // Bare HTML tags.
  t = t.replaceAll(RegExp(r'</?[a-zA-Z][^>\n]*>'), ' ');
  // "~1.85 GB" → "khoảng 1.85 GB" (approximation tilde), stray ~ dropped.
  t = t.replaceAllMapped(
      RegExp(r'~\s?(?=\d)'), (m) => 'khoảng ');
  t = t.replaceAll('~', ' ');
  // Collapse runs of spaces/tabs (newlines untouched).
  t = t.replaceAll(RegExp(r'[ \t]{2,}'), ' ');
  return t.trim();
}
