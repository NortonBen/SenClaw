import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:web_socket_channel/web_socket_channel.dart';
import 'package:xterm/xterm.dart';
import '../../core/i18n/l10n.dart';
import '../../core/prefs.dart';
import '../../core/transport/connection.dart';
import 'package:file_picker/file_picker.dart';
import '../../theme/tokens.dart';
import '../../widgets/embedded_web.dart';
import '../chat/groups_provider.dart';
import '../space/space_providers.dart';
import 'dispatch_provider.dart';
import 'todos_provider.dart';
import 'workbench_provider.dart';

/// Whether the right dock is shown, and which tab is active.
final dockVisibleProvider = StateProvider<bool>((ref) => false);
final dockTabProvider = StateProvider<int>((ref) => 0); // 0=Console 1=Workbench

/// Persisted, drag-resizable dock width (web ResizeGrips).
final dockWidthProvider = StateProvider<double>((ref) {
  final s = ref.read(prefsHelperProvider).string('chat:dockWidth', '');
  return double.tryParse(s) ?? AppTokens.dockMinWidth;
});

/// The right-hand dock: Agent Console (sub-agent dispatch activity) + Workbench
/// (artifacts). Ported from the React AppLayout right dock.
class RightDock extends ConsumerWidget {
  const RightDock({super.key, required this.jid});
  final String jid;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final tab = ref.watch(dockTabProvider);
    // Allow dragging the dock out to 70% of the window (room for embedded apps),
    // but never below a usable minimum.
    final maxW = (MediaQuery.of(context).size.width * 0.7).clamp(320.0, 2400.0);
    final width = ref.watch(dockWidthProvider).clamp(260.0, maxW);
    final isWide = width >= maxW - 1;

    return Row(children: [
      // Left-edge resize grip (drag left = wider).
      MouseRegion(
        cursor: SystemMouseCursors.resizeColumn,
        child: GestureDetector(
          behavior: HitTestBehavior.translucent,
          onHorizontalDragUpdate: (d) {
            final next = (ref.read(dockWidthProvider) - d.delta.dx)
                .clamp(260.0, maxW);
            ref.read(dockWidthProvider.notifier).state = next;
          },
          onHorizontalDragEnd: (_) {
            ref.read(prefsHelperProvider).setString('chat:dockWidth',
                ref.read(dockWidthProvider).toStringAsFixed(0));
          },
          child: Container(width: 5, color: Colors.transparent),
        ),
      ),
      Container(
      width: width,
      decoration: BoxDecoration(
        color: c.surface,
        border: Border(left: BorderSide(color: c.border)),
      ),
      child: Column(
        children: [
          // Tab bar
          Container(
            height: 44,
            padding: const EdgeInsets.symmetric(horizontal: AppTokens.s12),
            decoration: BoxDecoration(
              border: Border(bottom: BorderSide(color: c.border)),
            ),
            child: Row(
              children: [
                _Tab(label: context.tr('Console'), active: tab == 0, onTap: () =>
                    ref.read(dockTabProvider.notifier).state = 0),
                const SizedBox(width: AppTokens.s8),
                _Tab(label: context.tr('Workbench'), active: tab == 1, onTap: () =>
                    ref.read(dockTabProvider.notifier).state = 1),
                const SizedBox(width: AppTokens.s8),
                _Tab(label: context.tr('Files'), active: tab == 2, onTap: () =>
                    ref.read(dockTabProvider.notifier).state = 2),
                const SizedBox(width: AppTokens.s8),
                _Tab(label: context.tr('Apps'), active: tab == 3, onTap: () =>
                    ref.read(dockTabProvider.notifier).state = 3),
                const SizedBox(width: AppTokens.s8),
                _Tab(label: context.tr('Terminal'), active: tab == 4, onTap: () =>
                    ref.read(dockTabProvider.notifier).state = 4),
                const Spacer(),
                IconButton(
                  tooltip:
                      isWide ? context.tr('Shrink') : context.tr('Expand (70%)'),
                  icon: Icon(
                      isWide
                          ? Icons.close_fullscreen
                          : Icons.open_in_full,
                      size: 15),
                  onPressed: () {
                    final next = isWide ? AppTokens.dockMinWidth : maxW;
                    ref.read(dockWidthProvider.notifier).state = next;
                    ref.read(prefsHelperProvider).setString(
                        'chat:dockWidth', next.toStringAsFixed(0));
                  },
                ),
                IconButton(
                  tooltip: context.tr('Close'),
                  icon: const Icon(Icons.close, size: 16),
                  onPressed: () =>
                      ref.read(dockVisibleProvider.notifier).state = false,
                ),
              ],
            ),
          ),
          Expanded(
            child: switch (tab) {
              0 => const _ConsoleTab(),
              1 => _WorkbenchTab(jid: jid),
              2 => _FilesTab(jid: jid),
              3 => const _AppsDockTab(),
              _ => _TerminalTab(key: ValueKey(jid), jid: jid),
            },
          ),
        ],
      ),
      ),
    ]);
  }
}

class _Tab extends StatelessWidget {
  const _Tab({required this.label, required this.active, required this.onTap});
  final String label;
  final bool active;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return GestureDetector(
      onTap: onTap,
      child: Container(
        padding: const EdgeInsets.symmetric(
            horizontal: AppTokens.s12, vertical: AppTokens.s6),
        decoration: BoxDecoration(
          color: active ? c.accentSoft : Colors.transparent,
          borderRadius: BorderRadius.circular(AppTokens.rMd),
        ),
        child: Text(label,
            style: TextStyle(
              color: active ? c.accent : c.textMuted,
              fontWeight: FontWeight.w600,
              fontSize: 14,
            )),
      ),
    );
  }
}

class _ConsoleTab extends ConsumerWidget {
  const _ConsoleTab();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final d = ref.watch(dispatchProvider);
    final todos = ref.watch(agentTodosProvider);
    if (d.parents.isEmpty && d.activity.isEmpty && todos.isEmpty) {
      return Center(
        child: Text(context.tr('No sub-agent activity'),
            style: TextStyle(color: c.textMuted, fontSize: 12)),
      );
    }
    return ListView(
      padding: const EdgeInsets.all(AppTokens.s12),
      children: [
        // Agent todos (TodoWrite) — web AgentTodoPanel.
        for (final entry in todos.entries) ...[
          _TodoGroup(
            jid: entry.key,
            todos: entry.value,
            onRemove: () =>
                ref.read(agentTodosProvider.notifier).remove(entry.key),
          ),
          const Divider(height: AppTokens.s24),
        ],
        for (final p in d.parents) ...[
          Row(children: [
            Expanded(
              child: Text(p.goal.isEmpty ? context.tr('Dispatch') : p.goal,
                  style: TextStyle(
                      color: c.textPrimary, fontWeight: FontWeight.w700)),
            ),
            InkWell(
              onTap: () => _confirmRemove(
                  context,
                  context.tr('Remove this DAG card?'),
                  () => ref
                      .read(dispatchProvider.notifier)
                      .removeParent(p.id)),
              borderRadius: BorderRadius.circular(AppTokens.rFull),
              child: Padding(
                padding: const EdgeInsets.all(2),
                child: Icon(Icons.close, size: 14, color: c.textMuted),
              ),
            ),
          ]),
          const SizedBox(height: AppTokens.s8),
          for (final t in p.tasks)
            _TaskRow(
              label: t.label,
              status: t.status,
              onDelete: () => _confirmRemove(
                  context,
                  context.trArgs('Remove task "{label}"?', {'label': t.label}),
                  () => ref
                      .read(dispatchProvider.notifier)
                      .removeTask(p.id, t.id)),
            ),
          const Divider(height: AppTokens.s24),
        ],
        if (d.activity.isNotEmpty)
          Text(context.tr('ACTIVITY'),
              style: TextStyle(
                color: c.textMuted,
                fontSize: 11,
                fontWeight: FontWeight.w700,
                letterSpacing: 1,
              )),
        // Chronological feed (oldest → newest), capped to the most recent 80.
        for (final a in (d.activity.length > 80
            ? d.activity.sublist(d.activity.length - 80)
            : d.activity))
          Padding(
            padding: const EdgeInsets.symmetric(vertical: 2),
            child: Text('• ${a.text}',
                style: TextStyle(
                  color: c.textSecondary,
                  fontSize: 12,
                  fontFamily: AppTokens.fontMono,
                )),
          ),
      ],
    );
  }
}

/// Confirm-then-run dialog for destructive Console actions (local removal).
Future<void> _confirmRemove(
    BuildContext context, String message, VoidCallback onConfirm) async {
  final ok = await showDialog<bool>(
    context: context,
    builder: (dctx) => AlertDialog(
      backgroundColor: dctx.colors.surface,
      content: Text(message, style: TextStyle(color: dctx.colors.textPrimary)),
      actions: [
        TextButton(
            onPressed: () => Navigator.pop(dctx, false),
            child: Text(dctx.tr('Cancel'))),
        FilledButton(
          style: FilledButton.styleFrom(backgroundColor: AppTokens.danger),
          onPressed: () => Navigator.pop(dctx, true),
          child: Text(dctx.tr('Remove')),
        ),
      ],
    ),
  );
  if (ok == true) onConfirm();
}

class _TaskRow extends StatelessWidget {
  const _TaskRow(
      {required this.label, required this.status, this.onDelete});
  final String label;
  final String status;
  final VoidCallback? onDelete;

  Color _color() => switch (status) {
        'done' => AppTokens.success,
        'error' || 'timeout' => AppTokens.danger,
        'processing' => AppTokens.warning,
        _ => AppTokens.brandAlt,
      };

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 3),
      child: Row(
        children: [
          Container(
            width: 8,
            height: 8,
            decoration: BoxDecoration(color: _color(), shape: BoxShape.circle),
          ),
          const SizedBox(width: AppTokens.s8),
          Expanded(
            child: Text(label,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(color: c.textPrimary, fontSize: 14)),
          ),
          Text(status, style: TextStyle(color: c.textMuted, fontSize: 12)),
          if (onDelete != null)
            InkWell(
              onTap: onDelete,
              borderRadius: BorderRadius.circular(AppTokens.rFull),
              child: Padding(
                padding: const EdgeInsets.only(left: AppTokens.s6),
                child: Icon(Icons.delete_outline,
                    size: 14, color: c.textMuted),
              ),
            ),
        ],
      ),
    );
  }
}

class _WorkbenchTab extends ConsumerStatefulWidget {
  const _WorkbenchTab({required this.jid});
  final String jid;
  @override
  ConsumerState<_WorkbenchTab> createState() => _WorkbenchTabState();
}

class _WorkbenchTabState extends ConsumerState<_WorkbenchTab> {
  String? _openFile;
  String? _content;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final art = ref.watch(workbenchProvider)[widget.jid] ??
        ref.watch(workbenchProvider)['_'];
    if (art == null) {
      return Center(
        child: Text(context.tr('No artifacts yet'),
            style: TextStyle(color: c.textMuted, fontSize: 12)),
      );
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.all(AppTokens.s12),
          child: Row(
            children: [
              Icon(Icons.widgets_outlined, size: 16, color: c.accent),
              const SizedBox(width: AppTokens.s8),
              Expanded(
                child: Text(art.title,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                        color: c.textPrimary, fontWeight: FontWeight.w700)),
              ),
              Text(art.mode, style: TextStyle(color: c.textMuted, fontSize: 12)),
              IconButton(
                tooltip: context.tr('Close artifact'),
                icon: const Icon(Icons.close, size: 16),
                onPressed: () => ref
                    .read(workbenchProvider.notifier)
                    .close(widget.jid, art.id),
              ),
            ],
          ),
        ),
        const Divider(height: 1),
        // web / backend artifacts render embedded (iframe on web, open-in-
        // browser on desktop); static artifacts show their file tree.
        if (art.url != null && art.url!.isNotEmpty)
          Expanded(
              child: embeddedWebView(art.url!,
                  title: art.title,
                  theme: Theme.of(context).brightness == Brightness.dark
                      ? 'dark'
                      : 'light'))
        else
        Expanded(
          child: _openFile == null
              ? ListView(
                  children: [
                    for (final f in art.files)
                      ListTile(
                        dense: true,
                        leading:
                            const Icon(Icons.insert_drive_file_outlined, size: 16),
                        title: Text(f, style: const TextStyle(fontSize: 12)),
                        onTap: () async {
                          final content = await ref
                              .read(workbenchProvider.notifier)
                              .readFile(widget.jid, art.id, f);
                          setState(() {
                            _openFile = f;
                            _content = content;
                          });
                        },
                      ),
                  ],
                )
              : Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    Padding(
                      padding: const EdgeInsets.symmetric(
                          horizontal: AppTokens.s12, vertical: AppTokens.s4),
                      child: Row(
                        children: [
                          TextButton.icon(
                            onPressed: () => setState(() {
                              _openFile = null;
                              _content = null;
                            }),
                            icon: const Icon(Icons.arrow_back, size: 14),
                            label: Text(context.tr('Files')),
                          ),
                          const SizedBox(width: AppTokens.s8),
                          Expanded(
                            child: Text(_openFile!,
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style:
                                    TextStyle(color: c.textMuted, fontSize: 12)),
                          ),
                        ],
                      ),
                    ),
                    Expanded(
                      child: SingleChildScrollView(
                        padding: const EdgeInsets.all(AppTokens.s12),
                        child: SelectableText(
                          _content ?? '',
                          style: TextStyle(
                            color: c.textSecondary,
                            fontFamily: AppTokens.fontMono,
                            fontSize: 12,
                            height: 1.45,
                          ),
                        ),
                      ),
                    ),
                  ],
                ),
        ),
      ],
    );
  }
}

/// One agent's todo list with a progress count (web AgentTodoPanel item).
class _TodoGroup extends StatelessWidget {
  const _TodoGroup({required this.jid, required this.todos, this.onRemove});
  final String jid;
  final List<AgentTodo> todos;
  final VoidCallback? onRemove;
  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final done = todos.where((t) => t.status == 'completed').length;
    final running = todos.where((t) => t.status == 'in_progress').length;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          children: [
            Icon(Icons.checklist, size: 14, color: c.accent),
            const SizedBox(width: AppTokens.s6),
            Expanded(
              child: Text(jid.split(':').last,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                      color: c.textPrimary,
                      fontSize: 12,
                      fontWeight: FontWeight.w700)),
            ),
            Text('$done/${todos.length}',
                style: TextStyle(color: c.textMuted, fontSize: 11)),
            if (onRemove != null)
              InkWell(
                onTap: onRemove,
                borderRadius: BorderRadius.circular(AppTokens.rFull),
                child: Padding(
                  padding: const EdgeInsets.all(2),
                  child: Icon(Icons.close, size: 14, color: c.textMuted),
                ),
              ),
          ],
        ),
        const SizedBox(height: AppTokens.s6),
        ClipRRect(
          borderRadius: BorderRadius.circular(AppTokens.rSm),
          child: LinearProgressIndicator(
            value: todos.isEmpty ? 0 : done / todos.length,
            minHeight: 3,
            backgroundColor: c.surfaceAlt,
            color: running > 0 ? c.accent : AppTokens.success,
          ),
        ),
        const SizedBox(height: AppTokens.s6),
        for (final t in todos)
          Padding(
            padding: const EdgeInsets.symmetric(vertical: 2),
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                    t.status == 'completed'
                        ? '✓'
                        : t.status == 'in_progress'
                            ? '↻'
                            : '–',
                    style: TextStyle(
                        color: t.status == 'completed'
                            ? AppTokens.success
                            : t.status == 'in_progress'
                                ? c.accent
                                : c.textMuted,
                        fontSize: 12)),
                const SizedBox(width: AppTokens.s6),
                Expanded(
                  child: Text(
                      t.status == 'in_progress' && t.activeForm.isNotEmpty
                          ? t.activeForm
                          : t.content,
                      style: TextStyle(
                        color: t.status == 'completed'
                            ? c.textMuted
                            : c.textSecondary,
                        fontSize: 12,
                        decoration: t.status == 'completed'
                            ? TextDecoration.lineThrough
                            : null,
                      )),
                ),
              ],
            ),
          ),
      ],
    );
  }
}

/// Files tab — browses the CURRENT chat session's workspace dir (the group's
/// `allowedWorkDirs`). Switching sessions re-roots the browser; chats without a
/// workspace show an empty state. Lists via /api/workspace/files and reads via
/// /api/workspace/file. The folder-picker can override the root for this view.
class _FilesTab extends ConsumerStatefulWidget {
  const _FilesTab({required this.jid});
  final String jid;
  @override
  ConsumerState<_FilesTab> createState() => _FilesTabState();
}

class _FilesTabState extends ConsumerState<_FilesTab> {
  String? _root;
  // The workspace root we started at — navigation can't go above this.
  String? _baseRoot;
  // User-picked override (folder-picker); cleared when the session changes.
  String? _manualRoot;
  // Last session jid + last reconciled base, to detect changes in build.
  String? _lastJid;
  String? _loadedBase;
  List<Map<String, dynamic>> _entries = const [];
  bool _loading = false;
  String? _openPath; // file currently being viewed
  String _content = '';
  bool _truncated = false;

  Future<void> _list(String path, {bool asBase = false}) async {
    setState(() {
      _loading = true;
      _openPath = null;
    });
    try {
      final r = await ref
          .read(apiClientProvider)
          .get('/api/workspace/files', query: {'path': path, 'depth': 1});
      final entries = ((r is Map ? r['entries'] : null) as List?)
              ?.whereType<Map>()
              .map((m) => m.cast<String, dynamic>())
              .toList() ??
          <Map<String, dynamic>>[];
      entries.sort((a, b) {
        final d = (b['is_dir'] == true ? 1 : 0) - (a['is_dir'] == true ? 1 : 0);
        return d != 0
            ? d
            : '${a['name']}'.toLowerCase().compareTo('${b['name']}'.toLowerCase());
      });
      if (mounted) {
        setState(() {
          _root = '${(r is Map ? r['root'] : null) ?? path}';
          // Anchor the boundary to the server-resolved root on a base load.
          if (asBase || _baseRoot == null) _baseRoot = _root;
          _entries = entries;
          _loading = false;
        });
      }
    } catch (_) {
      if (mounted) setState(() => _loading = false);
    }
  }

  Future<void> _open(String path) async {
    setState(() => _loading = true);
    try {
      final r = await ref
          .read(apiClientProvider)
          .get('/api/workspace/file', query: {'path': path});
      if (mounted) {
        setState(() {
          _openPath = path;
          _content = '${(r is Map ? r['content'] : null) ?? ''}';
          _truncated = r is Map && r['truncated'] == true;
          _loading = false;
        });
      }
    } catch (e) {
      if (mounted) {
        setState(() {
          _openPath = path;
          _content = context.trArgs('Failed to read: {e}', {'e': e});
          _loading = false;
        });
      }
    }
  }

  String? get _parent {
    if (_root == null) return null;
    // Never navigate above the workspace root the tab was opened at.
    if (_baseRoot != null && _root == _baseRoot) return null;
    final i = _root!.lastIndexOf('/');
    final p = i > 0 ? _root!.substring(0, i) : null;
    // Guard against escaping the base root via a shorter path.
    if (p == null || (_baseRoot != null && !p.startsWith(_baseRoot!))) {
      return null;
    }
    return p;
  }

  Future<void> _pickRoot() async {
    final p = await FilePicker.platform.getDirectoryPath();
    if (p != null) {
      _manualRoot = p; // override the session dir for this view
      _loadedBase = p;
      _list(p, asBase: true);
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    // Resolve this session's workspace dir; re-root when the session changes.
    final groups = ref.watch(groupsProvider);
    final match = groups.where((g) => g.jid == widget.jid);
    final sessionDir = match.isEmpty ? null : match.first.workDir;
    if (_lastJid != widget.jid) {
      _lastJid = widget.jid;
      _manualRoot = null; // a new session drops any manual override
    }
    final effective = _manualRoot ?? sessionDir;
    if (effective != _loadedBase) {
      _loadedBase = effective;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted) return;
        if (effective == null) {
          setState(() {
            _root = null;
            _baseRoot = null;
            _entries = const [];
            _openPath = null;
          });
        } else {
          _list(effective, asBase: true);
        }
      });
    }
    if (_root == null) {
      return Center(
        child: Column(mainAxisSize: MainAxisSize.min, children: [
          Icon(Icons.folder_off_outlined, size: 32, color: c.textMuted),
          const SizedBox(height: AppTokens.s8),
          Text(context.tr('No workspace for this chat'),
              style: TextStyle(color: c.textMuted, fontSize: 12)),
          const SizedBox(height: AppTokens.s8),
          OutlinedButton.icon(
            onPressed: _pickRoot,
            icon: const Icon(Icons.folder_open, size: 16),
            label: Text(context.tr('Open folder')),
          ),
        ]),
      );
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        // Toolbar: current dir + change/reload.
        Padding(
          padding: const EdgeInsets.fromLTRB(
              AppTokens.s12, AppTokens.s8, AppTokens.s4, AppTokens.s8),
          child: Row(children: [
            Icon(Icons.folder_outlined, size: 14, color: c.textMuted),
            const SizedBox(width: AppTokens.s6),
            Expanded(
              child: Text(_root!.split('/').where((s) => s.isNotEmpty).last,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                      color: c.textPrimary,
                      fontSize: 12,
                      fontWeight: FontWeight.w600)),
            ),
            IconButton(
              tooltip: context.tr('Open folder'),
              icon: const Icon(Icons.drive_folder_upload_outlined, size: 16),
              onPressed: _pickRoot,
            ),
            IconButton(
              tooltip: context.tr('Reload'),
              icon: const Icon(Icons.refresh, size: 16),
              onPressed: () => _list(_root!),
            ),
          ]),
        ),
        const Divider(height: 1),
        Expanded(
          child: _loading
              ? const Center(child: CircularProgressIndicator())
              : _openPath != null
                  ? _fileView(context)
                  : _listView(context),
        ),
      ],
    );
  }

  Widget _listView(BuildContext context) {
    final c = context.colors;
    return ListView(
      padding: const EdgeInsets.symmetric(vertical: AppTokens.s4),
      children: [
        if (_parent != null)
          ListTile(
            dense: true,
            leading: Icon(Icons.arrow_upward, size: 16, color: c.textMuted),
            title: const Text('..', style: TextStyle(fontSize: 13)),
            onTap: () => _list(_parent!),
          ),
        for (final e in _entries)
          ListTile(
            dense: true,
            leading: Icon(
                e['is_dir'] == true
                    ? Icons.folder_outlined
                    : Icons.insert_drive_file_outlined,
                size: 16,
                color: e['is_dir'] == true ? c.accent : c.textMuted),
            title: Text('${e['name']}',
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(color: c.textPrimary, fontSize: 13)),
            onTap: () => e['is_dir'] == true
                ? _list('${e['path']}')
                : _open('${e['path']}'),
          ),
      ],
    );
  }

  Widget _fileView(BuildContext context) {
    final c = context.colors;
    final name = _openPath!.split('/').last;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(
              AppTokens.s8, AppTokens.s4, AppTokens.s8, AppTokens.s4),
          child: Row(children: [
            IconButton(
              tooltip: context.tr('Back'),
              icon: const Icon(Icons.arrow_back, size: 16),
              onPressed: () => setState(() => _openPath = null),
            ),
            Expanded(
              child: Text(name,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                      color: c.textPrimary,
                      fontSize: 12,
                      fontWeight: FontWeight.w600)),
            ),
          ]),
        ),
        const Divider(height: 1),
        Expanded(
          child: SingleChildScrollView(
            padding: const EdgeInsets.all(AppTokens.s12),
            child: SelectableText(
              _content.isEmpty ? context.tr('(empty file)') : _content,
              style: const TextStyle(
                  fontSize: 12, height: 1.4, fontFamily: AppTokens.fontMono),
            ),
          ),
        ),
        if (_truncated)
          Container(
            width: double.infinity,
            padding: const EdgeInsets.all(AppTokens.s8),
            color: AppTokens.warning.withValues(alpha: 0.12),
            child: Text(context.tr('Truncated to 512 KB'),
                style: TextStyle(color: c.textMuted, fontSize: 11)),
          ),
      ],
    );
  }
}

/// Apps tab — a compact launcher for Space Apps that opens the selected app
/// INSIDE the dock. State is shared with the main Apps screen via
/// [runningAppsProvider], so opening here marks the app running there too (and
/// vice-versa). A `dock-` instanceKey keeps this webview separate from the
/// main surface's so the same app can render in both without clobbering.
class _AppsDockTab extends ConsumerWidget {
  const _AppsDockTab();
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final appsAsync = ref.watch(spaceAppsProvider);
    final running = ref.watch(runningAppsProvider);
    final theme =
        Theme.of(context).brightness == Brightness.dark ? 'dark' : 'light';
    return appsAsync.when(
      loading: () => const Center(child: CircularProgressIndicator()),
      error: (e, _) => Center(
          child: Text('$e', style: TextStyle(color: c.textMuted, fontSize: 12))),
      data: (apps) {
        final matches = apps.where((a) => a.id == running.activeId);
        final active = matches.isEmpty ? null : matches.first;
        if (active != null) {
          return Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Padding(
                padding: const EdgeInsets.fromLTRB(
                    AppTokens.s8, AppTokens.s4, AppTokens.s4, AppTokens.s4),
                child: Row(children: [
                  IconButton(
                    tooltip: context.tr('All apps'),
                    icon: const Icon(Icons.grid_view, size: 16),
                    onPressed: () =>
                        ref.read(runningAppsProvider.notifier).background(),
                  ),
                  Expanded(
                    child: Text('${active.icon}  ${active.name}',
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: TextStyle(
                            color: c.textPrimary,
                            fontSize: 12,
                            fontWeight: FontWeight.w600)),
                  ),
                  IconButton(
                    tooltip: context.tr('Close app'),
                    icon: const Icon(Icons.close, size: 16),
                    onPressed: () =>
                        ref.read(runningAppsProvider.notifier).close(active.id),
                  ),
                ]),
              ),
              const Divider(height: 1),
              Expanded(
                child: embeddedWebView(active.url,
                    title: active.name,
                    theme: theme,
                    instanceKey: 'dock-${active.id}'),
              ),
            ],
          );
        }
        if (apps.isEmpty) {
          return Center(
              child: Text(context.tr('No apps installed'),
                  style: TextStyle(color: c.textMuted, fontSize: 12)));
        }
        return ListView(
          padding: const EdgeInsets.symmetric(vertical: AppTokens.s4),
          children: [
            for (final a in apps)
              ListTile(
                dense: true,
                leading: Text(a.icon, style: const TextStyle(fontSize: 18)),
                title: Text(a.name,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(color: c.textPrimary, fontSize: 13)),
                subtitle: a.description.isEmpty
                    ? null
                    : Text(a.description,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: TextStyle(color: c.textMuted, fontSize: 11)),
                trailing: running.isRunning(a.id)
                    ? const Icon(Icons.circle, size: 8, color: AppTokens.success)
                    : null,
                onTap: () => ref.read(runningAppsProvider.notifier).open(a),
              ),
          ],
        );
      },
    );
  }
}

/// Terminal tab — an interactive shell (PTY over WebSocket) rooted at the
/// current chat session's workspace dir (falls back to $HOME server-side).
/// Keyed by jid in the parent so switching sessions starts a fresh shell.
class _TerminalTab extends ConsumerStatefulWidget {
  const _TerminalTab({super.key, required this.jid});
  final String jid;
  @override
  ConsumerState<_TerminalTab> createState() => _TerminalTabState();
}

class _TerminalTabState extends ConsumerState<_TerminalTab> {
  late final Terminal _terminal;
  WebSocketChannel? _channel;
  StreamSubscription<dynamic>? _sub;
  bool _disposed = false;

  @override
  void initState() {
    super.initState();
    _terminal = Terminal(maxLines: 10000);
    _connect();
  }

  void _connect() {
    final cfg = ref.read(appConfigProvider);
    final wsBase = cfg.httpBase.replaceFirst('http', 'ws');
    final groups = ref.read(groupsProvider);
    final match = groups.where((g) => g.jid == widget.jid);
    final cwd = match.isEmpty ? null : match.first.workDir;
    // API token as query param: this is a WS upgrade, so no header slot, and
    // a LAN-exposed daemon gates /api/ws/terminal like any other /api route.
    final tok = cfg.apiToken;
    final query = <String>[
      if (cwd != null) 'cwd=${Uri.encodeComponent(cwd)}',
      if (tok != null && tok.isNotEmpty) 'token=${Uri.encodeComponent(tok)}',
    ].join('&');
    final uri = Uri.parse('$wsBase/api/ws/terminal${query.isEmpty ? '' : '?$query'}');
    try {
      final ch = WebSocketChannel.connect(uri);
      _channel = ch;
      // Keystrokes → PTY (binary so the server never confuses them with the
      // JSON resize control); resize → JSON text frame.
      _terminal.onOutput = (data) => ch.sink.add(utf8.encode(data));
      _terminal.onResize = (w, h, pw, ph) =>
          ch.sink.add(jsonEncode({'type': 'resize', 'cols': w, 'rows': h}));
      _sub = ch.stream.listen(
        (event) {
          if (event is String) {
            _terminal.write(event);
          } else if (event is List<int>) {
            _terminal.write(utf8.decode(event, allowMalformed: true));
          }
        },
        onDone: () {
          if (!_disposed) _terminal.write('\r\n\x1b[90m[session ended]\x1b[0m\r\n');
        },
        onError: (_) {},
      );
    } catch (e) {
      _terminal.write('Failed to connect: $e\r\n');
    }
  }

  @override
  void dispose() {
    _disposed = true;
    _sub?.cancel();
    _channel?.sink.close();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return TerminalView(
      _terminal,
      padding: const EdgeInsets.all(AppTokens.s8),
    );
  }
}
