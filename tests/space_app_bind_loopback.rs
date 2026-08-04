//! Guard against the copy-pasted `0.0.0.0` bind returning to Space Apps.
//!
//! Every `apps/*` bootstrap descends from the same template, so a single bad
//! copy used to be enough to publish an app's unauthenticated REST + MCP
//! surface to the whole LAN. These tests re-derive that property from the
//! sources on every `cargo test` instead of trusting review to catch it.

use std::path::{Path, PathBuf};

/// Repo root — `CARGO_MANIFEST_DIR` is the workspace root for the `senclaw` crate.
fn apps_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("apps")
}

/// Every immediate child directory of `apps/`, sorted for stable failure output.
fn app_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(apps_dir())
        .expect("apps/ must exist")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs
}

/// Lines that are not whole-line `//` comments. Trailing comments stay attached,
/// which only ever makes the checks stricter — never laxer.
fn code_lines(src: &str) -> impl Iterator<Item = (usize, &str)> {
    src.lines()
        .enumerate()
        .map(|(i, l)| (i + 1, l))
        .filter(|(_, l)| !l.trim_start().starts_with("//"))
}

fn rel(path: &Path) -> String {
    path.strip_prefix(Path::new(env!("CARGO_MANIFEST_DIR")))
        .unwrap_or(path)
        .display()
        .to_string()
}

/// No app may hardcode a wildcard bind or advertise one in a URL.
#[test]
fn no_wildcard_bind_in_app_sources() {
    let mut offenders = Vec::new();

    for dir in app_dirs() {
        let candidates = [
            dir.join("src/main.rs"),
            dir.join("src/extbridge.rs"),
            dir.join("server.js"),
        ];
        for file in candidates.iter().filter(|p| p.is_file()) {
            let src = std::fs::read_to_string(file).unwrap();
            for (line_no, line) in code_lines(&src) {
                if line.contains("0.0.0.0") {
                    offenders.push(format!("{}:{}: {}", rel(file), line_no, line.trim()));
                }
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "Space Apps must bind loopback by default — bind the host from \
         SENCLAW_BIND_HOST (default 127.0.0.1) instead of hardcoding 0.0.0.0:\n{}",
        offenders.join("\n")
    );
}

/// Every app that opens a listener must route the host through the env knob, so
/// LAN exposure stays a deliberate opt-in rather than a template default.
#[test]
fn listeners_read_bind_host_env() {
    // Apps that predate the fleet-wide knob and keep their own override first.
    const CRATE_LOCAL_OVERRIDES: [(&str, &str); 2] = [
        ("rule-engine", "RULE_ENGINE_BIND"),
        ("sentinel", "SENTINEL_BIND"),
    ];

    let mut offenders = Vec::new();

    for dir in app_dirs() {
        let app = dir.file_name().unwrap().to_string_lossy().to_string();
        let extra = CRATE_LOCAL_OVERRIDES
            .iter()
            .find(|(name, _)| *name == app)
            .map(|(_, var)| *var);

        // Some apps resolve the host through a `config::bind_host()` helper
        // instead of inlining the `env::var` at the listener, so the knob can
        // live in the crate's own config module.
        let via_config = std::fs::read_to_string(dir.join("src/config.rs"))
            .map(|s| s.contains("SENCLAW_BIND_HOST"))
            .unwrap_or(false);

        for file in [dir.join("src/main.rs"), dir.join("src/extbridge.rs")]
            .iter()
            .filter(|p| p.is_file())
        {
            let src = std::fs::read_to_string(file).unwrap();
            let binds = code_lines(&src).any(|(_, l)| l.contains("TcpListener::bind"));
            if !binds {
                continue;
            }
            let configurable = src.contains("SENCLAW_BIND_HOST")
                || via_config
                || extra.is_some_and(|var| src.contains(var));
            if !configurable {
                offenders.push(rel(file));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "these listeners bind a fixed host — read SENCLAW_BIND_HOST \
         (default 127.0.0.1) so LAN access is opt-in:\n{}",
        offenders.join("\n")
    );
}

/// `next start` defaults to 0.0.0.0, so the Next.js apps need an explicit `-H`.
#[test]
fn next_apps_pin_bind_host() {
    let mut offenders = Vec::new();

    for dir in app_dirs() {
        let pkg = dir.join("package.json");
        if !pkg.is_file() {
            continue;
        }
        let src = std::fs::read_to_string(&pkg).unwrap();
        for (line_no, line) in src.lines().enumerate() {
            let starts_next = line.contains("next start") || line.contains("next dev");
            if starts_next && !line.contains("-H ") {
                offenders.push(format!("{}:{}: {}", rel(&pkg), line_no + 1, line.trim()));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "`next start`/`next dev` bind 0.0.0.0 unless given -H — pass \
         -H ${{SENCLAW_BIND_HOST:-127.0.0.1}}:\n{}",
        offenders.join("\n")
    );
}
