//! MCP server `secscan-mcp` — JSON-RPC viết tay trên axum, cùng khuôn với các
//! Space App khác trong repo (không dùng rmcp).
//!
//! Mọi tool đều gọi lại `api::*_value()` mà REST dùng, để agent và người không
//! bao giờ thấy hành vi khác nhau.

use crate::api::{self, AppState};
use axum::extract::State;
use axum::response::sse::{Event, Sse};
use axum::Json;
use futures_util::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::convert::Infallible;

#[derive(Deserialize, Debug)]
pub struct JsonRpcRequest {
    #[allow(dead_code)]
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    pub params: Option<Value>,
}

pub async fn mcp_sse(
    State(state): State<AppState>,
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
fn json_result(v: &Value) -> Value {
    text_result(serde_json::to_string_pretty(v).unwrap_or_default())
}
fn error_result(text: String) -> Value {
    json!({ "isError": true, "content": [{ "type": "text", "text": text }] })
}

pub async fn mcp_message(
    State(state): State<AppState>,
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
            "serverInfo": { "name": "secscan-mcp", "version": "0.1.0" }
        })),
        "ping" => reply(json!({})),
        "notifications/initialized" => Json(json!({ "jsonrpc": "2.0", "id": req.id, "result": {} })),
        "tools/list" => reply(json!({ "tools": tools_list() })),
        "tools/call" => {
            let params = req.params.clone().unwrap_or_default();
            let name = params["name"].as_str().unwrap_or("").to_string();
            let args = params["arguments"].clone();
            reply(call_tool(&state, &name, &args).await)
        }
        _ => Json(json!("ok")),
    }
}

fn as_i64(a: &Value, k: &str) -> Option<i64> {
    a.get(k).and_then(|v| v.as_i64())
}
fn as_str<'a>(a: &'a Value, k: &str) -> Option<&'a str> {
    a.get(k).and_then(|v| v.as_str())
}

pub fn tools_list() -> Value {
    json!([
        {
            "name": "sec_asset_add",
            "description": "Thêm một tài sản vào sổ. kind: website (có URL) | host (máy chủ qua SSH) | domain (chỉ kiểm DNS). \
                            Thêm xong MỚI chỉ chạy được lớp thụ động; muốn quét chủ động phải xác minh sở hữu bằng \
                            sec_asset_verify_token rồi sec_asset_verify.",
            "inputSchema": { "type": "object", "properties": {
                "kind":   { "type": "string", "enum": ["website", "host", "domain"] },
                "target": { "type": "string", "description": "https://a.vn hoặc a.vn hoặc ssh://user@1.2.3.4" },
                "label":  { "type": "string" }
            }, "required": ["kind", "target"] }
        },
        {
            "name": "sec_asset_list",
            "description": "Liệt kê tài sản kèm trạng thái xác minh. Gọi cái này TRƯỚC khi quét để biết id và biết tài sản đã xác minh chưa.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "sec_asset_verify_token",
            "description": "Sinh token xác minh sở hữu và trả hướng dẫn đặt bằng chứng. Chưa kiểm gì cả — sau khi người dùng đặt xong thì gọi sec_asset_verify. \
                            Độ mạnh giảm dần: dns-txt > dns-cname > well-known > meta. Dùng dns-cname khi apex đã bị CNAME-flatten nên không gắn được TXT.",
            "inputSchema": { "type": "object", "properties": {
                "asset_id": { "type": "number" },
                "method":   { "type": "string", "enum": ["dns-txt", "dns-cname", "well-known", "meta", "local"] }
            }, "required": ["asset_id", "method"] }
        },
        {
            "name": "sec_asset_verify",
            "description": "Kiểm bằng chứng sở hữu có thật sự tồn tại không. Phải chạy sec_asset_verify_token trước. \
                            Bản ghi phải được GIỮ LẠI lâu dài — mất nó là phạm vi bị thu hồi ở lần kiểm sau.",
            "inputSchema": { "type": "object", "properties": { "asset_id": { "type": "number" } }, "required": ["asset_id"] }
        },
        {
            "name": "sec_asset_remove",
            "description": "Xoá tài sản cùng toàn bộ lần quét và phát hiện của nó. KHÔNG hoàn tác được.",
            "inputSchema": { "type": "object", "properties": { "asset_id": { "type": "number" } }, "required": ["asset_id"] }
        },
        {
            "name": "sec_scan_web",
            "description": "Quét THỤ ĐỘNG một website: security header, cờ cookie, lộ thông tin phiên bản, và tư thế DNS (SPF/DMARC/CAA/DNSSEC). \
                            Một GET duy nhất, không gửi payload tấn công nào — an toàn để chạy trên production và KHÔNG đòi xác minh sở hữu. \
                            Trả về điểm 0-100 và hạng A+..F.",
            "inputSchema": { "type": "object", "properties": { "asset_id": { "type": "number" } }, "required": ["asset_id"] }
        },
        {
            "name": "sec_scan_active",
            "description": "Quét CHỦ ĐỘNG (L2): dò tệp lộ ra ngoài (.git/, .env, backup, kết xuất CSDL), liệt kê thư mục, và cấu hình CORS. \
                            Có gửi yêu cầu tới đường dẫn không công khai nên khác hẳn quét thụ động, nhưng KHÔNG khai thác, \
                            không brute-force. Nhịp cố ý thấp (~4 req/s, trần 40 yêu cầu) — chạy được trên production. \
                            Nếu 'truncated' là true thì kết quả BÁN PHẦN, phải nói với người dùng.",
            "inputSchema": { "type": "object", "properties": { "asset_id": { "type": "number" } }, "required": ["asset_id"] }
        },
        {
            "name": "sec_scan_host",
            "description": "Quét L3: kiểm cấu hình máy chủ qua SSH (chỉ đọc — có test cưỡng chế không lệnh nào có động từ ghi). \
                            Tài sản phải có `ssh_ref` trỏ tới id máy bên app ssh-manager; secscan KHÔNG BAO GIỜ giữ mật khẩu \
                            hay khoá riêng. Cần biến môi trường SECSCAN_SSH_MANAGER_URL vì ssh-manager dùng cổng động. \
                            Đối chiếu OSV/KEV/EPSS luôn cho danh sách gói OS lấy được.",
            "inputSchema": { "type": "object", "properties": { "asset_id": { "type": "number" } }, "required": ["asset_id"] }
        },
        {
            "name": "sec_scans",
            "description": "Lịch sử các lần quét, mới nhất trước.",
            "inputSchema": { "type": "object", "properties": {
                "asset_id": { "type": "number" }, "limit": { "type": "number" }
            } }
        },
        {
            "name": "sec_scan_get",
            "description": "Chi tiết một lần quét kèm toàn bộ phát hiện.",
            "inputSchema": { "type": "object", "properties": { "scan_id": { "type": "number" } }, "required": ["scan_id"] }
        },
        {
            "name": "sec_findings",
            "description": "Danh sách phát hiện, xếp theo mức nặng giảm dần. Lọc được theo scan_id / asset_id / severity. \
                            LUÔN gọi cái này thay vì nhớ lại từ hội thoại trước — số liệu phải lấy từ công cụ.",
            "inputSchema": { "type": "object", "properties": {
                "scan_id":  { "type": "number" },
                "asset_id": { "type": "number" },
                "severity": { "type": "string", "enum": ["critical", "high", "medium", "low", "info"] }
            } }
        },
        {
            "name": "sec_finding_status",
            "description": "Đổi trạng thái một phát hiện. 'acked' = chấp nhận rủi ro (nên kèm lý do), 'fixed' = đã vá. \
                            Đã đánh dấu fixed mà lần quét sau nó quay lại thì hệ thống tự chuyển thành 'regressed'.",
            "inputSchema": { "type": "object", "properties": {
                "finding_id": { "type": "number" },
                "status":     { "type": "string", "enum": ["open", "acked", "fixed"] },
                "reason":     { "type": "string" }
            }, "required": ["finding_id", "status"] }
        },
        {
            "name": "sec_diff",
            "description": "So hai lần quét: cái gì MỚI xuất hiện, cái gì đã hết. Dùng để trả lời 'từ lần trước tới giờ có gì đổi' \
                            thay vì đọc lại cả danh sách phẳng.",
            "inputSchema": { "type": "object", "properties": {
                "from_scan": { "type": "number" }, "to_scan": { "type": "number" }
            }, "required": ["from_scan", "to_scan"] }
        },
        {
            "name": "sec_rules",
            "description": "Danh mục tiêu chuẩn quét: app kiểm những gì, mức nặng tối đa mỗi mục, LÝ DO đặt mức đó, và mục nào CHƯA cài. \
                            Gọi khi người dùng hỏi 'quét những gì', 'có kiểm X không', hoặc trước khi kết luận 'không có vấn đề' — \
                            trường not_covered nói rõ loại lỗ hổng nào công cụ tự động không thấy được.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "sec_dashboard",
            "description": "Tổng hợp một tài sản: xu hướng điểm qua các lần quét, phân bố theo mức và theo nhóm, \
                            5 mục nặng nhất còn mở (KEV lên đầu), số mục tái phát và số đã chấp nhận rủi ro. \
                            Dùng khi người dùng hỏi 'tình hình thế nào' thay vì đọc từng phát hiện.",
            "inputSchema": { "type": "object", "properties": { "asset_id": { "type": "number" } } }
        },
        {
            "name": "sec_rule_add",
            "description": "Thêm một luật quét TỰ VIẾT — luật này CHẠY THẬT, không phải chỉ ghi chú. \
                            Dạng khai báo: so khớp trên header HTTP, thuộc tính cookie, hoặc bản ghi TXT. \
                            id BẮT BUỘC bắt đầu bằng 'custom:' để không lẫn với luật dựng sẵn. \
                            Ví dụ bắt mọi phản hồi phải có header nội bộ: \
                            {id:'custom:x-req-id', title:'Thiếu X-Request-Id', severity:'low', \
                             check:{target:'header', name:'x-request-id', op:'present'}}",
            "inputSchema": { "type": "object", "properties": {
                "id":        { "type": "string", "description": "phải bắt đầu bằng 'custom:'" },
                "title":     { "type": "string" },
                "severity":  { "type": "string", "enum": ["critical", "high", "medium", "low", "info"] },
                "rationale": { "type": "string", "description": "vì sao kiểm — hiện trong báo cáo" },
                "fix":       { "type": "string", "description": "cách sửa" },
                "check": {
                    "type": "object",
                    "properties": {
                        "target": { "type": "string", "enum": ["header", "cookie_attr", "dns_txt"] },
                        "name":   { "type": "string", "description": "tên header, hoặc thuộc tính cookie (secure/httponly/samesite). Bỏ trống với dns_txt." },
                        "op":     { "type": "string", "enum": ["present", "absent", "equals", "contains", "not_contains", "regex", "not_regex"] },
                        "value":  { "type": "string" }
                    },
                    "required": ["target", "op"]
                }
            }, "required": ["id", "title", "severity", "check"] }
        },
        {
            "name": "sec_rule_import",
            "description": "Nhập bộ luật từ URL https:// hoặc từ JSON dán vào. MẶC ĐỊNH CHỈ XEM TRƯỚC (apply=false): \
                            trả về luật nào hợp lệ, luật nào bị loại và vì sao. Chỉ khi người dùng đã xem và ĐỒNG Ý \
                            thì mới gọi lại với apply=true. \
                            Nguồn ngoài đi qua cùng bộ chặn SSRF như đích quét, và chỉ nhận https.",
            "inputSchema": { "type": "object", "properties": {
                "url":   { "type": "string", "description": "https://… trỏ tới ruleset JSON" },
                "json":  { "type": "string", "description": "hoặc dán thẳng nội dung JSON" },
                "apply": { "type": "boolean", "description": "false = chỉ xem trước (mặc định). true = lưu thật." }
            } }
        },
        {
            "name": "sec_rule_remove",
            "description": "Xoá một luật tự thêm theo id. Không dùng được cho luật dựng sẵn — muốn tắt luật dựng sẵn thì dùng sec_rule_override.",
            "inputSchema": { "type": "object", "properties": { "id": { "type": "string" } }, "required": ["id"] }
        },
        {
            "name": "sec_rule_override",
            "description": "Chỉnh cách chấm một luật DỰNG SẴN: đổi mức, hoặc tắt hẳn. Khớp theo TIỀN TỐ nên 'hdr:csp' phủ cả họ luật CSP. \
                            enabled=false thì phát hiện bị loại hẳn khỏi kết quả, không phải hạ xuống info. \
                            Gọi không kèm severity/note và enabled=true thì xoá ghi đè, trả luật về mặc định.",
            "inputSchema": { "type": "object", "properties": {
                "rule_id":  { "type": "string", "description": "id hoặc tiền tố, ví dụ 'hdr:csp' hay 'dns:caa:missing'" },
                "severity": { "type": "string", "enum": ["critical", "high", "medium", "low", "info"] },
                "enabled":  { "type": "boolean" },
                "note":     { "type": "string", "description": "lý do — nên ghi, để sau còn biết vì sao tắt" }
            }, "required": ["rule_id"] }
        },
        {
            "name": "sec_activity",
            "description": "Nhật ký hoạt động của app.",
            "inputSchema": { "type": "object", "properties": { "limit": { "type": "number" } } }
        }
    ])
}

pub async fn call_tool(s: &AppState, name: &str, args: &Value) -> Value {
    let need = |k: &str| error_result(format!("thiếu tham số bắt buộc: {k}"));

    match name {
        "sec_asset_add" => {
            let Some(kind) = as_str(args, "kind") else { return need("kind") };
            let Some(target) = as_str(args, "target") else { return need("target") };
            let b = api::AssetIn {
                kind: kind.to_string(),
                target: target.to_string(),
                label: as_str(args, "label").unwrap_or("").to_string(),
            };
            json_result(&api::add_asset_value(s, &b))
        }
        "sec_asset_list" => json_result(&json!({ "ok": true, "assets": s.db.list_assets() })),
        "sec_asset_verify_token" => {
            let Some(asset_id) = as_i64(args, "asset_id") else { return need("asset_id") };
            let Some(method) = as_str(args, "method") else { return need("method") };
            let b = api::VerifyIn { asset_id, method: method.to_string() };
            json_result(&api::verify_token_value(s, &b))
        }
        "sec_asset_verify" => {
            let Some(asset_id) = as_i64(args, "asset_id") else { return need("asset_id") };
            json_result(&api::verify_run_value(s, asset_id).await)
        }
        "sec_asset_remove" => {
            let Some(asset_id) = as_i64(args, "asset_id") else { return need("asset_id") };
            json_result(&match s.db.delete_asset(asset_id) {
                Ok(()) => json!({ "ok": true }),
                Err(e) => api::err(e),
            })
        }
        "sec_scan_web" => {
            let Some(asset_id) = as_i64(args, "asset_id") else { return need("asset_id") };
            json_result(&api::scan_passive_value(s, &api::ScanIn { asset_id }).await)
        }
        "sec_scan_active" => {
            let Some(asset_id) = as_i64(args, "asset_id") else { return need("asset_id") };
            json_result(&api::scan_active_value(s, &api::ScanIn { asset_id }).await)
        }
        "sec_scan_host" => {
            let Some(asset_id) = as_i64(args, "asset_id") else { return need("asset_id") };
            json_result(&api::scan_host_value(s, asset_id).await)
        }
        "sec_scans" => json_result(&json!({
            "ok": true,
            "scans": s.db.list_scans(as_i64(args, "asset_id"), as_i64(args, "limit").unwrap_or(50)),
        })),
        "sec_scan_get" => {
            let Some(id) = as_i64(args, "scan_id") else { return need("scan_id") };
            json_result(&match s.db.get_scan(id) {
                Some(v) => json!({ "ok": true, "scan": v, "findings": s.db.findings(Some(id), None, None) }),
                None => api::err("không có lần quét này"),
            })
        }
        "sec_findings" => {
            let q = api::FindingsQuery {
                scan_id: as_i64(args, "scan_id"),
                asset_id: as_i64(args, "asset_id"),
                severity: as_str(args, "severity").map(|x| x.to_string()),
            };
            json_result(&api::findings_value(s, &q))
        }
        "sec_finding_status" => {
            let Some(id) = as_i64(args, "finding_id") else { return need("finding_id") };
            let Some(status) = as_str(args, "status") else { return need("status") };
            let b = api::StatusIn {
                status: status.to_string(),
                reason: as_str(args, "reason").map(|x| x.to_string()),
            };
            json_result(&api::set_status_value(s, id, &b))
        }
        "sec_diff" => {
            let Some(from) = as_i64(args, "from_scan") else { return need("from_scan") };
            let Some(to) = as_i64(args, "to_scan") else { return need("to_scan") };
            json_result(&s.db.diff(from, to))
        }
        "sec_rules" => {
            let mut v = crate::rules::to_json();
            // Gộp luật tự thêm + ghi đè vào cùng câu trả lời: agent hỏi "kiểm gì"
            // phải thấy TOÀN BỘ, không chỉ phần dựng sẵn.
            let extra = api::custom_rules_value(s);
            v["custom"] = extra["custom"].clone();
            v["overrides"] = extra["overrides"].clone();
            json_result(&v)
        }
        "sec_rule_add" => json_result(&api::rule_add_value(s, args)),
        "sec_rule_import" => {
            let b = api::ImportIn {
                url: as_str(args, "url").map(|x| x.to_string()),
                json: as_str(args, "json").map(|x| x.to_string()),
                apply: args.get("apply").and_then(|x| x.as_bool()).unwrap_or(false),
            };
            json_result(&api::rule_import_value(s, &b).await)
        }
        "sec_rule_remove" => {
            let Some(id) = as_str(args, "id") else { return need("id") };
            json_result(&api::rule_remove_value(s, id))
        }
        "sec_rule_override" => {
            let Some(rule_id) = as_str(args, "rule_id") else { return need("rule_id") };
            let b = api::OverrideIn {
                rule_id: rule_id.to_string(),
                severity: as_str(args, "severity").map(|x| x.to_string()),
                enabled: args.get("enabled").and_then(|x| x.as_bool()).unwrap_or(true),
                note: as_str(args, "note").map(|x| x.to_string()),
            };
            json_result(&api::override_set_value(s, &b))
        }
        "sec_dashboard" => json_result(&api::dashboard_value(s, as_i64(args, "asset_id"))),
        "sec_activity" => json_result(&json!({
            "ok": true,
            "activity": s.db.activity(as_i64(args, "limit").unwrap_or(50)),
        })),
        _ => error_result(format!("không có tool tên '{name}'")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listed_tool_uses_the_sec_prefix() {
        // Quy ước đặt tên MCP của repo: mcp__secscan-mcp__sec_*
        for t in tools_list().as_array().unwrap() {
            let n = t["name"].as_str().unwrap();
            assert!(n.starts_with("sec_"), "{n} phải có tiền tố sec_");
            assert!(!t["description"].as_str().unwrap_or("").is_empty(), "{n} thiếu mô tả");
            assert_eq!(t["inputSchema"]["type"], "object", "{n} thiếu inputSchema");
        }
    }

    #[test]
    fn required_params_are_declared_in_schema() {
        let tools = tools_list();
        let find = |n: &str| {
            tools
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["name"] == n)
                .unwrap()
                .clone()
        };
        for (tool, param) in [
            ("sec_scan_web", "asset_id"),
            ("sec_asset_verify", "asset_id"),
            ("sec_scan_get", "scan_id"),
            ("sec_finding_status", "finding_id"),
        ] {
            let req = find(tool)["inputSchema"]["required"].clone();
            assert!(
                req.as_array().unwrap().iter().any(|x| x == param),
                "{tool} phải khai {param} là bắt buộc"
            );
        }
    }

    #[tokio::test]
    async fn unknown_tool_returns_an_error_result_not_a_panic() {
        let (mcp_tx, _) = tokio::sync::broadcast::channel(100);
        let s = AppState {
            db: std::sync::Arc::new(crate::db::Db::open_memory().unwrap()),
            http: crate::scan::http_client(),
            sc: app_space_sdk::SpaceClient::from_env(),
            mcp_tx,
        };
        let r = call_tool(&s, "sec_nope", &json!({})).await;
        assert_eq!(r["isError"], true);
    }

    #[tokio::test]
    async fn missing_required_arg_is_reported_by_name() {
        let (mcp_tx, _) = tokio::sync::broadcast::channel(100);
        let s = AppState {
            db: std::sync::Arc::new(crate::db::Db::open_memory().unwrap()),
            http: crate::scan::http_client(),
            sc: app_space_sdk::SpaceClient::from_env(),
            mcp_tx,
        };
        let r = call_tool(&s, "sec_scan_web", &json!({})).await;
        assert_eq!(r["isError"], true);
        assert!(r["content"][0]["text"].as_str().unwrap().contains("asset_id"));
    }
}
