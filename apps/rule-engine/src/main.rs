//! Rule Engine — a SenClaw Space App.
//!
//! Graph-shaped data flows: sources push events, nodes filter/transform/branch
//! through named ports, edges carry the result.

#![recursion_limit = "512"]

mod api;
mod config;
mod daq;
mod db;
mod engine;
mod expr;
mod mcp;
mod model;
mod rules;
mod state;
#[cfg(test)]
mod testkit;

use std::path::PathBuf;

use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    let port = config::http_port();

    let state = match state::boot() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[rule-engine] không khởi tạo được: {e}");
            std::process::exit(1);
        }
    };

    let dist_path = resolve_dist();
    let serve_dir =
        ServeDir::new(&dist_path).not_found_service(ServeFile::new(dist_path.join("index.html")));

    let app = Router::new()
        .nest("/api", api::api_router(state.clone()))
        .fallback_service(serve_dir)
        .layer(CorsLayer::permissive());

    // Bind loopback, not 0.0.0.0. This app has no authentication (the daemon
    // reaches it over localhost and the UI is same-origin), and its config
    // stores plaintext secrets — bot tokens, API keys — plus lets a caller
    // delete any chain. Loopback keeps it off the LAN. A page in the user's own
    // browser can still reach 127.0.0.1, so treat this as localhost-trust, not
    // a security boundary. `RULE_ENGINE_BIND` overrides for containerized runs.
    let host = std::env::var("RULE_ENGINE_BIND").unwrap_or_else(|_| "127.0.0.1".to_string());
    let addr = format!("{host}:{port}");
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[rule-engine] không bind được {addr}: {e}");
            std::process::exit(1);
        }
    };
    println!(
        "[rule-engine] http://127.0.0.1:{port}  (UI: {})",
        dist_path.display()
    );

    // Deploy chains only after the port answers: the daemon health-gates a
    // Space App for 30s, and a source with a slow first tick would eat it.
    let boot_state = state.clone();
    tokio::spawn(async move {
        state::resume_active_chains(&boot_state).await;
    });
    state::spawn_janitor(state.clone());

    if let Err(e) = axum::serve(listener, app).await {
        eprintln!("[rule-engine] máy chủ dừng: {e}");
    }
}

/// App-specific paths first: running `cargo run -p rule-engine` from the repo
/// root would otherwise pick up SenClaw's own `web/dist`.
fn resolve_dist() -> PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_default();
    let candidates = [
        PathBuf::from("apps/rule-engine/web/dist"),
        PathBuf::from("web_dist"),
        exe_dir.join("web_dist"),
        exe_dir.join("web").join("dist"),
        PathBuf::from("web/dist"),
    ];
    candidates
        .iter()
        .find(|c| c.join("index.html").exists())
        .cloned()
        .unwrap_or_else(|| PathBuf::from("web/dist"))
}
