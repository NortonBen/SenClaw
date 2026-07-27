//! `senclaw-send` — gửi tin nhắn ra kênh chat của SenClaw.
//!
//! `mcp__senclaw-send__send_message` là MCP của **lõi** daemon, không phải của
//! Space App, nên `app_mcp_call` không chạm tới được. Đường duy nhất là nhờ một
//! lượt agent (`agent.run`) với allowlist đúng một tool đó.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::daq;
use crate::engine::spec::{Category, Rule, RuleSpec, RunCtx};
use crate::engine::types::{Message, Outcome};

const SEND_TOOL: &str = "mcp__senclaw-send__send_message";

const SYSTEM: &str = "Bạn là bộ phận gửi tin của rule engine. Chỉ làm đúng một việc: \
gọi tool mcp__senclaw-send__send_message với đúng target và nội dung được cho, \
nguyên văn, không thêm bớt, không diễn giải, không hỏi lại. Sau khi gọi tool xong \
thì trả lời ngắn gọn `đã gửi`.";

pub struct SenclawSendRule {
    spec: RuleSpec,
}

pub fn rule() -> Arc<dyn Rule> {
    Arc::new(SenclawSendRule::new())
}

impl SenclawSendRule {
    fn new() -> Self {
        let spec = RuleSpec::builder("senclaw-send", "Gửi tin SenClaw", Category::Sink)
            .desc("Gửi một tin nhắn tới kênh chat của SenClaw (web, Telegram, Feishu...).")
            .icon("💬")
            .color("#52c41a")
            .schema(json!({
                "type": "object",
                "required": ["target", "message"],
                "properties": {
                    "target": {
                        "type": "string",
                        "title": "Đích đến",
                        "placeholder": "web:main",
                        "description": "Jid của kênh, ví dụ `web:main` hoặc `telegram:123456`."
                    },
                    "message": {
                        "type": "string",
                        "title": "Nội dung",
                        "ui": "textarea",
                        "placeholder": "Cảnh báo: ${device_id} đang ở ${temperature} độ.",
                        "description": "Có nội suy ${field} / ${a.b[0]}."
                    },
                    "timeoutSeconds": {
                        "type": "integer",
                        "title": "Thời gian chờ (giây)",
                        "default": 120,
                        "description": "Daemon ép về khoảng 10..1800."
                    }
                }
            }))
            .doc(
                "Gửi tin nhắn qua chính SenClaw.\n\n\
                 **Đây là đường vòng.** Tool `mcp__senclaw-send__send_message` thuộc lõi \
                 daemon chứ không phải một Space App, nên app không gọi trực tiếp được; \
                 node này nhờ một lượt agent gọi hộ, với allowlist đúng một tool đó. \
                 Hệ quả: chậm (mỗi lần gửi là một lượt LLM), tốn token, và không đảm bảo \
                 tuyệt đối — agent vẫn có thể diễn giải sai hoặc bỏ qua lời gọi tool. \
                 Node trả lời của agent được ghi vào log của chuỗi để đối chiếu.\n\n\
                 Nếu chỉ cần báo trong giao diện rule engine thì dùng node `notification` \
                 — nhanh, không tốn LLM, và chắc chắn hơn.\n\n\
                 Payload đi ra cổng `out` nguyên vẹn, không bị thay đổi.",
            )
            .build();
        Self { spec }
    }
}

/// `(target, message, prompt)` sau khi nội suy. Tách ra để test không cần daemon.
pub fn render(ctx: &RunCtx, msg: &Message) -> (String, String, String) {
    let target = ctx
        .cfg_str("target")
        .map(|t| daq::interpolate(&t, &msg.data, &msg.meta))
        .unwrap_or_default();
    let body = ctx
        .cfg_str("message")
        .map(|t| daq::interpolate(&t, &msg.data, &msg.meta))
        .unwrap_or_default();
    let prompt = format!(
        "Gọi tool {SEND_TOOL} để gửi tin nhắn.\n\
         - target: {target}\n\
         - nội dung (gửi nguyên văn, giữ đúng xuống dòng):\n{body}"
    );
    (target, body, prompt)
}

#[async_trait]
impl Rule for SenclawSendRule {
    fn spec(&self) -> &RuleSpec {
        &self.spec
    }

    fn validate(&self, config: &Value) -> Vec<String> {
        let mut out = Vec::new();
        let filled = |k: &str| {
            config
                .get(k)
                .and_then(|v| v.as_str())
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false)
        };
        if !filled("target") {
            out.push("Thiếu đích đến, ví dụ `web:main`.".to_string());
        }
        if !filled("message") {
            out.push("Thiếu nội dung tin nhắn.".to_string());
        }
        out
    }

    async fn handle(&self, ctx: &RunCtx, msg: Message) -> Outcome {
        let (target, body, prompt) = render(ctx, &msg);
        if target.trim().is_empty() {
            return ctx.fail_config("Thiếu đích đến, ví dụ `web:main`.");
        }
        if body.trim().is_empty() {
            return ctx.fail_config("Nội dung tin nhắn rỗng sau khi nội suy.");
        }

        let reply = ctx
            .svc
            .bridge
            .agent_run(
                &prompt,
                Some(SYSTEM),
                Some(vec![SEND_TOOL.to_string()]),
                None,
                ctx.cfg_u64_or("timeoutSeconds", 120),
            )
            .await;
        match reply {
            // The agent's own words are the only evidence the send happened.
            Ok(text) => ctx.log("info", format!("gửi tới {target}: {}", text.trim())),
            Err(e) => return ctx.fail_runtime(e),
        }
        Outcome::out(msg.data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{ctx, failure, msg, msg_with_meta};

    #[test]
    fn the_prompt_carries_the_interpolated_target_and_message() {
        let c = ctx(
            "senclaw-send",
            json!({
                "target": "telegram:${chat_id}",
                "message": "Cảnh báo ${device_id}: ${temperature} độ."
            }),
        );
        let m = msg_with_meta(
            json!({ "temperature": 31.5 }),
            json!({ "chat_id": 42, "device_id": "d-1" }),
        );
        let (target, body, prompt) = render(&c, &m);
        assert_eq!(target, "telegram:42");
        assert_eq!(body, "Cảnh báo d-1: 31.5 độ.");
        assert!(prompt.contains(SEND_TOOL), "{prompt}");
        assert!(prompt.contains("telegram:42"), "{prompt}");
        assert!(prompt.contains("Cảnh báo d-1: 31.5 độ."), "{prompt}");
    }

    #[tokio::test]
    async fn a_missing_target_fails_before_spending_an_agent_turn() {
        let c = ctx("senclaw-send", json!({ "message": "xin chào" }));
        let err = failure(SenclawSendRule::new().handle(&c, msg(json!({}))).await);
        assert!(err.contains("đích đến"), "{err}");
    }

    #[tokio::test]
    async fn a_message_that_interpolates_to_nothing_fails() {
        let c = ctx(
            "senclaw-send",
            json!({ "target": "web:main", "message": "${nope}" }),
        );
        let err = failure(SenclawSendRule::new().handle(&c, msg(json!({}))).await);
        assert!(err.contains("rỗng"), "{err}");
    }

    #[test]
    fn validate_catches_both_missing_fields_at_save_time() {
        let r = SenclawSendRule::new();
        assert_eq!(r.validate(&json!({})).len(), 2);
        assert!(r
            .validate(&json!({ "target": "web:main", "message": "hi" }))
            .is_empty());
    }

    #[test]
    fn the_allowlist_is_exactly_the_core_send_tool() {
        assert_eq!(SEND_TOOL, "mcp__senclaw-send__send_message");
        let r = SenclawSendRule::new();
        assert!(r.spec().has_output("out"));
        assert!(r.spec().has_output("error"));
    }
}
