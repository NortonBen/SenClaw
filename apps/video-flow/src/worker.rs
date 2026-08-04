//! Request-queue worker + extension event routing — port of `internal/worker`
//! (processor.go + events.go). Polls PENDING `request` rows and executes them
//! through `process::*`; routes extension-originated events (token_captured,
//! extension_ready, media_urls_refresh) to the DB/dashboard.

use crate::config;
use crate::db::{self, Row};
use crate::process;
use crate::state::Core;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

/// Spawn the poll-loop task. `main.rs` gates this on `config::worker_enabled()`.
/// Resolve requests left in PROCESSING by a previous run.
///
/// Nothing can still be in flight right after boot — the DAG task that owned
/// the request died with the process — so a PROCESSING row is always stale.
/// Self-heal it: if the asset it was generating actually exists, the work did
/// finish (only the status write was lost), so mark it COMPLETED; otherwise
/// mark it FAILED so the UI stops showing a spinner that will never resolve.
pub fn reconcile_stale_requests(core: &Arc<Core>) {
    let db = &core.db;
    let stale = match db.query("SELECT * FROM request WHERE status = 'PROCESSING'", &[]) {
        Ok(rows) => rows,
        Err(e) => {
            eprintln!("[worker] reconcile query failed: {e}");
            return;
        }
    };
    if stale.is_empty() {
        return;
    }
    let (mut completed, mut failed) = (0usize, 0usize);
    for req in &stale {
        let id = db::str_of(req, "id");
        let character_id = db::str_of(req, "character_id");
        let scene_id = db::str_of(req, "scene_id");
        let typ = db::str_of(req, "type");
        let orientation = db::str_of(req, "orientation");

        // What asset was this request producing, and does it exist now?
        let (media_id, url) = if !character_id.is_empty() {
            db.get("character", &character_id)
                .ok()
                .flatten()
                .map(|c| {
                    (
                        db::str_of(&c, "media_id"),
                        db::str_of(&c, "reference_image_url"),
                    )
                })
                .unwrap_or_default()
        } else if !scene_id.is_empty() {
            let cols = db::scene_cols(&orientation);
            let (id_col, url_col) = if typ.contains("UPSCALE") {
                (cols.upscale_media_id, cols.upscale_url)
            } else if typ.contains("VIDEO") {
                (cols.video_media_id, cols.video_url)
            } else {
                (cols.image_media_id, cols.image_url)
            };
            db.get("scene", &scene_id)
                .ok()
                .flatten()
                .map(|s| (db::str_of(&s, &id_col), db::str_of(&s, &url_col)))
                .unwrap_or_default()
        } else {
            (String::new(), String::new())
        };

        let mut up = Row::new();
        if !media_id.is_empty() || !url.is_empty() {
            up.insert("status".into(), json!("COMPLETED"));
            if !media_id.is_empty() {
                up.insert("media_id".into(), json!(media_id));
            }
            if !url.is_empty() {
                up.insert("output_url".into(), json!(url));
            }
            completed += 1;
        } else {
            up.insert("status".into(), json!("FAILED"));
            up.insert(
                "error_message".into(),
                json!("gián đoạn: app khởi động lại khi yêu cầu đang chạy"),
            );
            failed += 1;
        }
        if let Err(e) = db.update("request", &id, &up) {
            eprintln!("[worker] reconcile request {id}: {e}");
        }
    }
    println!(
        "[worker] reconciled {} stale PROCESSING request(s): {completed} completed, {failed} failed",
        stale.len()
    );
}

pub fn spawn(core: Arc<Core>) {
    println!("[worker] started (poll={}s)", config::worker_poll_secs());
    tokio::spawn(async move {
        let interval = Duration::from_secs(config::worker_poll_secs().max(1));
        loop {
            tokio::time::sleep(interval).await;
            if !core.ext.is_connected() {
                continue;
            }
            if let Err(e) = tick(&core).await {
                eprintln!("[worker] tick: {e}");
            }
        }
    });
}

/// One poll tick: pick the highest-priority PENDING request and run it.
/// Runs inline in the loop, so at most one request is in flight at a time
/// (the Go loop had the same property).
async fn tick(core: &Core) -> Result<(), String> {
    let rows = core
        .db
        .query(
            "SELECT * FROM request WHERE status = 'PENDING' ORDER BY created_at ASC",
            &[],
        )
        .map_err(|e| e.to_string())?;
    let req = match pick_job(&rows) {
        Some(r) => r,
        None => return Ok(()),
    };
    match db::str_of(req, "type").as_str() {
        "GENERATE_VIDEO" | "REGENERATE_VIDEO" => handle_scene_video(core, req).await,
        "UPSCALE_VIDEO" => handle_upscale(core, req).await,
        _ => Ok(()),
    }
}

/// Priority: GENERATE_VIDEO > REGENERATE_VIDEO > UPSCALE_VIDEO; FIFO within a type.
fn pick_job<'a>(rows: &'a [Row]) -> Option<&'a Row> {
    for typ in ["GENERATE_VIDEO", "REGENERATE_VIDEO", "UPSCALE_VIDEO"] {
        if let Some(r) = rows.iter().find(|r| db::str_of(r, "type") == typ) {
            return Some(r);
        }
    }
    None
}

// ---------- scene video ----------

async fn handle_scene_video(core: &Core, req: &Row) -> Result<(), String> {
    let rid = db::str_of(req, "id");
    let pid = db::str_of(req, "project_id");
    let sid = db::str_of(req, "scene_id");
    let mut orientation = db::str_of(req, "orientation");
    if orientation.is_empty() {
        orientation = "VERTICAL".to_string();
    }
    if rid.is_empty() || pid.is_empty() || sid.is_empty() {
        return Ok(());
    }

    set_request_status(core, &rid, "PROCESSING");
    let regenerate = db::str_of(req, "type") == "REGENERATE_VIDEO";
    match process::scene_video(core, &sid, &pid, &orientation, regenerate).await {
        Ok(out) => {
            complete_request(core, &rid, &pid, &out);
            Ok(())
        }
        Err(e) => fail_request(core, &rid, &e),
    }
}

// ---------- upscale ----------

async fn handle_upscale(core: &Core, req: &Row) -> Result<(), String> {
    let rid = db::str_of(req, "id");
    let pid = db::str_of(req, "project_id");
    let sid = db::str_of(req, "scene_id");
    if rid.is_empty() || sid.is_empty() {
        return Ok(());
    }
    let mut orientation = db::str_of(req, "orientation");
    if orientation.is_empty() {
        orientation = "VERTICAL".to_string();
    }

    set_request_status(core, &rid, "PROCESSING");
    match process::upscale_video(core, &sid, &pid, &orientation).await {
        Ok(out) => {
            complete_request(core, &rid, &pid, &out);
            Ok(())
        }
        Err(e) => fail_request(core, &rid, &e),
    }
}

// ---------- request lifecycle ----------

fn set_request_status(core: &Core, rid: &str, status: &str) {
    let mut f = Row::new();
    f.insert("status".into(), json!(status));
    let _ = core.db.update("request", rid, &f);
}

fn complete_request(core: &Core, rid: &str, pid: &str, out: &process::GenOutcome) {
    let mut f = Row::new();
    f.insert("status".into(), json!("COMPLETED"));
    f.insert("media_id".into(), json!(out.media_id));
    if !out.url.is_empty() {
        f.insert("output_url".into(), json!(out.url));
    }
    let _ = core.db.update("request", rid, &f);
    core.dash.emit(
        "request_completed",
        json!({ "request_id": rid, "project_id": pid }),
    );
}

fn fail_request(core: &Core, rid: &str, msg: &str) -> Result<(), String> {
    let now = db::now();
    let _ = core.db.execute(
        "UPDATE request SET status = 'FAILED', error_message = ?2, \
         retry_count = retry_count + 1, updated_at = ?3 WHERE id = ?1",
        &[&rid, &msg, &now],
    );
    core.dash.emit(
        "request_failed",
        json!({ "request_id": rid, "error_message": msg }),
    );
    let short: String = rid.chars().take(8).collect();
    Err(format!("request {short}: {msg}"))
}

// ---------- extension events (port of events.go) ----------

/// Route events pushed by the Chrome extension. The bridge invokes the handler
/// from inside a spawned tokio task (see extbridge::dispatch_inbound), so the
/// sync `Fn(Value)` may hand DB work to the blocking pool (Go used a goroutine).
pub fn install_extension_event_handler(core: Arc<Core>) {
    let c = core.clone();
    core.ext.set_event_handler(move |msg: Value| {
        let t = msg
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        match t.as_str() {
            "token_captured" => {
                println!("[worker] extension: token captured");
                c.dash.emit("extension:token_captured", json!({}));
            }
            "extension_ready" => {
                println!("[worker] extension: ready");
                c.dash.emit("extension:ready", json!({}));
            }
            "media_urls_refresh" => {
                let core = c.clone();
                tokio::task::spawn_blocking(move || refresh_media_urls(&core, &msg));
            }
            "flow_project_id" => {
                // The real, browsable Flow project id from the user's open tab.
                // Only SEED `flow.session_project` when it is still empty — never
                // OVERWRITE one the app already established (via ensure_flow_project
                // search/create). Otherwise the extension fires on every keepAlive /
                // tab-navigation, so a user browsing to an unrelated project
                // mid-batch would hijack generation and scatter scenes into it.
                if let Some(pid) = msg.get("projectId").and_then(|v| v.as_str()) {
                    if crate::process::is_uuid(pid)
                        && c.db.kv_get("flow.session_project").is_empty()
                    {
                        let _ = c.db.kv_set("flow.session_project", pid);
                        println!("[worker] seeded Flow project id = {pid}");
                        c.dash.emit("flow:project", json!({ "project_id": pid }));
                    }
                }
            }
            "" => {}
            other => {
                // Unknown events forwarded to the dashboard for debugging.
                c.dash.emit("extension:event", json!({ "type": other }));
            }
        }
    });
}

/// Refresh scene/character URL columns for every media_id in the payload's
/// `items` list ({media_id, url} pairs).
fn refresh_media_urls(core: &Arc<Core>, msg: &Value) {
    // The extension sends `{type:"media_urls_refresh", urls:[{mediaId,url,mediaType}]}`.
    // Reading only `items`/`media_id` meant every scraped URL was dropped —
    // which is the recovery path when a render finishes without a URL.
    let items = msg
        .get("urls")
        .or_else(|| msg.get("items"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if items.is_empty() {
        return;
    }
    let mut applied = 0usize;
    for item in &items {
        let mid = item
            .get("mediaId")
            .or_else(|| item.get("media_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let url = item.get("url").and_then(|v| v.as_str()).unwrap_or("");
        if mid.is_empty() || url.is_empty() {
            continue;
        }
        applied += 1;

        // Scene image/video/upscale URLs by media_id (both orientations).
        for col in [
            "vertical_image_media_id",
            "vertical_video_media_id",
            "vertical_upscale_media_id",
            "horizontal_image_media_id",
            "horizontal_video_media_id",
            "horizontal_upscale_media_id",
        ] {
            let url_col = match url_column_for(col) {
                Some(u) => u,
                None => continue,
            };
            let _ = core.db.execute(
                &format!("UPDATE scene SET {url_col} = ?1 WHERE {col} = ?2"),
                &[&url, &mid],
            );
        }

        // Character reference image.
        let _ = core.db.execute(
            "UPDATE character SET reference_image_url = ?1 WHERE media_id = ?2",
            &[&url, &mid],
        );
    }

    // Learn the real Veo model key Flow is on, so app-built requests match the
    // user's selected model (e.g. Veo 3.1 Lite) instead of a hardcoded guess.
    if let Some(model) = msg.get("videoModel").and_then(|v| v.as_str()) {
        if model.starts_with("veo") && core.db.kv_get("flow.video_model") != model {
            let _ = core.db.kv_set("flow.video_model", model);
            println!("[worker] learned Flow video model: {model}");
        }
    }

    if applied > 0 {
        // Which tRPC procedure produced these — the only lead for fetching URLs
        // actively instead of waiting for the user to browse the Flow tab.
        if let Some(src) = msg.get("trpcUrl").and_then(|v| v.as_str()) {
            println!("[worker] media URLs came from tRPC: {src}");
        }
        println!("[worker] refreshed {applied} media URL(s) from the extension");
        core.dash
            .emit("media_urls_refreshed", json!({ "count": applied }));

        // These are short-lived signed URLs. Pull the assets down now so the
        // rescue is permanent instead of expiring again in a few hours.
        // `refresh_media_urls` runs on a blocking thread, so hop back onto the
        // runtime to do the downloads.
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let core2 = core.clone();
            handle.spawn(async move {
                let rep = crate::mediastore::localize_project(&core2, "").await;
                if rep.downloaded > 0 {
                    println!(
                        "[worker] downloaded {} refreshed asset(s) locally",
                        rep.downloaded
                    );
                }
            });
        }
    }
}

fn url_column_for(media_id_col: &str) -> Option<&'static str> {
    match media_id_col {
        "vertical_image_media_id" => Some("vertical_image_url"),
        "vertical_video_media_id" => Some("vertical_video_url"),
        "vertical_upscale_media_id" => Some("vertical_upscale_url"),
        "horizontal_image_media_id" => Some("horizontal_image_url"),
        "horizontal_video_media_id" => Some("horizontal_video_url"),
        "horizontal_upscale_media_id" => Some("horizontal_upscale_url"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(typ: &str, created: &str) -> Row {
        let mut m = Row::new();
        m.insert("type".into(), json!(typ));
        m.insert("created_at".into(), json!(created));
        m
    }

    #[test]
    fn pick_job_priority() {
        let rows = vec![
            req("UPSCALE_VIDEO", "2026-01-01T00:00:00Z"),
            req("REGENERATE_VIDEO", "2026-01-01T00:00:01Z"),
            req("GENERATE_VIDEO", "2026-01-01T00:00:02Z"),
        ];
        assert_eq!(
            db::str_of(pick_job(&rows).unwrap(), "type"),
            "GENERATE_VIDEO"
        );

        let rows = vec![
            req("UPSCALE_VIDEO", "2026-01-01T00:00:00Z"),
            req("REGENERATE_VIDEO", "2026-01-01T00:00:01Z"),
        ];
        assert_eq!(
            db::str_of(pick_job(&rows).unwrap(), "type"),
            "REGENERATE_VIDEO"
        );

        assert!(pick_job(&[]).is_none());
        assert!(pick_job(&[req("GENERATE_IMAGE", "x")]).is_none());
    }

    #[test]
    fn url_column_mapping() {
        assert_eq!(
            url_column_for("vertical_video_media_id"),
            Some("vertical_video_url")
        );
        assert_eq!(
            url_column_for("horizontal_upscale_media_id"),
            Some("horizontal_upscale_url")
        );
        assert_eq!(url_column_for("bogus"), None);
    }
}
