import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/i18n/l10n.dart';
import '../../core/transport/connection.dart';
import '../../models/cowork_models.dart';
import '../../theme/tokens.dart';
import '../../widgets/app_markdown.dart';
import '../../widgets/section_scaffold.dart';
import 'cowork_providers.dart';

class CoworkScreen extends ConsumerWidget {
  const CoworkScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final openTeam = ref.watch(openTeamProvider);
    if (openTeam != null) return _TeamDetail(teamId: openTeam);

    final teams = ref.watch(teamsProvider);
    return SectionScaffold(
      title: 'Cowork',
      subtitle: context.tr('Multi-agent teams'),
      actions: [
        OutlinedButton.icon(
          onPressed: () => ref.invalidate(teamsProvider),
          icon: const Icon(Icons.refresh, size: 16),
          label: Text(context.tr('Refresh')),
        ),
        const SizedBox(width: AppTokens.s8),
        FilledButton.icon(
          onPressed: () => showDialog(
              context: context, builder: (_) => const _TemplatePicker()),
          icon: const Icon(Icons.add_rounded, size: 18),
          label: Text(context.tr('New team')),
          style: FilledButton.styleFrom(
            backgroundColor: context.colors.accent,
            foregroundColor: Colors.white,
            elevation: 0,
            padding: const EdgeInsets.symmetric(
                horizontal: AppTokens.s16, vertical: AppTokens.s12),
            shape: RoundedRectangleBorder(
                borderRadius: BorderRadius.circular(AppTokens.rXl)),
            textStyle:
                const TextStyle(fontSize: 13, fontWeight: FontWeight.w600),
          ),
        ),
      ],
      body: teams.when(
        loading: () => const Center(child: CircularProgressIndicator()),
        error: (e, _) => Center(child: Text('$e')),
        data: (list) {
          if (list.isEmpty) {
            return Center(
              child: Text(context.tr('No teams yet'),
                  style: TextStyle(color: context.colors.textMuted)),
            );
          }
          return SingleChildScrollView(
            padding: const EdgeInsets.all(AppTokens.s24),
            child: Wrap(
              spacing: AppTokens.s16,
              runSpacing: AppTokens.s16,
              children: [for (final t in list) _TeamCard(team: t)],
            ),
          );
        },
      ),
    );
  }
}

/// Picks a Cowork template and spins up a new team from it.
class _TemplatePicker extends ConsumerWidget {
  const _TemplatePicker();
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final templates = ref.watch(coworkTemplatesProvider);
    return Dialog(
      backgroundColor: c.surface,
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 560, maxHeight: 560),
        child: Padding(
          padding: const EdgeInsets.all(AppTokens.s20),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(context.tr('New team from template'),
                  style: TextStyle(
                      color: c.textPrimary,
                      fontSize: 16,
                      fontWeight: FontWeight.w700)),
              const SizedBox(height: AppTokens.s16),
              Flexible(
                child: templates.when(
                  loading: () =>
                      const Center(child: CircularProgressIndicator()),
                  error: (e, _) => Text('$e'),
                  data: (list) => ListView(
                    shrinkWrap: true,
                    children: [
                      for (final t in list)
                        Padding(
                          padding: const EdgeInsets.only(bottom: AppTokens.s8),
                          child: InkWell(
                            borderRadius:
                                BorderRadius.circular(AppTokens.rMd),
                            onTap: () async {
                              final id =
                                  await createTeamFromTemplate(ref, t.id);
                              if (context.mounted) Navigator.of(context).pop();
                              if (id != null) {
                                ref.read(openTeamProvider.notifier).state = id;
                              }
                            },
                            child: Container(
                              padding: const EdgeInsets.all(AppTokens.s12),
                              decoration: BoxDecoration(
                                border: Border.all(color: c.border),
                                borderRadius:
                                    BorderRadius.circular(AppTokens.rMd),
                              ),
                              child: Row(
                                children: [
                                  Text(t.icon,
                                      style: const TextStyle(fontSize: 20)),
                                  const SizedBox(width: AppTokens.s12),
                                  Expanded(
                                    child: Column(
                                      crossAxisAlignment:
                                          CrossAxisAlignment.start,
                                      children: [
                                        Text(t.name,
                                            style: TextStyle(
                                                color: c.textPrimary,
                                                fontWeight: FontWeight.w600)),
                                        Text(t.description,
                                            maxLines: 2,
                                            overflow: TextOverflow.ellipsis,
                                            style: TextStyle(
                                                color: c.textMuted,
                                                fontSize: 12)),
                                      ],
                                    ),
                                  ),
                                  Text(
                                      context.trPlural(t.memberCount,
                                          '{n} member', '{n} members'),
                                      style: TextStyle(
                                          color: c.textMuted, fontSize: 12)),
                                ],
                              ),
                            ),
                          ),
                        ),
                    ],
                  ),
                ),
              ),
              const SizedBox(height: AppTokens.s12),
              Align(
                alignment: Alignment.centerRight,
                child: TextButton(
                    onPressed: () => Navigator.of(context).pop(),
                    child: Text(context.tr('Cancel'))),
              ),
            ],
          ),
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
    return SizedBox(
      width: 320,
      child: InkWell(
        borderRadius: BorderRadius.circular(AppTokens.rLg),
        onTap: () => ref.read(openTeamProvider.notifier).state = team.id,
        child: Container(
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
                    width: 36,
                    height: 36,
                    decoration: BoxDecoration(
                      gradient: const LinearGradient(
                          colors: [AppTokens.brand, AppTokens.brandAlt]),
                      borderRadius: BorderRadius.circular(AppTokens.rMd),
                    ),
                    alignment: Alignment.center,
                    child: const Icon(Icons.groups_2, color: Colors.white, size: 18),
                  ),
                  const SizedBox(width: AppTokens.s12),
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(team.name,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: TextStyle(
                                color: c.textPrimary,
                                fontWeight: FontWeight.w700)),
                        Text(
                            context.trArgs('manager · {folder}',
                                {'folder': team.managerFolder}),
                            style: TextStyle(color: c.textMuted, fontSize: 12)),
                      ],
                    ),
                  ),
                ],
              ),
              const SizedBox(height: AppTokens.s12),
              Wrap(
                spacing: AppTokens.s6,
                runSpacing: AppTokens.s6,
                children: [
                  for (final m in team.members)
                    Container(
                      padding: const EdgeInsets.symmetric(
                          horizontal: AppTokens.s8, vertical: AppTokens.s4),
                      decoration: BoxDecoration(
                        color: c.surfaceAlt,
                        borderRadius: BorderRadius.circular(AppTokens.rFull),
                        border: Border.all(color: c.border),
                      ),
                      child: Text('${m.folder} · ${m.role}',
                          style: TextStyle(color: c.textSecondary, fontSize: 12)),
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

class _TeamDetail extends ConsumerWidget {
  const _TeamDetail({required this.teamId});
  final String teamId;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final teams = ref.watch(teamsProvider).valueOrNull ?? const [];
    final team = teams.where((t) => t.id == teamId).firstOrNull;
    final tasks = ref.watch(teamTasksProvider(teamId));

    return Column(
      children: [
        Container(
          height: 56,
          padding: const EdgeInsets.symmetric(horizontal: AppTokens.s16),
          decoration: BoxDecoration(
            border: Border(bottom: BorderSide(color: c.border)),
          ),
          child: Row(
            children: [
              IconButton(
                onPressed: () => ref.read(openTeamProvider.notifier).state = null,
                icon: const Icon(Icons.arrow_back, size: 18),
              ),
              const SizedBox(width: AppTokens.s8),
              Text(team?.name ?? context.tr('Team'),
                  style: TextStyle(
                      color: c.textPrimary,
                      fontSize: 14,
                      fontWeight: FontWeight.w700)),
              const Spacer(),
              IconButton(
                tooltip: context.tr('Workspace files'),
                onPressed: () => showDialog(
                    context: context,
                    builder: (_) => _WorkspaceBrowser(teamId: teamId)),
                icon: const Icon(Icons.folder_open_outlined, size: 18),
              ),
              IconButton(
                tooltip: context.tr('Refresh tasks'),
                onPressed: () => ref.invalidate(teamTasksProvider(teamId)),
                icon: const Icon(Icons.refresh, size: 18),
              ),
            ],
          ),
        ),
        // Members strip — tap a chip to edit role/responsibilities.
        if (team != null && team.members.isNotEmpty)
          Container(
            padding: const EdgeInsets.symmetric(
                horizontal: AppTokens.s16, vertical: AppTokens.s8),
            decoration: BoxDecoration(
              border: Border(bottom: BorderSide(color: c.border)),
            ),
            child: Wrap(
              spacing: AppTokens.s8,
              runSpacing: AppTokens.s8,
              children: [
                for (final m in team.members)
                  _MemberChip(
                    member: m,
                    onTap: () => showDialog(
                        context: context,
                        builder: (_) =>
                            _MemberEditor(teamId: teamId, member: m)),
                  ),
                _AddMemberChip(teamId: teamId),
              ],
            ),
          ),
        Expanded(
          child: tasks.when(
            loading: () => const Center(child: CircularProgressIndicator()),
            error: (e, _) => Center(child: Text('$e')),
            data: (list) => _Kanban(teamId: teamId, tasks: list),
          ),
        ),
      ],
    );
  }
}

/// Sub-agent member chip styled by role (web AgentCard): lead=gold/crown,
/// reviewer=purple, others=blue/robot. Tap to edit.
class _MemberChip extends StatelessWidget {
  const _MemberChip({required this.member, required this.onTap});
  final CoworkMember member;
  final VoidCallback onTap;
  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final (color, icon) = switch (member.role) {
      'lead' || 'manager' =>
        (const Color(0xFFE0A500), Icons.workspace_premium_outlined),
      'reviewer' || 'verifier' =>
        (const Color(0xFF8B5CF6), Icons.verified_outlined),
      _ => (c.accent, Icons.smart_toy_outlined),
    };
    return InkWell(
      borderRadius: BorderRadius.circular(AppTokens.rLg),
      onTap: onTap,
      child: Container(
        padding: const EdgeInsets.symmetric(
            horizontal: AppTokens.s12, vertical: AppTokens.s8),
        decoration: BoxDecoration(
          color: color.withValues(alpha: 0.10),
          borderRadius: BorderRadius.circular(AppTokens.rLg),
          border: Border.all(color: color.withValues(alpha: 0.35)),
        ),
        child: Row(mainAxisSize: MainAxisSize.min, children: [
          Icon(icon, size: 15, color: color),
          const SizedBox(width: AppTokens.s8),
          Text(member.folder,
              style: TextStyle(
                  color: c.textPrimary,
                  fontSize: 13,
                  fontWeight: FontWeight.w600)),
          if (member.role.isNotEmpty) ...[
            const SizedBox(width: AppTokens.s8),
            Container(
              padding:
                  const EdgeInsets.symmetric(horizontal: 6, vertical: 1),
              decoration: BoxDecoration(
                color: color.withValues(alpha: 0.16),
                borderRadius: BorderRadius.circular(AppTokens.rSm),
              ),
              child: Text(member.role,
                  style: TextStyle(
                      color: color,
                      fontSize: 10,
                      fontWeight: FontWeight.w700)),
            ),
          ],
          const SizedBox(width: 4),
          Icon(Icons.edit_outlined, size: 12, color: c.textMuted),
        ]),
      ),
    );
  }
}

class _MemberEditor extends ConsumerStatefulWidget {
  const _MemberEditor({required this.teamId, required this.member});
  final String teamId;
  final CoworkMember member;
  @override
  ConsumerState<_MemberEditor> createState() => _MemberEditorState();
}

class _MemberEditorState extends ConsumerState<_MemberEditor> {
  late final _role = TextEditingController(text: widget.member.role);
  late final _resp =
      TextEditingController(text: widget.member.responsibilities ?? '');
  // Structured trigger rules (was a raw JSON text field — web TriggerEditor).
  late final List<Map<String, dynamic>> _triggerRules =
      _parseTriggers(widget.member.triggers);
  late final _handoff =
      TextEditingController(text: widget.member.handoffRules ?? '');
  late final _acceptance =
      TextEditingController(text: widget.member.acceptanceCriteria ?? '');
  late final _output =
      TextEditingController(text: widget.member.outputFormat ?? '');
  late final _sla = TextEditingController(text: widget.member.sla ?? '');
  late final _limits = TextEditingController(text: widget.member.limits ?? '');

  static const _triggerTypes = [
    'message_received',
    'on_mention',
    'task_assigned',
    'task_status_changed',
    'cron',
  ];
  static const _triggerLabels = {
    'message_received': '💬 Message received',
    'on_mention': '@ On mention',
    'task_assigned': '📋 Task assigned',
    'task_status_changed': '🔄 Task status changed',
    'cron': '⏰ Cron schedule',
  };

  static List<Map<String, dynamic>> _parseTriggers(String? raw) {
    if (raw == null || raw.trim().isEmpty) return [];
    try {
      final v = jsonDecode(raw);
      if (v is List) {
        return v
            .whereType<Map>()
            .where((m) => m['type'] is String)
            .map((m) => m.cast<String, dynamic>())
            .toList();
      }
    } catch (_) {}
    return [];
  }

  String? _serializeTriggers() {
    final clean = _triggerRules
        .map((r) => Map<String, dynamic>.fromEntries(r.entries.where(
            (e) => e.key == 'type' || '${e.value}'.trim().isNotEmpty)))
        .toList();
    return clean.isEmpty ? null : jsonEncode(clean);
  }

  @override
  void dispose() {
    _role.dispose();
    _resp.dispose();
    _handoff.dispose();
    _acceptance.dispose();
    _output.dispose();
    _sla.dispose();
    _limits.dispose();
    super.dispose();
  }

  String? _orNull(String s) => s.trim().isEmpty ? null : s.trim();

  /// Structured trigger-rules editor (replaces the raw JSON field) — one card
  /// per rule with a type dropdown and type-specific fields. Mirrors the web
  /// TriggerEditor. The rule maps are the source of truth; we serialize on save.
  Widget _buildTriggers() {
    final c = context.colors;
    Widget field(Map<String, dynamic> rule, String key, String label) =>
        Padding(
          padding: const EdgeInsets.only(top: AppTokens.s8),
          child: TextFormField(
            initialValue: '${rule[key] ?? ''}',
            decoration: InputDecoration(labelText: label, isDense: true),
            onChanged: (v) => rule[key] = v,
          ),
        );
    return Padding(
      padding: const EdgeInsets.only(top: AppTokens.s12),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(children: [
            Text(context.tr('Triggers'),
                style: TextStyle(
                    color: c.textMuted,
                    fontSize: 12,
                    fontWeight: FontWeight.w600)),
            const Spacer(),
            TextButton.icon(
              onPressed: () => setState(
                  () => _triggerRules.add({'type': 'message_received'})),
              icon: const Icon(Icons.add, size: 16),
              label: Text(context.tr('Add')),
            ),
          ]),
          if (_triggerRules.isEmpty)
            Text(context.tr('No triggers'),
                style: TextStyle(color: c.textMuted, fontSize: 12)),
          for (int i = 0; i < _triggerRules.length; i++)
            _triggerCard(c, field, i),
        ],
      ),
    );
  }

  Widget _triggerCard(
      dynamic c,
      Widget Function(Map<String, dynamic>, String, String) field,
      int i) {
    final rule = _triggerRules[i];
    final type =
        _triggerTypes.contains('${rule['type']}') ? '${rule['type']}' : 'message_received';
    return Container(
      key: ObjectKey(rule),
      margin: const EdgeInsets.only(top: AppTokens.s8),
      padding: const EdgeInsets.all(AppTokens.s8),
      decoration: BoxDecoration(
        border: Border.all(color: c.border),
        borderRadius: BorderRadius.circular(AppTokens.rMd),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(children: [
            Expanded(
              child: DropdownButtonFormField<String>(
                initialValue: type,
                isDense: true,
                decoration: const InputDecoration(isDense: true),
                items: [
                  for (final t in _triggerTypes)
                    DropdownMenuItem(
                        value: t, child: Text(context.tr(_triggerLabels[t]!))),
                ],
                onChanged: (v) => setState(
                    () => _triggerRules[i] = {'type': v ?? 'message_received'}),
              ),
            ),
            IconButton(
              tooltip: context.tr('Remove'),
              icon: const Icon(Icons.close, size: 16),
              onPressed: () => setState(() => _triggerRules.removeAt(i)),
            ),
          ]),
          if (type == 'message_received') ...[
            field(rule, 'from', context.tr('From (sender, optional)')),
            field(rule, 'messageType', context.tr('Message type (optional)')),
          ] else if (type == 'on_mention')
            field(rule, 'from', context.tr('From (sender, optional)'))
          else if (type == 'task_status_changed') ...[
            field(rule, 'status', context.tr('Status (optional)')),
            field(rule, 'assignee', context.tr('Assignee (optional)')),
            field(rule, 'to', context.tr('To status (optional)')),
          ] else if (type == 'cron')
            field(rule, 'cron', context.tr('Cron expression (e.g. 0 9 * * 1)')),
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    Widget area(String label, TextEditingController ctrl,
            {String? hint, int min = 2, int max = 4}) =>
        Padding(
          padding: const EdgeInsets.only(top: AppTokens.s8),
          child: TextField(
            controller: ctrl,
            minLines: min,
            maxLines: max,
            decoration: InputDecoration(labelText: label, hintText: hint),
          ),
        );
    return AlertDialog(
      backgroundColor: context.colors.surface,
      title: Text(context
          .trArgs('Edit {folder}', {'folder': widget.member.folder})),
      content: SizedBox(
        width: 480,
        height: 520,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              TextField(
                  controller: _role,
                  decoration:
                      InputDecoration(labelText: context.tr('Role'))),
              area(context.tr('Responsibilities'), _resp, min: 3, max: 5),
              _buildTriggers(),
              area(context.tr('Handoff rules'), _handoff),
              area(context.tr('Acceptance criteria'), _acceptance),
              area(context.tr('Output format'), _output),
              area(context.tr('SLA'), _sla, min: 1, max: 2),
              area(context.tr('Limits'), _limits, min: 1, max: 2),
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
          onPressed: () async {
            await ref.read(apiClientProvider).delete(
                '/api/cowork/teams/${widget.teamId}/members/${widget.member.folder}');
            ref.invalidate(teamsProvider);
            if (context.mounted) Navigator.of(context).pop();
          },
          child: Text(context.tr('Remove'),
              style: const TextStyle(color: AppTokens.danger)),
        ),
        TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: Text(context.tr('Cancel'))),
        FilledButton(
          onPressed: () async {
            await ref
                .read(apiClientProvider)
                .put('/api/cowork/teams/${widget.teamId}/members', body: {
              'folder': widget.member.folder,
              'role': _role.text.trim(),
              'responsibilities': _orNull(_resp.text),
              'triggers': _serializeTriggers(),
              'handoff_rules': _orNull(_handoff.text),
              'acceptance_criteria': _orNull(_acceptance.text),
              'output_format': _orNull(_output.text),
              'sla': _orNull(_sla.text),
              'limits': _orNull(_limits.text),
            });
            ref.invalidate(teamsProvider);
            if (context.mounted) Navigator.of(context).pop();
          },
          child: Text(context.tr('Save')),
        ),
      ],
    );
  }
}

class _Kanban extends StatelessWidget {
  const _Kanban({required this.teamId, required this.tasks});
  final String teamId;
  final List<CoworkTask> tasks;

  @override
  Widget build(BuildContext context) {
    return SingleChildScrollView(
      scrollDirection: Axis.horizontal,
      padding: const EdgeInsets.all(AppTokens.s16),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          for (final (key, label) in kCoworkColumns)
            _Column(
              teamId: teamId,
              label: context.tr(label),
              tasks: tasks.where((t) => t.status == key).toList(),
            ),
        ],
      ),
    );
  }
}

class _Column extends StatelessWidget {
  const _Column(
      {required this.teamId, required this.label, required this.tasks});
  final String teamId;
  final String label;
  final List<CoworkTask> tasks;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Container(
      width: 280,
      margin: const EdgeInsets.only(right: AppTokens.s12),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsets.symmetric(
                horizontal: AppTokens.s4, vertical: AppTokens.s8),
            child: Row(
              children: [
                Text(label.toUpperCase(),
                    style: TextStyle(
                      color: c.textMuted,
                      fontSize: 12,
                      fontWeight: FontWeight.w700,
                      letterSpacing: 0.8,
                    )),
                const SizedBox(width: AppTokens.s8),
                Text('${tasks.length}',
                    style: TextStyle(color: c.textMuted, fontSize: 12)),
              ],
            ),
          ),
          for (final t in tasks) _TaskCard(teamId: teamId, task: t),
        ],
      ),
    );
  }
}

class _TaskCard extends ConsumerWidget {
  const _TaskCard({required this.teamId, required this.task});
  final String teamId;
  final CoworkTask task;

  Color _priorityColor() => switch (task.priority) {
        'critical' => AppTokens.danger,
        'high' => AppTokens.warning,
        'low' => AppTokens.success,
        _ => AppTokens.brand,
      };

  Future<void> _patchStatus(WidgetRef ref, String status) async {
    await ref
        .read(apiClientProvider)
        .patch('/api/cowork/teams/$teamId/tasks/${task.id}',
            body: {'status': status});
    ref.invalidate(teamTasksProvider(teamId));
  }

  Future<void> _delete(WidgetRef ref) async {
    await ref
        .read(apiClientProvider)
        .delete('/api/cowork/teams/$teamId/tasks/${task.id}');
    ref.invalidate(teamTasksProvider(teamId));
  }

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final hasResult = task.resultOutput?.trim().isNotEmpty ?? false;
    return Padding(
      padding: const EdgeInsets.only(bottom: AppTokens.s8),
      child: Material(
        color: c.surface,
        borderRadius: BorderRadius.circular(AppTokens.rMd),
        child: InkWell(
          borderRadius: BorderRadius.circular(AppTokens.rMd),
          onTap: () => _showDetail(context, ref),
          child: Container(
            padding: const EdgeInsets.all(AppTokens.s12),
            decoration: BoxDecoration(
              border: Border.all(color: c.border),
              borderRadius: BorderRadius.circular(AppTokens.rMd),
            ),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
          Row(
            children: [
              Container(width: 6, height: 6,
                  decoration: BoxDecoration(
                      color: _priorityColor(), shape: BoxShape.circle)),
              const SizedBox(width: AppTokens.s6),
              Expanded(
                child: Text(task.title,
                    maxLines: 2,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                        color: c.textPrimary,
                        fontSize: 14,
                        fontWeight: FontWeight.w600)),
              ),
              // Quick actions: move to another column / delete.
              SizedBox(
                width: 24,
                height: 24,
                child: PopupMenuButton<String>(
                  tooltip: context.tr('Task actions'),
                  padding: EdgeInsets.zero,
                  iconSize: 16,
                  icon: Icon(Icons.more_vert, color: c.textMuted),
                  onSelected: (v) {
                    if (v == '__delete__') {
                      _delete(ref);
                    } else {
                      _patchStatus(ref, v);
                    }
                  },
                  itemBuilder: (_) => [
                    for (final (key, label) in kCoworkColumns)
                      if (key != task.status)
                        PopupMenuItem(
                          value: key,
                          child: Row(children: [
                            Icon(Icons.arrow_forward, size: 14, color: c.textMuted),
                            const SizedBox(width: AppTokens.s8),
                            Text(context.trArgs(
                                'Move to {label}', {'label': context.tr(label)})),
                          ]),
                        ),
                    const PopupMenuDivider(),
                    PopupMenuItem(
                      value: '__delete__',
                      child: Row(children: [
                        const Icon(Icons.delete_outline,
                            size: 14, color: AppTokens.danger),
                        const SizedBox(width: AppTokens.s8),
                        Text(context.tr('Delete'),
                            style: const TextStyle(color: AppTokens.danger)),
                      ]),
                    ),
                  ],
                ),
              ),
            ],
          ),
          if (task.assignee != null && task.assignee!.isNotEmpty) ...[
            const SizedBox(height: AppTokens.s8),
            Row(
              children: [
                Icon(Icons.person_outline, size: 12, color: c.textMuted),
                const SizedBox(width: AppTokens.s4),
                Text(task.assignee!,
                    style: TextStyle(color: c.textMuted, fontSize: 12)),
              ],
            ),
          ],
          if (hasResult) ...[
            const SizedBox(height: AppTokens.s8),
            Row(children: [
              const Icon(Icons.check_circle_outline,
                  size: 12, color: AppTokens.success),
              const SizedBox(width: AppTokens.s4),
              Text(context.tr('Result'),
                  style: const TextStyle(
                      color: AppTokens.success,
                      fontSize: 11,
                      fontWeight: FontWeight.w600)),
              const Spacer(),
              Text(
                  context.trArgs('{n} chars',
                      {'n': task.resultOutput!.trim().length}),
                  style: TextStyle(color: c.textMuted, fontSize: 11)),
            ]),
          ],
              ],
            ),
          ),
        ),
      ),
    );
  }

  /// Task detail sheet — description + result output rendered as markdown,
  /// with a copy button (web TaskResultCard).
  void _showDetail(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final result = task.resultOutput?.trim() ?? '';
    showDialog(
      context: context,
      builder: (dctx) => Dialog(
        backgroundColor: c.surface,
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 640, maxHeight: 640),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Padding(
                padding: const EdgeInsets.fromLTRB(AppTokens.s16,
                    AppTokens.s16, AppTokens.s8, AppTokens.s8),
                child: Row(children: [
                  Container(
                      width: 8,
                      height: 8,
                      decoration: BoxDecoration(
                          color: _priorityColor(), shape: BoxShape.circle)),
                  const SizedBox(width: AppTokens.s8),
                  Expanded(
                    child: Text(task.title,
                        style: TextStyle(
                            color: c.textPrimary,
                            fontSize: 16,
                            fontWeight: FontWeight.w700)),
                  ),
                  if (result.isNotEmpty)
                    IconButton(
                      tooltip: dctx.tr('Copy result'),
                      icon: const Icon(Icons.copy, size: 16),
                      onPressed: () {
                        Clipboard.setData(ClipboardData(text: result));
                        ScaffoldMessenger.of(dctx).showSnackBar(
                            SnackBar(content: Text(dctx.tr('Copied'))));
                      },
                    ),
                  IconButton(
                    icon: const Icon(Icons.close, size: 18),
                    onPressed: () => Navigator.of(dctx).pop(),
                  ),
                ]),
              ),
              const Divider(height: 1),
              Flexible(
                child: SingleChildScrollView(
                  padding: const EdgeInsets.all(AppTokens.s16),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Wrap(spacing: AppTokens.s8, runSpacing: AppTokens.s8, children: [
                        _chip(c, task.status),
                        _chip(c, task.priority),
                        if (task.assignee != null && task.assignee!.isNotEmpty)
                          _chip(c, '@${task.assignee}'),
                      ]),
                      if (task.description != null &&
                          task.description!.trim().isNotEmpty) ...[
                        const SizedBox(height: AppTokens.s12),
                        Text(dctx.tr('DESCRIPTION'),
                            style: TextStyle(
                                color: c.textMuted,
                                fontSize: 11,
                                fontWeight: FontWeight.w700,
                                letterSpacing: 0.5)),
                        const SizedBox(height: AppTokens.s4),
                        AppMarkdown(task.description!),
                      ],
                      const SizedBox(height: AppTokens.s12),
                      Text(dctx.tr('RESULT'),
                          style: TextStyle(
                              color: c.textMuted,
                              fontSize: 11,
                              fontWeight: FontWeight.w700,
                              letterSpacing: 0.5)),
                      const SizedBox(height: AppTokens.s4),
                      if (result.isEmpty)
                        Text(dctx.tr('No result yet'),
                            style:
                                TextStyle(color: c.textMuted, fontSize: 13))
                      else
                        AppMarkdown(result),
                    ],
                  ),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }

  Widget _chip(dynamic c, String text) => Container(
        padding: const EdgeInsets.symmetric(
            horizontal: AppTokens.s8, vertical: 3),
        decoration: BoxDecoration(
          color: c.sidebar,
          borderRadius: BorderRadius.circular(AppTokens.rFull),
          border: Border.all(color: c.border),
        ),
        child: Text(text,
            style: TextStyle(color: c.textSecondary, fontSize: 11)),
      );
}

/// A read-only file browser for a cowork team's workspace directory, backed by
/// GET /api/cowork/teams/:id/workspace?path= (new daemon route).
class _WorkspaceBrowser extends ConsumerStatefulWidget {
  const _WorkspaceBrowser({required this.teamId});
  final String teamId;
  @override
  ConsumerState<_WorkspaceBrowser> createState() => _WorkspaceBrowserState();
}

class _WorkspaceBrowserState extends ConsumerState<_WorkspaceBrowser> {
  String _rel = ''; // relative path under the workspace root
  bool _loading = true;
  String? _error;
  String _root = '';
  List<Map<String, dynamic>> _entries = const [];

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
      final r = await ref.read(apiClientProvider).get(
          '/api/cowork/teams/${widget.teamId}/workspace',
          query: {'path': _rel});
      final m = r is Map ? r : const {};
      _root = '${m['root'] ?? ''}';
      _entries = ((m['entries'] as List?) ?? const [])
          .whereType<Map>()
          .map((e) => e.cast<String, dynamic>())
          .toList()
        ..sort((a, b) {
          final ad = a['is_dir'] == true ? 0 : 1;
          final bd = b['is_dir'] == true ? 0 : 1;
          return ad != bd
              ? ad - bd
              : '${a['name']}'.compareTo('${b['name']}');
        });
    } catch (e) {
      _error = '$e';
    }
    if (mounted) setState(() => _loading = false);
  }

  void _enter(String name) {
    setState(() => _rel = _rel.isEmpty ? name : '$_rel/$name');
    _load();
  }

  void _up() {
    if (_rel.isEmpty) return;
    final i = _rel.lastIndexOf('/');
    setState(() => _rel = i < 0 ? '' : _rel.substring(0, i));
    _load();
  }

  String _fmtSize(num? n) {
    if (n == null) return '';
    if (n < 1024) return '${n}B';
    if (n < 1024 * 1024) return '${(n / 1024).toStringAsFixed(0)}K';
    return '${(n / 1024 / 1024).toStringAsFixed(1)}M';
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Dialog(
      backgroundColor: c.surface,
      child: SizedBox(
        width: 620,
        height: 520,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(
                  AppTokens.s16, AppTokens.s12, AppTokens.s8, AppTokens.s8),
              child: Row(
                children: [
                  Icon(Icons.folder_open_outlined, size: 18, color: c.accent),
                  const SizedBox(width: AppTokens.s8),
                  Expanded(
                    child: Text(
                        _rel.isEmpty
                            ? context.tr('Workspace')
                            : context.trArgs(
                                'Workspace / {path}', {'path': _rel}),
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: TextStyle(
                            color: c.textPrimary,
                            fontWeight: FontWeight.w700)),
                  ),
                  IconButton(
                    tooltip: context.tr('Reload'),
                    icon: const Icon(Icons.refresh, size: 16),
                    onPressed: _load,
                  ),
                  IconButton(
                    icon: const Icon(Icons.close, size: 18),
                    onPressed: () => Navigator.of(context).pop(),
                  ),
                ],
              ),
            ),
            if (_root.isNotEmpty)
              Padding(
                padding:
                    const EdgeInsets.symmetric(horizontal: AppTokens.s16),
                child: Text(_root,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(color: c.textMuted, fontSize: 11)),
              ),
            const Divider(height: AppTokens.s16),
            Expanded(
              child: _loading
                  ? const Center(child: CircularProgressIndicator())
                  : _error != null
                      ? Center(
                          child: Text(_error!,
                              style:
                                  const TextStyle(color: AppTokens.danger)))
                      : ListView(
                          padding: const EdgeInsets.symmetric(
                              horizontal: AppTokens.s8),
                          children: [
                            if (_rel.isNotEmpty)
                              ListTile(
                                dense: true,
                                leading: const Icon(Icons.arrow_upward,
                                    size: 18),
                                title: const Text('..'),
                                onTap: _up,
                              ),
                            for (final e in _entries)
                              ListTile(
                                dense: true,
                                leading: Icon(
                                    e['is_dir'] == true
                                        ? Icons.folder_outlined
                                        : Icons.insert_drive_file_outlined,
                                    size: 18,
                                    color: e['is_dir'] == true
                                        ? c.accent
                                        : c.textMuted),
                                title: Text('${e['name']}',
                                    maxLines: 1,
                                    overflow: TextOverflow.ellipsis),
                                trailing: e['is_dir'] == true
                                    ? const Icon(Icons.chevron_right, size: 16)
                                    : Text(_fmtSize(e['size'] as num?),
                                        style: TextStyle(
                                            color: c.textMuted,
                                            fontSize: 11)),
                                onTap: e['is_dir'] == true
                                    ? () => _enter('${e['name']}')
                                    : null,
                              ),
                            if (_entries.isEmpty)
                              Padding(
                                padding: const EdgeInsets.all(AppTokens.s24),
                                child: Center(
                                  child: Text(context.tr('Empty folder'),
                                      style: TextStyle(
                                          color: c.textMuted, fontSize: 12)),
                                ),
                              ),
                          ],
                        ),
            ),
          ],
        ),
      ),
    );
  }
}

/// "+ Add member" chip → prompts for a folder slug + role, PUTs a new member
/// (update_team_member upserts). 
class _AddMemberChip extends ConsumerWidget {
  const _AddMemberChip({required this.teamId});
  final String teamId;

  Future<void> _add(BuildContext context, WidgetRef ref) async {
    final folder = TextEditingController();
    final role = TextEditingController(text: 'member');
    final ok = await showDialog<bool>(
      context: context,
      builder: (dctx) => AlertDialog(
        backgroundColor: dctx.colors.surface,
        title: Text(dctx.tr('Add member')),
        content: SizedBox(
          width: 420,
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              TextField(
                controller: folder,
                autofocus: true,
                decoration: InputDecoration(
                    labelText: dctx.tr('Profile folder (slug)'),
                    hintText: 'web-scout',
                    border: const OutlineInputBorder()),
              ),
              const SizedBox(height: AppTokens.s12),
              TextField(
                controller: role,
                decoration: InputDecoration(
                    labelText: dctx.tr('Role'),
                    hintText: 'scout / reviewer / lead',
                    border: const OutlineInputBorder()),
              ),
            ],
          ),
        ),
        actions: [
          TextButton(
              onPressed: () => Navigator.of(dctx).pop(false),
              child: Text(dctx.tr('Cancel'))),
          FilledButton(
              onPressed: () => Navigator.of(dctx).pop(true),
              child: Text(dctx.tr('Add'))),
        ],
      ),
    );
    if (ok != true || folder.text.trim().isEmpty) return;
    await ref.read(apiClientProvider).put(
        '/api/cowork/teams/$teamId/members',
        body: {'folder': folder.text.trim(), 'role': role.text.trim()});
    ref.invalidate(teamsProvider);
  }

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    return InkWell(
      borderRadius: BorderRadius.circular(AppTokens.rLg),
      onTap: () => _add(context, ref),
      child: Container(
        padding: const EdgeInsets.symmetric(
            horizontal: AppTokens.s12, vertical: AppTokens.s8),
        decoration: BoxDecoration(
          borderRadius: BorderRadius.circular(AppTokens.rLg),
          border: Border.all(
              color: c.border, style: BorderStyle.solid),
        ),
        child: Row(mainAxisSize: MainAxisSize.min, children: [
          Icon(Icons.add, size: 15, color: c.textMuted),
          const SizedBox(width: 4),
          Text(context.tr('Add member'),
              style: TextStyle(color: c.textMuted, fontSize: 13)),
        ]),
      ),
    );
  }
}
