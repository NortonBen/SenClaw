//! JSON-RPC MCP server over HTTP + SSE.
//!
//! Hand-written like every other Space App: `rmcp` is for the core's stdio
//! servers, not for apps.

use std::convert::Infallible;
use std::sync::Arc;

use axum::{
    extract::State,
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use futures_util::stream::Stream;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::engine::graph;
use crate::engine::types::{next_id, Edge};
use crate::model::{ChainStatus, Node};
use crate::state::AppState;

#[derive(Deserialize, Debug)]
pub struct JsonRpcRequest {
    #[allow(dead_code)]
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    pub params: Option<Value>,
}

pub async fn mcp_sse(
    State(state): State<Arc<AppState>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = state.mcp_tx.subscribe();
    let stream = async_stream::stream! {
        yield Ok(Event::default().event("endpoint").data("/api/mcp/message".to_string()));
        while let Ok(msg) = rx.recv().await {
            yield Ok(Event::default().event("message").data(msg));
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::default())
}

fn text_result(text: String) -> Value {
    json!({ "content": [{ "type": "text", "text": text }] })
}
fn json_result(v: Value) -> Value {
    text_result(serde_json::to_string_pretty(&v).unwrap_or_default())
}
fn error_result(text: String) -> Value {
    json!({ "isError": true, "content": [{ "type": "text", "text": text }] })
}

pub async fn mcp_message(
    State(state): State<Arc<AppState>>,
    Json(req): Json<JsonRpcRequest>,
) -> Json<Value> {
    let reply = |result: Value| -> Json<Value> {
        let resp = json!({ "jsonrpc": "2.0", "id": req.id, "result": result });
        let _ = state.mcp_tx.send(resp.to_string());
        Json(resp)
    };

    match req.method.as_str() {
        "initialize" => reply(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "rule-engine-mcp", "version": env!("CARGO_PKG_VERSION") }
        })),
        "ping" => reply(json!({})),
        "notifications/initialized" => reply(json!({})),
        "tools/list" => reply(json!({ "tools": tools_list() })),
        "tools/call" => {
            let params = req.params.clone().unwrap_or(json!({}));
            let name = params["name"].as_str().unwrap_or("").to_string();
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            reply(call_tool(&state, &name, &args).await)
        }
        other => reply(error_result(format!("phương thức không hỗ trợ: {other}"))),
    }
}

pub fn tools_list() -> Vec<Value> {
    let chain_id = json!({ "type": "number", "description": "ID của luồng" });
    vec![
        json!({
            "name": "rule_registry",
            "description": "Liệt kê mọi loại node dùng được, kèm cổng vào/ra và schema cấu hình. GỌI TRƯỚC khi dựng hoặc sửa luồng.",
            "inputSchema": { "type": "object", "properties": {
                "category": { "type": "string", "description": "Lọc: source | transform | logic | filter | sink | ai" }
            }}
        }),
        json!({
            "name": "rule_list_chains",
            "description": "Liệt kê các luồng (rule chain) hiện có, kèm trạng thái và có đang chạy hay không.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "rule_get_chain",
            "description": "Xem chi tiết một luồng: node, cạnh nối và các cảnh báo kiểm tra.",
            "inputSchema": { "type": "object", "required": ["chainId"], "properties": { "chainId": chain_id }}
        }),
        json!({
            "name": "rule_create_chain",
            "description": "Tạo một luồng rỗng.",
            "inputSchema": { "type": "object", "required": ["name"], "properties": {
                "name": { "type": "string" },
                "description": { "type": "string" }
            }}
        }),
        json!({
            "name": "rule_update_graph",
            "description": "Thay toàn bộ đồ thị của luồng. `nodes`: [{id, rule, name, config, x, y, opts?}]. `edges`: [{id, from:{node,port}, to:{node,port}}]. Node id tự đặt, ngắn gọn. Không nhét id node vào config — mọi liên kết nằm ở edges.",
            "inputSchema": { "type": "object", "required": ["chainId", "nodes", "edges"], "properties": {
                "chainId": chain_id,
                "nodes": { "type": "array", "items": { "type": "object" }},
                "edges": { "type": "array", "items": { "type": "object" }}
            }}
        }),
        json!({
            "name": "rule_validate",
            "description": "Kiểm tra đồ thị đã lưu: cổng có tồn tại không, cạnh có hợp lệ không, node mồ côi, vòng lặp.",
            "inputSchema": { "type": "object", "required": ["chainId"], "properties": { "chainId": chain_id }}
        }),
        json!({
            "name": "rule_activate",
            "description": "Nạp và chạy luồng. Thất bại nếu đồ thị còn lỗi.",
            "inputSchema": { "type": "object", "required": ["chainId"], "properties": { "chainId": chain_id }}
        }),
        json!({
            "name": "rule_deactivate",
            "description": "Dừng luồng: huỷ node nguồn và mọi run đang chạy.",
            "inputSchema": { "type": "object", "required": ["chainId"], "properties": { "chainId": chain_id }}
        }),
        json!({
            "name": "rule_delete_chain",
            "description": "Xoá hẳn một luồng cùng toàn bộ lịch sử chạy.",
            "inputSchema": { "type": "object", "required": ["chainId"], "properties": { "chainId": chain_id }}
        }),
        json!({
            "name": "rule_trigger",
            "description": "Bơm một sự kiện thử vào luồng đang chạy (mặc định vào node `manual`). Trả về runId để xem trace.",
            "inputSchema": { "type": "object", "required": ["chainId"], "properties": {
                "chainId": chain_id,
                "node": { "type": "string", "description": "Node nguồn cần bơm. Bỏ trống = node `manual` đầu tiên." },
                "port": { "type": "string", "description": "Cổng ra của node để phát (mặc định `out`)." },
                "data": { "type": "object", "description": "Payload gửi vào luồng" },
                "meta": { "type": "object", "description": "Metadata kèm theo message (truy cập qua `meta_data` trong biểu thức)." }
            }}
        }),
        json!({
            "name": "rule_push",
            "description": "PUSH (bất đồng bộ): bơm một sự kiện vào luồng đang chạy rồi trả về NGAY với runId — không chờ luồng chạy xong. Dùng cho fire-and-forget. Mặc định vào node `manual`.",
            "inputSchema": { "type": "object", "required": ["chainId"], "properties": {
                "chainId": chain_id,
                "node": { "type": "string", "description": "Node nguồn cần bơm. Bỏ trống = node `manual`/`request` đầu tiên." },
                "port": { "type": "string", "description": "Cổng ra để phát (mặc định `out`)." },
                "data": { "type": "object", "description": "Payload gửi vào luồng" },
                "meta": { "type": "object", "description": "Metadata kèm theo" }
            }}
        }),
        json!({
            "name": "rule_call",
            "description": "PULL / request-response (đồng bộ): bơm dữ liệu vào node `request` (hoặc `manual`) rồi CHỜ tới khi run chạm node `respond`, trả về { status, result, error }. `result` là dữ liệu tới `respond`. Dùng để gọi một luồng như một hàm.",
            "inputSchema": { "type": "object", "required": ["chainId"], "properties": {
                "chainId": chain_id,
                "node": { "type": "string", "description": "Node vào. Bỏ trống = node `request` đầu tiên, không có thì `manual` đầu tiên." },
                "data": { "type": "object", "description": "Payload gửi vào luồng" },
                "meta": { "type": "object", "description": "Metadata kèm theo" },
                "timeoutMs": { "type": "number", "description": "Chờ tối đa (mặc định 15000)." }
            }}
        }),
        json!({
            "name": "rule_get",
            "description": "GET: đọc giá trị mới nhất mà một node `store` đã cache, KHÔNG chạy luồng. Trả về { value, ts } hoặc value rỗng nếu chưa có.",
            "inputSchema": { "type": "object", "required": ["chainId", "node"], "properties": {
                "chainId": chain_id,
                "node": { "type": "string", "description": "ID của node `store` cần đọc." }
            }}
        }),
        json!({
            "name": "rule_runs",
            "description": "Lịch sử các lần chạy của một luồng (trạng thái, số bước, lỗi).",
            "inputSchema": { "type": "object", "required": ["chainId"], "properties": {
                "chainId": chain_id,
                "limit": { "type": "number", "description": "Mặc định 20" }
            }}
        }),
        json!({
            "name": "rule_run_trace",
            "description": "Trace từng bước của một lần chạy: node nào, vào cổng nào, ra cổng nào, dữ liệu và lỗi. Chỉ có dữ liệu khi luồng hoặc node bật debug.",
            "inputSchema": { "type": "object", "required": ["runId"], "properties": {
                "runId": { "type": "number" }
            }}
        }),
        json!({
            "name": "rule_logs",
            "description": "Log gần đây của một luồng.",
            "inputSchema": { "type": "object", "required": ["chainId"], "properties": {
                "chainId": chain_id,
                "limit": { "type": "number", "description": "Mặc định 50" }
            }}
        }),
        json!({
            "name": "rule_set_debug",
            "description": "Bật/tắt trace toàn luồng. Bật thì mỗi bước được ghi lại (tốn dung lượng, chỉ dùng khi cần soi).",
            "inputSchema": { "type": "object", "required": ["chainId", "debug"], "properties": {
                "chainId": chain_id,
                "debug": { "type": "boolean" }
            }}
        }),
        json!({
            "name": "rule_generate",
            "description": "Dựng đồ thị luồng từ mô tả bằng lời. Tự đọc registry, sinh node + cạnh, kiểm tra rồi lưu vào luồng (tạo mới nếu không truyền chainId). KHÔNG tự kích hoạt.",
            "inputSchema": { "type": "object", "required": ["request"], "properties": {
                "request": { "type": "string", "description": "Mô tả luồng cần dựng, ví dụ: mỗi 5 phút gọi API thời tiết, nếu nhiệt độ > 35 thì gửi Telegram" },
                "chainId": { "type": "number", "description": "Ghi đè đồ thị của luồng này. Bỏ trống = tạo luồng mới." },
                "name": { "type": "string", "description": "Tên luồng khi tạo mới" }
            }}
        }),
    ]
}

pub async fn call_tool(state: &Arc<AppState>, name: &str, args: &Value) -> Value {
    let chain_id = || -> Option<i64> { args.get("chainId").and_then(|v| v.as_i64()) };

    match name {
        "rule_registry" => {
            let filter = args.get("category").and_then(|v| v.as_str());
            let rules: Vec<Value> = state
                .engine
                .registry
                .specs()
                .into_iter()
                .filter(|s| {
                    filter.is_none()
                        || filter == Some(format!("{:?}", s.category).to_lowercase().as_str())
                })
                .map(|s| {
                    // Some ports are derived from config (switch → one per case,
                    // join/merge → one per `inputs` entry). The static lists
                    // alone would tell an agent join has no inputs and switch has
                    // only `error` out — so flag the dynamic ones explicitly.
                    let dyn_out = matches!(s.id.as_str(), "switch");
                    let dyn_in = matches!(s.id.as_str(), "join" | "merge");
                    json!({
                        "id": s.id,
                        "name": s.name,
                        "category": s.category,
                        "isSource": state.engine.registry.is_source(&s.id),
                        "description": s.description,
                        "inputs": s.inputs.iter().map(|p| &p.id).collect::<Vec<_>>(),
                        "outputs": s.outputs.iter().map(|p| &p.id).collect::<Vec<_>>(),
                        "dynamicInputs": dyn_in,
                        "dynamicOutputs": dyn_out,
                        "portNote": if dyn_in {
                            "Cổng vào sinh từ `config.inputs`; cần đặt opts.join = all/merge."
                        } else if dyn_out {
                            "Cổng ra sinh từ `config.cases` (mỗi case một cổng) + `default`."
                        } else { "" },
                        "config": s.config_schema,
                    })
                })
                .collect();
            json_result(json!({ "rules": rules }))
        }

        "rule_list_chains" => match state.db.list_chains() {
            Ok(chains) => {
                let deployed = state.engine.deployed_chains();
                json_result(json!({
                    "chains": chains.iter().map(|c| json!({
                        "id": c.id, "name": c.name, "status": c.status,
                        "debug": c.debug, "deployed": deployed.contains(&c.id),
                        "updatedAt": c.updated_at,
                    })).collect::<Vec<_>>()
                }))
            }
            Err(e) => error_result(e.to_string()),
        },

        "rule_get_chain" => {
            let Some(id) = chain_id() else {
                return error_result("thiếu `chainId`".into());
            };
            match state.db.get_chain(id) {
                Ok(Some(chain)) => {
                    let nodes = state.db.list_nodes(id).unwrap_or_default();
                    let edges = state.db.list_edges(id).unwrap_or_default();
                    let issues = graph::validate(&nodes, &edges, &state.engine.registry);
                    json_result(json!({
                        "chain": chain, "nodes": nodes, "edges": edges, "issues": issues,
                        "deployed": state.engine.is_deployed(id),
                    }))
                }
                Ok(None) => error_result(format!("không có luồng {id}")),
                Err(e) => error_result(e.to_string()),
            }
        }

        "rule_create_chain" => {
            // Trim + reject blank, matching the REST endpoint so both doors into
            // the same table enforce the same rule.
            let name = args
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .trim();
            if name.is_empty() {
                return error_result("tên luồng không được để trống".into());
            }
            let desc = args
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            match state.db.create_chain(next_id() as i64, name, desc) {
                Ok(c) => json_result(json!({ "chain": c })),
                Err(e) => error_result(e.to_string()),
            }
        }

        "rule_update_graph" => {
            let Some(id) = chain_id() else {
                return error_result("thiếu `chainId`".into());
            };
            match save_graph(state, id, args).await {
                Ok(v) => json_result(v),
                Err(e) => error_result(e),
            }
        }

        "rule_validate" => {
            let Some(id) = chain_id() else {
                return error_result("thiếu `chainId`".into());
            };
            let nodes = state.db.list_nodes(id).unwrap_or_default();
            let edges = state.db.list_edges(id).unwrap_or_default();
            let issues = graph::validate(&nodes, &edges, &state.engine.registry);
            json_result(json!({ "ok": !graph::has_errors(&issues), "issues": issues }))
        }

        "rule_activate" => {
            let Some(id) = chain_id() else {
                return error_result("thiếu `chainId`".into());
            };
            let Ok(Some(chain)) = state.db.get_chain(id) else {
                return error_result(format!("không có luồng {id}"));
            };
            let nodes = state.db.list_nodes(id).unwrap_or_default();
            let edges = state.db.list_edges(id).unwrap_or_default();
            match state.engine.deploy(&chain, &nodes, &edges).await {
                Ok(issues) => {
                    let _ = state.db.set_chain_status(id, ChainStatus::Active);
                    json_result(json!({ "activated": true, "warnings": issues }))
                }
                Err(e) => error_result(format!("không kích hoạt được: {e}")),
            }
        }

        "rule_deactivate" => {
            let Some(id) = chain_id() else {
                return error_result("thiếu `chainId`".into());
            };
            state.engine.undeploy(id).await;
            let _ = state.db.set_chain_status(id, ChainStatus::Inactive);
            json_result(json!({ "deactivated": true }))
        }

        "rule_delete_chain" => {
            let Some(id) = chain_id() else {
                return error_result("thiếu `chainId`".into());
            };
            state.engine.undeploy(id).await;
            match state.db.delete_chain(id) {
                Ok(_) => json_result(json!({ "deleted": true })),
                Err(e) => error_result(e.to_string()),
            }
        }

        "rule_trigger" => {
            let Some(id) = chain_id() else {
                return error_result("thiếu `chainId`".into());
            };
            if !state.engine.is_deployed(id) {
                return error_result("luồng chưa chạy — gọi `rule_activate` trước".into());
            }
            let node = match args.get("node").and_then(|v| v.as_str()) {
                Some(n) => n.to_string(),
                None => {
                    let nodes = state.db.list_nodes(id).unwrap_or_default();
                    match nodes.iter().find(|n| n.rule == "manual") {
                        Some(n) => n.id.clone(),
                        None => {
                            return error_result(
                                "luồng không có node `manual`; truyền `node` cụ thể".into(),
                            )
                        }
                    }
                }
            };
            let data = args.get("data").cloned().unwrap_or_else(|| json!({}));
            let port = args.get("port").and_then(|v| v.as_str()).unwrap_or("out");
            let meta = match args.get("meta").cloned() {
                Some(Value::Object(m)) => {
                    let mut m = m;
                    m.entry("_event".to_string()).or_insert(json!("mcp"));
                    Value::Object(m)
                }
                _ => json!({ "_event": "mcp" }),
            };
            match state.engine.start_run(id, &node, port, data, meta).await {
                Some(run_id) => json_result(json!({
                    "runId": run_id,
                    "hint": "gọi rule_run_trace với runId này để xem từng bước (cần bật debug)"
                })),
                None => error_result(format!("không tìm thấy node `{node}`")),
            }
        }

        // PUSH — inject and return immediately (fire-and-forget).
        "rule_push" => {
            let Some(id) = chain_id() else {
                return error_result("thiếu `chainId`".into());
            };
            if !state.engine.is_deployed(id) {
                return error_result("luồng chưa chạy — gọi `rule_activate` trước".into());
            }
            let node = match resolve_entry_node(state, id, args) {
                Ok(n) => n,
                Err(e) => return error_result(e),
            };
            let data = args.get("data").cloned().unwrap_or_else(|| json!({}));
            let port = args.get("port").and_then(|v| v.as_str()).unwrap_or("out");
            let meta = mcp_meta(args);
            match state.engine.start_run(id, &node, port, data, meta).await {
                Some(run_id) => json_result(json!({ "runId": run_id, "pushed": true })),
                None => error_result(format!("không tìm thấy node `{node}`")),
            }
        }

        // PULL — inject at a `request`/`manual` entry, wait for `respond`.
        "rule_call" => {
            let Some(id) = chain_id() else {
                return error_result("thiếu `chainId`".into());
            };
            if !state.engine.is_deployed(id) {
                return error_result("luồng chưa chạy — gọi `rule_activate` trước".into());
            }
            let node = match resolve_entry_node(state, id, args) {
                Ok(n) => n,
                Err(e) => return error_result(e),
            };
            let data = args.get("data").cloned().unwrap_or_else(|| json!({}));
            let meta = mcp_meta(args);
            let timeout_ms = args
                .get("timeoutMs")
                .and_then(|v| v.as_u64())
                .unwrap_or(15000)
                .clamp(100, 120_000);
            let out = state
                .engine
                .start_run_wait(id, &node, "out", data, meta, timeout_ms)
                .await;
            json_result(json!({
                "runId": out.run_id,
                "status": out.status,
                "result": out.result,
                "error": out.error,
            }))
        }

        // GET — read a `store` node's cached value, no run.
        "rule_get" => {
            let Some(id) = chain_id() else {
                return error_result("thiếu `chainId`".into());
            };
            let Some(node) = args.get("node").and_then(|v| v.as_str()) else {
                return error_result("thiếu `node` (id của node store)".into());
            };
            match state.db.state_get(id, node, crate::rules::store::SCOPE) {
                Some(v) => json_result(json!({
                    "value": v.get("value").cloned().unwrap_or(Value::Null),
                    "ts": v.get("ts").cloned().unwrap_or(Value::Null),
                })),
                None => json_result(json!({
                    "value": Value::Null,
                    "ts": Value::Null,
                    "hint": "node store này chưa nhận giá trị nào, hoặc id node sai"
                })),
            }
        }

        "rule_runs" => {
            let Some(id) = chain_id() else {
                return error_result("thiếu `chainId`".into());
            };
            let limit = args
                .get("limit")
                .and_then(|v| v.as_i64())
                .unwrap_or(20)
                .clamp(1, 200);
            match state.db.list_runs(Some(id), limit) {
                Ok(runs) => json_result(json!({ "runs": runs })),
                Err(e) => error_result(e.to_string()),
            }
        }

        "rule_run_trace" => {
            let Some(run_id) = args.get("runId").and_then(|v| v.as_i64()) else {
                return error_result("thiếu `runId`".into());
            };
            match state.db.list_hops(run_id) {
                Ok(hops) if hops.is_empty() => json_result(json!({
                    "hops": [],
                    "hint": "không có trace — bật debug bằng rule_set_debug rồi chạy lại"
                })),
                Ok(hops) => json_result(json!({ "hops": hops })),
                Err(e) => error_result(e.to_string()),
            }
        }

        "rule_logs" => {
            let Some(id) = chain_id() else {
                return error_result("thiếu `chainId`".into());
            };
            let limit = args
                .get("limit")
                .and_then(|v| v.as_i64())
                .unwrap_or(50)
                .clamp(1, 500);
            match state.db.list_logs(id, limit) {
                Ok(logs) => json_result(json!({ "logs": logs })),
                Err(e) => error_result(e.to_string()),
            }
        }

        "rule_set_debug" => {
            let Some(id) = chain_id() else {
                return error_result("thiếu `chainId`".into());
            };
            // Require `debug` explicitly: silently defaulting a missing flag to
            // `false` would turn a malformed "turn debug on" call into "turn it
            // off", the opposite of intent.
            let Some(debug) = args.get("debug").and_then(|v| v.as_bool()) else {
                return error_result("thiếu `debug` (true/false)".into());
            };
            if let Err(e) = state.db.update_chain_meta(id, None, None, Some(debug)) {
                return error_result(e.to_string());
            }
            if state.engine.is_deployed(id) {
                if let Ok(Some(chain)) = state.db.get_chain(id) {
                    let nodes = state.db.list_nodes(id).unwrap_or_default();
                    let edges = state.db.list_edges(id).unwrap_or_default();
                    let _ = state.engine.deploy(&chain, &nodes, &edges).await;
                }
            }
            json_result(json!({ "debug": debug }))
        }

        "rule_generate" => generate_chain(state, args).await,

        other => error_result(format!("công cụ không tồn tại: {other}")),
    }
}

async fn save_graph(state: &Arc<AppState>, id: i64, args: &Value) -> Result<Value, String> {
    // Require both keys to be PRESENT. Defaulting a missing key to `[]` turns a
    // wrong-shaped payload (a model that answered `{"graph": …}`) into a silent
    // wipe of a running chain. An explicit `[]` is still allowed — that is a
    // deliberate clear.
    let nodes_val = args
        .get("nodes")
        .ok_or_else(|| "thiếu `nodes` (mảng node)".to_string())?;
    let edges_val = args
        .get("edges")
        .ok_or_else(|| "thiếu `edges` (mảng cạnh)".to_string())?;
    let nodes: Vec<Node> = serde_json::from_value(nodes_val.clone())
        .map_err(|e| format!("`nodes` sai định dạng: {e}"))?;
    let edges: Vec<Edge> = serde_json::from_value(edges_val.clone())
        .map_err(|e| format!("`edges` sai định dạng: {e}"))?;

    let issues = graph::validate(&nodes, &edges, &state.engine.registry);
    let has_errors = graph::has_errors(&issues);
    state
        .db
        .replace_graph(id, &nodes, &edges)
        .map_err(|e| e.to_string())?;

    // Mirror the REST `put_graph` semantics exactly, so both doors behave alike:
    // a running chain saved with a broken graph is taken down and marked ERROR
    // rather than left running the stale deployment under an ACTIVE badge.
    let mut redeployed = false;
    if state.engine.is_deployed(id) {
        if has_errors {
            state.engine.undeploy(id).await;
            let _ = state.db.set_chain_status(id, ChainStatus::Error);
        } else if let Ok(Some(chain)) = state.db.get_chain(id) {
            redeployed = state.engine.deploy(&chain, &nodes, &edges).await.is_ok();
        }
    }
    Ok(json!({
        "saved": true,
        "hasErrors": has_errors,
        "issues": issues,
        "redeployed": redeployed,
    }))
}

/// Resolve which source node to inject into: an explicit `node`, else the first
/// `request` node, else the first `manual` node.
fn resolve_entry_node(
    state: &Arc<AppState>,
    chain_id: i64,
    args: &Value,
) -> Result<String, String> {
    if let Some(n) = args.get("node").and_then(|v| v.as_str()) {
        return Ok(n.to_string());
    }
    let nodes = state.db.list_nodes(chain_id).unwrap_or_default();
    if let Some(n) = nodes.iter().find(|n| n.rule == "request") {
        return Ok(n.id.clone());
    }
    if let Some(n) = nodes.iter().find(|n| n.rule == "manual") {
        return Ok(n.id.clone());
    }
    Err("luồng không có node `request` hoặc `manual`; truyền `node` cụ thể".into())
}

/// Build the message meta from an optional `meta` arg, always tagging `_event`.
fn mcp_meta(args: &Value) -> Value {
    match args.get("meta").cloned() {
        Some(Value::Object(mut m)) => {
            m.entry("_event".to_string()).or_insert(json!("mcp"));
            Value::Object(m)
        }
        _ => json!({ "_event": "mcp" }),
    }
}

/// Ask the model for a graph, then hold it to the same validation a human gets.
async fn generate_chain(state: &Arc<AppState>, args: &Value) -> Value {
    let Some(request) = args.get("request").and_then(|v| v.as_str()) else {
        return error_result("thiếu `request`".into());
    };

    let catalogue: Vec<Value> = state
        .engine
        .registry
        .specs()
        .into_iter()
        .map(|s| {
            let dyn_in = matches!(s.id.as_str(), "join" | "merge" | "aggregate");
            let dyn_out = matches!(s.id.as_str(), "switch");
            json!({
                "rule": s.id,
                "category": format!("{:?}", s.category).to_lowercase(),
                "isSource": state.engine.registry.is_source(&s.id),
                "desc": s.description,
                "in": s.inputs.iter().map(|p| &p.id).collect::<Vec<_>>(),
                "out": s.outputs.iter().map(|p| &p.id).collect::<Vec<_>>(),
                "dynamicPorts": dyn_in || dyn_out,
                "config": s.config_schema,
            })
        })
        .collect();

    let system = "Bạn dựng đồ thị luồng xử lý dữ liệu cho một rule engine. \
        Chỉ trả về JSON, không giải thích, không rào đón.";
    let prompt = format!(
        "Danh mục node dùng được (JSON):\n{}\n\n\
         Yêu cầu của người dùng: {}\n\n\
         Trả về DUY NHẤT một object JSON dạng:\n\
         {{\"nodes\":[{{\"id\":\"n1\",\"rule\":\"manual\",\"name\":\"Bắt đầu\",\"config\":{{}},\"x\":0,\"y\":0}}],\
         \"edges\":[{{\"id\":\"e1\",\"from\":{{\"node\":\"n1\",\"port\":\"out\"}},\"to\":{{\"node\":\"n2\",\"port\":\"in\"}}}}]}}\n\n\
         Quy tắc bắt buộc:\n\
         - Luồng phải bắt đầu bằng đúng một node nguồn (isSource=true).\n\
         - `port` phải nằm trong danh sách in/out của node đó. Cổng `error` luôn dùng được.\n\
         - TUYỆT ĐỐI không đặt id node vào `config`; mọi liên kết chỉ nằm ở `edges`.\n\
         - `config` phải khớp schema của node.\n\
         - Node rộng ~230px: toạ độ x giãn 320 một bước (0, 320, 640...), y giãn 160 cho các nhánh.\n\
         - Đặt tên node bằng tiếng Việt, ngắn.\n\
         Chọn kiểu luồng theo yêu cầu:\n\
         - Hỏi-đáp đồng bộ (gọi như một hàm, cần trả kết quả): bắt đầu bằng `request`, kết thúc bằng đúng một `respond`.\n\
         - Tra cứu giá trị mới nhất: cho nhánh đi qua `store` để bên ngoài đọc bằng rule_get.\n\
         - Việc chạy nền theo lịch/sự kiện: dùng `schedule`/`webhook`/`manual`.\n\
         - Gộp nhiều nhánh phải đặt opts.join = all/merge trên node `join`/`merge` (mặc định `any` KHÔNG gộp).\n\
         - Node có dynamicPorts=true sinh cổng theo config (switch: theo cases; join/merge/aggregate: theo inputs).",
        serde_json::to_string(&catalogue).unwrap_or_default(),
        request
    );

    let reply = match state
        .engine
        .svc
        .bridge
        .llm_request(system, &prompt, 8000, None)
        .await
    {
        Ok(r) => r,
        Err(e) => return error_result(format!("gọi LLM thất bại: {e}")),
    };

    let parsed: Value = match parse_json_block(&reply.text) {
        Some(v) => v,
        None => {
            return error_result(format!(
                "mô hình không trả JSON hợp lệ. Nội dung nhận được:\n{}",
                reply.text
            ))
        }
    };

    let id = match args.get("chainId").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => {
            let name = args
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("Luồng mới");
            match state.db.create_chain(next_id() as i64, name, request) {
                Ok(c) => c.id,
                Err(e) => return error_result(e.to_string()),
            }
        }
    };

    match save_graph(state, id, &parsed).await {
        Ok(mut v) => {
            v["chainId"] = json!(id);
            v["model"] = json!(reply.model);
            json_result(v)
        }
        Err(e) => error_result(format!("đồ thị sinh ra không lưu được: {e}")),
    }
}

/// Models like to wrap JSON in prose or a fenced block; dig it out.
fn parse_json_block(text: &str) -> Option<Value> {
    if let Ok(v) = serde_json::from_str::<Value>(text.trim()) {
        return Some(v);
    }
    let fenced = text
        .split("```")
        .map(|s| s.trim_start_matches("json").trim())
        .find(|s| s.starts_with('{'));
    if let Some(f) = fenced {
        if let Ok(v) = serde_json::from_str::<Value>(f) {
            return Some(v);
        }
    }
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if end <= start {
        return None;
    }
    serde_json::from_str::<Value>(&text[start..=end]).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> Arc<AppState> {
        let db = Arc::new(crate::db::Db::open(":memory:").unwrap());
        let bus = crate::engine::services::EventBus::new();
        let svc = Arc::new(crate::engine::services::Services::new(db.clone(), bus));
        let mut reg = crate::engine::registry::Registry::new();
        crate::rules::register(&mut reg);
        let engine = crate::engine::Engine::start(Arc::new(reg), svc);
        let (mcp_tx, _) = tokio::sync::broadcast::channel(8);
        Arc::new(AppState { db, engine, mcp_tx })
    }

    /// Drift guard: a tool that is advertised but not dispatched is invisible
    /// to the agent until someone tries it in anger.
    #[tokio::test]
    async fn every_listed_tool_has_a_dispatch_arm() {
        let st = state();
        for tool in tools_list() {
            let name = tool["name"].as_str().unwrap();
            let result = call_tool(&st, name, &json!({})).await;
            let text = result["content"][0]["text"].as_str().unwrap_or_default();
            assert!(
                !text.contains("công cụ không tồn tại"),
                "`{name}` được liệt kê nhưng không có nhánh xử lý"
            );
        }
    }

    #[tokio::test]
    async fn missing_arguments_produce_an_error_result_not_a_panic() {
        let st = state();
        let r = call_tool(&st, "rule_get_chain", &json!({})).await;
        assert_eq!(r["isError"], true);
    }

    #[tokio::test]
    async fn registry_tool_lists_sources_separately() {
        let st = state();
        let r = call_tool(&st, "rule_registry", &json!({ "category": "source" })).await;
        let text = r["content"][0]["text"].as_str().unwrap();
        let v: Value = serde_json::from_str(text).unwrap();
        let rules = v["rules"].as_array().unwrap();
        assert!(!rules.is_empty());
        assert!(rules.iter().all(|r| r["isSource"] == true));
    }

    #[tokio::test]
    async fn create_update_and_validate_a_chain_through_mcp() {
        let st = state();
        let created = call_tool(&st, "rule_create_chain", &json!({ "name": "L1" })).await;
        let text = created["content"][0]["text"].as_str().unwrap();
        let id = serde_json::from_str::<Value>(text).unwrap()["chain"]["id"]
            .as_i64()
            .unwrap();

        let r = call_tool(
            &st,
            "rule_update_graph",
            &json!({
                "chainId": id,
                "nodes": [
                    { "id": "n1", "rule": "manual", "name": "Bắt đầu", "config": {}, "x": 0, "y": 0 },
                    { "id": "n2", "rule": "log", "name": "Ghi log", "config": {}, "x": 260, "y": 0 }
                ],
                "edges": [
                    { "id": "e1", "from": { "node": "n1", "port": "out" }, "to": { "node": "n2", "port": "in" }}
                ]
            }),
        )
        .await;
        let saved: Value = serde_json::from_str(r["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(saved["saved"], true);
        assert_eq!(saved["hasErrors"], false, "issues: {}", saved["issues"]);

        let v = call_tool(&st, "rule_validate", &json!({ "chainId": id })).await;
        let parsed: Value =
            serde_json::from_str(v["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(parsed["ok"], true);
    }

    #[test]
    fn json_block_survives_prose_and_fences() {
        assert!(parse_json_block("{\"a\":1}").is_some());
        assert!(parse_json_block("đây nhé:\n```json\n{\"a\":1}\n```").is_some());
        assert!(parse_json_block("trước {\"a\":1} sau").is_some());
        assert!(parse_json_block("không có json").is_none());
    }
}
