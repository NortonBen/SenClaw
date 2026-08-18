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

    if let Err(e) = extract_bundle(staged, &new, parent).and_then(|()| verify_bundle_payload(&new))
    {
        let _ = remove_path(&new);
        return Err(e);
    }

    // Past this point the live bundle is in motion — keep the window tight.
    let had_old = target.exists();
    if had_old {
        if let Err(dir_err) = std::fs::rename(target, &old) {
            // Windows refuses to rename a DIRECTORY that holds an open file
            // (os error 32) — one surviving handle anywhere in the bundle is
            // enough, and killing processes cannot be made airtight. Renaming
            // the files one by one is still allowed even while they are mapped
            // as running images (the loader opens them with FILE_SHARE_DELETE),
            // so fall back to that before giving up.
            match swap_entries(&new, target, &old) {
                Ok(()) => {
                    let _ = remove_path(&old);
                    let _ = remove_path(&new);
                    return Ok(());
                }
                Err(entry_err) => {
                    let _ = remove_path(&new);
                    let _ = remove_path(&old);
                    return Err(entry_err.context(format!(
                        "cannot move {} aside — a file inside it is locked by a \
                         running process, or the directory is not writable \
                         ({dir_err})",
                        target.display()
                    )));
                }
            }
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

/// Swap `new` into `target` one directory entry at a time, parking the old
/// entries in `quarantine`.
///
/// The fallback for a refused whole-directory rename. Ordering is chosen so a
/// failure is always recoverable: every old entry is evacuated first, and only
/// then are the new ones moved in — so a bundle that cannot be fully replaced
/// is fully restored instead of left half-swapped and unstartable.
/// The speech sidecar's file name, mirroring `MEDIA_BIN` in the main repo's
/// src/media_sidecar.rs — the module that looks for it at runtime.
const MEDIA_BIN: &str = if cfg!(windows) {
    "senclaw-media.exe"
} else {
    "senclaw-media"
};

/// MLX loads its kernels from a `mlx.metallib` next to the executable using
/// them; the path compiled into `mlx-sys` points into the CI build directory
/// and does not exist on a user's machine.
#[cfg(target_os = "macos")]
const METALLIB: &str = "mlx.metallib";

/// Every file an extracted desktop bundle must contain, and where.
fn bundle_payload(bundle: &Path) -> Vec<(&'static str, PathBuf)> {
    #[cfg(target_os = "macos")]
    {
        let res = bundle.join("Contents").join("Resources");
        vec![
            ("senclaw", res.join("senclaw")),
            (MEDIA_BIN, res.join(MEDIA_BIN)),
            (METALLIB, res.join(METALLIB)),
        ]
    }
    #[cfg(target_os = "windows")]
    {
        vec![
            ("senclaw.exe", bundle.join("senclaw.exe")),
            (MEDIA_BIN, bundle.join(MEDIA_BIN)),
        ]
    }
    #[cfg(target_os = "linux")]
    {
        vec![
            ("senclaw", bundle.join("senclaw")),
            (MEDIA_BIN, bundle.join(MEDIA_BIN)),
        ]
    }
}

/// Refuse to install a bundle missing the daemon or the speech sidecar.
///
/// Runs on the freshly extracted `.new` copy, BEFORE the live bundle is moved
/// aside — the one moment where rejecting costs nothing and the user keeps a
/// working install.
///
/// Both files fail *late*: a bundle with no `senclaw` is a desktop app that
/// never starts, and one with no `senclaw-media` looks healthy right up until
/// someone records a voice message weeks later. The sidecar ships beside the
/// daemon on every platform, so a missing copy is a broken release build.
fn verify_bundle_payload(bundle: &Path) -> Result<()> {
    let missing: Vec<&str> = bundle_payload(bundle)
        .into_iter()
        .filter(|(_, path)| !path.is_file())
        .map(|(name, _)| name)
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    bail!(
        "the downloaded bundle is incomplete — missing {}. Nothing was installed \
         and the existing app is untouched; this is a bad release build",
        missing.join(", ")
    )
}

fn swap_entries(new: &Path, target: &Path, quarantine: &Path) -> Result<()> {
    std::fs::create_dir_all(quarantine)
        .with_context(|| format!("cannot create {}", quarantine.display()))?;

    // Phase 1 — evacuate the live install.
    let mut evacuated: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut stuck: Vec<String> = Vec::new();
    for entry in
        std::fs::read_dir(target).with_context(|| format!("cannot read {}", target.display()))?
    {
        let entry = entry?;
        let from = entry.path();
        let to = quarantine.join(entry.file_name());
        match std::fs::rename(&from, &to) {
            Ok(()) => evacuated.push((from, to)),
            // Keep going: naming EVERY stuck file is what makes the report
            // actionable ("close X"), rather than just the first one.
            Err(e) => stuck.push(format!("{} ({e})", entry.file_name().to_string_lossy())),
        }
    }
    if !stuck.is_empty() {
        restore_evacuated(&evacuated);
        anyhow::bail!("these files are still in use: {}", stuck.join(", "));
    }

    // Phase 2 — move the new bundle in.
    let mut placed: Vec<PathBuf> = Vec::new();
    for entry in
        std::fs::read_dir(new).with_context(|| format!("cannot read {}", new.display()))?
    {
        let entry = entry?;
        let to = target.join(entry.file_name());
        if let Err(e) = std::fs::rename(entry.path(), &to) {
            for p in &placed {
                let _ = remove_path(p);
            }
            restore_evacuated(&evacuated);
            return Err(anyhow::Error::new(e).context(format!(
                "cannot install {} (rolled back)",
                entry.file_name().to_string_lossy()
            )));
        }
        placed.push(to);
    }
    Ok(())
}

/// Put evacuated entries back. Best-effort by nature — it runs on a path that
/// has already failed, and a partial restore still leaves more of the old
/// install than giving up would.
fn restore_evacuated(evacuated: &[(PathBuf, PathBuf)]) {
    for (from, to) in evacuated {
        let _ = std::fs::rename(to, from);
    }
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
                    // Re-sweep every attempt, not just once before the first:
                    // a process can finish dying (or be spawned) between
                    // attempts, and the sweep is what makes a retry more than
                    // a wait.
                    #[cfg(windows)]
                    {
                        let killed = crate::win::kill_target_lockers(target);
                        log.line(&format!("re-swept {killed} locker process(es)"));
                    }
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

    // ── swap_entries: the locked-directory fallback ──────────────────────────

    fn write(path: &Path, body: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    // ── verify_bundle_payload ────────────────────────────────────────────────

    /// Lay out a complete bundle for this platform, then drop `omit` from it.
    fn staged_bundle(root: &Path, omit: Option<&str>) -> PathBuf {
        let bundle = root.join("bundle");
        for (name, path) in bundle_payload(&bundle) {
            if Some(name) == omit {
                continue;
            }
            write(&path, name);
        }
        bundle
    }

    /// The guarantee: an update carrying the daemon but not the speech sidecar
    /// must be refused. Installed, it would look like a clean update until the
    /// first voice message — long after anyone would connect the two.
    #[test]
    fn verify_bundle_payload_rejects_a_bundle_without_the_speech_sidecar() {
        let tmp = tempfile::tempdir().unwrap();
        let bundle = staged_bundle(tmp.path(), Some(MEDIA_BIN));

        let err = verify_bundle_payload(&bundle).unwrap_err().to_string();
        assert!(err.contains(MEDIA_BIN), "must name what is missing: {err}");
    }

    #[test]
    fn verify_bundle_payload_accepts_a_complete_bundle() {
        let tmp = tempfile::tempdir().unwrap();
        let bundle = staged_bundle(tmp.path(), None);

        verify_bundle_payload(&bundle).unwrap();
    }

    #[test]
    fn swap_entries_replaces_every_entry_and_parks_the_old_ones() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("app");
        let new = tmp.path().join("app.new");
        let quarantine = tmp.path().join("app.old");

        write(&target.join("senclaw.exe"), "v1");
        write(&target.join("data/marker"), "v1");
        write(&new.join("senclaw.exe"), "v2");
        write(&new.join("data/marker"), "v2");
        write(&new.join("added.dll"), "v2");

        swap_entries(&new, &target, &quarantine).unwrap();

        assert_eq!(
            std::fs::read_to_string(target.join("senclaw.exe")).unwrap(),
            "v2"
        );
        assert_eq!(
            std::fs::read_to_string(target.join("data/marker")).unwrap(),
            "v2"
        );
        assert!(target.join("added.dll").exists(), "new file not installed");
        assert_eq!(
            std::fs::read_to_string(quarantine.join("senclaw.exe")).unwrap(),
            "v1",
            "the old copy must be parked, not destroyed"
        );
    }

    /// The property that matters on Windows: when one file cannot be moved,
    /// the user keeps the OLD install intact — a half-swapped bundle would
    /// not start at all.
    #[test]
    fn swap_entries_restores_the_old_install_when_one_entry_is_stuck() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("app");
        let new = tmp.path().join("app.new");
        let quarantine = tmp.path().join("app.old");

        write(&target.join("a.txt"), "v1");
        write(&target.join("locked/inner"), "v1");
        write(&new.join("a.txt"), "v2");
        write(&new.join("locked/inner"), "v2");
        // Stands in for a locked entry: renaming a directory onto a non-empty
        // one fails, the same "this entry will not move" shape as a mapped exe.
        write(&quarantine.join("locked/occupied"), "squatter");

        let err = swap_entries(&new, &target, &quarantine).unwrap_err();
        assert!(
            format!("{err:#}").contains("locked"),
            "the stuck entry must be named: {err:#}"
        );
        assert_eq!(
            std::fs::read_to_string(target.join("a.txt")).unwrap(),
            "v1",
            "the evacuated entry was not put back"
        );
        assert!(target.join("locked/inner").exists());
    }

    /// A whole-directory rename is impossible while a file inside is open, so
    /// the fallback must work on a target the caller could not move.
    #[test]
    fn swap_entries_works_where_a_directory_rename_would_not() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("app");
        let new = tmp.path().join("app.new");
        write(&target.join("senclaw.exe"), "v1");
        write(&new.join("senclaw.exe"), "v2");

        // Hold the file open for the duration, the closest portable stand-in
        // for a running image.
        let _held = std::fs::File::open(target.join("senclaw.exe")).unwrap();
        swap_entries(&new, &target, &tmp.path().join("app.old")).unwrap();

        assert_eq!(
            std::fs::read_to_string(target.join("senclaw.exe")).unwrap(),
            "v2"
        );
    }
}
