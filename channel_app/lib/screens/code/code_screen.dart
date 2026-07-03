import 'dart:async';
import 'package:flutter/material.dart';
import '../../models/code_models.dart';
import '../../services/code_api.dart';
import '../../services/relay_manager.dart';
import '../../theme/tokens.dart';
import '../../widgets/states.dart';
import 'code_session_screen.dart';
import 'folder_picker.dart';

/// Code remote: lists git-backed code sessions and opens them.
/// Backed by `/api/code/sessions` over the relay tunnel.
class CodeScreen extends StatefulWidget {
  const CodeScreen({super.key});

  @override
  State<CodeScreen> createState() => _CodeScreenState();
}

class _CodeScreenState extends State<CodeScreen> {
  final _api = CodeApi();

  List<CodeSession> _sessions = [];
  bool _loading = true;
  String? _error;
  bool _loadedOnce = false;

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
    var fresh = false;
    // Local-DB paint races the relay fetch in parallel — the relay result
    // always wins once it arrives.
    if (_sessions.isEmpty && !_loadedOnce) {
      unawaited(_api.listSessionsCached().then((cached) {
        if (fresh || cached.isEmpty || !mounted) return;
        if (_sessions.isNotEmpty) return;
        setState(() {
          _sessions = cached;
          _loading = false;
          _loadedOnce = true;
          _error = null;
        });
      }));
    }
    try {
      final sessions = await _api.listSessions();
      fresh = true;
      if (!mounted) return;
      setState(() {
        _sessions = sessions;
        _loading = false;
        _loadedOnce = true;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        // Keep the cached view usable when the refresh fails.
        _error = _sessions.isEmpty ? '$e' : null;
        _loading = false;
        _loadedOnce = true;
      });
    }
  }

  Future<void> _openCreateDialog() async {
    final created = await showModalBottomSheet<bool>(
      context: context,
      isScrollControlled: true,
      backgroundColor: context.colors.surface,
      shape: const RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(top: Radius.circular(18)),
      ),
      builder: (_) => const _CreateSessionSheet(),
    );
    if (created == true) _load();
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Scaffold(
      backgroundColor: c.bg,
      appBar: AppBar(
        backgroundColor: c.surface,
        elevation: 0,
        title: Row(
          children: [
            Text('Code', style: TextStyle(color: c.textPrimary)),
            const SizedBox(width: 8),
            AnimatedBuilder(
              animation: RelayManager(),
              builder: (_, _) =>
                  ConnectionDot(connected: RelayManager().connected),
            ),
          ],
        ),
        actions: [
          IconButton(
            icon: Icon(Icons.refresh, color: c.textSecondary),
            onPressed: _loading ? null : _load,
          ),
        ],
      ),
      floatingActionButton: FloatingActionButton.extended(
        onPressed: _openCreateDialog,
        backgroundColor: c.accent,
        foregroundColor: Colors.white,
        icon: const Icon(Icons.add),
        label: const Text('Session'),
      ),
      body: Container(
        decoration: BoxDecoration(color: c.bg),
        child: _buildBody(),
      ),
    );
  }

  Widget _buildBody() {
    if (_loading && !_loadedOnce) {
      return const LoadingState(text: 'Đang tải sessions…');
    }
    if (_error != null && _sessions.isEmpty) {
      return ErrorState(message: _error!, onRetry: _load);
    }
    if (_sessions.isEmpty) {
      return EmptyState(
        icon: Icons.code_off,
        message: 'Chưa có code session',
        hint: 'Nhấn + để tạo session từ một thư mục dự án',
      );
    }
    final c = context.colors;
    return RefreshIndicator(
      onRefresh: _load,
      color: c.accent,
      backgroundColor: c.surface,
      child: ListView.builder(
        padding: const EdgeInsets.fromLTRB(12, 12, 12, 88),
        itemCount: _sessions.length,
        itemBuilder: (ctx, i) => _sessionCard(_sessions[i]),
      ),
    );
  }

  Widget _sessionCard(CodeSession s) {
    final c = context.colors;
    return Card(
      color: c.surfaceAlt,
      margin: const EdgeInsets.only(bottom: 10),
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(14),
        side: BorderSide(color: c.border),
      ),
      child: ListTile(
        contentPadding: const EdgeInsets.symmetric(horizontal: 16, vertical: 6),
        leading: CircleAvatar(
          backgroundColor: c.accent.withValues(alpha: 0.15),
          child: Icon(Icons.folder_special, color: c.accent),
        ),
        title: Text(
          s.name,
          style: TextStyle(
            color: c.textPrimary,
            fontWeight: FontWeight.w600,
          ),
        ),
        subtitle: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            const SizedBox(height: 4),
            Text(
              s.workspace,
              style: TextStyle(color: c.textMuted, fontSize: 11),
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
            ),
            const SizedBox(height: 6),
            Row(
              children: [
                if (s.language != null && s.language!.isNotEmpty)
                  _chip(s.language!, AppTokens.cyan),
                if (s.gitEnabled) _chip('git', AppTokens.success),
                _chip(s.status, c.textMuted),
              ],
            ),
          ],
        ),
        trailing: Icon(Icons.chevron_right, color: c.textMuted),
        onTap: () {
          Navigator.push(
            context,
            MaterialPageRoute(builder: (_) => CodeSessionScreen(session: s)),
          );
        },
      ),
    );
  }

  Widget _chip(String label, Color color) {
    return Container(
      margin: const EdgeInsets.only(right: 6),
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
      decoration: BoxDecoration(
        color: color.withValues(alpha: 0.12),
        borderRadius: BorderRadius.circular(6),
        border: Border.all(color: color.withValues(alpha: 0.3)),
      ),
      child: Text(
        label,
        style: TextStyle(color: color, fontSize: 10, fontWeight: FontWeight.w500),
      ),
    );
  }
}

/// Bottom sheet to create a new code session.
class _CreateSessionSheet extends StatefulWidget {
  const _CreateSessionSheet();

  @override
  State<_CreateSessionSheet> createState() => _CreateSessionSheetState();
}

class _CreateSessionSheetState extends State<_CreateSessionSheet> {
  final _api = CodeApi();
  final _nameCtrl = TextEditingController();
  final _langCtrl = TextEditingController();
  String? _workspace;
  bool _initGit = false;
  bool _saving = false;
  String? _error;

  @override
  void dispose() {
    _nameCtrl.dispose();
    _langCtrl.dispose();
    super.dispose();
  }

  Future<void> _pickFolder() async {
    final path = await FolderPicker.show(context);
    if (path != null && mounted) {
      setState(() {
        _workspace = path;
        if (_nameCtrl.text.trim().isEmpty) {
          _nameCtrl.text = path.split('/').where((s) => s.isNotEmpty).last;
        }
      });
    }
  }

  Future<void> _create() async {
    final name = _nameCtrl.text.trim();
    final ws = _workspace;
    if (name.isEmpty || ws == null || ws.isEmpty) {
      setState(() => _error = 'Cần tên và thư mục dự án');
      return;
    }
    setState(() {
      _saving = true;
      _error = null;
    });
    try {
      await _api.createSession(
        name: name,
        workspace: ws,
        language: _langCtrl.text.trim(),
        initGit: _initGit,
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
        20,
        20,
        20,
        MediaQuery.of(context).viewInsets.bottom + 20,
      ),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            'Tạo Code Session',
            style: TextStyle(
              color: c.textPrimary,
              fontSize: 18,
              fontWeight: FontWeight.bold,
            ),
          ),
          const SizedBox(height: 18),
          _field(_nameCtrl, 'Tên session', Icons.label_outline),
          const SizedBox(height: 12),
          InkWell(
            onTap: _pickFolder,
            borderRadius: BorderRadius.circular(10),
            child: Container(
              padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 14),
              decoration: BoxDecoration(
                color: c.surfaceAlt,
                borderRadius: BorderRadius.circular(10),
                border: Border.all(color: c.border),
              ),
              child: Row(
                children: [
                  Icon(Icons.folder_open,
                      color: AppTokens.warning, size: 20),
                  const SizedBox(width: 12),
                  Expanded(
                    child: Text(
                      _workspace ?? 'Chọn thư mục dự án…',
                      style: TextStyle(
                        color: _workspace == null
                            ? c.textMuted
                            : c.textPrimary,
                        fontSize: 13,
                      ),
                      overflow: TextOverflow.ellipsis,
                    ),
                  ),
                ],
              ),
            ),
          ),
          const SizedBox(height: 12),
          _field(_langCtrl, 'Ngôn ngữ (tuỳ chọn)', Icons.terminal),
          const SizedBox(height: 6),
          SwitchListTile(
            contentPadding: EdgeInsets.zero,
            value: _initGit,
            onChanged: (v) => setState(() => _initGit = v),
            activeThumbColor: c.accent,
            title: Text('Khởi tạo git',
                style: TextStyle(color: c.textSecondary, fontSize: 14)),
            subtitle: Text('Cho phép checkpoint & rollback',
                style: TextStyle(color: c.textMuted, fontSize: 12)),
          ),
          if (_error != null) ...[
            const SizedBox(height: 8),
            Text(_error!,
                style: TextStyle(color: AppTokens.danger, fontSize: 12)),
          ],
          const SizedBox(height: 16),
          SizedBox(
            width: double.infinity,
            child: ElevatedButton(
              onPressed: _saving ? null : _create,
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
                          strokeWidth: 2, color: Colors.white),
                    )
                  : const Text('Tạo session'),
            ),
          ),
        ],
      ),
    );
  }

  Widget _field(TextEditingController ctrl, String hint, IconData icon) {
    final c = context.colors;
    return TextField(
      controller: ctrl,
      style: TextStyle(color: c.textPrimary),
      decoration: InputDecoration(
        hintText: hint,
        hintStyle: TextStyle(color: c.textMuted),
        prefixIcon: Icon(icon, color: c.textMuted, size: 20),
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
      ),
    );
  }
}
