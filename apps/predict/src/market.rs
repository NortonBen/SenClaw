//! Gold/FX trend math over the locally accumulated `price_history` series.
//! Simple, explainable indicators only (SMA, momentum, range) — narrated by the
//! LLM but never presented as investment advice.

/// Appended verbatim to every market/gold answer. Hard-coded in code.
pub const DISCLAIMER: &str =
    "⚠️ Thông tin xu hướng chỉ mang tính tham khảo, KHÔNG phải lời khuyên đầu tư.";

/// Troy-ounce → Vietnamese lượng (37.5 g / 31.1034768 g).
pub const OZ_PER_LUONG: f64 = 1.2056788;

/// Simple moving average of the last `n` points (None if fewer than n).
pub fn sma(series: &[(i64, f64)], n: usize) -> Option<f64> {
    if series.len() < n || n == 0 {
        return None;
    }
    let s: f64 = series[series.len() - n..].iter().map(|(_, p)| p).sum();
    Some(s / n as f64)
}

/// Percent change between the last point and the closest point at least
/// `secs` older (None when history is too short).
pub fn momentum_pct(series: &[(i64, f64)], secs: i64) -> Option<f64> {
    let (last_ts, last_p) = *series.last()?;
    let past = series.iter().rev().find(|(ts, _)| last_ts - ts >= secs)?;
    if past.1 == 0.0 {
        return None;
    }
    Some((last_p - past.1) / past.1 * 100.0)
}

/// (min, max) over the series.
pub fn range(series: &[(i64, f64)]) -> Option<(f64, f64)> {
    let first = series.first()?.1;
    Some(
        series
            .iter()
            .fold((first, first), |(lo, hi), (_, p)| (lo.min(*p), hi.max(*p))),
    )
}

/// Coarse trend verdict from short vs long SMA: "tăng" / "giảm" / "đi ngang".
pub fn trend_label(series: &[(i64, f64)], short_n: usize, long_n: usize) -> &'static str {
    match (sma(series, short_n), sma(series, long_n)) {
        (Some(s), Some(l)) if l != 0.0 => {
            let d = (s - l) / l;
            if d > 0.003 {
                "tăng"
            } else if d < -0.003 {
                "giảm"
            } else {
                "đi ngang"
            }
        }
        _ => "chưa đủ dữ liệu",
    }
}

/// World XAU (USD/oz) → domestic-style quote (triệu VND / lượng).
pub fn xau_to_vnd_luong(xau_usd: f64, vnd_per_usd: f64) -> f64 {
    xau_usd * vnd_per_usd * OZ_PER_LUONG / 1_000_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn series(prices: &[f64]) -> Vec<(i64, f64)> {
        prices
            .iter()
            .enumerate()
            .map(|(i, p)| (i as i64 * 3600, *p))
            .collect()
    }

    #[test]
    fn sma_and_momentum() {
        let s = series(&[100.0, 102.0, 104.0, 106.0]);
        assert_eq!(sma(&s, 2), Some(105.0));
        assert!(sma(&s, 10).is_none());
        // 3h window: last=106 vs first>=3h older = 100 → +6%
        assert!((momentum_pct(&s, 3 * 3600).unwrap() - 6.0).abs() < 1e-9);
        assert!(momentum_pct(&s, 100 * 3600).is_none());
    }

    #[test]
    fn trend_labels() {
        assert_eq!(
            trend_label(&series(&[100.0, 100.1, 103.0, 106.0]), 2, 4),
            "tăng"
        );
        assert_eq!(
            trend_label(&series(&[106.0, 105.0, 101.0, 100.0]), 2, 4),
            "giảm"
        );
        assert_eq!(
            trend_label(&series(&[100.0, 100.0, 100.0, 100.0]), 2, 4),
            "đi ngang"
        );
        assert_eq!(trend_label(&series(&[100.0]), 2, 4), "chưa đủ dữ liệu");
    }

    #[test]
    fn gold_conversion() {
        // 4000 USD/oz × 26,000 VND/USD ≈ 125.4 triệu/lượng.
        let v = xau_to_vnd_luong(4000.0, 26000.0);
        assert!((v - 125.39) < 1.0 && v > 120.0);
    }
}
