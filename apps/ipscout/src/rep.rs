//! Tiếng tăm của IP: tra các danh sách chặn (DNSBL).
//!
//! Toàn bộ là truy vấn DNS — **không một gói tin nào tới mục tiêu**. Cách tra là
//! đảo octet rồi hỏi bản ghi A: `4.3.2.1.zen.spamhaus.org`.
//!
//! Hai cái bẫy, và cả hai đều biến thành kết luận sai nếu bỏ qua:
//!
//! 1. **Spamhaus từ chối truy vấn đến từ resolver công cộng** (Google 8.8.8.8,
//!    Cloudflare 1.1.1.1) và trả về `127.255.255.252/254`. Địa chỉ đó nằm trong
//!    `127.0.0.0/8` nên bộ kiểm ngây thơ đọc thành "CÓ trong danh sách" — báo
//!    động giả trên mọi IP sạch. Máy nào cấu hình DNS công cộng cũng dính.
//! 2. **NXDOMAIN là câu trả lời "sạch"**, còn timeout thì không nói lên điều gì.
//!    Gộp hai thứ đó là im lặng biến lỗi mạng thành lời xác nhận sạch sẽ.

use serde_json::{json, Value};
use std::net::{IpAddr, Ipv4Addr};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Có trong danh sách.
    Listed,
    /// Không có — NXDOMAIN, câu trả lời dứt khoát.
    Clean,
    /// Truy vấn bị từ chối (dùng resolver công cộng) hoặc hỏng. **Không kết luận.**
    Unknown,
}

impl Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Listed => "listed",
            Self::Clean => "clean",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Hit {
    pub zone: &'static str,
    pub status: Status,
    pub codes: Vec<String>,
    pub meaning: String,
}

pub const ZONES: &[(&str, &str)] = &[
    ("zen.spamhaus.org", "Spamhaus ZEN"),
    ("bl.spamcop.net", "SpamCop"),
    ("b.barracudacentral.org", "Barracuda"),
    ("dnsbl.sorbs.net", "SORBS"),
];

/// Giải mã mã trả về của Spamhaus ZEN.
///
/// `127.0.0.10/11` (PBL) đáng chú ý vì lý do ngoài chống thư rác: nó nghĩa là
/// **dải IP dân dụng, không phải hạ tầng máy chủ** — một tín hiệu hạ tầng đúng
/// nghĩa, không chỉ là điểm trừ danh tiếng.
pub fn decode_zen(code: &str) -> Option<&'static str> {
    Some(match code {
        "127.0.0.2" => "SBL — nguồn phát thư rác đã biết",
        "127.0.0.3" => "CSS — nguồn thư rác dạng snowshoe",
        "127.0.0.4" | "127.0.0.5" | "127.0.0.6" | "127.0.0.7" => {
            "XBL — máy đã bị chiếm quyền (botnet/proxy mở)"
        }
        "127.0.0.9" => "DROP/EDROP — dải bị khuyến nghị chặn hoàn toàn",
        "127.0.0.10" | "127.0.0.11" => {
            "PBL — dải IP dân dụng, không nên gửi thư trực tiếp (đây là hạ tầng dân dụng, không phải máy chủ)"
        }
        _ => return None,
    })
}

/// Địa chỉ trả về có phải mã lỗi "truy vấn bị từ chối" không.
///
/// `127.255.255.0/24` được Spamhaus dành riêng cho việc này. Không tách ra thì
/// mọi IP đều bị báo là nằm trong danh sách chặn.
pub fn is_refusal(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    o[0] == 127 && o[1] == 255 && o[2] == 255
}

/// Diễn giải tập địa chỉ mà một zone trả về.
pub fn interpret(zone: &str, addrs: &[Ipv4Addr]) -> Hit {
    let zone_name = ZONES
        .iter()
        .find(|(z, _)| *z == zone)
        .map(|(_, n)| *n)
        .unwrap_or(zone);

    if addrs.is_empty() {
        return Hit {
            zone: zone_leak(zone),
            status: Status::Clean,
            codes: vec![],
            meaning: format!("Không có trong {zone_name}."),
        };
    }
    if addrs.iter().all(|a| is_refusal(*a)) {
        return Hit {
            zone: zone_leak(zone),
            status: Status::Unknown,
            codes: addrs.iter().map(|a| a.to_string()).collect(),
            meaning: format!(
                "{zone_name} TỪ CHỐI truy vấn (mã {}). Nguyên nhân gần như luôn là máy \
                 này dùng resolver công cộng (8.8.8.8 / 1.1.1.1) — Spamhaus chặn các \
                 resolver đó. Đây KHÔNG phải kết luận IP bẩn; đổi sang resolver riêng \
                 rồi tra lại.",
                addrs
                    .iter()
                    .map(|a| a.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        };
    }

    let codes: Vec<String> = addrs
        .iter()
        .filter(|a| !is_refusal(**a))
        .map(|a| a.to_string())
        .collect();
    let mut why: Vec<String> = vec![];
    if zone == "zen.spamhaus.org" {
        for c in &codes {
            if let Some(m) = decode_zen(c) {
                why.push(m.to_string());
            }
        }
    }
    let meaning = if why.is_empty() {
        format!("CÓ trong {zone_name} (mã {}).", codes.join(", "))
    } else {
        format!("CÓ trong {zone_name}: {}", why.join("; "))
    };
    Hit {
        zone: zone_leak(zone),
        status: Status::Listed,
        codes,
        meaning,
    }
}

/// `ZONES` là hằng nên tên zone luôn có tuổi thọ `'static`; hàm này chỉ để lấy
/// lại tham chiếu đó thay vì cấp phát chuỗi mới cho mỗi lần tra.
fn zone_leak(zone: &str) -> &'static str {
    ZONES
        .iter()
        .find(|(z, _)| *z == zone)
        .map(|(z, _)| *z)
        .unwrap_or("unknown")
}

pub async fn check(ip: IpAddr) -> Vec<Hit> {
    let Some(rev) = crate::resolve::reverse_octets(ip) else {
        // Các DNSBL lớn phủ IPv6 rất mỏng, và định dạng truy vấn khác hẳn.
        // Trả rỗng còn hơn tra sai rồi báo "sạch".
        return vec![];
    };
    let Ok(r) = crate::resolve::resolver() else {
        return vec![];
    };

    let mut out = vec![];
    for (zone, name) in ZONES {
        let q = crate::resolve::fqdn(&format!("{rev}.{zone}"));
        match r.ipv4_lookup(q).await {
            Ok(l) => {
                let addrs: Vec<Ivp4> = l.iter().map(|a| Ipv4Addr::from(a.0)).collect();
                out.push(interpret(zone, &addrs));
            }
            Err(e) => {
                use hickory_resolver::error::ResolveErrorKind;
                // NXDOMAIN = sạch, và đó là câu trả lời dứt khoát.
                if matches!(e.kind(), ResolveErrorKind::NoRecordsFound { .. }) {
                    out.push(interpret(zone, &[]));
                } else {
                    out.push(Hit {
                        zone: zone_leak(zone),
                        status: Status::Unknown,
                        codes: vec![],
                        meaning: format!(
                            "Không tra được {name}: {e}. Đây là lỗi tra cứu, KHÔNG phải kết luận sạch."
                        ),
                    });
                }
            }
        }
    }
    out
}

type Ivp4 = Ipv4Addr;

pub fn to_json(hits: &[Hit]) -> Value {
    let listed = hits.iter().filter(|h| h.status == Status::Listed).count();
    let unknown = hits.iter().filter(|h| h.status == Status::Unknown).count();
    json!({
        "listed_count": listed,
        "unknown_count": unknown,
        "checked": hits.len(),
        "results": hits.iter().map(|h| json!({
            "zone": h.zone,
            "status": h.status.as_str(),
            "codes": h.codes,
            "meaning": h.meaning,
        })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v4(s: &str) -> Ipv4Addr {
        s.parse().unwrap()
    }

    #[test]
    fn a_refusal_code_is_never_read_as_listed() {
        // Bẫy số 1: 127.255.255.254 nằm trong 127/8 nên bộ kiểm ngây thơ đọc
        // thành "listed" → báo động giả trên MỌI IP sạch của máy dùng 8.8.8.8.
        let h = interpret("zen.spamhaus.org", &[v4("127.255.255.254")]);
        assert_eq!(h.status, Status::Unknown);
        assert!(h.meaning.contains("TỪ CHỐI"));
        assert!(h.meaning.contains("resolver"));
        assert!(is_refusal(v4("127.255.255.252")));
        assert!(!is_refusal(v4("127.0.0.2")));
    }

    #[test]
    fn no_records_means_clean_and_says_so() {
        let h = interpret("zen.spamhaus.org", &[]);
        assert_eq!(h.status, Status::Clean);
        assert!(h.codes.is_empty());
    }

    #[test]
    fn zen_codes_are_decoded_into_reasons() {
        let h = interpret("zen.spamhaus.org", &[v4("127.0.0.4")]);
        assert_eq!(h.status, Status::Listed);
        assert!(h.meaning.contains("XBL"));
        assert!(h.meaning.contains("chiếm quyền"));
    }

    #[test]
    fn pbl_is_surfaced_as_an_infrastructure_signal_not_just_a_reputation_hit() {
        // PBL nói "đây là IP dân dụng" — thông tin hạ tầng thật sự, đáng giá hơn
        // điểm trừ danh tiếng.
        let h = interpret("zen.spamhaus.org", &[v4("127.0.0.11")]);
        assert!(h.meaning.contains("dân dụng"));
    }

    #[test]
    fn a_mix_of_refusal_and_real_codes_keeps_only_the_real_ones() {
        let h = interpret("zen.spamhaus.org", &[v4("127.255.255.254"), v4("127.0.0.2")]);
        assert_eq!(h.status, Status::Listed);
        assert_eq!(h.codes, vec!["127.0.0.2"]);
    }

    #[test]
    fn unknown_codes_from_other_zones_still_report_listed() {
        // SpamCop không dùng bảng mã của Spamhaus; không giải mã được vẫn phải
        // báo là có trong danh sách.
        let h = interpret("bl.spamcop.net", &[v4("127.0.0.2")]);
        assert_eq!(h.status, Status::Listed);
        assert!(h.meaning.contains("SpamCop"));
    }

    #[test]
    fn summary_counts_unknown_separately_from_listed() {
        let hits = vec![
            interpret("zen.spamhaus.org", &[v4("127.255.255.254")]),
            interpret("bl.spamcop.net", &[v4("127.0.0.2")]),
            interpret("dnsbl.sorbs.net", &[]),
        ];
        let j = to_json(&hits);
        assert_eq!(j["listed_count"], 1);
        assert_eq!(j["unknown_count"], 1);
        assert_eq!(j["checked"], 3);
    }
}
