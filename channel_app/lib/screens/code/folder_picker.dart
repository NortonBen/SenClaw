import 'package:flutter/material.dart';
import '../../models/code_models.dart';
import '../../services/code_api.dart';
import '../../theme/tokens.dart';
import '../../widgets/states.dart';

/// Server-side directory browser backed by `/api/fs/ls`. Returns the chosen
/// absolute path, or null if cancelled.
class FolderPicker extends StatefulWidget {
  const FolderPicker({super.key});

  static Future<String?> show(BuildContext context) {
    return showModalBottomSheet<String>(
      context: context,
      isScrollControlled: true,
      backgroundColor: context.colors.surface,
      shape: const RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(top: Radius.circular(18)),
      ),
      builder: (_) => const FractionallySizedBox(
        heightFactor: 0.85,
        child: FolderPicker(),
      ),
    );
  }

  @override
  State<FolderPicker> createState() => _FolderPickerState();
}

class _FolderPickerState extends State<FolderPicker> {
  final _api = CodeApi();
  FsListing? _listing;
  bool _loading = true;
  String? _error;

  @override
  void initState() {
    super.initState();
    _load(null);
  }

  Future<void> _load(String? path) async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final listing = await _api.fsLs(path: path);
      if (!mounted) return;
      setState(() {
        _listing = listing;
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

  /// Codex-style "start from scratch" — create a new folder under the current
  /// directory and step into it.
  Future<void> _createFolder() async {
    final current = _listing?.current;
    if (current == null) return;
    final c = context.colors;
    final ctrl = TextEditingController();
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: c.surface,
        title: Text('Thư mục dự án mới',
            style: TextStyle(color: c.textPrimary)),
        content: TextField(
          controller: ctrl,
          autofocus: true,
          style: TextStyle(color: c.textPrimary),
          decoration: InputDecoration(
            labelText: 'Tên thư mục',
            labelStyle: TextStyle(color: c.textSecondary),
          ),
        ),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(ctx, false),
              child: const Text('Huỷ')),
          TextButton(
              onPressed: () => Navigator.pop(ctx, true),
              child:
                  Text('Tạo', style: TextStyle(color: c.accent))),
        ],
      ),
    );
    if (ok != true || ctrl.text.trim().isEmpty) return;
    final sep = current.endsWith('/') ? '' : '/';
    final target = '$current$sep${ctrl.text.trim()}';
    try {
      final created = await _api.workspaceMkdir(target);
      await _load(created);
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Lỗi tạo thư mục: $e')));
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final listing = _listing;
    return Column(
      children: [
        const SizedBox(height: 10),
        Container(
          width: 40,
          height: 4,
          decoration: BoxDecoration(
            color: c.borderStrong,
            borderRadius: BorderRadius.circular(2),
          ),
        ),
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 14, 16, 8),
          child: Row(
            children: [
              Icon(Icons.folder_open, color: c.accent, size: 20),
              const SizedBox(width: 8),
              Expanded(
                child: Text(
                  listing?.current ?? 'Chọn thư mục',
                  style: TextStyle(color: c.textPrimary, fontSize: 13),
                  overflow: TextOverflow.ellipsis,
                ),
              ),
              if (listing != null)
                IconButton(
                  tooltip: 'Tạo thư mục mới',
                  icon: Icon(Icons.create_new_folder_outlined,
                      color: c.accent, size: 20),
                  onPressed: _createFolder,
                ),
            ],
          ),
        ),
        Divider(color: c.border, height: 1),
        Expanded(
          child: _loading
              ? const LoadingState()
              : _error != null
                  ? ErrorState(message: _error!, onRetry: () => _load(null))
                  : ListView(
                      children: [
                        if (listing?.parent != null)
                          ListTile(
                            leading: Icon(Icons.arrow_upward,
                                color: c.textSecondary),
                            title: Text('..',
                                style: TextStyle(color: c.textSecondary)),
                            onTap: () => _load(listing!.parent),
                          ),
                        ...?listing?.dirs.map(
                          (d) => ListTile(
                            leading: Icon(Icons.folder,
                                color: AppTokens.warning),
                            title: Text(d.name,
                                style:
                                    TextStyle(color: c.textSecondary)),
                            onTap: () => _load(d.path),
                          ),
                        ),
                      ],
                    ),
        ),
        SafeArea(
          top: false,
          child: Padding(
            padding: const EdgeInsets.all(12),
            child: SizedBox(
              width: double.infinity,
              child: ElevatedButton.icon(
                onPressed: listing == null
                    ? null
                    : () => Navigator.pop(context, listing.current),
                icon: const Icon(Icons.check),
                label: const Text('Chọn thư mục này'),
                style: ElevatedButton.styleFrom(
                  backgroundColor: c.accent,
                  foregroundColor: Colors.white,
                  padding: const EdgeInsets.symmetric(vertical: 14),
                ),
              ),
            ),
          ),
        ),
      ],
    );
  }
}
