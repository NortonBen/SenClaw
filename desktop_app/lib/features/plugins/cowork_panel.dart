import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import '../../core/i18n/l10n.dart';
import '../../core/transport/connection.dart';
import '../../models/cowork_models.dart';
import '../../theme/tokens.dart';
import '../cowork/cowork_providers.dart';

/// Cowork management panel for the Plugins screen — mirrors the web
/// `CoworkPanel`: two tabs (Templates · Teams) for managing reusable team
/// blueprints and the live teams spun up from them. The full team
/// detail/Kanban lives on the `/cowork` route (reached via "Open").
class CoworkPanel extends ConsumerWidget {
  const CoworkPanel({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final teams = ref.watch(teamsProvider).valueOrNull ?? const [];
    return DefaultTabController(
      length: 3,
      child: Column(
        children: [
          // Header
          Padding(
            padding: const EdgeInsets.fromLTRB(
                AppTokens.s24, AppTokens.s16, AppTokens.s24, 0),
            child: Row(
              children: [
                Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text('Cowork',
                        style: TextStyle(
                            color: c.textPrimary,
                            fontSize: 16,
                            fontWeight: FontWeight.w700)),
                    Text(context.tr('Manage team templates & multi-agent teams'),
                        style: TextStyle(color: c.textMuted, fontSize: 12)),
                  ],
                ),
                const Spacer(),
                IconButton(
                  tooltip: context.tr('Reload'),
                  icon: const Icon(Icons.refresh, size: 18),
                  onPressed: () {
                    ref.invalidate(coworkTemplatesProvider);
                    ref.invalidate(teamsProvider);
                  },
                ),
              ],
            ),
          ),
          TabBar(
            isScrollable: true,
            tabAlignment: TabAlignment.start,
            labelColor: c.accent,
            unselectedLabelColor: c.textMuted,
            indicatorColor: c.accent,
            tabs: [
              Tab(text: context.tr('Templates')),
              Tab(text: context.trArgs('Teams ({n})', {'n': teams.length})),
              Tab(text: context.tr('Personas')),
            ],
          ),
          const Expanded(
            child: TabBarView(
              children: [_TemplatesTab(), _TeamsTab(), _PersonasTab()],
            ),
          ),
        ],
      ),
    );
  }
}

class _TemplatesTab extends ConsumerWidget {
  const _TemplatesTab();
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final templates = ref.watch(coworkTemplatesProvider);
    return templates.when(
      loading: () => const Center(child: CircularProgressIndicator()),
      error: (e, _) => Center(child: Text('$e')),
      data: (list) => SingleChildScrollView(
        padding: const EdgeInsets.all(AppTokens.s24),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Expanded(
                  child: Text(
                      context.tr('Built-in blueprints + your custom templates. '
                          'Use one to spin up a team.'),
                      style: TextStyle(color: c.textMuted, fontSize: 12)),
                ),
                FilledButton.icon(
                  onPressed: () => showDialog(
                      context: context,
                      builder: (_) => const _TemplateEditor()),
                  icon: const Icon(Icons.add, size: 16),
                  label: Text(context.tr('New template')),
                ),
              ],
            ),
            const SizedBox(height: AppTokens.s16),
            Wrap(
              spacing: AppTokens.s12,
              runSpacing: AppTokens.s12,
              children: [for (final t in list) _TemplateCard(template: t)],
            ),
          ],
        ),
      ),
    );
  }
}

class _TemplateCard extends ConsumerWidget {
  const _TemplateCard({required this.template});
  final CoworkTemplate template;
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final t = template;
    return Container(
      width: 300,
      padding: const EdgeInsets.all(AppTokens.s16),
      decoration: BoxDecoration(
        color: c.surface,
        border: Border.all(color: c.border),
        borderRadius: BorderRadius.circular(AppTokens.rLg),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Text(t.icon, style: const TextStyle(fontSize: 22)),
              const SizedBox(width: AppTokens.s8),
              Expanded(
                child: Text(t.name,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                        color: c.textPrimary, fontWeight: FontWeight.w700)),
              ),
              if (t.builtin)
                Container(
                  padding:
                      const EdgeInsets.symmetric(horizontal: 6, vertical: 1),
                  decoration: BoxDecoration(
                    color: c.accentSoft,
                    borderRadius: BorderRadius.circular(AppTokens.rSm),
                  ),
                  child: Text(context.tr('built-in'),
                      style: TextStyle(color: c.accent, fontSize: 11)),
                ),
            ],
          ),
          const SizedBox(height: AppTokens.s8),
          SizedBox(
            height: 54,
            child: Text(
                t.description.isEmpty
                    ? context.tr('No description')
                    : t.description,
                maxLines: 3,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(color: c.textMuted, fontSize: 12)),
          ),
          const SizedBox(height: AppTokens.s8),
          Row(
            children: [
              Icon(Icons.smart_toy_outlined, size: 14, color: c.textMuted),
              const SizedBox(width: 4),
              Expanded(
                child: Text(
                    '${t.manager.isEmpty ? 'manager' : t.manager} · '
                    '${context.trPlural(t.memberCount, '{n} member', '{n} members')}',
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(color: c.textMuted, fontSize: 12)),
              ),
            ],
          ),
          const SizedBox(height: AppTokens.s12),
          Row(
            children: [
              FilledButton.icon(
                onPressed: () async {
                  final id = await createTeamFromTemplate(ref, t.id);
                  if (id != null && context.mounted) {
                    DefaultTabController.of(context).animateTo(1);
                    ScaffoldMessenger.of(context).showSnackBar(SnackBar(
                        content: Text(context.trArgs(
                            'Team created from {name}', {'name': t.name}))));
                  }
                },
                icon: const Icon(Icons.bolt, size: 16),
                label: Text(context.tr('Use')),
              ),
              const Spacer(),
              IconButton(
                tooltip:
                    context.tr(t.builtin ? 'Clone to edit' : 'Edit'),
                icon: const Icon(Icons.edit_outlined, size: 16),
                onPressed: () => showDialog(
                    context: context,
                    builder: (_) => _TemplateEditor(template: t)),
              ),
              if (!t.builtin)
                IconButton(
                  tooltip: context.tr('Delete'),
                  icon: const Icon(Icons.delete_outline,
                      size: 16, color: AppTokens.danger),
                  onPressed: () async {
                    await ref
                        .read(apiClientProvider)
                        .delete('/api/cowork/templates/${t.id}');
                    ref.invalidate(coworkTemplatesProvider);
                  },
                ),
            ],
          ),
        ],
      ),
    );
  }
}

class _TeamsTab extends ConsumerWidget {
  const _TeamsTab();
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final teams = ref.watch(teamsProvider);
    return teams.when(
      loading: () => const Center(child: CircularProgressIndicator()),
      error: (e, _) => Center(child: Text('$e')),
      data: (list) => list.isEmpty
          ? Center(
              child: Text(context.tr('No teams yet — create one from a template'),
                  style: TextStyle(color: c.textMuted)))
          : SingleChildScrollView(
              padding: const EdgeInsets.all(AppTokens.s24),
              child: Wrap(
                spacing: AppTokens.s12,
                runSpacing: AppTokens.s12,
                children: [for (final t in list) _TeamCard(team: t)],
              ),
            ),
    );
  }
}

class _TeamCard extends ConsumerWidget {
  const _TeamCard({required this.team});
  final CoworkTeam team;
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final t = team;
    return Container(
      width: 340,
      padding: const EdgeInsets.all(AppTokens.s16),
      decoration: BoxDecoration(
        color: c.surface,
        border: Border.all(color: c.border),
        borderRadius: BorderRadius.circular(AppTokens.rLg),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Container(
                width: 34,
                height: 34,
                decoration: BoxDecoration(
                  gradient: const LinearGradient(
                      colors: [AppTokens.brand, AppTokens.brandAlt]),
                  borderRadius: BorderRadius.circular(AppTokens.rMd),
                ),
                child: const Icon(Icons.groups, size: 18, color: Colors.white),
              ),
              const SizedBox(width: AppTokens.s8),
              Expanded(
                child: Text(t.name,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                        color: c.textPrimary, fontWeight: FontWeight.w700)),
              ),
            ],
          ),
          const SizedBox(height: AppTokens.s8),
          Row(
            children: [
              Icon(Icons.smart_toy_outlined, size: 14, color: c.textMuted),
              const SizedBox(width: 4),
              Text(
                  '${t.managerFolder} · '
                  '${context.trPlural(t.members.length, '{n} member', '{n} members')}',
                  style: TextStyle(color: c.textMuted, fontSize: 12)),
            ],
          ),
          const SizedBox(height: AppTokens.s8),
          Wrap(
            spacing: AppTokens.s6,
            runSpacing: AppTokens.s6,
            children: [
              for (final m in t.members)
                Container(
                  padding:
                      const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
                  decoration: BoxDecoration(
                    color: c.surfaceAlt,
                    borderRadius: BorderRadius.circular(AppTokens.rXl),
                    border: Border.all(color: c.border),
                  ),
                  child: Text(
                      '${m.folder}${m.role.isEmpty ? '' : ' · ${m.role}'}',
                      style: TextStyle(color: c.textSecondary, fontSize: 11)),
                ),
            ],
          ),
          const SizedBox(height: AppTokens.s12),
          Wrap(
            spacing: AppTokens.s8,
            children: [
              FilledButton.icon(
                onPressed: () {
                  ref.read(openTeamProvider.notifier).state = t.id;
                  context.go('/cowork');
                },
                icon: const Icon(Icons.open_in_new, size: 16),
                label: Text(context.tr('Open')),
              ),
              OutlinedButton.icon(
                onPressed: () => showDialog(
                    context: context,
                    builder: (_) => _TeamSettingsDialog(team: t)),
                icon: const Icon(Icons.settings_outlined, size: 16),
                label: Text(context.tr('Settings')),
              ),
              TextButton.icon(
                onPressed: () async {
                  await ref.read(apiClientProvider).post(
                      '/api/cowork/teams/${t.id}/save-as-template',
                      body: {});
                  ref.invalidate(coworkTemplatesProvider);
                  if (context.mounted) {
                    ScaffoldMessenger.of(context).showSnackBar(SnackBar(
                        content: Text(context.tr('Saved as template'))));
                  }
                },
                icon: const Icon(Icons.bookmark_add_outlined, size: 16),
                label: Text(context.tr('Save as template')),
              ),
            ],
          ),
        ],
      ),
    );
  }
}

/// Create or edit a Cowork template (web CoworkPanel editor). Built-in
/// templates open as a clone (blank id → POSTs a new custom template).
class _TemplateEditor extends ConsumerStatefulWidget {
  const _TemplateEditor({this.template});
  final CoworkTemplate? template;
  @override
  ConsumerState<_TemplateEditor> createState() => _TemplateEditorState();
}

class _MemberFields {
  final folder = TextEditingController();
  final role = TextEditingController();
  final resp = TextEditingController();
  final triggers = TextEditingController();
  void dispose() {
    folder.dispose();
    role.dispose();
    resp.dispose();
    triggers.dispose();
  }
}

class _TemplateEditorState extends ConsumerState<_TemplateEditor> {
  final _name = TextEditingController();
  final _description = TextEditingController();
  final _icon = TextEditingController(text: '🧩');
  final _manager = TextEditingController();
  final _managerRole = TextEditingController(text: 'lead');
  final List<_MemberFields> _members = [];
  bool _autoCreateTasks = true;
  bool _saving = false;
  String? _error;

  bool get _isClone => widget.template?.builtin ?? false;
  // A blank id forces POST (new). Editing a custom template keeps its id (PUT).
  String get _id => _isClone ? '' : (widget.template?.id ?? '');

  @override
  void initState() {
    super.initState();
    final raw = widget.template?.raw ?? const {};
    if (raw.isNotEmpty) {
      _name.text =
          _isClone ? '${raw['name'] ?? ''} (copy)' : '${raw['name'] ?? ''}';
      _description.text = '${raw['description'] ?? ''}';
      _icon.text = '${raw['icon'] ?? '🧩'}';
      _manager.text = '${raw['manager'] ?? raw['manager_folder'] ?? ''}';
      _managerRole.text = '${raw['manager_role'] ?? 'lead'}';
      final settings = (raw['settings'] as Map?) ?? const {};
      _autoCreateTasks = settings['auto_create_tasks'] != false;
      for (final m in (raw['members'] as List?) ?? const []) {
        if (m is Map) {
          final f = _MemberFields()
            ..folder.text = '${m['folder'] ?? ''}'
            ..role.text = '${m['role'] ?? ''}'
            ..resp.text = '${m['responsibilities'] ?? ''}'
            ..triggers.text =
                m['triggers'] is String ? '${m['triggers']}' : '';
          _members.add(f);
        }
      }
    }
  }

  @override
  void dispose() {
    _name.dispose();
    _description.dispose();
    _icon.dispose();
    _manager.dispose();
    _managerRole.dispose();
    for (final m in _members) {
      m.dispose();
    }
    super.dispose();
  }

  Future<void> _save() async {
    if (_name.text.trim().isEmpty || _manager.text.trim().isEmpty) {
      setState(() =>
          _error = context.tr('Name and manager folder are required'));
      return;
    }
    setState(() {
      _saving = true;
      _error = null;
    });
    final body = {
      'name': _name.text.trim(),
      'description': _description.text.trim(),
      'icon': _icon.text.trim().isEmpty ? '🧩' : _icon.text.trim(),
      'manager_folder': _manager.text.trim(),
      'manager_role': _managerRole.text.trim(),
      'members': [
        for (final m in _members)
          if (m.folder.text.trim().isNotEmpty)
            {
              'folder': m.folder.text.trim(),
              'role': m.role.text.trim(),
              'responsibilities': m.resp.text.trim(),
              if (m.triggers.text.trim().isNotEmpty)
                'triggers': m.triggers.text.trim(),
            }
      ],
      'settings': {'auto_create_tasks': _autoCreateTasks},
    };
    try {
      final api = ref.read(apiClientProvider);
      if (_id.isEmpty) {
        await api.post('/api/cowork/templates', body: body);
      } else {
        await api.put('/api/cowork/templates/$_id', body: body);
      }
      ref.invalidate(coworkTemplatesProvider);
      if (mounted) Navigator.pop(context);
    } catch (e) {
      setState(() => _error = '$e');
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Dialog(
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 640, maxHeight: 660),
        child: Padding(
          padding: const EdgeInsets.all(AppTokens.s20),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(context.tr(_id.isEmpty ? 'New template' : 'Edit template'),
                  style: TextStyle(
                      color: c.textPrimary,
                      fontSize: 16,
                      fontWeight: FontWeight.w700)),
              const SizedBox(height: AppTokens.s12),
              Expanded(
                child: ListView(
                  children: [
                    Row(
                      children: [
                        SizedBox(
                          width: 70,
                          child: TextField(
                            controller: _icon,
                            textAlign: TextAlign.center,
                            decoration:
                                InputDecoration(labelText: context.tr('Icon')),
                          ),
                        ),
                        const SizedBox(width: AppTokens.s8),
                        Expanded(
                          child: TextField(
                            controller: _name,
                            decoration:
                                InputDecoration(labelText: context.tr('Name')),
                          ),
                        ),
                      ],
                    ),
                    const SizedBox(height: AppTokens.s8),
                    TextField(
                      controller: _description,
                      minLines: 2,
                      maxLines: 3,
                      decoration: InputDecoration(
                          labelText: context.tr('Description')),
                    ),
                    const SizedBox(height: AppTokens.s8),
                    Row(
                      children: [
                        Expanded(
                          child: TextField(
                            controller: _manager,
                            decoration: InputDecoration(
                                labelText: context.tr('Manager folder')),
                          ),
                        ),
                        const SizedBox(width: AppTokens.s8),
                        Expanded(
                          child: TextField(
                            controller: _managerRole,
                            decoration: InputDecoration(
                                labelText: context.tr('Manager role')),
                          ),
                        ),
                      ],
                    ),
                    const SizedBox(height: AppTokens.s12),
                    Row(
                      children: [
                        Text(context.tr('Members'),
                            style: TextStyle(
                                color: c.textSecondary,
                                fontWeight: FontWeight.w700)),
                        const Spacer(),
                        TextButton.icon(
                          onPressed: () =>
                              setState(() => _members.add(_MemberFields())),
                          icon: const Icon(Icons.add, size: 16),
                          label: Text(context.tr('Add member')),
                        ),
                      ],
                    ),
                    for (var i = 0; i < _members.length; i++)
                      Container(
                        margin: const EdgeInsets.only(bottom: AppTokens.s8),
                        padding: const EdgeInsets.all(AppTokens.s8),
                        decoration: BoxDecoration(
                          color: c.surfaceAlt,
                          borderRadius: BorderRadius.circular(AppTokens.rMd),
                          border: Border.all(color: c.border),
                        ),
                        child: Column(
                          children: [
                            Row(
                              children: [
                                Expanded(
                                  child: TextField(
                                    controller: _members[i].folder,
                                    decoration: InputDecoration(
                                        isDense: true,
                                        hintText:
                                            context.tr('folder (persona)')),
                                  ),
                                ),
                                const SizedBox(width: AppTokens.s8),
                                SizedBox(
                                  width: 130,
                                  child: TextField(
                                    controller: _members[i].role,
                                    decoration: InputDecoration(
                                        isDense: true,
                                        hintText: context.tr('role')),
                                  ),
                                ),
                                IconButton(
                                  icon: const Icon(Icons.delete_outline,
                                      size: 16, color: AppTokens.danger),
                                  onPressed: () => setState(() {
                                    _members[i].dispose();
                                    _members.removeAt(i);
                                  }),
                                ),
                              ],
                            ),
                            TextField(
                              controller: _members[i].resp,
                              decoration: InputDecoration(
                                  isDense: true,
                                  hintText: context.tr('responsibilities')),
                            ),
                            const SizedBox(height: 4),
                            TextField(
                              controller: _members[i].triggers,
                              style: const TextStyle(fontSize: 12),
                              decoration: InputDecoration(
                                  isDense: true,
                                  hintText: context.tr(
                                      'triggers JSON e.g. [{"type":"task_assigned"}]')),
                            ),
                          ],
                        ),
                      ),
                    const SizedBox(height: AppTokens.s8),
                    SwitchListTile(
                      contentPadding: EdgeInsets.zero,
                      title: Text(
                          context.tr('Auto-create tasks on each user message'),
                          style: TextStyle(
                              color: c.textPrimary, fontSize: 14)),
                      value: _autoCreateTasks,
                      onChanged: (v) =>
                          setState(() => _autoCreateTasks = v),
                    ),
                  ],
                ),
              ),
              if (_error != null)
                Padding(
                  padding: const EdgeInsets.only(top: AppTokens.s8),
                  child: Text(_error!,
                      style: const TextStyle(color: AppTokens.danger)),
                ),
              const SizedBox(height: AppTokens.s12),
              Row(
                mainAxisAlignment: MainAxisAlignment.end,
                children: [
                  TextButton(
                      onPressed: () => Navigator.pop(context),
                      child: Text(context.tr('Cancel'))),
                  const SizedBox(width: AppTokens.s8),
                  FilledButton(
                    onPressed: _saving ? null : _save,
                    child: _saving
                        ? const SizedBox(
                            width: 14,
                            height: 14,
                            child: CircularProgressIndicator(strokeWidth: 2))
                        : Text(context.tr('Save')),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// Edit a live team's behaviour settings (web BehaviourFields):
/// auto-create-tasks + manager preamble → PUT /api/cowork/teams/:id.
class _TeamSettingsDialog extends ConsumerStatefulWidget {
  const _TeamSettingsDialog({required this.team});
  final CoworkTeam team;
  @override
  ConsumerState<_TeamSettingsDialog> createState() =>
      _TeamSettingsDialogState();
}

class _TeamSettingsDialogState extends ConsumerState<_TeamSettingsDialog> {
  late bool _autoCreate;
  late final TextEditingController _preamble;
  bool _saving = false;

  @override
  void initState() {
    super.initState();
    final s = widget.team.settings;
    _autoCreate = s['auto_create_tasks'] != false;
    _preamble =
        TextEditingController(text: '${s['manager_preamble'] ?? ''}');
  }

  @override
  void dispose() {
    _preamble.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    setState(() => _saving = true);
    try {
      await ref.read(apiClientProvider).put(
        '/api/cowork/teams/${widget.team.id}',
        body: {
          'name': widget.team.name,
          'manager_folder': widget.team.managerFolder,
          'workspace_dir': '',
          'settings': {
            'auto_create_tasks': _autoCreate,
            'manager_preamble':
                _preamble.text.trim().isEmpty ? null : _preamble.text.trim(),
          },
        },
      );
      ref.invalidate(teamsProvider);
      if (mounted) Navigator.pop(context);
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return AlertDialog(
      backgroundColor: c.surface,
      title: Text(context
          .trArgs('{name} · settings', {'name': widget.team.name})),
      content: SizedBox(
        width: 480,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            SwitchListTile(
              contentPadding: EdgeInsets.zero,
              title: Text(context.tr('Auto-create tasks on each user message'),
                  style: TextStyle(color: c.textPrimary, fontSize: 14)),
              value: _autoCreate,
              onChanged: (v) => setState(() => _autoCreate = v),
            ),
            const SizedBox(height: AppTokens.s8),
            TextField(
              controller: _preamble,
              minLines: 3,
              maxLines: 6,
              decoration: InputDecoration(
                labelText: context.tr('Manager preamble'),
                hintText: context
                    .tr('Extra system instructions prepended for the manager'),
                alignLabelWithHint: true,
              ),
            ),
          ],
        ),
      ),
      actions: [
        TextButton(
            onPressed: () => Navigator.pop(context),
            child: Text(context.tr('Cancel'))),
        FilledButton(
          onPressed: _saving ? null : _save,
          child: _saving
              ? const SizedBox(
                  width: 14,
                  height: 14,
                  child: CircularProgressIndicator(strokeWidth: 2))
              : Text(context.tr('Save')),
        ),
      ],
    );
  }
}

// ── Personas tab (web /api/cowork/personas) ─────────────────────────────────
class _PersonaInfo {
  final String name;
  final String description;
  const _PersonaInfo(this.name, this.description);
}

final coworkPersonasProvider = FutureProvider<List<_PersonaInfo>>((ref) async {
  final r = await ref.read(apiClientProvider).get('/api/cowork/personas');
  final list = (r is List ? r : (r is Map ? r['personas'] : null)) as List? ??
      const [];
  return list
      .whereType<Map>()
      .map((m) => _PersonaInfo(
          '${m['name'] ?? ''}', '${m['description'] ?? ''}'))
      .toList();
});

class _PersonasTab extends ConsumerWidget {
  const _PersonasTab();
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final personas = ref.watch(coworkPersonasProvider);
    return personas.when(
      loading: () => const Center(child: CircularProgressIndicator()),
      error: (e, _) => Center(child: Text('$e')),
      data: (list) => list.isEmpty
          ? Center(
              child: Text(context.tr('No personas'),
                  style: TextStyle(color: c.textMuted)))
          : ListView.builder(
              padding: const EdgeInsets.all(AppTokens.s16),
              itemCount: list.length,
              itemBuilder: (_, i) {
                final p = list[i];
                return Container(
                  margin: const EdgeInsets.only(bottom: AppTokens.s8),
                  padding: const EdgeInsets.all(AppTokens.s12),
                  decoration: BoxDecoration(
                    color: c.surface,
                    border: Border.all(color: c.border),
                    borderRadius: BorderRadius.circular(AppTokens.rMd),
                  ),
                  child: Row(
                    children: [
                      Icon(Icons.person_outline, size: 16, color: c.accent),
                      const SizedBox(width: AppTokens.s12),
                      Expanded(
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Text(p.name,
                                style: TextStyle(
                                    color: c.textPrimary,
                                    fontWeight: FontWeight.w600)),
                            if (p.description.isNotEmpty)
                              Text(p.description,
                                  maxLines: 2,
                                  overflow: TextOverflow.ellipsis,
                                  style: TextStyle(
                                      color: c.textMuted, fontSize: 12)),
                          ],
                        ),
                      ),
                      TextButton.icon(
                        onPressed: () => showDialog(
                            context: context,
                            builder: (_) => _PersonaEditor(name: p.name)),
                        icon: const Icon(Icons.edit_outlined, size: 14),
                        label: Text(context.tr('Edit')),
                      ),
                    ],
                  ),
                );
              },
            ),
    );
  }
}

class _PersonaEditor extends ConsumerStatefulWidget {
  const _PersonaEditor({required this.name});
  final String name;
  @override
  ConsumerState<_PersonaEditor> createState() => _PersonaEditorState();
}

class _PersonaEditorState extends ConsumerState<_PersonaEditor> {
  final _content = TextEditingController();
  bool _loading = true;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    try {
      final r = await ref
          .read(apiClientProvider)
          .get('/api/cowork/personas/${widget.name}/file');
      if (mounted && r is Map) _content.text = '${r['content'] ?? ''}';
    } catch (_) {}
    if (mounted) setState(() => _loading = false);
  }

  @override
  void dispose() {
    _content.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    await ref.read(apiClientProvider).put(
        '/api/cowork/personas/${widget.name}/file',
        body: {'content': _content.text});
    ref.invalidate(coworkPersonasProvider);
    if (mounted) Navigator.of(context).pop();
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Dialog(
      backgroundColor: c.surface,
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 640, maxHeight: 560),
        child: Padding(
          padding: const EdgeInsets.all(AppTokens.s20),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(context.trArgs('Persona · {name}', {'name': widget.name}),
                  style: TextStyle(
                      color: c.textPrimary,
                      fontSize: 16,
                      fontWeight: FontWeight.w700)),
              const SizedBox(height: AppTokens.s12),
              Expanded(
                child: _loading
                    ? const Center(child: CircularProgressIndicator())
                    : TextField(
                        controller: _content,
                        expands: true,
                        maxLines: null,
                        textAlignVertical: TextAlignVertical.top,
                        decoration: InputDecoration(
                            hintText: context.tr('Persona markdown…'),
                            border: const OutlineInputBorder()),
                      ),
              ),
              const SizedBox(height: AppTokens.s16),
              Row(
                mainAxisAlignment: MainAxisAlignment.end,
                children: [
                  TextButton(
                      onPressed: () => Navigator.of(context).pop(),
                      child: Text(context.tr('Cancel'))),
                  const SizedBox(width: AppTokens.s8),
                  FilledButton(
                      onPressed: _loading ? null : _save,
                      child: Text(context.tr('Save'))),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }
}
