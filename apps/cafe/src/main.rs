// json! của tools_list (27 tool, schema lồng nhau) vượt recursion limit 128.
#![recursion_limit = "256"]

//! SenClaw Cafe Space App — quản lý quán cafe / đồ uống: kho nguyên liệu
//! (g/ml/cái, nhập kg/lít tự quy đổi, giá vốn bình quân gia quyền), thực đơn +
//! công thức pha chế định lượng, bán hàng trừ kho theo công thức, báo cáo nhập
//! hàng / doanh thu – lãi gộp, dự đoán lượng bán + nguyên liệu và AI phân tích
//! qua bridge. Dữ liệu 100% local (SQLite) — app chỉ GHI SỔ, không kết nối máy
//! POS hay bán hàng online.

mod api;
mod calc;
mod db;
mod llm;
mod mcp;

use axum::response::IntoResponse;
use axum::Router;
use tower_http::services::ServeDir;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let port = std::env::var("PORT").unwrap_or_else(|_| "4700".to_string());
    // Mặc định chỉ nghe loopback. Bind 0.0.0.0 phơi API ra cả LAN mà app không
    // có lớp xác thực nào — muốn truy cập từ máy khác thì phải khai tường minh.
    let host = std::env::var("SENCLAW_BIND_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let state = api::make_state();
    let api_router = api::api_router(state);

    let exe_dir = std::env::current_exe()
        .map(|p| p.parent().unwrap().to_path_buf())
        .unwrap_or_default();
    // App-specific and packaged paths first; generic `web/dist` last so running
    // from the repo root doesn't pick up SenClaw's own web build.
    let candidates = [
        std::path::PathBuf::from("apps/cafe/web/dist"),
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

    // SPA fallback qua `.fallback(...)`, KHÔNG dùng not_found_service — cái sau
    // ép status 404 lên mọi route phía client nên health-check/proxy tưởng app
    // chết dù UI vẫn render.
    let index_path = dist_path.join("index.html");
    let spa_index = axum::routing::get(move || {
        let index_path = index_path.clone();
        async move {
            match tokio::fs::read_to_string(&index_path).await {
                Ok(html) => axum::response::Html(html).into_response(),
                Err(_) => (
                    axum::http::StatusCode::NOT_FOUND,
                    "Quán Cafe UI chưa được build (thiếu web_dist/index.html)",
                )
                    .into_response(),
            }
        }
    });
    let serve_dir = ServeDir::new(&dist_path).fallback(spa_index);

    // KHÔNG có CorsLayer::permissive(): UI được serve cùng origin (iframe app),
    // còn `Access-Control-Allow-Origin: *` trên một dịch vụ loopback không xác
    // thực nghĩa là bất kỳ trang web nào người dùng mở cũng đọc được sổ sách
    // của quán qua trình duyệt.
    let app = Router::new().nest("/api", api_router).fallback_service(serve_dir);

    let listener = tokio::net::TcpListener::bind(format!("{host}:{port}"))
        .await
        .unwrap();
    println!("SenClaw Cafe running on http://{host}:{port}");
    axum::serve(listener, app).await.unwrap();
}
