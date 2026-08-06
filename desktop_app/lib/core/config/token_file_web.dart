/// Web build: no filesystem — the token comes from Settings (prefs) or the
/// session cookie set by the web login flow.
Future<String?> readLocalDaemonToken() async => null;
