//! X (Twitter) — official API v2 path.
//!
//! Real endpoint (once wired): `POST https://api.twitter.com/2/tweets` with an
//! OAuth 2.0 user-context bearer token. Posting requires a paid tier; search and
//! high-volume DM require higher tiers still.

use serde_json::Value;

fn cfg<'a>(c: &'a Value, key: &str) -> &'a str {
    c.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

/// The user-context OAuth 2.0 token (posting needs user context, not app-only).
fn bearer(c: &Value) -> &str {
    let t = cfg(c, "access_token");
    if !t.is_empty() {
        t
    } else {
        cfg(c, "bearer_token")
    }
}

fn configured(c: &Value) -> bool {
    !bearer(c).is_empty()
}

pub async fn official_post(c: &Value, text: &str) -> Result<String, String> {
    if !configured(c) {
        return Err("X: cần access_token (OAuth 2.0 user context, tier trả phí) trong official_config trước khi đăng qua API v2.".into());
    }
    let resp = super::http()
        .post("https://api.twitter.com/2/tweets")
        .bearer_auth(bearer(c))
        .json(&serde_json::json!({ "text": text }))
        .send()
        .await
        .map_err(|e| format!("X API v2 lỗi mạng: {e}"))?;
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    if let Some(id) = body.get("data").and_then(|d| d.get("id")).and_then(|v| v.as_str()) {
        return Ok(id.to_string());
    }
    let err = body
        .get("detail")
        .or_else(|| body.get("title"))
        .and_then(|m| m.as_str())
        .unwrap_or("phản hồi không có data.id");
    Err(format!("X API v2 {status}: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn post_without_token_errors_before_any_network_call() {
        let err = official_post(&json!({}), "hi").await.unwrap_err();
        assert!(err.contains("access_token"), "got: {err}");
    }

    #[test]
    fn access_token_wins_over_bearer_token() {
        assert_eq!(bearer(&json!({"access_token": "A", "bearer_token": "B"})), "A");
        assert_eq!(bearer(&json!({"bearer_token": "B"})), "B");
    }
}
