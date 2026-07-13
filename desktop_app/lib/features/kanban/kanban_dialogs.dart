import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../theme/tokens.dart';
import 'kanban_providers.dart';

/// A folder field with a "Browse…" button. Returns the picked path via [ctl].
class WorkspaceField extends StatefulWidget {
  const WorkspaceField({super.key, required this.controller, this.label = 'Workspace folder (optional)'});
  final TextEditingController controller;
  final String label;

  @override
  State<WorkspaceField> createState() => _WorkspaceFieldState();
}

class _WorkspaceFieldState extends State<WorkspaceField> {
  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
      Text(widget.label, style: TextStyle(color: c.textMuted, fontSize: 11)),
      const SizedBox(height: 2),
      Row(children: [
        Expanded(
          child: TextField(
            controller: widget.controller,
            decoration: const InputDecoration(
                isDense: true, hintText: '~/work/… (worker outputs land here)'),
          ),
        ),
        const SizedBox(width: AppTokens.s8),
        OutlinedButton.icon(
          onPressed: () async {
            final dir = await FilePicker.platform.getDirectoryPath(
                dialogTitle: 'Choose the board workspace folder');
            if (dir != null) setState(() => widget.controller.text = dir);
          },
          icon: const Icon(Icons.folder_open, size: 16),
          label: const Text('Browse…'),
        ),
      ]),
    ]);
  }
}

/// Template picker dropdown. `value` may be a template id, or the special
/// `'__ai__'` sentinel (AI-generated columns) when [allowAi] is true.
const aiTemplateSentinel = '__ai__';

class TemplateDropdown extends ConsumerWidget {
  const TemplateDropdown(
      {super.key,
      required this.value,
      required this.onChanged,
      this.allowAi = false});
  final String value;
  final void Function(String) onChanged;
  final bool allowAi;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final templates = ref.watch(kanbanTemplatesProvider);
    return Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
      Text('Columns template', style: TextStyle(color: c.textMuted, fontSize: 11)),
      templates.when(
        loading: () => const LinearProgressIndicator(),
        error: (e, _) => Text('$e', style: const TextStyle(fontSize: 11)),
        data: (list) {
          final items = <DropdownMenuItem<String>>[
            if (allowAi)
              const DropdownMenuItem(
                  value: aiTemplateSentinel, child: Text('✨ AI generates columns')),
            for (final t in list)
              DropdownMenuItem(
                  value: t.id,
                  child: Text('${t.name}${t.builtin ? '' : ' (custom)'}')),
          ];
          final safe = items.any((i) => i.value == value)
              ? value
              : (items.isNotEmpty ? items.first.value! : '');
          return DropdownButton<String>(
            value: safe,
            isExpanded: true,
            underline: Divider(color: c.border, height: 1),
            items: items,
            onChanged: (v) {
              if (v != null) onChanged(v);
            },
          );
        },
      ),
    ]);
  }
}

/// New-board dialog: title + columns template + workspace folder.
Future<void> showNewBoardDialog(BuildContext context, WidgetRef ref) async {
  final titleCtl = TextEditingController();
  final wsCtl = TextEditingController();
  String template = 'standard';
  final created = await showDialog<int>(
    context: context,
    builder: (dctx) => StatefulBuilder(
      builder: (dctx, setSt) => AlertDialog(
        backgroundColor: dctx.colors.surface,
        title: const Text('New board'),
        content: SizedBox(
          width: 460,
          child: Column(mainAxisSize: MainAxisSize.min, children: [
            TextField(
              controller: titleCtl,
              autofocus: true,
              decoration: const InputDecoration(
                  labelText: 'Title', hintText: 'e.g. Q3 product launch'),
            ),
            const SizedBox(height: AppTokens.s12),
            TemplateDropdown(
                value: template, onChanged: (v) => setSt(() => template = v)),
            const SizedBox(height: AppTokens.s12),
            WorkspaceField(controller: wsCtl),
          ]),
        ),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(dctx), child: const Text('Cancel')),
          FilledButton(
            onPressed: () async {
              if (titleCtl.text.trim().isEmpty) return;
              final id = await ref.read(kanbanApiProvider).createBoard(
                    titleCtl.text.trim(),
                    templateId: template,
                    workspaceDir: wsCtl.text.trim(),
                  );
              if (dctx.mounted) Navigator.pop(dctx, id);
            },
            child: const Text('Create'),
          ),
        ],
      ),
    ),
  );
  if (created != null) ref.read(openBoardProvider.notifier).state = created;
}

/// AI-board dialog: goal + template (incl. "AI generates columns") + workspace.
Future<void> showGenerateBoardDialog(BuildContext context, WidgetRef ref) async {
  final goalCtl = TextEditingController();
  final wsCtl = TextEditingController();
  String template = aiTemplateSentinel;
  final ok = await showDialog<bool>(
    context: context,
    builder: (dctx) => StatefulBuilder(
      builder: (dctx, setSt) => AlertDialog(
        backgroundColor: dctx.colors.surface,
        title: const Text('✨ AI board from a goal'),
        content: SizedBox(
          width: 480,
          child: Column(mainAxisSize: MainAxisSize.min, children: [
            TextField(
              controller: goalCtl,
              autofocus: true,
              maxLines: 3,
              decoration: const InputDecoration(
                  labelText: 'Goal',
                  hintText: 'e.g. Plan a customer workshop in 6 weeks'),
            ),
            const SizedBox(height: AppTokens.s12),
            TemplateDropdown(
                value: template,
                allowAi: true,
                onChanged: (v) => setSt(() => template = v)),
            const SizedBox(height: AppTokens.s12),
            WorkspaceField(controller: wsCtl),
          ]),
        ),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(dctx, false),
              child: const Text('Cancel')),
          FilledButton(
              onPressed: () => Navigator.pop(dctx, true),
              child: const Text('Generate')),
        ],
      ),
    ),
  );
  if (ok == true && goalCtl.text.trim().isNotEmpty && context.mounted) {
    final messenger = ScaffoldMessenger.of(context);
    messenger.showSnackBar(
        const SnackBar(content: Text('Generating board with AI…')));
    try {
      final id = await ref.read(kanbanApiProvider).generateBoard(
            goalCtl.text.trim(),
            templateId: template == aiTemplateSentinel ? null : template,
            workspaceDir: wsCtl.text.trim(),
          );
      if (id != null) ref.read(openBoardProvider.notifier).state = id;
    } catch (e) {
      messenger.showSnackBar(SnackBar(content: Text('AI failed: $e')));
    }
  }
}
