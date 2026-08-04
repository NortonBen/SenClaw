//! Đầu dò L1 cho DNS: SPF / DMARC / CAA / DNSSEC. Toàn bộ là truy vấn DNS
//! thuần — không gửi một gói tin nào tới web server của mục tiêu.
//!
//! Phần phân tích tách khỏi phần truy vấn để test được mà không cần mạng.

use crate::db::Finding;
use anyhow::{anyhow, Result};
use serde_json::json;

/// Bản ghi thô đã đọc về, để hàm phân tích thuần xử lý.
///
/// `*_ok` phân biệt **"truy vấn chạy được và không có bản ghi"** với
/// **"truy vấn hỏng"**. Thiếu phân biệt này là scanner sẽ báo "không có SPF"
/// trong khi thật ra nó chưa hỏi được — đúng loại dương tính giả nguy hiểm
/// nhất, vì nó tạo việc cho người vận hành mà chẳng có vấn đề nào cả.
#[derive(Debug)]
pub struct DnsFacts {
    pub txt_apex: Vec<String>,
    pub txt_apex_ok: bool,
    pub txt_dmarc: Vec<String>,
    pub txt_dmarc_ok: bool,
    pub caa: Vec<String>,
    pub caa_ok: bool,
    pub ds: Vec<String>,
    pub ds_ok: bool,
    pub mx: Vec<String>,
}

impl Default for DnsFacts {
    fn default() -> Self {
        // Mặc định coi như hỏi được: các test dựng DnsFacts bằng tay đang mô tả
        // câu trả lời thật, không phải một lần hỏi thất bại.
        Self {
            txt_apex: vec![],
            txt_apex_ok: true,
            txt_dmarc: vec![],
            txt_dmarc_ok: true,
            caa: vec![],
            caa_ok: true,
            ds: vec![],
            ds_ok: true,
            mx: vec![],
        }
    }
}

pub fn analyze(domain: &str, f: &DnsFacts) -> Vec<Finding> {
    let mut out = vec![];
    for (ok, what) in [
        (f.txt_apex_ok, "TXT ở apex (SPF)"),
        (f.txt_dmarc_ok, "TXT _dmarc (DMARC)"),
        (f.caa_ok, "CAA"),
        (f.ds_ok, "DS (DNSSEC)"),
    ] {
        if !ok {
            out.push(
                Finding::new("dns", "info", format!("dns:query-failed:{what}"), format!("Không truy vấn được {what}"))
                    .detail("Đây là lỗi tra cứu, KHÔNG phải kết luận 'không có bản ghi'."),
            );
        }
    }
    spf(domain, f, &mut out);
    dmarc(domain, f, &mut out);
    if f.caa_ok && f.caa.is_empty() {
        out.push(
            Finding::new("dns", "low", "dns:caa:missing", "Không có bản ghi CAA")
                .detail("CAA giới hạn CA nào được phép cấp chứng thư cho tên miền này.")
                .fix(format!("Thêm: {domain}. IN CAA 0 issue \"letsencrypt.org\""))
                .wstg("WSTG-CONF-11"),
        );
    }
    if f.ds_ok && f.ds.is_empty() {
        out.push(
            Finding::new("dns", "low", "dns:dnssec:missing", "Không bật DNSSEC")
                .detail("Không có bản ghi DS ở zone cha nên câu trả lời DNS không xác thực được — CAA và MTA-STS vì thế cũng dễ bị giả mạo hơn.")
                .fix("Bật DNSSEC tại nhà đăng ký tên miền."),
        );
    }
    // DKIM cố ý KHÔNG báo "thiếu": selector phải biết trước, không enumerate được.
    // Báo "không có DKIM" khi thực ra không kiểm được là dương tính giả.
    if !f.mx.is_empty() {
        out.push(
            Finding::new("dns", "info", "dns:dkim:unknown", "Không kiểm tra được DKIM")
                .detail("DKIM nằm ở <selector>._domainkey và selector không thể liệt kê qua DNS. Cần lấy selector từ header một email thật rồi kiểm tay.")
                .evidence(json!({ "mx": f.mx })),
        );
    }
    out
}

fn spf(domain: &str, f: &DnsFacts, out: &mut Vec<Finding>) {
    if !f.txt_apex_ok {
        return; // chưa hỏi được thì không kết luận gì về SPF
    }
    let records: Vec<&String> = f
        .txt_apex
        .iter()
        .filter(|r| r.trim_start().to_ascii_lowercase().starts_with("v=spf1"))
        .collect();

    if records.is_empty() {
        out.push(
            Finding::new("dns", "medium", "dns:spf:missing", "Không có bản ghi SPF")
                .detail("Bất kỳ ai cũng gửi được email giả danh tên miền này.")
                .fix(format!("Thêm TXT ở apex: {domain}. IN TXT \"v=spf1 ... -all\""))
                .wstg("WSTG-CONF-11"),
        );
        return;
    }
    // Nhiều bản ghi SPF là cấu hình KHÔNG hợp lệ — RFC bắt phải có đúng một.
    if records.len() > 1 {
        out.push(
            Finding::new("dns", "medium", "dns:spf:multiple", "Có nhiều bản ghi SPF — không hợp lệ")
                .detail("RFC 7208 chỉ cho phép đúng một; nhiều bản ghi khiến bên nhận trả permerror và SPF mất tác dụng hoàn toàn.")
                .evidence(json!({ "count": records.len() })),
        );
        return;
    }
    let rec = records[0].to_ascii_lowercase();
    if rec.contains("+all") {
        out.push(
            Finding::new("dns", "high", "dns:spf:plusall", "SPF kết thúc bằng '+all' — cho phép mọi nguồn")
                .detail("Đây là cấu hình tệ hơn cả không có SPF: nó khẳng định mọi máy chủ đều được phép gửi.")
                .evidence(json!({ "record": records[0] }))
                .fix("Đổi '+all' thành '-all'."),
        );
    } else if rec.contains("?all") {
        out.push(
            Finding::new("dns", "low", "dns:spf:neutral", "SPF kết thúc bằng '?all' (trung lập)")
                .detail("Bên nhận không có căn cứ để từ chối thư giả mạo.")
                .evidence(json!({ "record": records[0] }))
                .fix("Đổi sang '~all' rồi '-all' khi đã chắc danh sách nguồn."),
        );
    } else if !rec.contains("-all") && !rec.contains("~all") {
        out.push(
            Finding::new("dns", "low", "dns:spf:noall", "SPF không có cơ chế 'all' kết thúc")
                .evidence(json!({ "record": records[0] })),
        );
    }
    // Giới hạn 10 lần tra cứu DNS của RFC 7208: vượt là permerror, và khi đó
    // SPF coi như không tồn tại — một lỗi im lặng rất hay gặp.
    let lookups = count_spf_lookups(&rec);
    if lookups > 10 {
        out.push(
            Finding::new("dns", "medium", "dns:spf:too-many-lookups", "SPF vượt giới hạn 10 lần tra cứu DNS")
                .detail(format!("Đếm được {lookups} cơ chế cần tra cứu. Vượt 10 thì bên nhận trả permerror và SPF mất tác dụng."))
                .evidence(json!({ "lookups": lookups, "record": records[0] }))
                .fix("Gộp bớt include:, hoặc chuyển sang ip4:/ip6: trực tiếp."),
        );
    }
}

/// Đếm số cơ chế SPF phải tra cứu DNS (include, a, mx, ptr, exists, redirect).
/// `ip4:`/`ip6:`/`all` không tốn lượt tra cứu.
pub fn count_spf_lookups(record_lower: &str) -> usize {
    record_lower
        .split_whitespace()
        .filter(|t| {
            let t = t.trim_start_matches(['+', '-', '~', '?']);
            t.starts_with("include:")
                || t.starts_with("exists:")
                || t.starts_with("redirect=")
                || t == "a"
                || t == "mx"
                || t == "ptr"
                || t.starts_with("a:")
                || t.starts_with("mx:")
                || t.starts_with("ptr:")
        })
        .count()
}

fn dmarc(domain: &str, f: &DnsFacts, out: &mut Vec<Finding>) {
    if !f.txt_dmarc_ok {
        return;
    }
    let rec = f
        .txt_dmarc
        .iter()
        .find(|r| r.trim_start().to_ascii_lowercase().starts_with("v=dmarc1"));
    let Some(rec) = rec else {
        out.push(
            Finding::new("dns", "medium", "dns:dmarc:missing", "Không có bản ghi DMARC")
                .detail("Không có chính sách nào bảo bên nhận phải làm gì với thư giả mạo tên miền này.")
                .fix(format!("Thêm: _dmarc.{domain}. IN TXT \"v=DMARC1; p=none; rua=mailto:...\" rồi siết dần lên quarantine/reject."))
                .wstg("WSTG-CONF-11"),
        );
        return;
    };
    let low = rec.to_ascii_lowercase();
    let policy = low
        .split(';')
        .find_map(|p| p.trim().strip_prefix("p=").map(|s| s.trim().to_string()))
        .unwrap_or_default();
    match policy.as_str() {
        "none" => out.push(
            Finding::new("dns", "medium", "dns:dmarc:none", "DMARC ở chế độ p=none — chỉ giám sát")
                .detail("Thư giả mạo vẫn được gửi vào hộp thư người nhận; DMARC lúc này chỉ để thu báo cáo.")
                .evidence(json!({ "record": rec }))
                .fix("Sau 2 tuần đọc báo cáo rua, chuyển sang p=quarantine rồi p=reject."),
        ),
        "quarantine" | "reject" => {
            if !low.contains("rua=") {
                out.push(
                    Finding::new("dns", "low", "dns:dmarc:no-rua", "DMARC đang cưỡng chế nhưng không có địa chỉ nhận báo cáo")
                        .detail("Không có rua= thì không biết mình đang chặn nhầm thư hợp lệ nào.")
                        .evidence(json!({ "record": rec })),
                );
            }
        }
        _ => out.push(
            Finding::new("dns", "medium", "dns:dmarc:invalid", "Bản ghi DMARC thiếu hoặc sai thẻ p=")
                .evidence(json!({ "record": rec })),
        ),
    }
}

// ---------------------------------------------------------------------------
// Thu thập
// ---------------------------------------------------------------------------

/// Làm tên tuyệt đối (thêm dấu chấm cuối).
///
/// Không có dấu chấm cuối, resolver coi tên là tương đối và **nối search domain
/// của hệ thống vào**: trên máy này `vnexpress.net` biến thành
/// `vnexpress.net.local.` → NXDOMAIN → scanner kết luận "không có SPF" trong khi
/// SPF vẫn nằm đó. Bẫy này chỉ lộ ra khi máy chạy scanner có search domain.
fn fqdn(name: &str) -> String {
    let n = name.trim().trim_end_matches('.');
    format!("{n}.")
}

/// `NoRecordsFound` là câu trả lời hợp lệ ("không có bản ghi loại này"), khác
/// hẳn lỗi mạng/timeout. Chỉ cái sau mới làm phép kiểm trở nên vô hiệu.
fn split_lookup<T>(r: std::result::Result<T, hickory_resolver::error::ResolveError>) -> (Option<T>, bool) {
    use hickory_resolver::error::ResolveErrorKind;
    match r {
        Ok(v) => (Some(v), true),
        Err(e) if matches!(e.kind(), ResolveErrorKind::NoRecordsFound { .. }) => (None, true),
        Err(_) => (None, false),
    }
}

/// Resolver dùng chung.
///
/// **Phải bật EDNS0 và cho phép lùi sang TCP.** Bản ghi TXT ở apex hay vượt
/// 512 byte (SPF + đủ loại token xác minh của Google/GlobalSign/…): đo thật trên
/// `vnexpress.net` là 6 bản ghi / 712 byte. Không có EDNS0 thì phản hồi bị cắt,
/// và không có TCP fallback thì truy vấn treo tới timeout — trong khi `_dmarc`
/// chỉ một bản ghi nhỏ nên vẫn chạy, khiến lỗi trông như "chỉ SPF mới hỏng".
pub fn resolver() -> Result<hickory_resolver::TokioAsyncResolver> {
    let (config, mut opts) = hickory_resolver::system_conf::read_system_conf()
        .map_err(|e| anyhow!("không đọc được cấu hình DNS hệ thống: {e}"))?;
    opts.edns0 = true;
    opts.try_tcp_on_error = true;
    opts.timeout = std::time::Duration::from_secs(5);
    opts.attempts = 2;
    Ok(hickory_resolver::TokioAsyncResolver::tokio(config, opts))
}

pub async fn collect(domain: &str) -> Result<DnsFacts> {
    use hickory_resolver::proto::rr::RecordType;

    let r = resolver()?;
    let mut f = DnsFacts::default();
    let apex = fqdn(domain);

    let (t, ok) = split_lookup(r.txt_lookup(apex.clone()).await);
    f.txt_apex_ok = ok;
    f.txt_apex = t.map(|t| t.iter().map(join_txt).collect()).unwrap_or_default();

    let (t, ok) = split_lookup(r.txt_lookup(fqdn(&format!("_dmarc.{domain}"))).await);
    f.txt_dmarc_ok = ok;
    f.txt_dmarc = t.map(|t| t.iter().map(join_txt).collect()).unwrap_or_default();

    let (l, ok) = split_lookup(r.lookup(apex.clone(), RecordType::CAA).await);
    f.caa_ok = ok;
    f.caa = l.map(|l| l.iter().map(|d| d.to_string()).collect()).unwrap_or_default();

    let (l, ok) = split_lookup(r.lookup(apex.clone(), RecordType::DS).await);
    f.ds_ok = ok;
    f.ds = l.map(|l| l.iter().map(|d| d.to_string()).collect()).unwrap_or_default();

    if let Ok(l) = r.mx_lookup(apex).await {
        f.mx = l.iter().map(|m| m.exchange().to_string()).collect();
    }
    Ok(f)
}

fn join_txt(t: &hickory_resolver::proto::rr::rdata::TXT) -> String {
    t.iter()
        .map(|b| String::from_utf8_lossy(b).to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(f: &[Finding]) -> Vec<&str> {
        f.iter().map(|x| x.fingerprint.as_str()).collect()
    }

    #[test]
    fn missing_spf_and_dmarc_are_both_flagged() {
        let f = analyze("a.vn", &DnsFacts::default());
        assert!(ids(&f).contains(&"dns:spf:missing"));
        assert!(ids(&f).contains(&"dns:dmarc:missing"));
    }

    #[test]
    fn dmarc_p_none_is_medium_not_ok() {
        // Đo thật trên vnexpress.net: SPF có -all nhưng DMARC p=none.
        let facts = DnsFacts {
            txt_apex: vec!["v=spf1 include:_spf.google.com -all".into()],
            txt_dmarc: vec!["v=DMARC1; p=none;".into()],
            ..Default::default()
        };
        let f = analyze("a.vn", &facts);
        let d = f.iter().find(|x| x.fingerprint == "dns:dmarc:none").unwrap();
        assert_eq!(d.severity, "medium");
        // SPF đã có -all thì không được báo
        assert!(!ids(&f).iter().any(|i| i.starts_with("dns:spf:") && *i != "dns:spf:missing"));
    }

    #[test]
    fn plus_all_is_worse_than_neutral() {
        let mk = |spf: &str| DnsFacts {
            txt_apex: vec![spf.into()],
            txt_dmarc: vec!["v=DMARC1; p=reject; rua=mailto:a@a.vn".into()],
            ..Default::default()
        };
        let a = analyze("a.vn", &mk("v=spf1 +all"));
        assert_eq!(a.iter().find(|x| x.fingerprint == "dns:spf:plusall").unwrap().severity, "high");
        let b = analyze("a.vn", &mk("v=spf1 ?all"));
        assert_eq!(b.iter().find(|x| x.fingerprint == "dns:spf:neutral").unwrap().severity, "low");
    }

    #[test]
    fn multiple_spf_records_are_invalid() {
        let facts = DnsFacts {
            txt_apex: vec!["v=spf1 -all".into(), "v=spf1 include:x -all".into()],
            ..Default::default()
        };
        assert!(ids(&analyze("a.vn", &facts)).contains(&"dns:spf:multiple"));
    }

    #[test]
    fn spf_lookup_limit_is_counted_correctly() {
        // ip4: không tốn lượt tra cứu; include:/a/mx thì có.
        let r = "v=spf1 ip4:1.2.3.4 ip4:5.6.7.8 include:a.com include:b.com a mx -all";
        assert_eq!(count_spf_lookups(r), 4);

        let many = format!(
            "v=spf1 {} -all",
            (0..11).map(|i| format!("include:s{i}.com")).collect::<Vec<_>>().join(" ")
        );
        let facts = DnsFacts {
            txt_apex: vec![many],
            txt_dmarc: vec!["v=DMARC1; p=reject; rua=mailto:a@a.vn".into()],
            ..Default::default()
        };
        assert!(ids(&analyze("a.vn", &facts)).contains(&"dns:spf:too-many-lookups"));
    }

    #[test]
    fn dkim_is_reported_as_uncheckable_never_as_missing() {
        let facts = DnsFacts {
            mx: vec!["mail.a.vn.".into()],
            ..Default::default()
        };
        let f = analyze("a.vn", &facts);
        let d = f.iter().find(|x| x.fingerprint == "dns:dkim:unknown").unwrap();
        assert_eq!(d.severity, "info");
        assert!(d.detail.contains("selector"));
        // không được tồn tại phát hiện nào nói DKIM "thiếu"
        assert!(!ids(&f).iter().any(|i| i.contains("dkim:missing")));
    }

    #[test]
    fn enforcing_dmarc_without_rua_is_flagged() {
        let facts = DnsFacts {
            txt_apex: vec!["v=spf1 -all".into()],
            txt_dmarc: vec!["v=DMARC1; p=reject".into()],
            ..Default::default()
        };
        assert!(ids(&analyze("a.vn", &facts)).contains(&"dns:dmarc:no-rua"));
    }

    #[test]
    fn caa_and_dnssec_present_produce_no_finding() {
        let facts = DnsFacts {
            txt_apex: vec!["v=spf1 -all".into()],
            txt_dmarc: vec!["v=DMARC1; p=reject; rua=mailto:a@a.vn".into()],
            caa: vec!["0 issue \"letsencrypt.org\"".into()],
            ds: vec!["12345 13 2 abcd".into()],
            ..Default::default()
        };
        let f = analyze("a.vn", &facts);
        assert!(!ids(&f).contains(&"dns:caa:missing"));
        assert!(!ids(&f).contains(&"dns:dnssec:missing"));
    }
}

#[cfg(test)]
mod regressions {
    use super::*;

    #[test]
    fn names_are_made_absolute_so_search_domains_cannot_be_appended() {
        // Bẫy đã gặp thật: không có dấu chấm cuối, resolver hỏi
        // "vnexpress.net.local." rồi trả NXDOMAIN → báo sai "không có SPF".
        assert_eq!(fqdn("vnexpress.net"), "vnexpress.net.");
        assert_eq!(fqdn("vnexpress.net."), "vnexpress.net.");
        assert_eq!(fqdn("  a.vn  "), "a.vn.");
        assert_eq!(fqdn("_dmarc.a.vn"), "_dmarc.a.vn.");
    }

    #[test]
    fn a_failed_query_never_becomes_a_missing_record_finding() {
        let facts = DnsFacts {
            txt_apex_ok: false,
            txt_dmarc_ok: false,
            caa_ok: false,
            ds_ok: false,
            ..Default::default()
        };
        let f = analyze("a.vn", &facts);
        let ids: Vec<&str> = f.iter().map(|x| x.fingerprint.as_str()).collect();
        // Không được kết luận thiếu bất cứ thứ gì khi chưa hỏi được
        for bad in ["dns:spf:missing", "dns:dmarc:missing", "dns:caa:missing", "dns:dnssec:missing"] {
            assert!(!ids.contains(&bad), "không được báo {bad} khi truy vấn hỏng");
        }
        // Thay vào đó phải nói rõ là không kiểm được
        assert_eq!(f.iter().filter(|x| x.fingerprint.starts_with("dns:query-failed:")).count(), 4);
        assert!(f.iter().all(|x| x.severity == "info"));
    }
}
