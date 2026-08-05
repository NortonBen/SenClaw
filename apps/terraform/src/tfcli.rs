//! Phát hiện Terraform CLI trên máy, và cài hộ nếu chưa có.
//!
//! Thứ tự tìm: settings `terraform_bin` → bản app tự cài (`<data>/bin`) →
//! PATH → các chỗ quen thuộc (/opt/homebrew/bin, /usr/local/bin…).
//!
//! Cài đặt: hỏi version mới nhất qua checkpoint API HashiCorp, tải zip
//! `terraform_<ver>_<os>_<arch>.zip` từ releases.hashicorp.com, giải nén vào
//! `~/.senclaw/apps/terraform/bin/`. Hỗ trợ macOS / Linux / Windows,
//! arm64 + amd64. Toàn bộ tiến trình log vào console (run kind `install`).

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Version dự phòng khi checkpoint API không truy cập được (mạng chặn).
/// Chỉ là fallback — bình thường luôn lấy version mới nhất từ checkpoint.
pub const FALLBACK_VERSION: &str = "1.10.5";

pub fn exe_name() -> &'static str {
    if cfg!(windows) { "terraform.exe" } else { "terraform" }
}

/// Thư mục app tự quản binary terraform đã cài.
pub fn managed_bin_dir() -> PathBuf {
    crate::db::data_dir().join("bin")
}

pub fn managed_bin() -> PathBuf {
    managed_bin_dir().join(exe_name())
}

/// (os, arch) theo cách đặt tên của releases.hashicorp.com.
pub fn platform() -> Result<(&'static str, &'static str)> {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "linux" => "linux",
        "windows" => "windows",
        other => bail!("hệ điều hành chưa hỗ trợ cài tự động: {other}"),
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "amd64",
        "x86" => "386",
        other => bail!("kiến trúc chưa hỗ trợ cài tự động: {other}"),
    };
    Ok((os, arch))
}

pub fn download_url(version: &str, os: &str, arch: &str) -> String {
    format!(
        "https://releases.hashicorp.com/terraform/{version}/terraform_{version}_{os}_{arch}.zip"
    )
}

/// Parse `current_version` từ JSON checkpoint API.
pub fn parse_checkpoint(v: &Value) -> Option<String> {
    v.get("current_version")
        .and_then(|s| s.as_str())
        .map(|s| s.to_string())
}

/// Parse version từ output `terraform version -json` hoặc bản text thường.
pub fn parse_version_output(stdout: &str) -> Option<String> {
    if let Ok(v) = serde_json::from_str::<Value>(stdout) {
        if let Some(s) = v.get("terraform_version").and_then(|s| s.as_str()) {
            return Some(s.to_string());
        }
    }
    // "Terraform v1.9.8" → "1.9.8"
    stdout
        .lines()
        .next()?
        .split_whitespace()
        .find(|w| w.starts_with('v') && w[1..].chars().next().is_some_and(|c| c.is_ascii_digit()))
        .map(|w| w.trim_start_matches('v').to_string())
}

fn is_executable(p: &Path) -> bool {
    if !p.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p)
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Quét PATH + chỗ quen thuộc tìm binary terraform.
fn find_in_path() -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    for extra in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"] {
        dirs.push(PathBuf::from(extra));
    }
    dirs.into_iter()
        .map(|d| d.join(exe_name()))
        .find(|p| is_executable(p))
}

/// Kết quả phát hiện CLI cho UI/MCP.
pub async fn discover(override_bin: Option<String>) -> Value {
    let candidates: Vec<(String, PathBuf)> = [
        override_bin.map(|p| ("settings".to_string(), PathBuf::from(p))),
        Some(("managed".to_string(), managed_bin())),
        find_in_path().map(|p| ("system".to_string(), p)),
    ]
    .into_iter()
    .flatten()
    .collect();

    for (source, path) in candidates {
        if !is_executable(&path) {
            continue;
        }
        let version = version_of(&path).await;
        return json!({
            "found": true,
            "path": path.to_string_lossy(),
            "source": source,
            "version": version,
            "managed_dir": managed_bin_dir().to_string_lossy(),
        });
    }
    let plat = platform().map(|(os, arch)| format!("{os}_{arch}")).ok();
    json!({
        "found": false,
        "platform": plat,
        "managed_dir": managed_bin_dir().to_string_lossy(),
        "install_hint": "POST /api/cli/install (hoặc tool tf_cli_install) để app tải Terraform về",
    })
}

pub async fn version_of(path: &Path) -> Option<String> {
    let out = tokio::process::Command::new(path)
        .args(["version", "-json"])
        .output()
        .await
        .ok()?;
    parse_version_output(&String::from_utf8_lossy(&out.stdout))
}

/// Binary terraform sẽ dùng để chạy lệnh — Err kèm hướng dẫn khi chưa có.
pub async fn resolve_bin(override_bin: Option<String>) -> Result<PathBuf> {
    let d = discover(override_bin).await;
    if d["found"].as_bool() == Some(true) {
        Ok(PathBuf::from(d["path"].as_str().unwrap_or_default()))
    } else {
        Err(anyhow!(
            "chưa có Terraform CLI trên máy — bấm \"Cài Terraform\" trong app (POST /api/cli/install) hoặc tự cài rồi thử lại"
        ))
    }
}

/// Lấy version mới nhất từ checkpoint API (fallback khi lỗi mạng).
pub async fn latest_version(client: &reqwest::Client) -> (String, Option<String>) {
    let r = client
        .get("https://checkpoint-api.hashicorp.com/v1/check/terraform")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await;
    match r {
        Ok(resp) => match resp.json::<Value>().await {
            Ok(v) => match parse_checkpoint(&v) {
                Some(ver) => (ver, None),
                None => (
                    FALLBACK_VERSION.into(),
                    Some("checkpoint API trả JSON lạ — dùng version dự phòng".into()),
                ),
            },
            Err(e) => (FALLBACK_VERSION.into(), Some(format!("checkpoint API lỗi: {e}"))),
        },
        Err(e) => (FALLBACK_VERSION.into(), Some(format!("không gọi được checkpoint API: {e}"))),
    }
}

/// Tải + giải nén + cài terraform. `log` nhận từng dòng tiến trình cho console.
pub async fn install(version: Option<String>, log: impl Fn(&str)) -> Result<PathBuf> {
    let (os, arch) = platform()?;
    let client = reqwest::Client::new();
    let version = match version.filter(|v| !v.is_empty()) {
        Some(v) => {
            log(&format!("Dùng version chỉ định: {v}"));
            v
        }
        None => {
            log("Hỏi version mới nhất từ checkpoint-api.hashicorp.com…");
            let (v, warn) = latest_version(&client).await;
            if let Some(w) = warn {
                log(&format!("⚠ {w}"));
            }
            log(&format!("Version: {v}"));
            v
        }
    };
    if !version.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') {
        bail!("version không hợp lệ: {version:?}");
    }

    let url = download_url(&version, os, arch);
    log(&format!("Tải {url}"));
    let resp = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(600))
        .send()
        .await
        .map_err(|e| anyhow!("tải thất bại: {e}"))?;
    if !resp.status().is_success() {
        bail!("tải thất bại: HTTP {} — kiểm tra version/platform ({url})", resp.status());
    }
    let bytes = resp.bytes().await.map_err(|e| anyhow!("tải thất bại: {e}"))?;
    log(&format!("Đã tải {:.1} MB — giải nén…", bytes.len() as f64 / 1_048_576.0));

    let dir = managed_bin_dir();
    std::fs::create_dir_all(&dir)?;
    let target = dir.join(exe_name());
    // zip của HashiCorp chứa đúng một entry `terraform(.exe)`.
    let reader = std::io::Cursor::new(bytes.to_vec());
    let mut archive = zip::ZipArchive::new(reader).map_err(|e| anyhow!("zip hỏng: {e}"))?;
    let mut found = false;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().to_string();
        if name == "terraform" || name == "terraform.exe" {
            let mut out = std::fs::File::create(&target)?;
            std::io::copy(&mut entry, &mut out)?;
            found = true;
            break;
        }
    }
    if !found {
        bail!("zip không chứa binary terraform");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))?;
    }
    log(&format!("Đã cài vào {}", target.display()));

    match version_of(&target).await {
        Some(v) => log(&format!("Kiểm tra: terraform v{v} chạy OK")),
        None => bail!("binary cài xong nhưng không chạy được — platform lệch?"),
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_url_shape() {
        assert_eq!(
            download_url("1.10.5", "darwin", "arm64"),
            "https://releases.hashicorp.com/terraform/1.10.5/terraform_1.10.5_darwin_arm64.zip"
        );
        assert_eq!(
            download_url("1.10.5", "windows", "amd64"),
            "https://releases.hashicorp.com/terraform/1.10.5/terraform_1.10.5_windows_amd64.zip"
        );
    }

    #[test]
    fn platform_maps_current_host() {
        // Máy dev/CI của repo là macOS/Linux — chỉ cần không panic và trả tên hợp lệ.
        let (os, arch) = platform().unwrap();
        assert!(["darwin", "linux", "windows"].contains(&os));
        assert!(["arm64", "amd64", "386"].contains(&arch));
    }

    #[test]
    fn checkpoint_parse() {
        let v = serde_json::json!({ "product": "terraform", "current_version": "1.12.0" });
        assert_eq!(parse_checkpoint(&v), Some("1.12.0".into()));
        assert_eq!(parse_checkpoint(&serde_json::json!({})), None);
    }

    #[test]
    fn version_output_parse_json_and_text() {
        assert_eq!(
            parse_version_output(r#"{"terraform_version":"1.9.8","platform":"darwin_arm64"}"#),
            Some("1.9.8".into())
        );
        assert_eq!(
            parse_version_output("Terraform v1.5.7\non darwin_arm64"),
            Some("1.5.7".into())
        );
        assert_eq!(parse_version_output("gibberish"), None);
    }
}
