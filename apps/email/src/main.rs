mod api;
mod db;
mod mailer;
mod mcp;
mod models;
mod store;

use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let port = std::env::var("PORT").unwrap_or_else(|_| "8082".to_string());

    let api_router = api::api_router();

    // Locate web/dist directory
    let exe_dir = std::env::current_exe()
        .map(|p| p.parent().unwrap().to_path_buf())
        .unwrap_or_default();

    let mut dist_path = std::path::PathBuf::from("web/dist");
    if std::path::Path::new("web_dist").exists() {
        dist_path = std::path::PathBuf::from("web_dist");
    } else if std::path::Path::new("apps/email/web/dist").exists() {
        dist_path = std::path::PathBuf::from("apps/email/web/dist");
    } else if exe_dir.join("web_dist").exists() {
        dist_path = exe_dir.join("web_dist");
    } else if exe_dir.join("web").join("dist").exists() {
        dist_path = exe_dir.join("web").join("dist");
    }

    let serve_dir = ServeDir::new(&dist_path).not_found_service(
        tower_http::services::ServeFile::new(dist_path.join("index.html")),
    );

    let app = Router::new()
        .nest("/api", api_router)
        .fallback_service(serve_dir)
        .layer(CorsLayer::permissive());

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port))
        .await
        .unwrap();
    println!("Email App running on http://0.0.0.0:{}", port);

    axum::serve(listener, app).await.unwrap();
}
