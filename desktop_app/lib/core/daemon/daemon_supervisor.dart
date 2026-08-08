import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:path/path.dart' as p;

import 'port_tools.dart';

/// Lifecycle of the local daemon as seen by the desktop app.
enum DaemonPhase {
  idle, // not started yet
  starting, // spawned, waiting for the UI port
  running, // we spawned it and the port is up
  adopted, // an existing daemon was already listening — we attached to it
  crashed, // process exited / failed to start
  external, // web build: cannot spawn, assume a daemon exists elsewhere
}

/// Supervises the `senclaw` daemon as a CHILD PROCESS — the desktop analog of
/// the old Tauri shell, which embedded `run_daemon()` in-process. Flutter can't
/// host Rust in-process, so instead we locate the bundled binary, spawn it,
/// stream its logs, watch it, and restart on demand.
///
/// If a daemon is already listening on the UI port (e.g. started by `cargo run`
/// or a second window), we ADOPT it instead of spawning a conflicting one.
class DaemonSupervisor extends ChangeNotifier {
  DaemonSupervisor({
    this.host = '127.0.0.1',
    this.uiPort = 18788,
    this.wsPort = 18789,
    String bindHost = kPrivateBindHost,
    this.adoptProbeBudget = const Duration(seconds: 12),
  }) : _bindHost = bindHost;

  /// How long a port that is open but silent gets to start answering before we
  /// declare it unusable. Injectable so tests need not wait it out.
  final Duration adoptProbeBudget;

  /// Loopback: the daemon answers this machine only. The default, and the
  /// only setting that needs no authentication.
  static const String kPrivateBindHost = '127.0.0.1';

  /// Every interface: the daemon is reachable from the LAN. The daemon turns
  /// API-token auth on by itself for non-loopback peers (see auth.rs).
  static const String kPublicBindHost = '0.0.0.0';

  final String host;
  final int uiPort;
  final int wsPort;

  String _bindHost;
  String? _activeBindHost;

  /// What the daemon we are TALKING TO actually bound, as opposed to what the
  /// next spawn will use. `null` when no daemon of ours is up, and also for an
  /// adopted one — a daemon started outside the app took its bind host from
  /// its own environment and we cannot know it. Kept apart from [bindHost] so
  /// the settings panel can tell "already applied" from "needs a restart"; one
  /// field for both would always report "applied" the instant it was changed.
  String? get activeBindHost => _activeBindHost;

  /// Whether the running daemon is serving something other than the current
  /// choice — i.e. whether a restart is owed. False when nothing is running:
  /// there is nothing to restart, and the next start picks the choice up.
  bool get bindHostPending => isUp && _activeBindHost != _bindHost;

  /// What the spawned daemon binds to (`SENCLAW_UI_BIND_HOST`). Takes effect on
  /// the next [start]/[restart] — a listening socket cannot be re-bound.
  String get bindHost => _bindHost;
  set bindHost(String value) {
    final next = value.trim().isEmpty ? kPrivateBindHost : value.trim();
    if (next == _bindHost) return;
    _bindHost = next;
    notifyListeners();
  }

  bool get isPublicBind => _bindHost != kPrivateBindHost && _bindHost != 'localhost';

  static const int _logCap = 2000;

  Process? _proc;
  final List<String> _logs = <String>[];
  DaemonPhase _phase = DaemonPhase.idle;
  String? _lastError;
  DateTime? _startedAt;
  Timer? _logFlush;
  bool _logDirty = false;

  DaemonPhase get phase => _phase;
  String? get lastError => _lastError;
  DateTime? get startedAt => _startedAt;
  bool get isUp =>
      _phase == DaemonPhase.running ||
      _phase == DaemonPhase.adopted ||
      _phase == DaemonPhase.external;

  /// Newest-last snapshot of the captured log ring buffer.
  List<String> get logs => List.unmodifiable(_logs);

  /// Idempotent: safe to call once at startup. On web it's a no-op.
  Future<void> start() async {
    if (kIsWeb) {
      _setPhase(DaemonPhase.external);
      return;
    }
    if (_phase == DaemonPhase.starting || _phase == DaemonPhase.running) return;

    // Adopt an already-running daemon rather than fighting over the port —
    // but only one that ANSWERS. A TCP accept proves a process holds the port,
    // not that it serves the API: a daemon killed mid-update, a wedged one, or
    // an unrelated program on 18788 all pass the connect test and then never
    // reply, which used to strand the app on a blank white screen forever.
    if (await _portOpen(uiPort)) {
      if (await _httpAnswers(uiPort, budget: adoptProbeBudget)) {
        _log('[supervisor] daemon already listening on $uiPort — adopting');
        _activeBindHost = null; // its environment, not ours
        _setPhase(DaemonPhase.adopted);
        return;
      }
      _lastError = 'port $uiPort is held by a process that does not answer the '
          'SenClaw API — close it (Diagnostics → Kill port) and retry';
      _log('[supervisor] $_lastError');
      _setPhase(DaemonPhase.crashed);
      return;
    }

    final bin = await resolveBinary();
    if (bin == null) {
      _lastError =
          'senclaw binary not found (set SENCLAW_BIN or bundle it next to the app)';
      _log('[supervisor] $_lastError');
      _setPhase(DaemonPhase.crashed);
      return;
    }

    _lastError = null;
    _startedAt = DateTime.now();
    _setPhase(DaemonPhase.starting);
    _log('[supervisor] spawning ${bin.path} start');

    try {
      final env = <String, String>{
        ...Platform.environment,
        // The daemon shells out to itself for MCP stdio servers.
        'SENCLAW_BIN': bin.path,
        'SENCLAW_UI_PORT': '$uiPort',
        'SENCLAW_WS_PORT': '$wsPort',
        // Loopback unless the user opted into LAN access (Settings → General →
        // Network access). Deliberately NOT `SENCLAW_BIND_HOST`, which is the
        // Space Apps' knob — those authenticate nothing of their own.
        'SENCLAW_UI_BIND_HOST': _bindHost,
      };
      final proc = await Process.start(
        bin.path,
        ['start'],
        environment: env,
        workingDirectory: bin.parent.path,
      );
      _proc = proc;
      // Malformed-tolerant + never cancel: a strict utf8 decoder throws on any
      // non-UTF-8 byte, which kills the subscription and CLOSES the pipe — the
      // daemon's next eprintln! then panics with EPIPE (observed poisoning the
      // Whisper engine mutex). Keep draining no matter what arrives.
      proc.stdout
          .transform(const Utf8Decoder(allowMalformed: true))
          .transform(const LineSplitter())
          .listen(_log, onError: (Object _) {}, cancelOnError: false);
      proc.stderr
          .transform(const Utf8Decoder(allowMalformed: true))
          .transform(const LineSplitter())
          .listen(_log, onError: (Object _) {}, cancelOnError: false);
      unawaited(proc.exitCode.then(_onExit));
    } catch (e) {
      _lastError = 'failed to spawn daemon: $e';
      _log('[supervisor] $_lastError');
      _setPhase(DaemonPhase.crashed);
      return;
    }

    // Wait (up to ~60s) for the UI port to accept connections.
    final ok = await _waitForPort(uiPort, attempts: 300);
    if (ok) {
      _activeBindHost = _bindHost; // what this process was handed at spawn
      _setPhase(DaemonPhase.running);
      _log('[supervisor] daemon up on $uiPort');
    } else if (_phase != DaemonPhase.crashed) {
      _lastError = 'daemon did not open port $uiPort in time';
      _setPhase(DaemonPhase.crashed);
    }
  }

  Future<void> restart() async {
    _log('[supervisor] restart requested');
    await stop();
    // An adopted daemon is nobody's child — `stop()` has no process to signal,
    // so without this the "restart" would just re-adopt the same old daemon and
    // a changed bind host (or binary) would silently never take effect.
    if (await _portOpen(uiPort)) {
      final pid = await PortTools.killPort(uiPort);
      _log(pid == null
          ? '[supervisor] port $uiPort still held; could not identify the owner'
          : '[supervisor] killed the listener on $uiPort (pid $pid)');
    }
    await Future.delayed(const Duration(milliseconds: 400));
    _phase = DaemonPhase.idle;
    await start();
  }

  Future<void> stop() async {
    final proc = _proc;
    _proc = null;
    if (proc == null) return;
    proc.kill(ProcessSignal.sigterm);
    // Escalate if it lingers.
    await Future.any([
      proc.exitCode,
      Future.delayed(const Duration(milliseconds: 1500)),
    ]);
    proc.kill(ProcessSignal.sigkill);
  }

  void _onExit(int code) {
    if (_proc == null) return; // intentional stop
    _lastError = 'daemon exited (code $code)';
    _log('[supervisor] $_lastError');
    _setPhase(DaemonPhase.crashed);
  }

  // ── Binary resolution ──────────────────────────────────────────────────
  /// Search order: env override → next to the app executable / inside the
  /// macOS app bundle → dev tree (target/release, src-tauri/binaries).
  ///
  /// Public because the updater needs the same binary: it copies it out of the
  /// bundle and runs `apply-update` from there. Two answers to "where is
  /// senclaw" would be one too many.
  Future<File?> resolveBinary() async {
    final name = Platform.isWindows ? 'senclaw.exe' : 'senclaw';

    final fromEnv = Platform.environment['SENCLAW_BIN'];
    if (fromEnv != null && File(fromEnv).existsSync()) return File(fromEnv);

    final exeDir = p.dirname(Platform.resolvedExecutable);
    final candidates = <String>[
      p.join(exeDir, name), // alongside the app binary
      p.join(exeDir, 'binaries', name),
      // macOS .app bundle: Contents/MacOS/<app> → Contents/Resources/<name>
      p.normalize(p.join(exeDir, '..', 'Resources', name)),
      p.normalize(p.join(exeDir, '..', 'Resources', 'binaries', name)),
    ];

    // Dev convenience: walk up looking for a Cargo workspace, then check
    // target/{release,debug} and the legacy src-tauri/binaries copy.
    Directory dir = Directory(exeDir);
    for (var i = 0; i < 8; i++) {
      if (File(p.join(dir.path, 'Cargo.toml')).existsSync()) {
        candidates.addAll([
          p.join(dir.path, 'target', 'release', name),
          p.join(dir.path, 'target', 'debug', name),
          p.join(dir.path, 'src-tauri', 'binaries', name),
        ]);
        break;
      }
      final parent = dir.parent;
      if (parent.path == dir.path) break;
      dir = parent;
    }

    for (final c in candidates) {
      if (File(c).existsSync()) return File(c);
    }
    return null;
  }

  // ── Port helpers ───────────────────────────────────────────────────────
  Future<bool> _portOpen(int port) async {
    try {
      final s = await Socket.connect(host, port,
          timeout: const Duration(milliseconds: 300));
      s.destroy();
      return true;
    } catch (_) {
      return false;
    }
  }

  /// Does whatever holds [port] actually speak the SenClaw API?
  ///
  /// ANY HTTP status counts — 401 from a token-gated daemon or 404 from an
  /// older route set still proves a live server on the other end. Only silence
  /// (or a non-HTTP peer) is a failure. Uses `dart:io` directly rather than the
  /// app's [ApiClient] so the probe carries no auth headers and cannot be
  /// affected by a stale config.
  Future<bool> _httpAnswers(int port,
      {Duration budget = const Duration(seconds: 12)}) async {
    final deadline = DateTime.now().add(budget);
    HttpClient? client;
    try {
      client = HttpClient()..connectionTimeout = const Duration(seconds: 2);
      while (DateTime.now().isBefore(deadline)) {
        try {
          final req = await client
              .getUrl(Uri.parse('http://$host:$port/api/config'))
              .timeout(const Duration(seconds: 3));
          final res = await req.close().timeout(const Duration(seconds: 3));
          await res.drain<void>().timeout(const Duration(seconds: 3));
          return true;
        } catch (e) {
          _log('[supervisor] health probe on $port: $e');
          await Future.delayed(const Duration(milliseconds: 600));
        }
      }
      return false;
    } finally {
      client?.close(force: true);
    }
  }

  Future<bool> _waitForPort(int port, {int attempts = 300}) async {
    for (var i = 0; i < attempts; i++) {
      if (_phase == DaemonPhase.crashed) return false;
      if (await _portOpen(port)) return true;
      await Future.delayed(const Duration(milliseconds: 200));
    }
    return false;
  }

  // ── Logging (coalesced to avoid rebuild storms) ────────────────────────
  static final _ansi = RegExp(r'\x1B\[[0-9;?]*[a-zA-Z]');

  void _log(String rawLine) {
    // The daemon logs with ANSI colors; strip them so the Diagnostics screen
    // and startup splash show clean text instead of `[2m...[0m` garbage.
    final line = rawLine.replaceAll(_ansi, '');
    if (line.isEmpty) return;
    _logs.add(line);
    while (_logs.length > _logCap) {
      _logs.removeAt(0);
    }
    _logDirty = true;
    _logFlush ??= Timer(const Duration(milliseconds: 250), () {
      _logFlush = null;
      if (_logDirty) {
        _logDirty = false;
        notifyListeners();
      }
    });
  }

  void _setPhase(DaemonPhase phase) {
    _phase = phase;
    notifyListeners();
  }

  @override
  void dispose() {
    _logFlush?.cancel();
    // Best-effort: don't orphan the daemon when the app quits.
    _proc?.kill(ProcessSignal.sigterm);
    super.dispose();
  }
}
