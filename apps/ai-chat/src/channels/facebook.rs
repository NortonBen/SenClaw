//! Facebook Messenger adapter (Graph API v21.0, polling inbound + Send API
//! outbound). No webhooks. Config: `{ "page_id", "access_token" }` (a
//! long-lived Page access token). `cursor` stores the newest message time (ms).

use crate::channels::Inbound;
use crate::db::{Channel, Db};
use crate::llm::http;
use serde_json::{json, Value};
use std::sync::Arc;

const GRAPH: &str = "https://graph.facebook.com/v21.0";

fn cfg<'a>(ch: &'a Channel, key: &str) -> &'a str {
    ch.config.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

/// Parse a Graph RFC3339 timestamp into epoch millis (0 on failure).
pub fn rfc3339_ms(s: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.timestamp_millis())
        .unwrap_or(0)
}

/// Normalize one conversation's `messages` into inbound CUSTOMER messages newer
/// than `since_ms`. A message is from the customer when `from.id != page_id`.
/// Returns `(messages, newest_ms)`. Pure — unit-tested below.
pub fn normalize_messages(page_id: &str, list: &[Value], since_ms: i64) -> (Vec<Inbound>, i64) {
    let mut out = Vec::new();
    let mut newest = since_ms;
    for m in list {
        let t = rfc3339_ms(m.get("created_time").and_then(|x| x.as_str()).unwrap_or(""));
        newest = newest.max(t);
        let from_id = m["from"]["id"].as_str().unwrap_or("");
        let is_customer = !from_id.is_empty() && from_id != page_id;
        let text = m
            .get("message")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if is_customer && t > since_ms && !text.is_empty() {
            out.push(Inbound {
                external_id: from_id.to_string(),
                customer_name: m["from"]["name"].as_str().unwrap_or("").to_string(),
                text,
            });
        }
    }
    (out, newest)
}

pub async fn poll(db: &Arc<Db>, ch: &Channel) -> Result<Vec<Inbound>, String> {
    let page_id = cfg(ch, "page_id");
    let token = cfg(ch, "access_token");
    if page_id.is_empty() || token.is_empty() {
        return Err("kênh Facebook thiếu page_id/access_token".into());
    }
    let since: i64 = ch
        .cursor
        .parse()
        .unwrap_or_else(|_| crate::db::now_ms() - 7 * 24 * 3600 * 1000);

    let convs: Value = http()
        .get(format!("{GRAPH}/{page_id}/conversations"))
        .query(&[
            ("fields", "id,updated_time"),
            ("limit", "25"),
            ("access_token", token),
        ])
        .send()
        .await
        .map_err(|e| format!("facebook conversations lỗi: {e}"))?
        .json()
        .await
        .map_err(|e| format!("facebook phản hồi lỗi: {e}"))?;
    if let Some(err) = convs.get("error") {
        return Err(format!(
            "facebook lỗi: {}",
            err["message"].as_str().unwrap_or("")
        ));
    }

    let mut out = Vec::new();
    let mut newest = since;
    for conv in convs["data"]
        .as_array()
        .unwrap_or(&Vec::new())
        .iter()
        .take(25)
    {
        let Some(conv_id) = conv["id"].as_str() else {
            continue;
        };
        let msgs: Value = http()
            .get(format!("{GRAPH}/{conv_id}/messages"))
            .query(&[
                ("fields", "id,message,from,created_time"),
                ("limit", "25"),
                ("access_token", token),
            ])
            .send()
            .await
            .map_err(|e| format!("facebook messages lỗi: {e}"))?
            .json()
            .await
            .map_err(|e| format!("facebook phản hồi lỗi: {e}"))?;
        let (m, max_ms) = normalize_messages(
            page_id,
            msgs["data"].as_array().unwrap_or(&Vec::new()),
            since,
        );
        newest = newest.max(max_ms);
        out.extend(m);
    }
    if newest > since {
        let _ = db.set_channel_sync(ch.id, "ok", "", Some(&newest.to_string()));
    }
    Ok(out)
}

pub async fn send(
    _db: &Arc<Db>,
    ch: &Channel,
    external_id: &str,
    text: &str,
) -> Result<(), String> {
    let page_id = cfg(ch, "page_id");
    let token = cfg(ch, "access_token");
    if page_id.is_empty() || token.is_empty() {
        return Err("kênh Facebook thiếu page_id/access_token".into());
    }
    let resp = http()
        .post(format!("{GRAPH}/{page_id}/messages"))
        .query(&[("access_token", token)])
        .json(&json!({
            "recipient": { "id": external_id },
            "messaging_type": "RESPONSE",
            "message": { "text": text },
        }))
        .send()
        .await
        .map_err(|e| format!("facebook gửi lỗi: {e}"))?;
    let v: Value = resp
        .json()
        .await
        .map_err(|e| format!("facebook phản hồi lỗi: {e}"))?;
    if let Some(err) = v.get("error") {
        return Err(format!(
            "facebook từ chối gửi: {}",
            err["message"].as_str().unwrap_or("")
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rfc3339() {
        assert!(
            rfc3339_ms("2026-07-16T09:30:00+0000") > 0
                || rfc3339_ms("2026-07-16T09:30:00+00:00") > 0
        );
        assert_eq!(rfc3339_ms("not-a-date"), 0);
    }

    #[test]
    fn customer_vs_page_direction() {
        let list = vec![
            json!({ "message": "hỏi", "from": { "id": "PSID1", "name": "Lan" }, "created_time": "2026-07-16T10:00:00+00:00" }),
            json!({ "message": "page trả lời", "from": { "id": "PAGE", "name": "Shop" }, "created_time": "2026-07-16T10:01:00+00:00" }),
        ];
        let (msgs, _newest) = normalize_messages("PAGE", &list, 0);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].external_id, "PSID1");
        assert_eq!(msgs[0].text, "hỏi");
    }
}
