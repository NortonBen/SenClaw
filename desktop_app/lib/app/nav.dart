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
  NavSection('/space', 'Space', Icons.workspaces_outline),
  NavSection('/wiki', 'Wiki', Icons.menu_book_outlined),
  NavSection('/plugins', 'Plugins', Icons.extension_outlined),
  NavSection('/settings', 'Settings', Icons.settings_outlined),
];
