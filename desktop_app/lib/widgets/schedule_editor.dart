import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../core/i18n/l10n.dart';
import '../models/space_models.dart';
import '../theme/tokens.dart';
import '../features/space/space_providers.dart';
import '../features/chat/agents_provider.dart';
import '../features/chat/new_chat_dialog.dart' show llmConfigsProvider;

/// Shared schedule create/edit dialog used from both the Space Schedules tab
/// and the session-level schedule info popup.
///
/// When [existing] is non-null the dialog edits that schedule (PATCH); when
/// null it creates a new one (POST). When [showStatus] is true an extra
/// status dropdown (active/paused/cancelled) is shown — useful for the
/// session-level popup.
class ScheduleEditorDialog extends ConsumerStatefulWidget {
  const ScheduleEditorDialog({super.key, this.existing, this.showStatus = false});
  final SpaceSchedule? existing;
  final bool showStatus;

  @override
  ConsumerState<ScheduleEditorDialog> createState() =>
      _ScheduleEditorDialogState();
}

class _ScheduleEditorDialogState extends ConsumerState<ScheduleEditorDialog> {
  late final _prompt = TextEditingController(
      text: widget.existing?.prompt.isNotEmpty == true
          ? widget.existing!.prompt
          : (widget.existing?.label ?? ''));
  late final _time = TextEditingController(text: _initTime());
  late final _cron = TextEditingController(text: _initCronRaw());
  late String _freq = _initFreq();
  late int _weekday = _initWeekday();
  late int _dom = _initDom();
  late DateTime? _onceDate = _initOnceDate();
  late String _mode = widget.existing?.agentMode.isNotEmpty == true
      ? widget.existing!.agentMode
      : 'agent';
  late String _status = widget.existing?.status.isNotEmpty == true
      ? widget.existing!.status
      : 'active';
  late String? _agentFolder = widget.existing?.agentFolder;
  // Seed from the schedule too, not null: an uninitialised field made the Model
  // dropdown read "Active default" on every edit regardless of the real value.
  late String? _modelId = widget.existing?.modelId;

  @override
  void initState() {
    super.initState();
    // Force a fresh agents fetch when the editor opens. `agentsProvider` is a
    // global StateNotifier that only (re)fetches on socket-connect transitions,
    // so if it was first created while disconnected — or the editor is the very
    // first screen to need it — the Profile dropdown would sit empty (only
    // "Default", nothing else to pick). Re-requesting here guarantees the list
    // fills in; the dropdown rebuilds when the `agents` event lands.
    ref.read(agentsProvider.notifier).refresh();
  }

  static const _freqs = [
    'daily',
    'weekdays',
    'weekly',
    'monthly',
    'once',
    'once_delete',
    'advanced',
  ];
  static const _freqLabels = {
    'daily': 'Daily',
    'weekdays': 'Weekdays',
    'weekly': 'Weekly',
    'monthly': 'Monthly',
    'once': 'Once',
    'once_delete': 'Once (auto-delete)',
    'advanced': 'Advanced (cron)',
  };
  static const _weekdays = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];

  String _initFreq() {
    // One-shot schedules carry an ISO datetime (not a cron) in scheduleValue;
    // trust the explicit schedule_type instead of parsing.
    final type = widget.existing?.scheduleType ?? '';
    if (type == 'once' || type == 'once_delete') return type;
    final cron = widget.existing?.scheduleValue ?? '';
    final parts = cron.trim().split(RegExp(r'\s+'));
    if (parts.length != 5) return cron.isNotEmpty ? 'advanced' : 'daily';
    final dom = parts[2], dow = parts[4];
    if (dom == '*' && dow == '*') return 'daily';
    if (dom == '*' && dow == '1-5') return 'weekdays';
    if (dom == '*' && RegExp(r'^\d$').hasMatch(dow)) return 'weekly';
    if (RegExp(r'^\d+$').hasMatch(dom) && dow == '*') return 'monthly';
    return 'advanced';
  }

  String _initTime() {
    final type = widget.existing?.scheduleType ?? '';
    if (type == 'once' || type == 'once_delete') {
      final dt = DateTime.tryParse(widget.existing?.scheduleValue ?? '');
      if (dt != null) {
        final l = dt.toLocal();
        return '${l.hour.toString().padLeft(2, '0')}:${l.minute.toString().padLeft(2, '0')}';
      }
      return '09:00';
    }
    final cron = widget.existing?.scheduleValue ?? '';
    final parts = cron.trim().split(RegExp(r'\s+'));
    if (parts.length == 5) {
      final m = int.tryParse(parts[0]);
      final h = int.tryParse(parts[1]);
      if (m != null && h != null) {
        return '${h.toString().padLeft(2, '0')}:${m.toString().padLeft(2, '0')}';
      }
    }
    return '09:00';
  }

  int _initWeekday() {
    final cron = widget.existing?.scheduleValue ?? '';
    final parts = cron.trim().split(RegExp(r'\s+'));
    if (parts.length == 5 && RegExp(r'^\d$').hasMatch(parts[4])) {
      final dow = int.tryParse(parts[4]) ?? 0;
      return dow == 0 ? 7 : dow;
    }
    return 1;
  }

  int _initDom() {
    final cron = widget.existing?.scheduleValue ?? '';
    final parts = cron.trim().split(RegExp(r'\s+'));
    if (parts.length == 5 && RegExp(r'^\d+$').hasMatch(parts[2])) {
      return int.tryParse(parts[2]) ?? 1;
    }
    return 1;
  }

  DateTime? _initOnceDate() {
    final type = widget.existing?.scheduleType ?? '';
    if (type == 'once' || type == 'once_delete') {
      final dt = DateTime.tryParse(widget.existing?.scheduleValue ?? '');
      if (dt != null) {
        final l = dt.toLocal();
        return DateTime(l.year, l.month, l.day);
      }
    }
    return null;
  }

  String _fmtDate(DateTime d) =>
      '${d.year.toString().padLeft(4, '0')}-${d.month.toString().padLeft(2, '0')}-${d.day.toString().padLeft(2, '0')}';

  Future<void> _pickDate() async {
    final now = DateTime.now();
    final picked = await showDatePicker(
      context: context,
      initialDate: _onceDate ?? now,
      firstDate: DateTime(now.year, now.month, now.day),
      lastDate: now.add(const Duration(days: 3650)),
    );
    if (picked != null) setState(() => _onceDate = picked);
  }

  String _initCronRaw() {
    final type = widget.existing?.scheduleType ?? '';
    if (type == 'once' || type == 'once_delete') return '';
    final cron = widget.existing?.scheduleValue ?? '';
    final parts = cron.trim().split(RegExp(r'\s+'));
    if (parts.length != 5) return cron;
    final dom = parts[2], dow = parts[4];
    if (dom == '*' && dow == '*') return '';
    if (dom == '*' && dow == '1-5') return '';
    if (dom == '*' && RegExp(r'^\d$').hasMatch(dow)) return '';
    if (RegExp(r'^\d+$').hasMatch(dom) && dow == '*') return '';
    return cron;
  }

  @override
  void dispose() {
    _prompt.dispose();
    _time.dispose();
    _cron.dispose();
    super.dispose();
  }

  /// Items for the Profile dropdown.
  ///
  /// Always carries an explicit Default (a profile could otherwise be picked
  /// but never un-picked), and always carries the schedule's own saved profile
  /// even when it isn't in [agents] — the agent list arrives over the WS and
  /// may be empty on first build, and Flutter asserts unless the selected
  /// value has exactly one matching item once the list is non-empty. Keeping it
  /// also means a schedule bound to a since-deleted profile shows what it is
  /// bound to instead of quietly reading "Default".
  List<DropdownMenuItem<String>> _profileItems(Iterable<AgentInfo> agents) {
    final list = agents.toList();
    return [
      DropdownMenuItem(value: null, child: Text(context.tr('Default'))),
      for (final a in list)
        DropdownMenuItem(value: a.folder, child: Text(a.name)),
      if (_agentFolder != null &&
          _agentFolder!.isNotEmpty &&
          !list.any((a) => a.folder == _agentFolder))
        DropdownMenuItem(
            value: _agentFolder,
            child: Text(_agentFolder!, overflow: TextOverflow.ellipsis)),
    ];
  }

  Future<void> _save() async {
    final body = <String, dynamic>{
      'prompt': _prompt.text.trim(),
      'label': _prompt.text.trim().split('\n').first,
      'agent_mode': _mode,
      if (widget.showStatus) 'status': _status,
      // Always send both, empty string for "Default". Omitting them on null
      // meant the server saw "no change", so a profile or model could be set
      // but never removed. The editor always knows the intended value, so
      // there is no ambiguity to preserve here.
      'agent_folder': _agentFolder ?? '',
      'model_id': _modelId ?? '',
    };
    if (_freq == 'advanced') {
      body['cron_advanced'] = _cron.text.trim();
    } else {
      body['frequency'] = _freq;
      body['time_local'] = _time.text.trim();
      if (_freq == 'weekly') body['weekday'] = _weekday;
      if (_freq == 'monthly') body['day_of_month'] = _dom;
      if ((_freq == 'once' || _freq == 'once_delete') && _onceDate != null) {
        body['date_local'] = _fmtDate(_onceDate!);
      }
    }
    final api = ref.read(spaceApiProvider);
    try {
      if (widget.existing != null) {
        await api.updateSchedule(widget.existing!.id, body);
      } else {
        await api.createSchedule(body);
      }
    } catch (e) {
      // Surface the failure instead of silently leaving the dialog open.
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
              content:
                  Text(context.trArgs('Failed to save schedule: {e}', {'e': e}))),
        );
      }
      return;
    }
    if (mounted) Navigator.of(context).pop();
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return AlertDialog(
      backgroundColor: c.surface,
      shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(AppTokens.rXl)),
      title: Row(children: [
        Icon(Icons.event_repeat_outlined, size: 20, color: c.accent),
        const SizedBox(width: AppTokens.s8),
        Text(widget.existing == null
            ? context.tr('New schedule')
            : context.tr('Edit schedule')),
      ]),
      content: SizedBox(
        width: 460,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              TextField(
                controller: _prompt,
                minLines: 3,
                maxLines: 8,
                onChanged: (_) => setState(() {}),
                style: const TextStyle(fontSize: 14, height: 1.4),
                decoration: InputDecoration(
                  labelText: context.tr('Prompt'),
                  alignLabelWithHint: true,
                  hintText: context.tr('Describe the task to run on schedule…'),
                  filled: true,
                  fillColor: c.surfaceAlt,
                  contentPadding: const EdgeInsets.all(AppTokens.s12),
                  enabledBorder: OutlineInputBorder(
                    borderRadius: BorderRadius.circular(AppTokens.rMd),
                    borderSide: BorderSide(color: c.border),
                  ),
                  focusedBorder: OutlineInputBorder(
                    borderRadius: BorderRadius.circular(AppTokens.rMd),
                    borderSide: BorderSide(color: c.accent, width: 1.5),
                  ),
                ),
              ),
              const SizedBox(height: AppTokens.s12),
              Row(
                children: [
                  Expanded(
                    child: DropdownButtonFormField<String>(
                      initialValue: _freq,
                      isExpanded: true,
                      decoration: InputDecoration(
                          labelText: context.tr('Frequency'),
                          border: const OutlineInputBorder()),
                      items: [
                        for (final f in _freqs)
                          DropdownMenuItem(
                              value: f,
                              child: Text(context.tr(_freqLabels[f] ?? f))),
                      ],
                      onChanged: (v) => setState(() => _freq = v ?? 'daily'),
                    ),
                  ),
                  const SizedBox(width: AppTokens.s8),
                  if (_freq != 'advanced')
                    SizedBox(
                      width: 110,
                      child: TextField(
                        controller: _time,
                        decoration: InputDecoration(
                            labelText: context.tr('Time'),
                            hintText: 'HH:MM',
                            border: const OutlineInputBorder()),
                      ),
                    ),
                ],
              ),
              if (_freq == 'once' || _freq == 'once_delete') ...[
                const SizedBox(height: AppTokens.s12),
                InkWell(
                  onTap: _pickDate,
                  borderRadius: BorderRadius.circular(AppTokens.rMd),
                  child: InputDecorator(
                    decoration: InputDecoration(
                      labelText: context.tr('Run date'),
                      border: const OutlineInputBorder(),
                      suffixIcon:
                          const Icon(Icons.calendar_today_outlined, size: 18),
                    ),
                    child: Text(
                      _onceDate != null
                          ? _fmtDate(_onceDate!)
                          : context
                              .tr('Next occurrence of the time (today / tomorrow)'),
                      style: TextStyle(
                        color: _onceDate != null ? c.textPrimary : c.textMuted,
                      ),
                    ),
                  ),
                ),
                if (_onceDate != null)
                  Align(
                    alignment: Alignment.centerRight,
                    child: TextButton.icon(
                      onPressed: () => setState(() => _onceDate = null),
                      icon: const Icon(Icons.clear, size: 16),
                      label: Text(context.tr('Clear date')),
                    ),
                  ),
              ],
              if (_freq == 'weekly') ...[
                const SizedBox(height: AppTokens.s12),
                DropdownButtonFormField<int>(
                  initialValue: _weekday,
                  decoration: InputDecoration(
                      labelText: context.tr('Weekday'),
                      border: const OutlineInputBorder()),
                  items: [
                    for (var i = 0; i < 7; i++)
                      DropdownMenuItem(
                          value: i + 1, child: Text(context.tr(_weekdays[i]))),
                  ],
                  onChanged: (v) => setState(() => _weekday = v ?? 1),
                ),
              ],
              if (_freq == 'monthly') ...[
                const SizedBox(height: AppTokens.s12),
                DropdownButtonFormField<int>(
                  initialValue: _dom,
                  decoration: InputDecoration(
                      labelText: context.tr('Day of month'),
                      border: const OutlineInputBorder()),
                  items: [
                    for (var d = 1; d <= 28; d++)
                      DropdownMenuItem(value: d, child: Text('$d')),
                  ],
                  onChanged: (v) => setState(() => _dom = v ?? 1),
                ),
              ],
              if (_freq == 'advanced') ...[
                const SizedBox(height: AppTokens.s12),
                TextField(
                  controller: _cron,
                  decoration: InputDecoration(
                      labelText: context.tr('Cron expression'),
                      hintText: '0 9 * * *',
                      border: const OutlineInputBorder()),
                  style: const TextStyle(fontFamily: AppTokens.fontMono),
                ),
              ],
              const SizedBox(height: AppTokens.s12),
              Row(
                children: [
                  Expanded(
                    child: DropdownButtonFormField<String>(
                      initialValue: _mode,
                      isExpanded: true,
                      decoration: InputDecoration(
                          labelText: context.tr('Agent mode'),
                          border: const OutlineInputBorder()),
                      items: [
                        DropdownMenuItem(
                            value: 'agent', child: Text(context.tr('Agent'))),
                        DropdownMenuItem(
                            value: 'plan', child: Text(context.tr('Plan'))),
                      ],
                      onChanged: (v) =>
                          setState(() => _mode = v ?? 'agent'),
                    ),
                  ),
                  const SizedBox(width: AppTokens.s8),
                  Expanded(
                    child: DropdownButtonFormField<String>(
                      initialValue: _agentFolder,
                      isExpanded: true,
                      decoration: InputDecoration(
                          labelText: context.tr('Profile (agent)'),
                          border: const OutlineInputBorder()),
                      hint: Text(context.tr('Default')),
                      items: _profileItems(
                          ref.watch(agentsProvider).where((a) => !a.isSchedule)),
                      onChanged: (v) =>
                          setState(() => _agentFolder = v),
                    ),
                  ),
                ],
              ),
              const SizedBox(height: AppTokens.s12),
              ref.watch(llmConfigsProvider).maybeWhen(
                    data: (d) => DropdownButtonFormField<String?>(
                      initialValue: _modelId,
                      isExpanded: true,
                      decoration: InputDecoration(
                          labelText: context.tr('Model'),
                          border: const OutlineInputBorder()),
                      items: [
                        DropdownMenuItem(
                            value: null,
                            child: Text(context.tr('Active default'))),
                        for (final m in d.configs)
                          DropdownMenuItem(
                              value: m.id,
                              child: Text(m.label,
                                  overflow: TextOverflow.ellipsis)),
                      ],
                      onChanged: (v) =>
                          setState(() => _modelId = v),
                    ),
                    orElse: () => const SizedBox.shrink(),
                  ),
              if (widget.showStatus && widget.existing != null) ...[
                const SizedBox(height: AppTokens.s12),
                DropdownButtonFormField<String>(
                  initialValue:
                      ['active', 'paused', 'cancelled'].contains(_status)
                          ? _status
                          : 'active',
                  decoration: InputDecoration(
                      labelText: context.tr('Status'),
                      border: const OutlineInputBorder()),
                  items: [
                    DropdownMenuItem(
                        value: 'active', child: Text(context.tr('Active'))),
                    DropdownMenuItem(
                        value: 'paused', child: Text(context.tr('Paused'))),
                    DropdownMenuItem(
                        value: 'cancelled',
                        child: Text(context.tr('Cancelled'))),
                  ],
                  onChanged: (v) =>
                      setState(() => _status = v ?? _status),
                ),
              ],
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: Text(context.tr('Cancel'))),
        FilledButton(
          onPressed: _prompt.text.trim().isEmpty ? null : _save,
          child: Text(widget.existing == null
              ? context.tr('Create')
              : context.tr('Save')),
        ),
      ],
    );
  }
}
