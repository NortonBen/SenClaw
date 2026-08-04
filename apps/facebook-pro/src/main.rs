//! SenClaw Facebook Pro Space App — connects an agent to your Facebook **Pages**
//! via the official **Graph API**, driven by your own Facebook Developer App
//! (App ID + App Secret + OAuth). Posts/comments/replies are draft-first: nothing
//! is published until approved (or autonomy is switched to `live`).
//!
//! See `README.md` for setup and the boundary (official Graph API only — no
//! scraping, no session-token harvesting, no anti-bot evasion, no bulk posting).

#![recursion_limit = "512"]

mod api;
mod db;
mod engine;
mod fb;
mod llm;
mod mcp;

use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let port = std::env::var("PORT").unwrap_or_else(|_| "4590".to_string());
    let state = api::make_state();

    // Draft-first heartbeat: scans new comments and, per rule triggers, drafts a
    // reply or logs a notification. No-op until connected + a Page + a trigger.
    engine::spawn_heartbeat(state.clone());

    let api_router = api::api_router(state);

    let exe_dir = std::env::current_exe()
        .map(|p| p.parent().unwrap().to_path_buf())
        .unwrap_or_default();
    // App-specific and packaged paths first; generic `web/dist` last so running
    // from the repo root doesn't pick up SenClaw's own web build.
    let candidates = [
        std::path::PathBuf::from("apps/facebook-pro/web/dist"),
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
        // Allow image uploads up to 25 MB (default axum limit is 2 MB).
        .layer(axum::extract::DefaultBodyLimit::max(25 * 1024 * 1024))
        .layer(CorsLayer::permissive());

    // Loopback by default. A Space App authenticates nothing of its own — the
    // daemon reaches it over 127.0.0.1 and the UI is same-origin — so binding
    // 0.0.0.0 hands the whole REST + MCP surface to anyone on the LAN. Set
    // SENCLAW_BIND_HOST=0.0.0.0 to opt in to that explicitly.
    let host = std::env::var("SENCLAW_BIND_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let listener = tokio::net::TcpListener::bind(format!("{host}:{port}"))
        .await
        .unwrap();
    println!("SenClaw Facebook Pro running on http://{host}:{port}");
    axum::serve(listener, app).await.unwrap();
}
