//! SenClaw Siêu Dự Đoán Space App — AI forecasting across football (Elo +
//! Poisson on real ClubElo/TheSportsDB data), XSMB lottery *statistics*
//! (honest, disclaimer hard-coded), weather (Open-Meteo) and gold/FX trends —
//! with the differentiator: EVERY forecast lands in a ledger that auto-resolves
//! against real outcomes and reports Brier/accuracy/calibration publicly.
//!
//! All Phase-1 data sources are keyless. See docs/sieu-du-doan-app-design.md.

mod api;
mod builder;
mod db;
mod engine;
mod evidence;
mod fetch;
mod football;
mod ledger;
mod llm;
mod lottery;
mod market;
mod mcp;
mod methodology;
mod timeutil;
mod topic;

use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let port = std::env::var("PORT").unwrap_or_else(|_| "4600".to_string());
    let state = api::make_state();

    // Background loop: staleness-aware fetches + ledger auto-resolve.
    engine::spawn_scheduler(state.clone());

    let api_router = api::api_router(state);

    let exe_dir = std::env::current_exe()
        .map(|p| p.parent().unwrap().to_path_buf())
        .unwrap_or_default();
    // App-specific and packaged paths first; generic `web/dist` last so running
    // from the repo root doesn't pick up SenClaw's own web build.
    let candidates = [
        std::path::PathBuf::from("apps/predict/web/dist"),
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

    // Loopback by default. A Space App authenticates nothing of its own — the
    // daemon reaches it over 127.0.0.1 and the UI is same-origin — so binding
    // 0.0.0.0 hands the whole REST + MCP surface to anyone on the LAN. Set
    // SENCLAW_BIND_HOST=0.0.0.0 to opt in to that explicitly.
    let host = std::env::var("SENCLAW_BIND_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let listener = tokio::net::TcpListener::bind(format!("{host}:{port}"))
        .await
        .unwrap();
    println!("SenClaw Siêu Dự Đoán running on http://{host}:{port}");
    axum::serve(listener, app).await.unwrap();
}
