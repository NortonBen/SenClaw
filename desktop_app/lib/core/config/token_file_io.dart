import 'dart:io';

/// Read `~/.senclaw/api_token` — the token the daemon on this machine
/// auto-generated. Lets the desktop app talk to a LAN-exposed local daemon
/// with zero configuration. Null when absent/unreadable.
Future<String?> readLocalDaemonToken() async {
  try {
    final home = Platform.environment['HOME'] ??
        Platform.environment['USERPROFILE'];
    if (home == null || home.isEmpty) return null;
    final file = File('$home/.senclaw/api_token');
    if (!await file.exists()) return null;
    final token = (await file.readAsString()).trim();
    return token.isEmpty ? null : token;
  } catch (_) {
    return null;
  }
}
