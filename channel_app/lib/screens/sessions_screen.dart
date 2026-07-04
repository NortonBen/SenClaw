import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../core/prefs.dart';
import '../models/session_model.dart';
import '../services/relay_manager.dart';
import '../services/sessions_provider.dart';
import '../services/language_service.dart';
import '../theme/tokens.dart';
import 'agent_select_screen.dart';

/// Full session manager, ported from the desktop `SessionList`: New Session,
/// Pinned section, group-by/sort, collapsible buckets, and per-item
/// pin/rename/delete. Selecting a session activates it and returns to chat.
class SessionsScreen extends ConsumerStatefulWidget {
  const SessionsScreen({super.key});

  @override
  ConsumerState<SessionsScreen> createState() => _SessionsScreenState();
}

class _SessionsScreenState extends ConsumerState<SessionsScreen> {
  late Set<String> _collapsed;
  String? _renaming;
  final _renameCtrl = TextEditingController();

  @override
  void initState() {
    super.initState();
    _collapsed = ref.read(prefsHelperProvider).stringSet(kCollapsedKey);
    // Make sure the list is fresh when the screen opens.
    ref.read(sessionsProvider.notifier).refresh();
  }

  @override
  void dispose() {
    _renameCtrl.dispose();
    super.dispose();
  }

  void _toggleBucket(String key) {
    setState(() {
      _collapsed.contains(key) ? _collapsed.remove(key) : _collapsed.add(key);
    });
    ref.read(prefsHelperProvider).setStringSet(kCollapsedKey, _collapsed);
  }

  /// Open [s]: mark it active server-side, record it as the UI selection, and
  /// pop back to the chat.
  void _open(SessionInfo s) {
    ref.read(sessionsProvider.notifier).select(s.jid, folder: s.folder);
    ref.read(selectedSessionJidProvider.notifier).state = s.jid;
    Navigator.of(context).pop();
  }

  /// Create a new session with a chosen agent, then return to chat following
  /// the daemon's new active session (selection is cleared so chat follows it).
  Future<void> _newSession() async {
    final agents = RelayManager().agents;
    AgentFolderName? pick;
    if (agents.isEmpty) {
      pick = const AgentFolderName('', 'New session');
    } else if (agents.length == 1) {
      pick = AgentFolderName(agents.first.folder, agents.first.name);
    } else {
      final chosen = await AgentSelectScreen.show(context, agents: agents);
      if (chosen == null) return;
      pick = AgentFolderName(chosen.folder, chosen.name);
    }
    ref
        .read(sessionsProvider.notifier)
        .create(folder: pick.folder, name: pick.name);
    // Clear the explicit selection so the chat follows the freshly-active
    // session the daemon just created.
    ref.read(selectedSessionJidProvider.notifier).state = null;
    if (mounted) Navigator.of(context).pop();
  }

  // ── Time helpers ───────────────────────────────────────────────────────
  int _ts(SessionInfo s) => s.lastActivity ?? 0;

  String _relTime(int ms) {
    if (ms <= 0) return '';
    final dt = DateTime.fromMillisecondsSinceEpoch(ms);
    final d = DateTime.now().difference(dt);
    if (d.inSeconds < 60) return tr('vừa xong', 'now');
    if (d.inMinutes < 60) return '${d.inMinutes}m';
    if (d.inHours < 24) return '${d.inHours}h';
    if (d.inDays < 7) return '${d.inDays}d';
    return '${dt.day}/${dt.month}';
  }

  (String, String) _bucket(SessionInfo s, GroupMode mode, int ts) {
    if (mode == GroupMode.agent) {
      final f = s.folder.isEmpty ? '(none)' : s.folder;
      return (f, f);
    }
    if (mode == GroupMode.none) return ('all', tr('Phiên', 'Sessions'));
    if (ts == 0) return ('older', tr('Cũ hơn', 'Older'));
    final d = DateTime.fromMillisecondsSinceEpoch(ts);
    final now = DateTime.now();
    bool sameDay(DateTime a, DateTime b) =>
        a.year == b.year && a.month == b.month && a.day == b.day;
    if (sameDay(d, now)) return ('today', tr('Hôm nay', 'Today'));
    if (sameDay(d, now.subtract(const Duration(days: 1)))) {
      return ('yesterday', tr('Hôm qua', 'Yesterday'));
    }
    final diff = now.difference(d).inDays;
    if (diff <= 7) return ('past7', tr('7 ngày trước', 'Previous 7 days'));
    if (diff <= 30) return ('past30', tr('30 ngày trước', 'Previous 30 days'));
    return ('older', tr('Cũ hơn', 'Older'));
  }

  static const _chronoOrder = {
    'today': 0, 'yesterday': 1, 'past7': 2, 'past30': 3, 'older': 4,
  };

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final sessions = ref.watch(sessionsProvider);
    final pinned = ref.watch(pinnedProvider);
    final groupMode = ref.watch(groupModeProvider);
    final sort = ref.watch(sortProvider);
    final selected = ref.watch(selectedSessionJidProvider);

    final pinnedItems =
        sessions.where((s) => pinned.contains(s.jid)).toList();
    final rest = sessions.where((s) => !pinned.contains(s.jid)).toList();

    final buckets =
        <String, ({String label, List<SessionInfo> items, int maxTs})>{};
    for (final s in rest) {
      final ts = _ts(s);
      final (key, label) = _bucket(s, groupMode, ts);
      final b = buckets[key];
      if (b == null) {
        buckets[key] = (label: label, items: [s], maxTs: ts);
      } else {
        b.items.add(s);
        if (ts > b.maxTs) {
          buckets[key] = (label: b.label, items: b.items, maxTs: ts);
        }
      }
    }
    for (final b in buckets.values) {
      if (sort == SortMode.name) {
        b.items.sort((a, x) =>
            a.title.toLowerCase().compareTo(x.title.toLowerCase()));
      } else {
        b.items.sort((a, x) => _ts(x).compareTo(_ts(a)));
      }
    }
    final ordered = buckets.entries.toList()
      ..sort((a, b) {
        switch (groupMode) {
          case GroupMode.date:
            return (_chronoOrder[a.key] ?? 99)
                .compareTo(_chronoOrder[b.key] ?? 99);
          case GroupMode.none:
            return 0;
          case GroupMode.agent:
            return sort == SortMode.name
                ? a.value.label
                    .toLowerCase()
                    .compareTo(b.value.label.toLowerCase())
                : b.value.maxTs.compareTo(a.value.maxTs);
        }
      });

    return Scaffold(
      backgroundColor: c.bg,
      appBar: AppBar(
        backgroundColor: c.surface,
        elevation: 0,
        title: Text(tr('Phiên trò chuyện', 'Sessions'),
            style: TextStyle(color: c.textPrimary, fontSize: 16)),
        iconTheme: IconThemeData(color: c.textSecondary),
        actions: [
          IconButton(
            tooltip: tr('Tải lại', 'Reload'),
            icon: Icon(Icons.refresh, color: c.textMuted),
            onPressed: () => ref.read(sessionsProvider.notifier).refresh(),
          ),
        ],
      ),
      body: SafeArea(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(
                  AppTokens.s12, AppTokens.s12, AppTokens.s12, AppTokens.s8),
              child: FilledButton.icon(
                onPressed: _newSession,
                icon: const Icon(Icons.add_rounded, size: 18),
                label: Text(tr('Phiên mới', 'New Session')),
                style: FilledButton.styleFrom(
                  backgroundColor: c.accent,
                  foregroundColor: Colors.white,
                  elevation: 0,
                  padding:
                      const EdgeInsets.symmetric(vertical: AppTokens.s12),
                  shape: RoundedRectangleBorder(
                      borderRadius: BorderRadius.circular(AppTokens.rXl)),
                  textStyle: const TextStyle(
                      fontSize: 13, fontWeight: FontWeight.w600),
                ),
              ),
            ),
            Expanded(
              child: sessions.isEmpty
                  ? Center(
                      child: Text(
                        tr('Chưa có phiên nào.\nNhấn + Phiên mới ở trên.',
                            'No sessions yet.\nTap + New Session above.'),
                        textAlign: TextAlign.center,
                        style: TextStyle(color: c.textMuted, fontSize: 12),
                      ),
                    )
                  : ListView(
                      padding: const EdgeInsets.only(bottom: AppTokens.s16),
                      children: [
                        if (pinnedItems.isNotEmpty) ...[
                          _sectionLabel(tr('Đã ghim', 'Pinned')),
                          for (final s in pinnedItems)
                            _item(s, selected, pinned),
                        ],
                        _organizeHeader(groupMode),
                        for (final entry in ordered)
                          _bucketView(entry.key, entry.value.label,
                              entry.value.items, groupMode, selected, pinned),
                      ],
                    ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _sectionLabel(String label) => Padding(
        padding: const EdgeInsets.fromLTRB(
            AppTokens.s16, AppTokens.s8, AppTokens.s16, AppTokens.s4),
        child: Text(label.toUpperCase(),
            style: TextStyle(
              color: context.colors.textMuted,
              fontSize: 11,
              fontWeight: FontWeight.w700,
              letterSpacing: 1.2,
            )),
      );

  Widget _organizeHeader(GroupMode groupMode) {
    final c = context.colors;
    final title = groupMode == GroupMode.none
        ? tr('Phiên', 'Sessions')
        : groupMode == GroupMode.date
            ? ''
            : tr('Theo agent', 'By agent');
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
          const _OrganizeMenu(),
        ],
      ),
    );
  }

  Widget _bucketView(String key, String label, List<SessionInfo> items,
      GroupMode groupMode, String? selected, Set<String> pinned) {
    final c = context.colors;
    final collapsedKey = '${groupMode.name}:$key';
    final isCollapsed = _collapsed.contains(collapsedKey);
    final isAgent = groupMode == GroupMode.agent;
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
                if (isAgent) ...[
                  const SizedBox(width: 2),
                  Icon(Icons.smart_toy_outlined, size: 12, color: c.textMuted),
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
          for (final s in items) _item(s, selected, pinned),
      ],
    );
  }

  Widget _item(SessionInfo s, String? selected, Set<String> pinned) {
    final c = context.colors;
    final isSelected = s.jid == selected;
    final isRenaming = _renaming == s.jid;
    final isPinned = pinned.contains(s.jid);

    return Padding(
      padding:
          const EdgeInsets.symmetric(horizontal: AppTokens.s8, vertical: 1),
      child: InkWell(
        borderRadius: BorderRadius.circular(AppTokens.rMd),
        onTap: isRenaming ? null : () => _open(s),
        child: Container(
          padding: const EdgeInsets.symmetric(
              horizontal: AppTokens.s12, vertical: 6),
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
                  color: s.active
                      ? AppTokens.success
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
                          final name = v.trim();
                          if (name.isNotEmpty) {
                            ref
                                .read(sessionsProvider.notifier)
                                .rename(s.jid, name);
                          }
                          setState(() => _renaming = null);
                        },
                      )
                    : Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(
                            s.title,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: TextStyle(
                              color: isSelected ? c.accent : c.textPrimary,
                              fontSize: 14,
                              fontWeight: isSelected
                                  ? FontWeight.w600
                                  : FontWeight.w400,
                            ),
                          ),
                          if (s.folder.isNotEmpty)
                            Text(s.folder,
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: TextStyle(
                                    color: c.textMuted, fontSize: 10)),
                        ],
                      ),
              ),
              if (!isRenaming) ...[
                Text(
                  _relTime(_ts(s)),
                  style: TextStyle(color: c.textMuted, fontSize: 11),
                ),
                const SizedBox(width: AppTokens.s4),
              ],
              _itemMenu(s, isPinned),
            ],
          ),
        ),
      ),
    );
  }

  Widget _itemMenu(SessionInfo s, bool isPinned) {
    final c = context.colors;
    PopupMenuItem<String> item(String value, IconData icon, String label,
            {Color? color}) =>
        PopupMenuItem(
          value: value,
          height: 42,
          child: Row(children: [
            Icon(icon, size: 17, color: color ?? c.textSecondary),
            const SizedBox(width: AppTokens.s12),
            Text(label,
                style: TextStyle(
                    color: color ?? c.textPrimary,
                    fontSize: 14,
                    fontWeight: FontWeight.w500)),
          ]),
        );
    return PopupMenuButton<String>(
      tooltip: '',
      padding: EdgeInsets.zero,
      iconSize: 18,
      icon: Icon(Icons.more_horiz, size: 18, color: c.textMuted),
      color: c.surface,
      elevation: 12,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(AppTokens.rLg),
        side: BorderSide(color: c.border),
      ),
      onSelected: (key) {
        switch (key) {
          case 'pin':
            ref.read(pinnedProvider.notifier).toggle(s.jid);
          case 'rename':
            _renameCtrl.text = s.name;
            setState(() => _renaming = s.jid);
          case 'copy':
            Clipboard.setData(ClipboardData(text: s.jid));
          case 'delete':
            ref.read(sessionsProvider.notifier).delete(s.jid);
            if (ref.read(selectedSessionJidProvider) == s.jid) {
              ref.read(selectedSessionJidProvider.notifier).state = null;
            }
        }
      },
      itemBuilder: (_) => [
        item('pin', isPinned ? Icons.push_pin : Icons.push_pin_outlined,
            isPinned ? tr('Bỏ ghim', 'Unpin') : tr('Ghim', 'Pin')),
        item('rename', Icons.edit_outlined, tr('Đổi tên', 'Rename')),
        item('copy', Icons.copy_outlined, tr('Sao chép ID', 'Copy ID')),
        // The default session is the device's permanent fallback; can't delete.
        if (!s.isDefault) ...[
          const PopupMenuDivider(),
          item('delete', Icons.delete_outline, tr('Xoá', 'Delete'),
              color: AppTokens.danger),
        ],
      ],
    );
  }
}

/// A lightweight (folder, name) pair for creating a session from a picked agent.
class AgentFolderName {
  final String folder;
  final String name;
  const AgentFolderName(this.folder, this.name);
}

/// Group-by + sort-by dropdown.
class _OrganizeMenu extends ConsumerWidget {
  const _OrganizeMenu();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final groupMode = ref.watch(groupModeProvider);
    final sort = ref.watch(sortProvider);
    final c = context.colors;
    return PopupMenuButton<String>(
      tooltip: tr('Nhóm & sắp xếp', 'Group & sort'),
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
          case 'g:agent':
            ref.read(groupModeProvider.notifier).set(GroupMode.agent);
          case 'g:date':
            ref.read(groupModeProvider.notifier).set(GroupMode.date);
          case 'g:none':
            ref.read(groupModeProvider.notifier).set(GroupMode.none);
          case 's:updated':
            ref.read(sortProvider.notifier).set(SortMode.updated);
          case 's:name':
            ref.read(sortProvider.notifier).set(SortMode.name);
        }
      },
      itemBuilder: (ctx) {
        final cc = ctx.colors;
        PopupMenuItem<String> header(String label) => PopupMenuItem(
              enabled: false,
              height: 30,
              child: Text(label.toUpperCase(),
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
                  child: Text(label,
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
          header(tr('Nhóm theo', 'Group by')),
          option('g:agent', Icons.smart_toy_outlined, tr('Agent', 'Agent'),
              groupMode == GroupMode.agent),
          option('g:date', Icons.schedule_outlined, tr('Ngày', 'Date'),
              groupMode == GroupMode.date),
          option('g:none', Icons.menu_rounded, tr('Không', 'None'),
              groupMode == GroupMode.none),
          const PopupMenuDivider(),
          header(tr('Sắp xếp', 'Sort by')),
          option('s:updated', Icons.history_rounded,
              tr('Hoạt động gần đây', 'Recent activity'),
              sort == SortMode.updated),
          option('s:name', Icons.sort_by_alpha_rounded, tr('Tên', 'Name'),
              sort == SortMode.name),
        ];
      },
    );
  }
}
