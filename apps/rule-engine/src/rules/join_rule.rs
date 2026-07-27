//! `join` — wait for one message on every input port, then continue once.
//!
//! The barrier itself lives in the engine (`engine/join.rs`): by the time this
//! rule runs, the parts have already been folded into a single payload keyed by
//! port name. The rule only declares the ports and passes the result on — which
//! is why it must not touch the data.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::engine::spec::{Category, PortSpec, Rule, RuleSpec, RunCtx};
use crate::engine::types::{Message, Outcome};

pub struct JoinRule {
    spec: RuleSpec,
}

pub fn rule() -> Arc<dyn Rule> {
    Arc::new(JoinRule::new())
}

/// Input port names from config, defaulting to two.
pub(super) fn input_names(config: &Value) -> Vec<String> {
    let listed: Vec<String> = config
        .get("inputs")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .map(|v| match v {
                    Value::String(s) => s.trim().to_string(),
                    other => other.to_string(),
                })
                .collect()
        })
        .unwrap_or_default();
    if listed.is_empty() {
        vec!["a".to_string(), "b".to_string()]
    } else {
        listed
    }
}

/// Shared by `join` and `merge`: both grow one input port per configured name.
pub(super) fn input_ports(config: &Value, hint: &str) -> Vec<PortSpec> {
    input_names(config)
        .into_iter()
        .map(|n| {
            PortSpec::new(&n, &n)
                .color("#722ed1")
                .desc(&format!("Nhánh `{n}`. {hint}"))
        })
        .collect()
}

/// Shared by `join` and `merge`.
pub(super) fn validate_inputs(config: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let names = input_names(config);
    if names.len() < 2 {
        out.push("Cần ít nhất 2 cổng vào — gộp một nhánh thì không có gì để chờ.".to_string());
    }
    let mut seen: Vec<&str> = Vec::new();
    for n in &names {
        if n.trim().is_empty() {
            out.push("Có tên cổng vào đang để trống.".to_string());
            continue;
        }
        // Names are used verbatim as edge keys (no sanitizing), so an interior
        // space would produce a port id the UI and the edge can't line up on.
        if n.chars().any(char::is_whitespace) {
            out.push(format!(
                "Tên cổng vào `{n}` có khoảng trắng. Chỉ dùng chữ, số, `_`, `-` \
                 (tên được dùng nguyên văn làm khoá cạnh)."
            ));
            continue;
        }
        if seen.contains(&n.as_str()) {
            out.push(format!("Trùng tên cổng vào `{n}`."));
        } else {
            seen.push(n.as_str());
        }
    }
    out
}

impl JoinRule {
    fn new() -> Self {
        let spec = RuleSpec::builder("join", "Chờ đủ (join)", Category::Logic)
            .desc("Chờ đủ message trên mọi cổng vào rồi phát một message gộp theo tên cổng.")
            .icon("⇥")
            .color("#722ed1")
            // Ports come from `dynamic_inputs`; the default single `in` is wrong here.
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
                        "description": "Mỗi tên tạo một cổng vào. Tên cổng cũng là khoá trong dữ liệu đi ra."
                    }
                }
            }))
            .doc(
                "Điểm hẹn của nhiều nhánh.\n\n\
                 ```json\n\
                 { \"inputs\": [\"thoi_tiet\", \"ton_kho\"] }\n\
                 ```\n\n\
                 Dữ liệu đi ra được gộp **theo tên cổng**:\n\n\
                 ```json\n\
                 { \"thoi_tiet\": { ... }, \"ton_kho\": { ... } }\n\
                 ```\n\n\
                 **Bắt buộc**: node phải bật `opts.join = \"all\"` thì rào chắn mới hoạt \
                 động. Giao diện tự đặt `opts.join = \"all\"` ngay khi bạn kéo node `join` \
                 ra canvas, nên dựng bằng UI thì không cần làm gì thêm. **Nếu tạo node qua \
                 MCP/API** (`rule_update_graph`) thì phải tự đặt `opts.join = \"all\"` trên \
                 node — mặc định là `\"any\"`, và để `\"any\"` thì mỗi message tới sẽ chạy \
                 node một lần và không có gì được chờ.\n\n\
                 - `opts.joinTimeoutMs` giới hạn thời gian chờ. Quá hạn mà chưa đủ nhánh \
                   thì phần đã nhận bị huỷ và ghi log — chuỗi không treo mãi.\n\
                 - `opts.corrKey` gộp theo một giá trị trong dữ liệu (vd `order_id`) thay \
                   vì theo lượt chạy; cần khi nhiều item chạy song song.\n\
                 - Một nhánh lỗi làm cả message gộp thành lỗi và đi ra cổng `error`.\n\
                 - Muốn trộn phẳng các nhánh vào cùng một object thì dùng node `merge`.",
            )
            .build();
        Self { spec }
    }
}

#[async_trait]
impl Rule for JoinRule {
    fn spec(&self) -> &RuleSpec {
        &self.spec
    }

    fn dynamic_inputs(&self, config: &Value) -> Vec<PortSpec> {
        input_ports(config, "Dữ liệu ra sẽ nằm dưới khoá cùng tên.")
    }

    fn validate(&self, config: &Value) -> Vec<String> {
        validate_inputs(config)
    }

    async fn handle(&self, _ctx: &RunCtx, msg: Message) -> Outcome {
        // Already combined by the engine barrier; re-shaping here would hide
        // which branch contributed what.
        Outcome::out(msg.data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{ctx, msg, one};

    #[tokio::test]
    async fn the_combined_payload_passes_through_unchanged() {
        let r = JoinRule::new();
        let c = ctx("join", json!({ "inputs": ["a", "b"] }));
        let combined = json!({ "a": { "x": 1 }, "b": { "y": 2 } });
        let (port, data) = one(r.handle(&c, msg(combined.clone())).await);
        assert_eq!(port, "out");
        assert_eq!(data, combined);
    }

    #[test]
    fn inputs_default_to_two_ports() {
        let r = JoinRule::new();
        let ids: Vec<String> = r
            .dynamic_inputs(&json!({}))
            .into_iter()
            .map(|p| p.id)
            .collect();
        assert_eq!(ids, vec!["a".to_string(), "b".to_string()]);
        assert!(r.spec().inputs.is_empty(), "cổng vào chỉ đến từ cấu hình");
    }

    #[test]
    fn one_port_per_configured_name() {
        let r = JoinRule::new();
        let ids: Vec<String> = r
            .dynamic_inputs(&json!({ "inputs": ["thoi_tiet", "ton_kho", "gia"] }))
            .into_iter()
            .map(|p| p.id)
            .collect();
        assert_eq!(ids, vec!["thoi_tiet", "ton_kho", "gia"]);
    }

    #[test]
    fn validate_rejects_too_few_duplicate_and_blank_names() {
        let r = JoinRule::new();
        assert!(r.validate(&json!({ "inputs": ["a", "b"] })).is_empty());
        assert!(!r.validate(&json!({ "inputs": ["a"] })).is_empty());
        assert!(r
            .validate(&json!({ "inputs": ["a", "a"] }))
            .iter()
            .any(|e| e.contains("Trùng")));
        assert!(r
            .validate(&json!({ "inputs": ["a", "  "] }))
            .iter()
            .any(|e| e.contains("để trống")));
        // Interior whitespace can't be a verbatim port id.
        assert!(r
            .validate(&json!({ "inputs": ["a", "kho hang"] }))
            .iter()
            .any(|e| e.contains("khoảng trắng")));
    }
}
