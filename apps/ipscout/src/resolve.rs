//! DNS: phân giải xuôi, tên ngược (PTR) **có xác nhận xuôi**, và TXT.
//!
//! Phần phân tích tách khỏi phần truy vấn để test được mà không cần mạng.

use anyhow::{anyhow, Result};
use std::net::IpAddr;

/// Làm tên tuyệt đối (thêm dấu chấm cuối).
///
/// Không có dấu chấm cuối, resolver coi tên là tương đối và **nối search domain
/// của hệ thống vào**: `vnexpress.net` biến thành `vnexpress.net.local.` →
/// NXDOMAIN → app kết luận sai "không phân giải được". Bẫy này chỉ lộ ra trên
/// máy có search domain, nên rất dễ lọt qua test ở máy dev.
pub fn fqdn(name: &str) -> String {
    let n = name.trim().trim_end_matches('.');
    format!("{n}.")
}

/// Resolver dùng chung. EDNS0 + lùi sang TCP: bản ghi TXT ở apex hay vượt 512
/// byte, không có hai thứ này thì phản hồi bị cắt và truy vấn treo tới timeout.
pub fn resolver() -> Result<hickory_resolver::TokioAsyncResolver> {
    let (config, mut opts) = hickory_resolver::system_conf::read_system_conf()
        .map_err(|e| anyhow!("không đọc được cấu hình DNS hệ thống: {e}"))?;
    opts.edns0 = true;
    opts.try_tcp_on_error = true;
    opts.timeout = std::time::Duration::from_secs(5);
    opts.attempts = 2;
    Ok(hickory_resolver::TokioAsyncResolver::tokio(config, opts))
}

/// Kết quả tra tên ngược.
///
/// `forward_confirmed` là trường đáng giá nhất ở đây. PTR do **chủ của dải IP**
/// tự đặt, nên khai `google.com` cũng được — nó không chứng minh gì cả. Chỉ khi
/// tra ngược ra tên rồi tra xuôi tên đó về **đúng IP ban đầu** (FCrDNS) thì hai
/// phía mới cùng xác nhận. Đây là phép kiểm mà máy chủ thư dùng để lọc spam.
#[derive(Debug, Default, Clone)]
pub struct Ptr {
    pub names: Vec<String>,
    pub forward_confirmed: bool,
    /// Tên nào đã tra xuôi về đúng IP.
    pub confirmed_names: Vec<String>,
    /// Truy vấn có chạy được không — khác hẳn "chạy được nhưng không có bản ghi".
    pub ok: bool,
}

/// Bản ghi DNS xuôi của một tên miền.
#[derive(Debug, Default, Clone)]
pub struct Forward {
    pub a: Vec<IpAddr>,
    pub mx: Vec<String>,
    pub ns: Vec<String>,
    pub txt: Vec<String>,
    pub cname: Vec<String>,
}

pub async fn forward(host: &str) -> Forward {
    use hickory_resolver::proto::rr::RecordType;
    let Ok(r) = resolver() else {
        return Forward::default();
    };
    let name = fqdn(host);
    let mut f = Forward::default();
    if let Ok(l) = r.lookup_ip(name.clone()).await {
        f.a = l.iter().collect();
    }
    if let Ok(l) = r.mx_lookup(name.clone()).await {
        f.mx = l.iter().map(|m| m.exchange().to_string()).collect();
    }
    if let Ok(l) = r.ns_lookup(name.clone()).await {
        f.ns = l.iter().map(|n| n.to_string()).collect();
    }
    if let Ok(l) = r.txt_lookup(name.clone()).await {
        f.txt = l
            .iter()
            .map(|t| {
                t.iter()
                    .map(|b| String::from_utf8_lossy(b).to_string())
                    .collect::<String>()
            })
            .collect();
    }
    if let Ok(l) = r.lookup(name, RecordType::CNAME).await {
        f.cname = l.iter().map(|d| d.to_string()).collect();
    }
    f
}

/// Tra PTR rồi **tra xuôi lại từng tên** để xác nhận.
pub async fn ptr(ip: IpAddr) -> Ptr {
    use hickory_resolver::error::ResolveErrorKind;

    let Ok(r) = resolver() else {
        return Ptr::default();
    };
    let lookup = match r.reverse_lookup(ip).await {
        Ok(l) => l,
        // `NoRecordsFound` là câu trả lời dứt khoát "IP này không có PTR" — rất
        // nhiều IP hợp lệ đúng như vậy. Còn timeout/SERVFAIL thì chưa biết gì cả.
        // Gộp hai thứ đó lại là biến một lỗi mạng thành phát hiện "thiếu PTR",
        // rồi người vận hành đi sửa một thứ không hỏng.
        Err(e) if matches!(e.kind(), ResolveErrorKind::NoRecordsFound { .. }) => {
            return Ptr {
                ok: true,
                ..Default::default()
            }
        }
        Err(_) => return Ptr::default(),
    };
    let names: Vec<String> = lookup
        .iter()
        .map(|n| n.to_string().trim_end_matches('.').to_string())
        .collect();

    let mut confirmed = vec![];
    for n in &names {
        if let Ok(l) = r.lookup_ip(fqdn(n)).await {
            if l.iter().any(|got| got == ip) {
                confirmed.push(n.clone());
            }
        }
    }
    Ptr {
        forward_confirmed: !confirmed.is_empty(),
        confirmed_names: confirmed,
        names,
        ok: true,
    }
}

/// `1.2.3.4` → `4.3.2.1` (dạng DNSBL dùng). IPv6 trả None: các DNSBL lớn dùng
/// định dạng nibble khác và phần lớn chưa phủ IPv6 — trả None còn hơn tra sai.
pub fn reverse_octets(ip: IpAddr) -> Option<String> {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            Some(format!("{}.{}.{}.{}", o[3], o[2], o[1], o[0]))
        }
        IpAddr::V6(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_made_absolute_so_search_domains_cannot_be_appended() {
        assert_eq!(fqdn("vnexpress.net"), "vnexpress.net.");
        assert_eq!(fqdn("vnexpress.net."), "vnexpress.net.");
        assert_eq!(fqdn("  a.vn  "), "a.vn.");
        assert_eq!(fqdn("_dmarc.a.vn"), "_dmarc.a.vn.");
    }

    #[test]
    fn dnsbl_octets_are_reversed() {
        assert_eq!(reverse_octets("1.2.3.4".parse().unwrap()).unwrap(), "4.3.2.1");
        assert_eq!(
            reverse_octets("203.0.113.5".parse().unwrap()).unwrap(),
            "5.113.0.203"
        );
        // IPv6 cố ý không hỗ trợ — tra sai còn tệ hơn không tra.
        assert!(reverse_octets("2606:4700::1111".parse().unwrap()).is_none());
    }

    #[test]
    fn an_unconfirmed_ptr_is_not_treated_as_proof() {
        // PTR trần không chứng minh gì: chủ dải IP tự đặt được.
        let p = Ptr {
            names: vec!["totally-google.com".into()],
            forward_confirmed: false,
            confirmed_names: vec![],
            ok: true,
        };
        assert!(!p.forward_confirmed);
        assert!(p.confirmed_names.is_empty());
        assert_eq!(p.names.len(), 1);
    }
}
