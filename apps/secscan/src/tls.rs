//! Đầu dò TLS: chứng thư và phiên bản giao thức.
//!
//! **Tự gửi ClientHello thô, không dùng client TLS hiện đại.** Lý do đã đo được:
//! `openssl s_client -tls1` trả `no protocols available` — đó là *client* từ
//! chối, chưa gửi gói nào ra dây, nên kết luận "server không hỗ trợ TLS 1.0" là
//! **âm tính giả**. `rustls` còn hẹp hơn (chỉ 1.2/1.3, 9 bộ mã AEAD). Và cách
//! `openssl + SECLEVEL=0` cũng không bền: OpenSSL 3.5 bỏ hẳn TLS 1.0/1.1 khỏi
//! bản dựng mặc định, nên cùng đoạn mã sẽ báo "không hỗ trợ" trên máy libssl mới.
//!
//! Gửi byte thô thì không thư viện nào phủ quyết được — đây cũng là cách
//! testssl.sh và sslscan làm.
//!
//! Bắt tay TLS 1.2 gửi `Certificate` ở dạng **rõ** (trước ChangeCipherSpec), nên
//! cùng một socket vừa dò được phiên bản vừa lấy được chứng thư mà không cần
//! hoàn tất trao khoá.

use crate::db::Finding;
use anyhow::{anyhow, bail, Result};
use serde_json::json;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
const READ_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Version {
    Ssl3,
    Tls10,
    Tls11,
    Tls12,
    Tls13,
}

impl Version {
    pub fn wire(&self) -> u16 {
        match self {
            Self::Ssl3 => 0x0300,
            Self::Tls10 => 0x0301,
            Self::Tls11 => 0x0302,
            Self::Tls12 => 0x0303,
            Self::Tls13 => 0x0304,
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            Self::Ssl3 => "SSL 3.0",
            Self::Tls10 => "TLS 1.0",
            Self::Tls11 => "TLS 1.1",
            Self::Tls12 => "TLS 1.2",
            Self::Tls13 => "TLS 1.3",
        }
    }
    /// Đã lỗi thời theo RFC 8996 (TLS 1.0/1.1) và RFC 7568 (SSL 3.0).
    pub fn is_deprecated(&self) -> bool {
        matches!(self, Self::Ssl3 | Self::Tls10 | Self::Tls11)
    }
}

// ---------------------------------------------------------------------------
// Dựng ClientHello
// ---------------------------------------------------------------------------

/// Bộ mã đủ rộng để server cũ lẫn mới đều tìm được cái chung. Cố ý bao gồm cả
/// bộ yếu: mục đích là để SERVER quyết định, không phải để ta lọc hộ nó.
const CIPHERS: &[u16] = &[
    0x1301, 0x1302, 0x1303, // TLS 1.3
    0xc02b, 0xc02f, 0xc02c, 0xc030, // ECDHE AEAD
    0xc013, 0xc014, // ECDHE SHA1
    0x009c, 0x009d, 0x002f, 0x0035, // RSA
    0x000a, // 3DES — server cũ
];

fn u16b(v: u16) -> [u8; 2] {
    v.to_be_bytes()
}

/// ClientHello cho một phiên bản cụ thể.
///
/// `random` truyền vào thay vì tự sinh để test tái lập được — giá trị ngẫu nhiên
/// thật không ảnh hưởng kết quả dò.
pub fn client_hello(host: &str, ver: Version, random: [u8; 32]) -> Vec<u8> {
    let mut body = vec![];
    // legacy_version: TLS 1.3 buộc để 0x0303 rồi đàm phán qua supported_versions
    let legacy = if ver == Version::Tls13 { Version::Tls12 } else { ver };
    body.extend_from_slice(&u16b(legacy.wire()));
    body.extend_from_slice(&random);
    body.push(0); // session_id rỗng

    body.extend_from_slice(&u16b((CIPHERS.len() * 2) as u16));
    for c in CIPHERS {
        body.extend_from_slice(&u16b(*c));
    }
    body.extend_from_slice(&[0x01, 0x00]); // compression: null

    // --- phần mở rộng ---
    let mut ext = vec![];

    // SNI: thiếu nó thì máy chủ dùng chung IP trả sai chứng thư (hoặc từ chối).
    // Bỏ qua nếu đích là địa chỉ IP — RFC 6066 cấm đưa IP vào SNI.
    if host.parse::<std::net::IpAddr>().is_err() {
        let h = host.as_bytes();
        let mut sni = vec![0x00]; // host_name
        sni.extend_from_slice(&u16b(h.len() as u16));
        sni.extend_from_slice(h);
        let mut sni_list = u16b(sni.len() as u16).to_vec();
        sni_list.extend_from_slice(&sni);
        ext.extend_from_slice(&u16b(0x0000)); // server_name
        ext.extend_from_slice(&u16b(sni_list.len() as u16));
        ext.extend_from_slice(&sni_list);
    }

    // supported_groups — thiếu thì server ECDHE từ chối bắt tay
    let groups: [u16; 4] = [0x001d, 0x0017, 0x0018, 0x0019];
    let mut g = u16b((groups.len() * 2) as u16).to_vec();
    for x in groups {
        g.extend_from_slice(&u16b(x));
    }
    ext.extend_from_slice(&u16b(0x000a));
    ext.extend_from_slice(&u16b(g.len() as u16));
    ext.extend_from_slice(&g);

    // ec_point_formats: uncompressed
    ext.extend_from_slice(&u16b(0x000b));
    ext.extend_from_slice(&u16b(2));
    ext.extend_from_slice(&[0x01, 0x00]);

    // signature_algorithms — TLS 1.2 trở lên bắt buộc
    let sigs: [u16; 8] = [
        0x0403, 0x0503, 0x0603, 0x0804, 0x0805, 0x0806, 0x0401, 0x0201,
    ];
    let mut sa = u16b((sigs.len() * 2) as u16).to_vec();
    for x in sigs {
        sa.extend_from_slice(&u16b(x));
    }
    ext.extend_from_slice(&u16b(0x000d));
    ext.extend_from_slice(&u16b(sa.len() as u16));
    ext.extend_from_slice(&sa);

    if ver == Version::Tls13 {
        // supported_versions: chỉ khai 1.3 để câu trả lời không mập mờ
        ext.extend_from_slice(&u16b(0x002b));
        ext.extend_from_slice(&u16b(3));
        ext.extend_from_slice(&[2]);
        ext.extend_from_slice(&u16b(Version::Tls13.wire()));
        // key_share rỗng -> server trả HelloRetryRequest, vẫn đủ để biết nó nói 1.3
        ext.extend_from_slice(&u16b(0x0033));
        ext.extend_from_slice(&u16b(2));
        ext.extend_from_slice(&u16b(0));
    }

    body.extend_from_slice(&u16b(ext.len() as u16));
    body.extend_from_slice(&ext);

    // gói handshake
    let mut hs = vec![0x01]; // client_hello
    let n = body.len();
    hs.extend_from_slice(&[(n >> 16) as u8, (n >> 8) as u8, n as u8]);
    hs.extend_from_slice(&body);

    // gói record
    let mut rec = vec![0x16]; // handshake
    rec.extend_from_slice(&u16b(Version::Tls10.wire())); // record version: 1.0 cho tương thích
    rec.extend_from_slice(&u16b(hs.len() as u16));
    rec.extend_from_slice(&hs);
    rec
}

// ---------------------------------------------------------------------------
// Đọc phản hồi
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct HandshakeInfo {
    /// Phiên bản server chọn, đọc từ ServerHello.
    pub negotiated: Option<u16>,
    pub cipher: Option<u16>,
    /// Chứng thư DER, lá trước.
    pub certs: Vec<Vec<u8>>,
    /// Server trả alert (không hỗ trợ / từ chối).
    pub alert: Option<(u8, u8)>,
}

/// Tách các bản ghi TLS trong một mẻ byte đã đọc.
///
/// Cố ý bỏ qua bản ghi không phải handshake và chịu được dữ liệu cụt — ta chỉ
/// cần ServerHello + Certificate, không cần hoàn tất bắt tay.
pub fn parse_records(buf: &[u8]) -> HandshakeInfo {
    let mut info = HandshakeInfo::default();
    let mut hs_data: Vec<u8> = vec![];
    let mut i = 0usize;

    while i + 5 <= buf.len() {
        let ctype = buf[i];
        let len = ((buf[i + 3] as usize) << 8) | buf[i + 4] as usize;
        let start = i + 5;
        let end = start + len;
        if end > buf.len() {
            break; // bản ghi cụt
        }
        match ctype {
            0x16 => hs_data.extend_from_slice(&buf[start..end]),
            0x15 if len >= 2 => {
                info.alert = Some((buf[start], buf[start + 1]));
            }
            _ => {}
        }
        i = end;
    }

    // duyệt các message handshake đã ghép
    let mut j = 0usize;
    while j + 4 <= hs_data.len() {
        let mtype = hs_data[j];
        let mlen = ((hs_data[j + 1] as usize) << 16)
            | ((hs_data[j + 2] as usize) << 8)
            | hs_data[j + 3] as usize;
        let start = j + 4;
        let end = start + mlen;
        if end > hs_data.len() {
            break;
        }
        let m = &hs_data[start..end];
        match mtype {
            0x02 => parse_server_hello(m, &mut info),
            0x0b => parse_certificate(m, &mut info),
            _ => {}
        }
        j = end;
    }
    info
}

fn parse_server_hello(m: &[u8], info: &mut HandshakeInfo) {
    if m.len() < 35 {
        return;
    }
    let mut v = ((m[0] as u16) << 8) | m[1] as u16;
    let sid_len = m[34] as usize;
    let mut p = 35 + sid_len;
    if p + 2 <= m.len() {
        info.cipher = Some(((m[p] as u16) << 8) | m[p + 1] as u16);
    }
    p += 3; // cipher(2) + compression(1)

    // TLS 1.3 giấu phiên bản thật trong extension supported_versions; trường
    // legacy_version luôn là 0x0303. Đọc nhầm chỗ này là báo sai "chỉ có 1.2".
    if p + 2 <= m.len() {
        let ext_len = ((m[p] as usize) << 8) | m[p + 1] as usize;
        let mut q = p + 2;
        let ext_end = (q + ext_len).min(m.len());
        while q + 4 <= ext_end {
            let etype = ((m[q] as u16) << 8) | m[q + 1] as u16;
            let elen = ((m[q + 2] as usize) << 8) | m[q + 3] as usize;
            if etype == 0x002b && elen == 2 && q + 6 <= ext_end {
                v = ((m[q + 4] as u16) << 8) | m[q + 5] as u16;
            }
            q += 4 + elen;
        }
    }
    info.negotiated = Some(v);
}

fn parse_certificate(m: &[u8], info: &mut HandshakeInfo) {
    if m.len() < 3 {
        return;
    }
    let total = ((m[0] as usize) << 16) | ((m[1] as usize) << 8) | m[2] as usize;
    let mut p = 3usize;
    let end = (3 + total).min(m.len());
    while p + 3 <= end {
        let clen = ((m[p] as usize) << 16) | ((m[p + 1] as usize) << 8) | m[p + 2] as usize;
        let cstart = p + 3;
        let cend = cstart + clen;
        if cend > end {
            break;
        }
        info.certs.push(m[cstart..cend].to_vec());
        p = cend;
    }
}

// ---------------------------------------------------------------------------
// Dò trên mạng
// ---------------------------------------------------------------------------

async fn probe_once(host: &str, port: u16, ver: Version) -> Result<HandshakeInfo> {
    let addr = format!("{host}:{port}");
    let stream = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(&addr))
        .await
        .map_err(|_| anyhow!("hết giờ khi nối tới {addr}"))?
        .map_err(|e| anyhow!("không nối được {addr}: {e}"))?;
    let mut stream = stream;

    let random = {
        let mut r = [0u8; 32];
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        for (i, b) in r.iter_mut().enumerate() {
            *b = ((t >> (i % 16)) as u8) ^ (i as u8).wrapping_mul(31);
        }
        r
    };
    stream.write_all(&client_hello(host, ver, random)).await?;
    stream.flush().await?;

    // Đọc tới khi đủ ServerHello + Certificate, hoặc hết giờ. Chuỗi chứng thư
    // hay vượt một lần read nên phải lặp.
    let mut buf = vec![];
    let mut chunk = [0u8; 8192];
    let deadline = tokio::time::Instant::now() + READ_TIMEOUT;
    loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        if left.is_zero() {
            break;
        }
        match tokio::time::timeout(left, stream.read(&mut chunk)).await {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => {
                buf.extend_from_slice(&chunk[..n]);
                let info = parse_records(&buf);
                if info.alert.is_some() || !info.certs.is_empty() {
                    return Ok(info);
                }
                if buf.len() > 512 * 1024 {
                    break;
                }
            }
            _ => break,
        }
    }
    if buf.is_empty() {
        bail!("server đóng kết nối, không trả gì");
    }
    Ok(parse_records(&buf))
}

/// Server có chấp nhận phiên bản này không.
pub async fn supports(host: &str, port: u16, ver: Version) -> bool {
    match probe_once(host, port, ver).await {
        Ok(i) => i.alert.is_none() && i.negotiated == Some(ver.wire()),
        Err(_) => false,
    }
}

// ---------------------------------------------------------------------------
// Phân tích chứng thư
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct CertInfo {
    pub subject: String,
    pub issuer: String,
    pub not_before: i64,
    pub not_after: i64,
    pub sans: Vec<String>,
    pub sig_alg: String,
    pub key_bits: Option<usize>,
    pub self_signed: bool,
    pub chain_len: usize,
}

pub fn inspect_cert(der: &[u8], chain_len: usize) -> Result<CertInfo> {
    use x509_parser::prelude::*;
    let (_, c) = X509Certificate::from_der(der).map_err(|e| anyhow!("chứng thư hỏng: {e}"))?;

    let sans: Vec<String> = c
        .subject_alternative_name()
        .ok()
        .flatten()
        .map(|e| {
            e.value
                .general_names
                .iter()
                .filter_map(|g| match g {
                    GeneralName::DNSName(s) => Some(s.to_string()),
                    GeneralName::IPAddress(b) if b.len() == 4 => {
                        Some(format!("{}.{}.{}.{}", b[0], b[1], b[2], b[3]))
                    }
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();

    // Suy độ dài khoá từ kích thước bit của public key. Không cần phân biệt
    // RSA/EC ở đây — chỉ cần biết nó có nhỏ bất thường không.
    let key_bits = {
        use x509_parser::public_key::PublicKey;
        match c.public_key().parsed() {
            Ok(PublicKey::RSA(r)) => Some(r.key_size()),
            Ok(PublicKey::EC(e)) => Some(e.key_size()),
            _ => None,
        }
    };

    Ok(CertInfo {
        subject: c.subject().to_string(),
        issuer: c.issuer().to_string(),
        not_before: c.validity().not_before.timestamp(),
        not_after: c.validity().not_after.timestamp(),
        sans,
        sig_alg: format!("{}", c.signature_algorithm.algorithm),
        key_bits,
        self_signed: c.subject() == c.issuer(),
        chain_len,
    })
}

/// Tên miền có khớp chứng thư không, có xử lý ký tự đại diện.
///
/// `*.a.vn` khớp `x.a.vn` nhưng **không** khớp `a.vn` hay `x.y.a.vn` — đại diện
/// chỉ phủ đúng một nhãn. Nhiều bộ kiểm tra tự viết sai chỗ này theo hướng nới lỏng.
pub fn host_matches(host: &str, patterns: &[String]) -> bool {
    let h = host.trim_end_matches('.').to_ascii_lowercase();
    patterns.iter().any(|p| {
        let p = p.trim_end_matches('.').to_ascii_lowercase();
        if let Some(suffix) = p.strip_prefix("*.") {
            match h.split_once('.') {
                Some((_, rest)) => rest == suffix,
                None => false,
            }
        } else {
            p == h
        }
    })
}

/// Thuật toán ký yếu — đã bị bẻ trong thực tế, không chỉ trên lý thuyết.
fn weak_signature(alg: &str) -> Option<&'static str> {
    let a = alg.to_ascii_lowercase();
    // OID hoặc tên đều bắt
    if a.contains("md5") || a.contains("1.2.840.113549.1.1.4") {
        Some("MD5")
    } else if a.contains("sha1")
        || a.contains("sha-1")
        || a.contains("1.2.840.113549.1.1.5")
        || a.contains("1.2.840.10040.4.3")
    {
        Some("SHA-1")
    } else {
        None
    }
}

/// Chuyển thông tin chứng thư + phiên bản thành phát hiện.
pub fn analyze(host: &str, cert: Option<&CertInfo>, versions: &[Version], now: i64) -> Vec<Finding> {
    let mut out = vec![];

    for v in versions.iter().filter(|v| v.is_deprecated()) {
        let sev = if *v == Version::Ssl3 { "high" } else { "medium" };
        out.push(
            Finding::new("tls", sev, format!("tls:version:{}", v.label().replace(' ', "")),
                format!("Còn bật {} — đã lỗi thời", v.label()))
                .detail(if *v == Version::Ssl3 {
                    "SSL 3.0 bị khai tử bởi RFC 7568 (POODLE)."
                } else {
                    "RFC 8996 khai tử TLS 1.0 và 1.1 từ 2021."
                })
                .fix(format!("Tắt {} ở cấu hình máy chủ, chỉ để TLS 1.2 và 1.3.", v.label()))
                .wstg("WSTG-CRYP-01"),
        );
    }

    if !versions.is_empty() && !versions.contains(&Version::Tls13) {
        out.push(
            Finding::new("tls", "low", "tls:version:no13", "Chưa bật TLS 1.3")
                .detail("TLS 1.3 bắt tay nhanh hơn và bỏ hẳn các cấu trúc đã lỗi thời.")
                .fix("Bật TLS 1.3 ở máy chủ.")
                .wstg("WSTG-CRYP-01"),
        );
    }

    let Some(c) = cert else {
        return out;
    };

    let days = (c.not_after - now) / 86_400;
    if c.not_after < now {
        out.push(
            // Hết hạn là CRITICAL: không phải "nên sửa" mà là trình duyệt ĐANG
            // chặn người dùng ngay lúc này.
            Finding::new("tls", "critical", "tls:cert:expired", "Chứng thư ĐÃ HẾT HẠN")
                .detail(format!("Hết hạn {} ngày trước. Trình duyệt đang chặn truy cập.", -days))
                .evidence(json!({ "not_after": crate::db::iso(c.not_after) }))
                .fix("Gia hạn ngay, và bật gia hạn tự động để không lặp lại.")
                .wstg("WSTG-CRYP-01"),
        );
    } else if days <= 7 {
        out.push(
            Finding::new("tls", "high", "tls:cert:expiring", "Chứng thư sắp hết hạn")
                .detail(format!("Còn {days} ngày."))
                .evidence(json!({ "days_left": days, "not_after": crate::db::iso(c.not_after) }))
                .fix("Gia hạn ngay.")
                .wstg("WSTG-CRYP-01"),
        );
    } else if days <= 30 {
        out.push(
            Finding::new("tls", "medium", "tls:cert:expiring-soon", "Chứng thư hết hạn trong 30 ngày")
                .detail(format!("Còn {days} ngày."))
                .evidence(json!({ "days_left": days }))
                .fix("Kiểm tra gia hạn tự động có đang chạy không.")
                .wstg("WSTG-CRYP-01"),
        );
    }

    if c.not_before > now {
        out.push(
            Finding::new("tls", "high", "tls:cert:not-yet-valid", "Chứng thư chưa tới hạn hiệu lực")
                .detail("Thường là do đồng hồ máy chủ sai, hoặc cấp nhầm chứng thư tương lai.")
                .evidence(json!({ "not_before": crate::db::iso(c.not_before) })),
        );
    }

    if !host_matches(host, &c.sans) {
        out.push(
            Finding::new("tls", "high", "tls:cert:host-mismatch", "Chứng thư không khớp tên miền")
                .detail(format!("'{host}' không nằm trong danh sách SAN của chứng thư."))
                .evidence(json!({ "host": host, "sans": c.sans }))
                .fix("Cấp lại chứng thư có SAN đúng cho tên miền này.")
                .wstg("WSTG-CRYP-01"),
        );
    }

    if c.self_signed {
        out.push(
            Finding::new("tls", "high", "tls:cert:self-signed", "Chứng thư tự ký")
                .detail("Trình duyệt không tin, và người dùng sẽ quen với việc bấm qua cảnh báo.")
                .fix("Dùng chứng thư từ CA công khai (Let's Encrypt là miễn phí).")
                .wstg("WSTG-CRYP-01"),
        );
    } else if c.chain_len <= 1 {
        // Bẫy hay gặp: server chỉ gửi chứng thư lá, thiếu trung gian. Trình duyệt
        // máy tính thường tự vá bằng AIA; **điện thoại và client dòng lệnh thì
        // không** — nên site "chạy tốt trên máy tôi" mà hỏng trên di động.
        out.push(
            Finding::new("tls", "medium", "tls:cert:incomplete-chain", "Chuỗi chứng thư thiếu bản trung gian")
                .detail("Server chỉ gửi chứng thư lá. Trình duyệt máy tính thường tự bù bằng AIA, nhưng nhiều client di động và dòng lệnh thì không — lỗi chỉ hiện ở một phần người dùng.")
                .fix("Cấu hình fullchain.pem thay vì cert.pem.")
                .wstg("WSTG-CRYP-01"),
        );
    }

    if let Some(w) = weak_signature(&c.sig_alg) {
        out.push(
            Finding::new("tls", "high", "tls:cert:weak-signature", format!("Chứng thư ký bằng {w}"))
                .detail(format!("{w} đã bị bẻ trong thực tế, không chỉ trên lý thuyết."))
                .evidence(json!({ "algorithm": c.sig_alg }))
                .fix("Cấp lại chứng thư ký bằng SHA-256 trở lên.")
                .wstg("WSTG-CRYP-01"),
        );
    }

    if let Some(bits) = c.key_bits {
        // Ngưỡng theo EC: khoá 256-bit EC mạnh hơn RSA 2048, nên không dùng
        // chung một con số cho cả hai.
        let weak = if c.sig_alg.to_ascii_lowercase().contains("ecdsa") || bits <= 521 {
            bits < 224
        } else {
            bits < 2048
        };
        if weak {
            out.push(
                Finding::new("tls", "high", "tls:cert:weak-key", "Khoá chứng thư quá ngắn")
                    .detail(format!("{bits} bit."))
                    .evidence(json!({ "key_bits": bits }))
                    .fix("Cấp lại với RSA ≥ 2048 bit hoặc ECDSA P-256.")
                    .wstg("WSTG-CRYP-01"),
            );
        }
    }

    out
}

/// Dò đầy đủ một đích: phiên bản + chứng thư.
pub async fn scan(host: &str, port: u16, now: i64) -> Result<Vec<Finding>> {
    let mut versions = vec![];
    // Dò từng phiên bản riêng: hỏi chung rồi suy ra là sai, vì server chỉ trả
    // về cái CAO NHẤT nó chọn được.
    for v in [
        Version::Ssl3,
        Version::Tls10,
        Version::Tls11,
        Version::Tls12,
        Version::Tls13,
    ] {
        if supports(host, port, v).await {
            versions.push(v);
        }
    }

    // Chứng thư lấy từ bắt tay TLS 1.2 — ở đó `Certificate` còn ở dạng rõ.
    // TLS 1.3 mã hoá message này nên không đọc được nếu không trao khoá.
    let info = probe_once(host, port, Version::Tls12).await.ok();
    let cert = info
        .as_ref()
        .and_then(|i| i.certs.first().map(|d| (d, i.certs.len())))
        .and_then(|(d, n)| inspect_cert(d, n).ok());

    if versions.is_empty() && cert.is_none() {
        bail!("không bắt tay được TLS với {host}:{port}");
    }

    let mut out = analyze(host, cert.as_ref(), &versions, now);

    if cert.is_none() && !versions.is_empty() {
        out.push(
            Finding::new("tls", "info", "tls:cert:unavailable", "Không đọc được chứng thư")
                .detail("Máy chủ chỉ hỗ trợ TLS 1.3, nơi message Certificate đã được mã hoá — đây là tin TỐT về cấu hình, chỉ là đầu dò không xem được hạn dùng."),
        );
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(f: &[Finding]) -> Vec<&str> {
        f.iter().map(|x| x.fingerprint.as_str()).collect()
    }

    fn cert(not_after: i64) -> CertInfo {
        CertInfo {
            subject: "CN=a.vn".into(),
            issuer: "CN=R3".into(),
            not_before: 0,
            not_after,
            sans: vec!["a.vn".into()],
            sig_alg: "1.2.840.113549.1.1.11".into(), // sha256WithRSA
            key_bits: Some(2048),
            self_signed: false,
            chain_len: 2,
        }
    }

    #[test]
    fn client_hello_is_a_well_formed_record() {
        let h = client_hello("a.vn", Version::Tls12, [7u8; 32]);
        assert_eq!(h[0], 0x16, "phải là record handshake");
        let rec_len = ((h[3] as usize) << 8) | h[4] as usize;
        assert_eq!(h.len(), 5 + rec_len, "độ dài record phải khớp thân");
        assert_eq!(h[5], 0x01, "phải là ClientHello");
        let hs_len = ((h[6] as usize) << 16) | ((h[7] as usize) << 8) | h[8] as usize;
        assert_eq!(rec_len, 4 + hs_len, "độ dài handshake phải khớp");
        // client_version nằm ngay sau header handshake
        assert_eq!(((h[9] as u16) << 8) | h[10] as u16, Version::Tls12.wire());
    }

    #[test]
    fn sni_is_included_for_names_but_omitted_for_ip_literals() {
        let with = client_hello("example.com", Version::Tls12, [0u8; 32]);
        assert!(
            with.windows(11).any(|w| w == b"example.com"),
            "tên miền phải vào SNI"
        );
        // RFC 6066 cấm đưa IP vào SNI — server có thể từ chối bắt tay nếu vi phạm
        let ip = client_hello("93.184.216.34", Version::Tls12, [0u8; 32]);
        assert!(
            !ip.windows(13).any(|w| w == b"93.184.216.34"),
            "IP không được đưa vào SNI"
        );
    }

    #[test]
    fn tls13_hello_declares_supported_versions() {
        let h = client_hello("a.vn", Version::Tls13, [0u8; 32]);
        // legacy_version phải là 1.2, phiên bản thật nằm trong extension 0x002b
        assert_eq!(((h[9] as u16) << 8) | h[10] as u16, Version::Tls12.wire());
        assert!(
            h.windows(2).any(|w| w == [0x00, 0x2b]),
            "phải có extension supported_versions"
        );
    }

    #[test]
    fn parses_server_hello_and_alert() {
        // ServerHello tối giản: version 0x0303, random 32B, sid 0, cipher, comp, ext rỗng
        let mut sh = vec![0x03, 0x03];
        sh.extend_from_slice(&[0u8; 32]);
        sh.push(0);
        sh.extend_from_slice(&[0xc0, 0x2f]);
        sh.push(0);
        sh.extend_from_slice(&[0x00, 0x00]);
        let mut hs = vec![0x02];
        let n = sh.len();
        hs.extend_from_slice(&[(n >> 16) as u8, (n >> 8) as u8, n as u8]);
        hs.extend_from_slice(&sh);
        let mut rec = vec![0x16, 0x03, 0x03];
        rec.extend_from_slice(&(hs.len() as u16).to_be_bytes());
        rec.extend_from_slice(&hs);

        let info = parse_records(&rec);
        assert_eq!(info.negotiated, Some(0x0303));
        assert_eq!(info.cipher, Some(0xc02f));

        // alert: handshake_failure
        let alert = vec![0x15, 0x03, 0x03, 0x00, 0x02, 0x02, 0x28];
        assert_eq!(parse_records(&alert).alert, Some((2, 40)));
    }

    #[test]
    fn tls13_version_is_read_from_the_extension_not_the_legacy_field() {
        // Bẫy thật: TLS 1.3 luôn để legacy_version = 0x0303. Đọc trường đó sẽ
        // kết luận sai là "server chỉ có TLS 1.2".
        let mut sh = vec![0x03, 0x03];
        sh.extend_from_slice(&[0u8; 32]);
        sh.push(0);
        sh.extend_from_slice(&[0x13, 0x01]);
        sh.push(0);
        let ext = [0x00u8, 0x2b, 0x00, 0x02, 0x03, 0x04]; // supported_versions = 1.3
        sh.extend_from_slice(&(ext.len() as u16).to_be_bytes());
        sh.extend_from_slice(&ext);
        let mut hs = vec![0x02];
        let n = sh.len();
        hs.extend_from_slice(&[(n >> 16) as u8, (n >> 8) as u8, n as u8]);
        hs.extend_from_slice(&sh);
        let mut rec = vec![0x16, 0x03, 0x03];
        rec.extend_from_slice(&(hs.len() as u16).to_be_bytes());
        rec.extend_from_slice(&hs);

        assert_eq!(parse_records(&rec).negotiated, Some(0x0304));
    }

    #[test]
    fn parses_a_certificate_chain() {
        let c1 = vec![0xAAu8; 10];
        let c2 = vec![0xBBu8; 20];
        let mut body = vec![];
        for c in [&c1, &c2] {
            let n = c.len();
            body.extend_from_slice(&[(n >> 16) as u8, (n >> 8) as u8, n as u8]);
            body.extend_from_slice(c);
        }
        let mut m = vec![];
        let t = body.len();
        m.extend_from_slice(&[(t >> 16) as u8, (t >> 8) as u8, t as u8]);
        m.extend_from_slice(&body);
        let mut hs = vec![0x0b];
        let n = m.len();
        hs.extend_from_slice(&[(n >> 16) as u8, (n >> 8) as u8, n as u8]);
        hs.extend_from_slice(&m);
        let mut rec = vec![0x16, 0x03, 0x03];
        rec.extend_from_slice(&(hs.len() as u16).to_be_bytes());
        rec.extend_from_slice(&hs);

        let info = parse_records(&rec);
        assert_eq!(info.certs.len(), 2);
        assert_eq!(info.certs[0], c1);
        assert_eq!(info.certs[1], c2);
    }

    #[test]
    fn truncated_input_does_not_panic() {
        let full = client_hello("a.vn", Version::Tls12, [0u8; 32]);
        for cut in 0..full.len() {
            let _ = parse_records(&full[..cut]);
        }
    }

    #[test]
    fn wildcard_matching_covers_exactly_one_label() {
        let p = vec!["*.a.vn".to_string()];
        assert!(host_matches("x.a.vn", &p));
        assert!(!host_matches("a.vn", &p), "đại diện không phủ chính apex");
        assert!(!host_matches("x.y.a.vn", &p), "đại diện chỉ phủ một nhãn");

        let p2 = vec!["a.vn".to_string(), "*.a.vn".to_string()];
        assert!(host_matches("a.vn", &p2) && host_matches("w.a.vn", &p2));
        assert!(!host_matches("evil.vn", &p2));
        // không phân biệt hoa thường, bỏ dấu chấm cuối
        assert!(host_matches("A.VN.", &p2));
    }

    #[test]
    fn expired_certificate_is_critical_not_merely_high() {
        let now = 1_800_000_000;
        let f = analyze("a.vn", Some(&cert(now - 86_400)), &[Version::Tls12], now);
        let e = f.iter().find(|x| x.fingerprint == "tls:cert:expired").unwrap();
        assert_eq!(e.severity, "critical", "hết hạn = trình duyệt ĐANG chặn người dùng");
    }

    #[test]
    fn expiry_severity_steps_down_as_the_deadline_recedes() {
        let now = 1_800_000_000;
        let sev = |days: i64| {
            analyze("a.vn", Some(&cert(now + days * 86_400)), &[Version::Tls12], now)
                .iter()
                .find(|x| x.fingerprint.starts_with("tls:cert:expir"))
                .map(|x| x.severity)
        };
        assert_eq!(sev(3), Some("high"));
        assert_eq!(sev(20), Some("medium"));
        assert_eq!(sev(90), None, "còn xa thì không báo");
    }

    #[test]
    fn hostname_mismatch_is_detected() {
        let now = 1_800_000_000;
        let mut c = cert(now + 86_400 * 90);
        c.sans = vec!["other.vn".into()];
        assert!(ids(&analyze("a.vn", Some(&c), &[], now)).contains(&"tls:cert:host-mismatch"));
    }

    #[test]
    fn deprecated_versions_are_flagged_and_ssl3_ranks_higher() {
        let now = 1_800_000_000;
        let f = analyze("a.vn", None, &[Version::Ssl3, Version::Tls10, Version::Tls12], now);
        assert_eq!(f.iter().find(|x| x.fingerprint.contains("SSL3.0")).unwrap().severity, "high");
        assert_eq!(f.iter().find(|x| x.fingerprint.contains("TLS1.0")).unwrap().severity, "medium");
        // TLS 1.2 hiện đại thì không báo
        assert!(!ids(&f).iter().any(|i| i.contains("TLS1.2")));
    }

    #[test]
    fn modern_config_produces_no_tls_findings() {
        let now = 1_800_000_000;
        let f = analyze("a.vn", Some(&cert(now + 86_400 * 90)), &[Version::Tls12, Version::Tls13], now);
        assert!(f.is_empty(), "cấu hình tốt không được sinh cảnh báo: {:?}", ids(&f));
    }

    #[test]
    fn weak_signature_and_short_key_are_caught() {
        let now = 1_800_000_000;
        let mut c = cert(now + 86_400 * 90);
        c.sig_alg = "1.2.840.113549.1.1.5".into(); // sha1WithRSA
        c.key_bits = Some(1024);
        let f = analyze("a.vn", Some(&c), &[], now);
        assert!(ids(&f).contains(&"tls:cert:weak-signature"));
        assert!(ids(&f).contains(&"tls:cert:weak-key"));
    }

    #[test]
    fn ec_keys_are_not_judged_by_the_rsa_threshold() {
        let now = 1_800_000_000;
        let mut c = cert(now + 86_400 * 90);
        c.sig_alg = "ecdsa-with-SHA256".into();
        c.key_bits = Some(256); // P-256 mạnh hơn RSA 2048
        let f = analyze("a.vn", Some(&c), &[], now);
        assert!(!ids(&f).contains(&"tls:cert:weak-key"), "256-bit EC không phải khoá yếu");
    }

    #[test]
    fn incomplete_chain_is_reported_but_not_for_self_signed() {
        let now = 1_800_000_000;
        let mut c = cert(now + 86_400 * 90);
        c.chain_len = 1;
        assert!(ids(&analyze("a.vn", Some(&c), &[], now)).contains(&"tls:cert:incomplete-chain"));

        c.self_signed = true;
        let f = analyze("a.vn", Some(&c), &[], now);
        assert!(ids(&f).contains(&"tls:cert:self-signed"));
        assert!(!ids(&f).contains(&"tls:cert:incomplete-chain"), "tự ký thì chuỗi 1 là đương nhiên");
    }
}

#[cfg(test)]
mod live {
    //! Test chạm mạng thật — `--ignored` để không làm CI phụ thuộc badssl.com.
    //! badssl.com dựng sẵn từng loại chứng thư hỏng, nên đây là đối chứng có
    //! đáp án biết trước, không phải chỉ "chạy thử xem sao".
    use super::*;

    async fn ids_of(host: &str, port: u16) -> Vec<String> {
        let now = crate::db::now();
        scan(host, port, now)
            .await
            .unwrap_or_default()
            .iter()
            .map(|f| f.fingerprint.clone())
            .collect()
    }

    #[tokio::test]
    #[ignore]
    async fn broken_targets_produce_exactly_their_intended_finding() {
        for (host, want) in [
            ("expired.badssl.com", "tls:cert:expired"),
            ("self-signed.badssl.com", "tls:cert:self-signed"),
            ("wrong.host.badssl.com", "tls:cert:host-mismatch"),
            ("incomplete-chain.badssl.com", "tls:cert:incomplete-chain"),
        ] {
            let ids = ids_of(host, 443).await;
            assert!(ids.iter().any(|x| x == want), "{host} phải ra {want}, nhận: {ids:?}");
        }
    }

    #[tokio::test]
    #[ignore]
    async fn a_well_configured_host_produces_nothing() {
        assert!(ids_of("github.com", 443).await.is_empty());
    }

    #[tokio::test]
    #[ignore]
    async fn legacy_protocol_is_detected_where_a_modern_client_would_report_nothing() {
        // Đây là lý do tồn tại của ClientHello thô: openssl/rustls hiện đại từ
        // chối gửi TLS 1.0 nên sẽ kết luận "không hỗ trợ" — âm tính giả.
        let ids = ids_of("tls-v1-0.badssl.com", 1010).await;
        assert!(ids.iter().any(|x| x.contains("TLS1.0")), "phải bắt được TLS 1.0: {ids:?}");
    }
}
