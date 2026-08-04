use crate::api::AppState;
use axum::extract::State;
use axum::response::sse::{Event, Sse};
use axum::Json;
use futures_util::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::convert::Infallible;
use std::sync::Arc;

#[derive(Deserialize, Debug)]
pub struct JsonRpcRequest {
    #[allow(dead_code)]
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    pub params: Option<Value>,
}

/// GET /api/mcp/sse — opens the SSE channel; first event tells the client
/// where to POST JSON-RPC messages.
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

/// POST /api/mcp/sse or /api/mcp/message — JSON-RPC 2.0 dispatch.
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
            "serverInfo": { "name": "ai-office-mcp", "version": "1.0.0" }
        })),
        "ping" => reply(json!({})),
        "notifications/initialized" => {
            Json(json!({ "jsonrpc": "2.0", "id": req.id, "result": {} }))
        }
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

fn text_result(text: String) -> Value {
    json!({ "content": [{ "type": "text", "text": text }] })
}

fn json_result(v: Value) -> Value {
    text_result(serde_json::to_string_pretty(&v).unwrap_or_default())
}

fn error_result(text: String) -> Value {
    json!({ "isError": true, "content": [{ "type": "text", "text": text }] })
}

fn tools_list() -> Value {
    json!([
        {
            "name": "office_list_teams",
            "description": "Danh sách các đội nhóm trong AI Office (mỗi đội có roster riêng, chạy song song). Trả về key + tên + mô tả. Dùng key này khi giao việc/tuyển nhân sự cho đúng đội.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "office_add_team",
            "description": "Tạo một đội nhóm mới (tự có sẵn 1 Trưởng nhóm). Ví dụ: 'Nghiên cứu thị trường', 'Phát triển ứng dụng', 'Dữ liệu & thống kê'.",
            "inputSchema": { "type": "object", "properties": {
                "name": { "type": "string", "description": "Tên đội" },
                "description": { "type": "string", "description": "Mô tả nhiệm vụ của đội" }
            }, "required": ["name"] }
        },
        {
            "name": "office_status",
            "description": "Tình hình AI Office ngay lúc này: danh sách đội, trạng thái từng agent (đang làm / xong / đi bàn giao) và nhiệm vụ gần nhất. Gọi trước khi giao việc mới.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "office_create_task",
            "description": "Giao một nhiệm vụ mới cho MỘT ĐỘI. Nếu đội đang bận, nhiệm vụ XẾP VÀO HÀNG ĐỢI của đội và tự chạy khi xong việc trước (các đội chạy song song). Trưởng nhóm lập kế hoạch phân công (nhân sự 'tự nhận nhiệm vụ' luôn có phần việc, tăng cường chỉ khi cần); nhân sự nắm skill/sub-agent dùng công cụ thật (MCP/search/browser). Kiểm định soát chất lượng rồi Trưởng nhóm nộp báo cáo tổng hợp — báo cáo lên BẢNG VIỆC chờ Sếp duyệt (office_approve_task / office_return_task).",
            "inputSchema": { "type": "object", "properties": {
                "title": { "type": "string", "description": "Nội dung nhiệm vụ, ví dụ: 'nghiên cứu đối thủ ngành gia dụng'" },
                "team": { "type": "string", "description": "Key đội xử lý (từ office_list_teams). Bỏ trống = đội đầu tiên." },
                "goal_id": { "type": "number", "description": "Id mục tiêu quý mà việc này phục vụ (từ office_list_goals). Bỏ trống = việc 'lạc hướng' trên bảng." },
                "start": { "type": "boolean", "description": "false = chỉ đặt vào HỘP VIỆC trên bảng, chưa chạy (Sếp/agent chạy sau bằng office_start_task). Mặc định true = chạy ngay." }
            }, "required": ["title"] }
        },
        {
            "name": "office_list_tasks",
            "description": "Liệt kê các nhiệm vụ gần đây của văn phòng kèm trạng thái (pending/planning/running/review/done/error) và chế độ chạy.",
            "inputSchema": { "type": "object", "properties": {
                "limit": { "type": "number", "description": "Số nhiệm vụ tối đa, mặc định 20" }
            } }
        },
        {
            "name": "office_get_task",
            "description": "Chi tiết một nhiệm vụ: các phần việc đã phân công cho từng agent, kết quả từng phần và nhật ký bàn giao đầy đủ.",
            "inputSchema": { "type": "object", "properties": {
                "id": { "type": "number", "description": "Id nhiệm vụ" }
            }, "required": ["id"] }
        },
        {
            "name": "office_get_report",
            "description": "Lấy BÁO CÁO TỔNG HỢP mà Trưởng phòng đã nộp. Bỏ trống id để lấy báo cáo của nhiệm vụ hoàn thành gần nhất.",
            "inputSchema": { "type": "object", "properties": {
                "id": { "type": "number", "description": "Id nhiệm vụ (tùy chọn)" }
            } }
        },
        {
            "name": "office_list_agents",
            "description": "Danh sách nhân sự ảo: tên, vai trò, đội (team), trạng thái. Truyền 'team' để lọc theo một đội.",
            "inputSchema": { "type": "object", "properties": {
                "team": { "type": "string", "description": "Key đội để lọc (tùy chọn)" }
            } }
        },
        {
            "name": "office_update_agent",
            "description": "Cập nhật hồ sơ một nhân sự ảo: tên / vai trò / nhiệm vụ cố định, bật-tắt hoạt động (enabled), chế độ tự nhận nhiệm vụ (auto_assign), và danh sách skill/sub-agent nắm giữ (skills — lấy tên từ office inventory hoặc /api/skills của daemon, sub-agent dùng tiền tố 'persona:').",
            "inputSchema": { "type": "object", "properties": {
                "key": { "type": "string", "description": "Khóa agent, ví dụ 'noi-dung'" },
                "name": { "type": "string", "description": "Tên hiển thị mới" },
                "role": { "type": "string", "description": "Vai trò ngắn gọn mới" },
                "duty": { "type": "string", "description": "Mô tả nhiệm vụ cố định mới" },
                "enabled": { "type": "boolean", "description": "false = tạm nghỉ (không tham gia nhiệm vụ)" },
                "auto_assign": { "type": "boolean", "description": "true = luôn được phân công; false = chỉ khi cần chuyên môn" },
                "skills": { "type": "array", "items": { "type": "string" }, "description": "Skill/sub-agent nắm giữ, ví dụ ['browser', 'persona:researcher']" }
            }, "required": ["key"] }
        },
        {
            "name": "office_add_agent",
            "description": "Tuyển thêm một nhân sự ảo vào MỘT ĐỘI (tối đa 7 bàn/đội). kind: 'worker' (mặc định), 'manager'/'qa' chỉ khi đội chưa có. Nhân sự mới có knowledge space riêng ai-office:<key>.",
            "inputSchema": { "type": "object", "properties": {
                "name": { "type": "string", "description": "Tên hiển thị, ví dụ 'THIẾT KẾ'" },
                "role": { "type": "string", "description": "Vai trò ngắn gọn" },
                "duty": { "type": "string", "description": "Mô tả nhiệm vụ cố định" },
                "kind": { "type": "string", "enum": ["worker", "manager", "qa"], "description": "Loại nhân sự, mặc định worker" },
                "team": { "type": "string", "description": "Key đội (từ office_list_teams). Bỏ trống = đội đầu tiên." }
            }, "required": ["name"] }
        },
        {
            "name": "office_remove_agent",
            "description": "Cho một nhân sự ảo nghỉ việc (không xoá được Trưởng phòng; phải giữ ít nhất 1 worker; không đổi biên chế khi phòng đang chạy nhiệm vụ).",
            "inputSchema": { "type": "object", "properties": {
                "key": { "type": "string", "description": "Khóa agent, ví dụ 'thiet-ke'" }
            }, "required": ["key"] }
        },
        {
            "name": "office_stats",
            "description": "Sổ kế toán của văn phòng: tổng số nhiệm vụ, số đã hoàn thành, số lần gọi LLM, số token đã dùng (ước tính, vào/ra) và model gần nhất.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "office_board",
            "description": "BẢNG VIỆC kanban của văn phòng, chia 4 cột kiểu trụ sở điều hành: inbox (HỘP VIỆC — chưa chạy + việc lỗi), doing (ĐANG LÀM), waiting (CHỜ SẾP DUYỆT — AI xong, chờ nghiệm thu), done (HOÀN TẤT — Sếp đã duyệt). Mỗi việc kèm goal_id (null = việc lạc hướng, chưa gắn mục tiêu).",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "office_dashboard",
            "description": "DASHBOARD ĐIỀU HÀNH — bàn làm việc của Sếp: độ bám hướng (% việc mở có gắn mục tiêu), tiến độ trung bình mục tiêu quý, số việc chờ Sếp duyệt, nhịp điều hành (số ngày họp sáng liên tiếp) và token AI đã dùng trong tháng.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "office_approve_task",
            "description": "Sếp DUYỆT (nghiệm thu) một việc đang ở cột CHỜ SẾP DUYỆT → chuyển sang HOÀN TẤT. Chỉ dùng khi Sếp đã xem báo cáo (office_get_report) và đồng ý.",
            "inputSchema": { "type": "object", "properties": {
                "id": { "type": "number", "description": "Id việc đang chờ duyệt" }
            }, "required": ["id"] }
        },
        {
            "name": "office_return_task",
            "description": "Sếp TRẢ LẠI một việc đang chờ duyệt kèm ghi chú lý do — việc quay lại hàng đợi và văn phòng tự làm lại với ghi chú của Sếp trong context.",
            "inputSchema": { "type": "object", "properties": {
                "id": { "type": "number", "description": "Id việc đang chờ duyệt" },
                "note": { "type": "string", "description": "Ghi chú của Sếp: sai chỗ nào, cần sửa gì" }
            }, "required": ["id", "note"] }
        },
        {
            "name": "office_start_task",
            "description": "Chạy một việc đang nằm trong HỘP VIỆC (tạo với start=false) hoặc chạy lại một việc bị lỗi — việc vào hàng đợi của đội.",
            "inputSchema": { "type": "object", "properties": {
                "id": { "type": "number", "description": "Id việc trong Hộp việc / việc lỗi" }
            }, "required": ["id"] }
        },
        {
            "name": "office_set_task_goal",
            "description": "Gắn một việc vào mục tiêu quý (hoặc gỡ bằng goal_id=0) — việc không gắn mục tiêu bị coi là 'lạc hướng' và kéo tụt ĐỘ BÁM HƯỚNG trên dashboard.",
            "inputSchema": { "type": "object", "properties": {
                "id": { "type": "number", "description": "Id việc" },
                "goal_id": { "type": "number", "description": "Id mục tiêu (từ office_list_goals); 0 = gỡ mục tiêu" }
            }, "required": ["id", "goal_id"] }
        },
        {
            "name": "office_list_goals",
            "description": "Danh sách MỤC TIÊU QUÝ của công ty: tiêu đề, quý, các kết quả then chốt (key results ✓/☐), tiến độ %, số việc đang phục vụ mục tiêu.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "office_add_goal",
            "description": "Đặt một mục tiêu quý mới cho công ty (kiểu OKR). key_results là danh sách kết quả then chốt đo được.",
            "inputSchema": { "type": "object", "properties": {
                "title": { "type": "string", "description": "Mục tiêu, ví dụ: 'Đạt 30 triệu doanh thu/tháng từ khoá Canva'" },
                "quarter": { "type": "string", "description": "Quý, ví dụ 'Q3/2026'. Bỏ trống được." },
                "key_results": { "type": "array", "items": { "type": "string" }, "description": "Các kết quả then chốt, ví dụ ['50 học viên mới', 'chuỗi email 5 thư chạy tự động']" }
            }, "required": ["title"] }
        },
        {
            "name": "office_update_goal",
            "description": "Cập nhật mục tiêu quý: sửa tiêu đề/quý, tick ✓ một key result (done_index), thay cả danh sách key_results, hoặc lưu trữ (archived=true) khi mục tiêu kết thúc.",
            "inputSchema": { "type": "object", "properties": {
                "id": { "type": "number", "description": "Id mục tiêu" },
                "title": { "type": "string", "description": "Tiêu đề mới" },
                "quarter": { "type": "string", "description": "Quý mới" },
                "key_results": { "type": "array", "items": { "type": "string" }, "description": "Thay TOÀN BỘ danh sách key results (reset trạng thái tick)" },
                "done_index": { "type": "number", "description": "Đánh dấu HOÀN THÀNH key result thứ i (0-based)" },
                "undone_index": { "type": "number", "description": "Bỏ đánh dấu key result thứ i (0-based)" },
                "archived": { "type": "boolean", "description": "true = lưu trữ mục tiêu (ẩn khỏi dashboard)" }
            }, "required": ["id"] }
        },
        {
            "name": "office_run_meeting",
            "description": "Chạy một phiên HỌP ĐIỀU HÀNH với Giám đốc vận hành (AI) và trả về biên bản: kind='morning' (họp sáng: tình hình + 3 ưu tiên hôm nay + cảnh báo) hoặc kind='evening' (họp tối: đã làm + còn tồn + chuẩn bị ngày mai). Mỗi ngày mỗi loại một biên bản, họp lại = ghi đè. Đây là một lượt gọi LLM thật (~10-30s).",
            "inputSchema": { "type": "object", "properties": {
                "kind": { "type": "string", "enum": ["morning", "evening"], "description": "Loại phiên họp" }
            }, "required": ["kind"] }
        }
    ])
}

async fn call_tool(state: &Arc<AppState>, name: &str, args: &Value) -> Value {
    let db = &state.db;
    let out = match name {
        "office_list_teams" => db.list_teams().map(|teams| json!({ "teams": teams })),
        "office_add_team" => {
            let name = args["name"].as_str().unwrap_or("").trim().to_string();
            if name.is_empty() {
                return error_result("thiếu 'name' của đội".into());
            }
            db.add_team(&name, args["description"].as_str().unwrap_or(""))
                .map(|team| json!({ "team": team }))
        }
        "office_status" => db.list_agents().and_then(|agents| {
            let teams = db.list_teams()?;
            let task = db.latest_task()?;
            Ok(json!({ "teams": teams, "agents": agents, "latestTask": task }))
        }),
        "office_create_task" => {
            let title = args["title"].as_str().unwrap_or("").trim().to_string();
            if title.is_empty() {
                return error_result("thiếu 'title' — nội dung nhiệm vụ".into());
            }
            let teams = match db.list_teams() {
                Ok(t) => t,
                Err(e) => return error_result(e.to_string()),
            };
            let team = args["team"]
                .as_str()
                .filter(|t| teams.iter().any(|x| x.key == *t))
                .map(|s| s.to_string())
                .or_else(|| teams.first().map(|t| t.key.clone()));
            let Some(team) = team else {
                return error_result("chưa có đội nào — tạo bằng office_add_team".into());
            };
            match db.list_agents_in(&team) {
                Ok(agents) if !agents.iter().any(|a| a.kind == "worker" && a.enabled) => {
                    return error_result(format!(
                        "đội '{}' không còn nhân sự chuyên môn đang hoạt động",
                        team
                    ))
                }
                Err(e) => return error_result(e.to_string()),
                _ => {}
            }
            let goal_id = args["goal_id"].as_i64().filter(|g| *g > 0);
            if let Some(gid) = goal_id {
                match db.get_goal(gid) {
                    Ok(Some(_)) => {}
                    Ok(None) => {
                        return error_result(format!(
                            "không có mục tiêu id={} — xem office_list_goals",
                            gid
                        ))
                    }
                    Err(e) => return error_result(e.to_string()),
                }
            }
            let start = args["start"].as_bool().unwrap_or(true);
            let busy = start && db.has_running_task(&team).unwrap_or(false);
            db.create_task(&title, "live", &team, goal_id, start).map(|task| {
                let hint = if !start {
                    "việc đã đặt vào HỘP VIỆC trên Bảng việc — chạy bằng office_start_task khi sẵn sàng"
                } else if busy {
                    "đội đang bận — nhiệm vụ đã xếp vào hàng đợi, sẽ tự chạy khi xong việc hiện tại"
                } else {
                    "theo dõi bằng office_get_task, lấy kết quả bằng office_get_report; xong sẽ chờ Sếp duyệt (office_approve_task)"
                };
                json!({ "task": task, "team": team, "queued": busy, "hint": hint })
            })
        }
        "office_list_tasks" => {
            let limit = args["limit"].as_i64().unwrap_or(20).clamp(1, 200);
            db.list_tasks(limit).map(|tasks| json!({ "tasks": tasks }))
        }
        "office_get_task" => {
            let Some(id) = args["id"].as_i64() else {
                return error_result("thiếu 'id' nhiệm vụ".into());
            };
            match db.get_task(id) {
                Ok(Some(task)) => db.list_steps(id).and_then(|steps| {
                    let events = db.list_events(Some(id), 0, 500)?;
                    Ok(json!({ "task": task, "steps": steps, "events": events }))
                }),
                Ok(None) => return error_result(format!("không có nhiệm vụ id={}", id)),
                Err(e) => Err(e),
            }
        }
        "office_get_report" => {
            let task = match args["id"].as_i64() {
                Some(id) => db.get_task(id),
                None => db.latest_task(),
            };
            match task {
                Ok(Some(t)) if !t.report.is_empty() => Ok(json!({
                    "taskId": t.id, "title": t.title, "status": t.status, "report": t.report
                })),
                Ok(Some(t)) => {
                    return error_result(format!(
                        "nhiệm vụ {} ({}) chưa có báo cáo — trạng thái hiện tại: {}",
                        t.id, t.title, t.status
                    ))
                }
                Ok(None) => return error_result("chưa có nhiệm vụ nào".into()),
                Err(e) => Err(e),
            }
        }
        "office_list_agents" => {
            let team = args["team"].as_str();
            db.list_agents().map(|agents| {
                let agents: Vec<_> = match team {
                    Some(t) => agents.into_iter().filter(|a| a.team == t).collect(),
                    None => agents,
                };
                json!({ "agents": agents })
            })
        }
        "office_update_agent" => {
            let key = args["key"].as_str().unwrap_or("");
            if key.is_empty() {
                return error_result("thiếu 'key' của agent".into());
            }
            let skills: Option<Vec<String>> = args["skills"].as_array().map(|arr| {
                arr.iter()
                    .filter_map(|s| s.as_str().map(|s| s.trim().to_string()))
                    .filter(|s| !s.is_empty())
                    .collect()
            });
            if args["enabled"].as_bool() == Some(false) {
                match db.list_agents() {
                    Ok(agents) => {
                        if let Some(a) = agents.iter().find(|a| a.key == key) {
                            if db.has_running_task(&a.team).unwrap_or(false) {
                                return error_result(
                                    "đội đang xử lý nhiệm vụ — chờ xong rồi tạm dừng nhân sự"
                                        .into(),
                                );
                            }
                            if a.kind == "manager" {
                                return error_result("không thể tạm dừng Trưởng nhóm".into());
                            }
                            if a.kind == "worker"
                                && agents
                                    .iter()
                                    .filter(|x| x.team == a.team && x.kind == "worker" && x.enabled)
                                    .count()
                                    <= 1
                            {
                                return error_result(
                                    "đội cần ít nhất một nhân sự chuyên môn đang hoạt động".into(),
                                );
                            }
                        }
                    }
                    Err(e) => return error_result(e.to_string()),
                }
            }
            db.update_agent(
                key,
                args["name"].as_str(),
                args["role"].as_str(),
                args["duty"].as_str(),
                args["enabled"].as_bool(),
                args["auto_assign"].as_bool(),
                skills.as_deref(),
            )
            .and_then(|found| {
                if found {
                    Ok(json!({ "ok": true }))
                } else {
                    anyhow::bail!("không có agent '{}'", key)
                }
            })
        }
        "office_add_agent" => {
            let name = args["name"].as_str().unwrap_or("").trim().to_string();
            if name.is_empty() {
                return error_result("thiếu 'name' của nhân sự".into());
            }
            let kind = match args["kind"].as_str() {
                Some("manager") => "manager",
                Some("qa") => "qa",
                _ => "worker",
            };
            let teams = match db.list_teams() {
                Ok(t) => t,
                Err(e) => return error_result(e.to_string()),
            };
            let team = args["team"]
                .as_str()
                .filter(|t| teams.iter().any(|x| x.key == *t))
                .map(|s| s.to_string())
                .or_else(|| teams.first().map(|t| t.key.clone()));
            let Some(team) = team else {
                return error_result("chưa có đội nào — tạo bằng office_add_team".into());
            };
            match db.list_agents() {
                Ok(agents)
                    if kind != "worker"
                        && agents.iter().any(|a| a.team == team && a.kind == kind) =>
                {
                    return error_result(format!("đội đã có một nhân sự giữ vai trò '{}'", kind))
                }
                Err(e) => return error_result(e.to_string()),
                _ => {}
            }
            db.add_agent(
                &name,
                args["role"].as_str().unwrap_or(""),
                args["duty"].as_str().unwrap_or(""),
                kind,
                &team,
            )
            .map(|agent| json!({ "agent": agent }))
        }
        "office_remove_agent" => {
            let key = args["key"].as_str().unwrap_or("");
            if key.is_empty() {
                return error_result("thiếu 'key' của nhân sự".into());
            }
            match db.list_agents() {
                Ok(agents) => {
                    let Some(agent) = agents.iter().find(|a| a.key == key) else {
                        return error_result(format!("không có agent '{}'", key));
                    };
                    if db.has_running_task(&agent.team).unwrap_or(false) {
                        return error_result(
                            "đội đang chạy nhiệm vụ — chờ xong rồi thay đổi biên chế".into(),
                        );
                    }
                    if agent.kind == "manager" {
                        return error_result("không thể xoá Trưởng nhóm".into());
                    }
                    if agent.kind == "worker"
                        && agents
                            .iter()
                            .filter(|a| a.team == agent.team && a.kind == "worker")
                            .count()
                            <= 1
                    {
                        return error_result(
                            "đội cần ít nhất một nhân sự chuyên môn (worker)".into(),
                        );
                    }
                }
                Err(e) => return error_result(e.to_string()),
            }
            db.delete_agent(key).map(|_| json!({ "ok": true }))
        }
        "office_stats" => db.stats(),
        "office_board" => {
            // Dùng chung mapper với REST /api/board (một nguồn sự thật cột).
            crate::api::board_json(db.as_ref())
        }
        "office_dashboard" => Ok(crate::meeting::dashboard_json(db.as_ref())),
        "office_approve_task" => {
            let Some(id) = args["id"].as_i64() else {
                return error_result("thiếu 'id' việc".into());
            };
            match db.approve_task(id) {
                Ok(true) => {
                    let title = db
                        .get_task(id)
                        .ok()
                        .flatten()
                        .map(|t| t.title)
                        .unwrap_or_default();
                    let _ = db.add_event(
                        Some(id),
                        "boss",
                        "sep",
                        "",
                        &format!("Sếp DUYỆT việc \"{}\" — nghiệm thu kết quả.", title),
                    );
                    Ok(json!({ "ok": true, "id": id }))
                }
                Ok(false) => return error_result(
                    "việc này không ở trạng thái chờ duyệt (xem office_board cột waiting)".into(),
                ),
                Err(e) => Err(e),
            }
        }
        "office_return_task" => {
            let Some(id) = args["id"].as_i64() else {
                return error_result("thiếu 'id' việc".into());
            };
            let note = args["note"].as_str().unwrap_or("").trim().to_string();
            if note.is_empty() {
                return error_result("thiếu 'note' — Sếp phải nói rõ cần sửa gì".into());
            }
            match db.return_task(id, &note) {
                Ok(true) => {
                    let title = db
                        .get_task(id)
                        .ok()
                        .flatten()
                        .map(|t| t.title)
                        .unwrap_or_default();
                    let _ = db.add_event(
                        Some(id),
                        "boss",
                        "sep",
                        "",
                        &format!("Sếp TRẢ LẠI việc \"{}\" — ghi chú: {}", title, note),
                    );
                    Ok(json!({ "ok": true, "id": id, "requeued": true }))
                }
                Ok(false) => return error_result(
                    "việc này không ở trạng thái chờ duyệt (xem office_board cột waiting)".into(),
                ),
                Err(e) => Err(e),
            }
        }
        "office_start_task" => {
            let Some(id) = args["id"].as_i64() else {
                return error_result("thiếu 'id' việc".into());
            };
            match db.start_task(id) {
                Ok(true) => Ok(json!({ "ok": true, "id": id })),
                Ok(false) => return error_result(
                    "chỉ chạy được việc trong Hộp việc hoặc việc bị lỗi".into(),
                ),
                Err(e) => Err(e),
            }
        }
        "office_set_task_goal" => {
            let Some(id) = args["id"].as_i64() else {
                return error_result("thiếu 'id' việc".into());
            };
            let Some(gid) = args["goal_id"].as_i64() else {
                return error_result("thiếu 'goal_id' (0 = gỡ mục tiêu)".into());
            };
            let goal_id = if gid == 0 { None } else { Some(gid) };
            if let Some(g) = goal_id {
                match db.get_goal(g) {
                    Ok(Some(_)) => {}
                    Ok(None) => return error_result(format!("không có mục tiêu id={}", g)),
                    Err(e) => return error_result(e.to_string()),
                }
            }
            match db.set_task_goal(id, goal_id) {
                Ok(true) => Ok(json!({ "ok": true })),
                Ok(false) => return error_result(format!("không có việc id={}", id)),
                Err(e) => Err(e),
            }
        }
        "office_list_goals" => db.list_goals(true).and_then(|goals| {
            let counts = db.goal_task_counts()?;
            let goals: Vec<Value> = goals
                .into_iter()
                .map(|g| {
                    let (total, open) = counts.get(&g.id).copied().unwrap_or((0, 0));
                    let mut v = serde_json::to_value(&g).unwrap_or_default();
                    v["taskCount"] = json!(total);
                    v["openTaskCount"] = json!(open);
                    v
                })
                .collect();
            Ok(json!({ "goals": goals }))
        }),
        "office_add_goal" => {
            let title = args["title"].as_str().unwrap_or("").trim().to_string();
            if title.is_empty() {
                return error_result("thiếu 'title' mục tiêu".into());
            }
            let krs: Vec<crate::db::KeyResult> = args["key_results"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|s| s.as_str())
                        .filter(|s| !s.trim().is_empty())
                        .map(|s| crate::db::KeyResult {
                            text: s.trim().to_string(),
                            done: false,
                        })
                        .collect()
                })
                .unwrap_or_default();
            db.add_goal(&title, args["quarter"].as_str().unwrap_or(""), &krs)
                .map(|goal| json!({ "goal": goal }))
        }
        "office_update_goal" => {
            let Some(id) = args["id"].as_i64() else {
                return error_result("thiếu 'id' mục tiêu".into());
            };
            let Some(goal) = (match db.get_goal(id) {
                Ok(g) => g,
                Err(e) => return error_result(e.to_string()),
            }) else {
                return error_result(format!("không có mục tiêu id={}", id));
            };
            // key_results mới thay cả danh sách; done/undone_index tick lên
            // danh sách hiện có.
            let mut krs = goal.key_results.clone();
            let mut krs_changed = false;
            if let Some(arr) = args["key_results"].as_array() {
                krs = arr
                    .iter()
                    .filter_map(|s| s.as_str())
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| crate::db::KeyResult {
                        text: s.trim().to_string(),
                        done: false,
                    })
                    .collect();
                krs_changed = true;
            }
            for (arg, want) in [("done_index", true), ("undone_index", false)] {
                if let Some(i) = args[arg].as_i64() {
                    match krs.get_mut(i as usize) {
                        Some(kr) => {
                            kr.done = want;
                            krs_changed = true;
                        }
                        None => {
                            return error_result(format!(
                                "{}={} vượt quá số key results ({})",
                                arg,
                                i,
                                krs.len()
                            ))
                        }
                    }
                }
            }
            db.update_goal(
                id,
                args["title"].as_str(),
                args["quarter"].as_str(),
                krs_changed.then_some(krs.as_slice()),
                args["archived"].as_bool(),
            )
            .and_then(|_| Ok(json!({ "goal": db.get_goal(id)? })))
        }
        "office_run_meeting" => {
            let kind = match args["kind"].as_str() {
                Some("morning") => "morning",
                Some("evening") => "evening",
                _ => return error_result("'kind' phải là morning hoặc evening".into()),
            };
            match crate::meeting::run_meeting(db, kind).await {
                Ok(m) => Ok(json!({ "meeting": m })),
                Err(e) => return error_result(e),
            }
        }
        other => return error_result(format!("không có tool '{}'", other)),
    };
    match out {
        Ok(v) => json_result(v),
        Err(e) => error_result(e.to_string()),
    }
}
