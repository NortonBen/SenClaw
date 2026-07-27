//! Autonomy gate — draft → approve → live.
//!
//! Every write (post/reply) flows through here. The app's `autonomy` setting
//! decides what happens:
//!   * `observe` — writes are refused (read-only).
//!   * `draft`   — a write is recorded as a pending draft; a human must approve.
//!   * `live`    — the write executes immediately (still through the cadence
//!                 governor + audit log).
//!
//! This composes with `cadence.rs`: draft→approve→live is the human-in-the-loop
//! control, cadence is the pacing control. Both are on for a live write.

use crate::cadence::Decision;
use crate::channels::Platform;
use crate::state::AppState;
use crate::web_ops;
use serde_json::{json, Value};

/// Submit a write. Returns the data to hand back to the caller (MCP wraps it),
/// or an error string. In draft mode the data carries `{drafted, draft_id}`.
pub async fn submit(
    state: &AppState,
    kind: &str,
    platform: Platform,
    handle: &str,
    text: &str,
    thread_id: &str,
    media: &Value,
) -> Result<Value, String> {
    // Refuse a capability the platform doesn't have before anything else — no
    // point drafting a DM for a platform with no DM.
    if kind == "reply" && platform.capability("dm") == crate::channels::Capability::None {
        return Err(platform.unsupported_reason("dm"));
    }
    let has_media = media.as_array().map(|a| !a.is_empty()).unwrap_or(false);
    match state.core.db.autonomy().as_str() {
        "observe" => Err(
            "Chế độ observe: chỉ đọc, không đăng/nhắn. Đổi bằng social_autonomy(mode=\"draft\"|\"live\").".into(),
        ),
        "draft" => {
            let id = state
                .core
                .db
                .create_draft(platform.as_str(), handle, kind, text, thread_id, media)
                .map_err(|e| e.to_string())?;
            Ok(json!({
                "drafted": true,
                "draft_id": id,
                "note": "Đã tạo nháp — gọi social_approve để gửi, hoặc social_reject để bỏ."
            }))
        }
        _ /* live */ => {
            // Image publishing isn't wired into the official-API path yet; a live
            // write goes out text-only. Route through draft when media matters.
            if has_media {
                return Err(
                    "Đăng kèm ảnh chỉ hỗ trợ ở chế độ draft (đăng ảnh thật lên nền tảng chưa nối). Đổi sang draft để lưu nháp kèm ảnh.".into(),
                );
            }
            let ref_id = execute_write(state, kind, platform, handle, text, thread_id).await?;
            Ok(json!({ "ok": true, "kind": kind, "ref_id": ref_id }))
        }
    }
}

/// Actually perform the write (shared by live-mode submit and draft approval).
/// `post` → official API (through cadence); `reply` → extension DM.
pub async fn execute_write(
    state: &AppState,
    kind: &str,
    platform: Platform,
    handle: &str,
    text: &str,
    thread_id: &str,
) -> Result<String, String> {
    let plat = platform.as_str();
    match kind {
        "post" => {
            let cfg = state.core.db.official_config(plat, handle);
            // Not officially configured (no Page token / only a captured
            // web_session) → we can't use the Graph API.
            if !crate::channels::official_configured(platform, &cfg) {
                // Facebook personal profile has no post API — drive the on-page
                // composer through the extension (uses the logged-in session).
                if platform == Platform::Facebook {
                    // The extension posts via FB's internal GraphQL (learned from
                    // a real request) when available, else the DOM composer.
                    let v = web_ops::run(state, platform, handle, "post", "post", json!({ "text": text }))
                        .await?;
                    let ref_id = v.get("ref").and_then(|r| r.as_str()).unwrap_or("posted").to_string();
                    let via = v.get("via").and_then(|r| r.as_str()).unwrap_or("");
                    state.core.db.log_post(plat, "post", &ref_id, "ok", text);
                    if !via.is_empty() {
                        state.core.db.log_action(plat, "post", "sent", via);
                    }
                    return Ok(ref_id);
                }
                // Other platforms: surface the clear "needs official config" error
                // without spending a cadence slot (no network hit).
                let e = crate::channels::official_post(platform, &cfg, text)
                    .await
                    .unwrap_err();
                state.core.db.log_post(plat, "post", "", "error", &e);
                return Err(format!("{e}\n(gợi ý: {})", platform.official_note()));
            }
            match state.cadence.reserve(plat, handle, "post") {
                Decision::Blocked { reason } => {
                    state.core.db.log_action(plat, "post", "blocked", &reason);
                    return Err(reason);
                }
                Decision::Ok { delay } => {
                    state.core.db.log_action(plat, "post", "reserved", "official_post");
                    if !delay.is_zero() {
                        tokio::time::sleep(delay).await;
                    }
                }
            }
            match crate::channels::official_post(platform, &cfg, text).await {
                Ok(ref_id) => {
                    state.core.db.log_post(plat, "post", &ref_id, "ok", text);
                    Ok(ref_id)
                }
                Err(e) => {
                    state.core.db.log_post(plat, "post", "", "error", &e);
                    Err(format!("{e}\n(gợi ý: {})", platform.official_note()))
                }
            }
        }
        "reply" => {
            if thread_id.is_empty() {
                return Err("reply cần thread_id (DM chỉ để trả lời)".into());
            }
            let params = json!({
                "platform": plat, "handle": handle, "thread_id": thread_id, "text": text
            });
            // web_ops::run applies the "dm" cadence class itself.
            web_ops::run(state, platform, handle, "dm", "send_dm", params).await?;
            let _ = state.core.db.insert_inbox(plat, thread_id, "", "out", text);
            Ok("sent".to_string())
        }
        other => Err(format!("kind không hỗ trợ: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cadence::Cadence;
    use crate::db::Db;
    use crate::extbridge::ExtBridge;
    use crate::state::{AppState, Core};
    use std::sync::Arc;

    fn state() -> AppState {
        let (mcp_tx, _) = tokio::sync::broadcast::channel(8);
        AppState {
            core: Arc::new(Core { db: Db::open_memory().unwrap() }),
            mcp_tx,
            ext: ExtBridge::new(),
            cadence: Arc::new(Cadence::new()),
        }
    }

    #[tokio::test]
    async fn observe_mode_refuses_writes() {
        let s = state();
        s.core.db.set_setting("autonomy", "observe").unwrap();
        let err = submit(&s, "post", Platform::X, "@me", "hi", "", &json!([])).await.unwrap_err();
        assert!(err.contains("observe"), "got: {err}");
    }

    #[tokio::test]
    async fn draft_mode_records_a_pending_draft_and_does_not_send() {
        let s = state();
        // default autonomy is "draft"
        let out = submit(&s, "post", Platform::Threads, "@me", "xin chào", "", &json!([])).await.unwrap();
        assert_eq!(out["drafted"], json!(true));
        let drafts = s.core.db.list_drafts(Some("pending"), 10).unwrap();
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0]["text"], "xin chào");
        // Nothing was posted.
        assert_eq!(s.core.db.recent_posts(10).unwrap().len(), 0);
    }

    /// A drafted reply, once approved, actually goes out through the extension
    /// and is recorded in the inbox as an outbound message.
    #[tokio::test]
    async fn approving_a_reply_draft_sends_via_extension_and_logs_it() {
        let ext = ExtBridge::new();
        let mut rx = ext.test_connect();
        let ext_fake = ext.clone();
        let fake = tokio::spawn(async move {
            if let Some(text) = rx.recv().await {
                let v: Value = serde_json::from_str(&text).unwrap();
                let id = v["id"].as_str().unwrap().to_string();
                ext_fake.complete_callback(&id, json!({ "id": id, "sent": true }));
            }
        });

        let (mcp_tx, _) = tokio::sync::broadcast::channel(8);
        let s = AppState {
            core: Arc::new(Core { db: Db::open_memory().unwrap() }),
            mcp_tx,
            ext,
            cadence: Arc::new(Cadence::new()),
        };

        // 1. Drafted (default mode), nothing sent yet.
        let out = submit(&s, "reply", Platform::Facebook, "Page", "cảm ơn bạn", "t-42", &json!([]))
            .await
            .unwrap();
        assert_eq!(out["drafted"], json!(true));
        assert!(s.core.db.list_inbox(None, 10).unwrap().is_empty());

        // 2. Approve → executes through the extension.
        let ref_id = execute_write(&s, "reply", Platform::Facebook, "Page", "cảm ơn bạn", "t-42")
            .await
            .unwrap();
        assert_eq!(ref_id, "sent");
        let inbox = s.core.db.list_inbox(None, 10).unwrap();
        assert_eq!(inbox.len(), 1);
        assert_eq!(inbox[0]["direction"], "out");
        assert_eq!(inbox[0]["external_id"], "t-42");
        fake.await.unwrap();
    }

    /// A Facebook personal profile (only a captured web_session, no Page token)
    /// posts via the extension DOM composer instead of erroring on `page_id`.
    #[tokio::test]
    async fn facebook_personal_post_routes_to_extension_dom() {
        let ext = ExtBridge::new();
        let mut rx = ext.test_connect();
        let ext_fake = ext.clone();
        let fake = tokio::spawn(async move {
            if let Some(text) = rx.recv().await {
                let v: Value = serde_json::from_str(&text).unwrap();
                let id = v["id"].as_str().unwrap().to_string();
                assert_eq!(v["params"]["op"], "post");
                assert_eq!(v["params"]["text"], "Test");
                ext_fake.complete_callback(&id, json!({ "id": id, "ok": true, "ref": "gql:123", "via": "graphql" }));
            }
        });
        let (mcp_tx, _) = tokio::sync::broadcast::channel(8);
        let s = AppState {
            core: Arc::new(Core { db: Db::open_memory().unwrap() }),
            mcp_tx,
            ext,
            cadence: Arc::new(Cadence::new()),
        };
        // Saved with only a web_session — NOT a real Page config.
        s.core
            .db
            .upsert_account("facebook", "bacnd.120", "Nguyễn Bắc", &json!({ "web_session": { "fb_dtsg": "x" } }))
            .unwrap();
        let ref_id = execute_write(&s, "post", Platform::Facebook, "bacnd.120", "Test", "").await.unwrap();
        assert_eq!(ref_id, "gql:123");
        assert_eq!(s.core.db.recent_posts(10).unwrap()[0]["status"], "ok");
        fake.await.unwrap();
    }

    #[tokio::test]
    async fn unconfigured_post_does_not_consume_the_post_quota() {
        let s = state();
        s.core.db.set_setting("autonomy", "live").unwrap();
        // Attempt many posts on an account with no official_config.
        for _ in 0..20 {
            let _ = submit(&s, "post", Platform::X, "@nocfg", "hi", "", &json!([])).await;
        }
        // Every attempt was logged as a failed post…
        assert_eq!(s.core.db.recent_posts(50).unwrap().len(), 20);
        // …but no cadence slot was reserved (no "reserved" action rows), so the
        // post budget is intact.
        let acts = s.core.db.recent_actions(50).unwrap();
        assert!(acts.iter().all(|a| a["status"] != "reserved"), "no reservation expected");
        assert!(matches!(
            s.cadence.reserve("x", "@nocfg", "post"),
            crate::cadence::Decision::Ok { .. }
        ));
    }

    #[tokio::test]
    async fn reply_without_thread_id_is_refused() {
        let s = state();
        let err = execute_write(&s, "reply", Platform::X, "@me", "hi", "").await.unwrap_err();
        assert!(err.contains("thread_id"), "got: {err}");
    }

    #[tokio::test]
    async fn live_mode_attempts_the_write_immediately() {
        let s = state();
        s.core.db.set_setting("autonomy", "live").unwrap();
        // No official_config on a platform with an official-only post path (X) →
        // the write is attempted and fails clearly (proving it did NOT just
        // draft), and the attempt is logged.
        let err = submit(&s, "post", Platform::X, "@me", "hi", "", &json!([])).await.unwrap_err();
        assert!(err.contains("access_token"), "got: {err}");
        assert_eq!(s.core.db.list_drafts(None, 10).unwrap().len(), 0, "live must not draft");
        assert_eq!(s.core.db.recent_posts(10).unwrap()[0]["status"], "error");
    }
}
