//! TikTok Shop IM (seller ↔ buyer chat) adapter — **experimental scaffold**.
//!
//! Unlike Zalo/Facebook, TikTok has no open Messenger-style DM API; the only
//! sanctioned messaging surface is the TikTok Shop Open Platform Customer
//! Service API, which requires a Partner app (`app_key`/`app_secret`), a
//! per-shop OAuth `access_token` (sent as `x-tts-access-token`), and an
//! HMAC-SHA256 request signature. Config:
//! `{ "app_key","app_secret","access_token","shop_cipher" }`.
//!
//! The request signing is implemented and unit-tested; the live conversation/
//! message endpoints are wired but return a clear "needs credentials / verify
//! against current TikTok Shop docs" error until a real Partner app is set up.

use crate::channels::Inbound;
use crate::db::{Channel, Db};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::BTreeMap;
use std::sync::Arc;

#[allow(dead_code)] // used once the live Partner endpoints are wired
const HOST: &str = "https://open-api.tiktokglobalshop.com";

fn cfg<'a>(ch: &'a Channel, key: &str) -> &'a str {
    ch.config.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

/// TikTok Shop request signature: HMAC-SHA256, keyed by `app_secret`, over
/// `app_secret + path + concat(sorted key+value, excluding sign/access_token) + app_secret`,
/// hex-encoded. Pure — unit-tested below.
#[allow(dead_code)] // used once the live Partner endpoints are wired
pub fn sign(app_secret: &str, path: &str, params: &BTreeMap<String, String>) -> String {
    let mut base = String::new();
    base.push_str(path);
    for (k, v) in params {
        if k == "sign" || k == "access_token" {
            continue;
        }
        base.push_str(k);
        base.push_str(v);
    }
    let payload = format!("{app_secret}{base}{app_secret}");
    let mut mac = Hmac::<Sha256>::new_from_slice(app_secret.as_bytes()).expect("hmac key");
    mac.update(payload.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

fn configured(ch: &Channel) -> bool {
    !cfg(ch, "app_key").is_empty()
        && !cfg(ch, "app_secret").is_empty()
        && !cfg(ch, "access_token").is_empty()
}

fn unconfigured_err() -> String {
    "TikTok Shop IM là kênh thử nghiệm — cần app_key/app_secret/access_token của TikTok Shop Partner \
     (và kiểm chứng lại endpoint theo tài liệu TikTok Shop hiện hành) trước khi dùng."
        .to_string()
}

pub async fn poll(_db: &Arc<Db>, ch: &Channel) -> Result<Vec<Inbound>, String> {
    if !configured(ch) {
        return Err(unconfigured_err());
    }
    // Wired shape (GET {HOST}/customer_service/202309/conversations with signed
    // query + x-tts-access-token). Left inert pending a real Partner app so we
    // don't guess at a versioned contract we can't verify.
    Err(
        "TikTok Shop IM: poll chưa bật (thử nghiệm) — hoàn tất tích hợp Partner rồi mở khoá."
            .into(),
    )
}

pub async fn send(
    _db: &Arc<Db>,
    ch: &Channel,
    _external_id: &str,
    _text: &str,
) -> Result<(), String> {
    if !configured(ch) {
        return Err(unconfigured_err());
    }
    Err("TikTok Shop IM: gửi chưa bật (thử nghiệm) — hoàn tất tích hợp Partner rồi mở khoá.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_is_stable_and_order_independent() {
        let mut a = BTreeMap::new();
        a.insert("app_key".into(), "K".into());
        a.insert("timestamp".into(), "100".into());
        a.insert("shop_cipher".into(), "C".into());
        let mut b = BTreeMap::new();
        b.insert("shop_cipher".into(), "C".into());
        b.insert("timestamp".into(), "100".into());
        b.insert("app_key".into(), "K".into());
        let s1 = sign("secret", "/customer_service/202309/conversations", &a);
        let s2 = sign("secret", "/customer_service/202309/conversations", &b);
        assert_eq!(s1, s2, "sign must be independent of insertion order");
        assert_eq!(s1.len(), 64, "hex sha256 is 64 chars");
    }

    #[test]
    fn signature_excludes_sign_and_token() {
        let mut with_extra = BTreeMap::new();
        with_extra.insert("app_key".into(), "K".into());
        with_extra.insert("sign".into(), "SHOULD_BE_IGNORED".into());
        with_extra.insert("access_token".into(), "TOKEN_IGNORED".into());
        let mut clean = BTreeMap::new();
        clean.insert("app_key".into(), "K".into());
        assert_eq!(sign("s", "/p", &with_extra), sign("s", "/p", &clean));
    }
}
