//! Thin LLM helper: compose a customer-service reply through the SenClaw daemon
//! bridge (never a direct provider call). Used to draft replies to a buyer's
//! message; the draft still goes through the human-approval queue before it is
//! ever sent to the customer.

use app_space_sdk::SpaceClient;

/// Compose a Vietnamese CSKH reply to a customer message. Returns
/// `(reply_text, model)`. Best-effort: on any bridge error the caller falls
/// back to an empty draft the human fills in.
pub async fn compose_reply(
    sc: &SpaceClient,
    shop_name: &str,
    customer_msg: &str,
    context: &str,
) -> (String, String) {
    let system = format!(
        "Bạn là nhân viên CSKH của shop \"{shop_name}\" trên Shopee. Trả lời khách \
         ngắn gọn, lịch sự, đúng thông tin. Chỉ dựa trên dữ liệu shop được cung cấp; \
         KHÔNG bịa giá, tồn kho, hay chính sách. Nếu không chắc, hẹn kiểm tra lại. \
         Xưng \"em\", gọi khách \"anh/chị\"."
    );
    let prompt = format!(
        "Tin nhắn của khách:\n{customer_msg}\n\nBối cảnh (đơn/sản phẩm/chính sách shop):\n{context}\n\n\
         Soạn 1 câu trả lời gửi khách:"
    );
    match sc.llm_request(&system, &prompt, 400).await {
        Ok((text, model)) => (text.trim().to_string(), model),
        Err(_) => (String::new(), String::new()),
    }
}
