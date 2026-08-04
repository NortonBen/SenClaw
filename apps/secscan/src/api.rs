//! REST + trạng thái dùng chung.
//!
//! Quy ước của repo: mọi tool MCP đều dựng cùng struct `*In` mà REST handler
//! deserialize, rồi gọi cùng hàm `*_value()`. Agent và người không thể lệch nhau.

use crate::db::Db;
use crate::{custom, scan, scope};
use app_space_sdk::SpaceClient;
use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::fmt::Display;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Db>,
    pub http: reqwest::Client,
    #[allow(dead_code)]
    pub sc: SpaceClient,
    /// Fan-out phản hồi JSON-RPC tới SSE client đang nối.
    pub mcp_tx: tokio::sync::broadcast::Sender<String>,
}

pub fn make_state() -> AppState {
    let db = Arc::new(Db::open_default().expect("mở secscan db"));
    let (mcp_tx, _) = tokio::sync::broadcast::channel(100);
    AppState {
        db,
        http: scan::http_client(),
        sc: SpaceClient::from_env(),
        mcp_tx,
    }
}

pub fn err(e: impl Display) -> Value {
    json!({ "ok": false, "error": e.to_string() })
}

// ---------------------------------------------------------------------------
// Tài sản
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct AssetIn {
    pub kind: String,
    pub target: String,
    #[serde(default)]
    pub label: String,
}

pub fn add_asset_value(s: &AppState, b: &AssetIn) -> Value {
    match s.db.add_asset(&b.kind, &b.target, &b.label) {
        Ok(id) => {
            s.db.log("asset", &format!("thêm tài sản {}", b.target), Some(id));
            json!({ "ok": true, "id": id })
        }
        Err(e) => err(e),
    }
}

#[derive(Deserialize)]
pub struct VerifyIn {
    pub asset_id: i64,
    pub method: String,
}

/// Sinh token và trả hướng dẫn — chưa kiểm gì cả.
pub fn verify_token_value(s: &AppState, b: &VerifyIn) -> Value {
    let Some(m) = scope::Method::parse(&b.method) else {
        return err("method phải là dns-txt | dns-cname | well-known | meta | local");
    };
    let Some(a) = s.db.get_asset(b.asset_id) else {
        return err("không có tài sản này");
    };
    let host = match scope::host_of(a["target"].as_str().unwrap_or_default()) {
        Ok(h) => h,
        Err(e) => return err(e),
    };
    let token = scope::gen_token(b.asset_id);
    if let Err(e) = s.db.set_asset_token(b.asset_id, m.as_str(), &token) {
        return err(e);
    }
    json!({
        "ok": true,
        "method": m.as_str(),
        "token": token,
        "instructions": m.instructions(&host, &token),
    })
}

/// Kiểm bằng chứng có thật sự tồn tại không.
pub async fn verify_run_value(s: &AppState, asset_id: i64) -> Value {
    let Some(a) = s.db.get_asset(asset_id) else {
        return err("không có tài sản này");
    };
    let (Some(method), Some(token)) = (
        a["verify_method"].as_str().and_then(scope::Method::parse),
        a["verify_token"].as_str(),
    ) else {
        return err("chưa sinh token — gọi sec_asset_verify_token trước");
    };
    let host = match scope::host_of(a["target"].as_str().unwrap_or_default()) {
        Ok(h) => h,
        Err(e) => return err(e),
    };
    match scope::verify(&s.http, method, &host, token).await {
        Ok(()) => {
            let _ = s.db.mark_verified(asset_id, true, None);
            s.db.log("asset", &format!("xác minh thành công {host}"), Some(asset_id));
            json!({ "ok": true, "verified": true })
        }
        Err(e) => {
            let msg = e.to_string();
            let _ = s.db.mark_verified(asset_id, false, Some(&msg));
            json!({ "ok": true, "verified": false, "error": msg })
        }
    }
}

// ---------------------------------------------------------------------------
// Quét
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ScanIn {
    pub asset_id: i64,
}

pub async fn scan_passive_value(s: &AppState, b: &ScanIn) -> Value {
    // L1 thụ động KHÔNG đòi xác minh — theo thông lệ Snyk/Pentest-Tools: quan
    // sát thụ động là thứ một trình duyệt bình thường cũng làm.
    match scan::scan_passive(&s.db, &s.http, b.asset_id).await {
        Ok(v) => v,
        Err(e) => err(e),
    }
}

/// Quét chủ động. **Không có cổng xác minh** — anh là người duy nhất của app,
/// tự chịu trách nhiệm target mình thêm. Rào SSRF (chỉ tài sản đã xác minh
/// bằng phương thức `local` mới chạm được dải nội bộ) VẪN CÒN trong `scan.rs`,
/// đó là chuyện khác: giữ scanner khỏi tự biến thành công cụ tấn công.
pub async fn scan_active_value(s: &AppState, b: &ScanIn) -> Value {
    match scan::scan_active(&s.db, &s.http, b.asset_id).await {
        Ok(v) => v,
        Err(e) => err(e),
    }
}

pub async fn scan_host_value(s: &AppState, asset_id: i64) -> Value {
    match scan::scan_host(&s.db, &s.http, asset_id).await {
        Ok(v) => v,
        Err(e) => err(e),
    }
}

/// Vẫn giữ — allow_local trong scan.rs kiểm nó để bật đường vào dải nội bộ.
pub fn require_verified(s: &AppState, asset_id: i64) -> Result<(), Value> {
    match s.db.get_asset(asset_id) {
        None => Err(err("không có tài sản này")),
        Some(a) if a["verified_at"].is_null() => Err(err(
            "tài sản chưa xác minh sở hữu — chỉ chạy được lớp thụ động (L1). \
             Gọi sec_asset_verify_token rồi sec_asset_verify trước.",
        )),
        Some(_) => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// Phát hiện
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
pub struct FindingsQuery {
    pub scan_id: Option<i64>,
    pub asset_id: Option<i64>,
    pub severity: Option<String>,
}

pub fn findings_value(s: &AppState, q: &FindingsQuery) -> Value {
    json!({
        "ok": true,
        "findings": s.db.findings(q.scan_id, q.asset_id, q.severity.as_deref()),
    })
}

#[derive(Deserialize)]
pub struct StatusIn {
    pub status: String,
    #[serde(default)]
    pub reason: Option<String>,
}

pub fn set_status_value(s: &AppState, id: i64, b: &StatusIn) -> Value {
    match s.db.set_finding_status(id, &b.status, b.reason.as_deref()) {
        Ok(()) => json!({ "ok": true }),
        Err(e) => err(e),
    }
}

#[derive(Deserialize)]
pub struct DiffQuery {
    pub from: i64,
    pub to: i64,
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Tổng hợp cho tab Tổng quan: xu hướng điểm, phân bố mức, và các mục nặng
/// nhất còn mở. Gộp một lượt để UI không phải gọi bốn endpoint rồi tự ghép.
pub fn dashboard_value(s: &AppState, asset_id: Option<i64>) -> Value {
    let assets = s.db.list_assets();
    let scans = s.db.list_scans(asset_id, 30);
    let done: Vec<&Value> = scans.iter().filter(|x| x["status"] == "done").collect();

    // Xu hướng theo thứ tự thời gian tăng dần (list_scans trả mới nhất trước).
    let trend: Vec<Value> = done
        .iter()
        .rev()
        .map(|x| json!({
            "scan_id": x["id"], "at": x["started_at"],
            "score": x["score"], "grade": x["grade"],
        }))
        .collect();

    let latest_id = done.first().and_then(|x| x["id"].as_i64());
    let findings = latest_id
        .map(|id| s.db.findings(Some(id), None, None))
        .unwrap_or_default();

    let mut by_sev = serde_json::Map::new();
    for sev in ["critical", "high", "medium", "low", "info"] {
        let n = findings.iter().filter(|f| f["severity"] == sev).count();
        by_sev.insert(sev.to_string(), json!(n));
    }
    let mut by_cat = serde_json::Map::new();
    for f in &findings {
        let c = f["category"].as_str().unwrap_or("khác").to_string();
        let e = by_cat.entry(c).or_insert(json!(0));
        *e = json!(e.as_u64().unwrap_or(0) + 1);
    }

    // Các mục đáng làm trước: còn mở, mức nặng, KEV lên đầu.
    let mut open: Vec<&Value> = findings
        .iter()
        .filter(|f| f["status"] == "open" || f["status"] == "regressed")
        .collect();
    let rank = |f: &Value| -> (u8, u8) {
        let kev = if f["kev"] == true { 0 } else { 1 };
        let sev = match f["severity"].as_str().unwrap_or("info") {
            "critical" => 0, "high" => 1, "medium" => 2, "low" => 3, _ => 4,
        };
        (kev, sev)
    };
    open.sort_by_key(|f| rank(f));

    json!({
        "ok": true,
        "assets_total": assets.len(),
        "assets_verified": assets.iter().filter(|a| !a["verified_at"].is_null()).count(),
        "scans_total": scans.len(),
        "trend": trend,
        "latest_scan_id": latest_id,
        "by_severity": by_sev,
        "by_category": by_cat,
        "top_open": open.into_iter().take(5).cloned().collect::<Vec<_>>(),
        "regressed": findings.iter().filter(|f| f["status"] == "regressed").count(),
        "acked": findings.iter().filter(|f| f["status"] == "acked").count(),
    })
}

// ---------------------------------------------------------------------------
// Cài đặt luật: tự thêm, nhập từ nguồn ngoài, ghi đè luật dựng sẵn
// ---------------------------------------------------------------------------

/// Thêm/sửa luật tự viết. Kiểm hợp lệ TRƯỚC khi lưu — luật hỏng nằm trong DB sẽ
/// im lặng không chạy, và người thêm không bao giờ biết vì sao.
pub fn rule_add_value(s: &AppState, body: &Value) -> Value {
    let rule: custom::CustomRule = match serde_json::from_value(body.clone()) {
        Ok(r) => r,
        Err(e) => return err(format!("cấu trúc luật sai: {e}")),
    };
    if let Err(e) = rule.validate() {
        return err(e);
    }
    let text = match serde_json::to_string(&rule) {
        Ok(t) => t,
        Err(e) => return err(e),
    };
    match s.db.put_custom_rule(&rule.id, &text, "manual") {
        Ok(()) => {
            s.db.log("rule", &format!("thêm luật {}", rule.id), None);
            json!({ "ok": true, "id": rule.id })
        }
        Err(e) => err(e),
    }
}

pub fn rule_remove_value(s: &AppState, id: &str) -> Value {
    match s.db.delete_custom_rule(id) {
        Ok(0) => err(format!("không có luật '{id}'")),
        Ok(_) => {
            s.db.log("rule", &format!("xoá luật {id}"), None);
            json!({ "ok": true })
        }
        Err(e) => err(e),
    }
}

pub fn custom_rules_value(s: &AppState) -> Value {
    let rules: Vec<Value> = s
        .db
        .custom_rules_raw()
        .iter()
        .filter_map(|j| serde_json::from_str::<Value>(j).ok())
        .collect();
    let ov: Vec<Value> = s
        .db
        .overrides()
        .into_iter()
        .map(|(id, sev, en, note)| json!({
            "rule_id": id, "severity": sev, "enabled": en, "note": note
        }))
        .collect();
    json!({ "ok": true, "custom": rules, "overrides": ov })
}

#[derive(Deserialize)]
pub struct ImportIn {
    /// Nguồn https://… — đi qua bộ chặn SSRF như mọi đích quét khác.
    #[serde(default)]
    pub url: Option<String>,
    /// Hoặc dán thẳng JSON vào đây.
    #[serde(default)]
    pub json: Option<String>,
    /// Mặc định CHỈ XEM TRƯỚC. Nạp nội dung từ nguồn ngoài không được tự đổi
    /// hành vi quét — người dùng phải nhìn thấy sẽ thêm gì rồi mới đồng ý.
    #[serde(default)]
    pub apply: bool,
}

pub async fn rule_import_value(s: &AppState, b: &ImportIn) -> Value {
    let (body, source) = if let Some(url) = b.url.as_deref().filter(|x| !x.trim().is_empty()) {
        match custom::fetch_ruleset(&s.http, url).await {
            Ok(t) => (t, url.to_string()),
            Err(e) => return err(e),
        }
    } else if let Some(j) = b.json.as_deref().filter(|x| !x.trim().is_empty()) {
        (j.to_string(), "manual".to_string())
    } else {
        return err("cần 'url' hoặc 'json'");
    };

    let mut report = match custom::parse_ruleset(&body, &source) {
        Ok(r) => r,
        Err(e) => return err(e),
    };

    if b.apply {
        for r in &report.valid {
            let Ok(text) = serde_json::to_string(r) else { continue };
            let _ = s.db.put_custom_rule(&r.id, &text, &source);
        }
        report.applied = true;
        s.db.log(
            "rule",
            &format!("nhập {} luật từ {}", report.valid.len(), source),
            None,
        );
    }
    custom::to_json(&report)
}

#[derive(Deserialize)]
pub struct OverrideIn {
    pub rule_id: String,
    #[serde(default)]
    pub severity: Option<String>,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub note: Option<String>,
}

fn yes() -> bool {
    true
}

pub fn override_set_value(s: &AppState, b: &OverrideIn) -> Value {
    if let Some(sev) = b.severity.as_deref() {
        if !["critical", "high", "medium", "low", "info"].contains(&sev) {
            return err(format!("mức '{sev}' không hợp lệ"));
        }
    }
    // Ghi đè bằng chính giá trị mặc định thì xoá luôn cho sổ khỏi rác.
    if b.severity.is_none() && b.enabled && b.note.is_none() {
        let _ = s.db.clear_override(&b.rule_id);
        return json!({ "ok": true, "cleared": true });
    }
    match s.db.set_override(&b.rule_id, b.severity.as_deref(), b.enabled, b.note.as_deref()) {
        Ok(()) => {
            s.db.log("rule", &format!("ghi đè luật {}", b.rule_id), None);
            json!({ "ok": true })
        }
        Err(e) => err(e),
    }
}

pub fn api_router(state: AppState) -> Router {
    Router::new()
        .route("/status", get(status))
        .route("/rules", get(rules))
        .route("/dashboard", get(dashboard))
        .route("/assets", get(list_assets).post(add_asset))
        .route("/assets/:id/verify-token", post(verify_token))
        .route("/assets/:id/verify", post(verify_run))
        .route("/assets/:id/delete", post(delete_asset))
        .route("/scan/passive", post(scan_passive))
        .route("/scan/active", post(scan_active))
        .route("/scan/host", post(scan_host))
        .route("/scans", get(list_scans))
        .route("/scans/:id", get(get_scan))
        .route("/findings", get(findings))
        .route("/findings/:id/status", post(set_status))
        .route("/diff", get(diff))
        .route("/activity", get(activity))
        .route("/settings", get(get_settings).post(set_settings))
        .route("/settings/rules", get(list_custom_rules).post(add_custom_rule))
        .route("/settings/rules/import", post(import_rules))
        .route("/settings/rules/:id/delete", post(delete_custom_rule))
        .route("/settings/overrides", post(set_override))
        .route("/mcp/sse", get(crate::mcp::mcp_sse).post(crate::mcp::mcp_message))
        .route("/mcp/message", post(crate::mcp::mcp_message))
        .with_state(state)
}

async fn status(State(s): State<AppState>) -> Json<Value> {
    let assets = s.db.list_assets();
    let verified = assets.iter().filter(|a| !a["verified_at"].is_null()).count();
    Json(json!({
        "ok": true,
        "app": "secscan",
        "assets": assets.len(),
        "verified": verified,
        "scans": s.db.list_scans(None, 1000).len(),
    }))
}

async fn rules(State(_s): State<AppState>) -> Json<Value> {
    Json(crate::rules::to_json())
}

#[derive(Deserialize)]
struct DashQuery {
    asset_id: Option<i64>,
}

async fn dashboard(State(s): State<AppState>, Query(q): Query<DashQuery>) -> Json<Value> {
    Json(dashboard_value(&s, q.asset_id))
}

async fn list_assets(State(s): State<AppState>) -> Json<Value> {
    Json(json!({ "ok": true, "assets": s.db.list_assets() }))
}

async fn add_asset(State(s): State<AppState>, Json(b): Json<AssetIn>) -> Json<Value> {
    Json(add_asset_value(&s, &b))
}

#[derive(Deserialize)]
struct MethodBody {
    method: String,
}

async fn verify_token(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Json(b): Json<MethodBody>,
) -> Json<Value> {
    Json(verify_token_value(
        &s,
        &VerifyIn {
            asset_id: id,
            method: b.method,
        },
    ))
}

async fn verify_run(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    Json(verify_run_value(&s, id).await)
}

async fn delete_asset(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    Json(match s.db.delete_asset(id) {
        Ok(()) => json!({ "ok": true }),
        Err(e) => err(e),
    })
}

async fn scan_passive(State(s): State<AppState>, Json(b): Json<ScanIn>) -> Json<Value> {
    Json(scan_passive_value(&s, &b).await)
}

#[derive(Deserialize)]
struct ScansQuery {
    asset_id: Option<i64>,
    limit: Option<i64>,
}

async fn scan_active(State(s): State<AppState>, Json(b): Json<ScanIn>) -> Json<Value> {
    Json(scan_active_value(&s, &b).await)
}

async fn scan_host(State(s): State<AppState>, Json(b): Json<ScanIn>) -> Json<Value> {
    Json(scan_host_value(&s, b.asset_id).await)
}

async fn list_scans(State(s): State<AppState>, Query(q): Query<ScansQuery>) -> Json<Value> {
    Json(json!({
        "ok": true,
        "scans": s.db.list_scans(q.asset_id, q.limit.unwrap_or(50)),
    }))
}

async fn get_scan(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    Json(match s.db.get_scan(id) {
        Some(v) => json!({ "ok": true, "scan": v, "findings": s.db.findings(Some(id), None, None) }),
        None => err("không có lần quét này"),
    })
}

async fn findings(State(s): State<AppState>, Query(q): Query<FindingsQuery>) -> Json<Value> {
    Json(findings_value(&s, &q))
}

async fn set_status(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Json(b): Json<StatusIn>,
) -> Json<Value> {
    Json(set_status_value(&s, id, &b))
}

async fn diff(State(s): State<AppState>, Query(q): Query<DiffQuery>) -> Json<Value> {
    Json(s.db.diff(q.from, q.to))
}

#[derive(Deserialize)]
struct ActivityQuery {
    limit: Option<i64>,
}

async fn activity(State(s): State<AppState>, Query(q): Query<ActivityQuery>) -> Json<Value> {
    Json(json!({ "ok": true, "activity": s.db.activity(q.limit.unwrap_or(50)) }))
}

async fn list_custom_rules(State(s): State<AppState>) -> Json<Value> {
    Json(custom_rules_value(&s))
}

async fn add_custom_rule(State(s): State<AppState>, Json(b): Json<Value>) -> Json<Value> {
    Json(rule_add_value(&s, &b))
}

async fn delete_custom_rule(State(s): State<AppState>, Path(id): Path<String>) -> Json<Value> {
    Json(rule_remove_value(&s, &id))
}

async fn import_rules(State(s): State<AppState>, Json(b): Json<ImportIn>) -> Json<Value> {
    Json(rule_import_value(&s, &b).await)
}

async fn set_override(State(s): State<AppState>, Json(b): Json<OverrideIn>) -> Json<Value> {
    Json(override_set_value(&s, &b))
}

async fn get_settings(State(s): State<AppState>) -> Json<Value> {
    Json(json!({ "ok": true, "settings": s.db.settings() }))
}

async fn set_settings(State(s): State<AppState>, Json(b): Json<Value>) -> Json<Value> {
    if let Some(obj) = b.as_object() {
        for (k, v) in obj {
            let val = v.as_str().map(|x| x.to_string()).unwrap_or_else(|| v.to_string());
            if let Err(e) = s.db.set_setting(k, &val) {
                return Json(err(e));
            }
        }
    }
    Json(json!({ "ok": true }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn state() -> AppState {
        let (mcp_tx, _) = tokio::sync::broadcast::channel(100);
        AppState {
            db: Arc::new(Db::open_memory().unwrap()),
            http: scan::http_client(),
            sc: SpaceClient::from_env(),
            mcp_tx,
        }
    }

    #[test]
    fn unverified_assets_can_be_scanned_at_every_layer() {
        // Quyết định thiết kế: SenClaw là AI cá nhân, người dùng tự chịu trách
        // nhiệm target mình thêm — không có cổng xác minh trước khi quét.
        // Cái CÒN LẠI là rào SSRF trong scan.rs: dải nội bộ chỉ được chạm khi
        // đã xác minh bằng phương thức 'local'. Test đó nằm trong scan::tests.
        let s = state();
        let id = s.db.add_asset("website", "https://a.vn", "").unwrap();
        // require_verified vẫn tồn tại như tín hiệu, không phải cổng chặn
        assert!(require_verified(&s, id).is_err(), "hàm vẫn phân biệt được trạng thái");
        // nhưng scan_active_value KHÔNG còn gọi nó nữa — không có test khoá cứng
        // hành vi 'phải xác minh trước khi quét' vì hành vi đó đã bị gỡ chủ ý.
    }

    #[test]
    fn verify_token_rejects_unknown_method() {
        let s = state();
        let id = s.db.add_asset("website", "https://a.vn", "").unwrap();
        // 'email' cố tình không hỗ trợ
        let v = verify_token_value(&s, &VerifyIn { asset_id: id, method: "email".into() });
        assert_eq!(v["ok"], false);
    }

    #[test]
    fn verify_token_persists_and_returns_instructions() {
        let s = state();
        let id = s.db.add_asset("website", "https://a.vn", "").unwrap();
        let v = verify_token_value(&s, &VerifyIn { asset_id: id, method: "dns-txt".into() });
        assert_eq!(v["ok"], true);
        let token = v["token"].as_str().unwrap();
        assert_eq!(token.len(), 32);
        assert!(v["instructions"].as_str().unwrap().contains(token));
        // token phải được lưu để bước verify sau đọc lại
        let a = s.db.get_asset(id).unwrap();
        assert_eq!(a["verify_token"], token);
        assert_eq!(a["verify_method"], "dns-txt");
        // sinh token KHÔNG được coi là đã xác minh
        assert!(a["verified_at"].is_null());
    }

    #[test]
    fn adding_a_rule_validates_before_storing() {
        let s = state();
        // Luật hỏng KHÔNG được vào DB — nằm trong đó nó sẽ im lặng không chạy.
        let bad = json!({
            "id": "custom:r", "title": "t", "severity": "medium",
            "check": { "target": "header", "name": "x", "op": "regex", "value": "[bad" }
        });
        assert_eq!(rule_add_value(&s, &bad)["ok"], false);
        assert!(s.db.custom_rules_raw().is_empty(), "luật hỏng không được lưu");

        let good = json!({
            "id": "custom:r", "title": "t", "severity": "medium",
            "check": { "target": "header", "name": "x-req", "op": "present" }
        });
        assert_eq!(rule_add_value(&s, &good)["ok"], true);
        assert_eq!(s.db.custom_rules_raw().len(), 1);
    }

    #[tokio::test]
    async fn import_without_apply_changes_nothing() {
        let s = state();
        let b = ImportIn {
            url: None,
            json: Some(r#"[{"id":"custom:a","title":"A","severity":"low",
                            "check":{"target":"header","name":"x","op":"present"}}]"#.into()),
            apply: false,
        };
        let v = rule_import_value(&s, &b).await;
        assert_eq!(v["accepted"], 1);
        assert_eq!(v["applied"], false);
        assert!(s.db.custom_rules_raw().is_empty(), "xem trước không được lưu gì");

        let b2 = ImportIn { apply: true, ..b };
        assert_eq!(rule_import_value(&s, &b2).await["applied"], true);
        assert_eq!(s.db.custom_rules_raw().len(), 1);
    }

    #[test]
    fn override_with_all_defaults_clears_instead_of_storing_noise() {
        let s = state();
        override_set_value(&s, &OverrideIn {
            rule_id: "hdr:csp".into(), severity: Some("low".into()), enabled: true, note: None,
        });
        assert_eq!(s.db.overrides().len(), 1);

        // đặt lại về mặc định -> xoá hẳn dòng ghi đè
        let v = override_set_value(&s, &OverrideIn {
            rule_id: "hdr:csp".into(), severity: None, enabled: true, note: None,
        });
        assert_eq!(v["cleared"], true);
        assert!(s.db.overrides().is_empty());
    }

    #[test]
    fn override_rejects_an_unknown_severity() {
        let s = state();
        let v = override_set_value(&s, &OverrideIn {
            rule_id: "hdr:csp".into(), severity: Some("nonsense".into()), enabled: true, note: None,
        });
        assert_eq!(v["ok"], false);
        assert!(s.db.overrides().is_empty());
    }

    #[test]
    fn add_asset_reports_duplicate_instead_of_silently_succeeding() {
        let s = state();
        let b = AssetIn { kind: "website".into(), target: "https://a.vn".into(), label: String::new() };
        assert_eq!(add_asset_value(&s, &b)["ok"], true);
        assert_eq!(add_asset_value(&s, &b)["ok"], false);
    }
}
