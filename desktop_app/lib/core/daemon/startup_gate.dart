import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../i18n/l10n.dart';
import '../transport/connection.dart';
import 'daemon_provider.dart';
import 'daemon_supervisor.dart';
import 'port_tools.dart';

/// Completes once the daemon is VERIFIED reachable over HTTP — not merely
/// spawned. `connectionBootstrapProvider` starts/adopts the daemon and waits
/// for its TCP port; this provider then keeps probing `/api/config` until the
/// HTTP stack actually answers. Screens are only built after this resolves
/// (see [StartupGate]), so no feature provider ever fires a request into a
/// dead or half-started daemon.
final daemonReadyProvider = FutureProvider<void>((ref) async {
  await ref.watch(connectionBootstrapProvider.future);

  final sup = ref.read(daemonSupervisorProvider);
  final api = ref.read(apiClientProvider);

  // Probe on a WALL-CLOCK deadline, not a fixed attempt count: each attempt now
  // carries its own timeout, so counting attempts no longer bounds the wait.
  // Whatever happens, this future settles — the gate must never be left
  // awaiting something that cannot complete.
  final deadline = DateTime.now().add(const Duration(seconds: 45));
  Object? lastErr;
  while (DateTime.now().isBefore(deadline)) {
    if (sup.phase == DaemonPhase.crashed) {
      throw StateError(sup.lastError ?? 'daemon crashed while starting');
    }
    try {
      await api.get('/api/config', timeout: const Duration(seconds: 5));
      return;
    } catch (e) {
      lastErr = e;
      await Future.delayed(const Duration(milliseconds: 500));
    }
  }
  throw StateError(
      'the daemon did not answer /api/config within 45s (state: ${sup.phase.name})'
      '${lastErr == null ? '' : ' — last error: $lastErr'}');
});

/// Startup gate for the whole app shell.
///
/// Two entry paths, matching what the user should see:
/// - Daemon ALREADY running (adopt path — the port answers immediately):
///   readiness resolves in a few hundred ms, so we render only a bare surface
///   in the meantime and the main UI opens straight away — no splash flash.
/// - Daemon NOT running (we spawn it): a dedicated "Starting daemon" screen
///   with live status, then a brief success confirmation, and only then the
///   main UI. If the daemon never comes up, a retryable error screen with the
///   daemon log tail.
class StartupGate extends ConsumerStatefulWidget {
  const StartupGate({super.key, required this.child});
  final Widget child;

  @override
  ConsumerState<StartupGate> createState() => _StartupGateState();
}

class _StartupGateState extends ConsumerState<StartupGate> {
  /// True once we've shown the "starting daemon" screen — gates the success
  /// confirmation so the adopt path (daemon already up) skips it entirely.
  bool _sawStarting = false;
  bool _successDone = false;
  Timer? _successTimer;

  @override
  void dispose() {
    _successTimer?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final ready = ref.watch(daemonReadyProvider);
    final sup = ref.watch(daemonSupervisorProvider);

    return ready.when(
      data: (_) {
        if (_sawStarting && !_successDone) {
          // We booted the daemon ourselves — confirm success briefly before
          // switching to the main UI.
          _successTimer ??= Timer(const Duration(milliseconds: 900), () {
            if (mounted) setState(() => _successDone = true);
          });
          return const _DaemonStartedSplash();
        }
        return widget.child;
      },
      loading: () {
        // Re-entering loading (e.g. after a retry) → re-arm the success flash.
        _successTimer?.cancel();
        _successTimer = null;
        _successDone = false;

        final spawning = sup.phase == DaemonPhase.starting ||
            sup.phase == DaemonPhase.crashed;
        if (spawning) _sawStarting = true;

        if (!_sawStarting) {
          // Adopt path probe (daemon likely already up) — normally resolves in
          // well under a second, so stay visually empty at first and let the
          // main UI appear to open immediately. But an empty frame with no time
          // limit is precisely the "white screen" bug: past [_kQuietProbe] the
          // probe is no longer quick, and the user gets told what we're waiting
          // for instead of staring at a blank window.
          return _ConnectingSplash(sup: sup);
        }
        return _DaemonStartingSplash(sup: sup);
      },
      error: (e, _) => _StartupError(
        message: '$e',
        onRetry: () {
          // start() is safe to re-enter after a crash: it re-adopts a live
          // port or re-spawns the binary. Invalidate the chain so both the
          // bootstrap and the readiness probe run again.
          ref.invalidate(connectionBootstrapProvider);
          ref.invalidate(daemonReadyProvider);
        },
      ),
    );
  }
}

/// How long the adopt path may stay blank before it owes the user an
/// explanation. Long enough that a healthy adopt (a few hundred ms) never
/// flashes a splash, short enough that nobody calls it a frozen window.
const Duration _kQuietProbe = Duration(milliseconds: 1200);

/// The adopt path: blank while the probe is quick, then a splash that says what
/// is being waited on. Never an unexplained empty window.
class _ConnectingSplash extends StatefulWidget {
  const _ConnectingSplash({required this.sup});
  final DaemonSupervisor sup;

  @override
  State<_ConnectingSplash> createState() => _ConnectingSplashState();
}

class _ConnectingSplashState extends State<_ConnectingSplash> {
  bool _slow = false;
  Timer? _t;

  @override
  void initState() {
    super.initState();
    _t = Timer(_kQuietProbe, () {
      if (mounted) setState(() => _slow = true);
    });
  }

  @override
  void dispose() {
    _t?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    if (!_slow) {
      return Scaffold(
        backgroundColor: Theme.of(context).colorScheme.surface,
        body: const SizedBox.expand(),
      );
    }
    return _DaemonStartingSplash(
      sup: widget.sup,
      label: context.tr('Connecting to the SenClaw daemon…'),
    );
  }
}

/// Secondary screen shown only when the app has to boot the daemon itself.
class _DaemonStartingSplash extends StatelessWidget {
  const _DaemonStartingSplash({required this.sup, this.label});
  final DaemonSupervisor sup;

  /// Overrides the "Starting…" caption — the adopt path is connecting to a
  /// daemon it did not start, and saying "starting" there would be a lie.
  final String? label;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final lastLog = sup.logs.isEmpty ? null : sup.logs.last;
    return Scaffold(
      backgroundColor: scheme.surface,
      body: Center(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            ClipRRect(
              borderRadius: BorderRadius.circular(20),
              child: Image.asset(
                'assets/branding/senclaw_icon_1024.png',
                width: 88,
                height: 88,
              ),
            ),
            const SizedBox(height: 28),
            const SizedBox(
              width: 22,
              height: 22,
              child: CircularProgressIndicator(strokeWidth: 2.4),
            ),
            const SizedBox(height: 16),
            Text(label ?? context.tr('Starting SenClaw daemon…'),
                style: Theme.of(context).textTheme.bodyMedium),
            if (lastLog != null) ...[
              const SizedBox(height: 10),
              ConstrainedBox(
                constraints: const BoxConstraints(maxWidth: 520),
                child: Text(
                  lastLog,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  textAlign: TextAlign.center,
                  style: TextStyle(
                    fontSize: 11,
                    fontFamily: 'monospace',
                    color: scheme.onSurfaceVariant,
                  ),
                ),
              ),
            ],
          ],
        ),
      ),
    );
  }
}

/// Brief confirmation after a spawn succeeds, before entering the main UI.
class _DaemonStartedSplash extends StatelessWidget {
  const _DaemonStartedSplash();

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Scaffold(
      backgroundColor: scheme.surface,
      body: Center(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(Icons.check_circle_rounded,
                size: 52, color: Colors.green.shade500),
            const SizedBox(height: 16),
            Text(context.tr('Daemon started'),
                style: Theme.of(context).textTheme.titleMedium),
          ],
        ),
      ),
    );
  }
}

class _StartupError extends ConsumerWidget {
  const _StartupError({required this.message, required this.onRetry});
  final String message;
  final VoidCallback onRetry;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final sup = ref.watch(daemonSupervisorProvider);
    final logs = sup.logs;
    final tail = logs.length <= 20 ? logs : logs.sublist(logs.length - 20);
    final scheme = Theme.of(context).colorScheme;
    return Scaffold(
      backgroundColor: scheme.surface,
      body: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 560),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.center,
            children: [
              Icon(Icons.error_outline_rounded,
                  size: 44, color: scheme.error),
              const SizedBox(height: 16),
              Text(context.tr('Cannot reach the SenClaw daemon'),
                  style: Theme.of(context).textTheme.titleMedium),
              const SizedBox(height: 8),
              Text(
                message,
                textAlign: TextAlign.center,
                style: Theme.of(context)
                    .textTheme
                    .bodySmall
                    ?.copyWith(color: scheme.onSurfaceVariant),
              ),
              if (tail.isNotEmpty) ...[
                const SizedBox(height: 16),
                Container(
                  height: 160,
                  width: double.infinity,
                  padding: const EdgeInsets.all(10),
                  decoration: BoxDecoration(
                    color: scheme.surfaceContainerHighest.withValues(alpha: .5),
                    borderRadius: BorderRadius.circular(10),
                  ),
                  child: SingleChildScrollView(
                    reverse: true,
                    child: SelectableText(
                      tail.join('\n'),
                      style: const TextStyle(
                          fontSize: 11, fontFamily: 'monospace'),
                    ),
                  ),
                ),
              ],
              const SizedBox(height: 20),
              Row(
                mainAxisAlignment: MainAxisAlignment.center,
                children: [
                  // The common cause is a leftover daemon holding the port
                  // without serving it — freeing the port IS the fix, so put it
                  // one click away instead of in a docs sentence.
                  OutlinedButton.icon(
                    onPressed: () async {
                      await PortTools.killPort(
                          ref.read(appConfigProvider).uiPort);
                      onRetry();
                    },
                    icon: const Icon(Icons.power_settings_new_rounded, size: 18),
                    label: Text(context.tr('Free the port and retry')),
                  ),
                  const SizedBox(width: 12),
                  FilledButton.icon(
                    onPressed: onRetry,
                    icon: const Icon(Icons.refresh_rounded, size: 18),
                    label: Text(context.tr('Retry')),
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
