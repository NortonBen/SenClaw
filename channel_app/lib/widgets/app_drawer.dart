import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../core/relay_providers.dart';
import '../models/session_model.dart';
import '../services/language_service.dart';
import '../services/sessions_provider.dart';
import '../theme/theme_mode_provider.dart';
import '../theme/tokens.dart';
import '../screens/sessions_screen.dart';
import '../screens/background/background_screen.dart';
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

    // Activate a session and return to the chat root.
    void openSession(SessionInfo s) {
      ref.read(sessionsProvider.notifier).select(s.jid, folder: s.folder);
      ref.read(selectedSessionJidProvider.notifier).state = s.jid;
      goHome();
    }

    // Three most-recent sessions, freshest first.
    final sessions = ref.watch(sessionsProvider);
    final selectedJid = ref.watch(selectedSessionJidProvider);
    final recents = [...sessions]
      ..sort((a, b) => (b.lastActivity ?? 0).compareTo(a.lastActivity ?? 0));
    final topRecents = recents.take(3).toList();

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
                          Text(
                              connected
                                  ? tr('Đã kết nối', 'Connected')
                                  : tr('Mất kết nối', 'Disconnected'),
                              style:
                                  TextStyle(color: c.textMuted, fontSize: 11)),
                        ]),
                      ],
                    ),
                  ),
                  IconButton(
                    tooltip: tr('Cài đặt', 'Settings'),
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
                  _navTile(c, Icons.forum_outlined, tr('Hội thoại', 'Chats'),
                      onTap: goHome),
                  // Recent sessions (mini) + a "More" entry into the full list.
                  for (final s in topRecents)
                    _recentTile(c, s, s.jid == selectedJid,
                        () => openSession(s)),
                  _moreSessionsTile(c, sessions.length,
                      () => open(const SessionsScreen())),
                  _sectionLabel(c, tr('Space', 'Space')),
                  _navTile(c, Icons.apps_outlined, tr('Apps', 'Apps'),
                      onTap: () => open(const AppsScreen())),
                  _navTile(
                      c, Icons.sticky_note_2_outlined, tr('Ghi chú', 'Notes'),
                      onTap: () => open(const NotesScreen())),
                  _navTile(c, Icons.event_note_outlined, tr('Lịch', 'Calendar'),
                      onTap: () => open(const CalendarScreen())),
                  _navTile(c, Icons.schedule, tr('Lịch trình', 'Schedules'),
                      onTap: () => open(const SchedulesScreen())),
                  _sectionLabel(c, tr('Tương tác', 'Interact')),
                  _navTile(c, Icons.code, tr('Code sessions', 'Code sessions'),
                      onTap: () => open(const CodeScreen())),
                  _navTile(c, Icons.groups_outlined, tr('Cowork', 'Cowork'),
                      onTap: () => open(const CoworkScreen())),
                  _navTile(c, Icons.account_tree_outlined,
                      tr('Workflow', 'Workflow'),
                      onTap: () => open(const WorkflowScreen())),
                  _navTile(c, Icons.motion_photos_auto_outlined,
                      tr('Tác vụ nền', 'Background'),
                      onTap: () => open(const BackgroundScreen())),
                  _navTile(c, Icons.menu_book_outlined, tr('Wiki', 'Wiki'),
                      onTap: () => open(const WikiScreen())),
                  _navTile(c, Icons.hub_outlined, tr('Tri thức', 'Knowledge'),
                      onTap: () => open(const CognitiveScreen())),
                  _navTile(c, Icons.extension_outlined,
                      tr('Plugins', 'Plugins'),
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
                  Text(tr('Giao diện', 'Theme'),
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

  /// A compact recent-session row shown under "Chats": a small state dot, the
  /// session title (mini) and its relative activity time.
  Widget _recentTile(
      AppColors c, SessionInfo s, bool isSelected, VoidCallback onTap) {
    return InkWell(
      onTap: onTap,
      child: Padding(
        padding: const EdgeInsets.only(
            left: 28, right: 12, top: 5, bottom: 5),
        child: Row(
          children: [
            Container(
              width: 6,
              height: 6,
              decoration: BoxDecoration(
                shape: BoxShape.circle,
                color: s.active
                    ? AppTokens.success
                    : isSelected
                        ? c.accent
                        : c.textMuted.withValues(alpha: 0.5),
              ),
            ),
            const SizedBox(width: 10),
            Expanded(
              child: Text(
                s.title,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(
                  color: isSelected ? c.accent : c.textSecondary,
                  fontSize: 12.5,
                  fontWeight:
                      isSelected ? FontWeight.w600 : FontWeight.w400,
                ),
              ),
            ),
            Text(
              _recentRelTime(s.lastActivity ?? 0),
              style: TextStyle(color: c.textMuted, fontSize: 10.5),
            ),
          ],
        ),
      ),
    );
  }

  /// The "More…" row that opens the full [SessionsScreen], with a session count.
  Widget _moreSessionsTile(AppColors c, int count, VoidCallback onTap) {
    return InkWell(
      onTap: onTap,
      child: Padding(
        padding: const EdgeInsets.only(
            left: 28, right: 12, top: 6, bottom: 6),
        child: Row(
          children: [
            Icon(Icons.more_horiz, size: 16, color: c.textMuted),
            const SizedBox(width: 10),
            Text(tr('Xem tất cả phiên', 'All sessions'),
                style: TextStyle(color: c.textMuted, fontSize: 12.5)),
            const Spacer(),
            if (count > 0)
              Text('$count',
                  style: TextStyle(color: c.textMuted, fontSize: 11)),
          ],
        ),
      ),
    );
  }

  String _recentRelTime(int ms) {
    if (ms <= 0) return '';
    final dt = DateTime.fromMillisecondsSinceEpoch(ms);
    final d = DateTime.now().difference(dt);
    if (d.inMinutes < 60) return '${d.inMinutes}m';
    if (d.inHours < 24) return '${d.inHours}h';
    if (d.inDays < 7) return '${d.inDays}d';
    return '${dt.day}/${dt.month}';
  }

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
