//! Client for the SenClaw CRM Space App (`apps/crm`, default port 4390).
//!
//! Recognizes an inbound customer: when someone messages from Telegram/Zalo/
//! Facebook/etc., search the CRM by the channel identifier and then by display
//! name; if a customer matches, pull a compact profile so the bot (and the
//! operator) know who they are. Entirely fail-safe — if the CRM app is down or
//! nothing matches, it returns `None` and the chat proceeds normally.

use crate::llm::{base_url, http};
use crate::senclaw::urlencode;
use serde_json::{json, Value};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

const CRM_APP_ID: &str = "crm";
const CRM_FALLBACK: &str = "http://127.0.0.1:4390";

/// Cached auto-discovered base (the CRM app's port rarely changes; a failed
/// discovery isn't cached so a later install is still picked up).
fn cache() -> &'static Mutex<Option<String>> {
    static CACHE: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

/// Ask the SenClaw daemon which port the installed CRM Space App runs on
/// (`GET /api/space/apps` → the `crm` entry's `manifest.runtime.port`).
async fn discover() -> Option<String> {
    let url = format!("{}/api/space/apps", base_url().trim_end_matches('/'));
    let v: Value = http()
        .get(&url)
        .timeout(Duration::from_secs(3))
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    let apps = v.as_array()?;
    let crm = apps.iter().find(|a| a["id"].as_str() == Some(CRM_APP_ID))?;
    let port = crm["manifest"]["runtime"]["port"].as_u64()?;
    Some(format!("http://127.0.0.1:{port}"))
}

/// Resolve the CRM base URL — no manual configuration needed:
/// `SENCLAW_CRM_BASE` override → auto-discovered from the daemon → fallback.
pub async fn resolve_base() -> String {
    if let Ok(v) = std::env::var("SENCLAW_CRM_BASE") {
        if !v.trim().is_empty() {
            return v;
        }
    }
    if let Some(b) = cache().lock().ok().and_then(|c| c.clone()) {
        return b;
    }
    if let Some(b) = discover().await {
        if let Ok(mut c) = cache().lock() {
            *c = Some(b.clone());
        }
        return b;
    }
    CRM_FALLBACK.to_string()
}

/// Search the CRM (FTS over names + channel identifiers + interactions) and
/// return the top matching customer id.
async fn search_customer(base: &str, q: &str) -> Option<i64> {
    let q = q.trim();
    if q.is_empty() {
        return None;
    }
    let url = format!(
        "{}/api/search?q={}&limit=3",
        base.trim_end_matches('/'),
        urlencode(q)
    );
    let v: Value = http()
        .get(&url)
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    v["hits"]
        .as_array()?
        .iter()
        .find_map(|h| h["customer_id"].as_i64())
}

/// Compact profile for a customer id (name/company/phone/role/tags/…).
pub async fn profile_of(base: &str, id: i64) -> Option<Value> {
    let url = format!("{}/api/customers/{}", base.trim_end_matches('/'), id);
    let v: Value = http()
        .get(&url)
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    let c = v.get("customer")?;
    let field = |k: &str| c.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    Some(json!({
        "id": id,
        "name": field("name"),
        "company": field("company"),
        "title": field("title"),
        "role": field("role"),
        "phone": field("phone"),
        "email": field("email"),
        "tags": c.get("tags").cloned().unwrap_or(json!([])),
        "notes": field("notes"),
        "url": format!("{}/#/customers/{}", base.trim_end_matches('/'), id),
    }))
}

/// Placeholder display names that must NOT drive a CRM search (they'd match an
/// unrelated customer via FTS).
const PLACEHOLDER_NAMES: [&str; 3] = ["khách web", "khách", "web"];

/// A real display name (not empty / a generic placeholder).
fn real_name(name: &str) -> bool {
    let n = name.trim().to_lowercase();
    n.chars().count() >= 3 && !PLACEHOLDER_NAMES.contains(&n.as_str())
}

/// A platform id that isn't an app-generated web/probe id.
fn real_id(external_id: &str) -> bool {
    let e = external_id.trim();
    e.chars().count() >= 3 && !e.starts_with("web-") && !e.starts_with("probe-")
}

/// Whether `(external_id, name)` is a meaningful identity worth a CRM lookup.
pub fn has_identity(external_id: &str, name: &str) -> bool {
    real_name(name) || real_id(external_id)
}

/// Recognize a customer from an inbound message. Tries the channel identifier
/// first (an operator may have stored it), then the display name. Returns a
/// compact profile object, or `None` when there's no match / the CRM is down.
pub async fn lookup(base: &str, external_id: &str, name: &str) -> Option<Value> {
    // Only search by identifiers we trust: a real platform id, then a real
    // display name. NEVER fall back to a placeholder name (it FTS-matches an
    // unrelated customer — e.g. "Khách web" → some random "Anna").
    let mut id = None;
    if real_id(external_id) {
        id = search_customer(base, external_id).await;
    }
    if id.is_none() && real_name(name) {
        id = search_customer(base, name).await;
    }
    profile_of(base, id?).await
}

/// List CRM customers for the "new conversation" picker: search when `q` is
/// given, else recent customers. Returns `[{id,name,company,phone}]`. Fail-safe.
pub async fn search_list(base: &str, q: &str) -> Vec<Value> {
    let base = base.trim_end_matches('/');
    let url = if q.trim().is_empty() {
        format!("{base}/api/customers?limit=30")
    } else {
        format!("{base}/api/search?q={}&limit=20", urlencode(q.trim()))
    };
    let Ok(resp) = http()
        .get(&url)
        .timeout(Duration::from_secs(3))
        .send()
        .await
    else {
        return Vec::new();
    };
    let Ok(v) = resp.json::<Value>().await else {
        return Vec::new();
    };
    // Search shape: {hits:[{customer_id,customer_name}]}.
    if let Some(hits) = v.get("hits").and_then(|x| x.as_array()) {
        let mut seen = std::collections::HashSet::new();
        return hits
            .iter()
            .filter_map(|h| {
                let id = h["customer_id"].as_i64()?;
                if !seen.insert(id) {
                    return None;
                }
                Some(json!({ "id": id, "name": h["customer_name"].as_str().unwrap_or("") }))
            })
            .collect();
    }
    // Customer-list shape: {customers:[..]} or a bare array.
    let arr = v
        .get("customers")
        .and_then(|x| x.as_array())
        .or_else(|| v.as_array());
    arr.map(|a| {
        a.iter()
            .map(|c| {
                json!({
                    "id": c["id"].as_i64().unwrap_or(0),
                    "name": c["name"].as_str().unwrap_or(""),
                    "company": c["company"].as_str().unwrap_or(""),
                    "phone": c["phone"].as_str().unwrap_or(""),
                })
            })
            .collect()
    })
    .unwrap_or_default()
}

/// Pick a customer's id for one platform from their stored contact channels.
/// Telegram falls back to a stored phone number.
pub fn value_for_kind(chans: &[(String, String)], kind: &str) -> Option<String> {
    let kind = kind.to_lowercase();
    chans
        .iter()
        .find(|(k, _)| *k == kind)
        .or_else(|| {
            chans
                .iter()
                .find(|(k, _)| kind == "telegram" && k == "phone")
        })
        .map(|(_, v)| v.clone())
}

/// Customer list annotated for one channel: each entry gains `reachable` +
/// `channelValue` so the picker can only offer customers we can actually reach
/// there. Web chat needs no platform id — everyone is reachable.
pub async fn search_list_for_channel(base: &str, q: &str, channel: Option<&str>) -> Vec<Value> {
    let mut list = search_list(base, q).await;
    let kind = match channel.map(str::to_lowercase) {
        Some(k) if !k.is_empty() && k != "websocket" => k,
        _ => {
            for c in list.iter_mut() {
                c["reachable"] = json!(true);
            }
            return list;
        }
    };
    // Resolve each customer's id on that platform (small local calls, parallel).
    let lookups = list.iter().map(|c| {
        let id = c["id"].as_i64().unwrap_or(0);
        let base = base.to_string();
        async move { (id, customer_channels(&base, id).await) }
    });
    let found: std::collections::HashMap<i64, Vec<(String, String)>> =
        futures_util::future::join_all(lookups)
            .await
            .into_iter()
            .collect();
    for c in list.iter_mut() {
        let id = c["id"].as_i64().unwrap_or(0);
        let val = found
            .get(&id)
            .and_then(|chans| value_for_kind(chans, &kind));
        c["reachable"] = json!(val.is_some());
        c["channelValue"] = json!(val);
    }
    list
}

/// A customer's stored contact identifiers, e.g. `[{kind:"zalo", value:"09…"}]`.
/// Used to reach a CRM customer on a real platform (Telegram/Zalo/Facebook).
pub async fn customer_channels(base: &str, id: i64) -> Vec<(String, String)> {
    let url = format!(
        "{}/api/customers/{}/channels",
        base.trim_end_matches('/'),
        id
    );
    let Ok(resp) = http()
        .get(&url)
        .timeout(Duration::from_secs(3))
        .send()
        .await
    else {
        return Vec::new();
    };
    let Ok(v) = resp.json::<Value>().await else {
        return Vec::new();
    };
    v["channels"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|c| {
                    let kind = c["kind"].as_str()?.trim().to_lowercase();
                    let value = c["value"].as_str()?.trim().to_string();
                    (!value.is_empty()).then_some((kind, value))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Render a CRM profile as a system-prompt block so the bot addresses the
/// customer by what's known about them.
pub fn profile_block(p: &Value) -> String {
    let mut lines = vec![format!("- Tên: {}", p["name"].as_str().unwrap_or(""))];
    for (label, key) in [
        ("Công ty", "company"),
        ("Chức danh", "title"),
        ("Vai trò", "role"),
        ("Điện thoại", "phone"),
        ("Email", "email"),
    ] {
        if let Some(v) = p[key].as_str().filter(|s| !s.is_empty()) {
            lines.push(format!("- {label}: {v}"));
        }
    }
    if let Some(tags) = p["tags"].as_array().filter(|a| !a.is_empty()) {
        let tags: Vec<&str> = tags.iter().filter_map(|t| t.as_str()).collect();
        if !tags.is_empty() {
            lines.push(format!("- Nhãn: {}", tags.join(", ")));
        }
    }
    if let Some(notes) = p["notes"].as_str().filter(|s| !s.is_empty()) {
        lines.push(format!(
            "- Ghi chú: {}",
            notes.chars().take(300).collect::<String>()
        ));
    }
    format!("## Hồ sơ khách hàng (CRM)\n{}", lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_guard_rejects_placeholders_and_generated_ids() {
        assert!(
            !has_identity("web-abc123", "Khách web"),
            "anonymous web session"
        );
        assert!(
            !has_identity("probe-999", "Khách"),
            "probe id + placeholder"
        );
        assert!(has_identity("tg-777", "Phạm Quốc Bảo"), "real name");
        assert!(
            has_identity("849012345678", "Khách web"),
            "real platform id"
        );
        assert!(!real_name("web"));
        assert!(!real_id("web-xyz"));
    }

    #[test]
    fn profile_block_includes_known_fields_only() {
        let p = json!({ "name": "Trần B", "company": "ACME", "phone": "", "role": "VIP", "tags": ["vip", "sỉ"], "notes": "khách quen" });
        let b = profile_block(&p);
        assert!(b.contains("Tên: Trần B"));
        assert!(b.contains("Công ty: ACME"));
        assert!(b.contains("Vai trò: VIP"));
        assert!(!b.contains("Điện thoại"), "empty phone must be omitted");
        assert!(b.contains("vip, sỉ"));
    }
}
