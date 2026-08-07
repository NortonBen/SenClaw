import 'package:desktop_multi_window/desktop_multi_window.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:window_manager/window_manager.dart';

import '../../core/i18n/l10n.dart';
import '../../models/group.dart';
import '../../theme/tokens.dart';
import 'agents_provider.dart';
import 'conversation_pane.dart';
import 'groups_provider.dart';
import 'new_chat_dialog.dart';

/// The `desktop_multi_window` id when this screen runs as its OWN native window
/// (the tray mini-chat). Null when it is the legacy in-main popover route
/// (`/mini`). Overridden in the sub-window's [ProviderScope] in `main()`.
final subWindowIdProvider = Provider<int?>((ref) => null);

/// Window id of the main application window (always 0 in desktop_multi_window).
const int kMainWindowId = 0;

/// Bumped by the mini-chat "expand" button when running as the legacy in-main
/// popover. [SenClawApp] listens and restores the full-size window. Unused in
/// real sub-window mode, which messages the main window directly instead.
final miniExpandRequestProvider = StateProvider<int>((ref) => 0);

bool get _isMacOS =>
    !kIsWeb && defaultTargetPlatform == TargetPlatform.macOS;

/// Compact, popover-style chat shown when the tray icon is clicked — the
/// Flutter port of the old Tauri `?embed=1` chat window. It deliberately
/// renders ONLY the active conversation: no nav rail, no session sidebar, no
/// right dock. Sessions are switched from the header dropdown.
class MiniChatScreen extends ConsumerWidget {
  const MiniChatScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final groups = ref.watch(groupsProvider);
    final selected = ref.watch(selectedJidProvider);
    final showNewChat = ref.watch(showNewChatProvider);

    // Picking an existing chat dismisses the New Chat page (chat_screen parity).
    ref.listen(selectedJidProvider, (prev, next) {
      if (next != null && ref.read(showNewChatProvider)) {
        ref.read(showNewChatProvider.notifier).state = false;
      }
    });

    final GroupInfo? current = selected == null
        ? null
        : groups.firstWhere(
            (g) => g.jid == selected,
            orElse: () => GroupInfo(jid: selected, name: selected),
          );

    final Widget body;
    if (showNewChat || selected == null || current == null) {
      body = const NewChatScreen();
    } else {
      body = ConversationPane(
        key: ValueKey(current.jid),
        jid: current.jid,
        title: current.name,
      );
    }

    return Scaffold(
      backgroundColor: c.bg,
      body: Column(
        children: [
          _MiniHeader(
            groups: groups,
            currentTitle: showNewChat || current == null
                ? context.tr('New chat')
                : current.name,
          ),
          Container(height: 1, color: c.border),
          Expanded(child: body),
        ],
      ),
    );
  }
}

class _MiniHeader extends ConsumerWidget {
  const _MiniHeader({required this.groups, required this.currentTitle});
  final List<GroupInfo> groups;
  final String currentTitle;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final subWindowId = ref.watch(subWindowIdProvider);
    // A real sub-window keeps its native title bar (traffic lights live there),
    // so the in-app header doesn't need to reserve room for them or act as a
    // drag handle. The legacy in-main popover is frameless and does.
    final isSubWindow = subWindowId != null;

    void startNewChat() {
      ref.read(showNewChatProvider.notifier).state = true;
    }

    void selectGroup(String jid) {
      ref.read(showNewChatProvider.notifier).state = false;
      ref.read(selectedJidProvider.notifier).state = jid;
    }

    // Expand to the full window: open the main SenClaw Desktop window and close
    // this mini window. Await the cross-window call first so it isn't dropped
    // when this engine tears down. (Legacy in-main popover just signals.)
    Future<void> expand() async {
      if (isSubWindow) {
        // Open the main window first; close THIS mini in a finally so it always
        // dismisses even if the cross-window call throws (engine teardown).
        try {
          await DesktopMultiWindow.invokeMethod(kMainWindowId, 'show_full');
        } finally {
          await WindowController.fromWindowId(subWindowId).close();
        }
      } else {
        ref.read(miniExpandRequestProvider.notifier).update((n) => n + 1);
      }
    }

    void hide() {
      if (isSubWindow) {
        WindowController.fromWindowId(subWindowId).close();
      } else {
        windowManager.hide();
      }
    }

    // The header doubles as the window drag handle on the frameless in-main
    // popover, and reserves space on the left so the traffic-light buttons
    // don't overlap. A sub-window has a native title bar, so neither applies.
    final header = Container(
      height: 46,
      color: c.sidebar,
      padding: EdgeInsets.only(
          left: (_isMacOS && !isSubWindow) ? 76 : AppTokens.s12, right: 6),
      child: Row(
        children: [
          Expanded(
            child: PopupMenuButton<String>(
              tooltip: context.tr('Switch session'),
              position: PopupMenuPosition.under,
              color: c.surface,
              onSelected: (v) =>
                  v == '__new__' ? startNewChat() : selectGroup(v),
              itemBuilder: (_) => [
                PopupMenuItem(
                  value: '__new__',
                  child: Row(children: [
                    const Icon(Icons.add, size: 16),
                    const SizedBox(width: 8),
                    Text(context.tr('New chat')),
                  ]),
                ),
                if (groups.isNotEmpty) const PopupMenuDivider(),
                for (final g in groups)
                  PopupMenuItem(
                    value: g.jid,
                    child: Row(children: [
                      Expanded(
                        child: Text(
                          g.name,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(color: c.textPrimary),
                        ),
                      ),
                      if (g.unread > 0)
                        Container(
                          margin: const EdgeInsets.only(left: 8),
                          padding: const EdgeInsets.symmetric(
                              horizontal: 6, vertical: 1),
                          decoration: BoxDecoration(
                            color: c.accent,
                            borderRadius: BorderRadius.circular(8),
                          ),
                          child: Text(
                            '${g.unread}',
                            style: const TextStyle(
                                color: Colors.white, fontSize: 11),
                          ),
                        ),
                    ]),
                  ),
              ],
              child: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Flexible(
                    child: Text(
                      currentTitle,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(
                        color: c.textPrimary,
                        fontSize: 14,
                        fontWeight: FontWeight.w700,
                      ),
                    ),
                  ),
                  Icon(Icons.expand_more, size: 18, color: c.textMuted),
                ],
              ),
            ),
          ),
          _IconBtn(
            icon: Icons.add,
            tooltip: context.tr('New chat'),
            onTap: startNewChat,
          ),
          _IconBtn(
            icon: Icons.open_in_full,
            tooltip: context.tr('Open full window'),
            onTap: expand,
          ),
          _IconBtn(
            icon: Icons.close,
            tooltip: isSubWindow ? context.tr('Close') : context.tr('Hide'),
            onTap: hide,
          ),
        ],
      ),
    );

    // Only the frameless in-main popover needs a manual drag handle; a real
    // sub-window is dragged by its native title bar.
    if (!_isMacOS || isSubWindow) return header;
    return GestureDetector(
      behavior: HitTestBehavior.translucent,
      onPanStart: (_) => windowManager.startDragging(),
      child: header,
    );
  }
}

class _IconBtn extends StatelessWidget {
  const _IconBtn({required this.icon, required this.tooltip, required this.onTap});
  final IconData icon;
  final String tooltip;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Tooltip(
      message: tooltip,
      child: InkWell(
        borderRadius: BorderRadius.circular(AppTokens.rMd),
        onTap: onTap,
        child: Padding(
          padding: const EdgeInsets.all(8),
          child: Icon(icon, size: 18, color: c.textMuted),
        ),
      ),
    );
  }
}
