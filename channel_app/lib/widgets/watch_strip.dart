import 'dart:async';

import 'package:flutter/material.dart';

import '../services/api_client.dart';
import '../theme/tokens.dart';

/// One in-flight watch, as `/api/watches` reports it.
class WatchInfo {
  final String id;
  final String? label;
  final String? tool;
  final int checks;
  final int maxChecks;
  final int intervalSecs;
  final String? lastError;

  const WatchInfo({
    required this.id,
    this.label,
    this.tool,
    this.checks = 0,
    this.maxChecks = 0,
    this.intervalSecs = 0,
    this.lastError,
  });

  factory WatchInfo.fromJson(Map<String, dynamic> j) => WatchInfo(
        id: '${j['id'] ?? ''}',
        label: j['label'] as String?,
        tool: j['tool'] as String?,
        checks: (j['checks'] as num?)?.toInt() ?? 0,
        maxChecks: (j['maxChecks'] as num?)?.toInt() ?? 0,
        intervalSecs: (j['intervalSecs'] as num?)?.toInt() ?? 0,
        lastError: j['lastError'] as String?,
      );
}

/// Poll cadence. A watch checks at most once a minute, so this is plenty.
const _refresh = Duration(seconds: 15);

/// What this chat is waiting on, and the button that stops it.
///
/// A watch runs silently — correct, since each check costs no tokens, but it
/// left no way to tell "waiting" from "forgotten" and no way to cancel. Polled
/// rather than pushed: a relay client cannot receive admin WebSocket events at
/// all, which is the same reason the dispatch screen polls its snapshot.
class WatchStrip extends StatefulWidget {
  const WatchStrip({super.key, required this.jid});
  final String jid;

  @override
  State<WatchStrip> createState() => _WatchStripState();
}

class _WatchStripState extends State<WatchStrip> {
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
    // Switching sessions must not leave the previous chat's watches on screen.
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
    if (widget.jid.isEmpty) return;
    try {
      final m = await ApiClient()
          .getObject('/api/watches?chatJid=${Uri.encodeComponent(widget.jid)}');
      final list = m['watches'] as List?;
      if (!mounted) return;
      setState(() => _watches = (list ?? const [])
          .whereType<Map>()
          .map((e) => WatchInfo.fromJson(e.cast<String, dynamic>()))
          .toList());
    } catch (_) {
      // A failed poll is not worth surfacing: the next one is 15s away, and an
      // error strip above the composer over a transient blip is worse noise.
    }
  }

  Future<void> _stop(WatchInfo w) async {
    setState(() => _stopping = w.id);
    try {
      await ApiClient().post('/api/watches/${w.id}/stop');
      if (!mounted) return;
      // Drop it now rather than waiting for the next poll — the user just
      // tapped Stop and needs to see that it took.
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
      padding: const EdgeInsets.fromLTRB(12, 0, 12, 6),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          for (final w in _watches)
            Container(
              margin: const EdgeInsets.only(top: 6),
              padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 4),
              decoration: BoxDecoration(
                color: c.surfaceAlt,
                borderRadius: BorderRadius.circular(8),
                border: Border.all(color: c.border),
              ),
              child: Row(
                children: [
                  Icon(Icons.visibility_outlined, size: 14, color: c.accent),
                  const SizedBox(width: 8),
                  Expanded(
                    child: Text(
                      'Đang theo dõi ${w.label ?? w.tool ?? "tác vụ nền"}'
                      ' · ${w.checks}/${w.maxChecks}'
                      '${w.intervalSecs > 0 ? " · ${w.intervalSecs}s" : ""}',
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(color: c.textMuted, fontSize: 11.5),
                    ),
                  ),
                  if (w.lastError != null)
                    Icon(Icons.warning_amber_rounded,
                        size: 14, color: AppTokens.warning),
                  TextButton(
                    onPressed: _stopping == w.id ? null : () => _stop(w),
                    style: TextButton.styleFrom(
                      foregroundColor: AppTokens.danger,
                      padding: const EdgeInsets.symmetric(horizontal: 6),
                      minimumSize: const Size(0, 28),
                      tapTargetSize: MaterialTapTargetSize.shrinkWrap,
                    ),
                    child: const Text('Dừng', style: TextStyle(fontSize: 11.5)),
                  ),
                ],
              ),
            ),
        ],
      ),
    );
  }
}
