//! Rào chắn phạm vi: xác minh quyền sở hữu tài sản, và chặn scanner tự biến
//! thành công cụ tấn công (SSRF / metadata cloud / DNS rebinding).
//!
//! Chưa xác minh thì L2/L3 **trả lỗi**, không phải cảnh báo. L1 (thụ động) được
//! phép chạy trước khi xác minh — theo thông lệ của Snyk API & Web
//! ("Lightning scan") và Pentest-Tools ("Light scan"): quan sát thụ động không
//! gửi payload nào, chỉ dò chủ động mới cần bằng chứng sở hữu.

use anyhow::{anyhow, Result};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

pub const TXT_PREFIX: &str = "senclaw-verify=";
pub const WELL_KNOWN_PATH: &str = "/.well-known/senclaw-verify";
pub const META_NAME: &str = "senclaw-verify";

/// Token 32 ký tự hex có nhúng asset id. Token ngẫu nhiên trần thì ai đọc được
/// zone cũng chép sang tenant của mình được — nhúng id để chống chuyện đó.
pub fn gen_token(asset_id: i64) -> String {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let stack_salt = &t as *const _ as usize;
    let mut h: u128 = 0xcbf29ce484222325;
    for b in t
        .to_le_bytes()
        .iter()
        .chain(stack_salt.to_le_bytes().iter())
        .chain(asset_id.to_le_bytes().iter())
    {
        h ^= *b as u128;
        h = h.wrapping_mul(0x100000001b3);
    }
    // hai vòng trộn để phần cao cũng đủ khuếch tán
    let lo = h as u64;
    let hi = (h >> 64) as u64 ^ (asset_id as u64).wrapping_mul(0x9e3779b97f4a7c15);
    format!("{hi:016x}{lo:016x}")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// TXT ở apex — mạnh nhất, đòi quyền sửa zone.
    DnsTxt,
    /// CNAME — **bắt buộc phải có**: apex bị CNAME-flatten (Cloudflare, Netlify…)
    /// không mang thêm được bản ghi TXT, đó là giới hạn của DNS.
    DnsCname,
    /// Tệp dưới /.well-known/ — chứng minh quyền web server, không phải quyền domain.
    WellKnown,
    /// Thẻ meta — yếu nhất: chèn được HTML là giả được.
    Meta,
    /// Đích trong dải nội bộ, người dùng chủ động khai.
    Local,
}

impl Method {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "dns-txt" => Self::DnsTxt,
            "dns-cname" => Self::DnsCname,
            "well-known" => Self::WellKnown,
            "meta" => Self::Meta,
            "local" => Self::Local,
            _ => return None,
        })
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::DnsTxt => "dns-txt",
            Self::DnsCname => "dns-cname",
            Self::WellKnown => "well-known",
            Self::Meta => "meta",
            Self::Local => "local",
        }
    }

    pub fn instructions(&self, host: &str, token: &str) -> String {
        match self {
            Self::DnsTxt => format!(
                "Thêm bản ghi TXT ở apex của {host}:\n  {host}. IN TXT \"{TXT_PREFIX}{token}\"\n\
                 Giữ nguyên bản ghi — mất nó là phạm vi bị thu hồi."
            ),
            Self::DnsCname => format!(
                "Thêm CNAME (dùng khi apex đã CNAME-flatten nên không gắn TXT được):\n  \
                 senclaw-{token}.{host}. IN CNAME verify.senclaw.local."
            ),
            Self::WellKnown => format!(
                "Đặt tệp tại https://{host}{WELL_KNOWN_PATH}, nội dung đúng bằng:\n  {token}"
            ),
            Self::Meta => format!(
                "Thêm vào <head> của https://{host}/:\n  \
                 <meta name=\"{META_NAME}\" content=\"{token}\">"
            ),
            Self::Local => "Đích nằm trong dải mạng nội bộ — tự khai là đủ.".to_string(),
        }
    }
}

/// Rút host từ target (`https://a.vn/x` → `a.vn`, `ssh://u@1.2.3.4:22` → `1.2.3.4`).
pub fn host_of(target: &str) -> Result<String> {
    let t = target.trim();
    if let Ok(u) = url::Url::parse(t) {
        if let Some(h) = u.host_str() {
            return Ok(h.to_string());
        }
    }
    let h = t.split('/').next().unwrap_or(t);
    let h = h.rsplit('@').next().unwrap_or(h);
    let h = h.split(':').next().unwrap_or(h);
    if h.is_empty() {
        return Err(anyhow!("không rút được host từ '{target}'"));
    }
    Ok(h.to_string())
}

// ---------------------------------------------------------------------------
// Chặn SSRF / metadata cloud
// ---------------------------------------------------------------------------

/// IP có bị cấm chạm tới không, kèm lý do.
///
/// Dùng **toàn bộ** registry special-purpose của IANA chứ không bốc vài dải —
/// danh sách tự chế luôn thiếu chỗ nào đó. Riêng `168.63.129.16` của Azure
/// trông y hệt IP công cộng nên lọt qua mọi bộ lọc "dải riêng" ngây thơ.
pub fn is_blocked_ip(ip: IpAddr) -> Option<&'static str> {
    match ip {
        IpAddr::V4(v4) => is_blocked_v4(v4),
        IpAddr::V6(v6) => is_blocked_v6(v6),
    }
}

fn is_blocked_v4(ip: Ipv4Addr) -> Option<&'static str> {
    let o = ip.octets();
    // Điểm cuối metadata trước — chúng mới là mục tiêu chính của SSRF.
    match o {
        [169, 254, 169, 254] => return Some("metadata cloud (AWS/GCP/Azure/OCI)"),
        [168, 63, 129, 16] => {
            return Some("Azure platform IP — trông như IP công cộng nhưng là nội bộ")
        }
        [100, 100, 100, 200] => return Some("metadata Alibaba ECS"),
        _ => {}
    }
    match o[0] {
        0 => Some("this-network 0.0.0.0/8"),
        10 => Some("riêng 10/8"),
        127 => Some("loopback 127/8"),
        100 if (64..128).contains(&o[1]) => Some("CGNAT 100.64/10"),
        169 if o[1] == 254 => Some("link-local 169.254/16"),
        172 if (16..32).contains(&o[1]) => Some("riêng 172.16/12"),
        192 if o[1] == 168 => Some("riêng 192.168/16"),
        192 if o[1] == 0 && o[2] == 0 => Some("IETF protocol 192.0.0/24"),
        192 if o[1] == 0 && o[2] == 2 => Some("TEST-NET-1"),
        192 if o[1] == 88 && o[2] == 99 => Some("6to4 relay 192.88.99/24"),
        198 if (18..20).contains(&o[1]) => Some("benchmark 198.18/15"),
        198 if o[1] == 51 && o[2] == 100 => Some("TEST-NET-2"),
        203 if o[1] == 0 && o[2] == 113 => Some("TEST-NET-3"),
        n if n >= 224 => Some("multicast/reserved >=224/4"),
        _ => None,
    }
}

fn is_blocked_v6(ip: Ipv6Addr) -> Option<&'static str> {
    let s = ip.segments();
    if ip.is_loopback() {
        return Some("loopback ::1");
    }
    if ip.is_unspecified() {
        return Some("::/128");
    }
    // ::ffff:0:0/96 — IPv4 ánh xạ; phải kiểm lại theo luật IPv4, nếu không
    // ::ffff:169.254.169.254 sẽ chui lọt.
    if s[0] == 0 && s[1] == 0 && s[2] == 0 && s[3] == 0 && s[4] == 0 && s[5] == 0xffff {
        let v4 = Ipv4Addr::new(
            (s[6] >> 8) as u8,
            (s[6] & 0xff) as u8,
            (s[7] >> 8) as u8,
            (s[7] & 0xff) as u8,
        );
        return is_blocked_v4(v4).or(Some("IPv4 ánh xạ trong IPv6"));
    }
    // ULA fc00::/7 bao cả fd00:ec2::254 (AWS) và fd20:ce::254 (GCP) — hai địa
    // chỉ metadata IPv6 KHÔNG nằm trong fe80::/10 như nhiều người tưởng.
    if s[0] & 0xfe00 == 0xfc00 {
        return Some("ULA fc00::/7 (gồm cả metadata IPv6 của AWS/GCP)");
    }
    if s[0] & 0xffc0 == 0xfe80 {
        return Some("link-local fe80::/10");
    }
    if s[0] == 0x2001 && s[1] == 0x0db8 {
        return Some("tài liệu 2001:db8::/32");
    }
    if s[0] == 0x2002 {
        return Some("6to4 2002::/16");
    }
    None
}

/// Có được phép chạm tới host này không. Trả về danh sách IP đã kiểm.
///
/// Phân giải trước rồi mới kiểm — và **phải gọi lại sau mỗi lần redirect**,
/// vì đó chính là chỗ DNS rebinding chui vào.
pub async fn check_host_allowed(host: &str, allow_local: bool) -> Result<Vec<IpAddr>> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        if let Some(why) = is_blocked_ip(ip) {
            if !allow_local {
                return Err(anyhow!("từ chối {ip}: {why}"));
            }
        }
        return Ok(vec![ip]);
    }

    let resolver = crate::dns::resolver()?;
    let ips: Vec<IpAddr> = resolver
        .lookup_ip(host)
        .await
        .map_err(|e| anyhow!("không phân giải được '{host}': {e}"))?
        .iter()
        .collect();
    if ips.is_empty() {
        return Err(anyhow!("'{host}' không phân giải ra IP nào"));
    }
    for ip in &ips {
        if let Some(why) = is_blocked_ip(*ip) {
            if !allow_local {
                return Err(anyhow!("'{host}' phân giải về {ip} — từ chối: {why}"));
            }
        }
    }
    Ok(ips)
}

// ---------------------------------------------------------------------------
// Xác minh
// ---------------------------------------------------------------------------

/// Kiểm tra bằng chứng sở hữu có thật sự tồn tại không.
pub async fn verify(
    http: &reqwest::Client,
    method: Method,
    host: &str,
    token: &str,
) -> Result<()> {
    match method {
        Method::Local => {
            let ips = check_host_allowed(host, true).await?;
            if ips.iter().any(|ip| is_blocked_ip(*ip).is_some()) {
                Ok(())
            } else {
                Err(anyhow!(
                    "'{host}' không nằm trong dải nội bộ — cách 'local' không dùng được"
                ))
            }
        }
        Method::DnsTxt => {
            let want = format!("{TXT_PREFIX}{token}");
            let found = txt_records(host).await?;
            if found.iter().any(|r| r.trim() == want) {
                Ok(())
            } else {
                Err(anyhow!(
                    "không thấy TXT \"{want}\" ở {host} (đọc được {} bản ghi)",
                    found.len()
                ))
            }
        }
        Method::DnsCname => {
            let name = format!("senclaw-{token}.{host}");
            let r = crate::dns::resolver()?;
            match r.lookup(name.clone(), hickory_resolver::proto::rr::RecordType::CNAME).await {
                Ok(l) if l.iter().next().is_some() => Ok(()),
                _ => Err(anyhow!("không thấy CNAME tại {name}")),
            }
        }
        Method::WellKnown => {
            let url = format!("https://{host}{WELL_KNOWN_PATH}");
            let body = fetch_text(http, &url).await?;
            if body.trim() == token {
                Ok(())
            } else {
                Err(anyhow!("nội dung {url} không khớp token"))
            }
        }
        Method::Meta => {
            let url = format!("https://{host}/");
            let body = fetch_text(http, &url).await?;
            if meta_content(&body, META_NAME).as_deref() == Some(token) {
                Ok(())
            } else {
                Err(anyhow!("không thấy <meta name=\"{META_NAME}\"> đúng token ở {url}"))
            }
        }
    }
}

async fn txt_records(host: &str) -> Result<Vec<String>> {
    let r = crate::dns::resolver()?;
    // Tên tuyệt đối, nếu không resolver nối search domain vào — xem dns::fqdn.
    let lookup = r
        .txt_lookup(format!("{}.", host.trim_end_matches('.')))
        .await
        .map_err(|e| anyhow!("không đọc được TXT của '{host}': {e}"))?;
    Ok(lookup
        .iter()
        .map(|t| {
            t.iter()
                .map(|b| String::from_utf8_lossy(b).to_string())
                .collect::<String>()
        })
        .collect())
}

async fn fetch_text(http: &reqwest::Client, url: &str) -> Result<String> {
    let host = host_of(url)?;
    check_host_allowed(&host, false).await?;
    let resp = http
        .get(url)
        .send()
        .await
        .map_err(|e| anyhow!("không tải được {url}: {e}"))?;
    if !resp.status().is_success() {
        return Err(anyhow!("{url} trả {}", resp.status().as_u16()));
    }
    Ok(resp.text().await.unwrap_or_default())
}

/// Rút `content` của một thẻ meta theo `name`. Cố ý chỉ quét thô — mục đích là
/// đọc token của chính mình, không phải parse HTML tổng quát.
pub fn meta_content(html: &str, name: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let needle = format!("name=\"{}\"", name.to_ascii_lowercase());
    let alt = format!("name='{}'", name.to_ascii_lowercase());
    let pos = lower.find(&needle).or_else(|| lower.find(&alt))?;
    // tìm content= trong phạm vi thẻ hiện tại
    let tag_end = lower[pos..].find('>').map(|e| pos + e).unwrap_or(lower.len());
    let tag_start = lower[..pos].rfind('<').unwrap_or(0);
    let seg = &html[tag_start..tag_end];
    let seg_lower = &lower[tag_start..tag_end];
    let cpos = seg_lower.find("content=")? + "content=".len();
    let rest = &seg[cpos..];
    let quote = rest.chars().next()?;
    if quote != '"' && quote != '\'' {
        return Some(
            rest.split_whitespace()
                .next()
                .unwrap_or_default()
                .to_string(),
        );
    }
    let rest = &rest[1..];
    let end = rest.find(quote)?;
    Some(rest[..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_every_metadata_endpoint() {
        // Bốn địa chỉ này là lý do cả module này tồn tại.
        for s in ["169.254.169.254", "168.63.129.16", "100.100.100.200"] {
            assert!(is_blocked_ip(s.parse().unwrap()).is_some(), "phải chặn {s}");
        }
        // metadata IPv6 nằm trong ULA, KHÔNG phải link-local
        for s in ["fd00:ec2::254", "fd20:ce::254"] {
            assert!(is_blocked_ip(s.parse().unwrap()).is_some(), "phải chặn {s}");
        }
    }

    #[test]
    fn blocks_private_and_special_ranges() {
        for s in [
            "0.0.0.0",
            "10.1.2.3",
            "127.0.0.1",
            "100.64.0.1",
            "169.254.1.1",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "192.0.2.1",
            "198.18.0.1",
            "198.51.100.1",
            "203.0.113.1",
            "224.0.0.1",
            "255.255.255.255",
        ] {
            assert!(is_blocked_ip(s.parse().unwrap()).is_some(), "phải chặn {s}");
        }
        for s in ["::1", "fe80::1", "fc00::1", "2001:db8::1", "2002::1"] {
            assert!(is_blocked_ip(s.parse().unwrap()).is_some(), "phải chặn {s}");
        }
    }

    #[test]
    fn allows_ordinary_public_addresses() {
        // 172.15 và 172.32 nằm NGOÀI 172.16/12 — hay bị chặn nhầm
        for s in ["1.1.1.1", "8.8.8.8", "93.184.216.34", "172.15.0.1", "172.32.0.1"] {
            assert!(is_blocked_ip(s.parse().unwrap()).is_none(), "không được chặn {s}");
        }
        assert!(is_blocked_ip("2606:4700::1111".parse().unwrap()).is_none());
    }

    #[test]
    fn ipv4_mapped_ipv6_cannot_smuggle_a_blocked_address() {
        for s in ["::ffff:169.254.169.254", "::ffff:127.0.0.1", "::ffff:10.0.0.1"] {
            assert!(is_blocked_ip(s.parse().unwrap()).is_some(), "phải chặn {s}");
        }
    }

    #[test]
    fn host_extraction() {
        assert_eq!(host_of("https://a.vn/x?y=1").unwrap(), "a.vn");
        assert_eq!(host_of("http://a.vn:8080/").unwrap(), "a.vn");
        assert_eq!(host_of("a.vn").unwrap(), "a.vn");
        assert_eq!(host_of("ssh://root@1.2.3.4").unwrap(), "1.2.3.4");
        assert_eq!(host_of("1.2.3.4:22").unwrap(), "1.2.3.4");
        assert!(host_of("").is_err());
    }

    #[test]
    fn token_is_32_hex_and_varies_by_asset() {
        let a = gen_token(1);
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(gen_token(1), gen_token(2));
    }

    #[test]
    fn method_roundtrip_and_instructions_carry_the_token() {
        for m in ["dns-txt", "dns-cname", "well-known", "meta", "local"] {
            assert_eq!(Method::parse(m).unwrap().as_str(), m);
        }
        // 'email' cố tình không hỗ trợ — CA/Browser Forum đang khai tử cách này
        assert!(Method::parse("email").is_none());
        let i = Method::DnsTxt.instructions("a.vn", "deadbeef");
        assert!(i.contains("deadbeef") && i.contains(TXT_PREFIX));
    }

    #[test]
    fn meta_parsing_handles_quotes_and_attribute_order() {
        let t = "0123456789abcdef0123456789abcdef";
        for html in [
            format!("<head><meta name=\"senclaw-verify\" content=\"{t}\"></head>"),
            format!("<meta content='{t}' name='senclaw-verify'>"),
            format!("<meta  NAME=\"Senclaw-Verify\"  CONTENT=\"{t}\" />"),
        ] {
            assert_eq!(meta_content(&html, META_NAME).as_deref(), Some(t), "{html}");
        }
        assert!(meta_content("<meta name=\"other\" content=\"x\">", META_NAME).is_none());
    }
}
