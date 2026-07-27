mod api;
mod db;
mod extbridge;
mod innertube;
mod llm;
mod mcp;
mod oauth;
mod youtube;

use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

/// The dedicated WS port the Chrome extension dials. `9222` is taken by video-flow,
/// so YouTube defaults to `9223` (override with `YOUTUBE_WS_PORT`).
fn ext_ws_port() -> u16 {
    std::env::var("YOUTUBE_WS_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(9223)
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let port = std::env::var("PORT").unwrap_or_else(|_| "4491".to_string());
    let state = api::make_state();

    // Dedicated extension-bridge WS server (separate port from the HTTP API).
    {
        let bridge = state.bridge.clone();
        let ws_port = ext_ws_port();
        tokio::spawn(async move { extbridge::serve_ws(bridge, ws_port).await });
    }

    let api_router = api::api_router(state);

    let exe_dir = std::env::current_exe()
        .map(|p| p.parent().unwrap().to_path_buf())
        .unwrap_or_default();
    // App-specific and packaged (`web_dist`) paths come FIRST; the generic cwd
    // `web/dist` is checked LAST so running from the repo root doesn't pick up
    // SenClaw's own main-UI `web/dist` (the static-dir collision gotcha).
    let candidates = [
        std::path::PathBuf::from("apps/youtube/web/dist"),
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
    println!("SenClaw YouTube running on http://0.0.0.0:{}", port);
    axum::serve(listener, app).await.unwrap();
}
