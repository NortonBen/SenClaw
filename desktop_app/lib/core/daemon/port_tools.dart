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
    final pid = await _lsofPid(port);
    if (pid == null) return null;
    await Process.run('kill', ['-TERM', '$pid']);
    await Future.delayed(const Duration(milliseconds: 800));
    if (await _lsofPid(port) != null) {
      await Process.run('kill', ['-KILL', '$pid']);
    }
    return pid;
  }
}
