import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../models/kanban_models.dart';
import '../../theme/tokens.dart';
import 'kanban_providers.dart';

/// Plugins → Kanban: manage the reusable column templates used when creating
/// boards. Builtins are read-only; custom templates can be created, edited,
/// imported (paste exported JSON) and exported (copy to clipboard).
class KanbanTemplatesPanel extends ConsumerWidget {
  const KanbanTemplatesPanel({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final templates = ref.watch(kanbanTemplatesProvider);
    return Column(children: [
      Padding(
        padding: const EdgeInsets.fromLTRB(
            AppTokens.s24, AppTokens.s16, AppTokens.s24, AppTokens.s12),
        child: Row(children: [
          Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
            Text('Kanban templates',
                style: TextStyle(
                    color: c.textPrimary,
                    fontSize: 16,
                    fontWeight: FontWeight.w700)),
            Text('Reusable column workflows for new boards',
                style: TextStyle(color: c.textMuted, fontSize: 12)),
          ]),
          const Spacer(),
          IconButton(
            tooltip: 'Reload',
            icon: const Icon(Icons.refresh, size: 18),
            onPressed: () => ref.invalidate(kanbanTemplatesProvider),
          ),
          const SizedBox(width: AppTokens.s8),
          OutlinedButton.icon(
            onPressed: () => _showImportDialog(context, ref),
            icon: const Icon(Icons.file_download_outlined, size: 16),
            label: const Text('Import'),
          ),
          const SizedBox(width: AppTokens.s8),
          FilledButton.icon(
            onPressed: () => _showEditor(context, ref, null),
            icon: const Icon(Icons.add, size: 16),
            label: const Text('New template'),
          ),
        ]),
      ),
      Divider(height: 1, color: c.border),
      Expanded(
        child: templates.when(
          loading: () => const Center(child: CircularProgressIndicator()),
          error: (e, _) => Center(child: Text('$e')),
          data: (list) => ListView(
            padding: const EdgeInsets.all(AppTokens.s24),
            children: [
              for (final t in list)
                _TemplateCard(template: t),
            ],
          ),
        ),
      ),
    ]);
  }
}

class _TemplateCard extends ConsumerWidget {
  const _TemplateCard({required this.template});
  final KanbanTemplate template;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    return Container(
      margin: const EdgeInsets.only(bottom: AppTokens.s12),
      padding: const EdgeInsets.all(AppTokens.s16),
      decoration: BoxDecoration(
        color: c.surface,
        border: Border.all(color: c.border),
        borderRadius: BorderRadius.circular(AppTokens.rLg),
      ),
      child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
        Row(children: [
          Text(template.name,
              style: TextStyle(
                  color: c.textPrimary,
                  fontSize: 14,
                  fontWeight: FontWeight.w600)),
          const SizedBox(width: AppTokens.s8),
          if (template.builtin)
            _pill(context, 'builtin', c.textMuted)
          else
            _pill(context, 'custom', AppTokens.brand),
          const Spacer(),
          IconButton(
            tooltip: 'Export (copy JSON)',
            iconSize: 16,
            onPressed: () => _export(context, template),
            icon: const Icon(Icons.file_upload_outlined),
          ),
          if (!template.builtin) ...[
            IconButton(
              tooltip: 'Edit',
              iconSize: 16,
              onPressed: () => _showEditor(context, ref, template),
              icon: const Icon(Icons.edit_outlined),
            ),
            IconButton(
              tooltip: 'Delete',
              iconSize: 16,
              onPressed: () => _confirmDelete(context, ref, template),
              icon: const Icon(Icons.delete_outline),
            ),
          ] else
            IconButton(
              tooltip: 'Duplicate as custom',
              iconSize: 16,
              onPressed: () => _showEditor(context, ref, template, clone: true),
              icon: const Icon(Icons.copy_outlined),
            ),
        ]),
        if (template.description.isNotEmpty)
          Padding(
            padding: const EdgeInsets.only(top: 2),
            child: Text(template.description,
                style: TextStyle(color: c.textMuted, fontSize: 12)),
          ),
        const SizedBox(height: AppTokens.s12),
        Wrap(
          spacing: AppTokens.s8,
          runSpacing: AppTokens.s8,
          children: [
            for (final col in template.columns) _ColumnPill(column: col),
          ],
        ),
      ]),
    );
  }
}

class _ColumnPill extends StatelessWidget {
  const _ColumnPill({required this.column});
  final KanbanTemplateColumn column;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final color = _parseColor(column.color) ?? c.textMuted;
    return Container(
      padding:
          const EdgeInsets.symmetric(horizontal: AppTokens.s12, vertical: 6),
      decoration: BoxDecoration(
        color: color.withValues(alpha: 0.12),
        borderRadius: BorderRadius.circular(AppTokens.rMd),
        border: Border.all(color: color.withValues(alpha: 0.4)),
      ),
      child: Row(mainAxisSize: MainAxisSize.min, children: [
        Container(
            width: 8,
            height: 8,
            decoration: BoxDecoration(color: color, shape: BoxShape.circle)),
        const SizedBox(width: 6),
        Text(column.title,
            style: TextStyle(
                color: c.textPrimary,
                fontSize: 12,
                fontWeight: FontWeight.w500)),
        if (column.role != 'custom') ...[
          const SizedBox(width: 6),
          Text(column.role,
              style: TextStyle(color: c.textMuted, fontSize: 10)),
        ],
        if (column.wipLimit != null) ...[
          const SizedBox(width: 6),
          Text('WIP ${column.wipLimit}',
              style: TextStyle(color: c.textMuted, fontSize: 10)),
        ],
      ]),
    );
  }
}

Widget _pill(BuildContext context, String text, Color color) => Container(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
      decoration: BoxDecoration(
        color: color.withValues(alpha: 0.14),
        borderRadius: BorderRadius.circular(999),
      ),
      child: Text(text,
          style: TextStyle(
              color: color, fontSize: 10, fontWeight: FontWeight.w600)),
    );

Color? _parseColor(String? hex) {
  if (hex == null) return null;
  var h = hex.replaceAll('#', '').trim();
  if (h.length == 6) h = 'FF$h';
  final v = int.tryParse(h, radix: 16);
  return v == null ? null : Color(v);
}

// ── Export / import ─────────────────────────────────────────────────────────

void _export(BuildContext context, KanbanTemplate t) {
  final json = const JsonEncoder.withIndent('  ').convert(t.toJson());
  Clipboard.setData(ClipboardData(text: json));
  ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(content: Text('Copied "${t.name}" template JSON to clipboard')));
}

Future<void> _showImportDialog(BuildContext context, WidgetRef ref) async {
  final messenger = ScaffoldMessenger.of(context);
  final ctl = TextEditingController();
  final ok = await showDialog<bool>(
    context: context,
    builder: (dctx) => AlertDialog(
      backgroundColor: dctx.colors.surface,
      title: const Text('Import template'),
      content: SizedBox(
        width: 480,
        child: TextField(
          controller: ctl,
          autofocus: true,
          maxLines: 12,
          style: const TextStyle(fontFamily: 'monospace', fontSize: 12),
          decoration: const InputDecoration(
              hintText: 'Paste exported template JSON…',
              border: OutlineInputBorder()),
        ),
      ),
      actions: [
        TextButton(
            onPressed: () => Navigator.pop(dctx, false),
            child: const Text('Cancel')),
        FilledButton(
            onPressed: () => Navigator.pop(dctx, true),
            child: const Text('Import')),
      ],
    ),
  );
  if (ok != true) return;
  try {
    final j = json.decode(ctl.text) as Map<String, dynamic>;
    final t = KanbanTemplate.fromJson(j);
    if (t.name.trim().isEmpty || t.columns.isEmpty) {
      throw 'Template needs a name and at least one column';
    }
    await ref
        .read(kanbanApiProvider)
        .saveTemplate(t.name, t.description, t.columns);
    messenger.showSnackBar(SnackBar(content: Text('Imported "${t.name}"')));
  } catch (e) {
    messenger.showSnackBar(SnackBar(content: Text('Import failed: $e')));
  }
}

Future<void> _confirmDelete(
    BuildContext context, WidgetRef ref, KanbanTemplate t) async {
  final ok = await showDialog<bool>(
    context: context,
    builder: (dctx) => AlertDialog(
      backgroundColor: dctx.colors.surface,
      title: Text('Delete "${t.name}"?'),
      content: const Text('This custom template will be removed. Boards already '
          'created from it are not affected.'),
      actions: [
        TextButton(
            onPressed: () => Navigator.pop(dctx, false),
            child: const Text('Cancel')),
        FilledButton(
            style: FilledButton.styleFrom(backgroundColor: AppTokens.danger),
            onPressed: () => Navigator.pop(dctx, true),
            child: const Text('Delete')),
      ],
    ),
  );
  if (ok == true) {
    await ref.read(kanbanApiProvider).deleteTemplate(t.id);
  }
}

// ── Editor ──────────────────────────────────────────────────────────────────

const _roles = [
  'triage',
  'todo',
  'ready',
  'in_progress',
  'blocked',
  'done',
  'custom',
];

class _EditableColumn {
  _EditableColumn(this.title, this.role, this.color, this.wip);
  String title;
  String role;
  String? color;
  int? wip;
}

Future<void> _showEditor(
    BuildContext context, WidgetRef ref, KanbanTemplate? existing,
    {bool clone = false}) async {
  await showDialog<void>(
    context: context,
    builder: (_) => _TemplateEditorDialog(existing: existing, clone: clone),
  );
}

class _TemplateEditorDialog extends ConsumerStatefulWidget {
  const _TemplateEditorDialog({this.existing, this.clone = false});
  final KanbanTemplate? existing;
  final bool clone;

  @override
  ConsumerState<_TemplateEditorDialog> createState() =>
      _TemplateEditorDialogState();
}

class _TemplateEditorDialogState extends ConsumerState<_TemplateEditorDialog> {
  late final TextEditingController _name;
  late final TextEditingController _desc;
  late final List<_EditableColumn> _cols;

  @override
  void initState() {
    super.initState();
    final e = widget.existing;
    _name = TextEditingController(
        text: e == null ? '' : (widget.clone ? '${e.name} copy' : e.name));
    _desc = TextEditingController(text: e?.description ?? '');
    _cols = e == null
        ? [
            _EditableColumn('To Do', 'todo', '#64748b', null),
            _EditableColumn('In Progress', 'in_progress', '#3b82f6', null),
            _EditableColumn('Done', 'done', '#22c55e', null),
          ]
        : [
            for (final col in e.columns)
              _EditableColumn(col.title, col.role, col.color, col.wipLimit),
          ];
  }

  @override
  void dispose() {
    _name.dispose();
    _desc.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    final messenger = ScaffoldMessenger.of(context);
    final columns = [
      for (final c in _cols)
        if (c.title.trim().isNotEmpty)
          KanbanTemplateColumn(c.title.trim(), c.role, c.color, c.wip),
    ];
    if (_name.text.trim().isEmpty || columns.isEmpty) {
      messenger.showSnackBar(const SnackBar(
          content: Text('A name and at least one column are required')));
      return;
    }
    try {
      await ref
          .read(kanbanApiProvider)
          .saveTemplate(_name.text.trim(), _desc.text.trim(), columns);
      if (mounted) Navigator.pop(context);
    } catch (e) {
      messenger.showSnackBar(SnackBar(content: Text('Save failed: $e')));
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final title = widget.existing == null || widget.clone
        ? 'New template'
        : 'Edit template';
    return AlertDialog(
      backgroundColor: c.surface,
      title: Text(title),
      content: SizedBox(
        width: 560,
        child: SingleChildScrollView(
          child: Column(mainAxisSize: MainAxisSize.min, children: [
            TextField(
              controller: _name,
              decoration: const InputDecoration(
                  labelText: 'Name', hintText: 'e.g. Marketing sprint'),
            ),
            const SizedBox(height: AppTokens.s8),
            TextField(
              controller: _desc,
              decoration: const InputDecoration(labelText: 'Description'),
            ),
            const SizedBox(height: AppTokens.s16),
            Align(
              alignment: Alignment.centerLeft,
              child: Text('COLUMNS',
                  style: TextStyle(
                      color: c.textMuted,
                      fontSize: 10,
                      fontWeight: FontWeight.w700,
                      letterSpacing: 0.6)),
            ),
            const SizedBox(height: AppTokens.s8),
            for (int i = 0; i < _cols.length; i++) _columnRow(context, i),
            const SizedBox(height: AppTokens.s8),
            Align(
              alignment: Alignment.centerLeft,
              child: TextButton.icon(
                onPressed: () => setState(() => _cols.add(
                    _EditableColumn('New column', 'custom', '#94a3b8', null))),
                icon: const Icon(Icons.add, size: 16),
                label: const Text('Add column'),
              ),
            ),
          ]),
        ),
      ),
      actions: [
        TextButton(
            onPressed: () => Navigator.pop(context),
            child: const Text('Cancel')),
        FilledButton(onPressed: _save, child: const Text('Save')),
      ],
    );
  }

  Widget _columnRow(BuildContext context, int i) {
    final col = _cols[i];
    final color = _parseColor(col.color) ?? context.colors.textMuted;
    return Padding(
      padding: const EdgeInsets.only(bottom: AppTokens.s8),
      child: Row(children: [
        Container(
            width: 12,
            height: 12,
            decoration: BoxDecoration(color: color, shape: BoxShape.circle)),
        const SizedBox(width: AppTokens.s8),
        Expanded(
          flex: 3,
          child: TextField(
            controller: TextEditingController(text: col.title)
              ..selection =
                  TextSelection.collapsed(offset: col.title.length),
            decoration: const InputDecoration(isDense: true, hintText: 'Title'),
            onChanged: (v) => col.title = v,
          ),
        ),
        const SizedBox(width: AppTokens.s8),
        Expanded(
          flex: 2,
          child: DropdownButton<String>(
            value: _roles.contains(col.role) ? col.role : 'custom',
            isExpanded: true,
            items: [
              for (final r in _roles)
                DropdownMenuItem(value: r, child: Text(r)),
            ],
            onChanged: (v) => setState(() => col.role = v ?? 'custom'),
          ),
        ),
        const SizedBox(width: AppTokens.s8),
        SizedBox(
          width: 56,
          child: TextField(
            controller: TextEditingController(text: col.wip?.toString() ?? '')
              ..selection = TextSelection.collapsed(
                  offset: (col.wip?.toString() ?? '').length),
            keyboardType: TextInputType.number,
            decoration: const InputDecoration(isDense: true, hintText: 'WIP'),
            onChanged: (v) => col.wip = int.tryParse(v.trim()),
          ),
        ),
        IconButton(
          tooltip: 'Remove',
          iconSize: 16,
          onPressed: _cols.length <= 1
              ? null
              : () => setState(() => _cols.removeAt(i)),
          icon: const Icon(Icons.close),
        ),
      ]),
    );
  }
}
