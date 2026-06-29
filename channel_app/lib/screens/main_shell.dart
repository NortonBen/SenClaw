import 'package:flutter/material.dart';
import '../services/relay_manager.dart';
import 'chat_screen.dart';

/// Post-pairing home. There is no bottom nav and no tab bar — Chat is the root
/// surface and every other destination (Notes, Calendar, Apps, Cài đặt, …) is
/// pushed from the shared [AppDrawer] sidebar (the ☰ button on each screen).
class MainShell extends StatefulWidget {
  const MainShell({super.key});

  @override
  State<MainShell> createState() => _MainShellState();
}

class _MainShellState extends State<MainShell> {
  @override
  void initState() {
    super.initState();
    // Bring up the shared relay as soon as the shell mounts.
    RelayManager().ensureStarted();
  }

  @override
  Widget build(BuildContext context) => const ChatScreen();
}
