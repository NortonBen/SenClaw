import 'package:flutter/material.dart';
import '../theme/tokens.dart';

/// Standard page chrome: a header bar with title + optional actions, then body.
class SectionScaffold extends StatelessWidget {
  const SectionScaffold({
    super.key,
    required this.title,
    this.subtitle,
    this.actions,
    required this.body,
  });

  final String title;
  final String? subtitle;
  final List<Widget>? actions;
  final Widget body;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Container(
          height: 56,
          padding: const EdgeInsets.symmetric(horizontal: AppTokens.s24),
          decoration: BoxDecoration(
            color: c.bg,
            border: Border(bottom: BorderSide(color: c.border)),
          ),
          child: Row(
            children: [
              // Flexible, not a bare Column + Spacer: a long subtitle claimed
              // its full intrinsic width and pushed `actions` past the right
              // edge (RenderFlex overflow) on anything but a wide window.
              Expanded(
                child: Column(
                  mainAxisAlignment: MainAxisAlignment.center,
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      title,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(
                        color: c.textPrimary,
                        fontSize: 16,
                        fontWeight: FontWeight.w700,
                      ),
                    ),
                    if (subtitle != null)
                      Text(
                        subtitle!,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: TextStyle(color: c.textMuted, fontSize: 12),
                      ),
                  ],
                ),
              ),
              if (actions != null && actions!.isNotEmpty) ...[
                const SizedBox(width: AppTokens.s16),
                ...actions!,
              ],
            ],
          ),
        ),
        Expanded(child: body),
      ],
    );
  }
}

/// Lightweight placeholder for feature areas not yet migrated. Records which
/// migration phase the feature lands in.
class ComingSoon extends StatelessWidget {
  const ComingSoon({super.key, required this.feature, required this.phase});
  final String feature;
  final String phase;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Center(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(Icons.construction_outlined, size: 40, color: c.textMuted),
          const SizedBox(height: AppTokens.s12),
          Text(
            '$feature — migration $phase',
            style: TextStyle(color: c.textSecondary, fontSize: 14),
          ),
          const SizedBox(height: AppTokens.s4),
          Text(
            'Scaffolded. Implementation tracked in the migration plan.',
            style: TextStyle(color: c.textMuted, fontSize: 12),
          ),
        ],
      ),
    );
  }
}
