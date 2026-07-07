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

    // Headful is the least detectable, but needs a display; default to the new
    // headless mode (much stealthier than old headless). MB_HEADFUL=1 → headful.
    if std::env::var("MB_HEADFUL").ok().as_deref() == Some("1") {
        builder = builder.with_head();
    } else {
        builder = builder.new_headless_mode();
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

#[cfg(test)]
mod tests {
    use super::launch_session;

    /// Live stealth check — launches Chromium and asserts the key bot-detection
    /// signals are neutralized. Ignored by default (needs a Chrome binary):
    ///   cargo test -p mini-browser -- --ignored stealth_smoke
    #[tokio::test]
    #[ignore]
    async fn stealth_smoke() {
        let session = launch_session().await.expect("launch");
        session.navigate("about:blank").await.expect("navigate");
        let checks = session
            .execute_js(
                "return { webdriver: navigator.webdriver, langs: navigator.languages, \
                 chrome: !!window.chrome, plugins: navigator.plugins.length, \
                 native: navigator.permissions.query.toString() };",
            )
            .await
            .expect("execute");
        // navigator.webdriver must not be true.
        assert_ne!(checks["webdriver"], serde_json::json!(true), "webdriver leaked");
        assert_eq!(checks["langs"][0], "vi-VN");
        assert_eq!(checks["chrome"], serde_json::json!(true));
        assert!(checks["plugins"].as_i64().unwrap_or(0) > 0, "plugins empty (headless tell)");
        assert!(
            checks["native"].as_str().unwrap_or("").contains("[native code]"),
            "patched fn is introspectable"
        );
    }
}
