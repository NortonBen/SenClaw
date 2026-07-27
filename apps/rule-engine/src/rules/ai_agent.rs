//! `ai-agent` — hỏi một mô hình ngôn ngữ rồi gắn câu trả lời vào payload.
//!
//! Ba backend dùng chung một khung: dựng prompt (có nội suy `${...}`), gọi ra
//! ngoài, rồi ghi kết quả vào một field. Chỉ chỗ "gọi ra ngoài" là khác nhau.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::daq;
use crate::engine::spec::{Category, Rule, RuleSpec, RunCtx};
use crate::engine::types::{Message, Outcome};

const BACKENDS: [&str; 3] = ["senclaw", "persona", "provider"];
const PROVIDERS: [&str; 5] = ["chatgpt", "deepseek", "ollama", "gemini", "dify"];

pub struct AiAgentRule {
    spec: RuleSpec,
}

pub fn rule() -> Arc<dyn Rule> {
    Arc::new(AiAgentRule::new())
}

impl AiAgentRule {
    fn new() -> Self {
        let spec = RuleSpec::builder("ai-agent", "AI Agent", Category::Ai)
            .desc("Gọi LLM (SenClaw, persona, hoặc provider ngoài) và gắn câu trả lời vào dữ liệu.")
            .icon("🧠")
            .color("#722ed1")
            .schema(json!({
                "type": "object",
                "properties": {
                    "backend": {
                        "type": "string",
                        "title": "Nguồn mô hình",
                        "ui": "select",
                        "enum": ["senclaw", "persona", "provider"],
                        "default": "senclaw",
                        "description": "senclaw = LLM của daemon; persona = chạy agent đầy đủ (có tool); provider = gọi thẳng API bên ngoài."
                    },
                    "systemPrompt": {
                        "type": "string",
                        "title": "System prompt",
                        "ui": "textarea",
                        "description": "Áp dụng cho cả 3 backend. Có nội suy ${field}."
                    },
                    "userPrompt": {
                        "type": "string",
                        "title": "Prompt người dùng",
                        "ui": "textarea",
                        "placeholder": "Nhiệt độ đang là ${temperature} độ, có bất thường không?",
                        "description": "Có nội suy ${field} / ${a.b[0]}. Bỏ trống = gửi nguyên JSON của payload."
                    },
                    "maxTokens": {
                        "type": "integer",
                        "title": "Số token tối đa",
                        "default": 2000,
                        "description": "Chỉ dùng cho backend `senclaw`."
                    },
                    "profile": {
                        "type": "string",
                        "title": "Profile LLM",
                        "description": "Chỉ dùng cho backend `senclaw`. Bỏ trống = profile mặc định của daemon."
                    },
                    "persona": {
                        "type": "string",
                        "title": "Persona",
                        "description": "Chỉ dùng cho backend `persona`. Tên persona sẽ được ghép vào system prompt."
                    },
                    "tools": {
                        "type": "array",
                        "items": { "type": "string" },
                        "title": "Danh sách tool cho phép",
                        "placeholder": "mcp__senclaw-memory__memory_search",
                        "description": "Chỉ dùng cho backend `persona`. Bỏ trống = không giới hạn (không truyền allowlist)."
                    },
                    "model": {
                        "type": "string",
                        "title": "Model",
                        "description": "Dùng cho backend `persona` và `provider`."
                    },
                    "timeoutSeconds": {
                        "type": "integer",
                        "title": "Thời gian chờ (giây)",
                        "default": 300,
                        "description": "Chỉ dùng cho backend `persona`. Daemon ép về khoảng 10..1800."
                    },
                    "provider": {
                        "type": "string",
                        "title": "Nhà cung cấp",
                        "ui": "select",
                        "enum": ["chatgpt", "deepseek", "ollama", "gemini", "dify"],
                        "default": "chatgpt",
                        "description": "Chỉ dùng cho backend `provider`."
                    },
                    "apiKey": {
                        "type": "string",
                        "title": "API key",
                        "ui": "password",
                        "description": "Bắt buộc với mọi provider trừ `ollama`."
                    },
                    "host": {
                        "type": "string",
                        "title": "Host Ollama",
                        "default": "http://localhost:11434",
                        "description": "Chỉ dùng cho provider `ollama`."
                    },
                    "baseUrl": {
                        "type": "string",
                        "title": "Base URL Dify",
                        "default": "https://api.dify.ai/v1",
                        "description": "Chỉ dùng cho provider `dify`."
                    },
                    "temperature": {
                        "type": "number",
                        "title": "Độ sáng tạo",
                        "default": 0.7,
                        "description": "Chỉ có tác dụng với backend `provider`, và chỉ với provider chatgpt / deepseek / ollama / gemini. Dify KHÔNG nhận temperature (API chat-messages không có tham số này). Backend senclaw/persona cũng không dùng — bridge của SenClaw không nhận temperature."
                    },
                    "apiBase": {
                        "type": "string",
                        "title": "Base URL thay thế (nội bộ)",
                        "description": "Ghi đè endpoint của provider. Chỉ dùng để kiểm thử hoặc trỏ qua proxy nội bộ."
                    },
                    "outputField": {
                        "type": "string",
                        "title": "Ghi kết quả vào field",
                        "default": "response",
                        "description": "Đường dẫn trong payload, ví dụ `ai.answer`."
                    },
                    "parseJson": {
                        "type": "boolean",
                        "title": "Phân tích kết quả thành JSON",
                        "default": false,
                        "description": "Bật khi prompt yêu cầu model trả JSON. Bóc được cả khối ```json ... ```. Parse hỏng = ra cổng error."
                    }
                }
            }))
            .doc(
                "Gọi LLM rồi gắn câu trả lời vào payload.\n\n\
                 **Backend `senclaw`** — dùng LLM của daemon qua bridge `llm.request`. \
                 Bridge KHÔNG nhận `temperature` (daemon ép 0.2), nên node này cố tình \
                 không cho chỉnh độ sáng tạo ở backend đó; núm `temperature` chỉ hiện \
                 hiệu lực ở backend `provider` (với chatgpt / deepseek / ollama / gemini; \
                 Dify không nhận temperature). Trả lời bị cắt vì chạm trần token \
                 (`finish=length`) được coi là lỗi và đi ra cổng `error`.\n\n\
                 **Backend `persona`** — chạy một lượt agent đầy đủ (`agent.run`), có thể \
                 kèm allowlist tool `mcp__...`. Chậm hơn nhưng dùng được công cụ.\n\n\
                 **Backend `provider`** — gọi thẳng ChatGPT / DeepSeek / Ollama / Gemini / \
                 Dify bằng API key của bạn.\n\n\
                 Khác bản Go:\n\
                 - Bản Go ghi đè sạch payload thành `{response: ...}`; ở đây kết quả \
                   được **cộng thêm** vào bản sao payload tại `outputField`, dữ liệu cũ giữ nguyên.\n\
                 - Bản Go không bao giờ set node kế tiếp nên chuỗi luôn dừng tại node AI; \
                   ở đây có cổng `out` thật, nối tiếp được.\n\
                 - Bản Go type-assert thẳng vào phản hồi nên panic khi API trả cấu trúc lạ; \
                   ở đây mọi phản hồi thiếu field đều thành lỗi có thông báo rõ ràng.",
            )
            .build();
        Self { spec }
    }
}

/// `(system, user)` sau khi nội suy. Tách ra để test không cần daemon.
pub fn render_prompts(ctx: &RunCtx, msg: &Message) -> (String, String) {
    let system = ctx
        .cfg_str("systemPrompt")
        .map(|t| daq::interpolate(&t, &msg.data, &msg.meta))
        .unwrap_or_default();
    let user = match ctx.cfg_str("userPrompt") {
        Some(t) => daq::interpolate(&t, &msg.data, &msg.meta),
        // No template: the payload itself is the question.
        None => serde_json::to_string(&msg.data).unwrap_or_else(|_| "{}".to_string()),
    };
    (system, user)
}

/// Lấy phần JSON trong một khối ```...``` nếu có, ngược lại trả nguyên chuỗi.
pub fn extract_json_block(s: &str) -> &str {
    let t = s.trim();
    if let Some(start) = t.find("```") {
        let after = &t[start + 3..];
        // The first line after the fence may be a language tag (`json`).
        let body = match after.find('\n') {
            Some(i) => &after[i + 1..],
            None => after,
        };
        if let Some(end) = body.find("```") {
            return body[..end].trim();
        }
    }
    t
}

fn str_list(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|x| x.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        Some(Value::String(s)) => s
            .split(',')
            .map(|x| x.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        _ => vec![],
    }
}

fn base_url(ctx: &RunCtx, provider: &str) -> String {
    if let Some(b) = ctx.cfg_str("apiBase") {
        return b.trim_end_matches('/').to_string();
    }
    let raw = match provider {
        "chatgpt" => "https://api.openai.com/v1".to_string(),
        "deepseek" => "https://api.deepseek.com/v1".to_string(),
        "ollama" => ctx.cfg_str_or("host", "http://localhost:11434"),
        "gemini" => "https://generativelanguage.googleapis.com/v1beta".to_string(),
        "dify" => ctx.cfg_str_or("baseUrl", "https://api.dify.ai/v1"),
        _ => String::new(),
    };
    raw.trim_end_matches('/').to_string()
}

/// Gọi API ngoài. Mọi bóc tách đều qua `get(...)` — không type-assert như bản Go.
async fn call_provider(ctx: &RunCtx, system: &str, user: &str) -> Result<String, String> {
    let provider = ctx.cfg_str_or("provider", "chatgpt");
    if !PROVIDERS.contains(&provider.as_str()) {
        return Err(format!(
            "Nhà cung cấp không hợp lệ: `{provider}`. Chọn một trong {}.",
            PROVIDERS.join(" | ")
        ));
    }
    let api_key = ctx.cfg_str("apiKey");
    if api_key.is_none() && provider != "ollama" {
        return Err(format!("Provider `{provider}` cần API key."));
    }
    let model = ctx.cfg_str("model");
    let temperature = ctx.cfg_f64_or("temperature", 0.7);
    let base = base_url(ctx, &provider);
    // Gemini and Dify have no system turn: fold it into the single user text.
    let merged = if system.trim().is_empty() {
        user.to_string()
    } else {
        format!("{system}\n\n{user}")
    };

    let (url, body, bearer) = match provider.as_str() {
        "chatgpt" | "deepseek" => {
            let model = model.unwrap_or_else(|| {
                if provider == "chatgpt" {
                    "gpt-4o-mini".to_string()
                } else {
                    "deepseek-chat".to_string()
                }
            });
            (
                format!("{base}/chat/completions"),
                json!({
                    "model": model,
                    "messages": [
                        { "role": "system", "content": system },
                        { "role": "user", "content": user }
                    ],
                    "temperature": temperature
                }),
                api_key.clone(),
            )
        }
        "ollama" => (
            format!("{base}/api/chat"),
            json!({
                "model": model.unwrap_or_else(|| "llama3".to_string()),
                "messages": [
                    { "role": "system", "content": system },
                    { "role": "user", "content": user }
                ],
                "stream": false,
                // Ollama takes generation params under `options`, not at the top.
                "options": { "temperature": temperature }
            }),
            None,
        ),
        "gemini" => {
            let model = model.unwrap_or_else(|| "gemini-1.5-flash".to_string());
            let key = api_key.clone().unwrap_or_default();
            (
                format!("{base}/models/{model}:generateContent?key={key}"),
                json!({
                    "contents": [{ "parts": [{ "text": merged }] }],
                    // Gemini takes temperature under `generationConfig`.
                    "generationConfig": { "temperature": temperature }
                }),
                None,
            )
        }
        // dify
        _ => (
            format!("{base}/chat-messages"),
            json!({
                "inputs": {},
                "query": merged,
                "response_mode": "blocking",
                "user": "senclaw-rule-engine"
            }),
            api_key.clone(),
        ),
    };

    let mut req = ctx.svc.http.post(&url).json(&body);
    if let Some(key) = bearer {
        req = req.header("Authorization", format!("Bearer {key}"));
    }
    let resp = req
        .send()
        .await
        .map_err(|e| format!("Gọi {provider} lỗi: {e}"))?;
    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("Đọc phản hồi {provider} lỗi: {e}"))?;
    if !status.is_success() {
        return Err(format!("{provider} trả HTTP {status}: {text}"));
    }
    let v: Value = serde_json::from_str(&text)
        .map_err(|e| format!("Phản hồi {provider} không phải JSON: {e}"))?;

    let answer = match provider.as_str() {
        "chatgpt" | "deepseek" => v
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|a| a.first())
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str()),
        "ollama" => v
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str()),
        "gemini" => v
            .get("candidates")
            .and_then(|c| c.as_array())
            .and_then(|a| a.first())
            .and_then(|c| c.get("content"))
            .and_then(|c| c.get("parts"))
            .and_then(|p| p.as_array())
            .and_then(|a| a.first())
            .and_then(|p| p.get("text"))
            .and_then(|t| t.as_str()),
        _ => v.get("answer").and_then(|a| a.as_str()),
    };

    answer.map(|s| s.to_string()).ok_or_else(|| {
        format!("Phản hồi {provider} không có nội dung trả lời như mong đợi: {text}")
    })
}

#[async_trait]
impl Rule for AiAgentRule {
    fn spec(&self) -> &RuleSpec {
        &self.spec
    }

    fn validate(&self, config: &Value) -> Vec<String> {
        let mut out = Vec::new();
        let backend = config
            .get("backend")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("senclaw");
        if !BACKENDS.contains(&backend) {
            out.push(format!(
                "Backend không hợp lệ: `{backend}`. Chọn một trong {}.",
                BACKENDS.join(" | ")
            ));
            return out;
        }
        if backend == "provider" {
            let provider = config
                .get("provider")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or("chatgpt");
            if !PROVIDERS.contains(&provider) {
                out.push(format!(
                    "Nhà cung cấp không hợp lệ: `{provider}`. Chọn một trong {}.",
                    PROVIDERS.join(" | ")
                ));
            }
            let has_key = config
                .get("apiKey")
                .and_then(|v| v.as_str())
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false);
            if provider != "ollama" && !has_key {
                out.push(format!("Provider `{provider}` cần API key."));
            }
        }
        if backend == "persona" {
            let empty = |k: &str| {
                config
                    .get(k)
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim().is_empty())
                    .unwrap_or(true)
            };
            if empty("persona") && empty("systemPrompt") {
                out.push(
                    "Backend `persona` chưa có persona lẫn system prompt — agent sẽ chạy \
                     không có vai trò nào. Nên điền ít nhất một trong hai."
                        .to_string(),
                );
            }
        }
        out
    }

    async fn handle(&self, ctx: &RunCtx, msg: Message) -> Outcome {
        let backend = ctx.cfg_str_or("backend", "senclaw");
        let (system, user) = render_prompts(ctx, &msg);

        let result: Result<String, String> = match backend.as_str() {
            "senclaw" => {
                let max_tokens = ctx.cfg_u64_or("maxTokens", 2000) as u32;
                let profile = ctx.cfg_str("profile");
                ctx.svc
                    .bridge
                    .llm_request(&system, &user, max_tokens, profile.as_deref())
                    .await
                    .map(|r| r.text)
            }
            "persona" => {
                let persona = ctx.cfg_str("persona");
                let system = match (&persona, system.trim().is_empty()) {
                    (Some(p), true) => format!("Bạn đóng vai persona `{p}`."),
                    (Some(p), false) => format!("Bạn đóng vai persona `{p}`.\n\n{system}"),
                    (None, _) => system.clone(),
                };
                let tools = str_list(ctx.cfg("tools"));
                let model = ctx.cfg_str("model");
                ctx.svc
                    .bridge
                    .agent_run(
                        &user,
                        if system.trim().is_empty() {
                            None
                        } else {
                            Some(system.as_str())
                        },
                        if tools.is_empty() { None } else { Some(tools) },
                        model.as_deref(),
                        ctx.cfg_u64_or("timeoutSeconds", 300),
                    )
                    .await
            }
            "provider" => call_provider(ctx, &system, &user).await,
            other => {
                return ctx.fail_config(format!(
                    "Backend không hợp lệ: `{other}`. Chọn một trong {}.",
                    BACKENDS.join(" | ")
                ))
            }
        };

        let text = match result {
            Ok(t) => t,
            Err(e) => return ctx.fail_runtime(e),
        };
        if text.trim().is_empty() {
            return ctx.fail_runtime("Mô hình trả về nội dung rỗng.");
        }

        let value = if ctx.cfg_bool("parseJson", false) {
            let block = extract_json_block(&text);
            match serde_json::from_str::<Value>(block) {
                Ok(v) => v,
                Err(e) => {
                    return ctx.fail_runtime(format!(
                        "Không phân tích được kết quả thành JSON: {e}. Nội dung: {block}"
                    ))
                }
            }
        } else {
            Value::String(text)
        };

        // Unlike the Go rule, which replaced the payload with `{response: ...}`.
        let mut data = msg.data;
        daq::set(&mut data, &ctx.cfg_str_or("outputField", "response"), value);
        Outcome::out(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{ctx, failure, msg, msg_with_meta, one};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A one-shot HTTP server that answers any request with `body`.
    async fn fake_api(body: &'static str) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            // Drain the request first: closing early would surface as a
            // connection error on the client instead of our canned response.
            let mut buf = Vec::new();
            let mut chunk = [0u8; 2048];
            loop {
                let Ok(n) = sock.read(&mut chunk).await else {
                    break;
                };
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
                // Byte-level scan: the body may be UTF-8, so offsets must not be
                // taken from a lowercased copy of the whole buffer.
                let end = buf.windows(4).position(|w| w == b"\r\n\r\n");
                if let Some(idx) = end {
                    let head = String::from_utf8_lossy(&buf[..idx]).to_lowercase();
                    let len: usize = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .and_then(|v| v.trim().parse().ok())
                        .unwrap_or(0);
                    if buf.len() >= idx + 4 + len {
                        break;
                    }
                }
            }
            let resp = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = sock.write_all(resp.as_bytes()).await;
            let _ = sock.flush().await;
        });
        format!("http://127.0.0.1:{port}")
    }

    #[test]
    fn user_prompt_is_interpolated_from_payload_and_meta() {
        let c = ctx(
            "ai-agent",
            json!({
                "systemPrompt": "Bạn là chuyên gia kho ${site}.",
                "userPrompt": "Nhiệt độ ${temperature} tại ${meta_missing}."
            }),
        );
        let m = msg_with_meta(json!({ "temperature": 31.5 }), json!({ "site": "kho A" }));
        let (system, user) = render_prompts(&c, &m);
        assert_eq!(system, "Bạn là chuyên gia kho kho A.");
        assert_eq!(user, "Nhiệt độ 31.5 tại .");
    }

    #[test]
    fn an_empty_user_prompt_falls_back_to_the_payload_json() {
        let c = ctx("ai-agent", json!({}));
        let (_, user) = render_prompts(&c, &msg(json!({ "a": 1 })));
        assert_eq!(user, "{\"a\":1}");
    }

    #[test]
    fn json_fences_are_stripped_before_parsing() {
        assert_eq!(extract_json_block("```json\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(
            extract_json_block("Đây nhé:\n```\n[1,2]\n```\nxong"),
            "[1,2]"
        );
        assert_eq!(extract_json_block("  {\"a\":1}  "), "{\"a\":1}");
    }

    #[tokio::test]
    async fn provider_backend_calls_the_openai_endpoint_and_keeps_the_payload() {
        let base = fake_api(r#"{"choices":[{"message":{"content":"ổn định"}}]}"#).await;
        let c = ctx(
            "ai-agent",
            json!({
                "backend": "provider",
                "provider": "chatgpt",
                "apiKey": "sk-test",
                "model": "gpt-4o-mini",
                "apiBase": base
            }),
        );
        let (port, data) = one(c_handle(&c, msg(json!({ "temperature": 31 }))).await);
        assert_eq!(port, "out");
        assert_eq!(data["response"], "ổn định");
        // The Go rule threw the rest of the payload away here.
        assert_eq!(data["temperature"], 31);
    }

    #[tokio::test]
    async fn provider_backend_parses_json_out_of_a_fenced_block() {
        let base = fake_api(
            r#"{"choices":[{"message":{"content":"```json\n{\"level\":\"cao\"}\n```"}}]}"#,
        )
        .await;
        let c = ctx(
            "ai-agent",
            json!({
                "backend": "provider",
                "apiKey": "sk-test",
                "apiBase": base,
                "parseJson": true,
                "outputField": "ai.verdict"
            }),
        );
        let (_, data) = one(c_handle(&c, msg(json!({}))).await);
        assert_eq!(data["ai"]["verdict"]["level"], "cao");
    }

    /// The Go implementation type-asserted straight into the response map and
    /// panicked on anything unexpected.
    #[tokio::test]
    async fn a_response_missing_the_content_field_fails_instead_of_panicking() {
        let base = fake_api(r#"{"error":{"message":"quota"}}"#).await;
        let c = ctx(
            "ai-agent",
            json!({ "backend": "provider", "apiKey": "sk-test", "apiBase": base }),
        );
        let err = failure(c_handle(&c, msg(json!({}))).await);
        assert!(err.contains("không có nội dung trả lời"), "{err}");
    }

    #[tokio::test]
    async fn parse_json_failure_goes_to_the_error_port() {
        let base = fake_api(r#"{"choices":[{"message":{"content":"không phải json"}}]}"#).await;
        let c = ctx(
            "ai-agent",
            json!({
                "backend": "provider",
                "apiKey": "sk-test",
                "apiBase": base,
                "parseJson": true
            }),
        );
        let err = failure(c_handle(&c, msg(json!({}))).await);
        assert!(err.contains("Không phân tích được"), "{err}");
    }

    #[tokio::test]
    async fn an_unknown_backend_fails_with_a_readable_message() {
        let c = ctx("ai-agent", json!({ "backend": "openai" }));
        let err = failure(c_handle(&c, msg(json!({}))).await);
        assert!(err.contains("Backend không hợp lệ"), "{err}");
    }

    #[test]
    fn validate_catches_bad_backend_and_missing_credentials() {
        let r = AiAgentRule::new();
        assert!(!r.validate(&json!({ "backend": "vertex" })).is_empty());
        assert!(
            r.validate(&json!({})).is_empty(),
            "senclaw là mặc định hợp lệ"
        );
        let missing_key = r.validate(&json!({ "backend": "provider", "provider": "chatgpt" }));
        assert!(
            missing_key.iter().any(|m| m.contains("API key")),
            "{missing_key:?}"
        );
        assert!(r
            .validate(&json!({ "backend": "provider", "provider": "ollama" }))
            .is_empty());
        let persona = r.validate(&json!({ "backend": "persona" }));
        assert!(persona.iter().any(|m| m.contains("persona")), "{persona:?}");
        assert!(r
            .validate(&json!({ "backend": "persona", "persona": "analyst" }))
            .is_empty());
    }

    #[test]
    fn the_node_has_both_an_out_and_an_error_port() {
        let r = AiAgentRule::new();
        assert!(r.spec().has_output("out"));
        assert!(r.spec().has_output("error"));
    }

    async fn c_handle(c: &RunCtx, m: Message) -> Outcome {
        AiAgentRule::new().handle(c, m).await
    }
}
