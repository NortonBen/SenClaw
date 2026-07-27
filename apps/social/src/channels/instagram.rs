//! Instagram — official Graph API path (IG Business/Creator publishing).
//!
//! Real flow (once wired): create a media container via
//! `POST https://graph.facebook.com/v21.0/{ig_user_id}/media` then publish with
//! `/{ig_user_id}/media_publish`. Requires an IG Business/Creator account linked
//! to a Facebook Page. DM only via Messenger Platform for Business accounts.

use serde_json::Value;

fn cfg<'a>(c: &'a Value, key: &str) -> &'a str {
    c.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

fn configured(c: &Value) -> bool {
    !cfg(c, "ig_user_id").is_empty() && !cfg(c, "access_token").is_empty()
}

pub fn official_post(c: &Value, _text: &str) -> Result<String, String> {
    if !configured(c) {
        return Err("Instagram: cần ig_user_id + access_token (IG Business/Creator liên kết Facebook Page) trong official_config trước khi đăng qua Graph API.".into());
    }
    Err("Instagram: đăng qua Graph API chưa bật (scaffold) — nối media + media_publish rồi mở khoá.".into())
}
