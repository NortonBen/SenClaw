import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../core/relay_providers.dart';
import '../theme/theme_mode_provider.dart';
import '../theme/tokens.dart';
import '../screens/code/code_screen.dart';
import '../screens/cognitive/cognitive_screen.dart';
import '../screens/cowork/cowork_screen.dart';
import '../screens/workflow/workflow_screen.dart';
import '../screens/more/more_screen.dart';
import '../screens/plugins/plugins_screen.dart';
import '../screens/space/space_screen.dart';
import '../screens/wiki/wiki_screen.dart';

/// App-wide sidebar (desktop-style nav), mounted as the Scaffold `drawer` on
/// every screen. Chat is the root surface; everything else is pushed from here.
class AppDrawer extends ConsumerWidget {
  const AppDrawer({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final connected = ref.watch(relayConnectedProvider);
    final mode = ref.watch(themeModeProvider);

    // Back to the Chat root.
    void goHome() {
      final nav = Navigator.of(context);
      nav.pop(); // close drawer
      nav.popUntil((r) => r.isFirst);
    }

    // Replace any pushed screen with [screen] so the stack stays flat.
    void open(Widget screen) {
      final nav = Navigator.of(context);
      nav.pop(); // close drawer
      nav.popUntil((r) => r.isFirst);
      nav.push(MaterialPageRoute(builder: (_) => screen));
    }

    return Drawer(
      backgroundColor: c.surface,
      child: SafeArea(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            // Header: logo + SenClaw + connection, with the Settings gear on
            // the right of the same row.
            Padding(
              padding: const EdgeInsets.fromLTRB(16, 18, 8, 14),
              child: Row(
                children: [
                  ClipRRect(
                    borderRadius: BorderRadius.circular(10),
                    child: Image.asset(
                      'assets/images/logo.png',
                      width: 40,
                      height: 40,
                      fit: BoxFit.cover,
                      filterQuality: FilterQuality.medium,
                    ),
                  ),
                  const SizedBox(width: 12),
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text('SenClaw',
                            style: TextStyle(
                                color: c.textPrimary,
                                fontSize: 18,
                                fontWeight: FontWeight.bold)),
                        Row(children: [
                          Container(
                            width: 8,
                            height: 8,
                            decoration: BoxDecoration(
                              shape: BoxShape.circle,
                              color: connected
                                  ? AppTokens.success
                                  : AppTokens.warning,
                            ),
                          ),
                          const SizedBox(width: 6),
                          Text(connected ? 'Đã kết nối' : 'Mất kết nối',
                              style:
                                  TextStyle(color: c.textMuted, fontSize: 11)),
                        ]),
                      ],
                    ),
                  ),
                  IconButton(
                    tooltip: 'Cài đặt',
                    icon: Icon(Icons.settings_outlined, color: c.textSecondary),
                    onPressed: () => open(const MoreScreen()),
                  ),
                ],
              ),
            ),
            Divider(color: c.border, height: 1),
            Expanded(
              child: ListView(
                padding: const EdgeInsets.symmetric(vertical: 6),
                children: [
                  _navTile(c, Icons.forum_outlined, 'Hội thoại',
                      onTap: goHome),
                  _sectionLabel(c, 'Space'),
                  _navTile(c, Icons.apps_outlined, 'Apps',
                      onTap: () => open(const AppsScreen())),
                  _navTile(c, Icons.sticky_note_2_outlined, 'Notes',
                      onTap: () => open(const NotesScreen())),
                  _navTile(c, Icons.event_note_outlined, 'Calendar',
                      onTap: () => open(const CalendarScreen())),
                  _navTile(c, Icons.schedule, 'Schedules',
                      onTap: () => open(const SchedulesScreen())),
                  _sectionLabel(c, 'Tương tác'),
                  _navTile(c, Icons.code, 'Code sessions',
                      onTap: () => open(const CodeScreen())),
                  _navTile(c, Icons.groups_outlined, 'Cowork',
                      onTap: () => open(const CoworkScreen())),
                  _navTile(c, Icons.account_tree_outlined, 'Workflow',
                      onTap: () => open(const WorkflowScreen())),
                  _navTile(c, Icons.menu_book_outlined, 'Wiki',
                      onTap: () => open(const WikiScreen())),
                  _navTile(c, Icons.hub_outlined, 'Tri thức',
                      onTap: () => open(const CognitiveScreen())),
                  _navTile(c, Icons.extension_outlined, 'Plugins',
                      onTap: () => open(const PluginsScreen())),
                ],
              ),
            ),
            Divider(color: c.border, height: 1),
            // Theme quick toggle.
            Padding(
              padding: const EdgeInsets.fromLTRB(16, 10, 16, 6),
              child: Row(
                children: [
                  Icon(Icons.palette_outlined, size: 18, color: c.textMuted),
                  const SizedBox(width: 12),
                  Text('Giao diện',
                      style: TextStyle(color: c.textSecondary, fontSize: 13)),
                  const Spacer(),
                  _themeBtn(c, Icons.brightness_auto, ThemeMode.system, mode, ref),
                  _themeBtn(c, Icons.light_mode, ThemeMode.light, mode, ref),
                  _themeBtn(c, Icons.dark_mode, ThemeMode.dark, mode, ref),
                ],
              ),
            ),
            const SizedBox(height: 8),
          ],
        ),
      ),
    );
  }

  Widget _sectionLabel(AppColors c, String t) => Padding(
        padding: const EdgeInsets.fromLTRB(20, 14, 20, 6),
        child: Text(t.toUpperCase(),
            style: TextStyle(
                color: c.textMuted,
                fontSize: 11,
                fontWeight: FontWeight.w700,
                letterSpacing: 0.8)),
      );

  Widget _navTile(AppColors c, IconData icon, String label,
      {Color? iconColor, Color? color, required VoidCallback onTap}) {
    return ListTile(
      dense: true,
      leading: Icon(icon, size: 20, color: iconColor ?? c.textSecondary),
      title: Text(label,
          style: TextStyle(color: color ?? c.textPrimary, fontSize: 14)),
      onTap: onTap,
    );
  }

  Widget _themeBtn(AppColors c, IconData icon, ThemeMode m, ThemeMode current,
      WidgetRef ref) {
    final on = current == m;
    return IconButton(
      visualDensity: VisualDensity.compact,
      icon: Icon(icon, size: 18, color: on ? c.accent : c.textMuted),
      onPressed: () => ref.read(themeModeProvider.notifier).set(m),
    );
  }
}
