import 'dart:async';
import 'dart:io' show File, Platform;
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:http/http.dart' as http;
import 'package:path_provider/path_provider.dart';
import 'package:record/record.dart';
import '../../core/transport/connection.dart';
import '../../models/chat_message.dart';
import '../../theme/tokens.dart';
import 'audio_service.dart';
import 'conversation_provider.dart';
import 'groups_provider.dart';

/// Stable jid for the single persistent "Voice assistant" chat. The dashboard's
/// voice button routes here so hands-free chatter builds up its own context and
/// stays out of the user's other conversations (mirrors [kRemindersJid]).
const kVoiceChatJid = 'web:voice:main';

/// Ensure the dedicated Voice-assistant chat exists, returning its jid — same
/// best-effort register-then-wait frame as `ensureRemindersChat`.
Future<String> ensureVoiceChat(WidgetRef ref) async {
  bool exists() => ref.read(groupsProvider).any((g) => g.jid == kVoiceChatJid);
  if (exists()) return kVoiceChatJid;
  ref.read(wsClientProvider).send({
    'type': 'register:group',
    'jid': kVoiceChatJid,
    'folder': 'voice',
    'name': 'Trợ lý thoại',
    'groupType': 'chat',
    'requiresTrigger': false,
  });
  final deadline = DateTime.now().add(const Duration(seconds: 5));
  while (DateTime.now().isBefore(deadline)) {
    await Future.delayed(const Duration(milliseconds: 100));
    if (exists()) return kVoiceChatJid;
  }
  return kVoiceChatJid; // daemon almost certainly registered it; proceed.
}

/// Open the hands-free voice-chat overlay for [jid].
///
/// Speech-in / speech-out loop: the user talks, Whisper transcribes and the
/// text is sent as a normal turn, the agent's reply is spoken back with TTS,
/// then the mic re-arms automatically for the next turn — a real back-and-forth
/// conversation with the agent, no typing.
Future<void> showVoiceChat(BuildContext context, String jid, String title) {
  return showDialog(
    context: context,
    barrierDismissible: true,
    barrierColor: Colors.black.withValues(alpha: 0.62),
    builder: (_) => _VoiceChatDialog(jid: jid, title: title),
  );
}

/// Ensure the dedicated voice chat exists, then open the hands-free overlay on
/// it. Convenience for surfaces (e.g. the Dashboard) that have no "current chat"
/// to talk to.
Future<void> showDefaultVoiceChat(BuildContext context, WidgetRef ref) async {
  final jid = await ensureVoiceChat(ref);
  if (context.mounted) showVoiceChat(context, jid, 'Trợ lý thoại');
}

/// Where we are in the conversation loop. Drives the orb colour, icon and
/// status caption, and gates every async continuation (a stale transcription
/// or TTS finish must not act if the phase moved on).
enum _VoicePhase { idle, listening, transcribing, thinking, speaking }

class _VoiceChatDialog extends ConsumerStatefulWidget {
  const _VoiceChatDialog({required this.jid, required this.title});
  final String jid;
  final String title;

  @override
  ConsumerState<_VoiceChatDialog> createState() => _VoiceChatDialogState();
}

class _VoiceChatDialogState extends ConsumerState<_VoiceChatDialog>
    with SingleTickerProviderStateMixin {
  // ── Voice-activity detection tuning (dBFS from record's amplitude stream) ──
  static const _ampInterval = Duration(milliseconds: 180);
  static const _speakDbfs = -30.0; // above this = the user is talking
  static const _silenceDbfs = -42.0; // below this = silence
  static const _silenceHoldSamples = 8; // ~1.4s of trailing silence → send
  static const _maxListenMs = 20000; // hard cap so a turn can't run forever

  final _recorder = AudioRecorder();
  StreamSubscription<Amplitude>? _ampSub;
  Timer? _maxTimer;
  ProviderSubscription<ConversationState>? _convoSub;
  late final AnimationController _pulse;

  _VoicePhase _phase = _VoicePhase.idle;

  /// Bumped whenever we interrupt / re-arm so an in-flight transcription or
  /// `speak()` future that resolves late can detect it's stale and bail.
  int _gen = 0;
  bool _closing = false;

  /// True once the loop is running; a paused loop stops re-arming the mic.
  bool _active = true;

  /// Id of the last completed agent message we've already handled — anchors
  /// "a NEW reply arrived" so we never re-speak an existing bubble.
  String? _spokenMsgId;

  String _lastUserText = '';
  String _lastAgentText = '';

  /// Normalised mic level (0..1) for the orb animation while listening.
  double _level = 0;

  AudioService get _audio => ref.read(audioServiceProvider);
  ConversationNotifier get _convo =>
      ref.read(conversationProvider(widget.jid).notifier);

  @override
  void initState() {
    super.initState();
    _pulse = AnimationController(
      vsync: this,
      duration: const Duration(milliseconds: 1600),
    )..repeat(reverse: true);

    // Watch for the agent's reply so we can speak it back.
    _convoSub = ref.listenManual(conversationProvider(widget.jid), (_, next) {
      if (_closing || _phase != _VoicePhase.thinking) return;
      final reply = _lastCompletedAgent(next);
      if (reply != null &&
          reply.id != _spokenMsgId &&
          (reply.text ?? '').trim().isNotEmpty) {
        _spokenMsgId = reply.id;
        _speakReply(reply.text!.trim());
      }
    });

    // Kick the loop off once the first frame is up.
    WidgetsBinding.instance.addPostFrameCallback((_) => _startListening());
  }

  @override
  void dispose() {
    _closing = true;
    _gen++;
    _ampSub?.cancel();
    _maxTimer?.cancel();
    _convoSub?.close();
    _pulse.dispose();
    _recorder.dispose();
    // Fire-and-forget: silence any TTS still playing when the sheet closes.
    unawaited(_audio.stop());
    super.dispose();
  }

  ChatMessage? _lastCompletedAgent(ConversationState s) {
    for (final m in s.messages.reversed) {
      if (m.kind == MessageKind.agent && !m.streaming) return m;
    }
    return null;
  }

  // ── Listening ────────────────────────────────────────────────────────────
  Future<void> _startListening() async {
    if (_closing || !_active) return;
    _gen++;
    await _stopCapture(); // clean slate — cancel any prior amp sub/timer
    if (!await _recorder.hasPermission()) {
      _snack('Không có quyền truy cập micro');
      if (mounted) setState(() => _phase = _VoicePhase.idle);
      return;
    }
    String path = '';
    if (!kIsWeb) {
      final dir = await getTemporaryDirectory();
      path =
          '${dir.path}${Platform.pathSeparator}senclaw_voice_${DateTime.now().millisecondsSinceEpoch}.m4a';
    }
    try {
      await _recorder.start(const RecordConfig(), path: path);
    } catch (e) {
      _snack('Không thể bắt đầu ghi âm: $e');
      if (mounted) setState(() => _phase = _VoicePhase.idle);
      return;
    }
    if (!mounted || _closing) return;
    setState(() {
      _phase = _VoicePhase.listening;
      _level = 0;
    });
    _beginVad();
  }

  /// Auto-stop on trailing silence (once the user has actually spoken) via the
  /// amplitude stream, plus a hard [_maxListenMs] backstop in case the stream
  /// never emits (some platforms) — the loop must always make progress.
  void _beginVad() {
    var hasSpoken = false;
    var silentRun = 0;
    final startedAt = DateTime.now();
    _ampSub = _recorder.onAmplitudeChanged(_ampInterval).listen(
      (amp) {
        if (_phase != _VoicePhase.listening) return;
        final db = amp.current; // dBFS, ≤ 0 (0 = loudest)
        final norm = ((db + 50) / 45).clamp(0.0, 1.0);
        if (mounted) setState(() => _level = norm);
        if (db > _speakDbfs) {
          hasSpoken = true;
          silentRun = 0;
        } else if (db < _silenceDbfs) {
          silentRun++;
        } else {
          silentRun = 0;
        }
        final elapsed = DateTime.now().difference(startedAt).inMilliseconds;
        if ((hasSpoken && silentRun >= _silenceHoldSamples) ||
            elapsed > _maxListenMs) {
          _finishListening();
        }
      },
      onError: (_) {/* amplitude unsupported → rely on the backstop / tap */},
    );
    _maxTimer = Timer(
        const Duration(milliseconds: _maxListenMs + 500), _finishListening);
  }

  /// Cancel the amplitude subscription and backstop timer, and stop the mic if
  /// it's still recording. Returns the recorded file path (or null).
  Future<String?> _stopCapture() async {
    await _ampSub?.cancel();
    _ampSub = null;
    _maxTimer?.cancel();
    _maxTimer = null;
    if (await _recorder.isRecording()) {
      return _recorder.stop();
    }
    return null;
  }

  /// End the current listen: transcribe, and either send the turn or (if
  /// nothing intelligible was heard) quietly re-arm the mic.
  Future<void> _finishListening() async {
    if (_phase != _VoicePhase.listening) return;
    setState(() => _phase = _VoicePhase.transcribing);
    final gen = _gen;
    try {
      final out = await _stopCapture();
      if (gen != _gen || _closing) return;
      if (out == null) {
        _startListening();
        return;
      }
      Uint8List bytes;
      String filename;
      if (kIsWeb) {
        bytes = (await http.get(Uri.parse(out))).bodyBytes;
        filename = 'recording.webm';
      } else {
        bytes = await File(out).readAsBytes();
        filename = out.split(Platform.pathSeparator).last;
      }
      final text = (await _audio.transcribe(bytes, filename)).trim();
      if (gen != _gen || _closing) return;
      if (text.isEmpty) {
        _startListening(); // heard nothing usable — listen again
        return;
      }
      // Anchor reply detection to the current tail BEFORE sending.
      _spokenMsgId =
          _lastCompletedAgent(ref.read(conversationProvider(widget.jid)))?.id;
      setState(() {
        _lastUserText = text;
        _phase = _VoicePhase.thinking;
      });
      _convo.sendText(text);
    } catch (e) {
      if (gen != _gen || _closing) return;
      _snack('Nhận dạng giọng nói thất bại: $e');
      _startListening();
    }
  }

  // ── Speaking the reply ─────────────────────────────────────────────────────
  Future<void> _speakReply(String text) async {
    if (_closing) return;
    setState(() {
      _lastAgentText = text;
      _phase = _VoicePhase.speaking;
    });
    final gen = _gen;
    try {
      await _audio.speak(text);
    } catch (_) {/* TTS is best-effort */}
    if (gen != _gen || _closing || !_active) return;
    _startListening(); // reply spoken → hand the turn back to the user
  }

  // ── Controls ───────────────────────────────────────────────────────────────
  /// The big orb's tap action depends on the phase.
  void _onOrbTap() {
    switch (_phase) {
      case _VoicePhase.listening:
        _finishListening(); // send now, don't wait for silence
      case _VoicePhase.speaking:
        _gen++; // barge-in: cut the TTS off and listen again
        unawaited(_audio.stop());
        _startListening();
      case _VoicePhase.thinking:
        _gen++; // cancel this turn, take the mic back
        _convo.stop();
        _startListening();
      case _VoicePhase.idle:
      case _VoicePhase.transcribing:
        _startListening();
    }
  }

  /// Pause/resume the hands-free loop without leaving the sheet.
  void _togglePause() {
    if (_active) {
      setState(() => _active = false);
      _gen++;
      unawaited(_audio.stop());
      unawaited(_stopCapture());
      setState(() => _phase = _VoicePhase.idle);
    } else {
      setState(() => _active = true);
      _startListening();
    }
  }

  void _snack(String msg) {
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(msg)));
  }

  // ── UI ──────────────────────────────────────────────────────────────────
  ({Color color, IconData icon, String label}) _phaseStyle() {
    switch (_phase) {
      case _VoicePhase.listening:
        return (color: AppTokens.brand, icon: Icons.mic, label: 'Đang nghe…');
      case _VoicePhase.transcribing:
        return (
          color: AppTokens.brand,
          icon: Icons.graphic_eq,
          label: 'Đang nhận dạng…'
        );
      case _VoicePhase.thinking:
        return (
          color: AppTokens.warning,
          icon: Icons.more_horiz,
          label: 'Đang suy nghĩ…'
        );
      case _VoicePhase.speaking:
        return (
          color: AppTokens.success,
          icon: Icons.volume_up_rounded,
          label: 'Đang trả lời…'
        );
      case _VoicePhase.idle:
        return (
          color: AppTokens.brand,
          icon: Icons.mic_none,
          label: _active ? 'Nhấn để nói' : 'Đã tạm dừng'
        );
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final s = _phaseStyle();
    final busy =
        _phase == _VoicePhase.transcribing || _phase == _VoicePhase.thinking;

    return Dialog(
      backgroundColor: Colors.transparent,
      insetPadding: const EdgeInsets.all(AppTokens.s24),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 460, maxHeight: 620),
        child: Container(
          decoration: BoxDecoration(
            color: c.surface,
            border: Border.all(color: c.border),
            borderRadius: BorderRadius.circular(AppTokens.rXl),
          ),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              // Header: title + close.
              Padding(
                padding: const EdgeInsets.fromLTRB(
                    AppTokens.s20, AppTokens.s16, AppTokens.s8, AppTokens.s8),
                child: Row(
                  children: [
                    Icon(Icons.graphic_eq, size: 18, color: c.accent),
                    const SizedBox(width: AppTokens.s8),
                    Expanded(
                      child: Text(
                        'Trò chuyện thoại · ${widget.title}',
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: TextStyle(
                          color: c.textPrimary,
                          fontWeight: FontWeight.w700,
                          fontSize: 15,
                        ),
                      ),
                    ),
                    IconButton(
                      tooltip: 'Kết thúc',
                      icon: const Icon(Icons.close, size: 20),
                      onPressed: () => Navigator.of(context).maybePop(),
                    ),
                  ],
                ),
              ),
              Divider(height: 1, color: c.border),
              // Center: the orb + status.
              Expanded(
                child: Padding(
                  padding: const EdgeInsets.symmetric(
                      horizontal: AppTokens.s24, vertical: AppTokens.s16),
                  child: Column(
                    mainAxisAlignment: MainAxisAlignment.center,
                    children: [
                      _orb(s.color, s.icon, busy),
                      const SizedBox(height: AppTokens.s24),
                      Text(
                        s.label,
                        style: TextStyle(
                          color: c.textPrimary,
                          fontSize: 16,
                          fontWeight: FontWeight.w600,
                        ),
                      ),
                      const SizedBox(height: AppTokens.s16),
                      _caption(c),
                    ],
                  ),
                ),
              ),
              Divider(height: 1, color: c.border),
              // Footer controls.
              Padding(
                padding: const EdgeInsets.all(AppTokens.s16),
                child: Row(
                  mainAxisAlignment: MainAxisAlignment.center,
                  children: [
                    TextButton.icon(
                      onPressed: _togglePause,
                      icon: Icon(_active
                          ? Icons.pause_rounded
                          : Icons.play_arrow_rounded),
                      label: Text(_active ? 'Tạm dừng' : 'Tiếp tục'),
                    ),
                    const SizedBox(width: AppTokens.s8),
                    TextButton.icon(
                      onPressed: () => Navigator.of(context).maybePop(),
                      icon: const Icon(Icons.call_end_rounded),
                      style: TextButton.styleFrom(
                          foregroundColor: AppTokens.danger),
                      label: const Text('Kết thúc'),
                    ),
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }

  /// The tappable, pulsing central orb. While listening it also swells with the
  /// live mic level; when busy it shows a spinner ring.
  Widget _orb(Color color, IconData icon, bool busy) {
    return GestureDetector(
      onTap: _onOrbTap,
      child: AnimatedBuilder(
        animation: _pulse,
        builder: (context, _) {
          final breathe = 1.0 + 0.06 * _pulse.value;
          final levelBoost =
              _phase == _VoicePhase.listening ? 0.28 * _level : 0.0;
          final scale = breathe + levelBoost;
          return SizedBox(
            width: 168,
            height: 168,
            child: Stack(
              alignment: Alignment.center,
              children: [
                // Soft outer halo.
                Transform.scale(
                  scale: scale,
                  child: Container(
                    width: 168,
                    height: 168,
                    decoration: BoxDecoration(
                      shape: BoxShape.circle,
                      color: color.withValues(alpha: 0.12),
                    ),
                  ),
                ),
                Transform.scale(
                  scale: 0.86 + levelBoost * 0.5,
                  child: Container(
                    width: 132,
                    height: 132,
                    decoration: BoxDecoration(
                      shape: BoxShape.circle,
                      color: color.withValues(alpha: 0.20),
                    ),
                  ),
                ),
                // Core disc.
                Container(
                  width: 96,
                  height: 96,
                  decoration: BoxDecoration(
                    shape: BoxShape.circle,
                    color: color,
                    boxShadow: [
                      BoxShadow(
                        color: color.withValues(alpha: 0.45),
                        blurRadius: 24,
                        spreadRadius: 2,
                      ),
                    ],
                  ),
                  child: Icon(icon, color: Colors.white, size: 40),
                ),
                if (busy)
                  SizedBox(
                    width: 132,
                    height: 132,
                    child: CircularProgressIndicator(
                      strokeWidth: 2.5,
                      valueColor: AlwaysStoppedAnimation(
                          color.withValues(alpha: 0.7)),
                    ),
                  ),
              ],
            ),
          );
        },
      ),
    );
  }

  Widget _caption(AppColors c) {
    if (_lastUserText.isEmpty && _lastAgentText.isEmpty) {
      return Text(
        'Hãy nói để bắt đầu. Trợ lý sẽ trả lời bằng giọng nói.',
        textAlign: TextAlign.center,
        style: TextStyle(color: c.textMuted, fontSize: 13),
      );
    }
    return SingleChildScrollView(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (_lastUserText.isNotEmpty)
            _bubble(c, 'Bạn', _lastUserText, c.surfaceAlt, c.textPrimary),
          if (_lastAgentText.isNotEmpty) ...[
            const SizedBox(height: AppTokens.s8),
            _bubble(c, 'Trợ lý', _lastAgentText,
                c.accent.withValues(alpha: 0.14), c.textPrimary),
          ],
        ],
      ),
    );
  }

  Widget _bubble(AppColors c, String who, String text, Color bg, Color fg) {
    return Container(
      padding: const EdgeInsets.symmetric(
          horizontal: AppTokens.s12, vertical: AppTokens.s8),
      decoration: BoxDecoration(
        color: bg,
        borderRadius: BorderRadius.circular(AppTokens.rLg),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(who,
              style: TextStyle(
                  color: c.textMuted,
                  fontSize: 11,
                  fontWeight: FontWeight.w700)),
          const SizedBox(height: 2),
          Text(
            text,
            maxLines: 4,
            overflow: TextOverflow.ellipsis,
            style: TextStyle(color: fg, fontSize: 13, height: 1.35),
          ),
        ],
      ),
    );
  }
}
