//! Rào chắn phạm vi.
//!
//! Có **hai** nhóm luật chặn, khác nhau ở chỗ chúng phục vụ ai:
//!
//! * [`is_blocked_ip`] — dùng khi app **gọi ra ngoài** (RDAP, GeoIP). Chặn toàn bộ
//!   dải đặc biệt của IANA để app không tự biến thành công cụ SSRF: hỏi RDAP về
//!   `10.0.0.5` là vô nghĩa, hỏi về `169.254.169.254` là nguy hiểm.
//! * [`is_metadata_endpoint`] — dùng ở đường **quét cổng**. Ngặt hơn nhiều: chỉ
//!   chặn đúng các điểm cuối metadata cloud (không có lý do hợp pháp nào để quét
//!   chúng — kết quả luôn là "app bị lừa"). Dải riêng bình thường (10/8, 192.168/16,
//!   loopback) **được phép** — người dùng muốn quét LAN của họ là chuyện thường,
//!   và đó chính là ý nghĩa của việc bỏ ownership verification.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use anyhow::{anyhow, Result};

/// Rút host từ input (`https://a.vn/x` → `a.vn`, `ssh://u@1.2.3.4:22` → `1.2.3.4`,
/// `[2606:4700::1111]:443` → `2606:4700::1111`).
pub fn host_of(target: &str) -> Result<String> {
    let t = target.trim();
    if t.is_empty() {
        return Err(anyhow!("mục tiêu rỗng"));
    }
    if let Ok(u) = url::Url::parse(t) {
        if let Some(h) = u.host_str() {
            return Ok(h.trim_matches(['[', ']']).to_string());
        }
    }
    let h = t.split('/').next().unwrap_or(t);
    let h = h.rsplit('@').next().unwrap_or(h);
    // IPv6 trong ngoặc vuông: `[::1]:80`. Phải xử lý trước khi cắt theo ':',
    // nếu không `2606:4700::1111` bị cắt thành `2606`.
    if let Some(rest) = h.strip_prefix('[') {
        if let Some(end) = rest.find(']') {
            return Ok(rest[..end].to_string());
        }
    }
    // IPv6 trần (nhiều dấu ':') thì giữ nguyên — không có cổng đi kèm.
    let h = if h.matches(':').count() > 1 {
        h
    } else {
        h.split(':').next().unwrap_or(h)
    };
    if h.is_empty() {
        return Err(anyhow!("không rút được host từ '{target}'"));
    }
    Ok(h.to_string())
}

// ---------------------------------------------------------------------------
// Chặn SSRF cho HTTP gọi ra ngoài (RDAP, GeoIP)
// ---------------------------------------------------------------------------

/// IP có phải dải đặc biệt của IANA không, kèm lý do.
///
/// Dùng cho đường HTTP ra RDAP/GeoIP: hỏi các nguồn đó về IP nội bộ không có
/// nghĩa gì, và một redirect trỏ về `169.254.169.254` là đúng bề mặt SSRF cần
/// chặn. Không dùng cho đường quét cổng — xem [`is_metadata_endpoint`].
pub fn is_blocked_ip(ip: IpAddr) -> Option<&'static str> {
    match ip {
        IpAddr::V4(v4) => is_blocked_v4(v4),
        IpAddr::V6(v6) => is_blocked_v6(v6),
    }
}

fn is_blocked_v4(ip: Ipv4Addr) -> Option<&'static str> {
    let o = ip.octets();
    if let Some(why) = metadata_v4(ip) {
        return Some(why);
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

// ---------------------------------------------------------------------------
// Chặn hẹp cho đường quét cổng
// ---------------------------------------------------------------------------

/// Chỉ trả `Some(...)` cho **điểm cuối metadata cloud**. Không chặn dải riêng
/// bình thường.
///
/// Lý do tách khỏi [`is_blocked_ip`]: sau khi bỏ ownership verification, người
/// dùng có toàn quyền quét mạng của họ — kể cả `10.0.0.5` hay `192.168.1.1`.
/// Nhưng metadata cloud (`169.254.169.254`, `168.63.129.16` của Azure trông y
/// hệt IP công cộng, `100.100.100.200` của Alibaba, hai địa chỉ IPv6 ULA của
/// AWS/GCP) **không bao giờ** là mục tiêu quét hợp lệ — chạm chúng nghĩa là app
/// đang bị lừa qua DNS rebinding hoặc bản ghi độc.
pub fn is_metadata_endpoint(ip: IpAddr) -> Option<&'static str> {
    match ip {
        IpAddr::V4(v4) => metadata_v4(v4),
        IpAddr::V6(v6) => metadata_v6(v6),
    }
}

fn metadata_v4(ip: Ipv4Addr) -> Option<&'static str> {
    match ip.octets() {
        [169, 254, 169, 254] => Some("metadata cloud (AWS/GCP/Azure/OCI)"),
        [168, 63, 129, 16] => Some("Azure platform IP — trông như IP công cộng nhưng là nội bộ"),
        [100, 100, 100, 200] => Some("metadata Alibaba ECS"),
        _ => None,
    }
}

fn metadata_v6(ip: Ipv6Addr) -> Option<&'static str> {
    let s = ip.segments();
    // ::ffff:169.254.169.254 & bạn bè — không được chui lọt qua ánh xạ IPv4-mapped.
    if s[0] == 0 && s[1] == 0 && s[2] == 0 && s[3] == 0 && s[4] == 0 && s[5] == 0xffff {
        let v4 = Ipv4Addr::new(
            (s[6] >> 8) as u8,
            (s[6] & 0xff) as u8,
            (s[7] >> 8) as u8,
            (s[7] & 0xff) as u8,
        );
        return metadata_v4(v4);
    }
    // fd00:ec2::254 (AWS) và fd20:ce::254 (GCP) — hai chỗ duy nhất của AWS/GCP
    // dùng ULA cho metadata. Không bao gồm cả /7 vì phần còn lại là dải riêng
    // hợp pháp mà người dùng có thể muốn quét.
    if ip == "fd00:ec2::254".parse::<Ipv6Addr>().unwrap()
        || ip == "fd20:ce::254".parse::<Ipv6Addr>().unwrap()
    {
        return Some("metadata cloud IPv6 (AWS/GCP)");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_every_metadata_endpoint() {
        for s in ["169.254.169.254", "168.63.129.16", "100.100.100.200"] {
            let ip: IpAddr = s.parse().unwrap();
            assert!(is_blocked_ip(ip).is_some(), "SSRF phải chặn {s}");
            assert!(is_metadata_endpoint(ip).is_some(), "scan phải chặn {s}");
        }
        for s in ["fd00:ec2::254", "fd20:ce::254"] {
            let ip: IpAddr = s.parse().unwrap();
            assert!(is_blocked_ip(ip).is_some(), "SSRF phải chặn {s}");
            assert!(is_metadata_endpoint(ip).is_some(), "scan phải chặn {s}");
        }
    }

    #[test]
    fn ssrf_check_still_blocks_the_full_iana_registry() {
        for s in [
            "0.0.0.0", "10.1.2.3", "127.0.0.1", "100.64.0.1", "169.254.1.1", "172.16.0.1",
            "172.31.255.255", "192.168.1.1", "192.0.2.1", "198.18.0.1", "198.51.100.1",
            "203.0.113.1", "224.0.0.1", "255.255.255.255",
        ] {
            assert!(is_blocked_ip(s.parse().unwrap()).is_some(), "SSRF phải chặn {s}");
        }
        for s in ["::1", "fe80::1", "fc00::1", "2001:db8::1", "2002::1"] {
            assert!(is_blocked_ip(s.parse().unwrap()).is_some(), "SSRF phải chặn {s}");
        }
    }

    #[test]
    fn scan_check_allows_private_ranges_because_users_scan_their_own_lans() {
        // Chốt khác biệt lớn nhất giữa hai hàm — sau khi bỏ verification, quét
        // LAN của chính mình là chuyện thường và app không được chặn.
        for s in [
            "10.0.0.5",         // dải riêng RFC 1918
            "192.168.1.1",      // router nhà
            "127.0.0.1",        // máy này
            "172.16.0.1",       // dải riêng
            "169.254.1.1",      // link-local KHÔNG phải metadata
            "100.64.0.1",       // CGNAT
        ] {
            let ip: IpAddr = s.parse().unwrap();
            assert!(is_metadata_endpoint(ip).is_none(), "scan không được chặn {s}");
            // nhưng SSRF vẫn phải chặn (RDAP không có dữ liệu về chúng)
            assert!(is_blocked_ip(ip).is_some(), "SSRF vẫn chặn {s}");
        }
    }

    #[test]
    fn scan_check_does_not_touch_normal_ula_addresses() {
        // fc00::/7 nói chung là dải riêng IPv6 — người dùng có thể dùng nó cho
        // mạng nội bộ. Chỉ hai địa chỉ metadata cụ thể mới bị chặn ở đường quét.
        for s in ["fc00::1", "fd00::1", "fd12:3456::1", "fd12:3456::254"] {
            let ip: IpAddr = s.parse().unwrap();
            assert!(is_metadata_endpoint(ip).is_none(), "scan không được chặn {s}");
        }
    }

    #[test]
    fn allows_ordinary_public_addresses() {
        for s in ["1.1.1.1", "8.8.8.8", "93.184.216.34", "172.15.0.1", "172.32.0.1"] {
            let ip: IpAddr = s.parse().unwrap();
            assert!(is_blocked_ip(ip).is_none(), "SSRF không được chặn {s}");
            assert!(is_metadata_endpoint(ip).is_none(), "scan không được chặn {s}");
        }
        assert!(is_blocked_ip("2606:4700::1111".parse().unwrap()).is_none());
    }

    #[test]
    fn ipv4_mapped_ipv6_cannot_smuggle_a_blocked_address() {
        // Cả hai đường phải khoá lại lỗ ánh xạ này, không phải chỉ SSRF.
        for s in ["::ffff:169.254.169.254", "::ffff:127.0.0.1", "::ffff:10.0.0.1"] {
            assert!(is_blocked_ip(s.parse().unwrap()).is_some(), "SSRF phải chặn {s}");
        }
        // Nhưng chỉ metadata mới bị chặn ở đường quét (127.0.0.1/10.0.0.1 hợp lệ khi quét)
        assert!(is_metadata_endpoint("::ffff:169.254.169.254".parse().unwrap()).is_some());
        assert!(is_metadata_endpoint("::ffff:127.0.0.1".parse().unwrap()).is_none());
    }

    #[test]
    fn host_extraction_handles_ipv6_and_ports() {
        assert_eq!(host_of("https://a.vn/x?y=1").unwrap(), "a.vn");
        assert_eq!(host_of("http://a.vn:8080/").unwrap(), "a.vn");
        assert_eq!(host_of("a.vn").unwrap(), "a.vn");
        assert_eq!(host_of("ssh://root@1.2.3.4").unwrap(), "1.2.3.4");
        assert_eq!(host_of("1.2.3.4:22").unwrap(), "1.2.3.4");
        // IPv6: cắt ngây thơ theo ':' sẽ ra "2606" — bẫy thật.
        assert_eq!(host_of("2606:4700::1111").unwrap(), "2606:4700::1111");
        assert_eq!(host_of("[2606:4700::1111]:443").unwrap(), "2606:4700::1111");
        assert_eq!(host_of("https://[2606:4700::1111]/").unwrap(), "2606:4700::1111");
        assert!(host_of("").is_err());
    }
}
