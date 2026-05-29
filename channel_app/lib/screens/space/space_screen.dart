import 'package:flutter/material.dart';
import '../../models/space_models.dart';
import '../../services/config_service.dart';
import '../../services/relay_manager.dart';
import '../../services/space_api.dart';
import '../../theme/app_colors.dart';
import '../../widgets/states.dart';

/// Space: Notes + Calendar over `/api/space/*` via the relay tunnel.
/// (Schedules & Email are slated for a later migration step.)
class SpaceScreen extends StatefulWidget {
  const SpaceScreen({super.key});

  @override
  State<SpaceScreen> createState() => _SpaceScreenState();
}

class _SpaceScreenState extends State<SpaceScreen>
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
        title: Row(
          children: [
            const Text('Space', style: TextStyle(color: Colors.white)),
            const SizedBox(width: 8),
            AnimatedBuilder(
              animation: RelayManager(),
              builder: (_, _) =>
                  ConnectionDot(connected: RelayManager().connected),
            ),
          ],
        ),
        bottom: TabBar(
          controller: _tabs,
          isScrollable: true,
          tabAlignment: TabAlignment.start,
          indicatorColor: AppColors.accent,
          labelColor: AppColors.accent,
          unselectedLabelColor: Colors.white54,
          tabs: const [
            Tab(icon: Icon(Icons.sticky_note_2_outlined), text: 'Notes'),
            Tab(icon: Icon(Icons.event_note_outlined), text: 'Calendar'),
            Tab(icon: Icon(Icons.schedule), text: 'Schedules'),
            Tab(icon: Icon(Icons.email_outlined), text: 'Email'),
          ],
        ),
      ),
      body: Container(
        decoration: AppColors.pageDecoration,
        child: TabBarView(
          controller: _tabs,
          children: const [
            _NotesTab(),
            _CalendarTab(),
            _SchedulesTab(),
            _EmailTab(),
          ],
        ),
      ),
    );
  }
}

// ─── Notes ───────────────────────────────────────────────────────────────────

class _NotesTab extends StatefulWidget {
  const _NotesTab();

  @override
  State<_NotesTab> createState() => _NotesTabState();
}

class _NotesTabState extends State<_NotesTab>
    with AutomaticKeepAliveClientMixin {
  final _api = SpaceApi();
  List<SpaceNote> _notes = [];
  bool _loading = true;
  String? _error;
  String _query = '';

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
      final notes = _query.trim().isEmpty
          ? await _api.listNotes()
          : await _api.searchNotes(_query.trim());
      if (!mounted) return;
      setState(() {
        _notes = notes;
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

  Future<void> _edit({SpaceNote? note}) async {
    final saved = await showModalBottomSheet<bool>(
      context: context,
      isScrollControlled: true,
      backgroundColor: AppColors.surface,
      shape: const RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(top: Radius.circular(18)),
      ),
      builder: (_) => _NoteEditor(note: note),
    );
    if (saved == true) _load();
  }

  Future<void> _delete(SpaceNote note) async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: AppColors.surface,
        title: const Text('Xoá ghi chú?',
            style: TextStyle(color: Colors.white)),
        content: Text(note.title,
            style: const TextStyle(color: Colors.white70)),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(ctx, false),
              child: const Text('Huỷ')),
          TextButton(
              onPressed: () => Navigator.pop(ctx, true),
              child: const Text('Xoá',
                  style: TextStyle(color: Colors.redAccent))),
        ],
      ),
    );
    if (ok != true) return;
    try {
      await _api.deleteNote(note.id);
      _load();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Lỗi xoá: $e')));
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    return Scaffold(
      backgroundColor: Colors.transparent,
      floatingActionButton: FloatingActionButton(
        onPressed: () => _edit(),
        backgroundColor: AppColors.accent,
        foregroundColor: Colors.black,
        child: const Icon(Icons.add),
      ),
      body: Column(
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(12, 12, 12, 4),
            child: TextField(
              style: const TextStyle(color: Colors.white, fontSize: 14),
              onSubmitted: (v) {
                _query = v;
                _load();
              },
              decoration: InputDecoration(
                hintText: 'Tìm ghi chú…',
                hintStyle: const TextStyle(color: Colors.white38),
                prefixIcon:
                    const Icon(Icons.search, color: Colors.white38, size: 20),
                isDense: true,
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
          ),
          Expanded(child: _buildList()),
        ],
      ),
    );
  }

  Widget _buildList() {
    if (_loading) return const LoadingState(text: 'Đang tải ghi chú…');
    if (_error != null) return ErrorState(message: _error!, onRetry: _load);
    if (_notes.isEmpty) {
      return const EmptyState(
        icon: Icons.sticky_note_2_outlined,
        message: 'Chưa có ghi chú',
        hint: 'Nhấn + để tạo ghi chú mới',
      );
    }
    return RefreshIndicator(
      onRefresh: _load,
      color: AppColors.accent,
      backgroundColor: AppColors.surface,
      child: ListView.builder(
        padding: const EdgeInsets.fromLTRB(12, 4, 12, 88),
        itemCount: _notes.length,
        itemBuilder: (ctx, i) => _noteCard(_notes[i]),
      ),
    );
  }

  Widget _noteCard(SpaceNote n) {
    return Card(
      color: Colors.white.withValues(alpha: 0.04),
      margin: const EdgeInsets.only(bottom: 10),
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(14),
        side: const BorderSide(color: AppColors.cardBorder),
      ),
      child: ListTile(
        contentPadding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
        title: Row(
          children: [
            if (n.pinned)
              const Padding(
                padding: EdgeInsets.only(right: 6),
                child: Icon(Icons.push_pin, color: AppColors.accent, size: 14),
              ),
            Expanded(
              child: Text(
                n.title.isEmpty ? '(không tiêu đề)' : n.title,
                style: const TextStyle(
                    color: Colors.white, fontWeight: FontWeight.w600),
              ),
            ),
          ],
        ),
        subtitle: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            if (n.body.isNotEmpty) ...[
              const SizedBox(height: 4),
              Text(
                n.body,
                style: const TextStyle(color: Colors.white54, fontSize: 12),
                maxLines: 2,
                overflow: TextOverflow.ellipsis,
              ),
            ],
            if (n.tags.isNotEmpty) ...[
              const SizedBox(height: 6),
              Wrap(
                spacing: 6,
                runSpacing: 4,
                children: n.tags
                    .map((t) => Container(
                          padding: const EdgeInsets.symmetric(
                              horizontal: 7, vertical: 2),
                          decoration: BoxDecoration(
                            color: AppColors.cyan.withValues(alpha: 0.12),
                            borderRadius: BorderRadius.circular(6),
                          ),
                          child: Text('#$t',
                              style: const TextStyle(
                                  color: AppColors.cyan, fontSize: 10)),
                        ))
                    .toList(),
              ),
            ],
          ],
        ),
        onTap: () => _edit(note: n),
        trailing: IconButton(
          icon: const Icon(Icons.delete_outline,
              color: Colors.white38, size: 20),
          onPressed: () => _delete(n),
        ),
      ),
    );
  }
}

class _NoteEditor extends StatefulWidget {
  final SpaceNote? note;
  const _NoteEditor({this.note});

  @override
  State<_NoteEditor> createState() => _NoteEditorState();
}

class _NoteEditorState extends State<_NoteEditor> {
  final _api = SpaceApi();
  late final _titleCtrl = TextEditingController(text: widget.note?.title ?? '');
  late final _bodyCtrl = TextEditingController(text: widget.note?.body ?? '');
  late final _tagsCtrl =
      TextEditingController(text: widget.note?.tags.join(', ') ?? '');
  bool _saving = false;
  String? _error;

  @override
  void dispose() {
    _titleCtrl.dispose();
    _bodyCtrl.dispose();
    _tagsCtrl.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    final title = _titleCtrl.text.trim();
    if (title.isEmpty) {
      setState(() => _error = 'Cần tiêu đề');
      return;
    }
    final tags = _tagsCtrl.text
        .split(',')
        .map((s) => s.trim())
        .where((s) => s.isNotEmpty)
        .toList();
    setState(() {
      _saving = true;
      _error = null;
    });
    try {
      if (widget.note == null) {
        await _api.createNote(title: title, body: _bodyCtrl.text, tags: tags);
      } else {
        await _api.updateNote(widget.note!.id,
            title: title, body: _bodyCtrl.text, tags: tags);
      }
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
    return Padding(
      padding: EdgeInsets.fromLTRB(
          20, 20, 20, MediaQuery.of(context).viewInsets.bottom + 20),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(widget.note == null ? 'Ghi chú mới' : 'Sửa ghi chú',
              style: const TextStyle(
                  color: Colors.white,
                  fontSize: 18,
                  fontWeight: FontWeight.bold)),
          const SizedBox(height: 16),
          _field(_titleCtrl, 'Tiêu đề'),
          const SizedBox(height: 10),
          _field(_bodyCtrl, 'Nội dung', maxLines: 6),
          const SizedBox(height: 10),
          _field(_tagsCtrl, 'Tags (phân tách bằng dấu phẩy)'),
          if (_error != null) ...[
            const SizedBox(height: 8),
            Text(_error!,
                style: const TextStyle(color: Colors.redAccent, fontSize: 12)),
          ],
          const SizedBox(height: 16),
          SizedBox(
            width: double.infinity,
            child: ElevatedButton(
              onPressed: _saving ? null : _save,
              style: ElevatedButton.styleFrom(
                backgroundColor: AppColors.accent,
                foregroundColor: Colors.black,
                padding: const EdgeInsets.symmetric(vertical: 14),
              ),
              child: _saving
                  ? const SizedBox(
                      width: 18,
                      height: 18,
                      child: CircularProgressIndicator(
                          strokeWidth: 2, color: Colors.black))
                  : const Text('Lưu'),
            ),
          ),
        ],
      ),
    );
  }

  Widget _field(TextEditingController c, String hint, {int maxLines = 1}) {
    return TextField(
      controller: c,
      maxLines: maxLines,
      style: const TextStyle(color: Colors.white),
      decoration: InputDecoration(
        hintText: hint,
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
    );
  }
}

// ─── Calendar ──────────────────────────────────────────────────────────────

class _CalendarTab extends StatefulWidget {
  const _CalendarTab();

  @override
  State<_CalendarTab> createState() => _CalendarTabState();
}

class _CalendarTabState extends State<_CalendarTab>
    with AutomaticKeepAliveClientMixin {
  final _api = SpaceApi();
  List<SpaceEvent> _events = [];
  bool _loading = true;
  String? _error;

  @override
  bool get wantKeepAlive => true;

  @override
  void initState() {
    super.initState();
    _load();
  }

  ({int from, int to}) _window() {
    final now = DateTime.now();
    final from = now.subtract(const Duration(days: 7));
    final to = now.add(const Duration(days: 60));
    return (
      from: from.millisecondsSinceEpoch,
      to: to.millisecondsSinceEpoch,
    );
  }

  Future<void> _load() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final w = _window();
      final events = await _api.listEvents(from: w.from, to: w.to);
      if (!mounted) return;
      setState(() {
        _events = events;
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
    final saved = await showModalBottomSheet<bool>(
      context: context,
      isScrollControlled: true,
      backgroundColor: AppColors.surface,
      shape: const RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(top: Radius.circular(18)),
      ),
      builder: (_) => const _EventEditor(),
    );
    if (saved == true) _load();
  }

  Future<void> _delete(SpaceEvent e) async {
    try {
      await _api.deleteEvent(e.id);
      _load();
    } catch (err) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Lỗi xoá: $err')));
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
    if (_loading) return const LoadingState(text: 'Đang tải sự kiện…');
    if (_error != null) return ErrorState(message: _error!, onRetry: _load);
    if (_events.isEmpty) {
      return const EmptyState(
        icon: Icons.event_busy,
        message: 'Không có sự kiện',
        hint: 'Nhấn + để thêm sự kiện',
      );
    }
    // Group by day.
    final byDay = <String, List<SpaceEvent>>{};
    for (final e in _events) {
      final d = e.start;
      final key = '${d.year}-${d.month.toString().padLeft(2, '0')}-${d.day.toString().padLeft(2, '0')}';
      byDay.putIfAbsent(key, () => []).add(e);
    }
    final days = byDay.keys.toList()..sort();
    return RefreshIndicator(
      onRefresh: _load,
      color: AppColors.accent,
      backgroundColor: AppColors.surface,
      child: ListView(
        padding: const EdgeInsets.fromLTRB(12, 12, 12, 88),
        children: [
          for (final day in days) ...[
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 8),
              child: Text(
                _dayLabel(byDay[day]!.first.start),
                style: const TextStyle(
                    color: Colors.white70,
                    fontSize: 13,
                    fontWeight: FontWeight.w600),
              ),
            ),
            ...byDay[day]!.map(_eventCard),
          ],
        ],
      ),
    );
  }

  String _dayLabel(DateTime d) {
    final now = DateTime.now();
    final today = DateTime(now.year, now.month, now.day);
    final that = DateTime(d.year, d.month, d.day);
    final diff = that.difference(today).inDays;
    final base =
        '${d.day.toString().padLeft(2, '0')}/${d.month.toString().padLeft(2, '0')}/${d.year}';
    if (diff == 0) return 'Hôm nay · $base';
    if (diff == 1) return 'Ngày mai · $base';
    if (diff == -1) return 'Hôm qua · $base';
    return base;
  }

  Widget _eventCard(SpaceEvent e) {
    final color = _parseColor(e.color) ?? AppColors.accent;
    final timeStr = e.allDay
        ? 'Cả ngày'
        : '${_hm(e.start)} – ${_hm(e.end)}';
    return Card(
      color: Colors.white.withValues(alpha: 0.04),
      margin: const EdgeInsets.only(bottom: 8),
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(12),
        side: const BorderSide(color: AppColors.cardBorder),
      ),
      child: ListTile(
        leading: Container(width: 4, height: 40, color: color),
        title: Text(e.title,
            style: const TextStyle(
                color: Colors.white, fontWeight: FontWeight.w600)),
        subtitle: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            const SizedBox(height: 2),
            Text(timeStr,
                style: const TextStyle(color: Colors.white54, fontSize: 12)),
            if (e.location != null && e.location!.isNotEmpty)
              Text('📍 ${e.location}',
                  style: const TextStyle(color: Colors.white38, fontSize: 11)),
          ],
        ),
        trailing: IconButton(
          icon: const Icon(Icons.delete_outline,
              color: Colors.white38, size: 20),
          onPressed: () => _delete(e),
        ),
      ),
    );
  }

  String _hm(DateTime d) =>
      '${d.hour.toString().padLeft(2, '0')}:${d.minute.toString().padLeft(2, '0')}';

  Color? _parseColor(String? hex) {
    if (hex == null || hex.isEmpty) return null;
    var h = hex.replaceFirst('#', '');
    if (h.length == 6) h = 'FF$h';
    final v = int.tryParse(h, radix: 16);
    return v == null ? null : Color(v);
  }
}

class _EventEditor extends StatefulWidget {
  const _EventEditor();

  @override
  State<_EventEditor> createState() => _EventEditorState();
}

class _EventEditorState extends State<_EventEditor> {
  final _api = SpaceApi();
  final _titleCtrl = TextEditingController();
  final _locCtrl = TextEditingController();
  DateTime _start = DateTime.now().add(const Duration(hours: 1));
  DateTime _end = DateTime.now().add(const Duration(hours: 2));
  bool _allDay = false;
  bool _saving = false;
  String? _error;

  @override
  void dispose() {
    _titleCtrl.dispose();
    _locCtrl.dispose();
    super.dispose();
  }

  Future<void> _pick(bool isStart) async {
    final initial = isStart ? _start : _end;
    final date = await showDatePicker(
      context: context,
      initialDate: initial,
      firstDate: DateTime(2020),
      lastDate: DateTime(2100),
    );
    if (date == null || !mounted) return;
    TimeOfDay? time = TimeOfDay.fromDateTime(initial);
    if (!_allDay) {
      time = await showTimePicker(context: context, initialTime: time);
      if (time == null) return;
    }
    setState(() {
      final dt = DateTime(
          date.year, date.month, date.day, time?.hour ?? 0, time?.minute ?? 0);
      if (isStart) {
        _start = dt;
        if (_end.isBefore(_start)) _end = _start.add(const Duration(hours: 1));
      } else {
        _end = dt;
      }
    });
  }

  Future<void> _save() async {
    final title = _titleCtrl.text.trim();
    if (title.isEmpty) {
      setState(() => _error = 'Cần tiêu đề');
      return;
    }
    if (_end.isBefore(_start)) {
      setState(() => _error = 'Thời gian kết thúc phải sau bắt đầu');
      return;
    }
    setState(() {
      _saving = true;
      _error = null;
    });
    try {
      await _api.createEvent(
        title: title,
        startAt: _start.millisecondsSinceEpoch,
        endAt: _end.millisecondsSinceEpoch,
        allDay: _allDay,
        location: _locCtrl.text.trim(),
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
    return Padding(
      padding: EdgeInsets.fromLTRB(
          20, 20, 20, MediaQuery.of(context).viewInsets.bottom + 20),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const Text('Sự kiện mới',
              style: TextStyle(
                  color: Colors.white,
                  fontSize: 18,
                  fontWeight: FontWeight.bold)),
          const SizedBox(height: 16),
          TextField(
            controller: _titleCtrl,
            style: const TextStyle(color: Colors.white),
            decoration: _dec('Tiêu đề'),
          ),
          const SizedBox(height: 10),
          TextField(
            controller: _locCtrl,
            style: const TextStyle(color: Colors.white),
            decoration: _dec('Địa điểm (tuỳ chọn)'),
          ),
          const SizedBox(height: 6),
          SwitchListTile(
            contentPadding: EdgeInsets.zero,
            value: _allDay,
            onChanged: (v) => setState(() => _allDay = v),
            activeThumbColor: AppColors.accent,
            title: const Text('Cả ngày',
                style: TextStyle(color: Colors.white70, fontSize: 14)),
          ),
          _timeRow('Bắt đầu', _start, () => _pick(true)),
          _timeRow('Kết thúc', _end, () => _pick(false)),
          if (_error != null) ...[
            const SizedBox(height: 8),
            Text(_error!,
                style: const TextStyle(color: Colors.redAccent, fontSize: 12)),
          ],
          const SizedBox(height: 16),
          SizedBox(
            width: double.infinity,
            child: ElevatedButton(
              onPressed: _saving ? null : _save,
              style: ElevatedButton.styleFrom(
                backgroundColor: AppColors.accent,
                foregroundColor: Colors.black,
                padding: const EdgeInsets.symmetric(vertical: 14),
              ),
              child: _saving
                  ? const SizedBox(
                      width: 18,
                      height: 18,
                      child: CircularProgressIndicator(
                          strokeWidth: 2, color: Colors.black))
                  : const Text('Tạo sự kiện'),
            ),
          ),
        ],
      ),
    );
  }

  Widget _timeRow(String label, DateTime dt, VoidCallback onTap) {
    final str = _allDay
        ? '${dt.day.toString().padLeft(2, '0')}/${dt.month.toString().padLeft(2, '0')}/${dt.year}'
        : '${dt.day.toString().padLeft(2, '0')}/${dt.month.toString().padLeft(2, '0')}/${dt.year}  ${dt.hour.toString().padLeft(2, '0')}:${dt.minute.toString().padLeft(2, '0')}';
    return ListTile(
      contentPadding: EdgeInsets.zero,
      leading: const Icon(Icons.schedule, color: Colors.white38, size: 20),
      title: Text(label,
          style: const TextStyle(color: Colors.white54, fontSize: 13)),
      trailing: Text(str, style: const TextStyle(color: Colors.white)),
      onTap: onTap,
    );
  }

  InputDecoration _dec(String hint) => InputDecoration(
        hintText: hint,
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
      );
}

// ─── Schedules ─────────────────────────────────────────────────────────────

class _SchedulesTab extends StatefulWidget {
  const _SchedulesTab();

  @override
  State<_SchedulesTab> createState() => _SchedulesTabState();
}

class _SchedulesTabState extends State<_SchedulesTab>
    with AutomaticKeepAliveClientMixin {
  final _api = SpaceApi();
  final _config = ConfigService();

  List<SpaceSchedule> _schedules = [];
  bool _loading = true;
  String? _error;
  String? _groupFolder;
  String? _chatJid;

  @override
  bool get wantKeepAlive => true;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _resolveContext() async {
    _groupFolder = await _config.selectedAgentFolder;
    final cid = await _config.channelId;
    _chatJid = cid == null ? null : 'app:$cid:user:mobile-app';
  }

  Future<void> _load() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      await _resolveContext();
      final folder = _groupFolder;
      if (folder == null || folder.isEmpty) {
        if (!mounted) return;
        setState(() => _loading = false);
        return;
      }
      final schedules = await _api.listSchedules(folder);
      if (!mounted) return;
      setState(() {
        _schedules = schedules;
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
    final folder = _groupFolder;
    final jid = _chatJid;
    if (folder == null || jid == null) return;
    final promptCtrl = TextEditingController();
    final cronCtrl = TextEditingController(text: '0 9 * * *');
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: AppColors.surface,
        title:
            const Text('Lịch trình mới', style: TextStyle(color: Colors.white)),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            TextField(
              controller: promptCtrl,
              maxLines: 3,
              style: const TextStyle(color: Colors.white),
              decoration: const InputDecoration(
                labelText: 'Nội dung yêu cầu agent',
                labelStyle: TextStyle(color: Colors.white54),
              ),
            ),
            const SizedBox(height: 8),
            TextField(
              controller: cronCtrl,
              style: const TextStyle(color: Colors.white, fontFamily: 'monospace'),
              decoration: const InputDecoration(
                labelText: 'Cron (vd: 0 9 * * *)',
                labelStyle: TextStyle(color: Colors.white54),
              ),
            ),
          ],
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
    if (ok != true) return;
    final prompt = promptCtrl.text.trim();
    final cron = cronCtrl.text.trim();
    if (prompt.isEmpty || cron.isEmpty) return;
    try {
      await _api.createSchedule(
        prompt: prompt,
        cron: cron,
        groupFolder: folder,
        chatJid: jid,
      );
      _load();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Lỗi tạo: $e')));
      }
    }
  }

  Future<void> _cancel(SpaceSchedule s) async {
    try {
      await _api.cancelSchedule(s.id, s.groupFolder);
      _load();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Lỗi huỷ: $e')));
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    final canCreate = (_groupFolder?.isNotEmpty ?? false) && _chatJid != null;
    return Scaffold(
      backgroundColor: Colors.transparent,
      floatingActionButton: canCreate
          ? FloatingActionButton(
              onPressed: _create,
              backgroundColor: AppColors.accent,
              foregroundColor: Colors.black,
              child: const Icon(Icons.add),
            )
          : null,
      body: _buildBody(),
    );
  }

  Widget _buildBody() {
    if (_loading) return const LoadingState(text: 'Đang tải lịch trình…');
    if (_groupFolder == null || _groupFolder!.isEmpty) {
      return const EmptyState(
        icon: Icons.schedule,
        message: 'Chưa chọn agent',
        hint: 'Mở tab Chat và chọn một agent trước để quản lý lịch trình',
      );
    }
    if (_error != null) return ErrorState(message: _error!, onRetry: _load);
    if (_schedules.isEmpty) {
      return const EmptyState(
        icon: Icons.event_repeat,
        message: 'Chưa có lịch trình',
        hint: 'Nhấn + để tạo tác vụ định kỳ cho agent',
      );
    }
    return RefreshIndicator(
      onRefresh: _load,
      color: AppColors.accent,
      backgroundColor: AppColors.surface,
      child: ListView.builder(
        padding: const EdgeInsets.fromLTRB(12, 12, 12, 88),
        itemCount: _schedules.length,
        itemBuilder: (ctx, i) => _scheduleCard(_schedules[i]),
      ),
    );
  }

  Widget _scheduleCard(SpaceSchedule s) {
    final active = s.status == 'active' || s.status == 'pending';
    return Card(
      color: Colors.white.withValues(alpha: 0.04),
      margin: const EdgeInsets.only(bottom: 10),
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(12),
        side: const BorderSide(color: AppColors.cardBorder),
      ),
      child: ListTile(
        leading: Icon(
          active ? Icons.alarm_on : Icons.alarm_off,
          color: active ? const Color(0xFF66BB6A) : Colors.white38,
        ),
        title: Text(s.prompt,
            style: const TextStyle(color: Colors.white, fontSize: 14),
            maxLines: 2,
            overflow: TextOverflow.ellipsis),
        subtitle: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            const SizedBox(height: 4),
            Text(
              '${s.scheduleType} · ${s.scheduleValue}',
              style: const TextStyle(
                  color: AppColors.cyan,
                  fontSize: 11,
                  fontFamily: 'monospace'),
            ),
            if (s.nextRun != null && s.nextRun!.isNotEmpty)
              Text('Lần tới: ${s.nextRun}',
                  style: const TextStyle(color: Colors.white38, fontSize: 11)),
          ],
        ),
        trailing: active
            ? IconButton(
                icon: const Icon(Icons.cancel_outlined,
                    color: Colors.redAccent, size: 20),
                onPressed: () => _cancel(s),
              )
            : Text(s.status,
                style: const TextStyle(color: Colors.white38, fontSize: 11)),
      ),
    );
  }
}

// ─── Email ───────────────────────────────────────────────────────────────────

class _EmailTab extends StatefulWidget {
  const _EmailTab();

  @override
  State<_EmailTab> createState() => _EmailTabState();
}

class _EmailTabState extends State<_EmailTab>
    with AutomaticKeepAliveClientMixin {
  final _api = SpaceApi();
  List<SpaceEmailAccount> _accounts = [];
  List<SpaceEmail> _inbox = [];
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
      final accounts = await _api.listEmailAccounts();
      final inbox = accounts.isEmpty ? <SpaceEmail>[] : await _api.inbox();
      if (!mounted) return;
      setState(() {
        _accounts = accounts;
        _inbox = inbox;
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

  Future<void> _openEmail(SpaceEmail e) async {
    showModalBottomSheet(
      context: context,
      isScrollControlled: true,
      backgroundColor: AppColors.surface,
      shape: const RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(top: Radius.circular(18)),
      ),
      builder: (_) => _EmailViewer(id: e.id),
    );
  }

  Future<void> _compose() async {
    final sent = await showModalBottomSheet<bool>(
      context: context,
      isScrollControlled: true,
      backgroundColor: AppColors.surface,
      shape: const RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(top: Radius.circular(18)),
      ),
      builder: (_) => _ComposeSheet(
        accountId: _accounts.isNotEmpty ? _accounts.first.id : null,
      ),
    );
    if (sent == true && mounted) {
      ScaffoldMessenger.of(context)
          .showSnackBar(const SnackBar(content: Text('Đã gửi email')));
    }
  }

  Future<void> _manageAccounts() async {
    await showModalBottomSheet(
      context: context,
      isScrollControlled: true,
      backgroundColor: AppColors.surface,
      shape: const RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(top: Radius.circular(18)),
      ),
      builder: (_) => _AccountsSheet(accounts: _accounts),
    );
    _load(); // reflect add/remove
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    return Scaffold(
      backgroundColor: Colors.transparent,
      floatingActionButton: _accounts.isEmpty
          ? null
          : FloatingActionButton(
              onPressed: _compose,
              backgroundColor: AppColors.accent,
              foregroundColor: Colors.black,
              child: const Icon(Icons.edit),
            ),
      body: _buildBody(),
    );
  }

  Widget _buildBody() {
    if (_loading) return const LoadingState(text: 'Đang tải email…');
    if (_error != null) return ErrorState(message: _error!, onRetry: _load);
    if (_accounts.isEmpty) {
      return EmptyState(
        icon: Icons.mark_email_unread_outlined,
        message: 'Chưa có tài khoản email',
        hint: 'Thêm tài khoản IMAP/SMTP để đọc & gửi thư',
        action: OutlinedButton.icon(
          onPressed: _manageAccounts,
          icon: const Icon(Icons.add, color: AppColors.accent, size: 18),
          label: const Text('Thêm tài khoản',
              style: TextStyle(color: AppColors.accent)),
          style: OutlinedButton.styleFrom(
            side: const BorderSide(color: AppColors.accent),
          ),
        ),
      );
    }
    return Column(
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(12, 8, 8, 4),
          child: Row(
            children: [
              Expanded(
                child: Text(
                  '${_accounts.length} tài khoản · ${_inbox.length} thư',
                  style: const TextStyle(color: Colors.white38, fontSize: 12),
                ),
              ),
              TextButton.icon(
                onPressed: _manageAccounts,
                icon: const Icon(Icons.manage_accounts,
                    color: Colors.white54, size: 16),
                label: const Text('Quản lý',
                    style: TextStyle(color: Colors.white54, fontSize: 12)),
              ),
            ],
          ),
        ),
        Expanded(
          child: _inbox.isEmpty
              ? const EmptyState(
                  icon: Icons.inbox_outlined,
                  message: 'Hộp thư trống',
                  hint: 'Thư sẽ xuất hiện sau khi đồng bộ IMAP',
                )
              : RefreshIndicator(
                  onRefresh: _load,
                  color: AppColors.accent,
                  backgroundColor: AppColors.surface,
                  child: ListView.builder(
                    padding: const EdgeInsets.fromLTRB(12, 0, 12, 88),
                    itemCount: _inbox.length,
                    itemBuilder: (ctx, i) => _emailTile(_inbox[i]),
                  ),
                ),
        ),
      ],
    );
  }

  Widget _emailTile(SpaceEmail e) {
    return Card(
      color: Colors.white.withValues(alpha: 0.04),
      margin: const EdgeInsets.only(bottom: 8),
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(12),
        side: const BorderSide(color: AppColors.cardBorder),
      ),
      child: ListTile(
        leading: Icon(
          e.unread ? Icons.mark_email_unread : Icons.mark_email_read_outlined,
          color: e.unread ? AppColors.accent : Colors.white38,
          size: 20,
        ),
        title: Text(
          e.subject?.isNotEmpty == true ? e.subject! : '(không tiêu đề)',
          style: TextStyle(
            color: Colors.white,
            fontSize: 14,
            fontWeight: e.unread ? FontWeight.w700 : FontWeight.w500,
          ),
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
        ),
        subtitle: Text(
          e.from ?? '',
          style: const TextStyle(color: Colors.white54, fontSize: 12),
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
        ),
        onTap: () => _openEmail(e),
      ),
    );
  }
}

class _EmailViewer extends StatefulWidget {
  final String id;
  const _EmailViewer({required this.id});

  @override
  State<_EmailViewer> createState() => _EmailViewerState();
}

class _EmailViewerState extends State<_EmailViewer> {
  final _api = SpaceApi();
  SpaceEmail? _email;
  bool _loading = true;
  String? _error;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    try {
      final email = await _api.readEmail(widget.id);
      if (!mounted) return;
      setState(() {
        _email = email;
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
    final e = _email;
    return FractionallySizedBox(
      heightFactor: 0.9,
      child: _loading
          ? const LoadingState()
          : _error != null
              ? ErrorState(message: _error!, onRetry: _load)
              : Padding(
                  padding: const EdgeInsets.all(18),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        e?.subject?.isNotEmpty == true
                            ? e!.subject!
                            : '(không tiêu đề)',
                        style: const TextStyle(
                            color: Colors.white,
                            fontSize: 17,
                            fontWeight: FontWeight.bold),
                      ),
                      const SizedBox(height: 8),
                      Text('Từ: ${e?.from ?? ''}',
                          style: const TextStyle(
                              color: Colors.white54, fontSize: 12)),
                      if (e?.to != null && e!.to!.isNotEmpty)
                        Text('Đến: ${e.to}',
                            style: const TextStyle(
                                color: Colors.white54, fontSize: 12)),
                      const Divider(color: AppColors.cardBorder, height: 20),
                      Expanded(
                        child: SingleChildScrollView(
                          child: SelectableText(
                            e?.bodyText?.isNotEmpty == true
                                ? e!.bodyText!
                                : '(không có nội dung văn bản)',
                            style: const TextStyle(
                                color: Colors.white70,
                                fontSize: 13,
                                height: 1.45),
                          ),
                        ),
                      ),
                    ],
                  ),
                ),
    );
  }
}

class _ComposeSheet extends StatefulWidget {
  final String? accountId;
  const _ComposeSheet({this.accountId});

  @override
  State<_ComposeSheet> createState() => _ComposeSheetState();
}

class _ComposeSheetState extends State<_ComposeSheet> {
  final _api = SpaceApi();
  final _to = TextEditingController();
  final _subject = TextEditingController();
  final _body = TextEditingController();
  bool _sending = false;
  String? _error;

  @override
  void dispose() {
    _to.dispose();
    _subject.dispose();
    _body.dispose();
    super.dispose();
  }

  Future<void> _send() async {
    if (_to.text.trim().isEmpty) {
      setState(() => _error = 'Cần địa chỉ người nhận');
      return;
    }
    setState(() {
      _sending = true;
      _error = null;
    });
    try {
      await _api.sendEmail(
        to: _to.text.trim(),
        subject: _subject.text.trim(),
        body: _body.text,
        accountId: widget.accountId,
      );
      if (mounted) Navigator.pop(context, true);
    } catch (e) {
      if (mounted) {
        setState(() {
          _error = '$e';
          _sending = false;
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: EdgeInsets.fromLTRB(
          20, 20, 20, MediaQuery.of(context).viewInsets.bottom + 20),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const Text('Soạn email',
              style: TextStyle(
                  color: Colors.white,
                  fontSize: 18,
                  fontWeight: FontWeight.bold)),
          const SizedBox(height: 14),
          _f(_to, 'Đến (email)'),
          const SizedBox(height: 10),
          _f(_subject, 'Tiêu đề'),
          const SizedBox(height: 10),
          _f(_body, 'Nội dung', maxLines: 6),
          if (_error != null) ...[
            const SizedBox(height: 8),
            Text(_error!,
                style: const TextStyle(color: Colors.redAccent, fontSize: 12)),
          ],
          const SizedBox(height: 14),
          SizedBox(
            width: double.infinity,
            child: ElevatedButton.icon(
              onPressed: _sending ? null : _send,
              icon: _sending
                  ? const SizedBox(
                      width: 16,
                      height: 16,
                      child: CircularProgressIndicator(
                          strokeWidth: 2, color: Colors.black))
                  : const Icon(Icons.send),
              label: const Text('Gửi'),
              style: ElevatedButton.styleFrom(
                backgroundColor: AppColors.accent,
                foregroundColor: Colors.black,
                padding: const EdgeInsets.symmetric(vertical: 14),
              ),
            ),
          ),
        ],
      ),
    );
  }

  Widget _f(TextEditingController c, String hint, {int maxLines = 1}) {
    return TextField(
      controller: c,
      maxLines: maxLines,
      style: const TextStyle(color: Colors.white),
      decoration: InputDecoration(
        hintText: hint,
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
    );
  }
}

class _AccountsSheet extends StatefulWidget {
  final List<SpaceEmailAccount> accounts;
  const _AccountsSheet({required this.accounts});

  @override
  State<_AccountsSheet> createState() => _AccountsSheetState();
}

class _AccountsSheetState extends State<_AccountsSheet> {
  final _api = SpaceApi();
  late List<SpaceEmailAccount> _accounts = List.of(widget.accounts);

  Future<void> _refresh() async {
    final a = await _api.listEmailAccounts();
    if (mounted) setState(() => _accounts = a);
  }

  Future<void> _add() async {
    final added = await showModalBottomSheet<bool>(
      context: context,
      isScrollControlled: true,
      backgroundColor: AppColors.surface,
      shape: const RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(top: Radius.circular(18)),
      ),
      builder: (_) => const _AccountForm(),
    );
    if (added == true) _refresh();
  }

  Future<void> _delete(SpaceEmailAccount a) async {
    try {
      await _api.deleteEmailAccount(a.id);
      _refresh();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Lỗi xoá: $e')));
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 18, 20, 24),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              const Expanded(
                child: Text('Tài khoản email',
                    style: TextStyle(
                        color: Colors.white,
                        fontSize: 18,
                        fontWeight: FontWeight.bold)),
              ),
              IconButton(
                icon: const Icon(Icons.add, color: AppColors.accent),
                onPressed: _add,
              ),
            ],
          ),
          const SizedBox(height: 8),
          if (_accounts.isEmpty)
            const Padding(
              padding: EdgeInsets.symmetric(vertical: 20),
              child: Text('Chưa có tài khoản',
                  style: TextStyle(color: Colors.white38)),
            )
          else
            ..._accounts.map((a) => ListTile(
                  contentPadding: EdgeInsets.zero,
                  leading: const Icon(Icons.account_circle_outlined,
                      color: AppColors.cyan),
                  title: Text(a.label,
                      style: const TextStyle(color: Colors.white)),
                  subtitle: Text(a.email,
                      style: const TextStyle(
                          color: Colors.white54, fontSize: 12)),
                  trailing: IconButton(
                    icon: const Icon(Icons.delete_outline,
                        color: Colors.white38),
                    onPressed: () => _delete(a),
                  ),
                )),
        ],
      ),
    );
  }
}

class _AccountForm extends StatefulWidget {
  const _AccountForm();

  @override
  State<_AccountForm> createState() => _AccountFormState();
}

class _AccountFormState extends State<_AccountForm> {
  final _api = SpaceApi();
  final _label = TextEditingController();
  final _email = TextEditingController();
  final _imapHost = TextEditingController();
  final _imapPort = TextEditingController(text: '993');
  final _smtpHost = TextEditingController();
  final _smtpPort = TextEditingController(text: '587');
  final _username = TextEditingController();
  final _password = TextEditingController();
  bool _useTls = true;
  bool _saving = false;
  String? _error;

  @override
  void dispose() {
    for (final c in [
      _label,
      _email,
      _imapHost,
      _imapPort,
      _smtpHost,
      _smtpPort,
      _username,
      _password,
    ]) {
      c.dispose();
    }
    super.dispose();
  }

  Future<void> _save() async {
    if (_email.text.trim().isEmpty ||
        _imapHost.text.trim().isEmpty ||
        _smtpHost.text.trim().isEmpty) {
      setState(() => _error = 'Cần email, IMAP host và SMTP host');
      return;
    }
    setState(() {
      _saving = true;
      _error = null;
    });
    try {
      await _api.addEmailAccount({
        'label': _label.text.trim().isEmpty
            ? _email.text.trim()
            : _label.text.trim(),
        'email': _email.text.trim(),
        'imap_host': _imapHost.text.trim(),
        'imap_port': int.tryParse(_imapPort.text.trim()) ?? 993,
        'smtp_host': _smtpHost.text.trim(),
        'smtp_port': int.tryParse(_smtpPort.text.trim()) ?? 587,
        'username': _username.text.trim().isEmpty
            ? _email.text.trim()
            : _username.text.trim(),
        'password': _password.text,
        'use_tls': _useTls,
      });
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
    return Padding(
      padding: EdgeInsets.fromLTRB(
          20, 20, 20, MediaQuery.of(context).viewInsets.bottom + 20),
      child: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            const Text('Thêm tài khoản email',
                style: TextStyle(
                    color: Colors.white,
                    fontSize: 18,
                    fontWeight: FontWeight.bold)),
            const SizedBox(height: 14),
            _f(_label, 'Nhãn (tuỳ chọn)'),
            const SizedBox(height: 8),
            _f(_email, 'Email'),
            const SizedBox(height: 8),
            Row(
              children: [
                Expanded(flex: 2, child: _f(_imapHost, 'IMAP host')),
                const SizedBox(width: 8),
                Expanded(child: _f(_imapPort, 'Port', number: true)),
              ],
            ),
            const SizedBox(height: 8),
            Row(
              children: [
                Expanded(flex: 2, child: _f(_smtpHost, 'SMTP host')),
                const SizedBox(width: 8),
                Expanded(child: _f(_smtpPort, 'Port', number: true)),
              ],
            ),
            const SizedBox(height: 8),
            _f(_username, 'Username (mặc định = email)'),
            const SizedBox(height: 8),
            _f(_password, 'Mật khẩu', obscure: true),
            SwitchListTile(
              contentPadding: EdgeInsets.zero,
              value: _useTls,
              onChanged: (v) => setState(() => _useTls = v),
              activeThumbColor: AppColors.accent,
              title: const Text('Dùng TLS',
                  style: TextStyle(color: Colors.white70, fontSize: 14)),
            ),
            if (_error != null) ...[
              const SizedBox(height: 4),
              Text(_error!,
                  style:
                      const TextStyle(color: Colors.redAccent, fontSize: 12)),
            ],
            const SizedBox(height: 12),
            SizedBox(
              width: double.infinity,
              child: ElevatedButton(
                onPressed: _saving ? null : _save,
                style: ElevatedButton.styleFrom(
                  backgroundColor: AppColors.accent,
                  foregroundColor: Colors.black,
                  padding: const EdgeInsets.symmetric(vertical: 14),
                ),
                child: _saving
                    ? const SizedBox(
                        width: 18,
                        height: 18,
                        child: CircularProgressIndicator(
                            strokeWidth: 2, color: Colors.black))
                    : const Text('Lưu tài khoản'),
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _f(TextEditingController c, String hint,
      {bool number = false, bool obscure = false}) {
    return TextField(
      controller: c,
      obscureText: obscure,
      keyboardType: number ? TextInputType.number : null,
      style: const TextStyle(color: Colors.white),
      decoration: InputDecoration(
        hintText: hint,
        hintStyle: const TextStyle(color: Colors.white38),
        isDense: true,
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
    );
  }
}
