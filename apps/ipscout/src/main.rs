//! SenClaw ipscout Space App — điều tra một địa chỉ IP hoặc máy chủ: nó là ai
//! (ASN, tổ chức, dải CIDR, abuse), ở đâu (địa lý kèm độ tin), traffic đi qua
//! đâu (CDN/cloud đứng trước), cổng nào mở, cổng đó chạy ứng dụng gì phiên bản
//! nào, và hệ điều hành là gì.
//!
//! Hai lớp tách theo việc **có gửi gói tin tới mục tiêu hay không**: hồ sơ thụ
//! động (chạy với IP bất kỳ) và bề mặt chủ động (đòi xác minh quyền sở hữu).
//! **Không có lớp khai thác** — đó là ranh giới thiết kế, không phải giai đoạn
//! chưa tới.
//!
//! Thiết kế đầy đủ: docs/ip-investigation-app-design.md

mod api;
mod arp;
mod banner;
mod db;
mod geo;
mod investigate;
mod mcp;
mod netclass;
mod osguess;
mod registry;
mod rep;
mod resolve;
mod risk;
mod scan;
mod scope;
mod trace;

use axum::Router;
use tower_http::services::{ServeDir, ServeFile};

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let port = std::env::var("PORT").unwrap_or_else(|_| "4710".to_string());
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
        std::path::PathBuf::from("apps/ipscout/web/dist"),
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

    // KHÔNG có CorsLayer::permissive() ở đây, dù nhiều Space App khác vẫn có.
    // `Access-Control-Allow-Origin: *` trên một dịch vụ loopback không xác thực
    // nghĩa là *bất kỳ trang web nào người dùng đang mở* cũng đọc được API này
    // qua trình duyệt. App này giữ token xác minh sở hữu và kết quả điều tra hạ
    // tầng, nên càng không được mở.
    //
    // Không cần CORS để chạy: web UI do chính binary này phục vụ nên gọi `/api/*`
    // là same-origin, còn daemon gọi phía server chứ không qua trình duyệt.
    let app = Router::new()
        .nest("/api", api_router)
        .fallback_service(serve_dir);

    let listener = tokio::net::TcpListener::bind(format!("{host}:{port}"))
        .await
        .unwrap();
    println!("SenClaw ipscout chạy tại http://{host}:{port}");
    axum::serve(listener, app).await.unwrap();
}
