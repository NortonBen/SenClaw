//! `arithmetic` — compute new fields from expressions over the payload.
//!
//! The Go original evaluated every operator against the *original* snapshot, so
//! a second formula could never build on the first one. Here the working view is
//! updated after each assignment, which is what a list of formulas implies.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::daq;
use crate::engine::spec::{Category, Rule, RuleSpec, RunCtx};
use crate::engine::types::{Message, Outcome};
use crate::expr;

pub struct ArithmeticRule {
    spec: RuleSpec,
}

pub fn rule() -> Arc<dyn Rule> {
    Arc::new(ArithmeticRule::new())
}

/// The `(target, expr)` pairs in the order they will run.
///
/// New form is an ordered array `[{ "target": .., "expr": .. }]`, so the run
/// order is exactly the row order the user typed — independent of `serde_json`'s
/// map ordering (this crate does not enable `preserve_order`, so an object's keys
/// would otherwise iterate alphabetically). The legacy object form
/// `{ target: expr }` is still accepted for back-compat, ordered by key name.
/// Returns `None` when `operators` is missing or is neither array nor object.
fn operator_pairs(config: &Value) -> Option<Vec<(String, Value)>> {
    let ops = config.get("operators")?;
    if let Some(arr) = ops.as_array() {
        Some(
            arr.iter()
                .map(|row| {
                    let target = row
                        .get("target")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let expr = row.get("expr").cloned().unwrap_or(Value::Null);
                    (target, expr)
                })
                .collect(),
        )
    } else {
        ops.as_object()
            .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
    }
}

impl ArithmeticRule {
    fn new() -> Self {
        let spec = RuleSpec::builder("arithmetic", "Tính toán", Category::Transform)
            .desc("Tính một hoặc nhiều biểu thức rồi ghi kết quả vào các field của dữ liệu.")
            .icon("🔢")
            .color("#fa8c16")
            .schema(json!({
                "type": "object",
                "required": ["operators"],
                "properties": {
                    "operators": {
                        "type": "array",
                        "title": "Danh sách phép tính",
                        "ui": "table",
                        "default": [],
                        "description": "Mỗi dòng: field đích + biểu thức. Các dòng chạy lần lượt \
                                        TỪ TRÊN XUỐNG đúng thứ tự trên form, dòng sau dùng được \
                                        kết quả dòng trên.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "target": {
                                    "type": "string",
                                    "title": "Field đích",
                                    "placeholder": "delta",
                                    "description": "Hỗ trợ đường dẫn lồng nhau `stats.avg`, `list[0]`."
                                },
                                "expr": {
                                    "type": "string",
                                    "title": "Biểu thức",
                                    "placeholder": "max_temp - min_temp"
                                }
                            }
                        }
                    }
                }
            }))
            .doc(
                "Ghi kết quả biểu thức vào dữ liệu.\n\n\
                 Ví dụ (các dòng chạy lần lượt từ trên xuống):\n\n\
                 ```json\n\
                 {\n  \"operators\": [\n    \
                 { \"target\": \"delta\",  \"expr\": \"max_temp - min_temp\" },\n    \
                 { \"target\": \"is_hot\", \"expr\": \"delta > 10\" }\n  ]\n}\n\
                 ```\n\n\
                 - Field đích nhận đường dẫn lồng nhau: `stats.avg`, `list[0]`.\n\
                 - Biểu thức đọc field trực tiếp (`temperature`), metadata qua \
                   `sFromObj(meta_data, 'device_id')`.\n\
                 - Các dòng chạy **đúng thứ tự trong danh sách**, nên dòng sau thấy \
                   kết quả dòng trước và có thể xâu chuỗi — không phụ thuộc tên field.\n\
                 - Vẫn nhận dạng object cũ `{ \"delta\": \"...\" }` cho tương thích ngược, \
                   nhưng object thì thứ tự chạy theo tên khoá; dùng danh sách để chắc chắn.\n\
                 - Bất kỳ biểu thức nào lỗi (cú pháp, chia 0, không phải số) đều đẩy cả \
                   message ra cổng `error` kèm tên field — không ghi kết quả nửa vời.",
            )
            .build();
        Self { spec }
    }
}

#[async_trait]
impl Rule for ArithmeticRule {
    fn spec(&self) -> &RuleSpec {
        &self.spec
    }

    fn validate(&self, config: &Value) -> Vec<String> {
        let mut out = Vec::new();
        let Some(ops) = operator_pairs(config) else {
            out.push("Thiếu danh sách phép tính (`operators`).".to_string());
            return out;
        };
        if ops.is_empty() {
            out.push("Chưa có phép tính nào.".to_string());
        }
        for (target, raw) in &ops {
            if target.trim().is_empty() {
                out.push("Có phép tính không có field đích.".to_string());
                continue;
            }
            let Some(source) = raw.as_str() else {
                out.push(format!("Biểu thức của `{target}` phải là chuỗi."));
                continue;
            };
            if source.trim().is_empty() {
                out.push(format!("Biểu thức của `{target}` đang rỗng."));
                continue;
            }
            // Against an empty view every identifier is Null, so only syntax
            // errors are real config errors here.
            if let Err(e) = expr::eval(source, &json!({})) {
                if !e.contains("không phải số") && !e.contains("không so sánh") {
                    out.push(format!("Biểu thức của `{target}` không hợp lệ: {e}"));
                }
            }
        }
        out
    }

    async fn handle(&self, ctx: &RunCtx, msg: Message) -> Outcome {
        let Some(ops) = operator_pairs(&ctx.config) else {
            return ctx.fail_config("Thiếu danh sách phép tính (`operators`).");
        };
        if ops.is_empty() {
            return ctx.fail_config("Chưa có phép tính nào.");
        }

        let mut data = msg.data;
        let mut view = daq::view(&data, &msg.meta);
        for (target, raw) in &ops {
            if target.trim().is_empty() {
                return ctx.fail_config("Có phép tính không có field đích.");
            }
            let Some(source) = raw.as_str() else {
                return ctx.fail_config(format!("Biểu thức của `{target}` phải là chuỗi."));
            };
            let value = match expr::eval(source, &view) {
                Ok(v) => v,
                Err(e) => {
                    return ctx.fail_runtime(format!("Phép tính `{target}` = `{source}`: {e}"))
                }
            };
            daq::set(&mut data, target, value.clone());
            // Keep the view in step so the next formula can read this result.
            daq::set(&mut view, target, value);
        }
        Outcome::out(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{ctx, failure, msg, msg_with_meta, one};

    #[tokio::test]
    async fn writes_the_expression_result_into_the_payload() {
        let r = ArithmeticRule::new();
        let c = ctx("arithmetic", json!({ "operators": { "c": "a+b" } }));
        let (port, data) = one(r.handle(&c, msg(json!({ "a": 10, "b": 20 }))).await);
        assert_eq!(port, "out");
        assert_eq!(data["c"], 30.0);
        assert_eq!(data["a"], 10, "dữ liệu gốc phải giữ nguyên");
    }

    /// The whole point of running the operators in sequence.
    #[tokio::test]
    async fn a_later_formula_sees_an_earlier_result() {
        let r = ArithmeticRule::new();
        let c = ctx(
            "arithmetic",
            json!({ "operators": { "s1_tong": "a+b", "s2_gap_doi": "s1_tong*2" } }),
        );
        let (_, data) = one(r.handle(&c, msg(json!({ "a": 1, "b": 2 }))).await);
        assert_eq!(data["s1_tong"], 3.0);
        assert_eq!(data["s2_gap_doi"], 6.0);
    }

    /// The ordered array form runs top-to-bottom no matter how the targets are
    /// named — the whole reason the array form exists.
    #[tokio::test]
    async fn operators_run_in_list_order_regardless_of_name() {
        let r = ArithmeticRule::new();
        // `tong` must run before `avg` even though "avg" < "tong" alphabetically.
        let c = ctx(
            "arithmetic",
            json!({ "operators": [
                { "target": "tong", "expr": "a + b" },
                { "target": "avg",  "expr": "tong / 2" }
            ] }),
        );
        let (_, data) = one(r.handle(&c, msg(json!({ "a": 1, "b": 3 }))).await);
        assert_eq!(data["tong"], 4.0);
        assert_eq!(data["avg"], 2.0);
    }

    /// Legacy object form: still supported, and a later operator sees an earlier
    /// result (here the keys already sort in the intended order).
    #[tokio::test]
    async fn a_later_operator_sees_an_earlier_result_in_the_object_form() {
        let r = ArithmeticRule::new();
        let c = ctx(
            "arithmetic",
            json!({ "operators": { "c": "a+b", "d": "c*2" } }),
        );
        let (_, data) = one(r.handle(&c, msg(json!({ "a": 1, "b": 2 }))).await);
        assert_eq!(data["c"], 3.0);
        assert_eq!(data["d"], 6.0);
    }

    #[tokio::test]
    async fn nested_targets_are_created_on_the_way() {
        let r = ArithmeticRule::new();
        let c = ctx(
            "arithmetic",
            json!({ "operators": { "stats.avg": "(a+b)/2" } }),
        );
        let (_, data) = one(r.handle(&c, msg(json!({ "a": 4, "b": 6 }))).await);
        assert_eq!(data["stats"]["avg"], 5.0);
    }

    #[tokio::test]
    async fn metadata_is_reachable_from_a_formula() {
        let r = ArithmeticRule::new();
        let c = ctx(
            "arithmetic",
            json!({ "operators": { "dev": "sFromObj(meta_data, 'device_id')" } }),
        );
        let out = r
            .handle(&c, msg_with_meta(json!({}), json!({ "device_id": "d7" })))
            .await;
        assert_eq!(one(out).1["dev"], "d7");
    }

    #[tokio::test]
    async fn a_broken_expression_fails_with_the_field_name() {
        let r = ArithmeticRule::new();
        let c = ctx("arithmetic", json!({ "operators": { "c": "a +" } }));
        let err = failure(r.handle(&c, msg(json!({ "a": 1 }))).await);
        assert!(err.contains("`c`"), "{err}");
    }

    #[tokio::test]
    async fn division_by_zero_is_an_error_not_a_null() {
        let r = ArithmeticRule::new();
        let c = ctx("arithmetic", json!({ "operators": { "c": "a/0" } }));
        let err = failure(r.handle(&c, msg(json!({ "a": 1 }))).await);
        assert!(err.contains("chia cho 0"), "{err}");
    }

    #[tokio::test]
    async fn missing_config_fails_readably() {
        let r = ArithmeticRule::new();
        let c = ctx("arithmetic", json!({}));
        let err = failure(r.handle(&c, msg(json!({}))).await);
        assert!(err.contains("operators"), "{err}");
    }

    #[test]
    fn validate_catches_syntax_errors_and_empty_config() {
        let r = ArithmeticRule::new();
        assert!(r
            .validate(&json!({ "operators": { "c": "a + b" } }))
            .is_empty());
        assert!(!r
            .validate(&json!({ "operators": { "c": "a +" } }))
            .is_empty());
        assert!(!r.validate(&json!({ "operators": {} })).is_empty());
        assert!(!r.validate(&json!({})).is_empty());
        assert!(!r.validate(&json!({ "operators": { "c": 5 } })).is_empty());
        // Array form is validated the same way.
        assert!(r
            .validate(&json!({ "operators": [{ "target": "c", "expr": "a + b" }] }))
            .is_empty());
        assert!(!r
            .validate(&json!({ "operators": [{ "target": "c", "expr": "a +" }] }))
            .is_empty());
        assert!(!r.validate(&json!({ "operators": [] })).is_empty());
    }
}
