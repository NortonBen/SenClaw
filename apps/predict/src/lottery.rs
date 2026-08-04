//! XSMB lottery: CSV ingest (khiemdoan/vietnam-lottery-xsmb-analysis daily
//! dataset) and *honest* descriptive statistics — frequency, "lô gan" (days
//! absent), head/tail digits. The mandatory disclaimer lives here in code, not
//! in a prompt, so no output path can drop it.
//!
//! Loto extraction: each of the 27 prize numbers contributes its last two
//! digits (`n % 100`), matching how the CSV stores numbers as integers (a
//! leading zero in G7 "05" arrives as 5 — mod 100 is unaffected).

use crate::timeutil::parse_date_days;

/// Appended verbatim to every lottery suggestion/stat answer. Hard-coded.
pub const DISCLAIMER: &str = "⚠️ Xổ số là ngẫu nhiên — không hệ thống nào dự đoán được kết quả. \
Nội dung chỉ mang tính thống kê & giải trí, không khuyến khích chơi vượt khả năng.";

/// One draw parsed from the dataset: date + 27 prize numbers in column order
/// (special, prize1, prize2_1..2, prize3_1..6, prize4_1..4, prize5_1..6,
/// prize6_1..3, prize7_1..4).
pub struct Draw {
    pub date: String,
    pub numbers: Vec<i64>,
}

impl Draw {
    pub fn loto(&self) -> Vec<u8> {
        self.numbers
            .iter()
            .map(|n| (n.rem_euclid(100)) as u8)
            .collect()
    }
}

/// Parse the whole CSV (header line + one row per draw). Malformed rows are
/// skipped; rows may gain columns in the future — the first 28 matter.
pub fn parse_csv(text: &str) -> Vec<Draw> {
    let mut out = Vec::new();
    for line in text.lines().skip(1) {
        let cols: Vec<&str> = line.split(',').collect();
        if cols.len() < 28 || parse_date_days(cols[0]).is_none() {
            continue;
        }
        let numbers: Vec<i64> = cols[1..28]
            .iter()
            .filter_map(|c| c.trim().parse().ok())
            .collect();
        if numbers.len() == 27 {
            out.push(Draw {
                date: cols[0].to_string(),
                numbers,
            });
        }
    }
    out
}

/// Per-loto stats over a window of draws (newest first as stored):
/// occurrence counts and gan (draws since last hit; `window` = never in window).
pub struct LotoStats {
    /// counts[n] = how many times loto `n` appeared in the window (incl. repeats within a draw).
    pub counts: [u32; 100],
    /// gan[n] = draws elapsed since the most recent appearance (0 = in the latest draw).
    pub gan: [u32; 100],
    pub window: usize,
}

/// `draws_newest_first`: (date, loto set) rows, newest first.
pub fn loto_stats(draws_newest_first: &[(String, Vec<u8>)]) -> LotoStats {
    let window = draws_newest_first.len();
    let mut counts = [0u32; 100];
    let mut gan = [window as u32; 100];
    for (age, (_, loto)) in draws_newest_first.iter().enumerate() {
        for &n in loto {
            let n = n as usize % 100;
            counts[n] += 1;
            if gan[n] == window as u32 {
                gan[n] = age as u32;
            }
        }
    }
    LotoStats {
        counts,
        gan,
        window,
    }
}

/// Top-k lotos by frequency (count desc, then number asc). Returns (loto, count).
pub fn top_frequent(stats: &LotoStats, k: usize) -> Vec<(u8, u32)> {
    let mut v: Vec<(u8, u32)> = (0..100u8).map(|n| (n, stats.counts[n as usize])).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v.truncate(k);
    v
}

/// Top-k "lô gan" — longest absent (gan desc). Returns (loto, draws_absent).
pub fn top_gan(stats: &LotoStats, k: usize) -> Vec<(u8, u32)> {
    let mut v: Vec<(u8, u32)> = (0..100u8).map(|n| (n, stats.gan[n as usize])).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v.truncate(k);
    v
}

/// Head (tens) and tail (units) digit distributions over the window.
pub fn head_tail(stats: &LotoStats) -> ([u32; 10], [u32; 10]) {
    let mut heads = [0u32; 10];
    let mut tails = [0u32; 10];
    for n in 0..100usize {
        heads[n / 10] += stats.counts[n];
        tails[n % 10] += stats.counts[n];
    }
    (heads, tails)
}

/// Entertainment pick: lotos that are BOTH warm recently (top-frequency band)
/// and currently modestly gan (absent 2+ draws) — the classic "sắp nổ lại"
/// heuristic. Deterministic; purely for fun, hence the hard disclaimer.
pub fn suggest(stats: &LotoStats, n: usize) -> Vec<u8> {
    let mut scored: Vec<(u8, f64)> = (0..100u8)
        .map(|num| {
            let c = stats.counts[num as usize] as f64;
            let g = stats.gan[num as usize] as f64;
            // Frequency dominates; small bonus for 2–7 draws absent; heavy gan penalized.
            let gan_bonus = if (2.0..=7.0).contains(&g) {
                1.5
            } else if g > 15.0 {
                -2.0
            } else {
                0.0
            };
            (num, c + gan_bonus)
        })
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap().then(a.0.cmp(&b.0)));
    scored.into_iter().take(n).map(|(num, _)| num).collect()
}

/// Baseline probability that one specific loto pair appears in a 27-number
/// draw, if draws were uniform: 1 - (1 - 1/100)^27 ≈ 0.238. Used as the stated
/// probability when ledgering entertainment picks — honesty by construction.
pub fn baseline_hit_prob() -> f64 {
    1.0 - 0.99f64.powi(27)
}

pub fn fmt_loto(n: u8) -> String {
    format!("{:02}", n % 100)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CSV: &str = "date,special,prize1,prize2_1,prize2_2,prize3_1,prize3_2,prize3_3,prize3_4,prize3_5,prize3_6,prize4_1,prize4_2,prize4_3,prize4_4,prize5_1,prize5_2,prize5_3,prize5_4,prize5_5,prize5_6,prize6_1,prize6_2,prize6_3,prize7_1,prize7_2,prize7_3,prize7_4\n\
2026-07-26,42916,89162,78045,30605,76062,75348,73197,83441,93250,22158,6046,2619,705,4198,6546,1509,6938,1105,2610,3449,75,409,698,32,37,25,60\n\
2026-07-27,54796,90290,28866,84542,6770,93665,56666,78753,55641,4646,42,3127,8547,130,4852,8164,7651,8392,2961,5133,874,639,502,87,29,55,52\n\
garbage-row\n";

    #[test]
    fn parse_and_loto() {
        let draws = parse_csv(CSV);
        assert_eq!(draws.len(), 2);
        assert_eq!(draws[0].date, "2026-07-26");
        let loto = draws[0].loto();
        assert_eq!(loto.len(), 27);
        assert_eq!(loto[0], 16); // 42916 → 16
        assert_eq!(loto[26], 60);
        // "705" → 05: mod-100 preserves the two-digit tail despite lost leading zero.
        assert!(loto.contains(&5));
    }

    #[test]
    fn stats_counts_and_gan() {
        let draws = parse_csv(CSV);
        // newest first: 07-27 then 07-26
        let rows: Vec<(String, Vec<u8>)> = vec![
            (draws[1].date.clone(), draws[1].loto()),
            (draws[0].date.clone(), draws[0].loto()),
        ];
        let stats = loto_stats(&rows);
        assert_eq!(stats.window, 2);
        // 96 appears in 2026-07-27 (54796) → gan 0.
        assert_eq!(stats.gan[96], 0);
        // 16 appears only in 2026-07-26 (42916) → gan 1.
        assert_eq!(stats.gan[16], 1);
        // 66 appears twice in 07-27 (28866, 56666) → count ≥ 2.
        assert!(stats.counts[66] >= 2);
        // A loto in neither draw has gan == window.
        let absent = (0..100u8).find(|n| stats.counts[*n as usize] == 0).unwrap();
        assert_eq!(stats.gan[absent as usize], 2);

        let top = top_frequent(&stats, 5);
        assert!(top[0].1 >= top[4].1);
        let gan = top_gan(&stats, 3);
        assert_eq!(gan[0].1, 2);

        let (heads, tails) = head_tail(&stats);
        assert_eq!(heads.iter().sum::<u32>(), 54);
        assert_eq!(tails.iter().sum::<u32>(), 54);
    }

    #[test]
    fn suggest_deterministic_and_sized() {
        let draws = parse_csv(CSV);
        let rows: Vec<(String, Vec<u8>)> = draws
            .iter()
            .rev()
            .map(|d| (d.date.clone(), d.loto()))
            .collect();
        let stats = loto_stats(&rows);
        let a = suggest(&stats, 3);
        let b = suggest(&stats, 3);
        assert_eq!(a, b);
        assert_eq!(a.len(), 3);
    }

    #[test]
    fn baseline_prob_value() {
        let p = baseline_hit_prob();
        assert!((p - 0.2375).abs() < 0.005);
    }

    #[test]
    fn disclaimer_present() {
        assert!(DISCLAIMER.contains("ngẫu nhiên"));
    }
}
