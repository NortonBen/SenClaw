//! MCP server (HTTP + SSE) — `predict-mcp`, tools `predict_*`. Thin wrappers
//! over the same value builders REST uses. Read-heavy; the only writes are
//! ledger rows (predictions), which are local and harmless. Lottery/market
//! answers always carry their in-code disclaimers.

use axum::{
    extract::State,
    response::sse::{Event, Sse},
    Json,
};
use futures_util::stream::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::convert::Infallible;

use crate::api::{self, AppState};
use crate::engine;

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
            "serverInfo": { "name": "predict-mcp", "version": "1.0.0" }
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

pub fn tools_list() -> Value {
    json!([
        { "name": "predict_status", "description": "Trạng thái app Siêu Dự Đoán: số CLB có Elo, số trận, số kỳ xổ số, giá vàng mới nhất, thành phố & giải đang theo dõi, tổng quan sổ dự đoán.", "inputSchema": { "type": "object", "properties": {} } },
        { "name": "predict_brief", "description": "Bản tin tổng hợp hôm nay: thời tiết các thành phố, giá vàng/tỷ giá, các trận bóng sắp diễn ra kèm xác suất, kết quả xổ số kỳ gần nhất, điểm số sổ dự đoán. narrate=true để AI viết thành bản tin.", "inputSchema": { "type": "object", "properties": { "narrate": { "type": "boolean" } } } },
        { "name": "predict_football_today", "description": "Dự đoán các trận bóng đá sắp diễn ra trong N ngày tới (mặc định 2) của các giải đang theo dõi: xác suất 1X2, tỷ số khả dĩ nhất, Tài/Xỉu 2.5, BTTS — model Elo+Poisson, tự ghi vào sổ dự đoán.", "inputSchema": { "type": "object", "properties": { "days": { "type": "number" } } } },
        { "name": "predict_football_match", "description": "Dự đoán một trận bất kỳ theo tên hai đội (đội nhà, đội khách) bằng Elo ClubElo + Poisson. article=true để AI viết bài nhận định kiểu 'siêu máy tính' (không bịa số).", "inputSchema": { "type": "object", "properties": { "home": { "type": "string" }, "away": { "type": "string" }, "article": { "type": "boolean" } }, "required": ["home", "away"] } },
        { "name": "predict_football_elo", "description": "Bảng xếp hạng sức mạnh Elo (ClubElo) hiện tại — top CLB châu Âu.", "inputSchema": { "type": "object", "properties": { "limit": { "type": "number" } } } },
        { "name": "predict_lottery_results", "description": "Kết quả XSMB kỳ quay mới nhất: giải Đặc biệt → giải 7 + bảng loto 27 số.", "inputSchema": { "type": "object", "properties": {} } },
        { "name": "predict_lottery_stats", "description": "Thống kê XSMB theo cửa sổ N ngày (mặc định 30): tần suất loto, lô gan lâu chưa về, phân bố đầu–đuôi. Kèm disclaimer: xổ số là ngẫu nhiên.", "inputSchema": { "type": "object", "properties": { "days": { "type": "number" } } } },
        { "name": "predict_lottery_suggest", "description": "'Chốt số' GIẢI TRÍ dựa trên thống kê (tần suất + lô gan), kèm xác suất trúng THẬT (~24%/số) và tự ghi sổ để chứng minh trung thực. Luôn kèm disclaimer. note=true để AI bình luận vui.", "inputSchema": { "type": "object", "properties": { "n": { "type": "number", "description": "Số cặp loto muốn chốt (1-10, mặc định 3)." }, "note": { "type": "boolean" } } } },
        { "name": "predict_weather", "description": "Dự báo thời tiết 7 ngày cho một thành phố VN (Hà Nội, TP.HCM, Đà Nẵng, Hải Phòng, Cần Thơ, Huế, Nha Trang, Đà Lạt, Vinh, Quy Nhơn) — nhiệt độ, xác suất mưa. advice=true để AI khuyên (mang ô, phơi đồ…).", "inputSchema": { "type": "object", "properties": { "city": { "type": "string" }, "advice": { "type": "boolean" } } } },
        { "name": "predict_gold_price", "description": "Giá vàng thế giới XAU/USD hiện tại + quy đổi triệu VND/lượng + tỷ giá USD/VND + biến động 24h. Kèm disclaimer không phải lời khuyên đầu tư.", "inputSchema": { "type": "object", "properties": {} } },
        { "name": "predict_gold_trend", "description": "Xu hướng giá vàng từ dữ liệu tích lũy: SMA 1d/7d, momentum 24h/7d, nhãn xu hướng (tăng/giảm/đi ngang) + chuỗi giá. note=true để AI tóm tắt. Kèm disclaimer.", "inputSchema": { "type": "object", "properties": { "note": { "type": "boolean" } } } },
        { "name": "predict_make", "description": "Ghi MỘT dự đoán bất kỳ vào sổ (vd 'Việt Nam thắng Thái Lan', p=0.7, hạn 14 ngày). Truyền p (0..1, outcome yes/no) hoặc probs (map outcome→xác suất, tổng ≈ 1). Sổ sẽ tự chấm Brier khi resolve.", "inputSchema": { "type": "object", "properties": { "subject": { "type": "string" }, "p": { "type": "number" }, "probs": { "type": "object" }, "due_days": { "type": "number" }, "domain": { "type": "string" } }, "required": ["subject"] } },
        { "name": "predict_list", "description": "Liệt kê các dự đoán trong sổ. Lọc theo domain (football|lottery|weather|market|generic) và status (open|resolved).", "inputSchema": { "type": "object", "properties": { "domain": { "type": "string" }, "status": { "type": "string" }, "limit": { "type": "number" } } } },
        { "name": "predict_resolve", "description": "Resolve tay một dự đoán generic theo id + outcome thực tế (vd 'yes'/'no' hoặc key trong probs). Bóng đá/xổ số/thời tiết tự resolve.", "inputSchema": { "type": "object", "properties": { "id": { "type": "number" }, "outcome": { "type": "string" } }, "required": ["id", "outcome"] } },
        { "name": "predict_score", "description": "Báo cáo độ chính xác của sổ dự đoán: accuracy + Brier trung bình theo domain, và bảng calibration (nhóm tự tin 70% có đúng ~70% không).", "inputSchema": { "type": "object", "properties": {} } },
        { "name": "predict_topic_create", "description": "Tạo CHỦ ĐỀ dự đoán — KHÔNG gò bó: Cách 1 (khuyên dùng) — TỰ DO: truyền 'wish' = mô tả mong muốn bằng lời thường (vd 'theo dõi doanh số shop, dự đoán ngày bán chạy'), AI tự thiết kế tên + trường dữ liệu + câu hỏi mẫu rồi tạo luôn. Cách 2 — TEMPLATE có connector tự nạp dữ liệu: template = gold | weather (+params.city) | lottery | football (+params.league, 4328=NHA) | blank. Cách 3 — tự tay: name + fields (trường ĐỘNG theo thời gian: text|number|date|bool) + tuỳ chọn 'static' (bối cảnh TĨNH: vị trí, thông số cố định) + 'guide' (tài liệu hướng dẫn phân tích, dùng làm prompt cho mọi lần dự đoán của chủ đề). Sau đó nhập/import dữ liệu, AI phân tích, rút quy luật, hỏi dự đoán, dashboard riêng.", "inputSchema": { "type": "object", "properties": { "wish": { "type": "string", "description": "Mô tả tự do — AI thiết kế schema từ đây." }, "template": { "type": "string", "description": "gold|weather|lottery|football|blank." }, "params": { "type": "object", "description": "Tham số template: {city}, {league}, {name}." }, "name": { "type": "string" }, "description": { "type": "string" }, "fields": { "type": "array", "description": "Trường ĐỘNG (dữ liệu nhập theo thời gian) VD [{\"name\":\"ngày\",\"kind\":\"date\"},{\"name\":\"nhiệt độ\",\"kind\":\"number\"}]" }, "static": { "type": "object", "description": "Cấu hình TĨNH — bối cảnh cố định, vd {\"vị trí\":\"Đà Lạt\",\"độ cao\":\"1500m\"}" }, "guide": { "type": "string", "description": "Tài liệu hướng dẫn phân tích/dự đoán chủ đề — dùng làm prompt mỗi lần AI phân tích, rút quy luật, dự đoán." } } } },
        { "name": "predict_topic_list", "description": "Danh sách chủ đề dự đoán tùy chỉnh: id, tên, mô tả, schema trường, số bản ghi, số quy luật.", "inputSchema": { "type": "object", "properties": {} } },
        { "name": "predict_topic_data_add", "description": "Nhập MỘT bản ghi dữ liệu vào chủ đề (object {trường: giá trị} — tự ép kiểu theo schema).", "inputSchema": { "type": "object", "properties": { "topic": { "type": "string", "description": "Tên hoặc id chủ đề." }, "data": { "type": "object" }, "note": { "type": "string" } }, "required": ["topic", "data"] } },
        { "name": "predict_topic_import", "description": "Import NHIỀU bản ghi vào chủ đề: 'csv' (chuỗi CSV, dòng đầu là tên trường) hoặc 'records' (mảng object). Trả về số bản ghi vào + danh sách dòng lỗi.", "inputSchema": { "type": "object", "properties": { "topic": { "type": "string" }, "csv": { "type": "string" }, "records": { "type": "array" } }, "required": ["topic"] } },
        { "name": "predict_topic_doc_add", "description": "Lưu TÀI LIỆU / thông tin NGOÀI SỐ LIỆU vào chủ đề: ghi chú, tin tức, giải thích bối cảnh… 'date' (YYYY-MM-DD) gắn tài liệu với một ngày cụ thể, 'ref' gắn với một giá trị/ngữ cảnh (vd \"giá=124\", \"đợt lạnh\"). Tài liệu được đưa vào mọi lần AI phân tích, rút quy luật và dự đoán của chủ đề.", "inputSchema": { "type": "object", "properties": { "topic": { "type": "string" }, "title": { "type": "string" }, "content": { "type": "string" }, "date": { "type": "string" }, "ref": { "type": "string" } }, "required": ["topic"] } },
        { "name": "predict_topic_docs", "description": "Liệt kê / tìm tài liệu ngoài số liệu của chủ đề (lọc theo từ khoá, ngày hoặc ref).", "inputSchema": { "type": "object", "properties": { "topic": { "type": "string" }, "q": { "type": "string" }, "limit": { "type": "number" } }, "required": ["topic"] } },
        { "name": "predict_topic_search", "description": "Tìm kiếm bản ghi trong chủ đề theo từ khoá (tìm trong dữ liệu + ghi chú), mới nhất trước.", "inputSchema": { "type": "object", "properties": { "topic": { "type": "string" }, "q": { "type": "string" }, "limit": { "type": "number" } }, "required": ["topic"] } },
        { "name": "predict_topic_analyze", "description": "AI ĐÁNH GIÁ dữ liệu chủ đề: bức tranh chung, xu hướng/mẫu hình, chất lượng dữ liệu, nên thu thập thêm gì. Dùng tối đa 60 bản ghi mới nhất, không bịa số.", "inputSchema": { "type": "object", "properties": { "topic": { "type": "string" } }, "required": ["topic"] } },
        { "name": "predict_topic_rules", "description": "QUY LUẬT siêu dự đoán của chủ đề. derive=true → AI rút lại quy luật từ lịch sử (tối đa 6, kèm độ tin cậy; quy luật cũ do AI rút bị thay, quy luật user giữ nguyên). Bỏ derive → chỉ liệt kê.", "inputSchema": { "type": "object", "properties": { "topic": { "type": "string" }, "derive": { "type": "boolean" } }, "required": ["topic"] } },
        { "name": "predict_ask", "description": "HỎI 'điều X có xảy ra không?' — pipeline SIÊU DỰ BÁO đầy đủ theo sách Superforecasting (Tetlock): Fermi phân rã câu hỏi → nền tảng dữ liệu (thống kê chủ đề + quy luật + bài học + track record) + TỔNG HỢP TIN từ Search app (news/web/knowledge) → outside view (base rate) → inside view (bằng chứng thuận/nghịch) → điều chỉnh từng bước → premortem → p mịn + độ tin cậy + điều kiện cập nhật (trace đầy đủ trong kết quả). Có 'topic' → dùng lịch sử chủ đề; không có → dự đoán tự do. LUÔN ghi sổ; khi resolve sẽ tự rút bài học postmortem về chủ đề.", "inputSchema": { "type": "object", "properties": { "question": { "type": "string" }, "topic": { "type": "string", "description": "Tên/id chủ đề (tuỳ chọn)." }, "due_days": { "type": "number", "description": "Sau bao nhiêu ngày biết kết quả (mặc định 30)." } }, "required": ["question"] } },
        { "name": "predict_method", "description": "Nền tảng tri thức đánh giá — mặc định là phương pháp luận sách 'Siêu Dự Báo' (Superforecasting, Tetlock): 11 điều răn, kỹ thuật (outside view/base rate, Fermi, premortem, granularity, Brier, calibration), pipeline 5 bước và CHECKLIST bơm vào mọi lần tổng hợp dự đoán. Không tham số = đọc tri thức đang dùng. Truyền 'update' = {source?, principles?, techniques?, pipeline?, checklist?} để CẬP NHẬT tri thức (phần bỏ trống giữ mặc định); 'reset': true để khôi phục bản gốc từ sách.", "inputSchema": { "type": "object", "properties": { "update": { "type": "object", "description": "Nội dung tri thức mới (từng phần)." }, "reset": { "type": "boolean" } } } }
    ])
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
}

async fn call_tool(s: &AppState, name: &str, args: &Value) -> Value {
    match name {
        "predict_status" => json_result(&api::status_value(s)),
        "predict_brief" => {
            json_result(&api::brief_value(s, args["narrate"].as_bool().unwrap_or(true)).await)
        }
        "predict_football_today" => {
            let days = args["days"].as_i64().unwrap_or(2);
            json_result(&api::football_today_value(s, days, true).await)
        }
        "predict_football_match" => {
            let (Some(home), Some(away)) = (str_arg(args, "home"), str_arg(args, "away")) else {
                return error_result("cần 'home' và 'away'".into());
            };
            json_result(
                &api::predict_match_value(s, home, away, args["article"].as_bool().unwrap_or(true))
                    .await,
            )
        }
        "predict_football_elo" => {
            json_result(&api::elo_top_value(s, args["limit"].as_i64().unwrap_or(30)))
        }
        "predict_lottery_results" => json_result(&api::lottery_latest_value(s).await),
        "predict_lottery_stats" => {
            json_result(&api::lottery_stats_value(s, args["days"].as_i64().unwrap_or(30)).await)
        }
        "predict_lottery_suggest" => {
            let n = args["n"].as_u64().unwrap_or(3) as usize;
            json_result(
                &api::lottery_suggest_value(s, n, args["note"].as_bool().unwrap_or(true)).await,
            )
        }
        "predict_weather" => {
            let city = str_arg(args, "city").unwrap_or("Hà Nội");
            json_result(
                &api::weather_value(s, city, args["advice"].as_bool().unwrap_or(true)).await,
            )
        }
        "predict_gold_price" => json_result(&api::gold_value(s).await),
        "predict_gold_trend" => {
            json_result(&api::gold_trend_value(s, args["note"].as_bool().unwrap_or(true)).await)
        }
        "predict_make" => {
            let Some(subject) = str_arg(args, "subject") else {
                return error_result("thiếu 'subject'".into());
            };
            json_result(&api::ledger_make_value(
                s,
                str_arg(args, "domain").unwrap_or("generic"),
                subject,
                args.get("probs").cloned().filter(|v| v.is_object()),
                args["p"].as_f64(),
                args["due_days"].as_i64(),
                None,
            ))
        }
        "predict_list" => json_result(&api::ledger_list_value(
            s,
            str_arg(args, "domain"),
            str_arg(args, "status"),
            args["limit"].as_i64().unwrap_or(50),
        )),
        "predict_resolve" => {
            let (Some(id), Some(outcome)) = (args["id"].as_i64(), str_arg(args, "outcome")) else {
                return error_result("cần 'id' và 'outcome'".into());
            };
            // Chạy auto-resolve trước để không resolve tay thứ đã tự chấm được.
            let _ = engine::resolve_all(s).await;
            json_result(&api::ledger_resolve_value(s, id, outcome).await)
        }
        "predict_method" => {
            if args.get("update").map(|u| u.is_object()).unwrap_or(false)
                || args["reset"].as_bool().unwrap_or(false)
            {
                let mut body = args.get("update").cloned().unwrap_or_else(|| json!({}));
                if args["reset"].as_bool().unwrap_or(false) {
                    body = json!({ "reset": true });
                }
                return json_result(&api::method_update_value(s, &body));
            }
            json_result(&crate::methodology::methodology_json(&s.db))
        }
        "predict_score" => json_result(&api::ledger_score_value(s)),
        "predict_topic_create" => {
            if let Some(template) = str_arg(args, "template") {
                return json_result(&api::topic_from_template_value(
                    s,
                    template,
                    args.get("params").unwrap_or(&json!({})),
                ));
            }
            // Tự do: AI thiết kế schema từ mô tả mong muốn rồi tạo luôn.
            if let Some(wish) = str_arg(args, "wish") {
                let designed = api::topic_design_value(s, wish).await;
                let Some(p) = designed.get("proposal").filter(|p| p.is_object()) else {
                    return json_result(&designed); // error passthrough
                };
                let mut created = api::topic_create_full(
                    s,
                    p["name"].as_str().unwrap_or("Chủ đề mới"),
                    p["description"].as_str().unwrap_or(""),
                    &p["fields"],
                    &p["static"],
                    p["guide"].as_str().unwrap_or(""),
                );
                created["sample_questions"] = p["sample_questions"].clone();
                return json_result(&created);
            }
            let Some(tname) = str_arg(args, "name") else {
                return error_result(
                    "cần một trong: 'wish' (mô tả tự do — AI thiết kế schema), 'template' (gold|weather|lottery|football|blank), hoặc 'name' + 'fields'".into(),
                );
            };
            json_result(&api::topic_create_full(
                s,
                tname,
                str_arg(args, "description").unwrap_or(""),
                args.get("fields").unwrap_or(&json!([])),
                args.get("static").unwrap_or(&json!({})),
                str_arg(args, "guide").unwrap_or(""),
            ))
        }
        "predict_topic_list" => json_result(&api::topic_list_value(s)),
        "predict_topic_data_add" => {
            let Some(t) = str_arg(args, "topic") else {
                return error_result("thiếu 'topic'".into());
            };
            let Some(data) = args.get("data").filter(|d| d.is_object()) else {
                return error_result("thiếu 'data' (object {trường: giá trị})".into());
            };
            json_result(&api::topic_add_value(
                s,
                t,
                data,
                str_arg(args, "note").unwrap_or(""),
            ))
        }
        "predict_topic_import" => {
            let Some(t) = str_arg(args, "topic") else {
                return error_result("thiếu 'topic'".into());
            };
            json_result(&api::topic_import_value(
                s,
                t,
                str_arg(args, "csv"),
                args.get("records"),
            ))
        }
        "predict_topic_doc_add" => {
            let Some(t) = str_arg(args, "topic") else {
                return error_result("thiếu 'topic'".into());
            };
            json_result(&api::topic_doc_add_value(
                s,
                t,
                str_arg(args, "title").unwrap_or(""),
                str_arg(args, "content").unwrap_or(""),
                str_arg(args, "date").unwrap_or(""),
                str_arg(args, "ref").unwrap_or(""),
            ))
        }
        "predict_topic_docs" => {
            let Some(t) = str_arg(args, "topic") else {
                return error_result("thiếu 'topic'".into());
            };
            json_result(&api::topic_docs_value(
                s,
                t,
                str_arg(args, "q").unwrap_or(""),
                args["limit"].as_i64().unwrap_or(50),
            ))
        }
        "predict_topic_search" => {
            let Some(t) = str_arg(args, "topic") else {
                return error_result("thiếu 'topic'".into());
            };
            json_result(&api::topic_search_value(
                s,
                t,
                str_arg(args, "q").unwrap_or(""),
                args["limit"].as_i64().unwrap_or(50),
            ))
        }
        "predict_topic_analyze" => {
            let Some(t) = str_arg(args, "topic") else {
                return error_result("thiếu 'topic'".into());
            };
            json_result(&api::topic_analyze_value(s, t).await)
        }
        "predict_topic_rules" => {
            let Some(t) = str_arg(args, "topic") else {
                return error_result("thiếu 'topic'".into());
            };
            json_result(
                &api::topic_rules_value(s, t, args["derive"].as_bool().unwrap_or(false)).await,
            )
        }
        "predict_ask" => {
            let Some(q) = str_arg(args, "question") else {
                return error_result("thiếu 'question'".into());
            };
            json_result(
                &api::topic_ask_value(
                    s,
                    str_arg(args, "topic"),
                    q,
                    args["due_days"].as_i64().unwrap_or(30),
                )
                .await,
            )
        }
        other => error_result(format!("tool không tồn tại: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_list_names_and_schemas() {
        let tools = tools_list();
        let names: Vec<&str> = tools
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t["name"].as_str())
            .collect();
        assert_eq!(names.len(), 26);
        for expected in [
            "predict_status",
            "predict_brief",
            "predict_football_today",
            "predict_football_match",
            "predict_football_elo",
            "predict_lottery_results",
            "predict_lottery_stats",
            "predict_lottery_suggest",
            "predict_weather",
            "predict_gold_price",
            "predict_gold_trend",
            "predict_make",
            "predict_list",
            "predict_resolve",
            "predict_score",
            "predict_topic_create",
            "predict_topic_list",
            "predict_topic_data_add",
            "predict_topic_import",
            "predict_topic_search",
            "predict_topic_analyze",
            "predict_topic_rules",
            "predict_ask",
            "predict_method",
            "predict_topic_doc_add",
            "predict_topic_docs",
        ] {
            assert!(names.contains(&expected), "missing tool {expected}");
        }
        for t in tools.as_array().unwrap() {
            assert_eq!(
                t["inputSchema"]["type"], "object",
                "tool {} bad schema",
                t["name"]
            );
            assert!(t["name"].as_str().unwrap().starts_with("predict_"));
        }
    }

    #[tokio::test]
    async fn call_tool_local_paths() {
        let s = crate::api::test_state();
        // Unknown tool errors.
        let v = call_tool(&s, "predict_nope", &json!({})).await;
        assert_eq!(v["isError"], true);
        // Status works offline.
        let st = call_tool(&s, "predict_status", &json!({})).await;
        assert!(st["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("\"ok\": true"));
        // Ledger make/list/score/resolve round-trip offline.
        let mk = call_tool(
            &s,
            "predict_make",
            &json!({ "subject": "test", "p": 0.9, "due_days": 1 }),
        )
        .await;
        assert!(mk["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("\"ok\": true"));
        let ls = call_tool(&s, "predict_list", &json!({ "status": "open" })).await;
        assert!(ls["content"][0]["text"].as_str().unwrap().contains("test"));
        let sc = call_tool(&s, "predict_score", &json!({})).await;
        assert!(sc["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("calibration"));
        // predict_make validation error surfaces.
        let bad = call_tool(&s, "predict_make", &json!({ "subject": "x" })).await;
        assert!(bad["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("error"));
    }

    #[tokio::test]
    async fn topic_tools_roundtrip_offline() {
        let s = crate::api::test_state();
        let text = |v: &Value| v["content"][0]["text"].as_str().unwrap().to_string();

        let created = call_tool(
            &s,
            "predict_topic_create",
            &json!({
                "name": "Kèo test", "description": "d",
                "fields": [{ "name": "ngày", "kind": "date" }, { "name": "kq", "kind": "bool" }]
            }),
        )
        .await;
        assert!(text(&created).contains("\"ok\": true"));

        let added = call_tool(
            &s,
            "predict_topic_data_add",
            &json!({
                "topic": "Kèo test", "data": { "ngày": "2026-07-27", "kq": "có" }
            }),
        )
        .await;
        assert!(text(&added).contains("\"ok\": true"));

        let imported = call_tool(
            &s,
            "predict_topic_import",
            &json!({
                "topic": "Kèo test", "csv": "ngày,kq\n2026-07-26,1\n"
            }),
        )
        .await;
        assert!(text(&imported).contains("\"imported\": 1"));

        let listed = call_tool(&s, "predict_topic_list", &json!({})).await;
        assert!(text(&listed).contains("Kèo test"));
        assert!(text(&listed).contains("\"records\": 2"));

        let found = call_tool(
            &s,
            "predict_topic_search",
            &json!({ "topic": "kèo test", "q": "2026-07-26" }),
        )
        .await;
        assert!(text(&found).contains("2026-07-26"));

        // Missing-arg guards.
        for (tool, args) in [
            ("predict_topic_data_add", json!({ "topic": "Kèo test" })),
            ("predict_topic_search", json!({})),
            ("predict_ask", json!({})),
        ] {
            let v = call_tool(&s, tool, &args).await;
            assert_eq!(v["isError"], true, "{tool} should error");
        }
    }
}
