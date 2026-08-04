//! Pure helpers for the Warehouse app: move kinds, phiếu code prefixes, date
//! helpers and rounding. No I/O here so everything is unit-testable.

/// Kinds a stock move (phiếu kho) can have.
///   * `receipt`  — nhập kho (mua hàng / nhận hàng về)
///   * `issue`    — xuất kho (bán hàng / xuất dùng)
///   * `transfer` — chuyển giữa hai kho (trừ kho đi, cộng kho đến)
///   * `adjust`   — điều chỉnh sau kiểm kê (delta ±, đưa sổ về số đếm thực tế)
pub const MOVE_KINDS: [&str; 4] = ["receipt", "issue", "transfer", "adjust"];

/// Partner kinds: nhà cung cấp / khách hàng / khác.
pub const PARTNER_KINDS: [&str; 3] = ["supplier", "customer", "other"];

pub fn is_move_kind(k: &str) -> bool {
    MOVE_KINDS.contains(&k)
}

/// Mã phiếu theo kiểu chứng từ kho Việt Nam: NK (nhập), XK (xuất),
/// CK (chuyển kho), DC (điều chỉnh).
pub fn code_prefix(kind: &str) -> &'static str {
    match kind {
        "receipt" => "NK",
        "issue" => "XK",
        "transfer" => "CK",
        "adjust" => "DC",
        _ => "PH",
    }
}

pub fn move_code(kind: &str, id: i64) -> String {
    format!("{}-{:04}", code_prefix(kind), id)
}

pub fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0 + 0.0 // +0.0 normalizes -0.0
}

/// Quantities keep 3 decimals (kg, lít…); values keep 2.
pub fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0 + 0.0
}

pub fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_codes_follow_vietnamese_doc_prefixes() {
        assert_eq!(move_code("receipt", 7), "NK-0007");
        assert_eq!(move_code("issue", 12), "XK-0012");
        assert_eq!(move_code("transfer", 3), "CK-0003");
        assert_eq!(move_code("adjust", 12345), "DC-12345");
    }

    #[test]
    fn kind_validation() {
        for k in MOVE_KINDS {
            assert!(is_move_kind(k));
        }
        assert!(!is_move_kind("steal"));
        assert!(!is_move_kind(""));
    }

    #[test]
    fn rounding() {
        assert_eq!(round2(1.005 + 0.0001), 1.01);
        assert_eq!(round2(10.0 / 3.0), 3.33);
        assert_eq!(round3(1.0 / 3.0), 0.333);
    }

    #[test]
    fn today_is_iso_date() {
        let t = today();
        assert_eq!(t.len(), 10);
        assert_eq!(&t[4..5], "-");
    }
}
