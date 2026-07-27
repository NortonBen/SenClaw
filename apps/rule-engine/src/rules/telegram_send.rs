//! `telegram-send` — push a message to a Telegram chat.
//!
//! Two deliberate differences from the Go rule: a Telegram error is a real
//! failure (Go logged it and carried on as if the message had been sent), and
//! the node publishes an `out` message so the chain can continue (Go published
//! nothing, so every branch ending in Telegram simply stopped).

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::daq;
use crate::engine::spec::{Category, Rule, RuleSpec, RunCtx};
use crate::engine::types::{Message, Outcome};

const DEFAULT_API_BASE: &str = "https://api.telegram.org";
const PARSE_MODES: [&str; 3] = ["HTML", "Markdown", "None"];

pub struct TelegramSendRule {
    spec: RuleSpec,
}

pub fn rule() -> Arc<dyn Rule> {
    Arc::new(TelegramSendRule::new())
}

/// Telegram accepts either a numeric id or an `@channel` handle.
fn chat_id_value(raw: &str) -> Value {
    match raw.parse::<i64>() {
        Ok(n) => json!(n),
        Err(_) => json!(raw),
    }
}

impl TelegramSendRule {
    fn new() -> Self {
        let spec = RuleSpec::builder("telegram-send", "Gửi Telegram", Category::Sink)
            .desc("Gửi tin nhắn tới một chat Telegram qua Bot API.")
            .icon("✈️")
            .color("#1890ff")
            .schema(json!({
                "type": "object",
                "required": ["botToken", "chatId", "message"],
                "properties": {
                    "botToken": {
                        "type": "string",
                        "title": "Bot token",
                        "ui": "password",
                        "placeholder": "123456:ABC-DEF..."
                    },
                    "chatId": {
                        "type": "string",
                        "title": "Chat ID",
                        "placeholder": "-1001234567890 hoặc @kenh_cua_ban"
                    },
                    "message": {
                        "type": "string",
                        "title": "Nội dung",
                        "ui": "textarea",
                        "placeholder": "Cảnh báo ${name}: ${temperature} độ",
                        "description": "Chèn dữ liệu bằng ${field} hoặc ${a.b.c}."
                    },
                    "parseMode": {
                        "type": "string",
                        "title": "Định dạng",
                        "ui": "select",
                        "enum": PARSE_MODES,
                        "default": "HTML"
                    },
                    "silent": {
                        "type": "boolean",
                        "title": "Gửi im lặng",
                        "default": false,
                        "description": "Máy nhận không kêu thông báo."
                    },
                    "apiBase": {
                        "type": "string",
                        "title": "API base (chỉ để kiểm thử)",
                        "default": DEFAULT_API_BASE,
                        "description": "Chỉ đổi khi cần trỏ vào máy chủ giả trong kiểm thử."
                    }
                }
            }))
            .doc(
                "POST `{apiBase}/bot{token}/sendMessage`.\n\n\
                 - Lỗi HTTP hoặc `ok:false` từ Telegram đều là lỗi thật, đi ra cổng `error` \
                   kèm mô tả của Telegram — bản Go chỉ ghi log rồi coi như đã gửi.\n\
                 - Gửi xong vẫn phát `{ sent, messageId, chatId }` ra cổng `out` để nối tiếp \
                   node khác.\n\
                 - `apiBase` chỉ dùng cho kiểm thử; để mặc định khi chạy thật.",
            )
            .build();
        Self { spec }
    }
}

#[async_trait]
impl Rule for TelegramSendRule {
    fn spec(&self) -> &RuleSpec {
        &self.spec
    }

    fn validate(&self, config: &Value) -> Vec<String> {
        let mut out = Vec::new();
        for (key, label) in [
            ("botToken", "Bot token"),
            ("chatId", "Chat ID"),
            ("message", "Nội dung tin nhắn"),
        ] {
            match config.get(key).and_then(|v| v.as_str()) {
                Some(s) if !s.trim().is_empty() => {}
                _ => out.push(format!("Thiếu {label}.")),
            }
        }
        if let Some(m) = config.get("parseMode").and_then(|v| v.as_str()) {
            if !m.trim().is_empty() && !PARSE_MODES.contains(&m) {
                out.push(format!(
                    "Định dạng `{m}` không hợp lệ. Chọn: {}.",
                    PARSE_MODES.join(", ")
                ));
            }
        }
        out
    }

    async fn handle(&self, ctx: &RunCtx, msg: Message) -> Outcome {
        let Some(token) = ctx.cfg_str("botToken") else {
            return ctx.fail_config("Thiếu Bot token.");
        };
        let Some(chat_tpl) = ctx.cfg_str("chatId") else {
            return ctx.fail_config("Thiếu Chat ID.");
        };
        let Some(text_tpl) = ctx.cfg_str("message") else {
            return ctx.fail_config("Thiếu nội dung tin nhắn.");
        };

        let chat = daq::interpolate(&chat_tpl, &msg.data, &msg.meta);
        let text = daq::interpolate(&text_tpl, &msg.data, &msg.meta);
        if text.trim().is_empty() {
            return ctx.fail_runtime("Nội dung sau khi thay ${...} rỗng, Telegram sẽ từ chối.");
        }

        let mut payload = json!({ "chat_id": chat_id_value(&chat), "text": text });
        let mode = ctx.cfg_str_or("parseMode", "HTML");
        if mode != "None" {
            payload["parse_mode"] = json!(mode);
        }
        if ctx.cfg_bool("silent", false) {
            payload["disable_notification"] = json!(true);
        }

        let base = ctx
            .cfg_str_or("apiBase", DEFAULT_API_BASE)
            .trim_end_matches('/')
            .to_string();
        let url = format!("{base}/bot{token}/sendMessage");

        let resp = match ctx
            .svc
            .http
            .post(&url)
            .json(&payload)
            .timeout(Duration::from_secs(20))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return ctx.fail_runtime(format!("Không gọi được Telegram API: {e}")),
        };
        let status = resp.status();
        let body: Value = match resp.text().await {
            Ok(t) => serde_json::from_str(&t).unwrap_or_else(|_| json!({ "raw": t })),
            Err(e) => return ctx.fail_runtime(format!("Không đọc được phản hồi Telegram: {e}")),
        };
        if !status.is_success() {
            return ctx.fail_runtime(format!(
                "Telegram trả về HTTP {}: {}",
                status.as_u16(),
                body.get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or(&body.to_string())
            ));
        }
        if !body.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
            return ctx.fail_runtime(format!(
                "Telegram từ chối: {}",
                body.get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or(&body.to_string())
            ));
        }

        let message_id = body
            .get("result")
            .and_then(|r| r.get("message_id"))
            .cloned()
            .unwrap_or(Value::Null);
        Outcome::out(json!({
            "sent": true,
            "messageId": message_id,
            "chatId": chat,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{ctx, failure, msg, one};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Stands in for `api.telegram.org`; `apiBase` exists so tests can point
    /// the node at it without a mock HTTP client.
    async fn fake_telegram(
        status: u16,
        body: &str,
    ) -> (String, tokio::sync::oneshot::Receiver<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel();
        let body = body.to_string();
        tokio::spawn(async move {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let mut buf = Vec::new();
            let mut chunk = [0u8; 1024];
            loop {
                let n = match sock.read(&mut chunk).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                buf.extend_from_slice(&chunk[..n]);
                let text = String::from_utf8_lossy(&buf).to_string();
                if let Some(head) = text.find("\r\n\r\n") {
                    let len: usize = text
                        .to_lowercase()
                        .split("content-length:")
                        .nth(1)
                        .and_then(|s| s.split("\r\n").next())
                        .and_then(|s| s.trim().parse().ok())
                        .unwrap_or(0);
                    if buf.len() >= head + 4 + len {
                        break;
                    }
                }
            }
            let _ = tx.send(String::from_utf8_lossy(&buf).to_string());
            let resp = format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes()).await;
            let _ = sock.shutdown().await;
        });
        (format!("http://{addr}"), rx)
    }

    fn config(base: &str, extra: Value) -> Value {
        let mut c = json!({
            "botToken": "123:ABC",
            "chatId": "-100777",
            "message": "Cảnh báo ${name}: ${temp} độ",
            "apiBase": base,
        });
        for (k, v) in extra.as_object().cloned().unwrap_or_default() {
            c[k] = v;
        }
        c
    }

    #[tokio::test]
    async fn a_sent_message_continues_the_chain() {
        let (base, rx) = fake_telegram(200, r#"{"ok":true,"result":{"message_id":42}}"#).await;
        let r = TelegramSendRule::new();
        let c = ctx("telegram-send", config(&base, json!({})));
        let (port, data) = one(r
            .handle(&c, msg(json!({ "name": "kho A", "temp": 31.5 })))
            .await);
        assert_eq!(port, "out", "bản Go không phát gì nên chain dừng ở đây");
        assert_eq!(data["sent"], true);
        assert_eq!(data["messageId"], 42);
        assert_eq!(data["chatId"], "-100777");

        let req = rx.await.unwrap();
        assert!(req.starts_with("POST /bot123:ABC/sendMessage "), "{req}");
        assert!(req.contains("Cảnh báo kho A: 31.5 độ"), "{req}");
        assert!(req.contains("\"chat_id\":-100777"), "{req}");
        assert!(req.contains("\"parse_mode\":\"HTML\""), "{req}");
    }

    #[tokio::test]
    async fn parse_mode_none_and_silent_are_honoured() {
        let (base, rx) = fake_telegram(200, r#"{"ok":true,"result":{"message_id":1}}"#).await;
        let r = TelegramSendRule::new();
        let c = ctx(
            "telegram-send",
            config(&base, json!({ "parseMode": "None", "silent": true })),
        );
        one(r.handle(&c, msg(json!({ "name": "x", "temp": 1 }))).await);
        let req = rx.await.unwrap();
        assert!(!req.contains("parse_mode"), "{req}");
        assert!(req.contains("\"disable_notification\":true"), "{req}");
    }

    /// Go logged this and pretended the send had worked.
    #[tokio::test]
    async fn an_ok_false_body_is_a_real_failure() {
        let (base, _rx) =
            fake_telegram(200, r#"{"ok":false,"description":"chat not found"}"#).await;
        let r = TelegramSendRule::new();
        let c = ctx("telegram-send", config(&base, json!({})));
        let err = failure(r.handle(&c, msg(json!({ "name": "a", "temp": 1 }))).await);
        assert!(err.contains("chat not found"), "{err}");
    }

    #[tokio::test]
    async fn an_http_error_is_reported_with_its_status() {
        let (base, _rx) = fake_telegram(401, r#"{"ok":false,"description":"Unauthorized"}"#).await;
        let r = TelegramSendRule::new();
        let c = ctx("telegram-send", config(&base, json!({})));
        let err = failure(r.handle(&c, msg(json!({ "name": "a", "temp": 1 }))).await);
        assert!(err.contains("401"), "{err}");
        assert!(err.contains("Unauthorized"), "{err}");
    }

    #[tokio::test]
    async fn an_at_handle_chat_id_is_sent_as_a_string() {
        let (base, rx) = fake_telegram(200, r#"{"ok":true,"result":{"message_id":5}}"#).await;
        let r = TelegramSendRule::new();
        let c = ctx(
            "telegram-send",
            config(&base, json!({ "chatId": "@kenh_canh_bao" })),
        );
        one(r.handle(&c, msg(json!({ "name": "x", "temp": 2 }))).await);
        let req = rx.await.unwrap();
        assert!(req.contains("\"chat_id\":\"@kenh_canh_bao\""), "{req}");
    }

    #[tokio::test]
    async fn missing_config_fails_before_the_call() {
        let r = TelegramSendRule::new();
        let c = ctx("telegram-send", json!({ "chatId": "1", "message": "x" }));
        assert!(failure(r.handle(&c, msg(json!({}))).await).contains("Bot token"));
    }

    #[test]
    fn validate_lists_every_missing_field() {
        let r = TelegramSendRule::new();
        assert_eq!(r.validate(&json!({})).len(), 3);
        assert!(r
            .validate(&json!({ "botToken": "t", "chatId": "1", "message": "hi" }))
            .is_empty());
        assert!(!r
            .validate(&json!({ "botToken": "t", "chatId": "1", "message": "hi", "parseMode": "MarkdownV3" }))
            .is_empty());
    }

    #[test]
    fn chat_id_keeps_numbers_numeric() {
        assert_eq!(chat_id_value("-100777"), json!(-100777));
        assert_eq!(chat_id_value("@abc"), json!("@abc"));
    }
}
