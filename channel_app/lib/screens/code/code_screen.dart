import 'package:flutter/material.dart';
import '../../models/code_models.dart';
import '../../services/code_api.dart';
import '../../services/relay_manager.dart';
import '../../theme/app_colors.dart';
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
    try {
      final sessions = await _api.listSessions();
      if (!mounted) return;
      setState(() {
        _sessions = sessions;
        _loading = false;
        _loadedOnce = true;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = '$e';
        _loading = false;
        _loadedOnce = true;
      });
    }
  }

  Future<void> _openCreateDialog() async {
    final created = await showModalBottomSheet<bool>(
      context: context,
      isScrollControlled: true,
      backgroundColor: AppColors.surface,
      shape: const RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(top: Radius.circular(18)),
      ),
      builder: (_) => const _CreateSessionSheet(),
    );
    if (created == true) _load();
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
            const Text('Code', style: TextStyle(color: Colors.white)),
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
            icon: const Icon(Icons.refresh, color: Colors.white54),
            onPressed: _loading ? null : _load,
          ),
        ],
      ),
      floatingActionButton: FloatingActionButton.extended(
        onPressed: _openCreateDialog,
        backgroundColor: AppColors.accent,
        foregroundColor: Colors.black,
        icon: const Icon(Icons.add),
        label: const Text('Session'),
      ),
      body: Container(
        decoration: AppColors.pageDecoration,
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
    return RefreshIndicator(
      onRefresh: _load,
      color: AppColors.accent,
      backgroundColor: AppColors.surface,
      child: ListView.builder(
        padding: const EdgeInsets.fromLTRB(12, 12, 12, 88),
        itemCount: _sessions.length,
        itemBuilder: (ctx, i) => _sessionCard(_sessions[i]),
      ),
    );
  }

  Widget _sessionCard(CodeSession s) {
    return Card(
      color: Colors.white.withValues(alpha: 0.04),
      margin: const EdgeInsets.only(bottom: 10),
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(14),
        side: const BorderSide(color: AppColors.cardBorder),
      ),
      child: ListTile(
        contentPadding: const EdgeInsets.symmetric(horizontal: 16, vertical: 6),
        leading: CircleAvatar(
          backgroundColor: AppColors.accent.withValues(alpha: 0.15),
          child: const Icon(Icons.folder_special, color: AppColors.accent),
        ),
        title: Text(
          s.name,
          style: const TextStyle(
            color: Colors.white,
            fontWeight: FontWeight.w600,
          ),
        ),
        subtitle: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            const SizedBox(height: 4),
            Text(
              s.workspace,
              style: const TextStyle(color: Colors.white38, fontSize: 11),
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
            ),
            const SizedBox(height: 6),
            Row(
              children: [
                if (s.language != null && s.language!.isNotEmpty)
                  _chip(s.language!, AppColors.cyan),
                if (s.gitEnabled) _chip('git', const Color(0xFF66BB6A)),
                _chip(s.status, Colors.white38),
              ],
            ),
          ],
        ),
        trailing: const Icon(Icons.chevron_right, color: Colors.white38),
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
          const Text(
            'Tạo Code Session',
            style: TextStyle(
              color: Colors.white,
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
                color: Colors.white.withValues(alpha: 0.05),
                borderRadius: BorderRadius.circular(10),
                border: Border.all(color: AppColors.cardBorder),
              ),
              child: Row(
                children: [
                  const Icon(Icons.folder_open,
                      color: Color(0xFFFFB74D), size: 20),
                  const SizedBox(width: 12),
                  Expanded(
                    child: Text(
                      _workspace ?? 'Chọn thư mục dự án…',
                      style: TextStyle(
                        color: _workspace == null
                            ? Colors.white38
                            : Colors.white,
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
            activeColor: AppColors.accent,
            title: const Text('Khởi tạo git',
                style: TextStyle(color: Colors.white70, fontSize: 14)),
            subtitle: const Text('Cho phép checkpoint & rollback',
                style: TextStyle(color: Colors.white38, fontSize: 12)),
          ),
          if (_error != null) ...[
            const SizedBox(height: 8),
            Text(_error!,
                style: const TextStyle(color: Colors.redAccent, fontSize: 12)),
          ],
          const SizedBox(height: 16),
          SizedBox(
            width: double.infinity,
            child: ElevatedButton(
              onPressed: _saving ? null : _create,
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
                          strokeWidth: 2, color: Colors.black),
                    )
                  : const Text('Tạo session'),
            ),
          ),
        ],
      ),
    );
  }

  Widget _field(TextEditingController c, String hint, IconData icon) {
    return TextField(
      controller: c,
      style: const TextStyle(color: Colors.white),
      decoration: InputDecoration(
        hintText: hint,
        hintStyle: const TextStyle(color: Colors.white38),
        prefixIcon: Icon(icon, color: Colors.white38, size: 20),
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
