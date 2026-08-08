import 'dart:async';
import 'dart:convert';
import 'dart:io' show Process;

import 'package:appflowy_editor/appflowy_editor.dart'
    show AppFlowyEditorLocalizations;
import 'package:desktop_multi_window/desktop_multi_window.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:tray_manager/tray_manager.dart';
import 'package:window_manager/window_manager.dart';
import '../core/daemon/app_shutdown.dart';
import '../core/daemon/daemon_provider.dart';
import '../core/daemon/startup_gate.dart';
import '../core/i18n/l10n.dart';
import '../core/i18n/locale_provider.dart';
import '../core/notifications/system_notifier.dart';
import '../core/transport/connection.dart';
import '../core/update/update_provider.dart';
import '../features/capture/capture_hotkey.dart';
import '../features/capture/capture_review.dart';
import '../features/capture/screen_capture.dart';
import '../features/chat/mini_chat_screen.dart' show miniExpandRequestProvider;
import '../features/chat/reminder_interaction.dart';
import '../features/chat/widgets/plan_exit_dialog.dart';
import '../features/settings/settings_screen.dart' show settingsSectionProvider;
import '../theme/app_theme.dart';
import '../theme/theme_mode_provider.dart';
import 'caption_bar.dart';
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

  /// Hourly nudge for the background update check (which debounces to daily).
  Timer? _updateTimer;

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
      _startUpdateChecks();
      _listenToNativeMenu();
      // Same action as the tray's "Capture Screen…", on a global shortcut.
      if (isCaptureSupported) {
        ref.read(captureHotkeyProvider.notifier).bind((_) => _captureAndReview());
      }
    }
  }

  /// Background update checks. The launch one runs through
  /// [UpdateNotifier.startupCheck], which skips the daily debounce so every
  /// start answers "is there a new version?" — a find pops the update
  /// announcer's modal. Delayed a few seconds so it does not compete with
  /// the daemon boot for I/O. Then hourly via [UpdateNotifier.maybeCheck],
  /// which does debounce to once a day, because this app lives in the tray for
  /// days at a time and rarely restarts.
  void _startUpdateChecks() {
    Future.delayed(const Duration(seconds: 10), () {
      if (mounted) ref.read(updateProvider.notifier).startupCheck();
    });
    _updateTimer = Timer.periodic(const Duration(hours: 1), (_) {
      if (mounted) ref.read(updateProvider.notifier).maybeCheck();
    });
  }

  /// The macOS app menu ("Check for Updates…", "Settings…") calls in here —
  /// see AppDelegate.swift. The same `senclaw/app` channel already carries
  /// Dart → native "activate"; this is the reverse direction.
  ///
  /// macOS only: Windows/Linux have no app menu, and reaching these actions
  /// there is what the nav rail and Settings sidebar are for.
  void _listenToNativeMenu() {
    if (defaultTargetPlatform != TargetPlatform.macOS) return;
    const MethodChannel('senclaw/app').setMethodCallHandler((call) async {
      switch (call.method) {
        case 'checkForUpdates':
          appRouter.go('/settings');
          ref.read(settingsSectionProvider.notifier).state = 'updates';
          await ref.read(updateProvider.notifier).check();
        case 'showSettings':
          appRouter.go('/settings');
      }
      return null;
    });
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
    // Reading the provider both resolves the persisted language and keeps
    // L10n.global current before the labels below are built.
    ref.read(localeCodeProvider);
    final t = L10n.global.t;
    await trayManager.setIcon(
      defaultTargetPlatform == TargetPlatform.windows
          ? (await _windowsTaskbarIsLight()
              ? 'assets/tray_icon.ico' // black glyph for the light taskbar
              : 'assets/tray_icon_white.ico')
          : 'assets/tray_icon.png',
      isTemplate: true, // macOS recolors a template image for the menu bar
    );
    await trayManager.setContextMenu(Menu(items: [
      MenuItem(key: 'open', label: t('Open SenClaw')),
      // Only macOS has a native capture bridge — omitted rather than shown
      // disabled, so the menu doesn't advertise what it can't do.
      if (isCaptureSupported)
        MenuItem(key: 'capture', label: t('Capture Screen…')),
      MenuItem(key: 'diagnostics', label: t('Diagnostics…')),
      MenuItem.separator(),
      MenuItem(key: 'quit', label: t('Quit')),
    ]));
  }

  /// Windows shows tray icons as raw pixels — there is no template recolor
  /// like the macOS menu bar — so the glyph color must match the taskbar
  /// theme. SystemUsesLightTheme is the taskbar's own knob; AppsUseLightTheme
  /// (what platformBrightness reflects) can be set independently of it.
  /// Defaults to dark (white icon): dark is the Windows default, and a white
  /// glyph on a light taskbar is merely faint, while black on dark vanishes.
  Future<bool> _windowsTaskbarIsLight() async {
    try {
      final r = await Process.run('reg', [
        'query',
        r'HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize',
        '/v',
        'SystemUsesLightTheme',
      ]);
      return r.exitCode == 0 && (r.stdout as String).contains('0x1');
    } catch (_) {
      return false;
    }
  }

  /// Tray → capture a region → open the review sheet, where the shot becomes a
  /// note plus a reminder. The window is deliberately NOT raised first: the
  /// point is to capture what's on screen behind us.
  Future<void> _captureAndReview() async {
    try {
      final shot = await captureScreen(
        dir: ref.read(appConfigProvider).screenshotsDir,
      );
      if (shot == null) return; // ESC — say nothing.
      ref.read(pendingCaptureProvider.notifier).state = shot;
      // The review sheet lives over the main window, so it has to be up.
      await _showWindow();
    } on ScreenCapturePermissionDenied {
      await _showWindow();
      ref.read(capturePermissionNeededProvider.notifier).state = true;
    } on ScreenCaptureFailed catch (e) {
      await _showWindow();
      ref.read(captureErrorProvider.notifier).state = e.message;
    }
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
      case 'capture':
        await _captureAndReview();
      case 'diagnostics':
        await _showWindow();
        appRouter.go('/diagnostics');
      case 'quit':
        await _quitApp();
    }
  }

  /// Full shutdown — see [shutdownApp], which the updater shares.
  Future<void> _quitApp() => shutdownApp(
        supervisor: ref.read(daemonSupervisorProvider),
        uiPort: ref.read(appConfigProvider).uiPort,
      );

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
    _updateTimer?.cancel();
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
      // Tray labels are native — rebuild the menu when the language flips.
      ref.listen(localeCodeProvider, (_, _) => _initTray());
    }
    final localeCode = ref.watch(localeCodeProvider);
    return MaterialApp.router(
      title: 'SenClaw',
      debugShowCheckedModeBanner: false,
      theme: AppTheme.light(),
      darkTheme: AppTheme.dark(),
      themeMode: ref.watch(themeModeProvider),
      locale: Locale(localeCode),
      supportedLocales: const [Locale('en'), Locale('vi')],
      // L10nDelegate serves the app's own strings (context.tr). The AppFlowy
      // delegate is required by the inline AppFlowyEditor in the Notes screen —
      // it reads AppFlowyEditorLocalizations.current and throws without it.
      // The Global* delegates localize Flutter's built-in widgets.
      localizationsDelegates: const [
        L10nDelegate(),
        AppFlowyEditorLocalizations.delegate,
        GlobalMaterialLocalizations.delegate,
        GlobalWidgetsLocalizations.delegate,
        GlobalCupertinoLocalizations.delegate,
      ],
      routerConfig: appRouter,
      // StartupGate holds a splash until the daemon answers HTTP, so no route
      // (and none of its data providers) runs against a dead daemon. The
      // plan-approval modal stacks above all routes once the shell is live.
      // DesktopChrome sits above the gate: Windows/Linux draw their own
      // caption bar (drag + min/max/close), which must exist even on the
      // splash and daemon-crash screens.
      builder: (context, child) => DesktopChrome(
        child: StartupGate(
          child: Stack(
            children: [
              child ?? const SizedBox.shrink(),
              const PlanExitOverlay(),
              const ReminderInteractionOverlay(),
              const CaptureReviewOverlay(),
            ],
          ),
        ),
      ),
    );
  }
}
