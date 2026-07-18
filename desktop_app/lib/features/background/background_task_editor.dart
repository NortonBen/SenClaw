import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../models/background_models.dart';
import '../../theme/tokens.dart';
import 'background_providers.dart';

/// Create or edit a user-owned background task.
///
/// App- and system-owned tasks never reach here — an app's config lives in its
/// manifest and a native job's body is Rust; both are pause-only.
void showBackgroundTaskEditor(BuildContext context, WidgetRef ref,
    {BackgroundTask? task}) {
  showDialog(
    context: context,
    builder: (_) => _Editor(task: task),
  );
}

/// Frequencies the editor offers, mapped to what the daemon takes. Anything more
/// exotic goes through "Advanced (cron)" rather than growing this list.
enum _Freq { hourly, daily, weekly, monthly, interval, cron, once, manual }

const _freqLabels = {
  _Freq.hourly: 'Hourly',
  _Freq.daily: 'Daily',
  _Freq.weekly: 'Weekly',
  _Freq.monthly: 'Monthly',
  _Freq.interval: 'Every N minutes',
  _Freq.cron: 'Advanced (cron)',
  _Freq.once: 'Once, at a time',
  _Freq.manual: 'Manual only',
};

class _Editor extends ConsumerStatefulWidget {
  const _Editor({this.task});
  final BackgroundTask? task;

  @override
  ConsumerState<_Editor> createState() => _EditorState();
}

class _EditorState extends ConsumerState<_Editor> {
  late final TextEditingController _title;
  late final TextEditingController _desc;
  late final TextEditingController _prompt;
  late final TextEditingController _cron;
  late final TextEditingController _contextUrl;
  late final TextEditingController _persona;
  late final TextEditingController _tools;
  late final TextEditingController _once;

  _Freq _freq = _Freq.daily;
  TimeOfDay _at = const TimeOfDay(hour: 9, minute: 0);
  int _weekday = 1;
  int _dayOfMonth = 1;
  int _intervalMin = 30;
  String _promptKind = 'static';
  String _continuity = 'fresh';
  String _overlap = 'skip';
  bool _catchUp = false;
  bool _notify = false;
  bool _saving = false;
  String? _error;

  bool get _isEdit => widget.task != null;

  @override
  void initState() {
    super.initState();
    final t = widget.task;
    _title = TextEditingController(text: t?.title ?? '');
    _desc = TextEditingController(text: t?.description ?? '');
    _prompt = TextEditingController(text: t?.prompt ?? '');
    _contextUrl = TextEditingController(text: t?.contextUrl ?? '');
    _persona = TextEditingController(text: t?.persona ?? '');
    _tools = TextEditingController(text: t?.useTools.join(', ') ?? '');
    _cron = TextEditingController(
        text: t?.triggerType == 'cron' ? (t?.triggerValue ?? '') : '');
    _once = TextEditingController(
        text: t?.triggerType == 'once' ? (t?.triggerValue ?? '') : '');
    if (t != null) {
      _promptKind = t.promptKind;
      _continuity = t.continuity;
      _overlap = t.overlapPolicy;
      _catchUp = t.catchUp;
      _notify = t.notify;
      _hydrateTrigger(t);
    }
  }

  /// Map an existing task's trigger back onto the editor's controls. A cron
  /// expression the editor can't express stays in Advanced rather than being
  /// silently rewritten into something else on save.
  void _hydrateTrigger(BackgroundTask t) {
    switch (t.triggerType) {
      case 'manual':
      case 'on_install':
        _freq = _Freq.manual;
        return;
      case 'once':
        _freq = _Freq.once;
        return;
      case 'interval':
        _freq = _Freq.interval;
        _intervalMin = ((int.tryParse(t.triggerValue ?? '') ?? 1800000) / 60000).round();
        return;
      case 'cron':
        final f = (t.triggerValue ?? '').trim().split(RegExp(r'\s+'));
        final p = f.length == 6 ? f.sublist(1) : f;
        if (p.length != 5) {
          _freq = _Freq.cron;
          return;
        }
        final [min, hour, dom, mon, dow] = p;
        final m = int.tryParse(min), h = int.tryParse(hour);
        if (m != null && h != null) _at = TimeOfDay(hour: h, minute: m);
        if (dom == '*' && mon == '*' && dow == '*' && h != null) {
          _freq = _Freq.daily;
        } else if (dom == '*' && mon == '*' && int.tryParse(dow) != null) {
          _freq = _Freq.weekly;
          _weekday = int.parse(dow);
        } else if (mon == '*' && dow == '*' && int.tryParse(dom) != null) {
          _freq = _Freq.monthly;
          _dayOfMonth = int.parse(dom);
        } else if (dom == '*' && mon == '*' && dow == '*' && hour == '*') {
          _freq = _Freq.hourly;
        } else {
          _freq = _Freq.cron;
        }
    }
  }

  @override
  void dispose() {
    for (final c in [_title, _desc, _prompt, _cron, _contextUrl, _persona, _tools, _once]) {
      c.dispose();
    }
    super.dispose();
  }

  /// The editor's controls → (trigger_type, trigger_value).
  (String, String?) _trigger() {
    final m = _at.minute, h = _at.hour;
    return switch (_freq) {
      _Freq.manual => ('manual', null),
      _Freq.once => ('once', _once.text.trim()),
      _Freq.interval => ('interval', '${_intervalMin * 60000}'),
      _Freq.cron => ('cron', _cron.text.trim()),
      _Freq.hourly => ('cron', '$m * * * *'),
      _Freq.daily => ('cron', '$m $h * * *'),
      _Freq.weekly => ('cron', '$m $h * * $_weekday'),
      _Freq.monthly => ('cron', '$m $h $_dayOfMonth * *'),
    };
  }

  List<String> get _toolList => _tools.text
      .split(',')
      .map((s) => s.trim())
      .where((s) => s.isNotEmpty)
      .toList();

  /// Tools that reach outside the machine. A task holding one of these runs
  /// unattended forever, so it is created paused and enabled from this screen —
  /// see `docs/background-tasks-design.md` §10 guard 3.
  bool get _isOutwardFacing =>
      _promptKind == 'generator' ||
      _toolList.any((t) {
        final n = t.toLowerCase();
        return n.contains('send') ||
            n.contains('browser') ||
            n.contains('post') ||
            n.contains('mail') ||
            n.contains('message') ||
            n.contains('crm_') ||
            n.contains('moltbook');
      });

  Future<void> _save() async {
    setState(() {
      _saving = true;
      _error = null;
    });
    final (type, value) = _trigger();
    final body = <String, dynamic>{
      'title': _title.text.trim(),
      'prompt': _prompt.text.trim(),
      'trigger_type': type,
      if (value != null && value.isNotEmpty) 'trigger_value': value,
      'description': _desc.text.trim(),
      'prompt_kind': _promptKind,
      if (_promptKind == 'template' && _contextUrl.text.trim().isNotEmpty)
        'context_url': _contextUrl.text.trim(),
      if (_persona.text.trim().isNotEmpty) 'persona': _persona.text.trim(),
      'tools': _toolList,
      'continuity': _continuity,
      'overlap_policy': _overlap,
      'catch_up': _catchUp,
      'notify': _notify,
    };
    try {
      final api = ref.read(backgroundApiProvider);
      if (_isEdit) {
        await api.update(widget.task!.id, body);
      } else {
        // Guard 3: an outward-facing task is authored here but enabled
        // deliberately, as a separate act.
        await api.create({...body, 'paused': _isOutwardFacing});
      }
      if (mounted) Navigator.pop(context);
    } catch (e) {
      setState(() {
        _saving = false;
        _error = '$e'.replaceFirst(RegExp(r'^\w+Exception\(\d+\): '), '');
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Dialog(
      backgroundColor: c.bg,
      child: SizedBox(
        width: 620,
        height: 660,
        child: Column(
          children: [
            Container(
              padding: const EdgeInsets.all(AppTokens.s12),
              decoration:
                  BoxDecoration(border: Border(bottom: BorderSide(color: c.border))),
              child: Row(
                children: [
                  Expanded(
                    child: Text(
                      _isEdit ? 'Edit background task' : 'New background task',
                      style: TextStyle(
                          color: c.textPrimary,
                          fontSize: 14,
                          fontWeight: FontWeight.w700),
                    ),
                  ),
                  IconButton(
                    icon: const Icon(Icons.close, size: 16),
                    onPressed: () => Navigator.pop(context),
                  ),
                ],
              ),
            ),
            Expanded(
              child: ListView(
                padding: const EdgeInsets.all(AppTokens.s16),
                children: [
                  _field('Title', _title, hint: 'Daily knowledge cleanup'),
                  _field('Description', _desc,
                      hint: 'Optional — what this is for', maxLines: 2),
                  const SizedBox(height: AppTokens.s8),
                  _label('Prompt'),
                  Text(
                    'Nobody is on the other end: the prompt must be self-contained, '
                    'say what "done" looks like, and say what to do when there is '
                    'nothing to do.',
                    style: TextStyle(color: c.textMuted, fontSize: 10, height: 1.4),
                  ),
                  const SizedBox(height: AppTokens.s4),
                  TextField(
                    controller: _prompt,
                    maxLines: 5,
                    style: TextStyle(color: c.textPrimary, fontSize: 12),
                    decoration: _dec('Review the knowledge base for contradictions…'),
                  ),
                  const SizedBox(height: AppTokens.s12),

                  _label('Prompt source'),
                  SegmentedButton<String>(
                    segments: const [
                      ButtonSegment(value: 'static', label: Text('Static')),
                      ButtonSegment(value: 'template', label: Text('Template')),
                      ButtonSegment(value: 'generator', label: Text('Generated')),
                    ],
                    selected: {_promptKind},
                    showSelectedIcon: false,
                    style: const ButtonStyle(
                      visualDensity: VisualDensity.compact,
                      textStyle: WidgetStatePropertyAll(TextStyle(fontSize: 11)),
                    ),
                    onSelectionChanged: (s) =>
                        setState(() => _promptKind = s.first),
                  ),
                  if (_promptKind == 'template') ...[
                    const SizedBox(height: AppTokens.s8),
                    _field('Context URL', _contextUrl,
                        hint: 'http://127.0.0.1:4390/api/bg/context/followup'),
                    Text(
                      'Fetched before each run; its JSON fills {{placeholders}}. '
                      'An empty response skips the run — so a task with nothing to '
                      'do costs no tokens.',
                      style:
                          TextStyle(color: c.textMuted, fontSize: 10, height: 1.4),
                    ),
                  ],
                  if (_promptKind == 'generator')
                    Padding(
                      padding: const EdgeInsets.only(top: AppTokens.s6),
                      child: Text(
                        'The prompt above is an instruction; the model writes the real '
                        'prompt from it each run. Doubles token cost and can invent its '
                        'own task — prefer Template when the data can be fetched.',
                        style: TextStyle(
                            color: AppTokens.warning, fontSize: 10, height: 1.4),
                      ),
                    ),
                  const SizedBox(height: AppTokens.s12),

                  _label('Runs'),
                  DropdownButtonFormField<_Freq>(
                    initialValue: _freq,
                    isDense: true,
                    style: TextStyle(color: c.textPrimary, fontSize: 12),
                    decoration: _dec(null),
                    items: _Freq.values
                        .map((f) => DropdownMenuItem(
                            value: f,
                            child: Text(_freqLabels[f]!,
                                style: const TextStyle(fontSize: 12))))
                        .toList(),
                    onChanged: (v) => setState(() => _freq = v ?? _Freq.daily),
                  ),
                  const SizedBox(height: AppTokens.s8),
                  ..._triggerControls(),
                  const SizedBox(height: AppTokens.s12),

                  _field('Persona', _persona, hint: 'Optional — e.g. sale-closer'),
                  _field('Tools', _tools,
                      hint: 'Comma-separated. Empty = the persona\'s own list'),
                  const SizedBox(height: AppTokens.s12),

                  _label('Memory across runs'),
                  SegmentedButton<String>(
                    segments: const [
                      ButtonSegment(value: 'fresh', label: Text('Fresh')),
                      ButtonSegment(value: 'thread', label: Text('Remembers')),
                    ],
                    selected: {_continuity},
                    showSelectedIcon: false,
                    style: const ButtonStyle(
                      visualDensity: VisualDensity.compact,
                      textStyle: WidgetStatePropertyAll(TextStyle(fontSize: 11)),
                    ),
                    onSelectionChanged: (s) => setState(() => _continuity = s.first),
                  ),
                  Padding(
                    padding: const EdgeInsets.only(top: AppTokens.s4),
                    child: Text(
                      _continuity == 'thread'
                          ? 'Recent run summaries are injected. Use this for anything '
                              'touching people — otherwise it contacts the same person twice.'
                          : 'Each run starts clean.',
                      style: TextStyle(color: c.textMuted, fontSize: 10, height: 1.4),
                    ),
                  ),
                  const SizedBox(height: AppTokens.s12),

                  _label('If the previous run is still going'),
                  SegmentedButton<String>(
                    segments: const [
                      ButtonSegment(value: 'skip', label: Text('Skip')),
                      ButtonSegment(value: 'queue', label: Text('Wait')),
                      ButtonSegment(
                          value: 'cancel_previous', label: Text('Cancel it')),
                    ],
                    selected: {_overlap},
                    showSelectedIcon: false,
                    style: const ButtonStyle(
                      visualDensity: VisualDensity.compact,
                      textStyle: WidgetStatePropertyAll(TextStyle(fontSize: 11)),
                    ),
                    onSelectionChanged: (s) => setState(() => _overlap = s.first),
                  ),
                  const SizedBox(height: AppTokens.s8),
                  CheckboxListTile(
                    value: _catchUp,
                    dense: true,
                    contentPadding: EdgeInsets.zero,
                    controlAffinity: ListTileControlAffinity.leading,
                    title: Text('Catch up after downtime',
                        style: TextStyle(color: c.textSecondary, fontSize: 12)),
                    subtitle: Text(
                      'Run once for a window missed while the daemon was off. '
                      'Off = the gap is dropped.',
                      style: TextStyle(color: c.textMuted, fontSize: 10),
                    ),
                    onChanged: (v) => setState(() => _catchUp = v ?? false),
                  ),
                  CheckboxListTile(
                    value: _notify,
                    dense: true,
                    contentPadding: EdgeInsets.zero,
                    controlAffinity: ListTileControlAffinity.leading,
                    title: Text('🔔 Chỉ thông báo',
                        style: TextStyle(color: c.textSecondary, fontSize: 12)),
                    subtitle: Text(
                      'Đẩy thông báo OS với nội dung ở ô Prompt, KHÔNG chạy agent. '
                      'Dùng cho nhắc/thông báo — nhanh, chắc chắn, không tốn token.',
                      style: TextStyle(color: c.textMuted, fontSize: 10),
                    ),
                    onChanged: (v) => setState(() => _notify = v ?? false),
                  ),

                  if (!_isEdit && _isOutwardFacing)
                    Container(
                      margin: const EdgeInsets.only(top: AppTokens.s8),
                      padding: const EdgeInsets.all(AppTokens.s8),
                      decoration: BoxDecoration(
                        color: AppTokens.warning.withValues(alpha: 0.1),
                        borderRadius: BorderRadius.circular(AppTokens.rSm),
                        border: Border.all(
                            color: AppTokens.warning.withValues(alpha: 0.4)),
                      ),
                      child: Row(
                        children: [
                          const Icon(Icons.shield_outlined,
                              size: 14, color: AppTokens.warning),
                          const SizedBox(width: AppTokens.s6),
                          Expanded(
                            child: Text(
                              'This task can act outside this machine, so it will be '
                              'created paused. Review it and press play to start it.',
                              style: TextStyle(
                                  color: c.textSecondary, fontSize: 10, height: 1.4),
                            ),
                          ),
                        ],
                      ),
                    ),

                  if (_error != null)
                    Padding(
                      padding: const EdgeInsets.only(top: AppTokens.s12),
                      child: Text(_error!,
                          style: const TextStyle(
                              color: AppTokens.danger, fontSize: 11)),
                    ),
                ],
              ),
            ),
            Container(
              padding: const EdgeInsets.all(AppTokens.s12),
              decoration:
                  BoxDecoration(border: Border(top: BorderSide(color: c.border))),
              child: Row(
                mainAxisAlignment: MainAxisAlignment.end,
                children: [
                  TextButton(
                      onPressed: () => Navigator.pop(context),
                      child: const Text('Cancel')),
                  const SizedBox(width: AppTokens.s8),
                  FilledButton(
                    onPressed: _saving || _title.text.trim().isEmpty ? null : _save,
                    child: Text(_saving
                        ? 'Saving…'
                        : _isEdit
                            ? 'Save'
                            : _isOutwardFacing
                                ? 'Create (paused)'
                                : 'Create'),
                  ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }

  List<Widget> _triggerControls() {
    final c = context.colors;
    switch (_freq) {
      case _Freq.manual:
        return [
          Text('Only runs when you press "Run now".',
              style: TextStyle(color: c.textMuted, fontSize: 10)),
        ];
      case _Freq.cron:
        return [
          _field('Cron expression', _cron, hint: '0 9 * * *'),
          Text('5-field form, evaluated in your local timezone.',
              style: TextStyle(color: c.textMuted, fontSize: 10)),
        ];
      case _Freq.once:
        return [
          _field('When (RFC3339)', _once, hint: '2026-12-25T09:00:00+07:00'),
        ];
      case _Freq.interval:
        return [
          Row(
            children: [
              Text('Every', style: TextStyle(color: c.textSecondary, fontSize: 12)),
              const SizedBox(width: AppTokens.s8),
              SizedBox(
                width: 80,
                child: TextFormField(
                  initialValue: '$_intervalMin',
                  keyboardType: TextInputType.number,
                  style: TextStyle(color: c.textPrimary, fontSize: 12),
                  decoration: _dec(null),
                  onChanged: (v) =>
                      _intervalMin = (int.tryParse(v) ?? 30).clamp(1, 100000),
                ),
              ),
              const SizedBox(width: AppTokens.s8),
              Text('minutes',
                  style: TextStyle(color: c.textSecondary, fontSize: 12)),
            ],
          ),
        ];
      case _Freq.hourly:
        return [_minutePicker()];
      case _Freq.daily:
        return [_timePicker()];
      case _Freq.weekly:
        return [
          Row(children: [
            Expanded(child: _weekdayPicker()),
            const SizedBox(width: AppTokens.s8),
            Expanded(child: _timePicker()),
          ]),
        ];
      case _Freq.monthly:
        return [
          Row(children: [
            Expanded(child: _domPicker()),
            const SizedBox(width: AppTokens.s8),
            Expanded(child: _timePicker()),
          ]),
        ];
    }
  }

  Widget _timePicker() {
    final c = context.colors;
    return OutlinedButton.icon(
      icon: const Icon(Icons.schedule, size: 14),
      label: Text(
        'At ${_at.hour.toString().padLeft(2, '0')}:${_at.minute.toString().padLeft(2, '0')}',
        style: TextStyle(color: c.textPrimary, fontSize: 12),
      ),
      onPressed: () async {
        final t = await showTimePicker(context: context, initialTime: _at);
        if (t != null) setState(() => _at = t);
      },
    );
  }

  Widget _minutePicker() {
    final c = context.colors;
    return Row(children: [
      Text('At minute', style: TextStyle(color: c.textSecondary, fontSize: 12)),
      const SizedBox(width: AppTokens.s8),
      SizedBox(
        width: 70,
        child: TextFormField(
          initialValue: '${_at.minute}',
          keyboardType: TextInputType.number,
          style: TextStyle(color: c.textPrimary, fontSize: 12),
          decoration: _dec(null),
          onChanged: (v) => _at =
              TimeOfDay(hour: 0, minute: (int.tryParse(v) ?? 0).clamp(0, 59)),
        ),
      ),
    ]);
  }

  Widget _weekdayPicker() {
    const names = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'];
    return DropdownButtonFormField<int>(
      initialValue: _weekday,
      isDense: true,
      decoration: _dec(null),
      items: List.generate(
        7,
        (i) => DropdownMenuItem(
            value: i, child: Text(names[i], style: const TextStyle(fontSize: 12))),
      ),
      onChanged: (v) => setState(() => _weekday = v ?? 1),
    );
  }

  Widget _domPicker() {
    return DropdownButtonFormField<int>(
      initialValue: _dayOfMonth,
      isDense: true,
      decoration: _dec(null),
      items: List.generate(
        28,
        (i) => DropdownMenuItem(
            value: i + 1, child: Text('Day ${i + 1}', style: const TextStyle(fontSize: 12))),
      ),
      // Capped at 28 on purpose: day 29–31 silently skips short months.
      onChanged: (v) => setState(() => _dayOfMonth = v ?? 1),
    );
  }

  Widget _label(String s) => Padding(
        padding: const EdgeInsets.only(bottom: AppTokens.s4),
        child: Text(s,
            style: TextStyle(
                color: context.colors.textMuted,
                fontSize: 11,
                fontWeight: FontWeight.w600)),
      );

  Widget _field(String label, TextEditingController ctrl,
      {String? hint, int maxLines = 1}) {
    return Padding(
      padding: const EdgeInsets.only(bottom: AppTokens.s8),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          _label(label),
          TextField(
            controller: ctrl,
            maxLines: maxLines,
            style: TextStyle(color: context.colors.textPrimary, fontSize: 12),
            decoration: _dec(hint),
            onChanged: (_) => setState(() {}),
          ),
        ],
      ),
    );
  }

  InputDecoration _dec(String? hint) => InputDecoration(
        hintText: hint,
        hintStyle: TextStyle(color: context.colors.textMuted, fontSize: 11),
        isDense: true,
        contentPadding: const EdgeInsets.symmetric(
            horizontal: AppTokens.s8, vertical: AppTokens.s8),
        border: const OutlineInputBorder(),
      );
}
