//! `project` — build the outgoing payload field by field.
//!
//! Copy fields, pin constants, or compute expressions. `recreate` decides
//! whether you are shaping a brand-new object or patching the incoming one.
//!
//! The Go rule declared a `set_string` case and then never handled it, so every
//! string constant silently landed as `null`; it is a first-class type here.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::daq;
use crate::engine::spec::{Category, Rule, RuleSpec, RunCtx};
use crate::engine::types::{Message, Outcome};
use crate::expr;

const TYPES: [&str; 6] = [
    "assign",
    "set_string",
    "set_number",
    "set_float",
    "set_bool",
    "expr",
];

pub struct ProjectRule {
    spec: RuleSpec,
}

pub fn rule() -> Arc<dyn Rule> {
    Arc::new(ProjectRule::new())
}

impl ProjectRule {
    fn new() -> Self {
        let spec = RuleSpec::builder("project", "Dựng dữ liệu", Category::Transform)
            .desc("Chọn, đổi tên, gán hằng hoặc tính biểu thức để dựng payload đi tiếp.")
            .icon("🎯")
            .color("#096dd9")
            .schema(json!({
                "type": "object",
                "required": ["fields"],
                "properties": {
                    "recreate": {
                        "type": "boolean",
                        "title": "Dựng mới hoàn toàn",
                        "default": false,
                        "description": "Bật: bắt đầu từ object rỗng, chỉ giữ những field khai báo bên dưới. Tắt: giữ nguyên payload rồi thêm/ghi đè."
                    },
                    "fields": {
                        "type": "array",
                        "title": "Danh sách field",
                        "ui": "table",
                        "default": [],
                        "items": {
                            "type": "object",
                            "properties": {
                                "key": {
                                    "type": "string",
                                    "title": "Field đích",
                                    "description": "Hỗ trợ đường dẫn lồng nhau, vd `user.ten`."
                                },
                                "type": {
                                    "type": "string",
                                    "title": "Cách lấy giá trị",
                                    "ui": "select",
                                    "enum": ["assign", "set_string", "set_number", "set_float", "set_bool", "expr"],
                                    "default": "assign",
                                    "description": "`assign` = chép từ đường dẫn; `set_*` = hằng; `expr` = biểu thức."
                                },
                                "value": {
                                    "type": "string",
                                    "title": "Giá trị / đường dẫn / biểu thức",
                                    "description": "Nghĩa của ô này phụ thuộc cột `type`."
                                }
                            }
                        }
                    }
                }
            }))
            .doc(
                "Dựng payload đi tiếp theo đúng hình dạng bạn muốn.\n\n\
                 ```json\n\
                 {\n  \"recreate\": true,\n  \"fields\": [\n    \
                 { \"key\": \"ten\",   \"type\": \"assign\",     \"value\": \"user.name\" },\n    \
                 { \"key\": \"nguon\", \"type\": \"set_string\", \"value\": \"cam-bien\" },\n    \
                 { \"key\": \"tong\",  \"type\": \"expr\",       \"value\": \"a + b\" }\n  ]\n}\n\
                 ```\n\n\
                 | `type` | Ý nghĩa cột `value` |\n\
                 |---|---|\n\
                 | `assign` | đường dẫn đọc từ payload **gốc** |\n\
                 | `set_string` | hằng chuỗi |\n\
                 | `set_number` | hằng số nguyên (cắt phần thập phân) |\n\
                 | `set_float` | hằng số thực |\n\
                 | `set_bool` | `true`/`false` (nhận cả `1`/`0`, `yes`/`no`) |\n\
                 | `expr` | biểu thức, giống node Điều kiện/Tính toán |\n\n\
                 - `recreate = true` là cách gọn nhất để **bỏ bớt** field: cái gì không \
                   khai báo thì không đi tiếp.\n\
                 - `assign` luôn đọc từ payload **vào**, không đọc từ field vừa dựng — \
                   nhờ vậy đổi chỗ hai field (`a`↔`b`) cho ra kết quả đúng.\n\
                 - Đường dẫn `assign` không tồn tại → field nhận `null` (không phải lỗi).\n\
                 - Hằng số ghi sai (vd `set_number` = `\"abc\"`) → cổng `error` kèm tên field.",
            )
            .build();
        Self { spec }
    }
}

#[async_trait]
impl Rule for ProjectRule {
    fn spec(&self) -> &RuleSpec {
        &self.spec
    }

    fn validate(&self, config: &Value) -> Vec<String> {
        let mut out = Vec::new();
        let Some(fields) = config.get("fields").and_then(|v| v.as_array()) else {
            out.push("Thiếu danh sách field (`fields`).".to_string());
            return out;
        };
        if fields.is_empty() {
            out.push("Chưa khai báo field nào.".to_string());
        }
        for (i, f) in fields.iter().enumerate() {
            let row = i + 1;
            let key = f.get("key").and_then(|v| v.as_str()).unwrap_or("").trim();
            if key.is_empty() {
                out.push(format!("Dòng {row}: thiếu field đích (`key`)."));
            }
            let ty = f
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("assign")
                .trim();
            if !TYPES.contains(&ty) {
                out.push(format!(
                    "Dòng {row}: cách lấy giá trị `{ty}` không hợp lệ (chỉ nhận {}).",
                    TYPES.join(", ")
                ));
                continue;
            }
            let raw = text(f.get("value"));
            if raw.trim().is_empty() {
                // An empty `assign` path would copy the whole payload into the
                // field; the other types need a value too.
                let what = if ty == "assign" {
                    "đường dẫn nguồn"
                } else {
                    "giá trị"
                };
                out.push(format!("Dòng {row} (`{key}`): thiếu {what}."));
            }
            if ty == "expr" {
                if let Err(e) = expr::eval(&raw, &json!({})) {
                    if !e.contains("không phải số") && !e.contains("không so sánh") {
                        out.push(format!("Dòng {row} (`{key}`): biểu thức không hợp lệ: {e}"));
                    }
                }
            }
        }
        out
    }

    async fn handle(&self, ctx: &RunCtx, msg: Message) -> Outcome {
        let Some(fields) = ctx.cfg("fields").and_then(|v| v.as_array()) else {
            return ctx.fail_config("Thiếu danh sách field (`fields`).");
        };
        let recreate = ctx.cfg_bool("recreate", false);
        let view = daq::view(&msg.data, &msg.meta);
        let mut out = if recreate {
            json!({})
        } else {
            msg.data.clone()
        };

        for f in fields {
            let key = f.get("key").and_then(|v| v.as_str()).unwrap_or("").trim();
            if key.is_empty() {
                return ctx.fail_config("Có dòng thiếu field đích (`key`).");
            }
            let ty = f
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("assign")
                .trim();
            let raw = text(f.get("value"));

            let value = match ty {
                // Always the ORIGINAL payload: swapping two fields must work.
                "assign" => {
                    let path = raw.trim();
                    // An empty path reads the whole payload; refuse it rather than
                    // nest the payload inside itself.
                    if path.is_empty() {
                        return ctx.fail_config(format!(
                            "Field `{key}`: `assign` thiếu đường dẫn nguồn."
                        ));
                    }
                    daq::get(&msg.data, path).unwrap_or(Value::Null)
                }
                "set_string" => Value::String(raw),
                "set_number" => match number(&raw) {
                    Ok(n) => json!(n.trunc() as i64),
                    Err(e) => return ctx.fail_config(format!("Field `{key}`: {e}")),
                },
                "set_float" => match number(&raw) {
                    Ok(n) => json!(n),
                    Err(e) => return ctx.fail_config(format!("Field `{key}`: {e}")),
                },
                "set_bool" => match raw.trim().to_ascii_lowercase().as_str() {
                    "true" | "1" | "yes" => json!(true),
                    "false" | "0" | "no" => json!(false),
                    other => {
                        return ctx.fail_config(format!(
                            "Field `{key}`: `{other}` không phải true/false."
                        ))
                    }
                },
                "expr" => match expr::eval(&raw, &view) {
                    Ok(v) => v,
                    Err(e) => return ctx.fail_runtime(format!("Field `{key}`: {e}")),
                },
                other => {
                    return ctx.fail_config(format!(
                        "Field `{key}`: cách lấy giá trị `{other}` không hợp lệ."
                    ))
                }
            };
            daq::set(&mut out, key, value);
        }
        Outcome::out(out)
    }
}

fn text(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

fn number(raw: &str) -> Result<f64, String> {
    let n: f64 = raw
        .trim()
        .parse()
        .map_err(|_| format!("`{}` không phải số.", raw.trim()))?;
    if !n.is_finite() {
        return Err(format!("`{}` không phải số hữu hạn.", raw.trim()));
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{ctx, failure, msg, msg_with_meta, one};

    #[tokio::test]
    async fn recreate_keeps_only_the_projected_fields() {
        let r = ProjectRule::new();
        let c = ctx(
            "project",
            json!({
                "recreate": true,
                "fields": [{ "key": "x", "type": "assign", "value": "a" }]
            }),
        );
        let (port, data) = one(r.handle(&c, msg(json!({ "a": 1, "b": 2 }))).await);
        assert_eq!(port, "out");
        assert_eq!(data, json!({ "x": 1 }));
    }

    #[tokio::test]
    async fn without_recreate_the_payload_is_patched() {
        let r = ProjectRule::new();
        let c = ctx(
            "project",
            json!({ "fields": [{ "key": "x", "type": "assign", "value": "a" }] }),
        );
        let (_, data) = one(r.handle(&c, msg(json!({ "a": 1, "b": 2 }))).await);
        assert_eq!(data, json!({ "a": 1, "b": 2, "x": 1 }));
    }

    #[tokio::test]
    async fn assign_walks_nested_paths_in_both_directions() {
        let r = ProjectRule::new();
        let c = ctx(
            "project",
            json!({
                "recreate": true,
                "fields": [
                    { "key": "kh.ten", "type": "assign", "value": "user.profile.name" },
                    { "key": "dau", "type": "assign", "value": "list[0]" },
                    { "key": "thieu", "type": "assign", "value": "khong.co" }
                ]
            }),
        );
        let (_, data) = one(r
            .handle(
                &c,
                msg(json!({ "user": { "profile": { "name": "Lan" } }, "list": [7, 8] })),
            )
            .await);
        assert_eq!(data["kh"]["ten"], "Lan");
        assert_eq!(data["dau"], 7);
        assert_eq!(data["thieu"], Value::Null);
    }

    /// `assign` reads the original payload, so a swap is not self-clobbering.
    #[tokio::test]
    async fn assign_never_reads_a_field_this_node_just_wrote() {
        let r = ProjectRule::new();
        let c = ctx(
            "project",
            json!({
                "fields": [
                    { "key": "a", "type": "assign", "value": "b" },
                    { "key": "b", "type": "assign", "value": "a" }
                ]
            }),
        );
        let (_, data) = one(r.handle(&c, msg(json!({ "a": 1, "b": 2 }))).await);
        assert_eq!(data["a"], 2);
        assert_eq!(data["b"], 1);
    }

    #[tokio::test]
    async fn every_constant_type_lands_with_the_right_json_type() {
        let r = ProjectRule::new();
        let c = ctx(
            "project",
            json!({
                "recreate": true,
                "fields": [
                    { "key": "s", "type": "set_string", "value": "cam-bien" },
                    { "key": "i", "type": "set_number", "value": "42" },
                    { "key": "f", "type": "set_float",  "value": "3.5" },
                    { "key": "t", "type": "set_bool",   "value": "true" },
                    { "key": "n", "type": "set_bool",   "value": "0" }
                ]
            }),
        );
        let (_, data) = one(r.handle(&c, msg(json!({}))).await);
        assert_eq!(data["s"], "cam-bien");
        assert!(
            data["s"].is_string(),
            "set_string phải ra chuỗi, không phải null"
        );
        assert_eq!(data["i"], 42);
        assert!(data["i"].is_i64());
        assert_eq!(data["f"], 3.5);
        assert_eq!(data["t"], true);
        assert_eq!(data["n"], false);
    }

    #[tokio::test]
    async fn expr_computes_over_the_payload_and_metadata() {
        let r = ProjectRule::new();
        let c = ctx(
            "project",
            json!({
                "recreate": true,
                "fields": [
                    { "key": "tong", "type": "expr", "value": "a + b" },
                    { "key": "dev", "type": "expr", "value": "sFromObj(meta_data, 'device_id')" }
                ]
            }),
        );
        let out = r
            .handle(
                &c,
                msg_with_meta(json!({ "a": 1, "b": 2 }), json!({ "device_id": "d9" })),
            )
            .await;
        let data = one(out).1;
        assert_eq!(data["tong"], 3.0);
        assert_eq!(data["dev"], "d9");
    }

    #[tokio::test]
    async fn a_bad_numeric_constant_fails_with_the_field_name() {
        let r = ProjectRule::new();
        let c = ctx(
            "project",
            json!({ "fields": [{ "key": "n", "type": "set_number", "value": "abc" }] }),
        );
        let err = failure(r.handle(&c, msg(json!({}))).await);
        assert!(err.contains("`n`") && err.contains("abc"), "{err}");
    }

    #[tokio::test]
    async fn a_bad_boolean_constant_fails() {
        let r = ProjectRule::new();
        let c = ctx(
            "project",
            json!({ "fields": [{ "key": "b", "type": "set_bool", "value": "co le" }] }),
        );
        assert!(failure(r.handle(&c, msg(json!({}))).await).contains("true/false"));
    }

    #[tokio::test]
    async fn an_unknown_type_fails_rather_than_writing_null() {
        let r = ProjectRule::new();
        let c = ctx(
            "project",
            json!({ "fields": [{ "key": "x", "type": "set_bytes", "value": "1" }] }),
        );
        assert!(failure(r.handle(&c, msg(json!({}))).await).contains("set_bytes"));
    }

    /// An empty `assign` path used to copy the whole payload into the field.
    #[tokio::test]
    async fn assign_with_an_empty_path_fails_instead_of_copying_the_payload() {
        let r = ProjectRule::new();
        let c = ctx(
            "project",
            json!({ "fields": [{ "key": "ghi_chu", "type": "assign", "value": "" }] }),
        );
        let err = failure(r.handle(&c, msg(json!({ "a": 1, "b": 2 }))).await);
        assert!(
            err.contains("`ghi_chu`") && err.contains("đường dẫn"),
            "{err}"
        );
    }

    #[test]
    fn validate_catches_bad_types_missing_keys_and_broken_expressions() {
        let r = ProjectRule::new();
        assert!(r
            .validate(&json!({ "fields": [{ "key": "a", "type": "assign", "value": "b" }] }))
            .is_empty());
        assert!(!r
            .validate(&json!({ "fields": [{ "key": "", "type": "assign", "value": "b" }] }))
            .is_empty());
        assert!(!r
            .validate(&json!({ "fields": [{ "key": "a", "type": "set_bytes", "value": "1" }] }))
            .is_empty());
        assert!(!r
            .validate(&json!({ "fields": [{ "key": "a", "type": "expr", "value": "1 +" }] }))
            .is_empty());
        // An `assign` row with a blank path is now rejected.
        assert!(!r
            .validate(&json!({ "fields": [{ "key": "a", "type": "assign", "value": "" }] }))
            .is_empty());
        assert!(!r.validate(&json!({})).is_empty());
    }
}
