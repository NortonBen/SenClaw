import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/i18n/l10n.dart';
import '../../core/prefs.dart';
import '../../models/group.dart';
import '../../theme/tokens.dart';
import '../workflow/workflow_session_pane.dart' show WorkflowSessionSection;
import 'agent_states_provider.dart';
import 'agents_provider.dart';
import 'groups_provider.dart';

/// The full sidebar session list, ported from the React `SessionList`:
/// New Chat + reload, Pinned section, organize/sort, collapsible buckets,
/// per-item pin/rename/copy/delete, and active-state dots.
class SessionList extends ConsumerStatefulWidget {
  const SessionList({super.key, required this.onNewChat});
  final VoidCallback onNewChat;

  @override
  ConsumerState<SessionList> createState() => _SessionListState();
}

class _SessionListState extends ConsumerState<SessionList> {
  late Set<String> _collapsed;
  String? _renaming;
  final _renameCtrl = TextEditingController();

  @override
  void initState() {
    super.initState();
    final p = ref.read(prefsHelperProvider);
    _collapsed = p.stringSet(kCollapsedKey);
  }

  @override
  void dispose() {
    _renameCtrl.dispose();
    super.dispose();
  }

  void _select(String jid) {
    ref.read(selectedJidProvider.notifier).state = jid;
  }

  void _toggleBucket(String key) {
    setState(() {
      _collapsed.contains(key) ? _collapsed.remove(key) : _collapsed.add(key);
    });
    ref.read(prefsHelperProvider).setStringSet(kCollapsedKey, _collapsed);
  }

  // ── Timestamp helpers (mirror the React sidebar) ───────────────────────
  int _jidCreatedAt(String jid) {
    final m = RegExp(r':([0-9a-z]{6,})(?:-[0-9a-z]{4,})?$', caseSensitive: false)
        .firstMatch(jid);
    if (m == null) return 0;
    final ms = int.tryParse(m.group(1)!, radix: 36) ?? 0;
    return ms > 0 ? ms : 0;
  }

  // Sort:updated → last message/activity time (NOT when the user last opened
  // the chat — opening must not reorder the list). Sort:created → jid time.
  // Sort:name still needs a timestamp for date buckets — use activity time.
  int _ts(GroupInfo g, SortMode sort) => sort == SortMode.created
      ? _jidCreatedAt(g.jid)
      : (g.lastActivity ?? _jidCreatedAt(g.jid));

  String _title(GroupInfo g) => g.name.isNotEmpty ? g.name : g.jid;

  /// "How long ago" the chat was last active: now / Nm / Nh / Nd / d/m.
  String _relTime(int ms) {
    if (ms <= 0) return '';
    final dt = DateTime.fromMillisecondsSinceEpoch(ms);
    final d = DateTime.now().difference(dt);
    if (d.inSeconds < 60) return context.tr('now');
    if (d.inMinutes < 60) return context.trArgs('{n}m', {'n': d.inMinutes});
    if (d.inHours < 24) return context.trArgs('{n}h', {'n': d.inHours});
    if (d.inDays < 7) return context.trArgs('{n}d', {'n': d.inDays});
    return '${dt.day}/${dt.month}';
  }

  (String, String) _bucket(GroupInfo g, GroupMode mode, int ts) {
    if (mode == GroupMode.project) {
      final f = (g.folder == null || g.folder!.isEmpty)
          ? context.tr('(unknown)')
          : g.folder!;
      return (f, f);
    }
    if (mode == GroupMode.none) return ('all', context.tr('Sessions'));
    if (ts == 0) return ('older', context.tr('Older'));
    final d = DateTime.fromMillisecondsSinceEpoch(ts);
    final now = DateTime.now();
    bool sameDay(DateTime a, DateTime b) =>
        a.year == b.year && a.month == b.month && a.day == b.day;
    if (sameDay(d, now)) return ('today', context.tr('Today'));
    if (sameDay(d, now.subtract(const Duration(days: 1)))) {
      return ('yesterday', context.tr('Yesterday'));
    }
    final diff = now.difference(d).inDays;
    if (diff <= 7) return ('past7', context.tr('Previous 7 days'));
    if (diff <= 30) return ('past30', context.tr('Previous 30 days'));
    return ('older', context.tr('Older'));
  }

  static const _chronoOrder = {
    'today': 0, 'yesterday': 1, 'past7': 2, 'past30': 3, 'older': 4,
  };

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final groups = ref.watch(groupsProvider);
    final pinned = ref.watch(pinnedProvider);
    final groupMode = ref.watch(groupModeProvider);
    final sort = ref.watch(sortProvider);
    final selected = ref.watch(selectedJidProvider);
    final states = ref.watch(agentStatesProvider);

    final pinnedGroups = groups.where((g) => pinned.contains(g.jid)).toList();
    final rest = groups.where((g) => !pinned.contains(g.jid)).toList();

    // Bucket the unpinned groups.
    final buckets = <String, ({String label, List<GroupInfo> items, int maxTs})>{};
    for (final g in rest) {
      final ts = _ts(g, sort);
      final (key, label) = _bucket(g, groupMode, ts);
      final b = buckets[key];
      if (b == null) {
        buckets[key] = (label: label, items: [g], maxTs: ts);
      } else {
        b.items.add(g);
        if (ts > b.maxTs) buckets[key] = (label: b.label, items: b.items, maxTs: ts);
      }
    }
    for (final b in buckets.values) {
      if (sort == SortMode.name) {
        b.items.sort((a, x) =>
            _title(a).toLowerCase().compareTo(_title(x).toLowerCase()));
      } else {
        b.items.sort((a, x) => _ts(x, sort).compareTo(_ts(a, sort)));
      }
    }
    final ordered = buckets.entries.toList()
      ..sort((a, b) {
        switch (groupMode) {
          case GroupMode.date:
            return (_chronoOrder[a.key] ?? 99).compareTo(_chronoOrder[b.key] ?? 99);
          case GroupMode.none:
            return 0;
          case GroupMode.project:
            // Project buckets follow the sort mode: A–Z for name, else the
            // bucket with the freshest session first.
            return sort == SortMode.name
                ? a.value.label.toLowerCase().compareTo(b.value.label.toLowerCase())
                : b.value.maxTs.compareTo(a.value.maxTs);
        }
      });

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        // New chat + reload
        Padding(
          padding: const EdgeInsets.fromLTRB(
              AppTokens.s12, AppTokens.s12, AppTokens.s12, AppTokens.s8),
          child: Row(
            children: [
              Expanded(
                child: FilledButton.icon(
                  onPressed: widget.onNewChat,
                  icon: const Icon(Icons.add_rounded, size: 18),
                  label: Text(context.tr('New Session')),
                  style: FilledButton.styleFrom(
                    backgroundColor: c.accent,
                    foregroundColor: Colors.white,
                    elevation: 0,
                    padding: const EdgeInsets.symmetric(
                        vertical: AppTokens.s12),
                    shape: RoundedRectangleBorder(
                        borderRadius: BorderRadius.circular(AppTokens.rXl)),
                    textStyle: const TextStyle(
                        fontSize: 13, fontWeight: FontWeight.w600),
                  ),
                ),
              ),
              const SizedBox(width: AppTokens.s8),
              IconButton(
                tooltip: context.tr('Reload chats'),
                icon: const Icon(Icons.refresh, size: 18),
                onPressed: () => ref.read(groupsProvider.notifier).refresh(),
              ),
            ],
          ),
        ),
        Expanded(
          child: groups.isEmpty
              ? ListView(
                  padding: const EdgeInsets.only(bottom: AppTokens.s12),
                  children: [
                    const WorkflowSessionSection(),
                    Padding(
                      padding: const EdgeInsets.only(top: AppTokens.s24),
                      child: Text(
                          context.tr(
                              'No chats yet.\nClick + New Session above.'),
                          textAlign: TextAlign.center,
                          style:
                              TextStyle(color: c.textMuted, fontSize: 12)),
                    ),
                  ],
                )
              : ListView(
                  padding: const EdgeInsets.only(bottom: AppTokens.s12),
                  children: [
                    if (pinnedGroups.isNotEmpty) ...[
                      _sectionLabel('Pinned'),
                      for (final g in pinnedGroups)
                        _item(g, selected, states, pinned),
                    ],
                    // Workflow runs surface as sessions too — selecting one
                    // shows its flow activity in place of the chat pane.
                    const WorkflowSessionSection(),
                    _organizeHeader(groupMode),
                    for (final entry in ordered)
                      _bucketView(entry.key, entry.value.label,
                          entry.value.items, groupMode, selected, states, pinned),
                  ],
                ),
        ),
      ],
    );
  }

  Widget _sectionLabel(String label) => Padding(
        padding: const EdgeInsets.fromLTRB(
            AppTokens.s16, AppTokens.s8, AppTokens.s16, AppTokens.s4),
        child: Text(context.tr(label).toUpperCase(),
            style: TextStyle(
              color: context.colors.textMuted,
              fontSize: 11,
              fontWeight: FontWeight.w700,
              letterSpacing: 1.2,
            )),
      );

  Widget _organizeHeader(GroupMode groupMode) {
    final c = context.colors;
    // Date mode groups by time buckets below, so no redundant wrapper label
    // here — keep just the group/sort menu.
    final title = groupMode == GroupMode.none
        ? context.tr('Sessions')
        : groupMode == GroupMode.date
            ? ''
            : context.tr('Projects');
    return Padding(
      padding: const EdgeInsets.fromLTRB(
          AppTokens.s16, AppTokens.s8, AppTokens.s8, AppTokens.s4),
      child: Row(
        children: [
          Expanded(
            child: Text(title.toUpperCase(),
                style: TextStyle(
                  color: c.textMuted,
                  fontSize: 11,
                  fontWeight: FontWeight.w700,
                  letterSpacing: 1.2,
                )),
          ),
          _OrganizeMenu(),
        ],
      ),
    );
  }

  Widget _bucketView(String key, String label, List<GroupInfo> items,
      GroupMode groupMode, String? selected, Map<String, String> states,
      Set<String> pinned) {
    final c = context.colors;
    final collapsedKey = '${groupMode.name}:$key';
    final isCollapsed = _collapsed.contains(collapsedKey);
    final isProject = groupMode == GroupMode.project;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        InkWell(
          onTap: () => _toggleBucket(collapsedKey),
          child: Padding(
            padding: const EdgeInsets.fromLTRB(
                AppTokens.s16, AppTokens.s8, AppTokens.s16, AppTokens.s4),
            child: Row(
              children: [
                Icon(isCollapsed ? Icons.chevron_right : Icons.expand_more,
                    size: 14, color: c.textMuted),
                if (isProject) ...[
                  const SizedBox(width: 2),
                  Icon(Icons.folder_outlined, size: 12, color: c.textMuted),
                ],
                const SizedBox(width: AppTokens.s4),
                Expanded(
                  child: Text(label.toUpperCase(),
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(
                        color: c.textMuted,
                        fontSize: 11,
                        fontWeight: FontWeight.w700,
                        letterSpacing: 1,
                      )),
                ),
                Text('${items.length}',
                    style: TextStyle(color: c.textMuted, fontSize: 11)),
              ],
            ),
          ),
        ),
        if (!isCollapsed)
          for (final g in items) _item(g, selected, states, pinned),
      ],
    );
  }

  Widget _item(GroupInfo g, String? selected, Map<String, String> states,
      Set<String> pinned) {
    final c = context.colors;
    final isSelected = g.jid == selected;
    final state = states[g.jid] ?? 'idle';
    final isActive = kActiveStates.contains(state);
    final isRenaming = _renaming == g.jid;
    final isPinned = pinned.contains(g.jid);

    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: AppTokens.s8, vertical: 1),
      child: InkWell(
        borderRadius: BorderRadius.circular(AppTokens.rMd),
        onTap: isRenaming ? null : () => _select(g.jid),
        child: Container(
          padding: const EdgeInsets.symmetric(
              horizontal: AppTokens.s12, vertical: 3),
          decoration: BoxDecoration(
            color: isSelected ? c.accentSoft : Colors.transparent,
            borderRadius: BorderRadius.circular(AppTokens.rMd),
          ),
          child: Row(
            children: [
              Container(
                width: 7,
                height: 7,
                decoration: BoxDecoration(
                  shape: BoxShape.circle,
                  color: isActive
                      ? AppTokens.warning
                      : isSelected
                          ? c.accent
                          : c.textMuted.withValues(alpha: 0.5),
                ),
              ),
              const SizedBox(width: AppTokens.s8),
              Expanded(
                child: isRenaming
                    ? TextField(
                        controller: _renameCtrl,
                        autofocus: true,
                        style: TextStyle(color: c.textPrimary, fontSize: 14),
                        decoration: const InputDecoration(
                          isDense: true,
                          contentPadding: EdgeInsets.symmetric(vertical: 4),
                        ),
                        onSubmitted: (v) {
                          ref.read(groupsProvider.notifier).rename(g.jid, v);
                          setState(() => _renaming = null);
                        },
                      )
                    : Text(
                        g.name.isNotEmpty ? g.name : g.jid,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: TextStyle(
                          color: isSelected ? c.accent : c.textPrimary,
                          fontSize: 14,
                          fontWeight: isSelected ? FontWeight.w600 : FontWeight.w400,
                        ),
                      ),
              ),
              if (!isRenaming) ...[
                Text(
                  _relTime(g.lastActivity ?? _jidCreatedAt(g.jid)),
                  style: TextStyle(color: c.textMuted, fontSize: 11),
                ),
                const SizedBox(width: AppTokens.s4),
              ],
              _itemMenu(g, isPinned),
            ],
          ),
        ),
      ),
    );
  }

  Widget _itemMenu(GroupInfo g, bool isPinned) {
    final c = context.colors;
    PopupMenuItem<String> item(
            String value, IconData icon, String label,
            {Color? color}) =>
        PopupMenuItem(
          value: value,
          height: 42,
          child: Row(children: [
            Icon(icon, size: 17, color: color ?? c.textSecondary),
            const SizedBox(width: AppTokens.s12),
            Text(context.tr(label),
                style: TextStyle(
                    color: color ?? c.textPrimary,
                    fontSize: 14,
                    fontWeight: FontWeight.w500)),
          ]),
        );
    return PopupMenuButton<String>(
      tooltip: '',
      padding: EdgeInsets.zero,
      iconSize: 16,
      // Material's default 48x48 tap target dictates the row height — shrink
      // it so list rows stay text-sized and more sessions fit on screen.
      constraints: const BoxConstraints(),
      style: const ButtonStyle(
        minimumSize: WidgetStatePropertyAll(Size(24, 24)),
        fixedSize: WidgetStatePropertyAll(Size(24, 24)),
        padding: WidgetStatePropertyAll(EdgeInsets.zero),
        tapTargetSize: MaterialTapTargetSize.shrinkWrap,
      ),
      icon: Icon(Icons.more_horiz, size: 16, color: c.textMuted),
      color: c.surface,
      elevation: 12,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(AppTokens.rLg),
        side: BorderSide(color: c.border),
      ),
      onSelected: (key) {
        switch (key) {
          case 'pin':
            ref.read(pinnedProvider.notifier).toggle(g.jid);
          case 'rename':
            _renameCtrl.text = g.name;
            setState(() => _renaming = g.jid);
          case 'copy':
            Clipboard.setData(ClipboardData(text: g.jid));
          case 'delete':
            ref.read(groupsProvider.notifier).delete(g.jid);
            if (ref.read(selectedJidProvider) == g.jid) {
              ref.read(selectedJidProvider.notifier).state = null;
            }
        }
      },
      itemBuilder: (_) => [
        item('pin', isPinned ? Icons.push_pin : Icons.push_pin_outlined,
            isPinned ? 'Unpin' : 'Pin'),
        item('rename', Icons.edit_outlined, 'Rename'),
        item('copy', Icons.copy_outlined, 'Copy ID'),
        const PopupMenuDivider(),
        item('delete', Icons.delete_outline, 'Delete',
            color: AppTokens.danger),
      ],
    );
  }
}

/// Group-by + sort-by dropdown: two clearly labeled sections with one
/// radio-style choice each, instead of the old flat 6-item organize menu.
class _OrganizeMenu extends ConsumerWidget {
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final groupMode = ref.watch(groupModeProvider);
    final sort = ref.watch(sortProvider);
    final c = context.colors;
    return PopupMenuButton<String>(
      tooltip: context.tr('Group & sort'),
      iconSize: 16,
      icon: Icon(Icons.tune_rounded, size: 15, color: c.textMuted),
      color: c.surface,
      elevation: 12,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(AppTokens.rLg),
        side: BorderSide(color: c.border),
      ),
      onSelected: (k) {
        switch (k) {
          case 'g:project':
            ref.read(groupModeProvider.notifier).set(GroupMode.project);
          case 'g:date':
            ref.read(groupModeProvider.notifier).set(GroupMode.date);
          case 'g:none':
            ref.read(groupModeProvider.notifier).set(GroupMode.none);
          case 's:updated':
            ref.read(sortProvider.notifier).set(SortMode.updated);
          case 's:created':
            ref.read(sortProvider.notifier).set(SortMode.created);
          case 's:name':
            ref.read(sortProvider.notifier).set(SortMode.name);
        }
      },
      itemBuilder: (ctx) {
        final cc = ctx.colors;
        PopupMenuItem<String> header(String label) => PopupMenuItem(
              enabled: false,
              height: 30,
              child: Text(ctx.tr(label).toUpperCase(),
                  style: TextStyle(
                    color: cc.textMuted,
                    fontSize: 10.5,
                    fontWeight: FontWeight.w700,
                    letterSpacing: 1.1,
                  )),
            );
        PopupMenuItem<String> option(
                String value, IconData icon, String label, bool on) =>
            PopupMenuItem(
              value: value,
              height: 38,
              child: Row(children: [
                Icon(icon, size: 16, color: on ? cc.accent : cc.textSecondary),
                const SizedBox(width: AppTokens.s12),
                Expanded(
                  child: Text(ctx.tr(label),
                      style: TextStyle(
                        fontSize: 13.5,
                        color: on ? cc.accent : cc.textPrimary,
                        fontWeight: on ? FontWeight.w600 : FontWeight.w400,
                      )),
                ),
                if (on) Icon(Icons.check_rounded, size: 16, color: cc.accent),
              ]),
            );
        return [
          header('Group by'),
          option('g:project', Icons.folder_outlined, 'Project',
              groupMode == GroupMode.project),
          option('g:date', Icons.schedule_outlined, 'Date',
              groupMode == GroupMode.date),
          option('g:none', Icons.menu_rounded, 'None',
              groupMode == GroupMode.none),
          const PopupMenuDivider(),
          header('Sort by'),
          option('s:updated', Icons.history_rounded, 'Recent activity',
              sort == SortMode.updated),
          option('s:created', Icons.add_circle_outline, 'Created',
              sort == SortMode.created),
          option('s:name', Icons.sort_by_alpha_rounded, 'Name',
              sort == SortMode.name),
        ];
      },
    );
  }
}
