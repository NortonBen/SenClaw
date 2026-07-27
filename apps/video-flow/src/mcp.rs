//! MCP server (HTTP + SSE JSON-RPC) exposing the whole Video Flow pipeline to
//! SenClaw agents — projects/videos/scenes/characters CRUD, DAG pipeline
//! create/control/status, image/video/upscale generation via the Google Flow
//! extension bridge, request-queue inspection and souls editing.
//!
//! Long-running generation is NEVER awaited inside a tool call: image/video/
//! upscale tools spawn the work on tokio and return immediately; progress is
//! tracked through `vf_pipeline_status`, `vf_scene_get` and
//! `vf_requests_status` (plus the dashboard WS events `process` emits).

use axum::{
    extract::State,
    response::sse::{Event, Sse},
    Json,
};
use futures_util::stream::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::convert::Infallible;

use crate::db::{self, Row};
use crate::state::AppState;

#[derive(Deserialize, Debug)]
pub struct JsonRpcRequest {
    #[allow(dead_code)]
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    pub params: Option<Value>,
}

pub async fn mcp_sse(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = state.mcp_tx.subscribe();
    let stream = async_stream::stream! {
        yield Ok(Event::default().event("endpoint").data("/api/mcp/message".to_string()));
        while let Ok(msg) = rx.recv().await {
            yield Ok(Event::default().event("message").data(msg));
        }
    };
    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default())
}

fn text_result(text: String) -> Value {
    json!({ "content": [{ "type": "text", "text": text }] })
}
fn json_result(v: Value) -> Value {
    text_result(serde_json::to_string_pretty(&v).unwrap_or_default())
}
fn error_result(text: String) -> Value {
    json!({ "isError": true, "content": [{ "type": "text", "text": text }] })
}

pub async fn mcp_message(
    State(state): State<AppState>,
    Json(req): Json<JsonRpcRequest>,
) -> Json<Value> {
    let reply = |result: Value| -> Json<Value> {
        let resp = json!({ "jsonrpc": "2.0", "id": req.id, "result": result });
        let _ = state.mcp_tx.send(resp.to_string());
        Json(resp)
    };
    match req.method.as_str() {
        "initialize" => reply(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "video-flow-mcp", "version": "1.0.0" }
        })),
        "ping" => reply(json!({})),
        "notifications/initialized" => Json(json!({ "jsonrpc": "2.0", "id": req.id, "result": {} })),
        "tools/list" => reply(json!({ "tools": tools_list() })),
        "tools/call" => {
            let params = req.params.clone().unwrap_or_default();
            let name = params["name"].as_str().unwrap_or("").to_string();
            let args = params["arguments"].clone();
            reply(call_tool(&state, &name, &args).await)
        }
        _ => Json(json!("ok")),
    }
}

// ---- small arg / row helpers -------------------------------------------------

fn s(args: &Value, k: &str) -> String {
    args[k].as_str().unwrap_or("").trim().to_string()
}
fn flag(args: &Value, k: &str) -> bool {
    args[k].as_bool().unwrap_or(false)
}
fn int(args: &Value, k: &str, d: i64) -> i64 {
    args[k].as_i64().unwrap_or(d)
}

/// Copy only the provided keys out of `args` into a DB patch row.
fn patch_from(args: &Value, keys: &[&str]) -> Row {
    let mut m = Row::new();
    if let Some(o) = args.as_object() {
        for k in keys {
            if let Some(v) = o.get(*k) {
                if !v.is_null() {
                    m.insert((*k).to_string(), v.clone());
                }
            }
        }
    }
    m
}

fn row_value(r: Row) -> Value {
    Value::Object(r)
}
fn rows_value(rs: Vec<Row>) -> Value {
    Value::Array(rs.into_iter().map(Value::Object).collect())
}

/// `run_id`, or the project's last launched run. Agents mostly hold a
/// project_id, not a run id.
fn resolve_run_id(state: &AppState, args: &Value) -> Result<String, String> {
    let id = s(args, "run_id");
    if !id.is_empty() {
        return Ok(id);
    }
    let pid = s(args, "project_id");
    if pid.is_empty() {
        return Err("cần run_id hoặc project_id".into());
    }
    crate::wfclient::stored_run(state, &pid)
        .map(|(_, run)| run)
        .ok_or_else(|| format!("project {pid} chưa chạy workflow nào (vf_workflow_run)"))
}

fn count(state: &AppState, sql: &str) -> i64 {
    state
        .core
        .db
        .query_one(sql, &[])
        .ok()
        .flatten()
        .map(|r| db::i64_of(&r, "n"))
        .unwrap_or(0)
}

/// Resolve project_id + orientation for a scene (scene → video → project).
/// Orientation preference: explicit arg > video.orientation > env default.
fn scene_context(state: &AppState, scene_id: &str, ori_arg: &str) -> Result<(Row, String, String), String> {
    let scene = state
        .core
        .db
        .get("scene", scene_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("scene {scene_id} không tồn tại"))?;
    let video_id = db::str_of(&scene, "video_id");
    let video = state
        .core
        .db
        .get("video", &video_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("video {video_id} của scene không tồn tại"))?;
    let project_id = db::str_of(&video, "project_id");
    let ori = if !ori_arg.is_empty() {
        ori_arg.to_uppercase()
    } else {
        let vo = db::str_of(&video, "orientation");
        if vo.is_empty() { crate::config::default_orientation() } else { vo }
    };
    Ok((scene, project_id, ori))
}

fn ext_required(state: &AppState) -> Option<Value> {
    if state.core.ext.is_connected() {
        None
    } else {
        Some(error_result(
            "Chrome extension (Google Flow bridge) chưa kết nối — không thể sinh ảnh/video. \
             Mở Chrome với extension Flow Kit đã load, vào labs.google (Flow) để extension bắt token \
             và nối WS về app (mặc định :9222). Kiểm tra bằng vf_status."
                .into(),
        ))
    }
}

/// Compact scene projection for list views (full row via vf_scene_get).
fn scene_summary(r: &Row) -> Value {
    let t = |k: &str| crate::llm::truncate(&db::str_of(r, k), 140);
    json!({
        "id": db::str_of(r, "id"),
        "display_order": db::i64_of(r, "display_order"),
        "chain_type": db::str_of(r, "chain_type"),
        "parent_scene_id": db::str_of(r, "parent_scene_id"),
        "shot_type": db::str_of(r, "shot_type"),
        "duration": r.get("duration").cloned().unwrap_or(Value::Null),
        "prompt": t("prompt"),
        "video_prompt": t("video_prompt"),
        "narrator_text": t("narrator_text"),
        "vertical":   { "image": db::str_of(r, "vertical_image_status"),
                        "video": db::str_of(r, "vertical_video_status"),
                        "upscale": db::str_of(r, "vertical_upscale_status") },
        "horizontal": { "image": db::str_of(r, "horizontal_image_status"),
                        "video": db::str_of(r, "horizontal_video_status"),
                        "upscale": db::str_of(r, "horizontal_upscale_status") },
    })
}

fn simple_slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if (c == ' ' || c == '-' || c == '_') && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

// ---- tool catalogue ---------------------------------------------------------

fn tools_list() -> Value {
    json!([
        // ---- projects ----
        {
            "name": "vf_project_create",
            "description": "Create a new Video Flow project (the top-level container: one story → videos → scenes). Provide the name and ideally the story/concept text; pick a visual-style material (realistic, 3d_pixar, anime, stop_motion, minecraft, oil_painting or a custom material name). Orientation is NOT set here — it lives on each video (vf_video_create). Returns the project row with its UUID id.",
            "inputSchema": { "type": "object", "properties": {
                "name":        { "type": "string", "description": "Project name." },
                "story":       { "type": "string", "description": "Story / concept / plot summary the pipeline will work from." },
                "description": { "type": "string", "description": "Optional short description." },
                "language":    { "type": "string", "description": "Content language, e.g. 'vi' or 'en'." },
                "material":    { "type": "string", "description": "Visual style material name (default: realistic)." }
            }, "required": ["name"] }
        },
        {
            "name": "vf_project_list",
            "description": "List all Video Flow projects (newest first) with id, name, status, story excerpt and per-project video count. Start here when the user says 'dự án video của tôi', 'my video projects', or you need a project_id.",
            "inputSchema": { "type": "object", "properties": {
                "limit": { "type": "number", "description": "Max projects (default 50)." }
            } }
        },
        {
            "name": "vf_project_get",
            "description": "Fetch ONE project in full: the project row PLUS its videos (each with scene count and orientation) PLUS its linked characters/locations/assets (with reference-image state). The one call that shows where a production stands structurally.",
            "inputSchema": { "type": "object", "properties": {
                "project_id": { "type": "string" }
            }, "required": ["project_id"] }
        },
        {
            "name": "vf_project_update",
            "description": "Patch project fields: name, story, description, status, language, material, narrator_voice, allow_music, allow_voice. Only the fields you pass change.",
            "inputSchema": { "type": "object", "properties": {
                "project_id":    { "type": "string" },
                "name":          { "type": "string" },
                "story":         { "type": "string" },
                "description":   { "type": "string" },
                "status":        { "type": "string" },
                "language":      { "type": "string" },
                "material":      { "type": "string" },
                "narrator_voice":{ "type": "string" },
                "allow_music":   { "type": "boolean" },
                "allow_voice":   { "type": "boolean" }
            }, "required": ["project_id"] }
        },
        {
            "name": "vf_project_delete",
            "description": "DELETE a project and everything under it: its videos, scenes, generation requests, character links and DAG pipelines. Irreversible — confirm with the user first.",
            "inputSchema": { "type": "object", "properties": {
                "project_id": { "type": "string" }
            }, "required": ["project_id"] }
        },
        // ---- characters / entities ----
        {
            "name": "vf_character_create",
            "description": "Create a reusable entity — a CHARACTER, LOCATION or visual ASSET — and optionally link it to a project. The description must be the entity's base default look in ONE outfit only (no per-scene variants; outfits change via scene prompts). Reference images for ALL entities must exist BEFORE any scene image is generated (vf_generate_image with character_id or all_refs). Locations render landscape refs, characters portrait refs.",
            "inputSchema": { "type": "object", "properties": {
                "name":              { "type": "string", "description": "Entity name (for real famous people use a role-based English alias, never the real name)." },
                "entity_type":       { "type": "string", "description": "CHARACTER | LOCATION | ASSET (default CHARACTER)." },
                "description":       { "type": "string", "description": "Physical appearance only, single outfit, single clean image — no multi-panel grids." },
                "image_prompt":      { "type": "string", "description": "Optional explicit reference-image prompt override." },
                "voice_description": { "type": "string", "description": "Optional voice description for TTS." },
                "project_id":        { "type": "string", "description": "Link the entity to this project (project_character M:N)." }
            }, "required": ["name", "description"] }
        },
        {
            "name": "vf_character_list",
            "description": "List characters/locations/assets. Pass project_id to see only that project's linked entities (with reference-image readiness: an entity is ready when media_id is a UUID), or omit it for the whole reusable library.",
            "inputSchema": { "type": "object", "properties": {
                "project_id": { "type": "string", "description": "Optional project filter." }
            } }
        },
        {
            "name": "vf_character_update",
            "description": "Patch an entity: name, description, image_prompt, voice_description, entity_type. Changing the look? Regenerate its reference image afterwards (vf_generate_image with character_id + regenerate=true) — scene images made with the old ref stay until you regenerate them.",
            "inputSchema": { "type": "object", "properties": {
                "character_id":      { "type": "string" },
                "name":              { "type": "string" },
                "entity_type":       { "type": "string" },
                "description":       { "type": "string" },
                "image_prompt":      { "type": "string" },
                "voice_description": { "type": "string" }
            }, "required": ["character_id"] }
        },
        // ---- videos ----
        {
            "name": "vf_video_create",
            "description": "Create a video inside a project. THIS is where orientation is fixed (VERTICAL for Shorts/TikTok, HORIZONTAL for YouTube landscape) — every scene keeps fully independent vertical/horizontal generation state, so never hardcode one. Scenes are then added by the pipeline (script_parser/scene_builder) or manually via vf_scene_create.",
            "inputSchema": { "type": "object", "properties": {
                "project_id":  { "type": "string" },
                "title":       { "type": "string" },
                "orientation": { "type": "string", "description": "VERTICAL | HORIZONTAL (default from FLOWKIT_ORIENTATION)." },
                "description": { "type": "string" }
            }, "required": ["project_id", "title"] }
        },
        {
            "name": "vf_video_list",
            "description": "List videos (optionally of one project) with id, title, orientation, status and scene count. Needed to get the video_id that vf_scene_list expects.",
            "inputSchema": { "type": "object", "properties": {
                "project_id": { "type": "string", "description": "Optional project filter." }
            } }
        },
        // ---- scenes ----
        {
            "name": "vf_scene_list",
            "description": "List a video's scenes in display order, compact: prompts truncated + per-orientation image/video/upscale statuses (PENDING/PROCESSING/COMPLETED/FAILED) + chain info (ROOT/CONTINUATION). The fastest way to see production progress scene-by-scene. Full untruncated row: vf_scene_get.",
            "inputSchema": { "type": "object", "properties": {
                "video_id": { "type": "string" }
            }, "required": ["video_id"] }
        },
        {
            "name": "vf_scene_get",
            "description": "Fetch ONE scene in full — every column: prompt, image_prompt, video_prompt, narrator_text, camera_movement, chain fields, and all vertical_*/horizontal_* URLs, media_ids (UUIDs) and statuses. Use before editing prompts or debugging a failed generation.",
            "inputSchema": { "type": "object", "properties": {
                "scene_id": { "type": "string" }
            }, "required": ["scene_id"] }
        },
        {
            "name": "vf_scene_create",
            "description": "Manually add a scene to a video (the pipeline normally creates scenes for you). Scene `prompt` describes ACTION only — never character appearance (visual consistency comes from reference images). `video_prompt` should use sub-clip timing: '0-3s: ... 3-6s: ... 6-8s: ...'.",
            "inputSchema": { "type": "object", "properties": {
                "video_id":        { "type": "string" },
                "display_order":   { "type": "number", "description": "Position in the video (1-based)." },
                "prompt":          { "type": "string", "description": "Scene/image action prompt (no character looks)." },
                "video_prompt":    { "type": "string", "description": "Motion prompt with sub-clip timing '0-3s: …'." },
                "narrator_text":   { "type": "string", "description": "Narration line for TTS (optional)." },
                "camera_movement": { "type": "string" },
                "character_names": { "type": "string", "description": "Comma-separated entity names appearing in the scene." },
                "duration":        { "type": "number", "description": "Target seconds (typically 8)." },
                "parent_scene_id": { "type": "string", "description": "For CONTINUATION scenes: the parent scene." },
                "chain_type":      { "type": "string", "description": "ROOT | CONTINUATION | INSERT (default ROOT)." }
            }, "required": ["video_id", "prompt"] }
        },
        {
            "name": "vf_scene_update",
            "description": "Patch scene fields (prompt, image_prompt, video_prompt, narrator_text, camera_movement, character_names, duration, display_order, shot_type, transition_prompt...). Editing a prompt does NOT regenerate anything by itself — follow with vf_generate_image / vf_generate_video (regenerate=true) and remember the cascade: a new image clears that orientation's video + upscale; a new video clears its upscale.",
            "inputSchema": { "type": "object", "properties": {
                "scene_id":          { "type": "string" },
                "prompt":            { "type": "string" },
                "image_prompt":      { "type": "string" },
                "video_prompt":      { "type": "string" },
                "narrator_text":     { "type": "string" },
                "camera_movement":   { "type": "string" },
                "character_names":   { "type": "string" },
                "duration":          { "type": "number" },
                "display_order":     { "type": "number" },
                "shot_type":         { "type": "string" },
                "transition_prompt": { "type": "string" }
            }, "required": ["scene_id"] }
        },
        {
            "name": "vf_scene_delete",
            "description": "Delete one scene from its video. Irreversible; generated media rows/URLs of that scene are dropped with it.",
            "inputSchema": { "type": "object", "properties": {
                "scene_id": { "type": "string" }
            }, "required": ["scene_id"] }
        },
        // ---- pipeline (DAG) ----
        {
            "name": "vf_pipeline_create",
            "description": "Create AND start the multi-agent DAG pipeline for a project — the main 'make the video' entry point. Modes: 'production' (default; input is an EXISTING screenplay/script — runs script parsing → refs → frames → images → videos → audio → download → concat → critic), 'full' (input is a RAW CONCEPT — adds pre-production: director → screenwriter → scene_plan → shot_design → visual_asset first), 'custom' (the orchestrator LLM plans the DAG from your goal). One active pipeline per project. Image/video stages need the Google Flow extension connected (vf_status). Returns pipeline_id + task count; follow with vf_pipeline_status.",
            "inputSchema": { "type": "object", "properties": {
                "project_id":  { "type": "string" },
                "script":      { "type": "string", "description": "Screenplay markdown (production mode) or the raw story concept (full mode)." },
                "orientation": { "type": "string", "description": "VERTICAL | HORIZONTAL. Never hardcode — ask or read the project's video." },
                "goal":        { "type": "string", "description": "Free-text goal for the orchestrator (custom mode) or extra steering." },
                "mode":        { "type": "string", "description": "production | full | custom (default production)." }
            }, "required": ["project_id"] }
        },
        {
            "name": "vf_pipeline_status",
            "description": "Pipeline progress: the parent (status, goal, orientation) + every DAG task with label, agent_type, status (registered/active/done/error/timeout), timing and result summary. Pass pipeline_id, or just project_id to get that project's latest pipeline. Poll this to report per-stage progress; on 'error' tasks read the result then vf_pipeline_control retry_task.",
            "inputSchema": { "type": "object", "properties": {
                "pipeline_id": { "type": "string", "description": "The DAG parent id." },
                "project_id":  { "type": "string", "description": "Alternative: latest pipeline of this project." }
            } }
        },
        {
            "name": "vf_pipeline_control",
            "description": "Control a pipeline: action 'pause' (stop scheduling new tasks), 'start' (resume / re-queue), 'cancel' (abort), or 'retry_task' (re-run ONE failed task — also pass task_id from vf_pipeline_status).",
            "inputSchema": { "type": "object", "properties": {
                "pipeline_id": { "type": "string" },
                "action":      { "type": "string", "enum": ["start", "pause", "cancel", "retry_task"] },
                "task_id":     { "type": "string", "description": "Required for retry_task." }
            }, "required": ["pipeline_id", "action"] }
        },
        // ---- workflow engine (parallel pipeline) ----
        {
            "name": "vf_workflow_run",
            "description": "Run the project's pipeline on SenClaw's WORKFLOW ENGINE instead of the app's own DAG — prefer this for anything with more than 2-3 scenes. The definition is emitted with ONE NODE PER SCENE (`vid_k` depends only on `img_k`), so up to 5 scenes render CONCURRENTLY rather than one 20-45 minute serial loop, each scene RETRIES on its own, and one broken scene fails its node instead of sinking the whole stage. Progress, per-node logs and cancel are visible in the SenClaw workflow UI. Returns IMMEDIATELY with {workflow, run_id} — the run keeps going in the background; poll vf_workflow_status. Needs the Google Flow extension connected for image/video nodes (vf_status).",
            "inputSchema": { "type": "object", "properties": {
                "project_id":  { "type": "string" },
                "video_id":    { "type": "string", "description": "Which video to render (default: the project's first video by display_order)." },
                "orientation": { "type": "string", "description": "VERTICAL | HORIZONTAL (default: the app setting)." },
                "with_audio":  { "type": "boolean", "description": "Include the TTS narration stage (default false)." },
                "with_critic": { "type": "boolean", "description": "Include the critic QA stage after concat (default false)." }
            }, "required": ["project_id"] }
        },
        {
            "name": "vf_workflow_status",
            "description": "Progress of a workflow run: overall status (running/done/partial-failed/cancelled/interrupted), a done/total count, per-status counts and a compact per-node list (id + status) — plus the error text of any failed node. Pass run_id, or just project_id to look up that project's last run. Use this to report progress; it deliberately omits every node's stdout.",
            "inputSchema": { "type": "object", "properties": {
                "run_id":     { "type": "string" },
                "project_id": { "type": "string", "description": "Alternative: the last workflow run launched for this project." }
            } }
        },
        {
            "name": "vf_workflow_cancel",
            "description": "Cancel a workflow run: stop dispatching new nodes and abort the in-flight ones. Nodes already finished keep their generated media (steps are idempotent, so a later vf_workflow_run resumes rather than redoing them). Pass run_id or project_id.",
            "inputSchema": { "type": "object", "properties": {
                "run_id":     { "type": "string" },
                "project_id": { "type": "string" }
            } }
        },
        // ---- generation (async — spawns and returns immediately) ----
        {
            "name": "vf_generate_image",
            "description": "Queue image generation via the Google Flow extension and return IMMEDIATELY (work runs in background; track via vf_scene_get / vf_requests_status). Three uses: (1) character_id → the entity's REFERENCE image; (2) scene_id → that scene's frame for one orientation (all linked entities must already have reference media_ids — generate refs FIRST); (3) project_id + all_refs=true → reference images for every entity of the project still missing one. regenerate=true forces a redo and CASCADES: the new image clears that orientation's video + upscale. edit_prompt turns a scene job into an EDIT of the existing image (used for CONTINUATION consistency).",
            "inputSchema": { "type": "object", "properties": {
                "scene_id":     { "type": "string", "description": "Scene frame to generate." },
                "character_id": { "type": "string", "description": "Entity reference image to generate." },
                "project_id":   { "type": "string", "description": "With all_refs=true: batch all missing entity refs." },
                "all_refs":     { "type": "boolean", "description": "Generate every missing reference image of project_id." },
                "orientation":  { "type": "string", "description": "VERTICAL | HORIZONTAL (scene jobs; default = the scene's video orientation)." },
                "regenerate":   { "type": "boolean", "description": "Force re-generation even if COMPLETED (cascade applies)." },
                "edit_prompt":  { "type": "string", "description": "Edit instruction applied to the scene's existing image instead of generating from scratch." }
            } }
        },
        {
            "name": "vf_generate_video",
            "description": "Queue Veo3 video generation for a scene (one orientation) via the Google Flow extension and return IMMEDIATELY — a clip takes 2-5 minutes; poll vf_scene_get / vf_requests_status, never block on this. Requires that orientation's scene image to be COMPLETED first. regenerate=true forces a redo and clears that orientation's upscale (cascade). The scene's video_prompt should carry sub-clip timing ('0-3s: …').",
            "inputSchema": { "type": "object", "properties": {
                "scene_id":    { "type": "string" },
                "orientation": { "type": "string", "description": "VERTICAL | HORIZONTAL (default = the scene's video orientation)." },
                "regenerate":  { "type": "boolean" }
            }, "required": ["scene_id"] }
        },
        {
            "name": "vf_upscale_video",
            "description": "Queue a 4K upscale of a scene's COMPLETED video (one orientation) and return immediately. Only works after the video stage; a later video regeneration invalidates the upscale. Note: upscale requires a Google Flow TIER_TWO account.",
            "inputSchema": { "type": "object", "properties": {
                "scene_id":    { "type": "string" },
                "orientation": { "type": "string", "description": "VERTICAL | HORIZONTAL (default = the scene's video orientation)." }
            }, "required": ["scene_id"] }
        },
        // ---- observe ----
        {
            "name": "vf_requests_status",
            "description": "The generation request queue: counts grouped by status (PENDING/PROCESSING/COMPLETED/FAILED) and by type+status, plus the most recent requests with error messages. The first place to look when a stage seems stuck ('sao ảnh mãi chưa xong'). FAILED rows carry error_message — read it before retrying.",
            "inputSchema": { "type": "object", "properties": {
                "project_id": { "type": "string", "description": "Optional project filter." },
                "status":     { "type": "string", "description": "Optional status filter for the recent list." },
                "limit":      { "type": "number", "description": "Recent rows to return (default 20)." }
            } }
        },
        {
            "name": "vf_agents_list",
            "description": "List every sub-agent in the pipeline pool (director, screenwriter, scene_plan, shot_design, visual_asset, scene_builder, script_parser, gen_ref, director_frame, character, image, video, audio, media_download, concat, critic, orchestrator + DB-backed skill agents) with kind, description, whether it's disabled, and a soul (system-prompt) excerpt. Use to explain the pipeline or before tuning a soul.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "vf_soul_get",
            "description": "Read a sub-agent's SOUL — its full system-prompt markdown file (frontmatter included) from souls/. The soul is what actually steers that stage's LLM behaviour. agent_type as in vf_agents_list (e.g. 'director', 'image', 'critic', 'script_parser').",
            "inputSchema": { "type": "object", "properties": {
                "agent_type": { "type": "string" }
            }, "required": ["agent_type"] }
        },
        {
            "name": "vf_soul_set",
            "description": "Overwrite a sub-agent's soul file with new content — the way to tune a pipeline stage's prompt (e.g. make the critic stricter, change the screenwriter's tone). Takes effect on the agent's NEXT execution. Read the current soul first (vf_soul_get) and preserve its intent; this replaces the whole file.",
            "inputSchema": { "type": "object", "properties": {
                "agent_type": { "type": "string" },
                "content":    { "type": "string", "description": "Full new markdown content of the soul file." }
            }, "required": ["agent_type", "content"] }
        },
        {
            "name": "vf_generate_narration",
            "description": "Synthesize voice-over narration with SenClaw's TTS for scenes that have narrator_text, and attach a WAV to each scene. Unlike image/video generation this does NOT need the Chrome extension — it runs on the TTS model installed in SenClaw Settings (VieNeu-TTS Vietnamese, MMS-VITS, macOS Speech…). Voice/language default to the project's narrator_voice/language, then to the daemon's TTS settings; override per call. Already-narrated scenes are skipped unless regenerate=true. Runs in the background — poll vf_scene_list for narrator_audio_status.",
            "inputSchema": { "type": "object", "properties": {
                "video_id":   { "type": "string", "description": "Video whose scenes to narrate. Omit to use the project's first video (then project_id is required)." },
                "project_id": { "type": "string" },
                "scene_id":   { "type": "string", "description": "Narrate only this one scene." },
                "voice":      { "type": "string", "description": "TTS voice/preset name, e.g. a VieNeu preset like 'Phạm Tuyên'. Omit to use the project or SenClaw setting." },
                "language":   { "type": "string", "description": "Language code, e.g. 'vi' or 'en'." },
                "speed":      { "type": "number", "description": "Speaking rate multiplier (1.0 = normal)." },
                "model_id":   { "type": "string", "description": "TTS model id override; omit to use the model selected in SenClaw Settings." },
                "regenerate": { "type": "boolean", "description": "Re-synthesize scenes that already have narration." }
            } }
        },
        {
            "name": "vf_media_localize",
            "description": "Download every generated image/video/audio that is still on a remote Google Flow URL into this app's local media storage, and repoint the DB at the local copy. Flow's signed URLs expire after a few hours, so run this to rescue an older project whose thumbnails have gone blank, or after generating with a build that did not download inline. Returns counts of downloaded/skipped/failed.",
            "inputSchema": { "type": "object", "properties": {
                "project_id": { "type": "string", "description": "Scope to one project. Omit to sweep every project in this app." }
            } }
        },
        {
            "name": "vf_fetch_video_urls",
            "description": "Lấy link cho các clip đã render xong nhưng chưa có URL. Google Flow không còn trả link video ở API sinh (chỉ trả media id), nên link chỉ tồn tại trong dữ liệu trang Flow. Tool này nhờ Chrome extension mở trang project Flow trong một tab nền vài giây, bắt link, điền vào scene rồi tải clip về máy. Cần extension đang kết nối và project đã từng sinh ảnh/video (để biết Flow project id). Dùng khi scene báo COMPLETED mà không xem được video.",
            "inputSchema": { "type": "object", "properties": {
                "project_id": { "type": "string" }
            }, "required": ["project_id"] }
        },
        {
            "name": "vf_playbook_find",
            "description": "Tìm playbook (hướng dẫn thao tác) phù hợp với việc người dùng đang cần, theo cách họ diễn đạt. Mỗi playbook có sẵn các cụm kích hoạt (khớp cả khi gõ không dấu), ví dụ \"không xem được video\" → refresh-urls, \"lồng tiếng\" → gen-narrator, \"ghép video\" → concat. Trả về nội dung playbook khớp nhất để làm theo. Gọi tool này TRƯỚC khi tự mò API khi gặp yêu cầu lạ về video-flow.",
            "inputSchema": { "type": "object", "properties": {
                "query": { "type": "string", "description": "Câu người dùng nói, nguyên văn." },
                "full": { "type": "boolean", "description": "Trả nguyên nội dung playbook khớp nhất (mặc định true)." }
            }, "required": ["query"] }
        },
        {
            "name": "vf_playbook_list",
            "description": "Liệt kê mọi playbook của Video Flow kèm mô tả và cụm kích hoạt — dùng khi muốn biết app này làm được những gì.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "vf_tts_status",
            "description": "What narration WOULD sound like, without synthesizing: the SenClaw TTS models installed and the active model/voice/language/speed settings. Use this before vf_generate_narration to check a TTS model is installed at all, or to list the voice names the user can pick from.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "vf_status",
            "description": "Video Flow health in one call: is the Google Flow Chrome extension connected (REQUIRED for any image/video generation), worker + LLM-profile settings, and live counts (projects, videos, scenes, characters, pending/processing/failed requests, pipelines by status). Call this FIRST when anything generation-related misbehaves.",
            "inputSchema": { "type": "object", "properties": {} }
        }
    ])
}

// ---- dispatch ---------------------------------------------------------------

async fn call_tool(state: &AppState, name: &str, args: &Value) -> Value {
    let db = &state.core.db;
    match name {
        // ---- projects ----
        "vf_project_create" => {
            let pname = s(args, "name");
            if pname.is_empty() {
                return error_result("name là bắt buộc".into());
            }
            let mut row = patch_from(args, &["name", "story", "description", "language", "material"]);
            if !row.contains_key("material") {
                row.insert("material".into(), json!("realistic"));
            }
            match db.insert("project", &row) {
                Ok(id) => match db.get("project", &id) {
                    Ok(Some(p)) => json_result(json!({ "ok": true, "project": row_value(p),
                        "next": "Tạo video (vf_video_create, chọn orientation) và nhân vật/bối cảnh (vf_character_create), hoặc chạy thẳng vf_pipeline_create." })),
                    _ => json_result(json!({ "ok": true, "project_id": id })),
                },
                Err(e) => error_result(e.to_string()),
            }
        }
        "vf_project_list" => {
            let limit = int(args, "limit", 50).clamp(1, 500);
            let rows = match db.query("SELECT * FROM project ORDER BY created_at DESC LIMIT ?1", &[&limit]) {
                Ok(r) => r,
                Err(e) => return error_result(e.to_string()),
            };
            let vcounts = db
                .query("SELECT project_id, COUNT(*) AS n FROM video GROUP BY project_id", &[])
                .unwrap_or_default();
            let items: Vec<Value> = rows
                .into_iter()
                .map(|r| {
                    let pid = db::str_of(&r, "id");
                    let n = vcounts
                        .iter()
                        .find(|c| db::str_of(c, "project_id") == pid)
                        .map(|c| db::i64_of(c, "n"))
                        .unwrap_or(0);
                    json!({
                        "id": pid,
                        "name": db::str_of(&r, "name"),
                        "status": db::str_of(&r, "status"),
                        "material": db::str_of(&r, "material"),
                        "story": crate::llm::truncate(&db::str_of(&r, "story"), 200),
                        "videos": n,
                        "created_at": db::str_of(&r, "created_at"),
                    })
                })
                .collect();
            json_result(json!({ "count": items.len(), "projects": items }))
        }
        "vf_project_get" => {
            let pid = s(args, "project_id");
            if pid.is_empty() {
                return error_result("project_id là bắt buộc".into());
            }
            let project = match db.get("project", &pid) {
                Ok(Some(p)) => p,
                Ok(None) => return error_result(format!("project {pid} không tồn tại")),
                Err(e) => return error_result(e.to_string()),
            };
            let videos = db
                .query(
                    "SELECT * FROM video WHERE project_id = ?1 ORDER BY display_order, created_at",
                    &[&pid],
                )
                .unwrap_or_default();
            let scounts = db
                .query(
                    "SELECT video_id, COUNT(*) AS n FROM scene WHERE video_id IN \
                     (SELECT id FROM video WHERE project_id = ?1) GROUP BY video_id",
                    &[&pid],
                )
                .unwrap_or_default();
            let videos: Vec<Value> = videos
                .into_iter()
                .map(|v| {
                    let vid = db::str_of(&v, "id");
                    let mut jv = row_value(v);
                    jv["scene_count"] = json!(scounts
                        .iter()
                        .find(|c| db::str_of(c, "video_id") == vid)
                        .map(|c| db::i64_of(c, "n"))
                        .unwrap_or(0));
                    jv
                })
                .collect();
            let characters = db
                .query(
                    "SELECT c.* FROM character c JOIN project_character pc ON pc.character_id = c.id \
                     WHERE pc.project_id = ?1 ORDER BY c.created_at",
                    &[&pid],
                )
                .unwrap_or_default();
            let characters: Vec<Value> = characters
                .into_iter()
                .map(|c| {
                    let ready = !db::str_of(&c, "media_id").is_empty();
                    let mut jc = row_value(c);
                    jc["reference_ready"] = json!(ready);
                    jc
                })
                .collect();
            json_result(json!({
                "project": row_value(project),
                "videos": videos,
                "characters": characters,
                "note": "reference_ready=false → chạy vf_generate_image (character_id hoặc all_refs) TRƯỚC khi sinh ảnh scene.",
            }))
        }
        "vf_project_update" => {
            let pid = s(args, "project_id");
            if pid.is_empty() {
                return error_result("project_id là bắt buộc".into());
            }
            let patch = patch_from(args, &[
                "name", "story", "description", "status", "language", "material",
                "narrator_voice", "allow_music", "allow_voice",
            ]);
            if patch.is_empty() {
                return error_result("không có trường nào để cập nhật".into());
            }
            match db.update("project", &pid, &patch) {
                Ok(()) => json_result(json!({ "ok": true, "project": db.get("project", &pid).ok().flatten().map(row_value) })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "vf_project_delete" => {
            let pid = s(args, "project_id");
            if pid.is_empty() {
                return error_result("project_id là bắt buộc".into());
            }
            if db.get("project", &pid).ok().flatten().is_none() {
                return error_result(format!("project {pid} không tồn tại"));
            }
            // Cascade: pipelines first (reuses the tx helper), then children, then the project.
            let parents = db
                .query("SELECT id FROM dag_parents WHERE project_id = ?1", &[&pid])
                .unwrap_or_default();
            for p in &parents {
                let _ = db.delete_pipeline_cascade(&db::str_of(p, "id"), &pid);
            }
            let _ = db.execute("DELETE FROM request WHERE project_id = ?1", &[&pid]);
            let _ = db.execute(
                "DELETE FROM scene WHERE video_id IN (SELECT id FROM video WHERE project_id = ?1)",
                &[&pid],
            );
            let _ = db.execute("DELETE FROM video WHERE project_id = ?1", &[&pid]);
            let _ = db.execute("DELETE FROM project_character WHERE project_id = ?1", &[&pid]);
            match db.delete("project", &pid) {
                Ok(_) => {
                    state.core.dash.emit("pipeline:updated", json!({ "project_id": pid, "deleted": true }));
                    json_result(json!({ "ok": true, "deleted": pid }))
                }
                Err(e) => error_result(e.to_string()),
            }
        }

        // ---- characters ----
        "vf_character_create" => {
            let cname = s(args, "name");
            let desc = s(args, "description");
            if cname.is_empty() || desc.is_empty() {
                return error_result("name và description là bắt buộc".into());
            }
            let mut row = patch_from(args, &["name", "description", "image_prompt", "voice_description"]);
            let etype = s(args, "entity_type");
            row.insert("entity_type".into(), json!(if etype.is_empty() { "CHARACTER".to_string() } else { etype.to_uppercase() }));
            row.insert("slug".into(), json!(simple_slug(&cname)));
            let cid = match db.insert("character", &row) {
                Ok(id) => id,
                Err(e) => return error_result(e.to_string()),
            };
            let pid = s(args, "project_id");
            if !pid.is_empty() {
                if let Err(e) = db.execute(
                    "INSERT OR REPLACE INTO project_character (project_id, character_id) VALUES (?1, ?2)",
                    &[&pid, &cid],
                ) {
                    return error_result(format!("tạo entity ok ({cid}) nhưng link project thất bại: {e}"));
                }
            }
            json_result(json!({
                "ok": true,
                "character": db.get("character", &cid).ok().flatten().map(row_value),
                "linked_project": if pid.is_empty() { Value::Null } else { json!(pid) },
                "next": "Sinh ảnh tham chiếu: vf_generate_image { character_id } — bắt buộc trước khi sinh ảnh scene.",
            }))
        }
        "vf_character_list" => {
            let pid = s(args, "project_id");
            let rows = if pid.is_empty() {
                db.query("SELECT * FROM character ORDER BY created_at DESC", &[])
            } else {
                db.query(
                    "SELECT c.* FROM character c JOIN project_character pc ON pc.character_id = c.id \
                     WHERE pc.project_id = ?1 ORDER BY c.created_at",
                    &[&pid],
                )
            };
            match rows {
                Ok(rs) => {
                    let items: Vec<Value> = rs
                        .into_iter()
                        .map(|c| {
                            let ready = !db::str_of(&c, "media_id").is_empty();
                            let mut jc = row_value(c);
                            jc["reference_ready"] = json!(ready);
                            jc
                        })
                        .collect();
                    json_result(json!({ "count": items.len(), "characters": items }))
                }
                Err(e) => error_result(e.to_string()),
            }
        }
        "vf_character_update" => {
            let cid = s(args, "character_id");
            if cid.is_empty() {
                return error_result("character_id là bắt buộc".into());
            }
            let mut patch = patch_from(args, &["name", "description", "image_prompt", "voice_description"]);
            let etype = s(args, "entity_type");
            if !etype.is_empty() {
                patch.insert("entity_type".into(), json!(etype.to_uppercase()));
            }
            if patch.is_empty() {
                return error_result("không có trường nào để cập nhật".into());
            }
            match db.update("character", &cid, &patch) {
                Ok(()) => json_result(json!({
                    "ok": true,
                    "character": db.get("character", &cid).ok().flatten().map(row_value),
                    "note": "Đổi ngoại hình? Chạy vf_generate_image { character_id, regenerate: true } để làm lại ảnh tham chiếu.",
                })),
                Err(e) => error_result(e.to_string()),
            }
        }

        // ---- videos ----
        "vf_video_create" => {
            let pid = s(args, "project_id");
            let title = s(args, "title");
            if pid.is_empty() || title.is_empty() {
                return error_result("project_id và title là bắt buộc".into());
            }
            if db.get("project", &pid).ok().flatten().is_none() {
                return error_result(format!("project {pid} không tồn tại"));
            }
            let mut row = patch_from(args, &["project_id", "title", "description"]);
            let ori = s(args, "orientation");
            row.insert(
                "orientation".into(),
                json!(if ori.is_empty() { crate::config::default_orientation() } else { ori.to_uppercase() }),
            );
            match db.insert("video", &row) {
                Ok(id) => json_result(json!({ "ok": true, "video": db.get("video", &id).ok().flatten().map(row_value) })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "vf_video_list" => {
            let pid = s(args, "project_id");
            let rows = if pid.is_empty() {
                db.query("SELECT * FROM video ORDER BY created_at DESC", &[])
            } else {
                db.query(
                    "SELECT * FROM video WHERE project_id = ?1 ORDER BY display_order, created_at",
                    &[&pid],
                )
            };
            match rows {
                Ok(rs) => {
                    let scounts = db
                        .query("SELECT video_id, COUNT(*) AS n FROM scene GROUP BY video_id", &[])
                        .unwrap_or_default();
                    let items: Vec<Value> = rs
                        .into_iter()
                        .map(|v| {
                            let vid = db::str_of(&v, "id");
                            let mut jv = row_value(v);
                            jv["scene_count"] = json!(scounts
                                .iter()
                                .find(|c| db::str_of(c, "video_id") == vid)
                                .map(|c| db::i64_of(c, "n"))
                                .unwrap_or(0));
                            jv
                        })
                        .collect();
                    json_result(json!({ "count": items.len(), "videos": items }))
                }
                Err(e) => error_result(e.to_string()),
            }
        }

        // ---- scenes ----
        "vf_scene_list" => {
            let vid = s(args, "video_id");
            if vid.is_empty() {
                return error_result("video_id là bắt buộc".into());
            }
            match db.query(
                "SELECT * FROM scene WHERE video_id = ?1 ORDER BY display_order, created_at",
                &[&vid],
            ) {
                Ok(rs) => {
                    let items: Vec<Value> = rs.iter().map(scene_summary).collect();
                    json_result(json!({
                        "count": items.len(),
                        "scenes": items,
                        "note": "Trạng thái theo TỪNG orientation (vertical/horizontal độc lập). Chi tiết đầy đủ: vf_scene_get.",
                    }))
                }
                Err(e) => error_result(e.to_string()),
            }
        }
        "vf_scene_get" => {
            let sid = s(args, "scene_id");
            if sid.is_empty() {
                return error_result("scene_id là bắt buộc".into());
            }
            match db.get("scene", &sid) {
                Ok(Some(r)) => json_result(json!({ "scene": row_value(r) })),
                Ok(None) => error_result(format!("scene {sid} không tồn tại")),
                Err(e) => error_result(e.to_string()),
            }
        }
        "vf_scene_create" => {
            let vid = s(args, "video_id");
            let prompt = s(args, "prompt");
            if vid.is_empty() || prompt.is_empty() {
                return error_result("video_id và prompt là bắt buộc".into());
            }
            if db.get("video", &vid).ok().flatten().is_none() {
                return error_result(format!("video {vid} không tồn tại"));
            }
            let mut row = patch_from(args, &[
                "video_id", "display_order", "prompt", "video_prompt", "narrator_text",
                "camera_movement", "character_names", "duration", "parent_scene_id", "chain_type",
            ]);
            if !row.contains_key("chain_type") {
                row.insert("chain_type".into(), json!("ROOT"));
            }
            if !row.contains_key("display_order") {
                let max = db
                    .query_one("SELECT COALESCE(MAX(display_order), 0) AS n FROM scene WHERE video_id = ?1", &[&vid])
                    .ok()
                    .flatten()
                    .map(|r| db::i64_of(&r, "n"))
                    .unwrap_or(0);
                row.insert("display_order".into(), json!(max + 1));
            }
            match db.insert("scene", &row) {
                Ok(id) => {
                    state.core.dash.emit("scene_updated", json!({ "scene_id": id }));
                    json_result(json!({ "ok": true, "scene": db.get("scene", &id).ok().flatten().map(row_value) }))
                }
                Err(e) => error_result(e.to_string()),
            }
        }
        "vf_scene_update" => {
            let sid = s(args, "scene_id");
            if sid.is_empty() {
                return error_result("scene_id là bắt buộc".into());
            }
            let patch = patch_from(args, &[
                "prompt", "image_prompt", "video_prompt", "narrator_text", "camera_movement",
                "character_names", "duration", "display_order", "shot_type", "transition_prompt",
            ]);
            if patch.is_empty() {
                return error_result("không có trường nào để cập nhật".into());
            }
            match db.update("scene", &sid, &patch) {
                Ok(()) => {
                    state.core.dash.emit("scene_updated", json!({ "scene_id": sid }));
                    json_result(json!({
                        "ok": true,
                        "scene": db.get("scene", &sid).ok().flatten().map(row_value),
                        "note": "Prompt mới chưa tự sinh lại media — dùng vf_generate_image/vf_generate_video với regenerate=true (nhớ cascade).",
                    }))
                }
                Err(e) => error_result(e.to_string()),
            }
        }
        "vf_scene_delete" => {
            let sid = s(args, "scene_id");
            if sid.is_empty() {
                return error_result("scene_id là bắt buộc".into());
            }
            match db.delete("scene", &sid) {
                Ok(0) => error_result(format!("scene {sid} không tồn tại")),
                Ok(_) => {
                    let _ = db.execute("DELETE FROM request WHERE scene_id = ?1", &[&sid]);
                    state.core.dash.emit("scene_updated", json!({ "scene_id": sid, "deleted": true }));
                    json_result(json!({ "ok": true, "deleted": sid }))
                }
                Err(e) => error_result(e.to_string()),
            }
        }

        // ---- pipeline ----
        "vf_pipeline_create" => {
            let pid = s(args, "project_id");
            if pid.is_empty() {
                return error_result("project_id là bắt buộc".into());
            }
            let project = match db.get("project", &pid) {
                Ok(Some(p)) => p,
                Ok(None) => return error_result(format!("project {pid} không tồn tại")),
                Err(e) => return error_result(e.to_string()),
            };
            let mut script = s(args, "script");
            if script.is_empty() {
                script = db::str_of(&project, "story");
            }
            let ori = {
                let o = s(args, "orientation");
                if o.is_empty() { crate::config::default_orientation() } else { o.to_uppercase() }
            };
            let goal = s(args, "goal");
            let mode = {
                let m = s(args, "mode");
                if m.is_empty() { "production".to_string() } else { m }
            };
            match crate::pipeline::create(&state.core, &state.pool, &pid, &script, &ori, &goal, &mode).await {
                Ok((pipeline_id, task_count)) => json_result(json!({
                    "ok": true,
                    "pipeline_id": pipeline_id,
                    "task_count": task_count,
                    "mode": mode,
                    "orientation": ori,
                    "extension_connected": state.core.ext.is_connected(),
                    "note": "Engine DAG tự chạy (tick 500ms, tối đa 5 task song song). Theo dõi bằng vf_pipeline_status; các stage sinh ảnh/video cần extension connected.",
                })),
                Err(e) => error_result(e),
            }
        }
        "vf_pipeline_status" => {
            let mut id = s(args, "pipeline_id");
            if id.is_empty() {
                let pid = s(args, "project_id");
                if pid.is_empty() {
                    return error_result("cần pipeline_id hoặc project_id".into());
                }
                match db.query_one(
                    "SELECT id FROM dag_parents WHERE project_id = ?1 ORDER BY created_at DESC LIMIT 1",
                    &[&pid],
                ) {
                    Ok(Some(r)) => id = db::str_of(&r, "id"),
                    Ok(None) => return error_result(format!("project {pid} chưa có pipeline nào (vf_pipeline_create)")),
                    Err(e) => return error_result(e.to_string()),
                }
            }
            match crate::pipeline::get_status(&state.core, &id) {
                Ok(v) => json_result(v),
                Err(e) => error_result(e),
            }
        }
        "vf_pipeline_control" => {
            let id = s(args, "pipeline_id");
            let action = s(args, "action");
            if id.is_empty() || action.is_empty() {
                return error_result("pipeline_id và action là bắt buộc".into());
            }
            let r = match action.as_str() {
                "start" => crate::pipeline::start(&state.core, &id),
                "pause" => crate::pipeline::pause(&state.core, &id),
                "cancel" => crate::pipeline::cancel(&state.core, &id),
                "retry_task" => {
                    let tid = s(args, "task_id");
                    if tid.is_empty() {
                        return error_result("retry_task cần task_id (lấy từ vf_pipeline_status)".into());
                    }
                    crate::pipeline::retry_task(&state.core, &id, &tid)
                }
                other => return error_result(format!("action không hợp lệ: {other} (start|pause|cancel|retry_task)")),
            };
            match r {
                Ok(()) => json_result(json!({
                    "ok": true,
                    "pipeline_id": id,
                    "action": action,
                    "status": crate::pipeline::get_status(&state.core, &id).ok(),
                })),
                Err(e) => error_result(e),
            }
        }

        // ---- workflow engine ----
        "vf_workflow_run" => {
            let pid = s(args, "project_id");
            if pid.is_empty() {
                return error_result("project_id là bắt buộc".into());
            }
            match crate::wfclient::launch_project_workflow(
                state,
                &pid,
                &s(args, "video_id"),
                &s(args, "orientation"),
                flag(args, "with_audio"),
                flag(args, "with_critic"),
            )
            .await
            {
                Ok((workflow, run_id)) => json_result(json!({
                    "ok": true,
                    "workflow": workflow,
                    "run_id": run_id,
                    "extension_connected": state.core.ext.is_connected(),
                    "note": "Run đang chạy trên workflow engine (mỗi cảnh một node, tối đa 5 node song song). Theo dõi bằng vf_workflow_status.",
                })),
                Err(e) => error_result(e),
            }
        }
        "vf_workflow_status" => {
            let run_id = match resolve_run_id(state, args) {
                Ok(id) => id,
                Err(e) => return error_result(e),
            };
            match crate::wfclient::get_run(&run_id).await {
                Ok(run) => json_result(crate::wfclient::summarize_run(&run)),
                Err(e) => error_result(e),
            }
        }
        "vf_workflow_cancel" => {
            let run_id = match resolve_run_id(state, args) {
                Ok(id) => id,
                Err(e) => return error_result(e),
            };
            match crate::wfclient::cancel_run(&run_id).await {
                Ok(()) => json_result(json!({ "ok": true, "run_id": run_id, "status": "cancelled" })),
                Err(e) => error_result(e),
            }
        }

        // ---- generation (spawned, returns immediately) ----
        "vf_generate_image" => {
            if let Some(err) = ext_required(state) {
                return err;
            }
            let regenerate = flag(args, "regenerate");
            let scene_id = s(args, "scene_id");
            let character_id = s(args, "character_id");
            let project_id = s(args, "project_id");

            // Mode 3: all missing reference images of a project.
            if flag(args, "all_refs") || (scene_id.is_empty() && character_id.is_empty() && !project_id.is_empty()) {
                if project_id.is_empty() {
                    return error_result("all_refs cần project_id".into());
                }
                let core = state.core.clone();
                let pid = project_id.clone();
                tokio::spawn(async move {
                    match crate::process::process_all_entities(&core, &pid).await {
                        Ok(n) => core.dash.emit("request_completed", json!({ "project_id": pid, "kind": "all_refs", "generated": n })),
                        Err(e) => eprintln!("[mcp] process_all_entities {pid}: {e}"),
                    }
                });
                return json_result(json!({
                    "ok": true, "queued": true, "kind": "all_refs", "project_id": project_id,
                    "note": "Đang sinh nền mọi ảnh tham chiếu còn thiếu. Theo dõi: vf_character_list { project_id } (reference_ready) / vf_requests_status.",
                }));
            }

            // Mode 2: one entity reference image.
            if !character_id.is_empty() {
                let pid = if !project_id.is_empty() {
                    project_id.clone()
                } else {
                    match db.query_one(
                        "SELECT project_id FROM project_character WHERE character_id = ?1 LIMIT 1",
                        &[&character_id],
                    ) {
                        Ok(Some(r)) => db::str_of(&r, "project_id"),
                        _ => String::new(),
                    }
                };
                let core = state.core.clone();
                let (cid, pid2) = (character_id.clone(), pid.clone());
                tokio::spawn(async move {
                    if let Err(e) = crate::process::entity_image(&core, &cid, &pid2, regenerate).await {
                        eprintln!("[mcp] entity_image {cid}: {e}");
                    }
                });
                return json_result(json!({
                    "ok": true, "queued": true, "kind": "reference_image",
                    "character_id": character_id, "project_id": pid, "regenerate": regenerate,
                    "note": "Đang sinh nền. Khi xong entity có media_id (UUID) — kiểm tra vf_character_list / vf_requests_status.",
                }));
            }

            // Mode 1: scene frame.
            if scene_id.is_empty() {
                return error_result("cần scene_id, character_id hoặc project_id + all_refs".into());
            }
            let (_, pid, ori) = match scene_context(state, &scene_id, &s(args, "orientation")) {
                Ok(t) => t,
                Err(e) => return error_result(e),
            };
            let edit_prompt = s(args, "edit_prompt");
            let core = state.core.clone();
            let (sid, pid2, ori2) = (scene_id.clone(), pid.clone(), ori.clone());
            tokio::spawn(async move {
                let ep = if edit_prompt.is_empty() { None } else { Some(edit_prompt.as_str()) };
                if let Err(e) = crate::process::scene_image(&core, &sid, &pid2, &ori2, regenerate, ep).await {
                    eprintln!("[mcp] scene_image {sid}: {e}");
                }
            });
            json_result(json!({
                "ok": true, "queued": true, "kind": "scene_image",
                "scene_id": scene_id, "project_id": pid, "orientation": ori, "regenerate": regenerate,
                "cascade": if regenerate { "ảnh mới sẽ xoá video + upscale của orientation này" } else { "" },
                "note": "Đang sinh nền. Theo dõi: vf_scene_get (image_status của orientation) / vf_requests_status.",
            }))
        }
        "vf_generate_video" => {
            if let Some(err) = ext_required(state) {
                return err;
            }
            let scene_id = s(args, "scene_id");
            if scene_id.is_empty() {
                return error_result("scene_id là bắt buộc".into());
            }
            let regenerate = flag(args, "regenerate");
            let (scene, pid, ori) = match scene_context(state, &scene_id, &s(args, "orientation")) {
                Ok(t) => t,
                Err(e) => return error_result(e),
            };
            let cols = db::scene_cols(&ori);
            let img_status = db::str_of(&scene, &cols.image_status);
            if img_status != "COMPLETED" {
                return error_result(format!(
                    "Ảnh scene ({}) chưa COMPLETED (hiện: {}) — sinh ảnh trước bằng vf_generate_image.",
                    ori, if img_status.is_empty() { "chưa có".into() } else { img_status }
                ));
            }
            let core = state.core.clone();
            let (sid, pid2, ori2) = (scene_id.clone(), pid.clone(), ori.clone());
            tokio::spawn(async move {
                if let Err(e) = crate::process::scene_video(&core, &sid, &pid2, &ori2, regenerate).await {
                    eprintln!("[mcp] scene_video {sid}: {e}");
                }
            });
            json_result(json!({
                "ok": true, "queued": true, "kind": "scene_video",
                "scene_id": scene_id, "project_id": pid, "orientation": ori, "regenerate": regenerate,
                "eta": "2-5 phút / clip",
                "cascade": if regenerate { "video mới sẽ xoá upscale của orientation này" } else { "" },
                "note": "Đang sinh nền (submit → poll). ĐỪNG chờ trong tool call — poll vf_scene_get / vf_requests_status.",
            }))
        }
        "vf_upscale_video" => {
            if let Some(err) = ext_required(state) {
                return err;
            }
            let scene_id = s(args, "scene_id");
            if scene_id.is_empty() {
                return error_result("scene_id là bắt buộc".into());
            }
            let (scene, pid, ori) = match scene_context(state, &scene_id, &s(args, "orientation")) {
                Ok(t) => t,
                Err(e) => return error_result(e),
            };
            let cols = db::scene_cols(&ori);
            if db::str_of(&scene, &cols.video_status) != "COMPLETED" {
                return error_result(format!(
                    "Video scene ({ori}) chưa COMPLETED — upscale chỉ chạy sau khi video xong (vf_generate_video)."
                ));
            }
            let core = state.core.clone();
            let (sid, pid2, ori2) = (scene_id.clone(), pid.clone(), ori.clone());
            tokio::spawn(async move {
                if let Err(e) = crate::process::upscale_video(&core, &sid, &pid2, &ori2).await {
                    eprintln!("[mcp] upscale_video {sid}: {e}");
                }
            });
            json_result(json!({
                "ok": true, "queued": true, "kind": "upscale",
                "scene_id": scene_id, "project_id": pid, "orientation": ori,
                "note": "Đang upscale 4K nền (cần tài khoản Flow TIER_TWO). Theo dõi: vf_scene_get (upscale_status) / vf_requests_status.",
            }))
        }

        // ---- observe ----
        "vf_requests_status" => {
            let pid = s(args, "project_id");
            let limit = int(args, "limit", 20).clamp(1, 200);
            let status = s(args, "status");
            let by_status = if pid.is_empty() {
                db.query("SELECT status, COUNT(*) AS n FROM request GROUP BY status", &[])
            } else {
                db.query(
                    "SELECT status, COUNT(*) AS n FROM request WHERE project_id = ?1 GROUP BY status",
                    &[&pid],
                )
            }
            .unwrap_or_default();
            let by_type = if pid.is_empty() {
                db.query("SELECT type, status, COUNT(*) AS n FROM request GROUP BY type, status", &[])
            } else {
                db.query(
                    "SELECT type, status, COUNT(*) AS n FROM request WHERE project_id = ?1 GROUP BY type, status",
                    &[&pid],
                )
            }
            .unwrap_or_default();
            let mut sql = String::from("SELECT * FROM request WHERE 1=1");
            let mut params: Vec<&dyn rusqlite::ToSql> = Vec::new();
            if !pid.is_empty() {
                sql.push_str(" AND project_id = ?");
                sql.push_str(&(params.len() + 1).to_string());
                params.push(&pid);
            }
            if !status.is_empty() {
                sql.push_str(" AND status = ?");
                sql.push_str(&(params.len() + 1).to_string());
                params.push(&status);
            }
            sql.push_str(&format!(" ORDER BY updated_at DESC LIMIT ?{}", params.len() + 1));
            params.push(&limit);
            let recent = db.query(&sql, &params).unwrap_or_default();
            json_result(json!({
                "extension_connected": state.core.ext.is_connected(),
                "worker_enabled": crate::config::worker_enabled(),
                "by_status": rows_value(by_status),
                "by_type": rows_value(by_type),
                "recent": rows_value(recent),
                "note": "Hàng FAILED có error_message — đọc nguyên nhân trước khi retry. PENDING video/upscale do worker xử lý khi extension connected.",
            }))
        }
        "vf_agents_list" => {
            let infos = state.pool.list_info();
            let items: Vec<Value> = infos
                .iter()
                .map(|a| {
                    let soul = crate::souls::load(&state.core.souls_dir, &a.agent_type);
                    json!({
                        "agent_type": a.agent_type,
                        "name": a.name,
                        "description": a.description,
                        "kind": a.kind,
                        "disabled": db.builtin_agent_disabled(&a.agent_type),
                        "has_soul": !soul.is_empty(),
                        "soul_excerpt": crate::llm::truncate(&soul, 180),
                    })
                })
                .collect();
            json_result(json!({
                "count": items.len(),
                "agents": items,
                "note": "Đọc/sửa prompt từng agent: vf_soul_get / vf_soul_set.",
            }))
        }
        "vf_soul_get" => {
            let t = s(args, "agent_type");
            if t.is_empty() {
                return error_result("agent_type là bắt buộc".into());
            }
            let raw = crate::souls::load_raw(&state.core.souls_dir, &t);
            if raw.is_empty() {
                let types: Vec<String> = state.pool.list_info().iter().map(|a| a.agent_type.clone()).collect();
                return error_result(format!(
                    "Chưa có soul file cho '{t}'. Các agent_type hiện có: {}",
                    types.join(", ")
                ));
            }
            json_result(json!({ "agent_type": t, "file": crate::souls::canonical_basename(&t), "content": raw }))
        }
        "vf_soul_set" => {
            let t = s(args, "agent_type");
            let content = args["content"].as_str().unwrap_or("");
            if t.is_empty() || content.trim().is_empty() {
                return error_result("agent_type và content là bắt buộc".into());
            }
            match crate::souls::write(&state.core.souls_dir, &t, content) {
                Ok(path) => json_result(json!({
                    "ok": true,
                    "agent_type": t,
                    "path": path.to_string_lossy(),
                    "note": "Áp dụng từ lần chạy TIẾP THEO của agent này (task đang chạy giữ prompt cũ).",
                })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "vf_generate_narration" => {
            // Resolve the scene set: explicit scene → its video; else video_id;
            // else the project's first video.
            let mut video_id = s(args, "video_id");
            let scene_id = s(args, "scene_id");
            if !scene_id.is_empty() && video_id.is_empty() {
                match db.get("scene", &scene_id) {
                    Ok(Some(sc)) => video_id = crate::db::str_of(&sc, "video_id"),
                    _ => return error_result(format!("không tìm thấy scene {scene_id}")),
                }
            }
            let mut project_id = s(args, "project_id");
            if video_id.is_empty() {
                if project_id.is_empty() {
                    return error_result("cần video_id, scene_id hoặc project_id".into());
                }
                match db.query_one(
                    "SELECT id FROM video WHERE project_id = ?1 ORDER BY display_order LIMIT 1",
                    &[&project_id],
                ) {
                    Ok(Some(v)) => video_id = crate::db::str_of(&v, "id"),
                    _ => return error_result("project chưa có video nào".into()),
                }
            }
            if project_id.is_empty() {
                project_id = db
                    .get("video", &video_id)
                    .ok()
                    .flatten()
                    .map(|v| crate::db::str_of(&v, "project_id"))
                    .unwrap_or_default();
            }

            // Narrating a whole video can take minutes on a CPU backend, so run
            // it as a DAG task and return immediately — same contract as the
            // image/video generate tools.
            let mut params = serde_json::Map::new();
            params.insert("video_id".into(), json!(video_id));
            for k in ["voice", "language", "model_id"] {
                let v = s(args, k);
                if !v.is_empty() {
                    params.insert(k.into(), json!(v));
                }
            }
            if let Some(sp) = args.get("speed").and_then(|x| x.as_f64()) {
                params.insert("speed".into(), json!(sp));
            }
            if args.get("regenerate").and_then(|x| x.as_bool()).unwrap_or(false) {
                params.insert("regenerate".into(), json!(true));
            }
            let prompt = Value::Object(params).to_string();

            let pool = state.pool.clone();
            let pid = project_id.clone();
            let task = crate::agents::Task {
                id: crate::db::new_id(),
                label: "narration".into(),
                agent_type: "audio".into(),
                prompt,
                timeout_seconds: 900,
                upstream_results: Default::default(),
            };
            tokio::spawn(async move {
                match pool.execute(&task, "", &pid).await {
                    Ok(r) => println!("[mcp] narration done: {}", r.summary),
                    Err(e) => eprintln!("[mcp] narration failed: {e}"),
                }
            });

            let pending = db
                .query(
                    "SELECT COUNT(*) AS n FROM scene WHERE video_id = ?1 \
                     AND narrator_text IS NOT NULL AND TRIM(narrator_text) <> ''",
                    &[&video_id],
                )
                .ok()
                .and_then(|r| r.first().map(|x| crate::db::i64_of(x, "n")))
                .unwrap_or(0);
            json_result(json!({
                "queued": true,
                "video_id": video_id,
                "project_id": project_id,
                "scenes_with_narrator_text": pending,
                "note": "Đang tổng hợp giọng bằng TTS của SenClaw (không cần Chrome extension). \
Theo dõi narrator_audio_status qua vf_scene_list.",
            }))
        }
        "vf_media_localize" => {
            let project_id = s(args, "project_id");
            let rep = crate::mediastore::localize_project(&state.core, &project_id).await;
            json_result(json!({
                "downloaded": rep.downloaded,
                "skipped_already_local": rep.skipped,
                "failed": rep.failed,
                "errors": rep.errors,
                "scope": if project_id.is_empty() { "toàn bộ app" } else { "project" },
            }))
        }
        "vf_fetch_video_urls" => {
            let project_id = s(args, "project_id");
            if project_id.is_empty() {
                return error_result("project_id là bắt buộc".into());
            }
            match crate::process::recover_media_urls(&state.core, &project_id).await {
                Ok(()) => {
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    let rep = crate::mediastore::localize_project(&state.core, &project_id).await;
                    json_result(json!({
                        "ok": true,
                        "downloaded": rep.downloaded,
                        "failed": rep.failed,
                        "note": "Link được bắt từ trang Flow rồi tải về máy. Kiểm tra lại bằng vf_scene_list.",
                    }))
                }
                Err(e) => error_result(e),
            }
        }
        "vf_playbook_find" => {
            let query = s(args, "query");
            if query.trim().is_empty() {
                return error_result("query là bắt buộc".into());
            }
            let all = crate::skillcat::scan(&state.core.playbooks_dir).unwrap_or_default();
            let hits = crate::skillcat::match_playbooks(&all, &query);
            if hits.is_empty() {
                return json_result(json!({
                    "matched": 0,
                    "hint": "Không có playbook nào khớp. Dùng vf_playbook_list để xem app làm được gì.",
                }));
            }
            let full = args.get("full").and_then(|v| v.as_bool()).unwrap_or(true);
            let best = &hits[0];
            json_result(json!({
                "matched": hits.len(),
                "best": {
                    "id": best.id,
                    "name": best.name,
                    "description": best.description,
                    "body": if full { best.body.clone() } else { String::new() },
                },
                "others": hits.iter().skip(1).take(3)
                    .map(|s| json!({ "id": s.id, "description": s.description }))
                    .collect::<Vec<_>>(),
            }))
        }
        "vf_playbook_list" => {
            let all = crate::skillcat::scan(&state.core.playbooks_dir).unwrap_or_default();
            json_result(json!({
                "count": all.len(),
                "playbooks": all.iter().map(|s| json!({
                    "id": s.id,
                    "description": s.description,
                    "triggers": s.triggers,
                })).collect::<Vec<_>>(),
            }))
        }
        "vf_tts_status" => {
            let settings = crate::tts::settings().await;
            let models = crate::tts::models().await;
            match (settings, models) {
                (Ok(s), Ok(m)) => json_result(json!({ "settings": s, "models": m })),
                (Err(e), _) | (_, Err(e)) => error_result(format!(
                    "không đọc được cấu hình TTS của SenClaw ({e}) — daemon có đang chạy không?"
                )),
            }
        }
        "vf_status" => {
            let pipelines = db
                .query("SELECT status, COUNT(*) AS n FROM dag_parents GROUP BY status", &[])
                .unwrap_or_default();
            json_result(json!({
                "app": "video-flow",
                "extension_connected": state.core.ext.is_connected(),
                "extension": state.core.ext.stats(),
                "worker_enabled": crate::config::worker_enabled(),
                "llm_profile": db.kv_get("llm.profile"),
                "default_orientation": crate::config::default_orientation(),
                "counts": {
                    "projects":   count(state, "SELECT COUNT(*) AS n FROM project"),
                    "videos":     count(state, "SELECT COUNT(*) AS n FROM video"),
                    "scenes":     count(state, "SELECT COUNT(*) AS n FROM scene"),
                    "characters": count(state, "SELECT COUNT(*) AS n FROM character"),
                    "requests_pending":    count(state, "SELECT COUNT(*) AS n FROM request WHERE status = 'PENDING'"),
                    "requests_processing": count(state, "SELECT COUNT(*) AS n FROM request WHERE status = 'PROCESSING'"),
                    "requests_failed":     count(state, "SELECT COUNT(*) AS n FROM request WHERE status = 'FAILED'"),
                },
                "pipelines_by_status": rows_value(pipelines),
                "note": if state.core.ext.is_connected() {
                    "Sẵn sàng sinh ảnh/video."
                } else {
                    "Extension CHƯA kết nối — load extension/ vào Chrome, mở labs.google (Flow) để bắt token; extension nối WS về app (mặc định :9222)."
                },
            }))
        }

        _ => error_result(format!("Unknown tool: {name}")),
    }
}
