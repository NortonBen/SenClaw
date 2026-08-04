import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/prefs.dart';
import '../../theme/tokens.dart';
import '../../core/transport/connection.dart';
import '../../models/group.dart';
import '../dock/right_dock.dart';
import '../space/event_link.dart';
import '../workflow/workflow_session_pane.dart';
import 'agents_provider.dart';
import 'flow_defaults.dart';
import 'conversation_pane.dart';
import 'groups_provider.dart';
import 'new_chat_dialog.dart';
import 'session_list.dart';

/// Persisted, drag-resizable width of the chat list pane (web ResizeGrips).
final chatSidebarWidthProvider = StateProvider<double>((ref) {
  final s = ref.read(prefsHelperProvider).string('chat:sidebarWidth', '');
  return double.tryParse(s) ?? AppTokens.sidebarWidth;
});

/// Chat surface: the full ported sidebar SessionList (list pane) + the live
/// conversation pane for the selected chat.
class ChatScreen extends ConsumerWidget {
  const ChatScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    // Chat links honor the user's "Mở link" default (Plugins → Widget): warm
    // the cached defaults and (re-)register the in-app mini-browser opener
    // with this build's (context, ref) — openEventLink guards on mounted.
    ChatLinkFlow.prefetch(ref.read(appConfigProvider).httpBase);
    ChatLinkFlow.openInternal =
        (route) => openEventLink(context, ref, route);
    final groups = ref.watch(groupsProvider);
    final selected = ref.watch(selectedJidProvider);
    final sidebarWidth = ref.watch(chatSidebarWidthProvider);
    final showNewChat = ref.watch(showNewChatProvider);
    void openNewChat() =>
        ref.read(showNewChatProvider.notifier).state = true;

    // Picking an existing chat from the sidebar dismisses the New Chat page.
    ref.listen(selectedJidProvider, (prev, next) {
      if (next != null && ref.read(showNewChatProvider)) {
        ref.read(showNewChatProvider.notifier).state = false;
      }
    });

    return Row(
      children: [
        // ── List pane ──────────────────────────────────────────────────
        Container(
          width: sidebarWidth,
          color: c.sidebar,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Padding(
                padding: const EdgeInsets.fromLTRB(
                  AppTokens.s16,
                  AppTokens.s20,
                  AppTokens.s8,
                  AppTokens.s4,
                ),
                child: Text(
                  'Sessions',
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                    color: c.textPrimary,
                    fontSize: 16,
                    fontWeight: FontWeight.w700,
                  ),
                ),
              ),
              Expanded(
                child: SessionList(
                  onNewChat: openNewChat,
                ),
              ),
            ],
          ),
        ),
        const _SidebarResizeGrip(),
        // ── Conversation pane ──────────────────────────────────────────
        Expanded(
          child: (showNewChat || selected == null)
              ? const NewChatScreen()
              // Workflow "sessions" are not chats: show the read-only flow
              // activity view instead of a conversation (no composer/dock).
              : selected.startsWith(wfRunJidPrefix)
                  ? WorkflowSessionPane(
                      key: ValueKey(selected),
                      runId: selected.substring(wfRunJidPrefix.length),
                    )
                  : Row(
                      children: [
                        Expanded(
                          child: ConversationPane(
                            key: ValueKey(selected),
                            jid: selected,
                            title: groups
                                .firstWhere(
                                  (g) => g.jid == selected,
                                  orElse: () =>
                                      GroupInfo(jid: selected, name: selected),
                                )
                                .name,
                          ),
                        ),
                        if (ref.watch(dockVisibleProvider))
                          RightDock(jid: selected),
                      ],
                    ),
        ),
      ],
    );
  }
}

/// A thin draggable grip that resizes the chat list pane and persists the
/// chosen width (web ResizeGrips equivalent).
class _SidebarResizeGrip extends ConsumerWidget {
  const _SidebarResizeGrip();
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    return MouseRegion(
      cursor: SystemMouseCursors.resizeColumn,
      child: GestureDetector(
        behavior: HitTestBehavior.translucent,
        onHorizontalDragUpdate: (d) {
          final next = (ref.read(chatSidebarWidthProvider) + d.delta.dx)
              .clamp(200.0, 520.0);
          ref.read(chatSidebarWidthProvider.notifier).state = next;
        },
        onHorizontalDragEnd: (_) {
          ref.read(prefsHelperProvider).setString('chat:sidebarWidth',
              ref.read(chatSidebarWidthProvider).toStringAsFixed(0));
        },
        child: Container(
          width: 5,
          color: Colors.transparent,
          alignment: Alignment.center,
          child: Container(width: 1, color: c.border),
        ),
      ),
    );
  }
}
