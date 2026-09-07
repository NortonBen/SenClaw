import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import '../../models/api_models.dart';
import '../../models/pattern_models.dart';
import '../../services/language_service.dart';
import '../../services/llm_api.dart';
import '../../services/patterns_api.dart';
import '../../theme/tokens.dart';
import '../../widgets/markdown_text.dart';
import '../../widgets/states.dart';

/// Run one pattern: paste text, pick options, get transformed text back.
///
/// The result is markdown far more often than not (Fabric patterns emit
/// headed sections), so it renders through the app's own [MarkdownText]
/// rather than as a flat string.
class PatternRunScreen extends StatefulWidget {
  final String name;
  final List<PatternStrategy> strategies;

  const PatternRunScreen({
    super.key,
    required this.name,
    this.strategies = const [],
  });

  @override
  State<PatternRunScreen> createState() => _PatternRunScreenState();
}

class _PatternRunScreenState extends State<PatternRunScreen> {
  final _api = PatternsApi();
  final _input = TextEditingController();

  PatternFiles? _files;
  String? _loadError;

  String? _strategy;

  /// `auto` follows the input's language. This is the default on purpose:
  /// Fabric patterns pin **English** in their own `# OUTPUT INSTRUCTIONS`, so
  /// Vietnamese input comes back in English unless the language rule is
  /// appended at render time.
  String _language = 'auto';
  String? _profile;
  List<LlmOption> _profiles = const [];

  bool _running = false;
  PatternRunResult? _result;
  String? _runError;

  @override
  void initState() {
    super.initState();
    _loadPattern();
    _loadProfiles();
  }

  @override
  void dispose() {
    _input.dispose();
    super.dispose();
  }

  Future<void> _loadPattern() async {
    try {
      final f = await _api.get(widget.name);
      if (!mounted) return;
      setState(() => _files = f);
    } catch (e) {
      if (!mounted) return;
      setState(() => _loadError = e.toString());
    }
  }

  /// Best-effort: the model picker is a convenience, and a daemon that cannot
  /// list configs must not block a run on the active model.
  Future<void> _loadProfiles() async {
    try {
      final list = await LlmApi().list();
      if (!mounted) return;
      setState(() => _profiles = list.configs);
    } catch (_) {}
  }

  Future<void> _run({bool dryRun = false}) async {
    setState(() {
      _running = true;
      _runError = null;
      _result = null;
    });
    try {
      final r = await _api.run(
        widget.name,
        input: _input.text,
        strategy: _strategy,
        language: _language == 'off' ? null : _language,
        profile: _profile,
        dryRun: dryRun,
      );
      if (!mounted) return;
      setState(() {
        _result = r;
        _running = false;
      });
    } on ApiException catch (e) {
      if (!mounted) return;
      setState(() {
        _runError = e.message;
        _running = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _runError = e.toString();
        _running = false;
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Scaffold(
      backgroundColor: c.bg,
      appBar: AppBar(
        backgroundColor: c.surface,
        elevation: 0,
        title: Text(widget.name, style: TextStyle(color: c.textPrimary)),
        actions: [
          IconButton(
            tooltip: tr('Xem prompt', 'View prompt'),
            icon: Icon(Icons.description_outlined, color: c.textSecondary),
            onPressed: _files == null ? null : _showPrompt,
          ),
        ],
      ),
      body: ListView(
        padding: const EdgeInsets.fromLTRB(12, 12, 12, 32),
        children: [
          if (_loadError != null)
            ErrorState(message: _loadError!, onRetry: _loadPattern)
          else ...[
            if (_files != null && _files!.system.isNotEmpty)
              _descriptionCard(c),
            const SizedBox(height: 10),
            _inputField(c),
            const SizedBox(height: 10),
            _options(c),
            const SizedBox(height: 12),
            _runRow(c),
            if (_runError != null) ...[
              const SizedBox(height: 12),
              _errorBanner(c, _runError!),
            ],
            if (_result != null) ...[
              const SizedBox(height: 16),
              _resultCard(c, _result!),
            ],
          ],
        ],
      ),
    );
  }

  Widget _descriptionCard(AppColors c) {
    final first = _files!.system
        .split('\n')
        .map((l) => l.trim())
        .firstWhere(
          (l) => l.isNotEmpty && !l.startsWith('#'),
          orElse: () => '',
        );
    if (first.isEmpty) return const SizedBox.shrink();
    return Container(
      padding: const EdgeInsets.all(12),
      decoration: BoxDecoration(
        color: c.surfaceAlt,
        borderRadius: BorderRadius.circular(AppTokens.rMd),
        border: Border.all(color: c.border),
      ),
      child: Text(
        first,
        style: TextStyle(color: c.textSecondary, fontSize: 12.5, height: 1.45),
      ),
    );
  }

  Widget _inputField(AppColors c) => TextField(
        controller: _input,
        maxLines: 10,
        minLines: 5,
        style: TextStyle(color: c.textPrimary, fontSize: 14),
        decoration: InputDecoration(
          hintText: tr(
            'Dán nội dung cần xử lý…',
            'Paste the text to transform…',
          ),
          hintStyle: TextStyle(color: c.textMuted, fontSize: 14),
          alignLabelWithHint: true,
          filled: true,
          fillColor: c.surface,
          border: OutlineInputBorder(
            borderRadius: BorderRadius.circular(AppTokens.rMd),
            borderSide: BorderSide(color: c.border),
          ),
          enabledBorder: OutlineInputBorder(
            borderRadius: BorderRadius.circular(AppTokens.rMd),
            borderSide: BorderSide(color: c.border),
          ),
        ),
      );

  Widget _options(AppColors c) => Wrap(
        spacing: 8,
        runSpacing: 8,
        children: [
          if (widget.strategies.isNotEmpty)
            _dropdown<String?>(
              c,
              label: tr('Chiến lược', 'Strategy'),
              value: _strategy,
              items: [
                DropdownMenuItem(
                  value: null,
                  child: Text(tr('Không', 'None')),
                ),
                for (final s in widget.strategies)
                  DropdownMenuItem(value: s.name, child: Text(s.name)),
              ],
              onChanged: (v) => setState(() => _strategy = v),
            ),
          _dropdown<String>(
            c,
            label: tr('Ngôn ngữ', 'Language'),
            value: _language,
            items: [
              DropdownMenuItem(
                  value: 'auto', child: Text(tr('Theo đầu vào', 'Auto'))),
              DropdownMenuItem(
                  value: 'Vietnamese', child: Text(tr('Tiếng Việt', 'Vietnamese'))),
              DropdownMenuItem(value: 'English', child: Text('English')),
              DropdownMenuItem(
                  value: 'off',
                  child: Text(tr('Theo pattern', 'Pattern decides'))),
            ],
            onChanged: (v) => setState(() => _language = v ?? 'auto'),
          ),
          if (_profiles.isNotEmpty)
            _dropdown<String?>(
              c,
              label: tr('Mô hình', 'Model'),
              value: _profile,
              items: [
                DropdownMenuItem(
                  value: null,
                  child: Text(tr('Đang dùng', 'Active')),
                ),
                for (final p in _profiles)
                  DropdownMenuItem(value: p.id, child: Text(p.label)),
              ],
              onChanged: (v) => setState(() => _profile = v),
            ),
        ],
      );

  Widget _dropdown<T>(
    AppColors c, {
    required String label,
    required T value,
    required List<DropdownMenuItem<T>> items,
    required ValueChanged<T?> onChanged,
  }) =>
      Container(
        padding: const EdgeInsets.symmetric(horizontal: 10),
        decoration: BoxDecoration(
          color: c.surface,
          borderRadius: BorderRadius.circular(AppTokens.rMd),
          border: Border.all(color: c.border),
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text('$label: ',
                style: TextStyle(color: c.textMuted, fontSize: 12)),
            DropdownButton<T>(
              value: value,
              items: items,
              onChanged: onChanged,
              underline: const SizedBox.shrink(),
              isDense: true,
              dropdownColor: c.surface,
              style: TextStyle(color: c.textPrimary, fontSize: 12.5),
              icon: Icon(Icons.arrow_drop_down, size: 18, color: c.textMuted),
            ),
          ],
        ),
      );

  Widget _runRow(AppColors c) => Row(
        children: [
          Expanded(
            child: FilledButton.icon(
              style: FilledButton.styleFrom(backgroundColor: c.accent),
              onPressed: _running ? null : () => _run(),
              icon: _running
                  ? const SizedBox(
                      width: 16,
                      height: 16,
                      child: CircularProgressIndicator(
                          strokeWidth: 2, color: Colors.white),
                    )
                  : const Icon(Icons.play_arrow, size: 18),
              label: Text(_running
                  ? tr('Đang chạy…', 'Running…')
                  : tr('Chạy pattern', 'Run pattern')),
            ),
          ),
          const SizedBox(width: 8),
          // Render-only: the prompt comes back without an LLM call, which is
          // how you check a placeholder is filled without paying for a turn.
          OutlinedButton(
            style: OutlinedButton.styleFrom(
              foregroundColor: c.textSecondary,
              side: BorderSide(color: c.border),
            ),
            onPressed: _running ? null : () => _run(dryRun: true),
            child: Text(tr('Thử render', 'Dry run')),
          ),
        ],
      );

  Widget _errorBanner(AppColors c, String message) => Container(
        padding: const EdgeInsets.all(12),
        decoration: BoxDecoration(
          color: AppTokens.danger.withValues(alpha: 0.08),
          borderRadius: BorderRadius.circular(AppTokens.rMd),
          border: Border.all(color: AppTokens.danger.withValues(alpha: 0.4)),
        ),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            const Icon(Icons.error_outline, color: AppTokens.danger, size: 18),
            const SizedBox(width: 8),
            Expanded(
              child: Text(message,
                  style: TextStyle(color: c.textPrimary, fontSize: 12.5)),
            ),
          ],
        ),
      );

  Widget _resultCard(AppColors c, PatternRunResult r) {
    final body = r.dryRun ? r.renderedSystem : r.text;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          children: [
            Text(
              r.dryRun
                  ? tr('Prompt đã render', 'Rendered prompt')
                  : tr('Kết quả', 'Result'),
              style: TextStyle(
                  color: c.textPrimary,
                  fontWeight: FontWeight.w600,
                  fontSize: 14),
            ),
            const Spacer(),
            if (!r.dryRun && r.model.isNotEmpty)
              Text(
                '${r.model} · ${r.latencyMs}ms',
                style: TextStyle(color: c.textMuted, fontSize: 11.5),
              ),
            IconButton(
              tooltip: tr('Sao chép', 'Copy'),
              icon: Icon(Icons.copy, size: 18, color: c.textSecondary),
              onPressed: () {
                Clipboard.setData(ClipboardData(text: body));
                ScaffoldMessenger.of(context).showSnackBar(
                  SnackBar(content: Text(tr('Đã sao chép', 'Copied'))),
                );
              },
            ),
          ],
        ),
        // An unfilled `{{placeholder}}` stays verbatim in the prompt rather
        // than being blanked, so this list is the only sign the pattern ran
        // with an instruction it could not complete.
        if (r.unresolved.isNotEmpty) ...[
          const SizedBox(height: 6),
          Text(
            tr(
              'Biến chưa điền: ${r.unresolved.join(", ")}',
              'Unfilled variables: ${r.unresolved.join(", ")}',
            ),
            style: const TextStyle(color: AppTokens.warning, fontSize: 11.5),
          ),
        ],
        const SizedBox(height: 8),
        Container(
          width: double.infinity,
          padding: const EdgeInsets.all(12),
          decoration: BoxDecoration(
            color: c.surface,
            borderRadius: BorderRadius.circular(AppTokens.rMd),
            border: Border.all(color: c.border),
          ),
          child: r.dryRun
              ? SelectableText(
                  body,
                  style: TextStyle(
                      color: c.textSecondary,
                      fontSize: 12,
                      fontFamily: 'monospace'),
                )
              : MarkdownText(body),
        ),
      ],
    );
  }

  void _showPrompt() {
    final c = context.colors;
    final f = _files!;
    showModalBottomSheet(
      context: context,
      backgroundColor: c.surface,
      isScrollControlled: true,
      shape: const RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(top: Radius.circular(14)),
      ),
      builder: (_) => DraggableScrollableSheet(
        expand: false,
        initialChildSize: 0.75,
        builder: (_, controller) => ListView(
          controller: controller,
          padding: const EdgeInsets.all(16),
          children: [
            Text('system.md',
                style: TextStyle(
                    color: c.textPrimary, fontWeight: FontWeight.w600)),
            const SizedBox(height: 8),
            SelectableText(
              f.system,
              style: TextStyle(
                  color: c.textSecondary, fontSize: 12, fontFamily: 'monospace'),
            ),
            if (f.user != null && f.user!.isNotEmpty) ...[
              const SizedBox(height: 18),
              Text('user.md',
                  style: TextStyle(
                      color: c.textPrimary, fontWeight: FontWeight.w600)),
              const SizedBox(height: 8),
              SelectableText(
                f.user!,
                style: TextStyle(
                    color: c.textSecondary,
                    fontSize: 12,
                    fontFamily: 'monospace'),
              ),
            ],
            const SizedBox(height: 18),
            Text(
              f.path,
              style: TextStyle(color: c.textMuted, fontSize: 11),
            ),
          ],
        ),
      ),
    );
  }
}
