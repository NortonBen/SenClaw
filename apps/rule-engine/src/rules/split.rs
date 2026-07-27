//! `split` — turn one message carrying an array into one message per element.
//!
//! The `done` port fires once after the last element, so a chain can aggregate
//! or notify without guessing how many items were in flight.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::daq;
use crate::engine::spec::{Category, PortSpec, Rule, RuleSpec, RunCtx};
use crate::engine::types::{Emission, Message, Outcome};

const PORT_ITEM: &str = "item";
const PORT_DONE: &str = "done";

pub struct SplitRule {
    spec: RuleSpec,
}

pub fn rule() -> Arc<dyn Rule> {
    Arc::new(SplitRule::new())
}

impl SplitRule {
    fn new() -> Self {
        let spec = RuleSpec::builder("split", "Tách mảng", Category::Transform)
            .desc("Tách một mảng thành nhiều message, mỗi phần tử một message, rồi báo `done`.")
            .icon("✂️")
            .color("#13c2c2")
            .outputs(vec![
                PortSpec::new(PORT_ITEM, "item")
                    .color("#13c2c2")
                    .desc("Mỗi phần tử của mảng đi ra đây một lần."),
                PortSpec::new(PORT_DONE, "done")
                    .color("#52c41a")
                    .desc("Phát đúng một lần sau phần tử cuối, mang `{ count }`."),
            ])
            .schema(json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "title": "Đường dẫn tới mảng",
                        "placeholder": "items",
                        "description": "Bỏ trống nếu chính payload đã là một mảng. Hỗ trợ `a.b[0].list`."
                    },
                    "as": {
                        "type": "string",
                        "title": "Bọc phần tử vào field",
                        "placeholder": "item",
                        "description": "Bỏ trống sẽ phát thẳng phần tử làm payload. Điền tên sẽ phát `{ \"<tên>\": phần_tử }`."
                    },
                    "includeIndex": {
                        "type": "boolean",
                        "title": "Kèm số thứ tự",
                        "default": false,
                        "description": "Thêm field `_index` (bắt đầu từ 0) vào từng message phần tử."
                    }
                }
            }))
            .doc(
                "Biến `[a, b, c]` thành ba message chạy song song.\n\n\
                 ```json\n\
                 { \"path\": \"items\", \"as\": \"item\", \"includeIndex\": true }\n\
                 ```\n\n\
                 - Cổng `item` phát **N** message; cổng `done` phát thêm **1** message \
                   `{ \"count\": N }`. Mảng rỗng vẫn có `done` với `count = 0`.\n\
                 - Bỏ trống `as`: phần tử **chính là** payload. Nếu phần tử không phải \
                   object và bật `includeIndex`, nó được bọc thành `{ value, _index }` \
                   vì không thể gắn field vào một con số.\n\
                 - Giá trị tại `path` không phải mảng (hoặc không tồn tại) → cổng `error`.\n\
                 - Các message `item` chạy độc lập; muốn gom lại hãy dùng `done` làm \
                   tín hiệu kết thúc thay vì đoán số lượng.",
            )
            .build();
        Self { spec }
    }
}

#[async_trait]
impl Rule for SplitRule {
    fn spec(&self) -> &RuleSpec {
        &self.spec
    }

    async fn handle(&self, ctx: &RunCtx, msg: Message) -> Outcome {
        let path = ctx.cfg_str("path");
        let picked = match path.as_deref() {
            Some(p) => match daq::get(&msg.data, p) {
                Some(v) => v,
                None => return ctx.fail_runtime(format!("Không tìm thấy `{p}` trong dữ liệu.")),
            },
            None => msg.data,
        };
        let items = match picked {
            Value::Array(a) => a,
            other => {
                return ctx.fail_runtime(format!(
                    "`{}` không phải mảng (nhận kiểu `{}`).",
                    path.as_deref().unwrap_or("payload"),
                    kind_of(&other)
                ))
            }
        };

        let wrap = ctx.cfg_str("as");
        let with_index = ctx.cfg_bool("includeIndex", false);

        let mut out: Vec<Emission> = Vec::with_capacity(items.len() + 1);
        for (i, item) in items.into_iter().enumerate() {
            let data = match wrap.as_deref() {
                Some(field) => {
                    let mut o = json!({});
                    daq::set(&mut o, field, item);
                    if with_index {
                        daq::set(&mut o, "_index", json!(i));
                    }
                    o
                }
                None if !with_index => item,
                // A scalar has nowhere to hold `_index`, so give it a home.
                None if item.is_object() => {
                    let mut o = item;
                    daq::set(&mut o, "_index", json!(i));
                    o
                }
                None => json!({ "value": item, "_index": i }),
            };
            out.push(Emission::new(PORT_ITEM, data));
        }

        let count = out.len();
        out.push(Emission::new(PORT_DONE, json!({ "count": count })));
        Outcome::Emit(out)
    }
}

fn kind_of(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "số",
        Value::String(_) => "chuỗi",
        Value::Array(_) => "mảng",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{ctx, emitted, failure, msg};

    #[tokio::test]
    async fn n_items_produce_n_plus_one_emissions() {
        let r = SplitRule::new();
        let c = ctx("split", json!({ "path": "items" }));
        let out = emitted(
            r.handle(
                &c,
                msg(json!({ "items": [{ "id": 1 }, { "id": 2 }, { "id": 3 }] })),
            )
            .await,
        );
        assert_eq!(out.len(), 4);
        assert_eq!(out[0].0, PORT_ITEM);
        assert_eq!(out[0].1["id"], 1);
        assert_eq!(out[2].1["id"], 3);
        assert_eq!(out[3].0, PORT_DONE);
        assert_eq!(out[3].1["count"], 3);
    }

    #[tokio::test]
    async fn an_empty_array_still_reports_done() {
        let r = SplitRule::new();
        let c = ctx("split", json!({ "path": "items" }));
        let out = emitted(r.handle(&c, msg(json!({ "items": [] }))).await);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, PORT_DONE);
        assert_eq!(out[0].1["count"], 0);
    }

    #[tokio::test]
    async fn an_empty_path_splits_the_payload_itself() {
        let r = SplitRule::new();
        let c = ctx("split", json!({}));
        let out = emitted(r.handle(&c, msg(json!([10, 20]))).await);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].1, json!(10));
        assert_eq!(out[1].1, json!(20));
    }

    #[tokio::test]
    async fn as_wraps_each_element_in_a_named_field() {
        let r = SplitRule::new();
        let c = ctx(
            "split",
            json!({ "path": "l", "as": "row", "includeIndex": true }),
        );
        let out = emitted(r.handle(&c, msg(json!({ "l": ["a", "b"] }))).await);
        assert_eq!(out[0].1, json!({ "row": "a", "_index": 0 }));
        assert_eq!(out[1].1, json!({ "row": "b", "_index": 1 }));
    }

    #[tokio::test]
    async fn include_index_goes_inline_for_objects_and_wraps_scalars() {
        let r = SplitRule::new();
        let c = ctx("split", json!({ "path": "l", "includeIndex": true }));
        let out = emitted(r.handle(&c, msg(json!({ "l": [{ "id": 7 }, 42] }))).await);
        assert_eq!(out[0].1, json!({ "id": 7, "_index": 0 }));
        assert_eq!(out[1].1, json!({ "value": 42, "_index": 1 }));
    }

    #[tokio::test]
    async fn a_missing_path_fails() {
        let r = SplitRule::new();
        let c = ctx("split", json!({ "path": "nope" }));
        let err = failure(r.handle(&c, msg(json!({ "items": [] }))).await);
        assert!(err.contains("nope"), "{err}");
    }

    #[tokio::test]
    async fn a_non_array_value_fails_with_its_type() {
        let r = SplitRule::new();
        let c = ctx("split", json!({ "path": "items" }));
        let err = failure(r.handle(&c, msg(json!({ "items": "abc" }))).await);
        assert!(
            err.contains("không phải mảng") && err.contains("chuỗi"),
            "{err}"
        );
    }

    #[test]
    fn both_data_ports_take_many_edges() {
        let r = SplitRule::new();
        let s = r.spec();
        assert_eq!(
            s.output(PORT_ITEM).unwrap().arity,
            crate::engine::spec::PortArity::Many
        );
        assert_eq!(
            s.output(PORT_DONE).unwrap().arity,
            crate::engine::spec::PortArity::Many
        );
        assert!(s.has_output("error"));
    }
}
