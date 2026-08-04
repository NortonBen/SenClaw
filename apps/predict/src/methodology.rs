//! Nền tảng tri thức đánh giá — phương pháp luận **Siêu Dự Báo**
//! (Superforecasting, Philip E. Tetlock & Dan Gardner, 2015; bản Việt "Siêu
//! Dự Báo"). Mã hoá 11 "điều răn" cho siêu dự báo viên + các kỹ thuật lõi để
//! (a) hiển thị trong tab Tri thức / tool `predict_method` và (b) bơm vào
//! prompt tổng hợp dự đoán như một checklist bắt buộc. Nội dung là tri thức
//! nền tĩnh của app — không phụ thuộc LLM.

use serde_json::{json, Value};

/// (key, tên, tóm tắt áp dụng). Theo "Ten Commandments for Aspiring
/// Superforecasters" — phụ lục sách, kèm điều răn thứ 11.
pub const PRINCIPLES: &[(&str, &str, &str)] = &[
    (
        "triage",
        "1. Chọn lọc câu hỏi (Triage)",
        "Dồn công sức vào câu hỏi 'vùng Goldilocks' — không quá dễ (đoán ai cũng đúng) cũng không bất khả \
         (ngẫu nhiên thuần túy, ví dụ kết quả xổ số). Với câu hỏi bất khả, câu trả lời trung thực là base rate.",
    ),
    (
        "fermi",
        "2. Phân rã vấn đề (Fermi-ize)",
        "Bẻ câu hỏi lớn tưởng như không trả lời được thành các câu hỏi con trả lời được; ước lượng từng phần \
         rồi ghép lại. 'X có xảy ra không?' = những điều kiện nào phải cùng đúng để X xảy ra?",
    ),
    (
        "outside-inside",
        "3. Góc nhìn ngoài trước, trong sau (Outside → Inside view)",
        "LUÔN bắt đầu bằng base rate: trong các tình huống cùng loại, việc này xảy ra bao nhiêu % (góc nhìn \
         ngoài)? Rồi mới điều chỉnh theo đặc thù của trường hợp cụ thể (góc nhìn trong). Bắt đầu từ chi tiết \
         cụ thể trước là mắc bẫy neo sai.",
    ),
    (
        "update",
        "4. Cập nhật đúng liều (Under/overreacting to evidence)",
        "Cập nhật dự đoán THƯỜNG XUYÊN nhưng theo bước NHỎ khi có tin mới; tránh cả hai lỗi — bỏ qua bằng \
         chứng mới (bảo thủ) lẫn nhảy dựng theo tin giật gân (phản ứng thái quá). Bằng chứng càng chẩn đoán \
         (diagnostic) thì bước cập nhật càng lớn.",
    ),
    (
        "dragonfly",
        "5. Mắt chuồn chuồn (Dragonfly eye)",
        "Chủ động tìm các lực nhân quả NGƯỢC CHIỀU trong cùng vấn đề và các góc nhìn khác (kể cả góc nhìn \
         mình không thích). Tổng hợp nhiều góc nhìn cho dự đoán tốt hơn bất kỳ góc nhìn đơn lẻ nào.",
    ),
    (
        "granularity",
        "6. Chia độ chắc chắn thật mịn (Granularity)",
        "Phân biệt càng nhiều mức độ nghi ngờ càng tốt trong giới hạn bài toán cho phép: 63% khác 60% khác \
         70%. Siêu dự báo viên dùng thang xác suất mịn và điều đó đo được qua Brier. Tránh mặc định 50-50 \
         khi thực ra có thông tin.",
    ),
    (
        "confidence",
        "7. Cân bằng thận trọng – quyết đoán (Under/overconfidence)",
        "Đừng vống tự tin (khẳng định chắc nịch) cũng đừng núp mãi ở 'có thể'. Sai lầm đắt nhất là 0% và \
         100% — thực tế hiếm khi cho phép hai giá trị đó.",
    ),
    (
        "postmortem",
        "8. Mổ xẻ sai lầm, cảnh giác hindsight (Error postmortems)",
        "Khi dự đoán được chấm điểm, xem lại QUY TRÌNH chứ không chỉ kết quả: đúng nhờ may hay nhờ phương \
         pháp? Sai ở khâu nào — base rate, bằng chứng, hay cập nhật? Cảnh giác thiên kiến nhìn lại ('biết \
         ngay mà').",
    ),
    (
        "team",
        "9. Sức mạnh nhóm (Perspective from others)",
        "Khai thác góc nhìn người khác và để người khác phản biện mình — tranh luận đúng cách làm dự đoán \
         chính xác hơn. Trong app: quy luật user thêm tay + quy luật AI rút + bằng chứng search là các 'thành \
         viên nhóm' phải được đối chiếu nhau.",
    ),
    (
        "practice",
        "10. Luyện tập có phản hồi (Error-balancing bicycle)",
        "Kỹ năng dự báo chỉ lên nhờ LÀM thật — dự đoán, ghi sổ, chấm điểm, rút bài học, lặp lại. Đây chính \
         là lý do mọi dự đoán trong app đều vào Sổ và tự chấm Brier.",
    ),
    (
        "no-dogma",
        "11. Đừng coi điều răn là giáo điều",
        "Các nguyên tắc trên là hướng dẫn, không phải luật cứng — biết khi nào phá lệ cũng là một kỹ năng \
         của siêu dự báo viên.",
    ),
];

/// Kỹ thuật thao tác cụ thể dùng trong pipeline tổng hợp.
pub const TECHNIQUES: &[(&str, &str)] = &[
    ("premortem", "Trước khi chốt: giả định dự đoán ĐÃ SAI, tự hỏi 'vì sao sai?' — nếu tìm ra lý do mạnh, điều chỉnh lại xác suất."),
    ("base-rate", "Base rate lấy từ dữ liệu lịch sử của chính chủ đề khi có (tần suất sự kiện tương tự), thiếu thì từ lớp tình huống tương đương."),
    ("brier", "Brier = tổng (p − kết quả)². 0 là hoàn hảo, 2 là sai tuyệt đối. Chấm cả chuỗi, không chấm một phát."),
    ("calibration", "Hiệu chuẩn: nhóm các dự đoán 70% phải đúng ~70%. Lệch lên = vống tự tin, lệch xuống = quá rụt rè."),
    ("fate", "Câu hỏi số phận/ngẫu nhiên thuần túy (xổ số!) không thể dự báo — trả lời bằng base rate và nói thẳng như vậy."),
];

pub const DEFAULT_SOURCE: &str =
    "Superforecasting — The Art and Science of Prediction (Philip E. Tetlock & Dan Gardner, 2015). Bản Việt: 'Siêu Dự Báo'.";

pub const DEFAULT_PIPELINE: &[&str] = &[
    "1. Phân rã câu hỏi (Fermi) thành câu hỏi con + truy vấn thông tin cần tìm",
    "2. Nền tảng dữ liệu: thống kê chủ đề + quy luật + bài học + track record",
    "3. Thu thập bằng chứng ngoài: gọi Search app (news/web/knowledge) tổng hợp thông tin",
    "4. Tổng hợp: outside view (base rate) → inside view (bằng chứng thuận/nghịch) → điều chỉnh từng bước → premortem → chốt p mịn + độ tin cậy + điều kiện cập nhật",
    "5. Ghi sổ, tự chấm Brier khi có kết quả, rút bài học (postmortem) quay lại tri thức chủ đề",
];

/// Checklist mặc định bơm vào prompt tổng hợp.
pub const DEFAULT_CHECKLIST: &str = "PHƯƠNG PHÁP SIÊU DỰ BÁO (Tetlock) — BẮT BUỘC TUÂN THỦ:\n\
     (1) Bắt đầu từ OUTSIDE VIEW: base rate từ dữ liệu lịch sử/lớp tình huống tương đương, nêu rõ nguồn base rate.\n\
     (2) Rồi mới INSIDE VIEW: liệt kê bằng chứng THUẬN và NGHỊCH riêng rẽ (mắt chuồn chuồn — chủ động tìm chiều ngược).\n\
     (3) Điều chỉnh từ base rate theo TỪNG bằng chứng, bước nhỏ, ghi rõ mỗi bước cộng/trừ bao nhiêu và vì sao.\n\
     (4) PREMORTEM: giả định dự đoán đã sai, nêu lý do khả dĩ nhất; nếu lý do mạnh thì điều chỉnh thêm.\n\
     (5) Chốt p MỊN (không tròn về 0.5/0/1; 0.63 hợp lệ), kèm độ tin cậy thấp/vừa/cao theo lượng & chất bằng chứng.\n\
     (6) Nêu 2-3 ĐIỀU KIỆN CẬP NHẬT: tin gì xuất hiện thì phải sửa p theo hướng nào.\n\
     (7) Sự kiện ngẫu nhiên thuần túy → trả về đúng base rate và nói thẳng là không thể dự báo hơn.\n\
     (8) Chỉ dùng dữ liệu/bằng chứng ĐƯỢC CUNG CẤP, không bịa sự kiện.";

/// Tri thức MẶC ĐỊNH (seed từ sách) — dùng khi user chưa sửa gì.
pub fn default_methodology() -> Value {
    json!({
        "source": DEFAULT_SOURCE,
        "principles": PRINCIPLES.iter().map(|(k, t, b)| json!({ "key": k, "title": t, "body": b })).collect::<Vec<_>>(),
        "techniques": TECHNIQUES.iter().map(|(k, b)| json!({ "key": k, "body": b })).collect::<Vec<_>>(),
        "pipeline": DEFAULT_PIPELINE,
        "checklist": DEFAULT_CHECKLIST,
    })
}

/// Chuẩn hoá tri thức do người dùng gửi lên: giữ đúng shape, bỏ mục rỗng,
/// thiếu phần nào thì lấy lại phần mặc định (không bao giờ để tri thức rỗng).
pub fn normalize(v: &Value) -> Value {
    let d = default_methodology();
    let str_or = |val: &Value, fallback: &str| -> String {
        val.as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(fallback)
            .to_string()
    };
    let principles: Vec<Value> = v["principles"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|p| {
                    let title = p["title"].as_str()?.trim();
                    let body = p["body"].as_str().unwrap_or("").trim();
                    if title.is_empty() {
                        return None;
                    }
                    Some(json!({
                        "key": p["key"].as_str().unwrap_or("custom"),
                        "title": title, "body": body,
                    }))
                })
                .collect()
        })
        .unwrap_or_default();
    let techniques: Vec<Value> = v["techniques"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|t| {
                    let body = t["body"].as_str()?.trim();
                    if body.is_empty() {
                        return None;
                    }
                    Some(json!({ "key": t["key"].as_str().unwrap_or("custom"), "body": body }))
                })
                .collect()
        })
        .unwrap_or_default();
    let pipeline: Vec<Value> = v["pipeline"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(str::trim))
                .filter(|s| !s.is_empty())
                .map(|s| json!(s))
                .collect()
        })
        .unwrap_or_default();
    json!({
        "source": str_or(&v["source"], DEFAULT_SOURCE),
        "principles": if principles.is_empty() { d["principles"].clone() } else { json!(principles) },
        "techniques": if techniques.is_empty() { d["techniques"].clone() } else { json!(techniques) },
        "pipeline": if pipeline.is_empty() { d["pipeline"].clone() } else { json!(pipeline) },
        "checklist": str_or(&v["checklist"], DEFAULT_CHECKLIST),
    })
}

/// Tri thức đang dùng: bản user đã sửa (nếu có) hoặc mặc định.
pub fn methodology_json(db: &crate::db::Db) -> Value {
    let mut v = match db
        .get_setting("methodology")
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
    {
        Some(saved) => {
            let mut n = normalize(&saved);
            n["customized"] = json!(true);
            n
        }
        None => {
            let mut d = default_methodology();
            d["customized"] = json!(false);
            d
        }
    };
    v["default_available"] = json!(true);
    v
}

/// Khối chỉ dẫn bơm vào prompt tổng hợp — lấy checklist đang dùng.
pub fn methodology_prompt(db: &crate::db::Db) -> String {
    methodology_json(db)["checklist"]
        .as_str()
        .unwrap_or(DEFAULT_CHECKLIST)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knowledge_complete() {
        assert_eq!(PRINCIPLES.len(), 11);
        assert_eq!(TECHNIQUES.len(), 5);
        for (k, t, b) in PRINCIPLES {
            assert!(
                !k.is_empty() && !t.is_empty() && b.len() > 40,
                "principle {k} too thin"
            );
        }
        let v = default_methodology();
        assert_eq!(v["principles"].as_array().unwrap().len(), 11);
        assert!(v["source"].as_str().unwrap().contains("Tetlock"));
        assert_eq!(v["pipeline"].as_array().unwrap().len(), 5);
    }

    #[test]
    fn prompt_has_checklist() {
        for needle in [
            "OUTSIDE VIEW",
            "INSIDE VIEW",
            "PREMORTEM",
            "base rate",
            "ĐIỀU KIỆN CẬP NHẬT",
            "không bịa",
        ] {
            assert!(DEFAULT_CHECKLIST.contains(needle), "missing {needle}");
        }
    }

    #[test]
    fn override_and_reset() {
        let db = crate::db::Db::open_memory().unwrap();
        // Chưa sửa → mặc định.
        let base = methodology_json(&db);
        assert_eq!(base["customized"], false);
        assert_eq!(base["principles"].as_array().unwrap().len(), 11);
        assert_eq!(methodology_prompt(&db), DEFAULT_CHECKLIST);

        // Sửa: thay checklist + rút gọn nguyên tắc.
        let custom = json!({
            "source": "Tri thức nội bộ v2",
            "principles": [{ "key": "a", "title": "Luôn hỏi base rate", "body": "…" }, { "title": "", "body": "bỏ vì thiếu title" }],
            "techniques": [],
            "pipeline": ["Bước riêng của tôi"],
            "checklist": "CHECKLIST RIÊNG: luôn nêu base rate.",
        });
        db.set_setting("methodology", &normalize(&custom).to_string())
            .unwrap();
        let now = methodology_json(&db);
        assert_eq!(now["customized"], true);
        assert_eq!(now["source"], "Tri thức nội bộ v2");
        assert_eq!(now["principles"].as_array().unwrap().len(), 1); // mục rỗng bị loại
        assert_eq!(now["pipeline"].as_array().unwrap().len(), 1);
        // techniques rỗng → giữ mặc định (không bao giờ để tri thức trống)
        assert_eq!(now["techniques"].as_array().unwrap().len(), 5);
        assert!(methodology_prompt(&db).contains("CHECKLIST RIÊNG"));

        // Reset.
        db.set_setting("methodology", "").unwrap();
        assert_eq!(methodology_json(&db)["customized"], false);
    }
}
