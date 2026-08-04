//! Pure helpers cho app Quán Cafe: làm tròn, quy đổi đơn vị (g/kg, ml/lít),
//! mã chứng từ, bỏ dấu tiếng Việt để tìm kiếm, và bộ dự đoán lượng bán theo
//! trung bình cùng-thứ. Không I/O — mọi thứ ở đây test được không cần DB.

/// Đơn vị gốc hợp lệ của nguyên liệu (tồn kho + công thức luôn tính theo gốc).
pub const BASE_UNITS: [&str; 3] = ["g", "ml", "cái"];

pub fn round2(v: f64) -> f64 {
    let r = (v * 100.0).round() / 100.0;
    // -0.0 lọt ra JSON sẽ hiển thị "-0 đ" trên UI — chuẩn hoá về 0.0.
    if r == 0.0 { 0.0 } else { r }
}

pub fn round3(v: f64) -> f64 {
    let r = (v * 1000.0).round() / 1000.0;
    if r == 0.0 { 0.0 } else { r }
}

pub fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// Cộng `days` (âm được) vào một ngày `YYYY-MM-DD`; input hỏng thì trả nguyên.
pub fn date_add(date: &str, days: i64) -> String {
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.checked_add_signed(chrono::Duration::days(days)))
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| date.to_string())
}

/// Mã chứng từ: NH-0001 (phiếu nhập), BH-0001 (đơn bán).
pub fn doc_code(prefix: &str, id: i64) -> String {
    format!("{prefix}-{id:04}")
}

/// Hệ số quy đổi từ đơn vị người dùng khai sang đơn vị gốc của nguyên liệu.
/// `None` = đơn vị không tương thích (vd. nhập "kg" cho nguyên liệu gốc "ml").
pub fn unit_factor(input_unit: &str, base_unit: &str) -> Option<f64> {
    let u = input_unit.trim().to_lowercase();
    match (u.as_str(), base_unit) {
        ("g", "g") | ("ml", "ml") => Some(1.0),
        ("cái", "cái") | ("cai", "cái") => Some(1.0),
        ("kg", "g") => Some(1000.0),
        ("l", "ml") | ("lít", "ml") | ("lit", "ml") => Some(1000.0),
        _ => None,
    }
}

/// Hiển thị số lượng theo đơn vị gốc, tự nâng lên kg/lít khi đủ lớn cho dễ đọc.
pub fn qty_display(qty: f64, base_unit: &str) -> String {
    let a = qty.abs();
    match base_unit {
        "g" if a >= 1000.0 => format!("{} kg", round3(qty / 1000.0)),
        "ml" if a >= 1000.0 => format!("{} lít", round3(qty / 1000.0)),
        _ => format!("{} {base_unit}", round3(qty)),
    }
}

/// Bỏ dấu tiếng Việt + thường hoá để so khớp tìm kiếm ("ca phe" khớp "Cà Phê").
/// unicode61 của FTS không xử lý đ/Đ (chữ riêng, không phải dấu) nên fold tay cả bộ.
pub fn fold_vi(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'à' | 'á' | 'ả' | 'ã' | 'ạ' | 'ă' | 'ằ' | 'ắ' | 'ẳ' | 'ẵ' | 'ặ' | 'â' | 'ầ'
            | 'ấ' | 'ẩ' | 'ẫ' | 'ậ' => 'a',
            'è' | 'é' | 'ẻ' | 'ẽ' | 'ẹ' | 'ê' | 'ề' | 'ế' | 'ể' | 'ễ' | 'ệ' => 'e',
            'ì' | 'í' | 'ỉ' | 'ĩ' | 'ị' => 'i',
            'ò' | 'ó' | 'ỏ' | 'õ' | 'ọ' | 'ô' | 'ồ' | 'ố' | 'ổ' | 'ỗ' | 'ộ' | 'ơ' | 'ờ'
            | 'ớ' | 'ở' | 'ỡ' | 'ợ' => 'o',
            'ù' | 'ú' | 'ủ' | 'ũ' | 'ụ' | 'ư' | 'ừ' | 'ứ' | 'ử' | 'ữ' | 'ự' => 'u',
            'ỳ' | 'ý' | 'ỷ' | 'ỹ' | 'ỵ' => 'y',
            'đ' => 'd',
            c => c,
        })
        .collect()
}

/// Cắt chuỗi tối đa `max_bytes` byte, lùi về biên ký tự UTF-8 gần nhất.
/// `&s[..n]` panic khi n rơi giữa ký tự đa byte — tiếng Việt dính liên tục.
pub fn truncate_on_char_boundary(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Dự báo `days` ngày kế tiếp từ chuỗi ngày LIÊN TỤC `history` (phần tử cuối là
/// ngày gần nhất, đã điền 0 cho ngày không phát sinh). Mỗi ngày tương lai lấy
/// trung bình các mẫu CÙNG THỨ trong 4 tuần gần nhất; dưới 2 mẫu thì rơi về
/// trung bình 14 ngày cuối. `history` rỗng → toàn 0.
pub fn forecast_series(history: &[f64], days: usize) -> Vec<f64> {
    if history.is_empty() || days == 0 {
        return vec![0.0; days];
    }
    let n = history.len();
    let recent = &history[n.saturating_sub(14)..];
    let fallback = recent.iter().sum::<f64>() / recent.len() as f64;
    (0..days)
        .map(|i| {
            // Vị trí ảo của ngày tương lai trên trục lịch sử → lùi 7/14/21/28
            // là đúng ô cùng thứ trong tuần.
            let virtual_idx = n + i;
            let mut samples = Vec::new();
            for w in 1..=4usize {
                let back = w * 7;
                if virtual_idx >= back && virtual_idx - back < n {
                    samples.push(history[virtual_idx - back]);
                }
            }
            let v = if samples.len() >= 2 {
                samples.iter().sum::<f64>() / samples.len() as f64
            } else {
                fallback
            };
            round3(v.max(0.0))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_factor_conversions() {
        assert_eq!(unit_factor("g", "g"), Some(1.0));
        assert_eq!(unit_factor("kg", "g"), Some(1000.0));
        assert_eq!(unit_factor("KG", "g"), Some(1000.0));
        assert_eq!(unit_factor("ml", "ml"), Some(1.0));
        assert_eq!(unit_factor("l", "ml"), Some(1000.0));
        assert_eq!(unit_factor("lít", "ml"), Some(1000.0));
        assert_eq!(unit_factor("cái", "cái"), Some(1.0));
        // Không tương thích: kg cho nguyên liệu ml, hay đơn vị lạ.
        assert_eq!(unit_factor("kg", "ml"), None);
        assert_eq!(unit_factor("thùng", "cái"), None);
    }

    #[test]
    fn qty_display_scales_up() {
        assert_eq!(qty_display(2500.0, "g"), "2.5 kg");
        assert_eq!(qty_display(500.0, "g"), "500 g");
        assert_eq!(qty_display(1200.0, "ml"), "1.2 lít");
        assert_eq!(qty_display(3.0, "cái"), "3 cái");
    }

    #[test]
    fn fold_vi_strips_marks_and_d() {
        assert_eq!(fold_vi("Cà Phê Sữa Đá"), "ca phe sua da");
        assert_eq!(fold_vi("đường"), "duong");
        assert_eq!(fold_vi("Trà Đào Cam Sả"), "tra dao cam sa");
    }

    #[test]
    fn truncate_respects_utf8() {
        let s = "cà phê sữa";
        let t = truncate_on_char_boundary(s, 4);
        assert!(s.starts_with(t));
        assert!(t.len() <= 4);
        assert_eq!(truncate_on_char_boundary("abc", 10), "abc");
    }

    #[test]
    fn date_add_works() {
        assert_eq!(date_add("2026-07-31", 1), "2026-08-01");
        assert_eq!(date_add("2026-07-31", -31), "2026-06-30");
        assert_eq!(date_add("hỏng", 5), "hỏng");
    }

    #[test]
    fn forecast_uses_same_weekday_average() {
        // 28 ngày: thứ có chỉ số i%7==0 bán 70, còn lại bán 7.
        let hist: Vec<f64> = (0..28).map(|i| if i % 7 == 0 { 70.0 } else { 7.0 }).collect();
        let f = forecast_series(&hist, 7);
        // Ngày tương lai đầu tiên (virtual 28) cùng thứ với các ô 0/7/14/21 → 70.
        assert_eq!(f[0], 70.0);
        for v in &f[1..] {
            assert_eq!(*v, 7.0);
        }
    }

    #[test]
    fn forecast_falls_back_on_short_history() {
        // 5 ngày lịch sử — không đủ mẫu cùng thứ → trung bình 5 ngày cuối.
        let hist = vec![10.0, 10.0, 10.0, 10.0, 10.0];
        let f = forecast_series(&hist, 3);
        assert_eq!(f, vec![10.0, 10.0, 10.0]);
        assert_eq!(forecast_series(&[], 3), vec![0.0, 0.0, 0.0]);
    }

    #[test]
    fn doc_code_format() {
        assert_eq!(doc_code("NH", 7), "NH-0007");
        assert_eq!(doc_code("BH", 1234), "BH-1234");
    }
}
