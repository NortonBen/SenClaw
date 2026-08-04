//! `store` — remember the latest value flowing through, and pass it on.
//!
//! A passthrough (like `log`): the message continues on `out`, but a copy is
//! cached in node state so the `rule_get` MCP tool can read the most recent
//! value without starting a run. This is the "latest value" / GET endpoint of a
//! flow — e.g. the last sensor reading, the last computed total.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::daq;
use crate::engine::spec::{Category, PortSpec, Rule, RuleSpec, RunCtx};
use crate::engine::types::{now_ms, Message, Outcome};

/// State scope the value is cached under. `rule_get` reads the same scope.
pub const SCOPE: &str = "value";

pub struct StoreRule {
    spec: RuleSpec,
}

pub fn rule() -> Arc<dyn Rule> {
    Arc::new(StoreRule::new())
}

impl StoreRule {
    fn new() -> Self {
        let spec = RuleSpec::builder("store", "Lưu giá trị", Category::Transform)
            .desc("Ghi nhớ giá trị mới nhất chảy qua rồi cho message đi tiếp. Đọc lại bằng `rule_get`.")
            .icon("🗄️")
            .color("#faad14")
            .outputs(vec![PortSpec::output()
                .color("#faad14")
                .desc("Message đi tiếp nguyên vẹn sau khi đã lưu.")])
            .schema(json!({
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "title": "Trường cần lưu",
                        "placeholder": "temperature",
                        "description": "Chỉ lưu một trường con của payload. Bỏ trống = lưu toàn bộ payload."
                    }
                }
            }))
            .doc(
                "Đặt ở nơi có giá trị muốn tra cứu sau này.\n\n\
                 - Bỏ trống `key`: lưu toàn bộ payload.\n\
                 - Đặt `key`: chỉ lưu trường đó.\n\n\
                 Đọc lại bằng MCP `rule_get` với `{ \"chainId\": <id>, \"node\": \"<id node store>\" }` \
                 → trả `{ value, ts }` (ts là thời điểm lưu, ms). Chưa có giá trị nào thì `value` rỗng.",
            )
            .build();
        Self { spec }
    }
}

#[async_trait]
impl Rule for StoreRule {
    fn spec(&self) -> &RuleSpec {
        &self.spec
    }

    async fn handle(&self, ctx: &RunCtx, msg: Message) -> Outcome {
        let value = match ctx.cfg_str("key") {
            Some(key) => daq::get(&msg.data, &key).unwrap_or(Value::Null),
            None => msg.data.clone(),
        };
        ctx.state_set(SCOPE, &json!({ "value": value, "ts": now_ms() }));
        Outcome::out(msg.data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::engine::services::{EventBus, Services};
    use crate::engine::types::PortRef;

    fn ctx(config: Value) -> RunCtx {
        let db = Arc::new(Db::open(":memory:").unwrap());
        let _ = db.create_chain(1, "t", "");
        let svc = Arc::new(Services::new(db, EventBus::new()));
        RunCtx {
            chain_id: 1,
            run_id: 5,
            node: "s".into(),
            rule: "store".into(),
            config,
            svc,
        }
    }

    fn msg(data: Value) -> Message {
        Message::seed(5, 1, PortRef::new("s", "in"), data, json!({}))
    }

    #[tokio::test]
    async fn caches_and_passes_through() {
        let c = ctx(json!({}));
        let out = StoreRule::new().handle(&c, msg(json!({"t": 30}))).await;
        match out {
            Outcome::Emit(e) => assert_eq!(e[0].data, json!({"t": 30})),
            _ => panic!("phải đi tiếp trên out"),
        }
        let cached = c.svc.state.get(1, "s", SCOPE).unwrap();
        assert_eq!(cached["value"], json!({"t": 30}));
        assert!(cached["ts"].as_i64().unwrap() > 0);
    }

    #[tokio::test]
    async fn a_single_field_can_be_stored() {
        let c = ctx(json!({ "key": "t" }));
        StoreRule::new()
            .handle(&c, msg(json!({"t": 30, "x": 1})))
            .await;
        let cached = c.svc.state.get(1, "s", SCOPE).unwrap();
        assert_eq!(cached["value"], json!(30));
    }
}
