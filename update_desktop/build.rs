fn main() {
    // Embed the app icon + a manifest only when building FOR Windows ON
    // Windows (the resource compiler is not available when cross-checking
    // from macOS/Linux with `cargo check --target x86_64-pc-windows-msvc`).
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && cfg!(target_os = "windows")
    {
        let mut res = winresource::WindowsResource::new();
        // Same branded icon as the app itself, so the mini window and the
        // taskbar entry read as SenClaw, not as an anonymous tool.
        res.set_icon("../desktop_app/windows/runner/resources/app_icon.ico");
        // v6 common controls (modern progress bar) + per-monitor DPI.
        res.set_manifest(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls"
        version="6.0.0.0" processorArchitecture="*"
        publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </windowsSettings>
  </application>
</assembly>"#,
        );
        res.compile().expect("embed Windows resources");
    }
}
