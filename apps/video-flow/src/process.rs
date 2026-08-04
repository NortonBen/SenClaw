//! Google Flow generation processes — port of `internal/agent/process/*` (Go:
//! process.go, scene.go, video_scene.go, entity.go, ws.go, scene_lock.go).
//! Every Flow API call is proxied through the Chrome extension bridge via the
//! `api_request` method; media IDs must be UUIDs (never base64 CAMS ids) and
//! are recovered from fifeUrl when the API omits them.

use crate::config;
use crate::db::{self, Row};
use crate::state::Core;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant};

const IMAGE_MODEL: &str = "GEM_PIX_2";

const PATH_VIDEO_START_IMAGE: &str = "/v1/video:batchAsyncGenerateVideoStartImage";
const PATH_VIDEO_START_END_IMAGE: &str = "/v1/video:batchAsyncGenerateVideoStartAndEndImage";
const PATH_VIDEO_CHECK_STATUS: &str = "/v1/video:batchCheckAsyncVideoGenerationStatus";

const MEDIA_GEN_SUCCESS: &str = "MEDIA_GENERATION_STATUS_SUCCESSFUL";
const MEDIA_GEN_FAILED: &str = "MEDIA_GENERATION_STATUS_FAILED";

/// Space Flow video submits process-wide. A flurry of captcha-consuming video
/// calls in the same second is exactly what Google's reCAPTCHA scores as
/// `UNUSUAL_ACTIVITY`; holding a min-gap between them keeps the traffic looking
/// human even while scenes render in parallel.
async fn throttle_video_submit() {
    static GATE: OnceLock<tokio::sync::Mutex<Instant>> = OnceLock::new();
    let gap = Duration::from_millis(config::video_submit_gap_ms());
    if gap.is_zero() {
        return;
    }
    let gate = GATE.get_or_init(|| tokio::sync::Mutex::new(Instant::now() - gap));
    let mut last = gate.lock().await;
    let elapsed = last.elapsed();
    if elapsed < gap {
        tokio::time::sleep(gap - elapsed).await;
    }
    *last = Instant::now();
}

#[derive(Debug, Clone)]
pub struct GenOutcome {
    pub media_id: String,
    pub url: String,
}

// ---------------------------------------------------------------------------
// public API
// ---------------------------------------------------------------------------

/// Port of EntityImage.Run — reference image for one character/entity.
pub async fn entity_image(
    core: &Core,
    character_id: &str,
    project_id: &str,
    regenerate: bool,
) -> Result<GenOutcome, String> {
    println!(
        "[EntityImage] run char={character_id} project={project_id} ext_connected={}",
        core.ext.is_connected()
    );
    let ch = core
        .db
        .get("character", character_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("character not found: {character_id}"))?;
    let proj = core
        .db
        .get("project", project_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("project not found: {project_id}"))?;

    // Dedup: an entity that already has a reference image is done unless forced.
    let existing = db::str_of(&ch, "media_id");
    if !regenerate && !existing.is_empty() {
        return Ok(GenOutcome {
            media_id: existing,
            url: db::str_of(&ch, "reference_image_url"),
        });
    }

    let entity_type = db::str_of(&ch, "entity_type").to_lowercase();
    let mut prompt = db::str_of(&ch, "image_prompt");
    if prompt.is_empty() {
        let name = db::str_of(&ch, "name");
        let desc = db::str_of(&ch, "description");
        prompt = match entity_type.as_str() {
            "location" => format!("Location establishing shot: {name}. {desc}, photorealistic"),
            "creature" => format!(
                "Character model sheet (turnaround reference) of {name}: the SAME creature shown \
                 front view, 3/4 view, and side profile full-body, plus a head close-up, side by \
                 side on a plain neutral light-grey studio background, flat even lighting, neutral \
                 pose, no scene. {desc}. Consistent identity across every view, photorealistic."
            ),
            "visual_asset" => {
                format!("Product/prop reference: {name}. {desc}, neutral background, studio lighting")
            }
            "generic_troop" => format!(
                "Troop reference group shot: {name}. {desc}, neutral background, photorealistic"
            ),
            "faction" => format!(
                "Faction emblem/insignia: {name}. {desc}, clean background, graphic design style"
            ),
            // "character" and unknown
            _ => format!(
                "Character model sheet (turnaround reference) of {name}: the SAME person shown in a \
                 horizontal row — front view, 3/4 view, and side profile full-body — PLUS one \
                 head-and-shoulders face close-up, on a plain neutral light-grey studio background, \
                 flat even lighting, neutral standing pose, neutral expression, no props, no scene. \
                 {desc}. Keep every identity trait (face, hair, skin tone, build, outfit) identical \
                 across all views. Photorealistic."
            ),
        };
    }

    // A model sheet lays several views side by side, so characters/creatures use
    // a wide frame; locations/troops are already wide.
    let aspect = match entity_type.as_str() {
        "location" | "generic_troop" | "character" | "creature" => "16:9",
        // Unknown entity_type defaults to character (model sheet) too.
        e if e.is_empty() => "16:9",
        _ => "1:1",
    };
    let tier = paygate_tier(&proj);

    let _ = ensure_flow_project(core).await;
    let flow_project = effective_flow_project(core, project_id);
    let raw = api_request(
        core,
        build_image_params(&flow_project, &prompt, aspect, &tier, &[]),
    )
    .await
    .map_err(|e| format!("WS error: {e}"))?;
    let flow_err = extract_flow_error(&raw);
    if !flow_err.is_empty() {
        return Err(format!("flow API error: {flow_err}"));
    }

    let (mid, url) = extract_result(&raw);
    if mid.is_empty() {
        return Err("no media_id in response".to_string());
    }

    let mut f = Row::new();
    f.insert("media_id".into(), json!(mid));
    if !url.is_empty() {
        f.insert("reference_image_url".into(), json!(url));
    }
    let _ = core.db.update("character", character_id, &f);

    // Flow's signed URLs expire; keep a local copy so the reference survives.
    let url = crate::mediastore::localize_column(
        core,
        "character",
        character_id,
        "reference_image_url",
        &url,
        "image",
    )
    .await
    .unwrap_or(url);

    Ok(GenOutcome { media_id: mid, url })
}

/// Port of SceneImage.Run — still image for one scene (one orientation).
/// `edit_prompt` (EDIT_IMAGE) overrides the scene prompt and keeps the current
/// still as a reference input so the model edits rather than re-imagines.
pub async fn scene_image(
    core: &Core,
    scene_id: &str,
    project_id: &str,
    orientation: &str,
    regenerate: bool,
    edit_prompt: Option<&str>,
) -> Result<GenOutcome, String> {
    println!(
        "[SceneImage] run scene={scene_id} project={project_id} ext_connected={}",
        core.ext.is_connected()
    );
    let sc = core
        .db
        .get("scene", scene_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("scene not found: {scene_id}"))?;
    let proj = core
        .db
        .get("project", project_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("project not found: {project_id}"))?;

    let orient = norm_orientation(orientation);
    let cols = db::scene_cols(&orient);
    let edit = edit_prompt.map(str::trim).filter(|s| !s.is_empty());

    // Dedup: skip when already COMPLETED unless regenerating or editing.
    if !regenerate
        && edit.is_none()
        && db::str_of(&sc, &cols.image_status).eq_ignore_ascii_case("COMPLETED")
    {
        return Ok(GenOutcome {
            media_id: db::str_of(&sc, &cols.image_media_id),
            url: db::str_of(&sc, &cols.image_url),
        });
    }

    let prompt = match edit {
        Some(p) => p.to_string(),
        None => {
            let mut p = db::str_of(&sc, "image_prompt");
            if p.is_empty() {
                p = db::str_of(&sc, "prompt");
            }
            // Reinforce each named character's invariant look in the text prompt,
            // alongside the reference image, so identity stays consistent.
            let appearance = scene_ref_appearance(core, project_id, &sc);
            if !appearance.is_empty() && !p.contains("Character appearance") {
                p = format!("{}. {appearance}", p.trim().trim_end_matches('.'));
            }
            p
        }
    };

    let aspect = if orient == "HORIZONTAL" {
        "16:9"
    } else {
        "9:16"
    };
    let tier = paygate_tier(&proj);

    let mut refs = scene_ref_media_ids(core, project_id, &sc);
    if edit.is_some() {
        let cur = db::str_of(&sc, &cols.image_media_id);
        if !cur.is_empty() && !refs.contains(&cur) {
            refs.insert(0, cur);
        }
    }

    let _ = ensure_flow_project(core).await;
    let flow_project = effective_flow_project(core, project_id);
    let raw = api_request(
        core,
        build_image_params(&flow_project, &prompt, aspect, &tier, &refs),
    )
    .await
    .map_err(|e| format!("WS error: {e}"))?;
    let flow_err = extract_flow_error(&raw);
    if !flow_err.is_empty() {
        return Err(format!("flow API error: {flow_err}"));
    }

    let (mid, url) = extract_result(&raw);
    if mid.is_empty() {
        return Err("no media_id in response".to_string());
    }

    let mut f = Row::new();
    f.insert(cols.image_media_id.clone(), json!(mid));
    f.insert(cols.image_status.clone(), json!("COMPLETED"));
    if !url.is_empty() {
        f.insert(cols.image_url.clone(), json!(url));
    }
    let _ = core.db.update("scene", scene_id, &f);

    // C1 cascade: a new still invalidates any downstream clip + upscale.
    let _ = core.db.cascade_after_image(scene_id, &orient);

    // Mirror locally before the signed URL expires.
    let url =
        crate::mediastore::localize_column(core, "scene", scene_id, &cols.image_url, &url, "image")
            .await
            .unwrap_or(url);

    core.dash.emit(
        "scene_updated",
        json!({ "project_id": project_id, "scene_id": scene_id }),
    );

    Ok(GenOutcome { media_id: mid, url })
}

/// Port of SceneVideo.Run — async Veo3 clip: submit, then poll until done.
pub async fn scene_video(
    core: &Core,
    scene_id: &str,
    project_id: &str,
    orientation: &str,
    regenerate: bool,
) -> Result<GenOutcome, String> {
    println!(
        "[SceneVideo] run scene={scene_id} project={project_id} ext_connected={}",
        core.ext.is_connected()
    );
    if scene_id.is_empty() || project_id.is_empty() {
        return Err("scene and project are required".to_string());
    }
    let orient = norm_orientation(orientation);
    let cols = db::scene_cols(&orient);

    // C2: serialize per scene+orientation so the DAG video agent and the
    // request-queue worker can't submit the same clip concurrently.
    let lock = scene_video_lock(scene_id, &orient);
    let _guard = lock.lock().await;

    let sc = core
        .db
        .get("scene", scene_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "scene not found".to_string())?;

    // C2 dedup: the loser of the lock no-ops instead of regenerating.
    if !regenerate && db::str_of(&sc, &cols.video_status).eq_ignore_ascii_case("COMPLETED") {
        println!("[SceneVideo] scene={scene_id} {orient} already has a clip — skipping (REGENERATE to force)");
        let out = GenOutcome {
            media_id: db::str_of(&sc, &cols.video_media_id),
            url: db::str_of(&sc, &cols.video_url),
        };
        core.dash.emit(
            "scene_updated",
            json!({ "project_id": project_id, "scene_id": scene_id }),
        );
        return Ok(out);
    }

    let proj = core
        .db
        .get("project", project_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "project not found".to_string())?;
    let tier = paygate_tier(&proj);

    let start_media_id = db::str_of(&sc, &cols.image_media_id);
    if start_media_id.is_empty() {
        return Err("scene has no image media_id — generate image first".to_string());
    }
    let end_media_id = db::str_of(&sc, &cols.end_scene_media_id);

    let mut video_prompt = build_video_prompt(&sc);
    // Same identity reinforcement as the still: the clip animates from the frame-0
    // image but a text reminder keeps the face/outfit from drifting mid-shot.
    let appearance = scene_ref_appearance(core, project_id, &sc);
    if !appearance.is_empty() && !video_prompt.contains("Character appearance") {
        video_prompt = format!(
            "{}. {appearance}",
            video_prompt.trim().trim_end_matches('.')
        );
    }

    let aspect_ratio = if orient == "HORIZONTAL" {
        "VIDEO_ASPECT_RATIO_LANDSCAPE"
    } else {
        "VIDEO_ASPECT_RATIO_PORTRAIT"
    };

    let submit_url = if end_media_id.is_empty() {
        build_url(PATH_VIDEO_START_IMAGE)
    } else {
        build_url(PATH_VIDEO_START_END_IMAGE)
    };
    let has_end = !end_media_id.is_empty();
    // Make sure a real, browsable Flow project exists (look up or create one),
    // then send Flow that id so the clip lands in a project the user can open
    // and the URL scrape can read. Non-fatal: falls back to the app id.
    if let Err(e) = ensure_flow_project(core).await {
        eprintln!("[SceneVideo] ensure_flow_project: {e}");
    }
    let _ = ensure_flow_project(core).await;
    let flow_project = effective_flow_project(core, project_id);
    let submit = |model_key: &str| {
        api_request(
            core,
            build_video_submit_params(
                &submit_url,
                &flow_project,
                scene_id,
                &video_prompt,
                &start_media_id,
                &end_media_id,
                aspect_ratio,
                &tier,
                model_key,
            ),
        )
    };

    // Try the resolved model, then step down through models the account is more
    // likely to have: plain Fast (TIER_ONE, normal operations response), then
    // Lite Low Priority (every tier, 0 credits, base64 response). Each step only
    // fires on a MODEL_ACCESS_DENIED — any other error is returned as-is.
    let mut candidates: Vec<String> = vec![resolve_video_model(core, &tier, aspect_ratio, has_end)];
    let non_ultra = video_model_key("PAYGATE_TIER_ONE", aspect_ratio, has_end).to_string();
    if !candidates.contains(&non_ultra) {
        candidates.push(non_ultra);
    }
    if !candidates.iter().any(|k| k == VIDEO_MODEL_LITE) {
        candidates.push(VIDEO_MODEL_LITE.to_string());
    }
    // Move keys the account was already denied to the BACK so a persisted denial
    // (e.g. Lite/ultra on a plan without it) isn't re-submitted first on every
    // scene — that wasted request + throttle gap is what the denial memory exists
    // to avoid. Stable sort keeps the preferred order within each group.
    let denied = denied_video_models(core);
    candidates.sort_by_key(|k| denied.contains(k));

    let mut raw = Value::Null;
    let mut flow_err = String::new();
    for (i, model_key) in candidates.iter().enumerate() {
        if i > 0 {
            eprintln!(
                "[SceneVideo] model trước bị từ chối ({flow_err}); thử lại với `{model_key}`"
            );
        }
        throttle_video_submit().await;
        raw = submit(model_key).await?;
        flow_err = extract_flow_error(&raw);
        if flow_err.is_empty() || !is_model_access_denied(&flow_err) {
            break;
        }
        // Account can't use this key — remember so it's skipped first next time.
        mark_video_model_denied(core, model_key);
    }
    if !flow_err.is_empty() {
        if is_recaptcha_failure(&flow_err) {
            return Err(format!(
                "Google chặn vì nghi hoạt động tự động (reCAPTCHA): {flow_err}. \
                 Mở tab Google Flow đang đăng nhập, thao tác tay vài lần cho \"ấm\" phiên, \
                 giảm số video sinh cùng lúc, rồi thử lại sau ít phút."
            ));
        }
        return Err(format!("flow API error: {flow_err}"));
    }

    // Veo 3.1 Lite (Low Priority) returns the clip as an inline base64 MP4 right
    // in the submit body — if it's there, the render is already done, whatever
    // the envelope, so take it before poll/operations handling.
    if let Some((mid, url)) = inline_video_from_raw(core, &raw).await {
        println!("[SceneVideo] scene={scene_id} inline MP4 from Low Priority render");
        let mut f = Row::new();
        f.insert(cols.video_media_id.clone(), json!(mid));
        f.insert(cols.video_status.clone(), json!("COMPLETED"));
        f.insert(cols.video_url.clone(), json!(url));
        let _ = core.db.update("scene", scene_id, &f);
        let _ = core.db.cascade_after_video(scene_id, &orient);
        core.dash.emit(
            "scene_updated",
            json!({ "project_id": project_id, "scene_id": scene_id }),
        );
        return Ok(GenOutcome { media_id: mid, url });
    }

    let (ops, envelope) = extract_video_ops(&raw);
    if ops.is_empty() {
        // Carry the actual payload: the extension reports "done" for any HTTP
        // 200, so without the body this failure is undiagnosable.
        let body = serde_json::to_string(&raw).unwrap_or_default();
        eprintln!("[SceneVideo] scene={scene_id} submit returned no entries; raw = {body}");
        return Err(format!(
            "video submit trả HTTP 200 nhưng không có operation/media nào. Body: {}",
            crate::llm::truncate(&body, 400)
        ));
    }
    println!(
        "[SceneVideo] scene={scene_id} submitted, {} entry ({:?} envelope)",
        ops.len(),
        envelope
    );
    // Flow answers with ITS OWN project id — the only handle for loading the
    // project page later to scrape media URLs.
    remember_flow_project(core, project_id, &raw);

    let check_url = build_url(PATH_VIDEO_CHECK_STATUS);
    let (mid, video_url) = poll_video_ops(core, &check_url, ops, envelope).await?;
    if mid.is_empty() {
        return Err("no video media_id in response".to_string());
    }

    let mut f = Row::new();
    f.insert(cols.video_media_id.clone(), json!(mid));
    f.insert(cols.video_status.clone(), json!("COMPLETED"));
    if !video_url.is_empty() {
        f.insert(cols.video_url.clone(), json!(video_url));
    }
    let _ = core.db.update("scene", scene_id, &f);

    // C1 cascade: a fresh clip invalidates any existing upscale.
    let _ = core.db.cascade_after_video(scene_id, &orient);

    let mut video_url = crate::mediastore::localize_column(
        core,
        "scene",
        scene_id,
        &cols.video_url,
        &video_url,
        "video",
    )
    .await
    .unwrap_or(video_url);

    // Rendered but linkless: Flow's generation API no longer returns a URL, so
    // go get one instead of leaving a clip the user cannot watch.
    if video_url.is_empty() {
        match recover_media_urls(core, project_id).await {
            Ok(()) => {
                // The scraper writes URLs asynchronously via the extension's
                // media_urls_refresh event; give it a moment, then re-read.
                tokio::time::sleep(Duration::from_secs(3)).await;
                if let Ok(Some(fresh)) = core.db.get("scene", scene_id) {
                    video_url = db::str_of(&fresh, &cols.video_url);
                }
                if video_url.is_empty() {
                    eprintln!("[SceneVideo] scene={scene_id} vẫn chưa có URL sau khi quét Flow");
                } else {
                    println!("[SceneVideo] scene={scene_id} lấy được URL từ trang Flow");
                }
            }
            Err(e) => eprintln!("[SceneVideo] không lấy được URL video: {e}"),
        }
    }

    core.dash.emit(
        "scene_updated",
        json!({ "project_id": project_id, "scene_id": scene_id }),
    );

    Ok(GenOutcome {
        media_id: mid,
        url: video_url,
    })
}

/// Port of the worker's processUpscale core — extension-side `upscale_video`
/// method (the Go path was hardwired to vertical; orientation generalizes it).
pub async fn upscale_video(
    core: &Core,
    scene_id: &str,
    project_id: &str,
    orientation: &str,
) -> Result<GenOutcome, String> {
    let orient = norm_orientation(orientation);
    let cols = db::scene_cols(&orient);
    let sc = core
        .db
        .get("scene", scene_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "scene not found".to_string())?;

    let media_id = db::str_of(&sc, &cols.video_media_id);
    if media_id.is_empty() {
        return Err("no video to upscale".to_string());
    }

    let raw = core
        .ext
        .call(
            "upscale_video",
            json!({ "project_id": project_id, "media_id": media_id, "resolution": "4K" }),
            Duration::from_secs(config::worker_gen_timeout_secs()),
        )
        .await?;
    if let Some(e) = raw.get("error").and_then(|v| v.as_str()) {
        if !e.trim().is_empty() {
            return Err(e.to_string());
        }
    }

    let mid = match raw.get("media_id").and_then(|v| v.as_str()) {
        Some(v) if !v.is_empty() => v.to_string(),
        _ => media_id,
    };
    let url = raw
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let mut f = Row::new();
    f.insert(cols.upscale_media_id.clone(), json!(mid));
    f.insert(cols.upscale_status.clone(), json!("COMPLETED"));
    f.insert(cols.upscale_url.clone(), json!(url));
    let _ = core.db.update("scene", scene_id, &f);

    let url = crate::mediastore::localize_column(
        core,
        "scene",
        scene_id,
        &cols.upscale_url,
        &url,
        "video",
    )
    .await
    .unwrap_or(url);

    core.dash.emit(
        "scene_updated",
        json!({ "project_id": project_id, "scene_id": scene_id }),
    );

    Ok(GenOutcome { media_id: mid, url })
}

/// Port of AgentImage.ProcessAllEntities — ref images for every project entity
/// still missing one. Returns the generated count; errors only when every
/// attempt failed.
pub async fn process_all_entities(core: &Core, project_id: &str) -> Result<usize, String> {
    if !core.ext.is_connected() {
        return Err("image agent: extension bridge not connected".to_string());
    }
    let chars =
        list_project_characters(core, project_id).map_err(|e| format!("list characters: {e}"))?;
    let mut count = 0usize;
    let mut attempts = 0usize;
    let mut last_err = String::new();
    for ch in &chars {
        if !db::str_of(ch, "media_id").is_empty() {
            continue;
        }
        attempts += 1;
        let cid = db::str_of(ch, "id");
        match entity_image(core, &cid, project_id, false).await {
            Ok(_) => count += 1,
            Err(e) => {
                eprintln!("[AgentImage] entity {cid}: {e}");
                last_err = e;
            }
        }
    }
    if count == 0 && attempts > 0 {
        return Err(format!(
            "all {attempts} entity image generations failed: {last_err}"
        ));
    }
    Ok(count)
}

// ---------------------------------------------------------------------------
// extension bridge transport
// ---------------------------------------------------------------------------

async fn api_request(core: &Core, params: Value) -> Result<Value, String> {
    core.ext
        .call(
            "api_request",
            params,
            Duration::from_secs(config::worker_gen_timeout_secs()),
        )
        .await
}

// ---------------------------------------------------------------------------
// envelope building (Google Flow API)
// ---------------------------------------------------------------------------

fn paygate_tier(proj: &Row) -> String {
    let t = db::str_of(proj, "user_paygate_tier");
    if t.is_empty() {
        "PAYGATE_TIER_TWO".to_string()
    } else {
        t
    }
}

fn norm_orientation(orientation: &str) -> String {
    let u = orientation.trim().to_uppercase();
    if u.is_empty() {
        "VERTICAL".to_string()
    } else {
        u
    }
}

/// Full API URL; the key query param is attached only when configured (the
/// extension side may inject auth itself).
fn build_url(path: &str) -> String {
    let base = config::google_flow_api();
    let key = config::google_api_key();
    if key.is_empty() {
        format!("{base}{path}")
    } else {
        format!("{base}{path}?key={key}")
    }
}

fn build_image_url(project_id: &str) -> String {
    build_url(&format!(
        "/v1/projects/{project_id}/flowMedia:batchGenerateImages"
    ))
}

fn image_aspect_ratio(aspect: &str) -> &'static str {
    if aspect.to_uppercase() == "HORIZONTAL" || aspect == "16:9" {
        "IMAGE_ASPECT_RATIO_LANDSCAPE"
    } else {
        "IMAGE_ASPECT_RATIO_PORTRAIT"
    }
}

/// The project id to send Flow. Prefer the real, browsable project stored in
/// `flow.session_project` (created/looked-up via `ensure_flow_project`, or
/// learned by the extension) — the app's own id is accepted for generation but
/// Flow renders no browsable/scrapable project for it, breaking viewing and the
/// URL scrape. Falls back to the app's project id when nothing real is known.
fn effective_flow_project(core: &Core, project_id: &str) -> String {
    let real = core.db.kv_get("flow.session_project");
    if is_uuid(&real) {
        real
    } else {
        project_id.to_string()
    }
}

const FLOW_TRPC_BASE: &str = "https://labs.google/fx/api/trpc";

/// One tRPC call through the extension's authenticated `trpc_request` bridge.
async fn flow_trpc(
    core: &Core,
    proc: &str,
    method: &str,
    body: Option<Value>,
) -> Result<Value, String> {
    let mut params = serde_json::Map::new();
    params.insert("url".into(), json!(format!("{FLOW_TRPC_BASE}/{proc}")));
    params.insert("method".into(), json!(method));
    if let Some(b) = body {
        params.insert("body".into(), b);
    }
    core.ext
        .call(
            "trpc_request",
            Value::Object(params),
            Duration::from_secs(30),
        )
        .await
}

/// Deep-scan any Flow response for the first `projectId` (a UUID). The tRPC
/// envelope nests it deeply and differently per procedure, so key on the field.
fn deep_find_project_id(v: &Value) -> Option<String> {
    match v {
        Value::Object(o) => {
            if let Some(s) = o.get("projectId").and_then(|x| x.as_str()) {
                if is_uuid(s) {
                    return Some(s.to_string());
                }
            }
            o.values().find_map(deep_find_project_id)
        }
        Value::Array(a) => a.iter().find_map(deep_find_project_id),
        _ => None,
    }
}

/// Ensure a real, browsable Flow project exists and return its id. Order:
/// cached `flow.session_project` → the user's newest existing Flow project
/// (`project.searchUserProjects`) → a freshly created one (`project.createProject`).
/// Result is cached so this costs one round-trip per process, not per scene.
pub async fn ensure_flow_project(core: &Core) -> Result<String, String> {
    let cached = core.db.kv_get("flow.session_project");
    if is_uuid(&cached) {
        return Ok(cached);
    }
    if !core.ext.is_connected() {
        return Err("Chrome extension chưa kết nối".to_string());
    }

    // Reuse the user's most recent existing project if they have one.
    if let Ok(raw) = flow_trpc(core, "project.searchUserProjects", "GET", None).await {
        if let Some(pid) = deep_find_project_id(&raw) {
            let _ = core.db.kv_set("flow.session_project", &pid);
            println!("[flow] dùng project sẵn có {pid}");
            return Ok(pid);
        }
    }

    // None — create one on the user's Flow account.
    let title = format!(
        "Video Flow — {}",
        chrono::Utc::now().format("%Y-%m-%d %H:%M")
    );
    let raw = flow_trpc(
        core,
        "project.createProject",
        "POST",
        Some(json!({ "json": { "projectTitle": title, "toolName": "PINHOLE" } })),
    )
    .await?;
    let flow_err = extract_flow_error(&raw);
    if !flow_err.is_empty() {
        return Err(format!("createProject: {flow_err}"));
    }
    let pid = deep_find_project_id(&raw).ok_or_else(|| {
        format!(
            "createProject không trả projectId: {}",
            crate::llm::truncate(&raw.to_string(), 300)
        )
    })?;
    let _ = core.db.kv_set("flow.session_project", &pid);
    println!("[flow] đã tạo project mới {pid}");
    Ok(pid)
}

fn client_ctx(project_id: &str, tier: &str) -> Value {
    let tier = if tier.is_empty() {
        "PAYGATE_TIER_TWO"
    } else {
        tier
    };
    json!({
        "projectId": project_id,
        "recaptchaContext": {
            "applicationType": "RECAPTCHA_APPLICATION_TYPE_WEB",
            "token": "",
        },
        "sessionId": format!(";{}", chrono::Utc::now().timestamp_millis()),
        "tool": "PINHOLE",
        "userPaygateTier": tier,
    })
}

/// `api_request` params for batchGenerateImages (Go buildImageMsg, params only —
/// `ExtBridge::call` supplies the envelope id/method).
fn build_image_params(
    project_id: &str,
    prompt: &str,
    aspect: &str,
    tier: &str,
    ref_media_ids: &[String],
) -> Value {
    let http_url = build_image_url(project_id);
    let ts = chrono::Utc::now().timestamp_millis();
    let outer = client_ctx(project_id, tier);
    let mut req_ctx = outer.clone();
    req_ctx["sessionId"] = json!(format!(";{ts}"));

    let mut request_item = json!({
        "clientContext": req_ctx,
        "seed": ts % 1_000_000,
        "structuredPrompt": { "parts": [ { "text": prompt } ] },
        "imageAspectRatio": image_aspect_ratio(aspect),
        "imageModelName": IMAGE_MODEL,
    });
    let mut body = json!({ "clientContext": outer });
    if !ref_media_ids.is_empty() {
        let inputs: Vec<Value> = ref_media_ids
            .iter()
            .map(|mid| json!({ "name": mid, "imageInputType": "IMAGE_INPUT_TYPE_REFERENCE" }))
            .collect();
        request_item["imageInputs"] = json!(inputs);
        body["mediaGenerationContext"] = json!({ "batchId": db::new_id() });
        body["useNewMedia"] = json!(true);
    }
    body["requests"] = json!([request_item]);

    json!({
        "url": http_url,
        "method": "POST",
        "headers": { "content-type": "application/json", "accept": "*/*" },
        "body": body,
        "captchaAction": "IMAGE_GENERATION",
    })
}

/// `api_request` params for batchAsyncGenerateVideoStartImage / StartAndEndImage.
#[allow(clippy::too_many_arguments)]
fn build_video_submit_params(
    http_url: &str,
    project_id: &str,
    scene_id: &str,
    prompt: &str,
    start_media_id: &str,
    end_media_id: &str,
    aspect_ratio: &str,
    tier: &str,
    model_key: &str,
) -> Value {
    let seed = chrono::Utc::now().timestamp() % 10_000;
    let key = if model_key.is_empty() {
        video_model_key(tier, aspect_ratio, !end_media_id.is_empty()).to_string()
    } else {
        model_key.to_string()
    };
    let mut req = json!({
        "aspectRatio": aspect_ratio,
        "seed": seed,
        "textInput": { "structuredPrompt": { "parts": [ { "text": prompt } ] } },
        "videoModelKey": key,
        "startImage": { "mediaId": start_media_id },
        "metadata": { "sceneId": scene_id },
    });
    if !end_media_id.is_empty() {
        req["endImage"] = json!({ "mediaId": end_media_id });
    }
    json!({
        "url": http_url,
        "method": "POST",
        "headers": { "content-type": "application/json", "accept": "*/*" },
        "body": {
            "mediaGenerationContext": { "batchId": db::new_id() },
            "clientContext": client_ctx(project_id, tier),
            "requests": [ req ],
            "useV2ModelConfig": true,
        },
        "captchaAction": "VIDEO_GENERATION",
    })
}

/// `api_request` params for batchCheckAsyncVideoGenerationStatus (no captchaAction).
/// Echo the entries back under the key matching the envelope they arrived in —
/// the check endpoint rejects a body keyed the other way.
fn build_video_poll_params(check_url: &str, ops: &[Value], env: VideoEnvelope) -> Value {
    let mut body = serde_json::Map::new();
    body.insert(env.body_key().to_string(), json!(ops));
    json!({
        "url": check_url,
        "method": "POST",
        "headers": { "content-type": "application/json", "accept": "*/*" },
        "body": Value::Object(body),
    })
}

/// The video model key to send, most-authoritative source first:
///
/// 1. `FLOWKIT_VIDEO_MODEL` — an explicit override (full manual control).
/// 2. `flow.video_model` in app_kv — the real key the extension learned from
///    Flow's own tRPC traffic. This is how "Veo 3.1 Lite" takes effect: pick it
///    in Flow's agent settings, and the app adopts the exact internal key
///    instead of guessing one that might not exist.
/// 3. The hardcoded tier matrix — a known-good fallback (Fast) so generation
///    still works before anything has been learned.
///
/// An orientation guard on the learned key: it carries the aspect it was
/// captured for, so it's only reused when the current render matches; otherwise
/// we fall back rather than send a portrait key for a landscape clip.
/// Video model keys this account was told it can't access (403
/// MODEL_ACCESS_DENIED). Recorded so a preference the account can't use (e.g.
/// Lite on a PRO/TIER_ONE plan) isn't retried first on every scene.
fn denied_video_models(core: &Core) -> std::collections::HashSet<String> {
    core.db
        .kv_get("flow.video_model_denied")
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

fn mark_video_model_denied(core: &Core, key: &str) {
    let mut set = denied_video_models(core);
    if set.insert(key.to_string()) {
        let joined = set.into_iter().collect::<Vec<_>>().join(",");
        let _ = core.db.kv_set("flow.video_model_denied", &joined);
        eprintln!("[SceneVideo] ghi nhớ model bị từ chối: {key}");
    }
}

fn resolve_video_model(core: &Core, tier: &str, aspect_ratio: &str, has_end: bool) -> String {
    if let Ok(v) = std::env::var("FLOWKIT_VIDEO_MODEL") {
        let v = v.trim();
        if !v.is_empty() {
            return v.to_string();
        }
    }
    // Skip keys the account has already been denied — pick the first choice it
    // can actually use, ending at the tier matrix (TIER_ONE → plain Fast).
    let denied = denied_video_models(core);
    let ok = |k: String| -> Option<String> {
        if denied.contains(&k) {
            None
        } else {
            Some(k)
        }
    };

    // A custom literal key set in Settings wins over everything below it.
    let custom = core.db.kv_get("video.model_key");
    if custom.starts_with("veo") {
        if let Some(k) = ok(custom) {
            return k;
        }
    }
    // Explicit tier picked in Settings. `lite` is the 0-credit Low Priority model
    // (inline base64 MP4). `fast` uses the tier matrix. If that choice is already
    // known-denied for this account, fall through instead of wasting an attempt.
    match core.db.kv_get("video.model_tier").as_str() {
        "lite" => {
            if let Some(k) = ok(VIDEO_MODEL_LITE.to_string()) {
                return k;
            }
        }
        "fast" => {
            if let Some(k) = ok(video_model_key(tier, aspect_ratio, has_end).to_string()) {
                return k;
            }
        }
        _ => {}
    }
    let learned = core.db.kv_get("flow.video_model");
    if learned.starts_with("veo") {
        let is_portrait_key = learned.contains("portrait");
        let want_portrait = aspect_ratio == "VIDEO_ASPECT_RATIO_PORTRAIT";
        if is_portrait_key == want_portrait {
            if let Some(k) = ok(learned) {
                return k;
            }
        }
    }
    // Tier matrix: TIER_ONE → plain Fast (what a PRO account can use).
    video_model_key(tier, aspect_ratio, has_end).to_string()
}

/// Veo 3.1 Lite: the 0-credit Low Priority model. One key for both frame and
/// start+end, valid on every service tier — but its result comes back as an
/// inline base64 MP4 (in a `workflows` envelope), not an operations URL.
const VIDEO_MODEL_LITE: &str = "veo_3_1_i2v_lite_low_priority";

/// Veo 3.1 image-to-video model key by tier, aspect ratio, and end-image.
///
/// The `_ultra` (Fast) family needs SERVICE_TIER_ULTRA — a PRO/ADVANCED account
/// requesting it gets a 403 `PUBLIC_ERROR_MODEL_ACCESS_DENIED`. So only the
/// TIER_TWO branch asks for `_ultra`; TIER_ONE uses the plain Fast keys. Either
/// way, a denial is caught and retried with Lite Low Priority (all tiers) by the
/// caller, so this only has to pick a good first attempt.
fn video_model_key(tier: &str, aspect_ratio: &str, has_end: bool) -> &'static str {
    let portrait = aspect_ratio != "VIDEO_ASPECT_RATIO_LANDSCAPE";
    let ultra = tier != "PAYGATE_TIER_ONE"; // TIER_TWO / default → ultra
    match (portrait, has_end, ultra) {
        (true, false, false) => "veo_3_1_i2v_s_fast_portrait",
        (false, false, false) => "veo_3_1_i2v_s_fast",
        (true, true, false) => "veo_3_1_i2v_s_fast_portrait_fl",
        (false, true, false) => "veo_3_1_i2v_s_fast_fl",
        (true, false, true) => "veo_3_1_i2v_s_fast_portrait_ultra",
        (false, false, true) => "veo_3_1_i2v_s_fast_ultra",
        (true, true, true) => "veo_3_1_i2v_s_fast_portrait_ultra_fl",
        (false, true, true) => "veo_3_1_i2v_s_fast_ultra_fl",
    }
}

// ---------------------------------------------------------------------------
// response parsing
// ---------------------------------------------------------------------------

fn unwrap_data(raw: &Value) -> &Value {
    match raw.get("data") {
        Some(d) if d.is_object() => d,
        _ => raw,
    }
}

/// (mediaID, imageURL) from a batchGenerateImages response.
/// Shape: {data: {media: [{name: "<uuid>", image: {generatedImage: {fifeUrl, imageUri}}}]}}
fn extract_result(raw: &Value) -> (String, String) {
    let data = unwrap_data(raw);
    let first = match data
        .get("media")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
    {
        Some(f) => f,
        None => return (String::new(), String::new()),
    };
    let mut media_id = String::new();
    if let Some(name) = first.get("name").and_then(|v| v.as_str()) {
        if is_uuid(name) {
            media_id = name.to_string();
        }
    }
    let mut image_url = String::new();
    if let Some(gen) = first.get("image").and_then(|i| i.get("generatedImage")) {
        if media_id.is_empty() {
            if let Some(v) = gen.get("mediaId").and_then(|v| v.as_str()) {
                if !v.is_empty() {
                    media_id = uuid_from_str(v);
                }
            }
        }
        for k in ["fifeUrl", "imageUri"] {
            if let Some(u) = gen.get(k).and_then(|v| v.as_str()) {
                if !u.is_empty() {
                    image_url = u.to_string();
                    if media_id.is_empty() {
                        media_id = uuid_from_str(u);
                    }
                    break;
                }
            }
        }
    }
    (media_id, image_url)
}

/// Non-empty when the bridge/API response carries an error.
fn extract_flow_error(raw: &Value) -> String {
    if raw.is_null() {
        return "nil response".to_string();
    }
    if let Some(e) = raw.get("error").and_then(|v| v.as_str()) {
        if !e.trim().is_empty() {
            return e.to_string();
        }
    }
    // Google's structured error: {error:{code,message,status,details:[{reason}]}}.
    // It sits under `data` (the extension's response wrapper) or at the root.
    for base in [raw.get("data"), Some(raw)].into_iter().flatten() {
        if let Some(err) = base.get("error").filter(|e| e.is_object()) {
            let msg = err.get("message").and_then(|v| v.as_str()).unwrap_or("");
            let reason = err
                .get("details")
                .and_then(|d| d.as_array())
                .and_then(|a| {
                    a.iter()
                        .find_map(|d| d.get("reason").and_then(|v| v.as_str()))
                })
                .unwrap_or("");
            let joined = match (msg.is_empty(), reason.is_empty()) {
                (false, false) => format!("{msg} ({reason})"),
                (false, true) => msg.to_string(),
                (true, false) => reason.to_string(),
                (true, true) => String::new(),
            };
            if !joined.is_empty() {
                return joined;
            }
        }
    }
    if let Some(st) = raw.get("status").and_then(|v| v.as_f64()) {
        if st >= 400.0 {
            if let Some(e) = raw
                .get("data")
                .and_then(|d| d.get("error"))
                .and_then(|v| v.as_str())
            {
                if !e.is_empty() {
                    return e.to_string();
                }
            }
            return format!("HTTP {}", st as i64);
        }
    }
    if let Some(e) = raw
        .get("data")
        .and_then(|d| d.get("error"))
        .and_then(|v| v.as_str())
    {
        if !e.trim().is_empty() {
            return e.to_string();
        }
    }
    String::new()
}

/// True when Flow refused the requested *model* for this account — the tier
/// can't access that Veo variant. Only this warrants a model step-down.
///
/// Must NOT match generic `PERMISSION_DENIED`: reCAPTCHA / unusual-activity
/// failures share that status, and retrying those with more rapid requests just
/// deepens the anti-bot flag. Key on the model-specific reason only.
fn is_model_access_denied(err: &str) -> bool {
    err.to_ascii_uppercase().contains("MODEL_ACCESS_DENIED")
}

/// True when Flow's server-side reCAPTCHA scored the request as bot/unusual.
/// Retrying immediately makes it worse — the caller must back off, not hammer.
fn is_recaptcha_failure(err: &str) -> bool {
    let e = err.to_ascii_uppercase();
    e.contains("RECAPTCHA") || e.contains("UNUSUAL_ACTIVITY")
}

/// Operations array from a video submit or poll response.
/// Pull the async-generation operations out of a submit/poll response.
///
/// The Go original only looked at `data.operations`. Google has shipped the
/// batch response under other envelopes (and the extension may add its own
/// wrapper), which surfaced as "no operations in video submit response" even
/// though the HTTP call returned 200 — so search a few known keys at any depth
/// and accept a single operation object as a one-element batch.
fn extract_video_ops(raw: &Value) -> (Vec<Value>, VideoEnvelope) {
    fn walk(v: &Value, depth: usize, out: &mut Vec<Value>, env: &mut VideoEnvelope) {
        if depth > 6 || !out.is_empty() {
            return;
        }
        let Some(obj) = v.as_object() else {
            if let Some(arr) = v.as_array() {
                for item in arr {
                    walk(item, depth + 1, out, env);
                }
            }
            return;
        };
        // Prefer the `media[]` envelope when present: its entries carry the
        // canonical `mediaMetadata.mediaStatus.mediaGenerationStatus` (so polling
        // sees SCHEDULED→SUCCESSFUL) and, on completion, `video.generatedVideo`
        // with the URL. The live submit returns BOTH `workflows[]` and `media[]`;
        // `media[]` is the one that yields a URL without scraping.
        if let Some(arr) = obj.get("media").and_then(|x| x.as_array()) {
            let items: Vec<Value> = arr
                .iter()
                .filter(|x| {
                    x.is_object() && (x.get("video").is_some() || x.get("mediaMetadata").is_some())
                })
                .cloned()
                .collect();
            if !items.is_empty() {
                *out = items;
                *env = VideoEnvelope::Media;
                return;
            }
        }
        // Otherwise the operation/workflow envelope. `workflows` is the current
        // key (`{workflows:[{name, metadata:{primaryMediaId}}]}`); its entries are
        // operation-shaped, so treat them like `operations`.
        for key in [
            "operations",
            "operationList",
            "videoOperations",
            "workflows",
        ] {
            if let Some(arr) = obj.get(key).and_then(|x| x.as_array()) {
                let ops: Vec<Value> = arr.iter().filter(|x| x.is_object()).cloned().collect();
                if !ops.is_empty() {
                    *out = ops;
                    *env = VideoEnvelope::Operations;
                    return;
                }
            }
        }
        // A lone operation object stands in for a one-element batch.
        if obj.get("operation").map(|o| o.is_object()).unwrap_or(false) {
            *out = vec![v.clone()];
            *env = VideoEnvelope::Operations;
            return;
        }
        for (_, child) in obj {
            if child.is_object() || child.is_array() {
                walk(child, depth + 1, out, env);
                if !out.is_empty() {
                    return;
                }
            }
        }
    }
    let mut out = Vec::new();
    let mut env = VideoEnvelope::Operations;
    walk(raw, 0, &mut out, &mut env);
    (out, env)
}

/// (mediaID, fifeURL) from a successful operation entry. Media IDs are forced
/// to UUID form (base64 CAMS ids are rejected, fifeUrl is the fallback source).
/// Remember the Flow project id so the URL scraper can open the project page.
///
/// Flow may echo its project id anywhere in the generation response
/// (`data.projectId`, per-entry, …), so scan the whole payload. When it echoes
/// nothing, fall back to the id we sent as `clientContext.projectId` — Flow
/// renders into that project, so it is a valid handle for the scrape.
fn remember_flow_project(core: &Core, project_id: &str, raw: &Value) {
    fn find_project_id(v: &Value, depth: usize) -> Option<String> {
        if depth > 6 {
            return None;
        }
        match v {
            Value::Object(o) => {
                if let Some(s) = o.get("projectId").and_then(|x| x.as_str()) {
                    if is_uuid(s) {
                        return Some(s.to_string());
                    }
                }
                o.values().find_map(|c| find_project_id(c, depth + 1))
            }
            Value::Array(a) => a.iter().find_map(|c| find_project_id(c, depth + 1)),
            _ => None,
        }
    }
    // Response id first; else the real session project we generated into; else
    // the app's own id.
    let flow_pid =
        find_project_id(raw, 0).unwrap_or_else(|| effective_flow_project(core, project_id));
    if flow_pid.is_empty() {
        return;
    }
    let key = format!("flow.project:{project_id}");
    if core.db.kv_get(&key) != flow_pid {
        let _ = core.db.kv_set(&key, &flow_pid);
        println!("[SceneVideo] flow project for {project_id} = {flow_pid}");
    }
}

/// Ask the extension to load the Flow project page so its tRPC calls run and
/// the URL scraper fires.
///
/// Flow stopped returning video URLs from the generation API, so a rendered
/// clip has an id and no link. This is the only automatic way to get one; it
/// costs a background tab for a few seconds.
pub async fn recover_media_urls(core: &Core, project_id: &str) -> Result<(), String> {
    let flow_pid = core.db.kv_get(&format!("flow.project:{project_id}"));
    if flow_pid.is_empty() {
        return Err("chưa biết Flow project id (cần sinh ít nhất 1 ảnh/video trước)".to_string());
    }
    if !core.ext.is_connected() {
        return Err("Chrome extension chưa kết nối".to_string());
    }
    println!("[media] asking the extension to load Flow project {flow_pid} for URLs");
    core.ext
        .call(
            "open_flow_project",
            json!({ "projectId": flow_pid, "dwellMs": 9000 }),
            Duration::from_secs(60),
        )
        .await
        .map(|_| ())
}

/// First playable media URL anywhere inside `v`.
///
/// Prefers the signed GCS bucket Flow serves renders from, then any http(s)
/// string that looks like a media file, so a schema move doesn't cost us the
/// URL of a clip that actually rendered.
fn find_media_url(v: &Value) -> String {
    fn looks_like_media(u: &str) -> bool {
        if !u.starts_with("http") {
            return false;
        }
        u.contains("storage.googleapis.com")
            || u.contains("/video/")
            || u.contains("/image/")
            || u.contains(".mp4")
            || u.contains("lh3.googleusercontent.com")
    }
    fn walk(v: &Value, depth: usize, best: &mut String) {
        if depth > 8 || best.contains("storage.googleapis.com") {
            return;
        }
        match v {
            Value::String(s) => {
                if looks_like_media(s) {
                    // A GCS signed URL wins outright; otherwise keep the first hit.
                    if best.is_empty() || s.contains("storage.googleapis.com") {
                        *best = s.clone();
                    }
                }
            }
            Value::Array(a) => {
                for item in a {
                    walk(item, depth + 1, best);
                }
            }
            Value::Object(o) => {
                for (_, child) in o {
                    walk(child, depth + 1, best);
                }
            }
            _ => {}
        }
    }
    let mut best = String::new();
    walk(v, 0, &mut best);
    best
}

/// Decoded MP4 bytes from an inline base64 string anywhere in `v`.
///
/// Veo 3.1 Lite (Low Priority) returns the clip embedded as base64 in a
/// `workflows` envelope rather than as an operations URL, so a normal URL scan
/// finds nothing. A string qualifies only if it decodes to real MP4 (an `ftyp`
/// box at offset 4), which keeps this from firing on unrelated base64 blobs.
fn find_inline_video_b64(v: &Value) -> Option<Vec<u8>> {
    use base64::Engine as _;
    fn decode_mp4(s: &str) -> Option<Vec<u8>> {
        // Cheap rejects before the expensive decode.
        if s.len() < 500 {
            return None;
        }
        // Check the first 64 BYTES look like base64 — byte-level, never a `&s[..64]`
        // str slice, which panics when byte 64 lands inside a multibyte UTF-8 char
        // (arbitrary Vietnamese strings from the response reach this scan).
        if !s
            .as_bytes()
            .iter()
            .take(64)
            .all(|&b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'=')
        {
            return None;
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(s.trim())
            .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(s.trim()))
            .ok()?;
        // MP4/ISO-BMFF: `....ftyp` — the `ftyp` box tag sits at bytes 4..8.
        if bytes.len() > 12 && &bytes[4..8] == b"ftyp" {
            Some(bytes)
        } else {
            None
        }
    }
    fn walk(v: &Value, depth: usize) -> Option<Vec<u8>> {
        if depth > 8 {
            return None;
        }
        match v {
            Value::String(s) => decode_mp4(s),
            Value::Array(a) => a.iter().find_map(|i| walk(i, depth + 1)),
            Value::Object(o) => o.values().find_map(|c| walk(c, depth + 1)),
            _ => None,
        }
    }
    walk(v, 0)
}

/// Store an inline base64 MP4 found in `raw` and return `(media_id, local_url)`,
/// or `None` when there is no inline clip. Shared by the submit and poll paths
/// so a Low Priority render is caught wherever Flow emits it.
async fn inline_video_from_raw(core: &Core, raw: &Value) -> Option<(String, String)> {
    let bytes = find_inline_video_b64(raw)?;
    match crate::mediastore::store_bytes(core, &bytes, "video", ".mp4").await {
        Ok(url) => Some((db::new_id(), url)),
        Err(e) => {
            eprintln!("[SceneVideo] inline MP4 found but store failed: {e}");
            None
        }
    }
}

/// Which envelope Google answered a video submit/poll with.
///
/// The legacy `operations[]` shape is what the Go backend was written against.
/// Flow has since moved video onto the same `media[]` envelope images use —
/// entries carry `name` (the media UUID), `mediaMetadata.mediaStatus
/// .mediaGenerationStatus` and, once rendered, `video.generatedVideo.fifeUrl`.
#[derive(Debug, Clone, Copy, PartialEq)]
enum VideoEnvelope {
    Operations,
    Media,
}

impl VideoEnvelope {
    fn body_key(self) -> &'static str {
        match self {
            VideoEnvelope::Operations => "operations",
            VideoEnvelope::Media => "media",
        }
    }
}

/// Generation status of one entry, in either envelope.
fn video_entry_status(entry: &Value, env: VideoEnvelope) -> String {
    match env {
        VideoEnvelope::Operations => {
            // Status lives at `status`, or nested `operation.metadata.status`, or
            // (newer flat op) `metadata.status`. A freshly submitted op has none
            // → treated as not-ready so the caller keeps polling.
            for path in [
                entry.get("status"),
                entry
                    .get("operation")
                    .and_then(|o| o.get("metadata"))
                    .and_then(|m| m.get("status")),
                entry.get("metadata").and_then(|m| m.get("status")),
                entry
                    .get("operation")
                    .and_then(|o| o.get("done"))
                    .map(|_| entry), // marker
            ] {
                if let Some(s) = path.and_then(|v| v.as_str()) {
                    if !s.is_empty() {
                        return s.to_string();
                    }
                }
            }
            // `operation.done: true` with no explicit status ⇒ success.
            if entry
                .get("operation")
                .and_then(|o| o.get("done"))
                .and_then(|v| v.as_bool())
                == Some(true)
            {
                return MEDIA_GEN_SUCCESS.to_string();
            }
            String::new()
        }
        VideoEnvelope::Media => entry
            .get("mediaMetadata")
            .and_then(|m| m.get("mediaStatus"))
            .and_then(|s| s.get("mediaGenerationStatus"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
    }
}

/// Identifier used to report a failure and to poll the entry again.
fn video_entry_name(entry: &Value, env: VideoEnvelope) -> String {
    match env {
        // Nested `operation.name` (legacy) or a flat top-level `name` (current
        // batchAsyncGenerateVideo op). The name is the handle the check endpoint
        // polls, so missing it means the render can never be tracked.
        VideoEnvelope::Operations => entry
            .get("operation")
            .and_then(|o| o.get("name"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .or_else(|| entry.get("name").and_then(|v| v.as_str()))
            .unwrap_or("")
            .to_string(),
        VideoEnvelope::Media => entry
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
    }
}

/// `(media_id, url)` of a finished entry — empty media_id means "not ready".
fn video_meta_from_entry(entry: &Value, env: VideoEnvelope) -> (String, String) {
    // The metadata block that carries the render / media id, across both the
    // nested (`operation.metadata`) and flat (`metadata`) operation shapes.
    let op_meta = match env {
        VideoEnvelope::Operations => entry
            .get("operation")
            .and_then(|o| o.get("metadata"))
            .or_else(|| entry.get("metadata")),
        VideoEnvelope::Media => None,
    };
    let vid = match env {
        VideoEnvelope::Operations => op_meta.and_then(|m| m.get("video")),
        // `video.generatedVideo` holds the render; the media UUID is `name`.
        VideoEnvelope::Media => entry.get("video").and_then(|v| v.get("generatedVideo")),
    };
    // `metadata.primaryMediaId` is the eventual media id even before the render's
    // own `video` block exists — capture it so a fresh op is trackable.
    let primary_media = op_meta
        .and_then(|m| m.get("primaryMediaId"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let Some(vid) = vid else {
        // No render block yet. Only hand back the primaryMediaId once the op has
        // actually SUCCEEDED — otherwise report "not ready" so the caller keeps
        // polling instead of scraping a clip Google hasn't finished rendering.
        let done = video_entry_status(entry, env) == MEDIA_GEN_SUCCESS;
        return if done && is_uuid(&primary_media) {
            (primary_media, String::new())
        } else {
            (String::new(), String::new())
        };
    };
    let raw_mid = vid
        .get("mediaId")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(primary_media.as_str());
    let mut fife = vid
        .get("fifeUrl")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if fife.is_empty() {
        for k in ["videoUri", "url", "servingUrl"] {
            if let Some(u) = vid.get(k).and_then(|v| v.as_str()) {
                if !u.is_empty() {
                    fife = u.to_string();
                    break;
                }
            }
        }
    }
    // Last resort: scan the whole entry for a media URL. Flow keeps moving where
    // the rendered URL sits, and a COMPLETED clip with no URL is unwatchable —
    // this mirrors how the extension itself scrapes URLs out of responses.
    if fife.is_empty() {
        fife = find_media_url(entry);
    }
    let mut media_id = if is_uuid(raw_mid) {
        raw_mid.to_string()
    } else {
        uuid_from_str(raw_mid)
    };
    if media_id.is_empty() {
        // In the media envelope the entry's own `name` IS the media UUID.
        let name = video_entry_name(entry, env);
        if is_uuid(&name) {
            media_id = name;
        }
    }
    if media_id.is_empty() && !fife.is_empty() {
        media_id = uuid_from_str(&fife);
    }
    // A scheduled/pending entry has no render yet — report "not ready" so the
    // caller keeps polling instead of storing an id with no video behind it.
    let status = video_entry_status(entry, env);
    if fife.is_empty() && status != MEDIA_GEN_SUCCESS {
        return (String::new(), String::new());
    }
    (media_id, fife)
}

/// Poll batchCheckAsync until every op succeeds, one fails, or the deadline hits.
async fn poll_video_ops(
    core: &Core,
    check_url: &str,
    ops: Vec<Value>,
    envelope: VideoEnvelope,
) -> Result<(String, String), String> {
    // Already rendered straight from the submit response?
    if let Some(first) = ops.first() {
        let (mid, u) = video_meta_from_entry(first, envelope);
        if !mid.is_empty() {
            return Ok((mid, u));
        }
    }

    let interval = Duration::from_secs(config::video_poll_secs().max(1));
    let timeout_secs = config::video_poll_timeout_secs();
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    // FAILED → Some(Err), all-SUCCESS with a media id → Some(Ok), else None.
    // Evaluated both at the loop top (previous response) AND immediately after a
    // fresh poll, so a SUCCESS arriving just before the deadline isn't discarded
    // by the next sleep+deadline gate as a false "timed out".
    let evaluate =
        |entries: &[Value], env: VideoEnvelope| -> Option<Result<(String, String), String>> {
            for entry in entries {
                if video_entry_status(entry, env) == MEDIA_GEN_FAILED {
                    return Some(Err(format!(
                        "video generation failed: {}",
                        video_entry_name(entry, env)
                    )));
                }
            }
            if !entries.is_empty()
                && entries
                    .iter()
                    .all(|e| video_entry_status(e, env) == MEDIA_GEN_SUCCESS)
            {
                let (mid, u) = entries
                    .first()
                    .map(|e| video_meta_from_entry(e, env))
                    .unwrap_or_default();
                if !mid.is_empty() {
                    if u.is_empty() {
                        eprintln!(
                            "[SceneVideo] SUCCESSFUL but no URL found; entry = {}",
                            serde_json::to_string(&entries[0]).unwrap_or_default()
                        );
                    }
                    return Some(Ok((mid, u)));
                }
            }
            None
        };

    let mut current = ops;
    let mut env = envelope;
    loop {
        tokio::time::sleep(interval).await;
        if Instant::now() >= deadline {
            return Err(format!("video poll timed out after {timeout_secs}s"));
        }

        if let Some(done) = evaluate(&current, env) {
            return done;
        }

        match api_request(core, build_video_poll_params(check_url, &current, env)).await {
            Err(e) => {
                eprintln!("[SceneVideo] poll error: {e}");
                continue;
            }
            Ok(raw) => {
                let flow_err = extract_flow_error(&raw);
                if !flow_err.is_empty() {
                    return Err(format!("poll error: {flow_err}"));
                }
                let (updated, updated_env) = extract_video_ops(&raw);
                if !updated.is_empty() {
                    current = updated;
                    env = updated_env;
                    // Check the fresh response NOW, before looping back into the
                    // sleep + deadline check that would otherwise drop a late SUCCESS.
                    if let Some(done) = evaluate(&current, env) {
                        return done;
                    }
                } else if let Some(pair) = inline_video_from_raw(core, &raw).await {
                    // A poll with no operations but an inline MP4 = Low Priority
                    // (Lite) render finished; hand back the saved local clip.
                    return Ok(pair);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// per-scene+orientation submit lock (port of scene_lock.go)
// ---------------------------------------------------------------------------

/// Long-lived per-(scene, orientation) async mutex; entries are tiny and
/// bounded by the number of distinct scenes.
fn scene_video_lock(scene_id: &str, orientation: &str) -> Arc<tokio::sync::Mutex<()>> {
    static LOCKS: OnceLock<StdMutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> =
        OnceLock::new();
    let key = format!("{scene_id}|{}", orientation.trim().to_uppercase());
    LOCKS
        .get_or_init(|| StdMutex::new(HashMap::new()))
        .lock()
        .unwrap()
        .entry(key)
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

// ---------------------------------------------------------------------------
// prompt assembly + reference matching
// ---------------------------------------------------------------------------

/// Structured Veo3 prompt from scene fields:
/// [camera] — [scene/character/action] — [action sequence] — [dialogue] — [atmosphere].
fn build_video_prompt(sc: &Row) -> String {
    let mut base = db::str_of(sc, "video_prompt");
    if base.is_empty() {
        base = db::str_of(sc, "prompt");
    }
    let base_lower = base.to_lowercase();

    let mut sections: Vec<String> = Vec::new();

    let mut cam_parts: Vec<String> = Vec::new();
    let shot = db::str_of(sc, "shot_type");
    if !shot.is_empty() && !base_lower.contains(&shot.to_lowercase()) {
        cam_parts.push(shot);
    }
    let cam = db::str_of(sc, "camera_movement");
    if !cam.is_empty() && !base_lower.contains(&cam.to_lowercase()) {
        cam_parts.push(cam);
    }
    if !cam_parts.is_empty() {
        sections.push(cam_parts.join(" "));
    }

    if !base.is_empty() {
        sections.push(base.trim().trim_end_matches('.').to_string());
    }

    let seq = db::str_of(sc, "action_sequence");
    if !seq.is_empty() {
        let prefix = byte_prefix(&seq, 30).to_lowercase();
        if !base_lower.contains(&prefix) {
            sections.push(seq.trim().trim_end_matches('.').to_string());
        }
    }

    let narrator = db::str_of(sc, "narrator_text");
    if !narrator.is_empty() && !base_lower.contains("speaks:") {
        let d = format_dialogue(&narrator);
        if !d.is_empty() {
            sections.push(d);
        }
    }

    let env_json = db::str_of(sc, "scene_environment");
    if !env_json.is_empty() {
        if let Ok(env) = serde_json::from_str::<Value>(&env_json) {
            let has_light = base_lower.contains("light")
                || base_lower.contains("k ")
                || base_lower.contains("sun");
            let has_color = base_lower.contains("color")
                || base_lower.contains("saturat")
                || base_lower.contains("tone");
            if let Some(v) = env.get("lighting_setup").and_then(|v| v.as_str()) {
                if !v.is_empty() && !has_light {
                    sections.push(v.to_string());
                }
            }
            if let Some(v) = env.get("color_grading").and_then(|v| v.as_str()) {
                if !v.is_empty() && !has_color {
                    sections.push(v.to_string());
                }
            }
        }
    }

    sections.join(". ")
}

/// `"NAME: text"` → `NAME speaks: "text"`; speakers joined with `; then`.
fn format_dialogue(narrator: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for line in narrator.split('\n') {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(idx) = line.find(':') {
            if idx > 0 {
                let name = line[..idx].trim();
                let text = line[idx + 1..].trim();
                if !name.is_empty() && !text.is_empty() {
                    out.push(format!("{name} speaks: \"{text}\""));
                    continue;
                }
            }
        }
        out.push(line.to_string());
    }
    out.join("; then ")
}

fn list_project_characters(core: &Core, project_id: &str) -> anyhow::Result<Vec<Row>> {
    core.db.query(
        "SELECT c.* FROM character c \
         JOIN project_character pc ON pc.character_id = c.id \
         WHERE pc.project_id = ?1 \
         ORDER BY c.name",
        &[&project_id],
    )
}

/// media_ids of project entities relevant to the scene, used as reference
/// images. First pass: names listed in `character_names`; fallback for
/// locations: name appears in scene text. Only entities with a media_id count.
fn scene_ref_media_ids(core: &Core, project_id: &str, sc: &Row) -> Vec<String> {
    let chars = list_project_characters(core, project_id).unwrap_or_default();
    if chars.is_empty() {
        return Vec::new();
    }

    let raw = db::str_of(sc, "character_names");
    let mut char_names: Vec<String> = Vec::new();
    if !raw.is_empty() {
        match serde_json::from_str::<Vec<String>>(&raw) {
            Ok(v) => char_names = v,
            Err(_) => {
                for s in raw.split(',') {
                    let t = s.trim();
                    if !t.is_empty() {
                        char_names.push(t.to_string());
                    }
                }
            }
        }
    }
    let name_set: HashSet<String> = char_names
        .iter()
        .map(|n| canonical_entity_key(n))
        .filter(|k| !k.is_empty())
        .collect();

    // Word-boundary phrase (padded with spaces) for whole-word location matching.
    let scene_phrase = canonical_phrase(
        &[
            db::str_of(sc, "prompt"),
            db::str_of(sc, "video_prompt"),
            db::str_of(sc, "action_sequence"),
            db::str_of(sc, "narrator_text"),
        ]
        .join(" "),
    );

    let mut out = Vec::new();
    for ch in &chars {
        let name = db::str_of(ch, "name").to_lowercase();
        let name_key = canonical_entity_key(&name);
        if name.is_empty() || name_key.is_empty() {
            continue;
        }
        let entity_type = db::str_of(ch, "entity_type").to_lowercase();
        let mid = db::str_of(ch, "media_id");
        if mid.is_empty() {
            continue;
        }
        let mut matched = name_set.contains(&name_key);
        if !matched && entity_type == "location" {
            // Whole-word match, not raw substring: " ao " won't hit inside "vao".
            let name_phrase = canonical_phrase(&name);
            let needle = name_phrase.trim();
            matched = !needle.is_empty() && scene_phrase.contains(&format!(" {needle} "));
        }
        if matched {
            out.push(mid);
        }
    }
    out
}

/// A compact appearance reminder for the characters a scene names, e.g.
/// "NAM — 35yo, short black hair, blue shirt; BÀ CHỦ — 60yo, grey hair". Woven
/// into the scene prompt so a character's invariant look is reinforced in TEXT,
/// not only via the reference image — the two together hold consistency.
fn scene_ref_appearance(core: &Core, project_id: &str, sc: &Row) -> String {
    let chars = list_project_characters(core, project_id).unwrap_or_default();
    if chars.is_empty() {
        return String::new();
    }
    let raw = db::str_of(sc, "character_names");
    let mut char_names: Vec<String> = Vec::new();
    if !raw.is_empty() {
        match serde_json::from_str::<Vec<String>>(&raw) {
            Ok(v) => char_names = v,
            Err(_) => {
                for s in raw.split(',') {
                    let t = s.trim();
                    if !t.is_empty() {
                        char_names.push(t.to_string());
                    }
                }
            }
        }
    }
    if char_names.is_empty() {
        return String::new();
    }
    let name_set: HashSet<String> = char_names
        .iter()
        .map(|n| canonical_entity_key(n))
        .filter(|k| !k.is_empty())
        .collect();

    let mut parts = Vec::new();
    for ch in &chars {
        let name = db::str_of(ch, "name");
        let key = canonical_entity_key(&name);
        if key.is_empty() || !name_set.contains(&key) {
            continue;
        }
        // Prefer the compact appearance tags; fall back to the description.
        let mut tags = db::str_of(ch, "appearance_tags");
        if tags.trim().is_empty() {
            tags = db::str_of(ch, "description");
        }
        let tags = tags.trim();
        if tags.is_empty() {
            continue;
        }
        parts.push(format!(
            "{} — {}",
            name.trim(),
            crate::llm::truncate(tags, 220)
        ));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(
            "Character appearance (keep identical to reference): {}",
            parts.join("; ")
        )
    }
}

/// Fold one lowercased char: Vietnamese diacritics → base latin, else unchanged.
fn fold_vi(r: char) -> char {
    match r {
        'à' | 'á' | 'ả' | 'ã' | 'ạ' | 'ă' | 'ằ' | 'ắ' | 'ẳ' | 'ẵ' | 'ặ' | 'â' | 'ầ' | 'ấ' | 'ẩ'
        | 'ẫ' | 'ậ' => 'a',
        'è' | 'é' | 'ẻ' | 'ẽ' | 'ẹ' | 'ê' | 'ề' | 'ế' | 'ể' | 'ễ' | 'ệ' => 'e',
        'ì' | 'í' | 'ỉ' | 'ĩ' | 'ị' => 'i',
        'ò' | 'ó' | 'ỏ' | 'õ' | 'ọ' | 'ô' | 'ồ' | 'ố' | 'ổ' | 'ỗ' | 'ộ' | 'ơ' | 'ờ' | 'ớ' | 'ở'
        | 'ỡ' | 'ợ' => 'o',
        'ù' | 'ú' | 'ủ' | 'ũ' | 'ụ' | 'ư' | 'ừ' | 'ứ' | 'ử' | 'ữ' | 'ự' => 'u',
        'ỳ' | 'ý' | 'ỷ' | 'ỹ' | 'ỵ' => 'y',
        'đ' => 'd',
        other => other,
    }
}

/// Lowercase, fold Vietnamese diacritics, keep alphanumerics only (no spaces).
fn canonical_entity_key(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for r in s.trim().to_lowercase().chars() {
        let r = fold_vi(r);
        if r.is_alphanumeric() {
            out.push(r);
        }
    }
    out
}

/// Like `canonical_entity_key` but KEEPS word boundaries: every non-alphanumeric
/// run becomes a single space. Used for whole-word matching so a short name
/// ("Ao"/"Na") doesn't match inside a longer word ("vào"→"vao", "nào"→"nao").
/// The result is padded with a leading+trailing space so callers can test
/// `.contains(" name ")` as a bounded whole-word match.
fn canonical_phrase(s: &str) -> String {
    let mut out = String::from(" ");
    let mut last_space = true;
    for r in s.trim().to_lowercase().chars() {
        let f = fold_vi(r);
        if f.is_alphanumeric() {
            out.push(f);
            last_space = false;
        } else if !last_space {
            out.push(' ');
            last_space = true;
        }
    }
    if !out.ends_with(' ') {
        out.push(' ');
    }
    out
}

/// Longest prefix of `s` that fits in `n` bytes without splitting a char
/// (Go sliced bytes; a naive `&s[..n]` panics on multibyte text).
fn byte_prefix(s: &str, n: usize) -> &str {
    let mut n = n.min(s.len());
    while n > 0 && !s.is_char_boundary(n) {
        n -= 1;
    }
    &s[..n]
}

// ---------------------------------------------------------------------------
// UUID helpers (Go used a lowercase-hex regex; no regex dep here)
// ---------------------------------------------------------------------------

fn is_uuid_bytes(b: &[u8]) -> bool {
    if b.len() != 36 {
        return false;
    }
    for (i, &c) in b.iter().enumerate() {
        match i {
            8 | 13 | 18 | 23 => {
                if c != b'-' {
                    return false;
                }
            }
            _ => {
                if !(c.is_ascii_digit() || (b'a'..=b'f').contains(&c)) {
                    return false;
                }
            }
        }
    }
    true
}

pub(crate) fn is_uuid(s: &str) -> bool {
    is_uuid_bytes(s.as_bytes())
}

/// First lowercase-hex UUID substring of `s`, or "".
fn uuid_from_str(s: &str) -> String {
    let b = s.as_bytes();
    if b.len() < 36 {
        return String::new();
    }
    for w in b.windows(36) {
        if is_uuid_bytes(w) {
            // All-ASCII match, safe to rebuild as a string.
            return String::from_utf8_lossy(w).to_string();
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Google has shipped video submit under two envelopes; both must parse.
    #[test]
    fn extract_video_ops_handles_envelopes() {
        let op = serde_json::json!({"operation": {"name": "op/1"}, "sceneId": "s1"});

        // Legacy: data.operations
        let a = serde_json::json!({"data": {"operations": [op]}});
        let (ops, env) = extract_video_ops(&a);
        assert_eq!(ops.len(), 1);
        assert_eq!(env, VideoEnvelope::Operations);

        // Bare operations at the root
        let (ops, _) = extract_video_ops(&serde_json::json!({"operations": [op]}));
        assert_eq!(ops.len(), 1);

        // Double-wrapped by the extension
        let (ops, _) =
            extract_video_ops(&serde_json::json!({"data": {"data": {"operations": [op]}}}));
        assert_eq!(ops.len(), 1);

        // A single operation object, not a batch
        let (ops, _) =
            extract_video_ops(&serde_json::json!({"data": {"operation": {"name": "op/1"}}}));
        assert_eq!(ops.len(), 1);

        // Genuinely empty stays empty
        assert!(extract_video_ops(&serde_json::json!({"data": {}}))
            .0
            .is_empty());
        assert!(
            extract_video_ops(&serde_json::json!({"data": {"operations": []}}))
                .0
                .is_empty()
        );
    }

    /// The flat operation shape observed live on batchAsyncGenerateVideo:
    /// `{name, metadata:{primaryMediaId,...}}` with no nested `operation` key and
    /// no status on the fresh submit. The name must be pollable, and the media id
    /// must only surface once the op has SUCCEEDED — never on the fresh submit,
    /// or the render gets scraped before Google finishes it.
    #[test]
    fn flat_operation_with_primary_media_id() {
        let fresh = serde_json::json!({
            "name": "663bb2cb-72de-4130-bb28-f042469b2892",
            "metadata": {
                "displayName": "Man rejecting phone call shoreline",
                "createTime": "2026-07-20T01:46:20.091028Z",
                "primaryMediaId": "206d1a5d-4455-42f7-b0e2-36e32cc78abc"
            }
        });
        let (ops, env) =
            extract_video_ops(&serde_json::json!({ "data": { "operations": [fresh.clone()] } }));
        assert_eq!(ops.len(), 1);
        assert_eq!(env, VideoEnvelope::Operations);

        // The live envelope is `workflows[]` (seen with remainingCredits): same
        // operation-shaped entries, must parse identically.
        let wf = serde_json::json!({ "data": { "remainingCredits": 800, "workflows": [fresh.clone()] } });
        let (ops, env) = extract_video_ops(&wf);
        assert_eq!(ops.len(), 1);
        assert_eq!(env, VideoEnvelope::Operations);
        assert_eq!(
            video_entry_name(&ops[0], env),
            "663bb2cb-72de-4130-bb28-f042469b2892"
        );
        // Name is the poll handle.
        assert_eq!(
            video_entry_name(&ops[0], env),
            "663bb2cb-72de-4130-bb28-f042469b2892"
        );
        // Fresh submit (no status) → not ready, keep polling.
        assert_eq!(
            video_meta_from_entry(&ops[0], env),
            (String::new(), String::new())
        );

        // Same op, now SUCCEEDED → primaryMediaId surfaces, still no URL (comes
        // from the project-page scrape).
        let mut done = fresh;
        done["status"] = serde_json::json!(MEDIA_GEN_SUCCESS);
        let (mid, url) = video_meta_from_entry(&done, VideoEnvelope::Operations);
        assert_eq!(mid, "206d1a5d-4455-42f7-b0e2-36e32cc78abc");
        assert!(url.is_empty());
    }

    /// The shape Flow actually returns today: `media[]` with a scheduled status
    /// and the media UUID in `name`. This is what used to read as "no
    /// operations in video submit response".
    #[test]
    fn extract_video_ops_reads_media_envelope() {
        let scheduled = serde_json::json!({"data": {"media": [{
            "name": "daa86d53-0043-4eb1-a163-f49e1a42c0ee",
            "sceneId": "54f142f1-8e65-4e62-b142-19bc912f36a4",
            "mediaMetadata": {"mediaStatus": {"mediaGenerationStatus": "MEDIA_GENERATION_STATUS_SCHEDULED"}},
            "video": {"generatedVideo": {"model": "veo_3_1_i2v_s_fast_portrait", "seed": 831}}
        }], "remainingCredits": 740}});

        let (entries, env) = extract_video_ops(&scheduled);
        assert_eq!(entries.len(), 1);
        assert_eq!(env, VideoEnvelope::Media);
        assert_eq!(
            video_entry_status(&entries[0], env),
            "MEDIA_GENERATION_STATUS_SCHEDULED"
        );
        assert_eq!(
            video_entry_name(&entries[0], env),
            "daa86d53-0043-4eb1-a163-f49e1a42c0ee"
        );
        // Still rendering ⇒ not ready, so the caller keeps polling.
        assert_eq!(
            video_meta_from_entry(&entries[0], env),
            (String::new(), String::new())
        );
        // The poll body must echo back under `media`, not `operations`.
        let params = build_video_poll_params("https://x/check", &entries, env);
        assert!(params["body"]["media"].is_array());
        assert!(params["body"]["operations"].is_null());
    }

    /// A finished clip must yield a URL even when Flow moves it — a COMPLETED
    /// scene with an empty video_url is unwatchable (exactly what happened).
    #[test]
    fn finds_media_url_anywhere_in_entry() {
        let url = "https://storage.googleapis.com/ai-sandbox-videofx/video/6e955f71-ec2b-4399-994c-b7b8fe19f838?x=1";
        // URL buried somewhere other than video.generatedVideo.fifeUrl
        let entry = serde_json::json!({
            "name": "6e955f71-ec2b-4399-994c-b7b8fe19f838",
            "mediaMetadata": {"mediaStatus": {"mediaGenerationStatus": "MEDIA_GENERATION_STATUS_SUCCESSFUL"}},
            "video": {"generatedVideo": {"model": "veo"}, "servingData": {"downloadUri": url}}
        });
        let (mid, got) = video_meta_from_entry(&entry, VideoEnvelope::Media);
        assert_eq!(mid, "6e955f71-ec2b-4399-994c-b7b8fe19f838");
        assert_eq!(got, url);

        // Prefer the signed GCS URL over an unrelated http string
        let mixed = serde_json::json!({"a": "https://example.com/doc.html", "b": {"c": url}});
        assert_eq!(find_media_url(&mixed), url);

        // Nothing media-ish ⇒ empty
        assert_eq!(
            find_media_url(&serde_json::json!({"a": "hello", "b": 3})),
            ""
        );
    }

    /// Once rendered, the media entry carries the URL and its `name` is the id.
    #[test]
    fn video_meta_from_media_entry_when_done() {
        let done = serde_json::json!({"data": {"media": [{
            "name": "daa86d53-0043-4eb1-a163-f49e1a42c0ee",
            "mediaMetadata": {"mediaStatus": {"mediaGenerationStatus": "MEDIA_GENERATION_STATUS_SUCCESSFUL"}},
            "video": {"generatedVideo": {"fifeUrl": "https://storage.googleapis.com/x/video/daa86d53-0043-4eb1-a163-f49e1a42c0ee?a=1"}}
        }]}});
        let (entries, env) = extract_video_ops(&done);
        let (mid, url) = video_meta_from_entry(&entries[0], env);
        assert_eq!(mid, "daa86d53-0043-4eb1-a163-f49e1a42c0ee");
        assert!(url.starts_with("https://storage.googleapis.com/"));
    }

    #[test]
    fn uuid_matching() {
        assert!(is_uuid("123e4567-e89b-12d3-a456-426614174000"));
        assert!(!is_uuid("123E4567-E89B-12D3-A456-426614174000")); // uppercase rejected like the Go regex
        assert!(!is_uuid("not-a-uuid"));
        assert_eq!(
            uuid_from_str(
                "https://lh3.googleusercontent.com/x/123e4567-e89b-12d3-a456-426614174000=s0"
            ),
            "123e4567-e89b-12d3-a456-426614174000"
        );
        assert_eq!(uuid_from_str("CAMSbase64idAAAA"), "");
        // Multibyte text must not panic the scanner.
        assert_eq!(
            uuid_from_str("ảnh không có uuid ở đây, chỉ có chữ tiếng Việt thôi nhé"),
            ""
        );
    }

    #[test]
    fn canonical_key_folds_vietnamese() {
        assert_eq!(canonical_entity_key("Hồ Ngọc Đức"), "hongocduc");
        assert_eq!(canonical_entity_key("  Quán Cà Phê  "), "quancaphe");
        assert_eq!(canonical_entity_key("R2-D2!"), "r2d2");
    }

    /// Word-boundary phrase matching: a short entity name must not match inside a
    /// longer word ("Ao" ⊄ "vào"→"vao"), but must match as a whole token.
    #[test]
    fn canonical_phrase_is_word_bounded() {
        let scene = canonical_phrase("Người đàn ông bước vào nhà"); // "…buoc vao nha"
        assert!(!scene.contains(" ao "), "short name leaked inside a word");
        assert!(scene.contains(" vao "), "real token missing");
        // A genuine location mention matches.
        let s2 = canonical_phrase("Chiếc thuyền cập bến Ao Sen");
        assert!(s2.contains(&format!(" {} ", canonical_phrase("Ao").trim())));
        // Multi-word entity matches only as a contiguous phrase.
        let s3 = canonical_phrase("Bà chủ quán bưng cháo ra");
        assert!(s3.contains(&format!(" {} ", canonical_phrase("Bà chủ quán").trim())));
    }

    /// find_inline_video_b64 must never panic on multibyte UTF-8 (Vietnamese
    /// strings ≥500 bytes with a char straddling byte 64 used to slice-panic).
    #[test]
    fn inline_scan_survives_multibyte() {
        let long_vi = "cô gái trẻ đứng bên bờ biển lúc hoàng hôn ".repeat(30); // >500 bytes, multibyte
        let raw = json!({ "workflows": [{ "prompt": long_vi }], "other": "ẻộếề".repeat(200) });
        // Must return None (no MP4) rather than panic.
        assert!(find_inline_video_b64(&raw).is_none());
    }

    #[test]
    fn model_key_matrix() {
        // TIER_ONE (PRO) → plain Fast keys it can access.
        assert_eq!(
            video_model_key("PAYGATE_TIER_ONE", "VIDEO_ASPECT_RATIO_PORTRAIT", false),
            "veo_3_1_i2v_s_fast_portrait"
        );
        assert_eq!(
            video_model_key("PAYGATE_TIER_ONE", "VIDEO_ASPECT_RATIO_LANDSCAPE", true),
            "veo_3_1_i2v_s_fast_fl"
        );
        // TIER_TWO / default → the `_ultra` family.
        assert_eq!(
            video_model_key("PAYGATE_TIER_TWO", "VIDEO_ASPECT_RATIO_PORTRAIT", false),
            "veo_3_1_i2v_s_fast_portrait_ultra"
        );
        assert_eq!(
            video_model_key("", "VIDEO_ASPECT_RATIO_LANDSCAPE", true),
            "veo_3_1_i2v_s_fast_ultra_fl"
        );
    }

    #[test]
    fn model_access_denied_detection() {
        assert!(is_model_access_denied(
            "permission denied (PUBLIC_ERROR_MODEL_ACCESS_DENIED)"
        ));
        // Generic PERMISSION_DENIED must NOT step down the model — a reCAPTCHA /
        // unusual-activity 403 shares that status, and retrying it hammers the
        // anti-bot flag. Only the model-specific reason counts.
        assert!(!is_model_access_denied(
            "The caller does not have permission (PERMISSION_DENIED)"
        ));
        assert!(!is_model_access_denied("HTTP 500"));

        // The model-denied 403 parses and matches the model retry.
        let model = json!({ "status": 403.0, "data": { "error": {
            "code": 403, "message": "The caller does not have permission",
            "status": "PERMISSION_DENIED",
            "details": [{ "reason": "PUBLIC_ERROR_MODEL_ACCESS_DENIED" }]
        }}});
        let em = extract_flow_error(&model);
        assert!(em.contains("PUBLIC_ERROR_MODEL_ACCESS_DENIED"), "got: {em}");
        assert!(is_model_access_denied(&em));
        assert!(!is_recaptcha_failure(&em));

        // The reCAPTCHA / unusual-activity 403 is recognised as its own class and
        // must NOT be treated as a model denial.
        let cap = json!({ "status": 403.0, "data": { "error": {
            "code": 403, "message": "reCAPTCHA evaluation failed",
            "status": "PERMISSION_DENIED",
            "details": [{ "reason": "PUBLIC_ERROR_UNUSUAL_ACTIVITY" }]
        }}});
        let ec = extract_flow_error(&cap);
        assert!(is_recaptcha_failure(&ec), "got: {ec}");
        assert!(!is_model_access_denied(&ec));
    }

    /// An explicit model key (learned from Flow, e.g. Veo 3.1 Lite) must be sent
    /// verbatim; an empty key falls back to the known-good matrix so generation
    /// never breaks just because nothing was learned yet.
    #[test]
    fn submit_honors_explicit_model_key() {
        let with = build_video_submit_params(
            "http://x",
            "p1",
            "s1",
            "prompt",
            "startmid",
            "",
            "VIDEO_ASPECT_RATIO_PORTRAIT",
            "PAYGATE_TIER_ONE",
            "veo_3_1_i2v_s_lite_portrait",
        );
        let sent = with["body"]["requests"][0]["videoModelKey"]
            .as_str()
            .unwrap();
        assert_eq!(sent, "veo_3_1_i2v_s_lite_portrait");

        let without = build_video_submit_params(
            "http://x",
            "p1",
            "s1",
            "prompt",
            "startmid",
            "",
            "VIDEO_ASPECT_RATIO_PORTRAIT",
            "PAYGATE_TIER_ONE",
            "",
        );
        let fallback = without["body"]["requests"][0]["videoModelKey"]
            .as_str()
            .unwrap();
        assert_eq!(fallback, "veo_3_1_i2v_s_fast_portrait");
    }

    #[test]
    fn inline_base64_mp4_detected() {
        use base64::Engine as _;
        // A 400-byte buffer shaped like an MP4: `ftyp` box tag at offset 4.
        let mut mp4 = vec![0u8; 400];
        mp4[4] = b'f';
        mp4[5] = b't';
        mp4[6] = b'y';
        mp4[7] = b'p';
        let b64 = base64::engine::general_purpose::STANDARD.encode(&mp4);
        let raw = json!({ "workflows": [{ "media": { "videoBytes": b64 } }] });
        let found = find_inline_video_b64(&raw).expect("should find inline mp4");
        assert_eq!(&found[4..8], b"ftyp");

        // A base64 blob that is not MP4 must be ignored.
        let junk = base64::engine::general_purpose::STANDARD.encode(&vec![1u8; 400]);
        assert!(find_inline_video_b64(&json!({ "x": junk })).is_none());
        // A signed URL response must not be mistaken for inline media.
        assert!(find_inline_video_b64(
            &json!({ "url": "https://storage.googleapis.com/b/video/x?s=1" })
        )
        .is_none());
    }

    #[test]
    fn dialogue_formatting() {
        assert_eq!(
            format_dialogue("AN: Xin chào\nBÌNH: Chào bạn"),
            "AN speaks: \"Xin chào\"; then BÌNH speaks: \"Chào bạn\""
        );
        assert_eq!(format_dialogue("no speaker line"), "no speaker line");
    }

    #[test]
    fn flow_error_shapes() {
        assert_eq!(extract_flow_error(&Value::Null), "nil response");
        assert_eq!(extract_flow_error(&json!({"error": "boom"})), "boom");
        assert_eq!(extract_flow_error(&json!({"status": 403.0})), "HTTP 403");
        assert_eq!(
            extract_flow_error(&json!({"status": 500, "data": {"error": "quota"}})),
            "quota"
        );
        assert_eq!(extract_flow_error(&json!({"data": {"media": []}})), "");
    }

    #[test]
    fn extract_result_prefers_uuid_name() {
        let raw = json!({"data": {"media": [{
            "name": "123e4567-e89b-12d3-a456-426614174000",
            "image": {"generatedImage": {"fifeUrl": "https://x/img"}}
        }]}});
        let (mid, url) = extract_result(&raw);
        assert_eq!(mid, "123e4567-e89b-12d3-a456-426614174000");
        assert_eq!(url, "https://x/img");

        // Base64 CAMS name is rejected; UUID recovered from fifeUrl.
        let raw = json!({"data": {"media": [{
            "name": "CAMSabc",
            "image": {"generatedImage": {"fifeUrl": "https://x/00000000-1111-2222-3333-444444444444=s0"}}
        }]}});
        let (mid, _) = extract_result(&raw);
        assert_eq!(mid, "00000000-1111-2222-3333-444444444444");
    }

    #[test]
    fn byte_prefix_is_char_safe() {
        let s = "ếếếếếếếếếếếếếếếế"; // 3 bytes per char
        let p = byte_prefix(s, 30);
        assert!(p.len() <= 30);
        assert!(s.starts_with(p));
        assert_eq!(byte_prefix("short", 30), "short");
    }
}
