//! SenClaw secscan Space App — quét bảo mật website & máy chủ **của chính
//! mình**: security header, cờ cookie, lộ thông tin, tư thế DNS (SPF/DMARC/
//! CAA/DNSSEC), chấm điểm A+..F và so sánh giữa các lần quét.
//!
//! Ba lớp tách theo mức xâm nhập — L1 thụ động (đang có), L2 chủ động nhẹ và
//! L3 host qua SSH (chưa làm). **Không có lớp khai thác**: đó là ranh giới
//! thiết kế, không phải giai đoạn chưa tới.
//!
//! Thiết kế đầy đủ: docs/security-scan-app-research.md

mod active;
mod api;
mod custom;
mod db;
mod dns;
mod host;
mod mcp;
mod probe;
mod rules;
mod scan;
mod scope;
mod score;
mod tls;
mod vuln;

use axum::Router;
use tower_http::services::{ServeDir, ServeFile};

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let port = std::env::var("PORT").unwrap_or_else(|_| "4690".to_string());
    // Mặc định chỉ nghe loopback. Bind 0.0.0.0 phơi API ra cả LAN mà app không
    // có lớp xác thực nào — muốn truy cập từ máy khác thì phải khai tường minh.
    let host = std::env::var("SENCLAW_BIND_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());

    let state = api::make_state();
    let api_router = api::api_router(state);

    let exe_dir = std::env::current_exe()
        .map(|p| p.parent().unwrap().to_path_buf())
        .unwrap_or_default();
    // Đường dẫn riêng của app và đường dẫn đóng gói trước; `web/dist` chung để
    // cuối cùng, để chạy từ gốc repo không vớ nhầm bản build web của SenClaw.
    let candidates = [
        std::path::PathBuf::from("apps/secscan/web/dist"),
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

    // KHÔNG có CorsLayer::permissive() ở đây, dù các Space App khác đều có.
    // `Access-Control-Allow-Origin: *` trên một dịch vụ loopback không xác thực
    // nghĩa là *bất kỳ trang web nào người dùng đang mở* cũng đọc được API này
    // qua trình duyệt — đúng lỗ hổng vừa xác nhận là CRITICAL ở daemon
    // (`/api/llm-config` lộ apiKey). App này giữ kết quả quét và token xác minh
    // sở hữu, nên càng không được lặp lại.
    //
    // Không cần CORS để chạy: web UI được chính binary này phục vụ nên gọi
    // `/api/*` là same-origin, còn daemon gọi phía server chứ không qua trình duyệt.
    let app = Router::new()
        .nest("/api", api_router)
        .fallback_service(serve_dir);

    let listener = tokio::net::TcpListener::bind(format!("{host}:{port}"))
        .await
        .unwrap();
    println!("SenClaw secscan chạy tại http://{host}:{port}");
    axum::serve(listener, app).await.unwrap();
}
