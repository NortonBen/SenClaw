//! Minimal civil-date/time helpers (no chrono): unix ↔ `YYYY-MM-DD`, Vietnam
//! local time (fixed UTC+7 — VN has no DST), and ISO timestamp parsing for
//! TheSportsDB (`2026-08-21T19:00:00`, UTC).

pub const VN_OFFSET: i64 = 7 * 3600;

/// Days since 1970-01-01 for a civil date (Howard Hinnant's algorithm).
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = y - if m <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Inverse of [`days_from_civil`].
pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    (y + if m <= 2 { 1 } else { 0 }, m, d)
}

/// `YYYY-MM-DD` for a unix timestamp shifted by `offset` seconds.
pub fn date_str(unix: i64, offset: i64) -> String {
    let (y, m, d) = civil_from_days((unix + offset).div_euclid(86400));
    format!("{y:04}-{m:02}-{d:02}")
}

/// Parse `YYYY-MM-DD` → days since epoch. None on malformed input.
pub fn parse_date_days(s: &str) -> Option<i64> {
    let mut it = s.split('-');
    let y: i64 = it.next()?.parse().ok()?;
    let m: i64 = it.next()?.parse().ok()?;
    let d: i64 = it.next()?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some(days_from_civil(y, m, d))
}

/// Parse `YYYY-MM-DDTHH:MM:SS` (assumed UTC) → unix seconds.
pub fn parse_iso_utc(s: &str) -> Option<i64> {
    let (date, time) = s.split_once('T').or_else(|| s.split_once(' '))?;
    let days = parse_date_days(date)?;
    let mut it = time.trim_end_matches('Z').split(':');
    let h: i64 = it.next()?.parse().ok()?;
    let mi: i64 = it.next()?.parse().ok()?;
    let sec: i64 = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
    Some(days * 86400 + h * 3600 + mi * 60 + sec)
}

/// (hour, minute) of the Vietnam-local wall clock for a unix timestamp.
pub fn vn_hm(unix: i64) -> (i64, i64) {
    let t = (unix + VN_OFFSET).rem_euclid(86400);
    (t / 3600, (t % 3600) / 60)
}

/// Vietnam-local `YYYY-MM-DD` for a unix timestamp.
pub fn vn_date(unix: i64) -> String {
    date_str(unix, VN_OFFSET)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_roundtrip() {
        for &(y, m, d) in &[(1970, 1, 1), (2000, 2, 29), (2026, 7, 27), (1999, 12, 31)] {
            let days = days_from_civil(y, m, d);
            assert_eq!(civil_from_days(days), (y, m, d));
        }
        assert_eq!(days_from_civil(1970, 1, 1), 0);
    }

    #[test]
    fn date_str_and_parse() {
        // 2026-07-27 00:00 UTC = 1785110400 (verified: 2026-07-27 is a Monday, 20661 days).
        let days = parse_date_days("2026-07-27").unwrap();
        assert_eq!(date_str(days * 86400, 0), "2026-07-27");
        assert!(parse_date_days("garbage").is_none());
        assert!(parse_date_days("2026-13-01").is_none());
    }

    #[test]
    fn iso_parse() {
        let ts = parse_iso_utc("2026-08-21T19:00:00").unwrap();
        assert_eq!(date_str(ts, 0), "2026-08-21");
        assert_eq!(vn_hm(ts), (2, 0)); // 19:00 UTC = 02:00 VN next day
        assert_eq!(vn_date(ts), "2026-08-22");
    }
}
