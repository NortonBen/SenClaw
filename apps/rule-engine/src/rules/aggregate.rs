//! `aggregate` — gộp nhiều message thành một mảng. Nghịch đảo của `split`.
//!
//! Cổng `in` tích luỹ payload vào bộ đệm; khi bộ đệm đạt `count` message thì phát
//! `{ items, count }` ra `out` và xoá đệm. Cổng `flush` ép phát ngay những gì đang
//! có (kể cả rỗng) — dùng làm chốt kết thúc, ví dụ nối từ cổng `done` của `split`.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::engine::spec::{Category, PortSpec, Rule, RuleSpec, RunCtx};
use crate::engine::types::{Message, Outcome};

const SCOPE_BUF: &str = "buf";
const PORT_OUT: &str = "out";
const PORT_FLUSH: &str = "flush";
const DEFAULT_COUNT: u64 = 10;

pub struct AggregateRule {
    spec: RuleSpec,
}

pub fn rule() -> Arc<dyn Rule> {
    Arc::new(AggregateRule::new())
}

impl AggregateRule {
    fn new() -> Self {
        let spec = RuleSpec::builder("aggregate", "Gộp mảng", Category::Transform)
            .desc("Gộp nhiều message thành một mảng theo ngưỡng số lượng hoặc khi có tín hiệu `flush`.")
            .icon("📦")
            .color("#13c2c2")
            .inputs(vec![
                PortSpec::input(),
                PortSpec::new(PORT_FLUSH, "flush")
                    .color("#faad14")
                    .desc("Ép phát ngay những gì đang có trong đệm (kể cả rỗng)."),
            ])
            .schema(json!({
                "type": "object",
                "properties": {
                    "count": {
                        "type": "integer",
                        "title": "Ngưỡng gộp",
                        "default": DEFAULT_COUNT,
                        "minimum": 0,
                        "description": "Đạt bấy nhiêu message thì tự phát. Để 0 (hoặc bỏ trống) thì chỉ phát khi có `flush`."
                    }
                }
            }))
            .doc(
                "Nghịch đảo của `split`: gom payload lại rồi phát một message \
                 `{ \"items\": [...], \"count\": N }` ra cổng `out`.\n\n\
                 - Message vào cổng `in`: đẩy `data` vào đệm. Nếu đệm đạt `count` \
                   thì phát và xoá đệm; chưa đạt thì nhánh dừng (giữ state).\n\
                 - Message vào cổng `flush`: phát ngay những gì đang có (kể cả mảng \
                   rỗng) rồi xoá đệm.\n\n\
                 `count = 0` (hoặc bỏ trống): chỉ gộp thủ công qua `flush`, hợp khi \
                 nối `done` của `split` vào `flush` để gom đúng một lô.",
            )
            .build();
        Self { spec }
    }
}

#[async_trait]
impl Rule for AggregateRule {
    fn spec(&self) -> &RuleSpec {
        &self.spec
    }

    fn validate(&self, config: &Value) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(c) = config.get("count").filter(|v| !v.is_null()) {
            let n = match c {
                Value::Number(n) => n.as_f64(),
                Value::String(s) => s.trim().parse().ok(),
                _ => None,
            };
            match n {
                Some(v) if v >= 0.0 => {}
                _ => out.push("Ngưỡng gộp phải là số nguyên ≥ 0.".to_string()),
            }
        }
        out
    }

    async fn handle(&self, ctx: &RunCtx, msg: Message) -> Outcome {
        let count = ctx.cfg_u64_or("count", DEFAULT_COUNT) as usize;
        let port = msg.target.port.clone();

        let mut buf: Vec<Value> = ctx
            .state_get(SCOPE_BUF)
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();

        // `flush` ép phát ngay lô hiện tại, kể cả khi rỗng.
        if port == PORT_FLUSH {
            let n = buf.len();
            ctx.state_set(SCOPE_BUF, &json!([]));
            return Outcome::port(PORT_OUT, json!({ "items": buf, "count": n }));
        }

        // Cổng `in`: tích luỹ, phát khi đạt ngưỡng.
        buf.push(msg.data);
        if count > 0 && buf.len() >= count {
            let n = buf.len();
            ctx.state_set(SCOPE_BUF, &json!([]));
            Outcome::port(PORT_OUT, json!({ "items": buf, "count": n }))
        } else {
            ctx.state_set(SCOPE_BUF, &json!(buf));
            Outcome::Terminal
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::types::PortRef;
    use crate::testkit::{ctx, is_terminal, msg, one};

    fn flush_msg() -> Message {
        Message::seed(1, 1, PortRef::new("n1", PORT_FLUSH), json!({}), json!({}))
    }

    #[tokio::test]
    async fn it_buffers_until_count_then_emits() {
        let r = AggregateRule::new();
        let c = ctx("aggregate", json!({ "count": 2 }));

        let first = r.handle(&c, msg(json!({ "id": 1 }))).await;
        assert!(is_terminal(&first), "message đầu chỉ được đệm lại");

        let (port, data) = one(r.handle(&c, msg(json!({ "id": 2 }))).await);
        assert_eq!(port, PORT_OUT);
        assert_eq!(data["count"], 2);
        assert_eq!(data["items"], json!([{ "id": 1 }, { "id": 2 }]));
    }

    #[tokio::test]
    async fn a_flush_on_an_empty_buffer_emits_an_empty_array() {
        let r = AggregateRule::new();
        let c = ctx("aggregate", json!({ "count": 2 }));
        let (port, data) = one(r.handle(&c, flush_msg()).await);
        assert_eq!(port, PORT_OUT);
        assert_eq!(data["count"], 0);
        assert_eq!(data["items"], json!([]));
    }

    #[tokio::test]
    async fn a_flush_emits_a_partial_buffer_and_clears_it() {
        let r = AggregateRule::new();
        // count=0 → chỉ gộp thủ công qua flush.
        let c = ctx("aggregate", json!({ "count": 0 }));
        assert!(is_terminal(&r.handle(&c, msg(json!({ "id": 1 }))).await));
        let (_, data) = one(r.handle(&c, flush_msg()).await);
        assert_eq!(data["items"], json!([{ "id": 1 }]));
        // Đệm đã xoá: flush kế tiếp trả mảng rỗng.
        let (_, again) = one(r.handle(&c, flush_msg()).await);
        assert_eq!(again["count"], 0);
    }

    #[test]
    fn it_declares_two_inputs_and_the_out_port() {
        let r = AggregateRule::new();
        let s = r.spec();
        assert!(s.has_input("in"));
        assert!(s.has_input(PORT_FLUSH));
        assert!(s.has_output(PORT_OUT));
        assert!(s.has_output("error"));
    }

    #[test]
    fn validate_rejects_a_negative_count() {
        let r = AggregateRule::new();
        assert!(r.validate(&json!({})).is_empty());
        assert!(r.validate(&json!({ "count": 5 })).is_empty());
        assert!(!r.validate(&json!({ "count": -1 })).is_empty());
    }
}
