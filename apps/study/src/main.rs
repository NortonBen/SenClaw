//! SenClaw Study — turn uploaded material into a dated study plan with
//! flashcards, quizzes and cited answers.

mod api;
mod ask;
mod calendar;
mod cards;
mod config;
mod corpus;
mod db;
mod ingest;
mod lesson;
mod llm;
mod mcp;
mod outline;
mod planner;
mod quiz;
mod sources;
mod srs;
mod state;
mod tts;

use axum::response::IntoResponse;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let data_dir = config::data_dir();
    std::fs::create_dir_all(&data_dir).ok();
    std::fs::create_dir_all(config::audio_dir()).ok();
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
        std::path::PathBuf::from("apps/study/web/dist"),
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
    // and health checks see a failure. The fallback serves index.html only for
    // extension-less paths; a missed ASSET path gets a plain 404, because
    // handing the browser text/html for a module script is the blank-page
    // failure other apps here shipped with.
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
                    "Study UI chưa được build (thiếu web_dist/index.html)",
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

    // Loopback by default. A Space App on 0.0.0.0 is how this repo previously
    // exposed app APIs to everyone on the network.
    let host = config::bind_host();
    let port = config::http_port();
    let listener = tokio::net::TcpListener::bind(format!("{host}:{port}"))
        .await
        .unwrap_or_else(|e| panic!("bind {host}:{port}: {e}"));
    println!("SenClaw Study running on http://{host}:{port}");
    axum::serve(listener, app).await.unwrap();
}
