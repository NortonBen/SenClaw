//! SenClaw Rewrite Story — AI story rewriter Space App.
//!
//! Ported from the Go/Gin + Postgres `re-write-story` backend. Takes a source
//! story, splits it with a Vietnamese-aware hybrid splitter, and rewrites it
//! chunk by chunk through SenClaw's shared LLM, persisting each finished chunk
//! so a failed run resumes instead of restarting.

// The MCP `tools_list()` catalogue is one giant `json!` literal; bump past the
// 128-arm default so the macro expands cleanly.
#![recursion_limit = "512"]

mod api;
mod config;
mod dashws;
mod db;
mod export;
mod llm;
mod mcp;
mod process;
mod state;
mod text;

use axum::response::IntoResponse;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let core = state::Core::boot().expect("boot core");

    // Clear rows left `processing` by a crash before the poller can see them.
    process::reconcile_orphans(&core.db);
    process::spawn(core.clone());

    let (mcp_tx, _) = tokio::sync::broadcast::channel(64);
    let app_state = state::AppState {
        core: core.clone(),
        mcp_tx,
    };
    let api_router = api::api_router(app_state.clone());
    let root_router = api::root_router(app_state);

    // App-specific and packaged (`web_dist`) paths come FIRST; the generic cwd
    // `web/dist` is checked LAST so running from the repo root doesn't pick up
    // SenClaw's own `web/dist`.
    let exe_dir = std::env::current_exe()
        .map(|p| p.parent().unwrap().to_path_buf())
        .unwrap_or_default();
    let candidates = [
        std::path::PathBuf::from("apps/rewrite-story/web/dist"),
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
    // route would answer "404 + the app HTML" — the UI renders but proxies and
    // health checks see a failure.
    let index_path = dist_path.join("index.html");
    let spa_index = axum::routing::get(move || {
        let p = index_path.clone();
        async move {
            match tokio::fs::read(&p).await {
                Ok(bytes) => (
                    [(
                        axum::http::header::CONTENT_TYPE,
                        "text/html; charset=utf-8",
                    )],
                    bytes,
                )
                    .into_response(),
                Err(_) => (
                    axum::http::StatusCode::NOT_FOUND,
                    "Rewrite Story UI chưa được build (thiếu web_dist/index.html)",
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
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
        .await
        .unwrap();
    println!("SenClaw Rewrite Story running on http://0.0.0.0:{port}");
    axum::serve(listener, app).await.unwrap();
}
