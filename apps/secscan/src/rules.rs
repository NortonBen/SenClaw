//! Danh mục tiêu chuẩn quét — app tự khai nó kiểm những gì.
//!
//! Đây không phải tài liệu trang trí. Một scanner không nói rõ mình kiểm gì thì
//! người đọc báo cáo không cách nào biết "không có phát hiện" nghĩa là *an toàn*
//! hay là *không kiểm*. Danh mục này trả lời câu đó.
//!
//! **Chốt chặn chống lệch:** test ở cuối file chạy các đầu dò trên phản hồi dựng
//! sẵn rồi đối chiếu mọi `fingerprint` sinh ra với danh mục. Thêm phép kiểm mà
//! quên khai ở đây là test đỏ.

use serde_json::{json, Value};

/// Mức xâm nhập. Chỉ L1 đã cài; L2/L3 khai ở đây để người dùng biết lộ trình
/// và biết **hiện chưa kiểm** những mục đó.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    /// Quan sát thụ động — một GET, không payload. Không cần xác minh sở hữu.
    Passive,
    /// Dò chủ động nhẹ — cần xác minh sở hữu. Chưa cài.
    Active,
    /// Kiểm cấu hình máy chủ qua SSH. Chưa cài.
    Host,
}

impl Layer {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Passive => "passive",
            Self::Active => "active",
            Self::Host => "host",
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            Self::Passive => "L1 · Thụ động",
            Self::Active => "L2 · Chủ động",
            Self::Host => "L3 · Máy chủ",
        }
    }
}

pub struct Rule {
    /// Tiền tố fingerprint mà phép kiểm này sinh ra.
    pub id: &'static str,
    pub category: &'static str,
    pub layer: Layer,
    /// Mức nặng nhất phép kiểm có thể sinh ra.
    pub max_severity: &'static str,
    pub title: &'static str,
    /// Vì sao kiểm, và vì sao mức độ được đặt như vậy. Đây là phần đáng đọc.
    pub rationale: &'static str,
    pub wstg: &'static str,
    /// Đã cài chưa. `false` = khai để minh bạch phạm vi, chưa chạy.
    pub implemented: bool,
}

macro_rules! rule {
    ($id:expr, $cat:expr, $layer:expr, $sev:expr, $title:expr, $why:expr, $wstg:expr, $impl:expr) => {
        Rule {
            id: $id, category: $cat, layer: $layer, max_severity: $sev,
            title: $title, rationale: $why, wstg: $wstg, implemented: $impl,
        }
    };
}

pub fn catalogue() -> Vec<Rule> {
    use Layer::*;
    vec![
        // ---------------- Header truyền tải & khung ----------------
        rule!("hdr:hsts", "headers", Passive, "high",
            "Strict-Transport-Security",
            "Thiếu HSTS thì lần truy cập đầu tiên vẫn hạ được xuống HTTP. Riêng `max-age=0` bị chấm CAO chứ không phải 'thiếu': nó chủ động XOÁ trạng thái HSTS đã ghim trong trình duyệt, tức là tệ hơn chưa từng đặt.",
            "WSTG-CONF-07", true),

        rule!("hdr:csp", "headers", Passive, "high",
            "Content-Security-Policy",
            "Kiểm cả chất lượng chính sách, không chỉ sự tồn tại. Quan trọng nhất: nếu đã có nonce/hash thì trình duyệt BỎ QUA 'unsafe-inline', nên app không báo lỗi ở trường hợp đó — đây là nguồn dương-tính-giả phổ biến nhất của các scanner khác. Ngược lại 'strict-dynamic' mà thiếu nonce là chính sách HỎNG (chặn hết script), không phải chính sách chặt.",
            "WSTG-CONF-12", true),

        rule!("hdr:frame", "headers", Passive, "medium",
            "Chống đóng khung (clickjacking)",
            "Kiểm 'có chống đóng khung không', không kiểm riêng X-Frame-Options: nếu CSP có frame-ancestors thì trình duyệt bỏ qua XFO hoàn toàn, đòi thêm XFO là thừa. Bắt riêng trường hợp nhiều header XFO mà tất cả đều sai — theo thuật toán WHATWG nó bị xử lý như KHÔNG CÓ, tức hỏng theo hướng mở.",
            "WSTG-CLNT-09", true),

        rule!("hdr:xcto", "headers", Passive, "low",
            "X-Content-Type-Options",
            "Chặn trình duyệt tự đoán kiểu nội dung thay vì tin Content-Type. Không có nó, một tệp người dùng tải lên được khai là text/plain vẫn có thể bị trình duyệt thực thi như HTML hoặc JavaScript. Giá trị hợp lệ duy nhất là 'nosniff' — đặt giá trị khác cũng bằng không đặt.",
            "", true),

        rule!("hdr:referrer", "headers", Passive, "medium",
            "Referrer-Policy",
            "VẮNG MẶT chỉ là mức thông tin — mặc định của trình duyệt (strict-origin-when-cross-origin) vốn đã an toàn. Phát hiện thật là khi ai đó chủ động đặt giá trị RÒ RỈ HƠN mặc định, ví dụ 'unsafe-url'.",
            "WSTG-CONF-07", true),

        rule!("hdr:permpolicy", "headers", Passive, "low",
            "Permissions-Policy",
            "Vắng mặt chỉ là mức thông tin: Firefox và Safari không hỗ trợ header này ở bất kỳ phiên bản nào, nên chấm nặng một thứ chỉ chạy trên Chromium là không trung thực. Có báo riêng 'interest-cohort' — directive phổ biến nhất trên web mà đã hoàn toàn vô nghĩa từ khi FLoC bị khai tử.",
            "", true),

        rule!("hdr:xxp", "headers", Passive, "low",
            "X-XSS-Protection (đã khai tử)",
            "Báo khi header này đang BẬT, ngược với trực giác thông thường. Bộ lọc XSS của trình duyệt đã bị gỡ bỏ và MDN cảnh báo nó có thể TẠO RA lỗ hổng trên site vốn an toàn.",
            "", true),

        // ---------------- Cookie ----------------
        rule!("cookie:secure", "cookies", Passive, "high",
            "Cờ Secure",
            "Cookie trông giống cookie phiên (sess/login/auth/token/sid/jwt, PHPSESSID, JSESSIONID…) mà thiếu Secure thì bị chấm CAO, cookie thường chấm TRUNG BÌNH — vì hậu quả khác hẳn nhau.",
            "WSTG-SESS-02", true),

        rule!("cookie:httponly", "cookies", Passive, "medium",
            "Cờ HttpOnly",
            "Chỉ báo cho cookie phiên: JavaScript đọc được cookie phiên nghĩa là một lỗ XSS đủ để chiếm phiên.",
            "WSTG-SESS-02", true),

        rule!("cookie:samesite", "cookies", Passive, "high",
            "Thuộc tính SameSite",
            "Không coi nhẹ vì 'trình duyệt hiện đại mặc định Lax rồi' — điều đó chỉ đúng với Chrome/Edge. Firefox coi cookie không có SameSite như None (bug 1617609 đóng WONTFIX) và Safari không hỗ trợ, nên đây là thiếu sót thật. 'SameSite=None' mà không có Secure bị chấm CAO: tổ hợp đó không hợp lệ, trình duyệt loại bỏ cookie luôn.",
            "WSTG-SESS-02", true),

        rule!("cookie:host-prefix", "cookies", Passive, "medium",
            "Tiền tố __Host- / __Secure-",
            "Hai tiền tố này là bảo đảm do TRÌNH DUYỆT cưỡng chế. Vi phạm ràng buộc thì cookie bị từ chối âm thầm — vừa là lỗi bảo mật vừa là lỗi chức năng, và không có thông báo nào.",
            "WSTG-SESS-02", true),

        rule!("cookie:secure-prefix", "cookies", Passive, "medium",
            "Ràng buộc __Secure-",
            "Tiền tố __Secure- bắt buộc cookie phải có cờ Secure — ràng buộc nhẹ hơn __Host- (không đòi Path=/ và vẫn cho đặt Domain). Vi phạm thì trình duyệt từ chối cookie mà không báo gì, nên triệu chứng thường hiện ra dưới dạng 'đăng nhập xong lại bị đăng xuất' chứ không phải một lỗi bảo mật lộ liễu.",
            "WSTG-SESS-02", true),

        // ---------------- Lộ thông tin ----------------
        rule!("exp:banner", "exposure", Passive, "low",
            "Header lộ phiên bản",
            "server / x-powered-by / x-aspnet-version / x-generator. Có kèm SỐ PHIÊN BẢN thì chấm THẤP (ghép được với CSDL CVE để tìm lỗ hổng cụ thể); không có số thì chỉ là THÔNG TIN.",
            "WSTG-INFO-02", true),

        rule!("exp:sourcemap", "exposure", Passive, "medium",
            "Lộ source map",
            "Header SourceMap/X-SourceMap cho phép dựng lại mã nguồn gốc từ bản đã đóng gói.",
            "WSTG-INFO-02", true),

        // ---------------- DNS & email ----------------
        rule!("dns:spf", "dns", Passive, "high",
            "SPF",
            "Không chỉ kiểm có hay không. '+all' bị chấm CAO vì tệ hơn cả không có SPF — nó khẳng định mọi máy chủ đều được phép gửi. Nhiều bản ghi SPF là cấu hình KHÔNG hợp lệ (RFC 7208 buộc đúng một). Có đếm giới hạn 10 lần tra cứu DNS: vượt là bên nhận trả permerror và SPF mất tác dụng hoàn toàn — một lỗi im lặng rất hay gặp.",
            "WSTG-CONF-11", true),

        rule!("dns:dmarc", "dns", Passive, "medium",
            "DMARC",
            "'p=none' vẫn bị báo: nó chỉ giám sát, thư giả mạo vẫn vào hộp thư người nhận. Đang cưỡng chế mà thiếu 'rua=' cũng bị báo, vì không có báo cáo thì không biết mình đang chặn nhầm thư hợp lệ nào.",
            "WSTG-CONF-11", true),

        rule!("dns:caa", "dns", Passive, "low",
            "CAA",
            "Giới hạn CA nào được phép cấp chứng thư cho tên miền. Không có CAA thì BẤT KỲ CA công khai nào trên thế giới cũng cấp được chứng thư hợp lệ cho tên miền của bạn — chỉ cần một CA bị lừa hoặc bị xâm nhập là đủ. Mức THẤP vì đây là phòng thủ theo chiều sâu, không phải lỗ hổng khai thác trực tiếp.",
            "WSTG-CONF-11", true),

        rule!("dns:dnssec", "dns", Passive, "low",
            "DNSSEC",
            "Không có bản ghi DS ở zone cha thì câu trả lời DNS không xác thực được, nên mọi phòng thủ DỰA TRÊN DNS đều yếu theo: CAA, MTA-STS, và cả chính bằng chứng xác minh sở hữu bằng TXT. Mức THẤP vì tấn công đầu độc DNS đòi vị trí mạng thuận lợi, nhưng nó là nền móng cho các lớp trên.",
            "", true),

        rule!("dns:dkim", "dns", Passive, "info",
            "DKIM — chỉ báo là KHÔNG kiểm được",
            "Cố ý không bao giờ kết luận 'thiếu DKIM'. Selector nằm ở <selector>._domainkey và DNS không cho liệt kê, nên phải lấy selector từ header một email thật rồi kiểm tay. Báo 'không có DKIM' khi thực ra không kiểm được chính là dương tính giả.",
            "", true),

        rule!("dns:query-failed", "dns", Passive, "info",
            "Phân biệt tra cứu hỏng với không có bản ghi",
            "Truy vấn DNS thất bại (timeout, lỗi mạng) KHÔNG bao giờ được biến thành phát hiện 'thiếu bản ghi'. Hai bẫy đã gặp thật: tên không tuyệt đối khiến resolver nối search domain của máy vào; và TXT ở apex vượt 512 byte cần EDNS0 + TCP fallback, không có thì treo tới timeout.",
            "", true),

        // ---------------- Chưa cài — khai để minh bạch phạm vi ----------------
        rule!("tls:cert", "tls", Passive, "critical",
            "Chứng thư TLS",
            "Hết hạn bị chấm NGHIÊM TRỌNG chứ không phải 'cao': trình duyệt ĐANG chặn người dùng ngay lúc đó, không phải rủi ro tương lai. Chuỗi thiếu bản trung gian chấm TRUNG BÌNH vì trình duyệt máy tính thường tự bù bằng AIA còn client di động thì không — lỗi chỉ hiện với một phần người dùng. Ngưỡng độ dài khoá tách riêng EC và RSA: 256-bit EC mạnh hơn RSA 2048, dùng chung một con số là báo sai.",
            "WSTG-CRYP-01", true),

        rule!("tls:version", "tls", Passive, "high",
            "Phiên bản giao thức TLS",
            "Dò bằng ClientHello TỰ DỰNG chứ không qua client TLS hiện đại: `openssl s_client -tls1` trả 'no protocols available' là do CLIENT từ chối, chưa gửi gói nào — kết luận 'server không hỗ trợ' từ đó là âm tính giả. rustls chỉ nói được 1.2/1.3. Gửi byte thô thì không thư viện nào phủ quyết được, và đây cũng là cách testssl.sh làm.",
            "WSTG-CRYP-01", true),

        rule!("tls:unavailable", "tls", Passive, "info",
            "Không dò được TLS",
            "Lỗi mạng khi bắt tay được báo riêng ở mức thông tin, và KHÔNG làm hỏng cả lần quét — header với DNS đã lấy được vẫn giữ nguyên giá trị. Cùng nguyên tắc với DNS: không kiểm được thì nói là không kiểm được, đừng kết luận là đạt.",
            "", true),

        rule!("active:file", "active", Active, "critical",
            "Tệp lộ ra ngoài",
            "Hỏi từng đường dẫn nhạy cảm (.git/HEAD, .env, backup.sql, db.sqlite…). Chống dương-tính-giả bằng MỐC 404 MỀM: nhiều site trả 200 kèm trang lỗi đẹp cho mọi đường dẫn, nên phải hỏi vài đường dẫn ngẫu nhiên trước rồi so độ dài nội dung. Thêm một lớp nữa là kiểm dấu hiệu nội dung (.git/HEAD phải bắt đầu bằng 'ref:', package.json phải bắt đầu bằng '{').",
            "WSTG-CONF-04", true),

        rule!("active:dirlist", "active", Active, "medium",
            "Liệt kê thư mục",
            "Nhận diện theo dấu hiệu của máy chủ thật ('Index of /', 'Parent Directory') chứ không đoán theo mã trạng thái. Người ngoài xem được cả những tệp không có liên kết nào trỏ tới.",
            "WSTG-CONF-04", true),

        rule!("cors:", "cors", Active, "critical",
            "Cấu hình CORS",
            "Gửi một Origin lạ rồi xem máy chủ trả gì. PHẢN CHIẾU Origin tuỳ ý + Allow-Credentials là NGHIÊM TRỌNG: bất kỳ trang web nào người dùng mở cũng đọc được dữ liệu đã đăng nhập của họ. 'ACAO: *' nhẹ hơn một bậc vì trình duyệt cấm ghép nó với credentials — nhưng vẫn đủ để phá vỡ giả định 'dịch vụ này chỉ chạy nội bộ nên an toàn'.",
            "WSTG-CLNT-07", true),

        rule!("active:soft-404", "active", Active, "info",
            "Máy chủ trả 200 cho đường dẫn không tồn tại",
            "Không phải lỗ hổng, nhưng phải báo: khi mã trạng thái mất giá trị phân biệt thì mọi phép kiểm tệp lộ chuyển sang so độ dài nội dung, và độ tin cậy thấp hơn hẳn. Người đọc báo cáo cần biết điều đó.",
            "", true),

        rule!("active:budget-exhausted", "active", Active, "info",
            "Chạm trần số yêu cầu",
            "Nhịp gửi cố ý thấp hơn ngưỡng hình sự một bậc độ lớn (Điều 287 BLHS lấy mốc tê liệt 30 phút HOẶC 3 lần/24h, không cần thiệt hại tài chính). Khi chạm trần thì nói thẳng là kết quả bán phần — cắt bớt mà im lặng khiến báo cáo đọc như đã phủ hết.",
            "", true),

        rule!("cve:", "vuln", Active, "critical",
            "Đối chiếu CVE (OSV + KEV + EPSS)",
            "Xếp ưu tiên bằng KEV như phép ĐÈ CỨNG chứ không phải trọng số: một mục trong danh mục đang-bị-khai-thác-thật phải đứng trên mục điểm CVSS cao hơn nhưng không ai khai thác, và KEV còn nâng thẳng mức lên nghiêm trọng. Ngưỡng hành động EPSS 0.1 theo số liệu của FIRST — tốn ~2.7% công sức cho ~45% hiệu quả, so với lọc CVSS≥7 tốn ~50% công sức chỉ được ~6%. Nguồn gói hiện tại là manifest lộ ra ngoài (package.json/composer.json) mà lớp chủ động bắt được. Chặn cứng định danh Maven thiếu groupId: gửi 'log4j-core' trần thì OSV trả rỗng và kết quả trông như an toàn.",
            "", true),

        rule!("host:ssh", "host", Host, "critical",
            "Cấu hình SSHD",
            "PermitRootLogin=yes chấm CAO (đăng nhập trực tiếp bằng root làm mất dấu vết ai làm gì); PermitEmptyPasswords=yes chấm NGHIÊM TRỌNG (bất kỳ tài khoản trống mật khẩu nào cũng vào được). PasswordAuthentication chấm TRUNG BÌNH — mở đường cho dò mật khẩu tự động nhưng không tự nó là lỗ hổng.",
            "WSTG-CONF-05", true),

        rule!("host:port", "host", Host, "critical",
            "Cổng nhạy cảm phơi ra ngoài",
            "MySQL/PostgreSQL/MongoDB/Redis/Elasticsearch/Docker API nghe trên 0.0.0.0 là NGHIÊM TRỌNG: nhiều dịch vụ không có xác thực mạnh mặc định. Chỉ báo cổng phơi ra loopback vì bind 127.0.0.1 không phải lỗ hổng.",
            "", true),

        rule!("host:perm", "host", Host, "critical",
            "Quyền tệp hệ thống",
            "/etc/shadow đọc được bởi mọi người dùng là NGHIÊM TRỌNG (băm mật khẩu lộ). Tệp trong /etc hoặc /usr/local/bin ai cũng ghi được là đường leo thang đặc quyền trực tiếp.",
            "", true),

        rule!("host:firewall", "host", Host, "medium",
            "Trạng thái tường lửa",
            "Chỉ báo khi CÓ dữ liệu iptables mà thấy trống — không có dữ liệu = không kiểm được, khác hẳn không có tường lửa.",
            "", true),

        rule!("host:sudo", "host", Host, "medium",
            "Sudo NOPASSWD",
            "Tài khoản bị chiếm sẽ lên root mà không cần mật khẩu nào. Đôi khi là chủ ý (tự động hoá) — nếu vậy phải giới hạn đúng lệnh cần thiết.",
            "", true),

        rule!("host:updates", "host", Host, "medium",
            "Bản vá tồn đọng",
            "Không phải bản vá nào cũng liên quan bảo mật, nhưng tồn đọng lớn (>50) thường đi kèm CVE đã có bản vá. Nguồn gói OS sau đó được đối chiếu OSV/KEV/EPSS luôn — đây là chỗ CVE trả lãi lớn nhất.",
            "", true),
    ]
}

pub fn to_json() -> Value {
    let rules: Vec<Value> = catalogue()
        .iter()
        .map(|r| {
            json!({
                "id": r.id,
                "category": r.category,
                "layer": r.layer.as_str(),
                "layer_label": r.layer.label(),
                "max_severity": r.max_severity,
                "title": r.title,
                "rationale": r.rationale,
                "wstg": r.wstg,
                "implemented": r.implemented,
            })
        })
        .collect();
    let implemented = rules.iter().filter(|r| r["implemented"] == true).count();
    json!({
        "ok": true,
        "total": rules.len(),
        "implemented": implemented,
        "rules": rules,
        // Nói thẳng ranh giới, ngay trong dữ liệu chứ không chỉ trong tài liệu.
        "not_covered": [
            "Phân quyền sai theo vai trò/đối tượng (OWASP A01) — cần hiểu nghiệp vụ",
            "Thiết kế không an toàn (OWASP A06) — cần đọc kiến trúc",
            "Thiếu ghi log & giám sát (OWASP A09) — không quan sát được từ ngoài",
            "Lỗi logic nghiệp vụ — không công cụ tự động nào tìm được",
            "Khai thác lỗ hổng, brute-force, DoS — ranh giới thiết kế, sẽ không bao giờ làm"
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Finding;
    use crate::dns::{self, DnsFacts};
    use crate::probe::{self, Resp};

    fn ids() -> Vec<&'static str> {
        catalogue().iter().map(|r| r.id).collect()
    }

    /// Mọi fingerprint mà đầu dò sinh ra phải có mục tương ứng trong danh mục.
    /// Thêm phép kiểm mà quên khai ở đây thì test này đỏ.
    #[test]
    fn every_emitted_fingerprint_is_declared_in_the_catalogue() {
        let mut emitted: Vec<Finding> = vec![];

        // Phản hồi trần: kích hoạt hết nhóm 'thiếu ...'
        emitted.extend(probe::analyze(&Resp {
            url: "https://x.vn/".into(), status: 200, headers: vec![],
            body_snippet: String::new(), https: true,
        }));

        // Phản hồi có đủ header xấu: kích hoạt nhóm 'giá trị sai'
        let bad: Vec<(String, String)> = [
            ("strict-transport-security", "max-age=0"),
            ("content-security-policy", "script-src 'unsafe-inline' 'unsafe-eval' * https: data: 'strict-dynamic'; style-src 'unsafe-inline'"),
            ("x-frame-options", "ALLOW-FROM https://a.vn"),
            ("x-content-type-options", "sniff"),
            ("referrer-policy", "unsafe-url"),
            ("permissions-policy", "interest-cohort=(), camera=*"),
            ("x-xss-protection", "1; mode=block"),
            ("server", "nginx/1.18.0"),
            ("sourcemap", "/app.js.map"),
            ("set-cookie", "PHPSESSID=a; SameSite=None"),
            ("set-cookie", "__Host-sid=1; Domain=a.vn"),
            ("set-cookie", "__Secure-x=1"),
        ].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        emitted.extend(probe::analyze(&Resp {
            url: "https://x.vn/".into(), status: 200, headers: bad,
            body_snippet: String::new(), https: true,
        }));

        // DNS: trống, xấu, và hỏng truy vấn
        emitted.extend(dns::analyze("a.vn", &DnsFacts { mx: vec!["m.a.vn.".into()], ..Default::default() }));
        emitted.extend(dns::analyze("a.vn", &DnsFacts {
            txt_apex: vec!["v=spf1 +all".into()],
            txt_dmarc: vec!["v=DMARC1; p=none".into()],
            ..Default::default()
        }));
        emitted.extend(dns::analyze("a.vn", &DnsFacts {
            txt_apex_ok: false, txt_dmarc_ok: false, caa_ok: false, ds_ok: false,
            ..Default::default()
        }));

        let declared = ids();
        let mut missing: Vec<String> = vec![];
        for f in &emitted {
            if !declared.iter().any(|d| f.fingerprint.starts_with(d)) {
                missing.push(f.fingerprint.clone());
            }
        }
        missing.sort();
        missing.dedup();
        assert!(
            missing.is_empty(),
            "fingerprint chưa khai trong rules::catalogue(): {missing:?}"
        );
        assert!(emitted.len() > 25, "chỉ sinh {} phát hiện — test chưa phủ đủ", emitted.len());
    }

    #[test]
    fn every_layer_now_has_implementations() {
        // App đã tới điểm cả ba lớp đều có bộ chạy (`probe/dns/tls` cho L1,
        // `active` cho L2, `host` cho L3). Bất biến giờ là: MỖI lớp phải có ít
        // nhất một luật đã cài — nếu không, lớp đó bị treo mà không ai biết.
        let c = catalogue();
        for layer in [Layer::Passive, Layer::Active, Layer::Host] {
            let any = c.iter().any(|r| r.layer == layer && r.implemented);
            assert!(any, "lớp {} không có luật nào đã cài", layer.as_str());
        }
    }

    #[test]
    fn every_rule_explains_why_not_just_what() {
        for r in catalogue() {
            assert!(!r.title.is_empty(), "{} thiếu tiêu đề", r.id);
            // Lý do phải đủ dài để thật sự giải thích, không phải nhắc lại tiêu đề.
            assert!(r.rationale.chars().count() > 60, "{} có lý do quá sơ sài", r.id);
            assert!(
                ["critical", "high", "medium", "low", "info"].contains(&r.max_severity),
                "{} có mức lạ: {}", r.id, r.max_severity
            );
        }
    }

    #[test]
    fn json_declares_what_is_not_covered() {
        let v = to_json();
        let nc = v["not_covered"].as_array().unwrap();
        assert!(nc.len() >= 4);
        let joined = nc.iter().map(|x| x.as_str().unwrap()).collect::<Vec<_>>().join(" ");
        // Ba loại OWASP mà scanner không thấy được phải được nêu tên
        assert!(joined.contains("A01") && joined.contains("A06") && joined.contains("A09"));
        // implemented có thể bằng total khi cả ba lớp đã đủ luật; cái đáng khoá
        // là có mục 'ranh giới thiết kế, sẽ không bao giờ làm' trong not_covered.
        assert!(joined.contains("sẽ không bao giờ"));
    }
}
