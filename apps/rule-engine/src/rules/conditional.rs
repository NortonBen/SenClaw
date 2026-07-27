//! `conditional` — route on a boolean expression.
//!
//! Reference implementation for every other rule in this directory: declare
//! ports in the spec, read config through `RunCtx`, return `Outcome`.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::daq;
use crate::engine::spec::{Category, PortSpec, Rule, RuleSpec, RunCtx};
use crate::engine::types::{Message, Outcome};
use crate::expr;

pub struct ConditionalRule {
    spec: RuleSpec,
}

pub fn rule() -> Arc<dyn Rule> {
    Arc::new(ConditionalRule::new())
}

impl ConditionalRule {
    fn new() -> Self {
        let spec = RuleSpec::builder("conditional", "Điều kiện", Category::Logic)
            .desc("Đánh giá một biểu thức boolean rồi rẽ nhánh true/false.")
            .icon("🔀")
            .color("#eb2f96")
            .outputs(vec![
                PortSpec::new("true", "true")
                    .one()
                    .color("#52c41a")
                    .desc("Biểu thức đúng"),
                PortSpec::new("false", "false")
                    .one()
                    .color("#f5222d")
                    .desc("Biểu thức sai"),
            ])
            .schema(json!({
                "type": "object",
                "required": ["expr"],
                "properties": {
                    "expr": {
                        "type": "string",
                        "title": "Biểu thức",
                        "ui": "textarea",
                        "placeholder": "temperature > 30 && status == 'on'",
                        "description": "Trả về true/false. Dùng tên field trực tiếp; meta nằm trong `meta_data`."
                    },
                    "setResultTo": {
                        "type": "string",
                        "title": "Ghi kết quả vào field",
                        "placeholder": "is_hot",
                        "description": "Tuỳ chọn: ghi true/false vào field này của dữ liệu đi tiếp."
                    }
                }
            }))
            .doc(
                "Rẽ nhánh theo biểu thức.\n\n\
                 - Toán tử: `+ - * / % **`, `== != <> < > <= >=`, `&& || !`, ba ngôi `? :`\n\
                 - Hàm: `strlen`, `len`, `abs`, `round`, `min`, `max`, `lower`, `upper`, \
                   `contains`, `startsWith`, `sFromObj(obj,'path')`, `nFromObj(obj,'path')`\n\
                 - Truy cập lồng nhau: `user.name`, `list[0]`\n\
                 - Metadata của message: `sFromObj(meta_data, 'device_id')`\n\n\
                 Biểu thức lỗi hoặc không trả boolean sẽ đi ra cổng `error`, \
                 không âm thầm rơi vào `false`.",
            )
            .build();
        Self { spec }
    }
}

#[async_trait]
impl Rule for ConditionalRule {
    fn spec(&self) -> &RuleSpec {
        &self.spec
    }

    fn validate(&self, config: &Value) -> Vec<String> {
        let mut out = Vec::new();
        match config.get("expr").and_then(|v| v.as_str()) {
            None | Some("") => out.push("Thiếu biểu thức điều kiện.".to_string()),
            // Parse only: a syntax error is a real problem now, while a missing
            // field is not — it just is not there yet at save time.
            Some(e) => {
                if let Err(err) = expr::parse(e) {
                    out.push(format!("Biểu thức không hợp lệ: {err}"));
                }
            }
        }
        out
    }

    async fn handle(&self, ctx: &RunCtx, msg: Message) -> Outcome {
        let Some(source) = ctx.cfg_str("expr") else {
            return ctx.fail_config("Thiếu biểu thức điều kiện.");
        };
        let view = daq::view(&msg.data, &msg.meta);
        let result = match expr::eval_bool(&source, &view) {
            Ok(b) => b,
            Err(e) => return ctx.fail_runtime(e),
        };

        let mut data = msg.data;
        if let Some(key) = ctx.cfg_str("setResultTo") {
            daq::set(&mut data, &key, json!(result));
        }
        Outcome::port(if result { "true" } else { "false" }, data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{ctx, failure, msg, msg_with_meta, one};

    #[tokio::test]
    async fn routes_to_true_when_the_expression_holds() {
        let r = ConditionalRule::new();
        let c = ctx("conditional", json!({ "expr": "temperature > 30" }));
        let (port, data) = one(r.handle(&c, msg(json!({ "temperature": 31.2 }))).await);
        assert_eq!(port, "true");
        assert_eq!(data["temperature"], 31.2);
    }

    #[tokio::test]
    async fn routes_to_false_otherwise() {
        let r = ConditionalRule::new();
        let c = ctx("conditional", json!({ "expr": "temperature > 30" }));
        let (port, _) = one(r.handle(&c, msg(json!({ "temperature": 12 }))).await);
        assert_eq!(port, "false");
    }

    #[tokio::test]
    async fn set_result_to_writes_the_boolean_into_the_payload() {
        let r = ConditionalRule::new();
        let c = ctx(
            "conditional",
            json!({ "expr": "a == b", "setResultTo": "same" }),
        );
        let (port, data) = one(r.handle(&c, msg(json!({ "a": 2, "b": 2 }))).await);
        assert_eq!(port, "true");
        assert_eq!(data["same"], true);
    }

    #[tokio::test]
    async fn metadata_is_reachable() {
        let r = ConditionalRule::new();
        let c = ctx(
            "conditional",
            json!({ "expr": "sFromObj(meta_data, 'device_id') == 'd1'" }),
        );
        let out = r
            .handle(&c, msg_with_meta(json!({}), json!({ "device_id": "d1" })))
            .await;
        assert_eq!(one(out).0, "true");
    }

    /// The Go rule swallowed a bad expression into the error branch with the
    /// detail lost; here it must be a real failure carrying the reason.
    #[tokio::test]
    async fn a_non_boolean_result_fails_instead_of_defaulting_to_false() {
        let r = ConditionalRule::new();
        let c = ctx("conditional", json!({ "expr": "a + b" }));
        let err = failure(r.handle(&c, msg(json!({ "a": 1, "b": 2 }))).await);
        assert!(err.contains("true/false"), "{err}");
    }

    #[tokio::test]
    async fn missing_expression_fails_with_a_readable_message() {
        let r = ConditionalRule::new();
        let c = ctx("conditional", json!({}));
        let err = failure(r.handle(&c, msg(json!({}))).await);
        assert!(err.contains("Thiếu biểu thức"), "{err}");
    }

    #[test]
    fn validate_catches_syntax_errors_at_save_time() {
        let r = ConditionalRule::new();
        assert!(!r.validate(&json!({ "expr": "a >" })).is_empty());
        assert!(r.validate(&json!({ "expr": "a > 3" })).is_empty());
        assert!(!r.validate(&json!({})).is_empty());
    }

    #[test]
    fn ports_are_exclusive_branches() {
        let r = ConditionalRule::new();
        let t = r.spec().output("true").unwrap();
        assert_eq!(t.arity, crate::engine::spec::PortArity::One);
        assert!(r.spec().has_output("error"));
    }
}
