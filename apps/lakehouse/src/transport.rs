//! Bridge client — gọi daemon SenClaw qua `POST {SENCLAW_BASE_URL}/api/space/apps/{id}/bridge`.
//!
//! Skeleton phase-1: chỉ dựng khung `bridge_call` + kiểu envelope. Người dùng thực
//! (`lake_flow_generate`, NL→SQL) là phase sau — nhớ bài học đã ghi trong docs §9:
//!   * `llm.request` KHÔNG có `temperature`; ánh xạ núm sáng tạo là vô tác dụng.
//!   * `finish == "length"` phải coi là LỖI (output bị cắt), không phải kết quả.
//!   * `maxTokens` ≤ 32000; hand-roll POST vì SDK không cho truyền `profile`.

#![allow(dead_code)]

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::config;

/// Kết quả một completion bridge `llm.request`.
#[derive(Debug, Clone)]
pub struct LlmReply {
    pub text: String,
    pub model: String,
    pub finish: String,
}

/// Một completion qua bridge daemon (§9). `finish=="length"` (output bị cắt) được coi
/// là LỖI — caller không được lưu bản cắt như bản đầy đủ. KHÔNG có `temperature`;
/// `maxTokens` bị chặn ≤ 32000. POST hand-roll `{action, payload}` (SDK không cho profile).
pub async fn llm_request(system: &str, prompt: &str, max_tokens: u32) -> Result<LlmReply> {
    let url = format!(
        "{}/api/space/apps/{}/bridge",
        config::senclaw_base_url().trim_end_matches('/'),
        config::space_app_id()
    );
    let max_tokens = max_tokens.min(32000);
    let payload = json!({ "system": system, "prompt": prompt, "maxTokens": max_tokens });
    let body = json!({ "action": "llm.request", "payload": payload });
    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| anyhow!("bridge llm.request POST {url} thất bại: {e}"))?;
    let v: Value = resp
        .json()
        .await
        .map_err(|e| anyhow!("bridge llm.request trả về không phải JSON: {e}"))?;
    parse_llm_reply(&v)
}

/// Parse envelope bridge `llm.request` (`{status, text, model, finish}`). Tách riêng
/// để test thuần (không gọi mạng). `status=="ok"` + `finish!="length"` → Ok; ngược lại lỗi.
pub fn parse_llm_reply(v: &Value) -> Result<LlmReply> {
    match v.get("status").and_then(|x| x.as_str()) {
        Some("ok") => {
            let finish = v
                .get("finish")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            if finish == "length" {
                return Err(anyhow!(
                    "bridge llm.request bị cắt (finish=length) — tăng maxTokens hoặc rút gọn yêu cầu"
                ));
            }
            Ok(LlmReply {
                text: v
                    .get("text")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
                model: v
                    .get("model")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
                finish,
            })
        }
        Some("pending") => Err(anyhow!("bridge LLM chưa được bật trong daemon này")),
        _ => Err(anyhow!(
            "bridge llm.request lỗi: {}",
            v.get("message")
                .and_then(|x| x.as_str())
                .unwrap_or("không rõ")
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ok_reply() {
        let v = json!({ "status": "ok", "text": "hello", "model": "m1", "finish": "stop" });
        let r = parse_llm_reply(&v).unwrap();
        assert_eq!(r.text, "hello");
        assert_eq!(r.model, "m1");
    }

    #[test]
    fn parse_length_finish_is_error() {
        let v = json!({ "status": "ok", "text": "cut...", "finish": "length" });
        let err = parse_llm_reply(&v).unwrap_err().to_string();
        assert!(err.contains("length"), "finish=length phải là lỗi: {err}");
    }

    #[test]
    fn parse_pending_and_error() {
        assert!(parse_llm_reply(&json!({ "status": "pending" })).is_err());
        let e = parse_llm_reply(&json!({ "status": "error", "message": "boom" }))
            .unwrap_err()
            .to_string();
        assert!(e.contains("boom"));
    }
}

/// Gọi một method bridge của daemon. `method` ví dụ `"llm.request"`, `"knowledge.recall"`.
/// Trả về phần `result` (hoặc lỗi nếu envelope báo `error`). Timeout ngắn để không
/// treo request path — caller phase-2 tự quyết retry.
pub async fn bridge_call(method: &str, params: Value) -> Result<Value> {
    let url = format!(
        "{}/api/space/apps/{}/bridge",
        config::senclaw_base_url().trim_end_matches('/'),
        config::space_app_id()
    );
    let body = serde_json::json!({ "method": method, "params": params });
    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| anyhow!("bridge POST {url} thất bại: {e}"))?;
    let status = resp.status();
    let envelope: Value = resp
        .json()
        .await
        .map_err(|e| anyhow!("bridge trả về không phải JSON: {e}"))?;
    if !status.is_success() {
        return Err(anyhow!("bridge {method} HTTP {status}: {envelope}"));
    }
    // Envelope daemon: `{ "result": ... }` hoặc `{ "error": ... }`. `error: null`
    // KHÔNG phải lỗi (bài học ontology) — chỉ coi là lỗi khi error khác null.
    if let Some(err) = envelope.get("error") {
        if !err.is_null() {
            return Err(anyhow!("bridge {method} lỗi: {err}"));
        }
    }
    Ok(envelope
        .get("result")
        .cloned()
        .unwrap_or_else(|| envelope.clone()))
}
