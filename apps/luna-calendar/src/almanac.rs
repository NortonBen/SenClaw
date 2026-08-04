//! Vietnamese almanac ("lịch vạn niên" / "xem ngày tốt xấu").
//!
//! Everything here is a *deterministic* function of the Julian day + lunar date:
//! Can-Chi pillars, auspicious/inauspicious hours (giờ hoàng đạo / hắc đạo),
//! whether the day itself is a hoàng-đạo (good) or hắc-đạo (bad) day, the day's
//! Trực (12 officers) and Nhị Thập Bát Tú (28 mansions), its nạp-âm five-element,
//! the "xuất hành" fortune (Lý Thuần Phong) and lucky directions, and the folk
//! taboo days (Nguyệt kỵ, Tam nương). Assembled into `DayInfo` for the API/MCP.
//!
//! Sources: Hồ Ngọc Đức almanac; classic "lịch vạn niên" tables. Verified against
//! a reference almanac page for 7/7/2026 (giờ hoàng đạo Tý/Sửu/Mão/Ngọ/Thân/Dậu).

use serde::Serialize;

use crate::lunar::{jd_from_ymd, solar_term_index, solar_to_lunar, weekday_mon0, TZ_VN};

pub const CAN: [&str; 10] = [
    "Giáp", "Ất", "Bính", "Đinh", "Mậu", "Kỷ", "Canh", "Tân", "Nhâm", "Quý",
];
pub const CHI: [&str; 12] = [
    "Tý", "Sửu", "Dần", "Mão", "Thìn", "Tỵ", "Ngọ", "Mùi", "Thân", "Dậu", "Tuất", "Hợi",
];
/// Full 12-animal names for the year zodiac, in Chi order.
pub const CON_GIAP: [&str; 12] = [
    "Chuột", "Trâu", "Hổ", "Mèo", "Rồng", "Rắn", "Ngựa", "Dê", "Khỉ", "Gà", "Chó", "Lợn",
];
pub const THU: [&str; 7] = [
    "Thứ Hai",
    "Thứ Ba",
    "Thứ Tư",
    "Thứ Năm",
    "Thứ Sáu",
    "Thứ Bảy",
    "Chủ Nhật",
];

/// 24 solar terms, index 0 = 0° ecliptic longitude (Xuân phân).
pub const TIET_KHI: [&str; 24] = [
    "Xuân phân",
    "Thanh minh",
    "Cốc vũ",
    "Lập hạ",
    "Tiểu mãn",
    "Mang chủng",
    "Hạ chí",
    "Tiểu thử",
    "Đại thử",
    "Lập thu",
    "Xử thử",
    "Bạch lộ",
    "Thu phân",
    "Hàn lộ",
    "Sương giáng",
    "Lập đông",
    "Tiểu tuyết",
    "Đại tuyết",
    "Đông chí",
    "Tiểu hàn",
    "Đại hàn",
    "Lập xuân",
    "Vũ thủy",
    "Kinh trập",
];

/// The 12 "trực thần" (day officers), starting from Kiến on the nguyệt-kiến day.
pub const TRUC: [&str; 12] = [
    "Kiến", "Trừ", "Mãn", "Bình", "Định", "Chấp", "Phá", "Nguy", "Thành", "Thâu", "Khai", "Bế",
];

/// The 28 lunar mansions (Nhị Thập Bát Tú), in canonical order (0 = Giác).
pub const TU: [&str; 28] = [
    "Giác", "Cang", "Đê", "Phòng", "Tâm", "Vĩ", "Cơ", "Đẩu", "Ngưu", "Nữ", "Hư", "Nguy", "Thất",
    "Bích", "Khuê", "Lâu", "Vị", "Mão", "Tất", "Chủy", "Sâm", "Tỉnh", "Quỷ", "Liễu", "Tinh",
    "Trương", "Dực", "Chẩn",
];
/// Classical good(true)/bad(false) attribute of each mansion, same index as `TU`.
const TU_TOT: [bool; 28] = [
    true, false, false, true, false, false, true, // Đông – Thanh Long
    true, false, true, false, false, false, true, // Bắc – Huyền Vũ
    true, true, false, true, true, false, true, // Tây – Bạch Hổ
    true, false, false, false, true, true, false, // Nam – Chu Tước
];

/// The 12 gods of the hoàng-đạo/hắc-đạo cycle, in order from Thanh Long.
const GODS: [&str; 12] = [
    "Thanh Long",
    "Minh Đường",
    "Thiên Hình",
    "Chu Tước",
    "Kim Quỹ",
    "Bảo Quang",
    "Bạch Hổ",
    "Ngọc Đường",
    "Thiên Lao",
    "Nguyên Vũ",
    "Tư Mệnh",
    "Câu Trận",
];
/// Positions in `GODS` that are auspicious (hoàng đạo). The rest are hắc đạo.
const GOD_GOOD_POS: [usize; 6] = [0, 1, 4, 5, 7, 10];

/// Per-hour hoàng-đạo bitmap keyed by (day-Chi % 6). Char i (Tý..Hợi) = '1' → the
/// hour is a giờ hoàng đạo. Verified: a Ngọ day → Tý,Sửu,Mão,Ngọ,Thân,Dậu.
const GIO_HD: [&str; 6] = [
    "110100101100", // Tý / Ngọ
    "001101001011", // Sửu / Mùi
    "110011010010", // Dần / Thân
    "101100110100", // Mão / Dậu
    "001011001101", // Thìn / Tuất
    "010010110011", // Tỵ / Hợi
];

/// 30 nạp-âm five-elements (each covers 2 consecutive sexagenary pillars).
const NAP_AM: [(&str, &str); 30] = [
    ("Hải Trung Kim", "Kim"),
    ("Lư Trung Hỏa", "Hỏa"),
    ("Đại Lâm Mộc", "Mộc"),
    ("Lộ Bàng Thổ", "Thổ"),
    ("Kiếm Phong Kim", "Kim"),
    ("Sơn Đầu Hỏa", "Hỏa"),
    ("Giản Hạ Thủy", "Thủy"),
    ("Thành Đầu Thổ", "Thổ"),
    ("Bạch Lạp Kim", "Kim"),
    ("Dương Liễu Mộc", "Mộc"),
    ("Tuyền Trung Thủy", "Thủy"),
    ("Ốc Thượng Thổ", "Thổ"),
    ("Tích Lịch Hỏa", "Hỏa"),
    ("Tùng Bách Mộc", "Mộc"),
    ("Trường Lưu Thủy", "Thủy"),
    ("Sa Trung Kim", "Kim"),
    ("Sơn Hạ Hỏa", "Hỏa"),
    ("Bình Địa Mộc", "Mộc"),
    ("Bích Thượng Thổ", "Thổ"),
    ("Kim Bạch Kim", "Kim"),
    ("Phú Đăng Hỏa", "Hỏa"),
    ("Thiên Hà Thủy", "Thủy"),
    ("Đại Trạch Thổ", "Thổ"),
    ("Thoa Xuyến Kim", "Kim"),
    ("Tang Đố Mộc", "Mộc"),
    ("Đại Khê Thủy", "Thủy"),
    ("Sa Trung Thổ", "Thổ"),
    ("Thiên Thượng Hỏa", "Hỏa"),
    ("Thạch Lựu Mộc", "Mộc"),
    ("Đại Hải Thủy", "Thủy"),
];

/// Xuất-hành fortune (Lý Thuần Phong / Lục Diệu), 6-state cycle by lunar day.
const XUAT_HANH: [(&str, &str); 6] = [
    ("Đại An", "Mọi việc tốt lành, bình an. Cầu tài đi hướng Tây Nam; nhà cửa yên ổn, người đi xa trở về bình yên."),
    ("Tốc Hỷ", "Niềm vui sắp đến. Cầu tài đi hướng Nam; đi việc gặp may, mọi việc hanh thông, người đi sắp về."),
    ("Lưu Niên", "Việc khó thành, cầu tài mờ mịt. Nên đề phòng cãi cọ, kiện tụng nên hoãn; người đi chưa về."),
    ("Xích Khẩu", "Dễ sinh khẩu thiệt, cãi cọ. Đi đường nên cẩn thận, tránh tai nạn, va chạm; cầu tài không thuận."),
    ("Tiểu Cát", "Rất tốt lành. Cầu tài đi hướng Bắc; xuất hành thuận lợi, gặp quý nhân, mọi việc hòa hợp."),
    ("Không Vong", "Việc lớn không thành, cầu tài không có lợi. Ra đi gặp trắc trở; nên tránh xuất hành xa."),
];

/// Đánh giá tổng quan (overall verdict of a day).
#[derive(Serialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    /// Hoàng đạo — good day.
    Tot,
    /// Neutral / trung bình.
    Binh,
    /// Hắc đạo or heavily-taboo — bad day.
    Xau,
}

/// One canh giờ (2-hour block) and whether it's auspicious.
#[derive(Serialize)]
pub struct HourInfo {
    /// Chi name, e.g. "Tý".
    pub chi: String,
    /// Display range, e.g. "23:00 - 00:59".
    pub range: String,
    /// True = giờ hoàng đạo (auspicious), false = giờ hắc đạo.
    pub good: bool,
}

/// Lucky directions for the day (based on the day's Can).
#[derive(Serialize)]
pub struct Directions {
    /// Hỷ Thần (God of Joy) direction.
    pub hy_than: String,
    /// Tài Thần (God of Wealth) direction.
    pub tai_than: String,
}

/// The full almanac for one solar day.
#[derive(Serialize)]
pub struct DayInfo {
    // -- solar --
    pub solar_day: i64,
    pub solar_month: i64,
    pub solar_year: i64,
    /// ISO date string YYYY-MM-DD.
    pub solar_date: String,
    /// Weekday name in Vietnamese.
    pub weekday: String,
    pub jd: i64,

    // -- lunar --
    pub lunar_day: i64,
    pub lunar_month: i64,
    pub lunar_year: i64,
    pub lunar_leap: bool,
    /// e.g. "23/5" or "23/5 (nhuận)".
    pub lunar_date: String,

    // -- can chi --
    pub day_can_chi: String,
    pub month_can_chi: String,
    pub year_can_chi: String,
    /// Year zodiac animal, e.g. "Ngựa".
    pub year_animal: String,

    // -- almanac --
    pub tiet_khi: String,
    pub truc: String,
    pub tu: String,
    /// Whether the 28-mansion is classically good.
    pub tu_good: bool,
    /// Nạp-âm five-element phrase, e.g. "Dương Liễu Mộc".
    pub nap_am: String,
    /// The element word only: Kim | Mộc | Thủy | Hỏa | Thổ.
    pub ngu_hanh: String,

    // -- good / bad day --
    /// The controlling god (e.g. "Tư Mệnh").
    pub day_god: String,
    /// True = ngày hoàng đạo, false = ngày hắc đạo.
    pub hoang_dao: bool,
    pub verdict: Verdict,
    /// Short human summary of the verdict.
    pub verdict_label: String,
    /// Taboo/warning tags, e.g. ["Nguyệt kỵ"].
    pub warnings: Vec<String>,
    /// A one-line "nên làm / nên tránh" advisory.
    pub advice: String,

    // -- hours & directions --
    pub hours: Vec<HourInfo>,
    /// Comma-joined good hours for a quick glance.
    pub good_hours: String,
    pub directions: Directions,

    // -- xuất hành --
    pub xuat_hanh: String,
    pub xuat_hanh_detail: String,
}

fn hour_range(chi_idx: usize) -> String {
    let start = (chi_idx * 2 + 23) % 24;
    let end = (start + 1) % 24;
    format!("{:02}:00 - {:02}:59", start, end)
}

/// Sexagenary index (0 = Giáp Tý … 59) from a Can and Chi index.
fn sexagenary(can: usize, chi: usize) -> usize {
    (0..60)
        .find(|&n| n % 10 == can && n % 12 == chi)
        .unwrap_or(0)
}

/// Compute the whole almanac for a Gregorian date.
pub fn day_info(dd: i64, mm: i64, yy: i64) -> DayInfo {
    let jd = jd_from_ymd(dd, mm, yy);
    let (ld, lm, ly, leap) = solar_to_lunar(dd, mm, yy, TZ_VN);

    // -- Can-Chi pillars --
    let day_can = ((jd + 9) % 10) as usize;
    let day_chi = ((jd + 1) % 12) as usize;
    let year_can = (((ly + 6) % 10 + 10) % 10) as usize;
    let year_chi = (((ly + 8) % 12 + 12) % 12) as usize;
    let month_can = (((ly * 12 + lm + 3) % 10 + 10) % 10) as usize;
    let month_chi = ((lm + 1) % 12) as usize;

    // -- Nạp âm (day) --
    let day_sexa = sexagenary(day_can, day_chi);
    let (nap_am, ngu_hanh) = NAP_AM[day_sexa / 2];

    // -- Trực: Kiến falls on the day whose Chi = nguyệt-kiến (month Chi) --
    let truc_idx = ((day_chi as i64 - month_chi as i64 + 12) % 12) as usize;

    // -- Nhị Thập Bát Tú: follows the thất-chính (7-luminary) weekday rule; the
    //    Sun-luminary mansions {Phòng, Hư, Mão, Tinh} always fall on Sunday. --
    let tu_idx = (((jd + 4) % 28 + 28) % 28) as usize;

    // -- Ngày hoàng đạo / hắc đạo: 12-god cycle anchored by the lunar month --
    let god_base = ((lm - 1).rem_euclid(6) * 2) as i64; // Chi of Thanh Long this month
    let god_idx = ((day_chi as i64 - god_base + 12) % 12) as usize;
    let hoang_dao = GOD_GOOD_POS.contains(&god_idx);
    let day_god = GODS[god_idx];

    // -- Giờ hoàng đạo / hắc đạo --
    let pattern = GIO_HD[day_chi % 6];
    let hours: Vec<HourInfo> = (0..12)
        .map(|h| {
            let good = pattern.as_bytes()[h] == b'1';
            HourInfo {
                chi: CHI[h].to_string(),
                range: hour_range(h),
                good,
            }
        })
        .collect();
    let good_hours = hours
        .iter()
        .filter(|h| h.good)
        .map(|h| h.chi.clone())
        .collect::<Vec<_>>()
        .join(", ");

    // -- Directions by day Can (Hỷ Thần / Tài Thần) --
    let hy_than = match day_can {
        0 | 5 => "Đông Bắc",  // Giáp, Kỷ
        1 | 6 => "Tây Nam",   // Ất, Canh
        2 | 7 => "Tây Bắc",   // Bính, Tân
        3 | 8 => "Chính Nam", // Đinh, Nhâm
        _ => "Đông Nam",      // Mậu, Quý
    };
    let tai_than = match day_can {
        0 | 1 => "Đông Nam",   // Giáp, Ất
        2 | 3 => "Chính Đông", // Bính, Đinh
        4 => "Chính Bắc",      // Mậu
        5 => "Chính Nam",      // Kỷ
        6 | 7 => "Tây Nam",    // Canh, Tân
        8 => "Chính Tây",      // Nhâm
        _ => "Tây Bắc",        // Quý
    };

    // -- Xuất hành (Lý Thuần Phong): 6-state cycle by lunar day --
    let xh_idx = ((ld - 1).rem_euclid(6)) as usize;
    let (xuat_hanh, xuat_hanh_detail) = XUAT_HANH[xh_idx];

    // -- Folk taboo days --
    let mut warnings = Vec::new();
    if [5, 14, 23].contains(&ld) {
        warnings.push("Nguyệt kỵ".to_string()); // mùng 5, 14, 23
    }
    if [3, 7, 13, 18, 22, 27].contains(&ld) {
        warnings.push("Tam nương".to_string());
    }

    // -- Overall verdict --
    let verdict = if hoang_dao {
        if warnings.is_empty() {
            Verdict::Tot
        } else {
            Verdict::Binh
        }
    } else if warnings.is_empty() {
        Verdict::Binh
    } else {
        Verdict::Xau
    };
    let verdict_label = match verdict {
        Verdict::Tot => "Ngày tốt (Hoàng Đạo)".to_string(),
        Verdict::Binh => {
            if hoang_dao {
                "Ngày Hoàng Đạo nhưng phạm ngày kỵ".to_string()
            } else {
                "Ngày bình thường".to_string()
            }
        }
        Verdict::Xau => "Ngày xấu (Hắc Đạo, phạm kỵ)".to_string(),
    };
    let advice = build_advice(hoang_dao, &warnings, &good_hours);

    let leap_note = if leap { " (nhuận)" } else { "" };
    DayInfo {
        solar_day: dd,
        solar_month: mm,
        solar_year: yy,
        solar_date: format!("{:04}-{:02}-{:02}", yy, mm, dd),
        weekday: THU[weekday_mon0(jd) as usize].to_string(),
        jd,
        lunar_day: ld,
        lunar_month: lm,
        lunar_year: ly,
        lunar_leap: leap,
        lunar_date: format!("{}/{}{}", ld, lm, leap_note),
        day_can_chi: format!("{} {}", CAN[day_can], CHI[day_chi]),
        month_can_chi: format!("{} {}", CAN[month_can], CHI[month_chi]),
        year_can_chi: format!("{} {}", CAN[year_can], CHI[year_chi]),
        year_animal: CON_GIAP[year_chi].to_string(),
        tiet_khi: TIET_KHI[solar_term_index(jd, TZ_VN)].to_string(),
        truc: TRUC[truc_idx].to_string(),
        tu: TU[tu_idx].to_string(),
        tu_good: TU_TOT[tu_idx],
        nap_am: nap_am.to_string(),
        ngu_hanh: ngu_hanh.to_string(),
        day_god: day_god.to_string(),
        hoang_dao,
        verdict,
        verdict_label,
        warnings,
        advice,
        hours,
        good_hours,
        directions: Directions {
            hy_than: hy_than.to_string(),
            tai_than: tai_than.to_string(),
        },
        xuat_hanh: xuat_hanh.to_string(),
        xuat_hanh_detail: xuat_hanh_detail.to_string(),
    }
}

fn build_advice(hoang_dao: bool, warnings: &[String], good_hours: &str) -> String {
    let mut s = String::new();
    if hoang_dao {
        s.push_str("Ngày Hoàng Đạo — thuận cho các việc trọng đại (cưới hỏi, khai trương, xuất hành, động thổ). ");
    } else {
        s.push_str(
            "Ngày Hắc Đạo — nên thận trọng, tránh khởi sự việc lớn; ưu tiên việc thường ngày. ",
        );
    }
    if !warnings.is_empty() {
        s.push_str(&format!("Lưu ý phạm: {}. ", warnings.join(", ")));
    }
    if !good_hours.is_empty() {
        s.push_str(&format!("Chọn giờ Hoàng Đạo để hành sự: {}.", good_hours));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_day_7_7_2026() {
        let d = day_info(7, 7, 2026);
        assert_eq!(d.lunar_date, "23/5");
        assert_eq!(d.day_can_chi, "Nhâm Ngọ");
        assert_eq!(d.month_can_chi, "Giáp Ngọ");
        assert_eq!(d.year_can_chi, "Bính Ngọ");
        assert_eq!(d.weekday, "Thứ Ba");
        // Giờ hoàng đạo verified against the almanac image.
        assert_eq!(d.good_hours, "Tý, Sửu, Mão, Ngọ, Thân, Dậu");
        // Lunar day 23 → Nguyệt kỵ.
        assert!(d.warnings.contains(&"Nguyệt kỵ".to_string()));
    }

    #[test]
    fn hoang_dao_hours_are_six() {
        let d = day_info(7, 7, 2026);
        assert_eq!(d.hours.iter().filter(|h| h.good).count(), 6);
        assert_eq!(d.hours[0].range, "23:00 - 00:59"); // Tý
        assert_eq!(d.hours[2].range, "03:00 - 04:59"); // Dần
    }

    #[test]
    fn tu_luminary_rule_holds() {
        // Every Sunday must land on a Sun-luminary mansion: Phòng, Hư, Mão, Tinh.
        let sun_tu = ["Phòng", "Hư", "Mão", "Tinh"];
        for off in 0..28 {
            let jd = jd_from_ymd(1, 1, 2024) + off;
            let (dd, mm, yy) = crate::lunar::jd_to_ymd(jd);
            let d = day_info(dd, mm, yy);
            if d.weekday == "Chủ Nhật" {
                assert!(sun_tu.contains(&d.tu.as_str()), "{} on Sunday", d.tu);
            }
        }
    }
}
