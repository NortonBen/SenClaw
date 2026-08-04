//! SenClaw Sandbox — run commands and code isolated from the real machine.

mod api;
mod backend;
mod caps;
mod code;
mod config;
mod db;
mod files;
mod fsmode;
mod mcp;
mod monitor;
mod mounts;
mod ports;
mod pty;
mod runner;
mod settings;
mod state;
mod trace;

use axum::response::IntoResponse;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let data_dir = config::data_dir();
    std::fs::create_dir_all(&data_dir).ok();
    std::fs::create_dir_all(config::workspaces_dir()).ok();
    let db = db::Db::open(&config::db_path()).expect("open sqlite");

    let (mcp_tx, _) = tokio::sync::broadcast::channel(64);
    let app_state = state::AppState { db, mcp_tx };
    let api_router = api::api_router(app_state.clone());

    // App-specific and packaged (`web_dist`) paths come FIRST; the generic cwd
    // `web/dist` is checked LAST so running from the repo root doesn't pick up
    // SenClaw's own `web/dist`.
    let exe_dir = std::env::current_exe()
        .map(|p| p.parent().unwrap().to_path_buf())
        .unwrap_or_default();
    let candidates = [
        std::path::PathBuf::from("apps/sandbox/web/dist"),
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
    // 404 status onto every client-side route. The fallback serves index.html
    // only for extension-less paths; a missed ASSET path gets a plain 404,
    // because handing the browser text/html for a module script is the
    // blank-page failure other apps here shipped with.
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
                    "The Sandbox UI has not been built (web_dist/index.html is missing)",
                )
                    .into_response(),
            }
        }
    });
    let serve_dir = ServeDir::new(&dist_path).fallback(spa_index);

    let app = axum::Router::new()
        .nest("/api", api_router)
        .fallback_service(serve_dir)
        .layer(CorsLayer::permissive());

    // Loopback by default, and it matters more here than in any other Space
    // App: this one executes arbitrary commands on request, so a port on
    // 0.0.0.0 is an unauthenticated remote shell for the whole LAN.
    let host = config::bind_host();
    let port = config::http_port();
    let listener = tokio::net::TcpListener::bind(format!("{host}:{port}"))
        .await
        .unwrap_or_else(|e| panic!("bind {host}:{port}: {e}"));

    let c = caps::probe(true).await;
    println!("SenClaw Sandbox running on http://{host}:{port}");
    println!(
        "  backends available: {}",
        if c.backends.is_empty() {
            "(none)".to_string()
        } else {
            c.backends.join(", ")
        }
    );
    println!("  direct: {} — {}", c.direct.kind.as_str(), c.direct.detail);
    println!("  docker: {}", c.docker.detail);

    axum::serve(listener, app).await.unwrap();
}
