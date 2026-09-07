import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/i18n/l10n.dart';
import '../../core/transport/connection.dart';
import '../../theme/tokens.dart';

/// One in-flight watch, as `/api/watches` reports it.
class WatchInfo {
  final String id;
  final String? label;
  final String? tool;
  final int checks;
  final int maxChecks;
  final int intervalSecs;
  final String? givesUpAt;
  final String? lastError;

  const WatchInfo({
    required this.id,
    this.label,
    this.tool,
    this.checks = 0,
    this.maxChecks = 0,
    this.intervalSecs = 0,
    this.givesUpAt,
    this.lastError,
  });

  factory WatchInfo.fromJson(Map<String, dynamic> j) => WatchInfo(
        id: '${j['id'] ?? ''}',
        label: j['label'] as String?,
        tool: j['tool'] as String?,
        checks: (j['checks'] as num?)?.toInt() ?? 0,
        maxChecks: (j['maxChecks'] as num?)?.toInt() ?? 0,
        intervalSecs: (j['intervalSecs'] as num?)?.toInt() ?? 0,
        givesUpAt: j['givesUpAt'] as String?,
        lastError: j['lastError'] as String?,
      );
}

/// Poll cadence. A watch checks at most once a minute, so this is plenty.
const _refresh = Duration(seconds: 15);

/// What this chat is waiting on, and the button that stops it.
///
/// A watch runs silently — correct, because each check costs no tokens, but it
/// left the user unable to tell "waiting" from "forgotten" and with no way to
/// cancel. Polled rather than pushed: the state changes at most once a minute
/// and the chat socket carries no watch events.
class WatchStrip extends ConsumerStatefulWidget {
  const WatchStrip({super.key, required this.jid});
  final String jid;

  @override
  ConsumerState<WatchStrip> createState() => _WatchStripState();
}

class _WatchStripState extends ConsumerState<WatchStrip> {
  List<WatchInfo> _watches = const [];
  String? _stopping;
  Timer? _timer;

  @override
  void initState() {
    super.initState();
    _load();
    _timer = Timer.periodic(_refresh, (_) => _load());
  }

  @override
  void didUpdateWidget(covariant WatchStrip old) {
    super.didUpdateWidget(old);
    // Switching chats must not leave the previous one's watches on screen.
    if (old.jid != widget.jid) {
      setState(() => _watches = const []);
      _load();
    }
  }

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }

  Future<void> _load() async {
    try {
      final r = await ref.read(apiClientProvider).get(
        '/api/watches',
        query: {'chatJid': widget.jid},
      );
      final list = (r is Map ? r['watches'] : null) as List?;
      if (!mounted) return;
      setState(() => _watches = (list ?? const [])
          .whereType<Map>()
          .map((m) => WatchInfo.fromJson(m.cast<String, dynamic>()))
          .toList());
    } catch (_) {
      // A failed poll is not worth surfacing: the next one is 15s away and an
      // error strip above the composer over a transient blip is worse noise.
    }
  }

  Future<void> _stop(WatchInfo w) async {
    setState(() => _stopping = w.id);
    try {
      await ref.read(apiClientProvider).post('/api/watches/${w.id}/stop');
      if (!mounted) return;
      // Drop it now rather than waiting for the next poll — the user just
      // clicked Stop and needs to see that it took.
      setState(() => _watches = _watches.where((x) => x.id != w.id).toList());
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context)
          .showSnackBar(SnackBar(content: Text(e.toString())));
    } finally {
      if (mounted) setState(() => _stopping = null);
    }
  }

  @override
  Widget build(BuildContext context) {
    if (_watches.isEmpty) return const SizedBox.shrink();
    final c = context.colors;
    return Padding(
      padding: const EdgeInsets.fromLTRB(
          AppTokens.s16, 0, AppTokens.s16, AppTokens.s6),
      child: Column(
        children: [
          for (final w in _watches)
            Container(
              margin: const EdgeInsets.only(top: AppTokens.s6),
              padding: const EdgeInsets.symmetric(
                  horizontal: AppTokens.s8, vertical: 4),
              decoration: BoxDecoration(
                color: c.surfaceAlt,
                borderRadius: BorderRadius.circular(AppTokens.rSm),
                border: Border.all(color: c.border),
              ),
              child: Row(
                children: [
                  Icon(Icons.visibility_outlined, size: 14, color: c.accent),
                  const SizedBox(width: AppTokens.s8),
                  Expanded(
                    child: Text(
                      '${context.tr('Watching')} ${w.label ?? w.tool ?? context.tr('a background task')}'
                      ' · ${w.checks}/${w.maxChecks}'
                      '${w.intervalSecs > 0 ? ' · ${w.intervalSecs}s' : ''}',
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(color: c.textSecondary, fontSize: 12),
                    ),
                  ),
                  if (w.lastError != null)
                    Tooltip(
                      message: w.lastError!,
                      child: Icon(Icons.warning_amber_rounded,
                          size: 14, color: AppTokens.warning),
                    ),
                  TextButton(
                    onPressed:
                        _stopping == w.id ? null : () => _stop(w),
                    style: TextButton.styleFrom(
                      foregroundColor: AppTokens.danger,
                      padding:
                          const EdgeInsets.symmetric(horizontal: AppTokens.s8),
                      minimumSize: const Size(0, 28),
                      tapTargetSize: MaterialTapTargetSize.shrinkWrap,
                    ),
                    child: Text(context.tr('Stop'),
                        style: const TextStyle(fontSize: 12)),
                  ),
                ],
              ),
            ),
        ],
      ),
    );
  }
}
