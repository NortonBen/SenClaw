import Cocoa
import CoreGraphics
import FlutterMacOS

@main
class AppDelegate: FlutterAppDelegate {
  /// Kept so the menu-bar actions below can call INTO Dart. The channel is
  /// otherwise only used the other way (Dart → native "activate").
  private var appChannel: FlutterMethodChannel?

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
      appChannel = channel
      channel.setMethodCallHandler { [weak self] call, result in
        if call.method == "activate" {
          NSApp.setActivationPolicy(.regular)
          NSApp.activate(ignoringOtherApps: true)
          self?.mainFlutterWindow?.makeKeyAndOrderFront(nil)
          result(nil)
        } else if call.method == "capture" {
          self?.handleCapture(call: call, result: result)
        } else {
          result(FlutterMethodNotImplemented)
        }
      }
    }
    super.applicationDidFinishLaunching(notification)
  }

  // MARK: - Screen capture

  /// Interactive screen capture, shelled out to `/usr/sbin/screencapture -i`:
  /// the user drags a region, presses SPACE for window mode, or ESC to cancel.
  /// Reusing the system selector means we inherit all of its affordances for
  /// free, and the app is un-sandboxed so the spawn is permitted.
  ///
  /// Returns `{path, name}` on success, `nil` if the user cancelled, or a
  /// `permission_required` FlutterError if Screen Recording is not granted.
  private func handleCapture(call: FlutterMethodCall, result: @escaping FlutterResult) {
    let args = call.arguments as? [String: Any]
    let dir = (args?["dir"] as? String).map { URL(fileURLWithPath: $0) }
      ?? FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent(".senclaw/screenshots")

    do {
      try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    } catch {
      result(
        FlutterError(
          code: "io", message: "Cannot create \(dir.path): \(error.localizedDescription)",
          details: nil))
      return
    }

    // Preflight BEFORE spawning screencapture. Screen Recording is a launch-
    // scoped TCC permission: a grant made in System Settings stays invisible to
    // an already-running process until it relaunches, so preflight is the only
    // honest read of whether a capture can actually succeed right now.
    //
    // Running screencapture while denied is what triggers the SYSTEM's own
    // "would like to record" prompt — and pairing that with our Dart dialog is
    // the double-prompt users hit. Bailing here keeps exactly one prompt in
    // play. CGRequestScreenCaptureAccess registers the app in the TCC list and
    // shows the native prompt the FIRST time only; once the user has decided,
    // it's a silent no-op, so the repeated-attempt loop shows just our dialog.
    if !CGPreflightScreenCaptureAccess() {
      CGRequestScreenCaptureAccess()
      result(
        FlutterError(
          code: "permission_required",
          message: "SenClaw needs Screen Recording permission to capture the screen.",
          details: nil))
      return
    }

    let name = "shot-\(Self.stamp()).png"
    let out = dir.appendingPathComponent(name)

    // Off the main thread: the selector blocks until the user acts, and
    // blocking here would freeze the Flutter engine along with it.
    DispatchQueue.global(qos: .userInitiated).async {
      let proc = Process()
      proc.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
      // No -x: the shutter sound is the user's confirmation the shot landed.
      proc.arguments = ["-i", out.path]
      do {
        try proc.run()
        proc.waitUntilExit()
      } catch {
        DispatchQueue.main.async {
          result(
            FlutterError(
              code: "spawn", message: "screencapture failed: \(error.localizedDescription)",
              details: nil))
        }
        return
      }

      // Permission is confirmed, so a missing file can only mean the user
      // pressed ESC — a cancel, not a failure.
      let wrote = FileManager.default.fileExists(atPath: out.path)
      DispatchQueue.main.async {
        result(wrote ? ["path": out.path, "name": name] : nil)
      }
    }
  }

  /// Millisecond precision so two captures in the same second don't collide.
  private static func stamp() -> String {
    let f = DateFormatter()
    f.dateFormat = "yyyyMMdd-HHmmss-SSS"
    return f.string(from: Date())
  }

  // MARK: - Menu bar actions
  //
  // Wired to the app menu in MainMenu.xib. Both bring the window back first:
  // the app lives in the tray, so the menu bar is reachable while the window is
  // closed and an accessory app cannot show anything without being activated.

  @objc func senclawCheckForUpdates(_ sender: Any?) {
    showMainWindow()
    appChannel?.invokeMethod("checkForUpdates", arguments: nil)
  }

  /// The "Settings…" item shipped with the Flutter template but was never
  /// connected to anything, so macOS greyed it out.
  @objc func senclawShowSettings(_ sender: Any?) {
    showMainWindow()
    appChannel?.invokeMethod("showSettings", arguments: nil)
  }

  private func showMainWindow() {
    NSApp.setActivationPolicy(.regular)
    NSApp.activate(ignoringOtherApps: true)
    mainFlutterWindow?.makeKeyAndOrderFront(nil)
  }
}
