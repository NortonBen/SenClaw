//! widget-pack — Space App chứa các widget custom cho ô chat (iframe).
//!
//! Không MCP, không DB, không bridge: chỉ một static server phục vụ
//! `web/widget/*.html` — mỗi file là một widget tự chứa (HTML + JS inline),
//! nhận tham số qua query string do `emit_widget` kind `app` ghép vào entry.
//! Danh mục widget khai trong `senclaw-manifest.json → widgets[]` (surfaces
//! `chat`), agent khám phá qua tool `widget_list`.

use axum::{routing::get, Json, Router};
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

#[tokio::main]
async fn main() {
    let port = std::env::var("PORT").unwrap_or_else(|_| "4750".to_string());

    // App này không có bước build web — `web/` là file tĩnh dùng thẳng.
    // Bản đóng gói copy `web/` thành `web_dist/` cạnh binary.
    let exe_dir = std::env::current_exe()
        .map(|p| p.parent().unwrap().to_path_buf())
        .unwrap_or_default();
    let candidates = [
        std::path::PathBuf::from("apps/widget-pack/web"),
        std::path::PathBuf::from("web_dist"),
        exe_dir.join("web_dist"),
        std::path::PathBuf::from("web"),
    ];
    let dist_path = candidates
        .iter()
        .find(|c| c.join("index.html").exists())
        .cloned()
        .unwrap_or_else(|| std::path::PathBuf::from("web"));

    let serve_dir =
        ServeDir::new(&dist_path).not_found_service(ServeFile::new(dist_path.join("index.html")));

    let app = Router::new()
        .route(
            "/health",
            get(|| async { Json(serde_json::json!({ "ok": true, "app": "widget-pack" })) }),
        )
        .fallback_service(serve_dir)
        .layer(CorsLayer::permissive());

    // Loopback by default. A Space App authenticates nothing of its own — the
    // daemon reaches it over 127.0.0.1 and the UI is same-origin — so binding
    // 0.0.0.0 hands the whole surface to anyone on the LAN. Set
    // SENCLAW_BIND_HOST=0.0.0.0 to opt in to that explicitly.
    let host = std::env::var("SENCLAW_BIND_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let listener = tokio::net::TcpListener::bind(format!("{host}:{port}"))
        .await
        .unwrap();
    println!("SenClaw Widget Pack running on http://{host}:{port}");
    axum::serve(listener, app).await.unwrap();
}
