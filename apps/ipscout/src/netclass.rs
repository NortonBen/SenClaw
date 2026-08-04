//! "Traffic đi qua đâu" — phân loại mạng đứng sau một IP: CDN, cloud, hosting,
//! hay ISP dân dụng.
//!
//! Đây là kết luận **quan trọng nhất** của cả phần hồ sơ, vì nó quyết định mọi
//! kết luận khác nói về ai. Khi IP thuộc CDN, thứ ta chạm tới là **biên của
//! CDN chứ không phải máy chủ gốc**: cổng mở là cổng của Cloudflare, banner là
//! banner của Cloudflare, hệ điều hành là của Cloudflare. Không nói rõ điều đó
//! thì cả bản báo cáo mô tả sai đối tượng — và người đọc sẽ đi vá nhầm máy.
//!
//! Nhận diện theo **hai** đường: số ASN, và từ khoá trong tên tổ chức. Chỉ dựa
//! vào danh sách ASN thì danh sách sẽ cũ đi (nhà cung cấp mua thêm AS liên tục);
//! chỉ dựa vào tên thì trượt khi RIR ghi tên pháp nhân thay vì tên thương hiệu.

use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Cdn,
    Cloud,
    Hosting,
    Isp,
    Unknown,
}

impl Kind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Cdn => "cdn",
            Self::Cloud => "cloud",
            Self::Hosting => "hosting",
            Self::Isp => "isp",
            Self::Unknown => "unknown",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Cdn => "CDN / biên",
            Self::Cloud => "Nhà cung cấp cloud",
            Self::Hosting => "Hosting / VPS",
            Self::Isp => "ISP / mạng dân dụng",
            Self::Unknown => "Chưa phân loại",
        }
    }
}

#[derive(Debug, Clone)]
pub struct NetClass {
    pub kind: Kind,
    pub provider: Option<String>,
    /// IP anycast: cùng địa chỉ được quảng bá từ nhiều nơi. Địa lý mất nghĩa.
    pub anycast: bool,
    /// Máy chủ thấy được KHÔNG phải máy chủ gốc.
    pub fronted: bool,
    pub reason: String,
}

impl Default for NetClass {
    fn default() -> Self {
        Self {
            kind: Kind::Unknown,
            provider: None,
            anycast: false,
            fronted: false,
            reason: "Không đủ dữ liệu ASN/tổ chức để phân loại.".into(),
        }
    }
}

/// (ASN, tên hiển thị, loại). Danh sách cố tình ngắn: chỉ những nhà cung cấp mà
/// nhầm lẫn sẽ dẫn tới kết luận sai về hạ tầng. Phần còn lại để từ khoá lo.
const KNOWN_ASN: &[(u32, &str, Kind)] = &[
    (13335, "Cloudflare", Kind::Cdn),
    (209242, "Cloudflare", Kind::Cdn),
    (20940, "Akamai", Kind::Cdn),
    (16625, "Akamai", Kind::Cdn),
    (32787, "Akamai", Kind::Cdn),
    (63949, "Akamai (Linode)", Kind::Cloud),
    (54113, "Fastly", Kind::Cdn),
    (22822, "Edgio (Limelight)", Kind::Cdn),
    (19551, "Imperva Incapsula", Kind::Cdn),
    (60068, "CDN77", Kind::Cdn),
    (16509, "Amazon AWS", Kind::Cloud),
    (14618, "Amazon AWS", Kind::Cloud),
    (15169, "Google", Kind::Cloud),
    (396982, "Google Cloud", Kind::Cloud),
    (8075, "Microsoft Azure", Kind::Cloud),
    (45102, "Alibaba Cloud", Kind::Cloud),
    (37963, "Alibaba Cloud", Kind::Cloud),
    (132203, "Tencent Cloud", Kind::Cloud),
    (55990, "Huawei Cloud", Kind::Cloud),
    (14061, "DigitalOcean", Kind::Hosting),
    (20473, "Vultr", Kind::Hosting),
    (24940, "Hetzner", Kind::Hosting),
    (16276, "OVH", Kind::Hosting),
    (51167, "Contabo", Kind::Hosting),
    (7552, "Viettel", Kind::Isp),
    (45899, "VNPT", Kind::Isp),
    (18403, "FPT Telecom", Kind::Isp),
    (135905, "Viettel IDC", Kind::Hosting),
];

/// Từ khoá → (tên hiển thị, loại). Khớp không phân biệt hoa thường trên tên tổ
/// chức RDAP, tên AS của Cymru và tên PTR gộp lại.
const KEYWORDS: &[(&str, &str, Kind)] = &[
    ("cloudflare", "Cloudflare", Kind::Cdn),
    ("akamai", "Akamai", Kind::Cdn),
    ("fastly", "Fastly", Kind::Cdn),
    ("cloudfront", "Amazon CloudFront", Kind::Cdn),
    ("incapsula", "Imperva Incapsula", Kind::Cdn),
    ("stackpath", "StackPath", Kind::Cdn),
    ("bunnycdn", "Bunny CDN", Kind::Cdn),
    ("bunny.net", "Bunny CDN", Kind::Cdn),
    ("edgecast", "Edgecast", Kind::Cdn),
    ("limelight", "Edgio", Kind::Cdn),
    ("sucuri", "Sucuri", Kind::Cdn),
    ("amazon", "Amazon AWS", Kind::Cloud),
    ("aws", "Amazon AWS", Kind::Cloud),
    ("google", "Google Cloud", Kind::Cloud),
    ("microsoft", "Microsoft Azure", Kind::Cloud),
    ("azure", "Microsoft Azure", Kind::Cloud),
    ("alibaba", "Alibaba Cloud", Kind::Cloud),
    ("aliyun", "Alibaba Cloud", Kind::Cloud),
    ("tencent", "Tencent Cloud", Kind::Cloud),
    ("huawei", "Huawei Cloud", Kind::Cloud),
    ("oracle", "Oracle Cloud", Kind::Cloud),
    ("digitalocean", "DigitalOcean", Kind::Hosting),
    ("linode", "Linode", Kind::Hosting),
    ("vultr", "Vultr", Kind::Hosting),
    ("choopa", "Vultr", Kind::Hosting),
    ("hetzner", "Hetzner", Kind::Hosting),
    ("ovh", "OVH", Kind::Hosting),
    ("contabo", "Contabo", Kind::Hosting),
    ("scaleway", "Scaleway", Kind::Hosting),
    ("leaseweb", "Leaseweb", Kind::Hosting),
    ("godaddy", "GoDaddy", Kind::Hosting),
    ("hostinger", "Hostinger", Kind::Hosting),
    ("namecheap", "Namecheap", Kind::Hosting),
    ("viettel", "Viettel", Kind::Isp),
    ("vnpt", "VNPT", Kind::Isp),
    ("fpt", "FPT Telecom", Kind::Isp),
    ("cmc tele", "CMC Telecom", Kind::Isp),
    ("netnam", "NetNam", Kind::Isp),
    ("broadband", "ISP dân dụng", Kind::Isp),
    ("telecom", "Nhà mạng viễn thông", Kind::Isp),
];

/// Phân loại từ ASN + các chuỗi mô tả (tên AS, org RDAP, PTR).
///
/// ASN thắng khi có khớp, vì số AS là danh tính không mơ hồ; từ khoá chỉ là
/// mạng lưới hứng phần còn lại.
pub fn classify(asn: Option<u32>, hints: &[Option<String>]) -> NetClass {
    if let Some(n) = asn {
        if let Some((_, name, kind)) = KNOWN_ASN.iter().find(|(a, _, _)| *a == n) {
            return build(*kind, name, format!("AS{n} là của {name}"));
        }
    }

    let hay = hints
        .iter()
        .flatten()
        .map(|s| s.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    if !hay.is_empty() {
        if let Some((kw, name, kind)) = KEYWORDS.iter().find(|(k, _, _)| hay.contains(k)) {
            return build(
                *kind,
                name,
                format!("tên tổ chức/PTR chứa \"{kw}\" → {name}"),
            );
        }
    }

    NetClass::default()
}

fn build(kind: Kind, provider: &str, why: String) -> NetClass {
    let (anycast, fronted) = match kind {
        Kind::Cdn => (true, true),
        _ => (false, false),
    };
    let reason = if fronted {
        format!(
            "{why}. IP này là **biên của CDN, không phải máy chủ gốc** — cổng mở, \
             banner và hệ điều hành đọc được đều là của {provider}. Muốn biết hạ tầng \
             thật thì phải điều tra từ bên trong, không tra từ ngoài vào được."
        )
    } else {
        why
    };
    NetClass {
        kind,
        provider: Some(provider.to_string()),
        anycast,
        fronted,
        reason,
    }
}

pub fn to_json(c: &NetClass) -> Value {
    json!({
        "kind": c.kind.as_str(),
        "label": c.kind.label(),
        "provider": c.provider,
        "anycast": c.anycast,
        "fronted": c.fronted,
        "reason": c.reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(s: &str) -> Vec<Option<String>> {
        vec![Some(s.to_string())]
    }

    #[test]
    fn a_cdn_asn_marks_the_ip_as_fronted_and_anycast() {
        // Đây là lý do module tồn tại: không gắn cờ này thì cả báo cáo về sau
        // mô tả Cloudflare mà người đọc tưởng là máy chủ của họ.
        let c = classify(Some(13335), &[]);
        assert_eq!(c.kind, Kind::Cdn);
        assert!(c.anycast && c.fronted);
        assert_eq!(c.provider.as_deref(), Some("Cloudflare"));
        assert!(c.reason.contains("không phải máy chủ gốc"));
    }

    #[test]
    fn cloud_and_hosting_are_not_flagged_as_fronted() {
        // AWS EC2 là máy của người dùng — quét nó là quét đúng đối tượng.
        for (asn, want) in [(16509, Kind::Cloud), (14061, Kind::Hosting), (7552, Kind::Isp)] {
            let c = classify(Some(asn), &[]);
            assert_eq!(c.kind, want, "AS{asn}");
            assert!(!c.fronted && !c.anycast, "AS{asn} không được coi là CDN");
        }
    }

    #[test]
    fn keywords_catch_providers_whose_asn_is_not_in_the_table() {
        // Danh sách ASN chắc chắn sẽ cũ đi — từ khoá là lưới hứng.
        let c = classify(Some(999999), &h("BUNNYCDN Ltd"));
        assert_eq!(c.kind, Kind::Cdn);
        assert_eq!(c.provider.as_deref(), Some("Bunny CDN"));
        let d = classify(None, &h("Hetzner Online GmbH"));
        assert_eq!(d.kind, Kind::Hosting);
    }

    #[test]
    fn a_known_asn_beats_a_misleading_org_string() {
        // RDAP hay ghi tên khách thuê chứ không phải chủ dải; số AS thì không mơ hồ.
        let c = classify(Some(13335), &h("Some Reseller Telecom Co"));
        assert_eq!(c.provider.as_deref(), Some("Cloudflare"));
        assert_eq!(c.kind, Kind::Cdn);
    }

    #[test]
    fn ptr_names_are_usable_hints_too() {
        let c = classify(None, &[None, Some("ec2-1-2-3-4.compute.amazonaws.com".into())]);
        assert_eq!(c.kind, Kind::Cloud);
        assert_eq!(c.provider.as_deref(), Some("Amazon AWS"));
    }

    #[test]
    fn nothing_to_go_on_stays_unknown_instead_of_guessing() {
        let c = classify(None, &[]);
        assert_eq!(c.kind, Kind::Unknown);
        assert!(c.provider.is_none());
        assert!(!c.fronted);
        // và cả khi có chuỗi nhưng không khớp từ khoá nào
        let d = classify(Some(64999), &h("Private Customer Network"));
        assert_eq!(d.kind, Kind::Unknown);
    }

    #[test]
    fn matching_ignores_case() {
        for s in ["CLOUDFLARE", "cloudflare", "CloudFlare, Inc."] {
            assert_eq!(classify(None, &h(s)).kind, Kind::Cdn, "{s}");
        }
    }
}
