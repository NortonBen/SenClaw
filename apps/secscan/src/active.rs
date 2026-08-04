//! Lớp L2 — dò chủ động nhẹ. **Bắt buộc đã xác minh quyền sở hữu.**
//!
//! Khác L1 ở chỗ có gửi yêu cầu tới đường dẫn *không công khai* (`.git/HEAD`,
//! `.env`, trang quản trị). Vẫn không khai thác, không brute-force, không gửi
//! payload tấn công — chỉ hỏi "cái này có lộ ra ngoài không".
//!
//! Hai ràng buộc chi phối toàn bộ thiết kế module này:
//!
//! 1. **Chống 404 mềm.** Rất nhiều site trả `200` kèm trang lỗi đẹp cho mọi
//!    đường dẫn. Kết luận "tìm thấy .env" chỉ vì có `200` là dương tính giả
//!    hàng loạt — nên phải lấy mốc bằng đường dẫn ngẫu nhiên TRƯỚC.
//! 2. **Nhịp gửi thấp hơn ngưỡng hình sự một bậc độ lớn.** Điều 287 BLHS lấy
//!    mốc làm tê liệt mạng 30 phút–24h *hoặc* 3 lần/24h, **không cần thiệt hại
//!    tài chính**. Nên: một yêu cầu tại một thời điểm, có nghỉ giữa các yêu cầu,
//!    và trần cứng số yêu cầu mỗi lần quét.

use crate::db::Finding;
use crate::probe::Resp;
use crate::{scan, vuln};
use anyhow::Result;
use serde_json::json;
use std::time::Duration;

/// Trần cứng số yêu cầu chủ động mỗi lần quét.
pub const MAX_REQUESTS: usize = 40;
/// Nghỉ giữa hai yêu cầu. 250ms → ~4 req/s, thấp hơn nhiều so với mức có thể
/// coi là gây nghẽn.
pub const REQUEST_DELAY: Duration = Duration::from_millis(250);

/// Đường dẫn nhạy cảm đáng hỏi. Mỗi mục kèm mức và lý do — không phải mục nào
/// lộ ra cũng nghiêm trọng như nhau.
const PATHS: &[(&str, &str, &str)] = &[
    (".git/HEAD", "critical",
     "Lộ thư mục .git cho phép tải về TOÀN BỘ lịch sử mã nguồn, gồm cả khoá bí mật đã từng commit rồi xoá."),
    (".env", "critical",
     "Tệp .env thường chứa mật khẩu cơ sở dữ liệu và khoá API ở dạng rõ."),
    (".svn/entries", "high",
     "Siêu dữ liệu SVN cho phép dựng lại mã nguồn, kể cả các tệp đã bị xoá khỏi bản phát hành."),
    ("config.php.bak", "high",
     "Đuôi .bak khiến máy chủ trả nội dung dưới dạng văn bản thuần thay vì thực thi — tức là lộ nguyên mã nguồn kèm thông tin kết nối."),
    ("wp-config.php.bak", "high",
     "Đuôi .bak khiến máy chủ trả nội dung dưới dạng văn bản thuần — wp-config chứa thông tin kết nối CSDL và các khoá xác thực của WordPress."),
    (".DS_Store", "low",
     "Tệp của macOS lộ tên MỌI tệp trong thư mục, kể cả tệp không có liên kết nào trỏ tới — dùng để tìm đường dẫn ẩn."),
    ("composer.json", "low",
     "Lộ danh sách thư viện PHP kèm số phiên bản chính xác — ghép với CSDL CVE là ra ngay lỗ hổng cụ thể đang có, không cần dò."),
    ("package.json", "low",
     "Lộ danh sách thư viện Node kèm số phiên bản chính xác — ghép với CSDL CVE là ra ngay lỗ hổng cụ thể đang có, không cần dò."),
    ("phpinfo.php", "high", "phpinfo() lộ đường dẫn tuyệt đối, biến môi trường và cấu hình máy chủ."),
    ("server-status", "medium",
     "Trang trạng thái Apache lộ URL mà những người dùng KHÁC đang truy cập theo thời gian thực, gồm cả tham số trong query string."),
    ("actuator/health", "medium",
     "Spring Actuator đang mở. Bản thân /health thường vô hại, nhưng nó báo hiệu các endpoint anh em như /actuator/env và /actuator/heapdump cũng có thể đang mở — kiểm tra ngay."),
    ("debug/default/view", "high",
     "Thanh gỡ lỗi Yii lưu lại toàn bộ request gần đây, gồm cả cookie phiên và tham số đăng nhập của người dùng thật."),
    ("backup.sql", "critical",
     "Kết xuất cơ sở dữ liệu tải về công khai — toàn bộ dữ liệu người dùng, thường gồm cả bảng mật khẩu."),
    ("db.sqlite", "critical",
     "Tệp cơ sở dữ liệu tải về công khai — mở bằng bất kỳ trình đọc SQLite nào là đọc được sạch dữ liệu."),
];

/// Đường dẫn hay có liệt kê thư mục.
const DIRS: &[&str] = &["uploads/", "files/", "backup/", "static/", "assets/", "images/"];

// ---------------------------------------------------------------------------
// Mốc 404 mềm
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Baseline {
    /// Mã trạng thái cho đường dẫn chắc chắn không tồn tại.
    pub status: u16,
    /// Độ dài thân trang lỗi (lấy trung vị của các lần thử).
    pub len: usize,
    /// Site trả cùng một mã cho mọi thứ — mã trạng thái mất giá trị phân biệt.
    pub soft_404: bool,
}

/// Chênh lệch độ dài tương đối coi là "khác hẳn". Trang lỗi thường chỉ dao động
/// vài byte (đường dẫn echo lại), nên 25% là ngưỡng rộng rãi mà vẫn an toàn.
const LEN_TOLERANCE: f64 = 0.25;

impl Baseline {
    /// Phản hồi này có khác mốc đủ để coi là tệp THẬT SỰ tồn tại không.
    pub fn looks_real(&self, status: u16, len: usize) -> bool {
        if !(200..300).contains(&status) {
            return false;
        }
        // Site trả 200 cho mọi thứ: phải dựa vào độ dài, không dựa vào mã.
        if self.soft_404 {
            let base = self.len.max(1) as f64;
            let diff = (len as f64 - base).abs() / base;
            return diff > LEN_TOLERANCE;
        }
        true
    }
}

/// Lấy mốc bằng các đường dẫn chắc chắn không tồn tại.
///
/// Dùng nhiều đường dẫn vì một số site trả trang khác nhau tuỳ phần mở rộng.
pub async fn baseline(
    http: &reqwest::Client,
    base_url: &str,
    allow_local: bool,
    budget: &mut usize,
) -> Baseline {
    let probes = [
        "senclaw-probe-a1b2c3d4.txt",
        "senclaw-probe-e5f6a7b8/",
        "senclaw-probe-c9d0e1f2.php",
    ];
    let mut statuses = vec![];
    let mut lens = vec![];
    for p in probes {
        if *budget == 0 {
            break;
        }
        *budget -= 1;
        tokio::time::sleep(REQUEST_DELAY).await;
        if let Ok(r) = scan::fetch(http, &join(base_url, p), allow_local).await {
            statuses.push(r.status);
            lens.push(r.body_snippet.len());
        }
    }
    if statuses.is_empty() {
        return Baseline { status: 404, len: 0, soft_404: false };
    }
    lens.sort_unstable();
    let median = lens[lens.len() / 2];
    let soft = statuses.iter().any(|s| (200..300).contains(s));
    Baseline {
        status: statuses[0],
        len: median,
        soft_404: soft,
    }
}

fn join(base: &str, path: &str) -> String {
    format!("{}/{}", base.trim_end_matches('/'), path.trim_start_matches('/'))
}

// ---------------------------------------------------------------------------
// CORS
// ---------------------------------------------------------------------------

/// Phân tích cấu hình CORS từ phản hồi khi gửi `Origin` lạ.
///
/// Đây là phép kiểm đáng giá nhất của lớp này: `ACAO` phản chiếu Origin tuỳ ý
/// **cộng với** `Allow-Credentials: true` nghĩa là bất kỳ trang web nào cũng đọc
/// được dữ liệu đã đăng nhập của người dùng. `ACAO: *` trên dịch vụ không xác
/// thực cũng đủ tệ: mọi trang người dùng đang mở đều gọi được API đó.
pub fn analyze_cors(r: &Resp, probe_origin: &str) -> Vec<Finding> {
    let mut out = vec![];
    let Some(acao) = r.get("access-control-allow-origin") else {
        return out;
    };
    let creds = r
        .get("access-control-allow-credentials")
        .map(|v| v.trim().eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let acao = acao.trim();

    if acao == probe_origin {
        let sev = if creds { "critical" } else { "high" };
        out.push(
            Finding::new("cors", sev, "cors:reflects-origin", "CORS phản chiếu mọi Origin")
                .detail(if creds {
                    "Máy chủ phản chiếu lại Origin bất kỳ VÀ cho gửi kèm cookie. Bất kỳ trang web nào người dùng mở cũng đọc được dữ liệu đã đăng nhập của họ trên site này."
                } else {
                    "Máy chủ phản chiếu lại Origin bất kỳ, nên chính sách cùng nguồn của trình duyệt mất tác dụng với API này."
                })
                .evidence(json!({ "sent_origin": probe_origin, "acao": acao, "credentials": creds }))
                .fix("Chỉ phản chiếu Origin nằm trong danh sách cho phép; không bao giờ phản chiếu vô điều kiện.")
                .wstg("WSTG-CLNT-07"),
        );
    } else if acao == "*" {
        // Trình duyệt CẤM tổ hợp `*` + credentials, nên `*` một mình là mức thấp
        // hơn — trừ khi dịch vụ vốn không cần xác thực mà vẫn trả dữ liệu riêng.
        out.push(
            Finding::new("cors", "medium", "cors:wildcard", "CORS mở cho mọi nguồn (ACAO: *)")
                .detail("Bất kỳ trang web nào người dùng đang mở cũng gọi được API này từ trình duyệt của họ. Nếu dịch vụ tin tưởng vào việc 'chỉ chạy trên localhost' thì giả định đó không còn đúng.")
                .evidence(json!({ "acao": "*" }))
                .fix("Giới hạn danh sách nguồn, hoặc bỏ hẳn CORS nếu API chỉ dùng cùng nguồn.")
                .wstg("WSTG-CLNT-07"),
        );
    } else if acao.eq_ignore_ascii_case("null") {
        out.push(
            Finding::new("cors", "high", "cors:null-origin", "CORS chấp nhận Origin 'null'")
                .detail("Origin 'null' đến từ iframe sandbox và tệp local — kẻ tấn công tạo được nó dễ dàng.")
                .fix("Bỏ 'null' khỏi danh sách nguồn cho phép.")
                .wstg("WSTG-CLNT-07"),
        );
    }
    out
}

// ---------------------------------------------------------------------------
// Liệt kê thư mục
// ---------------------------------------------------------------------------

/// Trang này có phải danh sách thư mục không. Nhận diện theo dấu hiệu của các
/// máy chủ phổ biến chứ không đoán mò theo mã trạng thái.
pub fn is_directory_listing(body: &str) -> bool {
    let b = body.to_ascii_lowercase();
    b.contains("<title>index of /")
        || b.contains("<h1>index of /")
        || (b.contains("directory listing for") && b.contains("<ul>"))
        || (b.contains("parent directory") && b.contains("<a href="))
}

// ---------------------------------------------------------------------------
// Chạy
// ---------------------------------------------------------------------------

pub struct ActiveResult {
    pub findings: Vec<Finding>,
    pub requests: usize,
    /// Ngân sách chạm trần — phải nói ra, không được im lặng cắt bớt.
    pub truncated: bool,
    /// Số gói đã đối chiếu CSDL lỗ hổng (từ manifest lộ ra ngoài).
    pub packages_checked: usize,
}

pub async fn run(
    http: &reqwest::Client,
    base_url: &str,
    allow_local: bool,
) -> Result<ActiveResult> {
    let mut budget = MAX_REQUESTS;
    let mut findings = vec![];

    let mut manifests: Vec<(&str, String)> = vec![];
    let base = baseline(http, base_url, allow_local, &mut budget).await;

    // Nếu tất cả yêu cầu mốc đều fail thì rất có thể đích không chạm được (rào
    // SSRF chặn hoặc mạng hỏng). Chạy tiếp là im lặng trả "0 phát hiện" — đọc
    // như "sạch" trong khi thực ra chưa hỏi được gì.
    if base.status == 404 && base.len == 0 && !base.soft_404 {
        // baseline() trả về default khi mọi probe fail — dùng thử một fetch riêng
        // để biết đó là "site 404 đúng đắn" hay "không chạm được".
        if scan::fetch(http, base_url, allow_local).await.is_err() {
            return Ok(ActiveResult {
                findings: vec![Finding::new(
                    "exposure", "info", "active:unreachable",
                    "Không chạm được đích để quét chủ động",
                )
                .detail(
                    "Có thể đích đang tắt, hoặc là dải nội bộ mà tài sản chưa xác minh bằng \
                     phương thức 'local' (rào SSRF của chính scanner). Với hạ tầng nội bộ, \
                     vào tab 'Sở hữu' rồi chọn 'Mạng nội bộ'.",
                )],
                requests: MAX_REQUESTS - budget,
                truncated: false,
                packages_checked: 0,
            });
        }
    }
    if base.soft_404 {
        findings.push(
            Finding::new("exposure", "info", "active:soft-404", "Máy chủ trả 200 cho đường dẫn không tồn tại")
                .detail("Không thể dựa vào mã trạng thái để biết tệp có tồn tại hay không, nên phép kiểm chuyển sang so sánh độ dài nội dung — độ tin cậy thấp hơn.")
                .evidence(json!({ "baseline_len": base.len })),
        );
    }

    // --- tệp nhạy cảm ---
    for (path, sev, why) in PATHS {
        if budget == 0 {
            break;
        }
        budget -= 1;
        tokio::time::sleep(REQUEST_DELAY).await;
        let url = join(base_url, path);
        let Ok(r) = scan::fetch(http, &url, allow_local).await else {
            continue;
        };
        if !base.looks_real(r.status, r.body_snippet.len()) {
            continue;
        }
        // Kiểm nội dung cho vài loại có dấu hiệu rõ ràng — chống dương tính giả
        // khi site trả trang chủ cho mọi đường dẫn.
        let body = &r.body_snippet;
        let confirmed = match *path {
            ".git/HEAD" => body.trim_start().starts_with("ref:") || body.trim().len() == 40,
            ".env" => body.contains('=') && !body.to_ascii_lowercase().contains("<html"),
            "composer.json" | "package.json" => body.trim_start().starts_with('{'),
            "phpinfo.php" => body.to_ascii_lowercase().contains("phpinfo()"),
            ".DS_Store" => body.as_bytes().starts_with(&[0x00, 0x00, 0x00, 0x01]),
            _ => !body.to_ascii_lowercase().contains("<!doctype html>") || body.len() > 100,
        };
        if !confirmed {
            continue;
        }
        // Manifest lộ ra vừa là lỗi lộ thông tin, vừa là DANH SÁCH GÓI để đối
        // chiếu CVE — thứ có giá trị hơn hẳn bản thân việc nó lộ.
        match *path {
            "package.json" => manifests.push(("npm", body.clone())),
            "composer.json" => manifests.push(("Packagist", body.clone())),
            _ => {}
        }
        findings.push(
            Finding::new("exposure", sev, format!("active:file:{path}"), format!("Lộ tệp '{path}'"))
                .detail(*why)
                .evidence(json!({ "url": url, "status": r.status, "bytes": body.len() }))
                .fix(format!("Chặn '{path}' ở tầng web server, và kiểm tra xem nó đã lộ bao lâu."))
                .wstg("WSTG-CONF-04"),
        );
    }

    // --- liệt kê thư mục ---
    for d in DIRS {
        if budget == 0 {
            break;
        }
        budget -= 1;
        tokio::time::sleep(REQUEST_DELAY).await;
        let url = join(base_url, d);
        let Ok(r) = scan::fetch(http, &url, allow_local).await else {
            continue;
        };
        if (200..300).contains(&r.status) && is_directory_listing(&r.body_snippet) {
            findings.push(
                Finding::new("exposure", "medium", format!("active:dirlist:{d}"), format!("Liệt kê thư mục ở '{d}'"))
                    .detail("Người ngoài xem được toàn bộ danh sách tệp, gồm cả tệp không có liên kết nào trỏ tới.")
                    .evidence(json!({ "url": url }))
                    .fix("Tắt autoindex (nginx) hoặc Options -Indexes (Apache).")
                    .wstg("WSTG-CONF-04"),
            );
        }
    }

    // --- CORS ---
    if budget > 0 {
        budget -= 1;
        tokio::time::sleep(REQUEST_DELAY).await;
        const PROBE_ORIGIN: &str = "https://senclaw-cors-probe.invalid";
        if let Ok(r) = fetch_with_origin(http, base_url, PROBE_ORIGIN, allow_local).await {
            findings.extend(analyze_cors(&r, PROBE_ORIGIN));
        }
    }

    // --- đối chiếu CVE từ manifest bắt được ---
    let mut packages_checked = 0;
    for (eco, body) in &manifests {
        let pkgs = vuln::packages_from_manifest(body, eco);
        if pkgs.is_empty() {
            continue;
        }
        match vuln::scan(http, &pkgs).await {
            Ok(r) => {
                packages_checked += r.packages_checked;
                findings.extend(r.findings);
            }
            Err(e) => findings.push(
                Finding::new("cve", "info", "cve:lookup-failed", "Không tra được CSDL lỗ hổng")
                    .detail(format!("{e}. Danh sách gói đã lấy được nhưng chưa đối chiếu — kết quả KHÔNG có nghĩa là không có lỗ hổng.")),
            ),
        }
    }

    Ok(ActiveResult {
        requests: MAX_REQUESTS - budget,
        truncated: budget == 0,
        findings,
        packages_checked,
    })
}

/// Gửi một GET kèm `Origin` lạ để xem máy chủ trả CORS thế nào.
async fn fetch_with_origin(
    http: &reqwest::Client,
    url: &str,
    origin: &str,
    allow_local: bool,
) -> Result<Resp> {
    let host = crate::scope::host_of(url)?;
    crate::scope::check_host_allowed(&host, allow_local).await?;
    let resp = http.get(url).header("Origin", origin).send().await?;
    let status = resp.status().as_u16();
    let headers: Vec<(String, String)> = resp
        .headers()
        .iter()
        .map(|(k, v)| {
            (
                k.as_str().to_ascii_lowercase(),
                String::from_utf8_lossy(v.as_bytes()).to_string(),
            )
        })
        .collect();
    let body: String = resp.text().await.unwrap_or_default().chars().take(4096).collect();
    Ok(Resp {
        url: url.to_string(),
        status,
        headers,
        body_snippet: body,
        https: url.starts_with("https://"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resp(headers: &[(&str, &str)]) -> Resp {
        Resp {
            url: "https://x.vn/".into(),
            status: 200,
            headers: headers.iter().map(|(k, v)| (k.to_ascii_lowercase(), v.to_string())).collect(),
            body_snippet: String::new(),
            https: true,
        }
    }
    fn ids(f: &[Finding]) -> Vec<&str> {
        f.iter().map(|x| x.fingerprint.as_str()).collect()
    }

    const ORIGIN: &str = "https://senclaw-cors-probe.invalid";

    #[test]
    fn reflected_origin_with_credentials_is_critical() {
        // Tổ hợp tệ nhất: bất kỳ trang nào cũng đọc được dữ liệu đã đăng nhập.
        let r = resp(&[
            ("access-control-allow-origin", ORIGIN),
            ("access-control-allow-credentials", "true"),
        ]);
        let f = analyze_cors(&r, ORIGIN);
        assert_eq!(f[0].fingerprint, "cors:reflects-origin");
        assert_eq!(f[0].severity, "critical");
    }

    #[test]
    fn reflected_origin_without_credentials_ranks_lower() {
        let r = resp(&[("access-control-allow-origin", ORIGIN)]);
        assert_eq!(analyze_cors(&r, ORIGIN)[0].severity, "high");
    }

    #[test]
    fn wildcard_is_flagged_but_below_reflection() {
        let f = analyze_cors(&resp(&[("access-control-allow-origin", "*")]), ORIGIN);
        assert_eq!(f[0].fingerprint, "cors:wildcard");
        assert_eq!(f[0].severity, "medium");
    }

    #[test]
    fn a_properly_pinned_origin_produces_nothing() {
        // Máy chủ trả đúng nguồn của mình, không phản chiếu -> không phải lỗi.
        let r = resp(&[("access-control-allow-origin", "https://app.x.vn")]);
        assert!(analyze_cors(&r, ORIGIN).is_empty());
        // không có header CORS cũng không phải lỗi
        assert!(analyze_cors(&resp(&[]), ORIGIN).is_empty());
    }

    #[test]
    fn null_origin_is_flagged() {
        let f = analyze_cors(&resp(&[("access-control-allow-origin", "null")]), ORIGIN);
        assert_eq!(f[0].fingerprint, "cors:null-origin");
    }

    #[test]
    fn soft_404_site_cannot_be_judged_by_status_code_alone() {
        // Site trả 200 cho mọi thứ, trang lỗi dài 5000 byte.
        let b = Baseline { status: 200, len: 5000, soft_404: true };
        assert!(!b.looks_real(200, 5000), "trùng mốc -> không phải tệp thật");
        assert!(!b.looks_real(200, 5100), "lệch 2% vẫn là trang lỗi");
        assert!(b.looks_real(200, 200), "khác hẳn -> đáng nghi");
        assert!(b.looks_real(200, 20000), "dài hơn nhiều cũng khác hẳn");
        assert!(!b.looks_real(404, 100), "mã lỗi thì luôn loại");
    }

    #[test]
    fn normal_site_trusts_the_status_code() {
        let b = Baseline { status: 404, len: 150, soft_404: false };
        assert!(b.looks_real(200, 150), "site 404 đúng thì 200 là tin được");
        assert!(!b.looks_real(404, 150));
        assert!(!b.looks_real(403, 0), "403 không phải là lộ tệp");
        assert!(!b.looks_real(301, 0), "chuyển hướng cũng không");
    }

    #[test]
    fn directory_listing_detection_matches_common_servers() {
        for body in [
            "<html><head><title>Index of /uploads</title></head>",
            "<h1>Index of /files</h1><pre>",
            "<title>Directory listing for /x</title><ul><li>",
            "<a href=\"../\">Parent Directory</a>",
        ] {
            assert!(is_directory_listing(body), "phải nhận ra: {body}");
        }
        for body in [
            "<html><body>Trang chủ</body></html>",
            "<h1>404 Not Found</h1>",
            "index of the article", // chỉ là văn xuôi, không phải danh sách
        ] {
            assert!(!is_directory_listing(body), "không được nhận nhầm: {body}");
        }
    }

    #[test]
    fn url_join_handles_slashes_consistently() {
        assert_eq!(join("https://a.vn", ".env"), "https://a.vn/.env");
        assert_eq!(join("https://a.vn/", ".env"), "https://a.vn/.env");
        assert_eq!(join("https://a.vn/", "/.env"), "https://a.vn/.env");
        assert_eq!(join("https://a.vn/app/", "x/"), "https://a.vn/app/x/");
    }

    #[test]
    fn request_budget_is_low_enough_to_stay_far_from_any_dos_threshold() {
        // Điều 287 BLHS lấy mốc tê liệt 30 phút. Ở nhịp này cả lần quét kéo dài
        // ~10 giây với ~4 yêu cầu/giây — thấp hơn nhiều bậc độ lớn.
        assert!(MAX_REQUESTS <= 50);
        let total = REQUEST_DELAY * MAX_REQUESTS as u32;
        assert!(total >= Duration::from_secs(8), "phải có nghỉ thật giữa các yêu cầu");
        let rate = MAX_REQUESTS as f64 / total.as_secs_f64();
        assert!(rate <= 5.0, "nhịp {rate} req/s là quá nhanh");
    }

    #[test]
    fn every_declared_path_has_a_reason_and_a_valid_severity() {
        for (p, sev, why) in PATHS {
            assert!(
                ["critical", "high", "medium", "low", "info"].contains(sev),
                "{p} có mức lạ: {sev}"
            );
            assert!(why.chars().count() > 40, "{p} thiếu lý do tử tế");
            assert!(!p.starts_with('/'), "{p} không được bắt đầu bằng /");
        }
        // Ngân sách phải đủ cho mốc + toàn bộ đường dẫn + thư mục + CORS
        assert!(3 + PATHS.len() + DIRS.len() + 1 <= MAX_REQUESTS);
    }
}
