//! `switch` — route on the value of one field, one output port per case.
//!
//! The Go rule was broken end to end: it read the case list under a field name
//! nothing wrote, treated each case as a bare string *and* as an object in two
//! places, compared the case key against the payload value instead of the case
//! value, and never assigned the matched branch — so every message fell through.
//! This is the intent, implemented once: read `key`, walk `cases` in array
//! order, emit on the first match, otherwise on `default`.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::daq;
use crate::engine::spec::{Category, PortSpec, Rule, RuleSpec, RunCtx};
use crate::engine::types::{Message, Outcome, PORT_ERROR, PORT_IN};

/// Where a message goes when no case matched.
const PORT_DEFAULT: &str = "default";

pub struct SwitchRule {
    spec: RuleSpec,
}

pub fn rule() -> Arc<dyn Rule> {
    Arc::new(SwitchRule::new())
}

struct Case {
    value: Value,
    port: String,
}

/// Cases in declaration order. Anything malformed keeps its slot so the port
/// list the UI draws always lines up with the rows the user typed.
fn cases(config: &Value) -> Vec<Case> {
    config
        .get("cases")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .enumerate()
                .map(|(i, c)| {
                    let value = c.get("value").cloned().unwrap_or(Value::Null);
                    let port = c
                        .get("port")
                        .and_then(|p| p.as_str())
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| sanitize(&value, i));
                    Case { value, port }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Port ids end up as edge keys and DOM handle ids, so keep them to letters,
/// digits, `_` and `-`.
fn sanitize(value: &Value, index: usize) -> String {
    let cleaned: String = text(value)
        .trim()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let cleaned = cleaned.trim_matches('_');
    if cleaned.is_empty() {
        format!("case{}", index + 1)
    } else {
        cleaned.to_string()
    }
}

fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn matches(actual: &Value, expected: &Value, match_type: &str) -> bool {
    match match_type {
        "number" => match (number(actual), number(expected)) {
            (Some(a), Some(b)) => a == b,
            _ => false,
        },
        "string" => text(actual) == text(expected),
        // `auto`: numeric comparison when both sides look like numbers, so the
        // string "30" typed in the form matches the number 30 on the wire.
        _ => match (number(actual), number(expected)) {
            (Some(a), Some(b)) => a == b,
            _ => text(actual) == text(expected),
        },
    }
}

impl SwitchRule {
    fn new() -> Self {
        let spec = RuleSpec::builder("switch", "Rẽ nhiều nhánh", Category::Logic)
            .desc("So khớp giá trị của một field với từng case và đi ra cổng tương ứng.")
            .icon("🔀")
            .color("#eb2f96")
            // Cases are added by `dynamic_outputs`; only `error` is fixed.
            .outputs(vec![])
            .schema(json!({
                "type": "object",
                "required": ["key", "cases"],
                "properties": {
                    "key": {
                        "type": "string",
                        "title": "Field so khớp",
                        "placeholder": "status",
                        "description": "Đường dẫn tới giá trị cần so, vd `status`, `device.mode`, `list[0]`."
                    },
                    "matchType": {
                        "type": "string",
                        "title": "Kiểu so sánh",
                        "ui": "select",
                        "enum": ["auto", "string", "number"],
                        "default": "auto",
                        "description": "`auto` so số nếu cả hai bên đều là số, ngược lại so chuỗi."
                    },
                    "cases": {
                        "type": "array",
                        "title": "Danh sách case",
                        "ui": "table",
                        "default": [],
                        "items": {
                            "type": "object",
                            "properties": {
                                "value": {
                                    "type": "string",
                                    "title": "Giá trị",
                                    "description": "Giá trị cần khớp."
                                },
                                "port": {
                                    "type": "string",
                                    "title": "Tên cổng ra",
                                    "description": "Bỏ trống sẽ lấy theo giá trị đã chuẩn hoá."
                                }
                            }
                        }
                    }
                }
            }))
            .doc(
                "Mỗi case là một cổng ra riêng trên node.\n\n\
                 ```json\n\
                 {\n  \"key\": \"status\",\n  \"matchType\": \"auto\",\n  \"cases\": [\n    \
                 { \"value\": \"on\",  \"port\": \"bat\" },\n    \
                 { \"value\": \"off\", \"port\": \"tat\" }\n  ]\n}\n\
                 ```\n\n\
                 - Các case được duyệt **theo đúng thứ tự trong mảng**; case đầu tiên khớp \
                   sẽ thắng, nên hãy đặt case hẹp lên trước.\n\
                 - Không case nào khớp → message đi ra cổng `default`.\n\
                 - Không tìm thấy `key` trong dữ liệu → đi ra cổng `error` (khác với \
                   `default`: thiếu field là lỗi dữ liệu, không phải một nhánh hợp lệ).\n\
                 - Dữ liệu đi ra **không bị sửa**; node chỉ chọn đường.\n\
                 - Tên cổng bỏ trống sẽ lấy theo giá trị (chỉ giữ chữ, số, `_`, `-`).",
            )
            .build();
        Self { spec }
    }
}

#[async_trait]
impl Rule for SwitchRule {
    fn spec(&self) -> &RuleSpec {
        &self.spec
    }

    fn dynamic_outputs(&self, config: &Value) -> Vec<PortSpec> {
        let mut out: Vec<PortSpec> = cases(config)
            .into_iter()
            .map(|c| {
                let label = {
                    let t = text(&c.value);
                    if t.is_empty() {
                        c.port.clone()
                    } else {
                        t
                    }
                };
                PortSpec::new(&c.port, &label)
                    .one()
                    .color("#52c41a")
                    .desc(&format!("Khớp giá trị `{}`", text(&c.value)))
            })
            .collect();
        out.push(
            PortSpec::new(PORT_DEFAULT, "default")
                .one()
                .color("#faad14")
                .desc("Không case nào khớp."),
        );
        out
    }

    fn validate(&self, config: &Value) -> Vec<String> {
        let mut out = Vec::new();
        match config.get("key").and_then(|v| v.as_str()) {
            None => out.push("Thiếu field so khớp (`key`).".to_string()),
            Some(k) if k.trim().is_empty() => out.push("Thiếu field so khớp (`key`).".to_string()),
            Some(_) => {}
        }

        let list = cases(config);
        if list.is_empty() {
            out.push("Chưa khai báo case nào.".to_string());
        }
        let mut seen: Vec<&str> = Vec::new();
        for c in &list {
            if matches!(c.port.as_str(), PORT_DEFAULT | PORT_ERROR | PORT_IN) {
                out.push(format!(
                    "Tên cổng `{}` là tên dành riêng, hãy đổi tên khác.",
                    c.port
                ));
            }
            if seen.contains(&c.port.as_str()) {
                out.push(format!(
                    "Trùng cổng `{}`: hai case cùng đi ra một chỗ nên không rẽ nhánh được.",
                    c.port
                ));
            } else {
                seen.push(c.port.as_str());
            }
        }

        if let Some(m) = config.get("matchType").and_then(|v| v.as_str()) {
            if !matches!(m, "auto" | "string" | "number") {
                out.push(format!(
                    "Kiểu so sánh `{m}` không hợp lệ (chỉ nhận auto/string/number)."
                ));
            }
        }
        out
    }

    async fn handle(&self, ctx: &RunCtx, msg: Message) -> Outcome {
        let Some(key) = ctx.cfg_str("key") else {
            return ctx.fail_config("Thiếu field so khớp (`key`).");
        };
        let list = cases(&ctx.config);
        if list.is_empty() {
            return ctx.fail_config("Chưa khai báo case nào.");
        }
        let Some(actual) = daq::get(&msg.data, &key) else {
            return ctx.fail_runtime(format!("Không tìm thấy `{key}` trong dữ liệu."));
        };

        let match_type = ctx.cfg_str_or("matchType", "auto");
        for c in &list {
            if matches(&actual, &c.value, &match_type) {
                return Outcome::port(&c.port, msg.data);
            }
        }
        Outcome::port(PORT_DEFAULT, msg.data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{ctx, failure, msg, one};

    fn cfg() -> Value {
        json!({
            "key": "status",
            "cases": [
                { "value": "on",  "port": "bat" },
                { "value": "off", "port": "tat" }
            ]
        })
    }

    #[tokio::test]
    async fn a_string_case_routes_to_its_port() {
        let r = SwitchRule::new();
        let c = ctx("switch", cfg());
        let (port, data) = one(r.handle(&c, msg(json!({ "status": "off" }))).await);
        assert_eq!(port, "tat");
        assert_eq!(data["status"], "off", "dữ liệu không bị sửa");
    }

    /// The form stores every case value as a string; the wire carries numbers.
    #[tokio::test]
    async fn auto_matches_a_numeric_string_against_a_number() {
        let r = SwitchRule::new();
        let c = ctx(
            "switch",
            json!({ "key": "t", "cases": [{ "value": "30", "port": "nguong" }] }),
        );
        let (port, _) = one(r.handle(&c, msg(json!({ "t": 30 }))).await);
        assert_eq!(port, "nguong");
    }

    #[tokio::test]
    async fn string_mode_refuses_the_numeric_coercion() {
        let r = SwitchRule::new();
        let c = ctx(
            "switch",
            json!({
                "key": "t",
                "matchType": "string",
                "cases": [{ "value": "30.0", "port": "nguong" }]
            }),
        );
        let (port, _) = one(r.handle(&c, msg(json!({ "t": 30 }))).await);
        assert_eq!(port, "default");
    }

    #[tokio::test]
    async fn the_first_matching_case_wins() {
        let r = SwitchRule::new();
        let c = ctx(
            "switch",
            json!({
                "key": "s",
                "cases": [
                    { "value": "x", "port": "dau" },
                    { "value": "x", "port": "sau" }
                ]
            }),
        );
        assert_eq!(one(r.handle(&c, msg(json!({ "s": "x" }))).await).0, "dau");
    }

    #[tokio::test]
    async fn no_match_falls_through_to_default() {
        let r = SwitchRule::new();
        let c = ctx("switch", cfg());
        let (port, _) = one(r.handle(&c, msg(json!({ "status": "unknown" }))).await);
        assert_eq!(port, PORT_DEFAULT);
    }

    #[tokio::test]
    async fn a_missing_key_is_an_error_not_the_default_branch() {
        let r = SwitchRule::new();
        let c = ctx("switch", cfg());
        let err = failure(r.handle(&c, msg(json!({ "other": 1 }))).await);
        assert!(err.contains("status"), "{err}");
    }

    #[tokio::test]
    async fn empty_config_fails() {
        let r = SwitchRule::new();
        assert!(!failure(r.handle(&ctx("switch", json!({})), msg(json!({}))).await).is_empty());
        let c = ctx("switch", json!({ "key": "a", "cases": [] }));
        let err = failure(r.handle(&c, msg(json!({ "a": 1 }))).await);
        assert!(err.contains("case"), "{err}");
    }

    #[test]
    fn dynamic_outputs_grow_one_port_per_case_plus_default() {
        let r = SwitchRule::new();
        let ports = r.dynamic_outputs(&cfg());
        let ids: Vec<&str> = ports.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["bat", "tat", PORT_DEFAULT]);
        assert!(ports
            .iter()
            .all(|p| p.arity == crate::engine::spec::PortArity::One));
        assert!(r.spec().has_output(PORT_ERROR));
    }

    #[test]
    fn a_missing_port_name_is_derived_from_the_value() {
        let r = SwitchRule::new();
        let ports = r.dynamic_outputs(&json!({
            "key": "s",
            "cases": [{ "value": "nóng quá!" }, { "value": "" }]
        }));
        let ids: Vec<&str> = ports.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["nóng_quá", "case2", PORT_DEFAULT]);
    }

    #[test]
    fn validate_rejects_duplicate_and_reserved_ports() {
        let r = SwitchRule::new();
        assert!(r.validate(&cfg()).is_empty());

        let dup = json!({
            "key": "s",
            "cases": [{ "value": "a", "port": "p" }, { "value": "b", "port": "p" }]
        });
        assert!(r.validate(&dup).iter().any(|e| e.contains("Trùng cổng")));

        let reserved = json!({ "key": "s", "cases": [{ "value": "a", "port": "error" }] });
        assert!(r
            .validate(&reserved)
            .iter()
            .any(|e| e.contains("dành riêng")));

        assert!(!r.validate(&json!({ "cases": [] })).is_empty());
        assert!(!r
            .validate(&json!({ "key": "s", "cases": [{ "value": "a" }], "matchType": "weird" }))
            .is_empty());
    }
}
