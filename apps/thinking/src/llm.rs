//! AI qua bridge SenClaw (không bao giờ gọi thẳng provider). Mọi hàm sinh dữ
//! liệu có cấu trúc (5W, 6 mũ, giải pháp, chấm điểm) yêu cầu model trả STRICT
//! JSON và kiểm tra `finish_reason` — trả lời bị cắt vì trần token là lỗi,
//! không phải dữ liệu (xem ghi chú của `llm_request_full`). Riêng phần tổng
//! hợp (mũ Xanh Dương) là văn tự do.

use anyhow::{anyhow, Result};
use app_space_sdk::SpaceClient;
use serde_json::{json, Value};

use crate::logic;

/// Cắt phần JSON object đầu tiên ra khỏi trả lời của model: bỏ code fence,
/// lấy từ `{` đầu tiên đến `}` cuối cùng. Trả None nếu không parse được.
pub fn extract_json(text: &str) -> Option<Value> {
    let t = text.trim();
    let t = t
        .strip_prefix("```json")
        .or_else(|| t.strip_prefix("```"))
        .unwrap_or(t);
    let t = t.strip_suffix("```").unwrap_or(t).trim();
    let start = t.find('{')?;
    let end = t.rfind('}')?;
    if end <= start {
        return None;
    }
    serde_json::from_str(&t[start..=end]).ok()
}

/// Gọi bridge, bắt buộc kết quả là JSON object. Lỗi khi bridge fail, khi bị
/// cắt vì trần token (finish == "length") hoặc khi không parse được JSON.
async fn ask_json(
    sc: &SpaceClient,
    system: &str,
    prompt: &str,
    max_tokens: u32,
) -> Result<(Value, String)> {
    let (text, model, finish) = sc
        .llm_request_full(system, prompt, max_tokens, None)
        .await?;
    if finish == "length" {
        return Err(anyhow!(
            "trả lời của AI bị cắt vì vượt trần {max_tokens} token — thử lại hoặc rút gọn dữ liệu vấn đề"
        ));
    }
    let v = extract_json(&text).ok_or_else(|| {
        anyhow!(
            "AI không trả về JSON hợp lệ (model {model}): {}",
            text.chars().take(300).collect::<String>()
        )
    })?;
    Ok((v, model))
}

/// Tóm tắt vấn đề thành khối văn bản đưa vào prompt (chỉ dữ liệu người dùng
/// đã nhập — model bị cấm bịa thêm dữ kiện).
fn problem_block(detail: &Value) -> String {
    let p = &detail["problem"];
    let mut s = format!(
        "Tiêu đề: {}\nMô tả: {}\nBối cảnh: {}\nMục tiêu: {}",
        p["title"].as_str().unwrap_or(""),
        p["description"].as_str().unwrap_or(""),
        p["context"].as_str().unwrap_or(""),
        p["goal"].as_str().unwrap_or("")
    );
    let mut ws = String::new();
    for w in logic::W_KEYS {
        let c = detail["five_w"][w]["content"]
            .as_str()
            .unwrap_or("")
            .trim()
            .to_string();
        if !c.is_empty() {
            ws.push_str(&format!("- {}: {}\n", logic::w_label(w), c));
        }
    }
    if !ws.is_empty() {
        s.push_str(&format!("\n\nPhân tích 5W hiện có:\n{ws}"));
    }
    let mut hs = String::new();
    for h in logic::HAT_KEYS {
        let c = detail["hats"][h]["content"]
            .as_str()
            .unwrap_or("")
            .trim()
            .to_string();
        if !c.is_empty() {
            hs.push_str(&format!("- {}: {}\n", logic::hat_label(h), c));
        }
    }
    if !hs.is_empty() {
        s.push_str(&format!("\nPhân tích 6 mũ hiện có:\n{hs}"));
    }
    s
}

/// AI soạn nháp 5W. Trả về map `who/what/when/where/why` → String.
pub async fn gen_5w(sc: &SpaceClient, detail: &Value) -> Result<(Value, String)> {
    let system = "Bạn là chuyên gia làm rõ vấn đề theo phương pháp 5W. \
        Nhận mô tả một vấn đề (tiếng Việt) và điền 5 mục: \
        who (AI liên quan / bị ảnh hưởng), what (bản chất vấn đề là gì), \
        when (xảy ra từ khi nào, tần suất, thời hạn), where (ở đâu / khâu nào / kênh nào), \
        why (nguyên nhân gốc — hỏi 'tại sao' tới cùng, nêu 1-3 nguyên nhân khả dĩ). \
        NGUYÊN TẮC: chỉ suy luận từ thông tin được cung cấp; chỗ nào thiếu dữ kiện thì viết \
        'Cần làm rõ: …' thay vì bịa. Mỗi mục 1-3 câu, tiếng Việt. \
        CHỈ trả về đúng một JSON object, không giải thích gì thêm: \
        {\"who\":\"...\",\"what\":\"...\",\"when\":\"...\",\"where\":\"...\",\"why\":\"...\"}";
    let prompt = format!("Vấn đề cần phân tích 5W:\n\n{}", problem_block(detail));
    let (v, model) = ask_json(sc, system, &prompt, 2000).await?;
    for w in logic::W_KEYS {
        if v.get(w).and_then(|x| x.as_str()).is_none() {
            return Err(anyhow!("JSON 5W thiếu khóa '{w}'"));
        }
    }
    Ok((v, model))
}

/// AI đội mũ tư duy. `only = Some(hat)` → chỉ sinh một mũ; None → cả sáu.
/// Trả về map hat → String (chỉ chứa các mũ được sinh).
pub async fn gen_hats(
    sc: &SpaceClient,
    detail: &Value,
    only: Option<&str>,
) -> Result<(Value, String)> {
    let hats: Vec<&str> = match only {
        Some(h) => vec![h],
        None => logic::HAT_KEYS.to_vec(),
    };
    let mut spec = String::new();
    for h in &hats {
        let desc = match *h {
            "white" => "white — Mũ Trắng: chỉ dữ kiện, số liệu khách quan ĐÃ có trong dữ liệu; thiếu gì thì liệt kê 'dữ kiện còn thiếu cần thu thập'",
            "red" => "red — Mũ Đỏ: cảm xúc, trực giác, linh cảm của những người liên quan; không cần biện minh",
            "black" => "black — Mũ Đen: rủi ro, điểm yếu, kịch bản xấu, lý do có thể thất bại; phản biện sắc nhưng có căn cứ",
            "yellow" => "yellow — Mũ Vàng: lợi ích, giá trị, cơ hội, kịch bản tốt và điều kiện để đạt được",
            "green" => "green — Mũ Xanh Lá: ý tưởng mới, hướng đi thay thế, cách phá khung; nêu 2-4 hướng cụ thể",
            "blue" => "blue — Mũ Xanh Dương: nhìn toàn cục quá trình tư duy, còn thiếu góc nhìn nào, bước tiếp theo nên làm gì",
            _ => return Err(anyhow!("mũ không hợp lệ: {h}")),
        };
        spec.push_str(&format!("- {desc}\n"));
    }
    let keys = hats
        .iter()
        .map(|h| format!("\"{h}\":\"...\""))
        .collect::<Vec<_>>()
        .join(",");
    let system = format!(
        "Bạn là người điều phối một phiên 6 Mũ Tư Duy (Edward de Bono) bằng tiếng Việt. \
        Với vấn đề được cung cấp, hãy viết nội dung cho từng mũ sau, mỗi mũ 2-5 câu hoặc gạch đầu dòng:\n{spec}\
        NGUYÊN TẮC: giữ đúng kỷ luật từng mũ (mũ Trắng không phán xét, mũ Đen không bàn lợi ích…); \
        chỉ dựa trên thông tin được cung cấp, thiếu dữ kiện thì nói rõ là thiếu. \
        CHỈ trả về đúng một JSON object, không giải thích gì thêm: {{{keys}}}"
    );
    let prompt = format!("Vấn đề:\n\n{}", problem_block(detail));
    let (v, model) = ask_json(sc, &system, &prompt, 3000).await?;
    for h in &hats {
        if v.get(*h).and_then(|x| x.as_str()).is_none() {
            return Err(anyhow!("JSON 6 mũ thiếu khóa '{h}'"));
        }
    }
    Ok((v, model))
}

/// AI đề xuất giải pháp (tư duy mũ Xanh Lá). Trả về danh sách (title, description).
pub async fn gen_solutions(
    sc: &SpaceClient,
    detail: &Value,
    count: usize,
) -> Result<(Vec<(String, String)>, String)> {
    let count = count.clamp(2, 6);
    let system = format!(
        "Bạn là chuyên gia đề xuất giải pháp (tư duy mũ Xanh Lá) bằng tiếng Việt. \
        Từ vấn đề + phân tích được cung cấp, đề xuất đúng {count} giải pháp KHÁC NHAU rõ rệt \
        (đừng đưa {count} biến thể của cùng một ý). Mỗi giải pháp: title ngắn gọn (≤ 12 từ) \
        và description 2-4 câu nêu cách làm cụ thể, nguồn lực cần và kết quả kỳ vọng. \
        Chỉ dựa trên thông tin được cung cấp. \
        CHỈ trả về đúng một JSON object: {{\"solutions\":[{{\"title\":\"...\",\"description\":\"...\"}}]}}"
    );
    let prompt = format!("Vấn đề:\n\n{}", problem_block(detail));
    let (v, model) = ask_json(sc, &system, &prompt, 2200).await?;
    let arr = v["solutions"]
        .as_array()
        .ok_or_else(|| anyhow!("JSON thiếu mảng 'solutions'"))?;
    let mut out = Vec::new();
    for s in arr {
        let title = s["title"].as_str().unwrap_or("").trim().to_string();
        if title.is_empty() {
            continue;
        }
        out.push((
            title,
            s["description"].as_str().unwrap_or("").trim().to_string(),
        ));
    }
    if out.is_empty() {
        return Err(anyhow!("AI không đề xuất được giải pháp nào"));
    }
    Ok((out, model))
}

/// AI chấm một giải pháp theo 4 tiêu chí 0–10 (điểm tổng hợp do code tính,
/// KHÔNG lấy từ model). Trả về (benefit, risk, feasibility, effort, verdict, detail).
pub async fn evaluate_solution(
    sc: &SpaceClient,
    detail: &Value,
    solution: &Value,
) -> Result<((f64, f64, f64, f64, String, String), String)> {
    let system = "Bạn là hội đồng đánh giá giải pháp theo phương pháp 6 Mũ Tư Duy, trả lời tiếng Việt. \
        Chấm giải pháp được chỉ định theo 4 tiêu chí, mỗi tiêu chí một số 0-10: \
        benefit (mũ Vàng — lợi ích/giá trị nếu thành công; 10 = lợi ích rất lớn), \
        risk (mũ Đen — rủi ro/khả năng thất bại; 10 = cực rủi ro), \
        feasibility (khả thi với nguồn lực trong bối cảnh; 10 = làm được ngay), \
        effort (công sức/chi phí/thời gian; 10 = cực tốn kém). \
        verdict: MỘT câu kết luận. detail: 3-6 gạch đầu dòng markdown, mỗi gạch mở đầu bằng \
        đúng một trong ⚪/🔴/⚫/🟡/🟢 nêu góc nhìn mũ đó về giải pháp này. \
        Chỉ dựa trên thông tin được cung cấp — không bịa số liệu. \
        CHỈ trả về đúng một JSON object: \
        {\"benefit\":0,\"risk\":0,\"feasibility\":0,\"effort\":0,\"verdict\":\"...\",\"detail\":\"...\"}";
    let prompt = format!(
        "Vấn đề:\n\n{}\n\nGiải pháp cần đánh giá:\nTiêu đề: {}\nMô tả: {}",
        problem_block(detail),
        solution["title"].as_str().unwrap_or(""),
        solution["description"].as_str().unwrap_or("")
    );
    let (v, model) = ask_json(sc, system, &prompt, 2200).await?;
    let num = |k: &str| -> Result<f64> {
        v.get(k)
            .and_then(|x| x.as_f64())
            .ok_or_else(|| anyhow!("JSON đánh giá thiếu số '{k}'"))
    };
    let out = (
        num("benefit")?,
        num("risk")?,
        num("feasibility")?,
        num("effort")?,
        v["verdict"].as_str().unwrap_or("").trim().to_string(),
        v["detail"].as_str().unwrap_or("").trim().to_string(),
    );
    Ok((out, model))
}

/// Mũ Xanh Dương tổng kết: nhận toàn bộ phân tích + bảng so sánh điểm và viết
/// khuyến nghị markdown tự do. Lỗi bridge trả về chuỗi giải thích (như các app
/// SenClaw khác) thay vì Err — đây là bước "mềm" cuối phiên.
pub async fn synthesize(
    sc: &SpaceClient,
    detail: &Value,
    compare: &Value,
    question: &str,
) -> (String, String) {
    let system = "Bạn đội Mũ Xanh Dương — người điều phối chốt một phiên phân tích \
        6 Mũ Tư Duy + 5W, trả lời tiếng Việt. Nhận JSON gồm vấn đề, 5W, nội dung 6 mũ, \
        các giải pháp và BẢNG ĐIỂM đã tính sẵn (benefit/risk/feasibility/effort, overall 0-100). \
        NGUYÊN TẮC: không bịa dữ kiện, không tự chấm lại điểm — điểm do hệ thống tính; \
        kết luận trước, chi tiết sau. Trình bày: (1) tóm tắt vấn đề 1-2 câu; \
        (2) khuyến nghị giải pháp nên chọn và vì sao (dựa trên điểm + góc nhìn các mũ, \
        nêu rõ nếu điểm sát nhau); (3) rủi ro chính cần canh (mũ Đen) và cách giảm nhẹ; \
        (4) 2-3 bước hành động tiếp theo. \
        Kết thúc bằng đúng một dòng: \"Lưu ý: phân tích tham khảo dựa trên dữ liệu bạn nhập — \
        quyết định cuối cùng là của bạn.\"";
    let question = if question.trim().is_empty() {
        "Tổng kết phiên phân tích và khuyến nghị nên chọn giải pháp nào."
    } else {
        question.trim()
    };
    let payload = json!({ "analysis": detail, "score_board": compare });
    let prompt = format!(
        "Toàn bộ phiên phân tích (JSON):\n{}\n\nYêu cầu: {question}",
        serde_json::to_string_pretty(&payload).unwrap_or_default()
    );
    match sc.llm_request(system, &prompt, 2800).await {
        Ok((text, model)) => (text.trim().to_string(), model),
        Err(e) => (
            format!("Không gọi được AI qua bridge SenClaw: {e}"),
            String::new(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_json_plain() {
        let v = extract_json(r#"{"who":"khách"}"#).unwrap();
        assert_eq!(v["who"], "khách");
    }

    #[test]
    fn extract_json_fenced_and_prose() {
        let v = extract_json("Đây là kết quả:\n```json\n{\"a\": 1}\n```\nHết.").unwrap();
        assert_eq!(v["a"], 1);
        let v = extract_json("Kết quả {\"b\": {\"c\": 2}} xong").unwrap();
        assert_eq!(v["b"]["c"], 2);
    }

    #[test]
    fn extract_json_rejects_garbage() {
        assert!(extract_json("không có json nào cả").is_none());
        assert!(extract_json("{cắt giữa chừng").is_none());
        assert!(extract_json("").is_none());
    }

    #[test]
    fn problem_block_includes_existing_analysis() {
        let detail = serde_json::json!({
            "problem": { "title": "T", "description": "D", "context": "C", "goal": "G" },
            "five_w": { "who": { "content": "khách quen" } },
            "hats": { "black": { "content": "rủi ro X" } },
        });
        let s = problem_block(&detail);
        assert!(s.contains("Tiêu đề: T"));
        assert!(s.contains("khách quen"));
        assert!(s.contains("rủi ro X"));
        // Vấn đề trống 5W/mũ thì không chèn section rỗng.
        let bare = serde_json::json!({
            "problem": { "title": "T", "description": "", "context": "", "goal": "" },
            "five_w": {}, "hats": {},
        });
        let s = problem_block(&bare);
        assert!(!s.contains("Phân tích 5W hiện có"));
        assert!(!s.contains("Phân tích 6 mũ hiện có"));
    }
}
