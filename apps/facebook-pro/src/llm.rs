//! Thin LLM helpers via the SenClaw daemon bridge (never a direct provider call).
//! Used to (a) compose a reply to a Facebook comment and (b) analyze a post's
//! content + engagement. Composed replies still pass through the human-approval
//! queue before anything is published.

use app_space_sdk::SpaceClient;

/// Compose a short, polite Vietnamese reply to a Facebook comment. Returns
/// `(reply_text, model)`. Best-effort: on any bridge error returns an empty
/// string so the caller falls back to a draft the human fills in.
pub async fn compose_reply(
    sc: &SpaceClient,
    page_name: &str,
    comment: &str,
    hint: &str,
) -> (String, String) {
    let system = format!(
        "Bạn là quản trị viên Fanpage \"{page_name}\" trên Facebook. Trả lời bình luận của \
         người dùng: ngắn gọn, lịch sự, thân thiện, đúng trọng tâm. Chỉ dựa trên thông tin \
         được cung cấp; KHÔNG bịa giá, khuyến mãi, hay chính sách. Nếu không chắc, mời khách \
         nhắn tin trang để được hỗ trợ. Trả về đúng nội dung trả lời, không thêm chú thích."
    );
    let prompt = format!(
        "Bình luận của người dùng:\n{comment}\n\nGợi ý/định hướng trả lời (nếu có):\n{hint}\n\n\
         Soạn 1 câu trả lời gửi lại bình luận:"
    );
    match sc.llm_request(&system, &prompt, 300).await {
        Ok((text, model)) => (text.trim().to_string(), model),
        Err(_) => (String::new(), String::new()),
    }
}

/// Analyze a post: content quality + engagement read + concrete suggestions.
/// Returns `(analysis_text, model)`. `engagement` is a compact summary string.
pub async fn analyze_post(sc: &SpaceClient, message: &str, engagement: &str) -> (String, String) {
    let system = "Bạn là chuyên gia content & growth cho Fanpage Facebook. Phân tích một bài viết \
        và đưa nhận xét NGẮN GỌN, có cấu trúc: (1) điểm mạnh, (2) điểm yếu, (3) gợi ý cải thiện \
        cụ thể (tiêu đề/CTA/hình ảnh/thời điểm đăng), (4) đánh giá mức tương tác. Dựa trên dữ liệu \
        thật được cung cấp, không bịa số.";
    let prompt =
        format!("Nội dung bài viết:\n{message}\n\nSố liệu tương tác:\n{engagement}\n\nPhân tích:");
    match sc.llm_request(system, &prompt, 700).await {
        Ok((text, model)) => (text.trim().to_string(), model),
        Err(e) => (format!("(không phân tích được: {e})"), String::new()),
    }
}

/// Verdict on ad performance from real Ads Insights rows. Returns
/// `(verdict_text, model)`. `summary` is a compact per-campaign/ad metric dump
/// (spend, CTR, CPC, CPM, results, ROAS). The model judges effectiveness and,
/// crucially, whether each is "đốt tiền" (burning money) and should be paused.
pub async fn analyze_ads(sc: &SpaceClient, currency: &str, summary: &str) -> (String, String) {
    let system = "Bạn là chuyên gia Facebook Ads. Dựa TRÊN SỐ LIỆU THẬT được cung cấp (không bịa \
        thêm số), đánh giá hiệu quả quảng cáo. Với MỖI dòng (chiến dịch/nhóm/quảng cáo) hãy nêu: \
        (1) đọc nhanh chỉ số CTR/CPC/CPM/chi tiêu/kết quả/ROAS; (2) kết luận NGẮN — một trong \
        [HIỆU QUẢ ✅ | THEO DÕI ⚠️ | ĐỐT TIỀN ❌]; (3) nếu ĐỐT TIỀN thì nói rõ NÊN TẮT và vì sao \
        (vd CTR quá thấp, CPC/CPM cao bất thường, chi nhiều nhưng 0 kết quả). Cuối cùng đưa 1–2 \
        khuyến nghị hành động. Diễn giải chuẩn: CTR cao = nội dung hấp dẫn; CPC/CPM thấp = rẻ; \
        spend cao mà results≈0 hoặc ROAS<1 = đang lỗ.";
    let prompt = format!(
        "Đơn vị tiền: {currency}\n\nSố liệu Ads Insights:\n{summary}\n\nĐánh giá & khuyến nghị:"
    );
    match sc.llm_request(system, &prompt, 900).await {
        Ok((text, model)) => (text.trim().to_string(), model),
        Err(e) => (format!("(không phân tích được: {e})"), String::new()),
    }
}
