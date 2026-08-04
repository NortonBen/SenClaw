//! Đầu dò L1 thụ động cho HTTP: security header, cờ cookie, lộ thông tin.
//! Một GET, không payload nào.
//!
//! Mức độ ở đây **không phải tự đặt** — chúng theo hành vi trình duyệt thật
//! năm 2026. Đặt sai mức là cách nhanh nhất để scanner mất uy tín.

use crate::db::Finding;
use serde_json::json;

/// Đầu vào đã chuẩn hoá: tên header viết thường, giá trị giữ nguyên.
#[derive(Debug)]
pub struct Resp {
    pub url: String,
    pub status: u16,
    /// Nhiều header cùng tên (Set-Cookie, và XFO khi cấu hình sai) đều giữ lại.
    pub headers: Vec<(String, String)>,
    pub body_snippet: String,
    pub https: bool,
}

impl Resp {
    pub fn get(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
    pub fn all(&self, name: &str) -> Vec<&str> {
        self.headers
            .iter()
            .filter(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
            .collect()
    }
}

/// Toàn bộ phép kiểm header + cookie cho một phản hồi.
pub fn analyze(r: &Resp) -> Vec<Finding> {
    let mut out = vec![];
    hsts(r, &mut out);
    csp(r, &mut out);
    framing(r, &mut out);
    nosniff(r, &mut out);
    referrer(r, &mut out);
    permissions(r, &mut out);
    legacy_xss(r, &mut out);
    disclosure(r, &mut out);
    cookies(r, &mut out);
    out
}

fn hsts(r: &Resp, out: &mut Vec<Finding>) {
    if !r.https {
        return; // trên HTTP thuần trình duyệt bỏ qua HSTS theo thiết kế
    }
    let Some(v) = r.get("strict-transport-security") else {
        out.push(
            Finding::new("headers", "medium", "hdr:hsts:missing", "Thiếu Strict-Transport-Security")
                .detail("Trình duyệt có thể bị hạ xuống HTTP ở lần truy cập đầu.")
                .fix("Thêm: Strict-Transport-Security: max-age=63072000; includeSubDomains")
                .wstg("WSTG-CONF-07"),
        );
        return;
    };
    let max_age = v
        .to_ascii_lowercase()
        .split(';')
        .find_map(|p| p.trim().strip_prefix("max-age=").map(|s| s.trim().to_string()))
        .and_then(|s| s.parse::<i64>().ok());
    match max_age {
        // max-age=0 không phải "thiếu" — nó chủ động XOÁ HSTS đã ghim.
        Some(0) => out.push(
            Finding::new("headers", "high", "hdr:hsts:zero", "HSTS bị vô hiệu (max-age=0)")
                .detail("max-age=0 xoá HSTS đã ghim trong trình duyệt, không phải chỉ là thiếu.")
                .evidence(json!({ "value": v }))
                .fix("Đặt max-age=63072000 (2 năm).")
                .wstg("WSTG-CONF-07"),
        ),
        Some(n) if n < 15_768_000 => out.push(
            Finding::new("headers", "medium", "hdr:hsts:short", "HSTS max-age dưới 6 tháng")
                .detail(format!("max-age={n}s; mốc tối thiểu thường dùng là 15768000 (6 tháng)."))
                .evidence(json!({ "max_age": n }))
                .fix("Nâng lên 63072000 và thêm includeSubDomains.")
                .wstg("WSTG-CONF-07"),
        ),
        None => out.push(
            Finding::new("headers", "medium", "hdr:hsts:invalid", "HSTS không đọc được max-age")
                .evidence(json!({ "value": v }))
                .wstg("WSTG-CONF-07"),
        ),
        _ => {}
    }
}

fn csp(r: &Resp, out: &mut Vec<Finding>) {
    let Some(v) = r.get("content-security-policy") else {
        // 78% web không có CSP — gọi đây là "critical" là phóng đại.
        out.push(
            Finding::new("headers", "medium", "hdr:csp:missing", "Thiếu Content-Security-Policy")
                .fix("Bắt đầu với: script-src 'nonce-{NGẪU_NHIÊN}' 'strict-dynamic'; object-src 'none'; base-uri 'none'")
                .wstg("WSTG-CONF-12"),
        );
        return;
    };
    let lower = v.to_ascii_lowercase();
    let script_src = directive(&lower, "script-src").or_else(|| directive(&lower, "default-src"));
    let has_nonce_or_hash = script_src
        .as_deref()
        .map(|s| s.contains("'nonce-") || s.contains("'sha256-") || s.contains("'sha384-") || s.contains("'sha512-"))
        .unwrap_or(false);

    if let Some(ss) = &script_src {
        // QUY TẮC CHỐNG DƯƠNG-TÍNH-GIẢ: có nonce/hash thì trình duyệt BỎ QUA
        // 'unsafe-inline'. Báo lỗi ở đây là sai.
        if ss.contains("'unsafe-inline'") && !has_nonce_or_hash {
            out.push(
                Finding::new("headers", "high", "hdr:csp:unsafe-inline", "CSP cho phép 'unsafe-inline' trong script-src")
                    .detail("Không có nonce/hash nào để trình duyệt bỏ qua nó, nên script nội tuyến chạy được.")
                    .evidence(json!({ "script_src": ss }))
                    .fix("Dùng nonce hoặc hash thay cho 'unsafe-inline'.")
                    .wstg("WSTG-CONF-12"),
            );
        }
        // 'strict-dynamic' thiếu nonce/hash = chính sách HỎNG (chặn hết script),
        // không phải chính sách chặt.
        if ss.contains("'strict-dynamic'") && !has_nonce_or_hash {
            out.push(
                Finding::new("headers", "high", "hdr:csp:strict-dynamic-orphan", "CSP có 'strict-dynamic' nhưng không có nonce/hash")
                    .detail("Chính sách này chặn mọi script — là cấu hình hỏng chứ không phải cấu hình chặt.")
                    .evidence(json!({ "script_src": ss }))
                    .fix("Thêm 'nonce-{NGẪU_NHIÊN}' vào script-src.")
                    .wstg("WSTG-CONF-12"),
            );
        }
        if ss.contains("'unsafe-eval'") {
            out.push(
                Finding::new("headers", "medium", "hdr:csp:unsafe-eval", "CSP cho phép 'unsafe-eval'")
                    .evidence(json!({ "script_src": ss }))
                    .wstg("WSTG-CONF-12"),
            );
        }
        for broad in ["*", "http:", "https:", "data:"] {
            if ss.split_whitespace().any(|t| t == broad) {
                out.push(
                    Finding::new("headers", "high", format!("hdr:csp:broad:{broad}"), format!("CSP script-src cho phép nguồn quá rộng '{broad}'"))
                        .evidence(json!({ "script_src": ss }))
                        .wstg("WSTG-CONF-12"),
                );
            }
        }
    }
    if directive(&lower, "base-uri").is_none() {
        out.push(
            Finding::new("headers", "high", "hdr:csp:no-base-uri", "CSP thiếu base-uri")
                .detail("Chèn thẻ <base> có thể đổi hướng mọi script dùng đường dẫn tương đối.")
                .fix("Thêm: base-uri 'none'")
                .wstg("WSTG-CONF-12"),
        );
    }
    if directive(&lower, "object-src").is_none() && directive(&lower, "default-src").is_none() {
        out.push(
            Finding::new("headers", "high", "hdr:csp:no-object-src", "CSP thiếu cả object-src lẫn default-src")
                .fix("Thêm: object-src 'none'")
                .wstg("WSTG-CONF-12"),
        );
    }
    // 'unsafe-inline' chỉ ở style-src là mức thấp
    if let Some(st) = directive(&lower, "style-src") {
        if st.contains("'unsafe-inline'") {
            out.push(
                Finding::new("headers", "low", "hdr:csp:style-unsafe-inline", "CSP cho phép 'unsafe-inline' ở style-src")
                    .detail("Tác động thấp hơn hẳn so với script-src.")
                    .wstg("WSTG-CONF-12"),
            );
        }
    }
}

/// Kiểm **có chống đóng khung không**, không kiểm riêng X-Frame-Options.
/// Nếu CSP có frame-ancestors thì trình duyệt BỎ QUA XFO hoàn toàn.
fn framing(r: &Resp, out: &mut Vec<Finding>) {
    let csp_lower = r
        .get("content-security-policy")
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    if directive(&csp_lower, "frame-ancestors").is_some() {
        return; // CSP đã lo, XFO thành thừa (vô hại)
    }
    let xfo = r.all("x-frame-options");
    if xfo.is_empty() {
        out.push(
            Finding::new("headers", "medium", "hdr:frame:none", "Không có chống đóng khung (clickjacking)")
                .detail("Không có CSP frame-ancestors, cũng không có X-Frame-Options.")
                .fix("Thêm CSP: frame-ancestors 'none' (và X-Frame-Options: DENY cho trình duyệt cũ).")
                .wstg("WSTG-CLNT-09"),
        );
        return;
    }
    let valid = |v: &str| {
        let u = v.trim().to_ascii_uppercase();
        u == "DENY" || u == "SAMEORIGIN"
    };
    // Bẫy hiếm ai kiểm: nhiều header XFO mà TẤT CẢ đều không hợp lệ thì theo
    // thuật toán của WHATWG nó bị coi như KHÔNG CÓ — hỏng theo hướng mở.
    if xfo.len() > 1 && !xfo.iter().any(|v| valid(v)) {
        out.push(
            Finding::new("headers", "medium", "hdr:frame:multi-invalid", "Nhiều X-Frame-Options đều không hợp lệ — mất tác dụng")
                .detail("Theo thuật toán WHATWG, trường hợp này bị xử lý như không có header: hỏng theo hướng mở.")
                .evidence(json!({ "values": xfo }))
                .wstg("WSTG-CLNT-09"),
        );
        return;
    }
    if !xfo.iter().any(|v| valid(v)) {
        out.push(
            Finding::new("headers", "low", "hdr:frame:invalid", "X-Frame-Options có giá trị không hợp lệ")
                .detail("ALLOW-FROM đã bị khai tử và không trình duyệt nào cài.")
                .evidence(json!({ "values": xfo }))
                .fix("Dùng CSP frame-ancestors, hoặc XFO: DENY / SAMEORIGIN.")
                .wstg("WSTG-CLNT-09"),
        );
    }
}

fn nosniff(r: &Resp, out: &mut Vec<Finding>) {
    match r.get("x-content-type-options") {
        Some(v) if v.trim().eq_ignore_ascii_case("nosniff") => {}
        Some(v) => out.push(
            Finding::new("headers", "low", "hdr:xcto:invalid", "X-Content-Type-Options sai giá trị")
                .evidence(json!({ "value": v }))
                .fix("Giá trị hợp lệ duy nhất là: nosniff"),
        ),
        None => out.push(
            Finding::new("headers", "low", "hdr:xcto:missing", "Thiếu X-Content-Type-Options")
                .fix("Thêm: X-Content-Type-Options: nosniff"),
        ),
    }
}

/// Thiếu Referrer-Policy chỉ là INFO: **mặc định của trình duyệt đã an toàn**
/// (`strict-origin-when-cross-origin`). Phát hiện thật là khi ai đó chủ động
/// đặt một giá trị TỆ HƠN mặc định.
fn referrer(r: &Resp, out: &mut Vec<Finding>) {
    let Some(v) = r.get("referrer-policy") else {
        out.push(
            Finding::new("headers", "info", "hdr:referrer:absent", "Không đặt Referrer-Policy")
                .detail("Mặc định của trình duyệt là strict-origin-when-cross-origin — đã an toàn. Đặt tường minh chỉ để phòng cấu hình khác thường."),
        );
        return;
    };
    let low = v.to_ascii_lowercase();
    let unsafe_vals = ["unsafe-url", "origin", "origin-when-cross-origin", "no-referrer-when-downgrade"];
    if let Some(bad) = unsafe_vals.iter().find(|b| low.contains(*b)) {
        let sev = if *bad == "unsafe-url" { "medium" } else { "low" };
        out.push(
            Finding::new("headers", sev, format!("hdr:referrer:unsafe:{bad}"), format!("Referrer-Policy '{bad}' rò rỉ nhiều hơn mặc định"))
                .evidence(json!({ "value": v }))
                .fix("Dùng strict-origin-when-cross-origin hoặc chặt hơn.")
                .wstg("WSTG-CONF-07"),
        );
    }
}

/// Firefox và Safari **không hỗ trợ** Permissions-Policy ở bất kỳ phiên bản
/// nào. Chấm nặng một header chỉ chạy trên Chromium là không trung thực.
fn permissions(r: &Resp, out: &mut Vec<Finding>) {
    let Some(v) = r.get("permissions-policy") else {
        out.push(Finding::new("headers", "info", "hdr:permpolicy:absent", "Không đặt Permissions-Policy")
            .detail("Chỉ Chromium hỗ trợ; Firefox và Safari bỏ qua. Có thì tốt, không có không phải lỗi."));
        return;
    };
    let low = v.to_ascii_lowercase();
    // interest-cohort chết cùng FLoC; nó là giá trị Permissions-Policy phổ biến
    // nhất trên web mà lại hoàn toàn vô nghĩa.
    if low.contains("interest-cohort") {
        out.push(
            Finding::new("headers", "info", "hdr:permpolicy:interest-cohort", "Permissions-Policy còn 'interest-cohort' (đã vô nghĩa)")
                .detail("FLoC đã bị thay bằng Topics API; directive này không còn tác dụng gì. Gỡ đi cho gọn."),
        );
    }
    for risky in ["camera=*", "microphone=*", "geolocation=*"] {
        if low.replace(' ', "").contains(risky) {
            out.push(
                Finding::new("headers", "low", format!("hdr:permpolicy:wide:{risky}"), format!("Permissions-Policy mở rộng '{risky}'"))
                    .evidence(json!({ "value": v })),
            );
        }
    }
}

fn legacy_xss(r: &Resp, out: &mut Vec<Finding>) {
    if let Some(v) = r.get("x-xss-protection") {
        if !v.trim().starts_with('0') {
            out.push(
                Finding::new("headers", "low", "hdr:xxp:enabled", "X-XSS-Protection đang bật — nên gỡ")
                    .detail("Bộ lọc này đã bị khai tử; MDN cảnh báo nó có thể TẠO RA lỗ XSS trên site vốn an toàn.")
                    .evidence(json!({ "value": v }))
                    .fix("Đặt 'X-XSS-Protection: 0' hoặc gỡ hẳn header."),
            );
        }
    }
}

fn disclosure(r: &Resp, out: &mut Vec<Finding>) {
    let has_digit = |s: &str| s.chars().any(|c| c.is_ascii_digit());
    for h in ["server", "x-powered-by", "x-aspnet-version", "x-aspnetmvc-version", "x-generator"] {
        if let Some(v) = r.get(h) {
            if v.trim().is_empty() {
                continue;
            }
            let versioned = has_digit(v);
            out.push(
                Finding::new(
                    "exposure",
                    if versioned { "low" } else { "info" },
                    format!("exp:banner:{h}"),
                    format!("Header '{h}' lộ thông tin hệ thống"),
                )
                .detail(if versioned {
                    "Có chứa số phiên bản — ghép được với CSDL CVE để tìm lỗ hổng cụ thể."
                } else {
                    "Không có số phiên bản, nhưng vẫn lộ công nghệ đang dùng."
                })
                .evidence(json!({ "header": h, "value": v }))
                .fix(format!("Gỡ hoặc làm mờ header '{h}'."))
                .wstg("WSTG-INFO-02"),
            );
        }
    }
    // Lộ đường dẫn/host nội bộ trong header source-map là mức cao hơn.
    for h in ["sourcemap", "x-sourcemap", "x-sourcefiles"] {
        if r.get(h).is_some() {
            out.push(
                Finding::new("exposure", "medium", format!("exp:sourcemap:{h}"), "Lộ source map")
                    .detail("Source map cho phép dựng lại mã nguồn gốc.")
                    .fix(format!("Gỡ header '{h}' trên môi trường production."))
                    .wstg("WSTG-INFO-02"),
            );
        }
    }
}

/// Một cookie đã tách thuộc tính.
struct Cookie<'a> {
    name: &'a str,
    secure: bool,
    http_only: bool,
    same_site: Option<String>,
    domain: Option<&'a str>,
}

fn parse_cookie(raw: &str) -> Cookie<'_> {
    let mut parts = raw.split(';');
    let first = parts.next().unwrap_or("");
    let name = first.split('=').next().unwrap_or("").trim();
    let mut c = Cookie {
        name,
        secure: false,
        http_only: false,
        same_site: None,
        domain: None,
    };
    for p in parts {
        let p = p.trim();
        let low = p.to_ascii_lowercase();
        if low == "secure" {
            c.secure = true;
        } else if low == "httponly" {
            c.http_only = true;
        } else if let Some(v) = low.strip_prefix("samesite=") {
            c.same_site = Some(v.trim().to_string());
        } else if low.starts_with("domain=") {
            c.domain = Some(p[7..].trim());
        }
    }
    c
}

/// Đoán cookie phiên theo tên. Cố ý rộng hơn heuristic của Observatory
/// (họ chỉ khớp "login"/"sess").
fn looks_like_session(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    ["sess", "login", "auth", "token", "sid", "jwt"]
        .iter()
        .any(|k| n.contains(k))
        || matches!(
            n.as_str(),
            "phpsessid" | "jsessionid" | "asp.net_sessionid" | "connect.sid" | "cfid" | "cftoken"
        )
}

fn cookies(r: &Resp, out: &mut Vec<Finding>) {
    for raw in r.all("set-cookie") {
        let c = parse_cookie(raw);
        if c.name.is_empty() {
            continue;
        }
        let sess = looks_like_session(c.name);

        if !c.secure && r.https {
            let sev = if sess { "high" } else { "medium" };
            out.push(
                Finding::new("cookies", sev, format!("cookie:secure:{}", c.name), format!("Cookie '{}' thiếu cờ Secure", c.name))
                    .detail(if sess { "Đây là cookie phiên — gửi được qua HTTP thuần là rò danh tính." } else { "Cookie có thể bị gửi qua kết nối không mã hoá." })
                    .evidence(json!({ "cookie": c.name, "session_like": sess }))
                    .fix("Thêm thuộc tính Secure.")
                    .wstg("WSTG-SESS-02"),
            );
        }
        if !c.http_only && sess {
            out.push(
                Finding::new("cookies", "medium", format!("cookie:httponly:{}", c.name), format!("Cookie phiên '{}' thiếu HttpOnly", c.name))
                    .detail("JavaScript đọc được cookie phiên — một lỗ XSS là đủ để chiếm phiên.")
                    .fix("Thêm thuộc tính HttpOnly.")
                    .wstg("WSTG-SESS-02"),
            );
        }
        match c.same_site.as_deref() {
            Some("none") if !c.secure => out.push(
                Finding::new("cookies", "high", format!("cookie:samesite-none-insecure:{}", c.name), format!("Cookie '{}' có SameSite=None nhưng thiếu Secure", c.name))
                    .detail("Tổ hợp này không hợp lệ — Chrome/Edge/Firefox loại bỏ cookie luôn.")
                    .fix("Thêm Secure, hoặc đổi sang SameSite=Lax.")
                    .wstg("WSTG-SESS-02"),
            ),
            None => out.push(
                // "Trình duyệt hiện đại mặc định Lax rồi" là SAI: chỉ Chrome/Edge.
                // Firefox coi như None (bug 1617609 đóng WONTFIX), Safari không hỗ trợ.
                Finding::new("cookies", if sess { "medium" } else { "low" }, format!("cookie:samesite-absent:{}", c.name), format!("Cookie '{}' không đặt SameSite", c.name))
                    .detail("Chỉ Chrome/Edge mặc định Lax. Firefox coi như None, Safari không hỗ trợ — nên đây là thiếu sót thật, không phải hình thức.")
                    .fix("Đặt SameSite=Lax (hoặc None; Secure nếu thật sự cần dùng chéo site).")
                    .wstg("WSTG-SESS-02"),
            ),
            _ => {}
        }
        // Tiền tố __Host-/__Secure- là bảo đảm do TRÌNH DUYỆT cưỡng chế; vi phạm
        // ràng buộc thì cookie bị từ chối âm thầm — vừa lỗi bảo mật vừa lỗi chức năng.
        if let Some(rest) = c.name.strip_prefix("__Host-") {
            let _ = rest;
            if !c.secure || c.domain.is_some() {
                out.push(
                    Finding::new("cookies", "medium", format!("cookie:host-prefix:{}", c.name), format!("Cookie '{}' vi phạm ràng buộc tiền tố __Host-", c.name))
                        .detail("__Host- đòi Secure, Path=/ và KHÔNG có Domain. Sai là trình duyệt từ chối âm thầm.")
                        .evidence(json!({ "secure": c.secure, "has_domain": c.domain.is_some() })),
                );
            }
        } else if c.name.starts_with("__Secure-") && !c.secure {
            out.push(
                Finding::new("cookies", "medium", format!("cookie:secure-prefix:{}", c.name), format!("Cookie '{}' vi phạm ràng buộc tiền tố __Secure-", c.name))
                    .detail("__Secure- bắt buộc phải có cờ Secure."),
            );
        }
    }
}

/// Rút giá trị một directive CSP. `haystack` phải là chuỗi đã viết thường.
fn directive(csp_lower: &str, name: &str) -> Option<String> {
    csp_lower
        .split(';')
        .map(|s| s.trim())
        .find(|s| s == &name || s.starts_with(&format!("{name} ")))
        .map(|s| s.trim_start_matches(name).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resp(headers: &[(&str, &str)]) -> Resp {
        Resp {
            url: "https://x.vn/".into(),
            status: 200,
            headers: headers
                .iter()
                .map(|(k, v)| (k.to_ascii_lowercase(), v.to_string()))
                .collect(),
            body_snippet: String::new(),
            https: true,
        }
    }
    fn ids(f: &[Finding]) -> Vec<&str> {
        f.iter().map(|x| x.fingerprint.as_str()).collect()
    }

    #[test]
    fn csp_with_nonce_must_not_flag_unsafe_inline() {
        // Đây là quy tắc chống dương-tính-giả quan trọng nhất: có nonce thì
        // trình duyệt BỎ QUA 'unsafe-inline', nên báo lỗi là sai.
        let r = resp(&[(
            "content-security-policy",
            "script-src 'nonce-abc123' 'unsafe-inline'; object-src 'none'; base-uri 'none'",
        )]);
        let f = analyze(&r);
        assert!(!ids(&f).contains(&"hdr:csp:unsafe-inline"), "không được báo khi đã có nonce");
    }

    #[test]
    fn csp_unsafe_inline_without_nonce_is_flagged() {
        let r = resp(&[(
            "content-security-policy",
            "script-src 'self' 'unsafe-inline'; object-src 'none'; base-uri 'none'",
        )]);
        assert!(ids(&analyze(&r)).contains(&"hdr:csp:unsafe-inline"));
    }

    #[test]
    fn strict_dynamic_without_nonce_is_a_broken_policy() {
        let r = resp(&[(
            "content-security-policy",
            "script-src 'strict-dynamic'; object-src 'none'; base-uri 'none'",
        )]);
        assert!(ids(&analyze(&r)).contains(&"hdr:csp:strict-dynamic-orphan"));
    }

    #[test]
    fn frame_ancestors_satisfies_framing_so_xfo_is_not_required() {
        let r = resp(&[(
            "content-security-policy",
            "frame-ancestors 'none'; base-uri 'none'; object-src 'none'",
        )]);
        let f = analyze(&r);
        assert!(!ids(&f).contains(&"hdr:frame:none"), "CSP đã lo thì không đòi XFO");
    }

    #[test]
    fn multiple_invalid_xfo_headers_fail_open() {
        let mut r = resp(&[("x-frame-options", "ALLOW-FROM https://a.vn")]);
        r.headers.push(("x-frame-options".into(), "GARBAGE".into()));
        assert!(ids(&analyze(&r)).contains(&"hdr:frame:multi-invalid"));
    }

    #[test]
    fn hsts_zero_is_high_not_merely_missing() {
        let r = resp(&[("strict-transport-security", "max-age=0")]);
        let f = analyze(&r);
        let h = f.iter().find(|x| x.fingerprint == "hdr:hsts:zero").unwrap();
        assert_eq!(h.severity, "high");
    }

    #[test]
    fn referrer_policy_absent_is_info_but_unsafe_value_is_a_finding() {
        let f = analyze(&resp(&[]));
        let rp = f.iter().find(|x| x.fingerprint == "hdr:referrer:absent").unwrap();
        assert_eq!(rp.severity, "info", "mặc định trình duyệt đã an toàn");

        let f = analyze(&resp(&[("referrer-policy", "unsafe-url")]));
        assert!(f.iter().any(|x| x.fingerprint == "hdr:referrer:unsafe:unsafe-url"
            && x.severity == "medium"));
    }

    #[test]
    fn permissions_policy_absent_is_only_info() {
        let f = analyze(&resp(&[]));
        let p = f.iter().find(|x| x.fingerprint == "hdr:permpolicy:absent").unwrap();
        assert_eq!(p.severity, "info", "Firefox/Safari không hỗ trợ header này");
    }

    #[test]
    fn dead_interest_cohort_is_reported_as_noise() {
        let f = analyze(&resp(&[("permissions-policy", "interest-cohort=()")]));
        assert!(ids(&f).contains(&"hdr:permpolicy:interest-cohort"));
    }

    #[test]
    fn session_cookie_without_secure_outranks_a_plain_one() {
        let mut r = resp(&[("set-cookie", "PHPSESSID=abc; Path=/")]);
        r.headers.push(("set-cookie".into(), "theme=dark; Path=/; Secure; SameSite=Lax".into()));
        let f = analyze(&r);
        let sess = f.iter().find(|x| x.fingerprint == "cookie:secure:PHPSESSID").unwrap();
        assert_eq!(sess.severity, "high");
        // cookie thường đã đủ cờ thì không bị báo
        assert!(!ids(&f).contains(&"cookie:secure:theme"));
    }

    #[test]
    fn missing_samesite_is_a_real_finding() {
        // Không được coi nhẹ vì "trình duyệt mặc định Lax" — Firefox thì không.
        let r = resp(&[("set-cookie", "prefs=1; Path=/; Secure")]);
        assert!(ids(&analyze(&r)).contains(&"cookie:samesite-absent:prefs"));
    }

    #[test]
    fn samesite_none_without_secure_is_high() {
        let r = resp(&[("set-cookie", "x=1; SameSite=None")]);
        let f = analyze(&r);
        let hit = f.iter().find(|x| x.fingerprint == "cookie:samesite-none-insecure:x").unwrap();
        assert_eq!(hit.severity, "high");
    }

    #[test]
    fn host_prefix_violation_is_caught() {
        let r = resp(&[("set-cookie", "__Host-sid=1; Path=/; Domain=a.vn; Secure")]);
        assert!(ids(&analyze(&r)).contains(&"cookie:host-prefix:__Host-sid"));
    }

    #[test]
    fn versioned_server_banner_ranks_above_bare_one() {
        let a = analyze(&resp(&[("server", "nginx/1.18.0")]));
        assert_eq!(a.iter().find(|x| x.fingerprint == "exp:banner:server").unwrap().severity, "low");
        let b = analyze(&resp(&[("server", "nginx")]));
        assert_eq!(b.iter().find(|x| x.fingerprint == "exp:banner:server").unwrap().severity, "info");
    }

    #[test]
    fn directive_extraction() {
        let csp = "default-src 'self'; script-src 'nonce-x' 'strict-dynamic'; object-src 'none'";
        assert_eq!(directive(csp, "script-src").unwrap(), "'nonce-x' 'strict-dynamic'");
        assert_eq!(directive(csp, "object-src").unwrap(), "'none'");
        assert!(directive(csp, "base-uri").is_none());
        // không được khớp nhầm tiền tố: 'script-src-elem' khác 'script-src'
        assert!(directive("script-src-elem 'self'", "script-src").is_none());
    }
}
