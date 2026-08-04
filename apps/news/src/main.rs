//! SenClaw News Space App — thu thập tin tức từ nhiều nguồn RSS/Atom, gán chủ
//! đề theo keyword, phát hiện xu hướng, gom bài thành DÒNG SỰ KIỆN với timeline,
//! và AI phân tích / đánh giá / điểm tin qua bridge SenClaw. Dữ liệu nằm local
//! (SQLite); outbound duy nhất là chính các feed người dùng khai + bridge LLM.

mod api;
mod cluster;
mod db;
mod fetch;
mod llm;
mod mcp;

use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let port = std::env::var("PORT").unwrap_or_else(|_| "4660".to_string());
    // Mặc định chỉ nghe loopback. Bind 0.0.0.0 phơi API ra cả LAN mà app không
    // có lớp xác thực nào — muốn truy cập từ máy khác thì phải khai tường minh.
    let host = std::env::var("SENCLAW_BIND_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let state = api::make_state();

    state.db.apply_digest_markers();
    llm::set_output_language(&state.db.display_language());

    // Background collector: first sweep shortly after boot, then on the
    // user-configurable interval. `auto_fetch=0` pauses it without restart.
    {
        let state = state.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            loop {
                if state.db.setting("auto_fetch", "1") == "1" {
                    let r = api::fetch_all_value(&state).await;
                    let new = r["new"].as_i64().unwrap_or(0);
                    if new > 0 {
                        println!("[news] auto-fetch: {new} bài mới");
                    }
                }
                // Regroup after collecting, never during: the incremental
                // clusterer places each new article against the stories that
                // exist at that moment, so periodically re-deriving the whole
                // archive is what keeps early guesses from setting like
                // concrete. Also fires once when the rules themselves change.
                if state.db.regroup_due() {
                    match state.db.rebuild_stories() {
                        Ok(v) => println!(
                            "[news] gom lại dòng sự kiện: {} dòng / {} bài",
                            v["stories"], v["articles"]
                        ),
                        Err(e) => eprintln!("[news] gom lại thất bại: {e}"),
                    }
                }
                let mins: u64 = state
                    .db
                    .setting("fetch_interval_min", "30")
                    .parse()
                    .unwrap_or(30);
                tokio::time::sleep(std::time::Duration::from_secs(mins.clamp(5, 24 * 60) * 60))
                    .await;
            }
        });
    }

    let api_router = api::api_router(state);

    let exe_dir = std::env::current_exe()
        .map(|p| p.parent().unwrap().to_path_buf())
        .unwrap_or_default();
    // App-specific and packaged paths first; generic `web/dist` last so running
    // from the repo root doesn't pick up SenClaw's own web build.
    let candidates = [
        std::path::PathBuf::from("apps/news/web/dist"),
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
    println!("SenClaw News running on http://{host}:{port}");
    axum::serve(listener, app).await.unwrap();
}
