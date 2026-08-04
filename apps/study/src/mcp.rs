//! MCP server — hand-rolled JSON-RPC over HTTP + SSE, matching the other Space
//! Apps (the `rmcp` crate is not used here).
//!
//! Tools are prefixed `study_`. Agents reach them as `mcp__study-mcp__study_*`.

use std::convert::Infallible;

use axum::extract::State;
use axum::response::sse::{Event, Sse};
use axum::Json;
use futures_util::Stream;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::AppState;
use crate::{ask, calendar, cards, db, ingest, lesson, outline, planner, quiz, sources, srs, tts};

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
        yield Ok(Event::default().event("endpoint").data("/api/mcp/message"));
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

fn err(text: String) -> Value {
    json!({ "isError": true, "content": [{ "type": "text", "text": text }] })
}

pub async fn mcp_message(
    State(state): State<AppState>,
    Json(req): Json<JsonRpcRequest>,
) -> Json<Value> {
    // Results go back in the HTTP response only — never mirrored onto the SSE
    // fan-out (that would leak every caller's payload to every client).
    let reply = |result: Value| -> Json<Value> {
        Json(json!({ "jsonrpc": "2.0", "id": req.id, "result": result }))
    };
    match req.method.as_str() {
        "initialize" => reply(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "study-mcp", "version": "1.0.0" }
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

fn s(args: &Value, k: &str) -> String {
    args[k].as_str().unwrap_or("").trim().to_string()
}

fn opt(args: &Value, k: &str) -> Option<String> {
    let v = s(args, k);
    (!v.is_empty()).then_some(v)
}

fn int(args: &Value, k: &str, d: i64) -> i64 {
    args[k].as_i64().unwrap_or(d)
}

fn str_list(args: &Value, k: &str) -> Vec<String> {
    args[k]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn obj(props: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": props, "required": required })
}

fn today_of(db: &db::Db) -> String {
    let tz = srs::parse_tz(&db.setting("tz").unwrap_or_else(|| "Asia/Ho_Chi_Minh".into()));
    chrono::Utc::now()
        .with_timezone(&tz)
        .date_naive()
        .format("%Y-%m-%d")
        .to_string()
}

fn tools_list() -> Value {
    json!([
    {
        "name": "study_status",
        "description": "Tổng quan Study: số tài liệu, số kế hoạch, SỐ THẺ ĐẾN HẠN ÔN, và buổi học hôm nay. Gọi tool này TRƯỚC khi trả lời các câu như 'hôm nay học gì', 'còn bài nào phải ôn không'.",
        "inputSchema": obj(json!({}), &[])
    },
    {
        "name": "study_doc_add",
        "description": "Nạp tài liệu dạng VĂN BẢN THUẦN vào Study (dán nội dung, không phải đường dẫn tệp). Tự chia mục và lập chỉ mục tìm kiếm. Tệp PDF/DOCX thì người dùng tải lên bằng giao diện app. Dùng cho 'thêm tài liệu', 'học cái này', 'nạp giáo trình'.",
        "inputSchema": obj(json!({
            "title": {"type": "string", "description": "Tên tài liệu."},
            "text": {"type": "string", "description": "Toàn bộ nội dung văn bản."}
        }), &["title", "text"])
    },
    {
        "name": "study_doc_list",
        "description": "Liệt kê tài liệu đã nạp: id, tên, số mục, số đoạn, trạng thái (new/outlined/enriched). Gọi trước khi lập kế hoạch để lấy docId.",
        "inputSchema": obj(json!({}), &[])
    },
    {
        "name": "study_doc_outline",
        "description": "Xem dàn ý một tài liệu: danh sách mục kèm tóm tắt, ý chính, độ khó, số phút học ước tính và điều kiện tiên quyết.",
        "inputSchema": obj(json!({
            "doc_id": {"type": "string"}
        }), &["doc_id"])
    },
    {
        "name": "study_doc_enrich",
        "description": "Dùng AI mô tả từng mục của tài liệu (tóm tắt, ý chính, khái niệm, độ khó, số phút, tiên quyết). BẮT BUỘC chạy trước khi lập kế hoạch thì lịch mới sát thực tế. Trả về số mục đã mô tả và các mục lỗi.",
        "inputSchema": obj(json!({
            "doc_id": {"type": "string"}
        }), &["doc_id"])
    },
    {
        "name": "study_doc_summary",
        "description": "Tổng hợp toàn tài liệu thành: nói về cái gì, học xong làm được gì, nên học theo thứ tự nào. Viết từ dàn ý đã mô tả, không bịa.",
        "inputSchema": obj(json!({
            "doc_id": {"type": "string"}
        }), &["doc_id"])
    },
    {
        "name": "study_doc_delete",
        "description": "Xoá một tài liệu cùng toàn bộ mục, đoạn, thẻ và câu hỏi sinh ra từ nó.",
        "inputSchema": obj(json!({"doc_id": {"type": "string"}}), &["doc_id"])
    },
    {
        "name": "study_doc_clean",
        "description": "BƯỚC REVIEW làm sạch tài liệu. Không tham số `lines` = chỉ XEM các dòng ngắn lặp lại nhiều lần (thường là đầu/chân trang PDF) kèm số lần lặp — app KHÔNG tự xoá vì không phân biệt được đầu trang lặp với nhãn mục lặp kiểu 'Bài tập 1'. Truyền `lines` = bỏ đúng những dòng người dùng xác nhận, rồi lập chỉ mục lại. LUÔN hỏi người dùng trước khi bỏ.",
        "inputSchema": obj(json!({
            "doc_id": {"type": "string"},
            "lines": {"type": "array", "items": {"type": "string"}, "description": "Các dòng cần bỏ. Bỏ trống = chỉ xem danh sách."}
        }), &["doc_id"])
    },
    {
        "name": "study_reindex",
        "description": "Chia mục và lập chỉ mục lại một tài liệu (không gọi AI). Dùng khi dàn ý bị sai hoặc tìm kiếm không ra.",
        "inputSchema": obj(json!({"doc_id": {"type": "string"}}), &["doc_id"])
    },
    {
        "name": "study_concepts",
        "description": "Bản đồ khái niệm của tài liệu: mỗi khái niệm kèm các mục dạy nó.",
        "inputSchema": obj(json!({"doc_id": {"type": "string"}}), &["doc_id"])
    },
    {
        "name": "study_templates",
        "description": "Các mẫu lộ trình dựng sẵn (Nước rút / Chuẩn / Chuyên sâu / Vi mô / Ôn lại) kèm số ngày, phút mỗi ngày và mốc ôn. Gợi ý mẫu cho người dùng trước khi tạo kế hoạch.",
        "inputSchema": obj(json!({}), &[])
    },
    {
        "name": "study_plan_preview",
        "description": "XEM TRƯỚC lộ trình học — KHÔNG ghi gì vào cơ sở dữ liệu và KHÔNG tạo lịch. Trả về từng buổi, tổng thời lượng, và nếu không đủ thời gian thì trả về danh sách mục bị bỏ + 3 cách xử lý. LUÔN gọi tool này trước study_plan_create để người dùng duyệt.",
        "inputSchema": obj(json!({
            "doc_ids": {"type": "array", "items": {"type": "string"}, "description": "Các tài liệu cần học."},
            "template": {"type": "string", "description": "Khoá mẫu: sprint | standard | mastery | micro | refresher. Mặc định standard."},
            "days": {"type": "integer", "description": "Số BUỔI học (không phải số ngày lịch nếu có lọc thứ)."},
            "min_per_day": {"type": "integer", "description": "Số phút mỗi buổi."},
            "start_date": {"type": "string", "description": "YYYY-MM-DD. Mặc định hôm nay."},
            "weekdays": {"type": "string", "description": "Các thứ học, 1=Thứ Hai … 7=Chủ Nhật, ví dụ '2,4,6'. Mặc định tất cả."},
            "slot_hm": {"type": "string", "description": "Giờ bắt đầu HH:MM. Mặc định 20:00."}
        }), &["doc_ids"])
    },
    {
        "name": "study_plan_create",
        "description": "Tạo lộ trình học và LƯU lại. Đặt sync_calendar=true để đẩy luôn mỗi buổi thành một sự kiện trên lịch SenClaw (mở sự kiện sẽ mở đúng bài học hôm đó). Tham số giống study_plan_preview.",
        "inputSchema": obj(json!({
            "doc_ids": {"type": "array", "items": {"type": "string"}},
            "title": {"type": "string"},
            "goal": {"type": "string", "description": "Mục tiêu người học nói ra, ví dụ 'thi cuối kỳ ngày 20/9'."},
            "template": {"type": "string"},
            "days": {"type": "integer"},
            "min_per_day": {"type": "integer"},
            "start_date": {"type": "string"},
            "weekdays": {"type": "string"},
            "slot_hm": {"type": "string"},
            "sync_calendar": {"type": "boolean", "description": "Tạo sự kiện lịch cho từng buổi. Mặc định false."},
            "reminder_min": {"type": "integer", "description": "Nhắc trước bao nhiêu phút."}
        }), &["doc_ids"])
    },
    {
        "name": "study_plan_list",
        "description": "Liệt kê các lộ trình học: số buổi, số buổi đã xong, số buổi đã lên lịch.",
        "inputSchema": obj(json!({}), &[])
    },
    {
        "name": "study_plan_sessions",
        "description": "Chi tiết từng buổi của một lộ trình: ngày, giờ, thời lượng, các phần việc (đọc/thẻ/ôn/trắc nghiệm) và đã có sự kiện lịch chưa.",
        "inputSchema": obj(json!({"plan_id": {"type": "string"}}), &["plan_id"])
    },
    {
        "name": "study_plan_delete",
        "description": "Xoá lộ trình VÀ gỡ hết sự kiện lịch của nó.",
        "inputSchema": obj(json!({"plan_id": {"type": "string"}}), &["plan_id"])
    },
    {
        "name": "study_calendar_sync",
        "description": "Đẩy các buổi của một lộ trình lên lịch SenClaw. Chạy lại nhiều lần an toàn: buổi đã có sự kiện thì cập nhật, buổi đã học xong thì bỏ qua. Mỗi sự kiện mang liên kết mở thẳng bài học.",
        "inputSchema": obj(json!({
            "plan_id": {"type": "string"},
            "reminder_min": {"type": "integer", "description": "Nhắc trước bao nhiêu phút (bỏ trống = chỉ báo đúng giờ bắt đầu)."}
        }), &["plan_id"])
    },
    {
        "name": "study_calendar_unsync",
        "description": "Gỡ toàn bộ sự kiện lịch của một lộ trình (giữ nguyên lộ trình).",
        "inputSchema": obj(json!({"plan_id": {"type": "string"}}), &["plan_id"])
    },
    {
        "name": "study_today",
        "description": "Buổi học của HÔM NAY (mọi lộ trình đang chạy) kèm nội dung từng phần việc, và số thẻ đến hạn ôn. Đây là tool trả lời 'hôm nay học gì'.",
        "inputSchema": obj(json!({}), &[])
    },
    {
        "name": "study_session_open",
        "description": "Mở một buổi học: trả về từng phần việc kèm NỘI DUNG tài liệu tương ứng (đã cắt đúng phần của buổi đó), tóm tắt và ý chính.",
        "inputSchema": obj(json!({"session_id": {"type": "string"}}), &["session_id"])
    },
    {
        "name": "study_session_complete",
        "description": "Đánh dấu một buổi học đã hoàn thành (hoặc bỏ đánh dấu với done=false).",
        "inputSchema": obj(json!({
            "session_id": {"type": "string"},
            "done": {"type": "boolean", "description": "Mặc định true."}
        }), &["session_id"])
    },
    {
        "name": "study_cards_due",
        "description": "Các thẻ ghi nhớ ĐẾN HẠN ôn ngay bây giờ, thẻ mới xếp trước. Dùng cho 'ôn bài', 'có gì cần ôn không'.",
        "inputSchema": obj(json!({"limit": {"type": "integer", "description": "Mặc định 20."}}), &[])
    },
    {
        "name": "study_card_review",
        "description": "Chấm một thẻ theo mức tự đánh giá và lên lịch ôn lại: again (quên) | hard (khó) | good (được) | easy (dễ). Thang giãn cách: 30 phút → 1 → 3 → 7 → 30 → 90 ngày.",
        "inputSchema": obj(json!({
            "card_id": {"type": "string"},
            "grade": {"type": "string", "description": "again | hard | good | easy"}
        }), &["card_id", "grade"])
    },
    {
        "name": "study_cards_generate",
        "description": "Sinh thẻ ghi nhớ từ một MỤC của tài liệu (ưu tiên dạng điền khuyết lấy nguyên câu trong tài liệu). Trả về số thẻ tạo mới, số trùng và số bị loại.",
        "inputSchema": obj(json!({
            "section_id": {"type": "string"},
            "count": {"type": "integer", "description": "Mặc định 8, tối đa 30."}
        }), &["section_id"])
    },
    {
        "name": "study_quiz_generate",
        "description": "Sinh câu hỏi kiểm tra từ một MỤC. Mỗi câu bắt buộc kèm trích dẫn nguyên văn từ tài liệu; câu nào không kiểm chứng được sẽ bị loại và báo lý do. Dạng: single, multi, truefalse, cloze, order, match.",
        "inputSchema": obj(json!({
            "section_id": {"type": "string"},
            "count": {"type": "integer", "description": "Mặc định 6, tối đa 20."},
            "kinds": {"type": "array", "items": {"type": "string"}, "description": "Giới hạn dạng câu hỏi."}
        }), &["section_id"])
    },
    {
        "name": "study_quiz_take",
        "description": "Lấy một đề kiểm tra từ ngân hàng câu hỏi (ưu tiên câu hay sai và lâu chưa làm). KHÔNG kèm đáp án — chấm bằng study_quiz_grade.",
        "inputSchema": obj(json!({
            "doc_id": {"type": "string"},
            "section_ids": {"type": "array", "items": {"type": "string"}},
            "count": {"type": "integer", "description": "Mặc định 10."}
        }), &["doc_id"])
    },
    {
        "name": "study_quiz_grade",
        "description": "Chấm bài. Chấm bằng mã, không bằng AI. Mỗi câu trả về đúng/sai, giải thích và TRÍCH DẪN gốc trong tài liệu; câu sai tự sinh thẻ ghi nhớ để ôn lại.",
        "inputSchema": obj(json!({
            "quiz_id": {"type": "string", "description": "Lấy từ study_quiz_take."},
            "answers": {"type": "array", "description": "Mảng {question_id, answer}. Kiểu của answer theo dạng câu hỏi: single=chỉ số, multi=mảng chỉ số, truefalse=true/false, cloze=chuỗi, order/match=mảng chỉ số.", "items": {"type": "object"}}
        }), &["quiz_id", "answers"])
    },
    {
        "name": "study_weak_concepts",
        "description": "Các khái niệm người học hay sai nhất trong tài liệu, kèm tỉ lệ sai. Dùng để chọn nội dung ôn tiếp.",
        "inputSchema": obj(json!({"doc_id": {"type": "string"}}), &["doc_id"])
    },
    {
        "name": "study_ask",
        "description": "Hỏi đáp TRONG tài liệu của người học. Trả lời kèm trích dẫn [n] trỏ đúng đoạn (có docId và vị trí ký tự để mở tới nơi). Không bịa: nếu tài liệu không nói thì trả lời là không có.",
        "inputSchema": obj(json!({
            "question": {"type": "string"},
            "doc_ids": {"type": "array", "items": {"type": "string"}, "description": "Giới hạn trong các tài liệu này. Bỏ trống = tất cả."}
        }), &["question"])
    },
    {
        "name": "study_research",
        "description": "Như study_ask nhưng MỞ RỘNG sang các MCP tra cứu đang chạy (tự phát hiện, không gắn cứng địa chỉ). Bằng chứng từ nguồn ngoài được ghi nhãn RÕ là nguồn ngoài, chưa có trong tài liệu của người học — và không bao giờ dùng để ra đề kiểm tra.",
        "inputSchema": obj(json!({
            "question": {"type": "string"},
            "doc_ids": {"type": "array", "items": {"type": "string"}},
            "sources": {"type": "string", "description": "'auto' hoặc danh sách 'server.tool' ngăn cách bằng dấu phẩy."}
        }), &["question"])
    },
    {
        "name": "study_sources",
        "description": "Các nguồn MCP tra cứu hiện phát hiện được và nguồn nào đang được chọn. Tool ghi/sửa/xoá bị loại — nguồn bằng chứng không được có tác dụng phụ.",
        "inputSchema": obj(json!({}), &[])
    },
    {
        "name": "study_speak",
        "description": "Đọc một đoạn thành tiếng bằng TTS của SenClaw, trả về đường dẫn audio (đã cache). Nếu chưa cài giọng đọc thì báo lỗi rõ ràng chứ không im lặng.",
        "inputSchema": obj(json!({
            "text": {"type": "string", "description": "Tối đa ~1200 ký tự; dài hơn thì đặt split=true."},
            "split": {"type": "boolean", "description": "Cắt thành từng câu và đọc lần lượt."},
            "voice": {"type": "string"},
            "speed": {"type": "number", "description": "0.5–2.0, mặc định 1.0."}
        }), &["text"])
    },
    {
        "name": "study_settings",
        "description": "Xem hoặc đổi cài đặt: múi giờ, giờ học mặc định, khung giờ ôn, nguồn MCP tra cứu, giọng đọc, tốc độ đọc. Không truyền tham số nào = chỉ xem.",
        "inputSchema": obj(json!({
            "tz": {"type": "string"},
            "slot_hm": {"type": "string"},
            "study_slots": {"type": "array", "items": {"type": "string"}},
            "search_mcp": {"type": "string"},
            "voice": {"type": "string"},
            "speed": {"type": "number"}
        }), &[])
    }
    ])
}

async fn call_tool(state: &AppState, name: &str, args: &Value) -> Value {
    let db = &state.db;
    match name {
        "study_status" => {
            let now = srs::fmt(chrono::Utc::now());
            let today = today_of(db);
            match (db.doc_list(), db.plan_list(), db.card_due_count(&now), db.sessions_on(&today)) {
                (Ok(d), Ok(p), Ok(c), Ok(t)) => json_result(json!({
                    "docs": d.len(), "plans": p.len(), "cardsDue": c,
                    "today": today, "todaySessions": t,
                })),
                _ => err("không đọc được trạng thái".into()),
            }
        }

        // ── Documents ───────────────────────────────────────────────────────
        "study_doc_add" => {
            let title = s(args, "title");
            let text = args["text"].as_str().unwrap_or("");
            if title.is_empty() || text.trim().is_empty() {
                return err("cần cả `title` và `text`".into());
            }
            match ingest::ingest(db, "dan-vao.md", text.as_bytes(), &title) {
                Ok(mut v) => {
                    // The MCP surface has always called it `docId`.
                    v["docId"] = v["id"].clone();
                    json_result(v)
                }
                Err(e) => err(e),
            }
        }
        "study_doc_list" => match db.doc_list() {
            Ok(v) => json_result(json!(v)),
            Err(e) => err(e.to_string()),
        },
        "study_doc_outline" => match db.sections_of(&s(args, "doc_id")) {
            Ok(v) if v.is_empty() => err("tài liệu chưa có mục nào — chạy study_reindex".into()),
            Ok(v) => json_result(json!(v)),
            Err(e) => err(e.to_string()),
        },
        "study_doc_enrich" => match outline::enrich_document(db, &s(args, "doc_id")).await {
            Ok((n, problems)) => {
                let _ = db.doc_set_status(&s(args, "doc_id"), "enriched", None);
                json_result(json!({ "enriched": n, "problems": problems }))
            }
            Err(e) => err(e),
        },
        "study_doc_summary" => match outline::summarize_document(db, &s(args, "doc_id")).await {
            Ok(v) => text_result(v),
            Err(e) => err(e),
        },
        "study_doc_delete" => match db.doc_delete(&s(args, "doc_id")) {
            Ok(_) => text_result("đã xoá tài liệu".into()),
            Err(e) => err(e.to_string()),
        },
        "study_doc_clean" => {
            let id = s(args, "doc_id");
            let lines = str_list(args, "lines");
            if lines.is_empty() {
                return match db.suspects(&id) {
                    Ok(v) if v.is_empty() => text_result(
                        "Không có dòng nào lặp bất thường — tài liệu sạch.".into(),
                    ),
                    Ok(v) => json_result(json!({
                        "suspectedFurniture": v,
                        "hint": "hỏi người dùng dòng nào là đầu/chân trang, rồi gọi lại tool này với `lines`",
                    })),
                    Err(e) => err(e.to_string()),
                };
            }
            match db.strip_lines(&id, &lines) {
                Ok(mut out) => match outline::index_document(db, &id) {
                    Ok((sec, ch, note)) => {
                        let (repointed, orphaned) =
                            db.repoint_questions(&id).unwrap_or((0, 0));
                        out["sections"] = json!(sec);
                        out["chunks"] = json!(ch);
                        out["note"] = json!(note);
                        out["questionsRepointed"] = json!(repointed);
                        out["questionsOrphaned"] = json!(orphaned);
                        json_result(out)
                    }
                    Err(e) => err(e),
                },
                Err(e) => err(e.to_string()),
            }
        }
        "study_reindex" => match outline::index_document(db, &s(args, "doc_id")) {
            Ok((sec, ch, note)) => json_result(json!({ "sections": sec, "chunks": ch, "note": note })),
            Err(e) => err(e),
        },
        "study_concepts" => match db.concept_map(&s(args, "doc_id")) {
            Ok(v) => json_result(json!(v)),
            Err(e) => err(e.to_string()),
        },

        // ── Plans ───────────────────────────────────────────────────────────
        "study_templates" => match db.templates() {
            Ok(v) => json_result(json!(v)),
            Err(e) => err(e.to_string()),
        },
        "study_plan_preview" | "study_plan_create" => plan_tool(db, name, args).await,
        "study_plan_list" => match db.plan_list() {
            Ok(v) => json_result(json!(v)),
            Err(e) => err(e.to_string()),
        },
        "study_plan_sessions" => match db.sessions_of_plan(&s(args, "plan_id")) {
            Ok(v) => json_result(json!(v)),
            Err(e) => err(e.to_string()),
        },
        "study_plan_delete" => {
            let id = s(args, "plan_id");
            let removed = calendar::unsync_plan(db, &id).await.unwrap_or(0);
            match db.plan_delete(&id) {
                Ok(_) => json_result(json!({ "ok": true, "eventsRemoved": removed })),
                Err(e) => err(e.to_string()),
            }
        }
        "study_calendar_sync" => {
            let rm = args["reminder_min"].as_i64();
            match calendar::sync_plan(db, &s(args, "plan_id"), rm).await {
                Ok(r) => json_result(serde_json::to_value(r).unwrap_or(Value::Null)),
                Err(e) => err(e),
            }
        }
        "study_calendar_unsync" => match calendar::unsync_plan(db, &s(args, "plan_id")).await {
            Ok(n) => json_result(json!({ "removed": n })),
            Err(e) => err(e),
        },

        // ── Sessions ────────────────────────────────────────────────────────
        "study_today" => {
            let d = today_of(db);
            match db.sessions_on(&d) {
                Ok(list) => {
                    let now = srs::fmt(chrono::Utc::now());
                    json_result(json!({
                        "date": d,
                        "sessions": list,
                        "cardsDue": db.card_due_count(&now).unwrap_or(0),
                    }))
                }
                Err(e) => err(e.to_string()),
            }
        }
        "study_session_open" => match db.session_get(&s(args, "session_id")) {
            Ok(Some(v)) => json_result(lesson::attach_text(db, v)),
            Ok(None) => err("không tìm thấy buổi học".into()),
            Err(e) => err(e.to_string()),
        },
        "study_session_complete" => {
            let done = args["done"].as_bool().unwrap_or(true);
            match db.session_complete(&s(args, "session_id"), done) {
                Ok(_) => text_result(if done { "đã đánh dấu hoàn thành".into() } else { "đã bỏ đánh dấu".into() }),
                Err(e) => err(e.to_string()),
            }
        }

        // ── Cards ───────────────────────────────────────────────────────────
        "study_cards_due" => {
            let now = srs::fmt(chrono::Utc::now());
            let limit = int(args, "limit", 20).clamp(1, 200) as usize;
            match (db.cards_due(&now, limit), db.card_due_count(&now)) {
                (Ok(due), Ok(total)) => json_result(json!({ "due": due, "total": total })),
                (Err(e), _) | (_, Err(e)) => err(e.to_string()),
            }
        }
        "study_card_review" => match cards::review(db, &s(args, "card_id"), &s(args, "grade")) {
            Ok(v) => json_result(v),
            Err(e) => err(e),
        },
        "study_cards_generate" => {
            let n = int(args, "count", 8).clamp(1, 30) as usize;
            match cards::generate_for_section(db, &s(args, "section_id"), n).await {
                Ok(r) => json_result(serde_json::to_value(r).unwrap_or(Value::Null)),
                Err(e) => err(e),
            }
        }

        // ── Quiz ────────────────────────────────────────────────────────────
        "study_quiz_generate" => {
            let n = int(args, "count", 6).clamp(1, 20) as usize;
            let kinds = str_list(args, "kinds");
            match quiz::generate_for_section(db, &s(args, "section_id"), n, &kinds).await {
                Ok(r) => json_result(serde_json::to_value(r).unwrap_or(Value::Null)),
                Err(e) => err(e),
            }
        }
        "study_quiz_take" => {
            let sections = str_list(args, "section_ids");
            let n = int(args, "count", 10).clamp(1, 50) as usize;
            match db.questions_pick(&s(args, "doc_id"), &sections, n) {
                Ok(mut qs) if !qs.is_empty() => {
                    for q in qs.iter_mut() {
                        if let Some(o) = q.as_object_mut() {
                            // The answer key stays server-side until grading.
                            o.remove("answer");
                            o.remove("explain");
                            o.remove("quote");
                        }
                    }
                    json_result(json!({ "quizId": db::new_id(), "questions": qs }))
                }
                Ok(_) => err("chưa có câu hỏi nào cho tài liệu này — chạy study_quiz_generate trước".into()),
                Err(e) => err(e.to_string()),
            }
        }
        "study_quiz_grade" => {
            let pairs: Vec<(String, Value)> = args["answers"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| {
                            let id = x["question_id"].as_str()?.to_string();
                            Some((id, x["answer"].clone()))
                        })
                        .collect()
                })
                .unwrap_or_default();
            if pairs.is_empty() {
                return err("không có câu trả lời nào".into());
            }
            match quiz::grade(db, &s(args, "quiz_id"), &pairs) {
                Ok(v) => json_result(v),
                Err(e) => err(e),
            }
        }
        "study_weak_concepts" => match db.weak_concepts(&s(args, "doc_id"), 20) {
            Ok(v) => json_result(json!(v)),
            Err(e) => err(e.to_string()),
        },

        // ── Ask ─────────────────────────────────────────────────────────────
        "study_ask" => {
            let q = s(args, "question");
            if q.is_empty() {
                return err("chưa có câu hỏi".into());
            }
            match ask::ask(db, &q, &str_list(args, "doc_ids")).await {
                Ok(v) => json_result(v),
                Err(e) => err(e),
            }
        }
        "study_research" => {
            let q = s(args, "question");
            if q.is_empty() {
                return err("chưa có câu hỏi".into());
            }
            let setting = opt(args, "sources")
                .or_else(|| db.setting("search_mcp"))
                .unwrap_or_else(|| "auto".into());
            match ask::research(db, &q, &str_list(args, "doc_ids"), &setting).await {
                Ok(v) => json_result(v),
                Err(e) => err(e),
            }
        }
        "study_sources" => {
            let all = sources::discover().await;
            let setting = db.setting("search_mcp").unwrap_or_else(|| "auto".into());
            let picked = sources::select(&all, &setting, 2);
            json_result(json!({
                "setting": setting,
                "available": all.iter().map(|s| s.to_json()).collect::<Vec<_>>(),
                "selected": picked.iter().map(|s| s.key()).collect::<Vec<_>>(),
            }))
        }

        // ── Audio + settings ────────────────────────────────────────────────
        "study_speak" => {
            let text = args["text"].as_str().unwrap_or("");
            let voice = opt(args, "voice").or_else(|| db.setting("voice"));
            let speed = args["speed"].as_f64().unwrap_or_else(|| {
                db.setting("speed").and_then(|v| v.parse().ok()).unwrap_or(1.0)
            });
            if args["split"].as_bool().unwrap_or(false) {
                let mut clips = Vec::new();
                for p in tts::sentences(text, 400) {
                    match tts::speak(db, &p, voice.as_deref(), speed, None).await {
                        Ok(n) => clips.push(json!({ "text": p, "url": format!("/api/audio/{n}") })),
                        Err(e) => return err(e),
                    }
                }
                return json_result(json!({ "clips": clips }));
            }
            match tts::speak(db, text, voice.as_deref(), speed, None).await {
                Ok(n) => json_result(json!({ "url": format!("/api/audio/{n}") })),
                Err(e) => err(e),
            }
        }
        "study_settings" => {
            for (k, key) in [
                ("tz", "tz"),
                ("slot_hm", "slot_hm"),
                ("search_mcp", "search_mcp"),
                ("voice", "voice"),
            ] {
                if let Some(v) = opt(args, k) {
                    let _ = db.set_setting(key, &v);
                }
            }
            if let Some(v) = args["speed"].as_f64() {
                let _ = db.set_setting("speed", &v.to_string());
            }
            let slots = str_list(args, "study_slots");
            if !slots.is_empty() {
                let _ = db.set_setting("study_slots", &serde_json::to_string(&slots).unwrap_or_default());
            }
            json_result(json!({
                "tz": db.setting("tz").unwrap_or_else(|| "Asia/Ho_Chi_Minh".into()),
                "slotHm": db.setting("slot_hm").unwrap_or_else(|| "20:00".into()),
                "studySlots": db.setting("study_slots").unwrap_or_else(|| "[\"20:00\"]".into()),
                "searchMcp": db.setting("search_mcp").unwrap_or_else(|| "auto".into()),
                "voice": db.setting("voice"),
                "speed": db.setting("speed").unwrap_or_else(|| "1.0".into()),
            }))
        }

        other => err(format!("tool không tồn tại: {other}")),
    }
}

/// Shared plan preview/create path, so the two tools cannot drift apart.
async fn plan_tool(db: &db::Db, name: &str, args: &Value) -> Value {
    let doc_ids = str_list(args, "doc_ids");
    if doc_ids.is_empty() {
        return err("cần `doc_ids` — gọi study_doc_list để lấy id".into());
    }
    let key = opt(args, "template").unwrap_or_else(|| "standard".into());
    let Ok(Some(t)) = db.template_get(&key) else {
        return err(format!("không có mẫu `{key}` — gọi study_templates"));
    };
    let tz_name = opt(args, "tz")
        .or_else(|| db.setting("tz"))
        .unwrap_or_else(|| "Asia/Ho_Chi_Minh".into());
    let tz = srs::parse_tz(&tz_name);
    let start = match opt(args, "start_date") {
        Some(d) => match chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d") {
            Ok(v) => v,
            Err(_) => return err(format!("ngày bắt đầu không hợp lệ: {d} (cần YYYY-MM-DD)")),
        },
        None => chrono::Utc::now().with_timezone(&tz).date_naive(),
    };
    let weekdays = opt(args, "weekdays").unwrap_or_else(|| "1,2,3,4,5,6,7".into());
    let sections = match db.sections_for_docs(&doc_ids) {
        Ok(v) if !v.is_empty() => v,
        Ok(_) => return err("tài liệu chưa được chia mục — chạy study_reindex trước".into()),
        Err(e) => return err(e.to_string()),
    };
    let unenriched = sections.iter().filter(|s| s.enriched_at.is_none()).count();

    let req = planner::PlanRequest {
        start_date: start,
        days: args["days"].as_i64().unwrap_or(t.days),
        min_per_day: args["min_per_day"].as_i64().unwrap_or(t.min_per_day),
        weekdays: planner::parse_weekdays(&weekdays),
        slot_hm: opt(args, "slot_hm")
            .or_else(|| db.setting("slot_hm"))
            .unwrap_or_else(|| "20:00".into()),
        review_offsets: t.review_offsets.clone(),
        blocks: t.blocks.clone(),
        content_ratio: t.content_ratio,
    };
    let preview = planner::build(&sections, &req);

    let mut warn = Vec::new();
    if unenriched > 0 {
        // Not a failure — but the learner should know the minute estimates are
        // guesses from length, not from what the material actually is.
        warn.push(format!(
            "{unenriched}/{} mục chưa được AI mô tả — số phút đang ước từ độ dài; chạy study_doc_enrich để lịch sát hơn",
            sections.len()
        ));
    }

    if name == "study_plan_preview" {
        let mut v = serde_json::to_value(&preview).unwrap_or(Value::Null);
        v["warnings"] = json!(warn);
        v["templateUsed"] = json!({ "key": t.key, "label": t.label, "detail": t.detail });
        return json_result(v);
    }

    if preview.sessions.is_empty() {
        return err("không xếp được buổi nào — kiểm tra số buổi và số phút mỗi buổi".into());
    }
    let title = opt(args, "title").unwrap_or_else(|| {
        format!("Học {} mục trong {} buổi", sections.len(), preview.sessions.len())
    });
    let plan_id = match db.plan_insert(
        &title,
        &opt(args, "goal").unwrap_or_default(),
        &doc_ids,
        &key,
        &start.format("%Y-%m-%d").to_string(),
        req.days,
        req.min_per_day,
        &weekdays,
        &req.slot_hm,
        &tz_name,
        &preview.notes.join(" · "),
        &preview,
    ) {
        Ok(v) => v,
        Err(e) => return err(e.to_string()),
    };

    let mut out = json!({
        "planId": plan_id,
        "title": title,
        "feasible": preview.feasible,
        "sessions": preview.sessions.len(),
        "dropped": preview.dropped,
        "options": preview.options,
        "notes": preview.notes,
        "warnings": warn,
    });
    if args["sync_calendar"].as_bool().unwrap_or(false) {
        match calendar::sync_plan(db, &plan_id, args["reminder_min"].as_i64()).await {
            Ok(r) => out["calendar"] = serde_json::to_value(r).unwrap_or(Value::Null),
            Err(e) => out["calendarError"] = json!(e),
        }
    }
    json_result(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_declares_a_name_description_and_schema() {
        let tools = tools_list();
        let arr = tools.as_array().unwrap();
        assert!(arr.len() >= 26, "expected the full toolset, got {}", arr.len());
        for t in arr {
            let name = t["name"].as_str().unwrap_or("");
            assert!(name.starts_with("study_"), "bad tool name: {name}");
            assert!(
                t["description"].as_str().map(str::len).unwrap_or(0) > 40,
                "{name} needs a description an agent can route on"
            );
            assert_eq!(t["inputSchema"]["type"], "object", "{name}");
        }
    }

    #[test]
    fn tool_names_are_unique() {
        let tools = tools_list();
        let mut names: Vec<&str> = tools
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        let n = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), n, "duplicate tool name");
    }

    #[test]
    fn required_fields_exist_in_each_schema() {
        for t in tools_list().as_array().unwrap() {
            let name = t["name"].as_str().unwrap();
            let props = t["inputSchema"]["properties"].as_object().unwrap();
            for r in t["inputSchema"]["required"].as_array().unwrap() {
                let key = r.as_str().unwrap();
                assert!(props.contains_key(key), "{name}: required `{key}` not declared");
            }
        }
    }

    #[tokio::test]
    async fn an_unknown_tool_is_an_error_not_a_silent_ok() {
        let db = db::Db::open_memory().unwrap();
        let (tx, _) = tokio::sync::broadcast::channel(4);
        let st = AppState { db, mcp_tx: tx };
        let out = call_tool(&st, "study_nope", &json!({})).await;
        assert_eq!(out["isError"], true);
    }

    #[tokio::test]
    async fn adding_a_document_indexes_it_and_reports_what_to_do_next() {
        let db = db::Db::open_memory().unwrap();
        let (tx, _) = tokio::sync::broadcast::channel(4);
        let st = AppState { db, mcp_tx: tx };
        let body = format!("# Chương 1\n\n{}", "nội dung học tập ".repeat(80));
        let out = call_tool(
            &st,
            "study_doc_add",
            &json!({ "title": "Giáo trình", "text": body }),
        )
        .await;
        let text = out["content"][0]["text"].as_str().unwrap();
        let v: Value = serde_json::from_str(text).unwrap();
        assert!(v["sections"].as_u64().unwrap() >= 1);
        assert!(v["chunks"].as_u64().unwrap() >= 1);
        assert!(v["next"].as_str().unwrap().contains("enrich"));
    }

    #[tokio::test]
    async fn a_quiz_handed_to_the_client_never_contains_the_answer_key() {
        let db = db::Db::open_memory().unwrap();
        let doc = db.doc_insert("D", "d.md", "md", 1, "ok", "nội dung").unwrap();
        db.question_insert(
            &doc,
            None,
            None,
            "single",
            "Câu hỏi?",
            &json!(["a", "b"]),
            &json!(0),
            "vì thế",
            1,
            "trích dẫn gốc",
            2,
        )
        .unwrap();
        let (tx, _) = tokio::sync::broadcast::channel(4);
        let st = AppState { db, mcp_tx: tx };
        let out = call_tool(&st, "study_quiz_take", &json!({ "doc_id": doc })).await;
        let text = out["content"][0]["text"].as_str().unwrap();
        assert!(!text.contains("\"answer\""), "answer key leaked: {text}");
        assert!(!text.contains("trích dẫn gốc"), "quote leaked before grading");
    }

    #[tokio::test]
    async fn planning_without_documents_says_which_tool_to_call() {
        let db = db::Db::open_memory().unwrap();
        let (tx, _) = tokio::sync::broadcast::channel(4);
        let st = AppState { db, mcp_tx: tx };
        let out = call_tool(&st, "study_plan_preview", &json!({ "doc_ids": [] })).await;
        assert_eq!(out["isError"], true);
        assert!(out["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("study_doc_list"));
    }
}
