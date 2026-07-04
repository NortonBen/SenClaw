import 'dart:async';
import 'package:flutter/material.dart';
import '../../models/space_models.dart';
import '../../services/config_service.dart';
import '../../services/language_service.dart';
import '../../services/space_api.dart';
import '../../theme/tokens.dart';
import '../../widgets/states.dart';
import 'space_page.dart';

class SchedulesScreen extends StatelessWidget {
  const SchedulesScreen({super.key});
  @override
  Widget build(BuildContext context) => SpacePage(
      title: tr('Lịch trình', 'Schedules'), child: const _SchedulesTab());
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
      _loading = _schedules.isEmpty;
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
      var fresh = false;
      // Local-DB paint races the relay fetch in parallel — the relay
      // result always wins once it arrives.
      if (_schedules.isEmpty) {
        unawaited(_api.listSchedulesCached(folder).then((cached) {
          if (fresh || cached.isEmpty || !mounted) return;
          if (_schedules.isNotEmpty) return;
          setState(() {
            _schedules = cached;
            _loading = false;
            _error = null;
          });
        }));
      }
      final schedules = await _api.listSchedules(folder);
      fresh = true;
      if (!mounted) return;
      setState(() {
        _schedules = schedules;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        // Keep the cached view usable when the refresh fails.
        _error = _schedules.isEmpty ? '$e' : null;
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
    final c = context.colors;
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: c.surface,
        title: Text(tr('Lịch trình mới', 'New schedule'),
            style: TextStyle(color: c.textPrimary)),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            TextField(
              controller: promptCtrl,
              maxLines: 3,
              style: TextStyle(color: c.textPrimary),
              decoration: InputDecoration(
                labelText: tr('Nội dung yêu cầu agent', 'Prompt for the agent'),
                labelStyle: TextStyle(color: c.textMuted),
              ),
            ),
            const SizedBox(height: 8),
            TextField(
              controller: cronCtrl,
              style: TextStyle(color: c.textPrimary, fontFamily: 'monospace'),
              decoration: InputDecoration(
                labelText: tr('Cron (vd: 0 9 * * *)', 'Cron (e.g. 0 9 * * *)'),
                labelStyle: TextStyle(color: c.textMuted),
              ),
            ),
          ],
        ),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(ctx, false),
              child: Text(tr('Huỷ', 'Cancel'))),
          TextButton(
              onPressed: () => Navigator.pop(ctx, true),
              child: Text(tr('Tạo', 'Create'),
                  style: TextStyle(color: c.accent))),
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
            .showSnackBar(
                SnackBar(content: Text(tr('Lỗi tạo: $e', 'Create error: $e'))));
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
            .showSnackBar(
                SnackBar(content: Text(tr('Lỗi huỷ: $e', 'Cancel error: $e'))));
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    final c = context.colors;
    final canCreate = (_groupFolder?.isNotEmpty ?? false) && _chatJid != null;
    return Scaffold(
      backgroundColor: Colors.transparent,
      floatingActionButton: canCreate
          ? FloatingActionButton(
              onPressed: _create,
              backgroundColor: c.accent,
              foregroundColor: Colors.white,
              child: const Icon(Icons.add),
            )
          : null,
      body: _buildBody(),
    );
  }

  Widget _buildBody() {
    if (_loading) {
      return LoadingState(
          text: tr('Đang tải lịch trình…', 'Loading schedules…'));
    }
    if (_groupFolder == null || _groupFolder!.isEmpty) {
      return EmptyState(
        icon: Icons.schedule,
        message: tr('Chưa chọn agent', 'No agent selected'),
        hint: tr('Mở tab Chat và chọn một agent trước để quản lý lịch trình',
            'Open the Chat tab and pick an agent first to manage schedules'),
      );
    }
    if (_error != null) return ErrorState(message: _error!, onRetry: _load);
    if (_schedules.isEmpty) {
      return EmptyState(
        icon: Icons.event_repeat,
        message: tr('Chưa có lịch trình', 'No schedules yet'),
        hint: tr('Nhấn + để tạo tác vụ định kỳ cho agent',
            'Tap + to create a recurring task for the agent'),
      );
    }
    final c = context.colors;
    return RefreshIndicator(
      onRefresh: _load,
      color: c.accent,
      backgroundColor: c.surface,
      child: ListView.builder(
        padding: const EdgeInsets.fromLTRB(12, 12, 12, 88),
        itemCount: _schedules.length,
        itemBuilder: (ctx, i) => _scheduleCard(_schedules[i]),
      ),
    );
  }

  Widget _scheduleCard(SpaceSchedule s) {
    final c = context.colors;
    final active = s.status == 'active' || s.status == 'pending';
    return Card(
      color: c.surfaceAlt,
      margin: const EdgeInsets.only(bottom: 10),
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(12),
        side: BorderSide(color: c.border),
      ),
      child: ListTile(
        leading: Icon(
          active ? Icons.alarm_on : Icons.alarm_off,
          color: active ? AppTokens.success : c.textMuted,
        ),
        title: Text(s.prompt,
            style: TextStyle(color: c.textPrimary, fontSize: 14),
            maxLines: 2,
            overflow: TextOverflow.ellipsis),
        subtitle: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            const SizedBox(height: 4),
            Text(
              '${s.scheduleType} · ${s.scheduleValue}',
              style: const TextStyle(
                  color: AppTokens.cyan,
                  fontSize: 11,
                  fontFamily: 'monospace'),
            ),
            if (s.nextRun != null && s.nextRun!.isNotEmpty)
              Text(tr('Lần tới: ${s.nextRun}', 'Next run: ${s.nextRun}'),
                  style: TextStyle(color: c.textMuted, fontSize: 11)),
          ],
        ),
        trailing: active
            ? IconButton(
                icon: const Icon(Icons.cancel_outlined,
                    color: AppTokens.danger, size: 20),
                onPressed: () => _cancel(s),
              )
            : Text(s.status,
                style: TextStyle(color: c.textMuted, fontSize: 11)),
      ),
    );
  }
}
