// The MCP `tools_list()` catalogue is one giant `json!` literal; bump past the
// 128-arm default so the macro expands cleanly.
#![recursion_limit = "512"]

mod agents;
mod api;
mod config;
mod context;
mod dag;
mod dashws;
mod db;
mod extbridge;
mod llm;
mod material;
mod mcp;
mod media;
mod mediastore;
mod pipeline;
mod process;
mod script;
mod skillcat;
mod souls;
mod state;
mod steps;
mod tools;
mod tts;
mod wfclient;
mod wfdef;
mod worker;

use axum::response::IntoResponse;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let core = state::Core::boot().expect("boot core");
    material::seed(&core.db);

    let pool = agents::Pool::new(core.clone());
    let engine = dag::Engine::new(core.clone(), pool.clone());
    engine.clone().start();

    // A PROCESSING request cannot survive a restart — resolve the leftovers so
    // the UI never shows a spinner that will never finish.
    worker::reconcile_stale_requests(&core);

    // Worker: video/upscale request queue → extension bridge.
    if config::worker_enabled() {
        worker::spawn(core.clone());
    }
    worker::install_extension_event_handler(core.clone());

    // Dedicated extension WS server (the Chrome extension dials :9222).
    tokio::spawn(extbridge::serve_ws(core.ext.clone(), config::ws_port()));

    let (mcp_tx, _) = tokio::sync::broadcast::channel(64);
    let app_state = state::AppState {
        core: core.clone(),
        pool,
        engine,
        mcp_tx,
    };
    let api_router = api::api_router(app_state.clone());
    let root_router = api::root_router(app_state);

    let exe_dir = std::env::current_exe()
        .map(|p| p.parent().unwrap().to_path_buf())
        .unwrap_or_default();
    // App-specific and packaged (`web_dist`) paths come FIRST; the generic cwd
    // `web/dist` is checked LAST so running from the repo root doesn't pick up
    // SenClaw's own `web/dist` (the static-dir collision gotcha).
    let candidates = [
        std::path::PathBuf::from("apps/video-flow/web/dist"),
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
    // 404 status onto whatever the inner service returns, so every client-side
    // route (/settings, /projects, …) answered "404 + the app HTML" — the UI
    // rendered but proxies and health checks saw a failure.
    let index_path = dist_path.join("index.html");
    let spa_index = axum::routing::get(move || {
        let p = index_path.clone();
        async move {
            match tokio::fs::read(&p).await {
                Ok(bytes) => (
                    [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
                    bytes,
                )
                    .into_response(),
                Err(_) => (
                    axum::http::StatusCode::NOT_FOUND,
                    "Video Flow UI chưa được build (thiếu web_dist/index.html)",
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
    println!("SenClaw Video Flow running on http://{host}:{port}");
    axum::serve(listener, app).await.unwrap();
}
