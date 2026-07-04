import 'package:flutter/material.dart';
import '../../models/cowork_models.dart';
import '../../services/cowork_api.dart';
import '../../services/language_service.dart';
import '../../theme/tokens.dart';
import '../../widgets/states.dart';

/// Detail view for one Cowork team: kanban Tasks, Members, and Settings
/// (manager preamble / tools / auto-create-tasks). Backed by
/// `/api/cowork/teams/:id/*`.
class CoworkTeamScreen extends StatefulWidget {
  final CoworkTeam team;
  const CoworkTeamScreen({super.key, required this.team});

  @override
  State<CoworkTeamScreen> createState() => _CoworkTeamScreenState();
}

class _CoworkTeamScreenState extends State<CoworkTeamScreen>
    with SingleTickerProviderStateMixin {
  final _api = CoworkApi();
  late final TabController _tabs = TabController(length: 3, vsync: this);
  late CoworkTeam _team = widget.team;

  @override
  void dispose() {
    _tabs.dispose();
    super.dispose();
  }

  Future<void> _saveTemplate() async {
    try {
      await _api.saveAsTemplate(_team.id);
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
            content: Text(tr('Đã lưu thành mẫu', 'Saved as template'))));
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text(tr('Lỗi: $e', 'Error: $e'))));
      }
    }
  }

  Future<void> _deleteTeam() async {
    final c = context.colors;
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: c.surface,
        title: Text(tr('Xoá đội?', 'Delete team?'),
            style: TextStyle(color: c.textPrimary)),
        content: Text(_team.name,
            style: TextStyle(color: c.textSecondary)),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(ctx, false),
              child: Text(tr('Huỷ', 'Cancel'))),
          TextButton(
              onPressed: () => Navigator.pop(ctx, true),
              child: Text(tr('Xoá', 'Delete'),
                  style: const TextStyle(color: AppTokens.danger))),
        ],
      ),
    );
    if (ok != true) return;
    try {
      await _api.deleteTeam(_team.id);
      if (mounted) Navigator.pop(context);
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text(tr('Lỗi: $e', 'Error: $e'))));
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Scaffold(
      backgroundColor: c.bg,
      appBar: AppBar(
        backgroundColor: c.surface,
        elevation: 0,
        title: Text(_team.name,
            style: TextStyle(color: c.textPrimary, fontSize: 16)),
        actions: [
          PopupMenuButton<String>(
            color: c.surface,
            icon: Icon(Icons.more_vert, color: c.textSecondary),
            onSelected: (v) {
              if (v == 'template') _saveTemplate();
              if (v == 'delete') _deleteTeam();
            },
            itemBuilder: (_) => [
              PopupMenuItem(
                  value: 'template',
                  child: Text(tr('Lưu thành mẫu', 'Save as template'),
                      style: TextStyle(color: c.textPrimary))),
              PopupMenuItem(
                  value: 'delete',
                  child: Text(tr('Xoá đội', 'Delete team'),
                      style: const TextStyle(color: AppTokens.danger))),
            ],
          ),
        ],
        bottom: TabBar(
          controller: _tabs,
          indicatorColor: c.accent,
          labelColor: c.accent,
          unselectedLabelColor: c.textMuted,
          tabs: [
            Tab(text: tr('Công việc', 'Tasks')),
            Tab(text: tr('Thành viên', 'Members')),
            Tab(text: tr('Cài đặt', 'Settings')),
          ],
        ),
      ),
      body: Container(
        decoration: BoxDecoration(color: c.bg),
        child: TabBarView(
          controller: _tabs,
          children: [
            _TasksTab(api: _api, teamId: _team.id),
            _MembersTab(
              api: _api,
              team: _team,
              onChanged: (t) => setState(() => _team = t),
            ),
            _SettingsTab(
              api: _api,
              team: _team,
              onChanged: (t) => setState(() => _team = t),
            ),
          ],
        ),
      ),
    );
  }
}

// ─── Tasks (kanban list grouped by status) ────────────────────────────────────

const _statuses = ['backlog', 'todo', 'in_progress', 'review', 'done', 'blocked'];

/// Display label for a task status (raw status value falls through unchanged).
String _statusLabel(String s) {
  switch (s) {
    case 'backlog':
      return tr('Tồn đọng', 'Backlog');
    case 'todo':
      return tr('Cần làm', 'To do');
    case 'in_progress':
      return tr('Đang làm', 'In progress');
    case 'review':
      return tr('Rà soát', 'Review');
    case 'done':
      return tr('Hoàn thành', 'Done');
    case 'blocked':
      return tr('Bị chặn', 'Blocked');
    default:
      return s;
  }
}

Color _statusColor(String s, AppColors c) {
  switch (s) {
    case 'in_progress':
      return AppTokens.cyan;
    case 'review':
      return AppTokens.warning;
    case 'done':
      return AppTokens.success;
    case 'blocked':
      return AppTokens.danger;
    default:
      return c.textMuted;
  }
}

class _TasksTab extends StatefulWidget {
  final CoworkApi api;
  final String teamId;
  const _TasksTab({required this.api, required this.teamId});

  @override
  State<_TasksTab> createState() => _TasksTabState();
}

class _TasksTabState extends State<_TasksTab>
    with AutomaticKeepAliveClientMixin {
  List<CoworkTeamTask> _tasks = [];
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
      final t = await widget.api.listTasks(widget.teamId);
      if (!mounted) return;
      setState(() {
        _tasks = t;
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
    final ctrl = TextEditingController();
    final c = context.colors;
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: c.surface,
        title: Text(tr('Công việc mới', 'New task'),
            style: TextStyle(color: c.textPrimary)),
        content: TextField(
          controller: ctrl,
          autofocus: true,
          style: TextStyle(color: c.textPrimary),
          decoration: InputDecoration(
            labelText: tr('Tiêu đề', 'Title'),
            labelStyle: TextStyle(color: c.textMuted),
          ),
        ),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(ctx, false),
              child: Text(tr('Huỷ', 'Cancel'))),
          TextButton(
              onPressed: () => Navigator.pop(ctx, true),
              child: Text(tr('Tạo', 'Create'),
                  style: TextStyle(color: c.accent))),
        ],
      ),
    );
    if (ok != true || ctrl.text.trim().isEmpty) return;
    try {
      await widget.api.createTask(widget.teamId, title: ctrl.text.trim());
      _load();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text(tr('Lỗi: $e', 'Error: $e'))));
      }
    }
  }

  Future<void> _move(CoworkTeamTask t, String status) async {
    try {
      await widget.api.updateTask(widget.teamId, t.id, status: status);
      _load();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text(tr('Lỗi: $e', 'Error: $e'))));
      }
    }
  }

  Future<void> _delete(CoworkTeamTask t) async {
    try {
      await widget.api.deleteTask(widget.teamId, t.id);
      _load();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text(tr('Lỗi: $e', 'Error: $e'))));
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    final c = context.colors;
    return Scaffold(
      backgroundColor: Colors.transparent,
      floatingActionButton: FloatingActionButton(
        onPressed: _create,
        backgroundColor: c.accent,
        foregroundColor: Colors.white,
        child: const Icon(Icons.add),
      ),
      body: _buildBody(),
    );
  }

  Widget _buildBody() {
    final c = context.colors;
    if (_loading) return const LoadingState();
    if (_error != null) return ErrorState(message: _error!, onRetry: _load);
    if (_tasks.isEmpty) {
      return EmptyState(
        icon: Icons.task_alt_outlined,
        message: tr('Chưa có công việc', 'No tasks yet'),
        hint: tr('Nhấn + để thêm; agent quản lý cũng tự tạo khi xử lý',
            'Tap + to add; the manager agent also creates tasks while working'),
      );
    }
    final byStatus = <String, List<CoworkTeamTask>>{};
    for (final t in _tasks) {
      byStatus.putIfAbsent(t.status, () => []).add(t);
    }
    final order = [
      ..._statuses.where(byStatus.containsKey),
      ...byStatus.keys.where((k) => !_statuses.contains(k)),
    ];
    return RefreshIndicator(
      onRefresh: _load,
      color: c.accent,
      backgroundColor: c.surface,
      child: ListView(
        padding: const EdgeInsets.fromLTRB(12, 12, 12, 88),
        children: [
          for (final st in order) ...[
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 8),
              child: Row(
                children: [
                  Container(
                    width: 8,
                    height: 8,
                    decoration: BoxDecoration(
                      shape: BoxShape.circle,
                      color: _statusColor(st, c),
                    ),
                  ),
                  const SizedBox(width: 8),
                  Text(
                    '${_statusLabel(st)} · ${byStatus[st]!.length}',
                    style: TextStyle(
                        color: c.textSecondary,
                        fontSize: 13,
                        fontWeight: FontWeight.w600),
                  ),
                ],
              ),
            ),
            ...byStatus[st]!.map(_taskCard),
          ],
        ],
      ),
    );
  }

  Widget _taskCard(CoworkTeamTask t) {
    final c = context.colors;
    return Card(
      color: c.surfaceAlt,
      margin: const EdgeInsets.only(bottom: 8),
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(12),
        side: BorderSide(color: c.border),
      ),
      child: ListTile(
        title: Text(t.title,
            style: TextStyle(color: c.textPrimary, fontSize: 14)),
        subtitle: (t.assignee != null && t.assignee!.isNotEmpty)
            ? Text('→ ${t.assignee}',
                style: TextStyle(color: c.textMuted, fontSize: 11))
            : null,
        trailing: PopupMenuButton<String>(
          color: c.surface,
          icon: Icon(Icons.more_vert, color: c.textMuted, size: 20),
          onSelected: (v) {
            if (v == 'delete') {
              _delete(t);
            } else {
              _move(t, v);
            }
          },
          itemBuilder: (_) => [
            for (final st in _statuses)
              if (st != t.status)
                PopupMenuItem(
                  value: st,
                  child: Text('→ ${_statusLabel(st)}',
                      style: TextStyle(color: c.textPrimary)),
                ),
            PopupMenuItem(
                value: 'delete',
                child: Text(tr('Xoá', 'Delete'),
                    style: const TextStyle(color: AppTokens.danger))),
          ],
        ),
      ),
    );
  }
}

// ─── Members ─────────────────────────────────────────────────────────────────

class _MembersTab extends StatefulWidget {
  final CoworkApi api;
  final CoworkTeam team;
  final ValueChanged<CoworkTeam> onChanged;
  const _MembersTab(
      {required this.api, required this.team, required this.onChanged});

  @override
  State<_MembersTab> createState() => _MembersTabState();
}

class _MembersTabState extends State<_MembersTab> {
  late CoworkTeam _team = widget.team;

  Future<void> _addMember() async {
    final folderCtrl = TextEditingController();
    final roleCtrl = TextEditingController();
    final respCtrl = TextEditingController();
    final c = context.colors;
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: c.surface,
        title: Text(tr('Thêm thành viên', 'Add member'),
            style: TextStyle(color: c.textPrimary)),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            _dlgField(folderCtrl, 'Folder/persona'),
            const SizedBox(height: 8),
            _dlgField(roleCtrl, tr('Vai trò (tuỳ chọn)', 'Role (optional)')),
            const SizedBox(height: 8),
            _dlgField(respCtrl,
                tr('Trách nhiệm (tuỳ chọn)', 'Responsibilities (optional)')),
          ],
        ),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(ctx, false),
              child: Text(tr('Huỷ', 'Cancel'))),
          TextButton(
              onPressed: () => Navigator.pop(ctx, true),
              child: Text(tr('Thêm', 'Add'),
                  style: TextStyle(color: c.accent))),
        ],
      ),
    );
    if (ok != true || folderCtrl.text.trim().isEmpty) return;
    try {
      final updated = await widget.api.upsertMember(
        _team.id,
        TeamMember(
          folder: folderCtrl.text.trim(),
          role: roleCtrl.text.trim().isEmpty ? null : roleCtrl.text.trim(),
          responsibilities:
              respCtrl.text.trim().isEmpty ? null : respCtrl.text.trim(),
        ),
      );
      setState(() => _team = updated);
      widget.onChanged(updated);
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text(tr('Lỗi: $e', 'Error: $e'))));
      }
    }
  }

  Future<void> _remove(TeamMember m) async {
    try {
      final updated = await widget.api.removeMember(_team.id, m.folder);
      setState(() => _team = updated);
      widget.onChanged(updated);
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text(tr('Lỗi: $e', 'Error: $e'))));
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Scaffold(
      backgroundColor: Colors.transparent,
      floatingActionButton: FloatingActionButton(
        onPressed: _addMember,
        backgroundColor: c.accent,
        foregroundColor: Colors.white,
        child: const Icon(Icons.person_add_alt),
      ),
      body: ListView(
        padding: const EdgeInsets.fromLTRB(12, 12, 12, 88),
        children: [
          Card(
            color: c.surfaceAlt,
            margin: const EdgeInsets.only(bottom: 8),
            shape: RoundedRectangleBorder(
              borderRadius: BorderRadius.circular(12),
              side: BorderSide(color: c.accent.withValues(alpha: 0.4)),
            ),
            child: ListTile(
              leading: Icon(Icons.star, color: c.accent),
              title: Text(_team.managerFolder,
                  style: TextStyle(
                      color: c.textPrimary, fontWeight: FontWeight.w600)),
              subtitle: Text(tr('Quản lý', 'Manager'),
                  style: TextStyle(color: c.textMuted, fontSize: 12)),
            ),
          ),
          if (_team.members.isEmpty)
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 24),
              child: Center(
                child: Text(tr('Chưa có thành viên', 'No members yet'),
                    style: TextStyle(color: c.textMuted)),
              ),
            )
          else
            ..._team.members.map((m) => Card(
                  color: c.surfaceAlt,
                  margin: const EdgeInsets.only(bottom: 8),
                  shape: RoundedRectangleBorder(
                    borderRadius: BorderRadius.circular(12),
                    side: BorderSide(color: c.border),
                  ),
                  child: ListTile(
                    leading: const Icon(Icons.person_outline,
                        color: AppTokens.cyan),
                    title: Text(m.folder,
                        style: TextStyle(color: c.textPrimary)),
                    subtitle: (m.role != null || m.responsibilities != null)
                        ? Text(
                            [m.role, m.responsibilities]
                                .where((e) => e != null && e.isNotEmpty)
                                .join(' · '),
                            style: TextStyle(
                                color: c.textMuted, fontSize: 12))
                        : null,
                    trailing: IconButton(
                      icon: Icon(Icons.delete_outline,
                          color: c.textMuted, size: 20),
                      onPressed: () => _remove(m),
                    ),
                  ),
                )),
        ],
      ),
    );
  }

  Widget _dlgField(TextEditingController c, String hint) {
    final col = context.colors;
    return TextField(
      controller: c,
      style: TextStyle(color: col.textPrimary),
      decoration: InputDecoration(
        labelText: hint,
        labelStyle: TextStyle(color: col.textMuted),
      ),
    );
  }
}

// ─── Settings (manager preamble / tools / auto-create-tasks) ──────────────────

class _SettingsTab extends StatefulWidget {
  final CoworkApi api;
  final CoworkTeam team;
  final ValueChanged<CoworkTeam> onChanged;
  const _SettingsTab(
      {required this.api, required this.team, required this.onChanged});

  @override
  State<_SettingsTab> createState() => _SettingsTabState();
}

class _SettingsTabState extends State<_SettingsTab> {
  late final _preamble =
      TextEditingController(text: widget.team.settings.managerPreamble ?? '');
  late final _tools = TextEditingController(
      text: (widget.team.settings.managerTools ?? const []).join(', '));
  late bool _autoCreate = widget.team.settings.autoCreateTasks ?? true;
  bool _saving = false;

  @override
  void dispose() {
    _preamble.dispose();
    _tools.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    setState(() => _saving = true);
    try {
      final tools = _tools.text
          .split(',')
          .map((s) => s.trim())
          .where((s) => s.isNotEmpty)
          .toList();
      final updated = await widget.api.updateTeam(
        widget.team.id,
        settings: CoworkTeamSettings(
          managerPreamble:
              _preamble.text.trim().isEmpty ? null : _preamble.text.trim(),
          managerTools: tools.isEmpty ? null : tools,
          autoCreateTasks: _autoCreate,
        ),
      );
      widget.onChanged(updated);
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
            content: Text(tr('Đã lưu cài đặt đội', 'Team settings saved'))));
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text(tr('Lỗi: $e', 'Error: $e'))));
      }
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return ListView(
      padding: const EdgeInsets.all(16),
      children: [
        Text(tr('Preamble cho quản lý', 'Manager preamble'),
            style: TextStyle(color: c.textSecondary, fontSize: 13)),
        const SizedBox(height: 6),
        _field(
            _preamble,
            tr('Để trống = mặc định PLAN→DELEGATE→SYNTHESIZE',
                'Leave empty = default PLAN→DELEGATE→SYNTHESIZE'),
            maxLines: 5),
        const SizedBox(height: 16),
        Text(tr('Công cụ cho quản lý', 'Manager tools'),
            style: TextStyle(color: c.textSecondary, fontSize: 13)),
        const SizedBox(height: 6),
        _field(
            _tools,
            tr('Phân tách bằng dấu phẩy (trống = Task + TodoWrite)',
                'Comma-separated (empty = Task + TodoWrite)')),
        const SizedBox(height: 8),
        SwitchListTile(
          contentPadding: EdgeInsets.zero,
          value: _autoCreate,
          onChanged: (v) => setState(() => _autoCreate = v),
          activeThumbColor: c.accent,
          title: Text(
              tr('Tự tạo công việc khi có tin nhắn',
                  'Auto-create tasks from incoming messages'),
              style: TextStyle(color: c.textPrimary, fontSize: 14)),
        ),
        const SizedBox(height: 16),
        SizedBox(
          width: double.infinity,
          child: ElevatedButton(
            onPressed: _saving ? null : _save,
            style: ElevatedButton.styleFrom(
              backgroundColor: c.accent,
              foregroundColor: Colors.white,
              padding: const EdgeInsets.symmetric(vertical: 14),
            ),
            child: _saving
                ? const SizedBox(
                    width: 18,
                    height: 18,
                    child: CircularProgressIndicator(
                        strokeWidth: 2, color: Colors.white))
                : Text(tr('Lưu', 'Save')),
          ),
        ),
      ],
    );
  }

  Widget _field(TextEditingController c, String hint, {int maxLines = 1}) {
    final col = context.colors;
    return TextField(
      controller: c,
      maxLines: maxLines,
      style: TextStyle(color: col.textPrimary),
      decoration: InputDecoration(
        hintText: hint,
        hintStyle: TextStyle(color: col.textMuted),
        filled: true,
        fillColor: col.surfaceAlt,
        border: OutlineInputBorder(
          borderRadius: BorderRadius.circular(10),
          borderSide: BorderSide(color: col.border),
        ),
        enabledBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(10),
          borderSide: BorderSide(color: col.border),
        ),
      ),
    );
  }
}
