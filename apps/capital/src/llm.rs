//! AI analysis through the SenClaw daemon bridge (never a direct provider
//! call). The dashboard JSON is the ONLY ground truth handed to the model —
//! the prompt forbids inventing numbers, and every answer carries a
//! "phân tích tham khảo" disclaimer.

use app_space_sdk::SpaceClient;
use serde_json::Value;

/// Ask the LLM for a goal plan as STRICT JSON steps. Returns the raw reply
/// text (parsed by [`crate::goals::parse_ai_plan`]) + model, or Err on bridge
/// failure so the caller falls back to the deterministic plan.
pub async fn plan_goal(
    sc: &SpaceClient,
    goal: &Value,
    insight: &Value,
) -> anyhow::Result<(String, String)> {
    let system = "Bạn là chuyên viên lập kế hoạch tài chính. Nhận một MỤC TIÊU vốn (kèm tiến độ, \
        số còn thiếu, thời hạn) và tình trạng sổ nguồn vốn. Trả về DUY NHẤT một mảng JSON các bước \
        hành động, không lời dẫn, không markdown: \
        [{\"title\":\"...\",\"due_date\":\"YYYY-MM-DD\",\"amount\":số}] . \
        Tối đa 8 bước, mỗi bước cụ thể và đo được, bám đúng số liệu được cung cấp (không bịa), \
        tổng các amount của các bước tài chính phải khớp số còn thiếu 'remaining', \
        due_date tăng dần và không vượt deadline. Bước không gắn số tiền thì amount=0 \
        (ví dụ: đàm phán lãi suất, rà soát chi phí).";
    let prompt = format!(
        "Mục tiêu (JSON, đã kèm tiến độ):\n{}\n\nTình trạng sổ (đánh giá rule engine):\n{}\n\nLập kế hoạch:",
        serde_json::to_string_pretty(goal).unwrap_or_default(),
        serde_json::to_string_pretty(insight).unwrap_or_default()
    );
    let (text, model) = sc.llm_request(system, &prompt, 1500).await?;
    Ok((text, model))
}

/// Analyze the capital structure. Returns `(analysis_markdown, model)`;
/// on bridge failure returns an explanatory error string instead.
///
/// `insight` is the rule engine's output ([`crate::insight::evaluate`]) — the
/// LLM is told to treat its score/findings as ground truth and narrate them,
/// never to re-derive or contradict the numbers.
pub async fn analyze(
    sc: &SpaceClient,
    dashboard: &Value,
    insight: &Value,
    question: &str,
) -> (String, String) {
    let system = "Bạn là chuyên viên phân tích nguồn vốn cho một doanh nghiệp/cá nhân. \
        Bạn nhận: (1) JSON số liệu nguồn vốn, và (2) kết quả ĐÁNH GIÁ TỰ ĐỘNG từ rule engine \
        (điểm sức khoẻ 0–100 + các phát hiện kèm mức độ good/warn/crit). \
        NGUYÊN TẮC: coi số liệu và phát hiện được cung cấp là chân lý — TUYỆT ĐỐI không bịa số, \
        không tự tính lại, không mâu thuẫn với đánh giá tự động; kết luận trước, chi tiết sau; \
        diễn giải các phát hiện theo thứ tự nghiêm trọng (crit trước), giải thích VÌ SAO từng \
        phát hiện quan trọng với người dùng này; đề xuất tối đa 3 hành động cụ thể, bám vào \
        phát hiện. Kết thúc bằng đúng một dòng: \"Lưu ý: phân tích tham khảo dựa trên dữ liệu \
        bạn nhập, không phải tư vấn tài chính chuyên nghiệp.\"";
    let question = if question.trim().is_empty() {
        "Phân tích cơ cấu nguồn vốn hiện tại, rủi ro và khuyến nghị."
    } else {
        question.trim()
    };
    let prompt = format!(
        "Số liệu nguồn vốn (JSON):\n{}\n\nĐánh giá tự động (rule engine):\n{}\n\nCâu hỏi: {question}",
        serde_json::to_string_pretty(dashboard).unwrap_or_default(),
        serde_json::to_string_pretty(insight).unwrap_or_default()
    );
    match sc.llm_request(system, &prompt, 2500).await {
        Ok((text, model)) => (text.trim().to_string(), model),
        Err(e) => (
            format!("Không gọi được AI qua bridge SenClaw: {e}"),
            String::new(),
        ),
    }
}
