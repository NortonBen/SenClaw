//! SenClaw Capital Space App — quản lý nguồn vốn: các nguồn vốn (vốn chủ,
//! vay ngân hàng, hạn mức tín dụng, vốn góp…), sổ cái giải ngân/trả nợ,
//! lịch trả nợ (niên kim / gốc đều / lãi định kỳ), phân bổ vốn theo mục đích,
//! dashboard + AI phân tích qua bridge. Dữ liệu 100% local (SQLite).

mod api;
mod db;
mod finance;
mod goals;
mod insight;
mod llm;
mod mcp;

use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let port = std::env::var("PORT").unwrap_or_else(|_| "4620".to_string());
    let state = api::make_state();
    let api_router = api::api_router(state);

    let exe_dir = std::env::current_exe()
        .map(|p| p.parent().unwrap().to_path_buf())
        .unwrap_or_default();
    // App-specific and packaged paths first; generic `web/dist` last so running
    // from the repo root doesn't pick up SenClaw's own web build.
    let candidates = [
        std::path::PathBuf::from("apps/capital/web/dist"),
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
    println!("SenClaw Capital running on http://{host}:{port}");
    axum::serve(listener, app).await.unwrap();
}
