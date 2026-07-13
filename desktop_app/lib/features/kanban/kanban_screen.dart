import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:intl/intl.dart';

import '../../models/kanban_models.dart';
import '../../theme/tokens.dart';
import '../../widgets/app_markdown.dart';
import '../../widgets/refresh_on_mount.dart';
import '../../widgets/section_scaffold.dart';
import 'kanban_dialogs.dart';
import 'kanban_providers.dart';

const _priorities = ['low', 'medium', 'high', 'urgent'];

Color _priorityColor(String? p) => switch (p) {
      'urgent' => AppTokens.danger,
      'high' => AppTokens.warning,
      'medium' => AppTokens.brand,
      'low' => const Color(0xFF8C8C8C),
      _ => const Color(0xFF8C8C8C),
    };

Color _roleColor(String role) => switch (role) {
      'triage' => AppTokens.brandAlt,
      'todo' => const Color(0xFF64748B),
      'ready' => AppTokens.cyan,
      'in_progress' => AppTokens.brand,
      'blocked' => AppTokens.danger,
      'done' => AppTokens.success,
      _ => const Color(0xFF8C8C8C),
    };

Color _avatarColor(String name) {
  const palette = [
    Color(0xFFF56A00),
    Color(0xFF7265E6),
    Color(0xFF00A2AE),
    Color(0xFF1677FF),
    Color(0xFFED4192),
    Color(0xFF52C41A),
  ];
  var h = 0;
  for (final r in name.runes) {
    h = (h * 31 + r) & 0x7fffffff;
  }
  return palette[h % palette.length];
}

String _initials(String name) {
  final parts = name.trim().split(RegExp(r'\s+'));
  if (parts.length == 1) {
    return parts.first.substring(0, parts.first.length.clamp(0, 2)).toUpperCase();
  }
  return (parts.first[0] + parts.last[0]).toUpperCase();
}

class KanbanScreen extends ConsumerWidget {
  const KanbanScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final open = ref.watch(openBoardProvider);
    // Keep the WS live-update subscription alive while the screen is shown.
    ref.watch(kanbanLiveUpdatesProvider);
    return RefreshOnMount(
      providers: [kanbanBoardsProvider],
      child: open == null ? const _BoardListView() : _BoardView(boardId: open),
    );
  }
}

// ── Board list ────────────────────────────────────────────────────────────
class _BoardListView extends ConsumerWidget {
  const _BoardListView();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final boards = ref.watch(kanbanBoardsProvider);
    return SectionScaffold(
      title: 'Kanban',
      subtitle: 'Task board — agents work the Ready column',
      actions: [
        OutlinedButton.icon(
          onPressed: () => showGenerateBoardDialog(context, ref),
          icon: const Icon(Icons.auto_awesome, size: 16),
          label: const Text('AI board'),
        ),
        const SizedBox(width: AppTokens.s8),
        FilledButton.icon(
          onPressed: () => showNewBoardDialog(context, ref),
          icon: const Icon(Icons.add, size: 16),
          label: const Text('New board'),
        ),
      ],
      body: boards.when(
        loading: () => const Center(child: CircularProgressIndicator()),
        error: (e, _) => Center(child: Text('$e')),
        data: (list) {
          if (list.isEmpty) {
            return Center(
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Icon(Icons.view_kanban_outlined, size: 48, color: c.textMuted),
                  const SizedBox(height: AppTokens.s12),
                  Text('No boards yet',
                      style: TextStyle(color: c.textSecondary, fontSize: 15)),
                  const SizedBox(height: AppTokens.s4),
                  Text('Create a board, or let AI plan one from a goal.',
                      style: TextStyle(color: c.textMuted, fontSize: 12)),
                ],
              ),
            );
          }
          return SingleChildScrollView(
            padding: const EdgeInsets.all(AppTokens.s16),
            child: Wrap(
              spacing: AppTokens.s12,
              runSpacing: AppTokens.s12,
              children: [for (final b in list) _BoardCard(board: b)],
            ),
          );
        },
      ),
    );
  }

}

class _BoardCard extends ConsumerWidget {
  const _BoardCard({required this.board});
  final KanbanBoard board;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    return Material(
      color: c.surface,
      borderRadius: BorderRadius.circular(AppTokens.rLg),
      child: InkWell(
        borderRadius: BorderRadius.circular(AppTokens.rLg),
        onTap: () => ref.read(openBoardProvider.notifier).state = board.id,
        child: Container(
          width: 260,
          padding: const EdgeInsets.all(AppTokens.s16),
          decoration: BoxDecoration(
            border: Border.all(color: c.border),
            borderRadius: BorderRadius.circular(AppTokens.rLg),
          ),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(children: [
                Icon(Icons.view_kanban_outlined, size: 18, color: c.accent),
                const SizedBox(width: AppTokens.s8),
                Expanded(
                  child: Text(board.title,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(
                          color: c.textPrimary, fontWeight: FontWeight.w600)),
                ),
                IconButton(
                  tooltip: 'Delete board',
                  icon: Icon(Icons.delete_outline, size: 16, color: c.textMuted),
                  onPressed: () async {
                    final ok = await showDialog<bool>(
                      context: context,
                      builder: (dctx) => AlertDialog(
                        backgroundColor: dctx.colors.surface,
                        title: const Text('Delete board?'),
                        content: Text(
                            'Delete “${board.title}” and all of its cards?'),
                        actions: [
                          TextButton(
                              onPressed: () => Navigator.pop(dctx, false),
                              child: const Text('Cancel')),
                          TextButton(
                              onPressed: () => Navigator.pop(dctx, true),
                              child: const Text('Delete',
                                  style: TextStyle(color: AppTokens.danger))),
                        ],
                      ),
                    );
                    if (ok == true) {
                      await ref.read(kanbanApiProvider).deleteBoard(board.id);
                    }
                  },
                ),
              ]),
              const SizedBox(height: AppTokens.s8),
              Text('${board.columnCount} columns · ${board.cardCount} cards',
                  style: TextStyle(color: c.textMuted, fontSize: 12)),
            ],
          ),
        ),
      ),
    );
  }
}

// ── Board view ────────────────────────────────────────────────────────────
class _BoardView extends ConsumerWidget {
  const _BoardView({required this.boardId});
  final int boardId;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final cols = ref.watch(kanbanColumnsProvider(boardId));
    final boards = ref.watch(kanbanBoardsProvider).valueOrNull ?? const [];
    final board = boards.where((b) => b.id == boardId).firstOrNull;
    final lanes = ref.watch(workerLanesProvider);
    final filter = ref.watch(assigneeFilterProvider);

    final assignees = <String>{
      for (final col in cols.valueOrNull ?? const <KanbanColumn>[])
        for (final card in col.cards)
          if ((card.assignee ?? '').isNotEmpty) card.assignee!
    }.toList()
      ..sort();

    final drawerOpen = ref.watch(activityDrawerProvider);
    final ws = (board?.workspaceDir ?? '').trim();

    return SectionScaffold(
      title: board?.title ?? 'Board',
      subtitle: ws.isEmpty
          ? 'Drag cards between columns · Ready is picked up by workers'
          : 'Workspace: $ws',
      actions: [
        IconButton(
          tooltip: 'Back to boards',
          icon: const Icon(Icons.arrow_back, size: 18),
          onPressed: () => ref.read(openBoardProvider.notifier).state = null,
        ),
        const SizedBox(width: AppTokens.s8),
        Row(mainAxisSize: MainAxisSize.min, children: [
          Text('Worker lanes',
              style: TextStyle(color: c.textSecondary, fontSize: 12)),
          const SizedBox(width: AppTokens.s4),
          Switch(
            value: lanes,
            onChanged: (v) =>
                ref.read(workerLanesProvider.notifier).state = v,
          ),
        ]),
        const SizedBox(width: AppTokens.s8),
        if (assignees.isNotEmpty)
          DropdownButton<String?>(
            value: filter,
            hint: Text('All workers',
                style: TextStyle(color: c.textMuted, fontSize: 12)),
            underline: const SizedBox.shrink(),
            items: [
              const DropdownMenuItem<String?>(
                  value: null, child: Text('All workers')),
              for (final a in assignees)
                DropdownMenuItem<String?>(value: a, child: Text(a)),
            ],
            onChanged: (v) =>
                ref.read(assigneeFilterProvider.notifier).state = v,
          ),
        const SizedBox(width: AppTokens.s8),
        OutlinedButton.icon(
          onPressed: () => _showAddColumnDialog(context, ref, boardId),
          icon: const Icon(Icons.add, size: 16),
          label: const Text('Add column'),
        ),
        const SizedBox(width: AppTokens.s8),
        IconButton(
          tooltip: 'Refresh',
          onPressed: () => ref.invalidate(kanbanColumnsProvider(boardId)),
          icon: const Icon(Icons.refresh, size: 18),
        ),
        IconButton(
          tooltip: drawerOpen ? 'Hide activity' : 'Show running tasks',
          isSelected: drawerOpen,
          onPressed: () => ref.read(activityDrawerProvider.notifier).state =
              !drawerOpen,
          icon: const Icon(Icons.bolt, size: 18),
        ),
      ],
      body: Row(children: [
        Expanded(
          child: cols.when(
            loading: () => const Center(child: CircularProgressIndicator()),
            error: (e, _) => Center(child: Text('$e')),
            data: (columns) => _ColumnsRow(
                boardId: boardId,
                columns: columns,
                lanes: lanes,
                filter: filter),
          ),
        ),
        if (drawerOpen) _ActivityDrawer(boardId: boardId),
      ]),
    );
  }
}

/// Right-hand drawer showing the board's currently-running tasks and the
/// recent worker feed (tool-call comments, completions, blocks).
class _ActivityDrawer extends ConsumerWidget {
  const _ActivityDrawer({required this.boardId});
  final int boardId;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final activity = ref.watch(kanbanActivityProvider(boardId));
    return Container(
      width: 320,
      margin: const EdgeInsets.only(left: AppTokens.s12),
      decoration: BoxDecoration(
        color: c.surface,
        border: Border.all(color: c.border),
        borderRadius: BorderRadius.circular(AppTokens.rLg),
      ),
      child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(
              AppTokens.s16, AppTokens.s12, AppTokens.s8, AppTokens.s8),
          child: Row(children: [
            Icon(Icons.bolt, size: 16, color: AppTokens.brand),
            const SizedBox(width: AppTokens.s8),
            Text('Activity',
                style: TextStyle(
                    color: c.textPrimary,
                    fontWeight: FontWeight.w600,
                    fontSize: 13)),
            const Spacer(),
            IconButton(
              tooltip: 'Close',
              iconSize: 16,
              onPressed: () =>
                  ref.read(activityDrawerProvider.notifier).state = false,
              icon: const Icon(Icons.close),
            ),
          ]),
        ),
        Divider(height: 1, color: c.border),
        Expanded(
          child: activity.when(
            loading: () => const Center(child: CircularProgressIndicator()),
            error: (e, _) => Center(child: Text('$e')),
            data: (a) => ListView(
              padding: const EdgeInsets.all(AppTokens.s12),
              children: [
                _drawerLabel(context, 'Running now (${a.running.length})'),
                if (a.running.isEmpty)
                  _drawerEmpty(context, 'No tasks in progress')
                else
                  for (final card in a.running)
                    _RunningTile(boardId: boardId, card: card),
                const SizedBox(height: AppTokens.s16),
                _drawerLabel(context, 'Recent worker feed'),
                if (a.recent.isEmpty)
                  _drawerEmpty(context, 'No activity yet')
                else
                  for (final item in a.recent) _FeedTile(item: item),
              ],
            ),
          ),
        ),
      ]),
    );
  }
}

Widget _drawerLabel(BuildContext context, String text) => Padding(
      padding: const EdgeInsets.only(bottom: AppTokens.s8),
      child: Text(text.toUpperCase(),
          style: TextStyle(
              color: context.colors.textMuted,
              fontSize: 10,
              fontWeight: FontWeight.w700,
              letterSpacing: 0.6)),
    );

Widget _drawerEmpty(BuildContext context, String text) => Padding(
      padding: const EdgeInsets.symmetric(vertical: AppTokens.s8),
      child: Text(text,
          style: TextStyle(color: context.colors.textMuted, fontSize: 12)),
    );

class _RunningTile extends ConsumerWidget {
  const _RunningTile({required this.boardId, required this.card});
  final int boardId;
  final KanbanCard card;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    return Container(
      margin: const EdgeInsets.only(bottom: AppTokens.s8),
      padding: const EdgeInsets.all(AppTokens.s12),
      decoration: BoxDecoration(
        color: c.surfaceAlt,
        borderRadius: BorderRadius.circular(AppTokens.rMd),
        border: Border(left: BorderSide(color: AppTokens.brand, width: 3)),
      ),
      child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
        Row(children: [
          SizedBox(
            width: 12,
            height: 12,
            child: CircularProgressIndicator(
                strokeWidth: 2, color: AppTokens.brand),
          ),
          const SizedBox(width: AppTokens.s8),
          Expanded(
            child: InkWell(
              onTap: () => showDialog(
                context: context,
                builder: (_) =>
                    _CardDetailDialog(boardId: boardId, cardId: card.id),
              ),
              child: Text(card.title,
                  maxLines: 2,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                      color: c.textPrimary,
                      fontSize: 12,
                      fontWeight: FontWeight.w600)),
            ),
          ),
        ]),
        if ((card.assignee ?? '').isNotEmpty)
          Padding(
            padding: const EdgeInsets.only(top: AppTokens.s4),
            child: Text('@${card.assignee}',
                style: TextStyle(color: c.textMuted, fontSize: 11)),
          ),
      ]),
    );
  }
}

class _FeedTile extends StatelessWidget {
  const _FeedTile({required this.item});
  final KanbanActivityItem item;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final (icon, color) = switch (item.kind) {
      'complete' => (Icons.check_circle, AppTokens.success),
      'block' => (Icons.block, AppTokens.danger),
      'unblock' => (Icons.lock_open, AppTokens.cyan),
      'system' => (Icons.settings, c.textMuted),
      _ => (Icons.chat_bubble_outline, AppTokens.brand),
    };
    return Padding(
      padding: const EdgeInsets.only(bottom: AppTokens.s12),
      child: Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
        Icon(icon, size: 14, color: color),
        const SizedBox(width: AppTokens.s8),
        Expanded(
          child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
            Text(item.cardTitle,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(
                    color: c.textSecondary,
                    fontSize: 11,
                    fontWeight: FontWeight.w600)),
            const SizedBox(height: 2),
            Text(item.body,
                maxLines: 4,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(color: c.textMuted, fontSize: 11.5)),
            if (item.author.isNotEmpty)
              Padding(
                padding: const EdgeInsets.only(top: 2),
                child: Text(item.author,
                    style: TextStyle(color: c.textMuted, fontSize: 10)),
              ),
          ]),
        ),
      ]),
    );
  }
}

class _ColumnsRow extends ConsumerWidget {
  const _ColumnsRow(
      {required this.boardId,
      required this.columns,
      required this.lanes,
      required this.filter});
  final int boardId;
  final List<KanbanColumn> columns;
  final bool lanes;
  final String? filter;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return SingleChildScrollView(
      scrollDirection: Axis.horizontal,
      padding: const EdgeInsets.all(AppTokens.s16),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          for (int i = 0; i < columns.length; i++) ...[
            if (i > 0) const SizedBox(width: AppTokens.s12),
            _ColumnView(
                boardId: boardId,
                column: columns[i],
                lanes: lanes,
                filter: filter),
          ],
        ],
      ),
    );
  }
}

class _ColumnView extends ConsumerWidget {
  const _ColumnView(
      {required this.boardId,
      required this.column,
      required this.lanes,
      required this.filter});
  final int boardId;
  final KanbanColumn column;
  final bool lanes;
  final String? filter;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final cards = filter == null
        ? column.cards
        : column.cards.where((k) => k.assignee == filter).toList();
    final over =
        column.wipLimit != null && cards.length > (column.wipLimit ?? 0);

    // Worker lanes: group by assignee (unassigned last).
    final laneKeys = <String?>[];
    final byLane = <String?, List<KanbanCard>>{};
    if (lanes) {
      for (final card in cards) {
        final k = (card.assignee ?? '').isEmpty ? null : card.assignee;
        byLane.putIfAbsent(k, () => []).add(card);
      }
      final named = byLane.keys.whereType<String>().toList()..sort();
      laneKeys.addAll(named);
      if (byLane.containsKey(null)) laneKeys.add(null);
    }

    return DragTarget<KanbanCard>(
      onWillAcceptWithDetails: (d) => d.data.columnId != column.id,
      onAcceptWithDetails: (d) => ref
          .read(kanbanApiProvider)
          .moveCard(boardId, d.data.id, column.id),
      builder: (context, candidates, _) {
        final highlight = candidates.isNotEmpty;
        return Container(
          width: 290,
          constraints: const BoxConstraints(minHeight: 200),
          decoration: BoxDecoration(
            color: c.surfaceAlt.withValues(alpha: 0.5),
            borderRadius: BorderRadius.circular(AppTokens.rXl),
            border: Border.all(
                color: highlight ? c.accent : c.border,
                width: highlight ? 2 : 1),
          ),
          padding: const EdgeInsets.all(AppTokens.s8),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Padding(
                padding: const EdgeInsets.symmetric(
                    horizontal: AppTokens.s4, vertical: AppTokens.s4),
                child: Row(children: [
                  Container(
                    width: 9,
                    height: 9,
                    decoration: BoxDecoration(
                        color: _roleColor(column.role), shape: BoxShape.circle),
                  ),
                  const SizedBox(width: AppTokens.s8),
                  Expanded(
                    child: Text(column.title,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: TextStyle(
                            color: c.textPrimary, fontWeight: FontWeight.w700)),
                  ),
                  Container(
                    padding: const EdgeInsets.symmetric(
                        horizontal: AppTokens.s8, vertical: 1),
                    decoration: BoxDecoration(
                      color: over
                          ? AppTokens.danger
                          : c.surfaceAlt,
                      borderRadius: BorderRadius.circular(AppTokens.rFull),
                    ),
                    child: Text(
                      '${cards.length}${column.wipLimit != null ? '/${column.wipLimit}' : ''}',
                      style: TextStyle(
                          fontSize: 11,
                          color: over ? Colors.white : c.textSecondary),
                    ),
                  ),
                  PopupMenuButton<String>(
                    tooltip: 'Column',
                    icon: Icon(Icons.more_horiz, size: 16, color: c.textMuted),
                    onSelected: (v) async {
                      if (v == 'delete') {
                        await ref
                            .read(kanbanApiProvider)
                            .deleteColumn(boardId, column.id);
                      }
                    },
                    itemBuilder: (_) => const [
                      PopupMenuItem(
                          value: 'delete', child: Text('Delete column')),
                    ],
                  ),
                ]),
              ),
              Flexible(
                child: SingleChildScrollView(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      if (!lanes)
                        for (final card in cards)
                          _CardChip(boardId: boardId, card: card)
                      else
                        for (final lane in laneKeys) ...[
                          _LaneHeader(
                              name: lane, count: byLane[lane]?.length ?? 0),
                          for (final card
                              in byLane[lane] ?? const <KanbanCard>[])
                            _CardChip(boardId: boardId, card: card),
                        ],
                    ],
                  ),
                ),
              ),
              _AddCardTile(boardId: boardId, columnId: column.id),
            ],
          ),
        );
      },
    );
  }
}

class _LaneHeader extends StatelessWidget {
  const _LaneHeader({required this.name, required this.count});
  final String? name;
  final int count;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Padding(
      padding: const EdgeInsets.symmetric(
          horizontal: AppTokens.s4, vertical: AppTokens.s6),
      child: Row(children: [
        if (name != null)
          CircleAvatar(
            radius: 8,
            backgroundColor: _avatarColor(name!),
            child: Text(_initials(name!),
                style: const TextStyle(fontSize: 8, color: Colors.white)),
          )
        else
          Icon(Icons.circle_outlined, size: 14, color: c.textMuted),
        const SizedBox(width: AppTokens.s6),
        Text(name ?? 'Unassigned',
            style: TextStyle(
                fontSize: 11, color: c.textMuted, fontWeight: FontWeight.w600)),
        Text(' · $count', style: TextStyle(fontSize: 11, color: c.textMuted)),
        const SizedBox(width: AppTokens.s6),
        Expanded(child: Divider(color: c.border, height: 1)),
      ]),
    );
  }
}

class _CardChip extends ConsumerWidget {
  const _CardChip({required this.boardId, required this.card});
  final int boardId;
  final KanbanCard card;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final chip = _CardChipBody(card: card);
    return Padding(
      padding: const EdgeInsets.symmetric(
          horizontal: AppTokens.s4, vertical: AppTokens.s4),
      child: Draggable<KanbanCard>(
        data: card,
        feedback: Material(
          color: Colors.transparent,
          child: SizedBox(width: 260, child: Opacity(opacity: 0.9, child: chip)),
        ),
        childWhenDragging: Opacity(opacity: 0.35, child: chip),
        child: InkWell(
          borderRadius: BorderRadius.circular(AppTokens.rLg),
          onTap: () => showDialog(
            context: context,
            builder: (_) => _CardDetailDialog(boardId: boardId, cardId: card.id),
          ),
          child: chip,
        ),
      ),
    );
  }
}

class _CardChipBody extends StatelessWidget {
  const _CardChipBody({required this.card});
  final KanbanCard card;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final blocked = card.openDeps > 0 && !card.done;
    return Container(
      padding: const EdgeInsets.all(AppTokens.s8),
      decoration: BoxDecoration(
        color: c.surface,
        borderRadius: BorderRadius.circular(AppTokens.rLg),
        border: Border(
          left: BorderSide(
              color: blocked ? AppTokens.danger : Colors.transparent, width: 3),
          top: BorderSide(color: c.border),
          right: BorderSide(color: c.border),
          bottom: BorderSide(color: c.border),
        ),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Wrap(
            spacing: AppTokens.s4,
            runSpacing: AppTokens.s2,
            crossAxisAlignment: WrapCrossAlignment.center,
            children: [
              Text('#${card.id}',
                  style: TextStyle(fontSize: 10, color: c.textMuted)),
              if (card.priority != null)
                _Tag(card.priority!.toUpperCase(),
                    color: _priorityColor(card.priority)),
              if (card.tenant != null) _Tag(card.tenant!),
              for (final l in card.labels) _Tag(l),
            ],
          ),
          const SizedBox(height: AppTokens.s4),
          Row(children: [
            if (card.done)
              const Padding(
                padding: EdgeInsets.only(right: 4),
                child:
                    Icon(Icons.check_circle, size: 14, color: AppTokens.success),
              ),
            Expanded(
              child: Text(card.title,
                  style: TextStyle(
                    color: card.done ? c.textMuted : c.textPrimary,
                    fontSize: 13,
                    decoration: card.done ? TextDecoration.lineThrough : null,
                  )),
            ),
          ]),
          const SizedBox(height: AppTokens.s6),
          Row(children: [
            if (blocked) ...[
              const Icon(Icons.lock_outline, size: 12, color: AppTokens.danger),
              Text(' ${card.openDeps}',
                  style:
                      const TextStyle(fontSize: 11, color: AppTokens.danger)),
              const SizedBox(width: AppTokens.s8),
            ],
            if (card.childTotal > 0) ...[
              Icon(Icons.account_tree_outlined, size: 12, color: c.textMuted),
              Text(' ${card.childDone}/${card.childTotal}',
                  style: TextStyle(fontSize: 11, color: c.textMuted)),
              const SizedBox(width: AppTokens.s8),
            ],
            if (card.commentCount > 0) ...[
              Icon(Icons.chat_bubble_outline, size: 12, color: c.textMuted),
              Text(' ${card.commentCount}',
                  style: TextStyle(fontSize: 11, color: c.textMuted)),
            ],
            const Spacer(),
            if ((card.assignee ?? '').isNotEmpty)
              Tooltip(
                message: card.assignee!,
                child: CircleAvatar(
                  radius: 10,
                  backgroundColor: _avatarColor(card.assignee!),
                  child: Text(_initials(card.assignee!),
                      style:
                          const TextStyle(fontSize: 9, color: Colors.white)),
                ),
              ),
          ]),
        ],
      ),
    );
  }
}

class _Tag extends StatelessWidget {
  const _Tag(this.text, {this.color});
  final String text;
  final Color? color;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final fg = color ?? c.textSecondary;
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 1),
      decoration: BoxDecoration(
        color: fg.withValues(alpha: 0.14),
        borderRadius: BorderRadius.circular(AppTokens.rSm),
      ),
      child: Text(text,
          style:
              TextStyle(fontSize: 10, color: fg, fontWeight: FontWeight.w600)),
    );
  }
}

class _AddCardTile extends ConsumerStatefulWidget {
  const _AddCardTile({required this.boardId, required this.columnId});
  final int boardId;
  final int columnId;

  @override
  ConsumerState<_AddCardTile> createState() => _AddCardTileState();
}

class _AddCardTileState extends ConsumerState<_AddCardTile> {
  bool _editing = false;
  final _ctl = TextEditingController();

  @override
  void dispose() {
    _ctl.dispose();
    super.dispose();
  }

  Future<void> _commit() async {
    final t = _ctl.text.trim();
    setState(() => _editing = false);
    _ctl.clear();
    if (t.isNotEmpty) {
      await ref.read(kanbanApiProvider).addCard(widget.boardId, widget.columnId, t);
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    if (!_editing) {
      return TextButton.icon(
        onPressed: () => setState(() => _editing = true),
        icon: Icon(Icons.add, size: 14, color: c.textMuted),
        label: Text('Add card',
            style: TextStyle(color: c.textMuted, fontSize: 12)),
      );
    }
    return Padding(
      padding: const EdgeInsets.all(AppTokens.s4),
      child: TextField(
        controller: _ctl,
        autofocus: true,
        decoration: const InputDecoration(hintText: 'Card title…'),
        onSubmitted: (_) => _commit(),
        onTapOutside: (_) => _commit(),
      ),
    );
  }
}

const _columnRoles = [
  ('custom', 'Custom'),
  ('triage', 'Triage'),
  ('todo', 'Todo'),
  ('ready', 'Ready'),
  ('in_progress', 'In Progress'),
  ('blocked', 'Blocked'),
  ('done', 'Done'),
];

Future<void> _showAddColumnDialog(
    BuildContext context, WidgetRef ref, int boardId) async {
  final ctl = TextEditingController();
  String role = 'custom';
  final ok = await showDialog<bool>(
    context: context,
    builder: (dctx) => StatefulBuilder(
      builder: (dctx, setSt) => AlertDialog(
        backgroundColor: dctx.colors.surface,
        title: const Text('New column'),
        content: SizedBox(
          width: 380,
          child: Column(mainAxisSize: MainAxisSize.min, children: [
            TextField(
              controller: ctl,
              autofocus: true,
              decoration: const InputDecoration(labelText: 'Title'),
              onSubmitted: (_) => Navigator.pop(dctx, true),
            ),
            const SizedBox(height: AppTokens.s12),
            DropdownButtonFormField<String>(
              initialValue: role,
              decoration: const InputDecoration(labelText: 'Type'),
              items: [
                for (final (key, label) in _columnRoles)
                  DropdownMenuItem(value: key, child: Text(label)),
              ],
              onChanged: (v) => setSt(() => role = v ?? 'custom'),
            ),
          ]),
        ),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(dctx, false),
              child: const Text('Cancel')),
          FilledButton(
              onPressed: () => Navigator.pop(dctx, true),
              child: const Text('Add')),
        ],
      ),
    ),
  );
  if (ok == true && ctl.text.trim().isNotEmpty) {
    await ref
        .read(kanbanApiProvider)
        .addColumn(boardId, ctl.text.trim(), role: role);
  }
}

// ── Card detail dialog ────────────────────────────────────────────────────
class _CardDetailDialog extends ConsumerStatefulWidget {
  const _CardDetailDialog({required this.boardId, required this.cardId});
  final int boardId;
  final int cardId;

  @override
  ConsumerState<_CardDetailDialog> createState() => _CardDetailDialogState();
}

class _CardDetailDialogState extends ConsumerState<_CardDetailDialog> {
  final _commentCtl = TextEditingController();

  @override
  void dispose() {
    _commentCtl.dispose();
    super.dispose();
  }

  Future<String?> _prompt(String title, {String hint = ''}) async {
    final ctl = TextEditingController();
    final ok = await showDialog<bool>(
      context: context,
      builder: (dctx) => AlertDialog(
        backgroundColor: dctx.colors.surface,
        title: Text(title),
        content: SizedBox(
          width: 380,
          child: TextField(
            controller: ctl,
            autofocus: true,
            maxLines: 3,
            decoration: InputDecoration(hintText: hint),
          ),
        ),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(dctx, false),
              child: const Text('Cancel')),
          FilledButton(
              onPressed: () => Navigator.pop(dctx, true),
              child: const Text('OK')),
        ],
      ),
    );
    return ok == true ? ctl.text.trim() : null;
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final detail = ref.watch(kanbanCardDetailProvider(widget.cardId));
    final cols = ref
            .watch(kanbanColumnsProvider(widget.boardId))
            .valueOrNull ??
        const <KanbanColumn>[];
    final api = ref.read(kanbanApiProvider);

    return Dialog(
      backgroundColor: c.surface,
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 560, maxHeight: 640),
        child: detail.when(
          loading: () => const SizedBox(
              height: 200, child: Center(child: CircularProgressIndicator())),
          error: (e, _) =>
              Padding(padding: const EdgeInsets.all(24), child: Text('$e')),
          data: (d) {
            final card = d.card;
            final col = cols.where((x) => x.id == card.columnId).firstOrNull;
            final isBlockedCol = col?.role == 'blocked';
            return Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                // header
                Padding(
                  padding: const EdgeInsets.fromLTRB(
                      AppTokens.s16, AppTokens.s16, AppTokens.s8, 0),
                  child: Row(children: [
                    Text('#${card.id}',
                        style: TextStyle(color: c.textMuted, fontSize: 12)),
                    const SizedBox(width: AppTokens.s8),
                    if (col != null) _Tag(col.title),
                    const Spacer(),
                    FilledButton.icon(
                      onPressed: () async {
                        final s = await _prompt('Complete — summary (optional)');
                        if (s == null) return;
                        await api.completeCard(widget.boardId, card.id, s);
                      },
                      icon: const Icon(Icons.check, size: 14),
                      label: const Text('Complete'),
                    ),
                    const SizedBox(width: AppTokens.s4),
                    if (isBlockedCol)
                      OutlinedButton.icon(
                        onPressed: () async {
                          await api.unblockCard(widget.boardId, card.id);
                        },
                        icon: const Icon(Icons.play_arrow, size: 14),
                        label: const Text('Unblock'),
                      )
                    else
                      OutlinedButton.icon(
                        onPressed: () async {
                          final r = await _prompt('Block — reason');
                          if (r == null) return;
                          await api.blockCard(widget.boardId, card.id, r);
                        },
                        icon: const Icon(Icons.block, size: 14,
                            color: AppTokens.danger),
                        label: const Text('Block',
                            style: TextStyle(color: AppTokens.danger)),
                      ),
                    IconButton(
                        onPressed: () => Navigator.pop(context),
                        icon: const Icon(Icons.close, size: 18)),
                  ]),
                ),
                Flexible(
                  child: SingleChildScrollView(
                    padding: const EdgeInsets.all(AppTokens.s16),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(card.title,
                            style: TextStyle(
                                color: c.textPrimary,
                                fontSize: 17,
                                fontWeight: FontWeight.w700)),
                        const SizedBox(height: AppTokens.s12),
                        if (card.description.isNotEmpty) ...[
                          AppMarkdown(card.description),
                          const SizedBox(height: AppTokens.s12),
                        ],
                        // properties
                        Wrap(
                          spacing: AppTokens.s16,
                          runSpacing: AppTokens.s8,
                          children: [
                            _PropDropdown(
                              label: 'Column',
                              value: '${card.columnId}',
                              items: [
                                for (final k in cols)
                                  MapEntry('${k.id}', k.title)
                              ],
                              onChanged: (v) => api.moveCard(
                                  widget.boardId, card.id, int.parse(v)),
                            ),
                            _PropDropdown(
                              label: 'Priority',
                              value: card.priority ?? '',
                              items: [
                                const MapEntry('', '—'),
                                for (final p in _priorities) MapEntry(p, p)
                              ],
                              onChanged: (v) => api.updateCard(widget.boardId,
                                  card.id, {'priority': v.isEmpty ? null : v}),
                            ),
                            _AssigneeField(
                              value: card.assignee ?? '',
                              onChanged: (v) => api.updateCard(widget.boardId,
                                  card.id, {'assignee': v.isEmpty ? null : v}),
                            ),
                            _PropText(
                              label: 'Labels (a, b)',
                              value: card.labels.join(', '),
                              onSubmit: (v) => api.updateCard(
                                  widget.boardId, card.id, {
                                'labels': v
                                    .split(',')
                                    .map((s) => s.trim())
                                    .where((s) => s.isNotEmpty)
                                    .toList()
                              }),
                            ),
                          ],
                        ),
                        const SizedBox(height: AppTokens.s12),
                        Row(children: [
                          OutlinedButton.icon(
                            onPressed: () async {
                              final messenger = ScaffoldMessenger.of(context);
                              messenger.showSnackBar(const SnackBar(
                                  content:
                                      Text('AI breaking the task down…')));
                              try {
                                await api.breakdownCard(
                                    widget.boardId, card.id);
                              } catch (e) {
                                messenger.showSnackBar(SnackBar(
                                    content: Text('AI failed: $e')));
                              }
                            },
                            icon: const Icon(Icons.auto_awesome, size: 14),
                            label: const Text('Break down (AI)'),
                          ),
                          const Spacer(),
                          TextButton.icon(
                            onPressed: () async {
                              await api.deleteCard(widget.boardId, card.id);
                              if (context.mounted) Navigator.pop(context);
                            },
                            icon: const Icon(Icons.delete_outline,
                                size: 14, color: AppTokens.danger),
                            label: const Text('Delete',
                                style: TextStyle(color: AppTokens.danger)),
                          ),
                        ]),
                        if (d.links.isNotEmpty) ...[
                          const SizedBox(height: AppTokens.s12),
                          Text('Dependencies',
                              style: TextStyle(
                                  color: c.textSecondary,
                                  fontWeight: FontWeight.w700,
                                  fontSize: 12)),
                          const SizedBox(height: AppTokens.s4),
                          for (final l in d.links)
                            Padding(
                              padding: const EdgeInsets.only(bottom: 2),
                              child: Text(
                                l.childId == card.id
                                    ? '⛔ blocked by: ${l.parentTitle}${l.parentDone ? ' ✓' : ''}'
                                    : '→ blocks: ${l.childTitle}${l.childDone ? ' ✓' : ''}',
                                style: TextStyle(
                                    color: c.textSecondary, fontSize: 12),
                              ),
                            ),
                        ],
                        const SizedBox(height: AppTokens.s12),
                        Text('Comments (${d.comments.length})',
                            style: TextStyle(
                                color: c.textSecondary,
                                fontWeight: FontWeight.w700,
                                fontSize: 12)),
                        const SizedBox(height: AppTokens.s4),
                        for (final m in d.comments) _CommentTile(m: m),
                        const SizedBox(height: AppTokens.s8),
                        Row(children: [
                          Expanded(
                            child: TextField(
                              controller: _commentCtl,
                              decoration: const InputDecoration(
                                  hintText: 'Add a note…'),
                              onSubmitted: (_) => _sendComment(api),
                            ),
                          ),
                          const SizedBox(width: AppTokens.s8),
                          FilledButton(
                              onPressed: () => _sendComment(api),
                              child: const Text('Send')),
                        ]),
                      ],
                    ),
                  ),
                ),
              ],
            );
          },
        ),
      ),
    );
  }

  Future<void> _sendComment(KanbanApi api) async {
    final t = _commentCtl.text.trim();
    if (t.isEmpty) return;
    _commentCtl.clear();
    await api.comment(widget.boardId, widget.cardId, t);
  }
}

class _CommentTile extends StatelessWidget {
  const _CommentTile({required this.m});
  final KanbanComment m;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final accent = switch (m.kind) {
      'complete' => AppTokens.success,
      'block' => AppTokens.danger,
      'unblock' => AppTokens.brand,
      _ => c.border,
    };
    final when = m.createdAt > 0
        ? DateFormat('dd/MM HH:mm')
            .format(DateTime.fromMillisecondsSinceEpoch(m.createdAt * 1000))
        : '';
    return Container(
      margin: const EdgeInsets.only(bottom: AppTokens.s6),
      padding: const EdgeInsets.all(AppTokens.s8),
      decoration: BoxDecoration(
        color: c.surfaceAlt,
        borderRadius: BorderRadius.circular(AppTokens.rMd),
        border: Border(left: BorderSide(color: accent, width: 3)),
      ),
      child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
        Row(children: [
          Text(m.author,
              style: TextStyle(
                  color: c.textPrimary,
                  fontSize: 11,
                  fontWeight: FontWeight.w700)),
          if (m.kind != 'comment') ...[
            const SizedBox(width: AppTokens.s6),
            _Tag(m.kind),
          ],
          const Spacer(),
          Text(when, style: TextStyle(color: c.textMuted, fontSize: 10)),
        ]),
        const SizedBox(height: 2),
        Text(m.body, style: TextStyle(color: c.textSecondary, fontSize: 12)),
      ]),
    );
  }
}

// ── Small property editors ────────────────────────────────────────────────
class _PropDropdown extends StatelessWidget {
  const _PropDropdown(
      {required this.label,
      required this.value,
      required this.items,
      required this.onChanged});
  final String label;
  final String value;
  final List<MapEntry<String, String>> items;
  final void Function(String) onChanged;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return SizedBox(
      width: 240,
      child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
        Text(label, style: TextStyle(color: c.textMuted, fontSize: 11)),
        DropdownButton<String>(
          value: items.any((e) => e.key == value) ? value : items.first.key,
          isExpanded: true,
          underline: Divider(color: c.border, height: 1),
          items: [
            for (final e in items)
              DropdownMenuItem(value: e.key, child: Text(e.value)),
          ],
          onChanged: (v) {
            if (v != null) onChanged(v);
          },
        ),
      ]),
    );
  }
}

class _PropText extends StatefulWidget {
  const _PropText(
      {required this.label, required this.value, required this.onSubmit});
  final String label;
  final String value;
  final void Function(String) onSubmit;

  @override
  State<_PropText> createState() => _PropTextState();
}

class _PropTextState extends State<_PropText> {
  late final _ctl = TextEditingController(text: widget.value);

  @override
  void dispose() {
    _ctl.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return SizedBox(
      width: 240,
      child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
        Text(widget.label, style: TextStyle(color: c.textMuted, fontSize: 11)),
        TextField(
          controller: _ctl,
          decoration: const InputDecoration(isDense: true),
          onSubmitted: widget.onSubmit,
          onTapOutside: (_) {
            if (_ctl.text != widget.value) widget.onSubmit(_ctl.text);
          },
        ),
      ]),
    );
  }
}

/// Assignee picker: choose a worker profile/persona. Empty = default profile.
/// If the card already carries an assignee that isn't a known persona, it is
/// kept as an extra option so it isn't silently dropped.
class _AssigneeField extends ConsumerWidget {
  const _AssigneeField({required this.value, required this.onChanged});
  final String value;
  final void Function(String) onChanged;

  static const _defaultKey = '';

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final profiles = ref.watch(workerProfilesProvider);
    return SizedBox(
      width: 240,
      child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
        Text('Assignee (worker profile)',
            style: TextStyle(color: c.textMuted, fontSize: 11)),
        profiles.when(
          loading: () => const LinearProgressIndicator(),
          error: (_, _) => _fallbackText(context),
          data: (list) {
            final names = <String>{for (final p in list) p.name};
            final options = <String>[
              _defaultKey,
              ...names,
              if (value.isNotEmpty && !names.contains(value)) value,
            ];
            return DropdownButton<String>(
              value: options.contains(value) ? value : _defaultKey,
              isExpanded: true,
              underline: Divider(color: c.border, height: 1),
              items: [
                for (final o in options)
                  DropdownMenuItem(
                    value: o,
                    child: Text(
                      o.isEmpty ? '— default profile —' : o,
                      style: o.isEmpty
                          ? TextStyle(color: c.textMuted, fontStyle: FontStyle.italic)
                          : null,
                    ),
                  ),
              ],
              onChanged: (v) => onChanged(v ?? _defaultKey),
            );
          },
        ),
      ]),
    );
  }

  Widget _fallbackText(BuildContext context) {
    final ctl = TextEditingController(text: value);
    return TextField(
      controller: ctl,
      decoration: const InputDecoration(isDense: true),
      onSubmitted: onChanged,
      onTapOutside: (_) {
        if (ctl.text != value) onChanged(ctl.text);
      },
    );
  }
}
