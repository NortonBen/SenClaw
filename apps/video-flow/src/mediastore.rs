//! Pull a remote asset into local media storage.
//!
//! Google Flow serves generated images/videos from short-lived signed URLs — a
//! project whose DB only holds those URLs shows broken thumbnails a few hours
//! later. Every successful generation therefore mirrors the asset locally and
//! the DB points at `/api/media/{id}/file` instead.
//!
//! Shared by `process.rs` (inline, right after generation) and the
//! `media_download` pipeline agent (bulk sweep).

use crate::db::{self, Db};
use crate::state::Core;
use serde_json::{json, Map};

pub fn is_remote_url(u: &str) -> bool {
    u.starts_with("http://") || u.starts_with("https://")
}

fn ext_from_content_type(ct: &str) -> &'static str {
    match ct.split(';').next().unwrap_or("").trim() {
        "image/jpeg" => ".jpg",
        "image/png" => ".png",
        "image/webp" => ".webp",
        "image/gif" => ".gif",
        "video/mp4" => ".mp4",
        "video/webm" => ".webm",
        "video/quicktime" => ".mov",
        _ => "",
    }
}

fn default_media_ext(media_type: &str) -> &'static str {
    if media_type == "video" {
        ".mp4"
    } else {
        ".jpg"
    }
}

/// Download `raw_url` into `core.media_dir`, record a `media` row and return the
/// local `/api/media/{id}/file` URL. Re-downloading the same `original_url`
/// returns the existing row instead of duplicating the file.
pub async fn store_remote(core: &Core, raw_url: &str, media_type: &str) -> Result<String, String> {
    let db: &Db = &core.db;
    if let Ok(Some(existing)) =
        db.query_one("SELECT id FROM media WHERE original_url = ?1", &[&raw_url])
    {
        return Ok(format!("/api/media/{}/file", db::str_of(&existing, "id")));
    }

    // Some CDNs (Wikimedia among them) answer 403 to a UA-less request.
    let resp = crate::llm::http()
        .get(raw_url)
        .header(
            "user-agent",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 \
             (KHTML, like Gecko) Chrome/124.0 Safari/537.36 SenClawVideoFlow/0.1",
        )
        .send()
        .await
        .map_err(|e| format!("download: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("download body: {e}"))?;
    if bytes.is_empty() {
        return Err("empty response body".to_string());
    }

    let mut ext = ext_from_content_type(&content_type).to_string();
    if ext.is_empty() {
        let path_part = raw_url.splitn(2, '?').next().unwrap_or("");
        if let Some(idx) = path_part.rfind('.') {
            let cand = &path_part[idx..];
            if cand.len() <= 5 && !cand.contains('/') {
                ext = cand.to_string();
            }
        }
    }
    if ext.is_empty() {
        ext = default_media_ext(media_type).to_string();
    }

    let id = db::new_id();
    std::fs::create_dir_all(&core.media_dir).map_err(|e| format!("mkdir: {e}"))?;
    let file_name = format!("{id}{ext}");
    let dest_path = core.media_dir.join(&file_name);
    std::fs::write(&dest_path, &bytes).map_err(|e| format!("write: {e}"))?;

    let mime_type = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    let (w_px, h_px) = crate::media::probe_dimensions(media_type, &dest_path).await;

    let mut cm = Map::new();
    cm.insert("id".into(), json!(id));
    cm.insert("file_name".into(), json!(file_name));
    cm.insert("file_path".into(), json!(dest_path.to_string_lossy()));
    cm.insert("mime_type".into(), json!(mime_type));
    cm.insert("size_bytes".into(), json!(bytes.len()));
    cm.insert("media_type".into(), json!(media_type));
    cm.insert("original_url".into(), json!(raw_url));
    if w_px > 0 && h_px > 0 {
        cm.insert("width_px".into(), json!(w_px));
        cm.insert("height_px".into(), json!(h_px));
    }
    db.insert("media", &cm).map_err(|e| {
        let _ = std::fs::remove_file(&dest_path);
        format!("create media record: {e}")
    })?;
    Ok(format!("/api/media/{id}/file"))
}

/// Persist raw media bytes locally (e.g. an inline base64 MP4 from a Low
/// Priority Veo render) and return the `/api/media/<id>/file` URL. No dedup by
/// URL — inline media has none — so the caller owns idempotency.
pub async fn store_bytes(
    core: &Core,
    bytes: &[u8],
    media_type: &str,
    ext: &str,
) -> Result<String, String> {
    if bytes.is_empty() {
        return Err("empty media bytes".to_string());
    }
    let id = db::new_id();
    std::fs::create_dir_all(&core.media_dir).map_err(|e| format!("mkdir: {e}"))?;
    let ext = if ext.starts_with('.') {
        ext.to_string()
    } else {
        format!(".{ext}")
    };
    let file_name = format!("{id}{ext}");
    let dest_path = core.media_dir.join(&file_name);
    std::fs::write(&dest_path, bytes).map_err(|e| format!("write: {e}"))?;

    let (w_px, h_px) = crate::media::probe_dimensions(media_type, &dest_path).await;
    let mut cm = Map::new();
    cm.insert("id".into(), json!(id));
    cm.insert("file_name".into(), json!(file_name));
    cm.insert("file_path".into(), json!(dest_path.to_string_lossy()));
    cm.insert(
        "mime_type".into(),
        json!(if media_type == "video" {
            "video/mp4"
        } else {
            "application/octet-stream"
        }),
    );
    cm.insert("size_bytes".into(), json!(bytes.len()));
    cm.insert("media_type".into(), json!(media_type));
    if w_px > 0 && h_px > 0 {
        cm.insert("width_px".into(), json!(w_px));
        cm.insert("height_px".into(), json!(h_px));
    }
    core.db.insert("media", &cm).map_err(|e| {
        let _ = std::fs::remove_file(&dest_path);
        format!("create media record: {e}")
    })?;
    Ok(format!("/api/media/{id}/file"))
}

/// Mirror `url` locally and rewrite `table.column` for row `id` to point at the
/// local copy. Best-effort: on any failure the remote URL is left in place, so a
/// download problem never loses a generated asset.
pub async fn localize_column(
    core: &Core,
    table: &str,
    id: &str,
    column: &str,
    url: &str,
    media_type: &str,
) -> Option<String> {
    if !is_remote_url(url) {
        return None;
    }
    match store_remote(core, url, media_type).await {
        Ok(local) => {
            let mut m = Map::new();
            m.insert(column.to_string(), json!(local.clone()));
            match core.db.update(table, id, &m) {
                Ok(()) => Some(local),
                Err(e) => {
                    eprintln!("[media] rewrite {table}.{column} for {id} failed: {e}");
                    None
                }
            }
        }
        Err(e) => {
            eprintln!("[media] localize {url} failed: {e}");
            None
        }
    }
}

/// Every DB column that can hold a generated asset URL, with its media type.
const SCENE_URL_COLUMNS: &[(&str, &str)] = &[
    ("vertical_image_url", "image"),
    ("horizontal_image_url", "image"),
    ("vertical_video_url", "video"),
    ("horizontal_video_url", "video"),
    ("vertical_upscale_url", "video"),
    ("horizontal_upscale_url", "video"),
    ("narrator_audio_url", "audio"),
];

/// Result of a bulk localize sweep.
#[derive(Default, serde::Serialize)]
pub struct LocalizeReport {
    pub downloaded: usize,
    pub skipped: usize,
    pub failed: usize,
    pub errors: Vec<String>,
}

/// Mirror every still-remote asset URL for a project (or the whole DB when
/// `project_id` is empty) into local media. Repairs projects generated before
/// inline downloading existed, and rescues URLs before they expire.
pub async fn localize_project(core: &Core, project_id: &str) -> LocalizeReport {
    let db = &core.db;
    let mut rep = LocalizeReport::default();

    let scenes = if project_id.is_empty() {
        db.query("SELECT * FROM scene", &[]).unwrap_or_default()
    } else {
        db.query(
            "SELECT s.* FROM scene s JOIN video v ON v.id = s.video_id WHERE v.project_id = ?1",
            &[&project_id],
        )
        .unwrap_or_default()
    };
    for sc in &scenes {
        let sid = db::str_of(sc, "id");
        for (col, media_type) in SCENE_URL_COLUMNS {
            let url = db::str_of(sc, col);
            if url.is_empty() {
                continue;
            }
            if !is_remote_url(&url) {
                rep.skipped += 1;
                continue;
            }
            match localize_column(core, "scene", &sid, col, &url, media_type).await {
                Some(_) => rep.downloaded += 1,
                None => {
                    rep.failed += 1;
                    if rep.errors.len() < 10 {
                        rep.errors.push(format!("scene {sid}.{col}"));
                    }
                }
            }
        }
    }

    let characters = if project_id.is_empty() {
        db.query("SELECT * FROM character", &[]).unwrap_or_default()
    } else {
        db.query(
            "SELECT c.* FROM character c JOIN project_character pc ON pc.character_id = c.id \
             WHERE pc.project_id = ?1",
            &[&project_id],
        )
        .unwrap_or_default()
    };
    for ch in &characters {
        let cid = db::str_of(ch, "id");
        let url = db::str_of(ch, "reference_image_url");
        if url.is_empty() {
            continue;
        }
        if !is_remote_url(&url) {
            rep.skipped += 1;
            continue;
        }
        match localize_column(
            core,
            "character",
            &cid,
            "reference_image_url",
            &url,
            "image",
        )
        .await
        {
            Some(_) => rep.downloaded += 1,
            None => {
                rep.failed += 1;
                if rep.errors.len() < 10 {
                    rep.errors.push(format!("character {cid}"));
                }
            }
        }
    }
    rep
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_url_detection() {
        assert!(is_remote_url("https://x/y.jpg"));
        assert!(is_remote_url("http://x/y.jpg"));
        assert!(!is_remote_url("/api/media/abc/file"));
        assert!(!is_remote_url(""));
    }

    #[test]
    fn extension_resolution() {
        assert_eq!(ext_from_content_type("image/png"), ".png");
        assert_eq!(ext_from_content_type("video/mp4; codecs=avc1"), ".mp4");
        assert_eq!(ext_from_content_type("application/octet-stream"), "");
        assert_eq!(default_media_ext("video"), ".mp4");
        assert_eq!(default_media_ext("image"), ".jpg");
    }
}
