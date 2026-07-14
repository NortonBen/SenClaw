use app_space_sdk::SpaceClient;

fn client() -> SpaceClient {
    if std::env::var("SENCLAW_SPACE_APP_ID").is_err() {
        std::env::set_var("SENCLAW_SPACE_APP_ID", "ai-office");
    }
    SpaceClient::from_env()
}

/// One completion through the daemon bridge. Returns `(text, model)`.
pub async fn bridge_llm(system: &str, user: &str, max_tokens: u32) -> Result<(String, String), String> {
    client()
        .llm_request(system, user, max_tokens)
        .await
        .map_err(|e| e.to_string())
}

/// Info for the Cài đặt panel: whether a live LLM is reachable and which model is active.
pub async fn llm_info() -> serde_json::Value {
    let base = std::env::var("SENCLAW_BASE_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:18788".to_string());
    let url = format!("{}/api/llm-config", base.trim_end_matches('/'));
    match reqwest::Client::new()
        .get(&url)
        .timeout(std::time::Duration::from_secs(4))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            let v: serde_json::Value = resp.json().await.unwrap_or_default();
            serde_json::json!({ "available": true, "config": v })
        }
        _ => serde_json::json!({ "available": false }),
    }
}
