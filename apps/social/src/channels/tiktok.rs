//! TikTok — official Content Posting API path.
//!
//! Real flow (once wired): init a direct-post upload via
//! `POST https://open.tiktokapi.com/v2/post/publish/video/init/` with a
//! `video.publish`-scoped user access token, upload the file, then poll status.
//! Limits ~15–25 videos/day, 6 req/min. There is NO third-party DM API; the
//! TikTok Shop Customer Service API (see `crate::channels::sign`) is the only
//! sanctioned messaging surface and is Shop-only.

use serde_json::Value;

fn cfg<'a>(c: &'a Value, key: &str) -> &'a str {
    c.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

fn configured(c: &Value) -> bool {
    !cfg(c, "access_token").is_empty()
}

pub fn official_post(c: &Value, _text: &str) -> Result<String, String> {
    if !configured(c) {
        return Err("TikTok: cần access_token (scope video.publish, app đã được TikTok duyệt) trong official_config trước khi đăng qua Content Posting API.".into());
    }
    Err("TikTok: đăng qua Content Posting API chưa bật (scaffold) — nối init/upload/status rồi mở khoá; nhớ tôn trọng hạn mức ~15–25/ngày.".into())
}
