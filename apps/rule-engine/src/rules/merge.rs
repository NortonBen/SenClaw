//! `merge` — wait for every input, then deep-merge the parts into one object.
//!
//! Same barrier as `join`; the difference is entirely in `opts.join`, which
//! selects how the engine folds the parts before this rule is called.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::engine::spec::{Category, PortSpec, Rule, RuleSpec, RunCtx};
use crate::engine::types::{Message, Outcome};
use crate::rules::join_rule::{input_ports, validate_inputs};

pub struct MergeRule {
    spec: RuleSpec,
}

pub fn rule() -> Arc<dyn Rule> {
    Arc::new(MergeRule::new())
}

impl MergeRule {
    fn new() -> Self {
        let spec = RuleSpec::builder("merge", "Trộn (merge)", Category::Logic)
            .desc("Chờ đủ mọi cổng vào rồi trộn sâu các nhánh thành một object phẳng.")
            .icon("⊕")
            .color("#722ed1")
            .inputs(vec![])
            .schema(json!({
                "type": "object",
                "required": ["inputs"],
                "properties": {
                    "inputs": {
                        "type": "array",
                        "title": "Tên các cổng vào",
                        "items": { "type": "string" },
                        "default": ["a", "b"],
                        "description": "Mỗi tên tạo một cổng vào. Khác `join`: tên cổng KHÔNG xuất hiện trong dữ liệu ra."
                    }
                }
            }))
            .doc(
                "Trộn nhiều nhánh vào cùng một object.\n\n\
                 Nhánh `a` trả `{ \"user\": { \"id\": 1 } }`, nhánh `b` trả \
                 `{ \"user\": { \"ten\": \"Lan\" }, \"vip\": true }` → dữ liệu ra:\n\n\
                 ```json\n\
                 { \"user\": { \"id\": 1, \"ten\": \"Lan\" }, \"vip\": true }\n\
                 ```\n\n\
                 **Bắt buộc**: node phải bật `opts.join = \"merge\"`. Giao diện tự đặt \
                 `opts.join = \"merge\"` khi bạn kéo node `merge` ra canvas. **Nếu tạo node \
                 qua MCP/API** thì phải tự đặt `opts.join = \"merge\"` — mặc định `\"any\"`. \
                 Để `\"any\"` thì mỗi message chạy node một lần và không có gì được trộn; \
                 để `\"all\"` thì kết quả bị gói theo tên cổng giống `join`.\n\n\
                 - Trộn **sâu**: hai object cùng khoá được hoà vào nhau, giá trị vô hướng \
                   thì nhánh đến sau ghi đè nhánh trước. Thứ tự theo `seq`, nên nhánh \
                   nào tới sau là do luồng chạy quyết định — đừng dựa vào nó để chọn giá trị.\n\
                 - Cần biết rõ giá trị nào từ nhánh nào thì dùng `join` (giữ tên cổng).\n\
                 - `opts.joinTimeoutMs` và `opts.corrKey` hoạt động y như ở `join`.\n\
                 - Một nhánh lỗi làm cả message trộn thành lỗi và đi ra cổng `error`.",
            )
            .build();
        Self { spec }
    }
}

#[async_trait]
impl Rule for MergeRule {
    fn spec(&self) -> &RuleSpec {
        &self.spec
    }

    fn dynamic_inputs(&self, config: &Value) -> Vec<PortSpec> {
        input_ports(config, "Nội dung được trộn phẳng vào kết quả.")
    }

    fn validate(&self, config: &Value) -> Vec<String> {
        validate_inputs(config)
    }

    async fn handle(&self, _ctx: &RunCtx, msg: Message) -> Outcome {
        // The engine already deep-merged the parts; nothing left but to forward.
        Outcome::out(msg.data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{ctx, msg, one};

    #[tokio::test]
    async fn the_merged_payload_passes_through_unchanged() {
        let r = MergeRule::new();
        let c = ctx("merge", json!({ "inputs": ["a", "b"] }));
        let merged = json!({ "user": { "id": 1, "ten": "Lan" }, "vip": true });
        let (port, data) = one(r.handle(&c, msg(merged.clone())).await);
        assert_eq!(port, "out");
        assert_eq!(data, merged);
    }

    #[test]
    fn inputs_are_dynamic_and_default_to_two() {
        let r = MergeRule::new();
        let ids: Vec<String> = r
            .dynamic_inputs(&json!({}))
            .into_iter()
            .map(|p| p.id)
            .collect();
        assert_eq!(ids, vec!["a", "b"]);
        assert!(r.spec().inputs.is_empty());
    }

    #[test]
    fn validate_shares_the_join_rules() {
        let r = MergeRule::new();
        assert!(r.validate(&json!({ "inputs": ["x", "y"] })).is_empty());
        assert!(!r.validate(&json!({ "inputs": ["x"] })).is_empty());
        assert!(!r.validate(&json!({ "inputs": ["x", "x"] })).is_empty());
    }

    #[test]
    fn merge_and_join_are_distinct_registry_entries() {
        assert_eq!(MergeRule::new().spec().id, "merge");
    }
}
