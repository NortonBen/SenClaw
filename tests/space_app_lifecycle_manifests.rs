//! Every bundled Space App's manifest has to mean what the launcher reads.
//!
//! `runtime.mode` decides whether an app is resident for the life of the daemon
//! or started on demand, and the two failure directions are both silent: a
//! misspelled mode falls back to `session`, so an app that must poll a channel
//! for inbound messages quietly stops doing so; a `background` on an app that
//! does not need it is fifty megabytes of RSS nobody notices. Neither shows up
//! in review, so it is derived from the sources here.
//!
//! The second test is the one that has teeth: it looks for the *signature* of
//! autonomous startup work in an app's own Rust — a spawned loop, a heartbeat,
//! a scheduler, an extension WebSocket bridge — and requires that app to be
//! declared `background`.

use std::path::{Path, PathBuf};

fn apps_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("apps")
}

struct App {
    id: String,
    dir: PathBuf,
    manifest: serde_json::Value,
}

fn apps() -> Vec<App> {
    let mut out: Vec<App> = std::fs::read_dir(apps_dir())
        .expect("apps/ must exist")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .filter_map(|dir| {
            let raw = std::fs::read_to_string(dir.join("senclaw-manifest.json")).ok()?;
            let manifest: serde_json::Value = serde_json::from_str(&raw)
                .unwrap_or_else(|e| panic!("{}: invalid manifest JSON: {e}", dir.display()));
            let id = manifest
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            Some(App { id, dir, manifest })
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// Concatenated Rust of an app, comments stripped, so a doc-comment mentioning
/// "heartbeat" cannot be mistaken for one.
fn app_sources(dir: &Path) -> String {
    fn walk(dir: &Path, out: &mut String) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.filter_map(Result::ok) {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                if let Ok(s) = std::fs::read_to_string(&p) {
                    for line in s.lines() {
                        if !line.trim_start().starts_with("//") {
                            out.push_str(line);
                            out.push('\n');
                        }
                    }
                }
            }
        }
    }
    let mut out = String::new();
    walk(&dir.join("src"), &mut out);
    out
}

#[test]
fn every_declared_mode_is_one_the_daemon_understands() {
    // A typo here does not fail anything at runtime — it silently means
    // `session`, which for a channel-polling app means it stops receiving.
    let mut bad = Vec::new();
    for app in apps() {
        let Some(mode) = app
            .manifest
            .get("runtime")
            .and_then(|r| r.get("mode"))
            .and_then(|v| v.as_str())
        else {
            continue;
        };
        if !matches!(mode, "background" | "session") {
            bad.push(format!("{}: runtime.mode = {mode:?}", app.id));
        }
    }
    assert!(
        bad.is_empty(),
        "runtime.mode must be \"background\" or \"session\":\n  {}",
        bad.join("\n  ")
    );
}

#[test]
fn a_mode_is_only_declared_on_an_app_that_has_a_process() {
    // `mode` on a static app is a claim about a lifecycle it does not have.
    let mut bad = Vec::new();
    for app in apps() {
        let rt = app.manifest.get("runtime");
        let has_mode = rt.and_then(|r| r.get("mode")).is_some();
        let is_server = rt.and_then(|r| r.get("kind")).and_then(|v| v.as_str()) == Some("server");
        if has_mode && !is_server {
            bad.push(app.id.clone());
        }
    }
    assert!(bad.is_empty(), "runtime.mode on a non-server app: {bad:?}");
}

#[test]
fn an_app_that_works_on_its_own_at_startup_is_declared_background() {
    // The expensive direction to get wrong. An app that polls a channel, runs a
    // schedule or holds the WebSocket a browser extension dials into does work
    // nobody asked for at that moment — as a session app it would be stopped a
    // minute after the last click, and the work would simply not happen.
    //
    // Each marker is a call an app makes *at startup*, not a word in prose:
    // comments are stripped above.
    const MARKERS: &[&str] = &[
        "extbridge::serve_ws",
        "spawn_heartbeat",
        "spawn_scheduler",
        "spawn_poller",
        "run_supervisor",
        "spawn_janitor",
    ];
    let mut missing = Vec::new();
    for app in apps() {
        let rt = app.manifest.get("runtime");
        if rt.and_then(|r| r.get("kind")).and_then(|v| v.as_str()) != Some("server") {
            continue;
        }
        let mode = rt
            .and_then(|r| r.get("mode"))
            .and_then(|v| v.as_str())
            .unwrap_or("session");
        if mode == "background" {
            continue;
        }
        let src = app_sources(&app.dir);
        if let Some(marker) = MARKERS.iter().find(|m| src.contains(**m)) {
            missing.push(format!("{} (calls `{marker}`)", app.id));
        }
    }
    assert!(
        missing.is_empty(),
        "these apps do autonomous work at startup but are not declared \
         `\"mode\": \"background\"` — as session apps the daemon stops them when idle:\n  {}",
        missing.join("\n  ")
    );
}

#[test]
fn the_apps_we_declared_background_are_still_declared_background() {
    // A pin, so an app's manifest cannot lose the mode in a reformat or a
    // regenerated file without a test saying which one and why.
    const BACKGROUND: &[(&str, &str)] = &[
        ("ai-chat", "polls Telegram/Zalo/FB/TikTok for inbound messages"),
        ("ai-office", "claims and runs queued team tasks"),
        ("discuss", "drives running discussions on a 700ms tick"),
        // Declared background at the user's request (2026-08-07). The first has
        // work of its own; the other two are resident because the user wants
        // them instant, not because they poll anything — a legitimate reason to
        // spend the RAM, and the reason is recorded here so nobody "fixes" it
        // back to session for looking idle.
        ("ssh-manager", "log-retention sweep every 30s + live port-forward tunnels"),
        ("email", "user-requested: always resident"),
        ("kaen", "user-requested: always resident"),
        ("shopee", "customer-support heartbeat"),
        ("autotest", "runs scheduled test suites every 30s"),
        ("crm", "channel pollers + sale scheduler"),
        ("facebook-pro", "rule heartbeat"),
        ("lakehouse", "ETL scheduler poller"),
        ("moltbook", "draft/approve heartbeat"),
        ("news", "RSS auto-fetch collector"),
        ("predict", "staleness-aware fetches + ledger auto-resolve"),
        ("rule-engine", "resumes active chains + janitor"),
        ("sentinel", "periodic security tick"),
        ("social", "browser-extension WebSocket bridge"),
        ("tiktok-activity", "browser-extension WebSocket bridge"),
        ("tiktok-dl", "download worker pool, resumes queued jobs"),
        ("video-flow", "browser-extension WebSocket bridge + DAG worker"),
        ("youtube", "browser-extension WebSocket bridge"),
    ];
    let all = apps();
    for (id, why) in BACKGROUND {
        let Some(app) = all.iter().find(|a| a.id == *id) else {
            continue; // the app was removed from this checkout; not this test's business
        };
        let mode = app
            .manifest
            .get("runtime")
            .and_then(|r| r.get("mode"))
            .and_then(|v| v.as_str())
            .unwrap_or("session");
        assert_eq!(
            mode, "background",
            "apps/{id} must stay `\"mode\": \"background\"` — it {why}"
        );
    }
}

#[test]
fn most_apps_are_on_demand() {
    // The default is the point of the change: before it, every installed app
    // was resident for as long as the daemon lived. If this ever inverts, the
    // cost is a machine running fifty idle servers again.
    let all = apps();
    let server_count = all
        .iter()
        .filter(|a| {
            a.manifest.get("runtime").and_then(|r| r.get("kind")).and_then(|v| v.as_str())
                == Some("server")
        })
        .count();
    let background = all
        .iter()
        .filter(|a| {
            a.manifest.get("runtime").and_then(|r| r.get("mode")).and_then(|v| v.as_str())
                == Some("background")
        })
        .count();
    assert!(server_count > 0, "no server apps found — is apps/ empty?");
    assert!(
        background * 2 < server_count,
        "{background} of {server_count} apps are always-on; session is meant to be the norm"
    );
}
