mod api;
mod db;
mod input;
mod llm;
mod mcp;
mod session;
mod stealth;

use std::sync::Arc;

use axum::Router;
use chromiumoxide::cdp::browser_protocol::target::CreateTargetParams;
use chromiumoxide::handler::viewport::Viewport;
use chromiumoxide::{Browser, BrowserConfig};
use futures::StreamExt;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

use crate::api::AppState;
use crate::db::{default_data_dir, Db};
use crate::session::BrowserSession;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let port = std::env::var("PORT").unwrap_or_else(|_| "4360".to_string());

    // Emit something to the runtime log immediately — launching Chromium can take
    // a few seconds, and without this the app's log panel shows "(no logs)" during
    // startup.
    println!("SenClaw Mini Browser starting on port {port} — launching Chromium…");

    // ---- Launch the stealth Chromium and build the shared session ----
    let session = match launch_session().await {
        Ok(s) => Arc::new(s),
        Err(e) => {
            eprintln!("mini-browser: failed to launch Chromium: {e}");
            eprintln!("Ensure Google Chrome / Chromium is installed, or set MB_CHROME to its path.");
            std::process::exit(1);
        }
    };

    let db = Arc::new(Db::open(&default_data_dir("mini-browser").join("browser.db")).expect("open db"));
    let (mcp_tx, _) = tokio::sync::broadcast::channel(100);
    let state = Arc::new(AppState { session, db, mcp_tx });

    let api_router = api::api_router(state);

    // Static web assets — app-specific and packaged paths first, generic last
    // (avoid the static-dir collision with SenClaw's own web/dist).
    let exe_dir = std::env::current_exe()
        .map(|p| p.parent().unwrap().to_path_buf())
        .unwrap_or_default();
    let candidates = [
        std::path::PathBuf::from("apps/mini-browser/web/dist"),
        std::path::PathBuf::from("web_dist"),
        exe_dir.join("web_dist"),
        exe_dir.join("web").join("dist"),
        std::path::PathBuf::from("web/dist"),
    ];
    let dist_path = candidates
        .iter()
        .find(|c| c.join("index.html").exists())
        .cloned()
        .unwrap_or_else(|| std::path::PathBuf::from("web/dist"));

    let serve_dir =
        ServeDir::new(&dist_path).not_found_service(ServeFile::new(dist_path.join("index.html")));

    let app = Router::new()
        .nest("/api", api_router)
        .fallback_service(serve_dir)
        .layer(CorsLayer::permissive());

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port))
        .await
        .unwrap();
    println!("SenClaw Mini Browser running on http://0.0.0.0:{}", port);
    axum::serve(listener, app).await.unwrap();
}

/// Launch Chromium with the stealth flags and return a ready `BrowserSession`.
async fn launch_session() -> anyhow::Result<BrowserSession> {
    let profile = default_data_dir("mini-browser").join("profile");
    std::fs::create_dir_all(&profile).ok();
    // Remove stale Chrome singleton locks left by a previous instance that was
    // killed uncleanly — otherwise Chrome aborts with "Failed to create
    // SingletonLock". Safe because this app runs a single browser at a time.
    for lock in ["SingletonLock", "SingletonSocket", "SingletonCookie"] {
        std::fs::remove_file(profile.join(lock)).ok();
    }

    let mut builder = BrowserConfig::builder()
        .disable_default_args()
        .args(stealth::chrome_args())
        .user_data_dir(&profile)
        .window_size(1280, 800)
        .viewport(Some(Viewport {
            width: 1280,
            height: 800,
            device_scale_factor: Some(1.0),
            emulating_mobile: false,
            is_landscape: true,
            has_touch: false,
        }));

    // Headless is the one remaining thing we have to rewrite about the browser
    // (its `HeadlessChrome` branding), so prefer a real window when the platform
    // can show one — it costs nothing, since the UI streams screenshots either
    // way. `MB_HEADLESS=1` opts back out on a server. Google's sign-in accepts
    // both modes as of Chrome 150; headful simply leaves nothing to correct.
    let headless = want_headless();
    if headless {
        builder = builder.new_headless_mode();
    } else {
        builder = builder.with_head();
    }

    // Point at an installed Chrome if we can find one.
    if let Some(path) = chrome_path() {
        builder = builder.chrome_executable(path);
    }

    let config = builder.build().map_err(anyhow::Error::msg)?;
    let (browser, mut handler) = Browser::launch(config).await?;

    // The handler MUST be polled for the connection to work. Keep draining it
    // regardless of transient per-event errors — only stop when the stream ends
    // (browser actually gone).
    tokio::spawn(async move {
        while handler.next().await.is_some() {}
    });

    let page = browser.new_page(CreateTargetParams::new("about:blank")).await?;
    BrowserSession::new(browser, page).await
}

/// `MB_HEADLESS=1` forces headless; `MB_HEADFUL=1` forces a window. Otherwise a
/// window whenever the platform can show one.
fn want_headless() -> bool {
    if let Ok(v) = std::env::var("MB_HEADLESS") {
        return v == "1";
    }
    if std::env::var("MB_HEADFUL").ok().as_deref() == Some("1") {
        return false;
    }
    !has_display()
}

fn has_display() -> bool {
    if cfg!(any(target_os = "macos", target_os = "windows")) {
        return true;
    }
    std::env::var("DISPLAY").is_ok() || std::env::var("WAYLAND_DISPLAY").is_ok()
}

/// Resolve a Chrome/Chromium executable: MB_CHROME env, then common macOS/Linux paths.
fn chrome_path() -> Option<String> {
    if let Ok(p) = std::env::var("MB_CHROME") {
        if std::path::Path::new(&p).exists() {
            return Some(p);
        }
    }
    let candidates = [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/usr/bin/google-chrome",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
    ];
    candidates.iter().find(|p| std::path::Path::new(p).exists()).map(|s| s.to_string())
}

/// Live tests each launch a browser against the shared profile directory, so
/// they must not run concurrently:
///   cargo test -p mini-browser -- --ignored --test-threads=1
/// Running them in parallel makes Chrome fail the profile lock, surfacing as a
/// bare "oneshot canceled".
#[cfg(test)]
mod tests {
    use super::launch_session;

    /// Live identity check — launches Chrome and asserts it presents itself as a
    /// coherent, real browser. Needs a Chrome binary, so ignored by default:
    ///   cargo test -p mini-browser -- --ignored identity_smoke
    #[tokio::test]
    #[ignore]
    async fn identity_smoke() {
        let session = launch_session().await.expect("launch");
        // navigator.userAgentData needs a secure context; about:blank is not one.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let app = axum::Router::new().route(
                "/",
                axum::routing::get(|| async { axum::response::Html("<html><body>x</body></html>") }),
            );
            axum::serve(listener, app).await.ok();
        });
        session.navigate(&format!("http://127.0.0.1:{}/", addr.port())).await.expect("navigate");

        let checks = session
            .execute_js(
                "const d = navigator.userAgentData; \
                 const gl = document.createElement('canvas').getContext('webgl'); \
                 const dbg = gl && gl.getExtension('WEBGL_debug_renderer_info'); \
                 return { ua: navigator.userAgent, webdriver: navigator.webdriver, \
                          langs: navigator.languages, chrome: !!window.chrome, \
                          plugins: navigator.plugins.length, \
                          brands: d ? d.brands.map(b => b.brand) : [], \
                          uaPlatform: d ? d.platform : '', \
                          renderer: dbg ? gl.getParameter(dbg.UNMASKED_RENDERER_WEBGL) : '' };",
            )
            .await
            .expect("execute");

        let ua = checks["ua"].as_str().unwrap_or_default();
        let brands: Vec<&str> = checks["brands"].as_array().unwrap().iter().filter_map(|b| b.as_str()).collect();

        assert_eq!(checks["webdriver"], serde_json::json!(false), "real Chrome reports false, not undefined");
        assert_eq!(checks["chrome"], serde_json::json!(true));
        assert!(checks["plugins"].as_i64().unwrap_or(0) > 0, "plugins empty");
        assert_eq!(checks["langs"][0], "vi-VN");

        // Nothing may still claim to be headless…
        assert!(!ua.contains("Headless"), "UA leaks headless: {ua}");
        assert!(!brands.iter().any(|b| b.contains("Headless")), "brands leak headless: {brands:?}");

        // …and the client hints must exist and agree with the UA. Empty brands
        // means the override dropped Sec-CH-UA — the original sign-in bug.
        assert!(!brands.is_empty(), "no client-hint brands ⇒ Sec-CH-UA suppressed");
        assert_eq!(checks["uaPlatform"], "macOS");
        assert!(ua.contains("Mac OS X"), "UA/platform disagree: {ua}");

        // The GPU must belong to the platform the UA claims.
        let renderer = checks["renderer"].as_str().unwrap_or_default();
        assert!(!renderer.contains("Direct3D"), "macOS UA with a Direct3D GPU: {renderer}");
    }

    /// The regression test for the bug this layer exists to fix: Google used to
    /// bounce us to `/v3/signin/rejected` ("Không thể đăng nhập cho bạn"), which
    /// is how a fabricated identity gets treated. Asserts the sign-in form is
    /// actually served — no credentials involved, only the landing URL.
    /// Needs network + a Chrome binary:
    ///   cargo test -p mini-browser -- --ignored google_serves_signin_form
    #[tokio::test]
    #[ignore]
    async fn google_serves_signin_form() {
        let session = launch_session().await.expect("launch");
        session
            .navigate("https://accounts.google.com/ServiceLogin?hl=vi")
            .await
            .expect("navigate");
        tokio::time::sleep(std::time::Duration::from_secs(4)).await;

        let url = session.info().await.expect("info")["url"].as_str().unwrap_or_default().to_string();
        let body = session.extract_text(None).await.unwrap_or_default();
        let text = body["text"].as_str().unwrap_or_default();

        assert!(
            !url.contains("/signin/rejected") && !text.contains("Không thể đăng nhập"),
            "Google rejected the browser as insecure — landed at {url}"
        );
        assert!(url.contains("/signin/identifier"), "expected the sign-in form, got {url}");
    }
}
