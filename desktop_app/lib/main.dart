import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:hotkey_manager/hotkey_manager.dart';
import 'package:local_notifier/local_notifier.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:window_manager/window_manager.dart';
import 'app/app.dart';
import 'app/mini_window_app.dart';
import 'core/prefs.dart';
import 'features/chat/mini_chat_screen.dart' show subWindowIdProvider;

Future<void> main(List<String> args) async {
  // `desktop_multi_window` re-launches this same executable for each sub-window
  // with argv `['multi_window', <windowId>, <jsonArgs>]`. That branch runs the
  // standalone tray mini-chat instead of the full app shell.
  if (args.firstOrNull == 'multi_window') {
    await _runMiniWindow(args);
    return;
  }

  WidgetsFlutterBinding.ensureInitialized();

  final prefs = await SharedPreferences.getInstance();

  // Desktop-only window chrome (no-op on web/mobile).
  if (!kIsWeb &&
      (defaultTargetPlatform == TargetPlatform.macOS ||
          defaultTargetPlatform == TargetPlatform.windows ||
          defaultTargetPlatform == TargetPlatform.linux)) {
    await windowManager.ensureInitialized();
    await localNotifier.setup(appName: 'SenClaw');
    // macOS hotkeys are registered with Carbon, which outlives a hot restart —
    // without this, a stale registration from the previous run keeps the combo
    // and the fresh one silently fails to bind.
    await hotKeyManager.unregisterAll();
    const opts = WindowOptions(
      size: Size(1280, 820),
      minimumSize: Size(900, 600),
      center: true,
      title: 'SenClaw',
      titleBarStyle: TitleBarStyle.hidden,
    );
    windowManager.waitUntilReadyToShow(opts, () async {
      await windowManager.show();
      await windowManager.focus();
    });
  }

  runApp(
    ProviderScope(
      overrides: [prefsProvider.overrideWithValue(prefs)],
      child: const SenClawApp(),
    ),
  );
}

/// Sub-window entrypoint: a second, independent Flutter engine that renders only
/// the compact mini-chat. It does NOT spawn a daemon (the main window owns that)
/// — it adopts the already-running daemon and connects as its own WS client, so
/// it can stay open alongside the full window.
Future<void> _runMiniWindow(List<String> args) async {
  WidgetsFlutterBinding.ensureInitialized();

  final windowId = int.tryParse(args.length > 1 ? args[1] : '') ?? 0;
  // args[2] is reserved for future per-window arguments (JSON); unused for now.
  if (args.length > 2 && args[2].isNotEmpty) {
    try {
      jsonDecode(args[2]);
    } catch (_) {}
  }

  final prefs = await SharedPreferences.getInstance();
  runApp(
    ProviderScope(
      overrides: [
        prefsProvider.overrideWithValue(prefs),
        subWindowIdProvider.overrideWithValue(windowId),
      ],
      child: MiniWindowApp(windowId: windowId),
    ),
  );
}
