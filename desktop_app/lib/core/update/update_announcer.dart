import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../theme/tokens.dart';
import '../../widgets/app_markdown.dart';
import '../i18n/l10n.dart';
import 'update_manifest.dart';
import 'update_provider.dart';

/// What the user did with the "update available" popup.
enum UpdateAnnouncementChoice {
  /// Go to Settings → Updates and get on with it.
  view,

  /// Not now — ask again after [kUpdateSnoozeDuration].
  later,

  /// Never for this version.
  skip,
}

/// Puts a newly-discovered release on screen once, as a modal.
///
/// Wraps the app shell rather than living inside it so the popup survives
/// navigation between screens, and so it can be pumped on its own in a test.
///
/// A modal, not the snackbar this replaced: the check now runs at every launch,
/// and an eight-second toast on a machine that just booted is exactly the
/// notification people miss. The three ways out — view / later / never — are
/// what keeps a modal from being rude.
class UpdateAnnouncer extends ConsumerStatefulWidget {
  const UpdateAnnouncer({
    super.key,
    required this.child,
    required this.onOpenUpdates,
  });

  final Widget child;

  /// Navigate to the Updates page. Injected because this lives under `core/`
  /// and the route belongs to the settings feature — and so a test can assert
  /// the navigation without mounting a router.
  final VoidCallback onOpenUpdates;

  @override
  ConsumerState<UpdateAnnouncer> createState() => _UpdateAnnouncerState();
}

class _UpdateAnnouncerState extends ConsumerState<UpdateAnnouncer> {
  bool _open = false;

  /// Versions already put on screen in this run. Dismissing with Esc persists
  /// nothing (the user gets asked again next launch), but re-opening the dialog
  /// on the next state change of the same version would be nagging.
  final _announced = <String>{};

  @override
  void initState() {
    super.initState();
    // The check may have completed before this widget mounted — the listener
    // below only fires on *changes*.
    WidgetsBinding.instance.addPostFrameCallback((_) => _maybeShow());
  }

  @override
  Widget build(BuildContext context) {
    ref.listen<UpdateState>(updateProvider, (_, _) => _maybeShow());
    return widget.child;
  }

  void _maybeShow() {
    if (_open || !mounted) return;
    final n = ref.read(updateProvider.notifier);
    if (!n.shouldAnnounce()) return;
    final m = ref.read(updateProvider).manifest!;
    final version = '${m.version}';
    if (!_announced.add(version)) return;

    _open = true;
    WidgetsBinding.instance.addPostFrameCallback((_) async {
      if (!mounted) {
        _open = false;
        return;
      }
      final choice = await showUpdateAvailableDialog(
        context,
        manifest: m,
        currentVersion: ref.read(updateServiceProvider).currentVersion,
      );
      _open = false;
      if (!mounted || choice == null) return; // Esc / click-away: ask next launch
      switch (choice) {
        case UpdateAnnouncementChoice.view:
          widget.onOpenUpdates();
        case UpdateAnnouncementChoice.later:
          await n.remindLater();
        case UpdateAnnouncementChoice.skip:
          await n.skipCurrent();
      }
    });
  }
}

/// The popup itself. Returns null when dismissed without choosing, which is
/// deliberately *not* the same as "later": nothing is persisted, so the next
/// launch asks again.
Future<UpdateAnnouncementChoice?> showUpdateAvailableDialog(
  BuildContext context, {
  required UpdateManifest manifest,
  required String currentVersion,
}) {
  return showDialog<UpdateAnnouncementChoice>(
    context: context,
    builder: (context) {
      final c = context.colors;
      final notes = manifest.notes;
      return AlertDialog(
        backgroundColor: c.surface,
        title: Text(context.tr('A new version of SenClaw is available')),
        content: SizedBox(
          width: 420,
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                context.trArgs(
                  'Version {v} — you are running {current}.',
                  {'v': '${manifest.version}', 'current': currentVersion},
                ),
                style: TextStyle(color: c.textSecondary, fontSize: 13),
              ),
              if (notes != null && notes.isNotEmpty) ...[
                const SizedBox(height: AppTokens.s12),
                // Release notes can be long; cap the popup instead of letting
                // it grow past the window.
                Container(
                  width: double.infinity,
                  constraints: const BoxConstraints(maxHeight: 220),
                  padding: const EdgeInsets.all(AppTokens.s12),
                  decoration: BoxDecoration(
                    color: c.bg,
                    border: Border.all(color: c.border),
                    borderRadius: BorderRadius.circular(AppTokens.rMd),
                  ),
                  child: SingleChildScrollView(child: AppMarkdown(notes)),
                ),
              ],
            ],
          ),
        ),
        actions: [
          TextButton(
            onPressed: () =>
                Navigator.of(context).pop(UpdateAnnouncementChoice.skip),
            child: Text(context.tr('Skip this version')),
          ),
          TextButton(
            onPressed: () =>
                Navigator.of(context).pop(UpdateAnnouncementChoice.later),
            child: Text(context.tr('Remind me later')),
          ),
          FilledButton(
            onPressed: () =>
                Navigator.of(context).pop(UpdateAnnouncementChoice.view),
            child: Text(context.tr('View update')),
          ),
        ],
      );
    },
  );
}
