//! Media storage — port of the media parts of `internal/api/handlers.go` plus
//! `internal/mediautil/dimensions.go`. Uploads stream to `core.media_dir`;
//! image dimensions are sniffed from the file header (png/jpeg/gif/webp),
//! videos go through `ffprobe` when available.

use crate::db;
use crate::state::AppState;
use axum::body::Body;
use axum::extract::{Multipart, Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Map};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub const MAX_UPLOAD_BYTES: usize = 500 << 20; // 500 MB

fn err(code: StatusCode, msg: impl Into<String>) -> Response {
    (code, Json(json!({ "error": msg.into() }))).into_response()
}

pub fn media_type_from_mime(mime: &str) -> &'static str {
    if mime.starts_with("image/") {
        "image"
    } else if mime.starts_with("audio/") {
        "audio"
    } else if mime.starts_with("video/") {
        "video"
    } else {
        "other"
    }
}

pub fn mime_from_ext(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        ".jpg" | ".jpeg" => "image/jpeg",
        ".png" => "image/png",
        ".webp" => "image/webp",
        ".gif" => "image/gif",
        ".mp3" => "audio/mpeg",
        ".wav" => "audio/wav",
        ".ogg" => "audio/ogg",
        ".aac" => "audio/aac",
        ".mp4" => "video/mp4",
        ".mov" => "video/quicktime",
        ".webm" => "video/webm",
        ".mkv" => "video/x-matroska",
        _ => "application/octet-stream",
    }
}

// ---------- dimension probing ----------

/// Pixel width/height for image or video files; (0,0) on failure.
pub async fn probe_dimensions(media_type: &str, path: &std::path::Path) -> (i64, i64) {
    match media_type {
        "image" => probe_image(path),
        "video" => probe_video(path).await,
        _ => (0, 0),
    }
}

fn probe_image(path: &std::path::Path) -> (i64, i64) {
    use std::io::Read;
    let Ok(f) = std::fs::File::open(path) else { return (0, 0) };
    let mut buf = Vec::new();
    // Headers live at the front; 5 MB covers even EXIF-heavy JPEGs.
    if f.take(5 * 1024 * 1024).read_to_end(&mut buf).is_err() {
        return (0, 0);
    }
    png_dims(&buf)
        .or_else(|| jpeg_dims(&buf))
        .or_else(|| gif_dims(&buf))
        .or_else(|| webp_dims(&buf))
        .unwrap_or((0, 0))
}

fn be16(a: u8, b: u8) -> i64 {
    ((a as i64) << 8) | b as i64
}

fn png_dims(b: &[u8]) -> Option<(i64, i64)> {
    const SIG: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];
    if b.len() < 24 || b[..8] != SIG || &b[12..16] != b"IHDR" {
        return None;
    }
    let w = ((b[16] as i64) << 24) | ((b[17] as i64) << 16) | be16(b[18], b[19]);
    let h = ((b[20] as i64) << 24) | ((b[21] as i64) << 16) | be16(b[22], b[23]);
    Some((w, h))
}

fn gif_dims(b: &[u8]) -> Option<(i64, i64)> {
    if b.len() < 10 || !b.starts_with(b"GIF8") {
        return None;
    }
    let w = b[6] as i64 | ((b[7] as i64) << 8);
    let h = b[8] as i64 | ((b[9] as i64) << 8);
    Some((w, h))
}

fn jpeg_dims(b: &[u8]) -> Option<(i64, i64)> {
    if b.len() < 4 || b[0] != 0xFF || b[1] != 0xD8 {
        return None;
    }
    let mut i = 2usize;
    while i + 4 <= b.len() {
        if b[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = b[i + 1];
        if marker == 0xFF {
            i += 1; // fill byte
            continue;
        }
        // Standalone markers without a length field.
        if marker == 0xD8 || marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            i += 2;
            continue;
        }
        let seg_len = be16(b[i + 2], b[i + 3]) as usize;
        let is_sof = (0xC0..=0xCF).contains(&marker) && marker != 0xC4 && marker != 0xC8 && marker != 0xCC;
        if is_sof {
            if i + 9 <= b.len() {
                let h = be16(b[i + 5], b[i + 6]);
                let w = be16(b[i + 7], b[i + 8]);
                return Some((w, h));
            }
            return None;
        }
        if seg_len < 2 {
            return None;
        }
        i += 2 + seg_len;
    }
    None
}

fn webp_dims(b: &[u8]) -> Option<(i64, i64)> {
    if b.len() < 30 || &b[0..4] != b"RIFF" || &b[8..12] != b"WEBP" {
        return None;
    }
    match &b[12..16] {
        b"VP8 " => {
            if b[23] == 0x9D && b[24] == 0x01 && b[25] == 0x2A {
                let w = (u16::from_le_bytes([b[26], b[27]]) & 0x3FFF) as i64;
                let h = (u16::from_le_bytes([b[28], b[29]]) & 0x3FFF) as i64;
                Some((w, h))
            } else {
                None
            }
        }
        b"VP8L" => {
            if b[20] != 0x2F {
                return None;
            }
            let bits = u32::from_le_bytes([b[21], b[22], b[23], b[24]]);
            Some((((bits & 0x3FFF) + 1) as i64, (((bits >> 14) & 0x3FFF) + 1) as i64))
        }
        b"VP8X" => {
            let w = 1 + (b[24] as i64 | ((b[25] as i64) << 8) | ((b[26] as i64) << 16));
            let h = 1 + (b[27] as i64 | ((b[28] as i64) << 8) | ((b[29] as i64) << 16));
            Some((w, h))
        }
        _ => None,
    }
}

async fn probe_video(path: &std::path::Path) -> (i64, i64) {
    let out = tokio::process::Command::new("ffprobe")
        .args([
            "-v", "error",
            "-select_streams", "v:0",
            "-show_entries", "stream=width,height",
            "-of", "csv=p=0",
        ])
        .arg(path)
        .output()
        .await;
    let Ok(out) = out else { return (0, 0) };
    if !out.status.success() {
        return (0, 0);
    }
    let line = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let parts: Vec<&str> = line.split(',').collect();
    if parts.len() != 2 {
        return (0, 0);
    }
    let w: i64 = parts[0].trim().parse().unwrap_or(0);
    let h: i64 = parts[1].trim().parse().unwrap_or(0);
    if w <= 0 || h <= 0 {
        return (0, 0);
    }
    (w, h)
}

// ---------- upload / serve / delete ----------

/// POST /media/upload — multipart `file` field, streamed to media_dir.
pub async fn upload_media(State(st): State<AppState>, mut mp: Multipart) -> Response {
    loop {
        let field = match mp.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => return err(StatusCode::BAD_REQUEST, "missing file field"),
            Err(_) => return err(StatusCode::BAD_REQUEST, "file too large or bad form"),
        };
        if field.name() != Some("file") {
            continue;
        }
        let file_name = field.file_name().unwrap_or("").to_string();
        let ext = std::path::Path::new(&file_name)
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
            .unwrap_or_default();
        let mime = mime_from_ext(&ext);
        let media_type = media_type_from_mime(mime);

        let id = db::new_id();
        let dir = st.core.media_dir.clone();
        if std::fs::create_dir_all(&dir).is_err() {
            return err(StatusCode::INTERNAL_SERVER_ERROR, "cannot create media dir");
        }
        let dest = dir.join(format!("{id}{ext}"));
        let mut f = match tokio::fs::File::create(&dest).await {
            Ok(f) => f,
            Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "cannot create file"),
        };
        let mut size: i64 = 0;
        let mut field = field;
        loop {
            match field.chunk().await {
                Ok(Some(chunk)) => {
                    size += chunk.len() as i64;
                    if size as usize > MAX_UPLOAD_BYTES {
                        drop(f);
                        let _ = tokio::fs::remove_file(&dest).await;
                        return err(StatusCode::BAD_REQUEST, "file too large or bad form");
                    }
                    if f.write_all(&chunk).await.is_err() {
                        drop(f);
                        let _ = tokio::fs::remove_file(&dest).await;
                        return err(StatusCode::INTERNAL_SERVER_ERROR, "write failed");
                    }
                }
                Ok(None) => break,
                Err(_) => {
                    drop(f);
                    let _ = tokio::fs::remove_file(&dest).await;
                    return err(StatusCode::BAD_REQUEST, "file too large or bad form");
                }
            }
        }
        let _ = f.flush().await;
        drop(f);

        let (w_px, h_px) = probe_dimensions(media_type, &dest).await;
        let mut fields = Map::new();
        fields.insert("id".into(), json!(id));
        fields.insert("file_name".into(), json!(file_name));
        fields.insert("file_path".into(), json!(dest.to_string_lossy().to_string()));
        fields.insert("mime_type".into(), json!(mime));
        fields.insert("size_bytes".into(), json!(size));
        fields.insert("media_type".into(), json!(media_type));
        if w_px > 0 && h_px > 0 {
            fields.insert("width_px".into(), json!(w_px));
            fields.insert("height_px".into(), json!(h_px));
        }
        return match st.core.db.insert("media", &fields) {
            Ok(id) => match st.core.db.get("media", &id) {
                Ok(Some(row)) => {
                    (StatusCode::CREATED, Json(serde_json::Value::Object(row))).into_response()
                }
                Ok(None) => err(StatusCode::INTERNAL_SERVER_ERROR, "media row missing"),
                Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
            },
            Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        };
    }
}

/// GET /media/:mid/file — stream the file with mime + inline disposition + 1d cache.
pub async fn download_media(State(st): State<AppState>, Path(mid): Path<String>) -> Response {
    let row = match st.core.db.get("media", &mid) {
        Ok(Some(r)) => r,
        _ => return err(StatusCode::NOT_FOUND, "not found"),
    };
    let file_path = db::str_of(&row, "file_path");
    if file_path.is_empty() {
        return err(StatusCode::NOT_FOUND, "file path missing");
    }
    let file = match tokio::fs::File::open(&file_path).await {
        Ok(f) => f,
        Err(_) => return err(StatusCode::NOT_FOUND, "file not found on disk"),
    };
    let mut mime = db::str_of(&row, "mime_type");
    if mime.is_empty() {
        mime = "application/octet-stream".to_string();
    }
    let mut file_name = db::str_of(&row, "file_name");
    if file_name.is_empty() {
        let ext = std::path::Path::new(&file_path)
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        file_name = format!("{mid}{ext}");
    }

    let stream = async_stream::stream! {
        let mut file = file;
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match file.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => yield Ok::<Vec<u8>, std::io::Error>(buf[..n].to_vec()),
                Err(e) => {
                    yield Err(e);
                    break;
                }
            }
        }
    };

    let mut resp = Response::new(Body::from_stream(stream));
    let headers = resp.headers_mut();
    if let Ok(v) = header::HeaderValue::from_str(&mime) {
        headers.insert(header::CONTENT_TYPE, v);
    }
    let disp = format!("inline; filename=\"{file_name}\"");
    headers.insert(
        header::CONTENT_DISPOSITION,
        header::HeaderValue::from_str(&disp)
            .unwrap_or_else(|_| header::HeaderValue::from_static("inline")),
    );
    headers.insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("public, max-age=86400"),
    );
    resp
}

/// Remove one media row + its file. Ok(true) when the id did not exist.
pub fn delete_one_media(db: &crate::db::Db, id: &str) -> Result<bool, String> {
    let row = db.get("media", id).map_err(|e| e.to_string())?;
    let Some(row) = row else { return Ok(true) };
    let file_path = db::str_of(&row, "file_path");
    if !file_path.is_empty() {
        let _ = std::fs::remove_file(&file_path);
    }
    db.delete("media", id).map_err(|e| e.to_string())?;
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_header() {
        let mut b = vec![137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13];
        b.extend_from_slice(b"IHDR");
        b.extend_from_slice(&[0, 0, 0x04, 0x00, 0, 0, 0x02, 0x00]); // 1024x512
        b.extend_from_slice(&[8, 6, 0, 0, 0]);
        assert_eq!(png_dims(&b), Some((1024, 512)));
    }

    #[test]
    fn gif_header() {
        let mut b = b"GIF89a".to_vec();
        b.extend_from_slice(&[0x40, 0x01, 0xF0, 0x00]); // 320x240
        assert_eq!(gif_dims(&b), Some((320, 240)));
    }

    #[test]
    fn jpeg_sof() {
        // SOI + APP0 (len 16) + SOF0 with h=600 w=800.
        let mut b = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        b.extend_from_slice(&[0u8; 14]);
        b.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08, 0x02, 0x58, 0x03, 0x20]);
        assert_eq!(jpeg_dims(&b), Some((800, 600)));
    }

    #[test]
    fn mime_maps() {
        assert_eq!(mime_from_ext(".PNG"), "image/png");
        assert_eq!(media_type_from_mime("video/mp4"), "video");
        assert_eq!(media_type_from_mime("application/pdf"), "other");
    }
}
