mod analyze;
mod api;
mod codec;
mod convert;
mod fmt;
mod mcp;

use axum::response::IntoResponse;
use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let port = std::env::var("PORT").unwrap_or_else(|_| "4540".to_string());
    let state = api::make_state();
    let api_router = api::api_router(state);

    let exe_dir = std::env::current_exe()
        .map(|p| p.parent().unwrap().to_path_buf())
        .unwrap_or_default();
    let candidates = [
        std::path::PathBuf::from("apps/json/web/dist"),
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

    // SPA fallback via `fallback`, NOT `not_found_service`: the latter forces a
    // 404 status onto every client-side route (/json-to-csv, /json-diff …), so
    // the UI renders but proxies and health checks see a failure.
    // Only extension-less paths get index.html; a missed asset path keeps a
    // plain 404 rather than handing the browser HTML for a module script.
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
                    "JSON Tools UI chưa được build (thiếu web_dist/index.html)",
                )
                    .into_response(),
            }
        }
    });
    let serve_dir = ServeDir::new(&dist_path).fallback(spa_index);

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
    println!("SenClaw JSON Tools running on http://{host}:{port}");
    axum::serve(listener, app).await.unwrap();
}
