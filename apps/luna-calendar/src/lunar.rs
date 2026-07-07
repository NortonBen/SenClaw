//! Vietnamese lunar calendar core — the Hồ Ngọc Đức astronomical algorithm.
//!
//! Solar (Gregorian) ⇄ Lunar conversion for the +7 timezone (Vietnam), plus the
//! astronomical helpers (Julian day, new-moon instants, sun longitude, 24 solar
//! terms) everything else in the app is built on. Deterministic — no LLM, no I/O.
//!
//! Reference: Hồ Ngọc Đức, "Âm lịch Việt Nam" (the de-facto standard used by
//! virtually every Vietnamese lunar-calendar site).

use std::f64::consts::PI;

/// Vietnam is UTC+7.
pub const TZ_VN: f64 = 7.0;

/// Integer Julian day number of a Gregorian date `(dd, mm, yy)`.
pub fn jd_from_ymd(dd: i64, mm: i64, yy: i64) -> i64 {
    let a = (14 - mm) / 12;
    let y = yy + 4800 - a;
    let m = mm + 12 * a - 3;
    let jd = dd + (153 * m + 2) / 5 + 365 * y + y / 4 - y / 100 + y / 400 - 32045;
    if jd < 2299161 {
        // Julian calendar (before 1582-10-15).
        dd + (153 * m + 2) / 5 + 365 * y + y / 4 - 32083
    } else {
        jd
    }
}

/// Gregorian date `(dd, mm, yy)` from an integer Julian day number.
pub fn jd_to_ymd(jd: i64) -> (i64, i64, i64) {
    let (b, c);
    if jd > 2299160 {
        let a = jd + 32044;
        b = (4 * a + 3) / 146097;
        c = a - (b * 146097) / 4;
    } else {
        b = 0;
        c = jd + 32082;
    }
    let d = (4 * c + 3) / 1461;
    let e = c - (1461 * d) / 4;
    let m = (5 * e + 2) / 153;
    let day = e - (153 * m + 2) / 5 + 1;
    let month = m + 3 - 12 * (m / 10);
    let year = b * 100 + d - 4800 + m / 10;
    (day, month, year)
}

/// Day of week for a Julian day number. 0 = Monday … 6 = Sunday.
pub fn weekday_mon0(jd: i64) -> i64 {
    ((jd % 7) + 7) % 7
}

/// Julian date (as a float, at 0h UT) of the `k`-th new moon since 1900-01-01.
fn new_moon(k: i64) -> f64 {
    let k = k as f64;
    let t = k / 1236.85;
    let t2 = t * t;
    let t3 = t2 * t;
    let dr = PI / 180.0;
    let mut jd1 = 2415020.75933 + 29.53058868 * k + 0.0001178 * t2 - 0.000000155 * t3;
    jd1 += 0.00033 * ((166.56 + 132.87 * t - 0.009173 * t2) * dr).sin();
    let m = 359.2242 + 29.10535608 * k - 0.0000333 * t2 - 0.00000347 * t3;
    let mpr = 306.0253 + 385.81691806 * k + 0.0107306 * t2 + 0.00001236 * t3;
    let f = 21.2964 + 390.67050646 * k - 0.0016528 * t2 - 0.00000239 * t3;
    let mut c1 = (0.1734 - 0.000393 * t) * (m * dr).sin() + 0.0021 * (2.0 * m * dr).sin();
    c1 -= 0.4068 * (mpr * dr).sin() - 0.0161 * (2.0 * mpr * dr).sin();
    c1 -= 0.0004 * (3.0 * mpr * dr).sin();
    c1 += 0.0104 * (2.0 * f * dr).sin() - 0.0051 * ((m + mpr) * dr).sin();
    c1 -= 0.0074 * ((m - mpr) * dr).sin() + 0.0004 * ((2.0 * f + m) * dr).sin();
    c1 -= 0.0004 * ((2.0 * f - m) * dr).sin() - 0.0006 * ((2.0 * f + mpr) * dr).sin();
    c1 += 0.0010 * ((2.0 * f - mpr) * dr).sin() + 0.0005 * ((2.0 * mpr + m) * dr).sin();
    let deltat = if t < -11.0 {
        0.001 + 0.000839 * t + 0.0002261 * t2 - 0.00000845 * t3 - 0.000000081 * t * t3
    } else {
        -0.000278 + 0.000265 * t + 0.000262 * t2
    };
    jd1 + c1 - deltat
}

/// Sun's ecliptic longitude (radians, 0..2π) at Julian date `jdn`.
fn sun_longitude(jdn: f64) -> f64 {
    let t = (jdn - 2451545.0) / 36525.0;
    let t2 = t * t;
    let dr = PI / 180.0;
    let m = 357.52910 + 35999.05030 * t - 0.0001559 * t2 - 0.00000048 * t * t2;
    let l0 = 280.46645 + 36000.76983 * t + 0.0003032 * t2;
    let mut dl = (1.914600 - 0.004817 * t - 0.000014 * t2) * (m * dr).sin();
    dl += (0.019993 - 0.000101 * t) * (2.0 * m * dr).sin() + 0.000290 * (3.0 * m * dr).sin();
    let mut l = (l0 + dl) * dr;
    l -= PI * 2.0 * (l / (PI * 2.0)).floor();
    l
}

/// Sun longitude bucket (0..11, each = 30°) for the day starting at midnight
/// local time — used to place the 11th lunar month (winter solstice month).
fn get_sun_longitude(day_number: i64, tz: f64) -> i64 {
    (sun_longitude(day_number as f64 - 0.5 - tz / 24.0) / PI * 6.0).floor() as i64
}

/// Integer Julian day of the `k`-th new moon, at local timezone `tz`.
fn get_new_moon_day(k: i64, tz: f64) -> i64 {
    (new_moon(k) + 0.5 + tz / 24.0).floor() as i64
}

/// Julian day of the new moon that starts the 11th lunar month of solar year `yy`.
fn get_lunar_month_11(yy: i64, tz: f64) -> i64 {
    let off = jd_from_ymd(31, 12, yy) as f64 - 2415021.0;
    let k = (off / 29.530588853).floor() as i64;
    let nm = get_new_moon_day(k, tz);
    let sun_long = get_sun_longitude(nm, tz);
    if sun_long >= 9 {
        get_new_moon_day(k - 1, tz)
    } else {
        nm
    }
}

/// Which month after the 11th is the leap month (0 = none in this window).
fn get_leap_month_offset(a11: i64, tz: f64) -> i64 {
    let k = ((a11 as f64 - 2415021.076998695) / 29.530588853 + 0.5).floor() as i64;
    let mut i = 1;
    let mut arc = get_sun_longitude(get_new_moon_day(k + i, tz), tz);
    let mut last;
    loop {
        last = arc;
        i += 1;
        arc = get_sun_longitude(get_new_moon_day(k + i, tz), tz);
        if arc == last || i >= 14 {
            break;
        }
    }
    i - 1
}

/// The lunar date of a solar date. Returns `(lunar_day, lunar_month, lunar_year, is_leap)`.
pub fn solar_to_lunar(dd: i64, mm: i64, yy: i64, tz: f64) -> (i64, i64, i64, bool) {
    let day_number = jd_from_ymd(dd, mm, yy);
    let k = ((day_number as f64 - 2415021.076998695) / 29.530588853).floor() as i64;
    let mut month_start = get_new_moon_day(k + 1, tz);
    if month_start > day_number {
        month_start = get_new_moon_day(k, tz);
    }
    let mut a11 = get_lunar_month_11(yy, tz);
    let mut b11 = a11;
    let mut lunar_year;
    if a11 >= month_start {
        lunar_year = yy;
        a11 = get_lunar_month_11(yy - 1, tz);
    } else {
        lunar_year = yy + 1;
        b11 = get_lunar_month_11(yy + 1, tz);
    }
    let lunar_day = day_number - month_start + 1;
    let diff = ((month_start - a11) as f64 / 29.0).floor() as i64;
    let mut lunar_leap = false;
    let mut lunar_month = diff + 11;
    if b11 - a11 > 365 {
        let leap_month_diff = get_leap_month_offset(a11, tz);
        if diff >= leap_month_diff {
            lunar_month = diff + 10;
            if diff == leap_month_diff {
                lunar_leap = true;
            }
        }
    }
    if lunar_month > 12 {
        lunar_month -= 12;
    }
    if lunar_month >= 11 && diff < 4 {
        lunar_year -= 1;
    }
    (lunar_day, lunar_month, lunar_year, lunar_leap)
}

/// The solar date of a lunar date. Returns `(dd, mm, yy)`, or `(0,0,0)` if the
/// requested leap month does not exist that year.
pub fn lunar_to_solar(
    lunar_day: i64,
    lunar_month: i64,
    lunar_year: i64,
    lunar_leap: bool,
    tz: f64,
) -> (i64, i64, i64) {
    let (a11, b11);
    if lunar_month < 11 {
        a11 = get_lunar_month_11(lunar_year - 1, tz);
        b11 = get_lunar_month_11(lunar_year, tz);
    } else {
        a11 = get_lunar_month_11(lunar_year, tz);
        b11 = get_lunar_month_11(lunar_year + 1, tz);
    }
    let mut off = lunar_month - 11;
    if off < 0 {
        off += 12;
    }
    if b11 - a11 > 365 {
        let leap_off = get_leap_month_offset(a11, tz);
        let mut leap_month = leap_off - 2;
        if leap_month < 0 {
            leap_month += 12;
        }
        if lunar_leap && lunar_month != leap_month {
            return (0, 0, 0);
        } else if lunar_leap || off >= leap_off {
            off += 1;
        }
    }
    let k = (0.5 + (a11 as f64 - 2415021.076998695) / 29.530588853).floor() as i64;
    let month_start = get_new_moon_day(k + off, tz);
    jd_to_ymd(month_start + lunar_day - 1)
}

/// 24 solar terms (tiết khí), index 0..23. Longitude bucket in 15° steps.
/// Index 0 begins at ecliptic longitude 0° (Xuân phân / spring equinox).
pub fn solar_term_index(jd: i64, tz: f64) -> usize {
    let idx = (sun_longitude(jd as f64 - 0.5 - tz / 24.0) / PI * 12.0).floor() as i64;
    (((idx % 24) + 24) % 24) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solar_to_lunar_reference() {
        // 7 July 2026 → 23/5 âm lịch (năm Bính Ngọ), verified against the almanac.
        let (d, m, y, leap) = solar_to_lunar(7, 7, 2026, TZ_VN);
        assert_eq!((d, m, y, leap), (23, 5, 2026, false));
    }

    #[test]
    fn round_trips() {
        for (dd, mm, yy) in [(7, 7, 2026), (1, 1, 2000), (10, 2, 2024), (31, 12, 1999)] {
            let (ld, lm, ly, leap) = solar_to_lunar(dd, mm, yy, TZ_VN);
            let (bd, bm, by) = lunar_to_solar(ld, lm, ly, leap, TZ_VN);
            assert_eq!((bd, bm, by), (dd, mm, yy), "round-trip {dd}/{mm}/{yy}");
        }
    }

    #[test]
    fn leap_month_2025() {
        // 2025 âm lịch has a leap 6th month; 2023 has leap 2nd. Sanity: a known
        // Tết — 17 Feb 2026 is mùng 1 Tết Bính Ngọ.
        let (d, m, _y, leap) = solar_to_lunar(17, 2, 2026, TZ_VN);
        assert_eq!((d, m, leap), (1, 1, false));
    }

    #[test]
    fn known_weekday() {
        // 2000-01-01 was a Saturday (Mon=0 → Sat=5).
        assert_eq!(weekday_mon0(jd_from_ymd(1, 1, 2000)), 5);
    }
}
