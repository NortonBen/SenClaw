//! `fork` — split one message into several parallel branches.
//!
//! There is nothing to configure: the fan-out lives in the *edges*. The engine
//! deep-copies the payload once per edge on a `Many` port, so this rule only has
//! to hand the message straight back.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::engine::spec::{Category, PortSpec, Rule, RuleSpec, RunCtx};
use crate::engine::types::{Message, Outcome};

pub struct ForkRule {
    spec: RuleSpec,
}

pub fn rule() -> Arc<dyn Rule> {
    Arc::new(ForkRule::new())
}

impl ForkRule {
    fn new() -> Self {
        let spec = RuleSpec::builder("fork", "Chia nhánh", Category::Logic)
            .desc(
                "Gửi cùng một message ra mọi nhánh nối vào cổng out, mỗi nhánh một bản sao riêng.",
            )
            .icon("⑂")
            .color("#2f54eb")
            .outputs(vec![PortSpec::output().color("#2f54eb").desc(
                "Nối bao nhiêu cạnh cũng được; mỗi cạnh nhận một bản sao độc lập.",
            )])
            .schema(json!({ "type": "object", "properties": {} }))
            .doc(
                "Chạy nhiều nhánh song song từ cùng một dữ liệu.\n\n\
                 - Node này **không có cấu hình**. Số nhánh = số cạnh bạn kéo từ cổng `out`.\n\
                 - Mỗi cạnh nhận **một bản sao độc lập** của dữ liệu: nhánh này sửa payload \
                   không ảnh hưởng nhánh kia.\n\
                 - Các nhánh chạy độc lập, **không đảm bảo thứ tự** hoàn thành. Muốn chờ \
                   đủ rồi gộp lại thì nối chúng vào node `join` (hoặc `merge`).\n\n\
                 Thật ra bạn có thể kéo nhiều cạnh thẳng từ node trước đó và bỏ qua `fork`; \
                 node này tồn tại để chỗ rẽ nhánh hiện rõ trên sơ đồ.",
            )
            .build();
        Self { spec }
    }
}

#[async_trait]
impl Rule for ForkRule {
    fn spec(&self) -> &RuleSpec {
        &self.spec
    }

    async fn handle(&self, _ctx: &RunCtx, msg: Message) -> Outcome {
        Outcome::out(msg.data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::spec::PortArity;
    use crate::testkit::{ctx, msg, one};

    #[tokio::test]
    async fn the_payload_passes_through_untouched() {
        let r = ForkRule::new();
        let c = ctx("fork", json!({}));
        let input = json!({ "a": 1, "nested": { "b": [1, 2] } });
        let (port, data) = one(r.handle(&c, msg(input.clone())).await);
        assert_eq!(port, "out");
        assert_eq!(data, input);
    }

    #[tokio::test]
    async fn config_is_ignored_rather_than_rejected() {
        let r = ForkRule::new();
        let c = ctx("fork", json!({ "gi_do": 1 }));
        assert_eq!(one(r.handle(&c, msg(json!({ "x": 9 }))).await).1["x"], 9);
    }

    #[test]
    fn the_output_port_accepts_many_edges() {
        let r = ForkRule::new();
        assert_eq!(r.spec().output("out").unwrap().arity, PortArity::Many);
        assert!(r.spec().has_output("error"));
    }
}
