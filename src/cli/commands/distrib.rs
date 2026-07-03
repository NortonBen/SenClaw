//! `senclaw install desktop`, `senclaw uninstall desktop`, `senclaw web`.
//!
//! The `senclaw` binary ships without the desktop app or the Web UI bundle.
//! These commands download the prebuilt artifacts from GitHub Releases on
//! demand:
//!
//! - `install desktop`   → platform desktop bundle into the OS app location
//! - `uninstall desktop` → remove whatever `install desktop` put there
//! - `web`               → fetch `senclaw-web-dist.tar.gz` into
//!   `~/.senclaw/web/dist` (first run only), then start the daemon serving it
//!
//! Release asset names must match `.github/workflows/desktop.yml`.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::Subcommand;
use futures::StreamExt;

const REPO: &str = "NortonBen/SenClaw";
const WEB_DIST_ASSET: &str = "senclaw-web-dist.tar.gz";

#[derive(Subcommand, Debug)]
pub enum InstallCmd {
    /// Download the prebuilt SenClaw Desktop app for this platform and install it
    Desktop {
        /// Release tag to install (e.g. v0.3.0). Default: latest release.
        #[arg(long)]
        version: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum UninstallCmd {
    /// Remove the SenClaw Desktop app installed by `senclaw install desktop`
    Desktop,
}

pub async fn run_install(cmd: InstallCmd) -> Result<()> {
    match cmd {
        InstallCmd::Desktop { version } => install_desktop(version).await,
    }
}

pub async fn run_uninstall(cmd: UninstallCmd) -> Result<()> {
    match cmd {
        UninstallCmd::Desktop => uninstall_desktop(),
    }
}

/// `senclaw web` — make sure the Web UI bundle exists locally, then start the
/// daemon with `SENCLAW_WEB_DIST` pointing at it.
pub async fn run_web(force: bool, version: Option<String>) -> Result<()> {
    let dist = ensure_web_dist(force, version).await?;
    println!("Serving Web UI from {}", dist.display());
    std::env::set_var("SENCLAW_WEB_DIST", &dist);

    let mut cfg = crate::config::Config::from_env();
    let gcp = cfg.paths.global_config_path.clone();
    cfg.apply_persisted_overrides(&gcp);
    let port = cfg.ui_server.port;
    println!("Web UI: http://127.0.0.1:{port}");
    crate::run_daemon(cfg).await
}

// ===== Desktop install / uninstall =====

async fn install_desktop(version: Option<String>) -> Result<()> {
    let target = desktop_target()?;
    let tmp = tmp_dir()?;

    #[cfg(target_os = "macos")]
    {
        let asset = format!("SenClaw-{target}.app.zip");
        let zip_path = tmp.join(&asset);
        download(&asset_url(&asset, version.as_deref()), &zip_path).await?;

        // `ditto` preserves symlinks, permissions, and code signatures — the
        // zip crate does not, which breaks .app bundles.
        let extract_dir = tmp.join("desktop-extract");
        let _ = std::fs::remove_dir_all(&extract_dir);
        std::fs::create_dir_all(&extract_dir)?;
        run_tool(
            "ditto",
            &["-xk", &zip_path.to_string_lossy(), &extract_dir.to_string_lossy()],
        )?;
        let app_src = extract_dir.join("SenClaw Desktop.app");
        if !app_src.exists() {
            bail!("archive did not contain 'SenClaw Desktop.app'");
        }

        let app_dst = macos_app_dir().join("SenClaw Desktop.app");
        let _ = std::fs::remove_dir_all(&app_dst);
        std::fs::create_dir_all(app_dst.parent().unwrap())?;
        run_tool(
            "mv",
            &[&app_src.to_string_lossy(), &app_dst.to_string_lossy()],
        )?;
        let _ = std::fs::remove_file(&zip_path);
        let _ = std::fs::remove_dir_all(&extract_dir);
        println!("Installed {}", app_dst.display());
        println!("Launch it from Finder, Spotlight, or: open \"{}\"", app_dst.display());
    }

    #[cfg(target_os = "windows")]
    {
        let asset = format!("SenClaw-{target}.zip");
        let zip_path = tmp.join(&asset);
        download(&asset_url(&asset, version.as_deref()), &zip_path).await?;

        let dest = windows_desktop_dir();
        let _ = std::fs::remove_dir_all(&dest);
        std::fs::create_dir_all(&dest)?;
        let file = std::fs::File::open(&zip_path)
            .with_context(|| format!("open {}", zip_path.display()))?;
        zip::ZipArchive::new(file)?.extract(&dest)?;
        let _ = std::fs::remove_file(&zip_path);
        println!("Installed {}", dest.display());
        println!("Launch: {}", dest.join("senclaw_desktop.exe").display());
    }

    #[cfg(target_os = "linux")]
    {
        let asset = format!("SenClaw-{target}.tar.gz");
        let tar_path = tmp.join(&asset);
        download(&asset_url(&asset, version.as_deref()), &tar_path).await?;

        let dest = linux_desktop_dir();
        let _ = std::fs::remove_dir_all(&dest);
        std::fs::create_dir_all(&dest)?;
        run_tool(
            "tar",
            &["-xzf", &tar_path.to_string_lossy(), "-C", &dest.to_string_lossy()],
        )?;
        let _ = std::fs::remove_file(&tar_path);
        write_linux_desktop_entry(&dest)?;
        println!("Installed {}", dest.display());
        println!("Launch: {}", dest.join("senclaw_desktop").display());
    }

    Ok(())
}

fn uninstall_desktop() -> Result<()> {
    let mut removed = false;

    #[cfg(target_os = "macos")]
    for dir in [
        PathBuf::from("/Applications/SenClaw Desktop.app"),
        home().join("Applications/SenClaw Desktop.app"),
    ] {
        if dir.exists() {
            std::fs::remove_dir_all(&dir)
                .with_context(|| format!("remove {}", dir.display()))?;
            println!("Removed {}", dir.display());
            removed = true;
        }
    }

    #[cfg(target_os = "windows")]
    {
        let dir = windows_desktop_dir();
        if dir.exists() {
            std::fs::remove_dir_all(&dir)
                .with_context(|| format!("remove {}", dir.display()))?;
            println!("Removed {}", dir.display());
            removed = true;
        }
    }

    #[cfg(target_os = "linux")]
    {
        let dir = linux_desktop_dir();
        if dir.exists() {
            std::fs::remove_dir_all(&dir)
                .with_context(|| format!("remove {}", dir.display()))?;
            println!("Removed {}", dir.display());
            removed = true;
        }
        let entry = linux_desktop_entry_path();
        if entry.exists() {
            std::fs::remove_file(&entry)?;
            println!("Removed {}", entry.display());
            removed = true;
        }
    }

    if !removed {
        println!("SenClaw Desktop is not installed (nothing to remove).");
    }
    Ok(())
}

// ===== Web UI bundle =====

/// Return the local Web UI dist directory, downloading and extracting the
/// release bundle on first use (or when `force` is set).
async fn ensure_web_dist(force: bool, version: Option<String>) -> Result<PathBuf> {
    let dist = home().join(".senclaw").join("web").join("dist");
    if !force && dist.join("index.html").exists() {
        return Ok(dist);
    }

    let tmp = tmp_dir()?;
    let tar_path = tmp.join(WEB_DIST_ASSET);
    download(&asset_url(WEB_DIST_ASSET, version.as_deref()), &tar_path).await?;

    let _ = std::fs::remove_dir_all(&dist);
    std::fs::create_dir_all(&dist)?;
    // `tar` ships with macOS, Linux, and Windows 10 1803+.
    run_tool(
        "tar",
        &["-xzf", &tar_path.to_string_lossy(), "-C", &dist.to_string_lossy()],
    )?;
    let _ = std::fs::remove_file(&tar_path);

    if !dist.join("index.html").exists() {
        bail!(
            "extracted bundle has no index.html at {} — the release asset may be malformed",
            dist.display()
        );
    }
    println!("Web UI bundle installed at {}", dist.display());
    Ok(dist)
}

// ===== Shared helpers =====

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

fn tmp_dir() -> Result<PathBuf> {
    let dir = home().join(".senclaw").join("tmp");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Rust target triple matching the release asset names in desktop.yml.
fn desktop_target() -> Result<&'static str> {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Ok("aarch64-apple-darwin")
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Ok("x86_64-apple-darwin")
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Ok("x86_64-pc-windows-msvc")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Ok("x86_64-unknown-linux-gnu")
    } else {
        bail!(
            "no prebuilt desktop bundle for this platform — build from source: \
             `make app-build` (see README, Desktop App section)"
        )
    }
}

fn asset_url(asset: &str, version: Option<&str>) -> String {
    match version {
        Some(tag) => {
            let tag = if tag.starts_with('v') {
                tag.to_string()
            } else {
                format!("v{tag}")
            };
            format!("https://github.com/{REPO}/releases/download/{tag}/{asset}")
        }
        None => format!("https://github.com/{REPO}/releases/latest/download/{asset}"),
    }
}

async fn download(url: &str, dest: &Path) -> Result<()> {
    println!("Downloading {url}");
    let client = reqwest::Client::builder()
        .user_agent(format!("senclaw/{}", env!("CARGO_PKG_VERSION")))
        .build()?;
    let resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        bail!(
            "download failed with HTTP {} — {url}\n\
             (no matching release asset? check https://github.com/{REPO}/releases)",
            resp.status()
        );
    }
    let total = resp.content_length();

    let mut file = tokio::fs::File::create(dest)
        .await
        .with_context(|| format!("create {}", dest.display()))?;
    let mut stream = resp.bytes_stream();
    let mut written: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await?;
        written += chunk.len() as u64;
    }
    tokio::io::AsyncWriteExt::flush(&mut file).await?;

    match total {
        Some(t) => println!("Downloaded {} MB", t / 1_048_576),
        None => println!("Downloaded {} MB", written / 1_048_576),
    }
    Ok(())
}

fn run_tool(program: &str, args: &[&str]) -> Result<()> {
    let status = std::process::Command::new(program)
        .args(args)
        .status()
        .with_context(|| format!("failed to run `{program}` — is it installed?"))?;
    if !status.success() {
        bail!("`{program} {}` exited with {status}", args.join(" "));
    }
    Ok(())
}

// ===== Platform-specific paths =====

#[cfg(target_os = "macos")]
fn macos_app_dir() -> PathBuf {
    // Prefer /Applications; fall back to ~/Applications when not writable.
    let system = PathBuf::from("/Applications");
    let probe = system.join(".senclaw-write-probe");
    match std::fs::File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            system
        }
        Err(_) => home().join("Applications"),
    }
}

#[cfg(target_os = "windows")]
fn windows_desktop_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| home().join("AppData/Local"))
        .join("SenClaw")
        .join("Desktop")
}

#[cfg(target_os = "linux")]
fn linux_desktop_dir() -> PathBuf {
    home().join(".senclaw").join("desktop")
}

#[cfg(target_os = "linux")]
fn linux_desktop_entry_path() -> PathBuf {
    home().join(".local/share/applications/senclaw-desktop.desktop")
}

#[cfg(target_os = "linux")]
fn write_linux_desktop_entry(bundle_dir: &Path) -> Result<()> {
    let entry = linux_desktop_entry_path();
    std::fs::create_dir_all(entry.parent().unwrap())?;
    std::fs::write(
        &entry,
        format!(
            "[Desktop Entry]\nType=Application\nName=SenClaw Desktop\nExec={}\nTerminal=false\nCategories=Utility;\n",
            bundle_dir.join("senclaw_desktop").display()
        ),
    )?;
    println!("Desktop entry: {}", entry.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_url_latest() {
        assert_eq!(
            asset_url("senclaw-web-dist.tar.gz", None),
            "https://github.com/NortonBen/SenClaw/releases/latest/download/senclaw-web-dist.tar.gz"
        );
    }

    #[test]
    fn asset_url_versioned_adds_v_prefix() {
        assert_eq!(
            asset_url("x", Some("0.3.0")),
            "https://github.com/NortonBen/SenClaw/releases/download/v0.3.0/x"
        );
        assert_eq!(
            asset_url("x", Some("v0.3.0")),
            "https://github.com/NortonBen/SenClaw/releases/download/v0.3.0/x"
        );
    }
}
