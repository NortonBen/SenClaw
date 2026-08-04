//! "Cổng này là ứng dụng gì" — đọc lời chào của dịch vụ rồi suy ra sản phẩm,
//! phiên bản, và (khi có) bản phân phối hệ điều hành.
//!
//! Phần lớn giao thức cũ **tự gửi banner ngay khi kết nối** (SSH, SMTP, FTP,
//! POP3, IMAP): chỉ cần mở kết nối rồi đọc. Số còn lại chờ client nói trước —
//! HTTP thì gửi một `GET /` bình thường; PostgreSQL/Redis thì app **không** dò,
//! chỉ ghi "mở, không có banner". Ranh giới: chỉ gửi thứ mà một client hợp lệ
//! của chính giao thức đó sẽ gửi, không gửi gói dị dạng để moi phản ứng.
//!
//! Toàn bộ hàm phân tích ở đây là hàm thuần — test được không cần mạng, và đó
//! là chỗ chứa gần hết giá trị của module.

#[derive(Debug, Default, Clone)]
pub struct Fingerprint {
    /// Tên giao thức đã nhận ra: ssh, http, smtp, ftp, pop3, imap, mysql…
    pub service: Option<String>,
    pub product: Option<String>,
    pub version: Option<String>,
    /// Bằng chứng về hệ điều hành rút từ banner này, để `osguess` cộng lại.
    pub os_evidence: Vec<OsEvidence>,
    pub raw: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OsEvidence {
    /// "Ubuntu 22.04", "Debian 12", "Windows"…
    pub os: String,
    /// 0–100. Hậu tố gói của bản phân phối là bằng chứng mạnh; header `Server`
    /// chỉ nói về phần mềm web nên yếu hơn nhiều.
    pub weight: u32,
    pub from: String,
}

/// Phiên bản OpenSSH thượng nguồn → bản phát hành Ubuntu.
///
/// Ubuntu đóng băng phiên bản OpenSSH cho từng bản LTS, nên ánh xạ này chặt chẽ
/// một cách bất thường: thấy `8.9p1` với hậu tố `ubuntu` là gần như chắc chắn
/// 22.04. Đây là bằng chứng OS mạnh nhất mà một lần quét từ ngoài lấy được.
const UBUNTU_BY_OPENSSH: &[(&str, &str)] = &[
    ("9.9", "Ubuntu 25.04"),
    ("9.7", "Ubuntu 24.10"),
    ("9.6", "Ubuntu 24.04 LTS"),
    ("8.9", "Ubuntu 22.04 LTS"),
    ("8.2", "Ubuntu 20.04 LTS"),
    ("7.6", "Ubuntu 18.04 LTS"),
    ("7.2", "Ubuntu 16.04 LTS"),
];

/// Số sau `+deb` trong hậu tố gói Debian → tên bản phát hành.
const DEBIAN_BY_SUFFIX: &[(&str, &str)] = &[
    ("deb13", "Debian 13 (trixie)"),
    ("deb12", "Debian 12 (bookworm)"),
    ("deb11", "Debian 11 (bullseye)"),
    ("deb10", "Debian 10 (buster)"),
    ("deb9", "Debian 9 (stretch)"),
];

/// Rút bản phân phối từ hậu tố gói của một banner SSH.
///
/// `SSH-2.0-OpenSSH_8.9p1 Ubuntu-3ubuntu0.4` → Ubuntu 22.04.
/// `SSH-2.0-OpenSSH_9.2p1 Debian-2+deb12u2`  → Debian 12.
pub fn os_from_ssh(banner: &str) -> Option<OsEvidence> {
    let low = banner.to_ascii_lowercase();
    if low.contains("debian") {
        for (key, name) in DEBIAN_BY_SUFFIX {
            if low.contains(key) {
                return Some(OsEvidence {
                    os: (*name).to_string(),
                    weight: 90,
                    from: format!("hậu tố gói Debian trong banner SSH ({key})"),
                });
            }
        }
        return Some(OsEvidence {
            os: "Debian".into(),
            weight: 60,
            from: "banner SSH có nhãn Debian nhưng không rõ bản phát hành".into(),
        });
    }
    if low.contains("ubuntu") {
        let ver = openssh_version(banner)?;
        let short: String = ver.split('p').next().unwrap_or(&ver).to_string();
        for (v, name) in UBUNTU_BY_OPENSSH {
            if short == *v {
                return Some(OsEvidence {
                    os: (*name).to_string(),
                    weight: 85,
                    from: format!("OpenSSH {short} + nhãn Ubuntu — Ubuntu đóng băng bản OpenSSH theo từng LTS"),
                });
            }
        }
        return Some(OsEvidence {
            os: "Ubuntu".into(),
            weight: 60,
            from: format!("banner SSH có nhãn Ubuntu, OpenSSH {short} không khớp bản LTS nào đã biết"),
        });
    }
    if low.contains("freebsd") {
        return Some(OsEvidence {
            os: "FreeBSD".into(),
            weight: 80,
            from: "banner SSH có nhãn FreeBSD".into(),
        });
    }
    if low.contains("_for_windows") {
        return Some(OsEvidence {
            os: "Windows".into(),
            weight: 90,
            from: "OpenSSH_for_Windows trong banner SSH".into(),
        });
    }
    None
}

/// `SSH-2.0-OpenSSH_8.9p1 Ubuntu-3ubuntu0.4` → `8.9p1`.
pub fn openssh_version(banner: &str) -> Option<String> {
    let i = banner.find("OpenSSH_")? + "OpenSSH_".len();
    let rest = &banner[i..];
    let end = rest
        .find(|c: char| c.is_whitespace() || c == '-')
        .unwrap_or(rest.len());
    let v = rest[..end].trim();
    (!v.is_empty()).then(|| v.to_string())
}

/// Nhận dạng từ banner thô + số cổng.
///
/// Cổng chỉ là **gợi ý**, không phải kết luận: dịch vụ chạy cổng lạ là chuyện
/// thường, và đó lại đúng là thứ đáng phát hiện. Nội dung banner luôn thắng.
pub fn fingerprint(port: u16, raw: &str) -> Fingerprint {
    let mut f = Fingerprint {
        raw: raw.trim().to_string(),
        ..Default::default()
    };
    let b = raw.trim();
    let low = b.to_ascii_lowercase();

    if b.starts_with("SSH-") {
        f.service = Some("ssh".into());
        if low.contains("openssh") {
            f.product = Some("OpenSSH".into());
            f.version = openssh_version(b);
        } else if low.contains("dropbear") {
            f.product = Some("Dropbear".into());
        }
        if let Some(e) = os_from_ssh(b) {
            f.os_evidence.push(e);
        }
        return f;
    }

    if low.starts_with("http/") || low.contains("\r\nserver:") || low.starts_with("server:") {
        f.service = Some("http".into());
        if let Some(s) = header_value(b, "server") {
            let (prod, ver) = split_product(&s);
            f.product = Some(prod);
            f.version = ver;
            if let Some(e) = os_from_server_header(&s) {
                f.os_evidence.push(e);
            }
        }
        if let Some(p) = header_value(b, "x-powered-by") {
            if f.product.is_none() {
                f.product = Some(p);
            }
        }
        return f;
    }

    // SMTP/FTP/POP3/IMAP đều mở đầu bằng mã trạng thái ba chữ số hoặc `+OK`/`* OK`.
    if b.starts_with("220 ") || b.starts_with("220-") {
        f.service = Some(if port == 21 { "ftp" } else { "smtp" }.into());
        let (prod, ver) = product_from_greeting(b);
        f.product = prod;
        f.version = ver;
        if let Some(e) = os_from_greeting(b) {
            f.os_evidence.push(e);
        }
        return f;
    }
    if b.starts_with("+OK") {
        f.service = Some("pop3".into());
        let (prod, ver) = product_from_greeting(b);
        f.product = prod;
        f.version = ver;
        return f;
    }
    if b.starts_with("* OK") {
        f.service = Some("imap".into());
        let (prod, ver) = product_from_greeting(b);
        f.product = prod;
        f.version = ver;
        return f;
    }
    // MySQL/MariaDB chào bằng gói nhị phân; chuỗi phiên bản nằm ngay sau 5 byte
    // đầu và kết thúc bằng NUL. Đọc được nó là biết cả sản phẩm lẫn phiên bản.
    if let Some(v) = mysql_version(raw) {
        f.service = Some("mysql".into());
        f.product = Some(
            if v.to_ascii_lowercase().contains("mariadb") {
                "MariaDB"
            } else {
                "MySQL"
            }
            .into(),
        );
        f.version = Some(v);
        return f;
    }
    if low.starts_with("-err") || low.contains("noauth authentication required") {
        f.service = Some("redis".into());
        f.product = Some("Redis".into());
        return f;
    }

    f
}

/// Chuỗi phiên bản trong gói chào của MySQL: 4 byte header + 1 byte protocol,
/// rồi chuỗi kết thúc NUL.
pub fn mysql_version(raw: &str) -> Option<String> {
    let b = raw.as_bytes();
    if b.len() < 7 {
        return None;
    }
    // byte thứ 5 là số hiệu giao thức; 10 là bản đang dùng phổ biến
    if b[4] != 10 {
        return None;
    }
    let rest = &b[5..];
    let end = rest.iter().position(|c| *c == 0)?;
    let v = String::from_utf8_lossy(&rest[..end]).to_string();
    // Chuỗi phiên bản phải bắt đầu bằng chữ số, nếu không là đọc nhầm rác nhị phân.
    (!v.is_empty() && v.chars().next().is_some_and(|c| c.is_ascii_digit())).then_some(v)
}

/// Lấy giá trị một header HTTP (không phân biệt hoa thường).
pub fn header_value(raw: &str, name: &str) -> Option<String> {
    let want = format!("{}:", name.to_ascii_lowercase());
    raw.lines().find_map(|l| {
        let l = l.trim_end_matches('\r');
        let low = l.to_ascii_lowercase();
        low.starts_with(&want)
            .then(|| l[want.len()..].trim().to_string())
            .filter(|v| !v.is_empty())
    })
}

/// `nginx/1.24.0` → ("nginx", "1.24.0"). `Apache` → ("Apache", None).
pub fn split_product(s: &str) -> (String, Option<String>) {
    let head = s.split_whitespace().next().unwrap_or(s);
    match head.split_once('/') {
        Some((p, v)) if !v.is_empty() => (p.to_string(), Some(v.to_string())),
        _ => (head.to_string(), None),
    }
}

/// `220 mail.example.com ESMTP Postfix (Ubuntu)` → ("Postfix", None).
pub fn product_from_greeting(b: &str) -> (Option<String>, Option<String>) {
    const KNOWN: &[&str] = &[
        "Postfix", "Exim", "Sendmail", "ProFTPD", "vsFTPd", "Pure-FTPd", "FileZilla",
        "Dovecot", "Courier", "Microsoft ESMTP", "Zimbra", "OpenSMTPD",
    ];
    let low = b.to_ascii_lowercase();
    for k in KNOWN {
        if let Some(pos) = low.find(&k.to_ascii_lowercase()) {
            let after = &b[pos + k.len()..];
            let ver = after
                .split_whitespace()
                .next()
                .map(|t| t.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '.'))
                .filter(|t| t.chars().next().is_some_and(|c| c.is_ascii_digit()))
                .map(|t| t.to_string());
            return (Some((*k).to_string()), ver);
        }
    }
    (None, None)
}

/// Nhiều bản dựng ghi thẳng tên bản phân phối trong ngoặc: `Postfix (Ubuntu)`.
pub fn os_from_greeting(b: &str) -> Option<OsEvidence> {
    let low = b.to_ascii_lowercase();
    for (kw, os) in [
        ("(ubuntu)", "Ubuntu"),
        ("(debian", "Debian"),
        ("centos", "CentOS"),
        ("red hat", "RHEL"),
        ("microsoft", "Windows"),
    ] {
        if low.contains(kw) {
            return Some(OsEvidence {
                os: os.to_string(),
                weight: 55,
                from: format!("lời chào dịch vụ có nhãn \"{kw}\""),
            });
        }
    }
    None
}

/// `Server: Apache/2.4.52 (Ubuntu)` → Ubuntu.
///
/// Trọng số thấp có chủ đích: header `Server` **cấu hình được tuỳ ý** và rất hay
/// bị đặt lại hoặc giấu đi. Nó là gợi ý, không phải bằng chứng.
pub fn os_from_server_header(s: &str) -> Option<OsEvidence> {
    let low = s.to_ascii_lowercase();
    for (kw, os, w) in [
        ("(ubuntu)", "Ubuntu", 60u32),
        ("(debian)", "Debian", 60),
        ("(centos)", "CentOS", 60),
        ("(red hat", "RHEL", 60),
        ("(win", "Windows", 65),
        ("microsoft-iis", "Windows", 85),
        ("(unix)", "Unix", 30),
        ("(freebsd)", "FreeBSD", 60),
    ] {
        if low.contains(kw) {
            return Some(OsEvidence {
                os: os.to_string(),
                weight: w,
                from: format!("header Server chứa \"{kw}\""),
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openssh_version_is_extracted_from_the_real_banner_shape() {
        assert_eq!(
            openssh_version("SSH-2.0-OpenSSH_8.9p1 Ubuntu-3ubuntu0.4").as_deref(),
            Some("8.9p1")
        );
        assert_eq!(
            openssh_version("SSH-2.0-OpenSSH_9.2p1 Debian-2+deb12u2").as_deref(),
            Some("9.2p1")
        );
        assert_eq!(openssh_version("SSH-2.0-OpenSSH_7.4").as_deref(), Some("7.4"));
        assert!(openssh_version("SSH-2.0-dropbear_2020.81").is_none());
    }

    #[test]
    fn ubuntu_release_is_derived_from_the_frozen_openssh_version() {
        // Ánh xạ chặt vì Ubuntu đóng băng OpenSSH theo từng LTS — đây là bằng
        // chứng OS mạnh nhất lấy được từ ngoài.
        let e = os_from_ssh("SSH-2.0-OpenSSH_8.9p1 Ubuntu-3ubuntu0.4").unwrap();
        assert_eq!(e.os, "Ubuntu 22.04 LTS");
        assert!(e.weight >= 80);
        assert_eq!(
            os_from_ssh("SSH-2.0-OpenSSH_8.2p1 Ubuntu-4ubuntu0.5").unwrap().os,
            "Ubuntu 20.04 LTS"
        );
        assert_eq!(
            os_from_ssh("SSH-2.0-OpenSSH_9.6p1 Ubuntu-3ubuntu13.5").unwrap().os,
            "Ubuntu 24.04 LTS"
        );
    }

    #[test]
    fn debian_release_comes_from_the_deb_suffix() {
        assert_eq!(
            os_from_ssh("SSH-2.0-OpenSSH_9.2p1 Debian-2+deb12u2").unwrap().os,
            "Debian 12 (bookworm)"
        );
        assert_eq!(
            os_from_ssh("SSH-2.0-OpenSSH_7.9p1 Debian-10+deb10u2").unwrap().os,
            "Debian 10 (buster)"
        );
        // Nhãn Debian nhưng không có hậu tố nhận ra được → vẫn kết luận Debian,
        // nhưng nhẹ hơn hẳn.
        let e = os_from_ssh("SSH-2.0-OpenSSH_9.2p1 Debian").unwrap();
        assert_eq!(e.os, "Debian");
        assert!(e.weight < 80);
    }

    #[test]
    fn an_unlabelled_ssh_banner_yields_no_os_evidence() {
        // Nhiều bản dựng gỡ nhãn phân phối. Không có bằng chứng thì không được
        // bịa ra "chắc là Linux".
        assert!(os_from_ssh("SSH-2.0-OpenSSH_9.3").is_none());
        assert!(os_from_ssh("SSH-2.0-dropbear").is_none());
    }

    #[test]
    fn windows_openssh_is_recognised() {
        let e = os_from_ssh("SSH-2.0-OpenSSH_for_Windows_8.1").unwrap();
        assert_eq!(e.os, "Windows");
        assert!(e.weight >= 85);
    }

    #[test]
    fn ssh_fingerprint_carries_product_version_and_os_together() {
        let f = fingerprint(22, "SSH-2.0-OpenSSH_8.9p1 Ubuntu-3ubuntu0.4\r\n");
        assert_eq!(f.service.as_deref(), Some("ssh"));
        assert_eq!(f.product.as_deref(), Some("OpenSSH"));
        assert_eq!(f.version.as_deref(), Some("8.9p1"));
        assert_eq!(f.os_evidence[0].os, "Ubuntu 22.04 LTS");
    }

    #[test]
    fn http_server_header_gives_product_version_and_os() {
        let raw = "HTTP/1.1 200 OK\r\nServer: Apache/2.4.52 (Ubuntu)\r\nContent-Type: text/html\r\n\r\n";
        let f = fingerprint(80, raw);
        assert_eq!(f.service.as_deref(), Some("http"));
        assert_eq!(f.product.as_deref(), Some("Apache"));
        assert_eq!(f.version.as_deref(), Some("2.4.52"));
        assert_eq!(f.os_evidence[0].os, "Ubuntu");
        // header Server đặt lại được tuỳ ý → phải nhẹ hơn hậu tố gói SSH
        assert!(f.os_evidence[0].weight < 85);
    }

    #[test]
    fn iis_is_strong_evidence_of_windows() {
        let e = os_from_server_header("Microsoft-IIS/10.0").unwrap();
        assert_eq!(e.os, "Windows");
        assert!(e.weight >= 80);
    }

    #[test]
    fn header_lookup_is_case_insensitive_and_ignores_empty_values() {
        let raw = "HTTP/1.1 200 OK\r\nSERVER: nginx\r\nX-Powered-By:\r\n\r\n";
        assert_eq!(header_value(raw, "server").as_deref(), Some("nginx"));
        assert!(header_value(raw, "x-powered-by").is_none());
        assert!(header_value(raw, "missing").is_none());
    }

    #[test]
    fn product_split_handles_both_slashed_and_bare_forms() {
        assert_eq!(split_product("nginx/1.24.0"), ("nginx".into(), Some("1.24.0".into())));
        assert_eq!(split_product("Apache"), ("Apache".into(), None));
        assert_eq!(
            split_product("Apache/2.4.52 (Ubuntu)"),
            ("Apache".into(), Some("2.4.52".into()))
        );
    }

    #[test]
    fn smtp_and_ftp_greetings_are_told_apart_by_port() {
        let smtp = fingerprint(25, "220 mail.example.com ESMTP Postfix (Ubuntu)");
        assert_eq!(smtp.service.as_deref(), Some("smtp"));
        assert_eq!(smtp.product.as_deref(), Some("Postfix"));
        assert_eq!(smtp.os_evidence[0].os, "Ubuntu");

        let ftp = fingerprint(21, "220 ProFTPD 1.3.5 Server ready");
        assert_eq!(ftp.service.as_deref(), Some("ftp"));
        assert_eq!(ftp.product.as_deref(), Some("ProFTPD"));
        assert_eq!(ftp.version.as_deref(), Some("1.3.5"));
    }

    #[test]
    fn pop3_and_imap_greetings_are_recognised() {
        assert_eq!(
            fingerprint(110, "+OK Dovecot ready.").service.as_deref(),
            Some("pop3")
        );
        assert_eq!(
            fingerprint(143, "* OK [CAPABILITY IMAP4rev1] Dovecot ready.")
                .service
                .as_deref(),
            Some("imap")
        );
    }

    #[test]
    fn mysql_binary_handshake_yields_the_version_string() {
        // 4 byte header, byte giao thức = 10, rồi chuỗi phiên bản kết thúc NUL.
        let mut raw = vec![0x4a, 0x00, 0x00, 0x00, 0x0a];
        raw.extend_from_slice(b"8.0.35-0ubuntu0.22.04.1\0");
        let s = String::from_utf8_lossy(&raw).to_string();
        let f = fingerprint(3306, &s);
        assert_eq!(f.service.as_deref(), Some("mysql"));
        assert_eq!(f.product.as_deref(), Some("MySQL"));
        assert_eq!(f.version.as_deref(), Some("8.0.35-0ubuntu0.22.04.1"));

        let mut m = vec![0x4a, 0x00, 0x00, 0x00, 0x0a];
        m.extend_from_slice(b"10.11.6-MariaDB-0+deb12u1\0");
        let g = fingerprint(3306, &String::from_utf8_lossy(&m));
        assert_eq!(g.product.as_deref(), Some("MariaDB"));
    }

    #[test]
    fn random_binary_noise_is_not_mistaken_for_mysql() {
        // Byte >0x7f không viết được trong chuỗi Rust, mà đường đi thật lại là
        // `from_utf8_lossy` trên rác nhị phân — dựng đúng như vậy để test khớp
        // với cái hàm thực sự nhận được.
        let noise = String::from_utf8_lossy(&[0x01, 0x02, 0x03, 0x04, 0x0a, 0xff, 0xfe, 0x00]);
        assert!(mysql_version(&noise).is_none());
        assert!(mysql_version("short").is_none());
        // đúng byte giao thức nhưng chuỗi không bắt đầu bằng chữ số
        assert!(mysql_version("\x01\x02\x03\x04\x0aXYZ\0").is_none());
    }

    #[test]
    fn an_unrecognised_banner_leaves_every_field_empty_rather_than_guessing() {
        let f = fingerprint(9999, "hello there");
        assert!(f.service.is_none() && f.product.is_none() && f.version.is_none());
        assert!(f.os_evidence.is_empty());
        assert_eq!(f.raw, "hello there");
    }

    #[test]
    fn banner_content_wins_over_the_port_number() {
        // Dịch vụ chạy cổng lạ là chuyện thường — và đúng là thứ đáng phát hiện.
        let f = fingerprint(8022, "SSH-2.0-OpenSSH_9.2p1 Debian-2+deb12u2");
        assert_eq!(f.service.as_deref(), Some("ssh"));
    }
}
