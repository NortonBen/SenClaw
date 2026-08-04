//! Đối chiếu thư viện với CSDL lỗ hổng: OSV.dev + CISA KEV + EPSS.
//!
//! Cả ba nguồn đều **không cần khoá API**, nên không phải cài scanner ngoài và
//! không phụ thuộc tài khoản nào.
//!
//! Cách xếp ưu tiên là điểm khác biệt so với việc chỉ liệt kê CVE theo CVSS:
//!
//! - **KEV là phép ĐÈ CỨNG, không phải trọng số.** Một mục nằm trong danh mục
//!   "đang bị khai thác thật" của CISA phải đứng trên mọi mục không nằm trong đó,
//!   kể cả khi điểm CVSS thấp hơn. Lý do đơn giản: một cái đang bị khai thác,
//!   cái kia thì chưa.
//! - **Ngưỡng hành động EPSS 0.1** theo số liệu hiệu quả của FIRST: lọc theo
//!   CVSS ≥ 7 bắt phải vá ~50% kho lỗ hổng để chạm tới ~6% số thật sự bị khai
//!   thác; lọc EPSS ≥ 0.1 chỉ tốn ~2.7% công sức mà đạt ~45%.
//!
//! Quy trình gọi mạng cố ý tiết kiệm: `querybatch` một lượt để biết gói NÀO có
//! lỗ hổng (rẻ), rồi mới `query` đầy đủ cho riêng những gói đó.

use crate::db::Finding;
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

pub const OSV_BATCH: &str = "https://api.osv.dev/v1/querybatch";
pub const OSV_QUERY: &str = "https://api.osv.dev/v1/query";
pub const KEV_URL: &str =
    "https://www.cisa.gov/sites/default/files/feeds/known_exploited_vulnerabilities.json";
pub const EPSS_URL: &str = "https://api.first.org/data/v1/epss";

/// Trần số gói mỗi lần quét. OSV nhận 1000 truy vấn/lượt nhưng ta không có lý do
/// gửi nhiều thế, và cắt sớm thì phải nói ra chứ không im lặng.
pub const MAX_PACKAGES: usize = 200;
/// Trần số gói được lấy chi tiết. Một gói OS có thể trả hàng chục lỗ hổng.
pub const MAX_DETAIL_QUERIES: usize = 40;

#[derive(Debug, Clone, PartialEq)]
pub struct Package {
    pub ecosystem: String,
    pub name: String,
    pub version: String,
}

impl Package {
    pub fn new(ecosystem: &str, name: &str, version: &str) -> Self {
        Self {
            ecosystem: ecosystem.to_string(),
            name: name.to_string(),
            version: version.trim().to_string(),
        }
    }

    /// Định danh có hợp lệ với hệ sinh thái này không.
    ///
    /// **Bẫy đã đo được:** Maven đòi `groupId:artifactId`. Gửi mỗi
    /// `log4j-core` thì OSV trả mảng RỖNG — không báo lỗi, chỉ im lặng, và
    /// scanner sẽ kết luận "an toàn" trong khi log4shell nằm ngay đó. Nên đây
    /// phải là lỗi cứng chứ không phải cảnh báo.
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            bail!("gói thiếu tên");
        }
        if self.version.is_empty() {
            bail!("gói '{}' thiếu phiên bản", self.name);
        }
        if self.ecosystem == "Maven" && !self.name.contains(':') {
            bail!(
                "Maven cần 'groupId:artifactId', nhận được '{}' — gửi thiếu groupId thì OSV trả rỗng và kết quả trông như an toàn",
                self.name
            );
        }
        Ok(())
    }

    fn query_json(&self) -> Value {
        json!({
            "package": { "ecosystem": self.ecosystem, "name": self.name },
            "version": self.version
        })
    }
}

#[derive(Debug, Clone)]
pub struct Vuln {
    pub id: String,
    pub cves: Vec<String>,
    /// CRITICAL | HIGH | MODERATE | LOW, theo `database_specific.severity` của OSV.
    pub osv_severity: Option<String>,
    pub summary: String,
    pub package: Package,
    pub kev: bool,
    pub epss: Option<f64>,
}

/// Ánh xạ mức của OSV sang thang của app.
pub fn map_severity(osv: Option<&str>) -> &'static str {
    match osv.map(|s| s.to_ascii_uppercase()).as_deref() {
        Some("CRITICAL") => "critical",
        Some("HIGH") => "high",
        Some("MODERATE") | Some("MEDIUM") => "medium",
        Some("LOW") => "low",
        // Không rõ mức thì để 'medium', KHÔNG để 'info': chưa biết mức độ không
        // có nghĩa là vô hại.
        _ => "medium",
    }
}

/// Ngưỡng EPSS đáng hành động — xem ghi chú đầu tệp.
pub const EPSS_ACTION: f64 = 0.1;

// ---------------------------------------------------------------------------
// Gọi mạng
// ---------------------------------------------------------------------------

/// Bước 1 (rẻ): gói nào có lỗ hổng. Trả về chỉ số của các gói cần tra tiếp.
pub async fn which_have_vulns(http: &reqwest::Client, pkgs: &[Package]) -> Result<Vec<usize>> {
    if pkgs.is_empty() {
        return Ok(vec![]);
    }
    let body = json!({ "queries": pkgs.iter().map(|p| p.query_json()).collect::<Vec<_>>() });
    let resp = http
        .post(OSV_BATCH)
        .json(&body)
        .send()
        .await
        .map_err(|e| anyhow!("không gọi được OSV: {e}"))?;
    if !resp.status().is_success() {
        bail!("OSV trả {}", resp.status().as_u16());
    }
    let v: Value = resp.json().await.map_err(|e| anyhow!("OSV trả JSON hỏng: {e}"))?;
    let results = v["results"].as_array().cloned().unwrap_or_default();
    Ok(results
        .iter()
        .enumerate()
        .filter(|(_, r)| r["vulns"].as_array().map(|a| !a.is_empty()).unwrap_or(false))
        .map(|(i, _)| i)
        .collect())
}

/// Bước 2: chi tiết đầy đủ cho một gói.
pub async fn details(http: &reqwest::Client, pkg: &Package) -> Result<Vec<Vuln>> {
    let resp = http
        .post(OSV_QUERY)
        .json(&pkg.query_json())
        .send()
        .await
        .map_err(|e| anyhow!("không gọi được OSV: {e}"))?;
    let v: Value = resp.json().await.map_err(|e| anyhow!("OSV trả JSON hỏng: {e}"))?;
    Ok(parse_vulns(&v, pkg))
}

/// Tách danh sách lỗ hổng từ phản hồi `/v1/query`.
pub fn parse_vulns(v: &Value, pkg: &Package) -> Vec<Vuln> {
    v["vulns"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|x| {
                    let id = x["id"].as_str()?.to_string();
                    let cves: Vec<String> = x["aliases"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|s| s.as_str())
                                .filter(|s| s.starts_with("CVE-"))
                                .map(|s| s.to_string())
                                .collect()
                        })
                        .unwrap_or_default();
                    Some(Vuln {
                        id,
                        cves,
                        osv_severity: x["database_specific"]["severity"]
                            .as_str()
                            .map(|s| s.to_string()),
                        summary: x["summary"].as_str().unwrap_or("").to_string(),
                        package: pkg.clone(),
                        kev: false,
                        epss: None,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Danh mục KEV của CISA — tập CVE đang bị khai thác trong thực tế.
pub async fn fetch_kev(http: &reqwest::Client) -> Result<HashSet<String>> {
    let v: Value = http
        .get(KEV_URL)
        .send()
        .await
        .map_err(|e| anyhow!("không tải được KEV: {e}"))?
        .json()
        .await
        .map_err(|e| anyhow!("KEV trả JSON hỏng: {e}"))?;
    Ok(parse_kev(&v))
}

pub fn parse_kev(v: &Value) -> HashSet<String> {
    v["vulnerabilities"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x["cveID"].as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

/// EPSS cho một mẻ CVE. API nhận danh sách ngăn cách bởi dấu phẩy.
pub async fn fetch_epss(http: &reqwest::Client, cves: &[String]) -> HashMap<String, f64> {
    let mut out = HashMap::new();
    if cves.is_empty() {
        return out;
    }
    // Chia mẻ để URL không quá dài.
    for chunk in cves.chunks(80) {
        let url = format!("{EPSS_URL}?cve={}", chunk.join(","));
        let Ok(resp) = http.get(&url).send().await else {
            continue;
        };
        let Ok(v) = resp.json::<Value>().await else {
            continue;
        };
        out.extend(parse_epss(&v));
    }
    out
}

pub fn parse_epss(v: &Value) -> HashMap<String, f64> {
    v["data"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    let cve = x["cve"].as_str()?.to_string();
                    let e = x["epss"].as_str()?.parse::<f64>().ok()?;
                    Some((cve, e))
                })
                .collect()
        })
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Xếp hạng & phát hiện
// ---------------------------------------------------------------------------

/// Khoá sắp xếp: KEV trước, rồi EPSS đáng hành động, rồi mức nặng.
///
/// Trả tuple so sánh được — nhỏ hơn là ưu tiên cao hơn.
pub fn rank(v: &Vuln) -> (u8, u8, i64) {
    let tier = if v.kev {
        0
    } else if v.epss.unwrap_or(0.0) >= EPSS_ACTION {
        1
    } else {
        2
    };
    let sev = match map_severity(v.osv_severity.as_deref()) {
        "critical" => 0,
        "high" => 1,
        "medium" => 2,
        "low" => 3,
        _ => 4,
    };
    // EPSS giảm dần trong cùng bậc; nhân 1e6 để so bằng số nguyên
    let epss_desc = -((v.epss.unwrap_or(0.0) * 1_000_000.0) as i64);
    (tier, sev, epss_desc)
}

/// Chuyển lỗ hổng thành phát hiện, đã xếp theo thứ tự nên vá.
pub fn to_findings(vulns: &mut Vec<Vuln>) -> Vec<Finding> {
    vulns.sort_by_key(rank);
    vulns
        .iter()
        .map(|v| {
            let cve = v.cves.first().cloned();
            // KEV nâng mức lên critical bất kể CVSS nói gì: đang bị khai thác
            // thật thì không còn là chuyện lý thuyết.
            let sev = if v.kev {
                "critical"
            } else {
                map_severity(v.osv_severity.as_deref())
            };
            let mut detail = if v.summary.is_empty() {
                format!("{} trong {} {}", v.id, v.package.name, v.package.version)
            } else {
                v.summary.clone()
            };
            if v.kev {
                detail.push_str(
                    "\n\nCVE này nằm trong danh mục KEV của CISA — tức là ĐANG bị khai thác trong thực tế, không phải rủi ro lý thuyết. Vá trước mọi thứ khác.",
                );
            } else if v.epss.unwrap_or(0.0) >= EPSS_ACTION {
                detail.push_str(&format!(
                    "\n\nEPSS {:.1}% — xác suất bị khai thác trong 30 ngày tới cao hơn hẳn mức trung bình.",
                    v.epss.unwrap_or(0.0) * 100.0
                ));
            }
            let mut f = Finding::new(
                "cve",
                sev,
                format!("cve:{}:{}", v.package.name, v.id),
                format!("{} {} — {}", v.package.name, v.package.version, v.id),
            )
            .detail(detail)
            .evidence(json!({
                "osv_id": v.id,
                "cves": v.cves,
                "ecosystem": v.package.ecosystem,
                "package": v.package.name,
                "version": v.package.version,
                "epss": v.epss,
                "kev": v.kev,
            }))
            .fix(format!(
                "Nâng cấp {} lên phiên bản đã vá. Chi tiết: https://osv.dev/vulnerability/{}",
                v.package.name, v.id
            ));
            f.cve = cve;
            f.kev = v.kev;
            f.epss = v.epss;
            f
        })
        .collect()
}

pub struct ScanResult {
    pub findings: Vec<Finding>,
    pub packages_checked: usize,
    pub truncated: bool,
}

/// Quét đầy đủ một danh sách gói.
pub async fn scan(http: &reqwest::Client, pkgs: &[Package]) -> Result<ScanResult> {
    let mut truncated = pkgs.len() > MAX_PACKAGES;
    let pkgs: Vec<Package> = pkgs.iter().take(MAX_PACKAGES).cloned().collect();

    // Định danh sai làm kết quả trông như an toàn — loại ra và nói rõ.
    let mut bad = vec![];
    let good: Vec<Package> = pkgs
        .iter()
        .filter(|p| match p.validate() {
            Ok(()) => true,
            Err(e) => {
                bad.push(e.to_string());
                false
            }
        })
        .cloned()
        .collect();

    let hits = which_have_vulns(http, &good).await?;
    let mut detailed = hits.len();
    if detailed > MAX_DETAIL_QUERIES {
        detailed = MAX_DETAIL_QUERIES;
        truncated = true;
    }

    let mut vulns = vec![];
    for i in hits.into_iter().take(detailed) {
        if let Ok(v) = details(http, &good[i]).await {
            vulns.extend(v);
        }
    }

    // Làm giàu: KEV + EPSS
    let all_cves: Vec<String> = {
        let mut s: Vec<String> = vulns.iter().flat_map(|v| v.cves.clone()).collect();
        s.sort();
        s.dedup();
        s
    };
    if !all_cves.is_empty() {
        let kev = fetch_kev(http).await.unwrap_or_default();
        let epss = fetch_epss(http, &all_cves).await;
        for v in vulns.iter_mut() {
            v.kev = v.cves.iter().any(|c| kev.contains(c));
            v.epss = v.cves.iter().filter_map(|c| epss.get(c).copied()).fold(None, |acc, e| {
                Some(acc.map_or(e, |a: f64| a.max(e)))
            });
        }
    }

    let mut findings = to_findings(&mut vulns);
    for msg in bad {
        findings.push(
            Finding::new("cve", "info", "cve:bad-identifier", "Bỏ qua gói vì định danh sai")
                .detail(msg),
        );
    }
    Ok(ScanResult {
        findings,
        packages_checked: good.len(),
        truncated,
    })
}

// ---------------------------------------------------------------------------
// Rút gói từ tệp manifest
// ---------------------------------------------------------------------------

/// Bỏ tiền tố dải phiên bản của npm/composer (`^4.17.20` → `4.17.20`).
///
/// OSV cần một phiên bản CỤ THỂ. Dải như `^1.2.3` hay `>=1.0` không tra được,
/// nên gói nào chỉ khai dải thì bỏ qua — thà thiếu còn hơn tra sai phiên bản
/// rồi báo lỗ hổng không tồn tại.
pub fn exact_version(spec: &str) -> Option<String> {
    let s = spec.trim().trim_start_matches(['^', '~', '=', 'v']).trim();
    if s.is_empty() || s == "*" || s.contains(['|', ' ', '<', '>', 'x', '*']) {
        return None;
    }
    // phải bắt đầu bằng chữ số
    if !s.chars().next()?.is_ascii_digit() {
        return None;
    }
    Some(s.to_string())
}

/// Rút gói từ `package.json` (npm) hoặc `composer.json` (Packagist).
pub fn packages_from_manifest(body: &str, ecosystem: &str) -> Vec<Package> {
    let Ok(v) = serde_json::from_str::<Value>(body) else {
        return vec![];
    };
    let mut out = vec![];
    for key in ["dependencies", "devDependencies", "require", "require-dev"] {
        let Some(obj) = v[key].as_object() else { continue };
        for (name, spec) in obj {
            // php / ext-* trong composer không phải gói tra được
            if name.starts_with("ext-") || name == "php" {
                continue;
            }
            let Some(spec) = spec.as_str() else { continue };
            let Some(ver) = exact_version(spec) else { continue };
            out.push(Package::new(ecosystem, name, &ver));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(id: &str, sev: Option<&str>, kev: bool, epss: Option<f64>) -> Vuln {
        Vuln {
            id: id.into(),
            cves: vec![format!("CVE-2024-{}", id.len())],
            osv_severity: sev.map(|s| s.into()),
            summary: "x".into(),
            package: Package::new("npm", "p", "1.0.0"),
            kev,
            epss,
        }
    }

    #[test]
    fn maven_without_group_id_is_a_hard_error_not_a_warning() {
        // Đã đo thật: 'log4j-core' trần trả 0 lỗ hổng — im lặng bảo an toàn
        // trong khi log4shell nằm ngay đó. Phải chặn ở tầng định danh.
        let bad = Package::new("Maven", "log4j-core", "2.14.1");
        let e = bad.validate().unwrap_err().to_string();
        assert!(e.contains("groupId"), "{e}");

        let good = Package::new("Maven", "org.apache.logging.log4j:log4j-core", "2.14.1");
        assert!(good.validate().is_ok());

        // hệ sinh thái khác không đòi dấu hai chấm
        assert!(Package::new("npm", "lodash", "4.17.20").validate().is_ok());
    }

    #[test]
    fn missing_version_is_rejected() {
        assert!(Package::new("npm", "lodash", "").validate().is_err());
        assert!(Package::new("npm", "", "1.0.0").validate().is_err());
    }

    #[test]
    fn kev_outranks_everything_including_a_higher_cvss() {
        let kev_low = v("A", Some("LOW"), true, Some(0.001));
        let plain_critical = v("B", Some("CRITICAL"), false, Some(0.9));
        // Một cái ĐANG bị khai thác; cái kia thì chưa. Thứ tự không đổi vì CVSS.
        assert!(rank(&kev_low) < rank(&plain_critical));
    }

    #[test]
    fn epss_above_threshold_outranks_severity_alone() {
        let hot = v("A", Some("LOW"), false, Some(0.5));
        let cold_critical = v("B", Some("CRITICAL"), false, Some(0.001));
        assert!(rank(&hot) < rank(&cold_critical));
    }

    #[test]
    fn within_a_tier_severity_then_epss_decide() {
        let a = v("A", Some("CRITICAL"), false, Some(0.01));
        let b = v("B", Some("HIGH"), false, Some(0.09));
        assert!(rank(&a) < rank(&b), "cùng bậc thì mức nặng quyết định trước");

        let c = v("C", Some("HIGH"), false, Some(0.05));
        let d = v("D", Some("HIGH"), false, Some(0.001));
        assert!(rank(&c) < rank(&d), "cùng mức thì EPSS cao lên trước");
    }

    #[test]
    fn epss_threshold_boundary_is_inclusive() {
        let at = v("A", Some("LOW"), false, Some(EPSS_ACTION));
        let below = v("B", Some("LOW"), false, Some(EPSS_ACTION - 0.001));
        assert!(rank(&at) < rank(&below));
    }

    #[test]
    fn kev_forces_critical_regardless_of_declared_severity() {
        let mut vs = vec![v("A", Some("LOW"), true, Some(0.001))];
        let f = to_findings(&mut vs);
        assert_eq!(f[0].severity, "critical");
        assert!(f[0].kev);
        assert!(f[0].detail.contains("ĐANG bị khai thác"));
    }

    #[test]
    fn unknown_severity_defaults_to_medium_not_info() {
        // Chưa biết mức không có nghĩa là vô hại.
        assert_eq!(map_severity(None), "medium");
        assert_eq!(map_severity(Some("WEIRD")), "medium");
        assert_eq!(map_severity(Some("critical")), "critical");
        assert_eq!(map_severity(Some("MODERATE")), "medium");
    }

    #[test]
    fn findings_come_out_in_remediation_order() {
        let mut vs = vec![
            v("low", Some("LOW"), false, Some(0.001)),
            v("kev", Some("LOW"), true, Some(0.001)),
            v("crit", Some("CRITICAL"), false, Some(0.001)),
            v("hot", Some("LOW"), false, Some(0.6)),
        ];
        let f = to_findings(&mut vs);
        let order: Vec<&str> = f.iter().map(|x| x.evidence["osv_id"].as_str().unwrap()).collect();
        assert_eq!(order, vec!["kev", "hot", "crit", "low"]);
    }

    #[test]
    fn parses_osv_query_response() {
        let raw = json!({ "vulns": [{
            "id": "GHSA-x",
            "aliases": ["CVE-2020-28500", "GHSA-other"],
            "summary": "ReDoS in lodash",
            "database_specific": { "severity": "MODERATE" }
        }]});
        let p = Package::new("npm", "lodash", "4.17.20");
        let vs = parse_vulns(&raw, &p);
        assert_eq!(vs.len(), 1);
        assert_eq!(vs[0].cves, vec!["CVE-2020-28500"], "chỉ giữ alias dạng CVE");
        assert_eq!(vs[0].osv_severity.as_deref(), Some("MODERATE"));
    }

    #[test]
    fn parses_kev_and_epss_payloads() {
        let kev = parse_kev(&json!({ "vulnerabilities": [
            { "cveID": "CVE-2021-44228" }, { "cveID": "CVE-2023-44487" }
        ]}));
        assert!(kev.contains("CVE-2021-44228") && kev.len() == 2);

        let epss = parse_epss(&json!({ "data": [
            { "cve": "CVE-2021-44228", "epss": "0.99999", "percentile": "1.0" }
        ]}));
        assert!((epss["CVE-2021-44228"] - 0.99999).abs() < 1e-9);

        // payload rỗng/hỏng không được panic
        assert!(parse_kev(&json!({})).is_empty());
        assert!(parse_epss(&json!({ "data": [{ "cve": "x" }] })).is_empty());
    }

    #[test]
    fn version_ranges_are_skipped_rather_than_guessed() {
        // Tra sai phiên bản còn tệ hơn không tra: nó báo lỗ hổng không tồn tại.
        assert_eq!(exact_version("^4.17.20").as_deref(), Some("4.17.20"));
        assert_eq!(exact_version("~1.2.3").as_deref(), Some("1.2.3"));
        assert_eq!(exact_version("v2.0.0").as_deref(), Some("2.0.0"));
        assert_eq!(exact_version("1.0.0").as_deref(), Some("1.0.0"));
        for r in [">=1.0.0", "1.x", "*", "", "^1 || ^2", "<2.0", "latest"] {
            assert!(exact_version(r).is_none(), "phải bỏ qua dải '{r}'");
        }
    }

    #[test]
    fn extracts_packages_from_manifests() {
        let pkg = r#"{"dependencies":{"lodash":"^4.17.20","express":"4.17.1","x":">=1.0"},
                      "devDependencies":{"jest":"29.0.0"}}"#;
        let ps = packages_from_manifest(pkg, "npm");
        let names: Vec<&str> = ps.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"lodash") && names.contains(&"jest"));
        assert!(!names.contains(&"x"), "dải phiên bản phải bị bỏ");
        assert_eq!(ps.iter().find(|p| p.name == "lodash").unwrap().version, "4.17.20");

        let comp = r#"{"require":{"php":"^8.1","ext-json":"*","monolog/monolog":"2.9.1"}}"#;
        let ps = packages_from_manifest(comp, "Packagist");
        assert_eq!(ps.len(), 1, "php và ext-* không phải gói tra được");
        assert_eq!(ps[0].name, "monolog/monolog");

        assert!(packages_from_manifest("không phải json", "npm").is_empty());
    }
}
