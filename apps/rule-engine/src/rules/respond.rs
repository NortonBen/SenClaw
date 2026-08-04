//! `respond` — the value returned to a synchronous caller.
//!
//! Whatever message reaches this node becomes the run's *result*: the object
//! `rule_call` returns under `result`. It is a sink (Terminal) — the branch
//! ends here. A flow meant to be called synchronously should have exactly one
//! `respond`; if several fire in one run, the last one wins.
//!
//! With no synchronous caller waiting, `respond` is harmless: it just records a
//! value that the run reaper later discards.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::daq;
use crate::engine::spec::{Category, Rule, RuleSpec, RunCtx};
use crate::engine::types::{Message, Outcome};

pub struct RespondRule {
    spec: RuleSpec,
}

pub fn rule() -> Arc<dyn Rule> {
    Arc::new(RespondRule::new())
}

impl RespondRule {
    fn new() -> Self {
        let spec = RuleSpec::builder("respond", "Trả kết quả", Category::Sink)
            .desc("Chốt giá trị trả về cho lời gọi đồng bộ `rule_call`. Nhánh kết thúc tại đây.")
            .icon("📤")
            .color("#722ed1")
            .outputs(vec![]) // sink: only the implicit `error` port
            .schema(json!({
                "type": "object",
                "properties": {
                    "field": {
                        "type": "string",
                        "title": "Trường trả về",
                        "placeholder": "ket_qua",
                        "description": "Chỉ trả về một trường con của payload. Bỏ trống = trả về toàn bộ payload."
                    }
                }
            }))
            .doc(
                "Đặt ở cuối luồng đồng bộ (bắt đầu bằng node `request`).\n\n\
                 - Bỏ trống `field`: `result` = toàn bộ payload tới đây.\n\
                 - Đặt `field`: `result` = giá trị của trường đó.\n\n\
                 Không nối `respond` mà gọi `rule_call` thì vẫn nhận được `status`, chỉ `result` rỗng.",
            )
            .build();
        Self { spec }
    }
}

#[async_trait]
impl Rule for RespondRule {
    fn spec(&self) -> &RuleSpec {
        &self.spec
    }

    async fn handle(&self, ctx: &RunCtx, msg: Message) -> Outcome {
        let value = match ctx.cfg_str("field") {
            Some(field) => daq::get(&msg.data, &field).unwrap_or(Value::Null),
            None => msg.data.clone(),
        };
        ctx.respond(value);
        Outcome::Terminal
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::engine::services::{EventBus, Services};
    use crate::engine::spec::RunCtx;
    use crate::engine::types::{now_ms, Message, PortRef};

    fn ctx(run_id: u64, config: Value) -> RunCtx {
        let db = Arc::new(Db::open(":memory:").unwrap());
        let _ = db.create_chain(1, "t", "");
        let svc = Arc::new(Services::new(db, EventBus::new()));
        RunCtx {
            chain_id: 1,
            run_id,
            node: "r".into(),
            rule: "respond".into(),
            config,
            svc,
        }
    }

    fn msg(data: Value) -> Message {
        Message::seed(7, 1, PortRef::new("r", "in"), data, json!({}))
    }

    #[tokio::test]
    async fn whole_payload_becomes_the_result() {
        let c = ctx(7, json!({}));
        let out = RespondRule::new().handle(&c, msg(json!({"a": 1}))).await;
        assert!(matches!(out, Outcome::Terminal));
        assert_eq!(c.svc.results.take(7), Some(json!({"a": 1})));
    }

    #[tokio::test]
    async fn a_single_field_can_be_returned() {
        let c = ctx(7, json!({ "field": "a" }));
        RespondRule::new()
            .handle(&c, msg(json!({"a": 42, "b": 9})))
            .await;
        assert_eq!(c.svc.results.take(7), Some(json!(42)));
        let _ = now_ms();
    }
}
