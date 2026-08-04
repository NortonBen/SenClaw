import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:window_manager/window_manager.dart';
import '../theme/tokens.dart';

/// Whether this platform needs app-drawn window chrome. The main window is
/// created with `TitleBarStyle.hidden` (main.dart): macOS keeps its native
/// traffic lights + a draggable transparent title strip, but on Windows and
/// Linux hiding the title bar removes EVERY caption control — no drag area,
/// no minimize/maximize/close — so the app must draw its own.
bool get needsCustomCaption =>
    !kIsWeb &&
    (defaultTargetPlatform == TargetPlatform.windows ||
        defaultTargetPlatform == TargetPlatform.linux);

/// Wraps the whole app in a Column with a caption bar on top (Windows/Linux
/// only; a no-op elsewhere). Lives ABOVE StartupGate in app.dart so the window
/// stays movable and closable during the boot splash and the daemon-crash
/// screen, not just once the shell is up.
class DesktopChrome extends StatelessWidget {
  const DesktopChrome({super.key, required this.child});
  final Widget child;

  @override
  Widget build(BuildContext context) {
    if (!needsCustomCaption) return child;
    return Column(
      children: [
        const _CaptionBar(),
        Expanded(child: child),
      ],
    );
  }
}

/// Windows-11-style caption strip: drag-to-move (double-click toggles
/// maximize) + minimize / maximize-restore / close buttons. Close goes through
/// windowManager.close() so the preventClose flow in app.dart hides to the
/// tray instead of quitting, same as macOS.
class _CaptionBar extends StatefulWidget {
  const _CaptionBar();

  @override
  State<_CaptionBar> createState() => _CaptionBarState();
}

class _CaptionBarState extends State<_CaptionBar> with WindowListener {
  bool _maximized = false;

  @override
  void initState() {
    super.initState();
    windowManager.addListener(this);
    windowManager.isMaximized().then((v) {
      if (mounted) setState(() => _maximized = v);
    });
  }

  @override
  void dispose() {
    windowManager.removeListener(this);
    super.dispose();
  }

  @override
  void onWindowMaximize() => setState(() => _maximized = true);

  @override
  void onWindowUnmaximize() => setState(() => _maximized = false);

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final brightness = Theme.of(context).brightness;
    return Material(
      color: c.sidebar,
      child: Container(
        height: kWindowCaptionHeight,
        decoration: BoxDecoration(
          border: Border(bottom: BorderSide(color: c.border)),
        ),
        child: Row(
          children: [
            Expanded(
              child: DragToMoveArea(
                child: SizedBox(
                  height: double.infinity,
                  child: Row(
                    children: [
                      const SizedBox(width: AppTokens.s12),
                      ClipRRect(
                        borderRadius: BorderRadius.circular(4),
                        child: Image.asset(
                          'assets/branding/senclaw_icon_1024.png',
                          width: 16,
                          height: 16,
                          fit: BoxFit.cover,
                          filterQuality: FilterQuality.medium,
                        ),
                      ),
                      const SizedBox(width: AppTokens.s8),
                      Text(
                        'SenClaw',
                        style: TextStyle(
                          color: c.textMuted,
                          fontSize: 12,
                          fontWeight: FontWeight.w500,
                        ),
                      ),
                    ],
                  ),
                ),
              ),
            ),
            WindowCaptionButton.minimize(
              brightness: brightness,
              onPressed: windowManager.minimize,
            ),
            if (_maximized)
              WindowCaptionButton.unmaximize(
                brightness: brightness,
                onPressed: windowManager.unmaximize,
              )
            else
              WindowCaptionButton.maximize(
                brightness: brightness,
                onPressed: windowManager.maximize,
              ),
            WindowCaptionButton.close(
              brightness: brightness,
              onPressed: windowManager.close,
            ),
          ],
        ),
      ),
    );
  }
}
