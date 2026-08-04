//! Chấm điểm và xếp hạng.
//!
//! Khuôn lấy từ MDN HTTP Observatory: khởi điểm 100, tra hạng bằng cách làm
//! tròn xuống bội của 5 rồi kẹp ở 100. Nhưng thêm **luật trần** kiểu SSL Labs:
//! một lỗi nặng kéo tụt hạng bất kể mọi thứ khác tốt đến đâu — vì lấy trung
//! bình sẽ để các mục màu xanh pha loãng một lỗ hổng thật.

use crate::db::Finding;

pub fn penalty(severity: &str) -> i64 {
    match severity {
        "critical" => 50,
        "high" => 20,
        "medium" => 10,
        "low" => 5,
        _ => 0, // info không trừ điểm
    }
}

/// Bảng hạng của MDN Observatory.
pub fn grade_for(score: i64) -> &'static str {
    let key = (score - score.rem_euclid(5)).min(100);
    match key {
        k if k >= 100 => "A+",
        95 | 90 => "A",
        85 => "A-",
        80 => "B+",
        75 | 70 => "B",
        65 => "B-",
        60 => "C+",
        55 | 50 => "C",
        45 => "C-",
        40 => "D+",
        35 | 30 => "D",
        25 => "D-",
        _ => "F",
    }
}

fn rank(g: &str) -> usize {
    ["A+", "A", "A-", "B+", "B", "B-", "C+", "C", "C-", "D+", "D", "D-", "F"]
        .iter()
        .position(|x| *x == g)
        .unwrap_or(12)
}

/// Hạng trần theo mức nặng nhất tìm thấy. Đây là chỗ chống "pha loãng".
fn cap_for(findings: &[Finding]) -> Option<&'static str> {
    let has = |s: &str| findings.iter().any(|f| f.severity == s && f.status_counts());
    if has("critical") {
        Some("F")
    } else if has("high") {
        Some("C")
    } else if has("medium") {
        Some("B")
    } else {
        None
    }
}

pub struct Scored {
    pub score: i64,
    pub grade: &'static str,
}

pub fn score(findings: &[Finding]) -> Scored {
    let total: i64 = findings
        .iter()
        .filter(|f| f.status_counts())
        .map(|f| penalty(f.severity))
        .sum();
    let score = (100 - total).max(0);
    let mut grade = grade_for(score);
    if let Some(cap) = cap_for(findings) {
        if rank(grade) < rank(cap) {
            grade = cap;
        }
    }
    Scored { score, grade }
}

impl Finding {
    /// Phát hiện đã được chấp nhận rủi ro thì không tính vào điểm nữa.
    /// (Hiện `Finding` chưa mang trạng thái — chỗ này để sẵn cho lúc chấm lại
    /// từ DB, nơi có cột `status`.)
    fn status_counts(&self) -> bool {
        true
    }
}

/// Xếp ưu tiên vá. KEV là phép **đè cứng**, không phải trọng số: một mục KEV
/// điểm CVSS thấp vẫn phải nằm trên một mục không-KEV điểm cao.
///
/// Ngưỡng EPSS 0.1 lấy theo số liệu hiệu quả của FIRST: lọc CVSS ≥ 7 bắt vá
/// ~50% kho lỗ hổng để bắt được ~6% cái thật sự bị khai thác; EPSS ≥ 0.1 chỉ
/// tốn ~2.7% công sức mà hiệu quả ~45%.
pub const EPSS_ACTION_THRESHOLD: f64 = 0.1;

pub fn priority_key(f: &Finding) -> (u8, i64) {
    let tier = if f.kev {
        0
    } else if f.epss.unwrap_or(0.0) >= EPSS_ACTION_THRESHOLD {
        1
    } else {
        2
    };
    let sev = match f.severity {
        "critical" => 0,
        "high" => 1,
        "medium" => 2,
        "low" => 3,
        _ => 4,
    };
    (tier, sev)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn f(sev: &'static str) -> Finding {
        Finding::new("headers", sev, format!("fp:{sev}:{}", rand_ish()), "x")
    }
    fn rand_ish() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        N.fetch_add(1, Ordering::Relaxed)
    }

    #[test]
    fn clean_scan_is_a_plus() {
        let s = score(&[]);
        assert_eq!(s.score, 100);
        assert_eq!(s.grade, "A+");
    }

    #[test]
    fn info_findings_do_not_reduce_the_score() {
        // Referrer-Policy vắng mặt là info — không được kéo điểm xuống.
        let s = score(&[f("info"), f("info"), f("info")]);
        assert_eq!(s.score, 100);
        assert_eq!(s.grade, "A+");
    }

    #[test]
    fn one_critical_caps_the_grade_at_f_however_good_the_rest() {
        // Đây là luật chống "pha loãng": chỉ một lỗi critical là F.
        let s = score(&[f("critical")]);
        assert_eq!(s.score, 50);
        assert_eq!(s.grade, "F", "critical phải kéo thẳng xuống F");
    }

    #[test]
    fn a_single_high_caps_at_c_even_with_a_high_score() {
        let s = score(&[f("high")]);
        assert_eq!(s.score, 80); // theo điểm thì là B+
        assert_eq!(s.grade, "C", "nhưng trần của 'high' là C");
    }

    #[test]
    fn medium_caps_at_b() {
        let s = score(&[f("medium")]);
        assert_eq!(s.score, 90);
        assert_eq!(s.grade, "B");
    }

    #[test]
    fn low_only_follows_the_score_chart() {
        let s = score(&[f("low"), f("low")]); // 100 - 10 = 90
        assert_eq!(s.score, 90);
        assert_eq!(s.grade, "A", "chỉ toàn 'low' thì không áp trần");
    }

    #[test]
    fn score_never_goes_below_zero() {
        let many: Vec<Finding> = (0..10).map(|_| f("critical")).collect();
        let s = score(&many);
        assert_eq!(s.score, 0);
        assert_eq!(s.grade, "F");
    }

    #[test]
    fn grade_chart_matches_observatory_boundaries() {
        assert_eq!(grade_for(100), "A+");
        assert_eq!(grade_for(105), "A+"); // vượt 100 vẫn A+
        assert_eq!(grade_for(95), "A");
        assert_eq!(grade_for(90), "A");
        assert_eq!(grade_for(89), "A-"); // làm tròn xuống 85
        assert_eq!(grade_for(85), "A-");
        assert_eq!(grade_for(80), "B+");
        assert_eq!(grade_for(70), "B");
        assert_eq!(grade_for(65), "B-");
        assert_eq!(grade_for(60), "C+");
        assert_eq!(grade_for(50), "C");
        assert_eq!(grade_for(45), "C-");
        assert_eq!(grade_for(40), "D+");
        assert_eq!(grade_for(30), "D");
        assert_eq!(grade_for(25), "D-");
        assert_eq!(grade_for(20), "F");
        assert_eq!(grade_for(0), "F");
    }

    #[test]
    fn kev_outranks_a_higher_severity_without_kev() {
        let mut kev_low = Finding::new("cve", "medium", "a", "KEV nhưng medium");
        kev_low.kev = true;
        kev_low.cve = Some("CVE-2021-44228".into());
        kev_low.evidence = json!({});

        let plain_critical = Finding::new("cve", "critical", "b", "critical nhưng không KEV");

        // KEV phải xếp trước, dù mức nhẹ hơn.
        assert!(priority_key(&kev_low) < priority_key(&plain_critical));
    }

    #[test]
    fn high_epss_outranks_low_epss_at_same_severity() {
        let mut hot = Finding::new("cve", "high", "a", "x");
        hot.epss = Some(0.4);
        let mut cold = Finding::new("cve", "high", "b", "y");
        cold.epss = Some(0.001);
        assert!(priority_key(&hot) < priority_key(&cold));
    }

    #[test]
    fn epss_threshold_is_the_documented_action_line() {
        assert!((EPSS_ACTION_THRESHOLD - 0.1).abs() < f64::EPSILON);
        let mut just_over = Finding::new("cve", "low", "a", "x");
        just_over.epss = Some(0.1);
        let mut just_under = Finding::new("cve", "low", "b", "y");
        just_under.epss = Some(0.099);
        assert!(priority_key(&just_over) < priority_key(&just_under));
    }
}
