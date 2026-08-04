//! Bridge client tới daemon SenClaw: `llm.request` (one-shot, không tool) và
//! `agent.run` (agent đầy đủ tool, one-shot — không session nối tiếp).
//!
//! Sự thật đã kiểm chứng (2026-08):
//! - agent.run bị chặn cứng 4 run đồng thời/app ("reached max concurrency",
//!   lỗi ngay không xếp hàng) → caller giữ Semaphore(3) + retry ở đây.
//! - payload `tools` hiện CHƯA được daemon enforce (soft) — vẫn truyền để tự
//!   cứng khi daemon vá; `model` hiện bị bỏ qua (chạy model active toàn cục).
//! - `finish == "length"` của llm.request = bị cắt trần token, JSON có thể đứt.

use serde_json::{json, Value};
use std::sync::OnceLock;
use std::time::Duration;

fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(125))
            .build()
            .expect("build reqwest client")
    })
}

fn bridge_url() -> String {
    format!(
        "{}/api/space/apps/{}/bridge",
        crate::config::senclaw_base_url(),
        crate::config::app_id()
    )
}

async fn bridge_post(body: &Value, timeout: Duration) -> Result<Value, String> {
    let resp = client()
        .post(bridge_url())
        .json(body)
        .timeout(timeout)
        .send()
        .await
        .map_err(|e| format!("bridge unreachable: {e}"))?;
    resp.json::<Value>()
        .await
        .map_err(|e| format!("bridge invalid json: {e}"))
}

/// One-shot completion trên model active. Trả (text, model, finish).
pub async fn llm_request(system: &str, prompt: &str, max_tokens: u32) -> Result<(String, String, String), String> {
    llm_request_on(system, prompt, max_tokens, None).await
}

/// Như trên nhưng ghim vào một **LLM profile** (id hoặc label trong Settings →
/// Models) — cách per-member model hoạt động THẬT hôm nay cho member không
/// dùng tool (VD 1 member Gemini, 1 member Claude tranh luận chéo).
/// Profile không tồn tại → daemon trả lỗi rõ, không âm thầm fallback.
pub async fn llm_request_on(
    system: &str,
    prompt: &str,
    max_tokens: u32,
    profile: Option<&str>,
) -> Result<(String, String, String), String> {
    let mut payload = json!({ "system": system, "prompt": prompt, "maxTokens": max_tokens });
    if let Some(p) = profile.map(str::trim).filter(|p| !p.is_empty()) {
        payload["profile"] = json!(p);
    }
    let body = json!({ "action": "llm.request", "payload": payload });
    let mut last_err = String::new();
    for attempt in 1..=3u64 {
        match bridge_post(&body, Duration::from_secs(125)).await {
            Ok(v) => match v.get("status").and_then(|s| s.as_str()) {
                Some("ok") => {
                    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
                    return Ok((s("text"), s("model"), s("finish")));
                }
                Some("pending") => return Err("daemon chưa bật LLM bridge".into()),
                _ => {
                    last_err = v
                        .get("message")
                        .and_then(|x| x.as_str())
                        .unwrap_or("unknown LLM error")
                        .to_string();
                }
            },
            Err(e) => last_err = e,
        }
        tokio::time::sleep(Duration::from_millis(700 * attempt)).await;
    }
    Err(last_err)
}

/// Agent đầy đủ tool, one-shot. `space` = ngăn bộ nhớ daemon riêng của member;
/// `workspace` = kho tài liệu phiên (member đọc bằng Read/Grep); `tools` =
/// allowlist (None = toàn bộ tool MCP hệ thống — mặc định của app này).
/// Trả (text, (tokens_in, tokens_out)). Retry khi đụng trần concurrency.
pub async fn agent_run(
    system: &str,
    prompt: &str,
    space: &str,
    workspace: &str,
    tools: Option<&[String]>,
    model: Option<&str>,
    timeout_secs: u64,
) -> Result<(String, (i64, i64)), String> {
    let mut payload = json!({
        "system": system,
        "prompt": prompt,
        "space": space,
        "workspace": workspace,
        "timeoutSeconds": timeout_secs,
    });
    if let Some(t) = tools {
        if !t.is_empty() {
            payload["tools"] = json!(t);
        }
    }
    if let Some(m) = model.map(str::trim).filter(|m| !m.is_empty()) {
        payload["model"] = json!(m);
    }
    let body = json!({ "action": "agent.run", "payload": payload });
    let http_timeout = Duration::from_secs(timeout_secs + 30);

    let mut last_err = String::new();
    for attempt in 1..=3u64 {
        match bridge_post(&body, http_timeout).await {
            Ok(v) => match v.get("status").and_then(|s| s.as_str()) {
                Some("ok") => {
                    let text = v.get("text").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    let usage = v.get("usage").cloned().unwrap_or_default();
                    let n = |k: &str| usage.get(k).and_then(|x| x.as_i64()).unwrap_or(0);
                    return Ok((text, (n("inputTokens"), n("outputTokens"))));
                }
                _ => {
                    last_err = v
                        .get("message")
                        .and_then(|x| x.as_str())
                        .unwrap_or("unknown agent error")
                        .to_string();
                    // Trần 4 run/app của daemon: lỗi ngay, không xếp hàng — chờ rồi thử lại.
                    if last_err.contains("max concurrency") {
                        tokio::time::sleep(Duration::from_secs(12 * attempt)).await;
                        continue;
                    }
                }
            },
            Err(e) => last_err = e,
        }
        tokio::time::sleep(Duration::from_millis(900 * attempt)).await;
    }
    Err(last_err)
}

/// Thông tin model active (cho /api/status) — best effort.
pub async fn llm_info() -> Value {
    let url = format!("{}/api/llm-config", crate::config::senclaw_base_url());
    match client().get(&url).timeout(Duration::from_secs(6)).send().await {
        Ok(r) => match r.json::<Value>().await {
            Ok(v) => {
                let active = v.get("activeId").cloned().unwrap_or(Value::Null);
                let name = v
                    .get("configs")
                    .and_then(|c| c.as_array())
                    .and_then(|arr| {
                        arr.iter().find(|c| c.get("id") == active.as_str().map(Value::from).as_ref())
                    })
                    .and_then(|c| c.get("modelName"))
                    .cloned()
                    .unwrap_or(Value::Null);
                json!({ "ok": true, "activeId": active, "model": name })
            }
            Err(_) => json!({ "ok": false }),
        },
        Err(_) => json!({ "ok": false }),
    }
}

/// Danh sách LLM profile của daemon (Settings → Models) cho picker per-member.
pub async fn llm_profiles() -> Vec<Value> {
    let url = format!("{}/api/llm-config", crate::config::senclaw_base_url());
    let Ok(resp) = client().get(&url).timeout(Duration::from_secs(6)).send().await else {
        return Vec::new();
    };
    let Ok(v) = resp.json::<Value>().await else {
        return Vec::new();
    };
    let active = v.get("activeId").and_then(|x| x.as_str()).unwrap_or("");
    v.get("configs")
        .and_then(|a| a.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|c| {
                    let id = c.get("id")?.as_str()?.to_string();
                    let label = c
                        .get("label")
                        .or_else(|| c.get("name"))
                        .and_then(|x| x.as_str())
                        .unwrap_or("");
                    let model_name = c.get("modelName").and_then(|x| x.as_str()).unwrap_or("");
                    let provider = c
                        .get("provider")
                        .or_else(|| c.get("adapt"))
                        .and_then(|x| x.as_str())
                        .unwrap_or("");
                    Some(json!({
                        "id": id,
                        "name": if !label.is_empty() { label } else { model_name },
                        "model": model_name,
                        "provider": provider,
                        "active": c.get("id").and_then(|x| x.as_str()) == Some(active),
                    }))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Danh sách tool MCP khả dụng từ daemon, ghép sẵn full identifier
/// `mcp__<server>__<tool>` cho UI picker + cẩm nang member.
pub async fn mcp_tool_catalog() -> Vec<Value> {
    let url = format!("{}/api/mcp-servers", crate::config::senclaw_base_url());
    let Ok(resp) = client().get(&url).timeout(Duration::from_secs(8)).send().await else {
        return Vec::new();
    };
    let Ok(v) = resp.json::<Value>().await else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for server in v.get("servers").and_then(|s| s.as_array()).unwrap_or(&Vec::new()) {
        let sname = server.get("name").and_then(|x| x.as_str()).unwrap_or("");
        if sname.is_empty() {
            continue;
        }
        let status = server.get("status").and_then(|x| x.as_str()).unwrap_or("");
        let builtin = server.get("builtin").and_then(|x| x.as_bool()).unwrap_or(false);
        for tool in server.get("tools").and_then(|t| t.as_array()).unwrap_or(&Vec::new()) {
            let tname = tool.get("name").and_then(|x| x.as_str()).unwrap_or("");
            if tname.is_empty() {
                continue;
            }
            out.push(json!({
                "server": sname,
                "tool": tname,
                "full": format!("mcp__{sname}__{tname}"),
                "description": tool.get("description").and_then(|x| x.as_str()).unwrap_or(""),
                "builtin": builtin,
                "status": status,
            }));
        }
    }
    out
}
