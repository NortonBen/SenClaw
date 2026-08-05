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

use anyhow::{Context, Result, bail};
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

// ===== Update =====

/// `senclaw update` — update the binary, Web UI (if present), and desktop app
/// (if installed) to the latest (or specified) release.
pub async fn run_update(version: Option<String>) -> Result<()> {
    println!("Updating SenClaw…");

    // 1. Update the binary itself
    update_binary(version.as_deref()).await?;

    // 2. Update Web UI if it was previously downloaded
    let web_dist = home().join(".senclaw").join("web").join("dist");
    if web_dist.join("index.html").exists() {
        println!("\nUpdating Web UI…");
        ensure_web_dist(true, version.clone()).await?;
    }

    // 3. Update desktop app if installed
    if is_desktop_installed() {
        println!("\nUpdating Desktop app…");
        install_desktop(version).await?;
    }

    println!("\nAll components updated successfully.");
    Ok(())
}

/// Download the latest senclaw binary and replace the current one.
async fn update_binary(version: Option<&str>) -> Result<()> {
    let target = binary_target()?;
    let asset = format!("senclaw-{target}");
    let url = asset_url(&asset, version);
    let tmp = tmp_dir()?;
    let tmp_bin = tmp.join("senclaw-update");

    download(&url, &tmp_bin).await?;

    // Make it executable (no-op on Windows)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp_bin, std::fs::Permissions::from_mode(0o755))?;
    }

    // Replace the running binary
    let current_exe = std::env::current_exe().context("cannot determine current binary path")?;
    let current_exe = current_exe
        .canonicalize()
        .unwrap_or_else(|_| current_exe.clone());

    // On Unix we can atomically rename over the running binary.
    // On Windows the running exe is locked, so we rename-away first.
    #[cfg(windows)]
    {
        let bak = current_exe.with_extension("exe.bak");
        let _ = std::fs::remove_file(&bak);
        std::fs::rename(&current_exe, &bak)
            .context("cannot move current binary aside — try running from an elevated prompt")?;
    }

    std::fs::rename(&tmp_bin, &current_exe).with_context(|| {
        format!(
            "cannot replace {} — you may need to run with sudo or adjust permissions",
            current_exe.display()
        )
    })?;

    println!("Binary updated: {}", current_exe.display());

    // Print the new version
    let output = std::process::Command::new(&current_exe)
        .arg("--version")
        .output();
    if let Ok(out) = output {
        let ver = String::from_utf8_lossy(&out.stdout);
        print!("{ver}");
    }
    Ok(())
}

/// Rust target triple for the current platform (binary, not desktop bundle).
fn binary_target() -> Result<&'static str> {
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
            "no prebuilt binary for this platform — build from source: \
             `cargo build --release`"
        )
    }
}

/// Check whether the desktop app is currently installed on this platform.
fn is_desktop_installed() -> bool {
    #[cfg(target_os = "macos")]
    {
        PathBuf::from("/Applications/SenClaw Desktop.app").exists()
            || home().join("Applications/SenClaw Desktop.app").exists()
    }
    #[cfg(target_os = "windows")]
    {
        windows_desktop_dir().exists()
    }
    #[cfg(target_os = "linux")]
    {
        linux_desktop_dir().exists()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        false
    }
}

// ===== Desktop install / uninstall =====

async fn install_desktop(version: Option<String>) -> Result<()> {
    let triple = desktop_target()?;
    let asset = bundle_asset_name(triple);
    let staged = tmp_dir()?.join(&asset);
    download(&asset_url(&asset, version.as_deref()), &staged).await?;

    let target = default_install_target();
    // Reinstalling over a copy that is still running would fail the folder
    // rename on Windows — close it first (no-op elsewhere).
    kill_target_lockers(&target);
    let attempts = if cfg!(windows) { SWAP_ATTEMPTS } else { 1 };
    swap_bundle_with_retry(&staged, &target, attempts, SWAP_RETRY_DELAY)?;
    let _ = std::fs::remove_file(&staged);

    #[cfg(target_os = "linux")]
    write_linux_desktop_entry(&target)?;

    create_windows_shortcuts(&target);

    println!("Installed {}", target.display());
    println!("{}", launch_hint(&target));
    Ok(())
}

// ===== Bundle swap (shared by `install desktop` and `apply-update`) =====

#[cfg(target_os = "macos")]
const APP_BUNDLE_NAME: &str = "SenClaw Desktop.app";

/// Release asset holding the desktop bundle for `triple` on this platform.
/// Must match the `Bundle daemon + collect artifacts` step in desktop.yml.
fn bundle_asset_name(triple: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("SenClaw-{triple}.app.zip")
    } else if cfg!(target_os = "windows") {
        format!("SenClaw-{triple}.zip")
    } else {
        format!("SenClaw-{triple}.tar.gz")
    }
}

/// Where a fresh `install desktop` puts the bundle. NOTE: `apply-update` does
/// NOT use this — it replaces the bundle the app is actually running from,
/// which the app passes in via `--target`. Re-probing there would happily
/// install a second copy in /Applications while the user runs one from
/// ~/Downloads.
fn default_install_target() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        macos_app_dir().join(APP_BUNDLE_NAME)
    }
    #[cfg(target_os = "windows")]
    {
        windows_desktop_dir()
    }
    #[cfg(target_os = "linux")]
    {
        linux_desktop_dir()
    }
}

fn launch_hint(target: &Path) -> String {
    #[cfg(target_os = "macos")]
    {
        format!(
            "Launch it from Finder, Spotlight, or: open \"{}\"",
            target.display()
        )
    }
    #[cfg(target_os = "windows")]
    {
        format!("Launch: {}", target.join("senclaw_desktop.exe").display())
    }
    #[cfg(target_os = "linux")]
    {
        format!("Launch: {}", target.join("senclaw_desktop").display())
    }
}

/// Append `.<suffix>` to a path without touching its existing extension.
///
/// `Path::with_extension` is wrong here: on "SenClaw Desktop.app" it REPLACES
/// `.app`, yielding "SenClaw Desktop.new" — a sibling that macOS no longer
/// treats as a bundle.
fn with_suffix(p: &Path, suffix: &str) -> PathBuf {
    let mut s = p.as_os_str().to_os_string();
    s.push(".");
    s.push(suffix);
    PathBuf::from(s)
}

fn remove_path(p: &Path) -> std::io::Result<()> {
    if p.is_dir() {
        std::fs::remove_dir_all(p)
    } else if p.exists() {
        std::fs::remove_file(p)
    } else {
        Ok(())
    }
}

/// Replace the desktop bundle at `target` with the contents of the downloaded
/// archive `staged`.
///
/// Every mutating step is a rename WITHIN `target`'s own directory, so none of
/// them can straddle a filesystem boundary or half-finish:
///
/// 1. extract → `<target>.new`   — `<target>` still untouched; a bad archive
///    or a full disk fails here and the installed app is pristine.
/// 2. `<target>` → `<target>.old`
/// 3. `<target>.new` → `<target>`
/// 4. remove `<target>.old`
///
/// A failure at (3) puts `.old` back rather than leaving the user with no app.
pub(crate) fn swap_bundle(staged: &Path, target: &Path) -> Result<()> {
    let parent = target
        .parent()
        .with_context(|| format!("{} has no parent directory", target.display()))?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("cannot create {}", parent.display()))?;

    let new = with_suffix(target, "new");
    let old = with_suffix(target, "old");
    // Leftovers from a previous run that died mid-swap.
    let _ = remove_path(&new);
    let _ = remove_path(&old);

    if let Err(e) = extract_bundle(staged, &new, parent) {
        // Nothing has moved yet, so the install is fine — just don't leave a
        // half-written `.new` sitting next to it.
        let _ = remove_path(&new);
        return Err(e);
    }

    // Past this point the live bundle is in motion — keep the window tight.
    let had_old = target.exists();
    if had_old {
        if let Err(e) = std::fs::rename(target, &old) {
            // Don't strand a fully-extracted `.new` next to the install — on
            // Windows this rename fails whenever something inside the bundle
            // is still running, and the leftover reads as "the update went to
            // a different folder".
            let _ = remove_path(&new);
            return Err(anyhow::Error::new(e).context(format!(
                "cannot move {} aside — a file inside it is locked by a running \
                 process, or the directory is not writable",
                target.display()
            )));
        }
    }

    if let Err(e) = std::fs::rename(&new, target) {
        if had_old {
            let _ = std::fs::rename(&old, target);
        }
        let _ = remove_path(&new);
        return Err(anyhow::Error::new(e).context(format!(
            "cannot move the new bundle into {} (rolled back)",
            target.display()
        )));
    }

    if had_old {
        let _ = remove_path(&old);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn extract_bundle(staged: &Path, new: &Path, parent: &Path) -> Result<()> {
    // Stage inside the TARGET's directory, not ~/.senclaw/tmp: the final move
    // must be a same-volume rename, and /Applications is often on a different
    // filesystem than $HOME.
    let stage = parent.join(".senclaw-update-stage");
    let _ = std::fs::remove_dir_all(&stage);
    std::fs::create_dir_all(&stage).with_context(|| {
        format!(
            "cannot write to {} — the install location is not writable by this user",
            parent.display()
        )
    })?;
    let result = extract_app_into(staged, &stage, new);
    let _ = std::fs::remove_dir_all(&stage);
    result
}

#[cfg(target_os = "macos")]
fn extract_app_into(staged: &Path, stage: &Path, new: &Path) -> Result<()> {
    // `ditto` preserves symlinks, permissions, and code signatures — the zip
    // crate does not, which breaks .app bundles.
    run_tool(
        "ditto",
        &["-xk", &staged.to_string_lossy(), &stage.to_string_lossy()],
    )?;
    let app = stage.join(APP_BUNDLE_NAME);
    if !app.exists() {
        bail!("archive did not contain '{APP_BUNDLE_NAME}'");
    }
    std::fs::rename(&app, new).context("cannot stage the new bundle")?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn extract_bundle(staged: &Path, new: &Path, _parent: &Path) -> Result<()> {
    std::fs::create_dir_all(new)?;
    let file = std::fs::File::open(staged).with_context(|| format!("open {}", staged.display()))?;
    zip::ZipArchive::new(file)?.extract(new)?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn extract_bundle(staged: &Path, new: &Path, _parent: &Path) -> Result<()> {
    std::fs::create_dir_all(new)?;
    run_tool(
        "tar",
        &[
            "-xzf",
            &staged.to_string_lossy(),
            "-C",
            &new.to_string_lossy(),
        ],
    )?;
    Ok(())
}

// ===== apply-update (internal: desktop app self-replacement) =====

/// Finish a desktop self-update. Not for humans — see the `ApplyUpdate` docs in
/// main.rs and docs/desktop-app-auto-update.md.
///
/// An app cannot replace the bundle it is executing from, so the desktop app
/// copies this binary OUT of that bundle into ~/.senclaw/tmp, spawns the copy
/// detached with its own pid, and quits. Running from outside is what makes the
/// swap possible at all: on Windows the OS locks a running .exe, and on macOS
/// the app's own Resources would be yanked out from under it mid-flight.
///
/// Synchronous on purpose: this is a one-shot process whose entire job is to
/// block on another process dying.
pub fn run_apply_update(
    staged: PathBuf,
    target: PathBuf,
    pid: u32,
    sha256: Option<String>,
    relaunch: bool,
) -> Result<()> {
    println!("apply-update: waiting for pid {pid} to exit…");
    wait_for_pid_exit(pid, std::time::Duration::from_secs(60))?;

    // Verify BEFORE touching the install. The bundle is unsigned, so this
    // checksum is the only thing between a corrupted or tampered download and
    // the user's app directory.
    if let Some(expected) = sha256.as_deref() {
        verify_sha256(&staged, expected)?;
        println!("apply-update: checksum ok");
    }

    // The app and its spawned daemon are gone (pid wait above), but orphaned
    // MCP-server children and WebView2 helpers keep the bundle locked on
    // Windows — no amount of retrying beats a process that never exits.
    kill_target_lockers(&target);

    let attempts = if cfg!(windows) { SWAP_ATTEMPTS } else { 1 };
    if let Err(e) = swap_bundle_with_retry(&staged, &target, attempts, SWAP_RETRY_DELAY) {
        // The updater runs detached with no visible console, so a silent exit
        // here reads as "the update did nothing". Leave a note where support
        // can find it, and start the OLD app back up — the swap is atomic, so
        // the previous install is still intact.
        if let Ok(tmp) = tmp_dir() {
            let _ = std::fs::write(
                tmp.join("apply-update-error.log"),
                format!("apply-update failed for {}:\n{e:#}\n", target.display()),
            );
        }
        if relaunch {
            let _ = relaunch_app(&target);
        }
        return Err(e);
    }
    let _ = std::fs::remove_file(&staged);
    println!("apply-update: installed {}", target.display());

    #[cfg(target_os = "linux")]
    write_linux_desktop_entry(&target)?;

    create_windows_shortcuts(&target);

    if relaunch {
        relaunch_app(&target)?;
    }
    Ok(())
}

/// How long apply-update keeps retrying a failed swap. Windows-only in effect:
/// the pid wait covers the app itself, but its children (the spawned daemon,
/// WebView2 helper processes) can hold exe/dll locks for a few more seconds,
/// and a locked file makes the directory rename fail with ACCESS_DENIED.
const SWAP_ATTEMPTS: u32 = 5;
const SWAP_RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(2);

// ===== Windows: release bundle locks + shortcuts =====

/// PowerShell that force-stops everything holding `dir` open. Two sweeps:
///
/// 1. Any process whose IMAGE lives inside the bundle. Quitting the app kills
///    the daemon it spawned, but TerminateProcess does not cascade — the
///    daemon's MCP-server children (more `senclaw.exe` from the same file)
///    survive as orphans with the exe mapped, which blocks renaming the
///    folder forever. This was why updates stalled at `<target>.new`.
/// 2. WebView2 helpers. They run from Program Files (so sweep 1 misses them)
///    but keep their user-data folder — which lives INSIDE the bundle by
///    default — locked; match them by command line instead.
///
/// A pure function over the path so the quoting is testable on every OS.
#[cfg_attr(not(windows), allow(dead_code))]
fn locker_kill_script(dir: &str) -> String {
    let dir = dir.replace('\'', "''");
    format!(
        "$t='{dir}'; \
         Get-Process | Where-Object {{ $_.Path -like ($t + '\\*') }} | \
           Stop-Process -Force -ErrorAction SilentlyContinue; \
         Get-CimInstance Win32_Process -Filter \"Name='msedgewebview2.exe'\" | \
           Where-Object {{ $_.CommandLine -like ('*' + $t + '*') }} | \
           ForEach-Object {{ Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }}"
    )
}

/// PowerShell that (re)creates "SenClaw Desktop.lnk" on the Desktop and in the
/// Start Menu, pointing at `exe`. Overwrites in place, so an update refreshes
/// a shortcut that already exists.
#[cfg_attr(not(windows), allow(dead_code))]
fn shortcut_script(exe: &str, dir: &str) -> String {
    let exe = exe.replace('\'', "''");
    let dir = dir.replace('\'', "''");
    format!(
        "$ws = New-Object -ComObject WScript.Shell; \
         foreach ($p in @([Environment]::GetFolderPath('Desktop'), \
                          (Join-Path ([Environment]::GetFolderPath('StartMenu')) 'Programs'))) {{ \
           $s = $ws.CreateShortcut((Join-Path $p 'SenClaw Desktop.lnk')); \
           $s.TargetPath = '{exe}'; $s.WorkingDirectory = '{dir}'; \
           $s.IconLocation = '{exe},0'; $s.Save() \
         }}"
    )
}

#[cfg(windows)]
fn run_powershell(script: &str) -> bool {
    std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Best-effort: stop every process still holding the bundle at `target`.
#[cfg(windows)]
fn kill_target_lockers(target: &Path) {
    if !target.exists() {
        return;
    }
    println!(
        "Closing processes still running from {}…",
        target.display()
    );
    let _ = run_powershell(&locker_kill_script(&target.to_string_lossy()));
    // Handles release asynchronously after TerminateProcess.
    std::thread::sleep(std::time::Duration::from_millis(500));
}

#[cfg(not(windows))]
fn kill_target_lockers(_target: &Path) {}

/// Best-effort Desktop + Start Menu shortcuts (Windows only).
#[cfg(windows)]
fn create_windows_shortcuts(target: &Path) {
    let exe = target.join("senclaw_desktop.exe");
    if run_powershell(&shortcut_script(
        &exe.to_string_lossy(),
        &target.to_string_lossy(),
    )) {
        println!("Shortcuts created: Desktop + Start Menu → SenClaw Desktop");
    } else {
        println!("Note: could not create the Desktop/Start Menu shortcuts.");
    }
}

#[cfg(not(windows))]
fn create_windows_shortcuts(_target: &Path) {}

/// [`swap_bundle`], retried up to `attempts` times. Callers pass 1 on Unix —
/// renames there succeed with the files still open, so the first failure is
/// real and retrying would only delay the error message.
fn swap_bundle_with_retry(
    staged: &Path,
    target: &Path,
    attempts: u32,
    delay: std::time::Duration,
) -> Result<()> {
    let attempts = attempts.max(1);
    let mut last = None;
    for attempt in 1..=attempts {
        match swap_bundle(staged, target) {
            Ok(()) => return Ok(()),
            Err(e) => {
                if attempt < attempts {
                    println!(
                        "apply-update: swap attempt {attempt}/{attempts} failed ({e:#}); \
                         retrying in {}s…",
                        delay.as_secs()
                    );
                    std::thread::sleep(delay);
                }
                last = Some(e);
            }
        }
    }
    Err(last.expect("at least one attempt ran"))
}

fn verify_sha256(path: &Path, expected: &str) -> Result<()> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).context("read staged archive")?;
    let actual = hex::encode(hasher.finalize());
    if !actual.eq_ignore_ascii_case(expected) {
        bail!(
            "checksum mismatch for {} — refusing to install\n  expected {expected}\n  actual   {actual}",
            path.display()
        );
    }
    Ok(())
}

/// Block until `pid` is gone, erroring on timeout rather than swapping the
/// bundle out from under a still-running app.
fn wait_for_pid_exit(pid: u32, timeout: std::time::Duration) -> Result<()> {
    let start = std::time::Instant::now();
    while pid_alive(pid) {
        if start.elapsed() >= timeout {
            bail!(
                "pid {pid} is still running after {}s — aborting before anything is touched",
                timeout.as_secs()
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    Ok(())
}

#[cfg(unix)]
fn pid_alive(pid: u32) -> bool {
    // Signal 0 runs the existence/permission check without delivering anything.
    let rc = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if rc == 0 {
        return true;
    }
    // EPERM means the process exists but belongs to another user — still alive.
    // (Errno is read via std rather than libc::__errno_location, which is
    // glibc-only; macOS spells it __error.)
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(windows)]
fn pid_alive(pid: u32) -> bool {
    // Asking tasklist avoids a winapi dependency for one call. The spawn cost
    // is irrelevant in a one-shot updater polling a few times a second.
    match std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
    {
        // No match prints "INFO: No tasks are running which match…" (no digits).
        Ok(o) => String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()),
        // If we cannot tell, assume gone rather than hang until the timeout.
        Err(_) => false,
    }
}

fn relaunch_app(target: &Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    run_tool("open", &["-a", &target.to_string_lossy()])?;

    #[cfg(not(target_os = "macos"))]
    {
        let exe = if cfg!(target_os = "windows") {
            target.join("senclaw_desktop.exe")
        } else {
            target.join("senclaw_desktop")
        };
        std::process::Command::new(&exe)
            .spawn()
            .with_context(|| format!("relaunch {}", exe.display()))?;
    }

    println!("apply-update: relaunched");
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
            std::fs::remove_dir_all(&dir).with_context(|| format!("remove {}", dir.display()))?;
            println!("Removed {}", dir.display());
            removed = true;
        }
    }

    #[cfg(target_os = "windows")]
    {
        let dir = windows_desktop_dir();
        if dir.exists() {
            std::fs::remove_dir_all(&dir).with_context(|| format!("remove {}", dir.display()))?;
            println!("Removed {}", dir.display());
            removed = true;
        }
    }

    #[cfg(target_os = "linux")]
    {
        let dir = linux_desktop_dir();
        if dir.exists() {
            std::fs::remove_dir_all(&dir).with_context(|| format!("remove {}", dir.display()))?;
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
        &[
            "-xzf",
            &tar_path.to_string_lossy(),
            "-C",
            &dist.to_string_lossy(),
        ],
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

    // ===== swap_bundle =====

    /// The `with_extension` trap: on "SenClaw Desktop.app" it would REPLACE
    /// `.app` and produce "SenClaw Desktop.new", which macOS stops treating as
    /// a bundle. The staging path must be a plain suffix append.
    #[test]
    fn with_suffix_appends_rather_than_replacing_extension() {
        let p = Path::new("/Applications/SenClaw Desktop.app");
        assert_eq!(
            with_suffix(p, "new"),
            Path::new("/Applications/SenClaw Desktop.app.new")
        );
        assert_eq!(
            with_suffix(p, "old"),
            Path::new("/Applications/SenClaw Desktop.app.old")
        );
        // And the difference from the buggy version is real:
        assert_eq!(
            p.with_extension("new"),
            Path::new("/Applications/SenClaw Desktop.new")
        );
    }

    #[test]
    fn bundle_asset_name_matches_desktop_yml() {
        let name = bundle_asset_name("aarch64-apple-darwin");
        if cfg!(target_os = "macos") {
            assert_eq!(name, "SenClaw-aarch64-apple-darwin.app.zip");
        } else if cfg!(target_os = "windows") {
            assert_eq!(name, "SenClaw-aarch64-apple-darwin.zip");
        } else {
            assert_eq!(name, "SenClaw-aarch64-apple-darwin.tar.gz");
        }
    }

    /// Build a release-shaped archive holding a bundle whose marker file says
    /// `version`, matching what desktop.yml uploads for this platform.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn fake_archive(dir: &Path, version: &str) -> PathBuf {
        #[cfg(target_os = "macos")]
        {
            let app = dir.join(APP_BUNDLE_NAME);
            std::fs::create_dir_all(app.join("Contents/Resources")).unwrap();
            std::fs::write(app.join("Contents/Resources/senclaw"), version).unwrap();
            let zip = dir.join("bundle.app.zip");
            run_tool(
                "ditto",
                &[
                    "-c",
                    "-k",
                    "--keepParent",
                    &app.to_string_lossy(),
                    &zip.to_string_lossy(),
                ],
            )
            .unwrap();
            std::fs::remove_dir_all(&app).unwrap();
            zip
        }
        #[cfg(target_os = "linux")]
        {
            let stage = dir.join("stage");
            std::fs::create_dir_all(&stage).unwrap();
            std::fs::write(stage.join("senclaw"), version).unwrap();
            let tar = dir.join("bundle.tar.gz");
            run_tool(
                "tar",
                &[
                    "-czf",
                    &tar.to_string_lossy(),
                    "-C",
                    &stage.to_string_lossy(),
                    ".",
                ],
            )
            .unwrap();
            std::fs::remove_dir_all(&stage).unwrap();
            tar
        }
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn marker(target: &Path) -> String {
        let p = if cfg!(target_os = "macos") {
            target.join("Contents/Resources/senclaw")
        } else {
            target.join("senclaw")
        };
        std::fs::read_to_string(p).unwrap()
    }

    /// Create a fake existing install at `target` with a `senclaw` marker file
    /// holding `version`. Two definitions (see install_dir below for why), and
    /// NO `target.join(".")` tricks: `create_dir_all` on a `/.`-suffixed path
    /// errors on Linux — which is exactly what kept both swap tests red in CI
    /// while macOS, which takes the Contents/Resources branch, never noticed.
    #[cfg(target_os = "macos")]
    fn seed_install(target: &Path, version: &str) {
        let dir = target.join("Contents/Resources");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("senclaw"), version).unwrap();
    }

    #[cfg(target_os = "linux")]
    fn seed_install(target: &Path, version: &str) {
        std::fs::create_dir_all(target).unwrap();
        std::fs::write(target.join("senclaw"), version).unwrap();
    }

    // Two definitions rather than one `if cfg!(...)`: cfg! is a RUNTIME macro,
    // so both of its branches must compile — and APP_BUNDLE_NAME only exists on
    // macOS, which would break the Linux test build.
    #[cfg(target_os = "macos")]
    fn install_dir(root: &Path) -> PathBuf {
        root.join(APP_BUNDLE_NAME)
    }

    #[cfg(target_os = "linux")]
    fn install_dir(root: &Path) -> PathBuf {
        root.join("desktop")
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn swap_bundle_replaces_existing_install_and_leaves_no_debris() {
        let tmp = tempfile::tempdir().unwrap();
        let staged = fake_archive(tmp.path(), "v2");

        // Pre-existing install to be replaced.
        let target = install_dir(tmp.path());
        seed_install(&target, "v1");

        swap_bundle(&staged, &target).unwrap();

        assert_eq!(marker(&target), "v2", "bundle was not replaced");
        assert!(!with_suffix(&target, "new").exists(), ".new left behind");
        assert!(!with_suffix(&target, "old").exists(), ".old left behind");
        assert!(
            !tmp.path().join(".senclaw-update-stage").exists(),
            "staging dir left behind"
        );
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn swap_bundle_onto_empty_location_installs_fresh() {
        let tmp = tempfile::tempdir().unwrap();
        let staged = fake_archive(tmp.path(), "v2");
        let target = install_dir(tmp.path());

        swap_bundle(&staged, &target).unwrap();

        assert_eq!(marker(&target), "v2");
    }

    /// The property that matters most: a bad download must never cost the user
    /// their working install. Extraction happens before anything is moved.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn swap_bundle_leaves_install_untouched_when_archive_is_garbage() {
        let tmp = tempfile::tempdir().unwrap();
        let staged = tmp.path().join("garbage.zip");
        std::fs::write(&staged, b"not an archive at all").unwrap();

        let target = install_dir(tmp.path());
        seed_install(&target, "v1");

        assert!(
            swap_bundle(&staged, &target).is_err(),
            "garbage must not install"
        );
        assert_eq!(
            marker(&target),
            "v1",
            "install was damaged by a bad archive"
        );
        assert!(
            !with_suffix(&target, "old").exists(),
            "install left half-moved"
        );
        assert!(
            !with_suffix(&target, "new").exists(),
            ".new debris left behind"
        );
    }

    /// Debris from a previous run that died mid-swap must not block the retry.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn swap_bundle_clears_leftovers_from_a_crashed_run() {
        let tmp = tempfile::tempdir().unwrap();
        let staged = fake_archive(tmp.path(), "v2");
        let target = install_dir(tmp.path());

        std::fs::create_dir_all(with_suffix(&target, "new")).unwrap();
        std::fs::write(with_suffix(&target, "new").join("junk"), "stale").unwrap();
        std::fs::create_dir_all(with_suffix(&target, "old")).unwrap();

        swap_bundle(&staged, &target).unwrap();

        assert_eq!(marker(&target), "v2");
        assert!(!with_suffix(&target, "new").exists());
        assert!(!with_suffix(&target, "old").exists());
    }

    // ===== Windows helper scripts (pure string builders) =====

    /// A path with an apostrophe must not break out of the single-quoted
    /// PowerShell string — quotes are doubled, the PS escape for `'`.
    #[test]
    fn powershell_scripts_escape_single_quotes_in_paths() {
        let kill = locker_kill_script(r"C:\Users\O'Brien\AppData\Local\SenClaw");
        assert!(kill.contains(r"$t='C:\Users\O''Brien\AppData\Local\SenClaw'"));

        let sc = shortcut_script(
            r"C:\Apps\O'Neil\senclaw_desktop.exe",
            r"C:\Apps\O'Neil",
        );
        assert!(sc.contains(r"'C:\Apps\O''Neil\senclaw_desktop.exe'"));
    }

    #[test]
    fn locker_script_sweeps_both_image_paths_and_webview2_helpers() {
        let s = locker_kill_script(r"C:\x\SenClaw");
        assert!(s.contains("Get-Process"), "image-path sweep missing");
        assert!(s.contains("msedgewebview2.exe"), "WebView2 sweep missing");
        assert!(s.contains("Stop-Process"), "must actually stop processes");
    }

    #[test]
    fn shortcut_script_targets_desktop_and_start_menu() {
        let s = shortcut_script(r"C:\x\senclaw_desktop.exe", r"C:\x");
        assert!(s.contains("GetFolderPath('Desktop')"));
        assert!(s.contains("GetFolderPath('StartMenu')"));
        assert!(s.contains("SenClaw Desktop.lnk"));
        assert!(s.contains(r"$s.TargetPath = 'C:\x\senclaw_desktop.exe'"));
    }

    /// A parent directory the user cannot write (the closest unix stand-in
    /// for Windows' locked-bundle rename failure) must fail the swap cleanly:
    /// no `.new` debris, original install untouched.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn swap_bundle_fails_cleanly_in_an_unwritable_parent() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let staged = fake_archive(tmp.path(), "v2");
        let root = tmp.path().join("apps");
        std::fs::create_dir_all(&root).unwrap();
        // install_dir, NOT `if cfg!(macos) { APP_BUNDLE_NAME }`: cfg! is a
        // runtime macro, so that branch would still have to COMPILE on Linux,
        // where APP_BUNDLE_NAME does not exist (E0425 — this exact line kept
        // the CI check job red from 2026-07-18 to 08-05).
        let target = install_dir(&root);
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("marker"), "v1").unwrap();

        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o555)).unwrap();
        let result = swap_bundle(&staged, &target);
        // Restore before asserting so the tempdir can be cleaned up.
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();

        assert!(result.is_err(), "swap must fail in a read-only parent");
        assert!(
            !with_suffix(&target, "new").exists(),
            ".new debris left behind"
        );
        assert!(
            target.join("marker").exists(),
            "the original install must survive"
        );
    }

    // ===== swap_bundle_with_retry =====

    /// A persistent failure must exhaust its attempts and still surface the
    /// error — not spin forever, not panic on the bookkeeping.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn swap_retry_gives_up_after_its_attempts_and_reports_the_error() {
        let tmp = tempfile::tempdir().unwrap();
        let staged = tmp.path().join("garbage.zip");
        std::fs::write(&staged, b"not an archive").unwrap();
        let target = install_dir(tmp.path());

        let err = swap_bundle_with_retry(&staged, &target, 3, std::time::Duration::from_millis(1))
            .unwrap_err();
        assert!(!format!("{err:#}").is_empty());
        assert!(!target.exists(), "a failed retry run must not half-install");
    }

    /// The happy path must not pay any retry delay.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn swap_retry_succeeds_first_time_without_waiting() {
        let tmp = tempfile::tempdir().unwrap();
        let staged = fake_archive(tmp.path(), "v2");
        let target = install_dir(tmp.path());

        let start = std::time::Instant::now();
        swap_bundle_with_retry(&staged, &target, 5, std::time::Duration::from_secs(60)).unwrap();
        assert!(
            start.elapsed() < std::time::Duration::from_secs(30),
            "a successful first attempt must not sleep"
        );
        assert_eq!(marker(&target), "v2");
    }

    /// `attempts == 0` is a caller bug; it must clamp to one real attempt
    /// rather than silently doing nothing and reporting success.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn swap_retry_clamps_zero_attempts_to_one() {
        let tmp = tempfile::tempdir().unwrap();
        let staged = fake_archive(tmp.path(), "v2");
        let target = install_dir(tmp.path());

        swap_bundle_with_retry(&staged, &target, 0, std::time::Duration::from_millis(1)).unwrap();
        assert_eq!(marker(&target), "v2");
    }

    // ===== verify_sha256 =====

    #[test]
    fn verify_sha256_accepts_match_and_is_case_insensitive() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("a.bin");
        std::fs::write(&f, b"hello").unwrap();
        // sha256("hello")
        let want = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        verify_sha256(&f, want).unwrap();
        verify_sha256(&f, &want.to_uppercase()).unwrap();
    }

    #[test]
    fn verify_sha256_rejects_mismatch() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("a.bin");
        std::fs::write(&f, b"hello world").unwrap();
        let err = verify_sha256(
            &f,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("checksum mismatch"), "unexpected error: {err}");
    }

    // ===== pid_alive =====

    #[test]
    fn pid_alive_sees_this_process_and_not_a_bogus_one() {
        assert!(
            pid_alive(std::process::id()),
            "our own pid must read as alive"
        );
        // Above the platform pid_max everywhere we ship; nothing can hold it.
        assert!(!pid_alive(4_000_000_000), "bogus pid must read as dead");
    }

    #[test]
    fn wait_for_pid_exit_times_out_on_a_live_process() {
        let err = wait_for_pid_exit(std::process::id(), std::time::Duration::from_millis(300))
            .unwrap_err()
            .to_string();
        assert!(err.contains("still running"), "unexpected error: {err}");
    }

    #[test]
    fn wait_for_pid_exit_returns_once_the_child_is_gone() {
        let mut child = std::process::Command::new(if cfg!(windows) { "cmd" } else { "true" })
            .args(if cfg!(windows) {
                vec!["/C", "exit"]
            } else {
                vec![]
            })
            .spawn()
            .unwrap();
        let pid = child.id();
        child.wait().unwrap(); // reap, so the pid is truly released
        wait_for_pid_exit(pid, std::time::Duration::from_secs(5)).unwrap();
    }

    // ===== asset urls =====

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
