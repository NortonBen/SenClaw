import 'package:flutter/material.dart';
import '../../models/cowork_models.dart';
import '../../services/cowork_api.dart';
import '../../theme/app_colors.dart';
import '../../widgets/states.dart';

const _taskStatuses = ['todo', 'in_progress', 'review', 'done', 'blocked'];

/// Workspace detail: Tasks (kanban-by-status) and team Chat.
class CoworkWorkspaceScreen extends StatefulWidget {
  final CoworkWorkspace workspace;
  const CoworkWorkspaceScreen({super.key, required this.workspace});

  @override
  State<CoworkWorkspaceScreen> createState() => _CoworkWorkspaceScreenState();
}

class _CoworkWorkspaceScreenState extends State<CoworkWorkspaceScreen>
    with SingleTickerProviderStateMixin {
  late final TabController _tabs = TabController(length: 4, vsync: this);

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
        title: Text(widget.workspace.name,
            style: const TextStyle(color: Colors.white, fontSize: 16)),
        bottom: TabBar(
          controller: _tabs,
          isScrollable: true,
          tabAlignment: TabAlignment.start,
          indicatorColor: AppColors.accent,
          labelColor: AppColors.accent,
          unselectedLabelColor: Colors.white54,
          tabs: const [
            Tab(icon: Icon(Icons.checklist), text: 'Tasks'),
            Tab(icon: Icon(Icons.dashboard_outlined), text: 'Board'),
            Tab(icon: Icon(Icons.forum_outlined), text: 'Chat'),
            Tab(icon: Icon(Icons.people_outline), text: 'Team'),
          ],
        ),
      ),
      body: Container(
        decoration: AppColors.pageDecoration,
        child: TabBarView(
          controller: _tabs,
          children: [
            _TasksTab(wsId: widget.workspace.id),
            _BoardTab(wsId: widget.workspace.id),
            _MessagesTab(wsId: widget.workspace.id),
            _TeamTab(wsId: widget.workspace.id),
          ],
        ),
      ),
    );
  }
}

// ─── Tasks ─────────────────────────────────────────────────────────────────

class _TasksTab extends StatefulWidget {
  final String wsId;
  const _TasksTab({required this.wsId});

  @override
  State<_TasksTab> createState() => _TasksTabState();
}

class _TasksTabState extends State<_TasksTab>
    with AutomaticKeepAliveClientMixin {
  final _api = CoworkApi();
  List<CoworkTask> _tasks = [];
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
      final tasks = await _api.listTasks(widget.wsId);
      if (!mounted) return;
      setState(() {
        _tasks = tasks;
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

  Future<void> _create() async {
    final titleCtrl = TextEditingController();
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: AppColors.surface,
        title: const Text('Task mới', style: TextStyle(color: Colors.white)),
        content: TextField(
          controller: titleCtrl,
          autofocus: true,
          style: const TextStyle(color: Colors.white),
          decoration: const InputDecoration(
            labelText: 'Tiêu đề',
            labelStyle: TextStyle(color: Colors.white54),
          ),
        ),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(ctx, false),
              child: const Text('Huỷ')),
          TextButton(
              onPressed: () => Navigator.pop(ctx, true),
              child: const Text('Tạo',
                  style: TextStyle(color: AppColors.accent))),
        ],
      ),
    );
    if (ok != true || titleCtrl.text.trim().isEmpty) return;
    try {
      await _api.createTask(widget.wsId, title: titleCtrl.text.trim());
      _load();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Lỗi tạo: $e')));
      }
    }
  }

  Future<void> _changeStatus(CoworkTask t) async {
    final newStatus = await showModalBottomSheet<String>(
      context: context,
      backgroundColor: AppColors.surface,
      shape: const RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(top: Radius.circular(18)),
      ),
      builder: (_) => SafeArea(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            const SizedBox(height: 12),
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 8),
              child: Text(t.title,
                  style: const TextStyle(color: Colors.white, fontSize: 15)),
            ),
            const Divider(color: AppColors.cardBorder, height: 1),
            for (final s in _taskStatuses)
              ListTile(
                leading: Icon(_statusIcon(s), color: _statusColor(s)),
                title: Text(_statusLabel(s),
                    style: const TextStyle(color: Colors.white)),
                trailing: t.status == s
                    ? const Icon(Icons.check, color: AppColors.accent)
                    : null,
                onTap: () => Navigator.pop(context, s),
              ),
          ],
        ),
      ),
    );
    if (newStatus == null || newStatus == t.status) return;
    try {
      await _api.updateTaskStatus(widget.wsId, t.id, newStatus);
      _load();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Lỗi cập nhật: $e')));
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    return Scaffold(
      backgroundColor: Colors.transparent,
      floatingActionButton: FloatingActionButton(
        onPressed: _create,
        backgroundColor: AppColors.accent,
        foregroundColor: Colors.black,
        child: const Icon(Icons.add),
      ),
      body: _buildBody(),
    );
  }

  Widget _buildBody() {
    if (_loading) return const LoadingState(text: 'Đang tải tasks…');
    if (_error != null) return ErrorState(message: _error!, onRetry: _load);
    if (_tasks.isEmpty) {
      return const EmptyState(
        icon: Icons.checklist_rtl,
        message: 'Chưa có task',
        hint: 'Nhấn + để tạo task cho nhóm agent',
      );
    }
    // Group by status, ordered by the canonical pipeline.
    final byStatus = <String, List<CoworkTask>>{};
    for (final t in _tasks) {
      byStatus.putIfAbsent(t.status, () => []).add(t);
    }
    final ordered = [
      ..._taskStatuses.where(byStatus.containsKey),
      ...byStatus.keys.where((k) => !_taskStatuses.contains(k)),
    ];
    return RefreshIndicator(
      onRefresh: _load,
      color: AppColors.accent,
      backgroundColor: AppColors.surface,
      child: ListView(
        padding: const EdgeInsets.fromLTRB(12, 12, 12, 88),
        children: [
          for (final st in ordered) ...[
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 8),
              child: Row(
                children: [
                  Icon(_statusIcon(st), color: _statusColor(st), size: 16),
                  const SizedBox(width: 6),
                  Text('${_statusLabel(st)}  ·  ${byStatus[st]!.length}',
                      style: const TextStyle(
                          color: Colors.white70,
                          fontSize: 13,
                          fontWeight: FontWeight.w600)),
                ],
              ),
            ),
            ...byStatus[st]!.map(_taskCard),
          ],
        ],
      ),
    );
  }

  Widget _taskCard(CoworkTask t) {
    return Card(
      color: Colors.white.withValues(alpha: 0.04),
      margin: const EdgeInsets.only(bottom: 8),
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(12),
        side: const BorderSide(color: AppColors.cardBorder),
      ),
      child: ListTile(
        title: Text(t.title,
            style: const TextStyle(color: Colors.white, fontSize: 14)),
        subtitle: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            if (t.description != null && t.description!.isNotEmpty) ...[
              const SizedBox(height: 4),
              Text(t.description!,
                  style: const TextStyle(color: Colors.white54, fontSize: 12),
                  maxLines: 2,
                  overflow: TextOverflow.ellipsis),
            ],
            const SizedBox(height: 6),
            Row(
              children: [
                if (t.assignee != null && t.assignee!.isNotEmpty)
                  _miniChip(Icons.person_outline, t.assignee!),
                _miniChip(Icons.flag_outlined, t.priority),
              ],
            ),
          ],
        ),
        trailing: TextButton(
          onPressed: () => _changeStatus(t),
          child: const Text('Status',
              style: TextStyle(color: AppColors.accent, fontSize: 12)),
        ),
        onTap: () => _changeStatus(t),
      ),
    );
  }

  Widget _miniChip(IconData icon, String label) {
    return Padding(
      padding: const EdgeInsets.only(right: 10),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(icon, color: Colors.white38, size: 13),
          const SizedBox(width: 3),
          Text(label,
              style: const TextStyle(color: Colors.white38, fontSize: 11)),
        ],
      ),
    );
  }

  String _statusLabel(String s) => switch (s) {
        'todo' => 'Cần làm',
        'in_progress' => 'Đang làm',
        'review' => 'Đang review',
        'done' => 'Hoàn thành',
        'blocked' => 'Bị chặn',
        _ => s,
      };

  IconData _statusIcon(String s) => switch (s) {
        'todo' => Icons.radio_button_unchecked,
        'in_progress' => Icons.timelapse,
        'review' => Icons.rate_review_outlined,
        'done' => Icons.check_circle_outline,
        'blocked' => Icons.block,
        _ => Icons.label_outline,
      };

  Color _statusColor(String s) => switch (s) {
        'todo' => Colors.white54,
        'in_progress' => AppColors.cyan,
        'review' => const Color(0xFFFFB74D),
        'done' => const Color(0xFF66BB6A),
        'blocked' => Colors.redAccent,
        _ => Colors.white38,
      };
}

// ─── Messages ────────────────────────────────────────────────────────────────

class _MessagesTab extends StatefulWidget {
  final String wsId;
  const _MessagesTab({required this.wsId});

  @override
  State<_MessagesTab> createState() => _MessagesTabState();
}

class _MessagesTabState extends State<_MessagesTab>
    with AutomaticKeepAliveClientMixin {
  final _api = CoworkApi();
  final _inputCtrl = TextEditingController();
  final _scroll = ScrollController();
  List<CoworkMessage> _messages = [];
  bool _loading = true;
  bool _sending = false;
  String? _error;

  @override
  bool get wantKeepAlive => true;

  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  void dispose() {
    _inputCtrl.dispose();
    _scroll.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final msgs = await _api.listMessages(widget.wsId);
      if (!mounted) return;
      setState(() {
        _messages = msgs;
        _loading = false;
      });
      _scrollToBottom();
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = '$e';
        _loading = false;
      });
    }
  }

  Future<void> _send() async {
    final text = _inputCtrl.text.trim();
    if (text.isEmpty || _sending) return;
    setState(() => _sending = true);
    try {
      await _api.sendMessage(widget.wsId, content: text);
      _inputCtrl.clear();
      await _load();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Lỗi gửi: $e')));
      }
    } finally {
      if (mounted) setState(() => _sending = false);
    }
  }

  void _scrollToBottom() {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (_scroll.hasClients) {
        _scroll.jumpTo(_scroll.position.maxScrollExtent);
      }
    });
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    return Column(
      children: [
        Expanded(
          child: _loading
              ? const LoadingState(text: 'Đang tải tin nhắn…')
              : _error != null
                  ? ErrorState(message: _error!, onRetry: _load)
                  : _messages.isEmpty
                      ? const EmptyState(
                          icon: Icons.forum_outlined,
                          message: 'Chưa có tin nhắn nhóm',
                        )
                      : RefreshIndicator(
                          onRefresh: _load,
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
        _inputArea(),
      ],
    );
  }

  Widget _bubble(CoworkMessage m) {
    final isMine = m.fromMember == 'mobile';
    return Align(
      alignment: isMine ? Alignment.centerRight : Alignment.centerLeft,
      child: Container(
        margin: const EdgeInsets.symmetric(vertical: 4),
        constraints: BoxConstraints(
          maxWidth: MediaQuery.of(context).size.width * 0.82,
        ),
        padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
        decoration: BoxDecoration(
          color: isMine
              ? AppColors.accent.withValues(alpha: 0.16)
              : Colors.white.withValues(alpha: 0.06),
          borderRadius: BorderRadius.circular(14),
          border: Border.all(
            color: isMine
                ? AppColors.accent.withValues(alpha: 0.3)
                : AppColors.cardBorder,
          ),
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                Text(
                  m.fromMember,
                  style: const TextStyle(
                    color: AppColors.cyan,
                    fontSize: 10,
                    fontWeight: FontWeight.w600,
                  ),
                ),
                const SizedBox(width: 6),
                Container(
                  padding:
                      const EdgeInsets.symmetric(horizontal: 5, vertical: 1),
                  decoration: BoxDecoration(
                    color: Colors.white.withValues(alpha: 0.06),
                    borderRadius: BorderRadius.circular(4),
                  ),
                  child: Text(m.messageType,
                      style:
                          const TextStyle(color: Colors.white38, fontSize: 9)),
                ),
              ],
            ),
            const SizedBox(height: 4),
            Text(m.content,
                style: const TextStyle(
                    color: Colors.white, fontSize: 13, height: 1.35)),
          ],
        ),
      ),
    );
  }

  Widget _inputArea() {
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
                minLines: 1,
                maxLines: 4,
                style: const TextStyle(color: Colors.white, fontSize: 14),
                decoration: const InputDecoration(
                  hintText: 'Nhắn cho nhóm…',
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
                  : const Icon(Icons.send, color: AppColors.accent),
              onPressed: _sending ? null : _send,
            ),
          ],
        ),
      ),
    );
  }
}

// ─── Board ───────────────────────────────────────────────────────────────────

class _BoardTab extends StatefulWidget {
  final String wsId;
  const _BoardTab({required this.wsId});

  @override
  State<_BoardTab> createState() => _BoardTabState();
}

class _BoardTabState extends State<_BoardTab>
    with AutomaticKeepAliveClientMixin {
  final _api = CoworkApi();
  List<CoworkBoardEntry> _entries = [];
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
      final entries = await _api.getBoard(widget.wsId);
      if (!mounted) return;
      setState(() {
        _entries = entries;
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

  Future<void> _edit(CoworkBoardEntry e) async {
    final ctrl = TextEditingController(text: e.content);
    final ok = await showModalBottomSheet<bool>(
      context: context,
      isScrollControlled: true,
      backgroundColor: AppColors.surface,
      shape: const RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(top: Radius.circular(18)),
      ),
      builder: (_) => Padding(
        padding: EdgeInsets.fromLTRB(
            20, 20, 20, MediaQuery.of(context).viewInsets.bottom + 20),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(_sectionLabel(e.section),
                style: const TextStyle(
                    color: Colors.white,
                    fontSize: 16,
                    fontWeight: FontWeight.bold)),
            const SizedBox(height: 14),
            TextField(
              controller: ctrl,
              maxLines: 10,
              minLines: 4,
              style: const TextStyle(color: Colors.white, fontSize: 13),
              decoration: InputDecoration(
                hintText: 'Nội dung…',
                hintStyle: const TextStyle(color: Colors.white38),
                filled: true,
                fillColor: Colors.white.withValues(alpha: 0.05),
                border: OutlineInputBorder(
                  borderRadius: BorderRadius.circular(10),
                  borderSide: const BorderSide(color: AppColors.cardBorder),
                ),
                enabledBorder: OutlineInputBorder(
                  borderRadius: BorderRadius.circular(10),
                  borderSide: const BorderSide(color: AppColors.cardBorder),
                ),
              ),
            ),
            const SizedBox(height: 14),
            SizedBox(
              width: double.infinity,
              child: ElevatedButton(
                onPressed: () => Navigator.pop(context, true),
                style: ElevatedButton.styleFrom(
                  backgroundColor: AppColors.accent,
                  foregroundColor: Colors.black,
                  padding: const EdgeInsets.symmetric(vertical: 14),
                ),
                child: const Text('Lưu'),
              ),
            ),
          ],
        ),
      ),
    );
    if (ok != true) return;
    try {
      await _api.updateBoardSection(widget.wsId, e.section,
          content: ctrl.text, title: e.title);
      _load();
    } catch (err) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Lỗi lưu: $err')));
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    if (_loading) return const LoadingState(text: 'Đang tải bảng…');
    if (_error != null) return ErrorState(message: _error!, onRetry: _load);
    if (_entries.isEmpty) {
      return const EmptyState(
        icon: Icons.dashboard_customize_outlined,
        message: 'Bảng trống',
        hint: 'Các mục brief / guidelines / progress sẽ hiện ở đây',
      );
    }
    return RefreshIndicator(
      onRefresh: _load,
      color: AppColors.accent,
      backgroundColor: AppColors.surface,
      child: ListView.builder(
        padding: const EdgeInsets.all(12),
        itemCount: _entries.length,
        itemBuilder: (ctx, i) => _card(_entries[i]),
      ),
    );
  }

  Widget _card(CoworkBoardEntry e) {
    return Card(
      color: Colors.white.withValues(alpha: 0.04),
      margin: const EdgeInsets.only(bottom: 10),
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(12),
        side: const BorderSide(color: AppColors.cardBorder),
      ),
      child: Padding(
        padding: const EdgeInsets.all(14),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Icon(_sectionIcon(e.section),
                    color: AppColors.cyan, size: 16),
                const SizedBox(width: 6),
                Expanded(
                  child: Text(
                    e.title?.isNotEmpty == true
                        ? e.title!
                        : _sectionLabel(e.section),
                    style: const TextStyle(
                        color: Colors.white,
                        fontWeight: FontWeight.w600,
                        fontSize: 14),
                  ),
                ),
                IconButton(
                  icon: const Icon(Icons.edit_outlined,
                      color: Colors.white38, size: 18),
                  padding: EdgeInsets.zero,
                  constraints: const BoxConstraints(),
                  onPressed: () => _edit(e),
                ),
              ],
            ),
            const SizedBox(height: 8),
            Text(
              e.content.isEmpty ? '(trống)' : e.content,
              style: const TextStyle(
                  color: Colors.white70, fontSize: 13, height: 1.4),
            ),
          ],
        ),
      ),
    );
  }

  String _sectionLabel(String s) => switch (s) {
        'brief' => 'Tóm tắt',
        'guidelines' => 'Hướng dẫn',
        'progress' => 'Tiến độ',
        'reference' => 'Tham chiếu',
        'decisions' => 'Quyết định',
        _ => s,
      };

  IconData _sectionIcon(String s) => switch (s) {
        'brief' => Icons.summarize_outlined,
        'guidelines' => Icons.rule,
        'progress' => Icons.trending_up,
        'reference' => Icons.link,
        'decisions' => Icons.gavel,
        _ => Icons.notes,
      };
}

// ─── Team ────────────────────────────────────────────────────────────────────

class _TeamTab extends StatefulWidget {
  final String wsId;
  const _TeamTab({required this.wsId});

  @override
  State<_TeamTab> createState() => _TeamTabState();
}

class _TeamTabState extends State<_TeamTab>
    with AutomaticKeepAliveClientMixin {
  final _api = CoworkApi();
  List<CoworkMember> _members = [];
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
      final members = await _api.listMembers(widget.wsId);
      if (!mounted) return;
      setState(() {
        _members = members;
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
    super.build(context);
    if (_loading) return const LoadingState(text: 'Đang tải thành viên…');
    if (_error != null) return ErrorState(message: _error!, onRetry: _load);
    if (_members.isEmpty) {
      return const EmptyState(
        icon: Icons.person_off_outlined,
        message: 'Chưa có thành viên',
        hint: 'Thêm agent vào workspace từ Web UI',
      );
    }
    return RefreshIndicator(
      onRefresh: _load,
      color: AppColors.accent,
      backgroundColor: AppColors.surface,
      child: ListView.builder(
        padding: const EdgeInsets.all(12),
        itemCount: _members.length,
        itemBuilder: (ctx, i) => _memberCard(_members[i]),
      ),
    );
  }

  Widget _memberCard(CoworkMember m) {
    return Card(
      color: Colors.white.withValues(alpha: 0.04),
      margin: const EdgeInsets.only(bottom: 8),
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(12),
        side: const BorderSide(color: AppColors.cardBorder),
      ),
      child: ListTile(
        leading: CircleAvatar(
          backgroundColor: _roleColor(m.role).withValues(alpha: 0.18),
          child: Icon(Icons.smart_toy_outlined,
              color: _roleColor(m.role), size: 20),
        ),
        title: Text(m.memberId,
            style: const TextStyle(
                color: Colors.white, fontWeight: FontWeight.w600)),
        subtitle: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            const SizedBox(height: 2),
            Text(
              m.persona?.isNotEmpty == true ? m.persona! : m.role,
              style: const TextStyle(color: Colors.white54, fontSize: 12),
            ),
            if (m.responsibilities != null &&
                m.responsibilities!.isNotEmpty) ...[
              const SizedBox(height: 2),
              Text(m.responsibilities!,
                  style: const TextStyle(color: Colors.white38, fontSize: 11),
                  maxLines: 2,
                  overflow: TextOverflow.ellipsis),
            ],
          ],
        ),
        trailing: Container(
          padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
          decoration: BoxDecoration(
            color: _roleColor(m.role).withValues(alpha: 0.12),
            borderRadius: BorderRadius.circular(6),
          ),
          child: Text(m.role,
              style: TextStyle(color: _roleColor(m.role), fontSize: 10)),
        ),
      ),
    );
  }

  Color _roleColor(String role) => switch (role) {
        'lead' => AppColors.accent,
        'reviewer' => const Color(0xFFFFB74D),
        'worker' => AppColors.cyan,
        _ => Colors.white54,
      };
}
