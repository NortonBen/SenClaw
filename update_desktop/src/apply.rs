//! The update transaction — a close port of `run_apply_update` and friends in
//! the main repo's src/cli/commands/distrib.rs (the `senclaw apply-update`
//! CLI, which remains the terminal path). Duplicated on purpose: this crate
//! must not depend on the daemon's dependency tree, and updater logic changes
//! rarely. If you fix a bug in either copy, port it to the other.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::Log;

#[cfg(target_os = "macos")]
const APP_BUNDLE_NAME: &str = "SenClaw Desktop.app";

/// How often a failed swap is retried. Windows-only in effect: the pid wait
/// covers the app itself, but freshly-terminated children release their file
/// locks asynchronously, and Defender may briefly hold newly-extracted files.
/// Unix renames succeed with files still open, so the first failure is real.
const SWAP_ATTEMPTS: u32 = if cfg!(windows) { 8 } else { 1 };
const SWAP_RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(2);

pub fn home() -> PathBuf {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var)
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

pub fn tmp_dir() -> Result<PathBuf> {
    let dir = home().join(".senclaw").join("tmp");
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    Ok(dir)
}

// ── Wait for the app to exit ─────────────────────────────────────────────────

/// Block until `pid` is gone, erroring on timeout rather than swapping the
/// bundle out from under a still-running app.
pub fn wait_for_pid_exit(pid: u32, timeout: std::time::Duration) -> Result<()> {
    let start = std::time::Instant::now();
    while pid_alive(pid) {
        if start.elapsed() >= timeout {
            bail!(
                "the app (pid {pid}) is still running after {}s — nothing was changed",
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
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(windows)]
fn pid_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return false; // no such process (or no access — assume gone, don't hang)
        }
        let mut code = 0u32;
        let ok = GetExitCodeProcess(h, &mut code);
        CloseHandle(h);
        ok != 0 && code == STILL_ACTIVE as u32
    }
}

// ── Checksum ─────────────────────────────────────────────────────────────────

pub fn verify_sha256(path: &Path, expected: &str) -> Result<()> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).context("read staged archive")?;
    let actual = hex(&hasher.finalize());
    if !actual.eq_ignore_ascii_case(expected) {
        bail!(
            "checksum mismatch for {} — refusing to install\n  expected {expected}\n  actual   {actual}",
            path.display()
        );
    }
    Ok(())
}

pub fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

// ── Atomic bundle swap ───────────────────────────────────────────────────────

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
fn swap_bundle(staged: &Path, target: &Path) -> Result<()> {
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
        let _ = remove_path(&new);
        return Err(e);
    }

    // Past this point the live bundle is in motion — keep the window tight.
    let had_old = target.exists();
    if had_old {
        if let Err(e) = std::fs::rename(target, &old) {
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

pub fn swap_bundle_with_retry(
    staged: &Path,
    target: &Path,
    log: &mut Log,
    status: &dyn Fn(&mut Log, &str),
) -> Result<()> {
    let mut last = None;
    for attempt in 1..=SWAP_ATTEMPTS {
        match swap_bundle(staged, target) {
            Ok(()) => return Ok(()),
            Err(e) => {
                if attempt < SWAP_ATTEMPTS {
                    log.line(&format!(
                        "swap attempt {attempt}/{SWAP_ATTEMPTS} failed: {e:#}"
                    ));
                    status(
                        log,
                        &format!(
                            "Installing the update… (retry {}/{})",
                            attempt + 1,
                            SWAP_ATTEMPTS
                        ),
                    );
                    std::thread::sleep(SWAP_RETRY_DELAY);
                }
                last = Some(e);
            }
        }
    }
    Err(last.expect("at least one attempt ran"))
}

// ── Per-OS extraction ────────────────────────────────────────────────────────

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
    // `ditto` preserves symlinks, permissions, and code signatures — a zip
    // library does not, which breaks .app bundles.
    let result = run_tool(
        "ditto",
        &["-xk", &staged.to_string_lossy(), &stage.to_string_lossy()],
    )
    .and_then(|()| {
        let app = stage.join(APP_BUNDLE_NAME);
        if !app.exists() {
            bail!("archive did not contain '{APP_BUNDLE_NAME}'");
        }
        std::fs::rename(&app, new).context("cannot stage the new bundle")?;
        Ok(())
    });
    let _ = std::fs::remove_dir_all(&stage);
    result
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
    )
}

#[cfg(not(target_os = "windows"))]
fn run_tool(program: &str, args: &[&str]) -> Result<()> {
    let status = std::process::Command::new(program)
        .args(args)
        .status()
        .with_context(|| format!("run {program}"))?;
    if !status.success() {
        bail!("{program} exited with {status}");
    }
    Ok(())
}

// ── Relaunch ─────────────────────────────────────────────────────────────────

pub fn relaunch_app(target: &Path) -> Result<()> {
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
            .current_dir(target)
            .spawn()
            .with_context(|| format!("relaunch {}", exe.display()))?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn write_linux_desktop_entry(bundle_dir: &Path) -> Result<()> {
    let exe = bundle_dir.join("senclaw_desktop");
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=SenClaw Desktop\nExec=\"{}\"\nTerminal=false\nCategories=Utility;\n",
        exe.display()
    );
    let dir = home().join(".local/share/applications");
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    std::fs::write(dir.join("senclaw-desktop.desktop"), entry).context("write .desktop entry")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_suffix_appends_after_a_dot() {
        assert_eq!(
            with_suffix(Path::new("/a/App Dir"), "new"),
            PathBuf::from("/a/App Dir.new")
        );
    }

    #[test]
    fn hex_encodes_lowercase() {
        assert_eq!(hex(&[0x00, 0xff, 0x1a]), "00ff1a");
    }

    #[test]
    fn verify_sha256_accepts_and_rejects() {
        let p = std::env::temp_dir().join(format!("upd-sha-{}", std::process::id()));
        std::fs::write(&p, b"senclaw").unwrap();
        // sha256("senclaw")
        let good = "1b41c61774d996c67fb29f240aa46e9cc7545cc8f6ff7f4862b1ac7629ca9c6e";
        let checked = verify_sha256(&p, good);
        let mismatch = verify_sha256(&p, &"0".repeat(64));
        std::fs::remove_file(&p).unwrap();
        checked.unwrap();
        assert!(mismatch.is_err());
    }

    #[test]
    fn wait_for_pid_exit_times_out_on_a_live_pid() {
        // Our own pid is definitionally alive.
        let err = wait_for_pid_exit(std::process::id(), std::time::Duration::ZERO).unwrap_err();
        assert!(err.to_string().contains("still running"));
    }
}
