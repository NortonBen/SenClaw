//! `mcp-call` — gọi một tool MCP của Space App khác.
//!
//! Đi thẳng tới endpoint JSON-RPC của app đích (`Bridge::app_mcp_call`) vì
//! action `mcp.call` trên bridge của daemon vẫn còn là stub.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Map, Value};

use crate::daq;
use crate::engine::spec::{Category, Rule, RuleSpec, RunCtx};
use crate::engine::types::{Message, Outcome};

pub struct McpCallRule {
    spec: RuleSpec,
}

pub fn rule() -> Arc<dyn Rule> {
    Arc::new(McpCallRule::new())
}

impl McpCallRule {
    fn new() -> Self {
        let spec = RuleSpec::builder("mcp-call", "Gọi MCP", Category::Sink)
            .desc("Gọi một tool MCP của Space App khác và gắn kết quả vào dữ liệu.")
            .icon("🔌")
            .color("#eb2f96")
            .schema(json!({
                "type": "object",
                "required": ["app", "tool"],
                "properties": {
                    "app": {
                        "type": "string",
                        "title": "Space App",
                        "placeholder": "hub",
                        "description": "Id của app đích, ví dụ `hub`, `crm`, `kanban`. App phải đang chạy."
                    },
                    "tool": {
                        "type": "string",
                        "title": "Tên tool",
                        "placeholder": "hub_send_command",
                        "description": "Tên tool MCP của app đích (không có tiền tố `mcp__`)."
                    },
                    "args": {
                        "type": "object",
                        "title": "Tham số",
                        "ui": "keyvalue",
                        "description": "Mọi giá trị chuỗi đều được nội suy ${field}, kể cả trong object/mảng lồng nhau."
                    },
                    "argsFrom": {
                        "type": "string",
                        "title": "Lấy tham số từ field",
                        "placeholder": "payload.command",
                        "description": "Nếu điền, object tại đường dẫn này được dùng làm tham số và `args` bị bỏ qua."
                    },
                    "outputField": {
                        "type": "string",
                        "title": "Ghi kết quả vào field",
                        "default": "result",
                        "description": "Đường dẫn trong payload, ví dụ `hub.reply`."
                    }
                }
            }))
            .doc(
                "Gọi tool MCP của một Space App khác.\n\n\
                 App đích được tra qua `/api/space/apps` của daemon rồi gọi thẳng \
                 `POST /api/mcp/message` — action `mcp.call` của bridge vẫn là stub nên \
                 không dùng được.\n\n\
                 Node này KHÔNG gọi được MCP của lõi SenClaw (`mcp__senclaw-*`); những \
                 server đó không phải Space App. Muốn gửi tin nhắn thì dùng node \
                 `senclaw-send`.\n\n\
                 Vỏ `content[0].text` của MCP được bóc sẵn: nếu nội dung là JSON thì \
                 kết quả là JSON đã parse, ngược lại là `{ \"text\": ... }`.\n\
                 Payload đi vào được giữ nguyên, kết quả chỉ cộng thêm vào `outputField`.",
            )
            .build();
        Self { spec }
    }
}

/// Nội suy đệ quy mọi chuỗi trong một cây JSON.
fn interpolate_deep(v: &Value, msg: &Message) -> Value {
    match v {
        Value::String(s) => Value::String(daq::interpolate(s, &msg.data, &msg.meta)),
        Value::Array(a) => Value::Array(a.iter().map(|x| interpolate_deep(x, msg)).collect()),
        Value::Object(m) => Value::Object(
            m.iter()
                .map(|(k, x)| (k.clone(), interpolate_deep(x, msg)))
                .collect::<Map<String, Value>>(),
        ),
        other => other.clone(),
    }
}

/// Dựng tham số gửi cho tool. Tách khỏi `handle` để test không cần app đích.
pub fn build_args(ctx: &RunCtx, msg: &Message) -> Result<Value, String> {
    if let Some(path) = ctx.cfg_str("argsFrom") {
        let Some(v) = daq::get(&msg.data, &path) else {
            return Err(format!("Không tìm thấy `{path}` trong dữ liệu."));
        };
        if !v.is_object() {
            return Err(format!("`{path}` phải là object, đang là {v}."));
        }
        return Ok(v);
    }
    match ctx.cfg("args") {
        None => Ok(json!({})),
        Some(v) if v.is_object() => Ok(interpolate_deep(v, msg)),
        Some(v) => Err(format!("Tham số phải là object, đang là {v}.")),
    }
}

#[async_trait]
impl Rule for McpCallRule {
    fn spec(&self) -> &RuleSpec {
        &self.spec
    }

    fn validate(&self, config: &Value) -> Vec<String> {
        let mut out = Vec::new();
        let filled = |k: &str| {
            config
                .get(k)
                .and_then(|v| v.as_str())
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false)
        };
        if !filled("app") {
            out.push("Thiếu id của Space App đích.".to_string());
        }
        if !filled("tool") {
            out.push("Thiếu tên tool MCP.".to_string());
        }
        if let Some(args) = config.get("args").filter(|v| !v.is_null()) {
            if !args.is_object() {
                out.push("Tham số phải là một object JSON.".to_string());
            }
        }
        out
    }

    async fn handle(&self, ctx: &RunCtx, msg: Message) -> Outcome {
        let Some(app) = ctx.cfg_str("app") else {
            return ctx.fail_config("Thiếu id của Space App đích.");
        };
        let Some(tool) = ctx.cfg_str("tool") else {
            return ctx.fail_config("Thiếu tên tool MCP.");
        };
        let args = match build_args(ctx, &msg) {
            Ok(a) => a,
            Err(e) => return ctx.fail_config(e),
        };

        let value = match ctx.svc.bridge.app_mcp_call(&app, &tool, args).await {
            Ok(v) => v,
            Err(e) => return ctx.fail_runtime(e),
        };
        let mut data = msg.data;
        daq::set(&mut data, &ctx.cfg_str_or("outputField", "result"), value);
        Outcome::out(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{ctx, failure, msg, msg_with_meta};

    #[test]
    fn args_are_interpolated_all_the_way_down() {
        let c = ctx(
            "mcp-call",
            json!({
                "app": "hub",
                "tool": "hub_send_command",
                "args": {
                    "device": "${device_id}",
                    "payload": { "level": "${level}", "tags": ["kho-${site}", 7] },
                    "retries": 3
                }
            }),
        );
        let m = msg_with_meta(
            json!({ "level": "cao", "site": "A" }),
            json!({ "device_id": "d-1" }),
        );
        let args = build_args(&c, &m).unwrap();
        assert_eq!(args["device"], "d-1");
        assert_eq!(args["payload"]["level"], "cao");
        assert_eq!(args["payload"]["tags"][0], "kho-A");
        // Non-string leaves keep their type instead of becoming strings.
        assert_eq!(args["payload"]["tags"][1], 7);
        assert_eq!(args["retries"], 3);
    }

    #[test]
    fn args_from_wins_over_args_and_is_taken_verbatim() {
        let c = ctx(
            "mcp-call",
            json!({
                "app": "hub",
                "tool": "t",
                "args": { "ignored": "${x}" },
                "argsFrom": "cmd"
            }),
        );
        let m = msg(json!({ "x": 1, "cmd": { "device": "d-1", "action": "on" } }));
        let args = build_args(&c, &m).unwrap();
        assert_eq!(args, json!({ "device": "d-1", "action": "on" }));
    }

    #[test]
    fn args_from_a_missing_or_non_object_path_is_an_error() {
        let c = ctx(
            "mcp-call",
            json!({ "app": "hub", "tool": "t", "argsFrom": "cmd" }),
        );
        let miss = build_args(&c, &msg(json!({}))).unwrap_err();
        assert!(miss.contains("Không tìm thấy"), "{miss}");
        let wrong = build_args(&c, &msg(json!({ "cmd": [1, 2] }))).unwrap_err();
        assert!(wrong.contains("phải là object"), "{wrong}");
    }

    #[test]
    fn missing_args_means_an_empty_object_and_a_scalar_is_rejected() {
        let c = ctx("mcp-call", json!({ "app": "hub", "tool": "t" }));
        assert_eq!(build_args(&c, &msg(json!({}))).unwrap(), json!({}));
        let bad = ctx(
            "mcp-call",
            json!({ "app": "hub", "tool": "t", "args": "x=1" }),
        );
        assert!(build_args(&bad, &msg(json!({}))).is_err());
    }

    #[tokio::test]
    async fn a_missing_app_or_tool_fails_before_any_network_call() {
        let c = ctx("mcp-call", json!({ "tool": "t" }));
        let err = failure(McpCallRule::new().handle(&c, msg(json!({}))).await);
        assert!(err.contains("Space App đích"), "{err}");

        let c2 = ctx("mcp-call", json!({ "app": "hub" }));
        let err2 = failure(McpCallRule::new().handle(&c2, msg(json!({}))).await);
        assert!(err2.contains("tên tool"), "{err2}");
    }

    #[test]
    fn validate_catches_the_same_mistakes_at_save_time() {
        let r = McpCallRule::new();
        assert_eq!(r.validate(&json!({})).len(), 2);
        assert!(r
            .validate(&json!({ "app": "hub", "tool": "hub_list_devices" }))
            .is_empty());
        let bad = r.validate(&json!({ "app": "hub", "tool": "t", "args": [1] }));
        assert!(bad.iter().any(|m| m.contains("object")), "{bad:?}");
    }
}
