//! Extension-driven operations — the web-session integration surface.
//!
//! Search, feed browsing, group browsing, inbox reading and (reactive) DM have
//! no sanctioned API on most of these platforms, so they ride the user's real
//! logged-in session via the shared Chrome extension: the app asks the
//! extension to `ReplayApi` (an authenticated fetch run in the page context,
//! using tokens the extension captured and never exposed to us).
//!
//! Every call here goes through the cadence governor first. If the extension
//! isn't connected, or has no live session for the target host, the call fails
//! with a clear, actionable message rather than silently doing nothing.

use crate::cadence::Decision;
use crate::channels::Platform;
use crate::state::AppState;
use serde_json::{json, Value};
use std::time::Duration;

/// How long to wait for the extension to answer a replayed API call.
const CALL_TIMEOUT: Duration = Duration::from_secs(45);

/// Run one extension-backed action for `platform`, gated by cadence.
///
/// `action` is the cadence class (search|feed|groups|inbox|dm). `op` is the
/// high-level operation name the extension understands. `params` is passed
/// through to the extension verbatim (it decides the concrete endpoint + signing).
pub async fn run(
    state: &AppState,
    platform: Platform,
    account: &str,
    action: &str,
    op: &str,
    mut params: Value,
) -> Result<Value, String> {
    let plat = platform.as_str();

    // 0. Does this platform even have this capability? Refuse up front rather
    //    than sending the user to connect an extension for something that will
    //    never exist (Threads/TikTok/YouTube have no DM at all).
    let cap = match action {
        "dm" => "dm",
        "search" => "search",
        _ => "browse", // feed | groups | inbox
    };
    if platform.capability(cap) == crate::channels::Capability::None {
        let reason = platform.unsupported_reason(cap);
        state
            .core
            .db
            .log_action(plat, action, "unsupported", &reason);
        return Err(reason);
    }

    // 1. Extension must be connected with a live session for this host. This is
    //    checked FIRST — before touching the cadence governor — so an
    //    impossible action (extension down / not logged in) does NOT consume the
    //    daily quota or make the caller sleep for a request that can never run.
    if !state.ext.is_connected() {
        return Err(
            "Extension chưa kết nối — mở Chrome đã cài extension Social và đăng nhập nền tảng trước.".into(),
        );
    }
    let hosts = state.ext.hosts_ready();
    if !hosts.is_empty() && !hosts.iter().any(|h| h == plat) {
        return Err(format!(
            "Extension đã kết nối nhưng chưa thấy phiên đăng nhập {plat} — mở tab {plat} và đăng nhập, rồi thử lại."
        ));
    }

    // 2. Cadence gate (only now that the action can actually run).
    match state.cadence.reserve(plat, account, action) {
        Decision::Blocked { reason } => {
            state.core.db.log_action(plat, action, "blocked", &reason);
            return Err(reason);
        }
        Decision::Ok { delay } => {
            state.core.db.log_action(plat, action, "reserved", op);
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
        }
    }

    // 3. Replay through the extension.
    if let Value::Object(ref mut m) = params {
        m.insert("platform".into(), json!(plat));
        m.insert("account".into(), json!(account));
        m.insert("op".into(), json!(op));
    }
    let resp = state.ext.call("ReplayApi", params, CALL_TIMEOUT).await?;

    // The extension answers RPCs as `{id, result}` on success or `{id, error}`
    // on failure. Surface the error instead of swallowing it as success (a
    // silent DOM/API failure must NOT be logged as "sent").
    if let Some(err) = resp.get("error").and_then(|e| e.as_str()) {
        eprintln!("[social] ext op '{op}' ({plat}) error: {err}");
        state.core.db.log_action(plat, action, "error", err);
        return Err(err.to_string());
    }
    let out = resp.get("result").cloned().unwrap_or(resp);
    if let Some(err) = out.get("error").and_then(|e| e.as_str()) {
        eprintln!("[social] ext op '{op}' ({plat}) error: {err}");
        state.core.db.log_action(plat, action, "error", err);
        return Err(err.to_string());
    }
    eprintln!(
        "[social] ext op '{op}' ({plat}) ok: ref={} via={}",
        out.get("ref").and_then(|r| r.as_str()).unwrap_or("-"),
        out.get("via").and_then(|r| r.as_str()).unwrap_or("-"),
    );
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cadence::Cadence;
    use crate::db::Db;
    use crate::extbridge::ExtBridge;
    use crate::state::{AppState, Core};
    use std::sync::Arc;

    fn test_state(ext: ExtBridge) -> AppState {
        let (mcp_tx, _) = tokio::sync::broadcast::channel(8);
        AppState {
            core: Arc::new(Core {
                db: Db::open_memory().unwrap(),
            }),
            mcp_tx,
            ext,
            cadence: Arc::new(Cadence::new()),
        }
    }

    /// Full round-trip: web_ops::run → cadence → extbridge → (fake extension
    /// replies) → result flows back. Proves the extension path works, not just
    /// the "not connected" branch.
    #[tokio::test]
    async fn run_round_trips_through_a_fake_extension() {
        let ext = ExtBridge::new();
        let mut rx = ext.test_connect(); // pretend the extension is connected
        let ext_for_fake = ext.clone();

        // Fake extension: read the one outbound RPC, answer it by id.
        let fake = tokio::spawn(async move {
            if let Some(text) = rx.recv().await {
                let v: Value = serde_json::from_str(&text).unwrap();
                let id = v["id"].as_str().unwrap().to_string();
                // Echo back a canned "search result".
                ext_for_fake.complete_callback(&id, json!({ "id": id, "hits": ["a", "b"] }));
            }
        });

        let state = test_state(ext);
        let out = run(
            &state,
            Platform::Tiktok,
            "@shop",
            "search",
            "search",
            json!({ "query": "áo" }),
        )
        .await
        .expect("should succeed");
        assert_eq!(out["hits"], json!(["a", "b"]));
        fake.await.unwrap();
    }

    #[tokio::test]
    async fn run_errors_clearly_when_extension_absent() {
        let state = test_state(ExtBridge::new()); // never connected
        let err = run(
            &state,
            Platform::Facebook,
            "me",
            "search",
            "search",
            json!({}),
        )
        .await
        .unwrap_err();
        assert!(err.contains("Extension chưa kết nối"), "got: {err}");
    }

    /// A capability the platform doesn't have is refused up front — with the
    /// platform's own reason, not a misleading "connect the extension" (which
    /// would never help, since Threads has no DM at all).
    #[tokio::test]
    async fn unsupported_capability_is_refused_before_the_extension_check() {
        let state = test_state(ExtBridge::new());
        for p in [Platform::Threads, Platform::Tiktok, Platform::Youtube] {
            let err = run(&state, p, "@me", "dm", "send_dm", json!({}))
                .await
                .unwrap_err();
            assert!(
                err.contains("không hỗ trợ nhắn tin"),
                "{} should say it has no DM, got: {err}",
                p.as_str()
            );
            assert!(
                !err.contains("Extension chưa kết nối"),
                "must not blame the extension"
            );
        }
        // Logged as 'unsupported', and no cadence slot was spent.
        let acts = state.core.db.recent_actions(50).unwrap();
        assert!(acts.iter().all(|a| a["status"] == "unsupported"));
        assert!(matches!(
            state.cadence.reserve("threads", "@me", "dm"),
            crate::cadence::Decision::Ok { .. }
        ));
    }

    /// Platforms that DO have DM must get past the capability gate (and then
    /// fail on the extension, which is the honest next blocker).
    #[tokio::test]
    async fn supported_capability_passes_the_gate() {
        let state = test_state(ExtBridge::new());
        for p in [Platform::Facebook, Platform::X, Platform::Instagram] {
            let err = run(&state, p, "@me", "dm", "send_dm", json!({}))
                .await
                .unwrap_err();
            assert!(
                err.contains("Extension chưa kết nối"),
                "{}: {err}",
                p.as_str()
            );
        }
    }

    /// A failed-before-network call (extension down) must NOT consume the daily
    /// cadence quota and must NOT write a "reserved" action-log row — otherwise
    /// hammering a disconnected extension would exhaust the budget on no-ops.
    #[tokio::test]
    async fn absent_extension_does_not_consume_cadence_or_log() {
        let state = test_state(ExtBridge::new());
        // 'post' has the smallest cap (12); burn well past it against a down ext.
        for _ in 0..20 {
            let _ = run(&state, Platform::X, "@me", "post", "post", json!({})).await;
        }
        // No action rows at all (neither reserved nor blocked) — we bailed before
        // the cadence gate.
        assert!(state.core.db.recent_actions(50).unwrap().is_empty());
        // And the cadence budget is untouched: a fresh reserve still succeeds.
        assert!(matches!(
            state.cadence.reserve("x", "@me", "post"),
            crate::cadence::Decision::Ok { .. }
        ));
    }
}
