//! Facebook — official Graph API path (Page publishing).
//!
//! Real endpoint (once wired): `POST https://graph.facebook.com/v21.0/{page_id}/feed`
//! with `{ message, access_token }` (Page access token). Personal-profile and
//! group posting have no sanctioned API — those go through the extension.

use serde_json::Value;

fn cfg<'a>(c: &'a Value, key: &str) -> &'a str {
    c.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

fn configured(c: &Value) -> bool {
    !cfg(c, "page_id").is_empty() && !cfg(c, "access_token").is_empty()
}

pub async fn official_post(c: &Value, text: &str) -> Result<String, String> {
    if !configured(c) {
        return Err("Facebook: cần page_id + access_token (Page access token) trong official_config trước khi đăng qua Graph API.".into());
    }
    let page_id = cfg(c, "page_id");
    let token = cfg(c, "access_token");
    // Current stable line (v20–v25 are live as of 2026; v18/v19 expired). The
    // `/{page}/feed` + `message` contract is unchanged across these.
    let version = graph_version(c);
    let url = format!("https://graph.facebook.com/{version}/{page_id}/feed");
    let resp = super::http()
        .post(&url)
        .form(&[("message", text), ("access_token", token)])
        .send()
        .await
        .map_err(|e| format!("Facebook Graph API lỗi mạng: {e}"))?;
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    if let Some(id) = body.get("id").and_then(|v| v.as_str()) {
        return Ok(id.to_string());
    }
    let err = body
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
        .unwrap_or("phản hồi không có id");
    Err(format!("Facebook Graph API {status}: {err}"))
}

fn graph_version(c: &Value) -> String {
    let v = cfg(c, "graph_version");
    if v.is_empty() {
        "v23.0".to_string()
    } else {
        v.to_string()
    }
}

/// Read a managed Page's metadata via the official Graph API — the reliable,
/// ToS-clean "scan" path. `fields` overrides the default field set.
/// `GET /{page_id}?fields=…&access_token=…`.
pub async fn page_info(c: &Value, fields: &str) -> Result<Value, String> {
    if !configured(c) {
        return Err("Facebook scan: cần page_id + access_token (Page token) trong official_config. Chỉ đọc được Page mà Sếp quản trị.".into());
    }
    let fields = if fields.trim().is_empty() {
        "name,about,category,fan_count,followers_count,link,verification_status,website,phone,emails,single_line_address,checkins,were_here_count"
    } else {
        fields
    };
    let url = format!(
        "https://graph.facebook.com/{}/{}",
        graph_version(c),
        cfg(c, "page_id")
    );
    let resp = super::http()
        .get(&url)
        .query(&[("fields", fields), ("access_token", cfg(c, "access_token"))])
        .send()
        .await
        .map_err(|e| format!("Facebook Graph API lỗi mạng: {e}"))?;
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    if let Some(err) = body
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
    {
        return Err(format!("Facebook page_info lỗi: {err}"));
    }
    Ok(body)
}

/// Read a managed Page's recent posts. `GET /{page_id}/feed?fields=…&limit=…`.
pub async fn page_feed(c: &Value, limit: i64) -> Result<Value, String> {
    if !configured(c) {
        return Err("Facebook scan: cần page_id + access_token (Page token) trong official_config. Chỉ đọc được Page mà Sếp quản trị.".into());
    }
    let limit = limit.clamp(1, 100).to_string();
    let url = format!(
        "https://graph.facebook.com/{}/{}/feed",
        graph_version(c),
        cfg(c, "page_id")
    );
    let resp = super::http()
        .get(&url)
        .query(&[
            ("fields", "id,message,story,created_time,permalink_url,status_type,shares,reactions.summary(true).limit(0),comments.summary(true).limit(0)"),
            ("limit", &limit),
            ("access_token", cfg(c, "access_token")),
        ])
        .send()
        .await
        .map_err(|e| format!("Facebook Graph API lỗi mạng: {e}"))?;
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    if let Some(err) = body
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
    {
        return Err(format!("Facebook page_feed lỗi: {err}"));
    }
    Ok(body)
}

/// Read a managed Page's insights. `GET /{page_id}/insights?metric=…`.
/// NOTE: Meta deprecated `impressions`-family metrics for `views` from
/// 2025-11-15; pass `metric` explicitly to track the current names.
pub async fn page_insights(c: &Value, metric: &str, period: &str) -> Result<Value, String> {
    if !configured(c) {
        return Err("Facebook scan: cần page_id + access_token (Page token) trong official_config với quyền read_insights.".into());
    }
    let metric = if metric.trim().is_empty() {
        "page_post_engagements,page_impressions_unique,page_daily_follows_unique"
    } else {
        metric
    };
    let period = if period.trim().is_empty() {
        "day"
    } else {
        period
    };
    let url = format!(
        "https://graph.facebook.com/{}/{}/insights",
        graph_version(c),
        cfg(c, "page_id")
    );
    let resp = super::http()
        .get(&url)
        .query(&[
            ("metric", metric),
            ("period", period),
            ("access_token", cfg(c, "access_token")),
        ])
        .send()
        .await
        .map_err(|e| format!("Facebook Graph API lỗi mạng: {e}"))?;
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    if let Some(err) = body
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
    {
        return Err(format!("Facebook page_insights lỗi: {err} (kiểm tra tên metric — impressions đã đổi sang views từ 15/11/2025)"));
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
        assert!(err.contains("page_id"), "got: {err}");
    }

    #[tokio::test]
    async fn page_reads_require_config_before_any_network_call() {
        assert!(page_info(&json!({}), "")
            .await
            .unwrap_err()
            .contains("page_id"));
        assert!(page_feed(&json!({}), 10)
            .await
            .unwrap_err()
            .contains("page_id"));
        assert!(page_insights(&json!({}), "", "")
            .await
            .unwrap_err()
            .contains("page_id"));
    }

    #[tokio::test]
    async fn page_info_uses_configured_graph_version() {
        // graph_version override flows through (defaults to a current version).
        assert_eq!(graph_version(&json!({ "graph_version": "v25.0" })), "v25.0");
        assert_eq!(graph_version(&json!({})), "v23.0");
    }
}
