//! AI qua bridge SenClaw (không bao giờ gọi thẳng provider). JSON số liệu từ
//! DB là ground truth duy nhất đưa cho model — prompt cấm bịa số, và mọi câu
//! trả lời đều kèm dòng "phân tích tham khảo".

use crate::calc::truncate_on_char_boundary;
use app_space_sdk::SpaceClient;
use serde_json::Value;

/// Prompt data cắt ở ~24 KB để không vượt cửa sổ khi quán có nhiều dữ liệu.
const MAX_CTX_BYTES: usize = 24_000;

/// Phân tích kinh doanh của quán. Trả `(markdown, model)`; lỗi bridge trả
/// chuỗi giải thích thay vì Err để UI/MCP hiển thị thẳng.
pub async fn analyze(sc: &SpaceClient, context: &Value, question: &str) -> (String, String) {
    let system = "Bạn là chuyên viên vận hành quán cafe / đồ uống. \
        Bạn nhận một JSON số liệu (doanh thu – giá vốn – lãi gộp hôm nay / 7 ngày / 14 ngày, \
        top món, doanh thu theo món 30 ngày, dự báo 7 ngày, tồn kho nguyên liệu, cảnh báo sắp hết \
        / kho âm / món chưa có công thức) và trả lời bằng tiếng Việt. \
        NGUYÊN TẮC: chỉ dựa trên số liệu được cung cấp, TUYỆT ĐỐI không bịa số; \
        kết luận trước, chi tiết sau; chỉ ra món lãi tốt / lãi kém (margin thấp), \
        nguyên liệu sắp hết cần nhập, bất thường doanh thu theo ngày; \
        đề xuất tối đa 3 hành động cụ thể (nhập gì, điều chỉnh giá món nào, đẩy bán món nào). \
        Kết thúc bằng đúng một dòng: \"Lưu ý: phân tích tham khảo dựa trên số liệu bạn ghi, \
        hãy đối chiếu thực tế trước khi quyết định.\"";
    let question = if question.trim().is_empty() {
        "Phân tích tình hình kinh doanh của quán, rủi ro và khuyến nghị."
    } else {
        question.trim()
    };
    let data = serde_json::to_string_pretty(context).unwrap_or_default();
    let prompt = format!(
        "Số liệu quán (JSON):\n{}\n\nCâu hỏi: {question}",
        truncate_on_char_boundary(&data, MAX_CTX_BYTES)
    );
    match sc.llm_request(system, &prompt, 2500).await {
        Ok((text, model)) => (text.trim().to_string(), model),
        Err(e) => (
            format!("Không gọi được AI qua bridge SenClaw: {e}"),
            String::new(),
        ),
    }
}

/// Gợi ý công thức món mới từ nguyên liệu sẵn có. Chỉ là gợi ý — người dùng
/// chốt rồi mới ghi vào thực đơn bằng tay/tool.
pub async fn menu_suggest(
    sc: &SpaceClient,
    idea: &str,
    context: &Value,
    target_margin_pct: Option<f64>,
) -> (String, String) {
    let system = "Bạn là barista trưởng kiêm quản lý chi phí của quán cafe / đồ uống Việt Nam. \
        Bạn nhận JSON gồm: danh sách nguyên liệu đang có (đơn vị gốc g/ml/cái, giá vốn bình quân \
        theo đơn vị gốc, tồn kho) và thực đơn hiện tại (giá bán, giá vốn). \
        Nhiệm vụ: đề xuất công thức đồ uống theo yêu cầu, ưu tiên dùng nguyên liệu ĐANG CÓ; \
        nguyên liệu phải mua thêm thì ghi rõ '(cần mua thêm)'. \
        Với mỗi món đề xuất: tên món, nhóm, định lượng từng nguyên liệu bằng g/ml/cái, \
        cách pha chế ngắn gọn từng bước, giá vốn ước tính (tính từ giá vốn trong JSON, ghi rõ phép \
        tính), và giá bán gợi ý đạt biên lãi mục tiêu. \
        Chỉ dựa trên giá vốn trong JSON — nguyên liệu chưa có giá thì nói rõ là ước lượng. \
        Kết thúc bằng đúng một dòng: \"Lưu ý: gợi ý tham khảo — hãy pha thử và cân chỉnh \
        định lượng thực tế trước khi đưa vào thực đơn.\"";
    let idea = if idea.trim().is_empty() {
        "Gợi ý 2-3 món đồ uống mới tận dụng nguyên liệu đang có sẵn trong kho."
    } else {
        idea.trim()
    };
    let margin = target_margin_pct.unwrap_or(70.0).clamp(0.0, 95.0);
    let data = serde_json::to_string_pretty(context).unwrap_or_default();
    let prompt = format!(
        "Dữ liệu quán (JSON):\n{}\n\nBiên lãi gộp mục tiêu: {margin}%\n\nYêu cầu: {idea}",
        truncate_on_char_boundary(&data, MAX_CTX_BYTES)
    );
    match sc.llm_request(system, &prompt, 2500).await {
        Ok((text, model)) => (text.trim().to_string(), model),
        Err(e) => (
            format!("Không gọi được AI qua bridge SenClaw: {e}"),
            String::new(),
        ),
    }
}
