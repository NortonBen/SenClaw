//! Text-only completions through the SenClaw daemon bridge.
//!
//! The video analysis itself cannot use this — the bridge carries no video and
//! no temperature (see `gemini.rs`). But translation is plain text, so it goes
//! through the daemon like every other Space App, using whatever model the user
//! has configured there rather than burning their Gemini quota.

use serde_json::{json, Value};
use std::time::Duration;

fn describe(e: &reqwest::Error) -> String {
    let mut out = e.to_string();
    let mut src: Option<&dyn std::error::Error> = std::error::Error::source(e);
    while let Some(s) = src {
        out.push_str(&format!(": {s}"));
        src = s.source();
    }
    out
}

/// One completion. Returns the model's text.
pub async fn bridge_llm(system: &str, user: &str, max_tokens: u32) -> Result<String, String> {
    let url = format!(
        "{}/api/space/apps/{}/bridge",
        crate::config::senclaw_base_url().trim_end_matches('/'),
        crate::config::app_id()
    );
    let body = json!({
        "action": "llm.request",
        "payload": { "system": system, "prompt": user, "maxTokens": max_tokens },
    });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(180))
        .build()
        .map_err(|e| format!("tạo HTTP client thất bại: {}", describe(&e)))?;

    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("gọi bridge thất bại ({url}): {}", describe(&e)))?;

    let v: Value = resp
        .json()
        .await
        .map_err(|e| format!("bridge trả về không phải JSON: {}", describe(&e)))?;

    match v.get("status").and_then(|x| x.as_str()) {
        Some("ok") => {
            let text = v
                .get("text")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            // A completion cut off at the token cap is a truncated translation,
            // which would silently ship half a prompt downstream.
            if v.get("finish").and_then(|x| x.as_str()) == Some("length") {
                return Err("model cắt output giữa chừng (đạt trần token)".into());
            }
            Ok(text)
        }
        Some("pending") => Err("bridge LLM chưa được bật trong daemon này".into()),
        _ => Err(v
            .get("message")
            .and_then(|x| x.as_str())
            .unwrap_or("lỗi LLM không rõ")
            .to_string()),
    }
}
