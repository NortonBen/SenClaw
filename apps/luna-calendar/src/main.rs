mod almanac;
mod api;
mod llm;
mod lunar;
mod mcp;

use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let port = std::env::var("PORT").unwrap_or_else(|_| "4351".to_string());
    let state = api::make_state();
    let api_router = api::api_router(state);

    let exe_dir = std::env::current_exe()
        .map(|p| p.parent().unwrap().to_path_buf())
        .unwrap_or_default();
    // Prefer the app's own web build. App-specific and packaged (`web_dist`) paths
    // come FIRST; the generic cwd `web/dist` is checked LAST so that running the
    // binary from the repo root doesn't pick up SenClaw's own `web/dist` — the
    // static-dir collision gotcha.
    let candidates = [
        std::path::PathBuf::from("apps/luna-calendar/web/dist"), // dev: cargo run from repo root
        std::path::PathBuf::from("web_dist"),                    // packaged: flat install cwd
        exe_dir.join("web_dist"),                                // packaged: next to the binary
        exe_dir.join("web").join("dist"),
        std::path::PathBuf::from("web/dist"),                    // last resort (may collide)
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
    println!("SenClaw Luna Calendar running on http://0.0.0.0:{}", port);
    axum::serve(listener, app).await.unwrap();
}
