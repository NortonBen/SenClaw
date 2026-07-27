//! SenClaw Shopee Space App — connects an agent to a Shopee **seller** shop via
//! the official Open Platform v2 API (OAuth, orders, buyer↔seller Chat). CSKW
//! replies are draft-first: nothing is sent to a customer until approved.
//!
//! See `docs/shopee-app-research.md` for the architecture and the boundary
//! (official OAuth only — no session-token harvesting, no anti-bot evasion, no
//! mass messaging).

#![recursion_limit = "512"]

mod api;
mod db;
mod engine;
mod llm;
mod mcp;
mod shopee;

use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let port = std::env::var("PORT").unwrap_or_else(|_| "4492".to_string());
    let state = api::make_state();

    // Draft-first heartbeat: reads unread buyer messages and queues CSKH reply
    // drafts. No-op until a shop is connected and autonomy is draft/live.
    engine::spawn_heartbeat(state.clone());

    let api_router = api::api_router(state);

    let exe_dir = std::env::current_exe()
        .map(|p| p.parent().unwrap().to_path_buf())
        .unwrap_or_default();
    // App-specific and packaged paths first; generic `web/dist` last so running
    // from the repo root doesn't pick up SenClaw's own web build.
    let candidates = [
        std::path::PathBuf::from("apps/shopee/web/dist"),
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
    println!("SenClaw Shopee running on http://0.0.0.0:{}", port);
    axum::serve(listener, app).await.unwrap();
}
