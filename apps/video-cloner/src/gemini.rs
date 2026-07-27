//! Gemini client.
//!
//! Unlike the other Space Apps, this one does NOT go through the daemon's
//! `llm.request` bridge. The bridge carries `{system, prompt, maxTokens,
//! profile}` only — it has no way to attach a video or an image, and no
//! `temperature`. Both are essential here: the video *is* the input, and the
//! creativity slider is expressed as sampling temperature. So we talk to the
//! Generative Language API directly with the app's own API key.

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

const API_ROOT: &str = "https://generativelanguage.googleapis.com";

/// Requests carrying inline data must stay under the 20 MB total request cap.
/// Anything larger goes through the Files API instead. The margin covers the
/// base64 expansion of the payload plus the prompt text.
const INLINE_LIMIT_BYTES: u64 = 14 * 1024 * 1024;

/// Files uploaded to the Gemini Files API are retained for 48 hours. We refresh
/// well before that so a long-running project never resumes onto a dead handle.
const FILE_URI_TTL_HOURS: i64 = 40;

fn http() -> Result<reqwest::Client> {
    // Video analysis is slow; a whole 8-second segment can take minutes.
    reqwest::Client::builder()
        .timeout(Duration::from_secs(900))
        .build()
        .context("building HTTP client")
}

/// reqwest's `Display` hides the underlying cause, which turns every network
/// problem into an indistinguishable "error sending request".
fn describe(e: &reqwest::Error) -> String {
    let mut out = e.to_string();
    let mut src: Option<&dyn std::error::Error> = std::error::Error::source(e);
    while let Some(s) = src {
        out.push_str(&format!(": {s}"));
        src = s.source();
    }
    out
}

pub fn is_file_uri_fresh(uploaded_at: &str) -> bool {
    if uploaded_at.trim().is_empty() {
        return false;
    }
    match chrono::DateTime::parse_from_rfc3339(uploaded_at) {
        Ok(t) => {
            let age = chrono::Utc::now().signed_duration_since(t.with_timezone(&chrono::Utc));
            age.num_hours() < FILE_URI_TTL_HOURS
        }
        Err(_) => false,
    }
}

/// How the video is attached to a request.
pub enum VideoPart {
    /// Base64 in the request body — small files only.
    Inline { mime: String, data: String },
    /// A Files API handle, reusable across requests.
    Remote { mime: String, uri: String },
}

impl VideoPart {
    fn to_part(&self) -> Value {
        match self {
            VideoPart::Inline { mime, data } => {
                json!({ "inline_data": { "mime_type": mime, "data": data } })
            }
            VideoPart::Remote { mime, uri } => {
                json!({ "file_data": { "mime_type": mime, "file_uri": uri } })
            }
        }
    }
}

pub fn needs_files_api(size: u64) -> bool {
    size > INLINE_LIMIT_BYTES
}

pub async fn read_inline(path: &Path, mime: &str) -> Result<VideoPart> {
    let bytes = tokio::fs::read(path)
        .await
        .with_context(|| format!("đọc file {}", path.display()))?;
    Ok(VideoPart::Inline {
        mime: mime.to_string(),
        data: base64::engine::general_purpose::STANDARD.encode(&bytes),
    })
}

/// Upload a file through the Files API resumable protocol and wait until the
/// service finishes processing it.
///
/// Video uploads land in `PROCESSING` state; referencing the URI before it
/// turns `ACTIVE` is rejected, so we poll here rather than letting the
/// generate call fail.
pub async fn upload_file(api_key: &str, path: &Path, mime: &str, display_name: &str) -> Result<String> {
    let client = http()?;
    let size = tokio::fs::metadata(path)
        .await
        .with_context(|| format!("stat file {}", path.display()))?
        .len();

    let start = client
        .post(format!("{API_ROOT}/upload/v1beta/files"))
        .query(&[("key", api_key)])
        .header("X-Goog-Upload-Protocol", "resumable")
        .header("X-Goog-Upload-Command", "start")
        .header("X-Goog-Upload-Header-Content-Length", size.to_string())
        .header("X-Goog-Upload-Header-Content-Type", mime)
        .json(&json!({ "file": { "display_name": display_name } }))
        .send()
        .await
        .map_err(|e| anyhow!("bắt đầu upload thất bại: {}", describe(&e)))?;

    if !start.status().is_success() {
        let status = start.status();
        let body = start.text().await.unwrap_or_default();
        bail!("bắt đầu upload thất bại ({status}): {}", trim_err(&body));
    }

    let upload_url = start
        .headers()
        .get("x-goog-upload-url")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("Gemini không trả về URL upload"))?;

    let bytes = tokio::fs::read(path).await?;
    let finish = client
        .post(&upload_url)
        .header("Content-Length", size.to_string())
        .header("X-Goog-Upload-Offset", "0")
        .header("X-Goog-Upload-Command", "upload, finalize")
        .body(bytes)
        .send()
        .await
        .map_err(|e| anyhow!("tải video lên thất bại: {}", describe(&e)))?;

    if !finish.status().is_success() {
        let status = finish.status();
        let body = finish.text().await.unwrap_or_default();
        bail!("tải video lên thất bại ({status}): {}", trim_err(&body));
    }

    let v: Value = finish.json().await.context("đọc kết quả upload")?;
    let uri = v["file"]["uri"]
        .as_str()
        .ok_or_else(|| anyhow!("kết quả upload thiếu file.uri"))?
        .to_string();
    let name = v["file"]["name"].as_str().unwrap_or("").to_string();

    wait_until_active(&client, api_key, &name).await?;
    Ok(uri)
}

async fn wait_until_active(client: &reqwest::Client, api_key: &str, name: &str) -> Result<()> {
    if name.is_empty() {
        return Ok(());
    }
    // Google processes video server-side; a few minutes is normal for long clips.
    for _ in 0..120 {
        let resp = client
            .get(format!("{API_ROOT}/v1beta/{name}"))
            .query(&[("key", api_key)])
            .send()
            .await
            .map_err(|e| anyhow!("kiểm tra trạng thái file thất bại: {}", describe(&e)))?;
        let v: Value = resp.json().await.unwrap_or(Value::Null);
        match v["state"].as_str().unwrap_or("") {
            "ACTIVE" => return Ok(()),
            "FAILED" => bail!(
                "Gemini không xử lý được video: {}",
                v["error"]["message"].as_str().unwrap_or("không rõ lý do")
            ),
            _ => tokio::time::sleep(Duration::from_secs(5)).await,
        }
    }
    bail!("Gemini xử lý video quá lâu (quá 10 phút)")
}

pub struct GenerateRequest<'a> {
    pub api_key: &'a str,
    pub model: &'a str,
    pub system: &'a str,
    pub prompt: &'a str,
    pub temperature: f64,
    pub video: &'a VideoPart,
    /// Optional character reference image, always inline (images are small).
    pub char_image: Option<(String, String)>,
}

/// One `generateContent` call. Returns the raw model text.
pub async fn generate(req: GenerateRequest<'_>) -> Result<String> {
    if req.api_key.trim().is_empty() {
        bail!("chưa có Gemini API key — vào Cài đặt của Video Cloner để nhập key");
    }

    let mut parts = vec![req.video.to_part()];
    if let Some((mime, data)) = &req.char_image {
        parts.push(json!({ "inline_data": { "mime_type": mime, "data": data } }));
    }
    parts.push(json!({ "text": req.prompt }));

    let body = json!({
        "system_instruction": { "parts": [{ "text": req.system }] },
        "contents": [{ "role": "user", "parts": parts }],
        "generationConfig": { "temperature": req.temperature },
    });

    let url = format!("{API_ROOT}/v1beta/models/{}:generateContent", req.model);
    let resp = http()?
        .post(&url)
        .query(&[("key", req.api_key)])
        .json(&body)
        .send()
        .await
        .map_err(|e| anyhow!("gọi Gemini thất bại: {}", describe(&e)))?;

    let status = resp.status();
    let v: Value = resp
        .json()
        .await
        .map_err(|e| anyhow!("phản hồi Gemini không phải JSON: {}", describe(&e)))?;

    if !status.is_success() {
        let msg = v["error"]["message"].as_str().unwrap_or("không rõ lỗi");
        bail!("Gemini trả lỗi ({status}): {}", trim_err(msg));
    }

    extract_text(&v)
}

/// Pull the model's text out of a `generateContent` response.
///
/// A blocked or empty candidate is an error rather than an empty string: the
/// caller would otherwise persist "0 scenes" as a successful run and the user
/// would never learn the prompt was rejected.
pub fn extract_text(v: &Value) -> Result<String> {
    if let Some(reason) = v["promptFeedback"]["blockReason"].as_str() {
        bail!("Gemini từ chối nội dung (blockReason: {reason})");
    }

    let candidate = v["candidates"].get(0);
    let text: String = candidate
        .and_then(|c| c["content"]["parts"].as_array())
        .map(|parts| {
            parts
                .iter()
                .filter_map(|p| p["text"].as_str())
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default();

    if text.trim().is_empty() {
        let finish = candidate
            .and_then(|c| c["finishReason"].as_str())
            .unwrap_or("");
        bail!(match finish {
            "SAFETY" => "Gemini chặn kết quả vì bộ lọc an toàn".to_string(),
            "RECITATION" => "Gemini chặn kết quả vì trùng nội dung có bản quyền".to_string(),
            "MAX_TOKENS" => "Gemini cắt kết quả vì quá dài — thử lại từng đoạn ngắn hơn".to_string(),
            other if !other.is_empty() => format!("Gemini không trả về nội dung ({other})"),
            _ => "Gemini không trả về nội dung".to_string(),
        });
    }
    Ok(text)
}

fn trim_err(s: &str) -> String {
    crate::scenes::truncate_chars(s.trim(), 400)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn small_videos_stay_inline() {
        assert!(!needs_files_api(1024));
        assert!(needs_files_api(INLINE_LIMIT_BYTES + 1));
    }

    #[test]
    fn inline_and_remote_parts_use_the_right_field_names() {
        let inline = VideoPart::Inline {
            mime: "video/mp4".into(),
            data: "AAA".into(),
        };
        assert_eq!(inline.to_part()["inline_data"]["mime_type"], "video/mp4");

        let remote = VideoPart::Remote {
            mime: "video/mp4".into(),
            uri: "https://x/files/1".into(),
        };
        assert_eq!(remote.to_part()["file_data"]["file_uri"], "https://x/files/1");
    }

    #[test]
    fn extract_text_joins_every_part() {
        let v = json!({
            "candidates": [{ "content": { "parts": [{"text": "a"}, {"text": "b"}] } }]
        });
        assert_eq!(extract_text(&v).unwrap(), "ab");
    }

    #[test]
    fn empty_candidate_is_an_error_not_an_empty_string() {
        let v = json!({ "candidates": [{ "content": { "parts": [] }, "finishReason": "SAFETY" }] });
        let err = extract_text(&v).unwrap_err().to_string();
        assert!(err.contains("an toàn"), "unexpected: {err}");
    }

    #[test]
    fn blocked_prompt_is_reported() {
        let v = json!({ "promptFeedback": { "blockReason": "OTHER" } });
        assert!(extract_text(&v).unwrap_err().to_string().contains("OTHER"));
    }

    #[test]
    fn a_missing_upload_timestamp_forces_a_re_upload() {
        assert!(!is_file_uri_fresh(""));
        assert!(!is_file_uri_fresh("not-a-date"));
        assert!(is_file_uri_fresh(&crate::db::now()));
    }

    #[test]
    fn an_old_upload_is_considered_expired() {
        let old = (chrono::Utc::now() - chrono::Duration::hours(FILE_URI_TTL_HOURS + 1))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        assert!(!is_file_uri_fresh(&old));
    }
}
