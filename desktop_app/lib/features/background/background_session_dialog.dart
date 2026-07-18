import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../models/background_models.dart';
import '../../theme/tokens.dart';
import 'background_providers.dart';
import 'background_screen.dart' show bgStatusColor;

/// The background-session viewer: one run's transcript.
///
/// A background run has no chat window, so this is the only place its work is
/// visible at all — the resolved prompt that was actually sent, every
/// think/tool/text line, and the result or error.
void showBackgroundSessionDialog(BuildContext context, String runId) {
  showDialog(
    context: context,
    builder: (_) => _SessionDialog(runId: runId),
  );
}

class _SessionDialog extends ConsumerWidget {
  const _SessionDialog({required this.runId});
  final String runId;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final session = ref.watch(bgSessionProvider(runId));

    return Dialog(
      backgroundColor: c.bg,
      child: SizedBox(
        width: 780,
        height: 620,
        child: session.when(
          loading: () => const Center(child: CircularProgressIndicator()),
          error: (e, _) => Center(
            child: Padding(
              padding: const EdgeInsets.all(AppTokens.s16),
              child: Text('$e',
                  style: const TextStyle(color: AppTokens.danger, fontSize: 12)),
            ),
          ),
          data: (s) => _Body(session: s, runId: runId),
        ),
      ),
    );
  }
}

class _Body extends ConsumerWidget {
  const _Body({required this.session, required this.runId});
  final BackgroundSession session;
  final String runId;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final run = session.run;

    // An in-flight run streams over WS; a finished one is whatever the DB has.
    // Appending the live tail avoids a refetch-per-line.
    final live = run.isRunning ? ref.watch(bgLiveActivityProvider(runId)) : const [];
    final activity = [...session.activity, ...live];

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        // Header
        Container(
          padding: const EdgeInsets.all(AppTokens.s12),
          decoration: BoxDecoration(
            border: Border(bottom: BorderSide(color: c.border)),
          ),
          child: Row(
            children: [
              Container(
                padding: const EdgeInsets.symmetric(horizontal: 7, vertical: 2),
                decoration: BoxDecoration(
                  color: bgStatusColor(run.status, c).withValues(alpha: 0.14),
                  borderRadius: BorderRadius.circular(AppTokens.rSm),
                  border: Border.all(
                      color: bgStatusColor(run.status, c).withValues(alpha: 0.4)),
                ),
                child: Text(
                  run.status,
                  style: TextStyle(
                    color: bgStatusColor(run.status, c),
                    fontSize: 10,
                    fontWeight: FontWeight.w700,
                  ),
                ),
              ),
              const SizedBox(width: AppTokens.s8),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      'Background session',
                      style: TextStyle(
                        color: c.textPrimary,
                        fontSize: 13,
                        fontWeight: FontWeight.w700,
                      ),
                    ),
                    Text(
                      // The session id is `bg:<run_id>` — surfaced because it is
                      // what shows up in the daemon log for this run.
                      run.sessionId.isEmpty ? 'bg:$runId' : run.sessionId,
                      style: TextStyle(
                          color: c.textMuted, fontSize: 10, fontFamily: 'monospace'),
                    ),
                  ],
                ),
              ),
              if (run.isRunning)
                TextButton.icon(
                  icon: const Icon(Icons.stop_circle_outlined, size: 15),
                  label: const Text('Cancel', style: TextStyle(fontSize: 11)),
                  onPressed: () async {
                    try {
                      await ref.read(backgroundApiProvider).cancelRun(runId);
                    } catch (_) {/* already finished */}
                  },
                ),
              IconButton(
                icon: const Icon(Icons.close, size: 16),
                onPressed: () => Navigator.pop(context),
              ),
            ],
          ),
        ),

        // Facts
        Padding(
          padding: const EdgeInsets.symmetric(
              horizontal: AppTokens.s12, vertical: AppTokens.s8),
          child: Wrap(
            spacing: AppTokens.s16,
            runSpacing: AppTokens.s4,
            children: [
              _fact(context, 'Started', fmtBgTime(run.startedAt)),
              _fact(context, 'Duration', fmtBgDuration(run.durationMs)),
              _fact(context, 'Trigger', run.triggerKind),
              if (run.turnCount != null) _fact(context, 'Turns', '${run.turnCount}'),
              if ((run.tokensIn ?? 0) + (run.tokensOut ?? 0) > 0)
                _fact(context, 'Tokens',
                    '${run.tokensIn ?? 0} in / ${run.tokensOut ?? 0} out'),
            ],
          ),
        ),

        Expanded(
          child: ListView(
            padding: const EdgeInsets.symmetric(horizontal: AppTokens.s12),
            children: [
              if (run.prompt != null && run.prompt!.isNotEmpty)
                _block(
                  context,
                  'Prompt sent',
                  run.prompt!,
                  // Worth stating: for a template task this is the *resolved*
                  // text, not the template — which is the thing you need when
                  // a run did something unexpected.
                  hint: 'after template/generator resolution',
                ),
              if (run.error != null && run.error!.isNotEmpty)
                _block(context, 'Error', run.error!, color: AppTokens.danger),
              if (run.result != null && run.result!.isNotEmpty)
                _block(
                  context,
                  run.status == 'skipped' ? 'Why it skipped' : 'Result',
                  run.result!,
                  // A skip is an outcome, not a fault — don't colour it red.
                  color: run.status == 'skipped' ? null : AppTokens.success,
                ),
              const SizedBox(height: AppTokens.s8),
              Text(
                'Transcript (${activity.length})',
                style: TextStyle(
                    color: c.textMuted, fontSize: 11, fontWeight: FontWeight.w600),
              ),
              const SizedBox(height: AppTokens.s4),
              if (activity.isEmpty)
                Padding(
                  padding: const EdgeInsets.symmetric(vertical: AppTokens.s16),
                  child: Text(
                    run.status == 'skipped'
                        ? 'Nothing ran — the task skipped this window.'
                        : 'No activity recorded.',
                    style: TextStyle(color: c.textMuted, fontSize: 11),
                  ),
                )
              else
                ...activity.map((a) => _ActivityLine(a)),
              const SizedBox(height: AppTokens.s16),
            ],
          ),
        ),
      ],
    );
  }

  Widget _fact(BuildContext context, String k, String v) {
    final c = context.colors;
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        Text('$k ', style: TextStyle(color: c.textMuted, fontSize: 10)),
        Text(v, style: TextStyle(color: c.textSecondary, fontSize: 10)),
      ],
    );
  }

  Widget _block(BuildContext context, String title, String body,
      {Color? color, String? hint}) {
    final c = context.colors;
    return Padding(
      padding: const EdgeInsets.only(bottom: AppTokens.s8),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Text(title,
                  style: TextStyle(
                      color: color ?? c.textMuted,
                      fontSize: 11,
                      fontWeight: FontWeight.w600)),
              if (hint != null) ...[
                const SizedBox(width: AppTokens.s4),
                Text('· $hint',
                    style: TextStyle(color: c.textMuted, fontSize: 10)),
              ],
            ],
          ),
          const SizedBox(height: AppTokens.s4),
          Container(
            width: double.infinity,
            padding: const EdgeInsets.all(AppTokens.s8),
            decoration: BoxDecoration(
              color: c.surface,
              borderRadius: BorderRadius.circular(AppTokens.rSm),
              border: Border.all(
                  color: color?.withValues(alpha: 0.35) ?? c.border),
            ),
            child: SelectableText(
              body,
              style: TextStyle(
                  color: c.textSecondary,
                  fontSize: 11,
                  height: 1.45,
                  fontFamily: 'monospace'),
            ),
          ),
        ],
      ),
    );
  }
}

class _ActivityLine extends StatelessWidget {
  const _ActivityLine(this.a);
  final BackgroundActivity a;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final (icon, color) = switch (a.kind) {
      'think' => (Icons.psychology_outlined, c.textMuted),
      'tool' => (Icons.build_outlined, AppTokens.brand),
      'tool_error' => (Icons.error_outline, AppTokens.danger),
      'message' => (Icons.chat_bubble_outline, AppTokens.brandAlt),
      _ => (Icons.notes_outlined, c.textSecondary),
    };
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 2),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Padding(
            padding: const EdgeInsets.only(top: 2),
            child: Icon(icon, size: 12, color: color),
          ),
          const SizedBox(width: AppTokens.s6),
          Expanded(
            child: SelectableText(
              a.detail,
              style: TextStyle(
                color: a.kind == 'tool_error' ? AppTokens.danger : c.textSecondary,
                fontSize: 11,
                height: 1.4,
              ),
            ),
          ),
        ],
      ),
    );
  }
}
