//! "Cổng này mở thì sao" — chuyển danh sách cổng thành kết luận có hành động.
//!
//! Một danh sách cổng trần không nói được gì. Điều đáng nói là: **có cổng nào
//! đáng lẽ không bao giờ được phơi ra Internet không.** Cơ sở dữ liệu là ví dụ
//! rõ nhất — MySQL/Redis/MongoDB/Elasticsearch mở ra ngoài gần như luôn là cấu
//! hình sai chứ không phải lựa chọn, và mấy vụ rò rỉ dữ liệu lớn nhất thập kỷ
//! qua đúng là chuyện đó.
//!
//! App dừng ở chỗ **ghi nhận và xếp mức**. Nó không thử kết nối vào, không thử
//! mật khẩu mặc định, không kiểm xem Redis có bật auth không — đó là ranh giới
//! giữa đánh giá tư thế và khai thác.

use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

impl Severity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Critical => "critical",
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
            Self::Info => "info",
        }
    }
}

pub struct Rule {
    pub port: u16,
    pub name: &'static str,
    pub severity: Severity,
    pub why: &'static str,
    pub fix: &'static str,
}

/// Cổng → mức rủi ro khi phơi ra Internet, kèm lý do.
///
/// `why` là phần đáng giá: "cổng 6379 mở" không giúp ai quyết định gì, còn
/// "Redis mặc định KHÔNG có xác thực" thì có.
pub const RULES: &[Rule] = &[
    Rule { port: 23, name: "Telnet", severity: Severity::Critical,
        why: "Telnet truyền mật khẩu dưới dạng chữ thường, không mã hoá. Bất kỳ ai trên đường truyền đọc được.",
        fix: "Tắt telnetd, dùng SSH." },
    Rule { port: 445, name: "SMB", severity: Severity::Critical,
        why: "SMB phơi ra Internet là đường vào của EternalBlue và gần như mọi họ ransomware phổ biến.",
        fix: "Chặn 445 ở tường lửa biên; chia sẻ tệp qua VPN." },
    Rule { port: 139, name: "NetBIOS", severity: Severity::High,
        why: "NetBIOS lộ tên máy, tên miền và danh sách chia sẻ mà không cần xác thực.",
        fix: "Chặn 137-139 ở biên." },
    Rule { port: 3306, name: "MySQL/MariaDB", severity: Severity::Critical,
        why: "CSDL không bao giờ nên nghe trực tiếp từ Internet. Đây là bề mặt cho dò mật khẩu và khai thác trực tiếp.",
        fix: "bind-address=127.0.0.1, truy cập qua SSH tunnel hoặc mạng riêng." },
    Rule { port: 5432, name: "PostgreSQL", severity: Severity::Critical,
        why: "CSDL phơi ra Internet. pg_hba.conf cấu hình lỏng là mất toàn bộ dữ liệu.",
        fix: "listen_addresses='localhost' hoặc giới hạn theo mạng riêng." },
    Rule { port: 6379, name: "Redis", severity: Severity::Critical,
        why: "Redis mặc định KHÔNG có xác thực. Mở ra Internet nghĩa là ai cũng đọc/ghi được, và có kỹ thuật đã biết để biến nó thành thực thi lệnh.",
        fix: "bind 127.0.0.1, bật requirepass, bật protected-mode." },
    Rule { port: 27017, name: "MongoDB", severity: Severity::Critical,
        why: "MongoDB phơi ra ngoài là nguyên nhân của hàng loạt vụ rò rỉ dữ liệu quy mô lớn.",
        fix: "bindIp: 127.0.0.1 và bật authorization." },
    Rule { port: 9200, name: "Elasticsearch", severity: Severity::Critical,
        why: "API Elasticsearch không xác thực cho phép đọc và xoá toàn bộ chỉ mục.",
        fix: "network.host: localhost, bật xpack.security." },
    Rule { port: 11211, name: "Memcached", severity: Severity::Critical,
        why: "Memcached qua UDP còn bị lợi dụng để khuếch đại tấn công từ chối dịch vụ nhắm vào bên thứ ba.",
        fix: "-l 127.0.0.1 và tắt UDP (-U 0)." },
    Rule { port: 2375, name: "Docker API (không TLS)", severity: Severity::Critical,
        why: "API Docker không xác thực tương đương quyền root trên máy chủ: tạo container mount / là xong.",
        fix: "Không bao giờ mở 2375. Dùng socket Unix, hoặc 2376 có TLS xác thực hai chiều." },
    Rule { port: 3389, name: "RDP", severity: Severity::High,
        why: "RDP mở ra Internet là mục tiêu dò mật khẩu thường trực và từng có lỗ hổng tiền xác thực (BlueKeep).",
        fix: "Đưa sau VPN, hoặc bật Network Level Authentication + giới hạn IP nguồn." },
    Rule { port: 21, name: "FTP", severity: Severity::High,
        why: "FTP truyền cả thông tin đăng nhập lẫn dữ liệu không mã hoá.",
        fix: "Dùng SFTP (qua SSH) hoặc FTPS." },
    Rule { port: 5900, name: "VNC", severity: Severity::High,
        why: "VNC thường không mã hoá và mật khẩu giới hạn 8 ký tự.",
        fix: "Đưa sau VPN hoặc SSH tunnel." },
    Rule { port: 161, name: "SNMP", severity: Severity::High,
        why: "SNMP v1/v2c dùng community string dạng chữ thường, và 'public' vẫn còn rất phổ biến — nó phơi cả sơ đồ mạng.",
        fix: "Chuyển SNMPv3, hoặc chặn ở biên." },
    Rule { port: 25, name: "SMTP", severity: Severity::Medium,
        why: "Bình thường với máy chủ thư. Đáng lo khi đây KHÔNG phải máy chủ thư — relay mở bị lợi dụng gửi thư rác.",
        fix: "Nếu không phải máy chủ thư thì tắt; nếu có thì kiểm tra không phải open relay." },
    Rule { port: 111, name: "rpcbind", severity: Severity::Medium,
        why: "rpcbind lộ danh sách dịch vụ RPC và bị lợi dụng khuếch đại tấn công.",
        fix: "Chặn 111 ở biên nếu không dùng NFS ra ngoài." },
    Rule { port: 22, name: "SSH", severity: Severity::Info,
        why: "Bình thường và cần thiết cho quản trị. Rủi ro nằm ở cấu hình: cho phép đăng nhập bằng mật khẩu, cho root đăng nhập trực tiếp.",
        fix: "PasswordAuthentication no, PermitRootLogin no, dùng khoá." },
    Rule { port: 80, name: "HTTP", severity: Severity::Low,
        why: "HTTP thuần không mã hoá. Chấp nhận được nếu chỉ để chuyển hướng sang HTTPS.",
        fix: "Chuyển hướng 301 sang https và bật HSTS." },
    Rule { port: 443, name: "HTTPS", severity: Severity::Info,
        why: "Bình thường.", fix: "" },
];

pub fn rule_for(port: u16) -> Option<&'static Rule> {
    RULES.iter().find(|r| r.port == port)
}

/// Mức rủi ro của một cổng mở, có tính tới bối cảnh.
///
/// `fronted` (IP là biên CDN) hạ mọi mức xuống info: cổng đó là của CDN, không
/// phải của người dùng, nên bắt họ đi vá là tạo việc vô ích.
pub fn assess(port: u16, fronted: bool) -> (Severity, String, String) {
    if fronted {
        return (
            Severity::Info,
            format!("Cổng {port} mở trên biên CDN — không phải hạ tầng của bạn."),
            String::new(),
        );
    }
    match rule_for(port) {
        Some(r) => (r.severity, format!("{} — {}", r.name, r.why), r.fix.to_string()),
        None => (
            Severity::Low,
            format!(
                "Cổng {port} mở nhưng không nằm trong danh mục đã biết. Cổng lạ đáng xem \
                 lại: nó thường là dịch vụ nội bộ vô tình phơi ra ngoài."
            ),
            "Xác nhận dịch vụ này có cần truy cập từ Internet không.".into(),
        ),
    }
}

/// Danh mục luật, để trả lời "app đánh giá những gì" mà không phải đoán.
pub fn catalog() -> Value {
    json!({
        "rules": RULES.iter().map(|r| json!({
            "port": r.port, "service": r.name,
            "severity": r.severity.as_str(), "why": r.why, "fix": r.fix,
        })).collect::<Vec<_>>(),
        "unknown_port_default": "low",
        "not_covered": [
            "KHÔNG thử đăng nhập, KHÔNG thử mật khẩu mặc định — app chỉ ghi nhận cổng mở, không kiểm dịch vụ có xác thực hay không.",
            "KHÔNG dò lỗ hổng theo CVE của phiên bản đọc được; phiên bản trong banner có thể đã được vá ngược (backport) mà số không đổi.",
            "KHÔNG quét UDP: quét UDP đáng tin cần gửi payload riêng cho từng giao thức và rất dễ nhầm 'không trả lời' thành 'đóng'.",
            "Cổng lọc bởi tường lửa (drop im lặng) không phân biệt được với cổng đóng khi chỉ dùng TCP connect."
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposed_databases_are_critical() {
        // Đây là kết luận đáng giá nhất của cả app.
        for p in [3306, 5432, 6379, 27017, 9200, 11211] {
            let (sev, why, fix) = assess(p, false);
            assert_eq!(sev, Severity::Critical, "cổng {p}");
            assert!(!why.is_empty() && !fix.is_empty(), "cổng {p} thiếu lý do/cách sửa");
        }
    }

    #[test]
    fn redis_explains_the_no_auth_default_because_that_is_the_actionable_part() {
        let (_, why, _) = assess(6379, false);
        assert!(why.contains("KHÔNG có xác thực"));
    }

    #[test]
    fn ssh_and_https_are_informational_not_alarms() {
        assert_eq!(assess(22, false).0, Severity::Info);
        assert_eq!(assess(443, false).0, Severity::Info);
        // nhưng vẫn phải nói rủi ro nằm ở đâu
        assert!(assess(22, false).1.contains("cấu hình"));
    }

    #[test]
    fn an_unknown_port_is_low_and_says_why_it_still_matters() {
        let (sev, why, fix) = assess(48231, false);
        assert_eq!(sev, Severity::Low);
        assert!(why.contains("48231"));
        assert!(!fix.is_empty());
    }

    #[test]
    fn a_cdn_edge_never_generates_work_for_the_user() {
        // Cổng 3306 trên biên Cloudflare không phải việc của người dùng.
        let (sev, why, fix) = assess(3306, true);
        assert_eq!(sev, Severity::Info);
        assert!(why.contains("CDN"));
        assert!(fix.is_empty());
    }

    #[test]
    fn severity_orders_from_info_up_to_critical() {
        // Thứ tự khai báo quyết định Ord dẫn xuất; báo cáo sắp xếp bằng cách đảo
        // chiều, nên nếu ai đổi thứ tự biến thể thì test này bắt được.
        let mut v = vec![Severity::Low, Severity::Critical, Severity::Info, Severity::High];
        v.sort();
        assert_eq!(v, vec![Severity::Info, Severity::Low, Severity::High, Severity::Critical]);
        assert!(Severity::Critical > Severity::High);
    }

    #[test]
    fn catalog_declares_the_limits_of_the_method() {
        let c = catalog();
        assert_eq!(c["rules"].as_array().unwrap().len(), RULES.len());
        let nc = c["not_covered"].as_array().unwrap();
        assert!(nc.len() >= 4);
        // phải nói rõ không thử đăng nhập — đó là ranh giới thiết kế
        assert!(nc.iter().any(|x| x.as_str().unwrap().contains("KHÔNG thử đăng nhập")));
    }

    #[test]
    fn every_rule_has_a_reason_and_non_info_rules_have_a_fix() {
        for r in RULES {
            assert!(!r.why.is_empty(), "cổng {} thiếu lý do", r.port);
            if r.severity != Severity::Info {
                assert!(!r.fix.is_empty(), "cổng {} thiếu cách sửa", r.port);
            }
        }
    }
}
