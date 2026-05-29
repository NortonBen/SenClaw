import 'package:flutter/material.dart';
import '../../models/code_models.dart';
import '../../services/code_api.dart';
import '../../theme/app_colors.dart';
import '../../widgets/states.dart';

/// Server-side directory browser backed by `/api/fs/ls`. Returns the chosen
/// absolute path, or null if cancelled.
class FolderPicker extends StatefulWidget {
  const FolderPicker({super.key});

  static Future<String?> show(BuildContext context) {
    return showModalBottomSheet<String>(
      context: context,
      isScrollControlled: true,
      backgroundColor: AppColors.surface,
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

  @override
  Widget build(BuildContext context) {
    final listing = _listing;
    return Column(
      children: [
        const SizedBox(height: 10),
        Container(
          width: 40,
          height: 4,
          decoration: BoxDecoration(
            color: Colors.white24,
            borderRadius: BorderRadius.circular(2),
          ),
        ),
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 14, 16, 8),
          child: Row(
            children: [
              const Icon(Icons.folder_open, color: AppColors.accent, size: 20),
              const SizedBox(width: 8),
              Expanded(
                child: Text(
                  listing?.current ?? 'Chọn thư mục',
                  style: const TextStyle(color: Colors.white, fontSize: 13),
                  overflow: TextOverflow.ellipsis,
                ),
              ),
            ],
          ),
        ),
        const Divider(color: AppColors.cardBorder, height: 1),
        Expanded(
          child: _loading
              ? const LoadingState()
              : _error != null
                  ? ErrorState(message: _error!, onRetry: () => _load(null))
                  : ListView(
                      children: [
                        if (listing?.parent != null)
                          ListTile(
                            leading: const Icon(Icons.arrow_upward,
                                color: Colors.white54),
                            title: const Text('..',
                                style: TextStyle(color: Colors.white70)),
                            onTap: () => _load(listing!.parent),
                          ),
                        ...?listing?.dirs.map(
                          (d) => ListTile(
                            leading: const Icon(Icons.folder,
                                color: Color(0xFFFFB74D)),
                            title: Text(d.name,
                                style:
                                    const TextStyle(color: Colors.white70)),
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
                  backgroundColor: AppColors.accent,
                  foregroundColor: Colors.black,
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
