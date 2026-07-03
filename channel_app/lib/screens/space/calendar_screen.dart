import 'dart:async';
import 'package:flutter/material.dart';
import 'package:table_calendar/table_calendar.dart';
import '../../models/space_models.dart';
import '../../services/space_api.dart';
import '../../services/relay_manager.dart';
import '../../theme/tokens.dart';
import '../../widgets/app_drawer.dart';
import '../../widgets/states.dart';

/// Calendar with two tabs: an event LIST and a month/week GRID where days with
/// events are marked and tapping a day shows that day's events.
class CalendarScreen extends StatefulWidget {
  const CalendarScreen({super.key});
  @override
  State<CalendarScreen> createState() => _CalendarScreenState();
}

class _CalendarScreenState extends State<CalendarScreen>
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
    return Scaffold(
      backgroundColor: c.bg,
      drawer: const AppDrawer(),
      appBar: AppBar(
        backgroundColor: c.surface,
        elevation: 0,
        leading: Builder(
          builder: (ctx) => IconButton(
            icon: Icon(Icons.menu, color: c.textSecondary),
            onPressed: () => Scaffold.of(ctx).openDrawer(),
          ),
        ),
        title: Row(
          children: [
            Text('Calendar', style: TextStyle(color: c.textPrimary)),
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
          indicatorColor: c.accent,
          labelColor: c.accent,
          unselectedLabelColor: c.textMuted,
          tabs: const [
            Tab(icon: Icon(Icons.view_list_outlined), text: 'Danh sách'),
            Tab(icon: Icon(Icons.calendar_month_outlined), text: 'Lịch'),
          ],
        ),
      ),
      body: Container(
        decoration: BoxDecoration(color: c.bg),
        child: TabBarView(
          controller: _tabs,
          children: const [_CalendarTab(), _CalendarGrid()],
        ),
      ),
    );
  }
}

/// Month/week grid (table_calendar) with per-day event markers + a detail list
/// of the selected day's events below.
class _CalendarGrid extends StatefulWidget {
  const _CalendarGrid();
  @override
  State<_CalendarGrid> createState() => _CalendarGridState();
}

class _CalendarGridState extends State<_CalendarGrid>
    with AutomaticKeepAliveClientMixin {
  final _api = SpaceApi();
  List<SpaceEvent> _events = [];
  bool _loading = true;
  String? _error;
  CalendarFormat _format = CalendarFormat.month;
  DateTime _focused = DateTime.now();
  DateTime _selected = DateTime.now();

  @override
  bool get wantKeepAlive => true;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() {
      _loading = _events.isEmpty;
      _error = null;
    });
    var fresh = false;
    // Local-DB paint races the relay fetch in parallel — the relay result
    // always wins once it arrives.
    if (_events.isEmpty) {
      unawaited(_api.listEventsCached().then((cached) {
        if (fresh || cached.isEmpty || !mounted || _events.isNotEmpty) return;
        setState(() {
          _events = cached;
          _loading = false;
          _error = null;
        });
      }));
    }
    try {
      // Wide window so markers show across months.
      final now = DateTime.now();
      final from = DateTime(now.year - 1, now.month).millisecondsSinceEpoch;
      final to = DateTime(now.year + 1, now.month).millisecondsSinceEpoch;
      final events = await _api.listEvents(from: from, to: to, cache: true);
      fresh = true;
      if (!mounted) return;
      setState(() {
        _events = events;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        // Keep the cached view usable when the refresh fails.
        _error = _events.isEmpty ? '$e' : null;
        _loading = false;
      });
    }
  }

  List<SpaceEvent> _eventsForDay(DateTime day) =>
      _events.where((e) => isSameDay(e.start, day)).toList()
        ..sort((a, b) => a.start.compareTo(b.start));

  Future<void> _addForSelected() async {
    final saved = await showModalBottomSheet<bool>(
      context: context,
      isScrollControlled: true,
      backgroundColor: context.colors.surface,
      shape: const RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(top: Radius.circular(18)),
      ),
      builder: (_) => _EventEditor(initialDay: _selected),
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
    final c = context.colors;
    if (_loading) return const LoadingState(text: 'Đang tải lịch…');
    if (_error != null) return ErrorState(message: _error!, onRetry: _load);
    return Scaffold(
      backgroundColor: Colors.transparent,
      floatingActionButton: FloatingActionButton(
        heroTag: 'calendar-grid-fab',
        onPressed: _addForSelected,
        backgroundColor: c.accent,
        foregroundColor: Colors.white,
        child: const Icon(Icons.add),
      ),
      body: Column(
        children: [
          Card(
            margin: const EdgeInsets.all(8),
            color: c.surfaceAlt,
            shape: RoundedRectangleBorder(
              borderRadius: BorderRadius.circular(12),
              side: BorderSide(color: c.border),
            ),
            child: Padding(
              padding: const EdgeInsets.symmetric(vertical: 4),
              child: TableCalendar<SpaceEvent>(
                firstDay: DateTime(DateTime.now().year - 2),
                lastDay: DateTime(DateTime.now().year + 2, 12, 31),
                focusedDay: _focused,
                calendarFormat: _format,
                availableCalendarFormats: const {
                  CalendarFormat.month: 'Tháng',
                  CalendarFormat.twoWeeks: '2 tuần',
                  CalendarFormat.week: 'Tuần',
                },
                startingDayOfWeek: StartingDayOfWeek.monday,
                selectedDayPredicate: (d) => isSameDay(_selected, d),
                eventLoader: _eventsForDay,
                onDaySelected: (sel, foc) => setState(() {
                  _selected = sel;
                  _focused = foc;
                }),
                onFormatChanged: (f) => setState(() => _format = f),
                onPageChanged: (foc) => _focused = foc,
                headerStyle: HeaderStyle(
                  titleCentered: true,
                  formatButtonShowsNext: false,
                  titleTextStyle: TextStyle(
                      color: c.textPrimary,
                      fontSize: 15,
                      fontWeight: FontWeight.w600),
                  formatButtonDecoration: BoxDecoration(
                    border: Border.all(color: c.border),
                    borderRadius: BorderRadius.circular(6),
                  ),
                  formatButtonTextStyle:
                      TextStyle(color: c.textSecondary, fontSize: 12),
                  leftChevronIcon:
                      Icon(Icons.chevron_left, color: c.textSecondary),
                  rightChevronIcon:
                      Icon(Icons.chevron_right, color: c.textSecondary),
                ),
                daysOfWeekStyle: DaysOfWeekStyle(
                  weekdayStyle: TextStyle(color: c.textMuted, fontSize: 12),
                  weekendStyle: TextStyle(color: c.textMuted, fontSize: 12),
                ),
                calendarStyle: CalendarStyle(
                  defaultTextStyle: TextStyle(color: c.textPrimary),
                  weekendTextStyle: TextStyle(color: c.textSecondary),
                  outsideTextStyle: TextStyle(color: c.textMuted),
                  todayDecoration: BoxDecoration(
                      color: c.accent.withValues(alpha: 0.25),
                      shape: BoxShape.circle),
                  todayTextStyle: TextStyle(color: c.textPrimary),
                  selectedDecoration:
                      BoxDecoration(color: c.accent, shape: BoxShape.circle),
                  selectedTextStyle: const TextStyle(color: Colors.white),
                  markerDecoration: const BoxDecoration(
                      color: AppTokens.cyan, shape: BoxShape.circle),
                  markersMaxCount: 3,
                ),
              ),
            ),
          ),
          Divider(height: 1, color: c.border),
          Expanded(child: _dayDetail(c, _eventsForDay(_selected))),
        ],
      ),
    );
  }

  Widget _dayDetail(AppColors c, List<SpaceEvent> events) {
    final label =
        '${_selected.day.toString().padLeft(2, '0')}/${_selected.month.toString().padLeft(2, '0')}/${_selected.year}';
    if (events.isEmpty) {
      return Center(
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: Column(mainAxisSize: MainAxisSize.min, children: [
            Icon(Icons.event_available, color: c.textMuted, size: 40),
            const SizedBox(height: 10),
            Text('Không có sự kiện ngày $label',
                style: TextStyle(color: c.textMuted, fontSize: 13)),
          ]),
        ),
      );
    }
    return ListView(
      padding: const EdgeInsets.fromLTRB(12, 8, 12, 88),
      children: [
        Padding(
          padding: const EdgeInsets.symmetric(vertical: 6),
          child: Text('Sự kiện ngày $label',
              style: TextStyle(
                  color: c.textSecondary,
                  fontSize: 13,
                  fontWeight: FontWeight.w600)),
        ),
        for (final e in events) _gridEventCard(c, e),
      ],
    );
  }

  Widget _gridEventCard(AppColors c, SpaceEvent e) {
    String hm(DateTime d) =>
        '${d.hour.toString().padLeft(2, '0')}:${d.minute.toString().padLeft(2, '0')}';
    final timeStr = e.allDay ? 'Cả ngày' : '${hm(e.start)} – ${hm(e.end)}';
    return Card(
      color: c.surfaceAlt,
      margin: const EdgeInsets.only(bottom: 8),
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(12),
        side: BorderSide(color: c.border),
      ),
      child: ListTile(
        leading: Icon(Icons.event, color: c.accent),
        title: Text(e.title,
            style:
                TextStyle(color: c.textPrimary, fontWeight: FontWeight.w600)),
        subtitle: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            const SizedBox(height: 2),
            Text(timeStr, style: TextStyle(color: c.textMuted, fontSize: 12)),
            if (e.location != null && e.location!.isNotEmpty)
              Text('📍 ${e.location}',
                  style: TextStyle(color: c.textMuted, fontSize: 11)),
          ],
        ),
        trailing: IconButton(
          icon: Icon(Icons.delete_outline, color: c.textMuted, size: 20),
          onPressed: () => _delete(e),
        ),
      ),
    );
  }
}

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
      backgroundColor: context.colors.surface,
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
    final c = context.colors;
    return Scaffold(
      backgroundColor: Colors.transparent,
      floatingActionButton: FloatingActionButton(
        heroTag: 'calendar-list-fab',
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
      color: c.accent,
      backgroundColor: c.surface,
      child: ListView(
        padding: const EdgeInsets.fromLTRB(12, 12, 12, 88),
        children: [
          for (final day in days) ...[
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 8),
              child: Text(
                _dayLabel(byDay[day]!.first.start),
                style: TextStyle(
                    color: c.textSecondary,
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
    final c = context.colors;
    final color = _parseColor(e.color) ?? c.accent;
    final timeStr = e.allDay
        ? 'Cả ngày'
        : '${_hm(e.start)} – ${_hm(e.end)}';
    return Card(
      color: c.surfaceAlt,
      margin: const EdgeInsets.only(bottom: 8),
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(12),
        side: BorderSide(color: c.border),
      ),
      child: ListTile(
        leading: Container(width: 4, height: 40, color: color),
        title: Text(e.title,
            style: TextStyle(
                color: c.textPrimary, fontWeight: FontWeight.w600)),
        subtitle: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            const SizedBox(height: 2),
            Text(timeStr,
                style: TextStyle(color: c.textMuted, fontSize: 12)),
            if (e.location != null && e.location!.isNotEmpty)
              Text('📍 ${e.location}',
                  style: TextStyle(color: c.textMuted, fontSize: 11)),
          ],
        ),
        trailing: IconButton(
          icon: Icon(Icons.delete_outline,
              color: c.textMuted, size: 20),
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
  const _EventEditor({this.initialDay});

  /// When set, the new event defaults to 09:00–10:00 on this day (used when
  /// adding from a tapped calendar day).
  final DateTime? initialDay;

  @override
  State<_EventEditor> createState() => _EventEditorState();
}

class _EventEditorState extends State<_EventEditor> {
  final _api = SpaceApi();
  final _titleCtrl = TextEditingController();
  final _locCtrl = TextEditingController();
  late DateTime _start = widget.initialDay != null
      ? DateTime(widget.initialDay!.year, widget.initialDay!.month,
          widget.initialDay!.day, 9)
      : DateTime.now().add(const Duration(hours: 1));
  late DateTime _end = _start.add(const Duration(hours: 1));
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
    final c = context.colors;
    return Padding(
      padding: EdgeInsets.fromLTRB(
          20, 20, 20, MediaQuery.of(context).viewInsets.bottom + 20),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text('Sự kiện mới',
              style: TextStyle(
                  color: c.textPrimary,
                  fontSize: 18,
                  fontWeight: FontWeight.bold)),
          const SizedBox(height: 16),
          TextField(
            controller: _titleCtrl,
            style: TextStyle(color: c.textPrimary),
            decoration: _dec('Tiêu đề'),
          ),
          const SizedBox(height: 10),
          TextField(
            controller: _locCtrl,
            style: TextStyle(color: c.textPrimary),
            decoration: _dec('Địa điểm (tuỳ chọn)'),
          ),
          const SizedBox(height: 6),
          SwitchListTile(
            contentPadding: EdgeInsets.zero,
            value: _allDay,
            onChanged: (v) => setState(() => _allDay = v),
            activeThumbColor: c.accent,
            title: Text('Cả ngày',
                style: TextStyle(color: c.textSecondary, fontSize: 14)),
          ),
          _timeRow('Bắt đầu', _start, () => _pick(true)),
          _timeRow('Kết thúc', _end, () => _pick(false)),
          if (_error != null) ...[
            const SizedBox(height: 8),
            Text(_error!,
                style: const TextStyle(color: AppTokens.danger, fontSize: 12)),
          ],
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
                  : const Text('Tạo sự kiện'),
            ),
          ),
        ],
      ),
    );
  }

  Widget _timeRow(String label, DateTime dt, VoidCallback onTap) {
    final c = context.colors;
    final str = _allDay
        ? '${dt.day.toString().padLeft(2, '0')}/${dt.month.toString().padLeft(2, '0')}/${dt.year}'
        : '${dt.day.toString().padLeft(2, '0')}/${dt.month.toString().padLeft(2, '0')}/${dt.year}  ${dt.hour.toString().padLeft(2, '0')}:${dt.minute.toString().padLeft(2, '0')}';
    return ListTile(
      contentPadding: EdgeInsets.zero,
      leading: Icon(Icons.schedule, color: c.textMuted, size: 20),
      title: Text(label,
          style: TextStyle(color: c.textMuted, fontSize: 13)),
      trailing: Text(str, style: TextStyle(color: c.textPrimary)),
      onTap: onTap,
    );
  }

  InputDecoration _dec(String hint) {
    final c = context.colors;
    return InputDecoration(
      hintText: hint,
      hintStyle: TextStyle(color: c.textMuted),
      filled: true,
      fillColor: c.surfaceAlt,
      border: OutlineInputBorder(
        borderRadius: BorderRadius.circular(10),
        borderSide: BorderSide(color: c.border),
      ),
      enabledBorder: OutlineInputBorder(
        borderRadius: BorderRadius.circular(10),
        borderSide: BorderSide(color: c.border),
      ),
    );
  }
}
