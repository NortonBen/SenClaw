import 'package:flutter/material.dart';
import 'package:webview_flutter/webview_flutter.dart';

import '../../models/workbench_models.dart';
import '../../services/language_service.dart';
import '../../services/workbench_api.dart';
import '../../services/workbench_store.dart';
import '../../theme/tokens.dart';
import '../../util/format.dart';
import '../../widgets/markdown_text.dart';
import '../../widgets/states.dart';

/// Artifacts an agent built during a chat: rendered pages, file sets, and
/// running local services.
///
/// The list comes from [WorkbenchStore], not from the daemon — there is no
/// list endpoint, only `workbench:new` events. So this shows what *this
/// device* received; a different device sees its own history.
class WorkbenchScreen extends StatefulWidget {
  final String jid;
  const WorkbenchScreen({super.key, required this.jid});

  @override
  State<WorkbenchScreen> createState() => _WorkbenchScreenState();
}

class _WorkbenchScreenState extends State<WorkbenchScreen> {
  final _store = WorkbenchStore();

  @override
  void initState() {
    super.initState();
    _store.loadCached(widget.jid);
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Scaffold(
      backgroundColor: c.bg,
      appBar: AppBar(
        backgroundColor: c.surface,
        elevation: 0,
        title: Text(tr('Sản phẩm', 'Workbench'),
            style: TextStyle(color: c.textPrimary)),
        actions: [
          AnimatedBuilder(
            animation: _store,
            builder: (_, _) => _store.artifactsFor(widget.jid).isEmpty
                ? const SizedBox.shrink()
                : IconButton(
                    tooltip: tr('Xoá danh sách', 'Clear list'),
                    icon: Icon(Icons.clear_all, color: c.textSecondary),
                    onPressed: () => _store.clear(widget.jid),
                  ),
          ),
        ],
      ),
      body: AnimatedBuilder(
        animation: _store,
        builder: (_, _) {
          final items = _store.artifactsFor(widget.jid);
          if (items.isEmpty) {
            return EmptyState(
              icon: Icons.build_outlined,
              message: tr('Chưa có sản phẩm nào', 'Nothing built yet'),
              hint: tr(
                'Trang, tệp và dịch vụ do agent tạo sẽ hiện ở đây. Danh sách này dựng từ sự kiện nhận được trên máy này.',
                'Pages, files and services the agent builds show up here. This list is built from events this device received.',
              ),
            );
          }
          return ListView.builder(
            padding: const EdgeInsets.fromLTRB(12, 12, 12, 32),
            itemCount: items.length,
            itemBuilder: (_, i) => _card(c, items[i]),
          );
        },
      ),
    );
  }

  Widget _card(AppColors c, WorkbenchArtifact a) {
    final status = a.process?.status;
    final statusColor = switch (status) {
      'ready' => AppTokens.success,
      'crashed' => AppTokens.danger,
      'stopped' => c.textMuted,
      'starting' => AppTokens.warning,
      _ => c.textMuted,
    };
    final unseen = !_store.isSeen(a.id);

    return Card(
      margin: const EdgeInsets.only(bottom: 8),
      color: c.surface,
      elevation: 0,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(AppTokens.rMd),
        side: BorderSide(color: unseen ? c.accent : c.border),
      ),
      child: InkWell(
        borderRadius: BorderRadius.circular(AppTokens.rMd),
        onTap: () => _open(a),
        child: Padding(
          padding: const EdgeInsets.all(12),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  Icon(
                    switch (a.mode) {
                      'web' => Icons.public,
                      'backend' => Icons.dns_outlined,
                      _ => Icons.description_outlined,
                    },
                    size: 18,
                    color: c.accent,
                  ),
                  const SizedBox(width: 8),
                  Expanded(
                    child: Text(
                      a.title.isEmpty ? a.id : a.title,
                      style: TextStyle(
                        color: c.textPrimary,
                        fontWeight: FontWeight.w600,
                        fontSize: 14,
                      ),
                    ),
                  ),
                  if (status != null)
                    Container(
                      padding: const EdgeInsets.symmetric(
                          horizontal: 7, vertical: 2),
                      decoration: BoxDecoration(
                        color: statusColor.withValues(alpha: 0.14),
                        borderRadius: BorderRadius.circular(4),
                      ),
                      child: Text(status,
                          style:
                              TextStyle(color: statusColor, fontSize: 10.5)),
                    ),
                  PopupMenuButton<String>(
                    icon: Icon(Icons.more_vert, size: 18, color: c.textMuted),
                    color: c.surface,
                    onSelected: (v) {
                      switch (v) {
                        case 'logs':
                          _showLogs(a);
                        case 'close':
                          _close(a);
                      }
                    },
                    itemBuilder: (_) => [
                      if (a.mode == 'backend')
                        PopupMenuItem(
                          value: 'logs',
                          child: Text(tr('Xem log', 'View logs')),
                        ),
                      PopupMenuItem(
                        value: 'close',
                        child: Text(tr('Đóng', 'Close'),
                            style: const TextStyle(color: AppTokens.danger)),
                      ),
                    ],
                  ),
                ],
              ),
              const SizedBox(height: 4),
              Text(
                [
                  a.mode,
                  if (a.files.isNotEmpty)
                    tr('${a.files.length} tệp', '${a.files.length} files'),
                  if (a.createdAt > 0) timeAgoEpochMs(a.createdAt),
                ].join(' · '),
                style: TextStyle(color: c.textMuted, fontSize: 11.5),
              ),
            ],
          ),
        ),
      ),
    );
  }

  void _open(WorkbenchArtifact a) {
    _store.markSeen(widget.jid, a.id);
    // Best-effort: the daemon's unread bookkeeping is separate from the local
    // seen set, and a failure there must not stop the viewer opening.
    WorkbenchApi().markViewed(widget.jid, a.id).catchError((_) => false);
    Navigator.of(context).push(MaterialPageRoute(
      builder: (_) => WorkbenchViewerScreen(jid: widget.jid, artifact: a),
    ));
  }

  Future<void> _close(WorkbenchArtifact a) async {
    // Drop it locally either way: the event that created it is never resent,
    // so leaving it listed after a failed close strands a dead row forever.
    try {
      await WorkbenchApi().close(widget.jid, a.id);
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text(tr('Đóng trên daemon lỗi: $e',
              'Close failed on the daemon: $e'))),
        );
      }
    }
    _store.remove(widget.jid, a.id);
  }

  Future<void> _showLogs(WorkbenchArtifact a) async {
    final c = context.colors;
    String text;
    try {
      text = await WorkbenchApi().logs(widget.jid, a.id);
    } catch (e) {
      text = '$e';
    }
    if (!mounted) return;
    showModalBottomSheet(
      context: context,
      backgroundColor: c.surface,
      isScrollControlled: true,
      shape: const RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(top: Radius.circular(14)),
      ),
      builder: (_) => DraggableScrollableSheet(
        expand: false,
        initialChildSize: 0.7,
        builder: (_, controller) => ListView(
          controller: controller,
          padding: const EdgeInsets.all(16),
          children: [
            Text(tr('Log', 'Logs'),
                style: TextStyle(
                    color: c.textPrimary, fontWeight: FontWeight.w600)),
            const SizedBox(height: 10),
            SelectableText(
              text.isEmpty ? tr('(trống)', '(empty)') : text,
              style: TextStyle(
                  color: c.textSecondary,
                  fontSize: 11.5,
                  fontFamily: 'monospace'),
            ),
          ],
        ),
      ),
    );
  }
}

/// Render one artifact: a live URL in a webview, an HTML file in a webview,
/// markdown through the app's renderer, anything else as text.
class WorkbenchViewerScreen extends StatefulWidget {
  final String jid;
  final WorkbenchArtifact artifact;

  const WorkbenchViewerScreen({
    super.key,
    required this.jid,
    required this.artifact,
  });

  @override
  State<WorkbenchViewerScreen> createState() => _WorkbenchViewerScreenState();
}

class _WorkbenchViewerScreenState extends State<WorkbenchViewerScreen> {
  WebViewController? _web;
  WorkbenchFile? _file;
  String? _content;
  String? _error;
  bool _loading = true;

  @override
  void initState() {
    super.initState();
    _prepare();
  }

  Future<void> _prepare() async {
    final a = widget.artifact;

    // A running service is reachable at its own URL; nothing to fetch.
    if (a.url != null && a.url!.isNotEmpty) {
      _web = WebViewController()
        ..setJavaScriptMode(JavaScriptMode.unrestricted)
        ..loadRequest(Uri.parse(a.url!));
      setState(() => _loading = false);
      return;
    }

    final file = a.primaryFile;
    if (file == null) {
      setState(() {
        _error = tr('Sản phẩm này không có tệp nào', 'This artifact has no files');
        _loading = false;
      });
      return;
    }

    var body = file.content;
    if (body == null || body.isEmpty) {
      try {
        body = await WorkbenchApi().readFile(widget.jid, a.id, file.path);
      } catch (e) {
        if (!mounted) return;
        setState(() {
          _error = '$e';
          _loading = false;
        });
        return;
      }
    }

    if (!mounted) return;
    if (file.extension == 'html' || file.extension == 'htm') {
      _web = WebViewController()
        ..setJavaScriptMode(JavaScriptMode.unrestricted)
        ..loadHtmlString(body);
    }
    setState(() {
      _file = file;
      _content = body;
      _loading = false;
    });
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final a = widget.artifact;
    return Scaffold(
      backgroundColor: c.bg,
      appBar: AppBar(
        backgroundColor: c.surface,
        elevation: 0,
        title: Text(a.title.isEmpty ? a.id : a.title,
            style: TextStyle(color: c.textPrimary, fontSize: 15)),
        actions: [
          if (a.files.length > 1)
            IconButton(
              tooltip: tr('Tệp khác', 'Other files'),
              icon: Icon(Icons.folder_outlined, color: c.textSecondary),
              onPressed: _pickFile,
            ),
        ],
      ),
      body: _loading
          ? const LoadingState()
          : _error != null
              ? ErrorState(message: _error!, onRetry: _prepare)
              : _web != null
                  ? WebViewWidget(controller: _web!)
                  : SingleChildScrollView(
                      padding: const EdgeInsets.all(14),
                      child: _file?.extension == 'md'
                          ? MarkdownText(_content ?? '')
                          : SelectableText(
                              _content ?? '',
                              style: TextStyle(
                                color: c.textSecondary,
                                fontSize: 12.5,
                                fontFamily: 'monospace',
                              ),
                            ),
                    ),
    );
  }

  Future<void> _pickFile() async {
    final c = context.colors;
    final picked = await showModalBottomSheet<WorkbenchFile>(
      context: context,
      backgroundColor: c.surface,
      builder: (_) => SafeArea(
        child: ListView(
          shrinkWrap: true,
          children: [
            for (final f in widget.artifact.files)
              ListTile(
                dense: true,
                leading: Icon(Icons.insert_drive_file_outlined,
                    size: 18, color: c.textMuted),
                title: Text(f.path,
                    style: TextStyle(color: c.textPrimary, fontSize: 13)),
                onTap: () => Navigator.pop(context, f),
              ),
          ],
        ),
      ),
    );
    if (picked == null) return;

    setState(() {
      _loading = true;
      _error = null;
      _web = null;
      _file = picked;
    });

    var body = picked.content;
    if (body == null || body.isEmpty) {
      try {
        body = await WorkbenchApi()
            .readFile(widget.jid, widget.artifact.id, picked.path);
      } catch (e) {
        if (!mounted) return;
        setState(() {
          _error = '$e';
          _loading = false;
        });
        return;
      }
    }
    if (!mounted) return;
    if (picked.extension == 'html' || picked.extension == 'htm') {
      _web = WebViewController()
        ..setJavaScriptMode(JavaScriptMode.unrestricted)
        ..loadHtmlString(body);
    }
    setState(() {
      _content = body;
      _loading = false;
    });
  }
}
