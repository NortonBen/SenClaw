//! Pure helpers cho app Tư Duy: hằng số 5W / 6 mũ, điểm tổng hợp của một giải
//! pháp và độ hoàn thiện của một phân tích. Không I/O — mọi thứ test được
//! độc lập với DB và AI.

/// Thứ tự chuẩn của 5 chữ W. Mọi nơi (DB, API, MCP, UI) dùng đúng các key này.
pub const W_KEYS: [&str; 5] = ["who", "what", "when", "where", "why"];

/// Thứ tự chuẩn của 6 mũ tư duy (trình bày theo trình tự chạy một phiên
/// de Bono điển hình: dữ kiện → cảm xúc → rủi ro → lợi ích → sáng tạo → tổng kết).
pub const HAT_KEYS: [&str; 6] = ["white", "red", "black", "yellow", "green", "blue"];

pub fn w_label(w: &str) -> &'static str {
    match w {
        "who" => "Who — Ai liên quan",
        "what" => "What — Vấn đề là gì",
        "when" => "When — Khi nào xảy ra",
        "where" => "Where — Xảy ra ở đâu",
        "why" => "Why — Tại sao xảy ra",
        _ => "?",
    }
}

pub fn hat_label(hat: &str) -> &'static str {
    match hat {
        "white" => "⚪ Mũ Trắng — Dữ kiện & số liệu",
        "red" => "🔴 Mũ Đỏ — Cảm xúc & trực giác",
        "black" => "⚫ Mũ Đen — Rủi ro & phản biện",
        "yellow" => "🟡 Mũ Vàng — Lợi ích & giá trị",
        "green" => "🟢 Mũ Xanh Lá — Sáng tạo & lựa chọn mới",
        "blue" => "🔵 Mũ Xanh Dương — Điều phối & tổng kết",
        _ => "?",
    }
}

pub fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

/// Kẹp một điểm thành 0..=10 (điểm nhập tay hoặc do AI trả về).
pub fn clamp10(v: f64) -> f64 {
    if v.is_nan() {
        return 0.0;
    }
    round1(v.clamp(0.0, 10.0))
}

/// Điểm tổng hợp 0..=100 của một giải pháp từ bốn tiêu chí 0..=10:
///   * `benefit`     — lợi ích (mũ Vàng), cao là tốt
///   * `risk`        — rủi ro (mũ Đen), cao là XẤU
///   * `feasibility` — tính khả thi, cao là tốt
///   * `effort`      — công sức/chi phí, cao là XẤU
/// Trọng số: lợi ích 35% · an toàn (10−risk) 30% · khả thi 25% · nhẹ công 10%.
pub fn overall_score(benefit: f64, risk: f64, feasibility: f64, effort: f64) -> f64 {
    let b = clamp10(benefit);
    let r = clamp10(risk);
    let f = clamp10(feasibility);
    let e = clamp10(effort);
    round1((0.35 * b + 0.30 * (10.0 - r) + 0.25 * f + 0.10 * (10.0 - e)) * 10.0)
}

/// Độ hoàn thiện phân tích 0..=100: 5W chiếm 40 điểm, 6 mũ chiếm 60 điểm.
pub fn completeness_pct(w_filled: usize, hats_filled: usize) -> i64 {
    let w = (w_filled.min(5) as f64) / 5.0 * 40.0;
    let h = (hats_filled.min(6) as f64) / 6.0 * 60.0;
    (w + h).round() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_bounds() {
        // Giải pháp hoàn hảo: lợi ích 10, rủi ro 0, khả thi 10, không tốn công.
        assert_eq!(overall_score(10.0, 0.0, 10.0, 0.0), 100.0);
        // Tệ nhất: không lợi ích, rủi ro tối đa, bất khả thi, cực tốn công.
        assert_eq!(overall_score(0.0, 10.0, 0.0, 10.0), 0.0);
        // Trung tính 5/5/5/5 → đúng 50.
        assert_eq!(overall_score(5.0, 5.0, 5.0, 5.0), 50.0);
    }

    #[test]
    fn score_clamps_wild_inputs() {
        assert_eq!(overall_score(999.0, -5.0, 20.0, f64::NAN), 100.0);
    }

    #[test]
    fn completeness_progression() {
        assert_eq!(completeness_pct(0, 0), 0);
        assert_eq!(completeness_pct(5, 0), 40);
        assert_eq!(completeness_pct(0, 6), 60);
        assert_eq!(completeness_pct(5, 6), 100);
        assert_eq!(completeness_pct(3, 3), 54); // 24 + 30
                                                // Không vượt trần khi dữ liệu thừa.
        assert_eq!(completeness_pct(9, 9), 100);
    }

    #[test]
    fn labels_cover_all_keys() {
        for w in W_KEYS {
            assert_ne!(w_label(w), "?");
        }
        for h in HAT_KEYS {
            assert_ne!(hat_label(h), "?");
        }
    }
}
