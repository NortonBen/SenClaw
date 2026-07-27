//! Threads (Meta) — official Threads API path.
//!
//! Real 2-step publish (fully wired): create a media container then publish it.
//!   1. `POST https://graph.threads.net/v1.0/{threads_user_id}/threads?media_type=TEXT&text=...&access_token=...` → creation_id
//!   2. `POST https://graph.threads.net/v1.0/{threads_user_id}/threads_publish?creation_id=...&access_token=...` → id
//! Threads has NO DM. It DOES have an official keyword-search API and replies.
//! Auth shares the Instagram login (a Threads token is minted from the linked IG
//! account). Config: `{ threads_user_id, access_token }`.

use serde_json::Value;

fn cfg<'a>(c: &'a Value, key: &str) -> &'a str {
    c.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

fn configured(c: &Value) -> bool {
    !cfg(c, "threads_user_id").is_empty() && !cfg(c, "access_token").is_empty()
}

pub async fn official_post(c: &Value, text: &str) -> Result<String, String> {
    if !configured(c) {
        return Err("Threads: cần threads_user_id + access_token (token Threads đúc từ tài khoản IG liên kết) trong official_config.".into());
    }
    let uid = cfg(c, "threads_user_id");
    let token = cfg(c, "access_token");
    let client = super::http();

    // Step 1 — create the container.
    let create = client
        .post(format!("https://graph.threads.net/v1.0/{uid}/threads"))
        .query(&[("media_type", "TEXT"), ("text", text), ("access_token", token)])
        .send()
        .await
        .map_err(|e| format!("Threads API lỗi mạng (create): {e}"))?;
    let cbody: Value = create.json().await.unwrap_or(Value::Null);
    let creation_id = cbody
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            let err = cbody
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("create không trả id");
            format!("Threads API create lỗi: {err}")
        })?
        .to_string();

    // Step 2 — publish it.
    let publish = client
        .post(format!("https://graph.threads.net/v1.0/{uid}/threads_publish"))
        .query(&[("creation_id", creation_id.as_str()), ("access_token", token)])
        .send()
        .await
        .map_err(|e| format!("Threads API lỗi mạng (publish): {e}"))?;
    let pbody: Value = publish.json().await.unwrap_or(Value::Null);
    if let Some(id) = pbody.get("id").and_then(|v| v.as_str()) {
        return Ok(id.to_string());
    }
    let err = pbody
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
        .unwrap_or("publish không trả id");
    Err(format!("Threads API publish lỗi: {err}"))
}

/// Threads keyword search — an official API (unlike most platforms here).
/// `GET https://graph.threads.net/v1.0/keyword_search?q=…&search_type=TOP`.
pub async fn official_search(c: &Value, query: &str) -> Result<Value, String> {
    if !configured(c) {
        return Err("Threads: tìm kiếm dùng API chính thức — cần threads_user_id + access_token trong official_config.".into());
    }
    let resp = super::http()
        .get("https://graph.threads.net/v1.0/keyword_search")
        .query(&[
            ("q", query),
            ("search_type", "TOP"),
            ("fields", "id,text,username,permalink,timestamp"),
            ("access_token", cfg(c, "access_token")),
        ])
        .send()
        .await
        .map_err(|e| format!("Threads search lỗi mạng: {e}"))?;
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    if let Some(err) = body.get("error").and_then(|e| e.get("message")).and_then(|m| m.as_str()) {
        return Err(format!("Threads search lỗi: {err}"));
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn post_without_config_errors_before_any_network_call() {
        let err = official_post(&json!({}), "hi").await.unwrap_err();
        assert!(err.contains("threads_user_id"), "got: {err}");
    }

    #[tokio::test]
    async fn search_without_config_errors_before_any_network_call() {
        let err = official_search(&json!({}), "áo").await.unwrap_err();
        assert!(err.contains("API chính thức"), "got: {err}");
    }
}
