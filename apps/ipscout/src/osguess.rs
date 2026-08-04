//! "Hệ điều hành là gì" — suy luận có trọng số từ bằng chứng thu được.
//!
//! **Đây không phải vân tay ngăn xếp TCP/IP kiểu `nmap -O`.** Cách đó gửi gói
//! dị dạng (cờ TCP không hợp lệ, ICMP đặc biệt) rồi đo phản ứng của kernel, và
//! nó cần raw socket tức là quyền root. App cố tình không làm: gói dị dạng là
//! kỹ thuật dò xâm nhập, và quyền root là thứ một Space App không nên đòi.
//!
//! Đổi lại app cộng những bằng chứng mà một client hợp lệ nhìn thấy được — hậu
//! tố gói của bản phân phối trong banner SSH, header `Server`, nhãn trong lời
//! chào SMTP. Với **máy chủ** cách này thường chắc hơn đoán theo TTL (TTL chỉ
//! phân biệt được Linux 64 / Windows 128 / thiết bị mạng 255, lại còn bị các
//! hop trung gian làm sai lệch), và quan trọng hơn: nó **cho người đọc thấy vì
//! sao**, thay vì một cái tên không kiểm chứng được.
//!
//! Kết luận luôn kèm phần trăm và danh sách bằng chứng. Không bao giờ 100%.

use crate::banner::OsEvidence;
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct Guess {
    pub os: Option<String>,
    /// 0–97. Trần 97 là có chủ ý: suy luận từ xa không bao giờ là chắc chắn.
    pub confidence: u32,
    pub evidence: Vec<OsEvidence>,
    /// Bằng chứng chỉ về những họ OS khác — dấu hiệu máy chủ đứng sau proxy,
    /// hoặc nhiều dịch vụ khác nhau cùng một IP.
    pub conflicts: Vec<String>,
    pub note: String,
}

/// Gom "Ubuntu 22.04 LTS" và "Ubuntu" về cùng một họ để cộng trọng số, nhưng
/// vẫn giữ được nhãn chi tiết nhất khi kết luận.
pub fn family(os: &str) -> &str {
    let low = os.to_ascii_lowercase();
    for f in [
        "ubuntu", "debian", "centos", "rhel", "alpine", "windows", "freebsd", "openbsd",
    ] {
        if low.contains(f) {
            return match f {
                "ubuntu" => "Ubuntu",
                "debian" => "Debian",
                "centos" => "CentOS",
                "rhel" => "RHEL",
                "alpine" => "Alpine",
                "windows" => "Windows",
                "freebsd" => "FreeBSD",
                _ => "OpenBSD",
            };
        }
    }
    "Khác"
}

/// Cộng nhiều bằng chứng độc lập theo kiểu noisy-OR: `1 - Π(1 - wᵢ)`.
///
/// Cộng thẳng thì hai bằng chứng 60% thành 120%; lấy max thì bằng chứng thứ hai
/// hoá ra vô ích. Noisy-OR cho đúng cái ta muốn: mỗi bằng chứng thêm vào đều
/// nâng độ tin, nhưng không bao giờ chạm 100%.
pub fn combine(weights: &[u32]) -> u32 {
    let p = weights
        .iter()
        .fold(1.0f64, |acc, w| acc * (1.0 - (*w as f64 / 100.0).clamp(0.0, 0.99)));
    (((1.0 - p) * 100.0).round() as u32).min(97)
}

pub fn guess(evidence: &[OsEvidence]) -> Guess {
    if evidence.is_empty() {
        return Guess {
            os: None,
            confidence: 0,
            evidence: vec![],
            conflicts: vec![],
            note: "Không có bằng chứng nào. Không kết luận được hệ điều hành — điều này \
                   BÌNH THƯỜNG với máy chủ được cấu hình kín: gỡ nhãn phân phối khỏi \
                   banner và giấu header Server là biện pháp làm cứng đúng đắn."
                .into(),
        };
    }

    // Gom theo họ, giữ nhãn chi tiết nhất (dài nhất) làm tên hiển thị.
    let mut by_family: BTreeMap<&str, (Vec<u32>, String)> = BTreeMap::new();
    for e in evidence {
        let f = family(&e.os);
        let slot = by_family.entry(f).or_insert_with(|| (vec![], e.os.clone()));
        slot.0.push(e.weight);
        if e.os.len() > slot.1.len() {
            slot.1 = e.os.clone();
        }
    }

    let mut scored: Vec<(&str, String, u32)> = by_family
        .iter()
        .map(|(f, (ws, label))| (*f, label.clone(), combine(ws)))
        .collect();
    scored.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(b.0)));

    let (_, best_label, best_score) = scored[0].clone();
    let conflicts: Vec<String> = scored[1..]
        .iter()
        .map(|(_, l, s)| format!("{l} ({s}%)"))
        .collect();

    // Hai họ khác nhau mà điểm sát nhau thì kết luận đơn lẻ là sai lệch: thường
    // là proxy/load balancer một hệ, ứng dụng phía sau một hệ khác.
    let contested = scored.len() > 1 && best_score.saturating_sub(scored[1].2) < 20;
    let confidence = if contested {
        best_score.saturating_sub(20)
    } else {
        best_score
    };

    let note = if contested {
        format!(
            "Bằng chứng chỉ về nhiều hệ khác nhau ({}). Thường gặp khi có proxy hoặc \
             load balancer đứng trước: lớp ngoài một hệ, ứng dụng phía sau một hệ khác. \
             Độ tin đã bị hạ vì lý do đó.",
            scored
                .iter()
                .map(|(_, l, s)| format!("{l} {s}%"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    } else {
        format!(
            "Suy luận từ {} bằng chứng quan sát được, KHÔNG phải vân tay ngăn xếp TCP/IP. \
             Nhãn phân phối trong banner có thể bị sửa hoặc gỡ, nên đây là kết luận có \
             xác suất chứ không phải sự thật đã kiểm chứng.",
            evidence.len()
        )
    };

    Guess {
        os: Some(best_label),
        confidence,
        evidence: evidence.to_vec(),
        conflicts,
        note,
    }
}

pub fn to_json(g: &Guess, fronted_by: Option<&str>) -> Value {
    let mut note = g.note.clone();
    if let Some(cdn) = fronted_by {
        // Không nói câu này thì cả kết luận mô tả sai đối tượng.
        note = format!(
            "IP nằm sau {cdn}: hệ điều hành suy ra ở đây là **của biên {cdn}**, không \
             phải của máy chủ gốc. {note}"
        );
    }
    json!({
        "os": g.os,
        "confidence": g.confidence,
        "method": "suy luận có trọng số từ banner/header (không phải vân tay ngăn xếp TCP/IP)",
        "conflicts": g.conflicts,
        "note": note,
        "evidence": g.evidence.iter().map(|e| json!({
            "os": e.os, "weight": e.weight, "from": e.from,
        })).collect::<Vec<_>>(),
        "not_covered": [
            "Vân tay ngăn xếp TCP/IP (nmap -O): cần raw socket + gửi gói dị dạng — cố ý không làm.",
            "Phiên bản kernel chính xác: không lộ ra qua banner của dịch vụ ứng dụng.",
            "Máy chủ đã gỡ nhãn phân phối khỏi banner sẽ không suy ra được gì — đó là cấu hình đúng, không phải lỗi quét."
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(os: &str, w: u32) -> OsEvidence {
        OsEvidence {
            os: os.into(),
            weight: w,
            from: "test".into(),
        }
    }

    #[test]
    fn no_evidence_means_no_conclusion_not_a_default_guess() {
        // Máy chủ làm cứng đúng cách sẽ không lộ gì — và app phải nói thế,
        // chứ không được mặc định "chắc là Linux".
        let g = guess(&[]);
        assert!(g.os.is_none());
        assert_eq!(g.confidence, 0);
        assert!(g.note.contains("BÌNH THƯỜNG"));
    }

    #[test]
    fn independent_evidence_accumulates_without_ever_reaching_certainty() {
        assert_eq!(combine(&[90]), 90);
        // hai bằng chứng 60% phải > 60 nhưng < 100
        let two = combine(&[60, 60]);
        assert!(two > 60 && two < 100, "được {two}");
        // rất nhiều bằng chứng mạnh vẫn bị chặn ở 97
        assert_eq!(combine(&[95, 95, 95, 95, 95]), 97);
        assert_eq!(combine(&[]), 0);
    }

    #[test]
    fn detailed_label_wins_over_the_generic_one_in_the_same_family() {
        let g = guess(&[ev("Ubuntu", 60), ev("Ubuntu 22.04 LTS", 85)]);
        assert_eq!(g.os.as_deref(), Some("Ubuntu 22.04 LTS"));
        // và hai bằng chứng cùng họ phải cộng dồn, không phải lấy max
        assert!(g.confidence > 85, "được {}", g.confidence);
        assert!(g.conflicts.is_empty());
    }

    #[test]
    fn conflicting_families_lower_confidence_and_are_reported() {
        // Trường hợp thật: nginx trên Debian đứng trước ứng dụng chạy Windows.
        let g = guess(&[ev("Debian 12 (bookworm)", 70), ev("Windows", 65)]);
        assert!(!g.conflicts.is_empty());
        assert!(g.note.contains("proxy") || g.note.contains("load balancer"));
        // độ tin phải thấp hơn khi chỉ có một mình bằng chứng mạnh nhất
        let alone = guess(&[ev("Debian 12 (bookworm)", 70)]);
        assert!(g.confidence < alone.confidence);
    }

    #[test]
    fn a_clear_winner_is_not_penalised_as_contested() {
        let g = guess(&[ev("Ubuntu 22.04 LTS", 90), ev("Windows", 30)]);
        assert_eq!(family(g.os.as_deref().unwrap()), "Ubuntu");
        assert!(g.confidence >= 85, "được {}", g.confidence);
    }

    #[test]
    fn family_grouping_folds_release_labels_together() {
        assert_eq!(family("Ubuntu 22.04 LTS"), "Ubuntu");
        assert_eq!(family("Debian 12 (bookworm)"), "Debian");
        assert_eq!(family("Windows"), "Windows");
        assert_eq!(family("Something Else"), "Khác");
    }

    #[test]
    fn a_cdn_fronted_result_says_whose_os_it_actually_describes() {
        let g = guess(&[ev("Ubuntu 22.04 LTS", 85)]);
        let j = to_json(&g, Some("Cloudflare"));
        let note = j["note"].as_str().unwrap();
        assert!(note.contains("Cloudflare"));
        assert!(note.contains("không phải của máy chủ gốc"));
    }

    #[test]
    fn json_always_declares_what_the_method_cannot_see() {
        let j = to_json(&guess(&[ev("Ubuntu", 60)]), None);
        assert!(j["method"].as_str().unwrap().contains("không phải vân tay"));
        assert_eq!(j["not_covered"].as_array().unwrap().len(), 3);
    }
}
