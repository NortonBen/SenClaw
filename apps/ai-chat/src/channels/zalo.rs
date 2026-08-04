//! Zalo Official Account adapter (polling inbound + v3.0 outbound). No
//! webhooks. Config:
//! `{ "app_id","app_secret","access_token","refresh_token","oa_id" }`.
//!
//! Auth: every API call is a GET/POST with the token in an `access_token`
//! HEADER and params packed into a single `data` JSON query field. Access
//! tokens are short-lived (~25h) and refresh tokens ROTATE — on API error
//! `-216` we refresh once, persist the new (access + refresh) pair back to the
//! channel config, and retry. `cursor` stores the newest message time (ms)
//! already delivered, for dedup.

use crate::channels::Inbound;
use crate::db::{Channel, Db};
use crate::llm::http;
use serde_json::{json, Value};
use std::sync::Arc;

const OA_V2: &str = "https://openapi.zalo.me/v2.0/oa";
const OA_V3: &str = "https://openapi.zalo.me/v3.0/oa";
const OAUTH: &str = "https://oauth.zaloapp.com/v4/oa/access_token";

fn cfg<'a>(ch: &'a Channel, key: &str) -> &'a str {
    ch.config.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

/// Zalo wraps its payload as `{"data":[...]}` or `{"data":{"data":[...]}}`.
pub fn extract_data_array(v: &Value) -> Vec<Value> {
    match &v["data"] {
        Value::Array(a) => a.clone(),
        Value::Object(o) => o
            .get("data")
            .and_then(|x| x.as_array())
            .cloned()
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// The API error code, if the response carries one (`0` == success).
fn error_code(v: &Value) -> i64 {
    v.get("error").and_then(|x| x.as_i64()).unwrap_or(0)
}

/// Normalize the `/conversation` message list into inbound CUSTOMER messages
/// newer than `since_ms`. `src == 1` means the customer sent it (0 == the OA).
/// Returns `(messages, newest_ms)`. Pure — unit-tested below.
pub fn normalize_messages(
    user_id: &str,
    name: &str,
    list: &[Value],
    since_ms: i64,
) -> (Vec<Inbound>, i64) {
    let mut out = Vec::new();
    let mut newest = since_ms;
    for m in list {
        let t = m.get("time").and_then(|x| x.as_i64()).unwrap_or(0);
        newest = newest.max(t);
        let is_customer = m.get("src").and_then(|x| x.as_i64()).unwrap_or(0) == 1;
        let text = m
            .get("message")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if is_customer && t > since_ms && !text.is_empty() {
            out.push(Inbound {
                external_id: user_id.to_string(),
                customer_name: name.to_string(),
                text,
            });
        }
    }
    (out, newest)
}

/// GET a Zalo OA endpoint with the `data` param, refreshing the token once on `-216`.
async fn zalo_get(
    db: &Arc<Db>,
    ch: &Channel,
    base: &str,
    path: &str,
    data: &Value,
) -> Result<Value, String> {
    let mut token = cfg(ch, "access_token").to_string();
    for attempt in 0..2 {
        let resp = http()
            .get(format!("{base}/{path}"))
            .header("access_token", &token)
            .query(&[("data", data.to_string())])
            .send()
            .await
            .map_err(|e| format!("zalo {path} lỗi: {e}"))?;
        let v: Value = resp
            .json()
            .await
            .map_err(|e| format!("zalo phản hồi lỗi: {e}"))?;
        if error_code(&v) == -216 && attempt == 0 {
            token = refresh_token(db, ch).await?;
            continue;
        }
        return Ok(v);
    }
    Err("zalo: token hết hạn và làm mới thất bại".into())
}

/// Poll recent chats and collect new customer messages.
pub async fn poll(db: &Arc<Db>, ch: &Channel) -> Result<Vec<Inbound>, String> {
    if cfg(ch, "access_token").is_empty() {
        return Err("kênh Zalo thiếu access_token".into());
    }
    let since: i64 = ch
        .cursor
        .parse()
        .unwrap_or_else(|_| crate::db::now_ms() - 7 * 24 * 3600 * 1000);
    let recent = zalo_get(
        db,
        ch,
        OA_V2,
        "listrecentchat",
        &json!({ "offset": 0, "count": 10 }),
    )
    .await?;
    let chats = extract_data_array(&recent);

    let mut out = Vec::new();
    let mut newest = since;
    for chat in chats.iter().take(10) {
        // The customer is the party that isn't the OA.
        let oa_id = cfg(ch, "oa_id");
        let from_id = chat["from_id"].as_str().unwrap_or("");
        let to_id = chat["to_id"].as_str().unwrap_or("");
        let user_id = if from_id == oa_id { to_id } else { from_id };
        if user_id.is_empty() {
            continue;
        }
        let name = chat["from_display_name"]
            .as_str()
            .filter(|_| from_id != oa_id)
            .or_else(|| chat["to_display_name"].as_str())
            .unwrap_or("")
            .to_string();
        let conv = zalo_get(
            db,
            ch,
            OA_V2,
            "conversation",
            &json!({ "user_id": user_id, "offset": 0, "count": 10 }),
        )
        .await?;
        let (msgs, max_ms) = normalize_messages(user_id, &name, &extract_data_array(&conv), since);
        newest = newest.max(max_ms);
        out.extend(msgs);
    }
    if newest > since {
        let _ = db.set_channel_sync(ch.id, "ok", "", Some(&newest.to_string()));
    }
    Ok(out)
}

/// Send a text reply via the v3.0 customer-service message endpoint.
pub async fn send(db: &Arc<Db>, ch: &Channel, external_id: &str, text: &str) -> Result<(), String> {
    let body = json!({ "recipient": { "user_id": external_id }, "message": { "text": text } });
    let mut token = cfg(ch, "access_token").to_string();
    if token.is_empty() {
        return Err("kênh Zalo thiếu access_token".into());
    }
    for attempt in 0..2 {
        let resp = http()
            .post(format!("{OA_V3}/message/cs"))
            .header("access_token", &token)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("zalo gửi lỗi: {e}"))?;
        let v: Value = resp
            .json()
            .await
            .map_err(|e| format!("zalo phản hồi lỗi: {e}"))?;
        match error_code(&v) {
            0 => return Ok(()),
            -216 if attempt == 0 => {
                token = refresh_token(db, ch).await?;
                continue;
            }
            code => {
                return Err(format!(
                    "zalo từ chối gửi (mã {code}): {}",
                    v.get("message").and_then(|x| x.as_str()).unwrap_or("")
                ))
            }
        }
    }
    Err("zalo: token hết hạn và làm mới thất bại".into())
}

/// Refresh the (rotating) token pair and persist it back to the channel config.
/// Returns the new access token.
async fn refresh_token(db: &Arc<Db>, ch: &Channel) -> Result<String, String> {
    let app_id = cfg(ch, "app_id");
    let app_secret = cfg(ch, "app_secret");
    let refresh = cfg(ch, "refresh_token");
    if app_id.is_empty() || app_secret.is_empty() || refresh.is_empty() {
        return Err("zalo: thiếu app_id/app_secret/refresh_token để làm mới token".into());
    }
    let resp = http()
        .post(OAUTH)
        .header("secret_key", app_secret)
        .form(&[
            ("refresh_token", refresh),
            ("app_id", app_id),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await
        .map_err(|e| format!("zalo refresh lỗi: {e}"))?;
    let v: Value = resp
        .json()
        .await
        .map_err(|e| format!("zalo refresh phản hồi lỗi: {e}"))?;
    let new_access = v["access_token"].as_str().unwrap_or("").to_string();
    if new_access.is_empty() {
        return Err(format!(
            "zalo refresh thất bại: {}",
            v.get("error_description")
                .and_then(|x| x.as_str())
                .unwrap_or("không có access_token")
        ));
    }
    // Persist the rotated pair (old refresh token is now dead).
    let mut new_cfg = ch.config.clone();
    new_cfg["access_token"] = json!(new_access);
    if let Some(nr) = v["refresh_token"].as_str() {
        new_cfg["refresh_token"] = json!(nr);
    }
    let _ = db.update_channel(ch.id, None, Some(&new_cfg), None);
    Ok(new_access)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_both_data_shapes() {
        let flat = json!({ "data": [{ "a": 1 }] });
        assert_eq!(extract_data_array(&flat).len(), 1);
        let nested = json!({ "data": { "data": [{ "a": 1 }, { "b": 2 }] } });
        assert_eq!(extract_data_array(&nested).len(), 2);
        assert!(extract_data_array(&json!({ "data": 5 })).is_empty());
    }

    #[test]
    fn normalizes_only_new_customer_messages() {
        let list = vec![
            json!({ "src": 1, "message": "cũ", "time": 100 }), // old
            json!({ "src": 1, "message": "mới", "time": 300 }), // new customer
            json!({ "src": 0, "message": "OA trả lời", "time": 400 }), // OA, skip
            json!({ "src": 1, "message": "   ", "time": 500 }), // empty, skip
        ];
        let (msgs, newest) = normalize_messages("u1", "An", &list, 200);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].text, "mới");
        assert_eq!(msgs[0].external_id, "u1");
        assert_eq!(newest, 500);
    }
}
