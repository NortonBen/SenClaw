import 'package:flutter/material.dart';
import '../../models/cowork_models.dart';
import '../../services/cowork_api.dart';
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
        ScaffoldMessenger.of(context).showSnackBar(
            const SnackBar(content: Text('Đã lưu thành mẫu')));
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Lỗi: $e')));
      }
    }
  }

  Future<void> _deleteTeam() async {
    final c = context.colors;
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: c.surface,
        title: Text('Xoá đội?', style: TextStyle(color: c.textPrimary)),
        content: Text(_team.name,
            style: TextStyle(color: c.textSecondary)),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(ctx, false),
              child: const Text('Huỷ')),
          TextButton(
              onPressed: () => Navigator.pop(ctx, true),
              child: const Text('Xoá',
                  style: TextStyle(color: AppTokens.danger))),
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
            .showSnackBar(SnackBar(content: Text('Lỗi: $e')));
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
                  child: Text('Lưu thành mẫu',
                      style: TextStyle(color: c.textPrimary))),
              const PopupMenuItem(
                  value: 'delete',
                  child: Text('Xoá đội',
                      style: TextStyle(color: AppTokens.danger))),
            ],
          ),
        ],
        bottom: TabBar(
          controller: _tabs,
          indicatorColor: c.accent,
          labelColor: c.accent,
          unselectedLabelColor: c.textMuted,
          tabs: const [
            Tab(text: 'Công việc'),
            Tab(text: 'Thành viên'),
            Tab(text: 'Cài đặt'),
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
const _statusLabel = {
  'backlog': 'Tồn đọng',
  'todo': 'Cần làm',
  'in_progress': 'Đang làm',
  'review': 'Rà soát',
  'done': 'Hoàn thành',
  'blocked': 'Bị chặn',
};

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
        title: Text('Công việc mới',
            style: TextStyle(color: c.textPrimary)),
        content: TextField(
          controller: ctrl,
          autofocus: true,
          style: TextStyle(color: c.textPrimary),
          decoration: InputDecoration(
            labelText: 'Tiêu đề',
            labelStyle: TextStyle(color: c.textMuted),
          ),
        ),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(ctx, false),
              child: const Text('Huỷ')),
          TextButton(
              onPressed: () => Navigator.pop(ctx, true),
              child: Text('Tạo',
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
            .showSnackBar(SnackBar(content: Text('Lỗi: $e')));
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
            .showSnackBar(SnackBar(content: Text('Lỗi: $e')));
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
            .showSnackBar(SnackBar(content: Text('Lỗi: $e')));
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
      return const EmptyState(
        icon: Icons.task_alt_outlined,
        message: 'Chưa có công việc',
        hint: 'Nhấn + để thêm; agent quản lý cũng tự tạo khi xử lý',
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
                    '${_statusLabel[st] ?? st} · ${byStatus[st]!.length}',
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
                  child: Text('→ ${_statusLabel[st] ?? st}',
                      style: TextStyle(color: c.textPrimary)),
                ),
            const PopupMenuItem(
                value: 'delete',
                child:
                    Text('Xoá', style: TextStyle(color: AppTokens.danger))),
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
        title: Text('Thêm thành viên',
            style: TextStyle(color: c.textPrimary)),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            _dlgField(folderCtrl, 'Folder/persona'),
            const SizedBox(height: 8),
            _dlgField(roleCtrl, 'Vai trò (tuỳ chọn)'),
            const SizedBox(height: 8),
            _dlgField(respCtrl, 'Trách nhiệm (tuỳ chọn)'),
          ],
        ),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(ctx, false),
              child: const Text('Huỷ')),
          TextButton(
              onPressed: () => Navigator.pop(ctx, true),
              child: Text('Thêm',
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
            .showSnackBar(SnackBar(content: Text('Lỗi: $e')));
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
            .showSnackBar(SnackBar(content: Text('Lỗi: $e')));
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
              subtitle: Text('Quản lý',
                  style: TextStyle(color: c.textMuted, fontSize: 12)),
            ),
          ),
          if (_team.members.isEmpty)
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 24),
              child: Center(
                child: Text('Chưa có thành viên',
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
        ScaffoldMessenger.of(context).showSnackBar(
            const SnackBar(content: Text('Đã lưu cài đặt đội')));
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Lỗi: $e')));
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
        Text('Preamble cho quản lý',
            style: TextStyle(color: c.textSecondary, fontSize: 13)),
        const SizedBox(height: 6),
        _field(_preamble, 'Để trống = mặc định PLAN→DELEGATE→SYNTHESIZE',
            maxLines: 5),
        const SizedBox(height: 16),
        Text('Công cụ cho quản lý',
            style: TextStyle(color: c.textSecondary, fontSize: 13)),
        const SizedBox(height: 6),
        _field(_tools, 'Phân tách bằng dấu phẩy (trống = Task + TodoWrite)'),
        const SizedBox(height: 8),
        SwitchListTile(
          contentPadding: EdgeInsets.zero,
          value: _autoCreate,
          onChanged: (v) => setState(() => _autoCreate = v),
          activeThumbColor: c.accent,
          title: Text('Tự tạo công việc khi có tin nhắn',
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
                : const Text('Lưu'),
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
