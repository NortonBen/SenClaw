import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:path/path.dart' as p;

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
  DaemonSupervisor({this.host = '127.0.0.1', this.uiPort = 18788, this.wsPort = 18789});

  final String host;
  final int uiPort;
  final int wsPort;

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

    // Adopt an already-running daemon rather than fighting over the port.
    if (await _portOpen(uiPort)) {
      _log('[supervisor] daemon already listening on $uiPort — adopting');
      _setPhase(DaemonPhase.adopted);
      return;
    }

    final bin = await _resolveBinary();
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
  Future<File?> _resolveBinary() async {
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
