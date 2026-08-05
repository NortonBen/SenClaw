//! Thao tác git cho workspace nguồn `git`: clone về thư mục app quản lý,
//! pull đồng bộ trước khi plan/apply, đọc trạng thái repo cho UI.
//!
//! Mọi lệnh chạy bằng argv trực tiếp (không qua shell) + `GIT_TERMINAL_PROMPT=0`
//! để repo private thiếu credential FAIL ngay thay vì treo chờ nhập.

use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::path::Path;
use tokio::process::Command;

/// URL repo chấp nhận: https/http/ssh/git@… — chặn scheme lạ và ký tự bẩn.
pub fn validate_repo_url(url: &str) -> Result<()> {
    let url = url.trim();
    if url.is_empty() {
        bail!("repo_url trống");
    }
    if url.chars().any(|c| c.is_whitespace() || c == ';' || c == '|' || c == '&') {
        bail!("repo_url chứa ký tự không hợp lệ");
    }
    // `-` đầu chuỗi sẽ bị git hiểu là cờ lệnh.
    if url.starts_with('-') {
        bail!("repo_url không hợp lệ");
    }
    let ok = url.starts_with("https://")
        || url.starts_with("http://")
        || url.starts_with("ssh://")
        || url.starts_with("git://")
        || (url.contains('@') && url.contains(':') && !url.contains("://"));
    if !ok {
        bail!("repo_url phải là https://…, ssh://… hoặc git@host:path");
    }
    Ok(())
}

/// Tên thư mục clone gọn từ URL repo: `https://x/y/infra.git` → `infra`.
pub fn repo_dir_name(url: &str) -> String {
    let base = url
        .trim_end_matches('/')
        .rsplit(['/', ':'])
        .next()
        .unwrap_or("repo")
        .trim_end_matches(".git");
    let cleaned: String = base
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    let trimmed = cleaned.trim_matches('-');
    if trimmed.is_empty() { "repo".into() } else { trimmed.to_string() }
}

/// Args cho bước clone (runner chạy). `--single-branch` để pull sync nhẹ.
pub fn clone_args(url: &str, branch: &str, dest: &Path) -> Vec<String> {
    let mut args = vec!["clone".to_string(), "--single-branch".to_string()];
    if !branch.is_empty() {
        args.push("--branch".into());
        args.push(branch.to_string());
    }
    args.push(url.to_string());
    args.push(dest.to_string_lossy().to_string());
    args
}

/// Args cho bước sync (pull) chạy trong thư mục workspace.
pub fn pull_args(dir: &Path) -> Vec<String> {
    vec![
        "-C".into(),
        dir.to_string_lossy().to_string(),
        "pull".into(),
        "--ff-only".into(),
    ]
}

pub fn is_git_repo(dir: &Path) -> bool {
    dir.join(".git").exists()
}

async fn git_out(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Trạng thái repo cho UI: branch, commit gần nhất, số file thay đổi local.
pub async fn info(dir: &Path) -> Value {
    if !is_git_repo(dir) {
        return json!({ "is_git": false });
    }
    let branch = git_out(dir, &["rev-parse", "--abbrev-ref", "HEAD"]).await;
    let commit = git_out(dir, &["log", "-1", "--format=%h %s (%cr)"]).await;
    let dirty = git_out(dir, &["status", "--porcelain"])
        .await
        .map(|s| if s.is_empty() { 0 } else { s.lines().count() });
    let remote = git_out(dir, &["remote", "get-url", "origin"]).await;
    json!({
        "is_git": true,
        "branch": branch,
        "commit": commit,
        "dirty_files": dirty,
        "remote": remote,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn url_validation() {
        assert!(validate_repo_url("https://github.com/x/infra.git").is_ok());
        assert!(validate_repo_url("git@github.com:x/infra.git").is_ok());
        assert!(validate_repo_url("ssh://git@host/x.git").is_ok());
        assert!(validate_repo_url("").is_err());
        assert!(validate_repo_url("file:///etc/passwd").is_err());
        assert!(validate_repo_url("https://x/y; rm -rf /").is_err());
        assert!(validate_repo_url("--upload-pack=evil").is_err());
        assert!(validate_repo_url("/local/path").is_err());
    }

    #[test]
    fn repo_dir_name_from_url() {
        assert_eq!(repo_dir_name("https://github.com/x/infra.git"), "infra");
        assert_eq!(repo_dir_name("git@github.com:team/hạ-tầng.git"), "h--t-ng");
        assert_eq!(repo_dir_name("https://host/grp/sub/net-prod/"), "net-prod");
        assert_eq!(repo_dir_name("///"), "repo");
    }

    #[test]
    fn clone_and_pull_args_shape() {
        let dest = PathBuf::from("/tmp/ws");
        assert_eq!(
            clone_args("https://x/y.git", "main", &dest),
            vec!["clone", "--single-branch", "--branch", "main", "https://x/y.git", "/tmp/ws"]
        );
        assert_eq!(
            clone_args("https://x/y.git", "", &dest),
            vec!["clone", "--single-branch", "https://x/y.git", "/tmp/ws"]
        );
        assert_eq!(
            pull_args(&dest),
            vec!["-C", "/tmp/ws", "pull", "--ff-only"]
        );
    }
}
