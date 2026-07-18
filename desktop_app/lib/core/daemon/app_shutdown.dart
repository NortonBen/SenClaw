import 'dart:io';

import 'package:desktop_multi_window/desktop_multi_window.dart';
import 'package:window_manager/window_manager.dart';

import 'daemon_supervisor.dart';
import 'port_tools.dart';

/// Full shutdown: close every open window (main + mini-chat sub-windows), stop
/// the SenClaw daemon (the process we spawned AND, for an adopted one, whatever
/// still listens on the UI port), then terminate the app.
///
/// Shared by the tray's Quit item and the updater — the updater needs exactly
/// this teardown, because `apply-update` is blocked on our pid and the bundle
/// it replaces contains the daemon we are running. Killing by port matters
/// there too: a daemon started outside the app (`cargo run`) would otherwise
/// survive the swap and keep serving the OLD binary to the new app.
///
/// Takes its collaborators rather than a Ref: the two callers hold different
/// Riverpod ref types (WidgetRef in the app widget, Ref in a provider).
Future<void> shutdownApp({
  required DaemonSupervisor supervisor,
  required int uiPort,
}) async {
  try {
    for (final id in await DesktopMultiWindow.getAllSubWindowIds()) {
      await WindowController.fromWindowId(id).close();
    }
  } catch (_) {}
  try {
    await supervisor.stop();
  } catch (_) {}
  try {
    await PortTools.killPort(uiPort);
  } catch (_) {}
  try {
    await windowManager.setPreventClose(false);
    await windowManager.destroy();
  } catch (_) {}
  exit(0);
}
