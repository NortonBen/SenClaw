//! Client for the SenClaw daemon's workflow engine.
//!
//! Video Flow keeps its own DAG engine (`dag.rs`) for the legacy pipeline, but
//! the workflow engine is what gives us per-scene parallelism, per-node retry
//! and a run UI we don't have to build. This module owns the HTTP calls; the
//! definition itself is generated in `wfdef.rs` and the work is executed by the
//! blocking step endpoints in `steps.rs`.
//!
//! Run records come back shaped by `crate::workflow::types::WorkflowRun` on the
//! daemon side, which is `camelCase`: `{id, workflowName, label?, inputs,
//! status, runDir, steps: [{id, kind, dependsOn, status, result, error?,
//! startedAt?, completedAt?}], trigger?, createdAt, completedAt?}`.

use serde_json::{json, Value};

use crate::db;
use crate::state::AppState;

/// app_kv key holding `"<workflow name>\n<run id>"` for a project. One slot per
/// project: re-running replaces it, which is what the UI wants (the older run
/// is still in the daemon's run history).
fn kv_key(project_id: &str) -> String {
    format!("workflow.run:{project_id}")
}

fn api(path: &str) -> String {
    format!("{}/api/workflows{}", crate::llm::base_url().trim_end_matches('/'), path)
}

/// Send a request and unwrap the daemon's `{"error": …}` convention. Any
/// non-2xx carries a body worth surfacing verbatim — a 400 here is almost
/// always a malformed definition, and the message names the offending step.
async fn send(req: reqwest::RequestBuilder) -> Result<Value, String> {
    let resp = req.send().await.map_err(|e| format!("workflow engine không phản hồi: {e}"))?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    let v: Value = if body.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&body).unwrap_or(Value::String(body.clone()))
    };
    if !status.is_success() {
        let msg = v
            .get("error")
            .and_then(|e| e.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| body.clone());
        return Err(format!("workflow engine {status}: {msg}"));
    }
    Ok(v)
}

/// Register (or overwrite) a definition. Returns the workflow name the engine
/// parsed out of the front matter — trust that, not our local guess.
pub async fn register_def(content: &str) -> Result<String, String> {
    let v = send(
        crate::llm::http()
            .post(api(""))
            .json(&json!({ "content": content, "overwrite": true })),
    )
    .await?;
    v.get("name")
        .and_then(|n| n.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "workflow engine không trả về name".to_string())
}

pub async fn start_run(name: &str) -> Result<String, String> {
    let v = send(
        crate::llm::http()
            .post(api(&format!("/{name}/run")))
            .json(&json!({ "inputs": {} })),
    )
    .await?;
    v.get("runId")
        .and_then(|n| n.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "workflow engine không trả về runId".to_string())
}

/// The run record itself (the daemon wraps it in `{"run": …}`; unwrapped here
/// so callers always see the same shape).
pub async fn get_run(run_id: &str) -> Result<Value, String> {
    let v = send(crate::llm::http().get(api(&format!("/runs/{run_id}")))).await?;
    Ok(v.get("run").cloned().unwrap_or(v))
}

pub async fn list_runs() -> Result<Value, String> {
    let v = send(crate::llm::http().get(api("/runs"))).await?;
    Ok(v.get("runs").cloned().unwrap_or(v))
}

pub async fn cancel_run(run_id: &str) -> Result<(), String> {
    send(crate::llm::http().post(api(&format!("/runs/{run_id}/cancel")))).await?;
    Ok(())
}

pub async fn run_activity(run_id: &str) -> Result<Value, String> {
    let v = send(crate::llm::http().get(api(&format!("/runs/{run_id}/activity")))).await?;
    Ok(v.get("entries").cloned().unwrap_or(v))
}

// ---------- project ↔ run binding ----------

pub fn remember_run(st: &AppState, project_id: &str, workflow: &str, run_id: &str) {
    let _ = st.core.db.kv_set(&kv_key(project_id), &format!("{workflow}\n{run_id}"));
}

/// `(workflow_name, run_id)` of the project's last launched run, if any.
pub fn stored_run(st: &AppState, project_id: &str) -> Option<(String, String)> {
    let raw = st.core.db.kv_get(&kv_key(project_id));
    let mut parts = raw.splitn(2, '\n');
    let wf = parts.next().unwrap_or("").trim().to_string();
    let run = parts.next().unwrap_or("").trim().to_string();
    if run.is_empty() {
        None
    } else {
        Some((wf, run))
    }
}

/// Build the def for a project, register it, start a run.
/// Returns `(workflow_name, run_id)`.
pub async fn launch_project_workflow(
    st: &AppState,
    project_id: &str,
    video_id: &str,
    orientation: &str,
    with_audio: bool,
    with_critic: bool,
) -> Result<(String, String), String> {
    let db_ = &st.core.db;
    let project = match db_.get("project", project_id) {
        Ok(Some(p)) => p,
        Ok(None) => return Err(format!("project {project_id} không tồn tại")),
        Err(e) => return Err(e.to_string()),
    };

    let video = if video_id.trim().is_empty() {
        match db_
            .query_one(
                "SELECT * FROM video WHERE project_id = ?1 ORDER BY display_order, created_at LIMIT 1",
                &[&project_id],
            )
            .map_err(|e| e.to_string())?
        {
            Some(v) => v,
            // A project with no video is the normal starting point (a fresh or
            // duplicated project). Refusing to run made the user do by hand
            // what the pipeline exists to do — create it and carry on; the
            // workflow's own `script_parser` fills in the scenes.
            None => {
                let mut row = db::Row::new();
                row.insert("project_id".into(), serde_json::json!(project_id));
                row.insert(
                    "title".into(),
                    serde_json::json!(format!(
                        "{} — Video 1",
                        db::str_of(&project, "name")
                    )),
                );
                row.insert("display_order".into(), serde_json::json!(1));
                row.insert(
                    "orientation".into(),
                    serde_json::json!(if orientation.trim().is_empty() {
                        crate::config::default_orientation()
                    } else {
                        orientation.trim().to_uppercase()
                    }),
                );
                let new_id = db_.insert("video", &row).map_err(|e| e.to_string())?;
                println!("[workflow] project {project_id} chưa có video — đã tạo {new_id}");
                db_.get("video", &new_id)
                    .map_err(|e| e.to_string())?
                    .ok_or_else(|| "không tạo được video".to_string())?
            }
        }
    } else {
        match db_.get("video", video_id.trim()) {
            Ok(Some(v)) => v,
            Ok(None) => return Err(format!("video {video_id} không tồn tại")),
            Err(e) => return Err(e.to_string()),
        }
    };
    let vid = db::str_of(&video, "id");

    let scenes = db_
        .query("SELECT * FROM scene WHERE video_id = ?1 ORDER BY display_order", &[&vid])
        .map_err(|e| e.to_string())?;

    // One workspace per project: the engine allows a single run per workspace,
    // so a shared dir would serialize unrelated projects.
    let workspace = crate::config::data_dir().join("workflow").join(project_id);
    std::fs::create_dir_all(&workspace)
        .map_err(|e| format!("không tạo được workspace {}: {e}", workspace.display()))?;

    let orientation = if orientation.trim().is_empty() {
        crate::config::default_orientation()
    } else {
        orientation.trim().to_uppercase()
    };

    let endpoint = crate::wfdef::StepEndpoint {
        base_url: format!("http://127.0.0.1:{}", crate::config::http_port()),
    };
    let def = crate::wfdef::build(&crate::wfdef::BuildOpts {
        project: &project,
        video: &video,
        scenes: &scenes,
        orientation: &orientation,
        endpoint: &endpoint,
        workspace: workspace.to_string_lossy().to_string(),
        with_audio,
        with_critic,
        // `script_parser` runs inside the workflow and can produce a different
        // number of scenes than exist right now. Provision a floor of slots so
        // a freshly-parsed script still gets rendered; surplus slots skip.
        scene_slots: scenes.len().max(crate::config::scene_slots_min()),
    });

    // A new run must not inherit pre-production output from an older one.
    crate::steps::clear_stage_results(st, project_id);

    let name = register_def(&def).await?;
    let run_id = start_run(&name).await?;
    remember_run(st, project_id, &name, &run_id);
    Ok((name, run_id))
}

/// Launch a workflow the user assembled from ordered stages of agents.
pub async fn launch_custom_workflow(
    st: &AppState,
    project_id: &str,
    video_id: &str,
    orientation: &str,
    stages: &[Vec<String>],
) -> Result<(String, String), String> {
    if stages.iter().all(|s| s.iter().all(|a| a.trim().is_empty())) {
        return Err("workflow rỗng — hãy thêm ít nhất một agent".to_string());
    }
    // Reject anything not in the builder's known set (also guards the id/curl).
    let known = crate::wfdef::known_agent_types();
    for stage in stages {
        for a in stage {
            let a = a.trim();
            if !a.is_empty() && !known.contains(&a) {
                return Err(format!("agent không hợp lệ: `{a}`"));
            }
        }
    }

    let db_ = &st.core.db;
    let project = match db_.get("project", project_id) {
        Ok(Some(p)) => p,
        Ok(None) => return Err(format!("project {project_id} không tồn tại")),
        Err(e) => return Err(e.to_string()),
    };
    let video = ensure_video(db_, &project, project_id, video_id, orientation)?;
    let vid = db::str_of(&video, "id");
    let scenes = db_
        .query("SELECT * FROM scene WHERE video_id = ?1 ORDER BY display_order", &[&vid])
        .map_err(|e| e.to_string())?;

    let workspace = crate::config::data_dir().join("workflow").join(project_id);
    std::fs::create_dir_all(&workspace)
        .map_err(|e| format!("không tạo được workspace {}: {e}", workspace.display()))?;
    let orientation = if orientation.trim().is_empty() {
        crate::config::default_orientation()
    } else {
        orientation.trim().to_uppercase()
    };

    let endpoint = crate::wfdef::StepEndpoint {
        base_url: format!("http://127.0.0.1:{}", crate::config::http_port()),
    };
    let def = crate::wfdef::build_custom(
        stages,
        &crate::wfdef::CustomBuildOpts {
            project: &project,
            video: &video,
            scenes: &scenes,
            orientation: &orientation,
            endpoint: &endpoint,
            workspace: workspace.to_string_lossy().to_string(),
            scene_slots: scenes.len().max(crate::config::scene_slots_min()),
        },
    );

    crate::steps::clear_stage_results(st, project_id);
    let name = register_def(&def).await?;
    let run_id = start_run(&name).await?;
    remember_run(st, project_id, &name, &run_id);
    Ok((name, run_id))
}

/// Get the project's video, creating a default one when the project has none —
/// the normal starting point for a fresh project. Shared by both launchers.
fn ensure_video(
    db_: &db::Db,
    project: &db::Row,
    project_id: &str,
    video_id: &str,
    orientation: &str,
) -> Result<db::Row, String> {
    if !video_id.trim().is_empty() {
        return match db_.get("video", video_id.trim()) {
            Ok(Some(v)) => Ok(v),
            Ok(None) => Err(format!("video {video_id} không tồn tại")),
            Err(e) => Err(e.to_string()),
        };
    }
    if let Some(v) = db_
        .query_one(
            "SELECT * FROM video WHERE project_id = ?1 ORDER BY display_order, created_at LIMIT 1",
            &[&project_id],
        )
        .map_err(|e| e.to_string())?
    {
        return Ok(v);
    }
    let mut row = db::Row::new();
    row.insert("project_id".into(), serde_json::json!(project_id));
    row.insert("title".into(), serde_json::json!(format!("{} — Video 1", db::str_of(project, "name"))));
    row.insert("display_order".into(), serde_json::json!(1));
    row.insert(
        "orientation".into(),
        serde_json::json!(if orientation.trim().is_empty() {
            crate::config::default_orientation()
        } else {
            orientation.trim().to_uppercase()
        }),
    );
    let new_id = db_.insert("video", &row).map_err(|e| e.to_string())?;
    db_.get("video", &new_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "không tạo được video".to_string())
}

// ---------- reporting ----------

/// Compact view of a run: enough for an agent to say "7/12 xong, cảnh 3 lỗi"
/// without dumping every step's stdout into the transcript.
pub fn summarize_run(run: &Value) -> Value {
    let steps = run.get("steps").and_then(|s| s.as_array()).cloned().unwrap_or_default();
    let mut tally: std::collections::BTreeMap<String, i64> = Default::default();
    let mut nodes = Vec::new();
    let mut failed = Vec::new();
    for st in &steps {
        let id = st.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let status = st.get("status").and_then(|v| v.as_str()).unwrap_or("pending").to_string();
        *tally.entry(status.clone()).or_insert(0) += 1;
        if status == "failed" {
            // The error is the one field worth carrying through verbatim.
            failed.push(json!({
                "id": id,
                "error": st.get("error").and_then(|v| v.as_str()).unwrap_or(""),
            }));
        }
        nodes.push(json!({ "id": id, "status": status }));
    }
    let done = tally.get("done").copied().unwrap_or(0);
    let counts: serde_json::Map<String, Value> =
        tally.into_iter().map(|(k, v)| (k, json!(v))).collect();
    json!({
        "run_id": run.get("id").and_then(|v| v.as_str()).unwrap_or(""),
        "workflow": run.get("workflowName").and_then(|v| v.as_str()).unwrap_or(""),
        "status": run.get("status").and_then(|v| v.as_str()).unwrap_or(""),
        "created_at": run.get("createdAt").and_then(|v| v.as_str()).unwrap_or(""),
        "completed_at": run.get("completedAt").and_then(|v| v.as_str()).unwrap_or(""),
        "progress": format!("{done}/{}", steps.len()),
        "counts": Value::Object(counts),
        "nodes": nodes,
        "failed": failed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_json() -> Value {
        json!({
            "id": "run-1",
            "workflowName": "video-flow-abc",
            "status": "running",
            "runDir": "/tmp/ws",
            "createdAt": "2026-07-19T00:00:00Z",
            "steps": [
                {"id": "parse", "kind": "script", "status": "done", "result": "ok"},
                {"id": "img_0", "kind": "script", "status": "done", "result": "ok"},
                {"id": "img_1", "kind": "script", "status": "failed", "result": "",
                 "error": "Chrome extension chưa kết nối"},
                {"id": "vid_0", "kind": "script", "status": "running", "result": ""},
                {"id": "concat", "kind": "script", "status": "pending", "result": ""}
            ]
        })
    }

    #[test]
    fn summary_counts_by_status() {
        let s = summarize_run(&run_json());
        assert_eq!(s["counts"]["done"], json!(2));
        assert_eq!(s["counts"]["failed"], json!(1));
        assert_eq!(s["counts"]["running"], json!(1));
        assert_eq!(s["counts"]["pending"], json!(1));
        assert_eq!(s["progress"], json!("2/5"));
    }

    /// The node list is id+status only — a run has 2N+6 nodes and each `result`
    /// can be a whole curl body, which is what we're keeping out of the reply.
    #[test]
    fn summary_nodes_are_compact() {
        let s = summarize_run(&run_json());
        let nodes = s["nodes"].as_array().unwrap();
        assert_eq!(nodes.len(), 5);
        assert_eq!(nodes[0], json!({"id": "parse", "status": "done"}));
        assert!(nodes.iter().all(|n| n.as_object().unwrap().len() == 2));
    }

    #[test]
    fn summary_surfaces_failed_node_errors() {
        let s = summarize_run(&run_json());
        let failed = s["failed"].as_array().unwrap();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0]["id"], json!("img_1"));
        assert!(failed[0]["error"].as_str().unwrap().contains("extension"));
    }

    #[test]
    fn summary_tolerates_a_run_without_steps() {
        let s = summarize_run(&json!({"id": "r", "workflowName": "w", "status": "done"}));
        assert_eq!(s["progress"], json!("0/0"));
        assert_eq!(s["nodes"], json!([]));
        assert_eq!(s["completed_at"], json!(""));
    }
}
