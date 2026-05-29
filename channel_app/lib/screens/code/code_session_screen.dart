import 'dart:async';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import '../../models/code_models.dart';
import '../../services/code_api.dart';
import '../../theme/app_colors.dart';
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
    return Scaffold(
      backgroundColor: AppColors.bg,
      appBar: AppBar(
        backgroundColor: AppColors.surface,
        elevation: 0,
        title: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(widget.session.name,
                style: const TextStyle(color: Colors.white, fontSize: 15)),
            Text(
              widget.session.workspace,
              style: const TextStyle(color: Colors.white38, fontSize: 10),
              overflow: TextOverflow.ellipsis,
            ),
          ],
        ),
        bottom: TabBar(
          controller: _tabs,
          indicatorColor: AppColors.accent,
          labelColor: AppColors.accent,
          unselectedLabelColor: Colors.white54,
          tabs: const [
            Tab(icon: Icon(Icons.chat_outlined), text: 'Chat'),
            Tab(icon: Icon(Icons.account_tree_outlined), text: 'Files'),
            Tab(icon: Icon(Icons.history), text: 'Git'),
          ],
        ),
      ),
      body: Container(
        decoration: AppColors.pageDecoration,
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
  Timer? _poll;

  @override
  bool get wantKeepAlive => true;

  @override
  void initState() {
    super.initState();
    _init();
  }

  @override
  void dispose() {
    _poll?.cancel();
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
      _maybeStartPolling();
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
      _maybeStartPolling();
    } catch (_) {
      // transient; keep last good state
    }
  }

  /// Poll while any message is queued/processing (no WS push over relay yet).
  void _maybeStartPolling() {
    final hasPending = _messages.any((m) => m.isPending);
    if (hasPending) {
      _poll ??= Timer.periodic(const Duration(seconds: 2), (_) {
        _refreshMessages();
      });
    } else {
      _poll?.cancel();
      _poll = null;
    }
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
            .showSnackBar(SnackBar(content: Text('Lỗi gửi: $e')));
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
            .showSnackBar(SnackBar(content: Text('Lỗi dừng: $e')));
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
    final processing = _messages.any((m) => m.isPending);
    return Column(
      children: [
        Expanded(
          child: _loading
              ? const LoadingState(text: 'Đang tải hội thoại…')
              : _error != null
                  ? ErrorState(message: _error!, onRetry: _init)
                  : _messages.isEmpty
                      ? EmptyState(
                          icon: Icons.chat_bubble_outline,
                          message: 'Bắt đầu hội thoại với code agent',
                          hint: 'Yêu cầu agent đọc, sửa hoặc chạy code',
                        )
                      : RefreshIndicator(
                          onRefresh: _refreshMessages,
                          color: AppColors.accent,
                          backgroundColor: AppColors.surface,
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
            color: AppColors.accent.withValues(alpha: 0.08),
            padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 6),
            child: Row(
              children: [
                const SizedBox(
                  width: 12,
                  height: 12,
                  child: CircularProgressIndicator(
                    strokeWidth: 2,
                    valueColor:
                        AlwaysStoppedAnimation<Color>(AppColors.accent),
                  ),
                ),
                const SizedBox(width: 10),
                const Expanded(
                  child: Text('Agent đang xử lý…',
                      style: TextStyle(color: Colors.white54, fontSize: 12)),
                ),
                TextButton(
                  onPressed: _stop,
                  child: const Text('Dừng',
                      style: TextStyle(color: Colors.redAccent, fontSize: 12)),
                ),
              ],
            ),
          ),
        _inputArea(),
      ],
    );
  }

  Widget _bubble(CodeChatMessage m) {
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
          color: isUser
              ? AppColors.accent.withValues(alpha: 0.16)
              : Colors.white.withValues(alpha: 0.06),
          borderRadius: BorderRadius.circular(14),
          border: Border.all(
            color: isUser
                ? AppColors.accent.withValues(alpha: 0.3)
                : AppColors.cardBorder,
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
                  style: const TextStyle(
                    color: AppColors.cyan,
                    fontSize: 10,
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ),
            if (isUser || (m.content.isEmpty && m.isPending))
              SelectableText(
                m.content.isEmpty && m.isPending ? '…' : m.content,
                style: TextStyle(
                  color: m.isPending ? Colors.white54 : Colors.white,
                  fontSize: 13,
                  height: 1.35,
                ),
              )
            else
              MarkdownText(
                m.content,
                color: m.isPending ? Colors.white54 : Colors.white,
                fontSize: 13,
              ),
            if (m.isPending) ...[
              const SizedBox(height: 4),
              Text(
                m.status == 'queued'
                    ? 'Đang chờ${m.queuePosition != null ? ' (#${m.queuePosition})' : ''}…'
                    : 'Đang xử lý…',
                style: const TextStyle(color: Colors.white38, fontSize: 10),
              ),
            ],
          ],
        ),
      ),
    );
  }

  Widget _inputArea() {
    final enabled = _group != null;
    return Container(
      padding: const EdgeInsets.fromLTRB(12, 8, 8, 12),
      decoration: BoxDecoration(
        color: Colors.black.withValues(alpha: 0.3),
        border: Border(
          top: BorderSide(color: Colors.white.withValues(alpha: 0.08)),
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
                style: const TextStyle(color: Colors.white, fontSize: 14),
                decoration: const InputDecoration(
                  hintText: 'Nhắn cho code agent…',
                  hintStyle: TextStyle(color: Colors.white38),
                  border: InputBorder.none,
                ),
              ),
            ),
            IconButton(
              icon: _sending
                  ? const SizedBox(
                      width: 18,
                      height: 18,
                      child: CircularProgressIndicator(
                          strokeWidth: 2, color: AppColors.accent),
                    )
                  : Icon(Icons.send,
                      color: enabled ? AppColors.accent : Colors.white24),
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
      backgroundColor: AppColors.surface,
      shape: const RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(top: Radius.circular(18)),
      ),
      builder: (_) => _FileViewer(sessionId: widget.session.id, file: f),
    );
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    if (_loading) return const LoadingState(text: 'Đang tải cây thư mục…');
    if (_error != null) return ErrorState(message: _error!, onRetry: _load);
    if (_tree.isEmpty) {
      return EmptyState(
        icon: Icons.folder_off_outlined,
        message: 'Thư mục trống',
        action: OutlinedButton.icon(
          onPressed: _load,
          icon: const Icon(Icons.refresh, color: AppColors.accent, size: 18),
          label: const Text('Tải lại', style: TextStyle(color: AppColors.accent)),
          style: OutlinedButton.styleFrom(
            side: const BorderSide(color: AppColors.accent),
          ),
        ),
      );
    }
    return RefreshIndicator(
      onRefresh: _load,
      color: AppColors.accent,
      backgroundColor: AppColors.surface,
      child: ListView(
        padding: const EdgeInsets.symmetric(vertical: 8),
        children: _tree.map((n) => _node(n, 0)).toList(),
      ),
    );
  }

  Widget _node(FileNode n, int depth) {
    if (n.isDir) {
      return Theme(
        data: Theme.of(context).copyWith(dividerColor: Colors.transparent),
        child: ExpansionTile(
          tilePadding: EdgeInsets.only(left: 16.0 + depth * 14, right: 16),
          leading: const Icon(Icons.folder, color: Color(0xFFFFB74D), size: 20),
          title: Text(n.name,
              style: const TextStyle(color: Colors.white, fontSize: 13)),
          iconColor: Colors.white54,
          collapsedIconColor: Colors.white54,
          childrenPadding: EdgeInsets.zero,
          children: n.children.map((c) => _node(c, depth + 1)).toList(),
        ),
      );
    }
    return ListTile(
      contentPadding: EdgeInsets.only(left: 28.0 + depth * 14, right: 16),
      dense: true,
      leading: Icon(_fileIcon(n.name), color: Colors.white38, size: 18),
      title: Text(n.name,
          style: const TextStyle(color: Colors.white70, fontSize: 13)),
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
    return FractionallySizedBox(
      heightFactor: 0.9,
      child: Column(
        children: [
          const SizedBox(height: 10),
          Container(
            width: 40,
            height: 4,
            decoration: BoxDecoration(
              color: Colors.white24,
              borderRadius: BorderRadius.circular(2),
            ),
          ),
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 12, 8, 8),
            child: Row(
              children: [
                const Icon(Icons.description_outlined,
                    color: AppColors.cyan, size: 18),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(
                    widget.file.path,
                    style: const TextStyle(color: Colors.white, fontSize: 13),
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
                if (_content != null)
                  IconButton(
                    icon: const Icon(Icons.copy, color: Colors.white54, size: 18),
                    onPressed: () {
                      Clipboard.setData(ClipboardData(text: _content!));
                      ScaffoldMessenger.of(context).showSnackBar(
                        const SnackBar(content: Text('Đã sao chép')),
                      );
                    },
                  ),
              ],
            ),
          ),
          const Divider(color: AppColors.cardBorder, height: 1),
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
                            _content!.isEmpty ? '(tệp trống)' : _content!,
                            style: const TextStyle(
                              color: Colors.white70,
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
    final ctrl = TextEditingController(text: '1');
    final steps = await showDialog<int>(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: AppColors.surface,
        title: const Text('Rollback commits',
            style: TextStyle(color: Colors.white)),
        content: TextField(
          controller: ctrl,
          keyboardType: TextInputType.number,
          style: const TextStyle(color: Colors.white),
          decoration: const InputDecoration(
            labelText: 'Số commit lùi lại',
            labelStyle: TextStyle(color: Colors.white54),
          ),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(ctx),
            child: const Text('Huỷ'),
          ),
          TextButton(
            onPressed: () =>
                Navigator.pop(ctx, int.tryParse(ctrl.text.trim()) ?? 0),
            child: const Text('Rollback',
                style: TextStyle(color: Colors.redAccent)),
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
          SnackBar(content: Text('Đã rollback $steps commit')),
        );
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Lỗi rollback: $e')));
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    if (_loading) return const LoadingState(text: 'Đang tải git log…');
    if (_error != null) return ErrorState(message: _error!, onRetry: _load);
    if (!widget.session.gitEnabled) {
      return const EmptyState(
        icon: Icons.source_outlined,
        message: 'Session này không bật git',
      );
    }
    if (_log.isEmpty) {
      return EmptyState(
        icon: Icons.source_outlined,
        message: 'Chưa có commit',
        action: OutlinedButton.icon(
          onPressed: _load,
          icon: const Icon(Icons.refresh, color: AppColors.accent, size: 18),
          label:
              const Text('Tải lại', style: TextStyle(color: AppColors.accent)),
          style: OutlinedButton.styleFrom(
            side: const BorderSide(color: AppColors.accent),
          ),
        ),
      );
    }
    return Column(
      children: [
        Expanded(
          child: RefreshIndicator(
            onRefresh: _load,
            color: AppColors.accent,
            backgroundColor: AppColors.surface,
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
                icon: const Icon(Icons.undo, color: Colors.redAccent),
                label: const Text('Rollback…',
                    style: TextStyle(color: Colors.redAccent)),
                style: OutlinedButton.styleFrom(
                  side: const BorderSide(color: Colors.redAccent),
                  padding: const EdgeInsets.symmetric(vertical: 12),
                ),
              ),
            ),
          ),
        ),
      ],
    );
  }

  Widget _commitTile(GitCommit c, bool isHead) {
    return Container(
      margin: const EdgeInsets.only(bottom: 8),
      padding: const EdgeInsets.all(12),
      decoration: BoxDecoration(
        color: Colors.white.withValues(alpha: 0.04),
        borderRadius: BorderRadius.circular(10),
        border: Border.all(color: AppColors.cardBorder),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Container(
                padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
                decoration: BoxDecoration(
                  color: AppColors.cyan.withValues(alpha: 0.12),
                  borderRadius: BorderRadius.circular(4),
                ),
                child: Text(
                  c.shortHash,
                  style: const TextStyle(
                    color: AppColors.cyan,
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
                    color: const Color(0xFF66BB6A).withValues(alpha: 0.15),
                    borderRadius: BorderRadius.circular(4),
                  ),
                  child: const Text('HEAD',
                      style: TextStyle(color: Color(0xFF66BB6A), fontSize: 10)),
                ),
              ],
              const Spacer(),
              Text(
                c.date.split(' ').first,
                style: const TextStyle(color: Colors.white38, fontSize: 10),
              ),
            ],
          ),
          const SizedBox(height: 6),
          Text(c.message,
              style: const TextStyle(color: Colors.white70, fontSize: 13)),
        ],
      ),
    );
  }
}
