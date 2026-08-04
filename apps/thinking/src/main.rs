//! SenClaw Thinking Space App — phân tích & đánh giá vấn đề, đề xuất giải pháp
//! và ra quyết định theo hai phương pháp kinh điển: **6 Mũ Tư Duy** (Six
//! Thinking Hats — de Bono) và **5W** (Who/What/When/Where/Why). Dữ liệu 100%
//! local (SQLite); AI chỉ được gọi qua bridge SenClaw. App KHÔNG tự thực thi
//! quyết định nào — nó là sổ tay tư duy + trợ lý phân tích.

mod api;
mod db;
mod llm;
mod logic;
mod mcp;

use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let port = std::env::var("PORT").unwrap_or_else(|_| "4650".to_string());
    let state = api::make_state();
    let api_router = api::api_router(state);

    let exe_dir = std::env::current_exe()
        .map(|p| p.parent().unwrap().to_path_buf())
        .unwrap_or_default();
    // App-specific and packaged paths first; generic `web/dist` last so running
    // from the repo root doesn't pick up SenClaw's own web build.
    let candidates = [
        std::path::PathBuf::from("apps/thinking/web/dist"),
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
    println!("SenClaw Thinking running on http://{host}:{port}");
    axum::serve(listener, app).await.unwrap();
}
