import 'dart:io';
import 'package:flutter/foundation.dart';

/// Listening-socket status for a port (desktop only; uses `lsof`).
class PortStatus {
  final int port;
  final int? pid;
  final String? process;
  final bool free;
  final bool isSelf;
  const PortStatus(this.port, this.pid, this.process, this.free, this.isSelf);
}

/// Port diagnostics, mirroring the old Tauri `port_status`/`kill_port` commands.
/// macOS/Linux only (lsof + ps + kill); no-ops on web/windows.
class PortTools {
  static Future<PortStatus> status(int port) async {
    if (kIsWeb || Platform.isWindows) {
      return PortStatus(port, null, null, true, false);
    }
    final pid = await _lsofPid(port);
    String? name;
    if (pid != null) name = await _procName(pid);
    return PortStatus(port, pid, name, pid == null, pid == pid0());
  }

  static int pid0() => 0; // the app's own pid is irrelevant for a separate daemon

  static Future<int?> _lsofPid(int port) async {
    try {
      final r = await Process.run(
        'lsof',
        ['-nP', '-iTCP:$port', '-sTCP:LISTEN', '-Fp'],
      );
      for (final line in '${r.stdout}'.split('\n')) {
        if (line.startsWith('p')) {
          return int.tryParse(line.substring(1).trim());
        }
      }
    } catch (_) {}
    return null;
  }

  static Future<String?> _procName(int pid) async {
    try {
      final r = await Process.run('ps', ['-o', 'comm=', '-p', '$pid']);
      final s = '${r.stdout}'.trim();
      if (s.isEmpty) return null;
      return s.split('/').last;
    } catch (_) {
      return null;
    }
  }

  /// SIGTERM then SIGKILL the listener on [port]. Returns the killed pid.
  static Future<int?> killPort(int port) async {
    if (kIsWeb) return null;
    if (Platform.isWindows) return _killPortWindows(port);
    final pid = await _lsofPid(port);
    if (pid == null) return null;
    await Process.run('kill', ['-TERM', '$pid']);
    await Future.delayed(const Duration(milliseconds: 800));
    if (await _lsofPid(port) != null) {
      await Process.run('kill', ['-KILL', '$pid']);
    }
    return pid;
  }

  // ── Windows ────────────────────────────────────────────────────────────
  /// Windows has no `lsof`; `netstat -ano` is the equivalent that ships with
  /// every install. Without this, Quit and "restart daemon" were silent no-ops
  /// there — a daemon left over from a previous run (or from an update that
  /// swapped the binary underneath it) kept the port and the new app adopted
  /// the OLD daemon.
  static Future<int?> _killPortWindows(int port) async {
    final pid = await _netstatPid(port);
    if (pid == null) return null;
    // /T takes the process tree with it: the daemon's MCP stdio servers and
    // Space Apps are children, and Windows does not reap them on its own.
    await Process.run('taskkill', ['/PID', '$pid', '/T', '/F']);
    return pid;
  }

  static Future<int?> _netstatPid(int port) async {
    try {
      final r = await Process.run('netstat', ['-ano', '-p', 'tcp']);
      return parseNetstatPid('${r.stdout}', port);
    } catch (_) {
      return null;
    }
  }

  /// Pull the LISTENING owner of [port] out of `netstat -ano` output.
  ///
  /// Only LISTENING rows count: an ESTABLISHED row's *remote* column can carry
  /// the same port number, and killing that pid would take down an innocent
  /// client — the same trap as `lsof` without `-sTCP:LISTEN`.
  @visibleForTesting
  static int? parseNetstatPid(String output, int port) {
    for (final raw in output.split('\n')) {
      final line = raw.trim();
      final cols = line.split(RegExp(r'\s+'));
      // Proto, Local, Foreign, State, PID — localized Windows builds translate
      // the header but not the LISTENING state.
      if (cols.length < 5 || cols[3].toUpperCase() != 'LISTENING') continue;
      // cols[1] is the local address: `127.0.0.1:18788` or `[::]:18788`.
      final local = cols[1];
      final colon = local.lastIndexOf(':');
      if (colon < 0) continue;
      if (int.tryParse(local.substring(colon + 1)) != port) continue;
      return int.tryParse(cols[4]);
    }
    return null;
  }
}
