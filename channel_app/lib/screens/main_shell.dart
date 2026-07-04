import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../services/background_sync_service.dart';
import '../services/notification_service.dart';
import '../services/relay_manager.dart';
import '../services/settings_provider.dart';
import 'chat_screen.dart';

/// Post-pairing home. There is no bottom nav and no tab bar — Chat is the root
/// surface and every other destination (Notes, Calendar, Apps, Cài đặt, …) is
/// pushed from the shared [AppDrawer] sidebar (the ☰ button on each screen).
///
/// Kept a plain [StatefulWidget]; the Riverpod-dependent settings wiring lives
/// in the nested [_SyncSettingsGate] so this widget's element type never has to
/// change (which would otherwise break hot reload).
class MainShell extends StatefulWidget {
  const MainShell({super.key});

  @override
  State<MainShell> createState() => _MainShellState();
}

class _MainShellState extends State<MainShell> {
  @override
  void initState() {
    super.initState();
    // Bring up the shared relay + notifications as soon as the shell mounts.
    RelayManager().ensureStarted();
    NotificationService().init();
  }

  @override
  Widget build(BuildContext context) => const _SyncSettingsGate();
}

/// Watches the notification / background-sync settings and (re)configures the
/// [BackgroundSyncService] on every change. Renders the chat surface.
class _SyncSettingsGate extends ConsumerWidget {
  const _SyncSettingsGate();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    // configure() is idempotent — a no-op unless a value actually changed — so
    // driving it from build (which re-runs on any setting change) is safe.
    BackgroundSyncService().configure(
      enabled: ref.watch(backgroundSyncEnabledProvider),
      intervalMinutes: ref.watch(syncIntervalProvider),
      notify: ref.watch(notificationsEnabledProvider),
    );
    return const ChatScreen();
  }
}
