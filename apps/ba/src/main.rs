//! SenClaw BA Studio Space App — trợ lý Business Analyst trọn vòng đời:
//! 9 giai đoạn làm việc, workflow, AI sinh 31 loại tài liệu có truy vết,
//! change request đồng bộ, dashboard. Xem docs/ba-app-design.md.

mod api;
mod config;
mod cr;
mod db;
mod engine;
mod export;
mod llm;
mod mcp;
mod state;
mod templates;
mod trace;

use axum::response::IntoResponse;
use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let port = config::http_port();
    let state = api::make_state();
    let api_router = api::api_router(state);

    let exe_dir = std::env::current_exe()
        .map(|p| p.parent().unwrap().to_path_buf())
        .unwrap_or_default();
    // App-specific and packaged paths first; generic `web/dist` last so running
    // from the repo root doesn't pick up SenClaw's own web build. Absolute
    // exe_dir paths TRƯỚC các path tương đối: repack `rm -rf release/` xoá cwd
    // của process đang chạy — path tương đối chết theo cwd, path tuyệt đối
    // vẫn resolve sang thư mục mới (bẫy "Space App stale cwd").
    let candidates = [
        std::path::PathBuf::from("apps/ba/web/dist"),
        exe_dir.join("web_dist"),
        exe_dir.join("web").join("dist"),
        std::path::PathBuf::from("web_dist"),
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
                    "BA Studio UI chưa được build (thiếu web_dist/index.html)",
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

    let host = config::bind_host();
    let listener = tokio::net::TcpListener::bind(format!("{host}:{port}"))
        .await
        .unwrap();
    println!("SenClaw BA Studio running on http://{host}:{port}");
    axum::serve(listener, app).await.unwrap();
}
