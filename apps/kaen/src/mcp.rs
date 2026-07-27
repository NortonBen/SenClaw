//! MCP server — hand-rolled JSON-RPC over HTTP + SSE, matching the other Space
//! Apps (the `rmcp` crate is not used here).
//!
//! Tools are prefixed `kaen_`. Agents reach them as `mcp__kaen-mcp__kaen_*`.

use std::convert::Infallible;

use axum::extract::State;
use axum::response::sse::{Event, Sse};
use axum::Json;
use futures_util::Stream;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::ops;
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

fn error_result(text: String) -> Value {
    json!({ "isError": true, "content": [{ "type": "text", "text": text }] })
}

pub async fn mcp_message(
    State(state): State<AppState>,
    Json(req): Json<JsonRpcRequest>,
) -> Json<Value> {
    // Result goes back in the HTTP response only — never mirrored onto the SSE
    // fan-out (that would leak every caller's payload to every client).
    let reply = |result: Value| -> Json<Value> {
        Json(json!({ "jsonrpc": "2.0", "id": req.id, "result": result }))
    };
    match req.method.as_str() {
        "initialize" => reply(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "kaen-mcp", "version": "1.0.0" }
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

fn int(args: &Value, k: &str, d: i64) -> i64 {
    args[k].as_i64().unwrap_or(d)
}

fn tools_list() -> Value {
    json!([
        {
            "name": "kaen_status",
            "description": "Tổng quan Kaen: số bài học, tổng số thẻ, số từ đã học, SỐ TỪ ĐẾN HẠN ÔN ngay bây giờ, streak, tổng XP và trạng thái báo bận. Gọi tool này TRƯỚC TIÊN khi người dùng hỏi về việc học từ vựng ('hôm nay có từ nào cần ôn không', 'tình hình học từ'), để biết có gì đến hạn.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "kaen_lesson_list",
            "description": "Liệt kê các bài học từ vựng, mới nhất trước: id, tiêu đề, mô tả, số thẻ. Dùng cho 'danh sách bài học', 'tôi có những bài từ vựng nào'.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "kaen_lesson_show",
            "description": "Xem một bài học kèm TOÀN BỘ thẻ từ vựng của nó (word, IPA, nghĩa, ví dụ, giải thích). Dùng khi người dùng muốn xem nội dung một bài cụ thể.",
            "inputSchema": { "type": "object", "properties": {
                "lesson_id": { "type": "string", "description": "Id bài học (lấy từ kaen_lesson_list)." }
            }, "required": ["lesson_id"] }
        },
        {
            "name": "kaen_lesson_create",
            "description": "Tạo bài học rỗng. Sau đó thêm thẻ bằng kaen_card_add từng từ, hoặc dùng kaen_import_text để nạp cả danh sách một lần (nhanh hơn nhiều khi soạn bài cho người dùng).",
            "inputSchema": { "type": "object", "properties": {
                "title": { "type": "string", "description": "Tiêu đề bài học." },
                "description": { "type": "string", "description": "Mô tả ngắn (tuỳ chọn)." }
            }, "required": ["title"] }
        },
        {
            "name": "kaen_import_text",
            "description": "Tạo bài học từ danh sách từ dạng text, mỗi dòng một từ theo format: word|nghĩa tiếng Việt|câu ví dụ|loại từ|IPA|giải thích tiếng Anh|nghĩa khác dạng jp:りんご,fr:pomme. Chỉ 2 cột đầu là bắt buộc. Đây là cách NHANH NHẤT để agent soạn một bài học hoàn chỉnh cho người dùng — tự sinh danh sách từ theo chủ đề rồi import một lần.",
            "inputSchema": { "type": "object", "properties": {
                "title":     { "type": "string", "description": "Tiêu đề bài học." },
                "raw_text":  { "type": "string", "description": "Danh sách từ, mỗi dòng một từ theo format trên." },
                "separator": { "type": "string", "description": "Ký tự phân cách cột, mặc định '|'." }
            }, "required": ["title", "raw_text"] }
        },
        {
            "name": "kaen_card_add",
            "description": "Thêm một thẻ từ vựng vào bài học có sẵn. Nên điền đủ IPA, nghĩa tiếng Việt, ví dụ và giải thích tiếng Anh để thẻ học có chất lượng.",
            "inputSchema": { "type": "object", "properties": {
                "lesson_id":      { "type": "string", "description": "Id bài học." },
                "word":           { "type": "string", "description": "Từ vựng (tiếng Anh)." },
                "meaning_vi":     { "type": "string", "description": "Nghĩa tiếng Việt." },
                "explain":        { "type": "string", "description": "Giải thích ngắn bằng tiếng Anh." },
                "ipa":            { "type": "string", "description": "Phiên âm IPA, ví dụ /ˈæp.əl/." },
                "part_of_speech": { "type": "string", "description": "Loại từ: noun, verb, adjective…" },
                "example":        { "type": "string", "description": "Một câu ví dụ." }
            }, "required": ["lesson_id", "word"] }
        },
        {
            "name": "kaen_study_session",
            "description": "Mở phiên học 6 phút: trả về ~5 từ mới + ~5 từ cần ôn (trộn theo cấp độ). Truyền lesson_id để học riêng một bài (trả về toàn bộ thẻ của bài, từ mới xếp trước, từ đến hạn xếp giữa). Mỗi thẻ có progress.level (0-6) và nextReview. Sau khi người dùng trả lời từng từ, chấm bằng kaen_review_submit.",
            "inputSchema": { "type": "object", "properties": {
                "lesson_id": { "type": "string", "description": "Bỏ trống = phiên trộn tự động; điền id để học một bài cụ thể." }
            } }
        },
        {
            "name": "kaen_review_submit",
            "description": "Chấm kết quả ôn MỘT thẻ theo SRS: result REMEMBER (nhớ) hoặc FORGOT (quên). Quên hay từ mới → hẹn lại sau 30 phút; nhớ → lên cấp và giãn 1/3/7/30/90 ngày, giờ ôn snap vào khung giờ học của người dùng. mode TYPING (tự gõ, đúng được +5 XP) hoặc FLIP (lật thẻ). Trả về XP và cấp độ mới.",
            "inputSchema": { "type": "object", "properties": {
                "card_id": { "type": "string", "description": "Id thẻ (từ kaen_study_session / kaen_lesson_show)." },
                "result":  { "type": "string", "description": "REMEMBER | FORGOT." },
                "mode":    { "type": "string", "description": "FLIP (mặc định) | TYPING." }
            }, "required": ["card_id", "result"] }
        },
        {
            "name": "kaen_due_count",
            "description": "Đếm nhanh số từ đã đến hạn ôn ngay bây giờ (nextReview <= now). Rẻ và nhanh — dùng khi chỉ cần con số để nhắc người dùng, không cần mở phiên học.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "kaen_snooze",
            "description": "Báo bận: tạm dừng nhắc ôn trong N giờ (mặc định 1, tối đa 72). Dùng khi người dùng nói 'nhắc lại sau', 'đang bận'.",
            "inputSchema": { "type": "object", "properties": {
                "hours": { "type": "integer", "description": "Số giờ tạm hoãn, mặc định 1." }
            } }
        },
        {
            "name": "kaen_stats",
            "description": "Thống kê học tập: phân bố từ theo cấp độ SRS (0-6), tổng từ đã học, số từ mới/đã ôn hôm nay, streak và XP. Dùng cho 'hôm nay tôi học được bao nhiêu từ', 'thống kê học tập'.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "kaen_grammar_list",
            "description": "Liệt kê bài học ngữ pháp: id, tiêu đề, slug, level (A1-C1), số lượt xem, tiến độ ôn (dueForReview). Lọc được theo level/search/study (completed|pending). Dùng cho 'danh sách bài ngữ pháp', 'bài grammar nào cần ôn'.",
            "inputSchema": { "type": "object", "properties": {
                "level":  { "type": "string", "description": "A1 | A2 | B1 | B1-B2 | B2 | C1 | OTHER." },
                "search": { "type": "string", "description": "Tìm trong tiêu đề/mô tả." },
                "study":  { "type": "string", "description": "completed = đã làm test; pending = chưa." }
            } }
        },
        {
            "name": "kaen_grammar_show",
            "description": "Đọc TOÀN VĂN một bài ngữ pháp (markdown) theo id hoặc slug, kèm bài trước/sau cùng level và tiến độ ôn. Dùng khi người dùng muốn xem/được giảng lại một bài ngữ pháp.",
            "inputSchema": { "type": "object", "properties": {
                "id_or_slug": { "type": "string", "description": "Id hoặc slug bài ngữ pháp." }
            }, "required": ["id_or_slug"] }
        },
        {
            "name": "kaen_grammar_create",
            "description": "Tạo bài học ngữ pháp mới với nội dung markdown. Đây là cách agent soạn giáo trình ngữ pháp cho người dùng: viết bài giảng đầy đủ (giải thích, công thức, ví dụ, lỗi thường gặp) rồi lưu vào đây; sau đó có thể sinh bài test gắn kèm bằng kaen_grammar_test_generate.",
            "inputSchema": { "type": "object", "properties": {
                "title":       { "type": "string", "description": "Tiêu đề, ví dụ 'Thì hiện tại đơn'." },
                "content":     { "type": "string", "description": "Nội dung bài giảng (markdown)." },
                "description": { "type": "string", "description": "Mô tả ngắn (tuỳ chọn)." },
                "level":       { "type": "string", "description": "A1 | A2 | B1 | B1-B2 | B2 | C1 | OTHER (mặc định B1)." },
                "index":       { "type": "integer", "description": "Thứ tự trong level (mặc định 0)." }
            }, "required": ["title", "content"] }
        },
        {
            "name": "kaen_grammar_test_generate",
            "description": "Sinh bài test trắc nghiệm ngữ pháp bằng AI (qua LLM chung của SenClaw) và LƯU vào một chủ đề test. Truyền grammar_slug để gắn test với bài học (AI sẽ dựa vào đúng nội dung bài) — khi đó nộp bài sẽ tính tiến độ ôn 7 ngày. Trả về danh sách câu hỏi ĐÃ GIẤU đáp án; lấy topicId từ kết quả để làm bài.",
            "inputSchema": { "type": "object", "properties": {
                "topic":        { "type": "string", "description": "Tên chủ đề, ví dụ 'Present Simple'." },
                "level":        { "type": "string", "description": "A1-C1, mặc định A1." },
                "count":        { "type": "integer", "description": "Số câu (1-50, mặc định 10)." },
                "grammar_slug": { "type": "string", "description": "Slug/id bài ngữ pháp để gắn test (khuyến nghị)." }
            }, "required": ["topic"] }
        },
        {
            "name": "kaen_grammar_test_questions",
            "description": "Lấy tối đa 10 câu hỏi của một chủ đề test (đáp án bị giấu) để đố người dùng trong chat. Sau khi người dùng chọn xong, nộp bằng kaen_grammar_test_submit để chấm.",
            "inputSchema": { "type": "object", "properties": {
                "topic_id": { "type": "string", "description": "Id chủ đề test." }
            }, "required": ["topic_id"] }
        },
        {
            "name": "kaen_grammar_test_submit",
            "description": "Nộp và chấm bài test ngữ pháp: trả về điểm, đáp án đúng và giải thích từng câu. Nếu chủ đề gắn với bài học, tự động đánh dấu đã học + hẹn nhắc ôn sau 7 ngày. answers = [{question_id, selected_answer_id}].",
            "inputSchema": { "type": "object", "properties": {
                "topic_id": { "type": "string", "description": "Id chủ đề test." },
                "answers":  { "type": "array", "items": { "type": "object", "properties": {
                    "question_id": { "type": "string" },
                    "selected_answer_id": { "type": "string", "description": "A | B | C | D." }
                } }, "description": "Câu trả lời của người dùng." }
            }, "required": ["topic_id", "answers"] }
        },
        {
            "name": "kaen_story_generate",
            "description": "Sinh truyện AI 3 bước từ một bài học từ vựng (qua LLM chung của SenClaw): bước 1 truyện tiếng Anh dùng ĐỦ mọi từ trong bài, bước 2 bản Anh kèm nghĩa tiếng Việt trong ngoặc, bước 3 bản dịch hoàn toàn. Đọc truyện giúp 'gặp' từ trong ngữ cảnh — từ đã gặp trong truyện không bị tính là từ mới ở phiên học. Có thể mất 30-120 giây.",
            "inputSchema": { "type": "object", "properties": {
                "lesson_id":   { "type": "string", "description": "Id bài học nguồn (lấy từ kaen_lesson_list)." },
                "title":       { "type": "string", "description": "Tiêu đề truyện; bỏ trống dùng tên bài học." },
                "description": { "type": "string", "description": "Gợi ý bối cảnh/chủ đề truyện (tuỳ chọn)." },
                "native_language": { "type": "string", "description": "Ngôn ngữ mẹ đẻ, mặc định theo settings (vi)." }
            }, "required": ["lesson_id"] }
        },
        {
            "name": "kaen_story_list",
            "description": "Liệt kê truyện đã tạo (id, tiêu đề, bài học nguồn). Dùng cho 'truyện của tôi', 'danh sách truyện'.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "kaen_story_show",
            "description": "Đọc một truyện: đủ 3 bước nội dung, thẻ từ vựng của bài học nguồn và tiến độ đọc. Dùng khi người dùng muốn đọc lại truyện trong chat.",
            "inputSchema": { "type": "object", "properties": {
                "story_id": { "type": "string", "description": "Id truyện." }
            }, "required": ["story_id"] }
        },
        {
            "name": "kaen_dictation_list",
            "description": "Liệt kê bài luyện chép chính tả (dictation) kèm phần trăm hoàn thành. Lọc theo topic slug nếu cần. Nội dung dictation được nạp bằng kaen_dictation_import.",
            "inputSchema": { "type": "object", "properties": {
                "topic": { "type": "string", "description": "Slug chủ đề (từ /api/dictation-lessons/topics)." },
                "limit": { "type": "integer" }, "page": { "type": "integer" }
            } }
        },
        {
            "name": "kaen_dictation_import",
            "description": "Nạp nội dung dictation từ JSON (output của crawler dailydictation hoặc tự soạn): { topics: [{name, slug, level?}], lessons: [{title, topicSlug, level?, audioUrl?, youtubeVideoId?, segments: [{content, solutions?, startTime, endTime}] }] }. audioUrl là URL audio đầy đủ; client tự tua theo startTime/endTime.",
            "inputSchema": { "type": "object", "properties": {
                "json": { "type": "string", "description": "Chuỗi JSON theo cấu trúc trên." }
            }, "required": ["json"] }
        },
        {
            "name": "kaen_dict_lookup",
            "description": "Tra từ điển một từ tiếng Anh: IPA, loại từ, định nghĩa, ví dụ, audio phát âm và bản dịch (mặc định tiếng Việt). Có cache — tra lại tức thì. Dùng khi người dùng hỏi nghĩa/phát âm một từ.",
            "inputSchema": { "type": "object", "properties": {
                "word":        { "type": "string", "description": "Từ cần tra." },
                "target_lang": { "type": "string", "description": "Mã ngôn ngữ dịch, mặc định 'vi'." }
            }, "required": ["word"] }
        },
        {
            "name": "kaen_settings_get",
            "description": "Đọc cấu hình: khung giờ học (study slots), múi giờ, mục tiêu từ mỗi ngày, ngôn ngữ mẹ đẻ.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "kaen_settings_set",
            "description": "Chỉnh cấu hình. study_slots là mảng giờ 'HH:MM' — giờ ôn của các từ cấp ≥2 sẽ snap vào slot ĐẦU TIÊN theo đúng múi giờ. Đổi lịch chỉ ảnh hưởng các lần hẹn ôn tương lai, không sửa lịch đã đặt.",
            "inputSchema": { "type": "object", "properties": {
                "study_slots":     { "type": "array", "items": { "type": "string" }, "description": "Ví dụ [\"08:00\",\"20:00\"]." },
                "timezone":        { "type": "string", "description": "IANA timezone, ví dụ Asia/Ho_Chi_Minh." },
                "daily_word_goal": { "type": "integer", "description": "Mục tiêu số từ mỗi ngày." },
                "native_language": { "type": "string", "description": "Mã ngôn ngữ mẹ đẻ, ví dụ 'vi'." }
            } }
        }
    ])
}

async fn call_tool(state: &AppState, name: &str, args: &Value) -> Value {
    let db = &state.db;

    // Network-bound tools run before the sync block.
    if name == "kaen_story_generate" {
        let lesson_id = s(args, "lesson_id");
        if lesson_id.is_empty() {
            return error_result("Thiếu lesson_id".to_string());
        }
        let native = db
            .settings()
            .map(|st| st.native_language)
            .unwrap_or_else(|_| "vi".into());
        let lang = {
            let l = s(args, "native_language");
            if l.is_empty() { native } else { l }
        };
        return match crate::story::generate_story(
            db,
            &lesson_id,
            &s(args, "title"),
            &s(args, "description"),
            &lang,
        )
        .await
        {
            Ok(v) => json_result(v),
            Err(e) => error_result(e.to_string()),
        };
    }
    if name == "kaen_dict_lookup" {
        let word = s(args, "word");
        if word.is_empty() {
            return error_result("Thiếu word".to_string());
        }
        let lang = {
            let l = s(args, "target_lang");
            if l.is_empty() { "vi".to_string() } else { l }
        };
        return match crate::dictionary::lookup(db, &word, &lang).await {
            Ok(v) => json_result(v),
            Err(e) => error_result(e.to_string()),
        };
    }
    if name == "kaen_grammar_test_generate" {
        let topic = s(args, "topic");
        if topic.is_empty() {
            return error_result("Thiếu topic".to_string());
        }
        let level = {
            let l = s(args, "level");
            if l.is_empty() { "A1".to_string() } else { l }
        };
        let count = int(args, "count", 10).clamp(1, 50) as u32;
        let link = s(args, "grammar_slug");
        let link = (!link.is_empty()).then_some(link);
        let content = link
            .as_deref()
            .and_then(|l| crate::grammar::grammar_content(db, l).ok().flatten())
            .map(|(_, _, c)| c);
        return match crate::llm::generate_grammar_questions(&topic, &level, count, content.as_deref())
            .await
        {
            Ok(items) => {
                match crate::grammar::save_generated_questions(db, &topic, &level, link.as_deref(), &items)
                {
                    Ok(v) => json_result(v),
                    Err(e) => error_result(e.to_string()),
                }
            }
            Err(e) => error_result(e),
        };
    }

    let r: anyhow::Result<Value> = (|| {
        match name {
            "kaen_status" => {
                let settings = db.settings()?;
                let lessons = db.list_lessons()?;
                let total_cards: i64 = lessons.iter().map(|l| l.card_count).sum();
                Ok(json!({
                    "lessons": lessons.len(),
                    "totalCards": total_cards,
                    "learnedWords": db.learned_count()?,
                    "dueNow": db.due_count(chrono::Utc::now())?,
                    "grammarDueForReview": crate::grammar::due_reminder_count(db)?,
                    "currentStreak": settings.current_streak,
                    "totalXP": settings.total_xp,
                    "snoozeUntil": settings.snooze_until,
                    "dailyWordGoal": settings.daily_word_goal,
                }))
            }
            "kaen_grammar_list" => {
                let level = s(args, "level");
                let search = s(args, "search");
                let study = s(args, "study");
                crate::grammar::list_grammars(
                    db,
                    1,
                    100,
                    (!level.is_empty()).then_some(level.as_str()),
                    (!search.is_empty()).then_some(search.as_str()),
                    (!study.is_empty()).then_some(study.as_str()),
                )
            }
            "kaen_grammar_show" => crate::grammar::view_grammar(db, &s(args, "id_or_slug")),
            "kaen_grammar_create" => {
                let title = s(args, "title");
                let content = s(args, "content");
                if title.is_empty() || content.is_empty() {
                    anyhow::bail!("Thiếu title hoặc content");
                }
                let desc = s(args, "description");
                let level = s(args, "level");
                crate::grammar::create_grammar(
                    db,
                    &title,
                    &content,
                    (!desc.is_empty()).then_some(desc.as_str()),
                    if level.is_empty() { "B1" } else { &level },
                    int(args, "index", 0),
                )
            }
            "kaen_grammar_test_questions" => {
                crate::grammar::questions_for_topic(db, &s(args, "topic_id"))
            }
            "kaen_grammar_test_submit" => {
                let topic_id = s(args, "topic_id");
                if topic_id.is_empty() {
                    anyhow::bail!("Thiếu topic_id");
                }
                let answers: Vec<Value> = args["answers"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .map(|a| {
                                json!({
                                    "questionId": a["question_id"].as_str().or(a["questionId"].as_str()).unwrap_or(""),
                                    "selectedAnswerId": a["selected_answer_id"].as_str().or(a["selectedAnswerId"].as_str()).unwrap_or(""),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                if answers.is_empty() {
                    anyhow::bail!("Thiếu answers");
                }
                crate::grammar::submit_test(db, &topic_id, &answers)
            }
            "kaen_lesson_list" => Ok(json!(db.list_lessons()?)),
            "kaen_lesson_show" => ops::lesson_json(db, &s(args, "lesson_id"), true),
            "kaen_lesson_create" => {
                let title = s(args, "title");
                if title.is_empty() {
                    anyhow::bail!("Thiếu title");
                }
                let desc = s(args, "description");
                Ok(json!(db.create_lesson(&title, (!desc.is_empty()).then_some(desc.as_str()))?))
            }
            "kaen_import_text" => {
                let title = s(args, "title");
                let raw = s(args, "raw_text");
                if title.is_empty() || raw.is_empty() {
                    anyhow::bail!("Thiếu title hoặc raw_text");
                }
                let sep = s(args, "separator");
                ops::import_lesson(db, &title, &raw, if sep.is_empty() { "|" } else { &sep })
            }
            "kaen_card_add" => {
                let lesson_id = s(args, "lesson_id");
                let word = s(args, "word");
                if lesson_id.is_empty() || word.is_empty() {
                    anyhow::bail!("Thiếu lesson_id hoặc word");
                }
                if db.get_lesson(&lesson_id)?.is_none() {
                    anyhow::bail!("Không tìm thấy bài học");
                }
                let meaning = s(args, "meaning_vi");
                let meanings = (!meaning.is_empty()).then(|| json!({ "vi": meaning }));
                let example = s(args, "example");
                let examples = (!example.is_empty()).then(|| json!([example]));
                let ipa = s(args, "ipa");
                let pos = s(args, "part_of_speech");
                Ok(json!(db.insert_card(
                    &lesson_id,
                    &word,
                    None,
                    (!ipa.is_empty()).then_some(ipa.as_str()),
                    (!pos.is_empty()).then_some(pos.as_str()),
                    examples.as_ref(),
                    &s(args, "explain"),
                    meanings.as_ref(),
                )?))
            }
            "kaen_study_session" => {
                let lesson_id = s(args, "lesson_id");
                if lesson_id.is_empty() {
                    ops::session(db)
                } else {
                    ops::lesson_session(db, &lesson_id)
                }
            }
            "kaen_review_submit" => {
                let card_id = s(args, "card_id");
                let result = s(args, "result");
                if card_id.is_empty() || result.is_empty() {
                    anyhow::bail!("Thiếu card_id hoặc result");
                }
                let mode = s(args, "mode");
                ops::submit_review(db, &card_id, &result, if mode.is_empty() { "FLIP" } else { &mode })
            }
            "kaen_due_count" => Ok(json!({ "dueNow": db.due_count(chrono::Utc::now())? })),
            "kaen_snooze" => ops::snooze(db, int(args, "hours", 1)),
            "kaen_stats" => {
                let settings = db.settings()?;
                let mut v = ops::stats_level(db)?;
                let today = ops::stats_today(db)?;
                let obj = v.as_object_mut().unwrap();
                obj.insert("today".into(), today);
                obj.insert("currentStreak".into(), json!(settings.current_streak));
                obj.insert("totalXP".into(), json!(settings.total_xp));
                Ok(v)
            }
            "kaen_story_list" => crate::story::list_stories(db),
            "kaen_story_show" => crate::story::get_story(db, &s(args, "story_id")),
            "kaen_dictation_list" => {
                let topic = s(args, "topic");
                crate::dictation::list_lessons(
                    db,
                    (!topic.is_empty()).then_some(topic.as_str()),
                    None,
                    int(args, "limit", 20),
                    int(args, "page", 1),
                )
            }
            "kaen_dictation_import" => {
                let payload: Value = serde_json::from_str(&s(args, "json"))
                    .map_err(|e| anyhow::anyhow!("JSON không hợp lệ: {e}"))?;
                crate::dictation::import_json(db, &payload)
            }
            "kaen_settings_get" => Ok(json!(db.settings()?)),
            "kaen_settings_set" => {
                let mut patch = serde_json::Map::new();
                if let Some(slots) = args.get("study_slots").filter(|v| v.is_array()) {
                    patch.insert("studySlots".into(), slots.clone());
                }
                if args["timezone"].is_string() {
                    patch.insert("timezone".into(), args["timezone"].clone());
                }
                if args["daily_word_goal"].is_i64() {
                    patch.insert("dailyWordGoal".into(), args["daily_word_goal"].clone());
                }
                if args["native_language"].is_string() {
                    patch.insert("nativeLanguage".into(), args["native_language"].clone());
                }
                ops::update_profile(db, &Value::Object(patch))
            }
            other => anyhow::bail!("Tool không tồn tại: {other}"),
        }
    })();
    match r {
        Ok(v) => json_result(v),
        Err(e) => error_result(e.to_string()),
    }
}
