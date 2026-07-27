//! The style, model, character and background pickers.
//!
//! These live on the backend rather than in the React bundle so the MCP tools
//! can offer an agent exactly the same menu the UI shows a human.

use serde_json::{json, Value};

pub const STYLES: &[&str] = &[
    "Phân tích theo video gốc (Original Style)",
    "Synthwave, neon sunset, 80s retro",
    "Dark fantasy, dramatic lighting",
    "Kawaii chibi cute style",
    "Hyper-realistic portrait",
    "Sci-fi futuristic spaceship",
    "Disney classic 2D animation",
];

pub fn models() -> Value {
    json!([
        { "id": "gemini-3-flash-preview", "name": "Gemini 3 Flash (Nhanh nhất)" },
        { "id": "gemini-3-pro-preview",   "name": "Gemini 3 Pro (Chính xác cao)" }
    ])
}

pub fn character_presets() -> Value {
    json!([
        { "name": "Chiến binh Cyberpunk", "desc": "Một chiến binh tương lai với giáp neon đen, mắt điện tử rực sáng, vẻ mặt lạnh lùng." },
        { "name": "Nữ sinh Anime", "desc": "Nữ sinh trung học Nhật Bản, tóc dài cột hai bên, đồng phục thủy thủ truyền thống." },
        { "name": "Samurai cổ đại", "desc": "Võ sĩ đạo thời Edo, áo giáp kình ngư, kiếm katana sắc lẹm, búi tóc búi cao." },
        { "name": "Phi hành gia", "desc": "Phi hành gia trong bộ đồ bảo hộ trắng hiện đại, mũ bảo hiểm phản chiếu ánh sao." },
        { "name": "Công chúa Disney", "desc": "Nàng công chúa phong cách cổ điển, váy lộng lẫy, vương miện nhỏ, đôi mắt to tròn." }
    ])
}

pub fn background_presets() -> Value {
    json!([
        { "name": "Cơ sở Sao Hỏa", "desc": "Một trạm nghiên cứu công nghệ cao trên Sao Hỏa, đất đỏ bụi bặm bên ngoài cửa kính, máy móc hiện đại rực rỡ bên trong." },
        { "name": "Rừng Pha Lê", "desc": "Khu rừng rậm rạp với các cây cổ thụ bằng thủy tinh phát sáng, thảm cỏ màu tím tâm linh, tiên nữ bay lượn." },
        { "name": "Phố Cổ Hội An 2077", "desc": "Kiến trúc cổ kính của Hội An nhưng được nâng cấp với bảng hiệu neon, xe bay lướt qua các mái ngói rêu phong." },
        { "name": "Đền Thờ Trên Mây", "desc": "Một ngôi đền cổ lơ lửng giữa những tầng mây vàng rực, thác nước chảy ngược lên trời, không gian thanh tịnh." },
        { "name": "Phòng Thí Nghiệm Robot", "desc": "Nơi sản xuất robot với những cánh tay máy đang hoạt động, tia lửa điện, sàn kim loại sáng bóng." }
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_style_is_the_keep_original_one() {
        assert!(crate::prompts::is_original_style(STYLES[0]));
    }

    #[test]
    fn presets_expose_name_and_desc_for_every_entry() {
        for list in [character_presets(), background_presets()] {
            for item in list.as_array().unwrap() {
                assert!(item["name"].as_str().is_some_and(|s| !s.is_empty()));
                assert!(item["desc"].as_str().is_some_and(|s| !s.is_empty()));
            }
        }
    }
}
