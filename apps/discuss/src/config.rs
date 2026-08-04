//! Env-driven config, đọc on-demand (không cache) theo khung apps/ba + apps/study.

use std::path::PathBuf;

/// Loopback by default. A Space App authenticates nothing of its own — the
/// daemon reaches it over 127.0.0.1 and the UI is same-origin — so binding
/// 0.0.0.0 hands the whole REST + MCP surface to anyone on the LAN. Set
/// SENCLAW_BIND_HOST=0.0.0.0 to opt in to that explicitly.
pub fn bind_host() -> String {
    std::env::var("SENCLAW_BIND_HOST").unwrap_or_else(|_| "127.0.0.1".to_string())
}

pub fn http_port() -> u16 {
    std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(4760)
}

pub fn app_id() -> String {
    std::env::var("SENCLAW_SPACE_APP_ID").unwrap_or_else(|_| "discuss".to_string())
}

pub fn senclaw_base_url() -> String {
    std::env::var("SENCLAW_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:18788".to_string())
}

/// Data dir nằm NGOÀI thư mục cài đặt: install/update zip sẽ `remove_dir_all`
/// thư mục app trước khi giải nén — DB đặt cạnh binary sẽ mất theo mỗi update.
pub fn data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("DISCUSS_DATA_DIR") {
        return PathBuf::from(d);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".senclaw")
        .join("space-app-data")
        .join(app_id())
}

pub fn db_path() -> PathBuf {
    data_dir().join("discuss.sqlite")
}

/// Kho tài liệu chung của một phiên — cũng chính là `workspace` truyền cho
/// agent.run để member đọc tài liệu bằng Read/Grep.
pub fn docs_dir(discussion_id: i64) -> PathBuf {
    data_dir().join("docs").join(discussion_id.to_string())
}
