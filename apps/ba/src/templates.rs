//! Registry template tài liệu BA — "hợp đồng đầu ra" giữa app và AI.
//!
//! Mỗi entry tái hiện một skill của quy trình BA-Kit (docs/ba-app-design.md §2-3):
//! sections bắt buộc, cột bảng, quy tắc ID truy vết, câu hỏi phỏng vấn khi đầu
//! vào mỏng, tài liệu upstream cần đọc trước khi sinh, và trần token. Đây là
//! source of truth — UI, engine, trace, dashboard đều đọc từ đây.

pub const PHASES: &[(u8, &str)] = &[
    (1, "Lập kế hoạch sản phẩm"),
    (2, "Thu thập & đặc tả"),
    (3, "Sơ đồ nghiệp vụ"),
    (4, "Use case & user story"),
    (5, "Thiết kế màn hình"),
    (6, "Tích hợp API"),
    (7, "Kiểm thử"),
    (8, "Kiểm soát chất lượng"),
    (9, "Bàn giao & vận hành"),
];

/// 8 chặng pipeline mỗi tính năng — đúng 8 cột dashboard của BA-Kit
/// (URD → BRD → PRD → SRS → UseCase → Story → AC → Test).
pub const PIPELINE: [&str; 8] = [
    "urd", "brd", "prd_epic", "srs", "usecase", "userstory", "ac", "test_cases",
];

pub const DOC_STATUSES: [&str; 5] = ["draft", "in_review", "revisions", "approved", "shipped"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scope {
    Project,
    Feature,
}

pub struct DocTemplate {
    /// Khoá loại tài liệu (snake_case). Cặp (doc_type, subtype) xác định một
    /// tài liệu "sống" duy nhất trong mỗi feature/project.
    pub doc_type: &'static str,
    pub subtype: &'static str,
    pub phase: u8,
    /// Tên lệnh kiểu BA-Kit, hiển thị ở catalog UI.
    pub skill: &'static str,
    pub title: &'static str,
    pub desc: &'static str,
    pub scope: Scope,
    /// doc_type upstream — vừa là ngữ cảnh nhét vào prompt, vừa là cạnh đồ thị
    /// staleness (doc cũ hơn upstream ⇒ stale).
    pub upstream: &'static [&'static str],
    /// Heading bắt buộc trong output (soft-check sau khi sinh).
    pub sections: &'static [&'static str],
    /// Ruột skill: chỉ dẫn chi tiết cấu trúc + quy tắc cho AI.
    pub prompt: &'static str,
    /// Câu hỏi phỏng vấn làm rõ khi đầu vào mỏng (interview mode).
    pub interview: &'static [&'static str],
    pub max_tokens: u32,
    /// "markdown" | "html" (wireframe_html/prototype_html render iframe).
    pub format: &'static str,
}

/// System prompt chung — persona BA + quy ước ID + kỷ luật chống bịa.
pub const SYSTEM_BA: &str = "Bạn là Business Analyst kỳ cựu (15 năm, chuẩn IIBA/BABOK), viết tài liệu tiếng Việt, giữ nguyên thuật ngữ ngành (FR, NFR, backlog, stakeholder...). Quy tắc bất di bất dịch:\n\
1) TRẢ VỀ DUY NHẤT nội dung tài liệu markdown, bắt đầu bằng dòng `# <tiêu đề>`. Không lời dẫn, không giải thích ngoài tài liệu, không bọc ```markdown.\n\
2) Theo ĐÚNG khung section được yêu cầu — đủ heading, đúng cột bảng, đúng thứ tự.\n\
3) ID truy vết đúng định dạng: FR-<feature>-001, NFR-<feature>-001, BR-<feature>-001, E-<feature>-001, SC-<feature>-01, US-<feature>-001, AC-<feature>-001, TC-<feature>-001, UC-<feature>-001, UR-<feature>-001 (số thứ tự 3 chữ số, feature là slug được cung cấp). Mục nào tham chiếu mục khác phải ghi đúng ID — truy vết là thứ được chấm điểm tự động.\n\
4) KHÔNG BỊA điều chưa có căn cứ. Con số, chính sách, ngưỡng... chưa được cho thì đưa vào bảng `## Open Questions` (OQ-1, OQ-2... với cột: OQ | Câu hỏi | Trạng thái | Chốt) và ghi giả định tạm rõ ràng. Đã đủ căn cứ thì bảng Open Questions ghi 'Không còn câu hỏi mở'.\n\
5) Mô tả yêu cầu theo mẫu điều kiện: 'Khi/Nếu <điều kiện>, hệ thống phải <hành vi>'.\n\
6) Sơ đồ dùng code fence ```mermaid đúng cú pháp Mermaid 11; DBML/PlantUML/BPMN-XML dùng fence riêng đúng ngôn ngữ.\n\
7) Văn phong đặc tả: câu ngắn, đo được, không marketing.";

pub const TEMPLATES: &[DocTemplate] = &[
    // ============ Giai đoạn 1 — Lập kế hoạch sản phẩm ============
    DocTemplate {
        doc_type: "prd",
        subtype: "",
        phase: 1,
        skill: "/prd",
        title: "PRD toàn sản phẩm",
        desc: "Tầm nhìn, người dùng, bóc tách danh sách tính năng",
        scope: Scope::Project,
        upstream: &["discover", "brainstorm"],
        sections: &[
            "## 1. Tầm nhìn & mục tiêu",
            "## 2. Người dùng mục tiêu & persona",
            "## 3. Bài toán cần giải",
            "## 4. Phạm vi",
            "## 5. Danh sách tính năng",
            "## 6. Chỉ số thành công",
            "## 7. Rủi ro & giả định",
            "## Open Questions",
        ],
        prompt: "Viết PRD cấp toàn sản phẩm.\n\
- Mục 1: tầm nhìn 2-3 câu + 3-5 mục tiêu SMART.\n\
- Mục 2: bảng persona `| ID (PER-<project>-001...) | Persona | Mô tả | Nhu cầu chính |`.\n\
- Mục 3: bài toán/JTBD từng persona, hiện trạng và chi phí của vấn đề.\n\
- Mục 4: hai danh sách rõ ràng `### Trong phạm vi` và `### Ngoài phạm vi (KHÔNG làm)` — ngoài phạm vi phải ghi lý do.\n\
- Mục 5 (QUAN TRỌNG NHẤT — app sẽ bóc bảng này thành danh sách tính năng): bảng `| Slug | Tên tính năng | Mô tả 1-2 câu | Ưu tiên (P0/P1/P2) |` — slug kebab-case tiếng Anh ngắn (vd `authentication`, `payment`). Bóc 6-15 tính năng phủ trọn tầm nhìn, P0 là lõi MVP.\n\
- Mục 6: bảng chỉ số `| Chỉ số | Cách đo | Mốc đạt |` gắn với mục tiêu mục 1.\n\
- Mục 7: bảng `| Rủi ro/Giả định | Loại | Ảnh hưởng | Ứng phó |`.",
        interview: &[
            "Sản phẩm giải quyết vấn đề gì, cho ai? Mô tả 2-3 câu.",
            "Người dùng mục tiêu gồm những nhóm nào (vai trò, bối cảnh sử dụng)?",
            "Mô hình kinh doanh/nguồn thu (nếu có)? Sản phẩm nội bộ hay thương mại?",
            "Nền tảng đích: web/mobile/desktop? Khu vực thị trường?",
            "Có sản phẩm cạnh tranh hoặc hệ thống hiện tại nào đang dùng không?",
            "Mốc thời gian mong muốn cho MVP?",
        ],
        max_tokens: 12000,
        format: "markdown",
    },
    DocTemplate {
        doc_type: "roadmap",
        subtype: "",
        phase: 1,
        skill: "/roadmap",
        title: "Roadmap Now / Next / Later",
        desc: "Xếp ưu tiên tính năng, chia đợt phát hành",
        scope: Scope::Project,
        upstream: &["prd"],
        sections: &[
            "## 1. Nguyên tắc ưu tiên",
            "## 2. Now",
            "## 3. Next",
            "## 4. Later",
            "## 5. Phụ thuộc & mốc",
            "## Open Questions",
        ],
        prompt: "Xếp roadmap từ danh sách tính năng của PRD (đọc ở ngữ cảnh).\n\
- Mục 1: nêu tiêu chí xếp (impact/effort/risk/phụ thuộc) và thang chấm.\n\
- Mục 2-4: mỗi đợt một bảng `| Tính năng | Lý do xếp đợt này | Impact (C/V/T) | Effort (C/V/T) | Ghi chú |`. Now = MVP phải có; Next = ngay sau khi MVP chạy; Later = chưa cam kết.\n\
- Mục 5: bảng phụ thuộc giữa tính năng (A cần B xong trước) + mốc release dự kiến (tương đối: tháng thứ N, không bịa ngày).",
        interview: &[
            "Nguồn lực hiện có (số dev, thời gian) để cân effort?",
            "Ràng buộc thời gian cứng nào (sự kiện ra mắt, hợp đồng)?",
            "Tiêu chí ưu tiên nào quan trọng nhất với bạn: tốc độ ra thị trường, doanh thu, hay giảm rủi ro?",
        ],
        max_tokens: 8000,
        format: "markdown",
    },
    DocTemplate {
        doc_type: "discover",
        subtype: "",
        phase: 1,
        skill: "/discover",
        title: "Điều tra ý tưởng (Discovery)",
        desc: "Nhu cầu, đối thủ — nên làm hay bỏ",
        scope: Scope::Project,
        upstream: &[],
        sections: &[
            "## 1. Ý tưởng & bối cảnh",
            "## 2. Nhu cầu thị trường",
            "## 3. Phân tích đối thủ",
            "## 4. Phân khúc người dùng",
            "## 5. Đánh giá khả thi",
            "## 6. Khuyến nghị Go / No-Go",
            "## Open Questions",
        ],
        prompt: "Điều tra một ý tưởng còn phân vân, kết luận nên làm hay bỏ.\n\
- Mục 2: nhu cầu là thật hay giả định? Bằng chứng nào có/cần thu thập; cách kiểm chứng rẻ nhất.\n\
- Mục 3: bảng `| Đối thủ/Giải pháp thay thế | Điểm mạnh | Điểm yếu | Khoảng trống ta khai thác |` — kể cả giải pháp thủ công hiện tại.\n\
- Mục 4: phân khúc + phân khúc nào đau nhất, trả tiền dễ nhất.\n\
- Mục 5: khả thi kỹ thuật / vận hành / pháp lý, mỗi mảng 1 đoạn ngắn + mức rủi ro.\n\
- Mục 6: kết luận rõ ràng GO (với điều kiện gì) hoặc NO-GO (vì sao) + 3 bước kiểm chứng tiếp theo. Không ba phải.",
        interview: &[
            "Ý tưởng là gì, xuất phát từ quan sát/nỗi đau nào?",
            "Bạn đã nói chuyện với người dùng tiềm năng nào chưa? Họ nói gì?",
            "Bạn biết những giải pháp/đối thủ nào đang tồn tại?",
            "Ràng buộc của bạn: vốn, thời gian, đội ngũ?",
        ],
        max_tokens: 8000,
        format: "markdown",
    },
    // ============ Giai đoạn 2 — Thu thập & đặc tả ============
    DocTemplate {
        doc_type: "brainstorm",
        subtype: "",
        phase: 2,
        skill: "/brainstorm",
        title: "Brainstorm & làm rõ nghiệp vụ",
        desc: "Ghi ý tưởng thô rồi phỏng vấn làm rõ trước khi viết tài liệu",
        scope: Scope::Feature,
        upstream: &["prd", "discover"],
        sections: &[
            "## 1. Ý tưởng thô",
            "## 2. Mục tiêu & phạm vi đã chốt",
            "## 3. Người dùng & vai trò",
            "## 4. Bóc tách khả năng (P0/P1/P2)",
            "## 5. Luồng chính",
            "## 6. Quyết định & ngoại lệ",
            "## 7. Ràng buộc & chính sách",
            "## Open Questions",
        ],
        prompt: "Đây là tài liệu GỐC của tính năng — mọi tài liệu sau (URD/SRS/story...) đều trích nguồn về các mục ở đây, nên đánh số mục rõ ràng.\n\
- Mục 1: giữ nguyên ý tưởng thô người dùng đưa (không viết lại ý), sắp thành gạch đầu dòng.\n\
- Mục 2: phạm vi cover / KHÔNG cover, mỗi dòng một quyết định.\n\
- Mục 4: bảng `| Khả năng | Ưu tiên | Ghi chú |` — P0 lõi, P1 nên có, P2 sau.\n\
- Mục 5: từng luồng đánh số 5.1, 5.2... mô tả bước-qua-bước bằng lời.\n\
- Mục 6: bảng quyết định `| D# | Tình huống | Quyết định | Lý do |` (D1, D2...) — các nhánh rẽ, ngoại lệ, xử lý lỗi.\n\
- Mục 7: chính sách/ngưỡng/giới hạn đã chốt (cooldown, hạn mức, thời hạn...).\n\
- Open Questions: MỌI điều chưa chốt vào đây (OQ-1...), trạng thái `open`. Khi người dùng trả lời (phần ĐÁP PHỎNG VẤN trong đầu vào), chuyển thành quyết định ở mục 6/7 và đánh dấu OQ đó `resolved`.",
        interview: &[
            "Tính năng này giải quyết việc gì, cho vai trò người dùng nào?",
            "Luồng chính diễn ra thế nào từ đầu đến cuối (kể như kể chuyện)?",
            "Những tình huống lỗi/ngoại lệ nào bạn đã nghĩ tới, xử lý ra sao?",
            "Chính sách/ngưỡng nào cần chốt (giới hạn, thời hạn, quyền hạn)?",
            "Dữ liệu nào cần lưu? Có tích hợp hệ thống ngoài nào không?",
            "Điều gì KHÔNG nằm trong phạm vi tính năng này?",
        ],
        max_tokens: 10000,
        format: "markdown",
    },
    DocTemplate {
        doc_type: "urd",
        subtype: "",
        phase: 2,
        skill: "/urd",
        title: "URD — Tài liệu yêu cầu người dùng",
        desc: "Persona, nhu cầu, hành trình người dùng",
        scope: Scope::Feature,
        upstream: &["brainstorm", "prd"],
        sections: &[
            "## 1. Persona",
            "## 2. Nhu cầu người dùng",
            "## 3. Hành trình người dùng",
            "## 4. Yêu cầu từ góc nhìn người dùng",
            "## 5. Tiêu chí hài lòng",
            "## Open Questions",
        ],
        prompt: "Viết URD (User Requirements Document) — nhìn từ NGƯỜI DÙNG, không phải hệ thống.\n\
- Mục 1: bảng `| ID (PER-<feature>-001...) | Persona | Bối cảnh | Mục tiêu | Nỗi đau hiện tại |`.\n\
- Mục 2: bảng `| ID (UR-<feature>-001...) | Nhu cầu (dạng 'Tôi cần... để...') | Persona | Độ ưu tiên | Nguồn (mục brainstorm) |`.\n\
- Mục 3: hành trình từng persona chính: bảng `| Bước | Hành động | Suy nghĩ/cảm xúc | Cơ hội cải thiện |` + một sơ đồ mermaid `journey` cho persona quan trọng nhất.\n\
- Mục 4: yêu cầu người dùng phát biểu bằng ngôn ngữ đời thường (không kỹ thuật), mỗi cái trỏ UR-xx.\n\
- Mục 5: người dùng coi là 'xong tốt' khi nào — đo được.",
        interview: &[
            "Ai sẽ dùng tính năng này (vai trò, độ thành thạo công nghệ)?",
            "Họ đang làm việc này bằng cách nào, khổ chỗ nào?",
            "Một ngày điển hình họ chạm vào tính năng này lúc nào, bối cảnh gì?",
        ],
        max_tokens: 10000,
        format: "markdown",
    },
    DocTemplate {
        doc_type: "brd",
        subtype: "",
        phase: 2,
        skill: "/brd",
        title: "BRD — Tài liệu yêu cầu nghiệp vụ",
        desc: "Mục tiêu, phạm vi, rủi ro, ROI",
        scope: Scope::Feature,
        upstream: &["brainstorm", "prd", "urd"],
        sections: &[
            "## 1. Bối cảnh kinh doanh",
            "## 2. Mục tiêu nghiệp vụ",
            "## 3. Phạm vi",
            "## 4. Stakeholder",
            "## 5. Yêu cầu nghiệp vụ",
            "## 6. Quy trình AS-IS / TO-BE",
            "## 7. Rủi ro",
            "## 8. Chi phí – lợi ích (ROI)",
            "## Open Questions",
        ],
        prompt: "Viết BRD (Business Requirements Document) — nhìn từ TỔ CHỨC/KINH DOANH.\n\
- Mục 2: bảng mục tiêu SMART `| Mục tiêu | Chỉ số | Hiện trạng | Mốc đạt | Hạn |`.\n\
- Mục 3: trong/ngoài phạm vi + lý do.\n\
- Mục 4: ma trận `| Stakeholder | Vai trò | Mối quan tâm | Ảnh hưởng (C/V/T) | Tham gia (RACI) |`.\n\
- Mục 5: bảng `| ID (BR-<feature>-001...) | Yêu cầu nghiệp vụ | Lý do | Ưu tiên | Nguồn |` — mức NGHIỆP VỤ (chính sách, quy trình), không phải chức năng hệ thống.\n\
- Mục 6: AS-IS (hiện tại) và TO-BE (tương lai) mỗi cái một đoạn + một sơ đồ mermaid flowchart ngắn.\n\
- Mục 7: bảng `| Rủi ro | Khả năng | Ảnh hưởng | Ứng phó |`.\n\
- Mục 8: chi phí (xây + vận hành) vs lợi ích (tiết kiệm/tăng thu) — chưa có số thì ghi công thức tính và OQ.",
        interview: &[
            "Kết quả kinh doanh mong đợi là gì (tăng thu, giảm chi, tuân thủ...)?",
            "Ai là người tài trợ/quyết định? Các phòng ban nào bị ảnh hưởng?",
            "Quy trình hiện tại (AS-IS) đang chạy thế nào, tốn kém ra sao?",
        ],
        max_tokens: 10000,
        format: "markdown",
    },
    DocTemplate {
        doc_type: "prd_epic",
        subtype: "",
        phase: 2,
        skill: "/prd-epic",
        title: "PRD tính năng (epic)",
        desc: "Capability P0/P1/P2, kế hoạch release",
        scope: Scope::Feature,
        upstream: &["brainstorm", "prd", "urd"],
        sections: &[
            "## 1. Tóm tắt tính năng",
            "## 2. Bài toán & giá trị",
            "## 3. Capability",
            "## 4. Luồng tổng quan",
            "## 5. Phụ thuộc",
            "## 6. Kế hoạch release",
            "## 7. Chỉ số thành công",
            "## Open Questions",
        ],
        prompt: "Đặc tả MỘT tính năng ở mức sản phẩm (epic-level PRD).\n\
- Mục 3 (lõi): bảng `| Capability | Mô tả | Ưu tiên (P0/P1/P2) | Persona hưởng lợi | Nguồn |` — P0 thiếu là tính năng vô nghĩa; P1 nên có khi ra mắt; P2 để sau.\n\
- Mục 4: mô tả luồng chính vắn tắt + một mermaid flowchart tổng quan (không chi tiết từng màn).\n\
- Mục 5: phụ thuộc tính năng khác / hệ thống ngoài / quyết định đang chờ.\n\
- Mục 6: release theo đợt: đợt 1 gồm capability nào, tiêu chí sẵn sàng.\n\
- Mục 7: bảng `| SC-<feature>-01... | Outcome | Đo bằng | Mốc đạt |`.",
        interview: &[
            "Trong tính năng này, điều gì là 'không có thì vô nghĩa' (P0)?",
            "Có thể chia đợt phát hành không, đợt đầu tối thiểu gồm gì?",
            "Tính năng phụ thuộc gì vào hệ thống/tính năng khác?",
        ],
        max_tokens: 10000,
        format: "markdown",
    },
    DocTemplate {
        doc_type: "srs",
        subtype: "",
        phase: 2,
        skill: "/srs",
        title: "SRS — Đặc tả yêu cầu phần mềm",
        desc: "FR / NFR / business rule / ma trận lỗi / success criteria",
        scope: Scope::Feature,
        upstream: &["brainstorm", "prd_epic", "urd", "brd"],
        sections: &[
            "## 1. Scope",
            "## 2. Actors & Stakeholders",
            "## 3. Functional Requirements",
            "## 4. Non-Functional Requirements",
            "## 5. Business Rules",
            "## 6. Error Matrix",
            "## 7. Success Criteria",
            "## 8. Data Entities",
            "## 9. Flows",
            "## 10. Screens",
            "## 11. Constraints, Dependencies & Assumptions",
            "## Open Questions",
        ],
        prompt: "Viết SRS 11 mục chuẩn — tài liệu quan trọng nhất, dev và QA làm việc trực tiếp trên nó. Truy vết là bắt buộc.\n\
- Mục 1 Scope: đoạn 'SRS này cover...' + danh sách 'KHÔNG cover' kèm lý do/nguồn quyết định.\n\
- Mục 2: bảng `| Actor | Loại (người/hệ thống ngoài) | Mục tiêu | Trong scope? |`.\n\
- Mục 3 FR: bảng `| ID | Title | Description | Priority | Verify by | Source |`; ID = FR-<feature>-001 tăng dần; Description dạng 'Khi/Nếu..., hệ thống phải...'; Priority P0/P1; Verify by = demo|test; Source trỏ mục brainstorm (vd 'Brainstorm Mục 4 P0'). Phủ ĐỦ mọi khả năng P0/P1 và mọi quyết định D# của brainstorm.\n\
- Mục 4 NFR: bảng `| ID (NFR-<feature>-001...) | Category (performance/availability/security/privacy/usability/compliance) | Requirement | Priority | Acceptance |` — Acceptance đo được (con số, ngưỡng).\n\
- Mục 5 BR: bảng `| ID (BR-<feature>-001...) | Rule | Trigger | Implements FR | Source |` — mỗi rule trỏ ít nhất 1 FR.\n\
- Mục 6 Error Matrix: bảng `| Error ID (E-<feature>-001...) | Title | Trigger | Severity (minor/major/critical) | Related FR | Screen state (NGUYÊN VĂN thông báo tiếng Việt hiển thị) | Recovery |`.\n\
- Mục 7: bảng `| ID (SC-<feature>-01...) | Outcome nghiệp vụ | Đo bằng | Mốc đạt |`.\n\
- Mục 8: gạch đầu dòng từng entity: tên, thuộc tính chính, quan hệ — tóm tắt, chi tiết để /erd.\n\
- Mục 9 Flows: mỗi luồng chính một tiểu mục `### Flow: <tên>` mở đầu bằng dòng 'Liên quan: FR-... | Error: E-... | Related UC: UC-...' + mermaid sequenceDiagram đầy đủ nhánh alt/opt cho lỗi.\n\
- Mục 10: danh sách màn hình (id-kebab — mô tả 1 câu + trạng thái lỗi nào hiện ở đó); mỗi trạng thái loại trừ nhau tách màn riêng.\n\
- Mục 11: bảng `| Ràng buộc/Phụ thuộc/Giả định | Loại | Source/Owner |`.",
        interview: &[
            "Luồng nghiệp vụ chính gồm những bước nào (nếu chưa có brainstorm)?",
            "Ngưỡng phi chức năng nào đã chốt (thời gian phản hồi, uptime, bảo mật)?",
            "Hệ thống tích hợp ngoài nào tham gia, vai trò gì?",
        ],
        max_tokens: 32000,
        format: "markdown",
    },
    DocTemplate {
        doc_type: "reverse_doc",
        subtype: "",
        phase: 2,
        skill: "/reverse-doc · /code-to-srs",
        title: "SRS tái lập (reverse)",
        desc: "Dựng lại SRS từ văn bản rời rạc hoặc source code, kèm mức tin cậy",
        scope: Scope::Feature,
        upstream: &[],
        sections: &[
            "## 0. Nguồn & phương pháp",
            "## 1. Scope",
            "## 2. Actors & Stakeholders",
            "## 3. Functional Requirements",
            "## 4. Non-Functional Requirements",
            "## 5. Business Rules",
            "## 6. Error Matrix",
            "## 7. Data Entities",
            "## 8. Khoảng trống & mâu thuẫn",
            "## Open Questions",
        ],
        prompt: "Tái lập SRS từ tư liệu dán vào (văn bản rời rạc HOẶC source code hệ thống cũ). Nguyên tắc: CHỈ viết điều có căn cứ trong tư liệu.\n\
- Mục 0: liệt kê nguồn đã nhận + phương pháp suy luận.\n\
- MỌI dòng FR/NFR/BR/E thêm 2 cột cuối: `| Tin cậy (cao/vừa/thấp) | Trích dẫn |` — Trích dẫn là câu chữ/tên file:dòng trong tư liệu làm căn cứ. Tin cậy 'thấp' = suy đoán từ ngữ cảnh, phải nói rõ.\n\
- Mục 8: điều tư liệu KHÔNG cho biết (khoảng trống) + chỗ tư liệu tự mâu thuẫn — trung thực, đây là mục giá trị nhất.\n\
- Cấu trúc bảng các mục 2-7 giống SRS chuẩn (xem quy ước ID).",
        interview: &[
            "Dán tư liệu vào (văn bản mô tả, ghi chú, hoặc source code). Nguồn gốc của nó?",
            "Hệ thống này đang chạy ở đâu, còn ai hiểu nó không?",
        ],
        max_tokens: 32000,
        format: "markdown",
    },
    // ============ Giai đoạn 3 — Sơ đồ nghiệp vụ ============
    DocTemplate {
        doc_type: "diagram",
        subtype: "sequence",
        phase: 3,
        skill: "/sequence",
        title: "Sơ đồ tuần tự",
        desc: "Ai gọi ai, theo thứ tự nào",
        scope: Scope::Feature,
        upstream: &["srs", "brainstorm"],
        sections: &["## Sơ đồ", "## Diễn giải", "## Open Questions"],
        prompt: "Vẽ mermaid `sequenceDiagram` cho TỪNG luồng chính của tính năng (mỗi luồng một tiểu mục `### Flow: <tên>` + dòng 'Liên quan: FR-... | Error: E-...').\n\
- Participant đặt tên vai trò (User, Web App, Backend, Database, <Hệ ngoài>).\n\
- Nhánh lỗi/rẽ dùng alt/opt/loop — luồng lỗi trong Error Matrix phải xuất hiện.\n\
- Diễn giải: bảng `| Bước | Diễn ra gì | FR liên quan |`.",
        interview: &["Luồng nào cần vẽ trước (nếu chưa có SRS thì mô tả luồng)?"],
        max_tokens: 12000,
        format: "markdown",
    },
    DocTemplate {
        doc_type: "diagram",
        subtype: "activity",
        phase: 3,
        skill: "/activity · /d2-activity",
        title: "Sơ đồ hoạt động",
        desc: "Luồng hoạt động có nhánh quyết định (kèm bản D2)",
        scope: Scope::Feature,
        upstream: &["srs", "brainstorm"],
        sections: &["## Sơ đồ", "## Diễn giải", "## Mã D2", "## Open Questions"],
        prompt: "Vẽ mermaid `flowchart TD` mô tả hoạt động nghiệp vụ đầu-cuối: nút bắt đầu/kết thúc, hình thoi cho quyết định (nhãn cạnh Có/Không), mọi nhánh lỗi phải về điểm kết thúc hợp lý (không nhánh cụt). Diễn giải từng nhánh quyết định trỏ D#/BR-xx nếu có.\n\
Mục 'Mã D2': fence ```d2 cùng sơ đồ đó viết bằng cú pháp D2 (shape: diamond cho quyết định, cạnh có nhãn) cho ai cần bản render đẹp đứng riêng bằng D2 CLI.",
        interview: &["Hoạt động nào cần vẽ, bắt đầu và kết thúc ở sự kiện gì?"],
        max_tokens: 10000,
        format: "markdown",
    },
    DocTemplate {
        doc_type: "diagram",
        subtype: "activity_swimlane",
        phase: 3,
        skill: "/activity-swimlane",
        title: "Sơ đồ hoạt động chia làn",
        desc: "Chia làn theo vai trò",
        scope: Scope::Feature,
        upstream: &["srs", "brainstorm"],
        sections: &["## Sơ đồ", "## Diễn giải", "## PlantUML", "## Open Questions"],
        prompt: "Sơ đồ hoạt động CHIA LÀN theo vai trò/phòng ban.\n\
- Bản render chính: mermaid `flowchart TD` dùng `subgraph <Vai trò>` làm làn, hoạt động của ai nằm trong subgraph người đó, mũi tên xuyên làn thể hiện bàn giao.\n\
- Mục PlantUML: fence ```plantuml activity diagram với |Làn| chuẩn cho ai cần import công cụ khác.",
        interview: &["Những vai trò/phòng ban nào tham gia quy trình?"],
        max_tokens: 10000,
        format: "markdown",
    },
    DocTemplate {
        doc_type: "diagram",
        subtype: "bpmn",
        phase: 3,
        skill: "/bpmn",
        title: "Quy trình BPMN 2.0",
        desc: "Chuẩn BPMN, import được Camunda / Bizagi",
        scope: Scope::Feature,
        upstream: &["srs", "brainstorm"],
        sections: &["## Sơ đồ xem nhanh", "## BPMN 2.0 XML", "## Diễn giải", "## Open Questions"],
        prompt: "Mô hình hoá quy trình theo BPMN 2.0.\n\
- 'Sơ đồ xem nhanh': mermaid flowchart mô phỏng (pool/lane bằng subgraph, gateway hình thoi ghi rõ loại X/O/+).\n\
- 'BPMN 2.0 XML': fence ```xml — tài liệu BPMN 2.0 HỢP LỆ import được Camunda/Bizagi: definitions + process + startEvent/endEvent, userTask/serviceTask, exclusiveGateway/parallelGateway, sequenceFlow đủ sourceRef/targetRef, kèm bpmndi tối thiểu (BPMNDiagram/BPMNPlane/BPMNShape/BPMNEdge với toạ độ hợp lý).\n\
- Diễn giải: bảng task → ai làm → FR/BR liên quan.",
        interview: &["Quy trình nào cần chuẩn hoá BPMN, ai tham gia, sự kiện bắt đầu/kết thúc?"],
        max_tokens: 16000,
        format: "markdown",
    },
    DocTemplate {
        doc_type: "diagram",
        subtype: "state",
        phase: 3,
        skill: "/state",
        title: "Sơ đồ trạng thái",
        desc: "Vòng đời trạng thái một đối tượng",
        scope: Scope::Feature,
        upstream: &["srs", "brainstorm"],
        sections: &["## Sơ đồ", "## Bảng chuyển trạng thái", "## Open Questions"],
        prompt: "Vẽ mermaid `stateDiagram-v2` cho vòng đời MỘT đối tượng (đơn hàng, tài khoản...): trạng thái [*] đầu/cuối, sự kiện trên cạnh.\n\
Bảng chuyển: `| Từ | Sự kiện | Điều kiện (BR) | Sang | Hành động kèm theo |` — mọi trạng thái phải có đường vào và (trừ terminal) đường ra; trạng thái cụt là lỗi thiết kế, nêu ở Open Questions.",
        interview: &["Đối tượng nào cần vẽ vòng đời? Các trạng thái bạn đã biết?"],
        max_tokens: 8000,
        format: "markdown",
    },
    DocTemplate {
        doc_type: "diagram",
        subtype: "erd",
        phase: 3,
        skill: "/erd · /d2-erd",
        title: "Sơ đồ quan hệ dữ liệu (ERD)",
        desc: "Thực thể và quan hệ (Mermaid, kèm bản D2)",
        scope: Scope::Feature,
        upstream: &["srs", "brainstorm"],
        sections: &["## Sơ đồ", "## Từ điển dữ liệu", "## Mã D2", "## Open Questions"],
        prompt: "Vẽ mermaid `erDiagram` từ Data Entities của SRS: đủ thực thể, quan hệ đúng bản số (||--o{ ...), thuộc tính chính có kiểu.\n\
Từ điển dữ liệu: mỗi entity một bảng `| Thuộc tính | Kiểu | Bắt buộc | Mô tả | Ràng buộc (unique/FK/default) |`.\n\
Mục 'Mã D2': fence ```d2 bản ERD cú pháp D2 (shape: sql_table từng entity, cạnh quan hệ có nhãn bản số) cho ai cần bản render đẹp bằng D2 CLI.",
        interview: &["Những thực thể dữ liệu chính nào (nếu chưa có SRS)?"],
        max_tokens: 12000,
        format: "markdown",
    },
    DocTemplate {
        doc_type: "diagram",
        subtype: "architecture",
        phase: 3,
        skill: "/d2-architect",
        title: "Sơ đồ kiến trúc hệ thống",
        desc: "Service, database, thành phần và liên kết",
        scope: Scope::Feature,
        upstream: &["srs", "api_design"],
        sections: &["## Sơ đồ", "## Thành phần", "## Mã D2", "## Open Questions"],
        prompt: "Vẽ kiến trúc hệ thống mức thành phần.\n\
- Bản render chính: mermaid `flowchart LR` — client / service / database / hệ ngoài, subgraph theo vùng (client/server/third-party), cạnh ghi giao thức (HTTPS/REST, WS, SQL...).\n\
- Bảng thành phần: `| Thành phần | Vai trò | Công nghệ (nếu đã chốt) | Giao tiếp với |`.\n\
- 'Mã D2': fence ```d2 tương đương cho ai dùng D2 CLI.",
        interview: &["Các thành phần hệ thống đã chốt hoặc dự kiến? Hệ ngoài nào tham gia?"],
        max_tokens: 10000,
        format: "markdown",
    },
    DocTemplate {
        doc_type: "diagram",
        subtype: "dbml",
        phase: 3,
        skill: "/dbdiagram",
        title: "Schema database (DBML)",
        desc: "DBML import dbdiagram.io, xuất SQL",
        scope: Scope::Feature,
        upstream: &["srs"],
        sections: &["## DBML", "## Ghi chú thiết kế", "## Open Questions"],
        prompt: "Sinh schema DBML (fence ```dbml) import thẳng dbdiagram.io: Table đủ cột + kiểu + not null/unique/default, Ref quan hệ đúng chiều, Enum cho trạng thái, indexes cho cột tra cứu.\n\
Ghi chú thiết kế: vì sao tách/gộp bảng, index nào cho truy vấn nào, trỏ về entity SRS.",
        interview: &["Hệ quản trị DB dự kiến (Postgres/MySQL/SQLite)? Quy mô dữ liệu ước tính?"],
        max_tokens: 10000,
        format: "markdown",
    },
    DocTemplate {
        doc_type: "diagram",
        subtype: "usecase",
        phase: 3,
        skill: "/usecase-diagram",
        title: "Sơ đồ use case",
        desc: "Actor và phạm vi hệ thống",
        scope: Scope::Feature,
        upstream: &["srs", "usecase"],
        sections: &["## Sơ đồ", "## Danh sách use case", "## PlantUML", "## Open Questions"],
        prompt: "Sơ đồ use case tổng quan.\n\
- Bản render chính: mermaid `flowchart LR` — actor hình tròn kép (((Actor))), use case hình bầu dục ([UC: ...]) trong subgraph 'Hệ thống', cạnh actor—UC, «include»/«extend» bằng cạnh đứt nét có nhãn.\n\
- Danh sách: bảng `| UC-<feature>-001... | Tên | Actor chính | FR liên quan |`.\n\
- PlantUML: fence ```plantuml usecase chuẩn.",
        interview: &["Actor nào tương tác với hệ thống (nếu chưa có SRS)?"],
        max_tokens: 8000,
        format: "markdown",
    },
    // ============ Giai đoạn 4 — Use case & user story ============
    DocTemplate {
        doc_type: "usecase",
        subtype: "",
        phase: 4,
        skill: "/usecase",
        title: "Use case chi tiết (Cockburn)",
        desc: "Actor, điều kiện, luồng chính, ngoại lệ",
        scope: Scope::Feature,
        upstream: &["srs", "brainstorm"],
        sections: &["## Danh mục use case", "## Open Questions"],
        prompt: "Viết use case chi tiết chuẩn Cockburn cho MỌI luồng chính của tính năng.\n\
Danh mục: bảng `| ID | Tên | Actor chính | FR phủ |`.\n\
Sau đó mỗi use case một tiểu mục `### UC-<feature>-001 — <Tên>` gồm đủ trường:\n\
- **Actor chính** / **Stakeholders & lợi ích** (bảng)\n\
- **Precondition** / **Trigger** / **Postcondition (Success guarantee)** / **Minimal guarantee**\n\
- **Main Success Scenario**: các bước đánh số 1..N, mỗi bước 'Actor làm X / Hệ thống làm Y'\n\
- **Extensions**: đánh số theo bước rẽ (3a, 3b, 5a...), mỗi extension: điều kiện → các bước xử lý → quay về bước nào/kết thúc; phủ MỌI error liên quan trong Error Matrix (ghi E-xx)\n\
- **Special requirements**: NFR liên quan (ghi NFR-xx)\n\
- **Liên quan**: FR-xx list.",
        interview: &["Các luồng người dùng chính (nếu chưa có SRS)?"],
        max_tokens: 20000,
        format: "markdown",
    },
    DocTemplate {
        doc_type: "userstory",
        subtype: "",
        phase: 4,
        skill: "/userstory",
        title: "User story backlog",
        desc: "Story sẵn sàng đưa vào backlog",
        scope: Scope::Feature,
        upstream: &["srs", "usecase", "brainstorm", "prd_epic"],
        sections: &["## Backlog", "## Ghi chú bóc tách", "## Open Questions"],
        prompt: "Bóc user story sẵn sàng vào backlog từ FR của SRS (mỗi FR phải được ÍT NHẤT một story phủ — truy vết được chấm tự động).\n\
Backlog: bảng `| ID | User story | Ưu tiên (MoSCoW) | FR phủ | Ước lượng (S/M/L) | Ghi chú |`\n\
- ID = US-<feature>-001 tăng dần.\n\
- Story đúng mẫu: 'Là <persona>, tôi muốn <hành động> để <giá trị>.' — persona lấy từ URD nếu có.\n\
- Cột 'FR phủ' ghi ĐẦY ĐỦ ID: FR-<feature>-001 (nhiều FR cách nhau dấu phẩy, KHÔNG viết tắt, không ghi khoảng '001-005').\n\
- Story quá to (phủ >3 FR) thì tách.\n\
Ghi chú bóc tách: story nào gộp/tách từ FR nào, vì sao; FR nào KHÔNG bóc thành story (kỹ thuật thuần) và lý do.",
        interview: &["Đội dev làm việc theo Scrum/Kanban? Sprint bao lâu (để cân cỡ story)?"],
        max_tokens: 16000,
        format: "markdown",
    },
    DocTemplate {
        doc_type: "ac",
        subtype: "",
        phase: 4,
        skill: "/ac",
        title: "Acceptance criteria",
        desc: "Given / When / Then cho từng story",
        scope: Scope::Feature,
        upstream: &["userstory", "srs"],
        sections: &["## Acceptance criteria", "## Open Questions"],
        prompt: "Viết acceptance criteria Given/When/Then cho TỪNG user story trong backlog (mọi US phải có ít nhất 1 AC).\n\
Mỗi story một tiểu mục `### US-<feature>-001 — <tên story>` chứa bảng `| ID | Kịch bản | Given | When | Then |`\n\
- ID = AC-<feature>-001 tăng dần XUYÊN SUỐT tài liệu (không reset theo story).\n\
- Mỗi story tối thiểu: 1 happy path + 1 negative/error + edge case nếu có (ngưỡng, giới hạn từ BR/Error Matrix — ghi số cụ thể).\n\
- Then phải quan sát được (thông báo hiện gì, trạng thái đổi gì) — khớp nguyên văn Screen state trong Error Matrix khi liên quan.",
        interview: &[],
        max_tokens: 20000,
        format: "markdown",
    },
    // ============ Giai đoạn 5 — Thiết kế màn hình ============
    DocTemplate {
        doc_type: "user_flow",
        subtype: "",
        phase: 5,
        skill: "/user-flow",
        title: "User flow",
        desc: "Luồng người dùng phủ happy / error / edge",
        scope: Scope::Feature,
        upstream: &["srs", "brainstorm", "usecase"],
        sections: &["## Tổng quan luồng", "## Chi tiết từng luồng", "## Ma trận phủ", "## Open Questions"],
        prompt: "Phân tích user flow phủ happy/error/edge.\n\
- Tổng quan: mermaid flowchart TD toàn cục — mọi entry point (từ đâu vào), các màn (id-kebab), điểm thoát.\n\
- Chi tiết: mỗi luồng một tiểu mục: bước → màn → hành động → kết quả; nhánh lỗi ghi E-xx; nhánh edge (mạng chậm, dữ liệu rỗng, quyền thiếu...).\n\
- Ma trận phủ: bảng `| Luồng | Happy | Error nào (E-xx) | Edge nào | Màn đi qua |` — chứng minh không sót nhánh.",
        interview: &["Người dùng vào tính năng từ những đâu (menu, link, notification...)?"],
        max_tokens: 12000,
        format: "markdown",
    },
    DocTemplate {
        doc_type: "wireframe_ascii",
        subtype: "",
        phase: 5,
        skill: "/wireframe-ascii",
        title: "Wireframe ASCII",
        desc: "Phác nhanh khung màn hình dạng ký tự",
        scope: Scope::Feature,
        upstream: &["user_flow", "srs"],
        sections: &["## Danh sách màn", "## Wireframe", "## Open Questions"],
        prompt: "Phác wireframe dạng ký tự cho TỪNG màn trong danh sách Screens của SRS/user flow.\n\
Mỗi màn một tiểu mục `### <screen-id> — <tên>`:\n\
- Khung box-drawing (┌─┐│└┘) trong fence ```text, bố cục đúng thứ tự thị giác: header, nội dung, action chính; input ghi `[.........]`, nút ghi `[ Đăng nhập ]`, radio/checkbox `( ) [x]`.\n\
- Dưới mỗi khung: bảng mô tả control `| # | Control | Loại | Bắt buộc | Hành vi / validation (FR/E liên quan) |` — số # đánh dấu ngay trong khung bằng chú thích (1), (2).\n\
- Mỗi trạng thái loại trừ nhau (thành công / lỗi) tách khung riêng.",
        interview: &[],
        max_tokens: 20000,
        format: "markdown",
    },
    DocTemplate {
        doc_type: "wireframe_html",
        subtype: "",
        phase: 5,
        skill: "/wireframe-html",
        title: "Wireframe HTML",
        desc: "Wireframe đen trắng, render element thật",
        scope: Scope::Feature,
        upstream: &["user_flow", "wireframe_ascii", "srs"],
        sections: &[],
        prompt: "Sinh MỘT file HTML wireframe đen trắng cho toàn bộ các màn của tính năng. TRẢ VỀ CHỈ HTML (không markdown, không fence), bắt đầu bằng <!DOCTYPE html>.\n\
Yêu cầu:\n\
- Tự chứa 100%: CSS inline trong <style>, KHÔNG tải resource ngoài (không CDN, không ảnh ngoài, không font ngoài).\n\
- Đen-trắng-xám wireframe thật: element form thật (input, button, select, table), khối ảnh là hộp gạch chéo, KHÔNG màu thương hiệu, KHÔNG logo.\n\
- Mỗi màn một <section class=\"screen\" id=\"<screen-id>\"> có tiêu đề màn + khung viền; các màn xếp dọc, đầu trang có mục lục anchor tới từng màn.\n\
- Dưới mỗi màn: bảng mô tả control (#, control, loại, bắt buộc, hành vi/validation kèm FR/E-id dạng text).\n\
- Trạng thái lỗi hiển thị đúng NGUYÊN VĂN thông báo trong Error Matrix (nếu có SRS).\n\
- JS tối thiểu chỉ để chuyển tab/anchor nếu cần, không framework.",
        interview: &[],
        max_tokens: 32000,
        format: "html",
    },
    DocTemplate {
        doc_type: "prototype_html",
        subtype: "",
        phase: 5,
        skill: "/prototype-html",
        title: "Prototype HTML bấm được",
        desc: "Điều hướng như app thật, lưu trạng thái",
        scope: Scope::Feature,
        upstream: &["user_flow", "wireframe_html", "srs"],
        sections: &[],
        prompt: "Sinh MỘT file HTML prototype BẤM ĐƯỢC mô phỏng tính năng như app thật. TRẢ VỀ CHỈ HTML, bắt đầu bằng <!DOCTYPE html>.\n\
Yêu cầu:\n\
- Tự chứa 100%, vanilla JS, không CDN.\n\
- SPA một file: mỗi màn một <section class=\"screen\">, JS show/hide theo hash (#/screen-id), nút điều hướng đúng user flow (happy + error + edge).\n\
- Trạng thái lưu localStorage (namespace 'ba-proto-<feature>') để refresh không mất; nút 'Reset demo' xoá state.\n\
- Validation thật theo Error Matrix: nhập sai hiện đúng NGUYÊN VĂN thông báo lỗi, đủ nhánh (thử được cả luồng lỗi).\n\
- Giao diện sạch trung tính (xám/xanh nhạt), mobile-friendly (max-width 480px căn giữa), đây là prototype nghiệp vụ không phải design cuối.\n\
- Góc màn hình có badge nhỏ ghi screen-id hiện tại để đối chiếu tài liệu.",
        interview: &[],
        max_tokens: 32000,
        format: "html",
    },
    // ============ Giai đoạn 6 — Tích hợp API ============
    DocTemplate {
        doc_type: "api_assess",
        subtype: "",
        phase: 6,
        skill: "/api-assess",
        title: "Đánh giá đối tác API",
        desc: "Cân nhắc tự xây hay mua/tích hợp",
        scope: Scope::Feature,
        upstream: &["brainstorm", "srs"],
        sections: &["## 1. Nhu cầu tích hợp", "## 2. Ứng viên", "## 3. Ma trận so sánh", "## 4. Build vs Buy", "## 5. Khuyến nghị", "## Open Questions"],
        prompt: "Đánh giá lựa chọn đối tác API cho một nhu cầu tích hợp.\n\
- Mục 2: từng ứng viên (kể cả phương án tự xây): mô tả, mô hình giá, giới hạn đã biết. Chưa rõ thông tin thật thì ghi OQ — KHÔNG bịa giá/SLA.\n\
- Mục 3: bảng so sánh theo tiêu chí: độ phủ nghiệp vụ, giá, SLA/uptime, tài liệu, hỗ trợ khu vực/pháp lý, khoá vendor — chấm C/V/T kèm căn cứ.\n\
- Mục 4: build vs buy: chi phí xây + vận hành vs phí dịch vụ, thời gian ra thị trường.\n\
- Mục 5: khuyến nghị dứt khoát + điều kiện đảo chiều.",
        interview: &["Cần tích hợp dịch vụ gì (thanh toán, email, vận chuyển...)? Ứng viên nào đang cân nhắc?", "Ngân sách và khối lượng dự kiến (giao dịch/tháng)?"],
        max_tokens: 10000,
        format: "markdown",
    },
    DocTemplate {
        doc_type: "api_doc",
        subtype: "",
        phase: 6,
        skill: "/api-doc",
        title: "Tóm tắt tài liệu API đối tác",
        desc: "Đọc doc API thành bản tóm tắt nghiệp vụ",
        scope: Scope::Feature,
        upstream: &[],
        sections: &["## 1. Tổng quan dịch vụ", "## 2. Xác thực & môi trường", "## 3. Endpoint theo nghiệp vụ", "## 4. Giới hạn & lưu ý", "## 5. Thuật ngữ đối tác", "## Open Questions"],
        prompt: "Đọc tài liệu API đối tác (dán trong đầu vào) và tóm tắt CHO NGƯỜI LÀM NGHIỆP VỤ.\n\
- Mục 3: bảng `| Nghiệp vụ | Endpoint | Method | Input chính | Output chính | Lỗi hay gặp |` — nhóm theo mục đích nghiệp vụ chứ không theo thứ tự doc gốc.\n\
- Mục 4: rate limit, idempotency, webhook/polling, timeout, phiên bản — điều doc gốc nói, kèm trích dẫn; không suy diễn.\n\
- Mục 5: bảng thuật ngữ đối tác ↔ thuật ngữ hệ mình.",
        interview: &["Dán tài liệu API đối tác (hoặc phần liên quan) vào đầu vào."],
        max_tokens: 12000,
        format: "markdown",
    },
    DocTemplate {
        doc_type: "api_design",
        subtype: "",
        phase: 6,
        skill: "/api-design",
        title: "Thiết kế tích hợp",
        desc: "Các hệ thống phối hợp thế nào",
        scope: Scope::Feature,
        upstream: &["srs", "api_doc", "api_assess"],
        sections: &["## 1. Bối cảnh & mục tiêu", "## 2. Kiến trúc tích hợp", "## 3. Luồng tích hợp", "## 4. Hợp đồng dữ liệu", "## 5. Xử lý lỗi & bù trừ", "## 6. Bảo mật", "## Open Questions"],
        prompt: "Thiết kế tích hợp giữa hệ mình và (các) hệ ngoài.\n\
- Mục 2: mermaid flowchart thành phần + vai trò từng bên; đồng bộ hay bất đồng bộ, webhook hay polling — nêu lý do chọn.\n\
- Mục 3: mỗi luồng một mermaid sequenceDiagram (happy + lỗi chính: timeout, từ chối, trùng lặp).\n\
- Mục 4: bảng payload chính `| Trường | Kiểu | Nguồn | Bắt buộc | Ghi chú |` cho từng bước trao đổi.\n\
- Mục 5: ma trận lỗi tích hợp `| Tình huống | Phát hiện thế nào | Xử lý | Bù trừ/Retry (idempotency key?) |` — quan trọng nhất: tiền/đơn không được mất hay đúp.\n\
- Mục 6: khoá lưu đâu (server-side secret store), dữ liệu nhạy cảm che thế nào trong log.",
        interview: &["Tích hợp với hệ nào, trao đổi dữ liệu gì, chiều nào?"],
        max_tokens: 16000,
        format: "markdown",
    },
    DocTemplate {
        doc_type: "api_map",
        subtype: "",
        phase: 6,
        skill: "/api-map",
        title: "Mapping field 3 tầng",
        desc: "API ↔ dữ liệu hệ thống ↔ màn hình",
        scope: Scope::Feature,
        upstream: &["api_doc", "api_design", "srs", "wireframe_html"],
        sections: &["## 1. Bảng mapping", "## 2. Transform & quy tắc", "## 3. Khoảng trống", "## Open Questions"],
        prompt: "Lập bảng mapping field 3 tầng cho từng luồng dữ liệu.\n\
- Mục 1: bảng `| Field API (path.to.field) | Kiểu API | Field hệ thống (entity.column) | Màn hình / control | Transform | Bắt buộc | Ghi chú |` — đủ mọi field dùng tới; field API bỏ qua ghi rõ 'không dùng'.\n\
- Mục 2: quy tắc transform viết rõ (format ngày, đơn vị tiền, mã enum đối tác ↔ enum mình, default khi thiếu).\n\
- Mục 3: field màn hình cần mà API không có (lấy đâu?), field hệ thống chưa có chỗ chứa (thêm cột nào?) — mỗi khoảng trống một đề xuất.",
        interview: &[],
        max_tokens: 16000,
        format: "markdown",
    },
    DocTemplate {
        doc_type: "api_checklist",
        subtype: "",
        phase: 6,
        skill: "/api-checklist",
        title: "Checklist test API",
        desc: "Cần test gì cho API",
        scope: Scope::Feature,
        upstream: &["api_design", "api_map", "api_doc"],
        sections: &["## Checklist", "## Lệnh thử nhanh", "## Open Questions"],
        prompt: "Checklist test tích hợp API, nhóm theo: Xác thực · Happy path · Validation · Lỗi đối tác (4xx/5xx/timeout) · Retry & idempotency · Dữ liệu biên · Bảo mật (khoá sai, replay) · Webhook (nếu có).\n\
Mỗi mục: `- [ ] <kịch bản> — kỳ vọng: <kết quả> (FR/E liên quan)`.\n\
'Lệnh thử nhanh': vài lệnh curl mẫu (fence ```bash) cho happy + 1-2 lỗi chính, dùng placeholder {{BASE_URL}} {{TOKEN}} — không nhúng khoá thật.",
        interview: &[],
        max_tokens: 10000,
        format: "markdown",
    },
    DocTemplate {
        doc_type: "api_test",
        subtype: "",
        phase: 6,
        skill: "/api-test",
        title: "Bộ test API (Bruno)",
        desc: "Test API kiểu Postman, sinh collection Bruno chạy được",
        scope: Scope::Feature,
        upstream: &["api_checklist", "api_doc", "api_design", "api_map"],
        sections: &["## 1. Collection Bruno", "## 2. Biến môi trường", "## 3. Hướng dẫn chạy", "## Open Questions"],
        prompt: "Sinh bộ test API dạng Bruno collection CHẠY ĐƯỢC từ API checklist/design.\n\
- Mục 1: mỗi request một tiểu mục `### <thứ tự> — <tên kịch bản>` chứa fence ```bru là MỘT file .bru hợp lệ cú pháp Bruno: khối meta { name, type: http, seq }, khối method (get/post...) { url, body:json nếu có, auth }, khối headers, khối assert (status, body jsonpath) và khối tests { } dùng JS expect khi cần kiểm sâu. Đặt tên file gợi ý ở dòng đầu tiểu mục (vd `auth/login-happy.bru`).\n\
- Phủ theo checklist: happy path, validation, lỗi 4xx/5xx, timeout/retry, idempotency (gửi trùng request), bảo mật (khoá sai). Mỗi kịch bản checklist P0 ít nhất 1 file.\n\
- Dùng biến {{baseUrl}} {{token}}... — KHÔNG nhúng khoá thật, KHÔNG bịa endpoint (thiếu spec thì ghi OQ).\n\
- Mục 2: fence ```bru file `environments/local.bru` khai vars { baseUrl, token... } giá trị placeholder.\n\
- Mục 3: lệnh cài + chạy (```bash): npm i -g @usebruno/cli, cách bung các fence thành thư mục collection, `bru run --env local`. Ghi rõ app BA Studio KHÔNG tự chạy test — dán vào repo hoặc app AutoTest.",
        interview: &["Base URL môi trường test và cách lấy token thử nghiệm?"],
        max_tokens: 24000,
        format: "markdown",
    },
    DocTemplate {
        doc_type: "api_readiness",
        subtype: "",
        phase: 6,
        skill: "/api-readiness",
        title: "Cổng kiểm tra production",
        desc: "Điều kiện trước khi lên production",
        scope: Scope::Feature,
        upstream: &["api_design", "api_checklist"],
        sections: &["## Cổng kiểm tra", "## Kế hoạch cutover", "## Rollback", "## Open Questions"],
        prompt: "Readiness gate trước khi bật tích hợp trên production.\n\
- Cổng kiểm tra: bảng `| # | Hạng mục | Điều kiện đạt | Bằng chứng | Trạng thái ☐/☑ | Owner |` nhóm: hợp đồng/pháp lý, khoá & môi trường prod, giám sát-cảnh báo, ngưỡng rate limit, xử lý sự cố đối tác, dữ liệu thật thử nghiệm.\n\
- Cutover: bật cho ai trước (canary %), bật thế nào, ai trực.\n\
- Rollback: điều kiện kích hoạt + các bước quay lui + dữ liệu dở dang xử lý sao.",
        interview: &[],
        max_tokens: 8000,
        format: "markdown",
    },
    // ============ Giai đoạn 7 — Kiểm thử ============
    DocTemplate {
        doc_type: "test_checklist",
        subtype: "",
        phase: 7,
        skill: "/test-checklist",
        title: "Test checklist",
        desc: "Outline kịch bản cần test để review trước",
        scope: Scope::Feature,
        upstream: &["usecase", "ac", "srs", "userstory"],
        sections: &["## Checklist theo luồng", "## Ma trận phủ", "## Open Questions"],
        prompt: "Outline các kịch bản cần test (để review TRƯỚC khi viết test case chi tiết).\n\
- Checklist: nhóm theo luồng/use case, mỗi mục `- [ ] <kịch bản> — loại (happy/error/edge/security/performance) — ưu tiên (P0/P1) — nguồn (UC-xx / US-xx / AC-xx / E-xx)`.\n\
- Phủ đủ: mọi UC, mọi AC, mọi dòng Error Matrix, các ngưỡng trong BR (test đúng ngưỡng, dưới ngưỡng, trên ngưỡng), mobile/responsive nếu SRS nhắc.\n\
- Ma trận phủ: bảng `| UC/US | Số kịch bản | Ghi chú lỗ hổng |` — UC/US nào 0 kịch bản phải nêu lý do.",
        interview: &[],
        max_tokens: 16000,
        format: "markdown",
    },
    DocTemplate {
        doc_type: "test_cases",
        subtype: "",
        phase: 7,
        skill: "/test-cases",
        title: "Test cases chi tiết",
        desc: "Test case chạy được, sinh từ checklist",
        scope: Scope::Feature,
        upstream: &["test_checklist", "ac", "srs"],
        sections: &["## Test cases", "## Dữ liệu test", "## Open Questions"],
        prompt: "Viết test case chi tiết CHẠY ĐƯỢC từ test checklist (mỗi mục checklist ≥1 case).\n\
- Bảng `| ID | Tiêu đề | Ưu tiên | Tiền điều kiện | Các bước (đánh số, xuống dòng <br>) | Dữ liệu | Kết quả mong đợi | Loại | Phủ (US/AC/UC/E) |`\n\
- ID = TC-<feature>-001 tăng dần. Bước cụ thể bấm-gõ được; kết quả mong đợi quan sát được, khớp NGUYÊN VĂN thông báo trong Error Matrix khi test lỗi.\n\
- Dữ liệu test: bảng bộ dữ liệu dùng chung (tài khoản, bản ghi mồi) để tester dựng môi trường.",
        interview: &[],
        max_tokens: 32000,
        format: "markdown",
    },
    DocTemplate {
        doc_type: "playwright",
        subtype: "",
        phase: 7,
        skill: "/playwright-gen",
        title: "Script Playwright",
        desc: "Script tự động chạy test trên trình duyệt",
        scope: Scope::Feature,
        upstream: &["test_cases", "wireframe_html"],
        sections: &["## Script", "## Hướng dẫn chạy", "## Open Questions"],
        prompt: "Sinh script Playwright TypeScript tự động hoá các test case P0 (fence ```ts, một file <feature>.spec.ts).\n\
- test.describe theo luồng; mỗi TC một test('TC-<feature>-001 — <tiêu đề>').\n\
- Selector ưu tiên getByRole/getByLabel/getByPlaceholder theo wireframe; BASE_URL đọc từ env.\n\
- Assertion khớp nguyên văn thông báo lỗi tiếng Việt trong Error Matrix.\n\
- Hướng dẫn chạy: lệnh cài + chạy (```bash), lưu ý app này KHÔNG tự chạy test — dán vào repo dự án hoặc app AutoTest.",
        interview: &[],
        max_tokens: 24000,
        format: "markdown",
    },
    // ============ Giai đoạn 8 — Kiểm soát chất lượng ============
    DocTemplate {
        doc_type: "gap_report",
        subtype: "",
        phase: 8,
        skill: "/gap",
        title: "Báo cáo gap",
        desc: "Soi tính năng còn thiếu luồng nghiệp vụ nào",
        scope: Scope::Feature,
        upstream: &["srs", "usecase", "userstory", "user_flow", "test_checklist"],
        sections: &["## 1. Tổng kết", "## 2. Gap phát hiện", "## 3. Đề xuất xử lý", "## Open Questions"],
        prompt: "Soi TOÀN BỘ tài liệu của tính năng tìm lỗ hổng nghiệp vụ. Đối chiếu chéo: luồng nào có màn nhưng không có FR? FR nào không xuất hiện trong flow? Error nào không màn nào hiển thị? Trạng thái nào của đối tượng không đường ra? Ngưỡng nào nói ở brainstorm mà SRS bỏ quên? Persona nào URD nêu mà không luồng nào phục vụ?\n\
- Mục 2: bảng `| # | Gap | Loại (thiếu luồng/thiếu màn/thiếu rule/mâu thuẫn/mồ côi) | Mức độ (cao/vừa/thấp) | Bằng chứng (trích tài liệu nào mục nào) |`.\n\
- Mục 3: mỗi gap một đề xuất: sửa tài liệu nào, thêm gì; gap đáng mở CR thì ghi rõ.\n\
- Trung thực: không thấy gap thì nói rõ đã soi những trục nào và sạch.",
        interview: &[],
        max_tokens: 12000,
        format: "markdown",
    },
    DocTemplate {
        doc_type: "doc_drift",
        subtype: "",
        phase: 8,
        skill: "/doc-drift",
        title: "Đối chiếu code ↔ tài liệu",
        desc: "Chỗ nào code lệch tài liệu, chỗ nào chưa làm",
        scope: Scope::Feature,
        upstream: &["srs", "ac"],
        sections: &["## 1. Phương pháp & phạm vi", "## 2. Bảng đối chiếu", "## 3. Kết luận", "## Open Questions"],
        prompt: "Đối chiếu source code / ghi chú dev (dán trong đầu vào) với tài liệu đặc tả của tính năng.\n\
- Mục 2: bảng `| FR/BR/E | Tài liệu nói gì | Code thấy gì (file:dòng hoặc trích đoạn) | Kết luận (khớp / lệch / chưa làm / code có mà doc không) | Mức độ |`.\n\
- Xét cả chiều ngược: hành vi trong code mà tài liệu không nhắc (undocumented behavior).\n\
- Mục 3: đếm khớp/lệch/chưa làm; lệch nghiêm trọng đề xuất mở CR hay sửa code.\n\
- Chỉ kết luận từ code được dán — thiếu code phần nào ghi 'không đủ dữ liệu', không đoán.",
        interview: &["Dán source code (hoặc phần liên quan) của tính năng vào đầu vào."],
        max_tokens: 16000,
        format: "markdown",
    },
    // ============ Giai đoạn 9 — Bàn giao & vận hành ============
    DocTemplate {
        doc_type: "userguide",
        subtype: "",
        phase: 9,
        skill: "/userguide",
        title: "Cẩm nang vận hành",
        desc: "Hướng dẫn cho admin và chăm sóc khách hàng",
        scope: Scope::Feature,
        upstream: &["srs", "wireframe_html", "usecase"],
        sections: &["## 1. Dành cho ai", "## 2. Hướng dẫn theo tình huống", "## 3. Câu hỏi thường gặp", "## 4. Xử lý sự cố", "## Open Questions"],
        prompt: "Viết cẩm nang vận hành cho admin + CSKH (KHÔNG phải cho dev).\n\
- Mục 2: theo TÌNH HUỐNG thực ('Khách báo không đăng nhập được' → các bước kiểm tra, làm gì, nói gì với khách) — bước đánh số, chỗ bấm ghi theo tên màn/nút trong wireframe.\n\
- Mục 3: FAQ từ Error Matrix — mỗi lỗi người dùng gặp: vì sao, khách tự xử được không, CSKH làm gì.\n\
- Mục 4: bảng `| Triệu chứng | Nguyên nhân khả dĩ | Bước xử lý | Khi nào chuyển kỹ thuật |`.\n\
- Giọng dễ hiểu, không thuật ngữ dev; thuật ngữ nghiệp vụ giải thích lần đầu dùng.",
        interview: &[],
        max_tokens: 16000,
        format: "markdown",
    },
    DocTemplate {
        doc_type: "meeting",
        subtype: "",
        phase: 9,
        skill: "/meet",
        title: "Biên bản họp",
        desc: "Ghi chú họp → quyết định + việc cần làm",
        scope: Scope::Project,
        upstream: &[],
        sections: &["## 1. Thông tin cuộc họp", "## 2. Nội dung thảo luận", "## 3. Quyết định", "## 4. Việc cần làm", "## 5. Chủ đề treo", "## Open Questions"],
        prompt: "Biến ghi chú họp thô (dán trong đầu vào) thành biên bản chuẩn.\n\
- Mục 1: chủ đề, thời gian (nếu ghi chú có), người tham dự.\n\
- Mục 2: gom theo chủ đề, mỗi chủ đề: ai nêu gì, tranh luận chính — trung thành với ghi chú, KHÔNG thêm ý không có.\n\
- Mục 3: bảng `| # | Quyết định | Căn cứ | Ảnh hưởng tài liệu nào (đề xuất CR nếu đổi đặc tả) |`.\n\
- Mục 4: bảng `| # | Việc | Ai làm | Hạn | Ghi chú |` — ghi chú không nói ai/hạn thì để '?' và đưa vào OQ.\n\
- Mục 5: chủ đề bàn dở, cần họp tiếp.",
        interview: &["Dán ghi chú họp thô vào đầu vào."],
        max_tokens: 10000,
        format: "markdown",
    },
    DocTemplate {
        doc_type: "overview",
        subtype: "",
        phase: 9,
        skill: "/update-overview",
        title: "Tài liệu dùng chung dự án",
        desc: "Glossary, môi trường, convention toàn dự án",
        scope: Scope::Project,
        upstream: &["prd", "srs"],
        sections: &["## 1. Glossary", "## 2. Operating Environment", "## 3. Conventions", "## 4. Đối tác tích hợp", "## Open Questions"],
        prompt: "Tài liệu nền dùng chung mọi tính năng (phần 'Giới thiệu & Mô tả chung' của bộ SRS).\n\
- Mục 1 Glossary: bảng `| Thuật ngữ | Định nghĩa | Xuất hiện ở (feature slugs) | Aliases (kể cả 'tránh dùng') |` — gom thuật ngữ từ MỌI tài liệu hiện có, thống nhất cách gọi (một khái niệm một tên).\n\
- Mục 2: nền tảng đích, trình duyệt/thiết bị hỗ trợ, breakpoint, ngôn ngữ giao diện, giả định mạng/phiên.\n\
- Mục 3: convention tài liệu + UI dùng chung (đánh dấu trường bắt buộc, format tiền/ngày, max width...).\n\
- Mục 4: bảng `| Đối tác | Vai trò | Feature dùng |`.",
        interview: &[],
        max_tokens: 12000,
        format: "markdown",
    },
];

/// 3 workflow mẫu của BA-Kit (bỏ bước /jira — ngoài phạm vi app).
/// (key, tên, mô tả, các bước (doc_type, subtype))
pub const WORKFLOW_TEMPLATES: &[(&str, &str, &str, &[(&str, &str)])] = &[
    (
        "full-lifecycle",
        "Trọn vòng đời (mặc định)",
        "Định hình sản phẩm, làm rõ nghiệp vụ, đặc tả, thiết kế, bóc story rồi kiểm thử.",
        &[
            ("prd", ""),
            ("roadmap", ""),
            ("brainstorm", ""),
            ("srs", ""),
            ("wireframe_html", ""),
            ("userstory", ""),
            ("test_checklist", ""),
            ("test_cases", ""),
        ],
    ),
    (
        "story-first",
        "User story trước, chi tiết sau",
        "Chốt backlog sớm cho team dev, rồi quay lại đặc tả sâu và kiểm thử.",
        &[
            ("brainstorm", ""),
            ("userstory", ""),
            ("ac", ""),
            ("srs", ""),
            ("wireframe_html", ""),
            ("test_checklist", ""),
            ("test_cases", ""),
        ],
    ),
    (
        "prototype-first",
        "Prototype demo trước",
        "Có bản bấm được cho khách xem sớm; chốt hướng xong mới làm chi tiết nghiệp vụ và kiểm thử.",
        &[
            ("user_flow", ""),
            ("prototype_html", ""),
            ("brainstorm", ""),
            ("srs", ""),
            ("userstory", ""),
            ("test_checklist", ""),
            ("test_cases", ""),
        ],
    ),
];

pub fn get(doc_type: &str, subtype: &str) -> Option<&'static DocTemplate> {
    TEMPLATES
        .iter()
        .find(|t| t.doc_type == doc_type && t.subtype == subtype)
        .or_else(|| {
            // diagram gọi thiếu subtype → không match; doc_type thường gọi kèm
            // subtype rỗng dư → match theo doc_type khi template chỉ có 1 entry.
            if subtype.is_empty() {
                let mut it = TEMPLATES.iter().filter(|t| t.doc_type == doc_type);
                let first = it.next();
                if it.next().is_none() {
                    return first;
                }
            }
            None
        })
}

pub fn phase_name(phase: u8) -> &'static str {
    PHASES
        .iter()
        .find(|(n, _)| *n == phase)
        .map(|(_, s)| *s)
        .unwrap_or("")
}

/// Danh sách template theo giai đoạn cho catalog UI/MCP.
pub fn catalog() -> serde_json::Value {
    let phases: Vec<serde_json::Value> = PHASES
        .iter()
        .map(|(num, name)| {
            let items: Vec<serde_json::Value> = TEMPLATES
                .iter()
                .filter(|t| t.phase == *num)
                .map(|t| {
                    serde_json::json!({
                        "doc_type": t.doc_type,
                        "subtype": t.subtype,
                        "skill": t.skill,
                        "title": t.title,
                        "desc": t.desc,
                        "scope": if t.scope == Scope::Project { "project" } else { "feature" },
                        "format": t.format,
                        "upstream": t.upstream,
                        "has_interview": !t.interview.is_empty(),
                    })
                })
                .collect();
            serde_json::json!({ "phase": num, "name": name, "items": items })
        })
        .collect();
    serde_json::json!(phases)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_keys_unique() {
        let mut seen = std::collections::HashSet::new();
        for t in TEMPLATES {
            assert!(
                seen.insert((t.doc_type, t.subtype)),
                "trùng template: {}/{}",
                t.doc_type,
                t.subtype
            );
        }
    }

    #[test]
    fn every_phase_has_templates_and_valid_fields() {
        for (num, _) in PHASES {
            assert!(
                TEMPLATES.iter().any(|t| t.phase == *num),
                "giai đoạn {num} không có template nào"
            );
        }
        for t in TEMPLATES {
            assert!(t.phase >= 1 && t.phase <= 9, "{} phase lạ", t.doc_type);
            assert!(t.prompt.len() > 80, "{} prompt quá mỏng", t.doc_type);
            assert!(t.max_tokens >= 2000 && t.max_tokens <= 32000, "{} max_tokens ngoài khoảng bridge", t.doc_type);
            assert!(matches!(t.format, "markdown" | "html"), "{} format lạ", t.doc_type);
            // Tài liệu markdown nào cũng phải chốt Open Questions — kỷ luật chống bịa.
            if t.format == "markdown" {
                assert!(
                    t.sections.iter().any(|s| s.contains("Open Questions")),
                    "{} thiếu section Open Questions",
                    t.doc_type
                );
            }
        }
    }

    #[test]
    fn pipeline_types_exist() {
        for p in PIPELINE {
            assert!(get(p, "").is_some(), "pipeline doc_type {p} không có template");
        }
    }

    #[test]
    fn workflow_template_steps_exist() {
        for (key, _, _, steps) in WORKFLOW_TEMPLATES {
            assert!(!steps.is_empty(), "{key} rỗng");
            for (dt, st) in *steps {
                assert!(get(dt, st).is_some(), "{key} bước {dt}/{st} không có template");
            }
        }
    }

    #[test]
    fn upstream_types_resolve() {
        for t in TEMPLATES {
            for up in t.upstream {
                assert!(
                    TEMPLATES.iter().any(|x| x.doc_type == *up),
                    "{}/{} upstream '{}' không tồn tại",
                    t.doc_type,
                    t.subtype,
                    up
                );
            }
        }
    }

    #[test]
    fn diagram_lookup_needs_subtype_but_single_types_tolerate_empty() {
        assert!(get("diagram", "erd").is_some());
        assert!(get("diagram", "").is_none(), "diagram có nhiều subtype, gọi rỗng phải fail");
        assert!(get("srs", "").is_some());
    }
}
