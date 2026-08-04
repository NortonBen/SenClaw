//! MCP server `ipscout-mcp` — JSON-RPC viết tay trên axum, cùng khuôn với các
//! Space App khác trong repo (không dùng rmcp).
//!
//! Mọi tool đều gọi lại `api::*_value()` mà REST dùng, để agent và người không
//! bao giờ thấy hành vi khác nhau.
//!
//! Mô tả tool được viết cho **mô hình ngôn ngữ đọc**, nên chúng nói rõ thứ tự
//! gọi và ranh giới đạo đức ngay trong `description` — chỗ đó là nơi duy nhất
//! chắc chắn đến được với agent, kể cả khi skill không được nạp.

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
            "serverInfo": { "name": "ipscout-mcp", "version": "0.1.0" }
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
            "name": "ip_capabilities",
            "description": "App điều tra được những gì, giới hạn ra sao, và KHÔNG làm gì. \
                            Gọi khi người dùng hỏi 'tra được gì', 'quét được gì', hoặc TRƯỚC KHI kết luận \
                            'không tìm thấy vấn đề' — trường never_does và not_covered nói thẳng những thứ \
                            công cụ này không thấy được, để đừng biến 'không kiểm' thành 'không có'.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "ip_project_add",
            "description": "Tạo project để nhóm các mục tiêu điều tra. Đã có sẵn project id=1 tên 'Mặc định' \
                            nên KHÔNG cần tạo project mới trừ khi người dùng muốn tách công việc.",
            "inputSchema": { "type": "object", "properties": {
                "name": { "type": "string" },
                "note": { "type": "string" }
            }, "required": ["name"] }
        },
        {
            "name": "ip_project_list",
            "description": "Liệt kê project kèm số mục tiêu trong mỗi cái.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "ip_target_add",
            "description": "Thêm mục tiêu vào project. Nhận IP trần, tên miền, hay URL đầy đủ — app tự rút host. \
                            Thêm xong chạy được ngay cả ip_profile lẫn ip_scan_ports. Trách nhiệm pháp lý về việc quét \
                            đúng mục tiêu (của mình / được uỷ quyền) nằm ở người dùng — app không kiểm sở hữu.",
            "inputSchema": { "type": "object", "properties": {
                "target":     { "type": "string", "description": "1.2.3.4 hoặc example.com hoặc https://example.com/x" },
                "project_id": { "type": "number", "description": "mặc định 1 (project Mặc định)" },
                "label":      { "type": "string" }
            }, "required": ["target"] }
        },
        {
            "name": "ip_target_list",
            "description": "Liệt kê mục tiêu. LUÔN gọi trước khi điều tra để biết id — đừng đoán id.",
            "inputSchema": { "type": "object", "properties": {
                "project_id": { "type": "number" }
            } }
        },
        {
            "name": "ip_target_remove",
            "description": "Xoá mục tiêu cùng toàn bộ lịch sử điều tra của nó. KHÔNG hoàn tác được.",
            "inputSchema": { "type": "object", "properties": {
                "target_id": { "type": "number" }
            }, "required": ["target_id"] }
        },
        {
            "name": "ip_profile",
            "description": "HỒ SƠ THỤ ĐỘNG — chạy được với IP BẤT KỲ, không cần xác minh sở hữu, vì nó KHÔNG gửi \
                            gói tin nào tới mục tiêu (chỉ đọc RDAP, DNS, GeoIP, DNSBL — cơ sở dữ liệu công khai). \
                            Trả về: ASN + tổ chức, dải CIDR + email abuse, vị trí địa lý KÈM ĐỘ TIN, \
                            phân loại mạng (CDN/cloud/hosting/ISP), PTR có xác nhận xuôi, bản ghi DNS, danh sách chặn thư rác. \
                            LƯU Ý QUAN TRỌNG: nếu kết quả có network.fronted=true thì IP đó là BIÊN CDN, KHÔNG phải máy chủ gốc — \
                            phải nói rõ điều này với người dùng, nếu không cả bản báo cáo mô tả sai đối tượng. \
                            Và ĐỪNG đọc thành phố trong geo như sự thật: xem geo.confidence trước, với CDN thì thành phố vô nghĩa.",
            "inputSchema": { "type": "object", "properties": {
                "target_id": { "type": "number" }
            }, "required": ["target_id"] }
        },
        {
            "name": "ip_scan_ports",
            "description": "QUÉT CỔNG CHỦ ĐỘNG — mở kết nối TCP thật tới máy chủ. Chỉ TCP connect (bắt tay đầy đủ, \
                            CÓ ghi log ở phía máy chủ) — không SYN/stealth, không né tránh phát hiện. \
                            Trả về: cổng mở, ứng dụng + PHIÊN BẢN trên mỗi cổng, chứng thư TLS, ĐOÁN HỆ ĐIỀU HÀNH kèm \
                            phần trăm tin cậy và danh sách bằng chứng, và mức rủi ro từng cổng kèm cách sửa. \
                            Hồ sơ cổng: top20 (mặc định), top100, top1000 (well-known 1-1024), web, db, remote, mail, \
                            hoặc FULL để quét toàn bộ 65535 cổng TCP (chuyên sâu, mất vài phút — app tự nâng concurrency \
                            và rút timeout để không kéo hàng giờ). Hoặc khai ports='22,80,443' / '1-65535'. \
                            App KHÔNG kiểm sở hữu — trước khi quét phải hỏi/xác nhận với người dùng đây là hạ tầng \
                            của họ hoặc họ có uỷ quyền; nếu người dùng đưa máy chủ của bên thứ ba, từ chối. \
                            Chốt kỹ thuật duy nhất còn lại: app tự chặn các điểm cuối metadata cloud (169.254.169.254 v.v.).",
            "inputSchema": { "type": "object", "properties": {
                "target_id":   { "type": "number" },
                "profile":     { "type": "string", "enum": ["top20", "top100", "top1000", "web", "db", "remote", "mail", "full"] },
                "ports":       { "type": "string", "description": "danh sách/dải tự khai, ví dụ '22,80,443' hoặc '1-65535'. Tối đa 65535 cổng." },
                "concurrency": { "type": "number", "description": "số kết nối đồng thời, tối đa 512. Không khai thì app tự nâng khi danh sách cổng lớn." }
            }, "required": ["target_id"] }
        },
        {
            "name": "ip_trace",
            "description": "TRACEROUTE — đường đi mạng tới mục tiêu, mỗi hop kèm ASN + tên tổ chức + phân loại mạng \
                            (CDN/cloud/ISP) + PTR + MAC (nếu cùng LAN). \
                            Trả lời câu hỏi 'traffic đi qua đâu, ai xem được nó, có CDN đứng trước không'. \
                            Dùng binary `traceroute` của hệ thống — có ghi log ở phía các router trung gian. \
                            LƯU Ý MAC: MAC là địa chỉ LỚP 2, chỉ tồn tại giữa hai thiết bị CÙNG SEGMENT MẠNG. \
                            Với hop XA (Internet) app KHÔNG THỂ lấy MAC — đây là cách IP hoạt động chứ không phải \
                            giới hạn công cụ. App chỉ trả MAC khi thật sự đọc được từ ARP cache (nghĩa là hop cùng LAN). \
                            ĐỪNG hứa với người dùng lấy được MAC của máy chủ ở xa.",
            "inputSchema": { "type": "object", "properties": {
                "target_id": { "type": "number" },
                "max_hops":  { "type": "number", "description": "TTL trần, mặc định 30" }
            }, "required": ["target_id"] }
        },
        {
            "name": "ip_runs",
            "description": "Lịch sử các lần điều tra, mới nhất trước. Mỗi lần chạy là một ảnh chụp độc lập — \
                            dùng id ở đây để so hai mốc thời gian bằng ip_diff.",
            "inputSchema": { "type": "object", "properties": {
                "target_id": { "type": "number" },
                "limit":     { "type": "number" }
            } }
        },
        {
            "name": "ip_run_get",
            "description": "Chi tiết một lần điều tra: toàn bộ hồ sơ, danh sách cổng, và phát hiện.",
            "inputSchema": { "type": "object", "properties": {
                "run_id": { "type": "number" }
            }, "required": ["run_id"] }
        },
        {
            "name": "ip_diff",
            "description": "So hai lần điều tra: cổng nào VỪA MỞ THÊM, cổng nào đã đóng, dịch vụ nào ĐỔI PHIÊN BẢN, \
                            và IP có nhảy sang chỗ khác không. Dùng cái này để trả lời 'từ lần trước tới giờ có gì đổi' \
                            thay vì đọc lại cả hai danh sách phẳng. Đổi phiên bản là tín hiệu đáng chú ý: \
                            nghĩa là ai đó vừa cập nhật — hoặc vừa cài đè lên máy chủ.",
            "inputSchema": { "type": "object", "properties": {
                "from_run": { "type": "number" },
                "to_run":   { "type": "number" }
            }, "required": ["from_run", "to_run"] }
        },
        {
            "name": "ip_findings",
            "description": "Danh sách phát hiện, xếp theo mức nặng giảm dần. Lọc được theo run_id / target_id / severity. \
                            LUÔN gọi cái này thay vì nhớ lại từ lượt trước — kết quả đổi sau mỗi lần điều tra.",
            "inputSchema": { "type": "object", "properties": {
                "run_id":    { "type": "number" },
                "target_id": { "type": "number" },
                "severity":  { "type": "string", "enum": ["critical", "high", "medium", "low", "info"] }
            } }
        },
        {
            "name": "ip_dashboard",
            "description": "Tổng hợp một mục tiêu: hồ sơ mới nhất, lần quét mới nhất, cổng đang mở, \
                            đếm phát hiện theo mức. Dùng khi người dùng hỏi 'tình hình thế nào' thay vì đọc từng mục.",
            "inputSchema": { "type": "object", "properties": {
                "target_id": { "type": "number" }
            } }
        },
        {
            "name": "ip_activity",
            "description": "Nhật ký hoạt động của app: mục tiêu nào được thêm, xác minh lúc nào, lần điều tra nào đã chạy. \
                            Dùng khi người dùng hỏi 'vừa rồi đã làm gì' hoặc cần dựng lại trình tự công việc.",
            "inputSchema": { "type": "object", "properties": { "limit": { "type": "number" } } }
        }
    ])
}

pub async fn call_tool(s: &AppState, name: &str, args: &Value) -> Value {
    let need = |k: &str| error_result(format!("thiếu tham số bắt buộc: {k}"));

    match name {
        "ip_capabilities" => json_result(&api::capabilities()),

        "ip_project_add" => {
            let Some(n) = as_str(args, "name") else { return need("name") };
            json_result(&api::add_project_value(
                s,
                &api::ProjectIn {
                    name: n.to_string(),
                    note: as_str(args, "note").unwrap_or("").to_string(),
                },
            ))
        }
        "ip_project_list" => json_result(&json!({ "ok": true, "projects": s.db.list_projects() })),

        "ip_target_add" => {
            let Some(t) = as_str(args, "target") else { return need("target") };
            json_result(&api::add_target_value(
                s,
                &api::TargetIn {
                    project_id: as_i64(args, "project_id").unwrap_or(1),
                    target: t.to_string(),
                    label: as_str(args, "label").unwrap_or("").to_string(),
                },
            ))
        }
        "ip_target_list" => json_result(&json!({
            "ok": true,
            "targets": s.db.list_targets(as_i64(args, "project_id")),
        })),
        "ip_target_remove" => {
            let Some(id) = as_i64(args, "target_id") else { return need("target_id") };
            json_result(&match s.db.delete_target(id) {
                Ok(()) => json!({ "ok": true }),
                Err(e) => api::err(e),
            })
        }

        "ip_profile" => {
            let Some(id) = as_i64(args, "target_id") else { return need("target_id") };
            json_result(&api::profile_value(s, &api::ProfileIn { target_id: id }).await)
        }
        "ip_trace" => {
            let Some(id) = as_i64(args, "target_id") else { return need("target_id") };
            json_result(
                &api::trace_value(
                    s,
                    &api::TraceIn {
                        target_id: id,
                        max_hops: as_i64(args, "max_hops").map(|x| x.clamp(1, 255) as u8),
                    },
                )
                .await,
            )
        }
        "ip_scan_ports" => {
            let Some(id) = as_i64(args, "target_id") else { return need("target_id") };
            json_result(
                &api::scan_value(
                    s,
                    &api::ScanIn {
                        target_id: id,
                        profile: as_str(args, "profile").map(|x| x.to_string()),
                        ports: as_str(args, "ports").map(|x| x.to_string()),
                        concurrency: as_i64(args, "concurrency").map(|x| x.max(1) as usize),
                    },
                )
                .await,
            )
        }

        "ip_runs" => json_result(&json!({
            "ok": true,
            "runs": s.db.list_runs(as_i64(args, "target_id"), as_i64(args, "limit").unwrap_or(50)),
        })),
        "ip_run_get" => {
            let Some(id) = as_i64(args, "run_id") else { return need("run_id") };
            json_result(&match s.db.get_run(id) {
                Some(r) => json!({
                    "ok": true, "run": r,
                    "ports": s.db.ports_of(id),
                    "findings": s.db.findings(Some(id), None, None),
                }),
                None => api::err("không có lần chạy này"),
            })
        }
        "ip_diff" => {
            let Some(a) = as_i64(args, "from_run") else { return need("from_run") };
            let Some(b) = as_i64(args, "to_run") else { return need("to_run") };
            json_result(&s.db.diff(a, b))
        }
        "ip_findings" => json_result(&api::findings_value(
            s,
            &api::FindingsQuery {
                run_id: as_i64(args, "run_id"),
                target_id: as_i64(args, "target_id"),
                severity: as_str(args, "severity").map(|x| x.to_string()),
            },
        )),
        "ip_dashboard" => json_result(&api::dashboard_value(s, as_i64(args, "target_id"))),
        "ip_activity" => json_result(&json!({
            "ok": true,
            "activity": s.db.activity(as_i64(args, "limit").unwrap_or(50)),
        })),

        _ => error_result(format!("không có tool tên '{name}'")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use std::sync::Arc;

    fn state() -> AppState {
        let (mcp_tx, _) = tokio::sync::broadcast::channel(100);
        AppState {
            db: Arc::new(Db::open_memory().unwrap()),
            http: api::http_client(),
            sc: app_space_sdk::SpaceClient::from_env(),
            mcp_tx,
        }
    }

    fn find(name: &str) -> Value {
        tools_list()
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == name)
            .unwrap_or_else(|| panic!("thiếu tool {name}"))
            .clone()
    }

    fn desc(name: &str) -> String {
        find(name)["description"].as_str().unwrap().to_string()
    }

    #[test]
    fn every_listed_tool_uses_the_ip_prefix_and_is_documented() {
        // Quy ước đặt tên MCP của repo: mcp__ipscout-mcp__ip_*
        for t in tools_list().as_array().unwrap() {
            let n = t["name"].as_str().unwrap();
            assert!(n.starts_with("ip_"), "{n} phải có tiền tố ip_");
            assert!(
                t["description"].as_str().unwrap_or("").len() > 40,
                "{n} mô tả quá sơ sài"
            );
            assert_eq!(t["inputSchema"]["type"], "object", "{n} thiếu inputSchema");
        }
    }

    #[test]
    fn required_params_are_declared_in_schema() {
        for (tool, param) in [
            ("ip_target_add", "target"),
            ("ip_profile", "target_id"),
            ("ip_scan_ports", "target_id"),
            ("ip_run_get", "run_id"),
            ("ip_diff", "from_run"),
        ] {
            let req = find(tool)["inputSchema"]["required"].clone();
            assert!(
                req.as_array().unwrap().iter().any(|x| x == param),
                "{tool} phải khai {param} là bắt buộc"
            );
        }
    }

    // Tôi lưu tên hàm này để lịch sử git cho thấy đúng chỗ ownership từng ở:
    // sau khi bỏ verification, chỉ còn quy tắc từ chối máy chủ bên thứ ba và
    // cấm SYN/stealth vẫn phải hiện trong mô tả.
    #[test]
    fn the_active_layer_description_still_states_the_refusal_rule() {
        // Mô tả tool là nơi DUY NHẤT chắc chắn đến được với agent, kể cả khi
        // skill không được nạp — ranh giới đạo đức phải nằm ở đây.
        let d = desc("ip_scan_ports");
        assert!(d.contains("KHÔNG kiểm sở hữu"));
        assert!(d.contains("từ chối"));
        assert!(d.contains("không SYN/stealth"));
        assert!(d.contains("metadata"));

        // Lớp thụ động vẫn phải nói rõ nó không gửi gói tới mục tiêu.
        let p = desc("ip_profile");
        assert!(p.contains("KHÔNG gửi"));
    }

    #[test]
    fn the_profile_tool_warns_the_agent_about_cdn_and_geo_confidence() {
        let d = desc("ip_profile");
        assert!(d.contains("fronted"));
        assert!(d.contains("KHÔNG phải máy chủ gốc"));
        assert!(d.contains("confidence"));
    }

    #[test]
    fn port_profile_names_in_the_schema_match_the_ones_the_scanner_knows() {
        // Lệch hai chỗ này thì agent gọi hồ sơ không tồn tại và nhận lỗi khó hiểu.
        let e = find("ip_scan_ports")["inputSchema"]["properties"]["profile"]["enum"].clone();
        for name in e.as_array().unwrap() {
            let n = name.as_str().unwrap();
            assert!(
                crate::scan::profile_ports(n).is_some(),
                "schema khai hồ sơ '{n}' nhưng scanner không có"
            );
        }
        assert_eq!(e.as_array().unwrap().len(), crate::scan::PROFILES.len());
    }

    #[tokio::test]
    async fn unknown_tool_returns_an_error_result_not_a_panic() {
        let r = call_tool(&state(), "ip_nope", &json!({})).await;
        assert_eq!(r["isError"], true);
    }

    #[tokio::test]
    async fn missing_required_arg_is_reported_by_name() {
        let r = call_tool(&state(), "ip_profile", &json!({})).await;
        assert_eq!(r["isError"], true);
        assert!(r["content"][0]["text"].as_str().unwrap().contains("target_id"));
    }

    #[tokio::test]
    async fn adding_a_target_through_mcp_matches_the_rest_path() {
        let s = state();
        let r = call_tool(&s, "ip_target_add", &json!({ "target": "https://example.com/x" })).await;
        let text = r["content"][0]["text"].as_str().unwrap();
        let v: Value = serde_json::from_str(text).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["host"], "example.com");
        // và mục tiêu mới phải hiện ra ở ip_target_list
        let l = call_tool(&s, "ip_target_list", &json!({})).await;
        let lv: Value = serde_json::from_str(l["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(lv["targets"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn no_verification_tools_remain_in_the_registered_list() {
        // Bảo vệ ngược lại việc gộp/rebase làm sống lại tool cũ.
        let names: std::collections::HashSet<String> = tools_list()
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect();
        for stale in ["ip_target_verify", "ip_target_verify_token"] {
            assert!(!names.contains(stale), "tool cũ {stale} không được quay lại");
        }
    }
}
