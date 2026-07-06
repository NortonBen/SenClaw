//! Built-in mind-map templates (MindMeister-style starter maps). Each template
//! carries a layout style, a root label, and a styled node tree that is inserted
//! under the new map's root.

use crate::db::GenNode;
use serde::Serialize;

/// A template shown in the gallery.
pub struct Template {
    pub id: &'static str,
    pub name: &'static str,
    pub icon: &'static str,
    pub category: &'static str,
    pub description: &'static str,
    pub layout: &'static str,
    pub root: &'static str,
    /// Builds the children of the root node.
    pub build: fn() -> Vec<GenNode>,
}

/// A template's metadata (no tree) for the gallery listing.
#[derive(Serialize)]
pub struct TemplateInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub icon: &'static str,
    pub category: &'static str,
    pub description: &'static str,
    pub layout: &'static str,
}

pub fn list() -> Vec<TemplateInfo> {
    all()
        .into_iter()
        .map(|t| TemplateInfo {
            id: t.id,
            name: t.name,
            icon: t.icon,
            category: t.category,
            description: t.description,
            layout: t.layout,
        })
        .collect()
}

pub fn find(id: &str) -> Option<Template> {
    all().into_iter().find(|t| t.id == id)
}

// ---- node builders ----

fn node(text: &str, children: Vec<GenNode>) -> GenNode {
    GenNode {
        text: text.to_string(),
        note: String::new(),
        color: None,
        shape: None,
        fill: false,
        icon: None,
        children,
    }
}

fn leaf(text: &str) -> GenNode {
    node(text, vec![])
}

/// A vivid, filled top-level branch (brainstorm look): color + icon + rounded fill.
fn branch(icon: &str, text: &str, color: &str, children: Vec<GenNode>) -> GenNode {
    GenNode {
        text: text.to_string(),
        note: String::new(),
        color: Some(color.to_string()),
        shape: Some("rounded".to_string()),
        fill: true,
        icon: Some(icon.to_string()),
        children,
    }
}

fn all() -> Vec<Template> {
    vec![
        Template {
            id: "study-exam",
            name: "Ôn thi Sinh học",
            icon: "🎓",
            category: "Học tập",
            description: "Sơ đồ ôn tập theo chủ đề, gom kiến thức thành các nhánh dễ nhớ.",
            layout: "mindmap",
            root: "Ôn thi Sinh học",
            build: || {
                vec![
                    node("Sinh học tế bào", vec![leaf("Bào quan"), leaf("Phân bào")]),
                    node("Tiến hoá", vec![leaf("Chọn lọc tự nhiên"), leaf("Hình thành loài")]),
                    node(
                        "Di truyền",
                        vec![leaf("Cấu trúc DNA"), leaf("Bảng Punnett"), leaf("Đột biến")],
                    ),
                    node(
                        "Sinh thái",
                        vec![leaf("Chuỗi thức ăn"), leaf("Quần xã"), leaf("Chu trình vật chất")],
                    ),
                    node(
                        "Cơ thể người",
                        vec![leaf("Tuần hoàn"), leaf("Thần kinh"), leaf("Tiêu hoá")],
                    ),
                ]
            },
        },
        Template {
            id: "brainstorm-campaign",
            name: "Chiến dịch mạng xã hội",
            icon: "💡",
            category: "Động não",
            description: "Bảng động não nhiều màu cho ý tưởng chiến dịch marketing.",
            layout: "mindmap",
            root: "Ý tưởng chiến dịch",
            build: || {
                vec![
                    branch(
                        "🎯",
                        "Mục tiêu chiến dịch",
                        "#f59e0b",
                        vec![leaf("Nhận diện thương hiệu"), leaf("Tỷ lệ tương tác")],
                    ),
                    branch(
                        "👥",
                        "Đối tượng mục tiêu",
                        "#3b82f6",
                        vec![leaf("Chính: Gen Z"), leaf("Phụ: Millennials")],
                    ),
                    branch(
                        "📱",
                        "Nền tảng chính",
                        "#f97316",
                        vec![leaf("Instagram Reels"), leaf("TikTok"), leaf("LinkedIn")],
                    ),
                    branch(
                        "🎨",
                        "Chủ đề nội dung",
                        "#a855f7",
                        vec![leaf("Hậu trường"), leaf("Nội dung từ người dùng")],
                    ),
                    branch(
                        "📊",
                        "Ngân sách & Nguồn lực",
                        "#ef4444",
                        vec![leaf("Hợp tác đối tác"), leaf("Phân bổ quảng cáo")],
                    ),
                    branch(
                        "🚀",
                        "Tiến độ & Đo lường",
                        "#ec4899",
                        vec![leaf("Ngày ra mắt"), leaf("KPI hàng tuần")],
                    ),
                ]
            },
        },
        Template {
            id: "study-macro",
            name: "Kinh tế vĩ mô",
            icon: "📈",
            category: "Học tập",
            description: "Sơ đồ tổ chức (org chart) trình bày các khái niệm chính.",
            layout: "org",
            root: "Kinh tế vĩ mô",
            build: || {
                vec![
                    node("GDP", vec![leaf("Định nghĩa"), leaf("Thực vs danh nghĩa")]),
                    node("Thất nghiệp", vec![leaf("Các loại"), leaf("Tỷ lệ tự nhiên")]),
                    node("Chính sách tiền tệ", vec![leaf("Lãi suất"), leaf("Ngân hàng TW")]),
                    node("Lạm phát", vec![leaf("Nguyên nhân"), leaf("Đo lường (CPI)")]),
                    node("Chính sách tài khoá", vec![leaf("Chi tiêu CP"), leaf("Thuế")]),
                ]
            },
        },
        Template {
            id: "meeting-sync",
            name: "Họp nhóm hàng tuần",
            icon: "🗓️",
            category: "Cuộc họp",
            description: "Bố cục danh sách (outline) cho lịch họp và mục hành động.",
            layout: "outline",
            root: "Lịch họp nhóm tuần",
            build: || {
                vec![
                    node(
                        "Thành quả",
                        vec![leaf("Tính năng đã ship"), leaf("Phản hồi khách hàng")],
                    ),
                    node("Vướng mắc", vec![leaf("Rủi ro"), leaf("Phụ thuộc")]),
                    node("Ưu tiên", vec![leaf("Mục tiêu sprint"), leaf("Deadline")]),
                    node("Chỉ số", vec![leaf("Traffic"), leaf("Chuyển đổi"), leaf("Ticket hỗ trợ")]),
                    node(
                        "Mục hành động",
                        vec![leaf("Workshop chiến dịch"), leaf("Newsletter"), leaf("Onboarding")],
                    ),
                ]
            },
        },
        Template {
            id: "swot",
            name: "Phân tích SWOT",
            icon: "🧭",
            category: "Kinh doanh",
            description: "Bốn nhánh Điểm mạnh / Điểm yếu / Cơ hội / Thách thức.",
            layout: "mindmap",
            root: "Phân tích SWOT",
            build: || {
                vec![
                    branch(
                        "💪",
                        "Điểm mạnh",
                        "#10b981",
                        vec![leaf("Lợi thế cạnh tranh"), leaf("Nguồn lực nội bộ")],
                    ),
                    branch(
                        "⚠️",
                        "Điểm yếu",
                        "#f59e0b",
                        vec![leaf("Hạn chế nguồn lực"), leaf("Khoảng trống năng lực")],
                    ),
                    branch(
                        "🌱",
                        "Cơ hội",
                        "#3b82f6",
                        vec![leaf("Xu hướng thị trường"), leaf("Phân khúc mới")],
                    ),
                    branch(
                        "🔥",
                        "Thách thức",
                        "#ef4444",
                        vec![leaf("Đối thủ"), leaf("Rủi ro vĩ mô")],
                    ),
                ]
            },
        },
        Template {
            id: "project-plan",
            name: "Kế hoạch dự án",
            icon: "📋",
            category: "Kinh doanh",
            description: "Phạm vi, mốc thời gian, nhóm và rủi ro cho một dự án.",
            layout: "mindmap",
            root: "Kế hoạch dự án",
            build: || {
                vec![
                    node("Mục tiêu", vec![leaf("Phạm vi"), leaf("Tiêu chí thành công")]),
                    node("Mốc thời gian", vec![leaf("Giai đoạn 1"), leaf("Giai đoạn 2"), leaf("Ra mắt")]),
                    node("Nhóm & Vai trò", vec![leaf("Chủ dự án"), leaf("Thực thi")]),
                    node("Nguồn lực", vec![leaf("Ngân sách"), leaf("Công cụ")]),
                    node("Rủi ro", vec![leaf("Kỹ thuật"), leaf("Tiến độ")]),
                ]
            },
        },
        Template {
            id: "empathy-map",
            name: "Bản đồ thấu cảm",
            icon: "❤️",
            category: "Kinh doanh",
            description: "Hiểu khách hàng: Nghĩ / Thấy / Nghe / Nói & Làm.",
            layout: "mindmap",
            root: "Bản đồ thấu cảm khách hàng",
            build: || {
                vec![
                    branch("🧠", "Nghĩ & Cảm nhận", "#a855f7", vec![leaf("Lo lắng"), leaf("Mong muốn")]),
                    branch("👀", "Nhìn thấy", "#3b82f6", vec![leaf("Môi trường"), leaf("Đối thủ")]),
                    branch("👂", "Nghe thấy", "#10b981", vec![leaf("Bạn bè"), leaf("Truyền thông")]),
                    branch("💬", "Nói & Làm", "#f97316", vec![leaf("Hành vi"), leaf("Thái độ")]),
                    branch("😖", "Nỗi đau", "#ef4444", vec![leaf("Trở ngại"), leaf("Rủi ro")]),
                    branch("🎁", "Lợi ích", "#14b8a6", vec![leaf("Kỳ vọng"), leaf("Thước đo thành công")]),
                ]
            },
        },
    ]
}
