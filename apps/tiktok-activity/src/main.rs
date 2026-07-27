#![recursion_limit = "256"]

mod ai;
mod api;
mod bridge;
mod config;
mod db;
mod domain;
mod engine;
mod extbridge;
mod mcp;
mod run_manager;

use axum::response::IntoResponse;
use axum::routing::{get, post};
use std::sync::Arc;
use tower_http::cors::CorsLayer;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .init();

    let db = db::Db::open(&config::db_path()).expect("open sqlite");

    let bridge = bridge::Bridge::from_config();

    // Legacy atomic rules (like/follow/next_video) loaded into the in-memory
    // book at boot, mirroring the Go main().
    if let Ok(raw) = db.get_legacy_atomic_rules_json() {
        engine::driver::apply_legacy_rules_at_boot(&raw);
    }

    // Extension bridge: a dedicated WS server the TikTok browser extension dials
    // to control ONE logged-in tab.
    let ext = extbridge::ExtBridge::new();
    {
        let ext = ext.clone();
        let port = config::ext_ws_port();
        tokio::spawn(async move { extbridge::serve_ws(ext, port).await });
    }

    // Driver: extension (single logged-in account) by default; `stub` for
    // tests / flow authoring without a browser.
    let driver: Arc<dyn engine::BrowserDriver> = match config::control_mode().as_str() {
        "stub" => {
            tracing::info!("engine driver = Stub (TIKTOK_CONTROL_MODE=stub)");
            Arc::new(engine::StubDriver)
        }
        _ => {
            tracing::info!("engine driver = Extension (điều khiển 1 tab TikTok qua ext-WS :{})", config::ext_ws_port());
            Arc::new(engine::driver::ExtensionDriver::new(ext.clone(), bridge.clone()))
        }
    };

    let runner = Arc::new(engine::Runner::new(driver, db.clone()));
    let runs = Arc::new(run_manager::RunManager::new(db.clone(), runner));
    runs.spawn_scheduler();

    let (mcp_tx, _) = tokio::sync::broadcast::channel::<String>(64);
    let state = api::AppState {
        db,
        runs,
        bridge,
        mcp_tx,
        ext,
    };

    // Static SPA (Vite build). Check packaged locations before the repo-root one.
    let exe_dir = std::env::current_exe().map(|p| p.parent().unwrap().to_path_buf()).unwrap_or_default();
    let candidates = [
        std::path::PathBuf::from("apps/tiktok-activity/web/dist"),
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
    // SPA fallback: serve the requested static file if it exists under dist,
    // otherwise index.html with a 200 (so deep client routes like /flows load
    // correctly — a 404 status breaks the iframe host and some SPAs).
    let dist = dist_path.clone();
    let app = api::router()
        .route("/api/mcp/sse", get(mcp::mcp_sse).post(mcp::mcp_message))
        .route("/api/mcp/message", post(mcp::mcp_message))
        .with_state(state)
        .fallback(get(move |uri: axum::http::Uri| {
            let dist = dist.clone();
            async move { serve_spa(dist, uri).await }
        }))
        .layer(CorsLayer::permissive());

    let port = config::http_port();
    let addr = format!("0.0.0.0:{port}");
    tracing::info!("tiktok-activity listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await.expect("bind");
    axum::serve(listener, app).await.expect("serve");
}

/// Serve a static asset from `dist` if it exists, else `index.html` (200) so the
/// SPA router owns client-side routes.
async fn serve_spa(dist: std::path::PathBuf, uri: axum::http::Uri) -> axum::response::Response {
    let rel = uri.path().trim_start_matches('/');
    if !rel.is_empty() && !rel.contains("..") {
        let candidate = dist.join(rel);
        if let Ok(bytes) = tokio::fs::read(&candidate).await {
            let ct = mime_for(&candidate);
            return ([(axum::http::header::CONTENT_TYPE, ct)], bytes).into_response();
        }
    }
    match tokio::fs::read(dist.join("index.html")).await {
        Ok(bytes) => (
            [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
            bytes,
        )
            .into_response(),
        Err(_) => (
            axum::http::StatusCode::NOT_FOUND,
            "TikTok Activity UI chưa được build (thiếu web/dist/index.html)",
        )
            .into_response(),
    }
}

fn mime_for(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "js" | "mjs" => "application/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "html" => "text/html; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ico" => "image/x-icon",
        _ => "application/octet-stream",
    }
}
