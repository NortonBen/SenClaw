//! `notification` — raise a human-facing notice.
//!
//! There is no notification table in the core daemon to write into, so the
//! notice goes where the operator is already looking: the chain log. `LogSink`
//! writes the row *and* publishes `EngineEvent::Log`, which is what feeds the
//! canvas console and `/api/events`, so one call covers both.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::daq;
use crate::engine::spec::{Category, Rule, RuleSpec, RunCtx};
use crate::engine::types::{now_ms, Message, Outcome};

const LEVELS: [&str; 4] = ["info", "warning", "error", "success"];

pub struct NotificationRule {
    spec: RuleSpec,
}

pub fn rule() -> Arc<dyn Rule> {
    Arc::new(NotificationRule::new())
}

impl NotificationRule {
    fn new() -> Self {
        let spec = RuleSpec::builder("notification", "Thông báo", Category::Sink)
            .desc("Ghi một thông báo có mức độ vào log của chain và đẩy lên UI.")
            .icon("🔔")
            .color("#1890ff")
            .schema(json!({
                "type": "object",
                "required": ["message"],
                "properties": {
                    "title": {
                        "type": "string",
                        "title": "Tiêu đề",
                        "placeholder": "Nhiệt độ vượt ngưỡng"
                    },
                    "message": {
                        "type": "string",
                        "title": "Nội dung",
                        "ui": "textarea",
                        "placeholder": "${name} đang ở ${temperature} độ",
                        "description": "Chèn dữ liệu bằng ${field} hoặc ${a.b.c}."
                    },
                    "level": {
                        "type": "string",
                        "title": "Mức độ",
                        "ui": "select",
                        "enum": LEVELS,
                        "default": "info"
                    }
                }
            }))
            .doc(
                "Ghi thông báo vào log của chain (kèm mức độ) và đẩy sự kiện lên \
                 `/api/events`, nên nó hiện ngay trong console của canvas.\n\n\
                 Không dừng nhánh: phát tiếp `{ title, message, level, ts }` ra cổng `out` \
                 để nối sang Telegram, HTTP hay bất cứ node nào khác.",
            )
            .build();
        Self { spec }
    }
}

#[async_trait]
impl Rule for NotificationRule {
    fn spec(&self) -> &RuleSpec {
        &self.spec
    }

    fn validate(&self, config: &Value) -> Vec<String> {
        let mut out = Vec::new();
        match config.get("message").and_then(|v| v.as_str()) {
            Some(s) if !s.trim().is_empty() => {}
            _ => out.push("Thiếu nội dung thông báo.".to_string()),
        }
        if let Some(l) = config.get("level").and_then(|v| v.as_str()) {
            if !l.trim().is_empty() && !LEVELS.contains(&l) {
                out.push(format!(
                    "Mức độ `{l}` không hợp lệ. Chọn: {}.",
                    LEVELS.join(", ")
                ));
            }
        }
        out
    }

    async fn handle(&self, ctx: &RunCtx, msg: Message) -> Outcome {
        let Some(message_tpl) = ctx.cfg_str("message") else {
            return ctx.fail_config("Thiếu nội dung thông báo.");
        };
        let level = ctx.cfg_str_or("level", "info");
        if !LEVELS.contains(&level.as_str()) {
            return ctx.fail_config(format!(
                "Mức độ `{level}` không hợp lệ. Chọn: {}.",
                LEVELS.join(", ")
            ));
        }

        let title = ctx
            .cfg_str("title")
            .map(|t| daq::interpolate(&t, &msg.data, &msg.meta))
            .unwrap_or_default();
        let message = daq::interpolate(&message_tpl, &msg.data, &msg.meta);

        let line = if title.is_empty() {
            message.clone()
        } else {
            format!("{title}: {message}")
        };
        ctx.log(&level, line);

        Outcome::out(json!({
            "title": title,
            "message": message,
            "level": level,
            "ts": now_ms(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{ctx, failure, msg, msg_with_meta, one};

    #[tokio::test]
    async fn title_and_message_are_interpolated_and_emitted() {
        let r = NotificationRule::new();
        let c = ctx(
            "notification",
            json!({
                "title": "Cảnh báo ${name}",
                "message": "${name} đang ở ${temp} độ",
                "level": "warning"
            }),
        );
        let (port, data) = one(r
            .handle(&c, msg(json!({ "name": "kho A", "temp": 31.5 })))
            .await);
        assert_eq!(port, "out");
        assert_eq!(data["title"], "Cảnh báo kho A");
        assert_eq!(data["message"], "kho A đang ở 31.5 độ");
        assert_eq!(data["level"], "warning");
        assert!(data["ts"].as_i64().unwrap() > 0);
    }

    #[tokio::test]
    async fn the_notice_is_readable_back_from_the_log() {
        let r = NotificationRule::new();
        let c = ctx(
            "notification",
            json!({ "title": "Sự cố", "message": "bơm ${id} dừng", "level": "error" }),
        );
        one(r.handle(&c, msg(json!({ "id": "P-2" }))).await);
        let logs = c.svc.db.list_logs(1, 10).unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].level, "error");
        assert_eq!(logs[0].message, "Sự cố: bơm P-2 dừng");
        assert_eq!(logs[0].node.as_deref(), Some("n1"));
    }

    #[tokio::test]
    async fn the_notice_is_pushed_onto_the_event_bus() {
        let r = NotificationRule::new();
        let c = ctx("notification", json!({ "message": "xong" }));
        let mut rx = c.svc.bus.subscribe();
        one(r.handle(&c, msg(json!({}))).await);
        let ev = rx.try_recv().expect("phải có sự kiện log");
        assert!(ev.contains("\"type\":\"log\""), "{ev}");
        assert!(ev.contains("xong"), "{ev}");
    }

    #[tokio::test]
    async fn metadata_is_reachable_from_the_template() {
        let r = NotificationRule::new();
        let c = ctx("notification", json!({ "message": "từ ${device_id}" }));
        let (_, data) = one(r
            .handle(&c, msg_with_meta(json!({}), json!({ "device_id": "d-1" })))
            .await);
        assert_eq!(data["message"], "từ d-1");
    }

    #[tokio::test]
    async fn a_missing_message_or_a_bad_level_fails() {
        let r = NotificationRule::new();
        let c = ctx("notification", json!({ "title": "chỉ có tiêu đề" }));
        assert!(failure(r.handle(&c, msg(json!({}))).await).contains("Thiếu nội dung"));

        let c = ctx("notification", json!({ "message": "hi", "level": "fatal" }));
        assert!(failure(r.handle(&c, msg(json!({}))).await).contains("Mức độ"));
    }

    #[test]
    fn validate_checks_the_message_and_the_level() {
        let r = NotificationRule::new();
        assert!(!r.validate(&json!({})).is_empty());
        assert!(!r
            .validate(&json!({ "message": "x", "level": "debug" }))
            .is_empty());
        assert!(r
            .validate(&json!({ "message": "x", "level": "success" }))
            .is_empty());
    }
}
