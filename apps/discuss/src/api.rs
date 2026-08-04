//! REST API + AppState. Mọi nghiệp vụ nằm trong các hàm `*_value` trả
//! `Result<Value, String>` — REST và MCP (mcp.rs) gọi CHUNG các hàm này để
//! agent và người dùng thấy hành vi y hệt nhau.

use axum::{
    extract::{DefaultBodyLimit, Multipart, Path as AxPath, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::{broadcast, Semaphore};

use crate::db::{self, Db, NewMessage};

/// Trần 4 agent.run đồng thời/app phía daemon — giữ 3 để còn chỗ thở.
pub const AGENT_PARALLEL: usize = 3;

#[derive(Default)]
pub struct DiscRuntime {
    pub busy: bool,
    pub statuses: HashMap<i64, String>,
}

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub mcp_tx: broadcast::Sender<String>,
    pub runtime: Arc<Mutex<HashMap<i64, DiscRuntime>>>,
    pub agent_sema: Arc<Semaphore>,
}

impl AppState {
    pub fn try_mark_busy(&self, disc_id: i64) -> bool {
        let mut map = self.runtime.lock().unwrap();
        let rt = map.entry(disc_id).or_default();
        if rt.busy {
            false
        } else {
            rt.busy = true;
            true
        }
    }

    pub fn clear_busy(&self, disc_id: i64) {
        let mut map = self.runtime.lock().unwrap();
        if let Some(rt) = map.get_mut(&disc_id) {
            rt.busy = false;
        }
    }

    pub fn set_member_status(&self, disc_id: i64, member_id: i64, status: &str) {
        let mut map = self.runtime.lock().unwrap();
        let rt = map.entry(disc_id).or_default();
        rt.statuses.insert(member_id, status.to_string());
    }

    pub fn runtime_statuses(&self, disc_id: i64) -> HashMap<i64, String> {
        let map = self.runtime.lock().unwrap();
        map.get(&disc_id)
            .map(|rt| rt.statuses.clone())
            .unwrap_or_default()
    }
}

pub fn make_state() -> AppState {
    let db = Db::open_default().expect("open sqlite");
    let (mcp_tx, _) = broadcast::channel(64);
    AppState {
        db,
        mcp_tx,
        runtime: Arc::new(Mutex::new(HashMap::new())),
        agent_sema: Arc::new(Semaphore::new(AGENT_PARALLEL)),
    }
}

#[cfg(test)]
pub fn make_test_state() -> AppState {
    let db = Db::open_memory().expect("open sqlite memory");
    let (mcp_tx, _) = broadcast::channel(64);
    AppState {
        db,
        mcp_tx,
        runtime: Arc::new(Mutex::new(HashMap::new())),
        agent_sema: Arc::new(Semaphore::new(AGENT_PARALLEL)),
    }
}

// ---------------- Lỗi HTTP ----------------

pub struct ApiError(pub StatusCode, pub String);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}
fn bad(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.into())
}
fn wrap(r: Result<Value, String>) -> Result<Json<Value>, ApiError> {
    r.map(Json).map_err(bad)
}

// ---------------- Kho tài liệu: file cho agent Read ----------------

/// Ghi tài liệu thành file .md trong workspace phiên để member đọc bằng
/// Read/Grep. Best-effort — DB vẫn là nguồn sự thật.
pub fn write_doc_file(discussion_id: i64, doc_id: i64, title: &str, content: &str) -> String {
    let dir = crate::config::docs_dir(discussion_id);
    std::fs::create_dir_all(&dir).ok();
    let slug = db::slugify(title);
    let name = if slug.is_empty() {
        format!("doc-{doc_id}.md")
    } else {
        format!("doc-{doc_id}-{slug}.md")
    };
    let body = format!("# {title}\n\n> Kho tài liệu chung — trích dẫn bằng `doc:{doc_id}`\n\n{content}\n");
    std::fs::write(dir.join(&name), body).ok();
    name
}

/// Vật chất hoá kho chung (discussion_id NULL) vào thư mục phiên mới.
fn materialize_shared_docs(state: &AppState, discussion_id: i64) {
    if let Ok(docs) = state.db.doc_list(None, 200) {
        for d in docs.iter().filter(|d| d.discussion_id.is_none()) {
            write_doc_file(discussion_id, d.id, &d.title, &d.content);
        }
    }
}

// ---------------- Các hàm nghiệp vụ dùng chung REST/MCP ----------------

pub async fn status_value(state: &AppState) -> Result<Value, String> {
    let discussions = state.db.discussion_list(5).map_err(|e| e.to_string())?;
    let members = state.db.member_list().map_err(|e| e.to_string())?;
    let llm = crate::llm::llm_info().await;
    Ok(json!({
        "app": "discuss",
        "name": "AI Discuss Team",
        "version": env!("CARGO_PKG_VERSION"),
        "port": crate::config::http_port(),
        "llm": llm,
        "member_count": members.len(),
        "discussions": discussions.iter().map(|d| json!({
            "id": d.id, "title": d.title, "status": d.status, "round": d.round,
            "manager_score": d.manager_score,
        })).collect::<Vec<_>>(),
    }))
}

pub async fn tools_value() -> Result<Value, String> {
    Ok(json!({ "tools": crate::llm::mcp_tool_catalog().await }))
}

pub async fn llm_profiles_value() -> Result<Value, String> {
    Ok(json!({ "profiles": crate::llm::llm_profiles().await }))
}

pub fn members_value(state: &AppState) -> Result<Value, String> {
    let members = state.db.member_list().map_err(|e| e.to_string())?;
    Ok(json!({ "members": members }))
}

pub fn member_add_value(state: &AppState, args: &Value) -> Result<Value, String> {
    let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("").trim();
    if name.is_empty() {
        return Err("name là bắt buộc".into());
    }
    let role = match args.get("role").and_then(|v| v.as_str()).unwrap_or("member") {
        r @ ("member" | "manager" | "secretary") => r,
        _ => return Err("role phải là member|manager|secretary".into()),
    };
    let hats = args
        .get("hat")
        .and_then(crate::parse::normalize_hats)
        .unwrap_or_default();
    let m = state
        .db
        .member_add(
            name,
            role,
            args.get("expertise").and_then(|v| v.as_str()).unwrap_or(""),
            args.get("style").and_then(|v| v.as_str()).unwrap_or(""),
            &hats,
            args.get("use_tools").and_then(|v| v.as_bool()).unwrap_or(true),
            args.get("tools").filter(|t| t.is_array()),
            args.get("model").and_then(|v| v.as_str()).filter(|s| !s.trim().is_empty()),
        )
        .map_err(|e| e.to_string())?;
    Ok(json!({ "member": m }))
}

fn resolve_member_id(state: &AppState, args: &Value) -> Result<i64, String> {
    if let Some(id) = args.get("id").and_then(|v| v.as_i64()) {
        return Ok(id);
    }
    if let Some(key) = args.get("key").and_then(|v| v.as_str()) {
        if let Ok(Some(m)) = state.db.member_get_by_key(key.trim()) {
            return Ok(m.id);
        }
        return Err(format!("không tìm thấy member key '{key}'"));
    }
    Err("cần id hoặc key của member".into())
}

pub fn member_update_value(state: &AppState, args: &Value) -> Result<Value, String> {
    let id = resolve_member_id(state, args)?;
    // tools: vắng = giữ nguyên; null = xoá giới hạn (toàn bộ tool); mảng = giới hạn
    let tools_patch: Option<Option<&Value>> = match args.get("tools") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(v) if v.is_array() => Some(Some(v)),
        Some(_) => return Err("tools phải là mảng tên tool hoặc null".into()),
    };
    let model_patch: Option<Option<&str>> = match args.get("model") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(v) => Some(v.as_str().filter(|s| !s.trim().is_empty())),
    };
    // hat: vắng = giữ nguyên; có (chuỗi phẩy hoặc mảng) = validate; rỗng/sai hết = xoá
    let hats_patch: Option<String> = args
        .get("hat")
        .map(|v| crate::parse::normalize_hats(v).unwrap_or_default());
    state
        .db
        .member_update(
            id,
            args.get("name").and_then(|v| v.as_str()),
            args.get("expertise").and_then(|v| v.as_str()),
            args.get("style").and_then(|v| v.as_str()),
            hats_patch.as_deref(),
            args.get("use_tools").and_then(|v| v.as_bool()),
            tools_patch,
            model_patch,
            args.get("enabled").and_then(|v| v.as_bool()),
        )
        .map_err(|e| e.to_string())?;
    let m = state.db.member_get(id).map_err(|e| e.to_string())?;
    Ok(json!({ "member": m }))
}

pub fn member_delete_value(state: &AppState, args: &Value) -> Result<Value, String> {
    let id = resolve_member_id(state, args)?;
    state.db.member_delete(id).map_err(|e| e.to_string())?;
    Ok(json!({ "ok": true }))
}

pub fn member_memory_value(state: &AppState, args: &Value) -> Result<Value, String> {
    let id = resolve_member_id(state, args)?;
    let memory = state.db.memory_list(id, 50).map_err(|e| e.to_string())?;
    let thinking: Vec<Value> = state
        .db
        .thinking_recent(id, args.get("discussion_id").and_then(|v| v.as_i64()).unwrap_or(0), 20)
        .map(|v| {
            v.into_iter()
                .map(|(round, content)| json!({ "round": round, "content": content }))
                .collect()
        })
        .unwrap_or_default();
    Ok(json!({ "memory": memory, "thinking": thinking }))
}

pub fn discussion_create_value(state: &AppState, args: &Value) -> Result<Value, String> {
    let title = args.get("title").and_then(|v| v.as_str()).unwrap_or("").trim();
    if title.is_empty() {
        return Err("title (chủ đề) là bắt buộc".into());
    }
    let requirement = args
        .get("requirement")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if requirement.is_empty() {
        return Err("requirement (yêu cầu kết quả của BOSS) là bắt buộc — Manager cần nó để biết khi nào đủ".into());
    }
    let mode = match args.get("mode").and_then(|v| v.as_str()).unwrap_or("sequential") {
        m @ ("sequential" | "parallel") => m,
        _ => return Err("mode phải là sequential|parallel".into()),
    };
    let pace = args.get("pace_secs").and_then(|v| v.as_i64()).unwrap_or(20).clamp(0, 600);
    let max_rounds = args.get("max_rounds").and_then(|v| v.as_i64()).unwrap_or(12).clamp(1, 100);

    // Member tham gia: theo keys/ids nếu truyền, mặc định = mọi member enabled.
    let mut member_ids: Vec<i64> = Vec::new();
    if let Some(keys) = args.get("member_keys").and_then(|v| v.as_array()) {
        for k in keys {
            if let Some(k) = k.as_str() {
                match state.db.member_get_by_key(k.trim()) {
                    Ok(Some(m)) if m.role == "member" => member_ids.push(m.id),
                    _ => return Err(format!("member key '{k}' không tồn tại hoặc không phải role member")),
                }
            }
        }
    } else if let Some(ids) = args.get("member_ids").and_then(|v| v.as_array()) {
        member_ids = ids.iter().filter_map(|x| x.as_i64()).collect();
    } else {
        member_ids = state
            .db
            .member_list()
            .map_err(|e| e.to_string())?
            .into_iter()
            .filter(|m| m.role == "member" && m.enabled)
            .map(|m| m.id)
            .collect();
    }
    if member_ids.is_empty() {
        return Err("phiên cần ít nhất 1 member".into());
    }

    let id = state
        .db
        .discussion_create(title, requirement, mode, pace, max_rounds, &member_ids)
        .map_err(|e| e.to_string())?;
    std::fs::create_dir_all(crate::config::docs_dir(id)).ok();
    materialize_shared_docs(state, id);

    // Tin hệ thống mở phòng
    let _ = state.db.message_insert(&NewMessage {
        discussion_id: id,
        round: 0,
        author_kind: "system".into(),
        kind: "system".into(),
        content: format!(
            "Phòng thảo luận mở: “{title}”. Yêu cầu của BOSS: {requirement}. {} thành viên tham gia.",
            member_ids.len()
        ),
        citations: json!([]),
        flags: json!({}),
        ..Default::default()
    });

    if args.get("start").and_then(|v| v.as_bool()).unwrap_or(false) {
        state.db.discussion_set_status(id, "running").map_err(|e| e.to_string())?;
    }
    let d = state.db.discussion_get(id).map_err(|e| e.to_string())?;
    Ok(json!({ "discussion": d }))
}

pub fn discussion_list_value(state: &AppState, limit: i64) -> Result<Value, String> {
    let list = state.db.discussion_list(limit).map_err(|e| e.to_string())?;
    Ok(json!({ "discussions": list }))
}

pub fn discussion_detail_value(state: &AppState, id: i64) -> Result<Value, String> {
    let Some(d) = state.db.discussion_get(id).map_err(|e| e.to_string())? else {
        return Err(format!("phiên #{id} không tồn tại"));
    };
    let members = state.db.discussion_members(id).map_err(|e| e.to_string())?;
    let minutes = state.db.minutes_latest(id).map_err(|e| e.to_string())?;
    let result = state.db.result_latest(id).map_err(|e| e.to_string())?;
    let statuses = state.runtime_statuses(id);
    Ok(json!({
        "discussion": d,
        "members": members,
        "minutes": minutes,
        "result": result,
        "member_statuses": statuses,
    }))
}

fn transition(state: &AppState, id: i64, from: &[&str], to: &str) -> Result<Value, String> {
    let Some(d) = state.db.discussion_get(id).map_err(|e| e.to_string())? else {
        return Err(format!("phiên #{id} không tồn tại"));
    };
    if !from.contains(&d.status.as_str()) {
        return Err(format!(
            "không thể chuyển từ '{}' sang '{}' (cho phép từ: {})",
            d.status,
            to,
            from.join("|")
        ));
    }
    state.db.discussion_set_status(id, to).map_err(|e| e.to_string())?;
    Ok(json!({ "ok": true, "status": to }))
}

pub fn start_value(state: &AppState, id: i64) -> Result<Value, String> {
    transition(state, id, &["draft", "paused"], "running")
}
pub fn pause_value(state: &AppState, id: i64) -> Result<Value, String> {
    transition(state, id, &["running"], "paused")
}
pub fn resume_value(state: &AppState, id: i64) -> Result<Value, String> {
    transition(state, id, &["paused"], "running")
}

/// BOSS ép chốt ngay: tổng hợp kết quả bất kể Manager thấy đủ hay chưa.
pub fn conclude_value(state: &AppState, id: i64) -> Result<Value, String> {
    let Some(d) = state.db.discussion_get(id).map_err(|e| e.to_string())? else {
        return Err(format!("phiên #{id} không tồn tại"));
    };
    if !["running", "paused"].contains(&d.status.as_str()) {
        return Err(format!("phiên đang '{}' — chỉ chốt được khi running/paused", d.status));
    }
    let st = state.clone();
    tokio::spawn(async move {
        crate::engine::synthesize_result(&st, id, "BOSS yêu cầu chốt ngay.").await;
    });
    Ok(json!({ "ok": true, "status": "review", "note": "Thư ký đang tổng hợp — theo dõi panel Kết quả." }))
}

pub fn approve_value(state: &AppState, id: i64) -> Result<Value, String> {
    let Some(d) = state.db.discussion_get(id).map_err(|e| e.to_string())? else {
        return Err(format!("phiên #{id} không tồn tại"));
    };
    if d.status != "review" {
        return Err(format!("phiên đang '{}' — chỉ nghiệm thu khi 'review'", d.status));
    }
    crate::engine::approve_result(state, id).map_err(|e| e.to_string())?;
    Ok(json!({ "ok": true, "status": "done" }))
}

pub fn reject_value(state: &AppState, id: i64, feedback: &str) -> Result<Value, String> {
    let feedback = feedback.trim();
    if feedback.is_empty() {
        return Err("feedback là bắt buộc khi từ chối — đội cần biết phải sửa gì".into());
    }
    let Some(d) = state.db.discussion_get(id).map_err(|e| e.to_string())? else {
        return Err(format!("phiên #{id} không tồn tại"));
    };
    if d.status != "review" {
        return Err(format!("phiên đang '{}' — chỉ từ chối khi 'review'", d.status));
    }
    crate::engine::reject_result(state, id, feedback).map_err(|e| e.to_string())?;
    Ok(json!({ "ok": true, "status": "running" }))
}

/// BOSS phát biểu — chen vào bất kỳ lúc nào; member kế tiếp phải trả lời trước tiên.
pub fn say_value(state: &AppState, id: i64, content: &str) -> Result<Value, String> {
    let content = content.trim();
    if content.is_empty() {
        return Err("content trống".into());
    }
    let Some(d) = state.db.discussion_get(id).map_err(|e| e.to_string())? else {
        return Err(format!("phiên #{id} không tồn tại"));
    };
    if d.status == "done" {
        return Err("phiên đã kết thúc — tạo phiên mới hoặc reject kết quả trước đó".into());
    }
    let mid = state
        .db
        .message_insert(&NewMessage {
            discussion_id: id,
            round: d.round,
            author_kind: "boss".into(),
            kind: "boss".into(),
            content: content.to_string(),
            citations: json!([]),
            flags: json!({}),
            ..Default::default()
        })
        .map_err(|e| e.to_string())?;
    Ok(json!({ "ok": true, "message_id": mid }))
}

pub fn pace_value(state: &AppState, id: i64, args: &Value) -> Result<Value, String> {
    let mode = match args.get("mode").and_then(|v| v.as_str()) {
        None => None,
        Some(m @ ("sequential" | "parallel")) => Some(m),
        Some(_) => return Err("mode phải là sequential|parallel".into()),
    };
    state
        .db
        .discussion_set_pace(
            id,
            args.get("pace_secs").and_then(|v| v.as_i64()),
            mode,
            args.get("max_rounds").and_then(|v| v.as_i64()),
        )
        .map_err(|e| e.to_string())?;
    let d = state.db.discussion_get(id).map_err(|e| e.to_string())?;
    Ok(json!({ "discussion": d }))
}

pub fn messages_value(state: &AppState, id: i64, after: i64, limit: i64) -> Result<Value, String> {
    let msgs = state
        .db
        .messages_after(id, after, limit.clamp(1, 500))
        .map_err(|e| e.to_string())?;
    Ok(json!({ "messages": msgs }))
}

pub fn minutes_value(state: &AppState, id: i64) -> Result<Value, String> {
    let m = state.db.minutes_latest(id).map_err(|e| e.to_string())?;
    Ok(json!({ "minutes": m }))
}

pub fn result_value(state: &AppState, id: i64) -> Result<Value, String> {
    let r = state.db.result_latest(id).map_err(|e| e.to_string())?;
    Ok(json!({ "result": r }))
}

pub fn progress_value(state: &AppState, id: i64) -> Result<Value, String> {
    let Some(d) = state.db.discussion_get(id).map_err(|e| e.to_string())? else {
        return Err(format!("phiên #{id} không tồn tại"));
    };
    let participation = state.db.participation(id, d.round).map_err(|e| e.to_string())?;
    let opens = state.db.open_opinions(id, 50).map_err(|e| e.to_string())?;
    Ok(json!({
        "status": d.status,
        "round": d.round,
        "max_rounds": d.max_rounds,
        "manager_score": d.manager_score,
        "manager_missing": d.manager_missing,
        "participation": participation,
        "open_opinions": opens.iter().map(|m| json!({"id": m.id, "content": m.content})).collect::<Vec<_>>(),
        "member_statuses": state.runtime_statuses(id),
    }))
}

pub fn docs_add_text_value(state: &AppState, args: &Value) -> Result<Value, String> {
    let title = args.get("title").and_then(|v| v.as_str()).unwrap_or("").trim();
    let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("").trim();
    if title.is_empty() || content.is_empty() {
        return Err("title và content là bắt buộc".into());
    }
    let disc = args.get("discussion_id").and_then(|v| v.as_i64());
    if let Some(did) = disc {
        if state.db.discussion_get(did).map_err(|e| e.to_string())?.is_none() {
            return Err(format!("phiên #{did} không tồn tại"));
        }
    }
    let created_by = args.get("created_by").and_then(|v| v.as_str()).unwrap_or("boss");
    let id = state
        .db
        .doc_add(disc, title, "", content, "paste", created_by)
        .map_err(|e| e.to_string())?;
    // Ghi file cho phiên cụ thể, hoặc cho MỌI phiên chưa kết thúc nếu là kho chung
    if let Some(did) = disc {
        let name = write_doc_file(did, id, title, content);
        let _ = state.db.doc_set_filename(id, &name);
    } else {
        let mut name = String::new();
        for d in state.db.discussion_list(50).map_err(|e| e.to_string())? {
            if d.status != "done" {
                name = write_doc_file(d.id, id, title, content);
            }
        }
        if !name.is_empty() {
            let _ = state.db.doc_set_filename(id, &name);
        }
    }
    Ok(json!({ "doc_id": id }))
}

pub fn docs_list_value(state: &AppState, q: Option<&str>, discussion_id: Option<i64>, limit: i64) -> Result<Value, String> {
    let docs = match q.map(str::trim).filter(|s| !s.is_empty()) {
        Some(q) => state.db.doc_search(q, discussion_id, limit),
        None => state.db.doc_list(discussion_id, limit),
    }
    .map_err(|e| e.to_string())?;
    // Danh sách không trả content đầy đủ (nặng) — chỉ preview
    Ok(json!({
        "docs": docs.iter().map(|d| json!({
            "id": d.id, "discussion_id": d.discussion_id, "title": d.title,
            "filename": d.filename, "source": d.source, "created_by": d.created_by,
            "created_at": d.created_at,
            "preview": d.content.chars().take(240).collect::<String>(),
            "chars": d.content.chars().count(),
        })).collect::<Vec<_>>(),
    }))
}

pub fn docs_get_value(state: &AppState, id: i64) -> Result<Value, String> {
    let Some(d) = state.db.doc_get(id).map_err(|e| e.to_string())? else {
        return Err(format!("doc #{id} không tồn tại"));
    };
    Ok(json!({ "doc": d }))
}

pub fn docs_delete_value(state: &AppState, id: i64) -> Result<Value, String> {
    state.db.doc_delete(id).map_err(|e| e.to_string())?;
    Ok(json!({ "ok": true }))
}

// ---------------- REST handlers ----------------

type S = State<AppState>;

async fn h_status(State(s): S) -> Result<Json<Value>, ApiError> {
    wrap(status_value(&s).await)
}
async fn h_tools() -> Result<Json<Value>, ApiError> {
    wrap(tools_value().await)
}
async fn h_llm_profiles() -> Result<Json<Value>, ApiError> {
    wrap(llm_profiles_value().await)
}
async fn h_members(State(s): S) -> Result<Json<Value>, ApiError> {
    wrap(members_value(&s))
}
async fn h_member_add(State(s): S, Json(body): Json<Value>) -> Result<Json<Value>, ApiError> {
    wrap(member_add_value(&s, &body))
}
async fn h_member_update(
    State(s): S,
    AxPath(id): AxPath<i64>,
    Json(mut body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    body["id"] = json!(id);
    wrap(member_update_value(&s, &body))
}
async fn h_member_delete(State(s): S, AxPath(id): AxPath<i64>) -> Result<Json<Value>, ApiError> {
    wrap(member_delete_value(&s, &json!({ "id": id })))
}
async fn h_member_memory(
    State(s): S,
    AxPath(id): AxPath<i64>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let mut args = json!({ "id": id });
    if let Some(d) = q.get("discussion_id").and_then(|x| x.parse::<i64>().ok()) {
        args["discussion_id"] = json!(d);
    }
    wrap(member_memory_value(&s, &args))
}

async fn h_disc_create(State(s): S, Json(body): Json<Value>) -> Result<Json<Value>, ApiError> {
    wrap(discussion_create_value(&s, &body))
}
async fn h_disc_list(State(s): S, Query(q): Query<HashMap<String, String>>) -> Result<Json<Value>, ApiError> {
    let limit = q.get("limit").and_then(|x| x.parse().ok()).unwrap_or(30);
    wrap(discussion_list_value(&s, limit))
}
async fn h_disc_detail(State(s): S, AxPath(id): AxPath<i64>) -> Result<Json<Value>, ApiError> {
    wrap(discussion_detail_value(&s, id))
}
async fn h_disc_start(State(s): S, AxPath(id): AxPath<i64>) -> Result<Json<Value>, ApiError> {
    wrap(start_value(&s, id))
}
async fn h_disc_pause(State(s): S, AxPath(id): AxPath<i64>) -> Result<Json<Value>, ApiError> {
    wrap(pause_value(&s, id))
}
async fn h_disc_resume(State(s): S, AxPath(id): AxPath<i64>) -> Result<Json<Value>, ApiError> {
    wrap(resume_value(&s, id))
}
async fn h_disc_conclude(State(s): S, AxPath(id): AxPath<i64>) -> Result<Json<Value>, ApiError> {
    wrap(conclude_value(&s, id))
}
async fn h_disc_approve(State(s): S, AxPath(id): AxPath<i64>) -> Result<Json<Value>, ApiError> {
    wrap(approve_value(&s, id))
}
async fn h_disc_reject(
    State(s): S,
    AxPath(id): AxPath<i64>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let feedback = body.get("feedback").and_then(|v| v.as_str()).unwrap_or("");
    wrap(reject_value(&s, id, feedback))
}
async fn h_disc_say(
    State(s): S,
    AxPath(id): AxPath<i64>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let content = body.get("content").and_then(|v| v.as_str()).unwrap_or("");
    wrap(say_value(&s, id, content))
}
async fn h_disc_pace(
    State(s): S,
    AxPath(id): AxPath<i64>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    wrap(pace_value(&s, id, &body))
}
async fn h_disc_messages(
    State(s): S,
    AxPath(id): AxPath<i64>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let after = q.get("after").and_then(|x| x.parse().ok()).unwrap_or(0);
    let limit = q.get("limit").and_then(|x| x.parse().ok()).unwrap_or(200);
    wrap(messages_value(&s, id, after, limit))
}
async fn h_disc_minutes(State(s): S, AxPath(id): AxPath<i64>) -> Result<Json<Value>, ApiError> {
    wrap(minutes_value(&s, id))
}
async fn h_disc_result(State(s): S, AxPath(id): AxPath<i64>) -> Result<Json<Value>, ApiError> {
    wrap(result_value(&s, id))
}
async fn h_disc_progress(State(s): S, AxPath(id): AxPath<i64>) -> Result<Json<Value>, ApiError> {
    wrap(progress_value(&s, id))
}

async fn h_docs_list(State(s): S, Query(q): Query<HashMap<String, String>>) -> Result<Json<Value>, ApiError> {
    let disc = q.get("discussion_id").and_then(|x| x.parse().ok());
    let limit = q.get("limit").and_then(|x| x.parse().ok()).unwrap_or(50);
    wrap(docs_list_value(&s, q.get("q").map(String::as_str), disc, limit))
}
async fn h_docs_text(State(s): S, Json(body): Json<Value>) -> Result<Json<Value>, ApiError> {
    wrap(docs_add_text_value(&s, &body))
}
async fn h_docs_get(State(s): S, AxPath(id): AxPath<i64>) -> Result<Json<Value>, ApiError> {
    wrap(docs_get_value(&s, id))
}
async fn h_docs_delete(State(s): S, AxPath(id): AxPath<i64>) -> Result<Json<Value>, ApiError> {
    wrap(docs_delete_value(&s, id))
}

/// Upload multipart: field `file` (+ `title`, `discussion_id` tuỳ chọn).
/// Hỗ trợ txt/md/html/pdf (pdf scan không có text → báo lỗi rõ, không nuốt).
async fn h_docs_upload(State(s): S, mut mp: Multipart) -> Result<Json<Value>, ApiError> {
    let mut filename = String::new();
    let mut bytes: Vec<u8> = Vec::new();
    let mut title = String::new();
    let mut discussion_id: Option<i64> = None;
    while let Some(field) = mp.next_field().await.map_err(|e| bad(format!("multipart: {e}")))? {
        match field.name().unwrap_or("") {
            "file" => {
                filename = field.file_name().unwrap_or("tai-lieu").to_string();
                bytes = field
                    .bytes()
                    .await
                    .map_err(|e| bad(format!("đọc file: {e}")))?
                    .to_vec();
            }
            "title" => title = field.text().await.unwrap_or_default(),
            "discussion_id" => {
                discussion_id = field.text().await.ok().and_then(|t| t.trim().parse().ok());
            }
            _ => {}
        }
    }
    if bytes.is_empty() {
        return Err(bad("thiếu field 'file'"));
    }
    let content = extract_text(&filename, &bytes).map_err(bad)?;
    let title = if title.trim().is_empty() {
        filename.clone()
    } else {
        title.trim().to_string()
    };
    let mut args = json!({ "title": title, "content": content });
    if let Some(d) = discussion_id {
        args["discussion_id"] = json!(d);
    }
    wrap(docs_add_text_value(&s, &args))
}

/// Rút text từ bytes theo đuôi file. PDF scan (không có lớp text) → lỗi rõ ràng.
pub fn extract_text(filename: &str, bytes: &[u8]) -> Result<String, String> {
    let lower = filename.to_lowercase();
    if lower.ends_with(".pdf") {
        let text = pdf_extract::extract_text_from_mem(bytes)
            .map_err(|e| format!("không đọc được PDF: {e}"))?;
        if text.trim().chars().count() < 20 {
            return Err("PDF này là bản scan (không có lớp chữ). Hãy OCR trước rồi tải lại.".into());
        }
        Ok(text)
    } else if lower.ends_with(".html") || lower.ends_with(".htm") {
        Ok(strip_html(&String::from_utf8_lossy(bytes)))
    } else {
        Ok(String::from_utf8_lossy(bytes).to_string())
    }
}

/// Bóc tag HTML thô sơ — đủ cho tài liệu tham khảo, không cần render đúng.
fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len() / 2);
    let mut in_tag = false;
    let mut in_script = false;
    let lower = s.to_lowercase();
    let mut idx = 0usize;
    for (i, c) in s.char_indices() {
        if idx > i {
            continue;
        }
        if !in_tag && (lower[i..].starts_with("<script") || lower[i..].starts_with("<style")) {
            let close = if lower[i..].starts_with("<script") { "</script>" } else { "</style>" };
            if let Some(end) = lower[i..].find(close) {
                idx = i + end + close.len();
                in_script = true;
                continue;
            }
        }
        if in_script {
            if idx <= i {
                in_script = false;
            } else {
                continue;
            }
        }
        match c {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    // gom khoảng trắng
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---------------- Router ----------------

pub fn api_router(state: AppState) -> Router {
    Router::new()
        .route("/status", get(h_status))
        .route("/tools", get(h_tools))
        .route("/llm-profiles", get(h_llm_profiles))
        .route("/members", get(h_members).post(h_member_add))
        .route("/members/:id", axum::routing::patch(h_member_update).delete(h_member_delete))
        .route("/members/:id/memory", get(h_member_memory))
        .route("/discussions", get(h_disc_list).post(h_disc_create))
        .route("/discussions/:id", get(h_disc_detail))
        .route("/discussions/:id/start", post(h_disc_start))
        .route("/discussions/:id/pause", post(h_disc_pause))
        .route("/discussions/:id/resume", post(h_disc_resume))
        .route("/discussions/:id/conclude", post(h_disc_conclude))
        .route("/discussions/:id/approve", post(h_disc_approve))
        .route("/discussions/:id/reject", post(h_disc_reject))
        .route("/discussions/:id/say", post(h_disc_say))
        .route("/discussions/:id/pace", post(h_disc_pace))
        .route("/discussions/:id/messages", get(h_disc_messages))
        .route("/discussions/:id/minutes", get(h_disc_minutes))
        .route("/discussions/:id/result", get(h_disc_result))
        .route("/discussions/:id/progress", get(h_disc_progress))
        .route("/docs", get(h_docs_list))
        .route("/docs/text", post(h_docs_text))
        .route(
            "/docs/upload",
            post(h_docs_upload).layer(DefaultBodyLimit::max(25 * 1024 * 1024)),
        )
        .route("/docs/:id", get(h_docs_get).delete(h_docs_delete))
        .route("/mcp/sse", get(crate::mcp::mcp_sse).post(crate::mcp::mcp_message))
        .route("/mcp/message", post(crate::mcp::mcp_message))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_requires_requirement() {
        let s = make_test_state();
        let err = discussion_create_value(&s, &json!({ "title": "X" })).unwrap_err();
        assert!(err.contains("requirement"));
    }

    #[test]
    fn create_defaults_to_enabled_members() {
        let s = make_test_state();
        let v = discussion_create_value(
            &s,
            &json!({ "title": "X", "requirement": "3 kết luận", "start": true }),
        )
        .unwrap();
        let id = v["discussion"]["id"].as_i64().unwrap();
        assert_eq!(v["discussion"]["status"], "running");
        let members = s.db.discussion_members(id).unwrap();
        assert!(members.len() >= 4);
        // Én tắt sẵn không tham gia
        assert!(!members.iter().any(|m| m.key == "en-thoi-su"));
    }

    #[test]
    fn say_and_messages_feed() {
        let s = make_test_state();
        let v = discussion_create_value(&s, &json!({ "title": "X", "requirement": "r" })).unwrap();
        let id = v["discussion"]["id"].as_i64().unwrap();
        say_value(&s, id, "BOSS đây, tập trung chi phí").unwrap();
        let msgs = messages_value(&s, id, 0, 100).unwrap();
        let arr = msgs["messages"].as_array().unwrap();
        // 1 system + 1 boss
        assert!(arr.iter().any(|m| m["kind"] == "boss"));
        let after = arr.last().unwrap()["id"].as_i64().unwrap();
        let inc = messages_value(&s, id, after, 100).unwrap();
        assert!(inc["messages"].as_array().unwrap().is_empty());
    }

    #[test]
    fn transitions_guarded() {
        let s = make_test_state();
        let v = discussion_create_value(&s, &json!({ "title": "X", "requirement": "r" })).unwrap();
        let id = v["discussion"]["id"].as_i64().unwrap();
        assert!(pause_value(&s, id).is_err()); // draft không pause được
        start_value(&s, id).unwrap();
        assert!(start_value(&s, id).is_err()); // đã running
        pause_value(&s, id).unwrap();
        resume_value(&s, id).unwrap();
        assert!(approve_value(&s, id).is_err()); // chưa review
    }

    #[test]
    fn member_crud_and_tools_patch() {
        let s = make_test_state();
        let v = member_add_value(
            &s,
            &json!({ "name": "Hà • Dữ liệu", "expertise": "phân tích số liệu", "hat": "white",
                     "tools": ["mcp__search-mcp__search_query"] }),
        )
        .unwrap();
        let id = v["member"]["id"].as_i64().unwrap();
        assert_eq!(v["member"]["key"], "ha-du-lieu");
        // null xoá giới hạn tool
        let v2 = member_update_value(&s, &json!({ "id": id, "tools": Value::Null })).unwrap();
        assert!(v2["member"]["tools"].is_null());
        member_delete_value(&s, &json!({ "id": id })).unwrap();
        assert!(s.db.member_get(id).unwrap().is_none());
    }

    #[test]
    fn member_model_profile_patch() {
        let s = make_test_state();
        // 2 member 2 model khác nhau (VD Gemini vs Claude) — lưu và đổi được
        let v = member_add_value(&s, &json!({ "name": "Gem", "model": "llm_gemini_1" })).unwrap();
        assert_eq!(v["member"]["model"], "llm_gemini_1");
        let id = v["member"]["id"].as_i64().unwrap();
        let v2 = member_update_value(&s, &json!({ "id": id, "model": "llm_claude_2" })).unwrap();
        assert_eq!(v2["member"]["model"], "llm_claude_2");
        let v3 = member_update_value(&s, &json!({ "id": id, "model": Value::Null })).unwrap();
        assert!(v3["member"]["model"].is_null());
    }

    #[test]
    fn docs_text_and_search() {
        let s = make_test_state();
        let v = docs_add_text_value(
            &s,
            &json!({ "title": "Báo giá điện 2026", "content": "Giá điện tăng 4,8% từ tháng Năm" }),
        )
        .unwrap();
        let doc_id = v["doc_id"].as_i64().unwrap();
        let hits = docs_list_value(&s, Some("gia dien"), None, 10).unwrap();
        assert!(hits["docs"].as_array().unwrap().iter().any(|d| d["id"] == doc_id));
    }

    #[test]
    fn strip_html_removes_tags_and_scripts() {
        let html = "<html><head><style>body{}</style></head><body><h1>Tiêu đề</h1><script>var x=1;</script><p>Nội dung chính</p></body></html>";
        let t = strip_html(html);
        assert!(t.contains("Tiêu đề"));
        assert!(t.contains("Nội dung chính"));
        assert!(!t.contains("var x"));
        assert!(!t.contains("body{}"));
    }

    #[test]
    fn extract_text_rejects_scanned_pdf() {
        // PDF hợp lệ tối thiểu nhưng không có text — pdf_extract trả chuỗi rỗng/lỗi
        let fake = b"%PDF-1.4\n%%EOF";
        assert!(extract_text("scan.pdf", fake).is_err());
    }
}
