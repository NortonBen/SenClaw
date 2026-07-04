import 'dart:async';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import '../../models/api_models.dart';
import '../../models/code_models.dart';
import '../../services/code_api.dart';
import '../../services/language_service.dart';
import '../../services/relay_manager.dart';
import '../../theme/tokens.dart';
import '../../widgets/markdown_text.dart';
import '../../widgets/states.dart';

/// Detail view for a single code session: Chat with the code agent, browse the
/// file tree, and inspect/rollback git history.
class CodeSessionScreen extends StatefulWidget {
  final CodeSession session;
  const CodeSessionScreen({super.key, required this.session});

  @override
  State<CodeSessionScreen> createState() => _CodeSessionScreenState();
}

class _CodeSessionScreenState extends State<CodeSessionScreen>
    with SingleTickerProviderStateMixin {
  late final TabController _tabs = TabController(length: 3, vsync: this);

  @override
  void dispose() {
    _tabs.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Scaffold(
      backgroundColor: c.bg,
      appBar: AppBar(
        backgroundColor: c.surface,
        elevation: 0,
        iconTheme: IconThemeData(color: c.textPrimary),
        title: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(widget.session.name,
                style: TextStyle(color: c.textPrimary, fontSize: 15)),
            Text(
              widget.session.workspace,
              style: TextStyle(color: c.textMuted, fontSize: 10),
              overflow: TextOverflow.ellipsis,
            ),
          ],
        ),
        bottom: TabBar(
          controller: _tabs,
          indicatorColor: c.accent,
          labelColor: c.accent,
          unselectedLabelColor: c.textSecondary,
          tabs: const [
            Tab(icon: Icon(Icons.chat_outlined), text: 'Chat'),
            Tab(icon: Icon(Icons.account_tree_outlined), text: 'Files'),
            Tab(icon: Icon(Icons.history), text: 'Git'),
          ],
        ),
      ),
      body: Container(
        decoration: BoxDecoration(color: c.bg),
        child: TabBarView(
          controller: _tabs,
          children: [
            _CodeChatTab(session: widget.session),
            _FilesTab(session: widget.session),
            _GitTab(session: widget.session),
          ],
        ),
      ),
    );
  }
}

// ─── Chat tab ────────────────────────────────────────────────────────────────

class _CodeChatTab extends StatefulWidget {
  final CodeSession session;
  const _CodeChatTab({required this.session});

  @override
  State<_CodeChatTab> createState() => _CodeChatTabState();
}

class _CodeChatTabState extends State<_CodeChatTab>
    with AutomaticKeepAliveClientMixin {
  final _api = CodeApi();
  final _inputCtrl = TextEditingController();
  final _scroll = ScrollController();

  CodeChatGroup? _group;
  List<CodeChatMessage> _messages = [];
  bool _loading = true;
  bool _sending = false;
  String? _error;
  StreamSubscription? _eventSub;

  @override
  bool get wantKeepAlive => true;

  @override
  void initState() {
    super.initState();
    _init();
  }

  @override
  void dispose() {
    _eventSub?.cancel();
    _inputCtrl.dispose();
    _scroll.dispose();
    super.dispose();
  }

  Future<void> _init() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final group = await _api.ensureDefaultGroup(widget.session.id);
      final messages = await _api.groupMessages(group.id);
      if (!mounted) return;
      setState(() {
        _group = group;
        _messages = messages;
        _loading = false;
      });
      _scrollToBottom();
      // Live updates pushed over the relay (no polling).
      _eventSub ??= RelayManager().relay?.apiEvents.listen(_onCodeEvent);
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = '$e';
        _loading = false;
      });
    }
  }

  Future<void> _refreshMessages() async {
    final group = _group;
    if (group == null) return;
    try {
      final messages = await _api.groupMessages(group.id);
      if (!mounted) return;
      setState(() => _messages = messages);
    } catch (_) {
      // transient; keep last good state
    }
  }

  /// Live `code:chat:update` pushed over the relay (replaces polling).
  void _onCodeEvent(ApiEvent event) {
    if (!mounted || event.topic != 'code:chat:update') return;
    final data = event.data;
    if (data is! Map) return;
    final m = data.cast<String, dynamic>();
    if ((m['group_id'] ?? '').toString() != _group?.id) return;
    final msgs = ((m['messages'] as List?) ?? const [])
        .map((e) => CodeChatMessage.fromJson(e as Map<String, dynamic>))
        .toList();
    setState(() => _messages = msgs);
    _scrollToBottom();
  }

  Future<void> _send() async {
    final text = _inputCtrl.text.trim();
    final group = _group;
    if (text.isEmpty || group == null || _sending) return;
    setState(() => _sending = true);
    try {
      await _api.sendChat(
        sessionId: widget.session.id,
        groupId: group.id,
        prompt: text,
      );
      _inputCtrl.clear();
      await _refreshMessages();
      _scrollToBottom();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(
                content: Text(tr('Lỗi gửi: $e', 'Send failed: $e'))));
      }
    } finally {
      if (mounted) setState(() => _sending = false);
    }
  }

  Future<void> _stop() async {
    final group = _group;
    if (group == null) return;
    try {
      await _api.stopCurrent(group.id);
      await _refreshMessages();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(
                content: Text(tr('Lỗi dừng: $e', 'Stop failed: $e'))));
      }
    }
  }

  void _scrollToBottom() {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (_scroll.hasClients) {
        _scroll.animateTo(
          _scroll.position.maxScrollExtent,
          duration: const Duration(milliseconds: 250),
          curve: Curves.easeOut,
        );
      }
    });
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    final c = context.colors;
    final processing = _messages.any((m) => m.isPending);
    return Column(
      children: [
        Expanded(
          child: _loading
              ? LoadingState(
                  text: tr('Đang tải hội thoại…', 'Loading conversation…'))
              : _error != null
                  ? ErrorState(message: _error!, onRetry: _init)
                  : _messages.isEmpty
                      ? EmptyState(
                          icon: Icons.chat_bubble_outline,
                          message: tr('Bắt đầu hội thoại với code agent',
                              'Start a conversation with the code agent'),
                          hint: tr('Yêu cầu agent đọc, sửa hoặc chạy code',
                              'Ask the agent to read, edit or run code'),
                        )
                      : RefreshIndicator(
                          onRefresh: _refreshMessages,
                          color: c.accent,
                          backgroundColor: c.surface,
                          child: ListView.builder(
                            controller: _scroll,
                            padding: const EdgeInsets.all(12),
                            itemCount: _messages.length,
                            itemBuilder: (ctx, i) => _bubble(_messages[i]),
                          ),
                        ),
        ),
        if (processing)
          Container(
            width: double.infinity,
            color: c.accent.withValues(alpha: 0.08),
            padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 6),
            child: Row(
              children: [
                SizedBox(
                  width: 12,
                  height: 12,
                  child: CircularProgressIndicator(
                    strokeWidth: 2,
                    valueColor:
                        AlwaysStoppedAnimation<Color>(c.accent),
                  ),
                ),
                const SizedBox(width: 10),
                Expanded(
                  child: Text(tr('Agent đang xử lý…', 'Agent is working…'),
                      style: TextStyle(color: c.textSecondary, fontSize: 12)),
                ),
                TextButton(
                  onPressed: _stop,
                  child: Text(tr('Dừng', 'Stop'),
                      style: TextStyle(color: AppTokens.danger, fontSize: 12)),
                ),
              ],
            ),
          ),
        _inputArea(),
      ],
    );
  }

  Widget _bubble(CodeChatMessage m) {
    final c = context.colors;
    final isUser = m.isUser;
    return Align(
      alignment: isUser ? Alignment.centerRight : Alignment.centerLeft,
      child: Container(
        margin: const EdgeInsets.symmetric(vertical: 4),
        constraints: BoxConstraints(
          maxWidth: MediaQuery.of(context).size.width * 0.82,
        ),
        padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
        decoration: BoxDecoration(
          color: isUser ? c.bubbleUser : c.bubbleAgent,
          borderRadius: BorderRadius.circular(14),
          border: Border.all(
            color: isUser
                ? c.accent.withValues(alpha: 0.3)
                : c.border,
          ),
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            if (!isUser)
              Padding(
                padding: const EdgeInsets.only(bottom: 4),
                child: Text(
                  m.role,
                  style: TextStyle(
                    color: AppTokens.cyan,
                    fontSize: 10,
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ),
            if (isUser || (m.content.isEmpty && m.isPending))
              SelectableText(
                m.content.isEmpty && m.isPending ? '…' : m.content,
                style: TextStyle(
                  color: m.isPending ? c.textSecondary : c.textPrimary,
                  fontSize: 13,
                  height: 1.35,
                ),
              )
            else
              MarkdownText(
                m.content,
                color: m.isPending ? c.textSecondary : c.textPrimary,
                fontSize: 13,
              ),
            if (m.isPending) ...[
              const SizedBox(height: 4),
              Text(
                m.status == 'queued'
                    ? tr(
                        'Đang chờ${m.queuePosition != null ? ' (#${m.queuePosition})' : ''}…',
                        'Waiting${m.queuePosition != null ? ' (#${m.queuePosition})' : ''}…')
                    : tr('Đang xử lý…', 'Processing…'),
                style: TextStyle(color: c.textMuted, fontSize: 10),
              ),
            ],
          ],
        ),
      ),
    );
  }

  Widget _inputArea() {
    final c = context.colors;
    final enabled = _group != null;
    return Container(
      padding: const EdgeInsets.fromLTRB(12, 8, 8, 12),
      decoration: BoxDecoration(
        color: c.surface,
        border: Border(
          top: BorderSide(color: c.border),
        ),
      ),
      child: SafeArea(
        top: false,
        child: Row(
          children: [
            Expanded(
              child: TextField(
                controller: _inputCtrl,
                enabled: enabled && !_sending,
                minLines: 1,
                maxLines: 5,
                style: TextStyle(color: c.textPrimary, fontSize: 14),
                decoration: InputDecoration(
                  hintText:
                      tr('Nhắn cho code agent…', 'Message the code agent…'),
                  hintStyle: TextStyle(color: c.textMuted),
                  border: InputBorder.none,
                ),
              ),
            ),
            IconButton(
              icon: _sending
                  ? SizedBox(
                      width: 18,
                      height: 18,
                      child: CircularProgressIndicator(
                          strokeWidth: 2, color: c.accent),
                    )
                  : Icon(Icons.send,
                      color: enabled ? c.accent : c.textMuted),
              onPressed: enabled && !_sending ? _send : null,
            ),
          ],
        ),
      ),
    );
  }
}

// ─── Files tab ───────────────────────────────────────────────────────────────

class _FilesTab extends StatefulWidget {
  final CodeSession session;
  const _FilesTab({required this.session});

  @override
  State<_FilesTab> createState() => _FilesTabState();
}

class _FilesTabState extends State<_FilesTab>
    with AutomaticKeepAliveClientMixin {
  final _api = CodeApi();
  List<FileNode> _tree = [];
  bool _loading = true;
  String? _error;

  @override
  bool get wantKeepAlive => true;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final (_, tree) = await _api.listFiles(widget.session.id);
      if (!mounted) return;
      setState(() {
        _tree = tree;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = '$e';
        _loading = false;
      });
    }
  }

  Future<void> _openFile(FileNode f) async {
    showModalBottomSheet(
      context: context,
      isScrollControlled: true,
      backgroundColor: context.colors.surface,
      shape: const RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(top: Radius.circular(18)),
      ),
      builder: (_) => _FileViewer(sessionId: widget.session.id, file: f),
    );
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    final c = context.colors;
    if (_loading) {
      return LoadingState(
          text: tr('Đang tải cây thư mục…', 'Loading file tree…'));
    }
    if (_error != null) return ErrorState(message: _error!, onRetry: _load);
    if (_tree.isEmpty) {
      return EmptyState(
        icon: Icons.folder_off_outlined,
        message: tr('Thư mục trống', 'Folder is empty'),
        action: OutlinedButton.icon(
          onPressed: _load,
          icon: Icon(Icons.refresh, color: c.accent, size: 18),
          label: Text(tr('Tải lại', 'Reload'),
              style: TextStyle(color: c.accent)),
          style: OutlinedButton.styleFrom(
            side: BorderSide(color: c.accent),
          ),
        ),
      );
    }
    return RefreshIndicator(
      onRefresh: _load,
      color: c.accent,
      backgroundColor: c.surface,
      child: ListView(
        padding: const EdgeInsets.symmetric(vertical: 8),
        children: _tree.map((n) => _node(n, 0)).toList(),
      ),
    );
  }

  Widget _node(FileNode n, int depth) {
    final c = context.colors;
    if (n.isDir) {
      return Theme(
        data: Theme.of(context).copyWith(dividerColor: Colors.transparent),
        child: ExpansionTile(
          tilePadding: EdgeInsets.only(left: 16.0 + depth * 14, right: 16),
          leading: Icon(Icons.folder, color: AppTokens.warning, size: 20),
          title: Text(n.name,
              style: TextStyle(color: c.textPrimary, fontSize: 13)),
          iconColor: c.textSecondary,
          collapsedIconColor: c.textSecondary,
          childrenPadding: EdgeInsets.zero,
          children: n.children.map((c) => _node(c, depth + 1)).toList(),
        ),
      );
    }
    return ListTile(
      contentPadding: EdgeInsets.only(left: 28.0 + depth * 14, right: 16),
      dense: true,
      leading: Icon(_fileIcon(n.name), color: c.textMuted, size: 18),
      title: Text(n.name,
          style: TextStyle(color: c.textSecondary, fontSize: 13)),
      onTap: () => _openFile(n),
    );
  }

  IconData _fileIcon(String name) {
    final lower = name.toLowerCase();
    if (lower.endsWith('.dart')) return Icons.flutter_dash;
    if (lower.endsWith('.md')) return Icons.article_outlined;
    if (lower.endsWith('.json') ||
        lower.endsWith('.yaml') ||
        lower.endsWith('.yml') ||
        lower.endsWith('.toml')) {
      return Icons.data_object;
    }
    return Icons.insert_drive_file_outlined;
  }
}

class _FileViewer extends StatefulWidget {
  final String sessionId;
  final FileNode file;
  const _FileViewer({required this.sessionId, required this.file});

  @override
  State<_FileViewer> createState() => _FileViewerState();
}

class _FileViewerState extends State<_FileViewer> {
  final _api = CodeApi();
  String? _content;
  bool _loading = true;
  String? _error;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    try {
      final content = await _api.fileContent(widget.sessionId, widget.file.path);
      if (!mounted) return;
      setState(() {
        _content = content;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = '$e';
        _loading = false;
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return FractionallySizedBox(
      heightFactor: 0.9,
      child: Column(
        children: [
          const SizedBox(height: 10),
          Container(
            width: 40,
            height: 4,
            decoration: BoxDecoration(
              color: c.borderStrong,
              borderRadius: BorderRadius.circular(2),
            ),
          ),
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 12, 8, 8),
            child: Row(
              children: [
                Icon(Icons.description_outlined,
                    color: AppTokens.cyan, size: 18),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(
                    widget.file.path,
                    style: TextStyle(color: c.textPrimary, fontSize: 13),
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
                if (_content != null)
                  IconButton(
                    icon: Icon(Icons.copy, color: c.textSecondary, size: 18),
                    onPressed: () {
                      Clipboard.setData(ClipboardData(text: _content!));
                      ScaffoldMessenger.of(context).showSnackBar(
                        SnackBar(
                            content: Text(tr('Đã sao chép', 'Copied'))),
                      );
                    },
                  ),
              ],
            ),
          ),
          Divider(color: c.border, height: 1),
          Expanded(
            child: _loading
                ? const LoadingState()
                : _error != null
                    ? ErrorState(message: _error!, onRetry: _load)
                    : SingleChildScrollView(
                        padding: const EdgeInsets.all(14),
                        child: SizedBox(
                          width: double.infinity,
                          child: SelectableText(
                            _content!.isEmpty
                                ? tr('(tệp trống)', '(empty file)')
                                : _content!,
                            style: TextStyle(
                              color: c.textSecondary,
                              fontFamily: 'monospace',
                              fontSize: 12,
                              height: 1.4,
                            ),
                          ),
                        ),
                      ),
          ),
        ],
      ),
    );
  }
}

// ─── Git tab ─────────────────────────────────────────────────────────────────

class _GitTab extends StatefulWidget {
  final CodeSession session;
  const _GitTab({required this.session});

  @override
  State<_GitTab> createState() => _GitTabState();
}

class _GitTabState extends State<_GitTab> with AutomaticKeepAliveClientMixin {
  final _api = CodeApi();
  List<GitCommit> _log = [];
  bool _loading = true;
  String? _error;

  @override
  bool get wantKeepAlive => true;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final log = await _api.gitLog(widget.session.id);
      if (!mounted) return;
      setState(() {
        _log = log;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = '$e';
        _loading = false;
      });
    }
  }

  Future<void> _rollback() async {
    final c = context.colors;
    final ctrl = TextEditingController(text: '1');
    final steps = await showDialog<int>(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: c.surface,
        title: Text(tr('Rollback commit', 'Rollback commits'),
            style: TextStyle(color: c.textPrimary)),
        content: TextField(
          controller: ctrl,
          keyboardType: TextInputType.number,
          style: TextStyle(color: c.textPrimary),
          decoration: InputDecoration(
            labelText:
                tr('Số commit lùi lại', 'Number of commits to roll back'),
            labelStyle: TextStyle(color: c.textSecondary),
          ),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(ctx),
            child: Text(tr('Huỷ', 'Cancel')),
          ),
          TextButton(
            onPressed: () =>
                Navigator.pop(ctx, int.tryParse(ctrl.text.trim()) ?? 0),
            child: Text('Rollback',
                style: TextStyle(color: AppTokens.danger)),
          ),
        ],
      ),
    );
    if (steps == null || steps <= 0) return;
    try {
      await _api.rollback(widget.session.id, steps);
      await _load();
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
              content: Text(tr('Đã rollback $steps commit',
                  'Rolled back $steps commit(s)'))),
        );
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(
                content: Text(tr('Lỗi rollback: $e', 'Rollback failed: $e'))));
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    final c = context.colors;
    if (_loading) {
      return LoadingState(text: tr('Đang tải git log…', 'Loading git log…'));
    }
    if (_error != null) return ErrorState(message: _error!, onRetry: _load);
    if (!widget.session.gitEnabled) {
      return EmptyState(
        icon: Icons.source_outlined,
        message:
            tr('Session này không bật git', 'Git is not enabled for this session'),
      );
    }
    if (_log.isEmpty) {
      return EmptyState(
        icon: Icons.source_outlined,
        message: tr('Chưa có commit', 'No commits yet'),
        action: OutlinedButton.icon(
          onPressed: _load,
          icon: Icon(Icons.refresh, color: c.accent, size: 18),
          label: Text(tr('Tải lại', 'Reload'),
              style: TextStyle(color: c.accent)),
          style: OutlinedButton.styleFrom(
            side: BorderSide(color: c.accent),
          ),
        ),
      );
    }
    return Column(
      children: [
        Expanded(
          child: RefreshIndicator(
            onRefresh: _load,
            color: c.accent,
            backgroundColor: c.surface,
            child: ListView.builder(
              padding: const EdgeInsets.all(12),
              itemCount: _log.length,
              itemBuilder: (ctx, i) => _commitTile(_log[i], i == 0),
            ),
          ),
        ),
        SafeArea(
          top: false,
          child: Padding(
            padding: const EdgeInsets.all(12),
            child: SizedBox(
              width: double.infinity,
              child: OutlinedButton.icon(
                onPressed: _rollback,
                icon: Icon(Icons.undo, color: AppTokens.danger),
                label: Text('Rollback…',
                    style: TextStyle(color: AppTokens.danger)),
                style: OutlinedButton.styleFrom(
                  side: BorderSide(color: AppTokens.danger),
                  padding: const EdgeInsets.symmetric(vertical: 12),
                ),
              ),
            ),
          ),
        ),
      ],
    );
  }

  Widget _commitTile(GitCommit commit, bool isHead) {
    final c = context.colors;
    return Container(
      margin: const EdgeInsets.only(bottom: 8),
      padding: const EdgeInsets.all(12),
      decoration: BoxDecoration(
        color: c.surfaceAlt,
        borderRadius: BorderRadius.circular(10),
        border: Border.all(color: c.border),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Container(
                padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
                decoration: BoxDecoration(
                  color: AppTokens.cyan.withValues(alpha: 0.12),
                  borderRadius: BorderRadius.circular(4),
                ),
                child: Text(
                  commit.shortHash,
                  style: TextStyle(
                    color: AppTokens.cyan,
                    fontFamily: 'monospace',
                    fontSize: 11,
                  ),
                ),
              ),
              if (isHead) ...[
                const SizedBox(width: 6),
                Container(
                  padding:
                      const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
                  decoration: BoxDecoration(
                    color: AppTokens.success.withValues(alpha: 0.15),
                    borderRadius: BorderRadius.circular(4),
                  ),
                  child: Text('HEAD',
                      style: TextStyle(color: AppTokens.success, fontSize: 10)),
                ),
              ],
              const Spacer(),
              Text(
                commit.date.split(' ').first,
                style: TextStyle(color: c.textMuted, fontSize: 10),
              ),
            ],
          ),
          const SizedBox(height: 6),
          Text(commit.message,
              style: TextStyle(color: c.textSecondary, fontSize: 13)),
        ],
      ),
    );
  }
}
