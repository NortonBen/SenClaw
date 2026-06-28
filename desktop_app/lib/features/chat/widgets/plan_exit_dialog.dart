import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../../theme/tokens.dart';
import '../../../widgets/app_markdown.dart';
import '../plan_provider.dart';

/// Global modal shown when the agent finishes a plan and awaits approval.
/// Mounted once over the whole app (via MaterialApp.builder).
class PlanExitOverlay extends ConsumerWidget {
  const PlanExitOverlay({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final req = ref.watch(planExitProvider);
    if (req == null) return const SizedBox.shrink();
    final c = context.colors;
    final notifier = ref.read(planExitProvider.notifier);

    return Positioned.fill(
      child: Material(
        color: Colors.black.withValues(alpha: 0.55),
        child: Center(
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 720, maxHeight: 640),
            child: Container(
              margin: const EdgeInsets.all(AppTokens.s24),
              decoration: BoxDecoration(
                color: c.surface,
                border: Border.all(color: c.border),
                borderRadius: BorderRadius.circular(AppTokens.rXl),
              ),
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Padding(
                    padding: const EdgeInsets.fromLTRB(
                        AppTokens.s24, AppTokens.s20, AppTokens.s24, AppTokens.s8),
                    child: Row(
                      children: [
                        Icon(Icons.checklist_rtl, color: c.accent, size: 20),
                        const SizedBox(width: AppTokens.s8),
                        Text(
                          'Plan ready for review',
                          style: TextStyle(
                            color: c.textPrimary,
                            fontWeight: FontWeight.w700,
                            fontSize: 16,
                          ),
                        ),
                      ],
                    ),
                  ),
                  Flexible(
                    child: SingleChildScrollView(
                      padding: const EdgeInsets.symmetric(
                          horizontal: AppTokens.s24, vertical: AppTokens.s8),
                      child: AppMarkdown(
                        req.planContent.isEmpty
                            ? '_No plan content._'
                            : req.planContent,
                        style: TextStyle(color: c.textSecondary, height: 1.5),
                      ),
                    ),
                  ),
                  Padding(
                    padding: const EdgeInsets.all(AppTokens.s20),
                    child: Wrap(
                      alignment: WrapAlignment.end,
                      spacing: AppTokens.s8,
                      runSpacing: AppTokens.s8,
                      children: [
                        TextButton(
                          onPressed: () => notifier.resolve('cancelled'),
                          child: const Text('Cancel'),
                        ),
                        OutlinedButton(
                          onPressed: () =>
                              notifier.resolve('clearContextAndStart'),
                          child: Text(req.clearContextLabel),
                        ),
                        FilledButton(
                          onPressed: () => notifier.resolve('startEditing'),
                          child: Text(req.startEditingLabel),
                        ),
                      ],
                    ),
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}
