// Lakehouse Space App — data lake + warehouse. Thiết kế: docs/data-lake-app-design.md
// Catalogue json! trong mcp.rs vượt recursion budget mặc định (như search/crm).
#![recursion_limit = "512"]

mod api;
mod config;
mod connectors;
mod dashws;
mod db;
mod engine;
mod export;
mod flow;
mod generate;
mod ingest;
mod lake;
mod mcp;
mod runner;
mod sync;
mod transform;
mod transport;

use axum::response::IntoResponse;
use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let port = config::http_port();
    let state = api::make_state().expect("lakehouse: không mở được catalog");
    let api_router = api::api_router(state.clone());

    // Reconcile trước khi nhận việc mới: run mồ côi -> failed, file ngoài manifest bị dọn.
    lake::boot_reconcile(&state);
    runner::spawn(state.clone());
    spawn_maintenance_tick(state.clone());

    let exe_dir = std::env::current_exe()
        .map(|p| p.parent().unwrap().to_path_buf())
        .unwrap_or_default();
    // Đường dẫn app-specific trước, `web/dist` chung CUỐI CÙNG — chạy từ repo root
    // mà đặt web/dist trước sẽ vớ nhầm UI chính của SenClaw.
    let candidates = [
        std::path::PathBuf::from("apps/lakehouse/web/dist"),
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

    // SPA fallback qua `fallback`, KHÔNG `not_found_service` (ép 404 lên mọi client
    // route — UI vẫn hiện nhưng health check/proxy thấy lỗi). Path có đuôi file giữ 404 thật.
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
                    "Lakehouse UI chưa được build (thiếu web_dist/index.html)",
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

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port))
        .await
        .unwrap();
    println!("SenClaw Lakehouse running on http://0.0.0.0:{}", port);
    axum::serve(listener, app).await.unwrap();
}

/// Tick bảo trì mỗi 60s: GC file tombstone quá grace; run_log sweep TỐI ĐA 1 lần/ngày
/// (theo `log_retention_days`). Chạy nền, lỗi chỉ log — không được làm chết daemon.
fn spawn_maintenance_tick(state: api::AppState) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(60));
        let mut last_sweep_day = String::new();
        loop {
            ticker.tick().await;
            match lake::gc(&state.db) {
                Ok(n) if n > 0 => println!("lakehouse gc: xóa {n} file tombstone"),
                Ok(_) => {}
                Err(e) => eprintln!("lakehouse gc lỗi: {e}"),
            }
            let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
            if today != last_sweep_day {
                let retention = state.db.setting_i64("log_retention_days", 14);
                match state.db.run_log_sweep(retention) {
                    Ok(n) if n > 0 => println!("lakehouse run_log sweep: xóa {n} dòng cũ"),
                    Ok(_) => {}
                    Err(e) => eprintln!("lakehouse run_log sweep lỗi: {e}"),
                }
                last_sweep_day = today;
            }
        }
    });
}
