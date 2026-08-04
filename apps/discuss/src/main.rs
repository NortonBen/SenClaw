//! AI Discuss Team — Space App bootstrap (khung apps/ba + apps/study).

mod api;
mod config;
mod db;
mod engine;
mod llm;
mod mcp;
mod parse;

use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    std::fs::create_dir_all(config::data_dir()).ok();

    let state = api::make_state();
    engine::spawn_scheduler(state.clone());

    // Dò web dist: path app-specific + absolute exe_dir TRƯỚC, path trần cuối —
    // chạy từ repo root không được nuốt web/dist của SenClaw; repack xoá cwd
    // thì path tuyệt đối theo exe vẫn resolve.
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_default();
    let candidates = [
        std::path::PathBuf::from("apps/discuss/web/dist"),
        exe_dir.join("web_dist"),
        exe_dir.join("web/dist"),
        std::path::PathBuf::from("web_dist"),
        std::path::PathBuf::from("web/dist"),
    ];
    let dist_path = candidates
        .iter()
        .find(|p| p.join("index.html").exists())
        .cloned()
        .unwrap_or_else(|| std::path::PathBuf::from("web_dist"));

    // SPA fallback: đường dẫn có dấu chấm ở segment cuối (asset) → 404 thật,
    // không trả index.html (browser nhận text/html cho module script = trang trắng).
    let index = dist_path.join("index.html");
    let spa = ServeDir::new(&dist_path).fallback(ServeFile::new(index));

    let app = Router::new()
        .nest("/api", api::api_router(state))
        .fallback_service(spa)
        .layer(CorsLayer::permissive());

    let host = config::bind_host();
    let port = config::http_port();
    let listener = tokio::net::TcpListener::bind(format!("{host}:{port}"))
        .await
        .unwrap_or_else(|e| panic!("không bind được {host}:{port}: {e}"));
    println!("AI Discuss Team chạy tại http://{host}:{port} (dist: {})", dist_path.display());
    axum::serve(listener, app).await.unwrap();
}
