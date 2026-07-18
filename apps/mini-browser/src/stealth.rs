//! Identity layer — make the embedded Chrome present itself *accurately*.
//!
//! This was a spoofing layer, and it invented an identity out of thin air: a
//! hardcoded `Chrome/131` UA on a machine running Chrome 150, a Windows NVIDIA
//! Direct3D GPU behind a macOS UA, a fake plugin list, and `Accept-Language:
//! en-US` while `navigator.languages` claimed `vi-VN`. Worst of all, overriding
//! the UA string *without* `userAgentMetadata` makes Chrome stop sending
//! `Sec-CH-UA` client hints altogether — no real Chrome does that. Measured on
//! the wire, the "stealth" browser was less plausible than an untouched one.
//!
//! The approach now: read the browser's genuine identity, correct only the one
//! thing that is actually untrue (the `HeadlessChrome` token, when running
//! headless), and pass everything else through unchanged.
//!
//! Scope note, so nobody over-trusts this: each defect above was confirmed by
//! probing what the browser actually emits, and all are fixed. But the specific
//! `/v3/signin/rejected` bounce that prompted the rewrite was never reproduced
//! here — Google serves the sign-in form to the old code too, so that rejection
//! comes from somewhere later in the flow (most likely the password step) and is
//! NOT known to be fixed by this. `google_serves_signin_form` in `main.rs` only
//! pins the entry point.

use anyhow::{anyhow, Result};
use chromiumoxide::cdp::browser_protocol::emulation::{
    SetUserAgentOverrideParams, UserAgentBrandVersion, UserAgentMetadata,
};
use chromiumoxide::cdp::js_protocol::runtime::EvaluateParams;
use chromiumoxide::Page;
use serde::Deserialize;

/// `Accept-Language` header *and* `navigator.languages`, kept in sync. The old
/// layer patched only the JS side, so the header said `en-US` while JS claimed
/// `vi-VN` — a contradiction any server could see for free.
///
/// Plain comma-separated locales only: Chrome appends the `q=` weights itself,
/// and passing our own produces `vi;q=0.9;q=0.9` on the wire.
pub fn accept_language() -> String {
    std::env::var("MB_ACCEPT_LANGUAGE").unwrap_or_else(|_| "vi-VN,vi,en-US,en".to_string())
}

/// Chrome's own view of itself, read from a page before any override is applied.
#[derive(Debug, Clone, Deserialize)]
pub struct RawIdentity {
    pub ua: String,
    pub brands: Vec<Brand>,
    #[serde(rename = "fullVersionList")]
    pub full_version_list: Vec<Brand>,
    pub mobile: bool,
    pub platform: String,
    #[serde(rename = "platformVersion")]
    pub platform_version: String,
    pub architecture: String,
    pub model: String,
    pub bitness: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Brand {
    pub brand: String,
    pub version: String,
}

/// Ask the browser who it really is, with no UA override in effect yet.
///
/// `navigator.userAgentData` is gated on a secure context, and `about:blank` is
/// not one — probing there silently yields no brands at all, which is how the
/// first cut of this fix ended up publishing an empty `Sec-CH-UA`. So we stand
/// up a throwaway loopback server (127.0.0.1 *is* a secure origin), read the
/// identity from a real page, and tear it down.
pub async fn probe(page: &Page) -> Result<RawIdentity> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(async move {
        let app = axum::Router::new()
            .route("/", axum::routing::get(|| async { axum::response::Html("<html><body></body></html>") }));
        axum::serve(listener, app).await.ok();
    });

    let result = probe_at(page, &format!("http://127.0.0.1:{}/", addr.port())).await;

    server.abort();
    page.goto("about:blank").await.ok();
    result
}

async fn probe_at(page: &Page, url: &str) -> Result<RawIdentity> {
    page.goto(url).await?;
    let params = EvaluateParams::builder()
        .expression(PROBE_JS)
        .await_promise(true)
        .return_by_value(true)
        .build()
        .map_err(anyhow::Error::msg)?;
    let raw: String = page
        .evaluate(params)
        .await?
        .into_value()
        .map_err(|e| anyhow!("identity probe decode: {e}"))?;
    let id: RawIdentity =
        serde_json::from_str(&raw).map_err(|e| anyhow!("identity probe parse: {e}"))?;

    // An empty brand list means we read from a non-secure context and got
    // nothing. Publishing that as metadata would disable client hints — the very
    // bug this layer exists to prevent — so fail loudly instead.
    if id.brands.is_empty() {
        return Err(anyhow!(
            "identity probe returned no client-hint brands (userAgentData unavailable at {url})"
        ));
    }
    Ok(id)
}

const PROBE_JS: &str = r#"(async () => {
  const d = navigator.userAgentData;
  let hi = {};
  try {
    hi = d ? await d.getHighEntropyValues(
      ['platform','platformVersion','architecture','model','bitness','fullVersionList']) : {};
  } catch (e) {}
  return JSON.stringify({
    ua: navigator.userAgent,
    brands: (d && d.brands) || [],
    fullVersionList: hi.fullVersionList || [],
    mobile: d ? !!d.mobile : false,
    platform: hi.platform || '',
    platformVersion: hi.platformVersion || '',
    architecture: hi.architecture || '',
    model: hi.model || '',
    bitness: hi.bitness || '',
  });
})()"#;

/// The identity we present: the real one, with any `Headless` token corrected.
pub struct Identity {
    pub ua: String,
    pub metadata: UserAgentMetadata,
    /// True when the browser was actually lying about being headless and we
    /// rewrote it. Headful needs no correction at all.
    pub corrected: bool,
}

/// Headless Chrome brands itself `HeadlessChrome` in both the UA string and the
/// client-hint brand list. That is the single tell worth correcting: the browser
/// is a genuine Chrome build, run without a window.
pub fn correct(raw: &RawIdentity) -> Identity {
    let corrected = raw.ua.contains("Headless")
        || raw.brands.iter().any(|b| b.brand.contains("Headless"));

    let ua = std::env::var("MB_USER_AGENT").unwrap_or_else(|_| raw.ua.replace("HeadlessChrome", "Chrome"));

    let fix = |list: &Vec<Brand>| -> Vec<UserAgentBrandVersion> {
        list.iter()
            .map(|b| UserAgentBrandVersion {
                brand: b.brand.replace("HeadlessChrome", "Google Chrome"),
                version: b.version.clone(),
            })
            .collect()
    };

    let metadata = UserAgentMetadata {
        brands: Some(fix(&raw.brands)),
        full_version_list: Some(fix(&raw.full_version_list)),
        platform: raw.platform.clone(),
        platform_version: raw.platform_version.clone(),
        architecture: raw.architecture.clone(),
        model: raw.model.clone(),
        mobile: raw.mobile,
        bitness: Some(raw.bitness.clone()),
        wow64: Some(false),
    };

    Identity { ua, metadata, corrected }
}

/// Build the UA override. We *always* send this — not to lie, but because it is
/// the only way to attach `acceptLanguage` and to keep `userAgentMetadata`
/// populated so Chrome keeps emitting `Sec-CH-UA` normally.
pub fn override_params(id: &Identity) -> Result<SetUserAgentOverrideParams> {
    SetUserAgentOverrideParams::builder()
        .user_agent(id.ua.clone())
        .accept_language(accept_language())
        .user_agent_metadata(id.metadata.clone())
        .build()
        .map_err(anyhow::Error::msg)
}

/// Clean launch args. `disable_default_args()` is called on the builder, so
/// Chrome never receives `--enable-automation`, and we supply this set instead.
pub fn chrome_args() -> Vec<String> {
    let mut v: Vec<String> = [
        "--disable-background-networking",
        "--disable-background-timer-throttling",
        "--disable-backgrounding-occluded-windows",
        "--disable-breakpad",
        "--disable-client-side-phishing-detection",
        "--disable-default-apps",
        "--disable-dev-shm-usage",
        // NOTE: site-per-process is deliberately NOT disabled here. Real Chrome
        // ships site isolation on, and this profile is one the user signs into
        // their own accounts with — turning it off would be both a fingerprint
        // deviation and a real security downgrade.
        "--disable-features=TranslateUI",
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
        // Keeps navigator.webdriver false, as on a normal Chrome.
        "--disable-blink-features=AutomationControlled",
        "--no-default-browser-check",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    v.push(format!("--lang={}", accept_language().split(',').next().unwrap_or("vi-VN")));
    v
}

// There is deliberately no injected JS payload here any more.
//
// The old one patched `navigator.webdriver`, `languages`, `plugins`,
// `permissions.query`, hardware hints and the WebGL vendor. Probing a bare
// browser launched with `chrome_args()` shows every one of those was either
// already correct or made things worse:
//
//   webdriver              false        (--disable-blink-features=AutomationControlled)
//   Notification.permission "default"   consistent with permissions.query → "prompt"
//   window.chrome          present
//   navigator.plugins      5            real built-in PDF viewer entries
//   WebGL renderer         "ANGLE (Apple, ANGLE Metal Renderer: Apple M4 Pro, …)"
//
// That last line is the punchline: headless Chrome on macOS renders through the
// real Metal GPU, so the old code was replacing a true Apple renderer with a
// fabricated *Windows Direct3D NVIDIA* one — while the UA claimed macOS. Every
// patch was a lie, and each lie is one more thing that can fail to line up.
//
// `languages` is handled truthfully by `acceptLanguage` on the UA override, so
// the header and JS cannot drift apart. Nothing else needs touching.

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(ua: &str, brand: &str) -> RawIdentity {
        RawIdentity {
            ua: ua.to_string(),
            brands: vec![Brand { brand: brand.to_string(), version: "150".into() }],
            full_version_list: vec![Brand { brand: brand.to_string(), version: "150.0.7871.125".into() }],
            mobile: false,
            platform: "macOS".into(),
            platform_version: "15.5.0".into(),
            architecture: "arm".into(),
            model: "".into(),
            bitness: "64".into(),
        }
    }

    #[test]
    fn headless_tokens_are_corrected() {
        let r = raw(
            "Mozilla/5.0 (Macintosh) HeadlessChrome/150.0.7871.125 Safari/537.36",
            "HeadlessChrome",
        );
        let id = correct(&r);
        assert!(id.corrected);
        assert!(!id.ua.contains("Headless"), "UA still headless: {}", id.ua);
        assert!(id.ua.contains("Chrome/150.0.7871.125"));
        assert_eq!(id.metadata.brands.as_ref().unwrap()[0].brand, "Google Chrome");
        assert_eq!(id.metadata.full_version_list.as_ref().unwrap()[0].brand, "Google Chrome");
    }

    #[test]
    fn headful_identity_passes_through_untouched() {
        let ua = "Mozilla/5.0 (Macintosh) Chrome/150.0.7871.125 Safari/537.36";
        let r = raw(ua, "Google Chrome");
        let id = correct(&r);
        assert!(!id.corrected, "headful needs no correction");
        assert_eq!(id.ua, ua);
        assert_eq!(id.metadata.brands.as_ref().unwrap()[0].brand, "Google Chrome");
    }

    /// The real browser's version must survive — the old layer hardcoded 131
    /// while shipping Chrome 150, and the client hints gave it away.
    #[test]
    fn version_is_never_hardcoded() {
        let r = raw("Mozilla/5.0 (Macintosh) HeadlessChrome/99.1.2.3 Safari/537.36", "HeadlessChrome");
        let id = correct(&r);
        assert!(id.ua.contains("99.1.2.3"));
        assert_eq!(id.metadata.full_version_list.as_ref().unwrap()[0].version, "150.0.7871.125");
    }

    #[test]
    fn metadata_preserves_real_platform() {
        let id = correct(&raw("HeadlessChrome/150.0.0.0", "HeadlessChrome"));
        assert_eq!(id.metadata.platform, "macOS");
        assert_eq!(id.metadata.architecture, "arm");
        assert!(!id.metadata.mobile);
    }

    #[test]
    fn accept_language_leads_with_a_real_locale() {
        let lang = accept_language();
        assert!(lang.starts_with("vi-VN"), "unexpected default: {lang}");
        // The --lang flag must agree with the header's primary locale.
        let args = chrome_args();
        assert!(args.iter().any(|a| a == "--lang=vi-VN"), "lang flag missing: {args:?}");
    }

    #[test]
    fn site_isolation_stays_on() {
        let args = chrome_args();
        assert!(
            !args.iter().any(|a| a.contains("site-per-process")),
            "site isolation must not be disabled — this profile holds real logins"
        );
        assert!(args.iter().any(|a| a == "--disable-blink-features=AutomationControlled"));
    }

    #[test]
    fn client_hint_metadata_is_always_populated() {
        // A UA override without metadata makes Chrome drop Sec-CH-UA entirely,
        // which is the tell that got sign-in rejected. Guard against a
        // regression that leaves the brand list empty.
        let id = correct(&raw("HeadlessChrome/150.0.0.0", "HeadlessChrome"));
        let p = override_params(&id).expect("build override");
        let md = p.user_agent_metadata.expect("metadata must be attached");
        assert!(!md.brands.unwrap().is_empty(), "empty brands ⇒ no Sec-CH-UA");
        assert_eq!(p.accept_language.as_deref(), Some("vi-VN,vi,en-US,en"));
    }

    /// Chrome appends its own q-values; supplying ours produced the mangled
    /// `vi;q=0.9;q=0.9` seen on the wire.
    #[test]
    fn accept_language_carries_no_q_values() {
        assert!(!accept_language().contains("q="), "Chrome adds the weights itself");
    }
}
