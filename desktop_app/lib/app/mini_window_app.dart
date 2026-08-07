import 'package:flutter/material.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../core/i18n/l10n.dart';
import '../core/i18n/locale_provider.dart';
import '../core/transport/connection.dart';
import '../features/chat/mini_chat_screen.dart';
import '../features/chat/widgets/plan_exit_dialog.dart';
import '../theme/app_theme.dart';
import '../theme/theme_mode_provider.dart';

/// Root widget for the standalone tray mini-chat window. Runs in its own Flutter
/// engine/isolate (spawned by `desktop_multi_window`), so it shares no in-memory
/// state with the main window — instead it bootstraps its own connection and
/// talks to the same local daemon as an independent WS client.
///
/// Deliberately minimal: no system tray, no daemon spawning (the main window
/// already owns those), no nav-rail shell — just the compact conversation.
class MiniWindowApp extends ConsumerStatefulWidget {
  const MiniWindowApp({super.key, required this.windowId});

  /// The `desktop_multi_window` id for THIS window, used by the header to close
  /// itself and to message the main window (e.g. the "expand" button).
  final int windowId;

  @override
  ConsumerState<MiniWindowApp> createState() => _MiniWindowAppState();
}

class _MiniWindowAppState extends ConsumerState<MiniWindowApp> {
  @override
  void initState() {
    super.initState();
    // Adopt the running daemon + open our own WebSocket (fire-and-forget).
    ref.read(connectionBootstrapProvider);
  }

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'SenClaw',
      debugShowCheckedModeBanner: false,
      theme: AppTheme.light(),
      darkTheme: AppTheme.dark(),
      themeMode: ref.watch(themeModeProvider),
      locale: Locale(ref.watch(localeCodeProvider)),
      supportedLocales: const [Locale('en'), Locale('vi')],
      localizationsDelegates: const [
        L10nDelegate(),
        GlobalMaterialLocalizations.delegate,
        GlobalWidgetsLocalizations.delegate,
        GlobalCupertinoLocalizations.delegate,
      ],
      home: const MiniChatScreen(),
      // Plan-approval modal works here too (the mini window can run plans).
      builder: (context, child) => Stack(
        children: [
          child ?? const SizedBox.shrink(),
          const PlanExitOverlay(),
        ],
      ),
    );
  }
}
