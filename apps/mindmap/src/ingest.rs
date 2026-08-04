//! Turn an uploaded file into plain text: decode text-like files directly, and
//! run images through the SenClaw daemon's OCR endpoint (`/api/ocr/recognize`).
//! The extracted text is then handed to the LLM to generate a mind map.

use std::time::Duration;

const TEXT_EXTS: &[&str] = &[
    "txt", "md", "markdown", "text", "csv", "tsv", "json", "log", "rtf", "org", "rst", "tex", "rs",
    "py", "js", "ts", "tsx", "jsx", "java", "kt", "c", "h", "cpp", "cc", "hpp", "go", "rb", "php",
    "swift", "sh", "html", "htm", "xml", "yaml", "yml", "toml", "ini", "conf", "sql",
];

const IMAGE_EXTS: &[&str] = &[
    "png", "jpg", "jpeg", "webp", "gif", "bmp", "tif", "tiff", "heic",
];

fn ext_of(name: &str) -> String {
    name.rsplit('.').next().unwrap_or("").to_lowercase()
}

/// Extract text from an uploaded file. Returns `(text, ocr_used)`.
pub async fn extract_text(filename: &str, bytes: Vec<u8>) -> Result<(String, bool), String> {
    let ext = ext_of(filename);

    if IMAGE_EXTS.contains(&ext.as_str()) {
        let text = ocr_image(filename, bytes).await?;
        if text.trim().is_empty() {
            return Err("Ảnh không có văn bản nhận dạng được (OCR trả về rỗng).".into());
        }
        return Ok((text, true));
    }

    if TEXT_EXTS.contains(&ext.as_str()) {
        return Ok((String::from_utf8_lossy(&bytes).to_string(), false));
    }

    if ext == "pdf" {
        return Err(
            "File PDF chưa được hỗ trợ trực tiếp — hãy tải lên ảnh (chụp trang) hoặc văn bản."
                .into(),
        );
    }

    // Unknown extension: accept if it looks like UTF-8 text, else reject.
    match std::str::from_utf8(&bytes) {
        Ok(s) if looks_texty(s) => Ok((s.to_string(), false)),
        _ => Err(format!(
            "Không hỗ trợ định dạng file `.{ext}`. Hãy dùng ảnh hoặc file văn bản."
        )),
    }
}

fn looks_texty(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let control = s
        .chars()
        .filter(|c| c.is_control() && *c != '\n' && *c != '\r' && *c != '\t')
        .count();
    (control as f64) / (s.chars().count().max(1) as f64) < 0.02
}

/// POST the image to the daemon's OCR endpoint and return the recognized text.
async fn ocr_image(filename: &str, bytes: Vec<u8>) -> Result<String, String> {
    let base =
        std::env::var("SENCLAW_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:18788".into());
    let url = format!("{}/api/ocr/recognize", base.trim_end_matches('/'));

    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name(filename.to_string())
        .mime_str("application/octet-stream")
        .map_err(|e| e.to_string())?;
    let form = reqwest::multipart::Form::new()
        .part("image", part)
        .text("language", "vi");

    let resp = reqwest::Client::new()
        .post(&url)
        .multipart(form)
        .timeout(Duration::from_secs(180))
        .send()
        .await
        .map_err(|e| format!("Không gọi được OCR của daemon ({url}): {e}"))?;

    let status = resp.status();
    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("OCR trả về không hợp lệ: {e}"))?;
    if !status.is_success() {
        let msg = v.get("error").and_then(|x| x.as_str()).unwrap_or("OCR lỗi");
        // Common case: no OCR model installed yet.
        return Err(format!(
            "OCR chưa sẵn sàng: {msg}. Cài model OCR trong SenClaw trước."
        ));
    }
    Ok(v.get("text")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string())
}
