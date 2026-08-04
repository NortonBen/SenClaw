import 'package:flutter/material.dart';

/// One entry in the left icon rail. Order here defines the rail order.
class NavSection {
  final String path;
  final String label;
  final IconData icon;
  const NavSection(this.path, this.label, this.icon);
}

const navSections = <NavSection>[
  NavSection('/dashboard', 'Dashboard', Icons.dashboard_outlined),
  NavSection('/chat', 'Chat', Icons.forum_outlined),
  NavSection('/apps', 'Apps', Icons.apps_outlined),
  NavSection('/kanban', 'Kanban', Icons.view_kanban_outlined),
  NavSection('/notes', 'Notes', Icons.sticky_note_2_outlined),
  NavSection('/calendar', 'Calendar', Icons.calendar_month_outlined),
  NavSection('/wiki', 'Wiki', Icons.menu_book_outlined),
  NavSection('/plugins', 'Plugins', Icons.extension_outlined),
  // Autonomous work the daemon runs by itself. Sits with Plugins/Settings at the
  // bottom rather than up with Calendar: it is something you check on, not
  // something you work in. It is NOT a Settings sub-section — a background task
  // is live state, not configuration.
  //
  // Not a moon: Settings → Appearance already uses `dark_mode_outlined` (moon)
  // for the Dark theme, and this item sits right next to the Settings gear, so a
  // moon here reads as a theme toggle. `brightness_auto_outlined` (the "A"
  // badge) is taken by the System theme for the same reason — avoid that whole
  // family.
  NavSection('/background', 'Background', Icons.pending_actions),
  // Token accounting. Sits with Background/Settings: it is something you
  // check on (spend), not something you work in.
  NavSection('/usage', 'Usage', Icons.bar_chart_outlined),
  NavSection('/settings', 'Settings', Icons.settings_outlined),
];
