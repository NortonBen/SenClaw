//! Optional LLM bridge — an AI "luận giải" (interpretation) of a day's almanac
//! and whether it suits a given activity. Every call goes through the SenClaw
//! Space-App open API (the app never contacts a provider directly). Core
//! almanac results are 100% deterministic; this only adds an advisory narrative.

use app_space_sdk::SpaceClient;

const ADVISE_SYSTEM: &str = "Bạn là một chuyên gia lịch vạn niên và phong thủy người Việt. \
Dựa trên dữ liệu ngày âm lịch được cung cấp (can chi, hoàng đạo/hắc đạo, giờ tốt, trực, \
sao, hướng xuất hành, ngày kỵ), hãy luận giải NGẮN GỌN xem ngày đó có phù hợp với công việc \
người dùng hỏi hay không. Đưa ra: (1) kết luận nên/không nên, (2) lý do chính, (3) khung giờ \
Hoàng Đạo & hướng tốt nếu nên làm. Trả lời bằng tiếng Việt, tối đa 5-6 câu, giọng điềm đạm, \
không mê tín cực đoan. Đây là tham khảo văn hóa truyền thống, không phải lời khuyên tuyệt đối.";

/// Ask the daemon's active LLM to interpret a day for an activity. `facts` is a
/// compact rendering of the DayInfo; `activity` is the việc the user is asking about.
pub async fn advise(facts: &str, activity: &str) -> Result<(String, String), String> {
    let prompt = format!(
        "Dữ liệu ngày:\n{facts}\n\nViệc cần xem: {activity}\n\nHãy luận giải ngắn gọn."
    );
    client().llm_request(ADVISE_SYSTEM, &prompt, 700).await.map_err(|e| e.to_string())
}

fn client() -> SpaceClient {
    if std::env::var("SENCLAW_SPACE_APP_ID").is_err() {
        std::env::set_var("SENCLAW_SPACE_APP_ID", "luna-calendar");
    }
    SpaceClient::from_env()
}
