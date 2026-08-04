//! REST + trạng thái dùng chung.
//!
//! Quy ước của repo: mọi tool MCP đều dựng cùng struct `*In` mà REST handler
//! deserialize, rồi gọi cùng hàm `*_value()`. Agent và người không thể lệch nhau —
//! nếu lệch thì một trong hai đang đọc số liệu không tồn tại.

use crate::db::Db;
use crate::{investigate, risk, scan, scope};
use app_space_sdk::SpaceClient;
use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::fmt::Display;
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Db>,
    pub http: reqwest::Client,
    #[allow(dead_code)]
    pub sc: SpaceClient,
    /// Fan-out phản hồi JSON-RPC tới SSE client đang nối.
    pub mcp_tx: tokio::sync::broadcast::Sender<String>,
}

/// Client HTTP dùng cho nguồn ngoài (RDAP, GeoIP).
///
/// Giới hạn redirect: chuỗi chuyển hướng dài là cách kinh điển để lôi client
/// tới đích khác hẳn cái nó tưởng.
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(scan::USER_AGENT)
        .timeout(Duration::from_secs(12))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .unwrap_or_default()
}

pub fn make_state() -> AppState {
    let db = Arc::new(Db::open_default().expect("mở ipscout db"));
    let (mcp_tx, _) = tokio::sync::broadcast::channel(100);
    AppState {
        db,
        http: http_client(),
        sc: SpaceClient::from_env(),
        mcp_tx,
    }
}

pub fn err(e: impl Display) -> Value {
    json!({ "ok": false, "error": e.to_string() })
}

// ---------------------------------------------------------------------------
// Project
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ProjectIn {
    pub name: String,
    #[serde(default)]
    pub note: String,
}

pub fn add_project_value(s: &AppState, b: &ProjectIn) -> Value {
    match s.db.add_project(&b.name, &b.note) {
        Ok(id) => {
            s.db.log("project", &format!("tạo project {}", b.name), Some(id));
            json!({ "ok": true, "id": id })
        }
        Err(e) => err(e),
    }
}

// ---------------------------------------------------------------------------
// Mục tiêu
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct TargetIn {
    #[serde(default = "one")]
    pub project_id: i64,
    pub target: String,
    #[serde(default)]
    pub label: String,
}

fn one() -> i64 {
    1
}

pub fn add_target_value(s: &AppState, b: &TargetIn) -> Value {
    let host = match scope::host_of(&b.target) {
        Ok(h) => h,
        Err(e) => return err(e),
    };
    match s.db.add_target(b.project_id, &b.target, &host, &b.label) {
        Ok(id) => {
            s.db.log("target", &format!("thêm mục tiêu {host}"), Some(id));
            json!({ "ok": true, "id": id, "host": host })
        }
        Err(e) => err(e),
    }
}

// ---------------------------------------------------------------------------
// Điều tra
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ProfileIn {
    pub target_id: i64,
}

pub async fn profile_value(s: &AppState, b: &ProfileIn) -> Value {
    match investigate::profile(&s.db, &s.http, b.target_id).await {
        Ok(v) => v,
        Err(e) => err(e),
    }
}

#[derive(Deserialize)]
pub struct ScanIn {
    pub target_id: i64,
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default)]
    pub ports: Option<String>,
    #[serde(default)]
    pub concurrency: Option<usize>,
}

pub async fn scan_value(s: &AppState, b: &ScanIn) -> Value {
    match investigate::scan_ports(
        &s.db,
        b.target_id,
        b.profile.as_deref(),
        b.ports.as_deref(),
        b.concurrency,
    )
    .await
    {
        Ok(v) => v,
        Err(e) => err(e),
    }
}

#[derive(Deserialize)]
pub struct TraceIn {
    pub target_id: i64,
    #[serde(default)]
    pub max_hops: Option<u8>,
}

pub async fn trace_value(s: &AppState, b: &TraceIn) -> Value {
    match investigate::traceroute(&s.db, b.target_id, b.max_hops).await {
        Ok(v) => v,
        Err(e) => err(e),
    }
}

#[derive(Deserialize)]
pub struct FindingsQuery {
    pub run_id: Option<i64>,
    pub target_id: Option<i64>,
    pub severity: Option<String>,
}

pub fn findings_value(s: &AppState, q: &FindingsQuery) -> Value {
    json!({
        "ok": true,
        "findings": s.db.findings(q.run_id, q.target_id, q.severity.as_deref()),
    })
}

/// Tổng hợp một mục tiêu: lần chạy gần nhất, cổng đang mở, phát hiện còn lại.
pub fn dashboard_value(s: &AppState, target_id: Option<i64>) -> Value {
    let Some(tid) = target_id else {
        return json!({
            "ok": true,
            "projects": s.db.list_projects(),
            "targets": s.db.list_targets(None),
        });
    };
    let runs = s.db.list_runs(Some(tid), 50);
    let latest_profile = runs.iter().find(|r| r["layer"] == "profile").cloned();
    let latest_scan = runs.iter().find(|r| r["layer"] == "ports").cloned();
    let open_ports = latest_scan
        .as_ref()
        .and_then(|r| r["id"].as_i64())
        .map(|id| s.db.ports_of(id))
        .unwrap_or_default();

    let findings = s.db.findings(None, Some(tid), None);
    let mut by_sev = serde_json::Map::new();
    for sev in ["critical", "high", "medium", "low", "info"] {
        let n = findings.iter().filter(|f| f["severity"] == sev).count();
        by_sev.insert(sev.to_string(), json!(n));
    }

    json!({
        "ok": true,
        "target": s.db.get_target(tid),
        "runs": runs.len(),
        "latest_profile": latest_profile,
        "latest_scan": latest_scan,
        "open_ports": open_ports,
        "severity_counts": Value::Object(by_sev),
        "top_findings": s.db.findings(None, Some(tid), None).into_iter().take(8).collect::<Vec<_>>(),
    })
}

/// Danh mục năng lực: app tra được gì, quét được gì, và **không** làm gì.
pub fn capabilities() -> Value {
    json!({
        "ok": true,
        "layers": [
            {
                "id": "profile",
                "name": "Hồ sơ (thụ động)",
                "sends_packets_to_target": false,
                "requires_ownership": false,
                "covers": [
                    "ASN + tên tổ chức (Team Cymru qua DNS)",
                    "Dải CIDR được cấp, ngày cấp, quốc gia đăng ký, email abuse (RDAP)",
                    "Vị trí địa lý kèm ĐỘ TIN và đối chiếu chéo hai nguồn",
                    "Phân loại mạng: CDN / cloud / hosting / ISP, và cờ 'IP này không phải máy chủ gốc'",
                    "Tên ngược PTR có xác nhận xuôi (FCrDNS)",
                    "Bản ghi DNS xuôi: A/AAAA/MX/NS/TXT/CNAME",
                    "Danh sách chặn thư rác (Spamhaus ZEN / SpamCop / Barracuda / SORBS)"
                ]
            },
            {
                "id": "ports",
                "name": "Bề mặt (chủ động)",
                "sends_packets_to_target": true,
                "requires_ownership": false,
                "covers": [
                    "Quét cổng TCP connect theo hồ sơ hoặc danh sách tự khai (đến 65535 cổng, `full` mode)",
                    "Bắt banner: SSH, SMTP, FTP, POP3, IMAP, MySQL/MariaDB, HTTP (mọi cổng, kể cả cổng lạ)",
                    "Nhận dạng sản phẩm + phiên bản đang chạy trên cổng mở",
                    "Chứng thư TLS: subject, SAN, nhà phát hành, hạn dùng, tự ký",
                    "Đoán hệ điều hành bằng suy luận có trọng số, kèm phần trăm và bằng chứng",
                    "Xếp mức rủi ro theo cổng, kèm lý do và cách sửa"
                ]
            },
            {
                "id": "trace",
                "name": "Đường đi (traceroute)",
                "sends_packets_to_target": true,
                "requires_ownership": false,
                "covers": [
                    "Traceroute qua binary hệ thống (macOS/Linux)",
                    "Enrich mỗi hop: ASN + tên tổ chức + phân loại mạng (CDN/cloud/ISP) + PTR",
                    "MAC của hop CÙNG LAN đọc từ ARP cache (hop xa KHÔNG lấy được — MAC là L2, bị viết lại ở mỗi router)",
                    "Đếm số ASN đường đi qua, phát hiện CDN đứng trước máy chủ gốc",
                    "Cờ vendor OUI cho MAC (VMware/QEMU/Raspberry Pi…) khi có"
                ]
            }
        ],
        "port_profiles": scan::PROFILES.iter().map(|(n, d)| json!({ "name": n, "desc": d,
            "ports": scan::profile_ports(n).map(|p| p.len()).unwrap_or(0) })).collect::<Vec<_>>(),
        "limits": {
            "max_ports_per_scan": scan::MAX_PORTS,
            "max_concurrency": scan::MAX_CONCURRENCY,
            "one_host_per_scan": true,
        },
        "never_does": [
            "Quét SYN/stealth hoặc bất kỳ kỹ thuật né tránh phát hiện nào — chỉ TCP connect, có ghi log ở phía máy chủ.",
            "Quét dải mạng hàng loạt — mỗi lần MỘT host (hồ sơ `full` quét toàn bộ 65535 cổng của HOST đó, không phải nhiều host).",
            "Dò mật khẩu hoặc thử thông tin đăng nhập mặc định.",
            "Khai thác lỗ hổng dưới mọi hình thức.",
            "Quét UDP hoặc gửi gói dị dạng để moi phản ứng của ngăn xếp mạng.",
            "Vân tay hệ điều hành kiểu nmap -O (cần raw socket + gói dị dạng).",
            "Quét các điểm cuối metadata cloud (169.254.169.254, 168.63.129.16, 100.100.100.200, fd00:ec2::254, fd20:ce::254) — không có ca dùng hợp lệ nào."
        ],
        "notes": [
            "Không đòi xác minh sở hữu. App tin người dùng chủ SenClaw có quyền với mục tiêu họ khai — trách nhiệm pháp lý về việc quét đúng chỗ nằm ở người dùng.",
            "Vẫn có một chốt duy nhất: không quét được các điểm cuối metadata cloud (xem never_does)."
        ],
        "risk_rules": risk::catalog(),
    })
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn api_router(state: AppState) -> Router {
    Router::new()
        .route("/status", get(status))
        .route("/capabilities", get(caps))
        .route("/dashboard", get(dashboard))
        .route("/projects", get(list_projects).post(add_project))
        .route("/projects/:id/delete", post(delete_project))
        .route("/targets", get(list_targets).post(add_target))
        .route("/targets/:id/delete", post(delete_target))
        .route("/profile", post(run_profile))
        .route("/scan", post(run_scan))
        .route("/trace", post(run_trace))
        .route("/runs", get(list_runs))
        .route("/runs/:id", get(get_run))
        .route("/findings", get(findings))
        .route("/diff", get(diff))
        .route("/activity", get(activity))
        .route("/mcp/sse", get(crate::mcp::mcp_sse).post(crate::mcp::mcp_message))
        .route("/mcp/message", post(crate::mcp::mcp_message))
        .with_state(state)
}

async fn status(State(s): State<AppState>) -> Json<Value> {
    let targets = s.db.list_targets(None);
    Json(json!({
        "ok": true,
        "app": "ipscout",
        "projects": s.db.list_projects().len(),
        "targets": targets.len(),
        "runs": s.db.list_runs(None, 1000).len(),
    }))
}

async fn caps() -> Json<Value> {
    Json(capabilities())
}

#[derive(Deserialize)]
struct TargetQuery {
    target_id: Option<i64>,
    project_id: Option<i64>,
}

async fn dashboard(State(s): State<AppState>, Query(q): Query<TargetQuery>) -> Json<Value> {
    Json(dashboard_value(&s, q.target_id))
}

async fn list_projects(State(s): State<AppState>) -> Json<Value> {
    Json(json!({ "ok": true, "projects": s.db.list_projects() }))
}

async fn add_project(State(s): State<AppState>, Json(b): Json<ProjectIn>) -> Json<Value> {
    Json(add_project_value(&s, &b))
}

async fn delete_project(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    Json(match s.db.delete_project(id) {
        Ok(()) => json!({ "ok": true }),
        Err(e) => err(e),
    })
}

async fn list_targets(State(s): State<AppState>, Query(q): Query<TargetQuery>) -> Json<Value> {
    Json(json!({ "ok": true, "targets": s.db.list_targets(q.project_id) }))
}

async fn add_target(State(s): State<AppState>, Json(b): Json<TargetIn>) -> Json<Value> {
    Json(add_target_value(&s, &b))
}

async fn delete_target(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    Json(match s.db.delete_target(id) {
        Ok(()) => json!({ "ok": true }),
        Err(e) => err(e),
    })
}

async fn run_profile(State(s): State<AppState>, Json(b): Json<ProfileIn>) -> Json<Value> {
    Json(profile_value(&s, &b).await)
}

async fn run_scan(State(s): State<AppState>, Json(b): Json<ScanIn>) -> Json<Value> {
    Json(scan_value(&s, &b).await)
}

async fn run_trace(State(s): State<AppState>, Json(b): Json<TraceIn>) -> Json<Value> {
    Json(trace_value(&s, &b).await)
}

#[derive(Deserialize)]
struct RunsQuery {
    target_id: Option<i64>,
    limit: Option<i64>,
}

async fn list_runs(State(s): State<AppState>, Query(q): Query<RunsQuery>) -> Json<Value> {
    Json(json!({
        "ok": true,
        "runs": s.db.list_runs(q.target_id, q.limit.unwrap_or(50)),
    }))
}

async fn get_run(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    Json(match s.db.get_run(id) {
        Some(r) => json!({
            "ok": true, "run": r,
            "ports": s.db.ports_of(id),
            "findings": s.db.findings(Some(id), None, None),
        }),
        None => err("không có lần chạy này"),
    })
}

async fn findings(State(s): State<AppState>, Query(q): Query<FindingsQuery>) -> Json<Value> {
    Json(findings_value(&s, &q))
}

#[derive(Deserialize)]
struct DiffQuery {
    from_run: i64,
    to_run: i64,
}

async fn diff(State(s): State<AppState>, Query(q): Query<DiffQuery>) -> Json<Value> {
    Json(s.db.diff(q.from_run, q.to_run))
}

#[derive(Deserialize)]
struct LimitQuery {
    limit: Option<i64>,
}

async fn activity(State(s): State<AppState>, Query(q): Query<LimitQuery>) -> Json<Value> {
    Json(json!({
        "ok": true,
        "activity": s.db.activity(q.limit.unwrap_or(50)),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> AppState {
        let (mcp_tx, _) = tokio::sync::broadcast::channel(100);
        AppState {
            db: Arc::new(Db::open_memory().unwrap()),
            http: http_client(),
            sc: SpaceClient::from_env(),
            mcp_tx,
        }
    }

    #[test]
    fn a_target_is_stored_with_the_host_extracted_from_whatever_the_user_typed() {
        let s = state();
        let r = add_target_value(
            &s,
            &TargetIn {
                project_id: 1,
                target: "https://example.com:8443/path".into(),
                label: "web".into(),
            },
        );
        assert_eq!(r["ok"], true);
        assert_eq!(r["host"], "example.com");
        // chuỗi gốc vẫn giữ, để sau còn biết người dùng đã nhập gì
        let t = s.db.get_target(r["id"].as_i64().unwrap()).unwrap();
        assert_eq!(t["input"], "https://example.com:8443/path");
    }

    #[test]
    fn capabilities_declares_all_layers_and_what_the_app_refuses_to_do() {
        let c = capabilities();
        let layers = c["layers"].as_array().unwrap();
        assert_eq!(layers.len(), 3, "profile + ports + trace");
        // Cả ba lớp đều **không** đòi ownership sau khi bỏ verification.
        assert_eq!(layers[0]["sends_packets_to_target"], false); // profile: passive
        assert_eq!(layers[1]["sends_packets_to_target"], true); // ports: chạm mục tiêu
        assert_eq!(layers[2]["sends_packets_to_target"], true); // trace: chạm cả đường đi
        assert!(layers.iter().all(|l| l["requires_ownership"] == false));

        let never = c["never_does"].as_array().unwrap();
        assert!(never.len() >= 6);
        assert!(never.iter().any(|x| x.as_str().unwrap().contains("SYN")));
        assert!(never.iter().any(|x| x.as_str().unwrap().contains("hàng loạt")));
        // Chốt duy nhất còn lại phải nói ra để agent không đi quét metadata rồi
        // ngạc nhiên vì bị từ chối.
        assert!(never.iter().any(|x| x.as_str().unwrap().contains("metadata")));
        assert_eq!(c["limits"]["max_ports_per_scan"], scan::MAX_PORTS as i64);
    }

    #[test]
    fn the_dashboard_of_a_fresh_target_is_empty_but_well_formed() {
        let s = state();
        let id = add_target_value(
            &s,
            &TargetIn { project_id: 1, target: "example.com".into(), label: String::new() },
        )["id"]
            .as_i64()
            .unwrap();
        let d = dashboard_value(&s, Some(id));
        assert_eq!(d["ok"], true);
        assert_eq!(d["runs"], 0);
        assert!(d["latest_profile"].is_null());
        assert!(d["open_ports"].as_array().unwrap().is_empty());
        assert_eq!(d["severity_counts"]["critical"], 0);
    }

    #[test]
    fn no_target_id_gives_the_project_overview_instead_of_an_error() {
        let s = state();
        let d = dashboard_value(&s, None);
        assert_eq!(d["ok"], true);
        assert_eq!(d["projects"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn an_unparseable_target_is_rejected_rather_than_stored_empty() {
        let s = state();
        let r = add_target_value(
            &s,
            &TargetIn { project_id: 1, target: "   ".into(), label: String::new() },
        );
        assert_eq!(r["ok"], false);
        assert!(s.db.list_targets(None).is_empty());
    }
}
