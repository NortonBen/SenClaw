use serde_json::{json, Value};
use std::sync::OnceLock;
use std::time::Duration;

pub fn base_url() -> String {
    std::env::var("SENCLAW_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:18788".to_string())
}

pub fn app_id() -> String {
    std::env::var("SENCLAW_SPACE_APP_ID").unwrap_or_else(|_| "ai-office".to_string())
}

/// One shared connection pool for every bridge call (a fresh Client per call
/// churns pools/FDs and produced intermittent connect errors).
pub fn http() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(125))
            .build()
            .expect("build http client")
    })
}

/// Walk the error chain so "error sending request" also says why.
fn describe(e: &reqwest::Error) -> String {
    let mut out = e.to_string();
    let mut src = std::error::Error::source(e);
    while let Some(s) = src {
        out.push_str(&format!(": {}", s));
        src = s.source();
    }
    out
}

/// One completion through the daemon bridge. Returns `(text, model, finish)`
/// where `finish` is `"length"` when the provider cut the output at the
/// token cap (older daemons don't report it → empty string).
/// Transport errors are retried (the daemon occasionally drops a connection
/// while its LLM is busy); application errors are returned as-is.
pub async fn bridge_llm(
    system: &str,
    user: &str,
    max_tokens: u32,
) -> Result<(String, String, String, Option<(i64, i64)>), String> {
    let url = format!(
        "{}/api/space/apps/{}/bridge",
        base_url().trim_end_matches('/'),
        app_id()
    );
    let body = json!({
        "action": "llm.request",
        "payload": { "system": system, "prompt": user, "maxTokens": max_tokens },
    });

    let mut last_err = String::new();
    for attempt in 0..3u32 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(700 * attempt as u64)).await;
        }
        let resp = match http().post(&url).json(&body).send().await {
            Ok(r) => r,
            Err(e) => {
                last_err = format!("bridge llm.request failed ({url}): {}", describe(&e));
                continue;
            }
        };
        let v: Value = match resp.json().await {
            Ok(v) => v,
            Err(e) => {
                last_err = format!("invalid bridge response: {}", describe(&e));
                continue;
            }
        };
        return match v.get("status").and_then(|x| x.as_str()) {
            Some("ok") => {
                // Real provider usage (daemons ≥ token-accounting; None on
                // older daemons or usage-less providers → caller falls back
                // to the chars/4 estimate).
                let usage = v.get("usage").filter(|u| u.is_object()).map(|u| {
                    let n = |k: &str| u.get(k).and_then(|x| x.as_i64()).unwrap_or(0);
                    (n("inputTokens"), n("outputTokens"))
                });
                Ok((
                    v.get("text")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string(),
                    v.get("model")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string(),
                    v.get("finish")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string(),
                    usage,
                ))
            }
            Some("pending") => Err("bridge LLM chưa được bật trong daemon này".to_string()),
            _ => Err(v
                .get("message")
                .and_then(|x| x.as_str())
                .unwrap_or("unknown LLM error")
                .to_string()),
        };
    }
    Err(last_err)
}

/// Info for the Cài đặt panel: whether a live LLM is reachable and which model is active.
pub async fn llm_info() -> Value {
    let url = format!("{}/api/llm-config", base_url().trim_end_matches('/'));
    match http()
        .get(&url)
        .timeout(Duration::from_secs(4))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            let v: Value = resp.json().await.unwrap_or_default();
            json!({ "available": true, "config": v })
        }
        _ => json!({ "available": false }),
    }
}
