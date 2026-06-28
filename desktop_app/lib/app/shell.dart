import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:window_manager/window_manager.dart';
import '../core/config/app_config.dart';
import '../core/transport/connection.dart';
import '../core/transport/ws_client.dart';
import '../features/chat/notifications.dart' show NotificationsBell;
import '../features/space/space_providers.dart';
import '../features/space/space_screen.dart' show RunningAppsLayer;
import '../theme/tokens.dart';
import 'nav.dart';

/// Height reserved at the top of the window for the macOS traffic-light
/// buttons (the title bar is hidden, so they float over the content).
const double _kMacTitleBar = 28;
bool get _isMacOS =>
    !kIsWeb && defaultTargetPlatform == TargetPlatform.macOS;

/// The persistent app frame: a draggable macOS title strip on top (so the
/// traffic lights don't overlap the UI), then the icon rail + routed content.
class AppShell extends ConsumerWidget {
  const AppShell({super.key, required this.child});
  final Widget child;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final location = GoRouterState.of(context).uri.path;

    final frame = Row(
      children: [
        _NavRail(location: location),
        Container(width: 1, color: c.border),
        Expanded(
          child: Stack(
            children: [
              child,
              // Running Space apps stay mounted here so they keep running while
              // the user is on other screens; shown only on /apps when active.
              Consumer(builder: (context, ref, _) {
                final active = ref.watch(runningAppsProvider).active;
                final show = location.startsWith('/apps') && active != null;
                return Offstage(
                  offstage: !show,
                  child: const RunningAppsLayer(),
                );
              }),
            ],
          ),
        ),
      ],
    );

    return Scaffold(
      backgroundColor: c.bg,
      body: _isMacOS
          ? Column(
              children: [
                // Reserve + drag the traffic-light strip.
                GestureDetector(
                  onPanStart: (_) => windowManager.startDragging(),
                  child: Container(
                    height: _kMacTitleBar,
                    color: c.sidebar,
                  ),
                ),
                Expanded(child: frame),
              ],
            )
          : frame,
    );
  }
}

class _NavRail extends ConsumerWidget {
  const _NavRail({required this.location});
  final String location;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    return Container(
      width: AppTokens.railWidth,
      color: c.sidebar,
      child: Column(
        children: [
          const SizedBox(height: AppTokens.s16),
          _Logo(),
          const SizedBox(height: AppTokens.s16),
          Expanded(
            child: ListView(
              padding: const EdgeInsets.symmetric(vertical: AppTokens.s8),
              children: [
                for (final s in navSections)
                  _RailItem(
                    section: s,
                    active: location.startsWith(s.path),
                    onTap: () => context.go(s.path),
                  ),
              ],
            ),
          ),
          // Global notifications bell — discoverable from every screen.
          const NotificationsBell(),
          const SizedBox(height: AppTokens.s4),
          const _ConnectionDot(),
          const SizedBox(height: AppTokens.s4),
          Text(
            'v$kAppVersion',
            style: TextStyle(
              color: c.textMuted,
              fontSize: 9,
              fontFamily: AppTokens.fontMono,
            ),
          ),
          const SizedBox(height: AppTokens.s12),
        ],
      ),
    );
  }
}

class _Logo extends StatelessWidget {
  @override
  Widget build(BuildContext context) {
    // The real app icon (rounded) instead of the old gradient "S" placeholder.
    return ClipRRect(
      borderRadius: BorderRadius.circular(AppTokens.rMd),
      child: Image.asset(
        'assets/branding/senclaw_icon_1024.png',
        width: 36,
        height: 36,
        fit: BoxFit.cover,
        filterQuality: FilterQuality.medium,
      ),
    );
  }
}

class _RailItem extends StatelessWidget {
  const _RailItem({
    required this.section,
    required this.active,
    required this.onTap,
  });
  final NavSection section;
  final bool active;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Tooltip(
      message: section.label,
      preferBelow: false,
      child: Padding(
        padding: const EdgeInsets.symmetric(
          horizontal: AppTokens.s12,
          vertical: AppTokens.s4,
        ),
        child: InkWell(
          borderRadius: BorderRadius.circular(AppTokens.rMd),
          onTap: onTap,
          child: Container(
            height: 40,
            decoration: BoxDecoration(
              color: active ? c.accentSoft : Colors.transparent,
              borderRadius: BorderRadius.circular(AppTokens.rMd),
            ),
            alignment: Alignment.center,
            child: Icon(
              section.icon,
              size: 20,
              color: active ? c.accent : c.textMuted,
            ),
          ),
        ),
      ),
    );
  }
}

class _ConnectionDot extends ConsumerWidget {
  const _ConnectionDot();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final status = ref.watch(wsStatusProvider).value ?? WsStatus.disconnected;
    final (color, label) = switch (status) {
      WsStatus.connected => (AppTokens.success, 'Connected'),
      WsStatus.connecting => (AppTokens.warning, 'Connecting…'),
      WsStatus.disconnected => (AppTokens.danger, 'Offline'),
    };
    return Tooltip(
      message: 'Daemon: $label · open Diagnostics',
      child: InkWell(
        onTap: () => context.go('/diagnostics'),
        customBorder: const CircleBorder(),
        child: Padding(
          padding: const EdgeInsets.all(AppTokens.s8),
          child: Container(
            width: 10,
            height: 10,
            decoration: BoxDecoration(
              color: color,
              shape: BoxShape.circle,
              boxShadow: [
                BoxShadow(color: color.withValues(alpha: 0.5), blurRadius: 6),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
