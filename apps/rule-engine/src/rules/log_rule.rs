//! `log` — write a line to the chain log, then keep going.
//!
//! Deliberately different from the Go rule, which logged and then published
//! nothing: dropping a `log` node into the middle of a chain silently killed
//! everything downstream, and the only way to find out was that the chain
//! stopped working. Here the message is forwarded on `out`, so `log` is a probe
//! you can insert anywhere without rewiring.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::daq;
use crate::engine::spec::{Category, PortSpec, Rule, RuleSpec, RunCtx};
use crate::engine::types::{Message, Outcome};

const LEVELS: [&str; 4] = ["debug", "info", "warn", "error"];

pub struct LogRule {
    spec: RuleSpec,
}

pub fn rule() -> Arc<dyn Rule> {
    Arc::new(LogRule::new())
}

impl LogRule {
    fn new() -> Self {
        let spec = RuleSpec::builder("log", "Ghi log", Category::Sink)
            .desc("Ghi một dòng log rồi vẫn cho message đi tiếp — dùng để soi dữ liệu giữa chuỗi.")
            .icon("📝")
            .color("#8c8c8c")
            .outputs(vec![PortSpec::output()
                .color("#8c8c8c")
                .desc("Message đi tiếp nguyên vẹn sau khi đã ghi log.")])
            .schema(json!({
                "type": "object",
                "properties": {
                    "level": {
                        "type": "string",
                        "title": "Mức độ",
                        "ui": "select",
                        "enum": ["debug", "info", "warn", "error"],
                        "default": "info",
                        "description": "Dùng để lọc trong bảng log."
                    },
                    "message": {
                        "type": "string",
                        "title": "Nội dung",
                        "ui": "textarea",
                        "placeholder": "Nhiệt độ ${temperature} tại ${device_id}",
                        "description": "Chèn dữ liệu bằng ${đường.dẫn}: lấy từ payload trước, không có thì lấy thẳng từ metadata bằng tên field (vd `${device_id}`) — KHÔNG có tiền tố `meta_data.`. Bỏ trống sẽ in toàn bộ payload dạng JSON."
                    }
                }
            }))
            .doc(
                "Đặt ở bất kỳ đâu để xem dữ liệu đang chảy qua.\n\n\
                 ```json\n\
                 { \"level\": \"warn\", \"message\": \"Nhiệt độ ${temperature} vượt ngưỡng\" }\n\
                 ```\n\n\
                 - `${...}` lấy từ payload trước, không có thì lấy từ metadata; \
                   đường dẫn lồng nhau và chỉ số mảng đều được (`${user.ten}`, `${l[0]}`).\n\
                 - Bỏ trống nội dung sẽ in `<tên node> ← <payload JSON>`.\n\
                 - **Message vẫn đi tiếp ra cổng `out`.** Node này soi dữ liệu chứ không \
                   chặn luồng, nên cắm vào giữa chuỗi cũng không làm gãy phần phía sau. \
                   Muốn kết thúc nhánh thì đơn giản là đừng nối gì vào `out`.\n\
                 - Log hiện ngay ở bảng log và luồng sự kiện của giao diện.",
            )
            .build();
        Self { spec }
    }
}

#[async_trait]
impl Rule for LogRule {
    fn spec(&self) -> &RuleSpec {
        &self.spec
    }

    fn validate(&self, config: &Value) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(l) = config.get("level").and_then(|v| v.as_str()) {
            if !l.trim().is_empty() && !LEVELS.contains(&l.trim().to_ascii_lowercase().as_str()) {
                out.push(format!(
                    "Mức độ `{l}` không hợp lệ (chỉ nhận {}).",
                    LEVELS.join(", ")
                ));
            }
        }
        out
    }

    async fn handle(&self, ctx: &RunCtx, msg: Message) -> Outcome {
        let raw = ctx.cfg_str_or("level", "info").trim().to_ascii_lowercase();
        // An unknown level is a form typo, not a reason to lose the line.
        let level = if LEVELS.contains(&raw.as_str()) {
            raw
        } else {
            "info".to_string()
        };

        let text = match ctx.cfg_str("message") {
            Some(t) => daq::interpolate(&t, &msg.data, &msg.meta),
            None => format!("{} ← {}", ctx.node, msg.data),
        };
        ctx.log(&level, text);

        Outcome::out(msg.data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{ctx, msg, msg_with_meta, one};

    #[tokio::test]
    async fn the_template_is_interpolated_and_stored() {
        let r = LogRule::new();
        let c = ctx(
            "log",
            json!({ "level": "warn", "message": "Nhiệt độ ${temperature} độ" }),
        );
        let out = r.handle(&c, msg(json!({ "temperature": 31.5 }))).await;
        assert_eq!(one(out).0, "out");

        let logs = c.svc.db.list_logs(1, 10).unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].level, "warn");
        assert_eq!(logs[0].message, "Nhiệt độ 31.5 độ");
        assert_eq!(logs[0].node.as_deref(), Some("n1"));
    }

    /// The Go rule stopped the chain here; ours must not.
    #[tokio::test]
    async fn the_message_still_continues_downstream() {
        let r = LogRule::new();
        let c = ctx("log", json!({ "message": "xin chào" }));
        let input = json!({ "a": 1, "nested": { "b": 2 } });
        let (port, data) = one(r.handle(&c, msg(input.clone())).await);
        assert_eq!(port, "out");
        assert_eq!(data, input);
    }

    #[tokio::test]
    async fn metadata_is_reachable_from_the_template() {
        let r = LogRule::new();
        let c = ctx("log", json!({ "message": "tu ${device_id}" }));
        let _ = r
            .handle(&c, msg_with_meta(json!({}), json!({ "device_id": "d3" })))
            .await;
        assert_eq!(c.svc.db.list_logs(1, 10).unwrap()[0].message, "tu d3");
    }

    #[tokio::test]
    async fn an_empty_template_dumps_the_payload() {
        let r = LogRule::new();
        let c = ctx("log", json!({}));
        let _ = r.handle(&c, msg(json!({ "a": 1 }))).await;
        let line = c.svc.db.list_logs(1, 10).unwrap().remove(0).message;
        assert!(line.contains("n1"), "{line}");
        assert!(line.contains("\"a\":1"), "{line}");
    }

    #[tokio::test]
    async fn an_unknown_level_falls_back_to_info_instead_of_dropping_the_line() {
        let r = LogRule::new();
        let c = ctx("log", json!({ "level": "PANIC", "message": "x" }));
        let _ = r.handle(&c, msg(json!({}))).await;
        assert_eq!(c.svc.db.list_logs(1, 10).unwrap()[0].level, "info");
    }

    #[test]
    fn validate_rejects_an_unknown_level() {
        let r = LogRule::new();
        assert!(r.validate(&json!({ "level": "warn" })).is_empty());
        assert!(r.validate(&json!({})).is_empty());
        assert!(!r.validate(&json!({ "level": "panic" })).is_empty());
    }
}
