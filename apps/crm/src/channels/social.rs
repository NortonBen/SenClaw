//! Social Space App bridge — pulls inbound DMs that the `apps/social` app
//! captured via its Chrome extension (personal Facebook Messenger, Instagram,
//! TikTok, X DMs — the surfaces the official `facebook`/`zalo` adapters cannot
//! reach), and lets operators reply back through it.
//!
//! Pull-based, matching this manager's poll design: we GET social's cursor feed
//! `GET {base_url}/api/inbox?since={cursor}` and advance the channel cursor by
//! the last row id seen. Replies POST to `{base_url}/api/inbox/reply`, which
//! routes through social's own autonomy gate + cadence.
//!
//! Config: `{ "base_url": "http://127.0.0.1:4520" }`.
//!
//! The `external_id` handed to the inbox is namespaced `"{platform}:{id}"` so a
//! thread id that collides across social platforms stays a distinct CRM
//! conversation; `send()` splits it back apart.

use crate::channels::{now_secs, Inbound};
use crate::db::Db;
use crate::db_inbox::Channel;
use serde_json::Value;
use std::sync::Arc;

fn cfg<'a>(ch: &'a Channel, key: &str) -> &'a str {
    ch.config.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

fn base_url(ch: &Channel) -> String {
    let b = cfg(ch, "base_url");
    let b = if b.is_empty() {
        "http://127.0.0.1:4520"
    } else {
        b
    };
    b.trim_end_matches('/').to_string()
}

pub async fn poll(db: &Arc<Db>, ch: &Channel) -> Result<Vec<Inbound>, String> {
    let since: i64 = ch.cursor.parse().unwrap_or(0);
    let url = format!("{}/api/inbox?since={since}&limit=200", base_url(ch));
    let resp = reqwest::Client::new()
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("social feed lỗi mạng: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("social feed HTTP {}", resp.status()));
    }
    let body: Value = resp
        .json()
        .await
        .map_err(|e| format!("social feed JSON lỗi: {e}"))?;
    let msgs = body
        .get("messages")
        .and_then(|m| m.as_array())
        .cloned()
        .unwrap_or_default();

    let mut out = Vec::new();
    let mut newest = since;
    for m in &msgs {
        let id = m.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
        if id > newest {
            newest = id;
        }
        let text = m
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if text.is_empty() {
            continue;
        }
        let platform = m.get("platform").and_then(|v| v.as_str()).unwrap_or("");
        let ext = m.get("external_id").and_then(|v| v.as_str()).unwrap_or("");
        let sender = m
            .get("sender")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        out.push(Inbound {
            external_id: format!("{platform}:{ext}"),
            customer_name: sender,
            text,
        });
    }
    if newest > since {
        let _ = db.set_channel_sync(ch.id, "ok", "", Some(&newest.to_string()), now_secs());
    }
    Ok(out)
}

pub async fn send(
    _db: &Arc<Db>,
    ch: &Channel,
    external_id: &str,
    text: &str,
) -> Result<(), String> {
    // external_id was namespaced "{platform}:{real_id}" during poll.
    let (platform, ext) = external_id.split_once(':').unwrap_or(("", external_id));
    let url = format!("{}/api/inbox/reply", base_url(ch));
    let resp = reqwest::Client::new()
        .post(&url)
        .json(&serde_json::json!({ "platform": platform, "external_id": ext, "text": text }))
        .send()
        .await
        .map_err(|e| format!("gửi qua social lỗi mạng: {e}"))?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("social reply HTTP {}", resp.status()))
    }
}

/// Health check: can we reach the social app's status endpoint?
pub async fn health_check(ch: &Channel) -> Result<String, String> {
    let url = format!("{}/api/status", base_url(ch));
    let resp = reqwest::Client::new()
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("không kết nối được social app: {e}"))?;
    if resp.status().is_success() {
        Ok(format!("Social app OK ({})", base_url(ch)))
    } else {
        Err(format!("social app HTTP {}", resp.status()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(config: Value) -> Channel {
        Channel {
            id: 1,
            kind: "social".into(),
            name: "s".into(),
            config,
            enabled: true,
            cursor: "0".into(),
            last_sync_at: None,
            last_status: String::new(),
            last_error: String::new(),
            created_at: 0,
        }
    }

    #[test]
    fn base_url_defaults_and_trims() {
        assert_eq!(base_url(&ch(json!({}))), "http://127.0.0.1:4520");
        assert_eq!(
            base_url(&ch(json!({"base_url": "http://x:9/"}))),
            "http://x:9"
        );
    }

    #[test]
    fn send_splits_the_namespaced_external_id() {
        // Just the parsing half (no network): "{platform}:{id}" → parts.
        let (p, e) = "facebook:t-42".split_once(':').unwrap();
        assert_eq!(p, "facebook");
        assert_eq!(e, "t-42");
        // A bare id (no namespace) degrades to empty platform, id unchanged.
        let (p2, e2) = "t-9".split_once(':').unwrap_or(("", "t-9"));
        assert_eq!(p2, "");
        assert_eq!(e2, "t-9");
    }

    use serde_json::json;
}
