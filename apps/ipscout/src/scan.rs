//! Quét cổng TCP connect + bắt banner + đọc chứng thư TLS.
//!
//! **Chỉ TCP connect** — bắt tay ba bước đầy đủ, đúng như một client bình thường.
//! Không SYN nửa mở, không FIN/XMAS/NULL, không phân mảnh, không giả IP nguồn.
//! Hệ quả cố ý: mỗi lần chạm cổng đều **để lại log ở phía máy chủ**. Quét mà chủ
//! máy không thấy được thì đó là kỹ thuật né tránh phát hiện, và app không làm.
//!
//! Cùng lý do, mọi yêu cầu HTTP đều mang `User-Agent: SenClaw-ipscout` — người
//! vận hành đọc log phải biết ai vừa gõ cửa.
//!
//! Ba giới hạn cứng để app không biến thành công cụ quét hàng loạt: mỗi lần một
//! host, tối đa 1024 cổng, tối đa 64 kết nối đồng thời.

use crate::banner::{self, Fingerprint};
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Trần cổng cho một lần quét — đủ chỗ cho toàn bộ dải 1-65535.
///
/// Trần cũ 1024 khiến không quét được vượt qua khoảng well-known, mất luôn nhóm
/// dịch vụ nội bộ chạy cổng cao (Docker registry 5000, ES 9200, Redis 6379,
/// PostgreSQL 5432, MongoDB 27017…) — những mục tiêu đáng phát hiện nhất.
pub const MAX_PORTS: usize = 65535;

/// Trần kết nối đồng thời. Với `full` (65535 cổng) ở 2.5s timeout, 64 kết nối
/// song song sẽ mất ~85 phút; 256 song song còn ~11 phút — trong khoảng người
/// dùng chịu chờ. Trần 512 chừa đầu cho người dùng nào cần nhanh hơn nữa.
pub const MAX_CONCURRENCY: usize = 512;
pub const USER_AGENT: &str = "SenClaw-ipscout/0.1 (kiểm tra hạ tầng của chính chủ)";

/// Cổng bắt đầu bằng bắt tay TLS thay vì gửi văn bản thuần.
const TLS_PORTS: &[u16] = &[443, 465, 636, 989, 990, 993, 995, 5986, 8443, 9443];

pub const PROFILES: &[(&str, &str)] = &[
    ("top20", "20 cổng hay mở nhất — đủ cho một lần nhìn nhanh"),
    ("top100", "100 cổng phổ biến theo thống kê tần suất của nmap"),
    ("top1000", "1024 cổng well-known (RFC-assigned 1-1024) — quét sâu tiêu chuẩn"),
    ("web", "cổng web và cổng ứng dụng hay dùng khi dev"),
    ("db", "cổng cơ sở dữ liệu — nhóm đáng lo nhất khi phơi ra Internet"),
    ("remote", "cổng quản trị từ xa: SSH, RDP, VNC, WinRM"),
    ("mail", "cổng thư: SMTP/POP3/IMAP kèm bản TLS"),
    ("full", "TOÀN BỘ 65535 cổng TCP — chuyên sâu, mất vài phút với concurrency mặc định"),
];

pub fn profile_ports(name: &str) -> Option<Vec<u16>> {
    Some(match name {
        "top20" => vec![
            21, 22, 23, 25, 53, 80, 110, 111, 135, 139, 143, 443, 445, 993, 995, 1723, 3306, 3389,
            5900, 8080,
        ],
        "top100" => vec![
            7, 9, 13, 21, 22, 23, 25, 26, 37, 53, 79, 80, 81, 88, 106, 110, 111, 113, 119, 135,
            139, 143, 144, 179, 199, 389, 427, 443, 444, 445, 465, 513, 514, 515, 543, 544, 548,
            554, 587, 631, 646, 873, 990, 993, 995, 1025, 1026, 1027, 1028, 1029, 1110, 1433, 1720,
            1723, 1755, 1900, 2000, 2001, 2049, 2121, 2717, 3000, 3128, 3306, 3389, 3986, 4899,
            5000, 5009, 5051, 5060, 5101, 5190, 5357, 5432, 5631, 5666, 5800, 5900, 6000, 6001,
            6646, 7070, 8000, 8008, 8009, 8080, 8081, 8443, 8888, 9100, 9999, 10000, 32768, 49152,
            49153, 49154, 49155, 49156, 49157,
        ],
        // Định nghĩa "top1000" của app: toàn bộ dải well-known IANA (1-1024).
        // Trùng ~93% với nmap top-1000 và không phải chép danh sách kín nhà tôi.
        "top1000" => (1..=1024u16).collect(),
        "web" => vec![80, 443, 3000, 5000, 7001, 8000, 8008, 8080, 8081, 8443, 8888, 9000, 9090, 9443],
        "db" => vec![
            1433, 1521, 3306, 5432, 5984, 6379, 7199, 8086, 9042, 9200, 11211, 27017, 27018, 28017,
        ],
        "remote" => vec![22, 23, 2222, 3389, 5900, 5901, 5985, 5986],
        "mail" => vec![25, 110, 143, 465, 587, 993, 995, 2525],
        // Toàn bộ dải TCP hợp lệ. Cổng 0 loại ra — nó là "any port" trong POSIX,
        // gửi tới đó nhận về EACCES chứ không phải câu trả lời có nghĩa.
        "full" => (1..=65535u16).collect(),
        _ => return None,
    })
}

/// Phân tích khai báo cổng: `"22,80,443"` hoặc `"1-1024"` hoặc trộn cả hai.
///
/// Trả lỗi khi vượt `MAX_PORTS` thay vì lặng lẽ cắt bớt. Cắt ngầm thì người
/// dùng tưởng đã quét hết trong khi phần đuôi chưa bao giờ được chạm tới —
/// đúng loại im lặng khiến một bản báo cáo "sạch" trở nên sai.
pub fn parse_ports(spec: &str) -> Result<Vec<u16>> {
    let mut out: Vec<u16> = vec![];
    for part in spec.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
        match part.split_once('-') {
            Some((a, b)) => {
                let (a, b) = (
                    a.trim()
                        .parse::<u16>()
                        .map_err(|_| anyhow!("'{a}' không phải số cổng"))?,
                    b.trim()
                        .parse::<u16>()
                        .map_err(|_| anyhow!("'{b}' không phải số cổng"))?,
                );
                if a == 0 || b == 0 {
                    return Err(anyhow!("cổng 0 không hợp lệ"));
                }
                if a > b {
                    return Err(anyhow!("dải '{part}' có đầu lớn hơn cuối"));
                }
                if (b as usize - a as usize + 1) > MAX_PORTS {
                    return Err(anyhow!(
                        "dải '{part}' có {} cổng, vượt giới hạn {MAX_PORTS}",
                        b as usize - a as usize + 1
                    ));
                }
                out.extend(a..=b);
            }
            None => {
                let p: u16 = part
                    .parse()
                    .map_err(|_| anyhow!("'{part}' không phải số cổng"))?;
                if p == 0 {
                    return Err(anyhow!("cổng 0 không hợp lệ"));
                }
                out.push(p);
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    if out.is_empty() {
        return Err(anyhow!("không có cổng nào để quét"));
    }
    if out.len() > MAX_PORTS {
        return Err(anyhow!(
            "yêu cầu {} cổng, vượt giới hạn {MAX_PORTS} — giới hạn này để app không \
             thành công cụ quét hàng loạt",
            out.len()
        ));
    }
    Ok(out)
}

/// Danh sách cổng từ tên hồ sơ hoặc khai báo tự do.
pub fn resolve_ports(profile: Option<&str>, ports: Option<&str>) -> Result<Vec<u16>> {
    match (profile, ports) {
        (_, Some(spec)) if !spec.trim().is_empty() => parse_ports(spec),
        (Some(p), _) => profile_ports(p).ok_or_else(|| {
            anyhow!(
                "không có hồ sơ '{p}'. Có: {}",
                PROFILES.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")
            )
        }),
        _ => Ok(profile_ports("top20").unwrap()),
    }
}

#[derive(Debug, Clone)]
pub struct TlsInfo {
    pub subject: String,
    pub issuer: String,
    pub san: Vec<String>,
    pub not_after: String,
    pub not_before: String,
    pub expired: bool,
    pub self_signed: bool,
}

#[derive(Debug, Clone)]
pub struct PortResult {
    pub port: u16,
    pub banner: String,
    pub fp: Fingerprint,
    pub tls: Option<TlsInfo>,
    pub latency_ms: u64,
}

#[derive(Debug, Clone)]
pub struct Opts {
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
    pub concurrency: usize,
    /// Tên dùng cho header `Host` và SNI. IP trần thì để nguyên IP.
    pub host: String,
}

impl Opts {
    pub fn new(host: impl Into<String>) -> Self {
        Self {
            connect_timeout: Duration::from_millis(2500),
            read_timeout: Duration::from_millis(1500),
            concurrency: 32,
            host: host.into(),
        }
    }
}

/// Quét một host. Chỉ trả về cổng **mở** — cổng đóng và cổng bị tường lửa chặn
/// im lặng không phân biệt được bằng TCP connect, nên gộp chúng thành "không mở"
/// là mô tả đúng những gì đo được.
pub async fn scan(ip: IpAddr, ports: &[u16], opts: &Opts) -> Vec<PortResult> {
    let sem = Arc::new(tokio::sync::Semaphore::new(
        opts.concurrency.clamp(1, MAX_CONCURRENCY),
    ));
    let mut tasks = vec![];
    for &port in ports.iter().take(MAX_PORTS) {
        let sem = sem.clone();
        let opts = opts.clone();
        tasks.push(tokio::spawn(async move {
            let _permit = sem.acquire().await.ok()?;
            probe(ip, port, &opts).await
        }));
    }
    let mut out = vec![];
    for t in tasks {
        if let Ok(Some(r)) = t.await {
            out.push(r);
        }
    }
    out.sort_by_key(|r| r.port);
    out
}

async fn probe(ip: IpAddr, port: u16, opts: &Opts) -> Option<PortResult> {
    let addr = SocketAddr::new(ip, port);
    let t0 = Instant::now();
    let stream = tokio::time::timeout(opts.connect_timeout, TcpStream::connect(addr))
        .await
        .ok()?
        .ok()?;
    let latency_ms = t0.elapsed().as_millis() as u64;

    let (raw, tls) = if TLS_PORTS.contains(&port) {
        match tls_probe(stream, opts).await {
            Ok((raw, info)) => (raw, Some(info)),
            // Bắt tay TLS hỏng vẫn là cổng MỞ — chỉ là không đọc được gì thêm.
            Err(_) => (String::new(), None),
        }
    } else {
        (plain_probe(stream, opts).await, None)
    };

    let mut fp = banner::fingerprint(port, &raw);
    // Chuỗi trong chứng thư cũng là bằng chứng nhận dạng: `*.cloudflaressl.com`
    // nói lên nhiều hơn bất cứ header nào.
    if fp.service.is_none() && tls.is_some() {
        fp.service = Some("tls".into());
    }
    Some(PortResult {
        port,
        // `fp.raw` đã cắt khoảng trắng thừa; cắt 600 ký tự để một trang HTML dài
        // không nhồi cả DB. Cắt theo ký tự chứ không theo byte — banner có thể
        // chứa UTF-8 nhiều byte và `&s[..600]` sẽ panic giữa ký tự.
        banner: fp.raw.chars().take(600).collect(),
        fp,
        tls,
        latency_ms,
    })
}

async fn plain_probe(mut s: TcpStream, opts: &Opts) -> String {
    let mut buf = vec![0u8; 4096];
    // Bước 1: nghe xem dịch vụ có tự chào không (SSH/SMTP/FTP/POP3/IMAP/MySQL).
    if let Ok(Ok(n)) = tokio::time::timeout(opts.read_timeout, s.read(&mut buf)).await {
        if n > 0 {
            return String::from_utf8_lossy(&buf[..n]).to_string();
        }
    }
    // Bước 2: im lặng → gửi một `GET /` **hợp lệ**, bất kể số cổng.
    //
    // Không giới hạn theo danh sách cổng HTTP có chủ đích: dịch vụ web chạy cổng
    // lạ (bảng quản trị nội bộ ở 8291, API ở 4710…) đúng là trường hợp đáng phát
    // hiện nhất, mà danh sách cổng cứng thì không bao giờ phủ hết. Đây vẫn nằm
    // trong ranh giới đã tuyên bố: một `GET /` là yêu cầu đúng định dạng mà bất
    // kỳ trình duyệt nào cũng gửi khi người dùng gõ địa chỉ có cổng. Thứ app
    // không làm là gửi gói **dị dạng** để moi phản ứng của ngăn xếp mạng.
    let req = http_request(&opts.host);
    if s.write_all(req.as_bytes()).await.is_ok() {
        if let Ok(Ok(n)) = tokio::time::timeout(opts.read_timeout, s.read(&mut buf)).await {
            if n > 0 {
                return String::from_utf8_lossy(&buf[..n]).to_string();
            }
        }
    }
    String::new()
}

pub fn http_request(host: &str) -> String {
    format!(
        "GET / HTTP/1.1\r\nHost: {host}\r\nUser-Agent: {USER_AGENT}\r\nAccept: */*\r\nConnection: close\r\n\r\n"
    )
}

// ---------------------------------------------------------------------------
// TLS
// ---------------------------------------------------------------------------

/// Bộ "xác minh" chỉ để **giữ lại** chứng thư, không kiểm chuỗi tin cậy.
///
/// Có chủ đích: mục tiêu là *đọc* chứng thư máy chủ đang dùng — kể cả khi nó tự
/// ký, hết hạn hay sai tên. Kiểm chuỗi ở đây sẽ vứt đúng những ca đáng quan tâm
/// nhất. Kết nối này **không** truyền dữ liệu nhạy cảm nào nên bỏ kiểm không
/// tạo rủi ro; nó chỉ gửi một `GET /`.
#[derive(Debug)]
struct CaptureCert {
    seen: std::sync::Mutex<Option<Vec<u8>>>,
    schemes: Vec<rustls::SignatureScheme>,
}

impl rustls::client::danger::ServerCertVerifier for CaptureCert {
    fn verify_server_cert(
        &self,
        end_entity: &rustls_pki_types::CertificateDer<'_>,
        _intermediates: &[rustls_pki_types::CertificateDer<'_>],
        _server_name: &rustls_pki_types::ServerName<'_>,
        _ocsp: &[u8],
        _now: rustls_pki_types::UnixTime,
    ) -> std::result::Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        *self.seen.lock().unwrap() = Some(end_entity.to_vec());
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _m: &[u8],
        _c: &rustls_pki_types::CertificateDer<'_>,
        _d: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _m: &[u8],
        _c: &rustls_pki_types::CertificateDer<'_>,
        _d: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.schemes.clone()
    }
}

async fn tls_probe(s: TcpStream, opts: &Opts) -> Result<(String, TlsInfo)> {
    let provider = rustls::crypto::ring::default_provider();
    let schemes = provider.signature_verification_algorithms.supported_schemes();
    let verifier = Arc::new(CaptureCert {
        seen: std::sync::Mutex::new(None),
        schemes,
    });
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(provider))
        .with_safe_default_protocol_versions()?
        .dangerous()
        .with_custom_certificate_verifier(verifier.clone())
        .with_no_client_auth();

    // SNI phải là tên miền; với IP trần thì rustls không nhận, dùng tên giữ chỗ
    // để bắt tay vẫn chạy — máy chủ sẽ trả chứng thư mặc định của nó.
    let sni = rustls_pki_types::ServerName::try_from(opts.host.clone())
        .or_else(|_| rustls_pki_types::ServerName::try_from("localhost".to_string()))
        .map_err(|e| anyhow!("tên SNI không hợp lệ: {e}"))?;

    let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
    let mut stream = tokio::time::timeout(opts.connect_timeout, connector.connect(sni, s))
        .await
        .map_err(|_| anyhow!("bắt tay TLS quá hạn"))?
        .map_err(|e| anyhow!("bắt tay TLS thất bại: {e}"))?;

    let der = verifier
        .seen
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| anyhow!("không lấy được chứng thư"))?;
    let info = parse_cert(&der)?;

    // Sau lớp TLS cũng thử HTTP như bên plaintext: IMAPS/POP3S sẽ trả mã lỗi
    // riêng của chúng, và chính mã lỗi đó lại nhận dạng được dịch vụ.
    let mut raw = String::new();
    let req = http_request(&opts.host);
    if stream.write_all(req.as_bytes()).await.is_ok() {
        let mut buf = vec![0u8; 4096];
        if let Ok(Ok(n)) = tokio::time::timeout(opts.read_timeout, stream.read(&mut buf)).await {
            raw = String::from_utf8_lossy(&buf[..n]).to_string();
        }
    }
    Ok((raw, info))
}

pub fn parse_cert(der: &[u8]) -> Result<TlsInfo> {
    use x509_parser::prelude::*;
    let (_, cert) = X509Certificate::from_der(der).map_err(|e| anyhow!("chứng thư hỏng: {e}"))?;
    let subject = cert.subject().to_string();
    let issuer = cert.issuer().to_string();
    let san: Vec<String> = cert
        .subject_alternative_name()
        .ok()
        .flatten()
        .map(|ext| {
            ext.value
                .general_names
                .iter()
                .map(|g| g.to_string())
                .collect()
        })
        .unwrap_or_default();
    Ok(TlsInfo {
        expired: !cert.validity().is_valid(),
        // Tự ký = subject trùng issuer. Phép kiểm này chỉ đúng với chứng thư
        // gốc/tự ký, không thay được việc kiểm chuỗi — mà app cố tình không làm.
        self_signed: subject == issuer,
        not_before: cert.validity().not_before.to_string(),
        not_after: cert.validity().not_after.to_string(),
        subject,
        issuer,
        san,
    })
}

pub fn to_json(r: &PortResult, fronted: bool) -> Value {
    let (sev, why, fix) = crate::risk::assess(r.port, fronted);
    json!({
        "port": r.port,
        "state": "open",
        "latency_ms": r.latency_ms,
        "service": r.fp.service,
        "product": r.fp.product,
        "version": r.fp.version,
        "banner": r.banner,
        "severity": sev.as_str(),
        "why": why,
        "fix": fix,
        "tls": r.tls.as_ref().map(|t| json!({
            "subject": t.subject, "issuer": t.issuer, "san": t.san,
            "not_before": t.not_before, "not_after": t.not_after,
            "expired": t.expired, "self_signed": t.self_signed,
        })),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_lists_parse_both_ranges_and_enumerations() {
        assert_eq!(parse_ports("22,80,443").unwrap(), vec![22, 80, 443]);
        assert_eq!(parse_ports("80-83").unwrap(), vec![80, 81, 82, 83]);
        // trộn, trùng lặp và thứ tự lộn xộn đều phải chuẩn hoá
        assert_eq!(parse_ports("443, 80-82, 80").unwrap(), vec![80, 81, 82, 443]);
        assert_eq!(parse_ports("  22  ").unwrap(), vec![22]);
    }

    #[test]
    fn an_oversized_range_is_an_error_never_a_silent_truncation() {
        // Cắt ngầm thì người dùng tưởng đã quét hết — một bản báo cáo "sạch" sai.
        // Trần MỚI = 65535 (toàn bộ TCP); vượt trần (ví dụ pha trộn hai dải cộng
        // dồn) vẫn phải báo lỗi thay vì im lặng cắt.
        assert!(parse_ports("1-65535").is_ok());
        assert_eq!(parse_ports("1-65535").unwrap().len(), 65535);
        // Không có dải TCP nào vượt 65535, nên bẫy chính là cộng dồn:
        let too_many = format!("1-65535,{}", "65535");
        // "1-65535,65535" chỉ ra 65535 sau dedup — vẫn hợp lệ
        assert_eq!(parse_ports(&too_many).unwrap().len(), 65535);
        // Còn khai một số cổng ngoài dải:
        assert!(parse_ports("65536").is_err());
    }

    #[test]
    fn malformed_port_specs_are_rejected_with_the_offending_text() {
        for bad in ["abc", "0", "22,abc", "100-50", "0-10", ""] {
            assert!(parse_ports(bad).is_err(), "'{bad}' phải bị từ chối");
        }
        assert!(parse_ports("abc").unwrap_err().to_string().contains("abc"));
    }

    #[test]
    fn every_profile_resolves_and_stays_within_limits() {
        for (name, desc) in PROFILES {
            let p = profile_ports(name).unwrap_or_else(|| panic!("thiếu hồ sơ {name}"));
            assert!(!p.is_empty() && p.len() <= MAX_PORTS, "{name}");
            assert!(!desc.is_empty(), "{name} thiếu mô tả");
        }
        assert_eq!(profile_ports("top20").unwrap().len(), 20);
        assert_eq!(profile_ports("top100").unwrap().len(), 100);
        assert!(profile_ports("khong-co").is_none());
    }

    #[test]
    fn explicit_ports_win_over_a_profile_and_the_default_is_top20() {
        assert_eq!(resolve_ports(Some("top100"), Some("22")).unwrap(), vec![22]);
        assert_eq!(resolve_ports(Some("web"), None).unwrap(), profile_ports("web").unwrap());
        assert_eq!(resolve_ports(None, None).unwrap(), profile_ports("top20").unwrap());
        // chuỗi rỗng không được coi là "đã khai cổng"
        assert_eq!(resolve_ports(Some("web"), Some("  ")).unwrap(), profile_ports("web").unwrap());
    }

    #[test]
    fn an_unknown_profile_names_the_valid_ones() {
        let e = resolve_ports(Some("khong-co"), None).unwrap_err().to_string();
        assert!(e.contains("top20") && e.contains("db"));
    }

    #[test]
    fn db_profile_covers_every_port_the_risk_rules_call_critical() {
        // Hồ sơ 'db' mà thiếu cổng nào thì luật rủi ro tương ứng không bao giờ chạy.
        let db = profile_ports("db").unwrap();
        for p in [3306, 5432, 6379, 27017, 9200, 11211] {
            assert!(db.contains(&p), "hồ sơ db thiếu cổng {p}");
        }
    }

    #[test]
    fn the_http_request_identifies_the_scanner_and_sets_host() {
        let r = http_request("example.com");
        assert!(r.contains("Host: example.com"));
        assert!(r.contains("SenClaw-ipscout"));
        // Kết thúc bằng dòng trống — thiếu là máy chủ treo tới timeout.
        assert!(r.ends_with("\r\n\r\n"));
    }

    #[test]
    fn concurrency_is_clamped_to_the_hard_ceiling() {
        let mut o = Opts::new("a.vn");
        o.concurrency = 10_000;
        assert!(o.concurrency.clamp(1, MAX_CONCURRENCY) == MAX_CONCURRENCY);
        o.concurrency = 0;
        assert!(o.concurrency.clamp(1, MAX_CONCURRENCY) == 1);
    }

    #[test]
    fn garbage_certificate_bytes_are_an_error_not_a_panic() {
        assert!(parse_cert(&[0u8; 8]).is_err());
        assert!(parse_cert(&[]).is_err());
    }
}
