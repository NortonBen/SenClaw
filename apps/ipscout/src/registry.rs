//! "IP này của ai" — ASN + dải mạng + tổ chức + liên hệ abuse.
//!
//! Hai nguồn, cả hai đều **không gửi gói nào tới mục tiêu**:
//!
//! * **Team Cymru qua DNS** cho ASN. `4.3.2.1.origin.asn.cymru.com` TXT trả về
//!   `"13335 | 1.1.1.0/24 | AU | apnic | 2011-08-11"`. Đây là cách chuẩn của
//!   ngành để tra ASN: không khoá API, không hạn mức, và vì là DNS nên đi qua
//!   cache của resolver. RDAP **không** đảm bảo trả ASN, nên không thay được.
//! * **RDAP** cho dải được cấp, tên tổ chức và email abuse. RDAP là bản kế thừa
//!   của WHOIS: trả JSON có cấu trúc thay vì văn bản tự do mỗi RIR một kiểu.
//!
//! Phần phân tích tách khỏi phần truy vấn — mọi hàm `parse_*` là hàm thuần và
//! test được không cần mạng.

use crate::scope;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::net::IpAddr;

#[derive(Debug, Default, Clone)]
pub struct AsnInfo {
    pub asn: Option<u32>,
    pub prefix: Option<String>,
    pub country: Option<String>,
    pub rir: Option<String>,
    pub allocated: Option<String>,
    pub org: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub struct RdapInfo {
    pub handle: Option<String>,
    pub name: Option<String>,
    pub range: Option<String>,
    pub cidr: Option<String>,
    pub country: Option<String>,
    pub kind: Option<String>,
    pub registered: Option<String>,
    pub changed: Option<String>,
    pub org: Option<String>,
    pub abuse_email: Option<String>,
    pub abuse_name: Option<String>,
}

// ---------------------------------------------------------------------------
// Team Cymru (DNS)
// ---------------------------------------------------------------------------

/// `"13335 | 1.1.1.0/24 | AU | apnic | 2011-08-11"`.
///
/// Một IP có thể được quảng bá bởi **nhiều** AS (multi-homing, anycast) — khi đó
/// trường đầu là `"13335 3356"`. Lấy AS đầu tiên và giữ nguyên chuỗi gốc để
/// người đọc thấy được, thay vì im lặng vứt phần còn lại.
pub fn parse_origin_txt(txt: &str) -> AsnInfo {
    let p: Vec<&str> = txt.split('|').map(|s| s.trim()).collect();
    let mut i = AsnInfo::default();
    if let Some(first) = p.first() {
        i.asn = first.split_whitespace().next().and_then(|a| a.parse().ok());
    }
    i.prefix = p.get(1).filter(|s| !s.is_empty()).map(|s| s.to_string());
    i.country = p.get(2).filter(|s| !s.is_empty()).map(|s| s.to_string());
    i.rir = p.get(3).filter(|s| !s.is_empty()).map(|s| s.to_string());
    i.allocated = p.get(4).filter(|s| !s.is_empty()).map(|s| s.to_string());
    i
}

/// `"13335 | US | arin | 2010-07-14 | CLOUDFLARENET, US"` → tên tổ chức.
pub fn parse_asname_txt(txt: &str) -> Option<String> {
    let p: Vec<&str> = txt.split('|').map(|s| s.trim()).collect();
    p.get(4).filter(|s| !s.is_empty()).map(|s| s.to_string())
}

pub async fn asn_of(ip: IpAddr) -> Result<AsnInfo> {
    let Some(rev) = crate::resolve::reverse_octets(ip) else {
        // IPv6 dùng zone khác (origin6.asn.cymru.com) với định dạng nibble.
        return asn_of_v6(ip).await;
    };
    let r = crate::resolve::resolver()?;
    let recs = r
        .txt_lookup(crate::resolve::fqdn(&format!("{rev}.origin.asn.cymru.com")))
        .await
        .map_err(|e| anyhow!("không tra được ASN của {ip}: {e}"))?;
    let first = recs
        .iter()
        .map(join_txt)
        .next()
        .ok_or_else(|| anyhow!("Cymru không trả bản ghi ASN cho {ip}"))?;
    let mut info = parse_origin_txt(&first);
    if let Some(asn) = info.asn {
        info.org = as_name(asn).await.ok().flatten();
    }
    Ok(info)
}

async fn asn_of_v6(ip: IpAddr) -> Result<AsnInfo> {
    let IpAddr::V6(v6) = ip else {
        return Err(anyhow!("không phải IPv6"));
    };
    // Nibble đảo, giống ip6.arpa: mỗi nửa byte một nhãn.
    let nibbles: String = v6
        .octets()
        .iter()
        .flat_map(|b| [b >> 4, b & 0x0f])
        .rev()
        .map(|n| format!("{n:x}."))
        .collect();
    let r = crate::resolve::resolver()?;
    let recs = r
        .txt_lookup(crate::resolve::fqdn(&format!(
            "{nibbles}origin6.asn.cymru.com"
        )))
        .await
        .map_err(|e| anyhow!("không tra được ASN của {ip}: {e}"))?;
    let first = recs
        .iter()
        .map(join_txt)
        .next()
        .ok_or_else(|| anyhow!("Cymru không trả bản ghi ASN cho {ip}"))?;
    let mut info = parse_origin_txt(&first);
    if let Some(asn) = info.asn {
        info.org = as_name(asn).await.ok().flatten();
    }
    Ok(info)
}

pub async fn as_name(asn: u32) -> Result<Option<String>> {
    let r = crate::resolve::resolver()?;
    let recs = r
        .txt_lookup(crate::resolve::fqdn(&format!("AS{asn}.asn.cymru.com")))
        .await
        .map_err(|e| anyhow!("không tra được tên AS{asn}: {e}"))?;
    Ok(recs.iter().map(join_txt).next().and_then(|t| parse_asname_txt(&t)))
}

fn join_txt(t: &hickory_resolver::proto::rr::rdata::TXT) -> String {
    t.iter()
        .map(|b| String::from_utf8_lossy(b).to_string())
        .collect()
}

// ---------------------------------------------------------------------------
// RDAP
// ---------------------------------------------------------------------------

/// Rút một trường vCard. Định dạng jCard là mảng lồng nhau:
/// `["vcard", [["fn", {}, "text", "Cloudflare, Inc."], ["email", {}, "text", "a@b.c"]]]`
pub fn vcard_field(vcard: &Value, key: &str) -> Option<String> {
    let entries = vcard.get(1)?.as_array()?;
    entries.iter().find_map(|e| {
        let a = e.as_array()?;
        (a.first()?.as_str()? == key)
            .then(|| a.get(3)?.as_str().map(|s| s.to_string()))
            .flatten()
    })
}

/// Đi đệ quy qua cây `entities` tìm vai trò cần. RIR lồng liên hệ abuse **bên
/// trong** entity của tổ chức chứ không để ở tầng trên — chỉ quét một tầng là
/// mất abuse email của phần lớn dải ARIN/RIPE.
fn find_role(entities: &Value, role: &str, depth: usize) -> Option<Value> {
    if depth > 4 {
        return None;
    }
    for e in entities.as_array()? {
        if e.get("roles")
            .and_then(|r| r.as_array())
            .map(|r| r.iter().any(|x| x.as_str() == Some(role)))
            .unwrap_or(false)
        {
            return Some(e.clone());
        }
        if let Some(found) = e.get("entities").and_then(|n| find_role(n, role, depth + 1)) {
            return Some(found);
        }
    }
    None
}

fn event_date(v: &Value, action: &str) -> Option<String> {
    v.get("events")?.as_array()?.iter().find_map(|e| {
        (e.get("eventAction")?.as_str()? == action)
            .then(|| e.get("eventDate")?.as_str().map(|s| s.to_string()))
            .flatten()
    })
}

pub fn parse_rdap(v: &Value) -> RdapInfo {
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(|x| x.to_string());
    let mut i = RdapInfo {
        handle: s("handle"),
        name: s("name"),
        country: s("country"),
        kind: s("type"),
        registered: event_date(v, "registration"),
        changed: event_date(v, "last changed"),
        ..Default::default()
    };
    if let (Some(a), Some(b)) = (s("startAddress"), s("endAddress")) {
        i.range = Some(format!("{a} – {b}"));
    }
    // `cidr0_cidrs` là phần mở rộng RDAP; không phải RIR nào cũng trả.
    if let Some(c) = v.get("cidr0_cidrs").and_then(|x| x.as_array()).and_then(|a| a.first()) {
        let pfx = c
            .get("v4prefix")
            .or_else(|| c.get("v6prefix"))
            .and_then(|x| x.as_str());
        let len = c.get("length").and_then(|x| x.as_u64());
        if let (Some(p), Some(l)) = (pfx, len) {
            i.cidr = Some(format!("{p}/{l}"));
        }
    }
    if let Some(ents) = v.get("entities") {
        if let Some(reg) = find_role(ents, "registrant", 0).or_else(|| find_role(ents, "administrative", 0)) {
            i.org = reg.get("vcardArray").and_then(|c| vcard_field(c, "fn"));
        }
        if let Some(ab) = find_role(ents, "abuse", 0) {
            if let Some(c) = ab.get("vcardArray") {
                i.abuse_email = vcard_field(c, "email");
                i.abuse_name = vcard_field(c, "fn");
            }
        }
    }
    i
}

/// Tra RDAP qua `rdap.org` — dịch vụ bootstrap chuyển hướng sang đúng RIR.
///
/// reqwest tự đi theo redirect; đích cuối vẫn là một RIR công khai. Bộ chặn
/// SSRF không áp ở đây vì URL do app dựng từ hằng số, không nhận từ dữ liệu ngoài.
pub async fn rdap(http: &reqwest::Client, ip: IpAddr) -> Result<RdapInfo> {
    let url = format!("https://rdap.org/ip/{ip}");
    let resp = http
        .get(&url)
        .header("Accept", "application/rdap+json")
        .send()
        .await
        .map_err(|e| anyhow!("RDAP không phản hồi: {e}"))?;
    if !resp.status().is_success() {
        return Err(anyhow!("RDAP trả {}", resp.status().as_u16()));
    }
    let v: Value = resp
        .json()
        .await
        .map_err(|e| anyhow!("RDAP trả dữ liệu không đọc được: {e}"))?;
    Ok(parse_rdap(&v))
}

pub fn asn_json(a: &AsnInfo) -> Value {
    json!({
        "asn": a.asn,
        "as_name": a.org,
        "prefix": a.prefix,
        "country": a.country,
        "rir": a.rir,
        "allocated": a.allocated,
    })
}

pub fn rdap_json(r: &RdapInfo) -> Value {
    json!({
        "handle": r.handle,
        "net_name": r.name,
        "range": r.range,
        "cidr": r.cidr,
        "country": r.country,
        "type": r.kind,
        "registered": r.registered,
        "last_changed": r.changed,
        "org": r.org,
        "abuse_email": r.abuse_email,
        "abuse_name": r.abuse_name,
    })
}

/// Đích RDAP/GeoIP có bị bộ chặn SSRF từ chối không. Gọi trước khi tra bất cứ
/// nguồn ngoài nào để không dùng app làm bàn đạp vào mạng nội bộ.
pub fn public_or_err(ip: IpAddr) -> Result<()> {
    match scope::is_blocked_ip(ip) {
        Some(why) => Err(anyhow!(
            "{ip} nằm trong dải đặc biệt ({why}) — không có bản ghi đăng ký công khai"
        )),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cymru_origin_record_is_parsed() {
        let i = parse_origin_txt("13335 | 1.1.1.0/24 | AU | apnic | 2011-08-11");
        assert_eq!(i.asn, Some(13335));
        assert_eq!(i.prefix.as_deref(), Some("1.1.1.0/24"));
        assert_eq!(i.country.as_deref(), Some("AU"));
        assert_eq!(i.rir.as_deref(), Some("apnic"));
        assert_eq!(i.allocated.as_deref(), Some("2011-08-11"));
    }

    #[test]
    fn multi_homed_prefix_takes_the_first_as_not_a_parse_error() {
        // Anycast/multi-homing trả nhiều AS trong cùng trường — không được sập.
        let i = parse_origin_txt("13335 3356 | 1.1.1.0/24 | AU | apnic | 2011-08-11");
        assert_eq!(i.asn, Some(13335));
    }

    #[test]
    fn cymru_asname_record_is_parsed() {
        let n = parse_asname_txt("13335 | US | arin | 2010-07-14 | CLOUDFLARENET, US");
        assert_eq!(n.as_deref(), Some("CLOUDFLARENET, US"));
        // thiếu trường tên thì trả None chứ không trả chuỗi rỗng
        assert!(parse_asname_txt("13335 | US | arin | 2010-07-14 |").is_none());
    }

    #[test]
    fn vcard_fields_are_extracted_from_the_nested_jcard_shape() {
        let vc = json!(["vcard", [
            ["version", {}, "text", "4.0"],
            ["fn", {}, "text", "Cloudflare, Inc."],
            ["email", {}, "text", "abuse@cloudflare.com"]
        ]]);
        assert_eq!(vcard_field(&vc, "fn").as_deref(), Some("Cloudflare, Inc."));
        assert_eq!(vcard_field(&vc, "email").as_deref(), Some("abuse@cloudflare.com"));
        assert!(vcard_field(&vc, "tel").is_none());
    }

    #[test]
    fn rdap_response_is_parsed_including_cidr_and_dates() {
        let v = json!({
            "handle": "104.16.0.0 - 104.31.255.255",
            "startAddress": "104.16.0.0",
            "endAddress": "104.31.255.255",
            "name": "CLOUDFLARENET",
            "type": "DIRECT ALLOCATION",
            "country": "US",
            "cidr0_cidrs": [{ "v4prefix": "104.16.0.0", "length": 12 }],
            "events": [
                { "eventAction": "registration", "eventDate": "2014-03-28T00:00:00-04:00" },
                { "eventAction": "last changed", "eventDate": "2021-11-02T00:00:00-04:00" }
            ],
            "entities": [{
                "handle": "CLOUD14", "roles": ["registrant"],
                "vcardArray": ["vcard", [["fn", {}, "text", "Cloudflare, Inc."]]]
            }]
        });
        let r = parse_rdap(&v);
        assert_eq!(r.name.as_deref(), Some("CLOUDFLARENET"));
        assert_eq!(r.cidr.as_deref(), Some("104.16.0.0/12"));
        assert_eq!(r.range.as_deref(), Some("104.16.0.0 – 104.31.255.255"));
        assert_eq!(r.org.as_deref(), Some("Cloudflare, Inc."));
        assert!(r.registered.unwrap().starts_with("2014-03-28"));
        assert!(r.changed.unwrap().starts_with("2021-11-02"));
    }

    #[test]
    fn abuse_contact_nested_two_levels_deep_is_still_found() {
        // Bẫy thật: ARIN/RIPE lồng entity abuse BÊN TRONG entity tổ chức.
        // Quét một tầng là mất abuse email của phần lớn dải.
        let v = json!({
            "entities": [{
                "roles": ["registrant"],
                "vcardArray": ["vcard", [["fn", {}, "text", "Example Org"]]],
                "entities": [{
                    "roles": ["abuse"],
                    "vcardArray": ["vcard", [
                        ["fn", {}, "text", "Abuse Desk"],
                        ["email", {}, "text", "abuse@example.net"]
                    ]]
                }]
            }]
        });
        let r = parse_rdap(&v);
        assert_eq!(r.abuse_email.as_deref(), Some("abuse@example.net"));
        assert_eq!(r.abuse_name.as_deref(), Some("Abuse Desk"));
        assert_eq!(r.org.as_deref(), Some("Example Org"));
    }

    #[test]
    fn an_empty_rdap_body_yields_empty_fields_not_a_panic() {
        let r = parse_rdap(&json!({}));
        assert!(r.name.is_none() && r.cidr.is_none() && r.abuse_email.is_none());
    }

    #[test]
    fn special_ranges_are_rejected_before_any_external_lookup() {
        assert!(public_or_err("8.8.8.8".parse().unwrap()).is_ok());
        assert!(public_or_err("10.0.0.1".parse().unwrap()).is_err());
        assert!(public_or_err("169.254.169.254".parse().unwrap()).is_err());
    }
}
