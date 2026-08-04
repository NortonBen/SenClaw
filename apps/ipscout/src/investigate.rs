//! Ghép các đầu dò thành một lần điều tra hoàn chỉnh và lưu lại thành ảnh chụp.
//!
//! Hai lối vào, tách đúng theo ranh giới "có gửi gói tới mục tiêu không":
//!
//! * [`profile`] — lớp hồ sơ. Chỉ đọc RDAP/DNS/GeoIP/DNSBL. Chạy được với IP bất kỳ.
//! * [`scan_ports`] — lớp bề mặt. Mở kết nối TCP thật, nên **đòi xác minh sở hữu**.

use crate::db::{Db, Finding};
use crate::{arp, banner, geo, netclass, osguess, registry, rep, resolve, scan, scope, trace};
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::net::IpAddr;
use std::sync::Arc;

/// Chọn IP để điều tra từ một host.
///
/// Ưu tiên IPv4: các nguồn dữ liệu bên ngoài (GeoIP, DNSBL) phủ IPv6 rất mỏng,
/// nên chọn IPv6 khi có cả hai sẽ cho ra một bản hồ sơ nghèo hơn hẳn mà người
/// dùng không hiểu vì sao.
pub async fn pick_ip(host: &str) -> Result<IpAddr> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(ip);
    }
    let r = resolve::resolver()?;
    let ips: Vec<IpAddr> = r
        .lookup_ip(resolve::fqdn(host))
        .await
        .map_err(|e| anyhow!("không phân giải được '{host}': {e}"))?
        .iter()
        .collect();
    ips.iter()
        .find(|i| i.is_ipv4())
        .or_else(|| ips.first())
        .copied()
        .ok_or_else(|| anyhow!("'{host}' không phân giải ra IP nào"))
}

fn target_or_err(db: &Db, id: i64) -> Result<Value> {
    db.get_target(id)
        .ok_or_else(|| anyhow!("không có mục tiêu id={id}"))
}

// ---------------------------------------------------------------------------
// Lớp hồ sơ (thụ động)
// ---------------------------------------------------------------------------

pub async fn profile(db: &Arc<Db>, http: &reqwest::Client, target_id: i64) -> Result<Value> {
    let t = target_or_err(db, target_id)?;
    let host = t["host"].as_str().unwrap_or_default().to_string();
    let run_id = db.start_run(target_id, "profile")?;

    let ip = match pick_ip(&host).await {
        Ok(ip) => ip,
        Err(e) => {
            db.finish_run(run_id, "failed", None, &json!({}), Some(&e.to_string()))?;
            return Err(e);
        }
    };

    // Dải riêng/đặc biệt không có bản ghi đăng ký công khai — nói thẳng thay vì
    // để bốn nguồn ngoài lần lượt trả lỗi khó hiểu.
    if let Err(e) = registry::public_or_err(ip) {
        let summary = json!({
            "ip": ip.to_string(),
            "host": host,
            "private": true,
            "note": e.to_string(),
        });
        db.finish_run(run_id, "done", Some(&ip.to_string()), &summary, None)?;
        db.log("profile", &format!("{host} → {ip} (dải nội bộ)"), Some(run_id));
        return Ok(json!({ "ok": true, "run_id": run_id, "profile": summary }));
    }

    // Bốn nguồn độc lập, chạy song song. Nguồn nào hỏng thì chỉ mất phần đó —
    // không được kéo cả lần điều tra xuống.
    // Tra DNS xuôi cho một IP trần là vô nghĩa (không có zone mang tên "1.1.1.1"),
    // và trả về một khối rỗng trông y như một lần tra thất bại. Bỏ hẳn khối đó.
    let is_bare_ip = host.parse::<IpAddr>().is_ok();

    let (asn, rdap_info, geo_pair, ptr_info, fwd, bl) = tokio::join!(
        registry::asn_of(ip),
        registry::rdap(http, ip),
        geo::locate(http, ip),
        resolve::ptr(ip),
        async {
            if is_bare_ip {
                None
            } else {
                Some(resolve::forward(&host).await)
            }
        },
        rep::check(ip),
    );

    let asn = asn.unwrap_or_default();
    let rdap_info = rdap_info.unwrap_or_default();
    let (geo_data, other_cc) = geo_pair;

    let cls = netclass::classify(
        asn.asn,
        &[
            asn.org.clone(),
            rdap_info.org.clone(),
            rdap_info.name.clone(),
            ptr_info.names.first().cloned(),
        ],
    );
    let conf = geo::rate(geo_data.as_ref(), other_cc.as_deref(), cls.anycast);

    let summary = json!({
        "ip": ip.to_string(),
        "host": host,
        "asn": registry::asn_json(&asn),
        "registry": registry::rdap_json(&rdap_info),
        "geo": geo::to_json(geo_data.as_ref(), &conf),
        "network": netclass::to_json(&cls),
        "ptr": {
            "names": ptr_info.names,
            "forward_confirmed": ptr_info.forward_confirmed,
            "confirmed_names": ptr_info.confirmed_names,
            "lookup_ok": ptr_info.ok,
        },
        "dns": fwd.map(|f| json!({
            "a": f.a.iter().map(|i| i.to_string()).collect::<Vec<_>>(),
            "mx": f.mx, "ns": f.ns, "cname": f.cname, "txt": f.txt,
        })),
        "reputation": rep::to_json(&bl),
    });

    for f in profile_findings(&summary) {
        db.add_finding(run_id, target_id, &f)?;
    }
    db.finish_run(run_id, "done", Some(&ip.to_string()), &summary, None)?;
    db.log("profile", &format!("hồ sơ {host} ({ip})"), Some(run_id));

    Ok(json!({
        "ok": true,
        "run_id": run_id,
        "profile": summary,
        "findings": db.findings(Some(run_id), None, None),
    }))
}

/// Rút phát hiện từ hồ sơ. Hàm thuần — nhận JSON tóm tắt, trả danh sách.
pub fn profile_findings(s: &Value) -> Vec<Finding> {
    let mut out = vec![];

    if s["network"]["fronted"].as_bool() == Some(true) {
        let provider = s["network"]["provider"].as_str().unwrap_or("CDN");
        out.push(
            Finding::new("registry", "info", "net:fronted", format!("IP nằm sau {provider}"))
                .detail(s["network"]["reason"].as_str().unwrap_or_default())
                .evidence(s["network"].clone()),
        );
    }

    let listed = s["reputation"]["listed_count"].as_i64().unwrap_or(0);
    if listed > 0 {
        let zones: Vec<&str> = s["reputation"]["results"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter(|r| r["status"] == "listed")
                    .filter_map(|r| r["zone"].as_str())
                    .collect()
            })
            .unwrap_or_default();
        out.push(
            Finding::new(
                "reputation",
                if listed >= 2 { "high" } else { "medium" },
                "rep:listed",
                format!("IP có trong {listed} danh sách chặn"),
            )
            .detail(format!(
                "Nằm trong: {}. Hệ quả trực tiếp: thư gửi từ IP này bị từ chối hoặc \
                 rơi vào hộp thư rác ở phía người nhận.",
                zones.join(", ")
            ))
            .evidence(s["reputation"].clone())
            .fix("Tra cứu tại trang gỡ chặn của từng danh sách; sửa nguyên nhân (máy bị chiếm quyền, relay mở) TRƯỚC khi xin gỡ."),
        );
    }

    // Nguồn tra hỏng phải hiện ra như "chưa biết", không được im lặng thành "sạch".
    let unknown = s["reputation"]["unknown_count"].as_i64().unwrap_or(0);
    if unknown > 0 {
        out.push(
            Finding::new("reputation", "info", "rep:unknown", format!("{unknown} danh sách chặn không tra được"))
                .detail("Đây là lỗi tra cứu, KHÔNG phải kết luận IP sạch. Nguyên nhân thường gặp nhất là máy đang dùng resolver công cộng (8.8.8.8 / 1.1.1.1) — Spamhaus từ chối các resolver đó.")
                .evidence(s["reputation"].clone()),
        );
    }

    let has_ptr = s["ptr"]["names"].as_array().map(|a| !a.is_empty()).unwrap_or(false);
    let confirmed = s["ptr"]["forward_confirmed"].as_bool() == Some(true);
    // Truy vấn hỏng thì KHÔNG được kết luận "thiếu PTR" — đó là hai chuyện khác
    // nhau, và nhầm chúng là tạo việc cho người vận hành mà chẳng có vấn đề nào.
    if s["ptr"]["lookup_ok"].as_bool() == Some(false) {
        out.push(
            Finding::new("registry", "info", "ptr:lookup-failed", "Không tra được PTR")
                .detail("Truy vấn DNS ngược thất bại (timeout hoặc SERVFAIL). Đây là lỗi tra cứu, KHÔNG phải kết luận 'IP không có tên ngược'."),
        );
    } else if !has_ptr {
        out.push(
            Finding::new("registry", "low", "ptr:missing", "Không có bản ghi PTR")
                .detail("Máy chủ thư lớn coi IP không có tên ngược là dấu hiệu nguồn thư rác. Không ảnh hưởng gì nếu máy này không gửi thư.")
                .fix("Nhờ nhà cung cấp đặt PTR trỏ về tên miền của bạn, rồi thêm bản ghi A cho tên đó về đúng IP này."),
        );
    } else if !confirmed {
        out.push(
            Finding::new("registry", "low", "ptr:not-confirmed", "PTR không xác nhận được bằng tra xuôi")
                .detail("Tên trong PTR không tra xuôi về lại IP này (FCrDNS thất bại). PTR do chủ dải IP tự đặt nên một mình nó không chứng minh gì — chính vì vậy bên nhận thư mới kiểm cả hai chiều.")
                .evidence(s["ptr"].clone())
                .fix("Thêm bản ghi A cho tên trong PTR trỏ về đúng IP này."),
        );
    }

    if s["geo"]["confidence"]["country"] == "không có" {
        out.push(
            Finding::new("geo", "info", "geo:unavailable", "Không tra được vị trí địa lý")
                .detail(s["geo"]["confidence"]["note"].as_str().unwrap_or_default()),
        );
    }

    out
}

// ---------------------------------------------------------------------------
// Lớp bề mặt (chủ động)
// ---------------------------------------------------------------------------

pub async fn scan_ports(
    db: &Arc<Db>,
    target_id: i64,
    profile_name: Option<&str>,
    ports_spec: Option<&str>,
    concurrency: Option<usize>,
) -> Result<Value> {
    let t = target_or_err(db, target_id)?;
    let host = t["host"].as_str().unwrap_or_default().to_string();
    let ports = scan::resolve_ports(profile_name, ports_spec)?;
    let ip = pick_ip(&host).await?;

    // Chốt duy nhất còn lại ở đây là metadata cloud. Dải riêng, LAN, loopback đều
    // được quét — người dùng chủ SenClaw có toàn quyền với mạng của họ. Riêng
    // các điểm cuối metadata (169.254.169.254 v.v.) thì không: chạm chúng nghĩa
    // là app đang bị lừa qua DNS rebinding, không có ca dùng hợp lệ nào.
    if let Some(why) = scope::is_metadata_endpoint(ip) {
        return Err(anyhow!(
            "'{host}' phân giải về {ip} — từ chối: {why}. Đây là điểm cuối metadata cloud, \
             không phải một máy chủ để quét."
        ));
    }

    let run_id = db.start_run(target_id, "ports")?;
    let mut opts = scan::Opts::new(host.clone());
    // Auto-scale khi người dùng không tự đặt: với `full` (65535 cổng) ở concurrency
    // mặc định 32 và timeout 2.5s, một tường lửa drop im lặng sẽ khiến quét mất
    // ~85 phút. Nâng concurrency + rút timeout để về khoảng 5-10 phút — vẫn trong
    // ngưỡng chờ được. Người dùng khai `concurrency` thì tôn trọng, không đè.
    if let Some(c) = concurrency {
        opts.concurrency = c;
    } else if ports.len() > 8192 {
        opts.concurrency = 256;
        opts.connect_timeout = std::time::Duration::from_millis(1200);
        opts.read_timeout = std::time::Duration::from_millis(900);
    } else if ports.len() > 1024 {
        opts.concurrency = 128;
        opts.connect_timeout = std::time::Duration::from_millis(1600);
    }

    let results = scan::scan(ip, &ports, &opts).await;

    // Bối cảnh CDN quyết định mọi kết luận về sau nói về ai, nên lấy từ lần chạy
    // hồ sơ gần nhất nếu có.
    let (fronted, provider) = last_known_fronting(db, target_id);

    let mut os_evidence: Vec<banner::OsEvidence> = vec![];
    let mut port_json = vec![];
    for r in &results {
        os_evidence.extend(r.fp.os_evidence.clone());
        // Chuỗi trong chứng thư cũng là bằng chứng OS: `Microsoft` trong issuer
        // của một dịch vụ nội bộ hay đi cùng Windows Server.
        if let Some(tls) = &r.tls {
            if tls.subject.to_ascii_lowercase().contains("microsoft") {
                os_evidence.push(banner::OsEvidence {
                    os: "Windows".into(),
                    weight: 40,
                    from: format!("chứng thư TLS cổng {} do Microsoft cấp", r.port),
                });
            }
        }
        let j = scan::to_json(r, fronted);
        db.add_port(
            run_id,
            target_id,
            r.port,
            r.fp.service.as_deref(),
            r.fp.product.as_deref(),
            r.fp.version.as_deref(),
            &r.banner,
            j["severity"].as_str().unwrap_or("info"),
            &j,
        )?;
        port_json.push(j);
    }

    let os = osguess::guess(&os_evidence);
    let os_json = osguess::to_json(&os, if fronted { provider.as_deref() } else { None });

    let summary = json!({
        "ip": ip.to_string(),
        "host": host,
        "scanned": ports.len(),
        "open": results.len(),
        "fronted": fronted,
        "fronted_by": provider,
        "ports": port_json,
        "os": os_json,
        "method": "TCP connect (bắt tay đầy đủ — có ghi log ở phía máy chủ). Không SYN/stealth.",
    });

    for f in port_findings(&summary) {
        db.add_finding(run_id, target_id, &f)?;
    }
    db.finish_run(run_id, "done", Some(&ip.to_string()), &summary, None)?;
    db.log(
        "scan",
        &format!("quét {host} ({ip}): {}/{} cổng mở", results.len(), ports.len()),
        Some(run_id),
    );

    Ok(json!({
        "ok": true,
        "run_id": run_id,
        "result": summary,
        "findings": db.findings(Some(run_id), None, None),
    }))
}

// ---------------------------------------------------------------------------
// Traceroute — đường đi + phân tích từng hop
// ---------------------------------------------------------------------------

/// Chạy traceroute tới mục tiêu và làm giàu từng hop bằng ASN + PTR + phân loại
/// mạng + MAC (nếu hop cùng LAN).
///
/// Chốt về MAC: MAC là địa chỉ **lớp 2**, chỉ tồn tại giữa hai giao diện trên
/// cùng segment. Với hop xa app **không thể** biết MAC — cứ nói rõ như vậy chứ
/// đừng bịa. Xem [`arp`] để biết vì sao.
pub async fn traceroute(
    db: &Arc<Db>,
    target_id: i64,
    max_hops: Option<u8>,
) -> Result<Value> {
    let t = target_or_err(db, target_id)?;
    let host = t["host"].as_str().unwrap_or_default().to_string();
    let run_id = db.start_run(target_id, "trace")?;

    // Phân giải trước để nhận diện đâu là hop cuối; và để kiểm scope trước khi
    // đưa host cho traceroute (traceroute sẽ tự phân giải, nhưng ta cần chặn
    // metadata endpoint sớm — nhánh này vẫn là "chạm mục tiêu").
    let ip = match pick_ip(&host).await {
        Ok(ip) => ip,
        Err(e) => {
            db.finish_run(run_id, "failed", None, &json!({}), Some(&e.to_string()))?;
            return Err(e);
        }
    };
    if let Some(why) = scope::is_metadata_endpoint(ip) {
        let msg = format!("từ chối: {why}");
        db.finish_run(run_id, "failed", Some(&ip.to_string()), &json!({}), Some(&msg))?;
        return Err(anyhow!(msg));
    }

    let hops = match trace::run(
        &host,
        max_hops.unwrap_or(trace::MAX_HOPS),
        std::time::Duration::from_secs(2),
    )
    .await
    {
        Ok(h) => h,
        Err(e) => {
            db.finish_run(run_id, "failed", Some(&ip.to_string()), &json!({}), Some(&e.to_string()))?;
            return Err(e);
        }
    };

    // Enrich mỗi hop **song song** — Cymru DNS là O(1), PTR + ARP cache là O(1),
    // 30 hop chạy tuần tự thì thêm mấy giây vô ích. Trần đồng thời 16 để không
    // dội resolver.
    let sem = Arc::new(tokio::sync::Semaphore::new(16));
    let mut tasks = vec![];
    for hop in hops {
        let sem = sem.clone();
        tasks.push(tokio::spawn(async move {
            let _p = sem.acquire().await.ok();
            enrich_hop(hop).await
        }));
    }
    let mut enriched = vec![];
    for t in tasks {
        if let Ok(h) = t.await {
            enriched.push(h);
        }
    }

    let responded = enriched.iter().filter(|h| h["ip"].is_string()).count();
    let unique_asns: std::collections::HashSet<i64> = enriched
        .iter()
        .filter_map(|h| h["asn"].as_i64())
        .collect();
    let cdn_ahead = enriched
        .iter()
        .find_map(|h| {
            if h["network"]["fronted"].as_bool() == Some(true) {
                h["network"]["provider"].as_str().map(|s| s.to_string())
            } else {
                None
            }
        });

    let summary = json!({
        "target_ip": ip.to_string(),
        "host": host,
        "total_hops": enriched.len(),
        "responded_hops": responded,
        "unique_asns": unique_asns.len(),
        "cdn_ahead": cdn_ahead,
        "hops": enriched,
        "method": "TCP/ICMP traceroute qua binary hệ thống, có ghi log ở phía router. Không gửi gói dị dạng.",
        "mac_note": "MAC chỉ có ở hop cùng LAN với máy này — với hop xa không thể lấy được, đây là cách IP hoạt động chứ không phải giới hạn công cụ.",
    });

    for f in trace_findings(&summary) {
        db.add_finding(run_id, target_id, &f)?;
    }
    db.finish_run(run_id, "done", Some(&ip.to_string()), &summary, None)?;
    db.log(
        "trace",
        &format!("traceroute {host}: {responded}/{} hop trả lời, {} ASN", enriched.len(), unique_asns.len()),
        Some(run_id),
    );

    Ok(json!({
        "ok": true,
        "run_id": run_id,
        "result": summary,
        "findings": db.findings(Some(run_id), None, None),
    }))
}

/// Làm giàu một hop bằng ASN, tổ chức, PTR, phân loại mạng, và MAC (chỉ LAN).
async fn enrich_hop(hop: trace::Hop) -> Value {
    let mut base = json!({
        "ttl": hop.ttl,
        "ip": hop.ip.map(|i| i.to_string()),
        "rtt_ms": hop.rtt_ms,
        "asn": Value::Null,
        "as_name": Value::Null,
        "org": Value::Null,
        "ptr": Value::Null,
        "network": Value::Null,
        "mac": Value::Null,
        "note": Value::Null,
    });
    let Some(ip) = hop.ip else {
        base["note"] = json!("hop im lặng — thường là router bỏ qua ICMP TTL Exceeded");
        return base;
    };

    // Dải riêng KHÔNG có bản ghi RDAP/ASN công khai. Ghi rõ để không nhồi lỗi
    // 404 từ RDAP vào giao diện.
    let is_public = scope::is_blocked_ip(ip).is_none();

    let (asn_res, ptr_info, mac) = tokio::join!(
        async {
            if is_public {
                registry::asn_of(ip).await.ok()
            } else {
                None
            }
        },
        resolve::ptr(ip),
        arp::lookup(ip),
    );

    if let Some(a) = asn_res {
        base["asn"] = a.asn.map(|v| json!(v)).unwrap_or(Value::Null);
        base["as_name"] = a.org.clone().map(Value::String).unwrap_or(Value::Null);
        base["org"] = a.org.clone().map(Value::String).unwrap_or(Value::Null);

        let cls = netclass::classify(a.asn, &[a.org.clone()]);
        base["network"] = netclass::to_json(&cls);
    } else if is_public {
        base["note"] = json!("không tra được ASN — có thể IP vừa đổi chủ, chưa cập nhật ở Cymru");
    } else {
        base["note"] = json!("IP nội bộ/dải riêng — không có bản ghi đăng ký công khai");
    }

    if !ptr_info.names.is_empty() {
        base["ptr"] = json!({
            "names": ptr_info.names,
            "forward_confirmed": ptr_info.forward_confirmed,
        });
    }

    if let Some(m) = mac {
        base["mac"] = json!({
            "addr": m.addr,
            "iface": m.iface,
            "vendor": m.vendor_hint(),
            "source": m.source,
        });
    }

    base
}

/// Rút phát hiện từ traceroute. Hàm thuần.
pub fn trace_findings(s: &Value) -> Vec<Finding> {
    let mut out = vec![];
    let empty = vec![];
    let hops = s["hops"].as_array().unwrap_or(&empty);

    // Chuỗi hop im lặng liên tục ở giữa route — thường là dấu hiệu firewall drop
    // ICMP, không phải "mạng hỏng". Chỉ báo khi im ≥ 3 hop liên tiếp.
    let mut streak = 0;
    let mut max_streak = 0;
    for h in hops {
        if h["ip"].is_null() {
            streak += 1;
            max_streak = max_streak.max(streak);
        } else {
            streak = 0;
        }
    }
    if max_streak >= 3 {
        out.push(
            Finding::new(
                "trace",
                "info",
                "trace:silent-streak",
                format!("Có {max_streak} hop liên tiếp không trả lời"),
            )
            .detail("Bình thường: rất nhiều nhà mạng cấu hình router bỏ ICMP TTL Exceeded để giảm tải. Không có nghĩa là mạng hỏng."),
        );
    }

    // ASN thay đổi = biên giới mạng. Đếm để giúp người đọc thấy đường đi qua
    // bao nhiêu nhà cung cấp khác nhau.
    let unique = s["unique_asns"].as_i64().unwrap_or(0);
    if unique >= 3 {
        out.push(
            Finding::new(
                "trace",
                "info",
                "trace:multi-asn",
                format!("Đường đi qua {unique} nhà cung cấp mạng khác nhau"),
            )
            .detail("Mỗi lần ASN đổi là traffic đã sang một tenant/nhà cung cấp khác. Càng nhiều biên giới, càng nhiều bên có thể quan sát traffic không mã hoá."),
        );
    }

    // CDN đứng trước ⇒ nói rõ, đúng như bên profile.
    if let Some(cdn) = s["cdn_ahead"].as_str() {
        out.push(
            Finding::new("trace", "info", "trace:cdn-ahead", format!("Traffic vào {cdn} trước khi tới máy chủ gốc"))
                .detail("Một trong các hop trên đường đi thuộc CDN — nghĩa là kết nối chấm dứt ở biên CDN, không đi tiếp tới hạ tầng của bạn (trừ khi CDN mở origin-fetch riêng)."),
        );
    }

    out
}

/// Lần chạy hồ sơ gần nhất có nói mục tiêu nằm sau CDN không.
fn last_known_fronting(db: &Db, target_id: i64) -> (bool, Option<String>) {
    for r in db.list_runs(Some(target_id), 20) {
        if r["layer"] == "profile" {
            if let Some(true) = r["summary"]["network"]["fronted"].as_bool() {
                return (
                    true,
                    r["summary"]["network"]["provider"].as_str().map(|s| s.to_string()),
                );
            }
        }
    }
    (false, None)
}

/// Rút phát hiện từ kết quả quét. Hàm thuần.
pub fn port_findings(s: &Value) -> Vec<Finding> {
    let mut out = vec![];
    let empty = vec![];
    let ports = s["ports"].as_array().unwrap_or(&empty);

    for p in ports {
        let sev = p["severity"].as_str().unwrap_or("info");
        // Cổng bình thường (SSH, HTTPS) không cần thành mục việc — nhồi thêm
        // dòng "info" chỉ làm loãng đúng những mục đáng đọc.
        if sev == "info" {
            continue;
        }
        let port = p["port"].as_i64().unwrap_or(0);
        let what = p["product"]
            .as_str()
            .map(|x| x.to_string())
            .or_else(|| p["service"].as_str().map(|x| x.to_string()))
            .unwrap_or_else(|| "dịch vụ chưa nhận dạng".into());
        out.push(
            Finding::new(
                "ports",
                sev,
                format!("port:{port}:open"),
                format!("Cổng {port} mở — {what}"),
            )
            .detail(p["why"].as_str().unwrap_or_default())
            .evidence(p.clone())
            .fix(p["fix"].as_str().unwrap_or_default()),
        );
    }

    // Chứng thư hết hạn là lỗi người dùng thấy ngay trên trình duyệt.
    for p in ports {
        if p["tls"]["expired"].as_bool() == Some(true) {
            let port = p["port"].as_i64().unwrap_or(0);
            out.push(
                Finding::new("tls", "high", format!("tls:{port}:expired"), format!("Chứng thư TLS cổng {port} đã hết hạn"))
                    .detail(format!("Hết hạn {}. Trình duyệt sẽ chặn và hiện cảnh báo toàn trang.", p["tls"]["not_after"].as_str().unwrap_or("?")))
                    .evidence(p["tls"].clone())
                    .fix("Gia hạn chứng thư và bật gia hạn tự động (certbot renew / ACME)."),
            );
        }
    }

    if let Some(os) = s["os"]["os"].as_str() {
        let conf = s["os"]["confidence"].as_i64().unwrap_or(0);
        out.push(
            Finding::new("os", "info", "os:guess", format!("Hệ điều hành: {os} ({conf}%)"))
                .detail(s["os"]["note"].as_str().unwrap_or_default())
                .evidence(s["os"].clone()),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(f: &[Finding]) -> Vec<&str> {
        f.iter().map(|x| x.fingerprint.as_str()).collect()
    }

    #[test]
    fn being_listed_on_more_blocklists_raises_severity() {
        let one = json!({
            "reputation": { "listed_count": 1, "unknown_count": 0,
                "results": [{ "zone": "bl.spamcop.net", "status": "listed" }] },
            "ptr": { "names": ["a.vn"], "forward_confirmed": true },
            "network": {}, "geo": { "confidence": { "country": "cao" } }
        });
        let f = profile_findings(&one);
        let r = f.iter().find(|x| x.fingerprint == "rep:listed").unwrap();
        assert_eq!(r.severity, "medium");

        let mut two = one.clone();
        two["reputation"]["listed_count"] = json!(2);
        let f2 = profile_findings(&two);
        assert_eq!(
            f2.iter().find(|x| x.fingerprint == "rep:listed").unwrap().severity,
            "high"
        );
    }

    #[test]
    fn an_unqueryable_blocklist_is_reported_as_unknown_never_as_clean() {
        // Bẫy đã mã hoá ở rep.rs, ở đây kiểm nó nổi lên tới tầng phát hiện.
        let s = json!({
            "reputation": { "listed_count": 0, "unknown_count": 1, "results": [] },
            "ptr": { "names": ["a.vn"], "forward_confirmed": true },
            "network": {}, "geo": { "confidence": { "country": "cao" } }
        });
        let f = profile_findings(&s);
        let u = f.iter().find(|x| x.fingerprint == "rep:unknown").unwrap();
        assert!(u.detail.contains("KHÔNG phải kết luận IP sạch"));
        assert!(!ids(&f).contains(&"rep:listed"));
    }

    #[test]
    fn a_ptr_that_does_not_forward_confirm_is_called_out_separately_from_a_missing_one() {
        let missing = json!({
            "reputation": { "listed_count": 0, "unknown_count": 0 },
            "ptr": { "names": [], "forward_confirmed": false },
            "network": {}, "geo": { "confidence": { "country": "cao" } }
        });
        assert!(ids(&profile_findings(&missing)).contains(&"ptr:missing"));

        let unconfirmed = json!({
            "reputation": { "listed_count": 0, "unknown_count": 0 },
            "ptr": { "names": ["claims-to-be-google.com"], "forward_confirmed": false },
            "network": {}, "geo": { "confidence": { "country": "cao" } }
        });
        let f = profile_findings(&unconfirmed);
        assert!(ids(&f).contains(&"ptr:not-confirmed"));
        assert!(!ids(&f).contains(&"ptr:missing"));
    }

    #[test]
    fn a_healthy_profile_produces_no_findings_at_all() {
        let s = json!({
            "reputation": { "listed_count": 0, "unknown_count": 0 },
            "ptr": { "names": ["mail.a.vn"], "forward_confirmed": true },
            "network": { "fronted": false },
            "geo": { "confidence": { "country": "cao" } }
        });
        assert!(profile_findings(&s).is_empty());
    }

    #[test]
    fn being_behind_a_cdn_is_surfaced_because_it_changes_who_the_report_is_about() {
        let s = json!({
            "reputation": { "listed_count": 0, "unknown_count": 0 },
            "ptr": { "names": ["x"], "forward_confirmed": true },
            "network": { "fronted": true, "provider": "Cloudflare", "reason": "AS13335 là của Cloudflare" },
            "geo": { "confidence": { "country": "thấp" } }
        });
        let f = profile_findings(&s);
        let n = f.iter().find(|x| x.fingerprint == "net:fronted").unwrap();
        assert!(n.title.contains("Cloudflare"));
        assert_eq!(n.severity, "info");
    }

    #[test]
    fn open_database_ports_become_findings_but_normal_ports_do_not() {
        let s = json!({
            "ports": [
                { "port": 22,   "severity": "info",     "service": "ssh",   "product": "OpenSSH", "why": "ok",  "fix": "" },
                { "port": 3306, "severity": "critical", "service": "mysql", "product": "MySQL",   "why": "CSDL phơi ra Internet", "fix": "bind-address=127.0.0.1" }
            ],
            "os": {}
        });
        let f = port_findings(&s);
        // SSH bình thường không được thành mục việc — nhồi info làm loãng báo cáo
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].fingerprint, "port:3306:open");
        assert_eq!(f[0].severity, "critical");
        assert!(f[0].title.contains("MySQL"));
        assert!(!f[0].fix.is_empty());
    }

    #[test]
    fn an_expired_certificate_is_its_own_finding() {
        let s = json!({
            "ports": [{ "port": 443, "severity": "info", "service": "http",
                "tls": { "expired": true, "not_after": "Jan 1 00:00:00 2024 GMT" } }],
            "os": {}
        });
        let f = port_findings(&s);
        let t = f.iter().find(|x| x.fingerprint == "tls:443:expired").unwrap();
        assert_eq!(t.severity, "high");
        assert!(t.detail.contains("2024"));
    }

    #[test]
    fn the_os_guess_is_recorded_as_info_with_its_confidence_in_the_title() {
        let s = json!({
            "ports": [],
            "os": { "os": "Ubuntu 22.04 LTS", "confidence": 85, "note": "suy luận" }
        });
        let f = port_findings(&s);
        assert_eq!(f[0].fingerprint, "os:guess");
        assert!(f[0].title.contains("Ubuntu 22.04 LTS") && f[0].title.contains("85%"));
        assert_eq!(f[0].severity, "info");
    }

    #[test]
    fn no_os_conclusion_means_no_os_finding() {
        let s = json!({ "ports": [], "os": { "os": null, "confidence": 0 } });
        assert!(port_findings(&s).is_empty());
    }

    #[tokio::test]
    async fn scanning_a_target_that_resolves_to_a_metadata_endpoint_is_refused() {
        // Đây là chốt duy nhất còn lại. Người dùng thêm `metadata.internal` mà
        // nó trỏ về 169.254.169.254 → phải chặn. Không có ca dùng hợp lệ nào cho
        // việc quét cloud metadata từ bên ngoài.
        let db = Arc::new(Db::open_memory().unwrap());
        let t = db
            .add_target(1, "169.254.169.254", "169.254.169.254", "")
            .unwrap();
        let e = scan_ports(&db, t, None, Some("80"), None)
            .await
            .unwrap_err()
            .to_string();
        assert!(e.contains("metadata"), "được: {e}");
        // Không để lại lần chạy dở trong lịch sử
        assert!(db.list_runs(Some(t), 10).is_empty());
    }

    #[tokio::test]
    async fn scanning_a_private_lan_target_is_allowed_because_verification_was_removed() {
        // Chốt "chạy tự do" — người dùng chủ SenClaw có toàn quyền với mạng của
        // họ. 10.0.0.5, 192.168.x, 127.0.0.1 đều phải qua được cửa scope. (Thất
        // bại thật sẽ ở TCP connect chứ không phải ở cửa này.)
        let db = Arc::new(Db::open_memory().unwrap());
        for host in ["127.0.0.1", "10.0.0.5", "192.168.1.1", "169.254.1.1"] {
            let t = db.add_target(1, host, host, "").unwrap();
            let r = scan_ports(&db, t, None, Some("65534"), Some(2)).await;
            assert!(r.is_ok(), "{host} phải qua được cửa scope, được: {r:?}");
            // và đã ghi được ảnh chụp
            assert_eq!(db.list_runs(Some(t), 10).len(), 1);
        }
    }

    #[tokio::test]
    async fn a_missing_target_is_an_error_not_a_panic() {
        let db = Arc::new(Db::open_memory().unwrap());
        assert!(scan_ports(&db, 999, None, None, None).await.is_err());
    }

    #[tokio::test]
    async fn a_bare_ip_is_used_directly_without_a_dns_lookup() {
        assert_eq!(
            pick_ip("8.8.8.8").await.unwrap(),
            "8.8.8.8".parse::<IpAddr>().unwrap()
        );
        assert_eq!(
            pick_ip("2606:4700::1111").await.unwrap(),
            "2606:4700::1111".parse::<IpAddr>().unwrap()
        );
    }
}
