//! Pure financial math for the Capital app: payment-schedule generation
//! (annuity / equal-principal / interest-only) and month arithmetic on
//! `YYYY-MM-DD` strings. No I/O here so everything is unit-testable.

use chrono::{Datelike, NaiveDate};

/// Round to 2 decimals — all money in this app is stored as REAL and rounded
/// at every arithmetic step so schedules sum exactly to the principal.
pub fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// Source kinds that count as debt (dư nợ, lãi vay). Everything else —
/// equity / investor / grant / other — is treated as owner-side capital.
pub fn is_debt_kind(kind: &str) -> bool {
    matches!(kind, "bank_loan" | "credit_line" | "personal_loan" | "bond")
}

pub const SOURCE_KINDS: [&str; 8] = [
    "equity",        // vốn chủ sở hữu
    "investor",      // vốn góp nhà đầu tư
    "bank_loan",     // vay ngân hàng
    "credit_line",   // hạn mức tín dụng
    "personal_loan", // vay cá nhân
    "bond",          // trái phiếu
    "grant",         // tài trợ / vốn không hoàn lại
    "other",
];

pub const TX_KINDS: [&str; 4] = ["disburse", "repay_principal", "repay_interest", "fee"];

pub fn today() -> String {
    chrono::Local::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string()
}

/// Add `months` to a `YYYY-MM-DD` date, clamping the day to the target
/// month's last day (2026-01-31 + 1 tháng → 2026-02-28).
pub fn add_months(date: &str, months: u32) -> String {
    let d = NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .unwrap_or_else(|_| chrono::Local::now().date_naive());
    let total = d.year() * 12 + d.month0() as i32 + months as i32;
    let (y, m0) = (total.div_euclid(12), total.rem_euclid(12));
    let m = m0 as u32 + 1;
    let last = last_day_of_month(y, m);
    NaiveDate::from_ymd_opt(y, m, d.day().min(last))
        .expect("valid clamped date")
        .format("%Y-%m-%d")
        .to_string()
}

fn last_day_of_month(y: i32, m: u32) -> u32 {
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    NaiveDate::from_ymd_opt(ny, nm, 1)
        .and_then(|d| d.pred_opt())
        .map(|d| d.day())
        .unwrap_or(28)
}

#[derive(Debug, Clone)]
pub struct Installment {
    pub seq: i64,
    pub due_date: String,
    pub principal: f64,
    pub interest: f64,
}

/// Generate a repayment schedule.
///
/// * `method` — `annuity` (niên kim cố định), `equal_principal` (gốc chia đều),
///   `interest_only` (trả lãi định kỳ, gốc trả cuối kỳ). Unknown → `annuity`.
/// * `annual_rate_pct` — %/năm (e.g. 9.5).
/// * `freq_months` — 1 = hằng tháng, 3 = hằng quý, 6, 12…
/// * `start_date` — first installment falls one period AFTER this date.
///
/// The last installment absorbs rounding so Σprincipal == principal exactly.
pub fn generate_schedule(
    method: &str,
    principal: f64,
    annual_rate_pct: f64,
    periods: u32,
    start_date: &str,
    freq_months: u32,
) -> Vec<Installment> {
    let n = periods.max(1);
    let freq = freq_months.max(1);
    let r = annual_rate_pct.max(0.0) / 100.0 * freq as f64 / 12.0;
    let principal = round2(principal.max(0.0));
    let mut out = Vec::with_capacity(n as usize);
    let mut remaining = principal;

    match method {
        "interest_only" => {
            for k in 1..=n {
                let p = if k == n { remaining } else { 0.0 };
                out.push(Installment {
                    seq: k as i64,
                    due_date: add_months(start_date, freq * k),
                    principal: round2(p),
                    interest: round2(principal * r),
                });
            }
        }
        "equal_principal" => {
            let per = round2(principal / n as f64);
            for k in 1..=n {
                let interest = round2(remaining * r);
                let p = if k == n {
                    remaining
                } else {
                    per.min(remaining)
                };
                remaining = round2(remaining - p);
                out.push(Installment {
                    seq: k as i64,
                    due_date: add_months(start_date, freq * k),
                    principal: round2(p),
                    interest,
                });
            }
        }
        _ => {
            // annuity: A = P·r / (1 − (1+r)^−n); zero-rate degenerates to P/n.
            let pay = if r > 0.0 {
                principal * r / (1.0 - (1.0 + r).powi(-(n as i32)))
            } else {
                principal / n as f64
            };
            for k in 1..=n {
                let interest = round2(remaining * r);
                let p = if k == n {
                    remaining
                } else {
                    round2(pay - interest).clamp(0.0, remaining)
                };
                remaining = round2(remaining - p);
                out.push(Installment {
                    seq: k as i64,
                    due_date: add_months(start_date, freq * k),
                    principal: round2(p),
                    interest,
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn total_principal(v: &[Installment]) -> f64 {
        round2(v.iter().map(|i| i.principal).sum())
    }

    #[test]
    fn add_months_clamps_short_months() {
        assert_eq!(add_months("2026-01-31", 1), "2026-02-28");
        assert_eq!(add_months("2028-01-31", 1), "2028-02-29"); // leap year
        assert_eq!(add_months("2026-03-31", 1), "2026-04-30");
        assert_eq!(add_months("2026-11-15", 2), "2027-01-15"); // year rollover
    }

    #[test]
    fn annuity_sums_to_principal() {
        let s = generate_schedule("annuity", 1_000_000_000.0, 9.5, 24, "2026-07-01", 1);
        assert_eq!(s.len(), 24);
        assert_eq!(total_principal(&s), 1_000_000_000.0);
        // Equal total payment each period (±1đ rounding), except possibly last.
        let pay0 = s[0].principal + s[0].interest;
        let pay10 = s[10].principal + s[10].interest;
        assert!(
            (pay0 - pay10).abs() < 1.0,
            "annuity payments should be flat"
        );
        // Interest declines as principal amortizes.
        assert!(s[0].interest > s[23].interest);
    }

    #[test]
    fn equal_principal_declining_interest() {
        let s = generate_schedule("equal_principal", 600.0, 12.0, 6, "2026-01-15", 1);
        assert_eq!(total_principal(&s), 600.0);
        assert_eq!(s[0].principal, 100.0);
        // 1%/month on declining balance: 6.0, 5.0, 4.0…
        assert_eq!(s[0].interest, 6.0);
        assert_eq!(s[1].interest, 5.0);
        assert_eq!(s[5].interest, 1.0);
    }

    #[test]
    fn interest_only_bullet_at_maturity() {
        let s = generate_schedule("interest_only", 500_000.0, 6.0, 4, "2026-01-01", 3);
        assert_eq!(s.len(), 4);
        // Quarterly rate = 6%/4 = 1.5% → 7,500 per period.
        assert!(s.iter().all(|i| i.interest == 7_500.0));
        assert!(s[..3].iter().all(|i| i.principal == 0.0));
        assert_eq!(s[3].principal, 500_000.0);
        assert_eq!(s[3].due_date, "2027-01-01");
    }

    #[test]
    fn zero_rate_annuity_splits_evenly() {
        let s = generate_schedule("annuity", 1000.0, 0.0, 3, "2026-01-01", 1);
        assert_eq!(total_principal(&s), 1000.0);
        assert!(s.iter().all(|i| i.interest == 0.0));
        assert_eq!(s[0].principal, 333.33);
        assert_eq!(s[2].principal, 333.34); // last absorbs rounding
    }

    #[test]
    fn unknown_method_defaults_to_annuity() {
        let a = generate_schedule("annuity", 1000.0, 10.0, 5, "2026-01-01", 1);
        let b = generate_schedule("whatever", 1000.0, 10.0, 5, "2026-01-01", 1);
        assert_eq!(a.len(), b.len());
        assert_eq!(a[2].principal, b[2].principal);
    }

    #[test]
    fn debt_kind_classification() {
        assert!(is_debt_kind("bank_loan"));
        assert!(is_debt_kind("credit_line"));
        assert!(!is_debt_kind("equity"));
        assert!(!is_debt_kind("investor"));
        assert!(!is_debt_kind("grant"));
    }
}
