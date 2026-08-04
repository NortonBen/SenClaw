//! SenClaw Kaen — vocabulary SRS Space App, ported from the Kaizen monorepo
//! (NestJS + PostgreSQL → axum + SQLite, single-user, no auth).

mod api;
mod config;
mod db;
mod dictation;
mod dictionary;
mod grammar;
mod llm;
mod mcp;
mod ops;
mod srs;
mod state;
mod story;

use axum::response::IntoResponse;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let data_dir = config::data_dir();
    std::fs::create_dir_all(&data_dir).ok();
    let db = db::Db::open(&config::db_path()).expect("open sqlite");

    let (mcp_tx, _) = tokio::sync::broadcast::channel(64);
    let app_state = state::AppState { db, mcp_tx };
    let api_router = api::api_router(app_state.clone());
    let root_router = api::root_router(app_state);

    // App-specific and packaged (`web_dist`) paths come FIRST; the generic cwd
    // `web/dist` is checked LAST so running from the repo root doesn't pick up
    // SenClaw's own `web/dist`.
    let exe_dir = std::env::current_exe()
        .map(|p| p.parent().unwrap().to_path_buf())
        .unwrap_or_default();
    let candidates = [
        std::path::PathBuf::from("apps/kaen/web/dist"),
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
    // 404 status onto every client-side route, so the UI renders but proxies
    // and health checks see a failure.
    // The fallback serves index.html only for extension-less paths (client-side
    // routes). A missed ASSET path (anything with a file extension, e.g.
    // /dictation/listen/assets/app.js) gets a plain 404 instead — serving HTML
    // there would hand the browser text/html for a module script, the exact
    // blank-page failure this app shipped with under `base: './'`.
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
                    "Kaen UI chưa được build (thiếu web_dist/index.html)",
                )
                    .into_response(),
            }
        }
    });
    let serve_dir = ServeDir::new(&dist_path).fallback(spa_index);

    let app = axum::Router::new()
        .nest("/api", api_router)
        .merge(root_router)
        .fallback_service(serve_dir)
        .layer(CorsLayer::permissive());

    let port = config::http_port();
    // Loopback by default. A Space App authenticates nothing of its own — the
    // daemon reaches it over 127.0.0.1 and the UI is same-origin — so binding
    // 0.0.0.0 hands the whole REST + MCP surface to anyone on the LAN. Set
    // SENCLAW_BIND_HOST=0.0.0.0 to opt in to that explicitly.
    let host = std::env::var("SENCLAW_BIND_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let listener = tokio::net::TcpListener::bind(format!("{host}:{port}"))
        .await
        .unwrap();
    println!("SenClaw Kaen running on http://{host}:{port}");
    axum::serve(listener, app).await.unwrap();
}
