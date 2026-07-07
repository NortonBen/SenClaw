//! Stealth layer — make the embedded Chromium look like a real human's browser.
//!
//! Two parts:
//!  1. Launch flags (`chrome_args`) that drop the automation tells Chrome adds by
//!     default (`--enable-automation`, the `AutomationControlled` blink feature,
//!     `IdleDetection`) and re-add a clean, stable arg set.
//!  2. A JS payload (`STEALTH_JS`) injected via `Page.addScriptToEvaluateOnNewDocument`
//!     so it runs *before* any page script — patching the properties bot-detectors
//!     probe (`navigator.webdriver`, `languages`, `plugins`, `permissions`,
//!     hardware hints, `window.chrome`).
//!
//! This dramatically reduces "is a bot" signals but is **not** a guarantee against
//! advanced anti-bot systems (Cloudflare Turnstile, DataDome).

/// A realistic, current macOS Chrome user-agent. Overridable via `MB_USER_AGENT`.
pub const DEFAULT_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

pub fn user_agent() -> String {
    std::env::var("MB_USER_AGENT").unwrap_or_else(|_| DEFAULT_UA.to_string())
}

/// Clean launch args. We call `disable_default_args()` on the builder and supply
/// these instead, so Chrome never gets `--enable-automation` /
/// `--enable-blink-features=IdleDetection`.
pub fn chrome_args() -> Vec<String> {
    [
        "--disable-background-networking",
        "--enable-features=NetworkService,NetworkServiceInProcess",
        "--disable-background-timer-throttling",
        "--disable-backgrounding-occluded-windows",
        "--disable-breakpad",
        "--disable-client-side-phishing-detection",
        "--disable-default-apps",
        "--disable-dev-shm-usage",
        "--disable-features=TranslateUI,site-per-process",
        "--disable-hang-monitor",
        "--disable-ipc-flooding-protection",
        "--disable-popup-blocking",
        "--disable-prompt-on-repost",
        "--disable-renderer-backgrounding",
        "--disable-sync",
        "--force-color-profile=srgb",
        "--metrics-recording-only",
        "--no-first-run",
        "--password-store=basic",
        "--use-mock-keychain",
        // The important anti-detection flags:
        "--disable-blink-features=AutomationControlled",
        "--no-default-browser-check",
        "--lang=vi-VN",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Extra JS injected on every new document, layered on top of chromiumoxide's
/// built-in `enable_stealth_mode` (which handles webdriver/chrome/webgl/plugins).
/// Here we harden `languages`, `permissions.query`, and hardware hints, and make
/// the patched functions un-introspectable (`toString` returns native-looking code).
pub const STEALTH_JS: &str = r#"
(() => {
  const patch = (obj, prop, value) => {
    try { Object.defineProperty(obj, prop, { get: () => value, configurable: true }); } catch (e) {}
  };

  // Language list of a real vi-VN user.
  patch(navigator, 'languages', ['vi-VN', 'vi', 'en-US', 'en']);

  // Belt-and-suspenders: ensure webdriver is gone even if built-in stealth missed it.
  patch(navigator, 'webdriver', undefined);

  // Plausible hardware for a modern laptop.
  patch(navigator, 'hardwareConcurrency', 8);
  patch(navigator, 'deviceMemory', 8);
  patch(navigator, 'maxTouchPoints', 0);

  // permissions.query should not reveal a headless "denied/prompt" mismatch.
  try {
    const orig = navigator.permissions.query.bind(navigator.permissions);
    navigator.permissions.query = (params) =>
      params && params.name === 'notifications'
        ? Promise.resolve({ state: Notification.permission, onchange: null })
        : orig(params);
  } catch (e) {}

  // window.chrome with a fuller shape than the built-in stub.
  try {
    if (!window.chrome || !window.chrome.runtime) {
      window.chrome = { runtime: {}, app: { isInstalled: false }, csi: () => {}, loadTimes: () => {} };
    }
  } catch (e) {}

  // Hide that we've monkeypatched anything: Function.prototype.toString should
  // report native code for our overrides.
  try {
    const nativeToString = Function.prototype.toString;
    const spoofed = new WeakSet();
    const wrap = (fn) => { spoofed.add(fn); return fn; };
    wrap(navigator.permissions.query);
    Function.prototype.toString = new Proxy(nativeToString, {
      apply(target, thisArg, args) {
        if (spoofed.has(thisArg)) return 'function () { [native code] }';
        return Reflect.apply(target, thisArg, args);
      },
    });
  } catch (e) {}
})();
"#;
