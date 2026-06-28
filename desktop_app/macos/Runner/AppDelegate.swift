import Cocoa
import FlutterMacOS

@main
class AppDelegate: FlutterAppDelegate {
  // Keep running in the menu bar (tray) when all windows close — like Docker /
  // CCleaner. Quitting is done explicitly via the tray "Quit" item.
  override func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
    return false
  }

  override func applicationSupportsSecureRestorableState(_ app: NSApplication) -> Bool {
    return true
  }

  // Bridge so Dart can force the app to the foreground. After the app has been
  // an accessory (closed to the tray), window_manager's show()/focus() alone
  // don't reliably activate it — `NSApp.activate(ignoringOtherApps:)` does.
  override func applicationDidFinishLaunching(_ notification: Notification) {
    if let controller = mainFlutterWindow?.contentViewController
      as? FlutterViewController
    {
      let channel = FlutterMethodChannel(
        name: "senclaw/app",
        binaryMessenger: controller.engine.binaryMessenger)
      channel.setMethodCallHandler { [weak self] call, result in
        if call.method == "activate" {
          NSApp.setActivationPolicy(.regular)
          NSApp.activate(ignoringOtherApps: true)
          self?.mainFlutterWindow?.makeKeyAndOrderFront(nil)
          result(nil)
        } else {
          result(FlutterMethodNotImplemented)
        }
      }
    }
    super.applicationDidFinishLaunching(notification)
  }
}
