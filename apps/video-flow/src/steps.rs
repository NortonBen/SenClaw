//! Blocking step endpoints — the unit of work a SenClaw workflow script step
//! invokes with `curl --fail`.
//!
//! The workflow engine orchestrates (DAG, parallelism, retry, cancel, run
//! history) while the domain logic stays here: the app's own agent pool with
//! its `souls/` prompts, and `process::` for anything that goes through the
//! Google Flow extension. A step blocks until its unit is done and answers
//! 200 / 500 so the engine can mark the node done or failed.

use crate::agents::Task;
use crate::db;
use crate::state::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

fn ok(v: Value) -> Response {
    (StatusCode::OK, Json(v)).into_response()
}

/// Failures must be non-2xx: `curl --fail` is what turns a broken unit of work
/// into a failed workflow node.
fn fail(msg: impl Into<String>) -> Response {
    let msg = msg.into();
    eprintln!("[step] {msg}");
    (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": msg }))).into_response()
}

#[derive(Deserialize, Default)]
pub struct AgentStepBody {
    #[serde(default)]
    pub agent_type: String,
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub video_id: String,
    /// Optional JSON/text prompt handed to the agent verbatim.
    #[serde(default)]
    pub prompt: String,
    /// Feed the project's story/screenplay as the prompt.
    ///
    /// `script_parser` parses whatever prompt it receives. Handing it the
    /// `{"video_id":…}` stub made it delete every scene and then "parse" that
    /// stub — so this flag exists to make the real screenplay the input, and
    /// the step fails loudly when there is none.
    #[serde(default)]
    pub use_project_story: bool,
    #[serde(default)]
    pub timeout_seconds: i64,
}

/// `POST /api/steps/agent` — run one pipeline agent to completion.
///
/// This is how planning/synthesis stages (director, screenwriter, script_parser,
/// gen_ref, concat, …) run under the workflow engine while keeping their
/// app-local soul prompts.
pub async fn run_agent_step(State(st): State<AppState>, Json(b): Json<AgentStepBody>) -> Response {
    let agent_type = b.agent_type.trim().to_string();
    if agent_type.is_empty() {
        return fail("agent_type là bắt buộc");
    }
    if b.project_id.trim().is_empty() {
        return fail("project_id là bắt buộc");
    }
    if st.pool.get(&agent_type).is_none() {
        return fail(format!("không có agent `{agent_type}`"));
    }

    // A disabled built-in is a deliberate skip, not a failure — mirror the DAG
    // engine so both orchestrators agree.
    if st.core.db.builtin_agent_disabled(&agent_type) {
        return ok(json!({ "status": "skipped", "agent_type": agent_type }));
    }

    let mut prompt = b.prompt.clone();
    if prompt.trim().is_empty() && b.use_project_story {
        let project = st.core.db.get("project", &b.project_id).ok().flatten().unwrap_or_default();
        for field in ["story", "story_original", "description"] {
            let v = db::str_of(&project, field);
            if !v.trim().is_empty() {
                prompt = v;
                break;
            }
        }
        if prompt.trim().is_empty() {
            return fail(format!(
                "{agent_type}: project chưa có story/kịch bản — nhập nội dung trước khi chạy pipeline"
            ));
        }
    }
    if prompt.trim().is_empty() && !b.video_id.trim().is_empty() {
        prompt = json!({ "video_id": b.video_id }).to_string();
    }

    let task = Task {
        id: db::new_id(),
        label: agent_type.clone(),
        agent_type: agent_type.clone(),
        prompt,
        timeout_seconds: if b.timeout_seconds > 0 { b.timeout_seconds } else { 900 },
        // Each workflow step is its own process, so there is no in-memory
        // upstream to inherit — replay what earlier stages persisted.
        upstream_results: load_stage_results(&st, &b.project_id),
    };

    match st.pool.execute(&task, "", &b.project_id).await {
        Ok(r) => {
            save_stage_result(&st, &b.project_id, &agent_type, &r.data);
            ok(json!({
                "status": "done",
                "agent_type": agent_type,
                "summary": r.summary,
                "data": r.data,
            }))
        }
        Err(e) => fail(format!("{agent_type}: {e}")),
    }
}

/// Stages whose output later stages read (director blocks, screenplay, shot
/// list, environments, character DNA). Kept small on purpose: these are the
/// only results `resolve_by_field` looks for.
const RELAYED_STAGES: &[&str] = &[
    "director",
    "screenwriter",
    "scene_plan",
    "shot_design",
    "visual_asset",
    "script_parser",
    "scene_builder",
];

fn stage_key(project_id: &str, agent_type: &str) -> String {
    format!("stage:{project_id}:{agent_type}")
}

fn save_stage_result(st: &AppState, project_id: &str, agent_type: &str, data: &serde_json::Map<String, Value>) {
    if !RELAYED_STAGES.contains(&agent_type) || data.is_empty() {
        return;
    }
    let raw = Value::Object(data.clone()).to_string();
    let _ = st.core.db.kv_set(&stage_key(project_id, agent_type), &raw);
}

/// Prior stage outputs for this project, keyed by agent type.
///
/// `resolve_by_field` matches on the *field* it needs (`scene_blocks`,
/// `screenplay`, `shots`, …) and falls back to scanning every entry, so the key
/// only has to be stable — it does not have to match the DAG label.
fn load_stage_results(st: &AppState, project_id: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for stage in RELAYED_STAGES {
        let raw = st.core.db.kv_get(&stage_key(project_id, stage));
        if !raw.trim().is_empty() {
            out.insert((*stage).to_string(), raw);
        }
    }
    out
}

/// Drop a project's relayed stage results — called when a fresh run starts so
/// stale pre-production data can't leak into it.
pub fn clear_stage_results(st: &AppState, project_id: &str) {
    for stage in RELAYED_STAGES {
        let _ = st.core.db.kv_set(&stage_key(project_id, stage), "");
    }
}

#[derive(Deserialize, Default)]
pub struct SceneStepBody {
    /// `image` | `video` | `upscale`
    #[serde(default)]
    pub op: String,
    #[serde(default)]
    pub scene_id: String,
    /// Address a scene by position instead of id. Required for workflow runs:
    /// `script_parser` deletes and recreates scenes, so any id baked into a
    /// definition before it ran is stale by the time the node executes.
    #[serde(default)]
    pub video_id: String,
    #[serde(default)]
    pub scene_index: Option<i64>,
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub orientation: String,
    #[serde(default)]
    pub regenerate: bool,
}

/// `POST /api/steps/scene` — generate one scene's image / video / upscale.
///
/// One scene per call is the whole point: the workflow fans these out so N
/// clips render concurrently instead of one 20-minute serial loop, and a scene
/// that fails is retried on its own without redoing the rest.
pub async fn run_scene_step(State(st): State<AppState>, Json(mut b): Json<SceneStepBody>) -> Response {
    // Resolve position → id at execution time, so a definition written before
    // `script_parser` ran still targets the right scene.
    if b.scene_id.trim().is_empty() {
        let (Some(idx), false) = (b.scene_index, b.video_id.trim().is_empty()) else {
            return fail("cần scene_id, hoặc video_id + scene_index");
        };
        let scenes = st
            .core
            .db
            .query(
                "SELECT id FROM scene WHERE video_id = ?1 ORDER BY display_order, created_at",
                &[&b.video_id],
            )
            .unwrap_or_default();
        match scenes.get(idx.max(0) as usize) {
            Some(row) => b.scene_id = db::str_of(row, "id"),
            // Fewer scenes than nodes is normal when the count is provisioned
            // ahead of parsing — skip, don't fail the run.
            None => {
                return ok(json!({
                    "status": "skipped",
                    "reason": format!("video chỉ có {} cảnh, không có cảnh #{}", scenes.len(), idx),
                }))
            }
        }
    }
    let project_id = if b.project_id.trim().is_empty() {
        // Resolve from the scene so a step definition only needs the scene id.
        match st.core.db.get("scene", &b.scene_id) {
            Ok(Some(sc)) => {
                let vid = db::str_of(&sc, "video_id");
                st.core
                    .db
                    .get("video", &vid)
                    .ok()
                    .flatten()
                    .map(|v| db::str_of(&v, "project_id"))
                    .unwrap_or_default()
            }
            _ => String::new(),
        }
    } else {
        b.project_id.clone()
    };
    if project_id.is_empty() {
        return fail(format!("không xác định được project cho scene {}", b.scene_id));
    }

    let orientation = if b.orientation.trim().is_empty() {
        crate::config::default_orientation()
    } else {
        b.orientation.clone()
    };

    // Every op needs the browser bridge; say so plainly instead of failing deep
    // inside the Flow call.
    if !st.core.ext.is_connected() {
        return fail("Chrome extension chưa kết nối (cần cho sinh ảnh/video)");
    }

    let out = match b.op.trim() {
        "image" => {
            crate::process::scene_image(&st.core, &b.scene_id, &project_id, &orientation, b.regenerate, None)
                .await
        }
        "video" => {
            crate::process::scene_video(&st.core, &b.scene_id, &project_id, &orientation, b.regenerate).await
        }
        "upscale" => {
            crate::process::upscale_video(&st.core, &b.scene_id, &project_id, &orientation).await
        }
        other => return fail(format!("op không hợp lệ: `{other}` (image|video|upscale)")),
    };

    match out {
        Ok(g) => ok(json!({
            "status": "done",
            "op": b.op,
            "scene_id": b.scene_id,
            "media_id": g.media_id,
            "url": g.url,
        })),
        Err(e) => fail(format!("scene {} {}: {e}", b.scene_id, b.op)),
    }
}

#[derive(Deserialize, Default)]
pub struct CatchupStepBody {
    #[serde(default)]
    pub video_id: String,
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub orientation: String,
    /// First scene position this node is responsible for — i.e. the number of
    /// per-scene slots the definition provisioned.
    #[serde(default)]
    pub from_index: usize,
}

/// `POST /api/steps/catchup` — render every scene past the provisioned slots.
///
/// Slot count is fixed when the definition is written, but `script_parser` runs
/// *inside* the run and can produce more scenes than that. Without this node
/// the surplus scenes have nobody to render them and sit at PENDING while the
/// run reports success — a silent under-delivery. Serial by design: the
/// overflow is normally a couple of scenes, and correctness beats speed here.
pub async fn run_catchup_step(State(st): State<AppState>, Json(b): Json<CatchupStepBody>) -> Response {
    if b.video_id.trim().is_empty() {
        return fail("cần video_id");
    }
    let scenes = st
        .core
        .db
        .query(
            "SELECT id FROM scene WHERE video_id = ?1 ORDER BY display_order, created_at",
            &[&b.video_id],
        )
        .unwrap_or_default();
    if scenes.len() <= b.from_index {
        return ok(json!({
            "status": "skipped",
            "reason": format!("{} cảnh, đã có node phụ trách hết", scenes.len()),
        }));
    }

    let extra: Vec<String> = scenes[b.from_index..].iter().map(|r| db::str_of(r, "id")).collect();
    let project_id = if b.project_id.trim().is_empty() {
        st.core
            .db
            .get("video", &b.video_id)
            .ok()
            .flatten()
            .map(|v| db::str_of(&v, "project_id"))
            .unwrap_or_default()
    } else {
        b.project_id.clone()
    };
    if project_id.is_empty() {
        return fail(format!("không xác định được project cho video {}", b.video_id));
    }
    let orientation = if b.orientation.trim().is_empty() {
        crate::config::default_orientation()
    } else {
        b.orientation.clone()
    };
    if !st.core.ext.is_connected() {
        return fail("Chrome extension chưa kết nối (cần cho sinh ảnh/video)");
    }

    eprintln!(
        "[step] catchup: {} cảnh vượt {} slot đã cấp",
        extra.len(),
        b.from_index
    );
    let mut done = 0usize;
    let mut errors: Vec<String> = Vec::new();
    for scene_id in &extra {
        if let Err(e) =
            crate::process::scene_image(&st.core, scene_id, &project_id, &orientation, false, None).await
        {
            errors.push(format!("cảnh {scene_id} ảnh: {e}"));
            continue;
        }
        match crate::process::scene_video(&st.core, scene_id, &project_id, &orientation, false).await {
            Ok(_) => done += 1,
            Err(e) => errors.push(format!("cảnh {scene_id} video: {e}")),
        }
    }
    if !errors.is_empty() {
        return fail(format!("bù {done}/{} cảnh; lỗi: {}", extra.len(), errors.join("; ")));
    }
    ok(json!({ "status": "done", "rendered": done, "from_index": b.from_index }))
}

#[derive(Deserialize, Default)]
pub struct EntityStepBody {
    #[serde(default)]
    pub project_id: String,
    /// Empty = every entity in the project that still lacks a reference image.
    #[serde(default)]
    pub character_id: String,
    #[serde(default)]
    pub regenerate: bool,
}

/// `POST /api/steps/entity` — reference image for one entity, or all missing.
pub async fn run_entity_step(State(st): State<AppState>, Json(b): Json<EntityStepBody>) -> Response {
    if b.project_id.trim().is_empty() {
        return fail("project_id là bắt buộc");
    }
    if !st.core.ext.is_connected() {
        return fail("Chrome extension chưa kết nối (cần cho sinh ảnh tham chiếu)");
    }
    if b.character_id.trim().is_empty() {
        return match crate::process::process_all_entities(&st.core, &b.project_id).await {
            Ok(n) => ok(json!({ "status": "done", "generated": n })),
            Err(e) => fail(format!("gen refs: {e}")),
        };
    }
    match crate::process::entity_image(&st.core, &b.character_id, &b.project_id, b.regenerate).await {
        Ok(g) => ok(json!({
            "status": "done",
            "character_id": b.character_id,
            "media_id": g.media_id,
            "url": g.url,
        })),
        Err(e) => fail(format!("entity {}: {e}", b.character_id)),
    }
}
