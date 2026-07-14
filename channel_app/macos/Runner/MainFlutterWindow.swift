import Cocoa
import FlutterMacOS

class MainFlutterWindow: NSWindow {
  override func awakeFromNib() {
    let flutterViewController = FlutterViewController()
    self.contentViewController = flutterViewController

    // Desktop sizing: open at a comfortable default and forbid shrinking the
    // window down to an unusable phone-sized frame.
    self.contentMinSize = NSSize(width: 900, height: 640)
    let initialSize = NSSize(width: 1200, height: 820)
    self.setContentSize(initialSize)
    self.center()

    RegisterGeneratedPlugins(registry: flutterViewController)

    super.awakeFromNib()
  }
}
