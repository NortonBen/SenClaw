//! Mọi giá trị môi trường đọc ở ĐÂY và chỉ ở đây (pattern rewrite-story/config.rs).
//! Daemon inject: PORT, SENCLAW_BASE_URL, SENCLAW_SPACE_APP_ID, SENCLAW_SPACE_LOG_FILE.

use std::path::PathBuf;

pub const DEFAULT_PORT: &str = "4560";
pub const APP_ID: &str = "lakehouse";

pub fn http_port() -> String {
    std::env::var("PORT").unwrap_or_else(|_| DEFAULT_PORT.to_string())
}

/// Gốc data của app — CỐ Ý nằm NGOÀI thư mục cài đặt: install zip
/// `remove_dir_all(<app_dir>)` trước khi extract, giữ gì cạnh binary là mất sạch
/// mỗi lần update (space.rs:957). Override: LAKEHOUSE_DATA_DIR.
pub fn data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("LAKEHOUSE_DATA_DIR") {
        return PathBuf::from(d);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let app_id =
        std::env::var("SENCLAW_SPACE_APP_ID").unwrap_or_else(|_| APP_ID.to_string());
    PathBuf::from(home)
        .join(".senclaw")
        .join("space-app-data")
        .join(app_id)
}

pub fn db_path() -> PathBuf {
    data_dir().join("catalog.sqlite")
}

/// Gốc lake — mọi file Parquet nằm dưới đây; object_store root cũng chốt tại đây
/// (không đọc path ngoài lake/).
pub fn lake_dir() -> PathBuf {
    data_dir().join("lake")
}

/// Thư mục allowlist mặc định cho lake_import_file{path} (chặn local-file-disclosure qua MCP).
pub fn inbox_dir() -> PathBuf {
    data_dir().join("inbox")
}

pub fn exports_dir() -> PathBuf {
    data_dir().join("exports")
}

/// Daemon UI server (REST + bridge). KHÔNG phải WS port 18789 (browser gateway).
pub fn senclaw_base_url() -> String {
    std::env::var("SENCLAW_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:18788".to_string())
}

pub fn space_app_id() -> String {
    std::env::var("SENCLAW_SPACE_APP_ID").unwrap_or_else(|_| APP_ID.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dir_is_outside_install_dir() {
        // Không chứa thành phần đường dẫn cài đặt space-apps/ (nơi bị wipe khi update).
        let d = data_dir();
        let s = d.to_string_lossy();
        assert!(
            s.contains("space-app-data") || std::env::var("LAKEHOUSE_DATA_DIR").is_ok(),
            "data dir phải ở ~/.senclaw/space-app-data/<id>: {s}"
        );
    }
}
