//! SenClaw Desktop self-update helper.
//!
//! The desktop app cannot replace the bundle it is running from, so it copies
//! this binary out of the bundle into `~/.senclaw/tmp`, spawns it detached and
//! quits. This process then: waits for the app to exit → verifies the download
//! → (Windows) kills leftover processes still locking the bundle → swaps the
//! bundle atomically → relaunches the app.
//!
//! This replaces the old flow of copying the full `senclaw` daemon binary and
//! running `senclaw apply-update` (see src/cli/commands/distrib.rs in the main
//! repo, which stays as the terminal/CLI path). That binary is a console app:
//! spawning it from the GUI flashed a console window, and its locker-kill +
//! shortcut steps shelled out to PowerShell — blocked outright on machines
//! with restrictive execution policies, which left the bundle locked and the
//! update dead. Here everything is native: on Windows this is a windowed
//! (no-console) binary showing a small progress window, and process cleanup
//! uses toolhelp + NtQueryInformationProcess instead of scripts.
//!
//! Usage (spawned by the app, not by humans):
//!   update_desktop --staged <archive> --target <bundle-dir> --pid <app-pid>
//!                  [--sha256 <hex>] [--relaunch]
#![cfg_attr(windows, windows_subsystem = "windows")]

mod apply;
#[cfg(windows)]
mod win;

use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{bail, Context, Result};

#[derive(Debug, PartialEq, Eq)]
pub struct Args {
    pub staged: PathBuf,
    pub target: PathBuf,
    pub pid: u32,
    pub sha256: Option<String>,
    pub relaunch: bool,
}

pub fn parse_args(argv: &[String]) -> Result<Args> {
    let (mut staged, mut target, mut pid, mut sha256, mut relaunch) =
        (None, None, None, None, false);
    let mut it = argv.iter();
    while let Some(arg) = it.next() {
        let mut value = |name: &str| {
            it.next()
                .cloned()
                .with_context(|| format!("{name} needs a value"))
        };
        match arg.as_str() {
            "--staged" => staged = Some(PathBuf::from(value("--staged")?)),
            "--target" => target = Some(PathBuf::from(value("--target")?)),
            "--pid" => {
                pid = Some(
                    value("--pid")?
                        .parse::<u32>()
                        .context("--pid not a number")?,
                )
            }
            "--sha256" => sha256 = Some(value("--sha256")?),
            "--relaunch" => relaunch = true,
            other => bail!("unknown argument: {other}"),
        }
    }
    Ok(Args {
        staged: staged.context("--staged is required")?,
        target: target.context("--target is required")?,
        pid: pid.context("--pid is required")?,
        sha256,
        relaunch,
    })
}

/// Append-only trace of every run in `~/.senclaw/tmp` — with no console, this
/// file IS the console.
pub struct Log {
    file: Option<std::fs::File>,
    start: Instant,
}

impl Log {
    fn open() -> Self {
        let file = apply::tmp_dir().ok().and_then(|dir| {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join("update-desktop.log"))
                .ok()
        });
        Self {
            file,
            start: Instant::now(),
        }
    }

    pub fn line(&mut self, msg: &str) {
        if let Some(f) = self.file.as_mut() {
            let _ = writeln!(f, "[{:7.1}s] {msg}", self.start.elapsed().as_secs_f32());
        }
    }
}

fn main() {
    let mut log = Log::open();
    log.line("---- update_desktop start ----");

    let args = match parse_args(&std::env::args().skip(1).collect::<Vec<_>>()) {
        Ok(a) => a,
        Err(e) => {
            log.line(&format!("bad arguments: {e:#}"));
            #[cfg(windows)]
            win::error_box(&format!(
                "SenClaw updater was started with bad arguments:\n{e:#}"
            ));
            std::process::exit(2);
        }
    };
    log.line(&format!(
        "staged={} target={} pid={} relaunch={}",
        args.staged.display(),
        args.target.display(),
        args.pid,
        args.relaunch
    ));

    #[cfg(windows)]
    let ui = win::Ui::open();

    let status = |log: &mut Log, msg: &str| {
        log.line(msg);
        #[cfg(windows)]
        ui.set_status(msg);
        #[cfg(not(windows))]
        let _ = msg;
    };

    let result = run(&args, &mut log, &status);

    match result {
        Ok(()) => {
            log.line("done");
            #[cfg(windows)]
            ui.close();
        }
        Err(e) => {
            log.line(&format!("FAILED: {e:#}"));
            // Same breadcrumb file the old updater left — support flows know it.
            if let Ok(tmp) = apply::tmp_dir() {
                let _ = std::fs::write(
                    tmp.join("apply-update-error.log"),
                    format!(
                        "apply-update failed for {}:\n{e:#}\n",
                        args.target.display()
                    ),
                );
            }
            // The swap is atomic, so whatever failed left the old install
            // intact — start it back up rather than leaving nothing running.
            if args.relaunch {
                let _ = apply::relaunch_app(&args.target);
            }
            #[cfg(windows)]
            {
                ui.close();
                win::error_box(&format!(
                    "SenClaw could not be updated:\n\n{e:#}\n\nThe previous version was kept and restarted."
                ));
            }
            std::process::exit(1);
        }
    }
}

fn run(args: &Args, log: &mut Log, status: &dyn Fn(&mut Log, &str)) -> Result<()> {
    status(log, "Waiting for SenClaw to close…");
    apply::wait_for_pid_exit(args.pid, std::time::Duration::from_secs(60))?;

    // Verify BEFORE touching the install: the bundle is unsigned, so this
    // checksum is the only thing between a corrupted download and the app dir.
    if let Some(expected) = args.sha256.as_deref() {
        status(log, "Verifying the downloaded bundle…");
        apply::verify_sha256(&args.staged, expected)?;
        log.line("checksum ok");
    }

    // The app and its daemon are gone (pid wait), but orphaned MCP-server
    // children and WebView2 helpers keep files in the bundle locked on
    // Windows — no amount of swap retries beats a process that never exits.
    #[cfg(windows)]
    {
        status(log, "Closing leftover SenClaw processes…");
        let killed = win::kill_target_lockers(&args.target);
        log.line(&format!("terminated {killed} locker process(es)"));
    }

    status(log, "Installing the update…");
    apply::swap_bundle_with_retry(&args.staged, &args.target, log, status)?;
    let _ = std::fs::remove_file(&args.staged);
    log.line(&format!("installed into {}", args.target.display()));

    #[cfg(target_os = "linux")]
    apply::write_linux_desktop_entry(&args.target)?;

    if args.relaunch {
        status(log, "Restarting SenClaw…");
        apply::relaunch_app(&args.target)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_the_full_contract() {
        let a = parse_args(&v(&[
            "--staged",
            "/tmp/b.zip",
            "--target",
            "/opt/app",
            "--pid",
            "42",
            "--sha256",
            "abc",
            "--relaunch",
        ]))
        .unwrap();
        assert_eq!(a.staged, PathBuf::from("/tmp/b.zip"));
        assert_eq!(a.target, PathBuf::from("/opt/app"));
        assert_eq!(a.pid, 42);
        assert_eq!(a.sha256.as_deref(), Some("abc"));
        assert!(a.relaunch);
    }

    #[test]
    fn sha_and_relaunch_are_optional() {
        let a = parse_args(&v(&["--staged", "s", "--target", "t", "--pid", "1"])).unwrap();
        assert_eq!(a.sha256, None);
        assert!(!a.relaunch);
    }

    #[test]
    fn rejects_missing_required_and_unknown_flags() {
        assert!(parse_args(&v(&["--staged", "s", "--pid", "1"])).is_err());
        assert!(parse_args(&v(&["--staged", "s", "--target", "t", "--pid", "x"])).is_err());
        assert!(parse_args(&v(&["--wat"])).is_err());
    }
}
