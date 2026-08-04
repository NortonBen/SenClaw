//! SenClaw TikTok Downloader Space App — tải video/ảnh/nhạc TikTok về máy:
//! bản không logo / HD / có logo, tách nhạc MP3, trọn bộ post ảnh, tải hàng
//! loạt từ danh sách link và cả trang cá nhân (best-effort). Hàng đợi chạy
//! nền với tiến trình, lịch sử tìm kiếm được, cài đặt lưu SQLite. Chỉ tải
//! post CÔNG KHAI, phục vụ lưu trữ cá nhân — tôn trọng bản quyền tác giả.

mod api;
mod db;
mod download;
mod mcp;
mod tiktok;

use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let port = std::env::var("PORT").unwrap_or_else(|_| "4670".to_string());
    // Mặc định chỉ nghe loopback. Bind 0.0.0.0 phơi API ra cả LAN mà app không
    // có lớp xác thực nào — `GET /api/downloads/:id/file` trả thẳng video đã
    // tải về. Muốn truy cập từ máy khác thì phải khai tường minh.
    let host = std::env::var("SENCLAW_BIND_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let state = api::make_state();

    // Worker pool: claims queued jobs (also ones left over from a previous
    // run — the DB reset stale actives back to queued on open).
    tokio::spawn(download::run_supervisor(state.worker_ctx()));

    let api_router = api::api_router(state);

    let exe_dir = std::env::current_exe()
        .map(|p| p.parent().unwrap().to_path_buf())
        .unwrap_or_default();
    // App-specific and packaged paths first; generic `web/dist` last so running
    // from the repo root doesn't pick up SenClaw's own web build.
    let candidates = [
        std::path::PathBuf::from("apps/tiktok-dl/web/dist"),
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

    let serve_dir =
        ServeDir::new(&dist_path).not_found_service(ServeFile::new(dist_path.join("index.html")));

    let app = Router::new()
        .nest("/api", api_router)
        .fallback_service(serve_dir)
        .layer(CorsLayer::permissive());

    let listener = tokio::net::TcpListener::bind(format!("{host}:{port}"))
        .await
        .unwrap();
    println!("SenClaw TikTok Downloader running on http://{host}:{port}");
    axum::serve(listener, app).await.unwrap();
}
