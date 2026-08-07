import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/transport/api_client.dart';
import '../../core/transport/connection.dart';
import '../../core/i18n/l10n.dart';
import '../../theme/tokens.dart';

/// Show an app's page only once the app is actually answering.
///
/// A server Space App is its own HTTP process on its own port, and the desktop
/// points a web view straight at that origin. Nothing about that is wrong — a
/// direct connection is the only way an app's own WebSockets work — but it
/// means the web view has no idea whether anything is listening. Point it at a
/// stopped app and you get a white rectangle: no error, no hint, no reason.
///
/// That got common rather than rare when apps gained a `session` mode, because
/// **stopped is now the resting state** for most of them. So the open path is:
///
///   POST /start  → daemon spawns and waits for health, or fails with the log
///   GET  /ready  → cheap re-probe (the app may have been reaped since)
///   → only then mount the child
///
/// The daemon's `/start` already blocks until the health check passes, so this
/// widget is a state machine around one request, not a polling loop. It polls
/// only in the narrow case where `/start` succeeded but the app answered slower
/// than the daemon's own gate.
class AppHealthGate extends ConsumerStatefulWidget {
  const AppHealthGate({
    super.key,
    required this.appId,
    required this.appName,
    this.appIcon = '',
    required this.builder,
  });

  final String appId;
  final String appName;

  /// The app's own emoji from its manifest. Shown while starting, so the wait
  /// looks like *this* app opening rather than a generic spinner.
  final String appIcon;

  /// Built once the app answers. Not called before that, so the web view is
  /// never pointed at a dead port.
  final Widget Function(BuildContext context) builder;

  @override
  ConsumerState<AppHealthGate> createState() => _AppHealthGateState();
}

enum _Phase { checking, starting, ready, failed }

class _AppHealthGateState extends ConsumerState<AppHealthGate> {
  _Phase _phase = _Phase.checking;
  String _error = '';
  int _attempt = 0;

  @override
  void initState() {
    super.initState();
    _open();
  }

  @override
  void didUpdateWidget(AppHealthGate old) {
    super.didUpdateWidget(old);
    // A different app in the same slot must re-run the gate, or it would show
    // the previous app's readiness.
    if (old.appId != widget.appId) _open();
  }

  Future<void> _open() async {
    if (!mounted) return;
    setState(() {
      _phase = _Phase.checking;
      _error = '';
    });
    final api = ref.read(apiClientProvider);
    final id = Uri.encodeComponent(widget.appId);

    try {
      // Already up? Then don't touch it — a running app should not be
      // restarted just because someone opened its window again.
      final probe = await api.get('/api/space/apps/$id/ready');
      if (probe is Map && probe['ready'] == true) {
        if (mounted) setState(() => _phase = _Phase.ready);
        return;
      }
    } catch (_) {
      // A failed probe is not an answer; fall through and try to start.
    }

    if (!mounted) return;
    setState(() => _phase = _Phase.starting);

    try {
      final res = await api.post('/api/space/apps/$id/start');
      if (res is Map && res['ready'] == false) {
        // Started, but not answering yet. The daemon waited already, so this is
        // a slow one — give it a little longer rather than declaring failure.
        if (await _waitReady(api, id)) {
          if (mounted) setState(() => _phase = _Phase.ready);
          return;
        }
        throw StateError(
            'App started but never answered its health check');
      }
      if (mounted) setState(() => _phase = _Phase.ready);
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _phase = _Phase.failed;
        _error = '$e';
      });
    }
  }

  Future<bool> _waitReady(ApiClient api, String id) async {
    for (var i = 0; i < 20; i++) {
      await Future<void>.delayed(const Duration(milliseconds: 500));
      if (!mounted) return false;
      try {
        final r = await api.get('/api/space/apps/$id/ready');
        if (r is Map && r['ready'] == true) return true;
      } catch (_) {
        // keep waiting
      }
    }
    return false;
  }

  @override
  Widget build(BuildContext context) {
    if (_phase == _Phase.ready) {
      // Keyed on the attempt so a retry rebuilds the web view rather than
      // reusing one that already failed to load.
      return KeyedSubtree(
        key: ValueKey('${widget.appId}#$_attempt'),
        child: widget.builder(context),
      );
    }
    if (_phase == _Phase.failed) return _failed(context);
    return _waiting(context);
  }

  Widget _waiting(BuildContext context) {
    final c = context.colors;
    return Container(
      color: c.bg,
      child: Center(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            _StartingBadge(icon: widget.appIcon, accent: c.accent),
            const SizedBox(height: AppTokens.s20),
            Text(
              _phase == _Phase.starting
                  ? context.tr('Starting {app}…').replaceFirst('{app}', widget.appName)
                  : context.tr('Checking {app}…').replaceFirst('{app}', widget.appName),
              style: TextStyle(
                  color: c.textPrimary,
                  fontWeight: FontWeight.w600,
                  fontSize: 14),
            ),
            const SizedBox(height: AppTokens.s8),
            SizedBox(
              width: 180,
              child: ClipRRect(
                borderRadius: BorderRadius.circular(AppTokens.rFull),
                child: LinearProgressIndicator(
                  minHeight: 3,
                  backgroundColor: c.border,
                  valueColor: AlwaysStoppedAnimation<Color>(c.accent),
                ),
              ),
            ),
            const SizedBox(height: AppTokens.s12),
            Text(
              context.tr('Waiting for the app to answer its health check.'),
              style: TextStyle(color: c.textMuted, fontSize: 11.5),
            ),
          ],
        ),
      ),
    );
  }

  Widget _failed(BuildContext context) {
    final c = context.colors;
    return Container(
      color: c.bg,
      padding: const EdgeInsets.all(AppTokens.s24),
      child: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 620),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  const Icon(Icons.error_outline,
                      color: AppTokens.danger, size: 20),
                  const SizedBox(width: AppTokens.s8),
                  Expanded(
                    child: Text(
                      context
                          .tr('{app} did not start')
                          .replaceFirst('{app}', widget.appName),
                      style: TextStyle(
                          color: c.textPrimary, fontWeight: FontWeight.w700),
                    ),
                  ),
                ],
              ),
              const SizedBox(height: AppTokens.s12),
              // The daemon appends the tail of the app's own log to the error,
              // which is nearly always the actual answer (missing binary, port
              // in use, a stack trace on boot).
              Container(
                width: double.infinity,
                constraints: const BoxConstraints(maxHeight: 260),
                padding: const EdgeInsets.all(AppTokens.s12),
                decoration: BoxDecoration(
                  color: c.surfaceAlt,
                  border: Border.all(color: c.border),
                  borderRadius: BorderRadius.circular(AppTokens.rLg),
                ),
                child: SingleChildScrollView(
                  child: SelectableText(
                    _error,
                    style: TextStyle(
                      color: c.textSecondary,
                      fontSize: 11.5,
                      fontFamily: 'monospace',
                    ),
                  ),
                ),
              ),
              const SizedBox(height: AppTokens.s16),
              Row(
                children: [
                  FilledButton.icon(
                    onPressed: () {
                      _attempt++;
                      _open();
                    },
                    icon: const Icon(Icons.refresh, size: 16),
                    label: Text(context.tr('Try again')),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// The app's own icon, breathing inside a ring that sweeps while it starts.
///
/// A bare spinner says "something is loading"; this says "*this* app is
/// opening", which is the difference between a wait that feels like progress
/// and one that feels like a hang. Both animations are cheap and infinite —
/// there is no real progress to report, so pretending otherwise with a
/// percentage would be a lie that stalls at 90%.
class _StartingBadge extends StatefulWidget {
  const _StartingBadge({required this.icon, required this.accent});
  final String icon;
  final Color accent;

  @override
  State<_StartingBadge> createState() => _StartingBadgeState();
}

class _StartingBadgeState extends State<_StartingBadge>
    with TickerProviderStateMixin {
  late final AnimationController _spin = AnimationController(
    vsync: this,
    duration: const Duration(milliseconds: 1400),
  )..repeat();
  late final AnimationController _breathe = AnimationController(
    vsync: this,
    duration: const Duration(milliseconds: 1600),
  )..repeat(reverse: true);

  @override
  void dispose() {
    _spin.dispose();
    _breathe.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final icon = widget.icon.trim();
    return SizedBox(
      width: 76,
      height: 76,
      child: Stack(
        alignment: Alignment.center,
        children: [
          SizedBox(
            width: 76,
            height: 76,
            child: RotationTransition(
              turns: _spin,
              child: CircularProgressIndicator(
                strokeWidth: 2,
                value: 0.18, // a sweeping arc, not a full ring
                valueColor: AlwaysStoppedAnimation<Color>(widget.accent),
                backgroundColor: Colors.transparent,
              ),
            ),
          ),
          ScaleTransition(
            scale: Tween<double>(begin: 0.92, end: 1.06).animate(
              CurvedAnimation(parent: _breathe, curve: Curves.easeInOut),
            ),
            child: icon.isEmpty
                ? Icon(Icons.apps_rounded, size: 30, color: widget.accent)
                : Text(icon, style: const TextStyle(fontSize: 30)),
          ),
        ],
      ),
    );
  }
}
