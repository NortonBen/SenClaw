//! Bake git + build metadata into the binary at compile time.
//!
//! `senclaw version` (and clap's `--version`) report these. Everything is
//! best-effort: building from a source tarball with no `.git`, or on a
//! machine without git, must still compile — the fields just read "unknown".

use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}

fn main() {
    // `--always` keeps this working on shallow CI checkouts that fetched no
    // tags: the describe degrades to the bare short hash instead of failing.
    let describe = git(&["describe", "--tags", "--always", "--dirty=+"])
        .unwrap_or_else(|| "unknown".into());
    let hash = git(&["rev-parse", "--short=12", "HEAD"]).unwrap_or_else(|| "unknown".into());

    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // The build-script TARGET env var is the triple being compiled FOR —
    // matches the release asset naming (aarch64-apple-darwin, …).
    let target = std::env::var("TARGET").unwrap_or_else(|_| "unknown".into());

    println!("cargo:rustc-env=SENCLAW_GIT_DESCRIBE={describe}");
    println!("cargo:rustc-env=SENCLAW_GIT_HASH={hash}");
    println!("cargo:rustc-env=SENCLAW_BUILD_EPOCH={epoch}");
    println!("cargo:rustc-env=SENCLAW_BUILD_TARGET={target}");

    // Recompile the metadata when the checked-out commit moves. (Deliberately
    // NOT watching the index: that would rebuild on every `git add`.)
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs");
}
