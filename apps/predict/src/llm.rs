//! LLM narration via the SenClaw daemon bridge. The models NEVER invent
//! numbers: every prompt hands over the computed statistics and instructs the
//! model to only narrate them. All calls are best-effort — on bridge failure the
//! caller still returns the raw numbers. Domain disclaimers are appended in
//! code (never left to the prompt).

use app_space_sdk::SpaceClient;
use serde_json::{json, Value};

use crate::{lottery, market};

/// Keep outputs comfortably under the bridge output ceiling; a "length" finish
/// on the bridge means the reply was cut — treat as failure and fall back.
async fn ask(sc: &SpaceClient, system: &str, prompt: &str, max_tokens: u32) -> Option<String> {
    match sc.llm_request_full(system, prompt, max_tokens, None).await {
        Ok((text, _model, finish)) if finish != "length" && !text.trim().is_empty() => {
            Some(text.trim().to_string())
        }
        _ => None,
    }
}

/// "Siêu máy tính" style match preview from the computed model output.
pub async fn football_article(sc: &SpaceClient, pred: &Value) -> Option<String> {
    let system = "Bạn là chuyên mục 'Siêu máy tính dự đoán' bóng đá tiếng Việt. Viết bài nhận định \
        NGẮN (≤180 từ) từ số liệu model được cung cấp. TUYỆT ĐỐI không bịa hay sửa xác suất/tỷ số — \
        chỉ diễn giải đúng các con số đã cho, văn phong thể thao hấp dẫn, kết bài nêu tỷ số khả dĩ nhất.";
    let prompt = format!(
        "Số liệu model (Elo + Poisson):\n{}\n\nViết bài nhận định:",
        serde_json::to_string_pretty(pred).ok()?
    );
    ask(sc, system, &prompt, 2400).await
}

/// Entertainment lottery note. Disclaimer appended HERE, not by the model.
pub async fn lottery_note(sc: &SpaceClient, stats_summary: &Value) -> String {
    let system = "Bạn là người dẫn chuyên mục thống kê xổ số vui tiếng Việt. Từ số liệu thống kê \
        THẬT được cung cấp (tần suất, lô gan), viết 2-3 câu bình luận GIẢI TRÍ. Không hứa hẹn trúng, \
        không khẳng định số nào 'chắc chắn về'. Chỉ dựa trên số liệu đã cho.";
    let prompt = format!(
        "Thống kê:\n{}\n\nBình luận vui:",
        serde_json::to_string(stats_summary).unwrap_or_default()
    );
    let note = ask(sc, system, &prompt, 1500).await.unwrap_or_default();
    if note.is_empty() {
        lottery::DISCLAIMER.to_string()
    } else {
        format!("{note}\n\n{}", lottery::DISCLAIMER)
    }
}

/// Gold/FX trend commentary. Investment disclaimer appended in code.
pub async fn market_note(sc: &SpaceClient, snapshot: &Value) -> String {
    let system = "Bạn là biên tập viên bản tin giá vàng/tỷ giá tiếng Việt. Từ số liệu THẬT được \
        cung cấp (giá hiện tại, SMA, momentum, nhãn xu hướng), viết 2-3 câu tóm tắt xu hướng. \
        Không đưa lời khuyên mua/bán, không dự đoán giá chính xác, không bịa số.";
    let prompt = format!(
        "Số liệu:\n{}\n\nTóm tắt xu hướng:",
        serde_json::to_string(snapshot).unwrap_or_default()
    );
    let note = ask(sc, system, &prompt, 1500).await.unwrap_or_default();
    if note.is_empty() {
        market::DISCLAIMER.to_string()
    } else {
        format!("{note}\n\n{}", market::DISCLAIMER)
    }
}

/// Weather advice ("mang ô, phơi đồ…") from a compact daily forecast.
pub async fn weather_advice(sc: &SpaceClient, city: &str, daily: &Value) -> Option<String> {
    let system = "Bạn là bản tin thời tiết thân thiện tiếng Việt. Từ dự báo THẬT được cung cấp, \
        viết 2-3 câu lời khuyên thiết thực (mang áo mưa, chống nắng, phơi đồ, đi lại). \
        Không bịa số liệu ngoài dữ liệu đã cho.";
    let prompt = format!(
        "Thành phố: {city}\nDự báo:\n{}\n\nLời khuyên:",
        serde_json::to_string(daily).unwrap_or_default()
    );
    ask(sc, system, &prompt, 1500).await
}

// ---- generic topics ("form chung") ----

/// AI evaluation of a topic's accumulated data: patterns, quality, gaps.
pub async fn topic_analyze(sc: &SpaceClient, topic: &Value, sample: &Value) -> Option<String> {
    let system = "Bạn là nhà phân tích dữ liệu tiếng Việt. Từ mô tả chủ đề + các bản ghi THẬT được \
        cung cấp, viết phân tích NGẮN có cấu trúc: (1) bức tranh chung, (2) xu hướng/mẫu hình nhận \
        thấy, (3) chất lượng dữ liệu (thiếu gì, nhiễu gì), (4) nên thu thập thêm gì. \
        Chủ đề có thể kèm `static` (bối cảnh cố định: vị trí, thông số), `guide` (tài liệu hướng dẫn \
        phân tích của người dùng) và `documents` (thông tin NGOÀI SỐ LIỆU: ghi chú, tin tức, giải thích — \
        có thể gắn `date`/`ref` với một ngày hoặc một giá trị cụ thể). Dùng bối cảnh, TUÂN THỦ hướng dẫn, \
        và ĐỐI CHIẾU tài liệu với số liệu để giải thích các bất thường. \
        Chỉ dựa trên dữ liệu đã cho, không bịa số.";
    let prompt = format!(
        "Chủ đề:\n{}\n\nDữ liệu (mới nhất trước):\n{}\n\nPhân tích:",
        serde_json::to_string(topic).unwrap_or_default(),
        serde_json::to_string(sample).unwrap_or_default()
    );
    ask(sc, system, &prompt, 2600).await
}

/// Derive "siêu dự đoán" rules from history. Returns parsed
/// `[{rule, confidence}]` (empty when the bridge fails or returns junk).
pub async fn topic_derive_rules(
    sc: &SpaceClient,
    topic: &Value,
    sample: &Value,
) -> Vec<(String, f64)> {
    let system = "Bạn là công cụ rút QUY LUẬT dự đoán từ dữ liệu lịch sử. Chủ đề có thể kèm `static` \
        (bối cảnh cố định), `guide` (hướng dẫn phân tích) và `documents` (thông tin ngoài số liệu gắn theo \
        ngày/giá trị) — dùng chúng làm định hướng và làm bằng chứng giải thích quy luật. \
        Từ chủ đề + bản ghi THẬT \
        được cung cấp, rút ra tối đa 6 quy luật NGẮN, mỗi quy luật kèm độ tin cậy 0..1 do bạn ước \
        lượng từ mức lặp lại trong dữ liệu (ít dữ liệu → tin cậy thấp). CHỈ trả về JSON: \
        [{\"rule\": \"...\", \"confidence\": 0.6}, ...] — không thêm chữ nào khác.";
    let prompt = format!(
        "Chủ đề:\n{}\n\nDữ liệu (mới nhất trước):\n{}\n\nJSON quy luật:",
        serde_json::to_string(topic).unwrap_or_default(),
        serde_json::to_string(sample).unwrap_or_default()
    );
    let Some(text) = ask(sc, system, &prompt, 2400).await else {
        return vec![];
    };
    let Some(v) = crate::topic::extract_json(&text) else {
        return vec![];
    };
    v.as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|r| {
                    let rule = r["rule"].as_str()?.trim().to_string();
                    if rule.is_empty() {
                        return None;
                    }
                    Some((
                        rule,
                        r["confidence"].as_f64().unwrap_or(0.5).clamp(0.0, 1.0),
                    ))
                })
                .take(6)
                .collect()
        })
        .unwrap_or_default()
}

/// Forecast "will X happen?" for a topic (or free-form when topic is null).
/// Returns parsed `{p, reasoning}`.
pub async fn topic_forecast(
    sc: &SpaceClient,
    topic: Option<&Value>,
    rules: &Value,
    relevant: &Value,
    question: &str,
) -> Option<(f64, String)> {
    let system = "Bạn là siêu dự đoán viên (superforecaster) tiếng Việt. Ước lượng xác suất một sự \
        kiện xảy ra, theo phương pháp: xuất phát từ base rate trong dữ liệu lịch sử được cung cấp, \
        điều chỉnh theo quy luật đã rút ra và bằng chứng cụ thể; thiếu dữ liệu thì nói rõ và giữ \
        xác suất gần 0.5, KHÔNG tự tin quá mức; không bao giờ trả 0 hay 1 tuyệt đối. \
        CHỈ trả về JSON: {\"p\": 0.xx, \"reasoning\": \"3-5 câu vì sao\"} — không thêm chữ nào khác.";
    let prompt = format!(
        "Chủ đề:\n{}\n\nQuy luật đã rút:\n{}\n\nDữ liệu liên quan (mới nhất trước):\n{}\n\nCâu hỏi: {}\n\nJSON:",
        topic.map(|t| serde_json::to_string(t).unwrap_or_default()).unwrap_or_else(|| "(không có — dự đoán tự do)".into()),
        serde_json::to_string(rules).unwrap_or_default(),
        serde_json::to_string(relevant).unwrap_or_default(),
        question
    );
    let text = ask(sc, system, &prompt, 2400).await?;
    let v = crate::topic::extract_json(&text)?;
    let p = v["p"].as_f64()?.clamp(0.01, 0.99);
    let reasoning = v["reasoning"].as_str().unwrap_or("").to_string();
    Some((p, reasoning))
}

/// AI thiết kế chủ đề từ mô tả TỰ DO của người dùng ("tôi muốn theo dõi…").
/// Returns normalized `{name, description, fields, sample_questions}`.
pub async fn design_topic(sc: &SpaceClient, wish: &str) -> Option<Value> {
    let system = "Bạn là công cụ thiết kế CHỦ ĐỀ dự đoán. Từ mô tả mong muốn tự do của người dùng, \
        hãy tách rõ hai loại thông tin:\n\
        • TĨNH (`static`): thứ KHÔNG đổi theo thời gian — vị trí/thành phố, đối tượng theo dõi, \
          đơn vị đo, thông số cố định. Đây là bối cảnh, không phải dữ liệu nhập hằng ngày.\n\
        • ĐỘNG (`fields`): thứ thay đổi theo từng lần ghi — ngày, giờ, nhiệt độ, gió, giá, số lượng… \
          Đây chính là dữ liệu đầu vào để dự đoán.\n\
        Ngoài ra viết `guide`: tài liệu hướng dẫn ngắn cách PHÂN TÍCH & DỰ ĐOÁN chủ đề này (yếu tố nào \
        quan trọng, quan hệ nhân quả cần chú ý, cạm bẫy) — sẽ được dùng làm prompt cho AI mỗi lần dự đoán.\n\
        CHỈ trả về JSON:\n\
        {\"name\": \"tên ngắn\", \"description\": \"1-2 câu: theo dõi gì, dự đoán gì\",\n\
         \"static\": {\"tên thông số cố định\": \"giá trị (để trống nếu chưa biết)\"},\n\
         \"fields\": [{\"name\": \"tên trường tiếng Việt ngắn\", \"kind\": \"text|number|date|bool\"}],\n\
         \"guide\": \"3-6 câu hướng dẫn phân tích/dự đoán\",\n\
         \"sample_questions\": [\"2-3 câu hỏi dự đoán mẫu\"]}\n\
        Quy tắc: 3-6 trường động; dữ liệu theo thời gian thì trường đầu là ngày (kind=date); \
        số đo dùng kind=number; sự kiện có/không dùng kind=bool. Không thêm chữ nào ngoài JSON.";
    let prompt = format!("Mong muốn của người dùng:\n{wish}\n\nJSON:");
    let text = ask(sc, system, &prompt, 2600).await?;
    let v = crate::topic::extract_json(&text)?;
    let name = v["name"].as_str()?.trim().to_string();
    if name.is_empty() {
        return None;
    }
    let fields = crate::topic::parse_fields(&v["fields"]);
    if fields.is_empty() {
        return None;
    }
    let questions: Vec<String> = v["sample_questions"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .take(3)
                .collect()
        })
        .unwrap_or_default();
    // Giữ cả khoá tĩnh chưa có giá trị để người dùng điền nốt trong form.
    let static_keys: Vec<Value> = v["static"]
        .as_object()
        .map(|m| {
            m.iter()
                .filter(|(k, _)| !k.trim().is_empty())
                .map(|(k, val)| json!({ "name": k.trim(), "value": val.as_str().unwrap_or("").trim() }))
                .collect()
        })
        .unwrap_or_default();
    Some(serde_json::json!({
        "name": name,
        "description": v["description"].as_str().unwrap_or("").trim(),
        "static": static_keys,
        "fields": crate::topic::fields_json(&fields),
        "guide": v["guide"].as_str().unwrap_or("").trim(),
        "sample_questions": questions,
    }))
}

// ---- superforecasting pipeline (Tetlock) ----

/// Bước 1 — Fermi decompose: câu hỏi → câu hỏi con + truy vấn tìm tin.
/// Returns `(sub_questions, search_queries)`; empty on failure (caller falls
/// back to searching the raw question).
pub async fn sf_decompose(
    sc: &SpaceClient,
    topic: Option<&Value>,
    question: &str,
) -> (Vec<String>, Vec<String>) {
    let system = "Bạn là siêu dự báo viên. PHÂN RÃ (Fermi) một câu hỏi dự đoán: những điều kiện nào phải \
        đúng để sự kiện xảy ra, và cần tìm thông tin gì. CHỈ trả về JSON: \
        {\"sub_questions\": [\"...\"], \"search_queries\": [\"truy vấn tìm tin tức ngắn\"]} — tối đa 4 câu hỏi \
        con, tối đa 3 truy vấn (tiếng Việt, dạng từ khoá tìm kiếm), không thêm chữ nào khác.";
    let prompt = format!(
        "Chủ đề: {}\nCâu hỏi: {}\n\nJSON:",
        topic
            .map(|t| serde_json::to_string(t).unwrap_or_default())
            .unwrap_or_else(|| "(tự do)".into()),
        question
    );
    let Some(text) = ask(sc, system, &prompt, 1800).await else {
        return (vec![], vec![]);
    };
    let Some(v) = crate::topic::extract_json(&text) else {
        return (vec![], vec![]);
    };
    let list = |key: &str, cap: usize| -> Vec<String> {
        v[key]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .take(cap)
                    .collect()
            })
            .unwrap_or_default()
    };
    (list("sub_questions", 4), list("search_queries", 3))
}

/// Bước 3 — Tổng hợp theo checklist Siêu Dự Báo. Returns the normalized trace
/// (see `evidence::normalize_trace`) or None on bridge/parse failure.
pub async fn sf_synthesize(sc: &SpaceClient, dossier: &Value, checklist: &str) -> Option<Value> {
    let system = format!(
        "{}\n\nHỒ SƠ có thể kèm `guide` — tài liệu hướng dẫn phân tích do người dùng viết cho chủ đề này: \
         TUÂN THỦ hướng dẫn đó; dùng `static_context` (vị trí, thông số cố định) làm bối cảnh; và coi \
         `documents` (thông tin ngoài số liệu do người dùng lưu, có thể gắn ngày/giá trị) là BẰNG CHỨNG \
         ngang hàng với số liệu khi lập luận.\n\n\
         Bạn nhận một HỒ SƠ gồm: chủ đề, câu hỏi, phân rã, thống kê dữ liệu lịch sử, quy luật đã rút, \
         bài học & track record cũ, và bằng chứng ngoài (news/search). Tổng hợp theo ĐÚNG checklist trên. \
         CHỈ trả về JSON:\n\
         {{\"outside_view\": {{\"base_rate\": 0.xx, \"rationale\": \"...\"}},\n\
          \"evidence_for\": [\"...\"], \"evidence_against\": [\"...\"],\n\
          \"adjustments\": [{{\"reason\": \"...\", \"delta\": 0.05}}],\n\
          \"premortem\": \"nếu sai thì vì...\",\n\
          \"p\": 0.xx, \"confidence\": \"thấp|vừa|cao\",\n\
          \"granularity_note\": \"vì sao chọn đúng con số này\",\n\
          \"update_triggers\": [\"tin gì → sửa p hướng nào\"]}}\n\
         — không thêm chữ nào khác.",
        checklist
    );
    let prompt = format!(
        "HỒ SƠ:\n{}\n\nJSON:",
        serde_json::to_string_pretty(dossier).ok()?
    );
    let text = ask(sc, &system, &prompt, 6000).await?;
    let v = crate::topic::extract_json(&text)?;
    crate::evidence::normalize_trace(&v)
}

/// Postmortem sau khi một dự đoán được chấm: rút MỘT bài học quy trình.
pub async fn sf_lesson(sc: &SpaceClient, prediction: &Value) -> Option<String> {
    let system = "Bạn là siêu dự báo viên đang mổ xẻ một dự đoán ĐÃ có kết quả (điều răn 8 — postmortem, \
        cảnh giác hindsight bias). Từ dự đoán + trace + kết quả thật, rút ra MỘT bài học QUY TRÌNH ngắn \
        (≤2 câu) dùng được cho các dự đoán sau của chủ đề này: sai/đúng ở khâu nào (base rate? bằng chứng? \
        cập nhật? premortem?). Đúng nhờ may mắn thì nói thẳng. Trả về đúng nội dung bài học, không thêm gì.";
    let prompt = format!(
        "Dự đoán đã chấm:\n{}\n\nBài học:",
        serde_json::to_string_pretty(prediction).ok()?
    );
    ask(sc, system, &prompt, 1500).await
}

/// Morning brief tying every domain together (weather + gold + football + lottery).
pub async fn morning_brief(sc: &SpaceClient, data: &Value) -> Option<String> {
    let system =
        "Bạn là bản tin buổi sáng 'Siêu Dự Đoán' tiếng Việt. Từ dữ liệu THẬT được cung cấp \
        (thời tiết, giá vàng, trận bóng hôm nay kèm xác suất, kết quả xổ số hôm qua), viết bản tin \
        NGẮN (≤200 từ) có gạch đầu dòng theo mục. Không bịa số. Giữ nguyên mọi con số được cho.";
    let prompt = format!(
        "Dữ liệu sáng nay:\n{}\n\nViết bản tin:",
        serde_json::to_string_pretty(data).ok()?
    );
    ask(sc, system, &prompt, 2400).await
}
