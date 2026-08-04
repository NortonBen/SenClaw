//! HTTP API of the TikTok Downloader app. Every handler funnels through
//! `*_value` helpers that the MCP server ([`crate::mcp`]) reuses, so REST and
//! agent tools always behave identically. Downloads themselves run in the
//! worker pool ([`crate::download`]) — enqueue endpoints return immediately
//! and the UI polls `/downloads`.

use crate::db::{data_dir, Db};
use crate::download::{Ctx, Queue};
use crate::tiktok::{self, Resolver};
use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Db>,
    pub http: reqwest::Client,
    pub resolver: Arc<Resolver>,
    pub queue: Arc<Queue>,
    /// Fan-out of MCP JSON-RPC responses to any connected SSE client.
    pub mcp_tx: tokio::sync::broadcast::Sender<String>,
}

pub fn make_state() -> AppState {
    let db = Arc::new(Db::open_default().expect("open tiktok-dl db"));
    let http = tiktok::http_client();
    let (mcp_tx, _) = tokio::sync::broadcast::channel(100);
    AppState {
        db,
        http: http.clone(),
        resolver: Arc::new(Resolver::new(http)),
        queue: Arc::new(Queue::new()),
        mcp_tx,
    }
}

impl AppState {
    pub fn worker_ctx(&self) -> Arc<Ctx> {
        Arc::new(Ctx {
            db: self.db.clone(),
            http: self.http.clone(),
            resolver: self.resolver.clone(),
            queue: self.queue.clone(),
        })
    }
}

pub fn api_router(state: AppState) -> Router {
    Router::new()
        .route("/status", get(status))
        .route("/resolve", post(resolve))
        .route("/download", post(download))
        .route("/download/batch", post(download_batch))
        .route("/downloads", get(list_downloads))
        .route("/downloads/clear", post(clear_downloads))
        .route("/downloads/:id", get(get_download))
        .route("/downloads/:id/cancel", post(cancel_download))
        .route("/downloads/:id/retry", post(retry_download))
        .route("/downloads/:id/delete", post(delete_download))
        .route("/downloads/:id/thumb", get(thumb))
        .route("/downloads/:id/file", get(serve_file))
        .route("/downloads/:id/open", post(open_download))
        .route("/profile/feed", post(profile_feed))
        .route("/profile/download", post(profile_download))
        .route("/avatar", post(avatar))
        .route("/settings", get(get_settings).post(set_settings))
        .route("/settings/open_dir", post(open_download_dir))
        .route("/activity", get(activity))
        // MCP (HTTP + SSE), same shape as the other Space Apps.
        .route(
            "/mcp/sse",
            get(crate::mcp::mcp_sse).post(crate::mcp::mcp_message),
        )
        .route("/mcp/message", post(crate::mcp::mcp_message))
        .with_state(state)
}

// ---- status ----

pub(crate) fn status_value(s: &AppState) -> Value {
    json!({
        "ok": true,
        "app": "tiktok-dl",
        "counters": s.db.counters(),
        "download_dir": s.db.setting("download_dir", ""),
        "default_quality": s.db.setting("default_quality", "nowm"),
        "max_concurrent": s.db.setting("max_concurrent", "2"),
    })
}

async fn status(State(s): State<AppState>) -> Json<Value> {
    Json(status_value(&s))
}

// ---- resolve (preview, no download) ----

pub(crate) async fn resolve_value(s: &AppState, text: &str) -> Value {
    let urls = tiktok::extract_urls(text);
    let Some(url) = urls.first() else {
        return json!({ "error": "không thấy link TikTok nào trong nội dung gửi lên" });
    };
    match s.resolver.resolve(url).await {
        Ok(meta) => json!({ "ok": true, "url": url, "meta": meta }),
        Err(e) => json!({ "error": e.to_string(), "url": url }),
    }
}

#[derive(Deserialize)]
pub struct ResolveIn {
    pub url: String,
}

async fn resolve(State(s): State<AppState>, Json(b): Json<ResolveIn>) -> Json<Value> {
    Json(resolve_value(&s, &b.url).await)
}

// ---- enqueue single / batch ----

fn normalize_quality(s: &AppState, q: &str) -> String {
    match q {
        "nowm" | "hd" | "wm" | "audio" | "avatar" => q.to_string(),
        "" => s.db.setting("default_quality", "nowm"),
        _ => "nowm".to_string(),
    }
}

/// Enqueue one URL. `meta` (a resolved snapshot from a preceding `/resolve`)
/// is optional sugar so the queue row shows title/cover instantly.
pub(crate) fn download_value(
    s: &AppState,
    url: &str,
    quality: &str,
    force: bool,
    meta: Option<&Value>,
) -> Value {
    let urls = tiktok::extract_urls(url);
    let Some(url) = urls.first() else {
        return json!({ "error": "link không hợp lệ — cần link tiktok.com / vm.tiktok.com / douyin.com" });
    };
    let quality = normalize_quality(s, quality);
    if !force {
        if let Some(dup) = s.db.find_done_duplicate(url, &quality) {
            return json!({
                "duplicate": true,
                "existing_id": dup,
                "message": format!("link này đã tải xong trước đó (bản ghi #{dup}) — truyền force=true để tải lại"),
            });
        }
    }
    match s.db.enqueue(url, &quality, meta) {
        Ok(id) => {
            s.db.log("queued", &format!("Xếp hàng tải ({quality})"), &id.to_string());
            s.queue.wake();
            json!({ "ok": true, "download": s.db.get_download(id) })
        }
        Err(e) => json!({ "error": format!("không xếp hàng được: {e}") }),
    }
}

#[derive(Deserialize)]
pub struct DownloadIn {
    pub url: String,
    #[serde(default)]
    pub quality: String,
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub meta: Option<Value>,
}

async fn download(State(s): State<AppState>, Json(b): Json<DownloadIn>) -> Json<Value> {
    Json(download_value(&s, &b.url, &b.quality, b.force, b.meta.as_ref()))
}

/// Batch: pull every TikTok link out of a text blob (or explicit list), skip
/// duplicates already downloaded, queue the rest. Capped so a runaway paste
/// cannot enqueue thousands of jobs.
pub(crate) fn batch_value(s: &AppState, text: &str, quality: &str, force: bool) -> Value {
    const MAX_BATCH: usize = 200;
    let mut urls = tiktok::extract_urls(text);
    let over = urls.len().saturating_sub(MAX_BATCH);
    urls.truncate(MAX_BATCH);
    if urls.is_empty() {
        return json!({ "error": "không thấy link TikTok nào trong nội dung gửi lên" });
    }
    let quality = normalize_quality(s, quality);
    let (mut queued, mut skipped) = (Vec::new(), 0usize);
    for url in &urls {
        if !force && s.db.find_done_duplicate(url, &quality).is_some() {
            skipped += 1;
            continue;
        }
        if let Ok(id) = s.db.enqueue(url, &quality, None) {
            queued.push(id);
        }
    }
    if !queued.is_empty() {
        s.db.log(
            "queued",
            &format!("Xếp hàng {} link (bỏ qua {} đã tải)", queued.len(), skipped),
            "",
        );
        s.queue.wake();
    }
    json!({
        "ok": true,
        "queued": queued.len(),
        "skipped_duplicates": skipped,
        "dropped_over_limit": over,
        "ids": queued,
    })
}

#[derive(Deserialize)]
pub struct BatchIn {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub urls: Vec<String>,
    #[serde(default)]
    pub quality: String,
    #[serde(default)]
    pub force: bool,
}

async fn download_batch(State(s): State<AppState>, Json(b): Json<BatchIn>) -> Json<Value> {
    let mut text = b.text;
    if !b.urls.is_empty() {
        text.push('\n');
        text.push_str(&b.urls.join("\n"));
    }
    Json(batch_value(&s, &text, &b.quality, b.force))
}

// ---- listing / detail ----

#[derive(Deserialize, Default)]
pub struct ListQuery {
    pub q: Option<String>,
    pub status: Option<String>,
    pub kind: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub(crate) fn list_value(s: &AppState, q: &ListQuery) -> Value {
    let items = s.db.list_downloads(
        q.q.as_deref(),
        q.status.as_deref(),
        q.kind.as_deref(),
        q.limit.unwrap_or(50),
        q.offset.unwrap_or(0),
    );
    json!({ "ok": true, "counters": s.db.counters(), "downloads": items })
}

async fn list_downloads(State(s): State<AppState>, Query(q): Query<ListQuery>) -> Json<Value> {
    Json(list_value(&s, &q))
}

pub(crate) fn get_value(s: &AppState, id: i64) -> Value {
    match s.db.get_download(id) {
        Some(d) => json!({ "ok": true, "download": d }),
        None => json!({ "error": format!("bản ghi #{id} không tồn tại") }),
    }
}

async fn get_download(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    Json(get_value(&s, id))
}

// ---- cancel / retry / delete / clear ----

pub(crate) fn cancel_value(s: &AppState, id: i64) -> Value {
    if !s.db.is_active(id) {
        return json!({ "error": format!("bản ghi #{id} không ở trạng thái đang tải/đang chờ") });
    }
    // A worker owns it → flag; still queued → cancel directly in the DB.
    if !s.queue.request_cancel(id) {
        s.db.set_status(id, "canceled", "");
    }
    json!({ "ok": true, "download": s.db.get_download(id) })
}

async fn cancel_download(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    Json(cancel_value(&s, id))
}

pub(crate) fn retry_value(s: &AppState, id: i64) -> Value {
    if s.db.requeue(id) {
        s.db.log("queued", "Tải lại", &id.to_string());
        s.queue.wake();
        json!({ "ok": true, "download": s.db.get_download(id) })
    } else {
        json!({ "error": format!("bản ghi #{id} không tải lại được (đang chạy hoặc không tồn tại)") })
    }
}

async fn retry_download(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    Json(retry_value(&s, id))
}

fn remove_files_of(row: &Value) -> usize {
    let mut n = 0;
    let files: Vec<String> = row["files"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|f| f.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    for f in &files {
        if std::fs::remove_file(f).is_ok() {
            n += 1;
        }
    }
    // Multi-file jobs own their folder — remove it when it emptied out.
    if let Some(dir) = row["dir"].as_str() {
        if !dir.is_empty() && files.len() > 1 {
            let _ = std::fs::remove_dir(dir);
        }
    }
    let _ = std::fs::remove_file(
        data_dir()
            .join("thumbs")
            .join(format!("{}.jpg", row["id"].as_i64().unwrap_or(0))),
    );
    n
}

pub(crate) fn delete_value(s: &AppState, id: i64, with_file: bool) -> Value {
    if s.db.is_active(id) {
        return json!({ "error": "đang tải — hãy hủy trước rồi mới xoá" });
    }
    match s.db.delete_download(id) {
        Some(row) => {
            let removed = if with_file { remove_files_of(&row) } else { 0 };
            s.db.log("deleted", &format!("Xoá bản ghi (files: {removed})"), &id.to_string());
            json!({ "ok": true, "removed_files": removed })
        }
        None => json!({ "error": format!("bản ghi #{id} không tồn tại") }),
    }
}

#[derive(Deserialize, Default)]
pub struct DeleteIn {
    #[serde(default)]
    pub with_file: bool,
}

async fn delete_download(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Json(b): Json<DeleteIn>,
) -> Json<Value> {
    Json(delete_value(&s, id, b.with_file))
}

pub(crate) fn clear_value(s: &AppState, status: Option<&str>, with_files: bool) -> Value {
    let rows = s.db.clear_downloads(status);
    let mut removed = 0;
    if with_files {
        for r in &rows {
            removed += remove_files_of(r);
        }
    }
    s.db.log("cleared", &format!("Dọn {} bản ghi lịch sử", rows.len()), "");
    json!({ "ok": true, "cleared": rows.len(), "removed_files": removed })
}

#[derive(Deserialize, Default)]
pub struct ClearIn {
    pub status: Option<String>,
    #[serde(default)]
    pub with_files: bool,
}

async fn clear_downloads(State(s): State<AppState>, Json(b): Json<ClearIn>) -> Json<Value> {
    Json(clear_value(&s, b.status.as_deref(), b.with_files))
}

// ---- thumb / file / open ----

async fn thumb(State(_s): State<AppState>, Path(id): Path<i64>) -> Response {
    let p = data_dir().join("thumbs").join(format!("{id}.jpg"));
    match tokio::fs::read(&p).await {
        Ok(bytes) => ([(header::CONTENT_TYPE, "image/jpeg")], bytes).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

#[derive(Deserialize, Default)]
pub struct FileQuery {
    #[serde(default)]
    pub i: usize,
}

/// Stream a finished file back to the browser (lets the UI offer "save/play"
/// even when SenClaw runs on another machine). Paths come from the DB only —
/// never from the request.
async fn serve_file(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Query(q): Query<FileQuery>,
) -> Response {
    let Some(row) = s.db.get_download(id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(path) = row["files"].as_array().and_then(|a| a.get(q.i)).and_then(|f| f.as_str())
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(file) = tokio::fs::File::open(path).await else {
        return (StatusCode::GONE, "file đã bị xoá khỏi đĩa").into_response();
    };
    let name = std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "tiktok".into());
    let ctype = match name.rsplit('.').next() {
        Some("mp4") => "video/mp4",
        Some("mp3") => "audio/mpeg",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("json") => "application/json",
        _ => "application/octet-stream",
    };
    let stream = tokio_util::io::ReaderStream::new(file);
    (
        [
            (header::CONTENT_TYPE, ctype.to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("inline; filename*=UTF-8''{}", urlencode(&name)),
            ),
        ],
        Body::from_stream(stream),
    )
        .into_response()
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'-' | b'_' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Open the downloaded file's folder (or reveal the file) on the machine the
/// daemon runs on. Desktop-app convenience, macOS/Linux only.
pub(crate) fn open_value(s: &AppState, id: i64, reveal: bool) -> Value {
    let Some(row) = s.db.get_download(id) else {
        return json!({ "error": format!("bản ghi #{id} không tồn tại") });
    };
    let first = row["files"][0].as_str().unwrap_or("");
    let dir = row["dir"].as_str().unwrap_or("");
    let target = if reveal && !first.is_empty() { first } else if !dir.is_empty() { dir } else { first };
    if target.is_empty() {
        return json!({ "error": "bản ghi chưa có file trên đĩa" });
    }
    let status = if cfg!(target_os = "macos") {
        let mut cmd = std::process::Command::new("open");
        if reveal && !first.is_empty() {
            cmd.arg("-R");
        }
        cmd.arg(target).status()
    } else {
        std::process::Command::new("xdg-open").arg(target).status()
    };
    match status {
        Ok(st) if st.success() => json!({ "ok": true, "opened": target }),
        Ok(st) => json!({ "error": format!("lệnh mở thư mục trả mã {st}") }),
        Err(e) => json!({ "error": format!("không mở được: {e}") }),
    }
}

#[derive(Deserialize, Default)]
pub struct OpenIn {
    #[serde(default)]
    pub reveal: bool,
}

async fn open_download(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Json(b): Json<OpenIn>,
) -> Json<Value> {
    Json(open_value(&s, id, b.reveal))
}

// ---- profile ----

pub(crate) async fn profile_feed_value(
    s: &AppState,
    unique_id: &str,
    count: i64,
    cursor: &str,
) -> Value {
    match s.resolver.user_posts(unique_id, count, cursor).await {
        Ok(v) => json!({ "ok": true, "feed": v }),
        Err(e) => json!({
            "error": e.to_string(),
            "hint": "Nguồn dữ liệu profile hay bị chặn hơn link lẻ. Nếu lỗi lặp lại: mở trang cá nhân, copy link các video và dùng tải hàng loạt.",
        }),
    }
}

#[derive(Deserialize)]
pub struct ProfileFeedIn {
    pub unique_id: String,
    #[serde(default)]
    pub count: Option<i64>,
    #[serde(default)]
    pub cursor: String,
}

async fn profile_feed(State(s): State<AppState>, Json(b): Json<ProfileFeedIn>) -> Json<Value> {
    Json(profile_feed_value(&s, &b.unique_id, b.count.unwrap_or(30), &b.cursor).await)
}

/// Fetch up to `max` newest posts of a profile and queue them all. Pages
/// through the feed (≤34/page) until `max` or the feed dries up.
pub(crate) async fn profile_download_value(
    s: &AppState,
    unique_id: &str,
    max: i64,
    quality: &str,
) -> Value {
    let max = if max <= 0 {
        s.db.setting("profile_max", "30").parse().unwrap_or(30)
    } else {
        max
    }
    .clamp(1, 200);
    let mut cursor = String::new();
    let mut urls: Vec<String> = Vec::new();
    for _ in 0..8 {
        let page = match s.resolver.user_posts(unique_id, 34, &cursor).await {
            Ok(p) => p,
            Err(e) => {
                if urls.is_empty() {
                    return json!({
                        "error": e.to_string(),
                        "hint": "Nguồn dữ liệu profile hay bị chặn hơn link lẻ. Hãy copy link video và dùng tải hàng loạt.",
                    });
                }
                break; // keep what we already collected
            }
        };
        for v in page["videos"].as_array().unwrap_or(&vec![]) {
            if let Some(u) = v["url"].as_str() {
                urls.push(u.to_string());
                if urls.len() as i64 >= max {
                    break;
                }
            }
        }
        if urls.len() as i64 >= max || !page["has_more"].as_bool().unwrap_or(false) {
            break;
        }
        cursor = page["cursor"].as_str().unwrap_or("").to_string();
        if cursor.is_empty() {
            break;
        }
    }
    if urls.is_empty() {
        return json!({ "error": "không lấy được video nào từ profile này" });
    }
    let text = urls.join("\n");
    let mut out = batch_value(s, &text, quality, false);
    out["profile"] = json!(unique_id.trim().trim_start_matches('@'));
    out["found"] = json!(urls.len());
    out
}

#[derive(Deserialize)]
pub struct ProfileDlIn {
    pub unique_id: String,
    #[serde(default)]
    pub max: i64,
    #[serde(default)]
    pub quality: String,
}

async fn profile_download(State(s): State<AppState>, Json(b): Json<ProfileDlIn>) -> Json<Value> {
    Json(profile_download_value(&s, &b.unique_id, b.max, &b.quality).await)
}

/// Avatar of the author of any post link (there is no public profile→avatar
/// endpoint that survives Cloudflare, so it rides on a post resolve).
pub(crate) fn avatar_value(s: &AppState, url: &str) -> Value {
    download_value(s, url, "avatar", false, None)
}

async fn avatar(State(s): State<AppState>, Json(b): Json<ResolveIn>) -> Json<Value> {
    Json(avatar_value(&s, &b.url))
}

// ---- settings / activity ----

const EDITABLE_SETTINGS: &[&str] = &[
    "download_dir",
    "default_quality",
    "filename_template",
    "max_concurrent",
    "photo_audio",
    "save_meta_json",
    "profile_max",
];

pub(crate) fn settings_value(s: &AppState) -> Value {
    json!({ "ok": true, "settings": s.db.all_settings() })
}

/// Patch-style: only known keys change; values are validated enough to keep
/// the worker sane (concurrency bounds, quality enum, non-empty dir).
pub(crate) fn set_settings_value(s: &AppState, patch: &Value) -> Value {
    let Some(obj) = patch.as_object() else {
        return json!({ "error": "body phải là object {key: value}" });
    };
    let mut changed = Vec::new();
    for (k, v) in obj {
        if !EDITABLE_SETTINGS.contains(&k.as_str()) {
            continue;
        }
        let val = match v {
            Value::String(x) => x.clone(),
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => if *b { "1" } else { "0" }.to_string(),
            _ => continue,
        };
        let val = match k.as_str() {
            "max_concurrent" => val
                .parse::<i64>()
                .map(|n| n.clamp(1, 4).to_string())
                .unwrap_or_else(|_| "2".into()),
            "profile_max" => val
                .parse::<i64>()
                .map(|n| n.clamp(1, 200).to_string())
                .unwrap_or_else(|_| "30".into()),
            "default_quality" => match val.as_str() {
                "nowm" | "hd" | "wm" | "audio" => val,
                _ => "nowm".into(),
            },
            "photo_audio" | "save_meta_json" => {
                if val == "1" || val == "true" { "1".into() } else { "0".into() }
            }
            "download_dir" => {
                let t = val.trim().to_string();
                if t.is_empty() {
                    return json!({ "error": "download_dir không được để trống" });
                }
                t
            }
            "filename_template" => {
                let t = val.trim().to_string();
                if t.is_empty() { "{author}_{id}".into() } else { t }
            }
            _ => val,
        };
        s.db.set_setting(k, &val);
        changed.push(k.clone());
    }
    if changed.is_empty() {
        return json!({ "error": "không có key hợp lệ nào trong patch", "editable": EDITABLE_SETTINGS });
    }
    s.db.log("settings", &format!("Đổi cài đặt: {}", changed.join(", ")), "");
    settings_value(s)
}

async fn get_settings(State(s): State<AppState>) -> Json<Value> {
    Json(settings_value(&s))
}

/// UI convenience: open the configured download folder in Finder/Files on the
/// machine the daemon runs on (created first so it always exists).
async fn open_download_dir(State(s): State<AppState>) -> Json<Value> {
    let dir = s.db.setting("download_dir", "");
    if dir.is_empty() {
        return Json(json!({ "error": "chưa cấu hình download_dir" }));
    }
    let _ = std::fs::create_dir_all(&dir);
    let cmd = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    match std::process::Command::new(cmd).arg(&dir).status() {
        Ok(st) if st.success() => Json(json!({ "ok": true, "opened": dir })),
        Ok(st) => Json(json!({ "error": format!("lệnh mở thư mục trả mã {st}") })),
        Err(e) => Json(json!({ "error": format!("không mở được: {e}") })),
    }
}

async fn set_settings(State(s): State<AppState>, Json(b): Json<Value>) -> Json<Value> {
    Json(set_settings_value(&s, &b))
}

async fn activity(State(s): State<AppState>) -> Json<Value> {
    Json(json!({ "ok": true, "activity": s.db.recent_activity(50) }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state() -> AppState {
        let db = Arc::new(Db::open_memory().unwrap());
        let http = tiktok::http_client();
        let (mcp_tx, _) = tokio::sync::broadcast::channel(4);
        AppState {
            db,
            http: http.clone(),
            resolver: Arc::new(Resolver::new(http)),
            queue: Arc::new(Queue::new()),
            mcp_tx,
        }
    }

    #[test]
    fn download_rejects_non_tiktok_and_dedups() {
        let s = test_state();
        let bad = download_value(&s, "https://example.com/x", "", false, None);
        assert!(bad["error"].as_str().is_some());

        let url = "https://www.tiktok.com/@a/video/1";
        let ok = download_value(&s, url, "hd", false, None);
        let id = ok["download"]["id"].as_i64().unwrap();
        s.db.claim_next_queued();
        s.db.finish_files(id, "/tmp", &[], 1);

        let dup = download_value(&s, url, "hd", false, None);
        assert_eq!(dup["duplicate"], true);
        assert_eq!(dup["existing_id"], id);
        let forced = download_value(&s, url, "hd", true, None);
        assert_eq!(forced["ok"], true, "force bỏ qua dedup");
    }

    #[test]
    fn batch_counts_queued_and_skipped() {
        let s = test_state();
        let u1 = "https://www.tiktok.com/@a/video/1";
        let done = download_value(&s, u1, "nowm", false, None);
        let id = done["download"]["id"].as_i64().unwrap();
        s.db.claim_next_queued();
        s.db.finish_files(id, "/tmp", &[], 1);

        let text = format!("{u1}\nhttps://www.tiktok.com/@a/video/2 rác https://example.com/no");
        let out = batch_value(&s, &text, "", false);
        assert_eq!(out["queued"], 1);
        assert_eq!(out["skipped_duplicates"], 1);
    }

    #[test]
    fn cancel_of_queued_row_flips_status_directly() {
        let s = test_state();
        let ok = download_value(&s, "https://www.tiktok.com/@a/video/7", "", false, None);
        let id = ok["download"]["id"].as_i64().unwrap();
        let c = cancel_value(&s, id);
        assert_eq!(c["ok"], true);
        assert_eq!(s.db.get_download(id).unwrap()["status"], "canceled");
        // Retry brings it back to queued.
        assert_eq!(retry_value(&s, id)["ok"], true);
        assert_eq!(s.db.get_download(id).unwrap()["status"], "queued");
    }

    #[test]
    fn settings_patch_validates() {
        let s = test_state();
        let out = set_settings_value(
            &s,
            &json!({"max_concurrent": 99, "default_quality": "hd", "bogus": "x"}),
        );
        assert_eq!(out["settings"]["max_concurrent"], "4", "clamp 1..4");
        assert_eq!(out["settings"]["default_quality"], "hd");
        assert!(out["settings"]["bogus"].is_null());
        let err = set_settings_value(&s, &json!({"download_dir": "  "}));
        assert!(err["error"].as_str().unwrap().contains("download_dir"));
    }
}
