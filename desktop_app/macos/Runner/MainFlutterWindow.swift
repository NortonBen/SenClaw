import Cocoa
import FlutterMacOS
import desktop_multi_window

class MainFlutterWindow: NSWindow {
  override func awakeFromNib() {
    let flutterViewController = FlutterViewController()
    let windowFrame = self.frame
    self.contentViewController = flutterViewController
    self.setFrame(windowFrame, display: true)

    RegisterGeneratedPlugins(registry: flutterViewController)

    // Sub-windows (the tray mini-chat) run in their own Flutter engine —
    // register the same plugins into each so window_manager / path_provider /
    // shared_preferences work there too, then style it as a fixed, frameless
    // menu-bar popover: no native title-bar buttons, can't be moved/resized,
    // floats above other windows. The in-app header provides its own controls.
    FlutterMultiWindowPlugin.setOnWindowCreatedCallback { controller in
      RegisterGeneratedPlugins(registry: controller)
      // Snapshot the cursor now (it's still on the clicked tray icon) so the
      // popover can anchor under it even after the async hop below.
      let mouse = NSEvent.mouseLocation // global, bottom-left origin
      DispatchQueue.main.async {
        guard let window = controller.view.window else { return }
        window.titlebarAppearsTransparent = true
        window.titleVisibility = .hidden
        window.styleMask.insert(.fullSizeContentView)
        // Hide the red/yellow/green traffic-light buttons.
        window.standardWindowButton(.closeButton)?.isHidden = true
        window.standardWindowButton(.miniaturizeButton)?.isHidden = true
        window.standardWindowButton(.zoomButton)?.isHidden = true
        // Pin in place (anchored under the tray icon) — not user-movable.
        window.isMovable = false
        window.isMovableByWindowBackground = false
        // Float above the app's main window like a popover.
        window.level = .floating
        window.collectionBehavior.insert(.fullScreenAuxiliary)
        // Menu-bar popover behavior: appear on whichever Space/desktop is
        // active so clicking the tray shows it there. NOTE: .canJoinAllSpaces
        // and .moveToActiveSpace are MUTUALLY EXCLUSIVE — setting both makes
        // -[NSWindow _validateCollectionBehavior:] throw and the app aborts.
        // Use only .canJoinAllSpaces (the standard menu-bar-popover choice).
        window.collectionBehavior.insert(.canJoinAllSpaces)
        // Anchor under the clicked menu-bar icon (CCleaner-style popover): use
        // the cursor x to center the window and pin its top just under the menu
        // bar. Done natively to avoid the plugin's setFrame treating Dart's
        // `top` as a bottom-left origin (which dropped it to the bottom).
        let screen = NSScreen.screens.first { NSMouseInRect(mouse, $0.frame, false) }
          ?? window.screen ?? NSScreen.main
        if let screen = screen {
          let vf = screen.visibleFrame // excludes menu bar + dock
          // Roomy default so the composer fits on one row; clamp to the screen
          // so it never exceeds the visible area. The user can still resize.
          let w: CGFloat = min(560, vf.width - 16)
          let h: CGFloat = min(700, vf.height - 16)
          var x = mouse.x - w / 2 // center under the tray icon
          // Keep fully on-screen with an 8pt margin.
          x = max(vf.minX + 8, min(x, vf.maxX - w - 8))
          let y = vf.maxY - h // top edge just under the menu bar
          window.setFrame(NSRect(x: x, y: y, width: w, height: h), display: true)
        }
      }
    }

    super.awakeFromNib()
  }
}
