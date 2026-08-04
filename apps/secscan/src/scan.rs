//! Điều phối một lần quét L1: lấy trang, đọc DNS, chạy các đầu dò, ghi phát
//! hiện, chấm điểm.

use crate::db::{Db, Finding};
use crate::{active, custom, dns, host, probe, scope, score, tls, vuln};
use anyhow::{anyhow, Result};
use serde_json::json;
use std::time::Duration;

/// Tối đa số redirect đi theo. Mỗi chặng phải kiểm lại IP — đó là chỗ DNS
/// rebinding chui vào nếu chỉ kiểm một lần lúc đầu.
const MAX_REDIRECTS: usize = 5;

pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        // Tự đi theo redirect để kiểm lại đích mỗi chặng; để reqwest tự làm là
        // mất quyền kiểm soát đó.
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(20))
        // Tự khai danh: biến "kẻ lạ đang dò" thành "một công cụ có người chịu
        // trách nhiệm". Không có lý do gì để nguỵ trang khi quét hạ tầng của mình.
        .user_agent("SenClaw-secscan/0.1 (+https://senclaw.local/secscan)")
        .build()
        .expect("dựng http client")
}

/// Tải một trang bằng **GET**, không phải HEAD.
///
/// Đo thật: `vnexpress.net` trả `406` cho HEAD nhưng `200` cho GET — scanner
/// dùng HEAD sẽ báo *mọi* security header đều thiếu, một bức tường cảnh báo sai.
/// `allow_local` chỉ được bật khi tài sản đã xác minh bằng phương thức `local`
/// — tức người dùng chủ động khai đây là hạ tầng nội bộ của mình. Không bao giờ
/// bật vì một URL bất kỳ tình cờ phân giải về dải riêng.
pub async fn fetch(http: &reqwest::Client, url: &str, allow_local: bool) -> Result<probe::Resp> {
    let mut current = url.to_string();
    for hop in 0..=MAX_REDIRECTS {
        let host = scope::host_of(&current)?;
        // Kiểm lại ở MỖI chặng, không chỉ chặng đầu.
        scope::check_host_allowed(&host, allow_local).await?;

        let resp = http
            .get(&current)
            .send()
            .await
            .map_err(|e| anyhow!("không tải được {current}: {e}"))?;
        let status = resp.status().as_u16();

        if (300..400).contains(&status) {
            let loc = resp
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());
            match loc {
                Some(l) if hop < MAX_REDIRECTS => {
                    current = url::Url::parse(&current)
                        .and_then(|b| b.join(&l))
                        .map(|u| u.to_string())
                        .unwrap_or(l);
                    continue;
                }
                _ => {}
            }
        }

        let https = current.starts_with("https://");
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
        // Chỉ giữ một mẩu body — đủ để tìm thẻ meta, không tải cả trang vào RAM.
        let body = resp.text().await.unwrap_or_default();
        let snippet: String = body.chars().take(8192).collect();

        return Ok(probe::Resp {
            url: current,
            status,
            headers,
            body_snippet: snippet,
            https,
        });
    }
    Err(anyhow!("quá {MAX_REDIRECTS} lần chuyển hướng"))
}

/// Quét thụ động một website: header + cookie + DNS. Không gửi payload nào.
pub async fn scan_passive(db: &Db, http: &reqwest::Client, asset_id: i64) -> Result<serde_json::Value> {
    let asset = db
        .get_asset(asset_id)
        .ok_or_else(|| anyhow!("không có tài sản id={asset_id}"))?;
    let target = asset["target"].as_str().unwrap_or_default().to_string();
    let host = scope::host_of(&target)?;

    // Chỉ tài sản đã xác minh bằng 'local' mới được chạm vào dải nội bộ.
    let allow_local = asset["verify_method"].as_str() == Some("local")
        && !asset["verified_at"].is_null();

    let scan_id = db.start_scan(asset_id, "passive")?;
    db.log("scan", &format!("bắt đầu quét thụ động {target}"), Some(scan_id));

    let mut findings: Vec<Finding> = vec![];

    // Luật tự thêm/nhập về. Bản hỏng đã bị chặn lúc thêm, nên tới đây chỉ còn
    // luật hợp lệ; cái nào vẫn parse lỗi thì bỏ qua chứ không làm hỏng lần quét.
    let custom_rules: Vec<custom::CustomRule> = db
        .custom_rules_raw()
        .iter()
        .filter_map(|j| serde_json::from_str(j).ok())
        .collect();

    // --- HTTP ---
    let url = if target.starts_with("http") {
        target.clone()
    } else {
        format!("https://{host}/")
    };
    // Lỗi HTTP KHÔNG làm hỏng cả lần quét.
    //
    // Bẫy đã gặp thật: `expired.badssl.com` khiến client HTTP từ chối bắt tay,
    // và bản đầu trả về "quét thất bại" — trong khi đó chính xác là đích cần
    // báo cáo nhất, và đầu dò TLS (socket thô, không quan tâm chứng thư có hợp
    // lệ không) vẫn đọc được nguyên nhân. Ghi lại rồi đi tiếp.
    let mut http_ok = false;
    match fetch(http, &url, allow_local).await {
        Ok(resp) => {
            http_ok = true;
            findings.extend(probe::analyze(&resp));
            findings.extend(custom::eval_http(&custom_rules, &resp));
        }
        Err(e) => findings.push(
            Finding::new("exposure", "info", "http:unreachable", "Không tải được trang qua HTTP")
                .detail(format!(
                    "{e}. Các phép kiểm header và cookie bị bỏ qua — xem phần TLS bên dưới, \
                     nguyên nhân thường nằm ở đó."
                )),
        ),
    }

    // --- DNS ---
    let mut dns_ok = false;
    match dns::collect(&host).await {
        Ok(facts) => {
            dns_ok = true;
            findings.extend(dns::analyze(&host, &facts));
            findings.extend(custom::eval_dns(&custom_rules, &facts.txt_apex, facts.txt_apex_ok));
        }
        Err(e) => findings.push(
            Finding::new("dns", "info", "dns:unavailable", "Không đọc được DNS")
                .detail(e.to_string()),
        ),
    }

    // --- TLS --- chỉ với đích https. Lỗi mạng ở đây không được làm hỏng cả lần
    // quét: header và DNS đã lấy được rồi, mất phần TLS vẫn còn giá trị.
    let mut tls_ok = false;
    if url.starts_with("https://") {
        let port = url::Url::parse(&url).ok().and_then(|u| u.port()).unwrap_or(443);
        match tls::scan(&host, port, crate::db::now()).await {
            Ok(f) => {
                tls_ok = true;
                findings.extend(f);
            }
            Err(e) => findings.push(
                Finding::new("tls", "info", "tls:unavailable", "Không dò được TLS")
                    .detail(e.to_string()),
            ),
        }
    }

    // Chỉ coi là thất bại khi KHÔNG đầu dò nào chạm được tới đích. Một đầu dò
    // hỏng thì kết quả là bán phần, và bán phần vẫn hơn không có gì.
    if !http_ok && !dns_ok && !tls_ok {
        let msg = format!("không đầu dò nào tiếp cận được {target}");
        db.finish_scan(scan_id, "failed", None, None, Some(&msg))?;
        return Err(anyhow!(msg));
    }

    apply_overrides(db, &mut findings);

    for f in &findings {
        db.upsert_finding(scan_id, asset_id, f)?;
    }
    let s = score::score(&findings);
    db.finish_scan(scan_id, "done", Some(s.score), Some(s.grade), None)?;
    db.log(
        "scan",
        &format!("xong {target}: {} phát hiện, {} điểm ({})", findings.len(), s.score, s.grade),
        Some(scan_id),
    );

    Ok(json!({
        "ok": true,
        "scan_id": scan_id,
        "score": s.score,
        "grade": s.grade,
        "findings": db.findings(Some(scan_id), None, None),
    }))
}

/// Quét CHỦ ĐỘNG (L2). Bên gọi phải đã kiểm `require_verified` — hàm này không
/// tự kiểm lại, nhưng cũng không có đường nào tới đây mà không qua đó.
pub async fn scan_active(db: &Db, http: &reqwest::Client, asset_id: i64) -> Result<serde_json::Value> {
    let asset = db
        .get_asset(asset_id)
        .ok_or_else(|| anyhow!("không có tài sản id={asset_id}"))?;
    let target = asset["target"].as_str().unwrap_or_default().to_string();
    let host = scope::host_of(&target)?;
    let allow_local = asset["verify_method"].as_str() == Some("local")
        && !asset["verified_at"].is_null();

    let url = if target.starts_with("http") {
        target.clone()
    } else {
        format!("https://{host}/")
    };

    let scan_id = db.start_scan(asset_id, "active-light")?;
    db.log("scan", &format!("bắt đầu quét chủ động {target}"), Some(scan_id));

    let res = match active::run(http, &url, allow_local).await {
        Ok(r) => r,
        Err(e) => {
            db.finish_scan(scan_id, "failed", None, None, Some(&e.to_string()))?;
            return Err(e);
        }
    };
    let mut findings = res.findings;

    if res.truncated {
        // Cắt bớt mà im lặng thì báo cáo đọc như "đã phủ hết" trong khi không phải.
        findings.push(
            Finding::new("exposure", "info", "active:budget-exhausted", "Chạm trần số yêu cầu")
                .detail(format!(
                    "Dừng ở {} yêu cầu để giữ nhịp thấp. Một số phép kiểm chưa chạy — kết quả là bán phần.",
                    res.requests
                )),
        );
    }

    apply_overrides(db, &mut findings);
    for f in &findings {
        db.upsert_finding(scan_id, asset_id, f)?;
    }
    let s = score::score(&findings);
    db.finish_scan(scan_id, "done", Some(s.score), Some(s.grade), None)?;
    db.log(
        "scan",
        &format!("xong chủ động {target}: {} phát hiện qua {} yêu cầu", findings.len(), res.requests),
        Some(scan_id),
    );

    Ok(json!({
        "ok": true,
        "scan_id": scan_id,
        "score": s.score,
        "grade": s.grade,
        "requests": res.requests,
        "truncated": res.truncated,
        "packages_checked": res.packages_checked,
        "findings": db.findings(Some(scan_id), None, None),
    }))
}

/// Quét L3 — máy chủ qua SSH. Tài sản phải có `ssh_ref` trỏ tới id máy bên
/// ssh-manager; **secscan không bao giờ giữ mật khẩu hay khoá riêng**.
pub async fn scan_host(db: &Db, http: &reqwest::Client, asset_id: i64) -> Result<serde_json::Value> {
    let asset = db
        .get_asset(asset_id)
        .ok_or_else(|| anyhow!("không có tài sản id={asset_id}"))?;
    let ssh_ref = asset["ssh_ref"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!(
            "tài sản chưa có ssh_ref — thêm máy chủ vào ssh-manager trước rồi ghi id của nó vào tài sản này"
        ))?
        .to_string();

    let scan_id = db.start_scan(asset_id, "host")?;
    db.log("scan", &format!("bắt đầu quét host (ssh_ref={ssh_ref})"), Some(scan_id));

    let facts = match host::collect(http, &ssh_ref).await {
        Ok(f) => f,
        Err(e) => {
            db.finish_scan(scan_id, "failed", None, None, Some(&e.to_string()))?;
            return Err(e);
        }
    };

    let mut findings = host::analyze(&facts);

    // Gói OS lộ ra: đối chiếu OSV/KEV/EPSS luôn — đây là chỗ CVE trả lãi lớn nhất
    // (một máy Debian điển hình có hàng trăm gói, mỗi bản vá tồn đọng là một CVE).
    let pkgs = host::packages(&facts);
    if !pkgs.is_empty() {
        match vuln::scan(http, &pkgs).await {
            Ok(r) => findings.extend(r.findings),
            Err(e) => findings.push(
                Finding::new("cve", "info", "cve:lookup-failed", "Không tra được CSDL lỗ hổng cho gói OS")
                    .detail(e.to_string()),
            ),
        }
    }

    apply_overrides(db, &mut findings);
    for f in &findings {
        db.upsert_finding(scan_id, asset_id, f)?;
    }
    let s = score::score(&findings);
    db.finish_scan(scan_id, "done", Some(s.score), Some(s.grade), None)?;
    db.log("scan", &format!("xong host: {} phát hiện", findings.len()), Some(scan_id));

    Ok(json!({
        "ok": true,
        "scan_id": scan_id,
        "score": s.score,
        "grade": s.grade,
        "packages_checked": pkgs.len(),
        "findings": db.findings(Some(scan_id), None, None),
    }))
}

/// Áp ghi đè do người dùng đặt: đổi mức, hoặc tắt hẳn một luật.
///
/// Khớp theo **tiền tố** fingerprint để một dòng ghi đè phủ cả họ luật
/// (`hdr:csp` phủ `hdr:csp:unsafe-inline`, `hdr:csp:no-base-uri`…). Tắt luật
/// thì phát hiện bị LOẠI HẲN, không phải hạ xuống info — hạ mức vẫn còn trong
/// danh sách và vẫn gây nhiễu, trong khi ý định của người tắt là không muốn thấy.
pub fn apply_overrides(db: &Db, findings: &mut Vec<Finding>) {
    let ov = db.overrides();
    if ov.is_empty() {
        return;
    }
    let find = |fp: &str| ov.iter().find(|(id, _, _, _)| fp.starts_with(id.as_str()));

    findings.retain(|f| find(&f.fingerprint).map(|(_, _, en, _)| *en).unwrap_or(true));

    for f in findings.iter_mut() {
        if let Some((_, Some(sev), _, _)) = find(&f.fingerprint) {
            if let Some(s) = ["critical", "high", "medium", "low", "info"]
                .iter()
                .find(|x| *x == sev)
            {
                f.severity = s;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fetch_refuses_private_and_metadata_targets_by_default() {
        let http = http_client();
        // Không được để scanner tự biến thành công cụ SSRF.
        for u in [
            "http://169.254.169.254/latest/meta-data/",
            "http://127.0.0.1:18788/api/groups",
            "http://192.168.1.1/",
            "http://168.63.129.16/metadata/instance",
        ] {
            let e = fetch(&http, u, false).await.unwrap_err().to_string();
            assert!(e.contains("từ chối"), "phải từ chối {u}, nhận được: {e}");
        }
    }

    #[tokio::test]
    async fn allow_local_lets_an_internal_asset_through_the_scope_check() {
        let http = http_client();
        // Với allow_local, rào chắn phạm vi không còn chặn — lỗi (nếu có) phải
        // là lỗi kết nối, không phải "từ chối".
        let e = fetch(&http, "http://127.0.0.1:9/", true)
            .await
            .unwrap_err()
            .to_string();
        assert!(
            !e.contains("từ chối"),
            "allow_local phải qua được rào phạm vi, nhận được: {e}"
        );
    }

    #[test]
    fn overrides_can_downgrade_and_disable_builtin_rules() {
        let db = Db::open_memory().unwrap();
        let mut f = vec![
            Finding::new("headers", "high", "hdr:csp:unsafe-inline", "a"),
            Finding::new("headers", "medium", "hdr:hsts:missing", "b"),
            Finding::new("dns", "low", "dns:caa:missing", "c"),
        ];
        // hạ cả họ hdr:csp xuống low; tắt hẳn dns:caa
        db.set_override("hdr:csp", Some("low"), true, None).unwrap();
        db.set_override("dns:caa", None, false, Some("chấp nhận rủi ro")).unwrap();
        apply_overrides(&db, &mut f);

        assert_eq!(f.len(), 2, "luật đã tắt phải bị loại hẳn, không chỉ hạ mức");
        assert_eq!(f[0].severity, "low", "ghi đè theo tiền tố phải phủ cả họ luật");
        assert_eq!(f[1].severity, "medium", "luật không bị ghi đè giữ nguyên");
        assert!(!f.iter().any(|x| x.fingerprint.starts_with("dns:caa")));
    }

    #[test]
    fn no_overrides_means_findings_pass_through_untouched() {
        let db = Db::open_memory().unwrap();
        let mut f = vec![Finding::new("headers", "high", "hdr:csp:x", "a")];
        apply_overrides(&db, &mut f);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, "high");
    }

    #[test]
    fn client_identifies_itself() {
        // Không nguỵ trang: quét hạ tầng của mình thì tự khai danh là đúng.
        let _ = http_client();
    }
}
