//! Luật tự thêm và nhập từ nguồn ngoài.
//!
//! **Nguyên tắc: luật thêm vào phải CHẠY được thật.** Nếu chỉ cho khai mô tả thì
//! danh mục sẽ có mục không bao giờ kích hoạt — tức là danh mục nói dối, đúng
//! thứ mà `rules.rs` sinh ra để chống. Vì vậy luật ở đây là **khai báo có ngữ
//! nghĩa thực thi**: một phép so khớp trên header / cookie / bản ghi DNS.
//!
//! Cố ý KHÔNG có luật kiểu script. Nhập luật từ URL là nạp nội dung không tin
//! cậy; một định dạng khai báo đóng thì kẻ soạn ruleset độc hại nhiều nhất cũng
//! chỉ tạo được cảnh báo sai, không chạy được mã.

use crate::db::Finding;
use crate::probe::Resp;
use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Trần kích thước nội dung nhập — chặn cả nhầm lẫn lẫn cố ý làm nghẽn.
pub const MAX_IMPORT_BYTES: usize = 512 * 1024;
/// Trần độ dài biểu thức chính quy. Crate `regex` chạy tuyến tính nên không có
/// ReDoS, nhưng biểu thức khổng lồ vẫn tốn bộ nhớ khi biên dịch.
pub const MAX_PATTERN_LEN: usize = 500;
pub const MAX_RULES: usize = 500;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    /// Header phản hồi HTTP, tên không phân biệt hoa thường.
    Header,
    /// Thuộc tính trong Set-Cookie (`secure`, `httponly`, `samesite`).
    CookieAttr,
    /// Bản ghi TXT ở apex.
    DnsTxt,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    /// Phải có mặt. Không có → phát hiện.
    Present,
    /// Không được có mặt. Có → phát hiện.
    Absent,
    Equals,
    Contains,
    NotContains,
    Regex,
    NotRegex,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Check {
    pub target: Target,
    /// Tên header / tên thuộc tính cookie. Bỏ trống với dns_txt.
    #[serde(default)]
    pub name: String,
    pub op: Op,
    #[serde(default)]
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomRule {
    pub id: String,
    pub title: String,
    #[serde(default = "default_category")]
    pub category: String,
    pub severity: String,
    #[serde(default)]
    pub rationale: String,
    #[serde(default)]
    pub fix: String,
    #[serde(default)]
    pub wstg: String,
    pub check: Check,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Nguồn gốc: "manual" hoặc URL đã nhập. Giữ lại để biết luật ở đâu ra.
    #[serde(default)]
    pub source: String,
}

fn default_category() -> String {
    "custom".to_string()
}
fn default_true() -> bool {
    true
}

const SEVERITIES: [&str; 5] = ["critical", "high", "medium", "low", "info"];

impl CustomRule {
    /// Kiểm tính hợp lệ **lúc thêm**, không phải lúc quét.
    ///
    /// Một biểu thức hỏng phát hiện ở giữa lần quét sẽ làm hỏng cả lần quét đó;
    /// bắt sớm thì người thêm luật nhận lỗi ngay khi còn nhớ mình vừa gõ gì.
    pub fn validate(&self) -> Result<()> {
        if self.id.trim().is_empty() {
            bail!("luật phải có id");
        }
        // Không cho giả dạng luật dựng sẵn: người đọc báo cáo phải phân biệt được.
        if !self.id.starts_with("custom:") {
            bail!("id luật tự thêm phải bắt đầu bằng 'custom:' (nhận được '{}')", self.id);
        }
        if self.title.trim().is_empty() {
            bail!("luật '{}' thiếu tiêu đề", self.id);
        }
        if !SEVERITIES.contains(&self.severity.as_str()) {
            bail!("luật '{}' có mức '{}' không hợp lệ — phải là một trong {:?}",
                self.id, self.severity, SEVERITIES);
        }
        if matches!(self.check.target, Target::Header | Target::CookieAttr)
            && self.check.name.trim().is_empty()
        {
            bail!("luật '{}' thiếu tên header/thuộc tính", self.id);
        }
        let needs_value = matches!(
            self.check.op,
            Op::Equals | Op::Contains | Op::NotContains | Op::Regex | Op::NotRegex
        );
        if needs_value && self.check.value.trim().is_empty() {
            bail!("luật '{}' dùng phép so khớp cần 'value' nhưng để trống", self.id);
        }
        if matches!(self.check.op, Op::Regex | Op::NotRegex) {
            if self.check.value.len() > MAX_PATTERN_LEN {
                bail!("luật '{}': biểu thức dài quá {MAX_PATTERN_LEN} ký tự", self.id);
            }
            regex::Regex::new(&self.check.value)
                .map_err(|e| anyhow!("luật '{}': biểu thức không hợp lệ — {e}", self.id))?;
        }
        Ok(())
    }

    fn to_finding(&self, evidence: Value) -> Finding {
        let sev: &'static str = SEVERITIES
            .iter()
            .find(|s| **s == self.severity)
            .copied()
            .unwrap_or("info");
        let cat: &'static str = match self.check.target {
            Target::Header => "headers",
            Target::CookieAttr => "cookies",
            Target::DnsTxt => "dns",
        };
        Finding::new(cat, sev, self.id.clone(), self.title.clone())
            .detail(self.rationale.clone())
            .fix(self.fix.clone())
            .evidence(evidence)
    }
}

/// So khớp một giá trị theo phép toán. `None` = trường không tồn tại.
fn matches(op: &Op, want: &str, got: Option<&str>) -> bool {
    match (op, got) {
        (Op::Present, Some(_)) => true,
        (Op::Present, None) => false,
        (Op::Absent, Some(_)) => false,
        (Op::Absent, None) => true,
        (_, None) => false,
        (Op::Equals, Some(v)) => v.trim().eq_ignore_ascii_case(want.trim()),
        (Op::Contains, Some(v)) => v.to_ascii_lowercase().contains(&want.to_ascii_lowercase()),
        (Op::NotContains, Some(v)) => !v.to_ascii_lowercase().contains(&want.to_ascii_lowercase()),
        (Op::Regex, Some(v)) => regex::Regex::new(want).map(|r| r.is_match(v)).unwrap_or(false),
        (Op::NotRegex, Some(v)) => regex::Regex::new(want).map(|r| !r.is_match(v)).unwrap_or(false),
    }
}

/// Luật "đạt" nghĩa là KHÔNG sinh phát hiện. Ngược lại sinh phát hiện.
fn passes(rule: &CustomRule, got: Option<&str>) -> bool {
    matches(&rule.check.op, &rule.check.value, got)
}

/// Chạy luật tự thêm trên một phản hồi HTTP.
pub fn eval_http(rules: &[CustomRule], r: &Resp) -> Vec<Finding> {
    let mut out = vec![];
    for rule in rules.iter().filter(|x| x.enabled) {
        match rule.check.target {
            Target::Header => {
                let name = rule.check.name.to_ascii_lowercase();
                let got = r.get(&name);
                if !passes(rule, got) {
                    out.push(rule.to_finding(json!({
                        "header": name,
                        "found": got,
                        "expected": format!("{:?} {}", rule.check.op, rule.check.value),
                    })));
                }
            }
            Target::CookieAttr => {
                // Áp cho TỪNG cookie: một cookie thiếu cờ là một phát hiện, chứ
                // không phải "có cookie nào đó đạt là xong".
                for raw in r.all("set-cookie") {
                    let cname = raw.split('=').next().unwrap_or("").trim().to_string();
                    let attr = rule.check.name.to_ascii_lowercase();
                    let got = cookie_attr(raw, &attr);
                    if !passes(rule, got.as_deref()) {
                        let mut f = rule.to_finding(json!({
                            "cookie": cname, "attribute": attr, "found": got,
                        }));
                        // fingerprint phải khác nhau theo từng cookie, nếu không
                        // các cookie sau ghi đè phát hiện của cookie trước.
                        f.fingerprint = format!("{}:{}", rule.id, cname);
                        f.title = format!("{} — cookie '{}'", rule.title, cname);
                        out.push(f);
                    }
                }
            }
            Target::DnsTxt => {} // xử lý ở eval_dns
        }
    }
    out
}

/// Chạy luật tự thêm trên tập bản ghi TXT.
pub fn eval_dns(rules: &[CustomRule], txt: &[String], txt_ok: bool) -> Vec<Finding> {
    let mut out = vec![];
    if !txt_ok {
        return out; // chưa hỏi được thì không kết luận — cùng nguyên tắc với dns.rs
    }
    let joined = txt.join("\n");
    for rule in rules.iter().filter(|x| x.enabled && x.check.target == Target::DnsTxt) {
        let got = if txt.is_empty() { None } else { Some(joined.as_str()) };
        if !passes(rule, got) {
            out.push(rule.to_finding(json!({ "txt_records": txt.len() })));
        }
    }
    out
}

fn cookie_attr(raw: &str, attr: &str) -> Option<String> {
    for part in raw.split(';').skip(1) {
        let p = part.trim();
        let low = p.to_ascii_lowercase();
        if low == attr {
            return Some(String::new()); // cờ trần: có mặt, không có giá trị
        }
        if let Some(v) = low.strip_prefix(&format!("{attr}=")) {
            return Some(v.trim().to_string());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Nhập từ nguồn ngoài
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct ImportReport {
    pub source: String,
    pub total: usize,
    pub valid: Vec<CustomRule>,
    pub rejected: Vec<(String, String)>,
    pub applied: bool,
}

/// Tách và kiểm tra ruleset. Nhận cả `{"rules":[…]}` lẫn mảng trần.
pub fn parse_ruleset(body: &str, source: &str) -> Result<ImportReport> {
    if body.len() > MAX_IMPORT_BYTES {
        bail!("nội dung {} byte, vượt trần {MAX_IMPORT_BYTES}", body.len());
    }
    let v: Value = serde_json::from_str(body).map_err(|e| anyhow!("JSON không hợp lệ: {e}"))?;
    let arr = v
        .get("rules")
        .and_then(|x| x.as_array())
        .or_else(|| v.as_array())
        .ok_or_else(|| anyhow!("cần một mảng luật, hoặc đối tượng có khoá 'rules'"))?;

    if arr.len() > MAX_RULES {
        bail!("{} luật, vượt trần {MAX_RULES}", arr.len());
    }

    let mut valid = vec![];
    let mut rejected = vec![];
    for (i, item) in arr.iter().enumerate() {
        let label = item
            .get("id")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("[mục thứ {}]", i + 1));
        match serde_json::from_value::<CustomRule>(item.clone()) {
            Ok(mut r) => {
                r.source = source.to_string();
                match r.validate() {
                    Ok(()) => valid.push(r),
                    Err(e) => rejected.push((label, e.to_string())),
                }
            }
            Err(e) => rejected.push((label, format!("cấu trúc sai: {e}"))),
        }
    }
    Ok(ImportReport {
        source: source.to_string(),
        total: arr.len(),
        valid,
        rejected,
        applied: false,
    })
}

/// Tải ruleset từ URL. Đi qua **cùng bộ chặn SSRF** mà scanner dùng — nếu không,
/// ô "nhập luật" thành đường vòng để gọi tới điểm cuối nội bộ.
pub async fn fetch_ruleset(http: &reqwest::Client, url: &str) -> Result<String> {
    let host = crate::scope::host_of(url)?;
    crate::scope::check_host_allowed(&host, false).await?;
    if !url.starts_with("https://") {
        bail!("chỉ nhận nguồn https:// — ruleset tải qua HTTP thuần có thể bị sửa trên đường");
    }
    let resp = http.get(url).send().await.map_err(|e| anyhow!("không tải được: {e}"))?;
    if !resp.status().is_success() {
        bail!("nguồn trả mã {}", resp.status().as_u16());
    }
    let body = resp.text().await.unwrap_or_default();
    if body.len() > MAX_IMPORT_BYTES {
        bail!("nội dung {} byte, vượt trần {MAX_IMPORT_BYTES}", body.len());
    }
    Ok(body)
}

pub fn to_json(r: &ImportReport) -> Value {
    json!({
        "ok": true,
        "source": r.source,
        "total": r.total,
        "accepted": r.valid.len(),
        "applied": r.applied,
        "rules": r.valid.iter().map(|x| json!({
            "id": x.id, "title": x.title, "severity": x.severity,
            "target": format!("{:?}", x.check.target).to_lowercase(),
            "name": x.check.name,
            "op": format!("{:?}", x.check.op).to_lowercase(),
            "value": x.check.value,
        })).collect::<Vec<_>>(),
        "rejected": r.rejected.iter().map(|(id, why)| json!({ "id": id, "reason": why }))
            .collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(id: &str, target: Target, name: &str, op: Op, value: &str) -> CustomRule {
        CustomRule {
            id: id.into(), title: "t".into(), category: "custom".into(),
            severity: "medium".into(), rationale: String::new(), fix: String::new(),
            wstg: String::new(),
            check: Check { target, name: name.into(), op, value: value.into() },
            enabled: true, source: "manual".into(),
        }
    }

    fn resp(headers: &[(&str, &str)]) -> Resp {
        Resp {
            url: "https://x.vn/".into(), status: 200,
            headers: headers.iter().map(|(k, v)| (k.to_ascii_lowercase(), v.to_string())).collect(),
            body_snippet: String::new(), https: true,
        }
    }

    #[test]
    fn header_present_rule_fires_only_when_missing() {
        let r = rule("custom:a", Target::Header, "x-req", Op::Present, "");
        assert!(eval_http(&[r.clone()], &resp(&[])).len() == 1, "thiếu header thì phải báo");
        assert!(eval_http(&[r], &resp(&[("x-req", "1")])).is_empty(), "có rồi thì không báo");
    }

    #[test]
    fn absent_rule_is_the_inverse() {
        let r = rule("custom:b", Target::Header, "x-powered-by", Op::Absent, "");
        assert_eq!(eval_http(&[r.clone()], &resp(&[("x-powered-by", "PHP")])).len(), 1);
        assert!(eval_http(&[r], &resp(&[])).is_empty());
    }

    #[test]
    fn regex_and_contains_operators_work() {
        let re = rule("custom:c", Target::Header, "server", Op::NotRegex, r"\d+\.\d+");
        // có số phiên bản -> vi phạm 'not_regex' -> báo
        assert_eq!(eval_http(&[re.clone()], &resp(&[("server", "nginx/1.18.0")])).len(), 1);
        assert!(eval_http(&[re], &resp(&[("server", "nginx")])).is_empty());

        let c = rule("custom:d", Target::Header, "cache-control", Op::Contains, "no-store");
        assert!(eval_http(&[c.clone()], &resp(&[("cache-control", "private, no-store")])).is_empty());
        assert_eq!(eval_http(&[c], &resp(&[("cache-control", "public")])).len(), 1);
    }

    #[test]
    fn cookie_rule_reports_each_offending_cookie_separately() {
        let r = rule("custom:e", Target::CookieAttr, "secure", Op::Present, "");
        let mut resp = resp(&[("set-cookie", "a=1; Path=/")]);
        resp.headers.push(("set-cookie".into(), "b=2; Secure".into()));
        resp.headers.push(("set-cookie".into(), "c=3".into()));
        let f = eval_http(&[r], &resp);
        assert_eq!(f.len(), 2, "chỉ 'a' và 'c' thiếu Secure");
        // fingerprint phải khác nhau, nếu không cookie sau ghi đè cookie trước
        assert_ne!(f[0].fingerprint, f[1].fingerprint);
        assert!(f.iter().all(|x| x.fingerprint.starts_with("custom:e:")));
    }

    #[test]
    fn dns_rule_stays_silent_when_the_query_failed() {
        let r = rule("custom:f", Target::DnsTxt, "", Op::Contains, "v=spf1");
        // hỏi được, không có bản ghi -> báo
        assert_eq!(eval_dns(&[r.clone()], &[], true).len(), 1);
        // KHÔNG hỏi được -> tuyệt đối không kết luận
        assert!(eval_dns(&[r], &[], false).is_empty());
    }

    #[test]
    fn validate_rejects_bad_rules_at_add_time() {
        let mut r = rule("custom:g", Target::Header, "x", Op::Regex, "[unclosed");
        assert!(r.validate().unwrap_err().to_string().contains("biểu thức không hợp lệ"));

        r.check.value = "ok".into();
        r.severity = "catastrophic".into();
        assert!(r.validate().unwrap_err().to_string().contains("không hợp lệ"));

        r.severity = "high".into();
        assert!(r.validate().is_ok());

        // id không có tiền tố custom: -> từ chối, để không giả dạng luật dựng sẵn
        r.id = "hdr:hsts".into();
        assert!(r.validate().unwrap_err().to_string().contains("custom:"));
    }

    #[test]
    fn oversized_regex_is_rejected() {
        let mut r = rule("custom:h", Target::Header, "x", Op::Regex, &"a".repeat(MAX_PATTERN_LEN + 1));
        assert!(r.validate().is_err());
        r.check.value = "a".repeat(10);
        assert!(r.validate().is_ok());
    }

    #[test]
    fn parse_accepts_both_shapes_and_reports_each_rejection() {
        let good = r#"{"rules":[
            {"id":"custom:x","title":"X","severity":"low",
             "check":{"target":"header","name":"x-a","op":"present"}}
        ]}"#;
        let rep = parse_ruleset(good, "manual").unwrap();
        assert_eq!(rep.valid.len(), 1);
        assert_eq!(rep.valid[0].source, "manual");

        // mảng trần cũng nhận
        let bare = r#"[{"id":"custom:y","title":"Y","severity":"info",
                       "check":{"target":"dns_txt","op":"contains","value":"a"}}]"#;
        assert_eq!(parse_ruleset(bare, "manual").unwrap().valid.len(), 1);

        // luật hỏng bị loại RIÊNG, không làm hỏng cả mẻ
        let mixed = r#"[
            {"id":"custom:ok","title":"OK","severity":"low","check":{"target":"header","name":"a","op":"present"}},
            {"id":"custom:bad","title":"B","severity":"nonsense","check":{"target":"header","name":"a","op":"present"}},
            {"id":"builtin:sneak","title":"S","severity":"low","check":{"target":"header","name":"a","op":"present"}}
        ]"#;
        let rep = parse_ruleset(mixed, "https://x.vn/r.json").unwrap();
        assert_eq!(rep.valid.len(), 1, "chỉ một luật hợp lệ");
        assert_eq!(rep.rejected.len(), 2);
        assert!(rep.rejected.iter().any(|(id, _)| id == "custom:bad"));
        assert!(rep.rejected.iter().any(|(id, _)| id == "builtin:sneak"));
    }

    #[test]
    fn import_is_not_applied_until_explicitly_asked() {
        let rep = parse_ruleset(
            r#"[{"id":"custom:z","title":"Z","severity":"low","check":{"target":"header","name":"a","op":"present"}}]"#,
            "https://x.vn/r.json",
        ).unwrap();
        // Nạp nội dung từ nguồn ngoài KHÔNG được tự đổi hành vi quét.
        assert!(!rep.applied, "nhập phải xem trước rồi mới áp dụng");
    }

    #[tokio::test]
    async fn fetch_refuses_internal_targets_and_plain_http() {
        let http = crate::scan::http_client();
        // Ô nhập luật không được thành đường vòng SSRF.
        for u in ["https://169.254.169.254/r.json", "https://127.0.0.1:18788/api/llm-config"] {
            let e = fetch_ruleset(&http, u).await.unwrap_err().to_string();
            assert!(e.contains("từ chối"), "phải chặn {u}: {e}");
        }
        let e = fetch_ruleset(&http, "http://example.com/r.json").await.unwrap_err().to_string();
        assert!(e.contains("https"), "phải đòi https: {e}");
    }
}
