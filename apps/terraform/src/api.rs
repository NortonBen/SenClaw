//! HTTP API cho app Terraform. Mọi handler đi qua các helper `*_value` mà MCP
//! server ([`crate::mcp`]) dùng lại — REST và tool agent luôn hành xử giống
//! hệt nhau. Outbound duy nhất: releases.hashicorp.com khi cài CLI, git remote
//! của user, và bridge LLM (giải thích lỗi run).

use crate::db::{self, Db};
use crate::gitops;
use crate::hcl_form;
use crate::runner::{Runner, Step};
use crate::tfcli;
use app_space_sdk::SpaceClient;
use axum::{
    extract::{Path as AxPath, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Db>,
    pub runner: Arc<Runner>,
    pub sc: SpaceClient,
    /// Fan-out MCP JSON-RPC responses tới SSE client đang nối.
    pub mcp_tx: tokio::sync::broadcast::Sender<String>,
}

pub fn make_state() -> AppState {
    let db = Arc::new(Db::open_default().expect("open terraform db"));
    let (mcp_tx, _) = tokio::sync::broadcast::channel(100);
    AppState {
        runner: Arc::new(Runner::new(db.clone())),
        db,
        sc: SpaceClient::from_env(),
        mcp_tx,
    }
}

pub fn api_router(state: AppState) -> Router {
    Router::new()
        .route("/status", get(status_h))
        .route("/cli", get(cli_h))
        .route("/cli/install", post(cli_install_h))
        .route("/fs", get(fs_h))
        .route("/workspaces", get(ws_list_h).post(ws_add_h))
        .route("/workspaces/:id", get(ws_get_h).post(ws_patch_h))
        .route("/workspaces/:id/delete", post(ws_delete_h))
        .route("/workspaces/:id/sync", post(ws_sync_h))
        .route("/workspaces/:id/open-dir", post(open_dir_h))
        .route("/workspaces/:id/subdirs", get(subdirs_h))
        .route("/workspaces/:id/tfvars-files", get(tfvars_pick_h))
        .route("/workspaces/:id/variables", get(vars_h))
        .route("/workspaces/:id/tfvars", get(tfvars_get_h).post(tfvars_set_h))
        .route("/workspaces/:id/run", post(run_h))
        .route("/runs", get(runs_h))
        .route("/runs/:id", get(run_get_h))
        .route("/runs/:id/cancel", post(run_cancel_h))
        .route("/runs/:id/explain", post(explain_h))
        .route("/activity", get(activity_h))
        .route("/settings", get(settings_get_h).post(settings_set_h))
        // MCP (HTTP + SSE), cùng shape với các Space App khác.
        .route("/mcp/sse", get(crate::mcp::mcp_sse).post(crate::mcp::mcp_message))
        .route("/mcp/message", post(crate::mcp::mcp_message))
        .with_state(state)
}

fn err(e: impl std::fmt::Display) -> Value {
    json!({ "ok": false, "error": e.to_string() })
}

// ---- status / cli ----

pub(crate) fn status_value(s: &AppState) -> Value {
    let workspaces = s.db.workspace_list().map(|v| v.len()).unwrap_or(0);
    let running = s
        .db
        .run_list(None, 200)
        .unwrap_or_default()
        .iter()
        .filter(|r| r["status"] == "running")
        .count();
    json!({
        "ok": true,
        "app": "terraform",
        "workspaces": workspaces,
        "running": running,
    })
}

async fn status_h(State(s): State<AppState>) -> Json<Value> {
    Json(status_value(&s))
}

pub(crate) async fn cli_value(s: &AppState) -> Value {
    let override_bin = s.db.setting_get("terraform_bin").ok().flatten();
    tfcli::discover(override_bin).await
}

async fn cli_h(State(s): State<AppState>) -> Json<Value> {
    Json(cli_value(&s).await)
}

#[derive(Deserialize, Default)]
pub struct InstallReq {
    pub version: Option<String>,
}

/// Cài terraform trong background; tiến trình xem ở console (run kind `install`).
pub(crate) fn cli_install_value(s: &AppState, version: Option<String>) -> Value {
    if let Ok(Some(id)) = s.db.running_kind("install") {
        return err(format!("đang có lần cài khác chạy (run #{id}) — xem console"));
    }
    let run_id = match s.db.run_create(None, "install") {
        Ok(id) => id,
        Err(e) => return err(e),
    };
    s.db.log("bắt đầu cài Terraform CLI");
    let db = s.db.clone();
    tokio::spawn(async move {
        let log_db = db.clone();
        let log = move |line: &str| {
            let _ = log_db.run_append(run_id, "out", line);
        };
        match tfcli::install(version, log).await {
            Ok(path) => {
                let _ = db.run_append(run_id, "sys", "✓ Hoàn tất");
                let _ = db.run_finish(run_id, "success", Some(0));
                db.log(&format!("đã cài Terraform: {}", path.display()));
            }
            Err(e) => {
                let _ = db.run_append(run_id, "sys", &format!("✗ {e}"));
                let _ = db.run_finish(run_id, "failed", None);
            }
        }
    });
    json!({ "ok": true, "run_id": run_id })
}

async fn cli_install_h(State(s): State<AppState>, Json(req): Json<InstallReq>) -> Json<Value> {
    Json(cli_install_value(&s, req.version))
}

// ---- folder picker ----

#[derive(Deserialize, Default)]
pub struct FsQuery {
    pub path: Option<String>,
    #[serde(default)]
    pub hidden: bool,
}

fn dir_has_tf(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.take(200)
                .filter_map(|e| e.ok())
                .any(|e| e.path().extension().is_some_and(|x| x == "tf"))
        })
        .unwrap_or(false)
}

pub(crate) fn fs_list_value(path: Option<String>, hidden: bool) -> Value {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| "/".into());
    let base = PathBuf::from(path.unwrap_or_else(|| home.clone()));
    let base = match base.canonicalize() {
        Ok(p) if p.is_dir() => p,
        _ => return err(format!("không mở được thư mục {}", base.display())),
    };
    let mut entries: Vec<Value> = std::fs::read_dir(&base)
        .map(|rd| {
            let mut dirs: Vec<PathBuf> = rd
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .filter(|p| {
                    hidden
                        || !p
                            .file_name()
                            .map(|n| n.to_string_lossy().starts_with('.'))
                            .unwrap_or(true)
                })
                .collect();
            dirs.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
            dirs.truncate(500);
            dirs.iter()
                .map(|p| {
                    json!({
                        "name": p.file_name().unwrap_or_default().to_string_lossy(),
                        "path": p.to_string_lossy(),
                        "has_tf": dir_has_tf(p),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    // Thư mục có *.tf nổi lên đầu cho dễ chọn.
    entries.sort_by_key(|e| e["has_tf"] != true);
    json!({
        "ok": true,
        "path": base.to_string_lossy(),
        "parent": base.parent().map(|p| p.to_string_lossy().to_string()),
        "home": home,
        "has_tf": dir_has_tf(&base),
        "entries": entries,
    })
}

async fn fs_h(Query(q): Query<FsQuery>) -> Json<Value> {
    Json(fs_list_value(q.path, q.hidden))
}

// ---- workspaces ----

#[derive(Deserialize, Default)]
pub struct WsAddReq {
    pub name: Option<String>,
    pub source: String,
    pub path: Option<String>,
    pub repo_url: Option<String>,
    pub branch: Option<String>,
    /// Root Terraform trong repo (vd `terraform` hay `infra/prod`) — trống = gốc.
    pub subdir: Option<String>,
}

pub(crate) fn ws_add_value(s: &AppState, req: &WsAddReq) -> Value {
    match req.source.as_str() {
        "folder" => {
            let Some(path) = req.path.as_deref().filter(|p| !p.is_empty()) else {
                return err("thiếu path cho workspace kiểu folder");
            };
            let dir = match PathBuf::from(path).canonicalize() {
                Ok(p) if p.is_dir() => p,
                _ => return err(format!("thư mục không tồn tại: {path}")),
            };
            let name = req
                .name
                .clone()
                .filter(|n| !n.trim().is_empty())
                .unwrap_or_else(|| {
                    dir.file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| "workspace".into())
                });
            match s.db.workspace_add(&name, "folder", &dir.to_string_lossy(), "", "", "ready") {
                Ok(id) => {
                    s.db.log(&format!("thêm workspace folder \"{name}\" → {}", dir.display()));
                    json!({ "ok": true, "workspace": s.db.workspace_get(id).ok().flatten() })
                }
                Err(e) => err(e),
            }
        }
        "git" => {
            let Some(url) = req.repo_url.as_deref().filter(|u| !u.is_empty()) else {
                return err("thiếu repo_url cho workspace kiểu git");
            };
            if let Err(e) = gitops::validate_repo_url(url) {
                return err(e);
            }
            let branch = req.branch.clone().unwrap_or_default();
            let subdir = req.subdir.clone().unwrap_or_default();
            let subdir = subdir.trim_matches('/').to_string();
            if let Err(e) = validate_subdir(&subdir) {
                return err(e);
            }
            let name = req
                .name
                .clone()
                .filter(|n| !n.trim().is_empty())
                .unwrap_or_else(|| gitops::repo_dir_name(url));
            let id = match s.db.workspace_add(&name, "git", "", url, &branch, "cloning") {
                Ok(id) => id,
                Err(e) => return err(e),
            };
            let dest = db::repos_dir().join(format!("{}-{}", gitops::repo_dir_name(url), id));
            let _ = std::fs::create_dir_all(db::repos_dir());
            let _ = s.db.workspace_update(
                id,
                None,
                None,
                None,
                None,
                None,
                None,
                Some(&dest.to_string_lossy()),
                Some(&subdir),
            );
            let run_id = match s.db.run_create(Some(id), "clone") {
                Ok(r) => r,
                Err(e) => return err(e),
            };
            s.runner.spawn_steps(
                run_id,
                vec![Step::new("git", gitops::clone_args(url, &branch, &dest), None)],
                Duration::from_secs(1200),
            );
            s.db.log(&format!("thêm workspace git \"{name}\" ← {url} (clone run #{run_id})"));
            json!({
                "ok": true,
                "workspace": s.db.workspace_get(id).ok().flatten(),
                "run_id": run_id,
            })
        }
        other => err(format!("source phải là folder|git, nhận: {other}")),
    }
}

async fn ws_add_h(State(s): State<AppState>, Json(req): Json<WsAddReq>) -> Json<Value> {
    Json(ws_add_value(&s, &req))
}

pub(crate) fn ws_list_value(s: &AppState) -> Value {
    match s.db.workspace_list() {
        Ok(list) => json!({ "ok": true, "workspaces": list }),
        Err(e) => err(e),
    }
}

async fn ws_list_h(State(s): State<AppState>) -> Json<Value> {
    Json(ws_list_value(&s))
}

pub(crate) async fn ws_get_value(s: &AppState, id: i64) -> Value {
    let Some(ws) = s.db.workspace_get(id).ok().flatten() else {
        return err(format!("workspace {id} không tồn tại"));
    };
    // Git thao tác ở gốc repo; terraform thao tác ở work_dir (gốc + subdir).
    let dir = PathBuf::from(ws["dir"].as_str().unwrap_or_default());
    let wd = work_dir(&ws);
    let git = if ws["source"] == "git" || gitops::is_git_repo(&dir) {
        gitops::info(&dir).await
    } else {
        json!({ "is_git": false })
    };
    let running = s.db.running_run(id).ok().flatten();
    let last_run = s.db.run_list(Some(id), 1).unwrap_or_default().into_iter().next();
    json!({
        "ok": true,
        "workspace": ws,
        "git": git,
        "work_dir": wd.to_string_lossy(),
        "work_dir_exists": wd.is_dir(),
        "tfvars_files": hcl_form::list_tfvars(&wd),
        "running_run": running,
        "last_run": last_run,
        "initialized": wd.join(".terraform").exists(),
    })
}

async fn ws_get_h(State(s): State<AppState>, AxPath(id): AxPath<i64>) -> Json<Value> {
    Json(ws_get_value(&s, id).await)
}

#[derive(Deserialize, Default)]
pub struct WsPatchReq {
    pub name: Option<String>,
    pub var_file: Option<String>,
    pub auto_sync: Option<bool>,
    /// Đổi root Terraform trong workspace (`""` = quay về gốc repo).
    pub subdir: Option<String>,
}

pub(crate) fn ws_patch_value(s: &AppState, id: i64, req: &WsPatchReq) -> Value {
    if s.db.workspace_get(id).ok().flatten().is_none() {
        return err(format!("workspace {id} không tồn tại"));
    }
    if let Some(f) = req.var_file.as_deref() {
        if !f.is_empty() {
            let Some(ws) = s.db.workspace_get(id).ok().flatten() else {
                return err(format!("workspace {id} không tồn tại"));
            };
            let root = PathBuf::from(ws["dir"].as_str().unwrap_or_default());
            if let Err(e) = resolve_var_file(&root, &work_dir(&ws), f) {
                return err(e);
            }
        }
    }
    let subdir = req.subdir.as_deref().map(|s| s.trim_matches('/').to_string());
    if let Some(sub) = subdir.as_deref() {
        if let Err(e) = validate_subdir(sub) {
            return err(e);
        }
    }
    match s.db.workspace_update(
        id,
        req.name.as_deref(),
        None,
        req.var_file.as_deref(),
        req.auto_sync,
        None,
        None,
        None,
        subdir.as_deref(),
    ) {
        Ok(()) => json!({ "ok": true, "workspace": s.db.workspace_get(id).ok().flatten() }),
        Err(e) => err(e),
    }
}

async fn ws_patch_h(
    State(s): State<AppState>,
    AxPath(id): AxPath<i64>,
    Json(req): Json<WsPatchReq>,
) -> Json<Value> {
    Json(ws_patch_value(&s, id, &req))
}

#[derive(Deserialize, Default)]
pub struct ConfirmReq {
    #[serde(default)]
    pub confirm: bool,
}

pub(crate) fn ws_delete_value(s: &AppState, id: i64, confirm: bool) -> Value {
    let Some(ws) = s.db.workspace_get(id).ok().flatten() else {
        return err(format!("workspace {id} không tồn tại"));
    };
    if !confirm {
        return err("cần confirm=true để xoá workspace");
    }
    if s.db.running_run(id).ok().flatten().is_some() {
        return err("workspace đang có run chạy — huỷ run trước");
    }
    // Chỉ xoá thư mục nếu là bản CLONE app tự quản; folder của user thì không đụng.
    let dir = PathBuf::from(ws["dir"].as_str().unwrap_or_default());
    if ws["source"] == "git" && dir.starts_with(db::repos_dir()) && dir.exists() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    match s.db.workspace_delete(id) {
        Ok(()) => {
            s.db.log(&format!("xoá workspace #{id} ({})", ws["name"].as_str().unwrap_or("")));
            json!({ "ok": true })
        }
        Err(e) => err(e),
    }
}

async fn ws_delete_h(
    State(s): State<AppState>,
    AxPath(id): AxPath<i64>,
    Json(req): Json<ConfirmReq>,
) -> Json<Value> {
    Json(ws_delete_value(&s, id, req.confirm))
}

pub(crate) fn ws_sync_value(s: &AppState, id: i64) -> Value {
    let Some(ws) = s.db.workspace_get(id).ok().flatten() else {
        return err(format!("workspace {id} không tồn tại"));
    };
    let dir = PathBuf::from(ws["dir"].as_str().unwrap_or_default());
    if !gitops::is_git_repo(&dir) {
        return err("workspace này không phải git repo — sync chỉ dành cho nguồn git");
    }
    if let Some(rid) = s.db.running_run(id).ok().flatten() {
        return err(format!("đang có run #{rid} chạy — đợi xong hoặc huỷ trước"));
    }
    let run_id = match s.db.run_create(Some(id), "sync") {
        Ok(r) => r,
        Err(e) => return err(e),
    };
    s.runner.spawn_steps(
        run_id,
        vec![Step::new("git", gitops::pull_args(&dir), None)],
        Duration::from_secs(600),
    );
    json!({ "ok": true, "run_id": run_id })
}

async fn ws_sync_h(State(s): State<AppState>, AxPath(id): AxPath<i64>) -> Json<Value> {
    Json(ws_sync_value(&s, id))
}

/// Subdir (root Terraform trong repo) hợp lệ: đường dẫn tương đối, không thoát
/// ra ngoài workspace.
pub fn validate_subdir(s: &str) -> anyhow::Result<()> {
    if s.is_empty() {
        return Ok(());
    }
    let clean = s.trim_matches('/');
    let bad = s.starts_with('/')
        || s.starts_with('\\')
        || s.contains('\0')
        || clean.is_empty()
        || clean.split(['/', '\\']).any(|seg| seg == ".." || seg.is_empty());
    if bad {
        anyhow::bail!("thư mục Terraform không hợp lệ: {s:?} (phải là đường dẫn tương đối trong repo, không ..)");
    }
    Ok(())
}

/// Thư mục làm việc Terraform thật sự của workspace = dir [+ subdir].
pub(crate) fn work_dir(ws: &Value) -> PathBuf {
    let base = PathBuf::from(ws["dir"].as_str().unwrap_or_default());
    match ws["subdir"].as_str().filter(|s| !s.is_empty()) {
        Some(sub) => base.join(sub.trim_matches('/')),
        None => base,
    }
}

/// Quét các thư mục con (depth ≤ 4) chứa file *.tf — gợi ý chọn root Terraform.
pub fn tf_subdirs(root: &Path) -> Vec<String> {
    fn walk(base: &Path, rel: String, depth: usize, out: &mut Vec<String>) {
        if depth > 4 || out.len() >= 200 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(base) else { return };
        let mut children: Vec<PathBuf> = rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        children.sort();
        for p in children {
            let Some(name) = p.file_name().map(|n| n.to_string_lossy().to_string()) else {
                continue;
            };
            if name.starts_with('.') || name == "node_modules" || name == "target" {
                continue;
            }
            let relp = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
            if dir_has_tf(&p) {
                out.push(relp.clone());
            }
            walk(&p, relp, depth + 1, out);
        }
    }
    let mut out = Vec::new();
    walk(root, String::new(), 1, &mut out);
    out
}

/// Gợi ý root Terraform cho UI: root repo có .tf không + các thư mục con có .tf.
pub(crate) fn subdirs_value(s: &AppState, id: i64) -> Value {
    let Some(ws) = s.db.workspace_get(id).ok().flatten() else {
        return err(format!("workspace {id} không tồn tại"));
    };
    let root = PathBuf::from(ws["dir"].as_str().unwrap_or_default());
    json!({
        "ok": true,
        "root_has_tf": dir_has_tf(&root),
        "subdir": ws["subdir"],
        "subdirs": tf_subdirs(&root),
    })
}

async fn subdirs_h(State(s): State<AppState>, AxPath(id): AxPath<i64>) -> Json<Value> {
    Json(subdirs_value(&s, id))
}

/// Resolve var-file của workspace: tên trần (`prod.tfvars`) nằm trong work_dir,
/// hoặc đường dẫn tương đối (`../../environments/prod.tfvars`) — miễn là điểm
/// đến vẫn NẰM TRONG thư mục gốc workspace và đúng đuôi .tfvars(.json).
pub fn resolve_var_file(root: &Path, work_dir: &Path, name: &str) -> anyhow::Result<PathBuf> {
    if !(name.ends_with(".tfvars") || name.ends_with(".tfvars.json")) {
        anyhow::bail!("var-file phải có đuôi .tfvars hoặc .tfvars.json: {name:?}");
    }
    if !name.contains('/') && !name.contains('\\') {
        hcl_form::validate_tfvars_name(name)?;
        return Ok(work_dir.join(name));
    }
    if name.starts_with('/') || name.starts_with('\\') || name.contains('\0') {
        anyhow::bail!("var-file phải là đường dẫn tương đối trong workspace: {name:?}");
    }
    let rootc = root
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("workspace không tồn tại: {e}"))?;
    let mut p = work_dir
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("thư mục Terraform không tồn tại: {e}"))?;
    for seg in name.split(['/', '\\']) {
        match seg {
            "" | "." => {}
            ".." => {
                p.pop();
            }
            s => {
                if !s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
                    || s.starts_with('.') && s != ".." && !s.ends_with(".tfvars") && !s.ends_with(".json")
                {
                    anyhow::bail!("var-file chứa thành phần không hợp lệ: {s:?}");
                }
                p.push(s);
            }
        }
    }
    if !p.starts_with(&rootc) {
        anyhow::bail!("var-file thoát ra ngoài workspace: {name:?}");
    }
    Ok(p)
}

/// Đường dẫn tương đối từ `from` tới `to` (cả hai tuyệt đối, cùng cây).
fn rel_between(from: &Path, to: &Path) -> String {
    let f: Vec<_> = from.components().collect();
    let t: Vec<_> = to.components().collect();
    let common = f.iter().zip(t.iter()).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = vec!["..".into(); f.len() - common];
    parts.extend(t[common..].iter().map(|c| c.as_os_str().to_string_lossy().to_string()));
    parts.join("/")
}

/// Quét toàn workspace (depth ≤ 4) tìm file *.tfvars / *.tfvars.json.
pub fn tfvars_files_deep(root: &Path) -> Vec<PathBuf> {
    fn walk(base: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        if depth > 4 || out.len() >= 200 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(base) else { return };
        let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            let Some(name) = p.file_name().map(|n| n.to_string_lossy().to_string()) else {
                continue;
            };
            if p.is_dir() {
                if name.starts_with('.') || name == "node_modules" || name == "target" {
                    continue;
                }
                walk(&p, depth + 1, out);
            } else if name.ends_with(".tfvars") || name.ends_with(".tfvars.json") {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, 1, &mut out);
    out
}

/// Danh sách file .tfvars trong toàn workspace cho dialog chọn — mỗi file kèm
/// `rel` (giá trị var_file: tương đối so với work_dir) + `display` (so với gốc).
pub(crate) fn tfvars_pick_value(s: &AppState, id: i64) -> Value {
    let Some(ws) = s.db.workspace_get(id).ok().flatten() else {
        return err(format!("workspace {id} không tồn tại"));
    };
    let root = PathBuf::from(ws["dir"].as_str().unwrap_or_default());
    let Ok(rootc) = root.canonicalize() else {
        return err("workspace không tồn tại trên đĩa");
    };
    let wdc = work_dir(&ws).canonicalize().unwrap_or_else(|_| rootc.clone());
    let files: Vec<Value> = tfvars_files_deep(&rootc)
        .into_iter()
        .map(|p| {
            let rel = rel_between(&wdc, &p);
            json!({
                "rel": rel,
                "display": rel_between(&rootc, &p),
                "in_work_dir": p.parent() == Some(wdc.as_path()),
            })
        })
        .collect();
    json!({ "ok": true, "files": files, "current": ws["var_file"] })
}

async fn tfvars_pick_h(State(s): State<AppState>, AxPath(id): AxPath<i64>) -> Json<Value> {
    Json(tfvars_pick_value(&s, id))
}

/// Lệnh mở thư mục trong file manager của hệ điều hành đang chạy.
pub fn opener_command(dir: &str) -> (&'static str, Vec<String>) {
    if cfg!(target_os = "macos") {
        ("open", vec![dir.to_string()])
    } else if cfg!(target_os = "windows") {
        ("explorer", vec![dir.to_string()])
    } else {
        ("xdg-open", vec![dir.to_string()])
    }
}

/// Mở thư mục workspace (bản clone nếu nguồn git) trong Finder/Explorer.
pub(crate) fn open_dir_value(s: &AppState, id: i64) -> Value {
    let Some(ws) = s.db.workspace_get(id).ok().flatten() else {
        return err(format!("workspace {id} không tồn tại"));
    };
    let dir = ws["dir"].as_str().unwrap_or_default().to_string();
    if dir.is_empty() || !Path::new(&dir).is_dir() {
        return err(format!("thư mục không tồn tại trên đĩa: {dir}"));
    }
    let (prog, args) = opener_command(&dir);
    match std::process::Command::new(prog).args(&args).spawn() {
        Ok(mut child) => {
            // Reap con để không để zombie tới khi app thoát.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            s.db.log(&format!("mở thư mục workspace #{id}: {dir}"));
            json!({ "ok": true, "dir": dir })
        }
        Err(e) => err(format!("không mở được file manager ({prog}): {e}")),
    }
}

async fn open_dir_h(State(s): State<AppState>, AxPath(id): AxPath<i64>) -> Json<Value> {
    Json(open_dir_value(&s, id))
}

// ---- variables / tfvars ----

pub(crate) fn vars_value(s: &AppState, id: i64) -> Value {
    let Some(ws) = s.db.workspace_get(id).ok().flatten() else {
        return err(format!("workspace {id} không tồn tại"));
    };
    let wd = work_dir(&ws);
    let (defs, mut errors) = hcl_form::parse_variables(&wd);
    if !wd.is_dir() {
        errors.push(format!(
            "thư mục Terraform không tồn tại: {} — chỉnh lại subdir trong tab Thông tin",
            wd.display()
        ));
    }
    json!({
        "ok": true,
        "variables": defs,
        "parse_errors": errors,
        "tfvars_files": hcl_form::list_tfvars(&wd),
        "var_file": ws["var_file"],
        "subdir": ws["subdir"],
        "work_dir": wd.to_string_lossy(),
    })
}

async fn vars_h(State(s): State<AppState>, AxPath(id): AxPath<i64>) -> Json<Value> {
    Json(vars_value(&s, id))
}

#[derive(Deserialize, Default)]
pub struct TfvarsQuery {
    pub file: Option<String>,
}

pub(crate) fn tfvars_get_value(s: &AppState, id: i64, file: Option<String>) -> Value {
    let Some(ws) = s.db.workspace_get(id).ok().flatten() else {
        return err(format!("workspace {id} không tồn tại"));
    };
    let root = PathBuf::from(ws["dir"].as_str().unwrap_or_default());
    let dir = work_dir(&ws);
    let file = file
        .filter(|f| !f.is_empty())
        .or_else(|| ws["var_file"].as_str().filter(|f| !f.is_empty()).map(String::from));
    let Some(file) = file else {
        return json!({ "ok": true, "file": null, "exists": false, "values": {} });
    };
    let path = match resolve_var_file(&root, &dir, &file) {
        Ok(p) => p,
        Err(e) => return err(e),
    };
    if !path.exists() {
        return json!({ "ok": true, "file": file, "exists": false, "values": {} });
    }
    match hcl_form::read_tfvars_at(&path) {
        Ok(values) => json!({ "ok": true, "file": file, "exists": true, "values": values }),
        Err(e) => err(e),
    }
}

async fn tfvars_get_h(
    State(s): State<AppState>,
    AxPath(id): AxPath<i64>,
    Query(q): Query<TfvarsQuery>,
) -> Json<Value> {
    Json(tfvars_get_value(&s, id, q.file))
}

#[derive(Deserialize)]
pub struct TfvarsSetReq {
    pub file: String,
    pub values: Map<String, Value>,
    /// false (mặc định) = merge đè lên giá trị sẵn có trong file; true = thay cả file.
    #[serde(default)]
    pub replace: bool,
}

pub(crate) fn tfvars_set_value(s: &AppState, id: i64, req: &TfvarsSetReq) -> Value {
    let Some(ws) = s.db.workspace_get(id).ok().flatten() else {
        return err(format!("workspace {id} không tồn tại"));
    };
    let root = PathBuf::from(ws["dir"].as_str().unwrap_or_default());
    let dir = work_dir(&ws);
    if !dir.is_dir() {
        return err(format!("thư mục Terraform không tồn tại: {}", dir.display()));
    }
    let path = match resolve_var_file(&root, &dir, &req.file) {
        Ok(p) => p,
        Err(e) => return err(e),
    };
    let mut merged = if !req.replace && path.exists() {
        hcl_form::read_tfvars_at(&path).unwrap_or_default()
    } else {
        Map::new()
    };
    for (k, v) in &req.values {
        merged.insert(k.clone(), v.clone());
    }
    if let Err(e) = hcl_form::write_tfvars_at(&path, &merged) {
        return err(e);
    }
    // Ghi xong tự chọn file này làm var-file mặc định của workspace.
    let _ = s.db.workspace_update(id, None, None, Some(&req.file), None, None, None, None, None);
    s.db.log(&format!(
        "workspace #{id}: lưu {} ({} biến)",
        req.file,
        merged.len()
    ));
    json!({ "ok": true, "file": req.file, "saved": merged.len(), "values": merged })
}

async fn tfvars_set_h(
    State(s): State<AppState>,
    AxPath(id): AxPath<i64>,
    Json(req): Json<TfvarsSetReq>,
) -> Json<Value> {
    Json(tfvars_set_value(&s, id, &req))
}

// ---- chạy terraform ----

pub const COMMANDS: [&str; 6] = ["init", "validate", "plan", "apply", "destroy", "output"];

/// Dựng chuỗi bước cho một lệnh terraform — tách pure để test được.
/// `repo_dir` là gốc workspace (nơi git pull); `work_dir` là root Terraform
/// (gốc + subdir) — nơi terraform thực chạy.
pub fn build_steps(
    tf_bin: &str,
    repo_dir: &Path,
    work_dir: &Path,
    source: &str,
    auto_sync: bool,
    command: &str,
    var_file: Option<&str>,
) -> Vec<Step> {
    let mut steps = Vec::new();
    let changes_infra = matches!(command, "plan" | "apply" | "destroy");
    // Yêu cầu cốt lõi: workspace clone từ git thì PULL trước khi plan/apply.
    if source == "git" && auto_sync && changes_infra && gitops::is_git_repo(repo_dir) {
        steps.push(Step::new("git", gitops::pull_args(repo_dir), None));
    }
    if command != "init" && !work_dir.join(".terraform").exists() {
        steps.push(Step::new(
            tf_bin,
            vec!["init".into(), "-input=false".into(), "-no-color".into()],
            Some(work_dir.to_path_buf()),
        ));
    }
    let mut args: Vec<String> = match command {
        "init" => vec!["init".into(), "-input=false".into(), "-no-color".into()],
        "validate" => vec!["validate".into(), "-no-color".into()],
        "plan" => vec!["plan".into(), "-input=false".into(), "-no-color".into()],
        "apply" => vec![
            "apply".into(),
            "-input=false".into(),
            "-no-color".into(),
            "-auto-approve".into(),
        ],
        "destroy" => vec![
            "destroy".into(),
            "-input=false".into(),
            "-no-color".into(),
            "-auto-approve".into(),
        ],
        "output" => vec!["output".into(), "-no-color".into()],
        _ => vec![command.into(), "-no-color".into()],
    };
    if changes_infra {
        if let Some(f) = var_file.filter(|f| !f.is_empty()) {
            args.push(format!("-var-file={f}"));
        }
    }
    steps.push(Step::new(tf_bin, args, Some(work_dir.to_path_buf())));
    steps
}

#[derive(Deserialize)]
pub struct RunReq {
    pub command: String,
    pub var_file: Option<String>,
    #[serde(default)]
    pub confirm: bool,
}

pub(crate) async fn run_value(s: &AppState, id: i64, req: &RunReq) -> Value {
    let Some(ws) = s.db.workspace_get(id).ok().flatten() else {
        return err(format!("workspace {id} không tồn tại"));
    };
    if !COMMANDS.contains(&req.command.as_str()) {
        return err(format!("command phải là một trong {COMMANDS:?}"));
    }
    if ws["status"] != "ready" {
        return err(format!(
            "workspace chưa sẵn sàng (status={}) — {}",
            ws["status"],
            ws["last_error"].as_str().unwrap_or("")
        ));
    }
    if matches!(req.command.as_str(), "apply" | "destroy") && !req.confirm {
        return err(format!(
            "{} thay đổi hạ tầng thật — cần confirm=true",
            req.command
        ));
    }
    if let Some(rid) = s.db.running_run(id).ok().flatten() {
        return err(format!("đang có run #{rid} chạy — đợi xong hoặc huỷ trước"));
    }
    let tf_bin = match tfcli::resolve_bin(s.db.setting_get("terraform_bin").ok().flatten()).await {
        Ok(p) => p.to_string_lossy().to_string(),
        Err(e) => return err(e),
    };
    let dir = PathBuf::from(ws["dir"].as_str().unwrap_or_default());
    let wd = work_dir(&ws);
    if !wd.is_dir() {
        return err(format!(
            "thư mục Terraform không tồn tại: {} — chỉnh subdir trong tab Thông tin",
            wd.display()
        ));
    }
    // var-file: ưu tiên tham số, fallback file đã chọn của workspace.
    let var_file = req
        .var_file
        .clone()
        .filter(|f| !f.is_empty())
        .or_else(|| ws["var_file"].as_str().filter(|f| !f.is_empty()).map(String::from));
    if let Some(f) = &var_file {
        let path = match resolve_var_file(&dir, &wd, f) {
            Ok(p) => p,
            Err(e) => return err(e),
        };
        if matches!(req.command.as_str(), "plan" | "apply" | "destroy") && !path.exists() {
            return err(format!("var-file {f} không tồn tại trong workspace"));
        }
    }
    let steps = build_steps(
        &tf_bin,
        &dir,
        &wd,
        ws["source"].as_str().unwrap_or("folder"),
        ws["auto_sync"].as_bool().unwrap_or(true),
        &req.command,
        var_file.as_deref(),
    );
    let labels: Vec<String> = steps.iter().map(|st| st.label.clone()).collect();
    let run_id = match s.db.run_create(Some(id), &req.command) {
        Ok(r) => r,
        Err(e) => return err(e),
    };
    let timeout = s
        .db
        .setting_get("run_timeout_secs")
        .ok()
        .flatten()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3600u64);
    s.runner.spawn_steps(run_id, steps, Duration::from_secs(timeout));
    s.db.log(&format!(
        "workspace #{id}: chạy terraform {} (run #{run_id})",
        req.command
    ));
    json!({ "ok": true, "run_id": run_id, "steps": labels, "var_file": var_file })
}

async fn run_h(
    State(s): State<AppState>,
    AxPath(id): AxPath<i64>,
    Json(req): Json<RunReq>,
) -> Json<Value> {
    Json(run_value(&s, id, &req).await)
}

// ---- runs / console ----

#[derive(Deserialize, Default)]
pub struct RunsQuery {
    pub workspace_id: Option<i64>,
    pub limit: Option<i64>,
}

pub(crate) fn runs_value(s: &AppState, workspace_id: Option<i64>, limit: i64) -> Value {
    match s.db.run_list(workspace_id, limit.clamp(1, 200)) {
        Ok(runs) => json!({ "ok": true, "runs": runs }),
        Err(e) => err(e),
    }
}

async fn runs_h(State(s): State<AppState>, Query(q): Query<RunsQuery>) -> Json<Value> {
    Json(runs_value(&s, q.workspace_id, q.limit.unwrap_or(50)))
}

#[derive(Deserialize, Default)]
pub struct RunGetQuery {
    pub after: Option<i64>,
    pub limit: Option<i64>,
}

pub(crate) fn run_get_value(s: &AppState, id: i64, after: i64, limit: i64) -> Value {
    let Some(run) = s.db.run_get(id).ok().flatten() else {
        return err(format!("run {id} không tồn tại"));
    };
    match s.db.run_lines_after(id, after, limit.clamp(1, 2000)) {
        Ok((lines, next_after)) => json!({
            "ok": true,
            "run": run,
            "lines": lines,
            "next_after": next_after,
        }),
        Err(e) => err(e),
    }
}

async fn run_get_h(
    State(s): State<AppState>,
    AxPath(id): AxPath<i64>,
    Query(q): Query<RunGetQuery>,
) -> Json<Value> {
    Json(run_get_value(&s, id, q.after.unwrap_or(0), q.limit.unwrap_or(1000)))
}

pub(crate) fn run_cancel_value(s: &AppState, id: i64) -> Value {
    if s.runner.cancel(id) {
        json!({ "ok": true })
    } else {
        err("run không còn chạy (hoặc là run cài đặt không huỷ được)")
    }
}

async fn run_cancel_h(State(s): State<AppState>, AxPath(id): AxPath<i64>) -> Json<Value> {
    Json(run_cancel_value(&s, id))
}

/// AI đọc đuôi console và giải thích lỗi bằng tiếng Việt (qua bridge SenClaw).
pub(crate) async fn explain_value(s: &AppState, id: i64) -> Value {
    let Some(run) = s.db.run_get(id).ok().flatten() else {
        return err(format!("run {id} không tồn tại"));
    };
    let tail = s.db.run_tail(id, 120).unwrap_or_default();
    if tail.is_empty() {
        return err("run chưa có output để phân tích");
    }
    let system = "Bạn là kỹ sư DevOps chuyên Terraform. Đọc log lệnh terraform/git dưới đây, \
        giải thích NGẮN GỌN bằng tiếng Việt: (1) lệnh làm gì / lỗi gì, (2) nguyên nhân khả dĩ nhất, \
        (3) các bước sửa cụ thể. Nếu log là kết quả thành công thì tóm tắt thay đổi hạ tầng.";
    let prompt = format!(
        "Lệnh: terraform {} — trạng thái: {}\n\nLog:\n{}",
        run["kind"].as_str().unwrap_or("?"),
        run["status"].as_str().unwrap_or("?"),
        tail
    );
    match s.sc.llm_request_usage(system, &prompt, 2000, None).await {
        Ok(r) => {
            if r.finish == "length" {
                json!({ "ok": true, "text": format!("{}\n\n(⚠ phân tích bị cắt do giới hạn độ dài)", r.text) })
            } else {
                json!({ "ok": true, "text": r.text })
            }
        }
        Err(e) => err(format!("bridge LLM lỗi: {e}")),
    }
}

async fn explain_h(State(s): State<AppState>, AxPath(id): AxPath<i64>) -> Json<Value> {
    Json(explain_value(&s, id).await)
}

// ---- settings / activity ----

const ALLOWED_SETTINGS: [&str; 2] = ["terraform_bin", "run_timeout_secs"];

async fn settings_get_h(State(s): State<AppState>) -> Json<Value> {
    let mut out = Map::new();
    for k in ALLOWED_SETTINGS {
        if let Ok(Some(v)) = s.db.setting_get(k) {
            out.insert(k.into(), json!(v));
        }
    }
    Json(json!({ "ok": true, "settings": out }))
}

async fn settings_set_h(State(s): State<AppState>, Json(body): Json<Map<String, Value>>) -> Json<Value> {
    for (k, v) in &body {
        if !ALLOWED_SETTINGS.contains(&k.as_str()) {
            return Json(err(format!("setting không hỗ trợ: {k}")));
        }
        let text = match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        if let Err(e) = s.db.setting_set(k, &text) {
            return Json(err(e));
        }
    }
    Json(json!({ "ok": true }))
}

async fn activity_h(State(s): State<AppState>) -> Json<Value> {
    Json(json!({ "ok": true, "activity": s.db.activity(100).unwrap_or_default() }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_steps_apply_git_syncs_then_inits_then_applies() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        let steps = build_steps("/usr/local/bin/terraform", dir.path(), dir.path(), "git", true, "apply", Some("prod.tfvars"));
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].program, "git");
        assert!(steps[0].args.contains(&"pull".to_string()));
        assert_eq!(steps[1].args[0], "init");
        let apply = &steps[2];
        assert_eq!(apply.args[0], "apply");
        assert!(apply.args.contains(&"-auto-approve".to_string()));
        assert!(apply.args.contains(&"-var-file=prod.tfvars".to_string()));
        assert_eq!(apply.cwd.as_deref(), Some(dir.path()));
    }

    #[test]
    fn build_steps_folder_plan_no_sync_no_reinit() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".terraform")).unwrap();
        let steps = build_steps("terraform", dir.path(), dir.path(), "folder", true, "plan", None);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].args[0], "plan");
        assert!(!steps[0].args.iter().any(|a| a.starts_with("-var-file")));
        assert!(!steps[0].args.contains(&"-auto-approve".to_string()));
    }

    #[test]
    fn build_steps_git_auto_sync_off_skips_pull() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        std::fs::create_dir_all(dir.path().join(".terraform")).unwrap();
        let steps = build_steps("terraform", dir.path(), dir.path(), "git", false, "plan", None);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].args[0], "plan");
    }

    #[test]
    fn build_steps_validate_and_output_take_no_var_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".terraform")).unwrap();
        for cmd in ["validate", "output"] {
            let steps = build_steps("terraform", dir.path(), dir.path(), "git", true, cmd, Some("prod.tfvars"));
            assert_eq!(steps.len(), 1, "{cmd} không sync/init lại");
            assert!(!steps[0].args.iter().any(|a| a.starts_with("-var-file")), "{cmd}");
        }
    }

    #[test]
    fn build_steps_subdir_pulls_at_root_runs_in_subdir() {
        let repo = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(repo.path().join(".git")).unwrap();
        let wd = repo.path().join("infra/prod");
        std::fs::create_dir_all(&wd).unwrap();
        let steps = build_steps("terraform", repo.path(), &wd, "git", true, "apply", Some("prod.tfvars"));
        assert_eq!(steps.len(), 3);
        // git pull ở GỐC repo…
        assert_eq!(steps[0].program, "git");
        assert!(steps[0].args.iter().any(|a| a == &repo.path().to_string_lossy().to_string()));
        // …còn init + apply chạy trong SUBDIR.
        assert_eq!(steps[1].cwd.as_deref(), Some(wd.as_path()));
        assert_eq!(steps[2].cwd.as_deref(), Some(wd.as_path()));
    }

    #[test]
    fn subdir_validation() {
        assert!(validate_subdir("").is_ok());
        assert!(validate_subdir("terraform").is_ok());
        assert!(validate_subdir("infra/prod").is_ok());
        assert!(validate_subdir("infra/prod/").is_ok());
        assert!(validate_subdir("/etc").is_err());
        assert!(validate_subdir("../thoat").is_err());
        assert!(validate_subdir("a/../../b").is_err());
        assert!(validate_subdir("a//b").is_err());
    }

    #[test]
    fn resolve_var_file_plain_and_relative() {
        let root = tempfile::tempdir().unwrap();
        let wd = root.path().join("deployments/terraform");
        std::fs::create_dir_all(&wd).unwrap();
        std::fs::create_dir_all(root.path().join("environments")).unwrap();
        std::fs::write(root.path().join("environments/prod.tfvars"), "a=1\n").unwrap();

        // Tên trần → nằm trong work_dir.
        let p = resolve_var_file(root.path(), &wd, "prod.tfvars").unwrap();
        assert_eq!(p, wd.join("prod.tfvars"));

        // Đường dẫn tương đối ../../ vẫn trong workspace → OK.
        let p = resolve_var_file(root.path(), &wd, "../../environments/prod.tfvars").unwrap();
        assert!(p.ends_with("environments/prod.tfvars"));
        assert!(p.starts_with(root.path().canonicalize().unwrap()));

        // Thoát ra ngoài workspace → chặn.
        assert!(resolve_var_file(root.path(), &wd, "../../../../etc/x.tfvars").is_err());
        // Đuôi sai → chặn. Tuyệt đối → chặn. Thành phần ẩn → chặn.
        assert!(resolve_var_file(root.path(), &wd, "../../environments/prod.txt").is_err());
        assert!(resolve_var_file(root.path(), &wd, "/etc/x.tfvars").is_err());
        assert!(resolve_var_file(root.path(), &wd, "../.git/x.tfvars").is_err());
    }

    #[test]
    fn tfvars_deep_scan_and_rel() {
        let root = tempfile::tempdir().unwrap();
        let rootc = root.path().canonicalize().unwrap();
        let wd = rootc.join("deployments/terraform");
        std::fs::create_dir_all(&wd).unwrap();
        std::fs::create_dir_all(rootc.join("environments")).unwrap();
        std::fs::write(rootc.join("environments/prod.tfvars"), "a=1\n").unwrap();
        std::fs::write(wd.join("dev.tfvars"), "a=2\n").unwrap();
        std::fs::write(rootc.join("ghi-chú.txt"), "x").unwrap();

        let files = tfvars_files_deep(&rootc);
        assert_eq!(files.len(), 2);
        let rels: Vec<String> = files.iter().map(|p| rel_between(&wd, p)).collect();
        assert!(rels.contains(&"dev.tfvars".to_string()));
        assert!(rels.contains(&"../../environments/prod.tfvars".to_string()));
    }

    #[test]
    fn tf_subdirs_finds_nested_roots() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("infra/prod")).unwrap();
        std::fs::create_dir_all(dir.path().join("infra/dev")).unwrap();
        std::fs::create_dir_all(dir.path().join("docs")).unwrap();
        std::fs::create_dir_all(dir.path().join(".git/objects")).unwrap();
        std::fs::write(dir.path().join("infra/prod/main.tf"), "x").unwrap();
        std::fs::write(dir.path().join("infra/dev/main.tf"), "x").unwrap();
        std::fs::write(dir.path().join("docs/a.md"), "x").unwrap();
        assert_eq!(tf_subdirs(dir.path()), vec!["infra/dev", "infra/prod"]);
    }

    #[test]
    fn fs_list_marks_tf_dirs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("infra")).unwrap();
        std::fs::create_dir_all(dir.path().join("docs")).unwrap();
        std::fs::write(dir.path().join("infra/main.tf"), "x").unwrap();
        let v = fs_list_value(Some(dir.path().to_string_lossy().to_string()), false);
        assert_eq!(v["ok"], true);
        let entries = v["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 2);
        // Thư mục có .tf nổi lên đầu.
        assert_eq!(entries[0]["name"], "infra");
        assert_eq!(entries[0]["has_tf"], true);
        assert_eq!(entries[1]["has_tf"], false);
    }
}
