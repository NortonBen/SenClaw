import 'package:flutter/material.dart';
import '../services/relay_manager.dart';
import '../theme/app_colors.dart';
import 'chat_screen.dart';
import 'code/code_screen.dart';
import 'space/space_screen.dart';
import 'cowork/cowork_screen.dart';

/// Post-pairing home. Hosts the migrated feature surfaces (Chat, Code, Space,
/// Cowork) behind a bottom navigation bar, sharing one relay connection.
class MainShell extends StatefulWidget {
  const MainShell({super.key});

  @override
  State<MainShell> createState() => _MainShellState();
}

class _MainShellState extends State<MainShell> {
  int _index = 0;

  // Built lazily but kept alive in an IndexedStack so each tab preserves state.
  late final List<Widget> _tabs = const [
    ChatScreen(),
    CodeScreen(),
    SpaceScreen(),
    CoworkScreen(),
  ];

  @override
  void initState() {
    super.initState();
    // Bring up the shared relay as soon as the shell mounts so the REST tunnel
    // is ready by the time the user opens Code/Space/Cowork.
    RelayManager().ensureStarted();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: AppColors.bg,
      body: IndexedStack(index: _index, children: _tabs),
      bottomNavigationBar: AnimatedBuilder(
        animation: RelayManager(),
        builder: (context, _) {
          final connected = RelayManager().connected;
          return BottomNavigationBar(
            currentIndex: _index,
            onTap: (i) => setState(() => _index = i),
            type: BottomNavigationBarType.fixed,
            backgroundColor: AppColors.surface,
            selectedItemColor: AppColors.accent,
            unselectedItemColor: Colors.white38,
            selectedFontSize: 11,
            unselectedFontSize: 11,
            items: [
              const BottomNavigationBarItem(
                icon: Icon(Icons.chat_bubble_outline),
                activeIcon: Icon(Icons.chat_bubble),
                label: 'Chat',
              ),
              const BottomNavigationBarItem(
                icon: Icon(Icons.code),
                label: 'Code',
              ),
              const BottomNavigationBarItem(
                icon: Icon(Icons.dashboard_outlined),
                activeIcon: Icon(Icons.dashboard),
                label: 'Space',
              ),
              BottomNavigationBarItem(
                icon: Stack(
                  clipBehavior: Clip.none,
                  children: [
                    const Icon(Icons.groups_outlined),
                    Positioned(
                      right: -2,
                      top: -2,
                      child: Container(
                        width: 8,
                        height: 8,
                        decoration: BoxDecoration(
                          shape: BoxShape.circle,
                          color: connected ? Colors.greenAccent : Colors.orangeAccent,
                          border: Border.all(color: AppColors.surface, width: 1.5),
                        ),
                      ),
                    ),
                  ],
                ),
                activeIcon: const Icon(Icons.groups),
                label: 'Cowork',
              ),
            ],
          );
        },
      ),
    );
  }
}
