import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import '../features/dashboard/dashboard_screen.dart';
import '../features/chat/chat_screen.dart';
import '../features/chat/mini_chat_screen.dart';
import '../features/cognitive/cognitive_screen.dart';
import '../features/cowork/cowork_screen.dart';
import '../features/kanban/kanban_screen.dart';
import '../features/diagnostics/diagnostics_screen.dart';
import '../features/plugins/plugins_screen.dart';
import '../features/settings/settings_screen.dart';
import '../features/space/space_screen.dart'
    show NotesScreen, CalendarScreen, SpaceAppsScreen;
import '../features/wiki/wiki_screen.dart';
import '../features/workflow/workflow_runs_screen.dart';
import 'shell.dart';

/// App routing. A single [ShellRoute] keeps the nav rail mounted while the
/// content pane swaps. Deep links like `/chat/<jid>` are handled inside Chat.
final appRouter = GoRouter(
  initialLocation: '/dashboard',
  routes: [
    // The tray mini-chat normally runs as its OWN native window (a separate
    // Flutter engine via desktop_multi_window). This route is the DEBUG-only
    // in-main preview (hot-reloadable) opened from the tray "Mini chat
    // (preview)" item — separate-engine windows can't be hot-reloaded.
    GoRoute(
      path: '/mini',
      pageBuilder: (context, state) => _noTransition(const MiniChatScreen()),
    ),
    ShellRoute(
      builder: (context, state, child) => AppShell(child: child),
      routes: [
        GoRoute(
          path: '/dashboard',
          pageBuilder: (context, state) => _noTransition(const DashboardScreen()),
        ),
        GoRoute(
          path: '/chat',
          pageBuilder: (context, state) => _noTransition(const ChatScreen()),
        ),
        GoRoute(
          path: '/apps',
          pageBuilder: (context, state) =>
              _noTransition(const SpaceAppsScreen()),
        ),
        // Kept (no nav item) — the "Open Cowork board" button navigates here.
        GoRoute(
          path: '/cowork',
          pageBuilder: (context, state) => _noTransition(const CoworkScreen()),
        ),
        GoRoute(
          path: '/kanban',
          pageBuilder: (context, state) => _noTransition(const KanbanScreen()),
        ),
        GoRoute(
          path: '/notes',
          pageBuilder: (context, state) => _noTransition(const NotesScreen()),
        ),
        GoRoute(
          path: '/calendar',
          pageBuilder: (context, state) =>
              _noTransition(const CalendarScreen()),
        ),
        GoRoute(
          path: '/wiki',
          pageBuilder: (context, state) => _noTransition(const WikiScreen()),
        ),
        GoRoute(
          path: '/cognitive',
          pageBuilder: (context, state) => _noTransition(const CognitiveScreen()),
        ),
        GoRoute(
          path: '/plugins',
          pageBuilder: (context, state) => _noTransition(const PluginsScreen()),
        ),
        // Kept (no nav item) — reached from Plugins → Workflow ("Run
        // history") and automatically after triggering a run.
        GoRoute(
          path: '/workflow-runs',
          pageBuilder: (context, state) =>
              _noTransition(const WorkflowRunsScreen()),
        ),
        GoRoute(
          path: '/settings',
          pageBuilder: (context, state) => _noTransition(const SettingsScreen()),
        ),
        GoRoute(
          path: '/diagnostics',
          pageBuilder: (context, state) =>
              _noTransition(const DiagnosticsScreen()),
        ),
      ],
    ),
  ],
);

NoTransitionPage _noTransition(Widget child) => NoTransitionPage(child: child);
