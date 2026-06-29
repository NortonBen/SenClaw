import 'package:flutter/material.dart';
import '../../models/cowork_models.dart';
import '../../services/cowork_api.dart';
import '../../services/relay_manager.dart';
import '../../theme/tokens.dart';
import '../../widgets/states.dart';
import 'cowork_workspace_screen.dart';

/// Cowork: DAG teams + reusable templates, backed by `/api/cowork/teams` and
/// `/api/cowork/templates` over the relay tunnel.
class CoworkScreen extends StatefulWidget {
  const CoworkScreen({super.key});

  @override
  State<CoworkScreen> createState() => _CoworkScreenState();
}

class _CoworkScreenState extends State<CoworkScreen>
    with SingleTickerProviderStateMixin {
  late final TabController _tabs = TabController(length: 2, vsync: this);

  @override
  void dispose() {
    _tabs.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final tabBar = TabBar(
      controller: _tabs,
      indicatorColor: c.accent,
      labelColor: c.accent,
      unselectedLabelColor: c.textMuted,
      tabs: const [
        Tab(icon: Icon(Icons.groups_outlined), text: 'Đội'),
        Tab(icon: Icon(Icons.dashboard_customize_outlined), text: 'Mẫu'),
      ],
    );
    return Scaffold(
      backgroundColor: c.bg,
      appBar: AppBar(
        backgroundColor: c.surface,
        elevation: 0,
        title: Row(
          children: [
            Text('Cowork', style: TextStyle(color: c.textPrimary)),
            const SizedBox(width: 8),
            AnimatedBuilder(
              animation: RelayManager(),
              builder: (_, _) =>
                  ConnectionDot(connected: RelayManager().connected),
            ),
          ],
        ),
        bottom: tabBar,
      ),
      body: Container(
        decoration: BoxDecoration(color: c.bg),
        child: TabBarView(
          controller: _tabs,
          children: const [_TeamsTab(), _TemplatesTab()],
        ),
      ),
    );
  }
}

// ─── Teams ───────────────────────────────────────────────────────────────────

class _TeamsTab extends StatefulWidget {
  const _TeamsTab();

  @override
  State<_TeamsTab> createState() => _TeamsTabState();
}

class _TeamsTabState extends State<_TeamsTab>
    with AutomaticKeepAliveClientMixin {
  final _api = CoworkApi();
  List<CoworkTeam> _teams = [];
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
      final t = await _api.listTeams();
      if (!mounted) return;
      setState(() {
        _teams = t;
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

  Future<void> _open(CoworkTeam team) async {
    await Navigator.push(
      context,
      MaterialPageRoute(builder: (_) => CoworkTeamScreen(team: team)),
    );
    _load();
  }

  Future<void> _create() async {
    final personas = await _safePersonas();
    if (!mounted) return;
    final created = await showModalBottomSheet<bool>(
      context: context,
      isScrollControlled: true,
      backgroundColor: context.colors.surface,
      shape: const RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(top: Radius.circular(18)),
      ),
      builder: (_) => _CreateTeamSheet(api: _api, personas: personas),
    );
    if (created == true) _load();
  }

  Future<List<CoworkPersona>> _safePersonas() async {
    try {
      return await _api.listPersonas();
    } catch (_) {
      return const [];
    }
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    final c = context.colors;
    return Scaffold(
      backgroundColor: Colors.transparent,
      floatingActionButton: FloatingActionButton.extended(
        onPressed: _create,
        backgroundColor: c.accent,
        foregroundColor: Colors.white,
        icon: const Icon(Icons.add),
        label: const Text('Đội mới'),
      ),
      body: _buildBody(),
    );
  }

  Widget _buildBody() {
    final c = context.colors;
    if (_loading) return const LoadingState(text: 'Đang tải đội…');
    if (_error != null) return ErrorState(message: _error!, onRetry: _load);
    if (_teams.isEmpty) {
      return const EmptyState(
        icon: Icons.groups_outlined,
        message: 'Chưa có đội',
        hint: 'Tạo đội mới hoặc dùng một mẫu có sẵn',
      );
    }
    return RefreshIndicator(
      onRefresh: _load,
      color: c.accent,
      backgroundColor: c.surface,
      child: ListView.builder(
        padding: const EdgeInsets.fromLTRB(12, 12, 12, 88),
        itemCount: _teams.length,
        itemBuilder: (ctx, i) {
          final t = _teams[i];
          return Card(
            color: c.surfaceAlt,
            margin: const EdgeInsets.only(bottom: 10),
            shape: RoundedRectangleBorder(
              borderRadius: BorderRadius.circular(14),
              side: BorderSide(color: c.border),
            ),
            child: ListTile(
              contentPadding:
                  const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
              leading: CircleAvatar(
                backgroundColor: c.accentSoft,
                child: Icon(Icons.groups, color: c.accent),
              ),
              title: Text(t.name,
                  style: TextStyle(
                      color: c.textPrimary, fontWeight: FontWeight.w600)),
              subtitle: Padding(
                padding: const EdgeInsets.only(top: 4),
                child: Text(
                  'Quản lý: ${t.managerFolder} · ${t.members.length} thành viên',
                  style: TextStyle(color: c.textMuted, fontSize: 12),
                ),
              ),
              trailing: Icon(Icons.chevron_right, color: c.textMuted),
              onTap: () => _open(t),
            ),
          );
        },
      ),
    );
  }
}

class _CreateTeamSheet extends StatefulWidget {
  final CoworkApi api;
  final List<CoworkPersona> personas;
  const _CreateTeamSheet({required this.api, required this.personas});

  @override
  State<_CreateTeamSheet> createState() => _CreateTeamSheetState();
}

class _CreateTeamSheetState extends State<_CreateTeamSheet> {
  final _name = TextEditingController();
  final _manager = TextEditingController();
  final Set<String> _members = {};
  bool _saving = false;
  String? _error;

  @override
  void dispose() {
    _name.dispose();
    _manager.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    if (_name.text.trim().isEmpty || _manager.text.trim().isEmpty) {
      setState(() => _error = 'Cần tên đội và quản lý');
      return;
    }
    setState(() {
      _saving = true;
      _error = null;
    });
    try {
      await widget.api.createTeam(
        name: _name.text.trim(),
        managerFolder: _manager.text.trim(),
        members: _members.toList(),
      );
      if (mounted) Navigator.pop(context, true);
    } catch (e) {
      if (mounted) {
        setState(() {
          _error = '$e';
          _saving = false;
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Padding(
      padding: EdgeInsets.fromLTRB(
          16, 16, 16, MediaQuery.of(context).viewInsets.bottom + 16),
      child: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text('Đội mới',
                style: TextStyle(
                    color: c.textPrimary,
                    fontSize: 16,
                    fontWeight: FontWeight.bold)),
            const SizedBox(height: 12),
            _f(_name, 'Tên đội'),
            const SizedBox(height: 10),
            _f(_manager, 'Quản lý (folder/persona)'),
            if (widget.personas.isNotEmpty) ...[
              const SizedBox(height: 12),
              Text('Thành viên (persona)',
                  style: TextStyle(color: c.textMuted, fontSize: 12)),
              const SizedBox(height: 6),
              Wrap(
                spacing: 6,
                runSpacing: 6,
                children: widget.personas.map((p) {
                  final sel = _members.contains(p.name);
                  return FilterChip(
                    label: Text(p.name, style: const TextStyle(fontSize: 12)),
                    selected: sel,
                    onSelected: (v) => setState(() =>
                        v ? _members.add(p.name) : _members.remove(p.name)),
                    selectedColor: c.accentSoft,
                    backgroundColor: c.surfaceAlt,
                    labelStyle: TextStyle(
                        color: sel ? c.textPrimary : c.textSecondary),
                    checkmarkColor: c.accent,
                  );
                }).toList(),
              ),
            ],
            if (_error != null) ...[
              const SizedBox(height: 8),
              Text(_error!,
                  style:
                      const TextStyle(color: AppTokens.danger, fontSize: 12)),
            ],
            const SizedBox(height: 14),
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
                    : const Text('Tạo đội'),
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _f(TextEditingController c, String hint) {
    final col = context.colors;
    return TextField(
      controller: c,
      style: TextStyle(color: col.textPrimary),
      decoration: InputDecoration(
        hintText: hint,
        hintStyle: TextStyle(color: col.textMuted),
        isDense: true,
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

// ─── Templates ───────────────────────────────────────────────────────────────

class _TemplatesTab extends StatefulWidget {
  const _TemplatesTab();

  @override
  State<_TemplatesTab> createState() => _TemplatesTabState();
}

class _TemplatesTabState extends State<_TemplatesTab>
    with AutomaticKeepAliveClientMixin {
  final _api = CoworkApi();
  List<CoworkTemplate> _templates = [];
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
      final t = await _api.listTemplates();
      if (!mounted) return;
      setState(() {
        _templates = t;
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

  Future<void> _use(CoworkTemplate t) async {
    final nameCtrl = TextEditingController(text: t.name);
    final c = context.colors;
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: c.surface,
        title: Text('Tạo đội từ "${t.name}"',
            style: TextStyle(color: c.textPrimary, fontSize: 16)),
        content: TextField(
          controller: nameCtrl,
          style: TextStyle(color: c.textPrimary),
          decoration: InputDecoration(
            labelText: 'Tên đội',
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
    if (ok != true) return;
    try {
      await _api.createFromTemplate(t.id, name: nameCtrl.text.trim());
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
            const SnackBar(content: Text('Đã tạo đội — xem tab Đội')));
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Lỗi: $e')));
      }
    }
  }

  Future<void> _delete(CoworkTemplate t) async {
    try {
      await _api.deleteTemplate(t.id);
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
    if (_loading) return const LoadingState(text: 'Đang tải mẫu…');
    if (_error != null) return ErrorState(message: _error!, onRetry: _load);
    if (_templates.isEmpty) {
      return const EmptyState(
        icon: Icons.dashboard_customize_outlined,
        message: 'Chưa có mẫu',
      );
    }
    return RefreshIndicator(
      onRefresh: _load,
      color: c.accent,
      backgroundColor: c.surface,
      child: ListView.builder(
        padding: const EdgeInsets.fromLTRB(12, 12, 12, 24),
        itemCount: _templates.length,
        itemBuilder: (ctx, i) {
          final t = _templates[i];
          return Card(
            color: c.surfaceAlt,
            margin: const EdgeInsets.only(bottom: 10),
            shape: RoundedRectangleBorder(
              borderRadius: BorderRadius.circular(14),
              side: BorderSide(color: c.border),
            ),
            child: ListTile(
              contentPadding:
                  const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
              leading: Text(t.icon, style: const TextStyle(fontSize: 26)),
              title: Row(
                children: [
                  Expanded(
                    child: Text(t.name,
                        style: TextStyle(
                            color: c.textPrimary,
                            fontWeight: FontWeight.w600)),
                  ),
                  if (t.builtin)
                    Container(
                      padding: const EdgeInsets.symmetric(
                          horizontal: 6, vertical: 2),
                      decoration: BoxDecoration(
                        color: c.surfaceAlt,
                        borderRadius: BorderRadius.circular(5),
                      ),
                      child: Text('builtin',
                          style:
                              TextStyle(color: c.textMuted, fontSize: 9)),
                    ),
                ],
              ),
              subtitle: Padding(
                padding: const EdgeInsets.only(top: 4),
                child: Text(
                  '${t.description}\n${t.manager} · ${t.members.length} thành viên',
                  style: TextStyle(color: c.textMuted, fontSize: 12),
                ),
              ),
              isThreeLine: true,
              trailing: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  IconButton(
                    tooltip: 'Tạo đội',
                    icon: Icon(Icons.play_circle_outline,
                        color: c.accent),
                    onPressed: () => _use(t),
                  ),
                  if (!t.builtin)
                    IconButton(
                      icon: Icon(Icons.delete_outline,
                          color: c.textMuted, size: 20),
                      onPressed: () => _delete(t),
                    ),
                ],
              ),
            ),
          );
        },
      ),
    );
  }
}
