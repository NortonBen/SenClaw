//! AI analysis through the SenClaw daemon bridge (never a direct provider
//! call). The dashboard JSON is the ONLY ground truth handed to the model —
//! the prompt forbids inventing numbers, and every answer carries a
//! "phân tích tham khảo" disclaimer.

use app_space_sdk::SpaceClient;
use serde_json::Value;

/// Analyze the inventory. Returns `(analysis_markdown, model)`;
/// on bridge failure returns an explanatory error string instead.
pub async fn analyze(sc: &SpaceClient, dashboard: &Value, question: &str) -> (String, String) {
    let system = "Bạn là chuyên viên quản lý kho / phân tích tồn kho. \
        Bạn nhận một JSON số liệu (sản phẩm, tồn kho từng kho, giá trị tồn, hàng dưới tồn tối thiểu, \
        nhập-xuất 30 ngày và 12 tháng) và trả lời bằng tiếng Việt. \
        NGUYÊN TẮC: chỉ dựa trên số liệu được cung cấp, TUYỆT ĐỐI không bịa số; \
        kết luận trước, chi tiết sau; chỉ ra hàng sắp hết (dưới tồn tối thiểu), hàng tồn đọng \
        (giá trị lớn nhưng ít xuất), lệch nhập-xuất theo tháng; \
        đề xuất tối đa 3 hành động cụ thể (nhập thêm gì, xả hàng gì, kiểm kê kho nào). \
        Kết thúc bằng đúng một dòng: \"Lưu ý: phân tích tham khảo dựa trên dữ liệu bạn nhập, \
        hãy kiểm kê thực tế trước khi quyết định.\"";
    let question = if question.trim().is_empty() {
        "Phân tích tình hình tồn kho hiện tại, rủi ro và khuyến nghị."
    } else {
        question.trim()
    };
    let prompt = format!(
        "Số liệu kho (JSON):\n{}\n\nCâu hỏi: {question}",
        serde_json::to_string_pretty(dashboard).unwrap_or_default()
    );
    match sc.llm_request(system, &prompt, 2500).await {
        Ok((text, model)) => (text.trim().to_string(), model),
        Err(e) => (
            format!("Không gọi được AI qua bridge SenClaw: {e}"),
            String::new(),
        ),
    }
}

/// Analyze the product portfolio (sản phẩm tiềm năng / bán chậm / tồn đọng).
/// Input is the deterministic performance JSON from
/// `Db::product_performance` — the model narrates and prioritizes, the
/// classification itself is already computed by code.
pub async fn analyze_products(
    sc: &SpaceClient,
    performance: &Value,
    question: &str,
) -> (String, String) {
    let system = "Bạn là chuyên viên phân tích danh mục sản phẩm cho một cửa hàng/kho. \
        Bạn nhận một JSON hiệu suất sản phẩm đã phân loại sẵn theo quy tắc: \
        'potential' (tiềm năng — đang bán tốt, tồn chỉ đủ ≤45 ngày, nên nhập thêm), \
        'steady' (ổn định), 'slow' (bán chậm — tồn đủ bán >180 ngày), \
        'dead' (tồn đọng — có tồn mà không bán được đơn nào trong cửa sổ), 'idle' (chưa kinh doanh). \
        Kèm số liệu: sold_qty/sold_value (đã bán), velocity_30d (tốc độ bán/30 ngày), \
        days_of_stock (tồn đủ bán bao nhiêu ngày), margin_pct (biên lãi trên giá vốn), \
        sell_through_pct, last_sale_date, dead_stock_value. Trả lời bằng tiếng Việt. \
        NGUYÊN TẮC: chỉ dựa trên số liệu được cung cấp, TUYỆT ĐỐI không bịa số, không đổi phân loại; \
        kết luận trước, chi tiết sau. Nêu rõ: (1) sản phẩm TIỀM NĂNG nhất — kèm lý do bằng số \
        (tốc độ bán, biên lãi, ngày tồn còn lại) và đề xuất số lượng nhập thêm ước tính; \
        (2) sản phẩm KHÔNG BÁN ĐƯỢC / tồn đọng — giá trị vốn bị chôn, lần bán cuối, đề xuất xử lý \
        (giảm giá xả, gộp combo, ngừng nhập); (3) hàng bán chậm cần theo dõi. \
        Tối đa 3 hành động ưu tiên. \
        Kết thúc bằng đúng một dòng: \"Lưu ý: phân tích tham khảo dựa trên dữ liệu bạn nhập, \
        hãy đối chiếu thực tế trước khi quyết định nhập/xả hàng.\"";
    let question = if question.trim().is_empty() {
        "Đánh giá danh mục: sản phẩm nào tiềm năng nên nhập thêm, sản phẩm nào không bán được cần xử lý?"
    } else {
        question.trim()
    };
    let prompt = format!(
        "Hiệu suất sản phẩm (JSON):\n{}\n\nCâu hỏi: {question}",
        serde_json::to_string_pretty(performance).unwrap_or_default()
    );
    match sc.llm_request(system, &prompt, 2500).await {
        Ok((text, model)) => (text.trim().to_string(), model),
        Err(e) => (
            format!("Không gọi được AI qua bridge SenClaw: {e}"),
            String::new(),
        ),
    }
}
