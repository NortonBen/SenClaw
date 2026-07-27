//! `knowledge` — ghi hoặc tra cứu tri thức của SenClaw ngay trong chuỗi rule.
//!
//! Ba hành động dùng chung một cách dựng tham số, chỉ khác action gửi qua bridge.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::daq;
use crate::engine::spec::{Category, Rule, RuleSpec, RunCtx};
use crate::engine::types::{Message, Outcome};

const ACTIONS: [&str; 3] = ["save", "search", "recall"];

pub struct KnowledgeRule {
    spec: RuleSpec,
}

pub fn rule() -> Arc<dyn Rule> {
    Arc::new(KnowledgeRule::new())
}

impl KnowledgeRule {
    fn new() -> Self {
        let spec = RuleSpec::builder("knowledge", "Tri thức", Category::Ai)
            .desc("Lưu vào hoặc tra cứu kho tri thức của SenClaw.")
            .icon("📚")
            .color("#096dd9")
            .schema(json!({
                "type": "object",
                "required": ["action"],
                "properties": {
                    "action": {
                        "type": "string",
                        "title": "Hành động",
                        "ui": "select",
                        "enum": ["save", "search", "recall"],
                        "default": "search",
                        "description": "save = ghi; search = tìm bản ghi thô; recall = để daemon tổng hợp lại."
                    },
                    "text": {
                        "type": "string",
                        "title": "Nội dung cần lưu",
                        "ui": "textarea",
                        "placeholder": "Thiết bị ${device_id} vượt ngưỡng ${temperature} độ lúc ${ts}.",
                        "description": "Chỉ dùng cho `save`. Có nội suy ${field}."
                    },
                    "query": {
                        "type": "string",
                        "title": "Câu truy vấn",
                        "ui": "textarea",
                        "placeholder": "ngưỡng an toàn của ${device_id}",
                        "description": "Chỉ dùng cho `search` / `recall`. Có nội suy ${field}."
                    },
                    "space": {
                        "type": "string",
                        "title": "Không gian tri thức",
                        "description": "Bỏ trống = không gian mặc định của app này."
                    },
                    "tags": {
                        "type": "array",
                        "items": { "type": "string" },
                        "title": "Nhãn",
                        "description": "Chỉ dùng cho `save`. Có nội suy ${field}."
                    },
                    "limit": {
                        "type": "integer",
                        "title": "Số kết quả tối đa",
                        "default": 6,
                        "description": "Chỉ dùng cho `search` / `recall`. Ép về khoảng 1..30."
                    },
                    "outputField": {
                        "type": "string",
                        "title": "Ghi kết quả vào field",
                        "default": "knowledge",
                        "description": "Đường dẫn trong payload, ví dụ `ctx.docs`."
                    }
                }
            }))
            .doc(
                "Nối chuỗi rule vào kho tri thức của SenClaw.\n\n\
                 - `save` — ghi `text` (đã nội suy) kèm `tags`, nguồn luôn là `rule-engine`.\n\
                 - `search` — trả về các bản ghi khớp, thô.\n\
                 - `recall` — để daemon tổng hợp thành câu trả lời.\n\n\
                 **Phạm vi tìm kiếm**: bridge mặc định chỉ tìm trong không gian tri thức của \
                 chính app này. Muốn đọc kho của app khác (hoặc kho chung) thì phải điền \
                 `space` đúng tên — bỏ trống KHÔNG có nghĩa là tìm toàn cục.\n\n\
                 Payload đi vào được giữ nguyên; kết quả chỉ được cộng thêm vào `outputField`.\n\
                 Trường `status` của bridge bị lược bỏ, phần còn lại giữ nguyên hình dạng.",
            )
            .build();
        Self { spec }
    }
}

/// Tham số đã nội suy xong — tách khỏi `handle` để test không cần daemon.
#[derive(Debug, PartialEq)]
pub struct Rendered {
    pub action: String,
    /// `text` khi save, `query` khi search/recall.
    pub input: String,
    pub space: Option<String>,
    pub tags: Vec<String>,
    pub limit: u32,
}

pub fn render(ctx: &RunCtx, msg: &Message) -> Rendered {
    let action = ctx.cfg_str_or("action", "search");
    let key = if action == "save" { "text" } else { "query" };
    let input = ctx
        .cfg_str(key)
        .map(|t| daq::interpolate(&t, &msg.data, &msg.meta))
        .unwrap_or_default();
    let tags = match ctx.cfg("tags") {
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|v| v.as_str())
            .map(|s| daq::interpolate(s, &msg.data, &msg.meta))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        Some(Value::String(s)) => daq::interpolate(s, &msg.data, &msg.meta)
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        _ => vec![],
    };
    Rendered {
        action,
        input,
        space: ctx.cfg_str("space"),
        tags,
        limit: (ctx.cfg_u64_or("limit", 6) as u32).clamp(1, 30),
    }
}

/// Bỏ `status` của bridge; phần còn lại là dữ liệu thật.
fn clean(v: Value) -> Value {
    match v {
        Value::Object(mut m) => {
            m.remove("status");
            Value::Object(m)
        }
        other => other,
    }
}

#[async_trait]
impl Rule for KnowledgeRule {
    fn spec(&self) -> &RuleSpec {
        &self.spec
    }

    fn validate(&self, config: &Value) -> Vec<String> {
        let mut out = Vec::new();
        let action = config
            .get("action")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("search");
        if !ACTIONS.contains(&action) {
            out.push(format!(
                "Hành động không hợp lệ: `{action}`. Chọn một trong {}.",
                ACTIONS.join(" | ")
            ));
            return out;
        }
        let filled = |k: &str| {
            config
                .get(k)
                .and_then(|v| v.as_str())
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false)
        };
        if action == "save" && !filled("text") {
            out.push("Hành động `save` cần nội dung cần lưu.".to_string());
        }
        if action != "save" && !filled("query") {
            out.push(format!("Hành động `{action}` cần câu truy vấn."));
        }
        out
    }

    async fn handle(&self, ctx: &RunCtx, msg: Message) -> Outcome {
        let r = render(ctx, &msg);
        if !ACTIONS.contains(&r.action.as_str()) {
            return ctx.fail_config(format!(
                "Hành động không hợp lệ: `{}`. Chọn một trong {}.",
                r.action,
                ACTIONS.join(" | ")
            ));
        }
        if r.input.trim().is_empty() {
            return ctx.fail_config(if r.action == "save" {
                "Nội dung cần lưu rỗng sau khi nội suy.".to_string()
            } else {
                "Câu truy vấn rỗng sau khi nội suy.".to_string()
            });
        }

        let bridge = &ctx.svc.bridge;
        let result = match r.action.as_str() {
            "save" => {
                bridge
                    .knowledge_save(
                        &r.input,
                        r.space.as_deref(),
                        r.tags.clone(),
                        Some("rule-engine"),
                    )
                    .await
            }
            "search" => {
                bridge
                    .knowledge_query("knowledge.search", &r.input, r.space.as_deref(), r.limit)
                    .await
            }
            _ => {
                bridge
                    .knowledge_query("knowledge.recall", &r.input, r.space.as_deref(), r.limit)
                    .await
            }
        };

        let value = match result {
            Ok(v) => clean(v),
            Err(e) => return ctx.fail_runtime(e),
        };
        let mut data = msg.data;
        daq::set(
            &mut data,
            &ctx.cfg_str_or("outputField", "knowledge"),
            value,
        );
        Outcome::out(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{ctx, failure, msg, msg_with_meta};

    #[test]
    fn save_renders_text_tags_and_space() {
        let c = ctx(
            "knowledge",
            json!({
                "action": "save",
                "text": "Thiết bị ${device_id} đạt ${temperature} độ.",
                "tags": ["iot", "kho-${site}"],
                "space": "canh-bao"
            }),
        );
        let m = msg_with_meta(
            json!({ "temperature": 31.5 }),
            json!({ "device_id": "d-1", "site": "A" }),
        );
        let r = render(&c, &m);
        assert_eq!(r.action, "save");
        assert_eq!(r.input, "Thiết bị d-1 đạt 31.5 độ.");
        assert_eq!(r.tags, vec!["iot".to_string(), "kho-A".to_string()]);
        assert_eq!(r.space.as_deref(), Some("canh-bao"));
    }

    #[test]
    fn search_renders_the_query_and_clamps_the_limit() {
        let c = ctx(
            "knowledge",
            json!({ "action": "search", "query": "ngưỡng của ${device_id}", "limit": 900 }),
        );
        let r = render(&c, &msg(json!({ "device_id": "d-9" })));
        assert_eq!(r.input, "ngưỡng của d-9");
        assert_eq!(r.limit, 30);

        let c0 = ctx(
            "knowledge",
            json!({ "action": "recall", "query": "x", "limit": 0 }),
        );
        assert_eq!(render(&c0, &msg(json!({}))).limit, 1);
    }

    #[test]
    fn tags_accept_a_comma_separated_string_too() {
        let c = ctx(
            "knowledge",
            json!({ "action": "save", "text": "x", "tags": "iot, ${site} ,," }),
        );
        let r = render(&c, &msg(json!({ "site": "kho A" })));
        assert_eq!(r.tags, vec!["iot".to_string(), "kho A".to_string()]);
    }

    #[test]
    fn the_default_action_is_search_and_space_defaults_to_none() {
        let c = ctx("knowledge", json!({ "query": "abc" }));
        let r = render(&c, &msg(json!({})));
        assert_eq!(r.action, "search");
        assert_eq!(r.space, None);
        assert_eq!(r.limit, 6);
    }

    #[tokio::test]
    async fn an_unknown_action_fails_before_touching_the_bridge() {
        let c = ctx("knowledge", json!({ "action": "forget", "query": "x" }));
        let err = failure(KnowledgeRule::new().handle(&c, msg(json!({}))).await);
        assert!(err.contains("Hành động không hợp lệ"), "{err}");
    }

    #[tokio::test]
    async fn an_empty_query_fails_before_touching_the_bridge() {
        let c = ctx(
            "knowledge",
            json!({ "action": "search", "query": "${nope}" }),
        );
        let err = failure(KnowledgeRule::new().handle(&c, msg(json!({}))).await);
        assert!(err.contains("Câu truy vấn rỗng"), "{err}");
    }

    #[test]
    fn clean_drops_the_bridge_status_envelope() {
        let v = clean(json!({ "status": "ok", "results": [1] }));
        assert_eq!(v, json!({ "results": [1] }));
    }

    #[test]
    fn validate_requires_the_field_matching_the_action() {
        let r = KnowledgeRule::new();
        assert!(!r.validate(&json!({ "action": "forget" })).is_empty());
        assert!(!r.validate(&json!({ "action": "save" })).is_empty());
        assert!(r
            .validate(&json!({ "action": "save", "text": "x" }))
            .is_empty());
        assert!(!r.validate(&json!({ "action": "recall" })).is_empty());
        assert!(r
            .validate(&json!({ "action": "recall", "query": "x" }))
            .is_empty());
    }
}
