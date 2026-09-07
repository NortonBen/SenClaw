import 'package:flutter/material.dart';
import '../../models/kit_models.dart';
import '../../services/kits_api.dart';
import '../../services/language_service.dart';
import '../../theme/tokens.dart';
import '../../widgets/states.dart';

/// Preview-then-install for one kit.
///
/// The preview is re-fetched whenever a parameter changes, because the daemon
/// substitutes `{{param.<key>}}` **before** anything reaches disk and reports
/// an unsatisfied one as `paramError`. Installing past that error would fail
/// with the same message, so the button stays disabled until it clears.
class KitInstallSheet extends StatefulWidget {
  final KitsApi api;
  final AvailableKit kit;

  const KitInstallSheet({super.key, required this.api, required this.kit});

  @override
  State<KitInstallSheet> createState() => _KitInstallSheetState();
}

class _KitInstallSheetState extends State<KitInstallSheet> {
  KitPreview? _preview;
  String? _error;
  bool _loading = true;
  bool _installing = false;

  final _values = <String, dynamic>{};

  @override
  void initState() {
    super.initState();
    _loadPreview();
  }

  Future<void> _loadPreview() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final p = await widget.api.preview(
        sourceId: widget.kit.sourceId,
        name: widget.kit.name,
        params: _values,
      );
      if (!mounted) return;
      // Seed defaults once, on the first preview — re-seeding on every reload
      // would overwrite what the user just typed.
      if (_preview == null) {
        for (final param in p.params) {
          if (param.defaultValue != null && !_values.containsKey(param.key)) {
            _values[param.key] = param.type == 'boolean'
                ? param.defaultValue == 'true'
                : param.defaultValue;
          }
        }
      }
      setState(() {
        _preview = p;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = e.toString();
        _loading = false;
      });
    }
  }

  Future<void> _install() async {
    setState(() => _installing = true);
    try {
      final report = await widget.api.install(
        sourceId: widget.kit.sourceId,
        name: widget.kit.name,
        params: _values,
        // A reinstall over an existing kit needs force, or the app half is
        // refused as already present.
        force: widget.kit.installed,
      );
      if (!mounted) return;
      Navigator.pop(context, report);
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _installing = false;
        _error = e.toString();
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return DraggableScrollableSheet(
      expand: false,
      initialChildSize: 0.8,
      maxChildSize: 0.95,
      builder: (_, controller) => Container(
        decoration: BoxDecoration(
          color: c.surface,
          borderRadius: const BorderRadius.vertical(top: Radius.circular(14)),
        ),
        child: Column(
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(16, 14, 8, 8),
              child: Row(
                children: [
                  Expanded(
                    child: Text(
                      widget.kit.name,
                      style: TextStyle(
                        color: c.textPrimary,
                        fontWeight: FontWeight.w700,
                        fontSize: 16,
                      ),
                    ),
                  ),
                  IconButton(
                    icon: Icon(Icons.close, color: c.textMuted),
                    onPressed: () => Navigator.pop(context),
                  ),
                ],
              ),
            ),
            Expanded(
              child: _loading
                  ? const LoadingState()
                  : _preview == null
                      ? ErrorState(
                          message: _error ?? 'preview failed',
                          onRetry: _loadPreview,
                        )
                      : _body(c, controller, _preview!),
            ),
            if (_preview != null) _footer(c, _preview!),
          ],
        ),
      ),
    );
  }

  Widget _body(
    AppColors c,
    ScrollController controller,
    KitPreview p,
  ) =>
      ListView(
        controller: controller,
        padding: const EdgeInsets.fromLTRB(16, 0, 16, 16),
        children: [
          if (p.description.isNotEmpty)
            Text(
              p.description,
              style:
                  TextStyle(color: c.textSecondary, fontSize: 13, height: 1.45),
            ),
          if (_error != null) ...[
            const SizedBox(height: 12),
            _banner(c, _error!, AppTokens.danger),
          ],
          if (p.paramError != null && p.paramError!.isNotEmpty) ...[
            const SizedBox(height: 12),
            _banner(c, p.paramError!, AppTokens.warning),
          ],
          if (p.params.isNotEmpty) ...[
            const SizedBox(height: 16),
            _heading(c, tr('Tham số', 'Parameters')),
            for (final param in p.params) _paramField(c, param),
          ],
          const SizedBox(height: 16),
          _heading(c, tr('Sẽ cài (${p.items.length})', 'Installs (${p.items.length})')),
          for (final item in p.items) _itemRow(c, item),
        ],
      );

  Widget _footer(AppColors c, KitPreview p) {
    final blocked = (p.paramError ?? '').isNotEmpty;
    return SafeArea(
      top: false,
      child: Padding(
        padding: const EdgeInsets.fromLTRB(16, 8, 16, 12),
        child: SizedBox(
          width: double.infinity,
          child: FilledButton.icon(
            style: FilledButton.styleFrom(
              backgroundColor: c.accent,
              padding: const EdgeInsets.symmetric(vertical: 13),
            ),
            onPressed: _installing || blocked ? null : _install,
            icon: _installing
                ? const SizedBox(
                    width: 16,
                    height: 16,
                    child: CircularProgressIndicator(
                        strokeWidth: 2, color: Colors.white),
                  )
                : const Icon(Icons.download, size: 18),
            label: Text(
              _installing
                  ? tr('Đang cài…', 'Installing…')
                  : widget.kit.installed
                      ? tr('Cài lại v${p.version}', 'Reinstall v${p.version}')
                      : tr('Cài v${p.version}', 'Install v${p.version}'),
            ),
          ),
        ),
      ),
    );
  }

  Widget _paramField(AppColors c, KitParam param) {
    if (param.type == 'boolean') {
      return SwitchListTile(
        contentPadding: EdgeInsets.zero,
        activeThumbColor: c.accent,
        title: Text(param.title,
            style: TextStyle(color: c.textPrimary, fontSize: 13.5)),
        subtitle: param.description.isEmpty
            ? null
            : Text(param.description,
                style: TextStyle(color: c.textMuted, fontSize: 11.5)),
        value: _values[param.key] == true,
        onChanged: (v) {
          setState(() => _values[param.key] = v);
          _loadPreview();
        },
      );
    }

    if (param.type == 'select' && param.options.isNotEmpty) {
      return Padding(
        padding: const EdgeInsets.only(top: 10),
        child: InputDecorator(
          decoration: _decoration(c, param),
          child: DropdownButtonHideUnderline(
            child: DropdownButton<String>(
              isExpanded: true,
              isDense: true,
              dropdownColor: c.surface,
              value: _values[param.key]?.toString(),
              hint: Text(tr('Chọn…', 'Choose…'),
                  style: TextStyle(color: c.textMuted, fontSize: 13)),
              style: TextStyle(color: c.textPrimary, fontSize: 13.5),
              items: [
                for (final o in param.options)
                  DropdownMenuItem(value: o.key, child: Text(o.value)),
              ],
              onChanged: (v) {
                setState(() => _values[param.key] = v);
                _loadPreview();
              },
            ),
          ),
        ),
      );
    }

    return Padding(
      padding: const EdgeInsets.only(top: 10),
      child: TextFormField(
        initialValue: _values[param.key]?.toString() ?? '',
        obscureText: param.secret,
        keyboardType:
            param.type == 'number' ? TextInputType.number : TextInputType.text,
        style: TextStyle(color: c.textPrimary, fontSize: 13.5),
        decoration: _decoration(c, param),
        onChanged: (v) => _values[param.key] = v,
        // Re-preview on blur, not per keystroke: each one is a relay round-trip
        // that re-fetches the kit's artifact.
        onFieldSubmitted: (_) => _loadPreview(),
      ),
    );
  }

  InputDecoration _decoration(AppColors c, KitParam param) => InputDecoration(
        isDense: true,
        labelText: param.required ? '${param.title} *' : param.title,
        labelStyle: TextStyle(color: c.textMuted, fontSize: 12.5),
        hintText: param.placeholder.isEmpty ? null : param.placeholder,
        helperText: param.description.isEmpty ? null : param.description,
        helperStyle: TextStyle(color: c.textMuted, fontSize: 11),
        helperMaxLines: 3,
        filled: true,
        fillColor: c.surfaceAlt,
        border: OutlineInputBorder(
          borderRadius: BorderRadius.circular(AppTokens.rMd),
          borderSide: BorderSide(color: c.border),
        ),
        enabledBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(AppTokens.rMd),
          borderSide: BorderSide(color: c.border),
        ),
      );

  Widget _itemRow(AppColors c, KitPreviewItem item) {
    final icon = switch (item.type) {
      'agent' => Icons.smart_toy_outlined,
      'skill' => Icons.extension_outlined,
      'workflow' => Icons.account_tree_outlined,
      'hook' => Icons.link,
      'job' => Icons.schedule,
      'app' => Icons.apps_outlined,
      _ => Icons.settings_ethernet,
    };
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 5),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Icon(icon,
              size: 16,
              color: item.unsupported ? c.textMuted : c.textSecondary),
          const SizedBox(width: 9),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  item.name,
                  style: TextStyle(
                    color: item.unsupported ? c.textMuted : c.textPrimary,
                    fontSize: 13,
                  ),
                ),
                if ((item.description ?? '').isNotEmpty)
                  Text(
                    item.description!,
                    maxLines: 2,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(color: c.textMuted, fontSize: 11.5),
                  ),
                if (item.cron != null)
                  Text(
                    // A job installed enabled starts spending tokens before
                    // anyone reads what it does — say which it is.
                    item.enabled == false
                        ? tr('${item.cron} · cài ở trạng thái tạm dừng',
                            '${item.cron} · installs paused')
                        : tr('${item.cron} · chạy ngay sau khi cài',
                            '${item.cron} · runs from install'),
                    style: TextStyle(
                      color: item.enabled == false ? c.textMuted : AppTokens.warning,
                      fontSize: 11.5,
                    ),
                  ),
                if (item.unsupported)
                  Text(
                    tr('Daemon không cài mục này', 'The daemon does not install this'),
                    style: TextStyle(color: c.textMuted, fontSize: 11),
                  ),
              ],
            ),
          ),
          Text(item.type,
              style: TextStyle(color: c.textMuted, fontSize: 10.5)),
        ],
      ),
    );
  }

  Widget _heading(AppColors c, String text) => Padding(
        padding: const EdgeInsets.only(bottom: 6),
        child: Text(
          text.toUpperCase(),
          style: TextStyle(
            color: c.textMuted,
            fontSize: 11,
            fontWeight: FontWeight.w600,
            letterSpacing: 0.6,
          ),
        ),
      );

  Widget _banner(AppColors c, String message, Color color) => Container(
        padding: const EdgeInsets.all(11),
        decoration: BoxDecoration(
          color: color.withValues(alpha: 0.08),
          borderRadius: BorderRadius.circular(AppTokens.rMd),
          border: Border.all(color: color.withValues(alpha: 0.4)),
        ),
        child: Text(message,
            style: TextStyle(color: c.textPrimary, fontSize: 12.5)),
      );
}
