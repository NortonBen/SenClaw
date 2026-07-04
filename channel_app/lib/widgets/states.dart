import 'package:flutter/material.dart';
import '../services/language_service.dart';
import '../theme/tokens.dart';

/// Centered spinner with an optional caption.
class LoadingState extends StatelessWidget {
  final String? text;
  const LoadingState({super.key, this.text});

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Center(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          CircularProgressIndicator(
            valueColor: AlwaysStoppedAnimation<Color>(c.accent),
          ),
          if (text != null) ...[
            const SizedBox(height: 14),
            Text(
              text!,
              style: TextStyle(color: c.textMuted, fontSize: 13),
              textAlign: TextAlign.center,
            ),
          ],
        ],
      ),
    );
  }
}

/// Error panel with a retry button.
class ErrorState extends StatelessWidget {
  final String message;
  final VoidCallback? onRetry;
  const ErrorState({super.key, required this.message, this.onRetry});

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            const Icon(Icons.error_outline, color: AppTokens.warning, size: 44),
            const SizedBox(height: 14),
            Text(
              message,
              style: TextStyle(color: c.textSecondary, fontSize: 13),
              textAlign: TextAlign.center,
            ),
            if (onRetry != null) ...[
              const SizedBox(height: 18),
              OutlinedButton.icon(
                onPressed: onRetry,
                icon: const Icon(Icons.refresh, size: 18),
                label: Text(tr('Thử lại', 'Retry')),
              ),
            ],
          ],
        ),
      ),
    );
  }
}

/// Empty-list placeholder.
class EmptyState extends StatelessWidget {
  final IconData icon;
  final String message;
  final String? hint;
  final Widget? action;
  const EmptyState({
    super.key,
    required this.icon,
    required this.message,
    this.hint,
    this.action,
  });

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(icon, color: c.textMuted, size: 48),
            const SizedBox(height: 12),
            Text(
              message,
              style: TextStyle(color: c.textSecondary, fontSize: 14),
              textAlign: TextAlign.center,
            ),
            if (hint != null) ...[
              const SizedBox(height: 6),
              Text(
                hint!,
                style: TextStyle(color: c.textMuted, fontSize: 12),
                textAlign: TextAlign.center,
              ),
            ],
            if (action != null) ...[const SizedBox(height: 16), action!],
          ],
        ),
      ),
    );
  }
}

/// Small status dot reflecting relay connectivity.
class ConnectionDot extends StatelessWidget {
  final bool connected;
  const ConnectionDot({super.key, required this.connected});

  @override
  Widget build(BuildContext context) {
    return Container(
      width: 9,
      height: 9,
      decoration: BoxDecoration(
        shape: BoxShape.circle,
        color: connected ? AppTokens.success : AppTokens.warning,
      ),
    );
  }
}
