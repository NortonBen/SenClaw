import 'dart:io' show File;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:intl/intl.dart';

import '../../core/daemon/app_shutdown.dart';
import '../../core/daemon/daemon_provider.dart';
import '../../core/i18n/l10n.dart';
import '../../core/transport/connection.dart';
import '../../theme/tokens.dart';
import '../space/space_providers.dart';
import 'screen_capture.dart';

/// Set to open the capture review sheet; null closes it. Written by the tray
/// handler in `app.dart`, watched by [CaptureReviewOverlay].
final pendingCaptureProvider = StateProvider<CaptureResult?>((ref) => null);

/// Set when macOS withheld Screen Recording — the user has to grant it in
/// System Settings before any capture can work.
final capturePermissionNeededProvider = StateProvider<bool>((ref) => false);

/// Set when a capture failed for a reason granting permission won't fix.
final captureErrorProvider = StateProvider<String?>((ref) => null);

/// Mounted once over the whole app (via `MaterialApp.builder`, alongside
/// `ReminderInteractionOverlay`). Renders nothing until a capture is pending.
class CaptureReviewOverlay extends ConsumerWidget {
  const CaptureReviewOverlay({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final permNeeded = ref.watch(capturePermissionNeededProvider);
    if (permNeeded) return const _PermissionCard();

    final err = ref.watch(captureErrorProvider);
    if (err != null) return _ErrorCard(message: err);

    final shot = ref.watch(pendingCaptureProvider);
    if (shot == null) return const SizedBox.shrink();
    // Key by the file so a second capture resets the form rather than
    // inheriting the previous shot's half-typed title.
    return _CaptureDialog(key: ValueKey(shot.name), shot: shot);
  }
}

/// Shared dim-barrier + card chrome, matching the reminder dialog's shape.
class _Scrim extends StatelessWidget {
  const _Scrim({required this.child, required this.onDismiss, this.maxWidth = 560});
  final Widget child;
  final VoidCallback onDismiss;
  final double maxWidth;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Positioned.fill(
      child: GestureDetector(
        onTap: onDismiss,
        child: Container(
          color: Colors.black.withValues(alpha: 0.55),
          alignment: Alignment.center,
          child: GestureDetector(
            onTap: () {}, // absorb taps inside the card
            child: ConstrainedBox(
              constraints: BoxConstraints(maxWidth: maxWidth, maxHeight: 720),
              // Material ancestor is REQUIRED: without one, every Text renders
              // with Flutter's debug double-yellow underline (these overlays
              // mount via MaterialApp.builder, above the Navigator's Material).
              child: Material(
                type: MaterialType.transparency,
                child: Container(
                  margin: const EdgeInsets.all(AppTokens.s24),
                  decoration: BoxDecoration(
                    color: c.surface,
                    border: Border.all(color: c.border),
                    borderRadius: BorderRadius.circular(AppTokens.rXl),
                  ),
                  child: child,
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class _PermissionCard extends ConsumerWidget {
  const _PermissionCard();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    void close() =>
        ref.read(capturePermissionNeededProvider.notifier).state = false;
    return _Scrim(
      onDismiss: close,
      maxWidth: 440,
      child: Padding(
        padding: const EdgeInsets.all(AppTokens.s24),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(context.tr('Screen Recording permission required'),
                style: TextStyle(
                    color: c.textPrimary,
                    fontWeight: FontWeight.w700,
                    fontSize: 16)),
            const SizedBox(height: AppTokens.s8),
            Text(
              context.tr('Enable SenClaw in System Settings → Privacy & '
                  'Security → Screen Recording.'),
              style: TextStyle(color: c.textMuted, fontSize: 13, height: 1.5),
            ),
            const SizedBox(height: AppTokens.s8),
            // The relaunch requirement is the whole reason a granted permission
            // "still asks" — macOS only reads Screen Recording access at launch.
            Container(
              padding: const EdgeInsets.all(AppTokens.s12),
              decoration: BoxDecoration(
                color: AppTokens.warning.withValues(alpha: 0.12),
                borderRadius: BorderRadius.circular(AppTokens.rLg),
              ),
              child: Text(
                context.tr('Already enabled but still asked? macOS only picks '
                    'this permission up after a restart. Quit and reopen '
                    'SenClaw.'),
                style: TextStyle(
                    color: c.textPrimary, fontSize: 12.5, height: 1.5),
              ),
            ),
            const SizedBox(height: AppTokens.s20),
            Row(
              children: [
                TextButton(onPressed: close, child: Text(context.tr('Not now'))),
                const Spacer(),
                OutlinedButton(
                  onPressed: () {
                    openScreenRecordingSettings();
                  },
                  child: Text(context.tr('Open Settings')),
                ),
                const SizedBox(width: AppTokens.s8),
                FilledButton(
                  onPressed: () => shutdownApp(
                    supervisor: ref.read(daemonSupervisorProvider),
                    uiPort: ref.read(appConfigProvider).uiPort,
                  ),
                  child: Text(context.tr('Quit to reopen')),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }
}

class _ErrorCard extends ConsumerWidget {
  const _ErrorCard({required this.message});
  final String message;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    void close() => ref.read(captureErrorProvider.notifier).state = null;
    return _Scrim(
      onDismiss: close,
      maxWidth: 440,
      child: Padding(
        padding: const EdgeInsets.all(AppTokens.s24),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(context.tr('Screenshot failed'),
                style: TextStyle(
                    color: c.textPrimary,
                    fontWeight: FontWeight.w700,
                    fontSize: 16)),
            const SizedBox(height: AppTokens.s8),
            Text(message,
                style: TextStyle(color: c.textMuted, fontSize: 13, height: 1.5)),
            const SizedBox(height: AppTokens.s20),
            Align(
              alignment: Alignment.centerRight,
              child:
                  FilledButton(onPressed: close, child: Text(context.tr('Close'))),
            ),
          ],
        ),
      ),
    );
  }
}

class _CaptureDialog extends ConsumerStatefulWidget {
  const _CaptureDialog({super.key, required this.shot});
  final CaptureResult shot;

  @override
  ConsumerState<_CaptureDialog> createState() => _CaptureDialogState();
}

class _CaptureDialogState extends ConsumerState<_CaptureDialog> {
  final _title = TextEditingController();
  final _body = TextEditingController();

  bool _saving = false;
  bool _extracting = false;
  String? _error;

  @override
  void dispose() {
    _title.dispose();
    _body.dispose();
    super.dispose();
  }

  void _close() => ref.read(pendingCaptureProvider.notifier).state = null;

  /// Ask the daemon to read the shot (vision, or OCR → text LLM) and fill the
  /// title + notes. Overwrites what's there — the button is an explicit "let AI
  /// do it", so replacing a half-typed draft is the expected intent.
  Future<void> _aiExtract() async {
    setState(() {
      _extracting = true;
      _error = null;
    });
    try {
      final r = await ref.read(apiClientProvider).post(
        '/api/space/screenshots/extract',
        body: {'name': widget.shot.name},
      );
      if (!mounted) return;
      final title = (r is Map ? r['title'] as String? : null)?.trim() ?? '';
      final notes = (r is Map ? r['notes'] as String? : null)?.trim() ?? '';
      setState(() {
        if (title.isNotEmpty) _title.text = title;
        if (notes.isNotEmpty) _body.text = notes;
      });
    } catch (e) {
      if (mounted) setState(() => _error = _msg(e));
    } finally {
      if (mounted) setState(() => _extracting = false);
    }
  }

  /// ApiException carries the daemon's own message (e.g. "no vision + OCR not
  /// ready"); surface that verbatim, other errors as their string.
  String _msg(Object e) {
    final s = e.toString();
    return s.startsWith('ApiException') && s.contains(':')
        ? s.substring(s.indexOf(':') + 1).trim()
        : s;
  }

  Future<void> _save() async {
    final title = _title.text.trim();
    if (title.isEmpty) {
      setState(() => _error = context.tr('Enter a title first.'));
      return;
    }
    setState(() {
      _saving = true;
      _error = null;
    });

    final cfg = ref.read(appConfigProvider);
    final url = widget.shot.url(cfg.host, cfg.uiPort);
    final api = ref.read(spaceApiProvider);

    try {
      // Body is markdown and FTS-indexed, so what the user types here is what
      // makes the shot findable later. The image goes in by URL rather than
      // file path — the markdown renderer is HTTP-only.
      final noteBody = StringBuffer()
        ..writeln('![screenshot]($url)')
        ..writeln();
      if (_body.text.trim().isNotEmpty) {
        noteBody
          ..writeln(_body.text.trim())
          ..writeln();
      }
      noteBody.writeln(context.trArgs('_Captured at {t}._',
          {'t': DateFormat('d/M/y HH:mm').format(DateTime.now())}));

      await api.createNote(title, noteBody.toString(), ['screenshot']);
      if (mounted) _close();
    } catch (e) {
      if (mounted) {
        setState(() {
          _saving = false;
          _error = context.trArgs('Save failed: {e}', {'e': e});
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return _Scrim(
      onDismiss: _saving ? () {} : _close,
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _header(c),
          Divider(height: 1, color: c.border),
          Flexible(child: SingleChildScrollView(child: _form(c))),
          if (_error != null)
            Padding(
              padding: const EdgeInsets.fromLTRB(
                  AppTokens.s20, 0, AppTokens.s20, AppTokens.s8),
              child: Text(_error!,
                  style: const TextStyle(
                      color: AppTokens.danger, fontSize: 12)),
            ),
          Divider(height: 1, color: c.border),
          _actions(c),
        ],
      ),
    );
  }

  Widget _header(AppColors c) => Padding(
        padding: const EdgeInsets.fromLTRB(
            AppTokens.s20, AppTokens.s16, AppTokens.s8, AppTokens.s12),
        child: Row(
          children: [
            const Text('📸', style: TextStyle(fontSize: 18)),
            const SizedBox(width: AppTokens.s8),
            Expanded(
              child: Text(context.tr('Save screenshot to a note'),
                  style: TextStyle(
                      color: c.textPrimary,
                      fontWeight: FontWeight.w700,
                      fontSize: 16)),
            ),
            IconButton(
              tooltip: context.tr('Close'),
              icon: const Icon(Icons.close, size: 18),
              onPressed: _saving ? null : _close,
            ),
          ],
        ),
      );

  Widget _form(AppColors c) => Padding(
        padding: const EdgeInsets.symmetric(
            horizontal: AppTokens.s20, vertical: AppTokens.s16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            // Preview straight off disk — no daemon round-trip needed just to
            // show the user what they just captured.
            ClipRRect(
              borderRadius: BorderRadius.circular(AppTokens.rLg),
              child: Container(
                constraints: const BoxConstraints(maxHeight: 220),
                width: double.infinity,
                color: c.surfaceAlt,
                child: Image.file(File(widget.shot.path), fit: BoxFit.contain),
              ),
            ),
            const SizedBox(height: AppTokens.s12),
            Row(
              children: [
                Text(context.tr('Fill in the details'),
                    style: TextStyle(
                        color: c.textMuted,
                        fontSize: 12,
                        fontWeight: FontWeight.w600)),
                const Spacer(),
                // Reads the shot with vision (or OCR → text LLM) and fills both
                // fields. The daemon picks whichever the active model supports.
                TextButton.icon(
                  onPressed:
                      (_extracting || _saving) ? null : _aiExtract,
                  icon: _extracting
                      ? const SizedBox(
                          width: 14,
                          height: 14,
                          child: CircularProgressIndicator(strokeWidth: 2))
                      : const Icon(Icons.auto_awesome, size: 16),
                  label: Text(_extracting
                      ? context.tr('Reading the image…')
                      : context.tr('AI fill')),
                ),
              ],
            ),
            const SizedBox(height: AppTokens.s4),
            TextField(
              controller: _title,
              autofocus: true,
              enabled: !_extracting,
              style: TextStyle(color: c.textPrimary, fontSize: 14),
              decoration: _dec(c, context.tr('Note title')),
            ),
            const SizedBox(height: AppTokens.s8),
            TextField(
              controller: _body,
              minLines: 2,
              maxLines: 4,
              enabled: !_extracting,
              style: TextStyle(color: c.textPrimary, fontSize: 14),
              decoration: _dec(c, context.tr('More notes (optional)')),
            ),
          ],
        ),
      );

  InputDecoration _dec(AppColors c, String hint) => InputDecoration(
        hintText: hint,
        hintStyle: TextStyle(color: c.textMuted, fontSize: 14),
        filled: true,
        fillColor: c.surfaceAlt,
        isDense: true,
        contentPadding: const EdgeInsets.symmetric(
            horizontal: AppTokens.s16, vertical: AppTokens.s12),
        enabledBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(AppTokens.rLg),
          borderSide: BorderSide(color: c.border),
        ),
        focusedBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(AppTokens.rLg),
          borderSide: BorderSide(color: c.accent, width: 1.5),
        ),
      );

  Widget _actions(AppColors c) => Padding(
        padding: const EdgeInsets.all(AppTokens.s12),
        child: Row(
          mainAxisAlignment: MainAxisAlignment.end,
          children: [
            TextButton(
                onPressed: _saving ? null : _close,
                child: Text(context.tr('Cancel'))),
            const SizedBox(width: AppTokens.s8),
            FilledButton(
              onPressed: _saving ? null : _save,
              child: _saving
                  ? const SizedBox(
                      width: 14,
                      height: 14,
                      child: CircularProgressIndicator(strokeWidth: 2))
                  : Text(context.tr('Save note')),
            ),
          ],
        ),
      );
}
