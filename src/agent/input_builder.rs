//! Input builder for processing user input with image attachments.
//! Mirrors `src-old/agent/InputBuilder.ts`.
//!
//! Detects and processes image URLs and file references in user input,
//! converting them to appropriate content blocks for the agent.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::time::Duration;
use base64::{Engine as _, engine::general_purpose::STANDARD};

/// Image attachment from explicit uploads (channel/UI).
#[derive(Debug, Clone)]
pub struct ImageAttachment {
    /// http(s) URL / absolute path / file:// / data: URL
    pub url: String,
    /// Optional MIME type
    pub mime_type: Option<String>,
}

/// Image attachment from WebSocket (data_url format).
#[derive(Debug, Clone)]
pub struct WebSocketImageAttachment {
    pub data_url: String,
    pub mime_type: String,
}

impl From<WebSocketImageAttachment> for ImageAttachment {
    fn from(ws_att: WebSocketImageAttachment) -> Self {
        ImageAttachment {
            url: ws_att.data_url,
            mime_type: Some(ws_att.mime_type),
        }
    }
}

/// Result of building agent input.
#[derive(Debug, Clone)]
pub struct BuildResult {
    /// Final input to feed to core.processUserInput
    pub input: Input,
    /// Detected image sources (for persistence, logging)
    pub image_srcs: Vec<String>,
    /// Failed image load placeholders
    pub failures: Vec<String>,
}

/// Input type that can be either plain text or content blocks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Input {
    /// Plain text input (no images)
    Text(String),
    /// Content blocks with images
    Blocks(Vec<ContentBlock>),
}

/// Content block for Anthropic API format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<ImageSource>,
}

/// Image source for content blocks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageSource {
    #[serde(rename = "type")]
    pub source_type: String,
    pub media_type: String,
    pub data: String,
}

/// Regex for detecting image URLs in text.
/// Matches http(s)://...png|jpg|jpeg|gif|webp
const URL_IMAGE_REGEX: &str = r"(https?://\S+?\.(?:png|jpe?g|gif|webp))(?:\?\S*)?";

/// Regex for detecting @/path/to/image references.
/// Matches @/abs/path/to/img.{png,jpg,...}
const AT_PATH_IMAGE_REGEX: &str = r"(?:^|\s)@(/[^\s'<>]+\.(?:png|jpe?g|gif|webp))";

/// Load image as base64 data URI for Anthropic API.
///
/// Supports:
/// - Local file paths (absolute or file://)
/// - HTTP/HTTPS URLs (with download)
/// - data: URLs (pass-through)
///
/// Returns base64-encoded image data or error message.
fn load_image_as_base64(src: &str) -> Result<String, String> {
    // Handle data: URLs (pass-through)
    if src.starts_with("data:") {
        // Extract the base64 part if it's a data URL
        if let Some(data_start) = src.find(',') {
            return Ok(src[data_start + 1..].to_string());
        }
        return Ok(src.to_string());
    }

    // Handle file:// URLs
    let path = if src.starts_with("file://") {
        &src[7..]
    } else {
        src
    };

    // Handle local file paths
    if path.starts_with('/') || path.starts_with("./") {
        match fs::read(path) {
            Ok(data) => {
                // Detect MIME type from file extension
                let mime_type = detect_mime_type(path);
                let base64 = STANDARD.encode(&data);
                Ok(format!("data:{};base64,{}", mime_type, base64))
            }
            Err(e) => Err(format!("Failed to read file: {}", e)),
        }
    } else if path.starts_with("http://") || path.starts_with("https://") {
        // Download image from HTTP/HTTPS URL
        download_http_image(path)
    } else {
        Err(format!("Unsupported image source: {}", src))
    }
}

/// Detect MIME type from file extension.
fn detect_mime_type(path: &str) -> String {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "image/png", // Default fallback
    }
    .to_string()
}

/// Download image from HTTP/HTTPS URL.
///
/// Uses blocking reqwest client with 30-second timeout.
/// Detects MIME type from Content-Type header or URL extension.
fn download_http_image(url: &str) -> Result<String, String> {
    use reqwest::blocking::Client;

    // Create blocking client with timeout
    let client: Client = match Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("SemaClaw/1.0")
        .build()
    {
        Ok(c) => c,
        Err(e) => return Err(format!("Failed to create HTTP client: {}", e)),
    };

    // Download image
    let response = match client.get(url).send() {
        Ok(r) => r,
        Err(e) => return Err(format!("HTTP request failed: {}", e)),
    };

    // Check response status
    if !response.status().is_success() {
        return Err(format!("HTTP error: {}", response.status()));
    }

    // Get MIME type from Content-Type header before consuming response
    let mime_type: String = response
        .headers()
        .get("content-type")
        .and_then(|v: &reqwest::header::HeaderValue| v.to_str().ok())
        .map(|s: &str| s.split(';').next().unwrap_or(s).to_string())
        .unwrap_or_else(|| detect_mime_type(url));

    // Get image data (this consumes response)
    let bytes = match response.bytes() {
        Ok(b) => b,
        Err(e) => return Err(format!("Failed to read response body: {}", e)),
    };

    // Encode to base64
    let base64 = STANDARD.encode(&bytes);
    Ok(format!("data:{};base64,{}", mime_type, base64))
}

/// Build agent input from prompt and optional attachments.
///
/// # Arguments
/// * `prompt` - User's original text (may contain memory injections, preserved as-is in text block)
/// * `attachments` - Explicitly uploaded attachments from channel/UI
///
/// # Returns
/// * `BuildResult` with processed input, detected image sources, and any failures
pub fn build_agent_input(prompt: &str, attachments: Option<&[ImageAttachment]>) -> BuildResult {
    let explicit_srcs: Vec<String> = attachments
        .unwrap_or(&[])
        .iter()
        .filter(|a| !a.url.is_empty())
        .map(|a| a.url.clone())
        .collect();

    let detected_from_text = detect_images_in_text(prompt);
    let all_srcs = dedupe(&[explicit_srcs, detected_from_text].concat());

    if all_srcs.is_empty() {
        return BuildResult {
            input: Input::Text(prompt.to_string()),
            image_srcs: vec![],
            failures: vec![],
        };
    }

    let mut failures: Vec<String> = vec![];
    let mut image_blocks: Vec<ContentBlock> = vec![];

    for src in &all_srcs {
        match load_image_as_base64(src) {
            Ok(data_uri) => {
                // Parse MIME type from data URI
                let mime_type = if let Some(mime_start) = data_uri.find(';') {
                    data_uri[..mime_start].to_string()
                } else {
                    "image/png".to_string()
                };

                // Extract base64 data
                let base64_data = if let Some(data_start) = data_uri.find(',') {
                    data_uri[data_start + 1..].to_string()
                } else {
                    data_uri.clone()
                };

                image_blocks.push(ContentBlock {
                    block_type: "image".to_string(),
                    text: None,
                    source: Some(ImageSource {
                        source_type: "base64".to_string(),
                        media_type: mime_type,
                        data: base64_data,
                    }),
                });
            }
            Err(e) => {
                failures.push(format!("{} — {}", src, e));
            }
        }
    }

    // Replace @/path references with [image:basename] placeholders
    let cleaned_text = strip_at_path_refs(prompt);

    let mut blocks: Vec<ContentBlock> = vec![];

    if !cleaned_text.trim().is_empty() {
        blocks.push(ContentBlock {
            block_type: "text".to_string(),
            text: Some(cleaned_text),
            source: None,
        });
    }

    for block in image_blocks {
        blocks.push(block);
    }

    if !failures.is_empty() {
        let warning_text = format!(
            "[image-load-warnings]\n{}",
            failures
                .iter()
                .map(|f| format!("- {}", f))
                .collect::<Vec<_>>()
                .join("\n")
        );
        blocks.push(ContentBlock {
            block_type: "text".to_string(),
            text: Some(warning_text),
            source: None,
        });
    }

    let input = if blocks.is_empty() {
        Input::Text(prompt.to_string())
    } else {
        Input::Blocks(blocks)
    };

    BuildResult {
        input,
        image_srcs: all_srcs,
        failures,
    }
}

/// Build agent input from prompt with WebSocket attachments (data_url format).
///
/// This is a convenience function that converts WebSocket image attachments
/// (with data_url and mime_type) to the InputBuilder format.
pub fn build_agent_input_with_attachments(prompt: &str, ws_attachments: &[WebSocketImageAttachment]) -> BuildResult {
    let attachments: Vec<ImageAttachment> = ws_attachments
        .iter()
        .map(|ws_att| ws_att.clone().into())
        .collect();
    build_agent_input(prompt, Some(&attachments))
}

/// Detect image addresses in text (URL + @path format).
fn detect_images_in_text(text: &str) -> Vec<String> {
    let mut found = Vec::new();

    // Detect URL images
    if let Ok(re) = Regex::new(URL_IMAGE_REGEX) {
        for cap in re.captures_iter(text) {
            if let Some(url) = cap.get(1) {
                found.push(url.as_str().to_string());
            }
        }
    }

    // Detect @/path images
    if let Ok(re) = Regex::new(AT_PATH_IMAGE_REGEX) {
        for cap in re.captures_iter(text) {
            if let Some(path) = cap.get(1) {
                found.push(path.as_str().to_string());
            }
        }
    }

    found
}

/// Replace `@/path/to/img.png` with [image:img.png] placeholder.
/// URL formats are preserved as-is for user/history readability.
fn strip_at_path_refs(text: &str) -> String {
    if let Ok(re) = Regex::new(AT_PATH_IMAGE_REGEX) {
        re.replace_all(text, |caps: &regex::Captures| {
            let path = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let name = path
                .split('/')
                .last()
                .unwrap_or("image")
                .to_string();
            format!(" [image:{}]", name)
        })
        .to_string()
    } else {
        text.to_string()
    }
}

/// Remove duplicate strings from a vector.
fn dedupe(arr: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();

    for item in arr {
        if seen.insert(item.clone()) {
            result.push(item.clone());
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_url_images() {
        let text = "Check this image https://example.com/photo.png and this one http://test.org/image.jpg";
        let found = detect_images_in_text(text);
        assert_eq!(found.len(), 2);
        assert!(found.contains(&"https://example.com/photo.png".to_string()));
        assert!(found.contains(&"http://test.org/image.jpg".to_string()));
    }

    #[test]
    fn test_detect_at_path_images() {
        let text = "Look at @/path/to/image.png and @/another/photo.webp";
        let found = detect_images_in_text(text);
        assert_eq!(found.len(), 2);
        assert!(found.contains(&"/path/to/image.png".to_string()));
        assert!(found.contains(&"/another/photo.webp".to_string()));
    }

    #[test]
    fn test_strip_at_path_refs() {
        let text = "Check @/path/to/image.png for details";
        let cleaned = strip_at_path_refs(text);
        assert!(cleaned.contains("[image:image.png]"));
        assert!(!cleaned.contains("@/path/to/image.png"));
    }

    #[test]
    fn test_build_agent_input_no_images() {
        let prompt = "Just plain text without images";
        let result = build_agent_input(prompt, None);
        match result.input {
            Input::Text(text) => assert_eq!(text, prompt),
            Input::Blocks(_) => panic!("Expected Text input"),
        }
        assert!(result.image_srcs.is_empty());
        assert!(result.failures.is_empty());
    }

    #[test]
    fn test_build_agent_input_with_url_images() {
        let prompt = "Check https://example.com/photo.png";
        let result = build_agent_input(prompt, None);
        assert!(!result.image_srcs.is_empty());
        assert!(result.image_srcs.contains(&"https://example.com/photo.png".to_string()));
    }

    #[test]
    fn test_dedupe() {
        let arr = vec![
            "a".to_string(),
            "b".to_string(),
            "a".to_string(),
            "c".to_string(),
            "b".to_string(),
        ];
        let deduped = dedupe(&arr);
        assert_eq!(deduped.len(), 3);
        assert!(deduped.contains(&"a".to_string()));
        assert!(deduped.contains(&"b".to_string()));
        assert!(deduped.contains(&"c".to_string()));
    }

    #[test]
    fn test_build_agent_input_with_attachments() {
        let prompt = "Check this image";
        let attachments = vec![ImageAttachment {
            url: "https://example.com/attached.png".to_string(),
            mime_type: Some("image/png".to_string()),
        }];
        let result = build_agent_input(prompt, Some(&attachments));
        assert!(!result.image_srcs.is_empty());
        assert!(result
            .image_srcs
            .contains(&"https://example.com/attached.png".to_string()));
    }

    #[test]
    fn test_image_attachment_with_empty_url() {
        let prompt = "Text only";
        let attachments = vec![
            ImageAttachment {
                url: "".to_string(),
                mime_type: None,
            },
            ImageAttachment {
                url: "https://example.com/valid.png".to_string(),
                mime_type: None,
            },
        ];
        let result = build_agent_input(prompt, Some(&attachments));
        assert_eq!(result.image_srcs.len(), 1);
        assert!(result
            .image_srcs
            .contains(&"https://example.com/valid.png".to_string()));
    }

    #[test]
    fn test_detect_mime_type() {
        assert_eq!(detect_mime_type("/path/to/image.png"), "image/png");
        assert_eq!(detect_mime_type("/path/to/photo.jpg"), "image/jpeg");
        assert_eq!(detect_mime_type("/path/to/photo.jpeg"), "image/jpeg");
        assert_eq!(detect_mime_type("/path/to/anim.gif"), "image/gif");
        assert_eq!(detect_mime_type("/path/to/img.webp"), "image/webp");
        assert_eq!(detect_mime_type("/path/to/unknown.xyz"), "image/png"); // default
    }

    #[test]
    fn test_load_image_as_base64_data_url() {
        // Test data URL pass-through
        let data_url = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";
        let result = load_image_as_base64(data_url);
        assert!(result.is_ok());
        assert!(result.unwrap().starts_with("iVBORw0KGgo"));
    }

    #[test]
    fn test_load_image_as_base64_file_not_found() {
        let result = load_image_as_base64("/nonexistent/path/to/image.png");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to read file"));
    }

    #[test]
    fn test_build_agent_input_with_local_file() {
        // This test requires a real file, so we'll skip it for now
        // In a real integration test, we would create a temporary image file
        let prompt = "Check @/tmp/test.png";
        let result = build_agent_input(prompt, None);
        // Should detect the file reference even if it doesn't exist
        assert!(!result.image_srcs.is_empty());
        assert!(result.image_srcs.contains(&"/tmp/test.png".to_string()));
        // Should have a failure since the file doesn't exist
        assert!(!result.failures.is_empty());
    }

    #[test]
    fn test_download_http_image_invalid_url() {
        // Test with invalid URL format
        let result = download_http_image("not-a-url");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("HTTP request failed"));
    }

    #[test]
    fn test_download_http_image_timeout() {
        // Test with a URL that will timeout (using a non-existent slow server)
        // This test may be flaky, so we'll use a localhost port that's unlikely to be open
        let result = download_http_image("http://localhost:99999/nonexistent.png");
        // Should fail due to connection error or timeout
        assert!(result.is_err());
    }

    #[test]
    fn test_download_http_image_404() {
        // Test with a valid URL but 404 response
        // Using httpbin.org for testing HTTP errors
        let result = download_http_image("https://httpbin.org/status/404");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("HTTP error"));
    }

    #[test]
    #[ignore] // This test requires network access and may be flaky
    fn test_download_http_image_success() {
        // Test with a real image URL (using a small test image)
        // This test is ignored by default as it requires network access
        let result = download_http_image("https://httpbin.org/image/png");
        assert!(result.is_ok());
        let data_uri = result.unwrap();
        assert!(data_uri.starts_with("data:image/png;base64,"));
    }
}
