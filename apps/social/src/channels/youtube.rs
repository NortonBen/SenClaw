//! YouTube — official Data API v3 path.
//!
//! Real flow (once wired): resumable upload via
//! `POST https://www.googleapis.com/upload/youtube/v3/videos` with an OAuth 2.0
//! token (scope `youtube.upload`). Comments/search are also available but spend
//! daily quota (default 10k units; an upload costs ~1600).
//!
//! Note: this app folds in what was scoped as a standalone `apps/youtube` — see
//! `docs/youtube-app-research.md`. That plan is superseded by `apps/social`.

use serde_json::Value;

fn cfg<'a>(c: &'a Value, key: &str) -> &'a str {
    c.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

fn configured(c: &Value) -> bool {
    !cfg(c, "access_token").is_empty() || !cfg(c, "refresh_token").is_empty()
}

pub fn official_post(c: &Value, _text: &str) -> Result<String, String> {
    if !configured(c) {
        return Err("YouTube: cần access_token/refresh_token (OAuth 2.0, scope youtube.upload) trong official_config trước khi upload qua Data API v3.".into());
    }
    Err("YouTube: upload qua Data API v3 chưa bật (scaffold) — nối resumable upload rồi mở khoá; nhớ quota ~1600 đơn vị/upload.".into())
}

/// YouTube search — an official API path (`search.list`, ~100 quota units).
/// Accepts either an OAuth `access_token` or a plain `api_key`.
pub async fn official_search(c: &Value, query: &str) -> Result<Value, String> {
    let api_key = cfg(c, "api_key");
    let token = cfg(c, "access_token");
    if api_key.is_empty() && token.is_empty() {
        return Err("YouTube: tìm kiếm dùng API chính thức (Data API v3 search.list) — cần api_key hoặc access_token trong official_config.".into());
    }
    let mut req = super::http()
        .get("https://www.googleapis.com/youtube/v3/search")
        .query(&[
            ("part", "snippet"),
            ("type", "video"),
            ("maxResults", "25"),
            ("q", query),
        ]);
    if !api_key.is_empty() {
        req = req.query(&[("key", api_key)]);
    } else {
        req = req.bearer_auth(token);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| format!("YouTube search lỗi mạng: {e}"))?;
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    if let Some(err) = body
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
    {
        return Err(format!("YouTube search lỗi: {err}"));
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn search_without_credentials_errors_before_any_network_call() {
        let err = official_search(&json!({}), "abc").await.unwrap_err();
        assert!(err.contains("api_key"), "got: {err}");
    }
}
