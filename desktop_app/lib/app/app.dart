import 'dart:convert';
import 'dart:io';

import 'package:desktop_multi_window/desktop_multi_window.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:tray_manager/tray_manager.dart';
import 'package:window_manager/window_manager.dart';
import '../core/daemon/daemon_provider.dart';
import '../core/daemon/port_tools.dart';
import '../core/notifications/system_notifier.dart';
import '../core/transport/connection.dart';
import '../features/chat/mini_chat_screen.dart' show miniExpandRequestProvider;
import '../features/chat/widgets/plan_exit_dialog.dart';
import '../theme/app_theme.dart';
import '../theme/theme_mode_provider.dart';
import 'router.dart';

/// Root widget. Kicks off connection bootstrap (daemon spawn/adopt → config
/// discovery → WS connect) once, installs the system tray + hide-on-close
/// window behavior (the desktop responsibilities the old Tauri shell owned),
/// then renders the routed shell.
class SenClawApp extends ConsumerStatefulWidget {
  const SenClawApp({super.key});
  @override
  ConsumerState<SenClawApp> createState() => _SenClawAppState();
}

class _SenClawAppState extends ConsumerState<SenClawApp>
    with TrayListener, WindowListener {
  /// Whether the main SenClaw Desktop window is currently shown (vs. closed to
  /// the tray). Drives the tray click: main shown → re-activate it; main closed
  /// → open the mini chat. Tracked explicitly because windowManager.isVisible()
  /// is unreliable across hide/skipTaskbar transitions. Starts true (main.dart
  /// shows the window on launch).
  bool _mainShown = true;

  bool get _isDesktop =>
      !kIsWeb &&
      (defaultTargetPlatform == TargetPlatform.macOS ||
          defaultTargetPlatform == TargetPlatform.windows ||
          defaultTargetPlatform == TargetPlatform.linux);

  @override
  void initState() {
    super.initState();
    ref.read(connectionBootstrapProvider); // fire-and-forget
    if (_isDesktop) {
      trayManager.addListener(this);
      windowManager.addListener(this);
      // The mini-chat sub-window messages us here (e.g. its "expand" button).
      DesktopMultiWindow.setMethodHandler(_handleSubWindowMethod);
      _initTray();
      windowManager.setPreventClose(true); // hide to tray instead of quitting
      ref.read(systemNotifierProvider).start(); // OS notifications when hidden
    }
  }

  /// Handles method calls sent FROM a sub-window (the mini-chat) TO the main
  /// window. Currently just "expand" → bring the full window forward on Chat.
  Future<dynamic> _handleSubWindowMethod(
      MethodCall call, int fromWindowId) async {
    if (call.method == 'show_full') {
      await _showWindow();
      appRouter.go('/chat');
    }
    return null;
  }

  Future<void> _initTray() async {
    await trayManager.setIcon(
      defaultTargetPlatform == TargetPlatform.windows
          ? 'assets/tray_icon.ico'
          : 'assets/tray_icon.png',
      isTemplate: true, // macOS recolors a template image for the menu bar
    );
    await trayManager.setContextMenu(Menu(items: [
      MenuItem(key: 'open', label: 'Open SenClaw'),
      MenuItem(key: 'diagnostics', label: 'Diagnostics…'),
      MenuItem.separator(),
      MenuItem(key: 'quit', label: 'Quit'),
    ]));
  }

  Future<void> _showWindow() async {
    _mainShown = true;
    // Becoming a regular app again restores the Dock icon + app switcher entry
    // (we drop them while backgrounded — see onWindowClose).
    await windowManager.setSkipTaskbar(false);
    await windowManager.show();
    await windowManager.focus();
    // A backgrounded (accessory) macOS app often won't come to the foreground
    // from show()/focus() alone — ask the native side to NSApp.activate, then a
    // brief always-on-top toggle as a belt-and-suspenders nudge.
    if (defaultTargetPlatform == TargetPlatform.macOS) {
      try {
        await const MethodChannel('senclaw/app').invokeMethod('activate');
      } catch (_) {}
      await windowManager.setAlwaysOnTop(true);
      await windowManager.setAlwaysOnTop(false);
    }
  }

  /// Open the compact "mini chat" as its OWN native window (OpenClaw-style
  /// menu-bar chat) — left-click the tray icon. It runs in a separate Flutter
  /// engine and stays open alongside the full window.
  ///
  /// If the full SenClaw Desktop window is already open, we DON'T pop the mini
  /// popover — we just re-activate the main window. The mini chat is only for
  /// quick access while the app runs in the background. Its size/position are
  /// set natively (see MainFlutterWindow.swift).
  /// Tray-icon click TOGGLES the mini chat: open if none is showing, close any
  /// open one. A fresh window is created each time so it lands on the user's
  /// CURRENT Space/desktop (combined with `.canJoinAllSpaces` natively, the
  /// popover follows the active desktop instead of being stuck on its origin).
  Future<void> _toggleMiniChat() async {
    // If the full SenClaw Desktop window is open, a tray click just re-activates
    // it — the mini chat is only for when the app is closed to the tray.
    if (_mainShown) {
      await _showWindow();
      return;
    }
    try {
      final ids = await DesktopMultiWindow.getAllSubWindowIds();
      if (ids.isNotEmpty) {
        // Already open → close it (toggle off).
        for (final id in ids) {
          await WindowController.fromWindowId(id).close();
        }
        return;
      }
    } catch (_) {}

    final window =
        await DesktopMultiWindow.createWindow(jsonEncode({'type': 'mini'}));
    await window.setTitle('SenClaw');
    await window.show();
  }

  @override
  void onTrayIconMouseDown() => _toggleMiniChat();

  @override
  void onTrayIconRightMouseDown() => trayManager.popUpContextMenu();

  @override
  void onTrayMenuItemClick(MenuItem item) async {
    switch (item.key) {
      case 'open':
        await _showWindow();
      case 'diagnostics':
        await _showWindow();
        appRouter.go('/diagnostics');
      case 'quit':
        await _quitApp();
    }
  }

  /// Full shutdown: close every open window (main + mini-chat sub-windows),
  /// stop the SenClaw daemon (the process we spawned AND, for an adopted one,
  /// whatever still listens on the UI port), then terminate the app.
  Future<void> _quitApp() async {
    // 1. Close mini-chat / any sub-windows (separate Flutter engines).
    try {
      for (final id in await DesktopMultiWindow.getAllSubWindowIds()) {
        await WindowController.fromWindowId(id).close();
      }
    } catch (_) {}
    // 2. Stop the daemon. supervisor.stop() kills a process we spawned;
    //    killPort also covers an adopted daemon (kill by listening port).
    try {
      await ref.read(daemonSupervisorProvider).stop();
    } catch (_) {}
    try {
      await PortTools.killPort(ref.read(appConfigProvider).uiPort);
    } catch (_) {}
    // 3. Tear down the main window and exit for real.
    try {
      await windowManager.setPreventClose(false);
      await windowManager.destroy();
    } catch (_) {}
    exit(0);
  }

  @override
  void onWindowClose() async {
    // Don't quit on close — hide to the menu bar and drop the Dock icon so the
    // app lives purely in the tray (Docker / CCleaner style). The tray icon
    // stays; reopening via tray restores the Dock icon (see _showWindow).
    if (await windowManager.isPreventClose()) {
      _mainShown = false;
      await windowManager.hide();
      await windowManager.setSkipTaskbar(true);
    }
  }

  @override
  void dispose() {
    if (_isDesktop) {
      trayManager.removeListener(this);
      windowManager.removeListener(this);
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    // The in-main mini preview's "expand" button (debug) signals here to
    // restore the full window on Chat.
    if (_isDesktop) {
      ref.listen(miniExpandRequestProvider, (_, _) {
        _showWindow();
        appRouter.go('/chat');
      });
    }
    return MaterialApp.router(
      title: 'SenClaw',
      debugShowCheckedModeBanner: false,
      theme: AppTheme.light(),
      darkTheme: AppTheme.dark(),
      themeMode: ref.watch(themeModeProvider),
      routerConfig: appRouter,
      // Global plan-approval modal, stacked above all routes.
      builder: (context, child) => Stack(
        children: [
          child ?? const SizedBox.shrink(),
          const PlanExitOverlay(),
        ],
      ),
    );
  }
}
