mod api;
mod db;
mod engine;
mod llm;
mod mcp;
mod senclaw;
mod workspace;

use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let port = std::env::var("PORT").unwrap_or_else(|_| "4420".to_string());
    let state = api::make_state();
    let api_router = api::api_router(state);

    let exe_dir = std::env::current_exe()
        .map(|p| p.parent().unwrap().to_path_buf())
        .unwrap_or_default();

    // App-specific + packaged `web_dist` paths come FIRST; the generic cwd
    // `web/dist` is LAST so running from the repo root doesn't pick up
    // SenClaw's own web/dist (the main "Senclaw Connect" UI).
    let candidates = [
        std::path::PathBuf::from("apps/ai-office/web/dist"), // dev: cargo run from repo root
        std::path::PathBuf::from("web_dist"),                // packaged: flat install cwd
        exe_dir.join("web_dist"),                            // packaged: next to the binary
        exe_dir.join("web").join("dist"),
        std::path::PathBuf::from("web/dist"),                // last resort (may collide)
    ];
    let dist_path = candidates
        .iter()
        .find(|c| c.join("index.html").exists())
        .cloned()
        .unwrap_or_else(|| std::path::PathBuf::from("web/dist"));

    let serve_dir = ServeDir::new(&dist_path)
        .not_found_service(ServeFile::new(dist_path.join("index.html")));

    let app = Router::new()
        .nest("/api", api_router)
        .fallback_service(serve_dir)
        .layer(CorsLayer::permissive());

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port))
        .await
        .unwrap();
    println!("SenClaw AI Office running on http://0.0.0.0:{}", port);
    axum::serve(listener, app).await.unwrap();
}
