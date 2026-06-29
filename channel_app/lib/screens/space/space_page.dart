import 'package:flutter/material.dart';
import '../../services/relay_manager.dart';
import '../../theme/tokens.dart';
import '../../widgets/app_drawer.dart';
import '../../widgets/states.dart';

/// Shared chrome for a standalone Space screen: titled AppBar + the shared
/// sidebar (☰) + the feature body. Used by Notes/Schedules/Apps screens.
class SpacePage extends StatelessWidget {
  const SpacePage({super.key, required this.title, required this.child});
  final String title;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Scaffold(
      backgroundColor: c.bg,
      drawer: const AppDrawer(),
      appBar: AppBar(
        backgroundColor: c.surface,
        elevation: 0,
        leading: Builder(
          builder: (ctx) => IconButton(
            icon: Icon(Icons.menu, color: c.textSecondary),
            onPressed: () => Scaffold.of(ctx).openDrawer(),
          ),
        ),
        title: Row(
          children: [
            Text(title, style: TextStyle(color: c.textPrimary)),
            const SizedBox(width: 8),
            AnimatedBuilder(
              animation: RelayManager(),
              builder: (_, _) =>
                  ConnectionDot(connected: RelayManager().connected),
            ),
          ],
        ),
      ),
      body: Container(
        decoration: BoxDecoration(color: c.bg),
        child: child,
      ),
    );
  }
}
