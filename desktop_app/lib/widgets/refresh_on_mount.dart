import 'package:flutter/widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// Invalidates the given providers when this widget mounts (and when the
/// provider set changes), so a page/tab re-fetches its API data every time
/// the user navigates to it instead of showing a stale cached snapshot.
///
/// Give each usage a distinct [key] (e.g. `ValueKey('mcp')`) when several
/// siblings swap in the same tree position — otherwise Flutter reuses the
/// element and `initState` never re-fires.
class RefreshOnMount extends ConsumerStatefulWidget {
  const RefreshOnMount({
    super.key,
    required this.providers,
    required this.child,
  });

  final List<ProviderOrFamily> providers;
  final Widget child;

  @override
  ConsumerState<RefreshOnMount> createState() => _RefreshOnMountState();
}

class _RefreshOnMountState extends ConsumerState<RefreshOnMount> {
  void _invalidate() {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted) return;
      for (final p in widget.providers) {
        ref.invalidate(p);
      }
    });
  }

  @override
  void initState() {
    super.initState();
    _invalidate();
  }

  @override
  void didUpdateWidget(RefreshOnMount oldWidget) {
    super.didUpdateWidget(oldWidget);
    // Element got reused for a different tab (same runtimeType, no key or
    // equal keys) — still treat it as a fresh navigation.
    if (!identical(oldWidget.providers, widget.providers) &&
        !_sameProviders(oldWidget.providers, widget.providers)) {
      _invalidate();
    }
  }

  static bool _sameProviders(
      List<ProviderOrFamily> a, List<ProviderOrFamily> b) {
    if (a.length != b.length) return false;
    for (var i = 0; i < a.length; i++) {
      if (!identical(a[i], b[i])) return false;
    }
    return true;
  }

  @override
  Widget build(BuildContext context) => widget.child;
}
