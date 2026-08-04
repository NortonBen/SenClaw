mod api;
mod db;
mod google;
mod mcp;

use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

/// `/health` mirrors `/api/status` — the manifest healthPath the daemon polls.
async fn health(state: axum::extract::State<Arc<api::AppState>>) -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "status": "ok",
        "app": "google-workspace",
        "connected": state.google.db.connected(),
    }))
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(4310);

    let db = Arc::new(db::Db::open_default().expect("open sqlite"));
    let state = api::make_state(db, port);
    let api_router = api::api_router(state.clone());

    let exe_dir = std::env::current_exe()
        .map(|p| p.parent().unwrap().to_path_buf())
        .unwrap_or_default();
    let candidates = [
        std::path::PathBuf::from("apps/google-workspace/web/dist"),
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

    // SPA fallback via `fallback`, NOT `not_found_service`: only extension-less
    // paths get index.html; a missed asset path keeps a plain 404 rather than
    // handing the browser HTML for a module script.
    let index_path = dist_path.join("index.html");
    let spa_index = axum::routing::get(move |uri: axum::http::Uri| {
        let p = index_path.clone();
        async move {
            let last_segment = uri.path().rsplit('/').next().unwrap_or("");
            if last_segment.contains('.') {
                return (axum::http::StatusCode::NOT_FOUND, "not found").into_response();
            }
            match tokio::fs::read(&p).await {
                Ok(bytes) => (
                    [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
                    bytes,
                )
                    .into_response(),
                Err(_) => (
                    axum::http::StatusCode::NOT_FOUND,
                    "Google Workspace UI chưa được build (thiếu web_dist/index.html)",
                )
                    .into_response(),
            }
        }
    });
    let serve_dir = ServeDir::new(&dist_path).fallback(spa_index);

    let app = Router::new()
        .route("/health", get(health).with_state(state))
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
    println!("SenClaw Google Workspace running on http://{host}:{port}");
    axum::serve(listener, app).await.unwrap();
}
