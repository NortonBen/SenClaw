//! Shared request signer, reused by the official-API platform paths.
//!
//! Lifted from the finished, unit-tested TikTok Shop signer in
//! `apps/crm/src/channels/tiktok.rs`: HMAC-SHA256 keyed by the app secret over
//! `secret + path + concat(sorted key+value, excluding sign/access_token) + secret`,
//! hex-encoded. Pure.

use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::BTreeMap;

#[allow(dead_code)] // used by tests + the official-API signing paths as they land
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_is_stable_and_order_independent() {
        let mut a = BTreeMap::new();
        a.insert("app_key".into(), "K".into());
        a.insert("timestamp".into(), "100".into());
        let mut b = BTreeMap::new();
        b.insert("timestamp".into(), "100".into());
        b.insert("app_key".into(), "K".into());
        let s1 = sign("secret", "/p", &a);
        let s2 = sign("secret", "/p", &b);
        assert_eq!(s1, s2);
        assert_eq!(s1.len(), 64);
    }

    #[test]
    fn signature_excludes_sign_and_token() {
        let mut with_extra = BTreeMap::new();
        with_extra.insert("app_key".into(), "K".into());
        with_extra.insert("sign".into(), "IGNORED".into());
        with_extra.insert("access_token".into(), "IGNORED".into());
        let mut clean = BTreeMap::new();
        clean.insert("app_key".into(), "K".into());
        assert_eq!(sign("s", "/p", &with_extra), sign("s", "/p", &clean));
    }
}
