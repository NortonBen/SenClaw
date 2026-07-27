//! MCP server (HTTP/SSE) exposing the JSON toolbox to SenClaw agents.
//!
//! Nine tools over the same engine the UI uses: format/minify/sort, validate,
//! stats, schema inference, conversion between JSON/YAML/CSV/TSV/XML, JSON
//! Pointer queries, structural diff, and the base64/base64url/hex/URL/escape/
//! MessagePack/JWT codecs. Every tool is a pure function — no state, no network.

use axum::{
    extract::State,
    response::sse::{Event, Sse},
    Json,
};
use futures_util::stream::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{convert::Infallible, sync::Arc};

use crate::api::{type_name, AppState};
use crate::{analyze, codec, convert, fmt};

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
    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default())
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
fn result_of(r: Result<String, String>) -> Value {
    match r {
        Ok(output) => text_result(output),
        Err(e) => error_result(e),
    }
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
            "serverInfo": { "name": "json-mcp", "version": "2.0.0" }
        })),
        "ping" => reply(json!({})),
        "notifications/initialized" => Json(json!({ "jsonrpc": "2.0", "id": req.id, "result": {} })),
        "tools/list" => reply(json!({ "tools": tools_list() })),
        "tools/call" => {
            let params = req.params.clone().unwrap_or(json!({}));
            let name = params["name"].as_str().unwrap_or("").to_string();
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            reply(call_tool(&name, &args))
        }
        _ => Json(json!("ok")),
    }
}

fn tools_list() -> Value {
    json!([
        {
            "name": "json_format",
            "description": "Format (pretty-print), nén (minify) hoặc sắp xếp khoá A→Z cho một chuỗi JSON. Chế độ pretty/minify GIỮ NGUYÊN thứ tự khoá gốc. Báo lỗi kèm dòng/cột nếu JSON sai. Dùng cho 'format JSON / làm đẹp JSON / minify JSON / sắp xếp khoá'.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "input": { "type": "string", "description": "Chuỗi JSON cần format." },
                    "mode": { "type": "string", "enum": ["pretty", "minify", "sort"], "description": "pretty (mặc định), minify, hoặc sort (sắp khoá A→Z)." },
                    "indent": { "type": "number", "description": "Số khoảng trắng mỗi cấp. Mặc định 2." }
                },
                "required": ["input"]
            }
        },
        {
            "name": "json_validate",
            "description": "Kiểm tra một chuỗi có phải JSON hợp lệ không; nếu sai thì trả về thông báo lỗi kèm dòng/cột. Dùng cho 'JSON này có hợp lệ không / validate JSON / JSON lỗi ở đâu'.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "input": { "type": "string", "description": "Chuỗi cần kiểm tra." }
                },
                "required": ["input"]
            }
        },
        {
            "name": "json_stats",
            "description": "Tóm tắt cấu trúc một tài liệu JSON: kích thước, độ sâu, số node theo kiểu, số khoá, mảng dài nhất, khoá cấp cao nhất. Dùng ĐẦU TIÊN với tài liệu lớn để khỏi phải đọc hết.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "input": { "type": "string", "description": "Tài liệu JSON." }
                },
                "required": ["input"]
            }
        },
        {
            "name": "json_schema",
            "description": "Suy ra JSON Schema (draft-07) từ một tài liệu mẫu: kiểu, thuộc tính, required (chỉ khoá xuất hiện ở MỌI phần tử mảng), và format cho date/date-time/email/uri. Dùng cho 'sinh schema từ JSON này'.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "input": { "type": "string", "description": "Tài liệu JSON mẫu." }
                },
                "required": ["input"]
            }
        },
        {
            "name": "json_convert",
            "description": "Chuyển đổi dữ liệu giữa JSON, YAML, CSV, TSV và XML (mọi chiều). Đặt from = to để chỉ định dạng lại (json→json giữ thứ tự khoá, xml→xml giữ nguyên thuộc tính). Dùng cho 'JSON sang CSV / CSV sang JSON / JSON sang YAML / XML sang JSON / format XML'.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "input": { "type": "string", "description": "Dữ liệu nguồn." },
                    "from": { "type": "string", "enum": ["json", "yaml", "csv", "tsv", "xml"], "description": "Định dạng nguồn." },
                    "to": { "type": "string", "enum": ["json", "yaml", "csv", "tsv", "xml"], "description": "Định dạng đích." },
                    "root": { "type": "string", "description": "Tên thẻ gốc khi xuất XML. Mặc định 'root'." },
                    "columns": { "type": "array", "items": { "type": "string" }, "description": "Thứ tự cột khi xuất CSV/TSV. Bỏ trống = hợp tất cả khoá, sắp A→Z." },
                    "indent": { "type": "number", "description": "Số khoảng trắng mỗi cấp khi xuất JSON/XML. Mặc định 2." }
                },
                "required": ["input", "from", "to"]
            }
        },
        {
            "name": "json_query",
            "description": "Lấy một giá trị bên trong tài liệu JSON theo đường dẫn: JSON Pointer ('/a/b/0') hoặc dạng chấm ('a.b[0]'). Dùng thay cho việc đọc thủ công khi tài liệu dài.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "input": { "type": "string", "description": "Tài liệu JSON." },
                    "path": { "type": "string", "description": "Đường dẫn, ví dụ 'data.items[0].name' hoặc '/data/items/0/name'." }
                },
                "required": ["input", "path"]
            }
        },
        {
            "name": "json_diff",
            "description": "So sánh hai tài liệu JSON theo cấu trúc, trả về danh sách thay đổi (added/removed/changed) kèm đường dẫn. Dùng cho 'so sánh 2 JSON / khác nhau chỗ nào'.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "left": { "type": "string", "description": "JSON bên trái (bản gốc)." },
                    "right": { "type": "string", "description": "JSON bên phải (bản mới)." }
                },
                "required": ["left", "right"]
            }
        },
        {
            "name": "json_encode",
            "description": "Mã hoá dữ liệu: base64, base64url, hex, URL-encode, escape chuỗi JSON, hoặc JSON → MessagePack (trả base64). Dùng cho 'encode base64 / url encode / hex / escape JSON / msgpack'.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "input": { "type": "string", "description": "Chuỗi nguồn (phải là JSON hợp lệ nếu format = msgpack)." },
                    "format": { "type": "string", "enum": ["base64", "base64url", "hex", "url", "escape", "msgpack"], "description": "Kiểu mã hoá." }
                },
                "required": ["input", "format"]
            }
        },
        {
            "name": "json_decode",
            "description": "Giải mã dữ liệu: base64, base64url, hex, URL-decode, unescape chuỗi JSON, MessagePack (nhận base64), hoặc JWT (chỉ đọc header/payload, KHÔNG xác minh chữ ký). Dùng cho 'decode base64 / url decode / xem nội dung token'.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "input": { "type": "string", "description": "Chuỗi đã mã hoá." },
                    "format": { "type": "string", "enum": ["base64", "base64url", "hex", "url", "escape", "msgpack", "jwt"], "description": "Kiểu giải mã." }
                },
                "required": ["input", "format"]
            }
        }
    ])
}

fn arg_str(args: &Value, key: &str) -> String {
    args.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string()
}

fn arg_usize(args: &Value, key: &str, default: usize) -> usize {
    args.get(key)
        .and_then(|v| v.as_u64())
        .map(|n| n.min(16) as usize)
        .unwrap_or(default)
}

fn call_tool(name: &str, args: &Value) -> Value {
    match name {
        "json_format" => {
            let input = arg_str(args, "input");
            let indent = arg_usize(args, "indent", 2);
            let result = match arg_str(args, "mode").as_str() {
                "minify" => fmt::minify(&input),
                "sort" => fmt::sorted(&input, indent),
                _ => fmt::pretty(&input, indent),
            };
            match result {
                Ok(output) => text_result(output),
                Err(e) => error_result(format!("JSON không hợp lệ: {e}")),
            }
        }
        "json_validate" => {
            let input = arg_str(args, "input");
            match fmt::validate(&input) {
                Ok(v) => json_result(json!({
                    "valid": true,
                    "type": type_name(&v),
                    "bytes": input.len(),
                    "summary": format!("JSON hợp lệ ({}), {} byte", type_name(&v), input.len()),
                })),
                Err(e) => json_result(json!({
                    "valid": false,
                    "error": e.message,
                    "line": e.line,
                    "column": e.column,
                    "summary": format!("JSON KHÔNG hợp lệ: {} (dòng {}, cột {})", e.message, e.line, e.column),
                })),
            }
        }
        "json_stats" => match analyze::stats(&arg_str(args, "input")) {
            Ok(mut v) => {
                let depth = v["max_depth"].as_u64().unwrap_or(0);
                let nodes = v["nodes"].as_u64().unwrap_or(0);
                v["summary"] = json!(format!(
                    "{} · {} node · sâu {} cấp · {} byte",
                    v["root_type"].as_str().unwrap_or("?"),
                    nodes,
                    depth,
                    v["bytes"].as_u64().unwrap_or(0)
                ));
                json_result(v)
            }
            Err(e) => error_result(e),
        },
        "json_schema" => match analyze::infer_schema(&arg_str(args, "input")) {
            Ok(v) => result_of(serde_json::to_string_pretty(&v).map_err(|e| e.to_string())),
            Err(e) => error_result(e),
        },
        "json_convert" => {
            let columns: Vec<String> = args
                .get("columns")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|c| c.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            let root = match args.get("root").and_then(|v| v.as_str()) {
                Some(r) if !r.trim().is_empty() => r.trim().to_string(),
                _ => "root".to_string(),
            };
            result_of(convert::convert(
                &arg_str(args, "from"),
                &arg_str(args, "to"),
                &arg_str(args, "input"),
                &root,
                &columns,
                arg_usize(args, "indent", 2),
            ))
        }
        "json_query" => {
            let path = arg_str(args, "path");
            match convert::query(&arg_str(args, "input"), &path) {
                Ok(v) => json_result(json!({
                    "path": path,
                    "pointer": convert::to_pointer(&path),
                    "type": type_name(&v),
                    "value": v,
                })),
                Err(e) => error_result(e),
            }
        }
        "json_diff" => match convert::diff(&arg_str(args, "left"), &arg_str(args, "right")) {
            Ok(mut v) => {
                let equal = v["equal"].as_bool().unwrap_or(false);
                let count = v["count"].as_u64().unwrap_or(0);
                v["summary"] = json!(if equal {
                    "Hai tài liệu JSON giống hệt nhau.".to_string()
                } else {
                    format!("Có {count} khác biệt.")
                });
                json_result(v)
            }
            Err(e) => error_result(e),
        },
        "json_encode" => result_of(codec::encode(
            &arg_str(args, "format"),
            &arg_str(args, "input"),
        )),
        "json_decode" => result_of(codec::decode(
            &arg_str(args, "format"),
            &arg_str(args, "input"),
        )),
        _ => error_result(format!("Unknown tool: {name}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(v: &Value) -> String {
        v["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    }

    #[test]
    fn every_listed_tool_is_callable() {
        let tools = tools_list();
        let list = tools.as_array().unwrap();
        assert_eq!(list.len(), 9, "tool count changed — update the docs/skill too");
        for tool in list {
            let name = tool["name"].as_str().unwrap();
            // Empty args are invalid input, not an unknown tool.
            assert!(
                !text_of(&call_tool(name, &json!({}))).starts_with("Unknown tool"),
                "{name} is listed but not implemented"
            );
        }
        assert!(text_of(&call_tool("json_nope", &json!({}))).starts_with("Unknown tool"));
    }

    #[test]
    fn format_modes() {
        let src = json!({ "input": "{\"b\":1,\"a\":2}" });
        assert_eq!(text_of(&call_tool("json_format", &src)), "{\n  \"b\": 1,\n  \"a\": 2\n}");
        let mut minify = src.clone();
        minify["mode"] = json!("minify");
        assert_eq!(text_of(&call_tool("json_format", &minify)), r#"{"b":1,"a":2}"#);
        let mut sort = src.clone();
        sort["mode"] = json!("sort");
        assert_eq!(text_of(&call_tool("json_format", &sort)), "{\n  \"a\": 2,\n  \"b\": 1\n}");
    }

    #[test]
    fn convert_and_reformat() {
        let csv = call_tool(
            "json_convert",
            &json!({ "input": "[{\"a\":1,\"b\":2}]", "from": "json", "to": "csv" }),
        );
        assert_eq!(text_of(&csv), "a,b\n1,2\n");
        let xml = call_tool(
            "json_convert",
            &json!({ "input": "<a><b>1</b></a>", "from": "xml", "to": "xml" }),
        );
        assert_eq!(text_of(&xml), "<a>\n  <b>1</b>\n</a>");
    }

    #[test]
    fn stats_and_schema_tools() {
        let s: Value =
            serde_json::from_str(&text_of(&call_tool("json_stats", &json!({ "input": "[1,2,3]" }))))
                .unwrap();
        assert_eq!(s["root_type"], json!("array"));
        assert!(s["summary"].as_str().unwrap().contains("node"));

        let schema: Value = serde_json::from_str(&text_of(&call_tool(
            "json_schema",
            &json!({ "input": r#"{"a":1}"# }),
        )))
        .unwrap();
        assert_eq!(schema["properties"]["a"]["type"], json!("integer"));
    }

    #[test]
    fn invalid_input_returns_is_error() {
        assert_eq!(
            call_tool("json_format", &json!({ "input": "{oops" }))["isError"],
            json!(true)
        );
        assert_eq!(
            call_tool(
                "json_convert",
                &json!({ "input": "{}", "from": "json", "to": "parquet" })
            )["isError"],
            json!(true)
        );
        assert_eq!(
            call_tool("json_stats", &json!({ "input": "nope" }))["isError"],
            json!(true)
        );
    }

    #[test]
    fn validate_reports_invalid_without_is_error() {
        let out = call_tool("json_validate", &json!({ "input": "{\"a\":}" }));
        assert!(out.get("isError").is_none());
        let parsed: Value = serde_json::from_str(&text_of(&out)).unwrap();
        assert_eq!(parsed["valid"], json!(false));
        assert!(parsed["line"].as_u64().is_some());
    }

    #[test]
    fn codec_tools_cover_every_codec() {
        for format in ["base64", "base64url", "hex", "url", "escape"] {
            let enc = call_tool("json_encode", &json!({ "input": "xin chào", "format": format }));
            let dec = call_tool(
                "json_decode",
                &json!({ "input": text_of(&enc), "format": format }),
            );
            assert_eq!(text_of(&dec), "xin chào", "codec {format}");
        }
        // jwt is decode-only.
        assert_eq!(
            call_tool("json_encode", &json!({ "input": "a.b.c", "format": "jwt" }))["isError"],
            json!(true)
        );
    }

    #[test]
    fn indent_is_capped() {
        assert_eq!(arg_usize(&json!({ "indent": 9999 }), "indent", 2), 16);
    }
}
