import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:intl/intl.dart';
import '../../models/space_models.dart';
import '../../core/transport/connection.dart';
import '../../core/transport/ws_client.dart';
import '../../models/group.dart';
import '../../theme/tokens.dart';
import '../../widgets/section_scaffold.dart';
import '../chat/agent_states_provider.dart';
import '../chat/agents_provider.dart';
import '../chat/groups_provider.dart';
import '../chat/notifications.dart';
import '../cognitive/cognitive_screen.dart' show cogStatsProvider;
import '../dock/dispatch_provider.dart';
import '../plugins/plugins_screen.dart' show skillsProvider, mcpServersProvider;
import '../space/space_providers.dart';
import '../space/space_screen.dart'
    show showCreateEventDialog, showDayEventsDialog, showCreateNoteDialog;

/// Total wiki documents (sum of per-category counts from /api/wiki/stats).
final wikiDocCountProvider = FutureProvider<int>((ref) async {
  final r = await ref.read(apiClientProvider).get('/api/wiki/stats');
  final cats = (r is Map ? r['byCategory'] : null) as List? ?? const [];
  return cats
      .whereType<Map>()
      .fold<int>(0, (sum, c) => sum + ((c['count'] as num?)?.toInt() ?? 0));
});

class DashboardScreen extends ConsumerWidget {
  const DashboardScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final status = ref.watch(wsStatusProvider).value ?? WsStatus.disconnected;
    final online = status == WsStatus.connected;
    final groups = ref.watch(groupsProvider);
    final agents = ref.watch(agentsProvider).where((a) => !a.isSchedule).length;
    final wikiDocs = ref.watch(wikiDocCountProvider).valueOrNull;
    final memNodes =
        ref.watch(cogStatsProvider).valueOrNull?['nodes_total'] as num?;
    final skills = ref.watch(skillsProvider).valueOrNull?.length;
    final mcp = ref.watch(mcpServersProvider).valueOrNull?.length;
    final agentStates = ref.watch(agentStatesProvider);
    final dispatch = ref.watch(dispatchProvider);
    final unreadNotifs =
        ref.watch(notificationsProvider).where((n) => !n.read).toList();
    final unreadChats =
        groups.fold<int>(0, (sum, g) => sum + g.unread);
    final pinnedAppIds = ref.watch(pinnedAppsProvider);
    final pinnedApps = (ref.watch(spaceAppsProvider).valueOrNull ?? const [])
        .where((a) => pinnedAppIds.contains(a.id))
        .toList();

    final activeChats = agentStates.entries
        .where((e) => kActiveStates.contains(e.value))
        .length;
    final runningDispatch =
        dispatch.parents.where((p) => p.status == 'running').length;

    void refreshAll() {
      ref.invalidate(wikiDocCountProvider);
      ref.invalidate(cogStatsProvider);
      ref.invalidate(skillsProvider);
      ref.invalidate(mcpServersProvider);
    }

    return SectionScaffold(
      title: 'Dashboard',
      subtitle: 'Overview of your SenClaw agents and activity',
      actions: [
        OutlinedButton.icon(
          onPressed: refreshAll,
          icon: const Icon(Icons.refresh, size: 16),
          label: const Text('Refresh'),
        ),
      ],
      body: ListView(
        padding: const EdgeInsets.all(AppTokens.s24),
        children: [
          _HeroBanner(
            online: online,
            activeChats: activeChats,
          ),
          if (unreadNotifs.isNotEmpty || unreadChats > 0) ...[
            const SizedBox(height: AppTokens.s16),
            _NotificationsAlert(
              notifs: unreadNotifs,
              unreadChats: unreadChats,
            ),
          ],
          const SizedBox(height: AppTokens.s24),
          _StatsGrid(
            cards: [
              _StatData('Active agents', '$agents', Icons.badge_outlined,
                  AppTokens.brand, '/chat'),
              _StatData('Total chats', '${groups.length}',
                  Icons.forum_outlined, AppTokens.brandAlt, '/chat'),
              _StatData('Wiki documents', wikiDocs?.toString() ?? '…',
                  Icons.menu_book_outlined, AppTokens.cyan, '/wiki'),
              _StatData('Memory nodes', memNodes?.toString() ?? '…',
                  Icons.hub_outlined, AppTokens.success, '/cognitive'),
              _StatData('Skills', skills?.toString() ?? '…',
                  Icons.bolt_outlined, AppTokens.warning, '/plugins'),
              _StatData('MCP servers', mcp?.toString() ?? '…',
                  Icons.dns_outlined, AppTokens.brand, '/plugins'),
            ],
          ),
          const SizedBox(height: AppTokens.s24),
          LayoutBuilder(
            builder: (context, cns) {
              // Left column: Pinned apps → Recent chats → Schedules.
              final left = Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  if (pinnedApps.isNotEmpty) ...[
                    _PinnedApps(apps: pinnedApps),
                    const SizedBox(height: AppTokens.s16),
                  ],
                  _RecentChats(groups: groups),
                  const SizedBox(height: AppTokens.s16),
                  const _SchedulesPanel(),
                ],
              );
              // Right column: Mini calendar → Events → Live activity.
              final right = Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  const _MiniCalendar(),
                  const SizedBox(height: AppTokens.s16),
                  const _EventsPanel(),
                  const SizedBox(height: AppTokens.s16),
                  _ActivityPanel(
                    agentStates: agentStates,
                    groups: groups,
                    dispatch: dispatch,
                    runningDispatch: runningDispatch,
                  ),
                ],
              );
              if (cns.maxWidth < 820) {
                return Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    left,
                    const SizedBox(height: AppTokens.s16),
                    right,
                  ],
                );
              }
              return Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Expanded(flex: 3, child: left),
                  const SizedBox(width: AppTokens.s16),
                  Expanded(flex: 2, child: right),
                ],
              );
            },
          ),
        ],
      ),
    );
  }
}

// ── Hero banner ────────────────────────────────────────────────────────────

class _HeroBanner extends StatelessWidget {
  const _HeroBanner({
    required this.online,
    required this.activeChats,
  });
  final bool online;
  final int activeChats;

  String get _greeting {
    final h = DateTime.now().hour;
    if (h < 5) return 'Good night';
    if (h < 12) return 'Good morning';
    if (h < 18) return 'Good afternoon';
    return 'Good evening';
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Container(
      padding: const EdgeInsets.all(AppTokens.s24),
      decoration: BoxDecoration(
        borderRadius: BorderRadius.circular(AppTokens.rXl),
        border: Border.all(color: c.border),
        gradient: LinearGradient(
          begin: Alignment.topLeft,
          end: Alignment.bottomRight,
          colors: [
            AppTokens.brand.withValues(alpha: 0.18),
            AppTokens.brandAlt.withValues(alpha: 0.10),
            c.surface,
          ],
        ),
      ),
      child: Row(
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  '$_greeting 👋',
                  style: TextStyle(
                    color: c.textPrimary,
                    fontSize: 22,
                    fontWeight: FontWeight.w700,
                  ),
                ),
                const SizedBox(height: AppTokens.s4),
                Text(
                  activeChats > 0
                      ? '$activeChats agent${activeChats == 1 ? '' : 's'} working right now'
                      : 'Your agents are standing by',
                  style: TextStyle(color: c.textSecondary, fontSize: 13),
                ),
                const SizedBox(height: AppTokens.s16),
                Row(
                  children: [
                    FilledButton.icon(
                      onPressed: () => context.go('/chat'),
                      icon: const Icon(Icons.add_comment_outlined, size: 16),
                      label: const Text('New chat'),
                    ),
                    const SizedBox(width: AppTokens.s8),
                    OutlinedButton.icon(
                      onPressed: () => showCreateNoteDialog(context),
                      icon: const Icon(Icons.note_add_outlined, size: 16),
                      label: const Text('New note'),
                    ),
                    const SizedBox(width: AppTokens.s8),
                    OutlinedButton.icon(
                      onPressed: () => context.go('/wiki'),
                      icon: const Icon(Icons.menu_book_outlined, size: 16),
                      label: const Text('Open wiki'),
                    ),
                  ],
                ),
              ],
            ),
          ),
          const SizedBox(width: AppTokens.s16),
          _StatusPill(online: online),
        ],
      ),
    );
  }
}

class _StatusPill extends StatelessWidget {
  const _StatusPill({required this.online});
  final bool online;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final color = online ? AppTokens.success : AppTokens.danger;
    return Container(
      padding: const EdgeInsets.symmetric(
          horizontal: AppTokens.s16, vertical: AppTokens.s12),
      decoration: BoxDecoration(
        color: c.surface.withValues(alpha: 0.6),
        border: Border.all(color: c.border),
        borderRadius: BorderRadius.circular(AppTokens.rLg),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          _PulseDot(color: color),
          const SizedBox(width: AppTokens.s8),
          Text(
            online ? 'Online' : 'Offline',
            style:
                TextStyle(color: c.textPrimary, fontWeight: FontWeight.w600),
          ),
        ],
      ),
    );
  }
}

class _PulseDot extends StatelessWidget {
  const _PulseDot({required this.color});
  final Color color;
  @override
  Widget build(BuildContext context) {
    return Container(
      width: 10,
      height: 10,
      decoration: BoxDecoration(
        shape: BoxShape.circle,
        color: color,
        boxShadow: [
          BoxShadow(color: color.withValues(alpha: 0.5), blurRadius: 6),
        ],
      ),
    );
  }
}

// ── Unread notifications alert ──────────────────────────────────────────────

class _NotificationsAlert extends ConsumerWidget {
  const _NotificationsAlert({required this.notifs, required this.unreadChats});
  final List<AppNotification> notifs;
  final int unreadChats;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    const accent = AppTokens.warning;
    final total = notifs.length + (unreadChats > 0 ? 1 : 0);

    return Container(
      decoration: BoxDecoration(
        color: accent.withValues(alpha: 0.08),
        border: Border.all(color: accent.withValues(alpha: 0.45)),
        borderRadius: BorderRadius.circular(AppTokens.rLg),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(AppTokens.s20, AppTokens.s12,
                AppTokens.s12, AppTokens.s12),
            child: Row(
              children: [
                const Icon(Icons.notifications_active_outlined,
                    size: 16, color: accent),
                const SizedBox(width: AppTokens.s8),
                Text(
                  '$total unread notification${total == 1 ? '' : 's'}',
                  style: TextStyle(
                    color: c.textPrimary,
                    fontSize: 13,
                    fontWeight: FontWeight.w700,
                  ),
                ),
                const Spacer(),
                if (notifs.isNotEmpty)
                  TextButton(
                    onPressed: () =>
                        ref.read(notificationsProvider.notifier).clearAll(),
                    child: const Text('Mark all read'),
                  ),
              ],
            ),
          ),
          if (unreadChats > 0)
            _AlertRow(
              icon: Icons.forum_outlined,
              title: '$unreadChats unread chat message'
                  '${unreadChats == 1 ? '' : 's'}',
              detail: 'Across your conversations',
              onTap: () => context.go('/chat'),
            ),
          for (final n in notifs.take(5))
            _AlertRow(
              icon: Icons.circle_notifications_outlined,
              title: n.title,
              detail: n.detail,
              onTap: () =>
                  ref.read(notificationsProvider.notifier).markRead(n.id),
            ),
          if (notifs.length > 5)
            Padding(
              padding: const EdgeInsets.fromLTRB(AppTokens.s20, 0,
                  AppTokens.s20, AppTokens.s12),
              child: Text(
                '+${notifs.length - 5} more',
                style: TextStyle(color: c.textMuted, fontSize: 12),
              ),
            ),
        ],
      ),
    );
  }
}

class _AlertRow extends StatelessWidget {
  const _AlertRow({
    required this.icon,
    required this.title,
    required this.detail,
    required this.onTap,
  });
  final IconData icon;
  final String title;
  final String detail;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return InkWell(
      onTap: onTap,
      child: Padding(
        padding: const EdgeInsets.symmetric(
            horizontal: AppTokens.s20, vertical: AppTokens.s8),
        child: Row(
          children: [
            Icon(icon, size: 15, color: c.textSecondary),
            const SizedBox(width: AppTokens.s12),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    title,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      color: c.textPrimary,
                      fontSize: 13,
                      fontWeight: FontWeight.w600,
                    ),
                  ),
                  if (detail.isNotEmpty)
                    Text(
                      detail,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(color: c.textMuted, fontSize: 12),
                    ),
                ],
              ),
            ),
            Icon(Icons.close, size: 14, color: c.textMuted),
          ],
        ),
      ),
    );
  }
}

// ── Stats grid ───────────────────────────────────────────────────────────

class _StatData {
  const _StatData(this.label, this.value, this.icon, this.accent, this.route);
  final String label;
  final String value;
  final IconData icon;
  final Color accent;
  final String route;
}

class _StatsGrid extends StatelessWidget {
  const _StatsGrid({required this.cards});
  final List<_StatData> cards;

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, cns) {
        const gap = AppTokens.s12;
        const minWidth = 128.0; // shrink to one row until cards hit this floor
        final n = cards.length;
        // Pick the most columns (up to n) that keep each card ≥ minWidth, so all
        // cards stay on a single row as long as they can shrink to fit.
        var cols = n;
        while (cols > 1 &&
            (cns.maxWidth - gap * (cols - 1)) / cols < minWidth) {
          cols--;
        }
        final width = (cns.maxWidth - gap * (cols - 1)) / cols;
        return Wrap(
          spacing: gap,
          runSpacing: gap,
          children: [
            for (final d in cards)
              SizedBox(width: width, child: _StatCard(data: d)),
          ],
        );
      },
    );
  }
}

class _StatCard extends StatefulWidget {
  const _StatCard({required this.data});
  final _StatData data;
  @override
  State<_StatCard> createState() => _StatCardState();
}

class _StatCardState extends State<_StatCard> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final d = widget.data;
    return MouseRegion(
      cursor: SystemMouseCursors.click,
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: GestureDetector(
        onTap: () => context.go(d.route),
        child: AnimatedContainer(
          duration: const Duration(milliseconds: 120),
          padding: const EdgeInsets.all(AppTokens.s16),
          decoration: BoxDecoration(
            color: c.surface,
            border: Border.all(color: _hover ? d.accent : c.border),
            borderRadius: BorderRadius.circular(AppTokens.rLg),
            boxShadow: _hover
                ? [
                    BoxShadow(
                      color: d.accent.withValues(alpha: 0.18),
                      blurRadius: 16,
                      offset: const Offset(0, 4),
                    ),
                  ]
                : null,
          ),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  Container(
                    padding: const EdgeInsets.all(AppTokens.s8),
                    decoration: BoxDecoration(
                      color: d.accent.withValues(alpha: 0.14),
                      borderRadius: BorderRadius.circular(AppTokens.rMd),
                    ),
                    child: Icon(d.icon, color: d.accent, size: 18),
                  ),
                  const Spacer(),
                  Icon(Icons.arrow_outward,
                      size: 14,
                      color: _hover ? d.accent : c.textMuted),
                ],
              ),
              const SizedBox(height: AppTokens.s12),
              Text(
                d.value,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(
                  color: c.textPrimary,
                  fontSize: 22,
                  fontWeight: FontWeight.w700,
                ),
              ),
              Text(
                d.label,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(color: c.textMuted, fontSize: 12),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

// ── Panel chrome ───────────────────────────────────────────────────────────

class _Panel extends StatelessWidget {
  const _Panel({required this.title, required this.icon, required this.child});
  final String title;
  final IconData icon;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Container(
      decoration: BoxDecoration(
        color: c.surface,
        border: Border.all(color: c.border),
        borderRadius: BorderRadius.circular(AppTokens.rLg),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(AppTokens.s20, AppTokens.s16,
                AppTokens.s20, AppTokens.s12),
            child: Row(
              children: [
                Icon(icon, size: 16, color: c.textSecondary),
                const SizedBox(width: AppTokens.s8),
                Text(
                  title,
                  style: TextStyle(
                    color: c.textPrimary,
                    fontSize: 13,
                    fontWeight: FontWeight.w700,
                  ),
                ),
              ],
            ),
          ),
          Divider(height: 1, color: c.border),
          child,
        ],
      ),
    );
  }
}

class _EmptyHint extends StatelessWidget {
  const _EmptyHint(this.text);
  final String text;
  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Padding(
      padding: const EdgeInsets.all(AppTokens.s24),
      child: Center(
        child: Text(text,
            style: TextStyle(color: c.textMuted, fontSize: 12)),
      ),
    );
  }
}

// ── Pinned apps (quick launch) ───────────────────────────────────────────────

class _PinnedApps extends ConsumerWidget {
  const _PinnedApps({required this.apps});
  final List<SpaceApp> apps;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return _Panel(
      title: 'Pinned apps',
      icon: Icons.push_pin_outlined,
      child: apps.isEmpty
          ? const _EmptyHint('No pinned apps — right-click an app to pin it.')
          : Padding(
              padding: const EdgeInsets.all(AppTokens.s16),
              child: Wrap(
                spacing: AppTokens.s12,
                runSpacing: AppTokens.s12,
                children: [
                  for (final app in apps)
                    _PinnedAppChip(
                      app: app,
                      running:
                          ref.watch(runningAppsProvider).isRunning(app.id),
                      onTap: () {
                        ref.read(runningAppsProvider.notifier).open(app);
                        context.go('/apps');
                      },
                    ),
                ],
              ),
            ),
    );
  }
}

// ── Mini calendar ────────────────────────────────────────────────────────────

/// Compact month calendar for the current month with today highlighted and a
/// dot under days that have a Space event.
class _MiniCalendar extends ConsumerWidget {
  const _MiniCalendar();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final now = DateTime.now();
    final month = DateTime(now.year, now.month);
    final events = ref.watch(eventsProvider).valueOrNull ?? const <SpaceEvent>[];
    final eventDays = <String>{
      for (final e in events) '${e.start.year}-${e.start.month}-${e.start.day}',
    };

    // weekday(): Mon=1..Sun=7 → Sunday-first leading offset.
    final lead = month.weekday % 7;
    final gridStart = month.subtract(Duration(days: lead));
    const dow = ['S', 'M', 'T', 'W', 'T', 'F', 'S'];

    return _Panel(
      title: DateFormat('MMMM yyyy').format(now),
      icon: Icons.calendar_today_outlined,
      child: Padding(
        padding: const EdgeInsets.fromLTRB(
            AppTokens.s16, AppTokens.s12, AppTokens.s16, AppTokens.s16),
        child: Column(
          children: [
            Row(
              children: [
                for (final d in dow)
                  Expanded(
                    child: Center(
                      child: Text(d,
                          style: TextStyle(
                              color: c.textMuted,
                              fontSize: 11,
                              fontWeight: FontWeight.w600)),
                    ),
                  ),
              ],
            ),
            const SizedBox(height: AppTokens.s8),
            for (var w = 0; w < 6; w++)
              Padding(
                padding: const EdgeInsets.only(bottom: AppTokens.s4),
                child: Row(
                  children: [
                    for (var i = 0; i < 7; i++)
                      _MiniDay(
                        day: gridStart.add(Duration(days: w * 7 + i)),
                        month: month,
                        now: now,
                        eventDays: eventDays,
                      ),
                  ],
                ),
              ),
          ],
        ),
      ),
    );
  }
}

class _MiniDay extends StatelessWidget {
  const _MiniDay({
    required this.day,
    required this.month,
    required this.now,
    required this.eventDays,
  });
  final DateTime day;
  final DateTime month;
  final DateTime now;
  final Set<String> eventDays;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final inMonth = day.month == month.month;
    final isToday =
        day.year == now.year && day.month == now.month && day.day == now.day;
    final hasEvent = eventDays.contains('${day.year}-${day.month}-${day.day}');
    return Expanded(
      child: Center(
        child: InkWell(
          onTap: () => showDayEventsDialog(context, day),
          customBorder: const CircleBorder(),
          child: SizedBox(
          width: 30,
          height: 30,
          child: Stack(
            alignment: Alignment.center,
            children: [
              Container(
                decoration: BoxDecoration(
                  shape: BoxShape.circle,
                  color: isToday ? AppTokens.brand : Colors.transparent,
                ),
                alignment: Alignment.center,
                child: Text(
                  '${day.day}',
                  style: TextStyle(
                    fontSize: 12,
                    fontWeight: isToday ? FontWeight.w700 : FontWeight.w400,
                    color: isToday
                        ? Colors.white
                        : (inMonth ? c.textSecondary : c.textMuted),
                  ),
                ),
              ),
              if (hasEvent && !isToday)
                Positioned(
                  bottom: 3,
                  child: Container(
                    width: 4,
                    height: 4,
                    decoration: const BoxDecoration(
                        color: AppTokens.cyan, shape: BoxShape.circle),
                  ),
                ),
            ],
          ),
        ),
        ),
      ),
    );
  }
}

class _PinnedAppChip extends StatelessWidget {
  const _PinnedAppChip({
    required this.app,
    required this.running,
    required this.onTap,
  });
  final SpaceApp app;
  final bool running;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return InkWell(
      onTap: onTap,
      borderRadius: BorderRadius.circular(AppTokens.rXl),
      child: Container(
        width: 100,
        height: 100,
        padding: const EdgeInsets.all(AppTokens.s8),
        decoration: BoxDecoration(
          color: c.bg,
          border: Border.all(
              color: running ? c.accent : c.border, width: running ? 1.5 : 1),
          borderRadius: BorderRadius.circular(AppTokens.rXl),
        ),
        child: Stack(
          children: [
            Center(
              child: Column(
                mainAxisAlignment: MainAxisAlignment.center,
                children: [
                  Text(app.icon, style: const TextStyle(fontSize: 30)),
                  const SizedBox(height: AppTokens.s6),
                  Text(
                    app.name,
                    maxLines: 2,
                    textAlign: TextAlign.center,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      color: c.textPrimary,
                      fontSize: 12,
                      fontWeight: FontWeight.w500,
                    ),
                  ),
                ],
              ),
            ),
            if (running)
              const Positioned(
                top: 0,
                right: 0,
                child: SizedBox(
                  width: 8,
                  height: 8,
                  child: DecoratedBox(
                    decoration: BoxDecoration(
                        color: AppTokens.success, shape: BoxShape.circle),
                  ),
                ),
              ),
          ],
        ),
      ),
    );
  }
}

// ── Upcoming events ──────────────────────────────────────────────────────────

class _EventsPanel extends ConsumerWidget {
  const _EventsPanel();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final async = ref.watch(eventsProvider);

    Widget body;
    if (async.isLoading && !async.hasValue) {
      body = const Padding(
        padding: EdgeInsets.all(AppTokens.s24),
        child: Center(
          child: SizedBox(
              width: 18, height: 18, child: CircularProgressIndicator()),
        ),
      );
    } else {
      final now = DateTime.now();
      final upcoming = (async.valueOrNull ?? const <SpaceEvent>[])
          .where((e) => e.start.isAfter(now))
          .toList()
        ..sort((a, b) => a.startAt.compareTo(b.startAt));
      final top = upcoming.take(6).toList();
      if (top.isEmpty) {
        body = const _EmptyHint('No upcoming events.');
      } else {
        body = Column(
          children: [
            for (var i = 0; i < top.length; i++) ...[
              if (i > 0) Divider(height: 1, color: c.border),
              _EventRow(event: top[i], isNext: i == 0),
            ],
          ],
        );
      }
    }

    return _Panel(
      title: 'Upcoming events',
      icon: Icons.event_outlined,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          body,
          Divider(height: 1, color: c.border),
          Padding(
            padding: const EdgeInsets.symmetric(
                horizontal: AppTokens.s8, vertical: AppTokens.s8),
            child: Row(
              children: [
                TextButton.icon(
                  onPressed: () => context.go('/space?tab=calendar'),
                  icon: const Icon(Icons.calendar_month_outlined, size: 14),
                  label: const Text('Open calendar'),
                ),
                const Spacer(),
                TextButton.icon(
                  onPressed: () => showCreateEventDialog(context),
                  icon: const Icon(Icons.add, size: 14),
                  label: const Text('New event'),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class _EventRow extends StatelessWidget {
  const _EventRow({required this.event, required this.isNext});
  final SpaceEvent event;
  final bool isNext;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final accent = isNext ? AppTokens.cyan : c.textMuted;
    return Padding(
      padding: const EdgeInsets.symmetric(
          horizontal: AppTokens.s20, vertical: AppTokens.s12),
      child: Row(
        children: [
          Icon(isNext ? Icons.event_available : Icons.event,
              size: 16, color: isNext ? AppTokens.cyan : AppTokens.brandAlt),
          const SizedBox(width: AppTokens.s12),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  event.title,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                    color: c.textPrimary,
                    fontSize: 13,
                    fontWeight: FontWeight.w600,
                  ),
                ),
                Text(
                  event.allDay
                      ? '${DateFormat('EEE d MMM').format(event.start)} · All day'
                      : DateFormat('EEE d MMM · HH:mm').format(event.start),
                  style: TextStyle(color: c.textMuted, fontSize: 12),
                ),
              ],
            ),
          ),
          const SizedBox(width: AppTokens.s8),
          Container(
            padding: const EdgeInsets.symmetric(
                horizontal: AppTokens.s8, vertical: 2),
            decoration: BoxDecoration(
              color: accent.withValues(alpha: isNext ? 0.16 : 0.0),
              borderRadius: BorderRadius.circular(AppTokens.rFull),
            ),
            child: Text(
              _untilLabel(event.start),
              style: TextStyle(
                  color: accent,
                  fontSize: 11,
                  fontWeight: isNext ? FontWeight.w700 : FontWeight.w400),
            ),
          ),
        ],
      ),
    );
  }
}

// ── Upcoming schedules ───────────────────────────────────────────────────────

class _SchedulesPanel extends ConsumerWidget {
  const _SchedulesPanel();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final async = ref.watch(schedulesProvider);

    Widget body;
    if (async.isLoading && !async.hasValue) {
      body = const Padding(
        padding: EdgeInsets.all(AppTokens.s24),
        child: Center(
          child: SizedBox(
              width: 18, height: 18, child: CircularProgressIndicator()),
        ),
      );
    } else {
      final list = async.valueOrNull ?? const <SpaceSchedule>[];
      // Sort by next-run ascending; un-parseable / no next-run sink to bottom.
      final sorted = [...list]..sort((a, b) {
          final da = _parseTs(a.nextRun);
          final db = _parseTs(b.nextRun);
          if (da == null && db == null) return 0;
          if (da == null) return 1;
          if (db == null) return -1;
          return da.compareTo(db);
        });
      final top = sorted.take(6).toList();
      if (top.isEmpty) {
        body = const _EmptyHint('No schedules — create one in Space › Schedules.');
      } else {
        body = Column(
          children: [
            for (var i = 0; i < top.length; i++) ...[
              if (i > 0) Divider(height: 1, color: c.border),
              _ScheduleRow(schedule: top[i], isNext: i == 0),
            ],
          ],
        );
      }
    }

    return _Panel(
      title: 'Upcoming schedules',
      icon: Icons.schedule_outlined,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          body,
          Divider(height: 1, color: c.border),
          Padding(
            padding: const EdgeInsets.all(AppTokens.s12),
            child: Align(
              alignment: Alignment.centerLeft,
              child: TextButton.icon(
                onPressed: () => context.go('/space?tab=schedules'),
                icon: const Icon(Icons.open_in_new, size: 14),
                label: const Text('Manage schedules'),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _ScheduleRow extends StatelessWidget {
  const _ScheduleRow({required this.schedule, required this.isNext});
  final SpaceSchedule schedule;
  final bool isNext;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final label = schedule.label.split('\n').first;
    final next = _parseTs(schedule.nextRun);
    final accent = isNext ? AppTokens.brand : c.textMuted;
    return Padding(
      padding: const EdgeInsets.symmetric(
          horizontal: AppTokens.s20, vertical: AppTokens.s12),
      child: Row(
        children: [
          Icon(isNext ? Icons.alarm : Icons.schedule,
              size: 16, color: isNext ? AppTokens.brand : AppTokens.brandAlt),
          const SizedBox(width: AppTokens.s12),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  label,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                    color: c.textPrimary,
                    fontSize: 13,
                    fontWeight: FontWeight.w600,
                  ),
                ),
                Text(
                  next == null
                      ? (schedule.nextRun ?? 'no upcoming run')
                      : DateFormat('EEE d MMM · HH:mm').format(next),
                  style: TextStyle(color: c.textMuted, fontSize: 12),
                ),
              ],
            ),
          ),
          const SizedBox(width: AppTokens.s8),
          if (next != null)
            Container(
              padding: const EdgeInsets.symmetric(
                  horizontal: AppTokens.s8, vertical: 2),
              decoration: BoxDecoration(
                color: accent.withValues(alpha: isNext ? 0.16 : 0.0),
                borderRadius: BorderRadius.circular(AppTokens.rFull),
              ),
              child: Text(
                _untilLabel(next),
                style: TextStyle(
                    color: accent,
                    fontSize: 11,
                    fontWeight: isNext ? FontWeight.w700 : FontWeight.w400),
              ),
            ),
        ],
      ),
    );
  }
}

/// Parse the daemon's next-run string (ISO-8601 or epoch ms) to local time.
DateTime? _parseTs(String? s) {
  if (s == null || s.isEmpty) return null;
  final asInt = int.tryParse(s);
  if (asInt != null) {
    return DateTime.fromMillisecondsSinceEpoch(asInt).toLocal();
  }
  return DateTime.tryParse(s)?.toLocal();
}

/// "in 5m" / "in 3h" / "in 2d" / "due" for the time until [t].
String _untilLabel(DateTime t) {
  final d = t.difference(DateTime.now());
  if (d.isNegative) return 'due';
  if (d.inMinutes < 1) return 'now';
  if (d.inMinutes < 60) return 'in ${d.inMinutes}m';
  if (d.inHours < 24) return 'in ${d.inHours}h';
  return 'in ${d.inDays}d';
}

// ── Recent chats ───────────────────────────────────────────────────────────

class _RecentChats extends ConsumerWidget {
  const _RecentChats({required this.groups});
  final List<GroupInfo> groups;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final recent = [...groups]
      ..sort((a, b) => (b.lastActivity ?? 0).compareTo(a.lastActivity ?? 0));
    final top = recent.take(6).toList();

    return _Panel(
      title: 'Recent chats',
      icon: Icons.forum_outlined,
      child: top.isEmpty
          ? const _EmptyHint('No chats yet — start one from the Chat tab.')
          : Column(
              children: [
                for (var i = 0; i < top.length; i++) ...[
                  if (i > 0) Divider(height: 1, color: c.border),
                  _ChatRow(
                    group: top[i],
                    onTap: () {
                      ref.read(selectedJidProvider.notifier).state =
                          top[i].jid;
                      context.go('/chat');
                    },
                  ),
                ],
              ],
            ),
    );
  }
}

class _ChatRow extends StatelessWidget {
  const _ChatRow({required this.group, required this.onTap});
  final GroupInfo group;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final isCode = group.groupType == 'code';
    return InkWell(
      onTap: onTap,
      child: Padding(
        padding: const EdgeInsets.symmetric(
            horizontal: AppTokens.s20, vertical: AppTokens.s12),
        child: Row(
          children: [
            Container(
              width: 32,
              height: 32,
              decoration: BoxDecoration(
                color: (isCode ? AppTokens.brandAlt : AppTokens.brand)
                    .withValues(alpha: 0.14),
                borderRadius: BorderRadius.circular(AppTokens.rMd),
              ),
              child: Icon(
                isCode ? Icons.terminal : Icons.chat_bubble_outline,
                size: 16,
                color: isCode ? AppTokens.brandAlt : AppTokens.brand,
              ),
            ),
            const SizedBox(width: AppTokens.s12),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    group.name,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      color: c.textPrimary,
                      fontSize: 13,
                      fontWeight: FontWeight.w600,
                    ),
                  ),
                  if ((group.lastMessage ?? '').isNotEmpty)
                    Text(
                      group.lastMessage!,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(color: c.textMuted, fontSize: 12),
                    ),
                ],
              ),
            ),
            const SizedBox(width: AppTokens.s8),
            if (group.unread > 0)
              Container(
                padding: const EdgeInsets.symmetric(
                    horizontal: AppTokens.s6, vertical: 1),
                decoration: BoxDecoration(
                  color: AppTokens.brand,
                  borderRadius: BorderRadius.circular(AppTokens.rFull),
                ),
                child: Text(
                  '${group.unread}',
                  style: const TextStyle(
                      color: Colors.white,
                      fontSize: 11,
                      fontWeight: FontWeight.w700),
                ),
              )
            else
              Text(
                _relativeTime(group.lastActivity),
                style: TextStyle(color: c.textMuted, fontSize: 11),
              ),
          ],
        ),
      ),
    );
  }
}

String _relativeTime(int? ms) {
  if (ms == null || ms == 0) return '';
  final d = DateTime.now().difference(DateTime.fromMillisecondsSinceEpoch(ms));
  if (d.inMinutes < 1) return 'now';
  if (d.inMinutes < 60) return '${d.inMinutes}m';
  if (d.inHours < 24) return '${d.inHours}h';
  return '${d.inDays}d';
}

// ── Activity panel ─────────────────────────────────────────────────────────

class _ActivityPanel extends StatelessWidget {
  const _ActivityPanel({
    required this.agentStates,
    required this.groups,
    required this.dispatch,
    required this.runningDispatch,
  });
  final Map<String, String> agentStates;
  final List<GroupInfo> groups;
  final DispatchState dispatch;
  final int runningDispatch;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final active = agentStates.entries
        .where((e) => kActiveStates.contains(e.value))
        .toList();
    String nameFor(String jid) =>
        groups.firstWhere((g) => g.jid == jid,
            orElse: () => GroupInfo(jid: jid, name: jid)).name;

    return _Panel(
      title: 'Live activity',
      icon: Icons.bolt_outlined,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (active.isEmpty && runningDispatch == 0)
            const _EmptyHint('All agents idle.')
          else ...[
            for (final e in active.take(6))
              _ActivityRow(
                color: AppTokens.success,
                label: nameFor(e.key),
                state: _prettyState(e.value),
              ),
            if (runningDispatch > 0)
              _ActivityRow(
                color: AppTokens.brandAlt,
                label: '$runningDispatch dispatch run'
                    '${runningDispatch == 1 ? '' : 's'} in progress',
                state: 'running',
              ),
          ],
          Divider(height: 1, color: c.border),
          Padding(
            padding: const EdgeInsets.all(AppTokens.s12),
            child: Align(
              alignment: Alignment.centerLeft,
              child: TextButton.icon(
                onPressed: () => context.go('/cowork'),
                icon: const Icon(Icons.dashboard_customize_outlined, size: 14),
                label: const Text('Open agent console'),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _ActivityRow extends StatelessWidget {
  const _ActivityRow({
    required this.color,
    required this.label,
    required this.state,
  });
  final Color color;
  final String label;
  final String state;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Padding(
      padding: const EdgeInsets.symmetric(
          horizontal: AppTokens.s20, vertical: AppTokens.s12),
      child: Row(
        children: [
          _PulseDot(color: color),
          const SizedBox(width: AppTokens.s12),
          Expanded(
            child: Text(
              label,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: TextStyle(color: c.textPrimary, fontSize: 13),
            ),
          ),
          Text(state,
              style: TextStyle(color: color, fontSize: 11)),
        ],
      ),
    );
  }
}

String _prettyState(String s) => switch (s) {
      'waiting_permission' => 'needs approval',
      'waiting_question' => 'needs input',
      _ => s,
    };
