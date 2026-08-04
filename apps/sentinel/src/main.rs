//! Sentinel — giám sát & điều tra bảo mật cho chính SenClaw.
//!
//! App đọc dấu vết hoạt động của agent (tool đã chạy, lịch đã đặt và đã chạy,
//! phê duyệt, tin nhắn), chép sang một kho chỉ-thêm có chuỗi băm, chạy bộ luật
//! phát hiện tất định, rồi cho con người điều tra: dòng thời gian, pivot, hồ sơ
//! vụ việc. Thiết kế đầy đủ: `docs/sentinel-app-design.md`.
//!
//! Ba ranh giới cố ý:
//! * **Chỉ đọc** DB daemon (`mode=ro` + `query_only`); app không bao giờ ghi.
//! * **Chỉ quan sát** — không đứng chắn trên đường thực thi tool. Việc chặn là
//!   của `src/zen_core/permissions.rs`, không phải của app này.
//! * **Luật là mã Rust**, AI chỉ diễn giải. Đầu vào của app chính là nội dung
//!   do agent sinh ra và có thể chứa prompt injection.

mod api;
mod db;
mod ingest;
mod llm;
mod mcp;
mod redact;
mod rules;
mod snapshot;
mod source;

use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let port = std::env::var("PORT").unwrap_or_else(|_| "4680".to_string());
    let state = api::make_state();

    // Ingest + chụp ảnh cấu hình chạy nền. Lượt đầu chạy ngay để giao diện có
    // dữ liệu, sau đó theo chu kỳ.
    {
        let bg = state.clone();
        tokio::spawn(async move {
            loop {
                let _ = api::tick(&bg).await;
                tokio::time::sleep(std::time::Duration::from_secs(api::TICK_SECS)).await;
            }
        });
    }

    let api_router = api::api_router(state);

    let exe_dir = std::env::current_exe()
        .map(|p| p.parent().unwrap().to_path_buf())
        .unwrap_or_default();
    // Đường dẫn riêng của app và đường dẫn đóng gói đứng trước; `web/dist` chung
    // phải ở CUỐI, nếu không chạy từ gốc repo sẽ nuốt nhầm web build của SenClaw.
    let candidates = [
        std::path::PathBuf::from("apps/sentinel/web/dist"),
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

    // CỐ Ý khác mọi Space App khác: bind loopback, không phải 0.0.0.0.
    // Các app khác lộ cổng ra LAN; app này chứa toàn bộ lịch sử hoạt động của
    // agent (kể cả nội dung tool result), nên không được phép nghe ngoài máy.
    // Đặt SENTINEL_BIND để ghi đè nếu thật sự cần (ví dụ chạy trong container).
    let bind = std::env::var("SENTINEL_BIND").unwrap_or_else(|_| "127.0.0.1".to_string());
    let listener = match tokio::net::TcpListener::bind(format!("{bind}:{port}")).await {
        Ok(l) => l,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            // Trường hợp thường gặp nhất: daemon đã tự chạy bản đã cài của app
            // trên đúng cổng này. Panic với "Address already in use" trần trụi
            // không nói lên điều đó, nên nói thẳng ra.
            eprintln!(
                "Cổng {port} đang bận — nhiều khả năng daemon SenClaw đã chạy sẵn bản Sentinel \
                 đã cài. Mở http://{bind}:{port} để dùng bản đó, hoặc đặt PORT=<cổng khác> \
                 nếu muốn chạy song song một bản dev."
            );
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("Không mở được {bind}:{port}: {e}");
            std::process::exit(1);
        }
    };
    println!("SenClaw Sentinel running on http://{bind}:{port}");
    if let Err(e) = axum::serve(listener, app).await {
        eprintln!("Máy chủ dừng: {e}");
        std::process::exit(1);
    }
}
