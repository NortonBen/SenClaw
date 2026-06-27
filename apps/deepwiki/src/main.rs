mod api;
mod mcp;
mod watch;
mod wiki;

// Code-intelligence core (tree-sitter index + call-graph queries), formerly the
// standalone `codeindex-core` crate, now folded into DeepWiki.
mod db;
mod index;
mod lang;
mod model;
mod parse;
mod query;

use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let port = std::env::var("PORT").unwrap_or_else(|_| "4330".to_string());
    let api_router = api::api_router();

    let exe_dir = std::env::current_exe()
        .map(|p| p.parent().unwrap().to_path_buf())
        .unwrap_or_default();
    // Prefer the app's own web/dist (dev: cwd; release: next to the binary).
    // The shared cargo target dir also contains the main UI's web/dist, so the
    // app-local paths must take priority over exe-relative fallbacks.
    let candidates = [
        std::path::PathBuf::from("web/dist"),
        std::path::PathBuf::from("web_dist"),
        std::path::PathBuf::from("apps/deepwiki/web/dist"),
        exe_dir.join("web_dist"),
        exe_dir.join("web").join("dist"),
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
    println!("DeepWiki App running on http://0.0.0.0:{}", port);
    axum::serve(listener, app).await.unwrap();
}
