import 'package:flutter/material.dart';
import '../../models/cowork_models.dart';
import '../../services/cowork_api.dart';
import '../../services/relay_manager.dart';
import '../../theme/app_colors.dart';
import '../../widgets/states.dart';
import 'cowork_workspace_screen.dart';

/// Cowork: lists multi-agent workspaces and opens them.
/// Backed by `/api/cowork/workspaces` over the relay tunnel.
class CoworkScreen extends StatefulWidget {
  const CoworkScreen({super.key});

  @override
  State<CoworkScreen> createState() => _CoworkScreenState();
}

class _CoworkScreenState extends State<CoworkScreen> {
  final _api = CoworkApi();
  List<CoworkWorkspace> _workspaces = [];
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
      final ws = await _api.listWorkspaces();
      if (!mounted) return;
      setState(() {
        _workspaces = ws;
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

  Future<void> _create() async {
    final nameCtrl = TextEditingController();
    final descCtrl = TextEditingController();
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: AppColors.surface,
        title: const Text('Tạo workspace',
            style: TextStyle(color: Colors.white)),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            TextField(
              controller: nameCtrl,
              style: const TextStyle(color: Colors.white),
              decoration: const InputDecoration(
                labelText: 'Tên',
                labelStyle: TextStyle(color: Colors.white54),
              ),
            ),
            TextField(
              controller: descCtrl,
              style: const TextStyle(color: Colors.white),
              decoration: const InputDecoration(
                labelText: 'Mô tả (tuỳ chọn)',
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
    final name = nameCtrl.text.trim();
    if (name.isEmpty) return;
    try {
      await _api.createWorkspace(
        name: name,
        description: descCtrl.text.trim(),
      );
      _load();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Lỗi tạo: $e')));
      }
    }
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
            const Text('Cowork', style: TextStyle(color: Colors.white)),
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
        onPressed: _create,
        backgroundColor: AppColors.accent,
        foregroundColor: Colors.black,
        icon: const Icon(Icons.add),
        label: const Text('Workspace'),
      ),
      body: Container(
        decoration: AppColors.pageDecoration,
        child: _buildBody(),
      ),
    );
  }

  Widget _buildBody() {
    if (_loading && !_loadedOnce) {
      return const LoadingState(text: 'Đang tải workspaces…');
    }
    if (_error != null && _workspaces.isEmpty) {
      return ErrorState(message: _error!, onRetry: _load);
    }
    if (_workspaces.isEmpty) {
      return const EmptyState(
        icon: Icons.groups_outlined,
        message: 'Chưa có workspace',
        hint: 'Nhấn + để tạo không gian cộng tác đa-agent',
      );
    }
    return RefreshIndicator(
      onRefresh: _load,
      color: AppColors.accent,
      backgroundColor: AppColors.surface,
      child: ListView.builder(
        padding: const EdgeInsets.fromLTRB(12, 12, 12, 88),
        itemCount: _workspaces.length,
        itemBuilder: (ctx, i) => _wsCard(_workspaces[i]),
      ),
    );
  }

  Widget _wsCard(CoworkWorkspace w) {
    return Card(
      color: Colors.white.withValues(alpha: 0.04),
      margin: const EdgeInsets.only(bottom: 10),
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(14),
        side: const BorderSide(color: AppColors.cardBorder),
      ),
      child: ListTile(
        contentPadding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
        leading: CircleAvatar(
          backgroundColor: AppColors.accent.withValues(alpha: 0.15),
          child: const Icon(Icons.workspaces_outline, color: AppColors.accent),
        ),
        title: Text(w.name,
            style: const TextStyle(
                color: Colors.white, fontWeight: FontWeight.w600)),
        subtitle: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            if (w.description != null && w.description!.isNotEmpty) ...[
              const SizedBox(height: 4),
              Text(w.description!,
                  style: const TextStyle(color: Colors.white54, fontSize: 12),
                  maxLines: 2,
                  overflow: TextOverflow.ellipsis),
            ],
            const SizedBox(height: 6),
            Container(
              padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
              decoration: BoxDecoration(
                color: Colors.white.withValues(alpha: 0.06),
                borderRadius: BorderRadius.circular(6),
              ),
              child: Text(w.status,
                  style: const TextStyle(color: Colors.white54, fontSize: 10)),
            ),
          ],
        ),
        trailing: const Icon(Icons.chevron_right, color: Colors.white38),
        onTap: () => Navigator.push(
          context,
          MaterialPageRoute(
              builder: (_) => CoworkWorkspaceScreen(workspace: w)),
        ),
      ),
    );
  }
}
