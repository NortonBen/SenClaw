//! Official **Shopee Open Platform v2** REST client.
//!
//! This is the *only* path this app uses to reach Shopee — the sanctioned
//! partner API at `partner.shopeemobile.com`. It is NOT the internal web API
//! (`shopee.vn/api/v4`), and it deliberately does not try to defeat any
//! anti-bot: the official gateway has none for authorized partners, only a
//! public rate limit. See `docs/shopee-app-research.md` for why this is the
//! only path we build.
//!
//! ## Auth model (per-shop OAuth)
//!
//! 1. Register a Partner App on <https://open.shopee.com> → get `partner_id` +
//!    `partner_key`.
//! 2. Build an **authorize link** (valid 5 minutes) → the seller approves →
//!    Shopee redirects back with `?code=...&shop_id=...`.
//! 3. Exchange `code` → `access_token` (~4h) + `refresh_token` (~30 days).
//! 4. Every shop-scoped call is signed HMAC-SHA256 over
//!    `partner_id + path + timestamp + access_token + shop_id`.
//!
//! `partner_key` and the tokens live only in this app's local SQLite DB and are
//! only ever sent to the configured Shopee host.

use anyhow::{anyhow, Result};
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};

type HmacSha256 = Hmac<Sha256>;

/// Live host. UAT/test uses `partner.test-stable.shopeemobile.com`; keep it
/// configurable so `sign` errors ("environment mismatch") are debuggable.
pub const DEFAULT_HOST: &str = "https://partner.shopeemobile.com";

/// Static partner credentials + which shop we're acting for.
#[derive(Clone, Debug)]
pub struct Config {
    pub host: String,
    pub partner_id: i64,
    pub partner_key: String,
    pub shop_id: i64,
}

impl Config {
    pub fn is_complete(&self) -> bool {
        self.partner_id != 0 && !self.partner_key.is_empty()
    }
}

fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// HMAC-SHA256(partner_key, base_string) as lowercase hex — Shopee's `sign`.
fn hmac_hex(partner_key: &str, base_string: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(partner_key.as_bytes())
        .expect("HMAC accepts a key of any length");
    mac.update(base_string.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// A signed Shopee API client for one shop.
pub struct Client {
    cfg: Config,
    http: reqwest::Client,
}

impl Client {
    pub fn new(cfg: Config) -> Self {
        Self {
            cfg,
            http: reqwest::Client::new(),
        }
    }

    // ---- OAuth (public, unsigned-token endpoints) ----

    /// The authorize URL the seller opens to grant this partner access. Signed
    /// over `partner_id + path + timestamp` (no token/shop yet). Valid 5 min.
    pub fn authorize_link(&self, redirect: &str) -> String {
        let path = "/api/v2/shop/auth_partner";
        let ts = now_ts();
        let base = format!("{}{}{}", self.cfg.partner_id, path, ts);
        let sign = hmac_hex(&self.cfg.partner_key, &base);
        format!(
            "{host}{path}?partner_id={pid}&timestamp={ts}&sign={sign}&redirect={redirect}",
            host = self.cfg.host,
            pid = self.cfg.partner_id,
        )
    }

    /// Exchange the `code` from the redirect for an access + refresh token.
    /// `POST /api/v2/auth/token/get`, signed over `partner_id + path + timestamp`.
    pub async fn token_by_code(&self, code: &str, shop_id: i64) -> Result<TokenResponse> {
        let path = "/api/v2/auth/token/get";
        let ts = now_ts();
        let base = format!("{}{}{}", self.cfg.partner_id, path, ts);
        let sign = hmac_hex(&self.cfg.partner_key, &base);
        let url = format!(
            "{}{}?partner_id={}&timestamp={}&sign={}",
            self.cfg.host, path, self.cfg.partner_id, ts, sign
        );
        let body = json!({ "code": code, "shop_id": shop_id, "partner_id": self.cfg.partner_id });
        let v = self.post_json(&url, body).await?;
        TokenResponse::from_value(&v)
    }

    /// Refresh an expiring access token.
    /// `POST /api/v2/auth/access_token/get`.
    pub async fn refresh_token(&self, refresh_token: &str, shop_id: i64) -> Result<TokenResponse> {
        let path = "/api/v2/auth/access_token/get";
        let ts = now_ts();
        let base = format!("{}{}{}", self.cfg.partner_id, path, ts);
        let sign = hmac_hex(&self.cfg.partner_key, &base);
        let url = format!(
            "{}{}?partner_id={}&timestamp={}&sign={}",
            self.cfg.host, path, self.cfg.partner_id, ts, sign
        );
        let body = json!({
            "refresh_token": refresh_token,
            "shop_id": shop_id,
            "partner_id": self.cfg.partner_id,
        });
        let v = self.post_json(&url, body).await?;
        TokenResponse::from_value(&v)
    }

    // ---- Shop-scoped signed calls ----

    /// Build the signed query string for a shop-scoped call. Base string is
    /// `partner_id + path + timestamp + access_token + shop_id`.
    fn signed_query(&self, path: &str, access_token: &str) -> String {
        let ts = now_ts();
        let base = format!(
            "{}{}{}{}{}",
            self.cfg.partner_id, path, ts, access_token, self.cfg.shop_id
        );
        let sign = hmac_hex(&self.cfg.partner_key, &base);
        format!(
            "partner_id={}&timestamp={}&access_token={}&shop_id={}&sign={}",
            self.cfg.partner_id, ts, access_token, self.cfg.shop_id, sign
        )
    }

    /// GET a shop-scoped endpoint. `extra` are additional query params.
    pub async fn get(
        &self,
        path: &str,
        access_token: &str,
        extra: &[(&str, String)],
    ) -> Result<Value> {
        let mut qs = self.signed_query(path, access_token);
        for (k, v) in extra {
            qs.push('&');
            qs.push_str(k);
            qs.push('=');
            qs.push_str(v);
        }
        let url = format!("{}{}?{}", self.cfg.host, path, qs);
        let v: Value = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| anyhow!("GET {path} failed: {e}"))?
            .json()
            .await
            .map_err(|e| anyhow!("GET {path} bad json: {e}"))?;
        check_api_error(&v)
    }

    /// POST a shop-scoped endpoint with a JSON body.
    pub async fn post(&self, path: &str, access_token: &str, body: Value) -> Result<Value> {
        let qs = self.signed_query(path, access_token);
        let url = format!("{}{}?{}", self.cfg.host, path, qs);
        let v = self.post_json(&url, body).await?;
        check_api_error(&v)
    }

    async fn post_json(&self, url: &str, body: Value) -> Result<Value> {
        self.http
            .post(url)
            .json(&body)
            .send()
            .await
            .map_err(|e| anyhow!("POST failed: {e}"))?
            .json()
            .await
            .map_err(|e| anyhow!("POST bad json: {e}"))
    }

    // ---- Thin typed wrappers over the endpoints this app uses ----

    /// Basic shop profile — a cheap call to confirm the token works.
    pub async fn get_shop_info(&self, access_token: &str) -> Result<Value> {
        self.get("/api/v2/shop/get_shop_info", access_token, &[])
            .await
    }

    /// Recent orders. `time_from`/`time_to` are unix seconds (≤15-day window).
    pub async fn get_order_list(
        &self,
        access_token: &str,
        time_from: i64,
        time_to: i64,
    ) -> Result<Value> {
        self.get(
            "/api/v2/order/get_order_list",
            access_token,
            &[
                ("time_range_field", "create_time".into()),
                ("time_from", time_from.to_string()),
                ("time_to", time_to.to_string()),
                ("page_size", "50".into()),
            ],
        )
        .await
    }

    /// Full detail for up to 50 orders by `order_sn`. Used to ground a CSKH
    /// reply in the real order (status, total, items) instead of guessing.
    pub async fn get_order_detail(
        &self,
        access_token: &str,
        order_sns: &[String],
    ) -> Result<Value> {
        let list = order_sns.join(",");
        self.get(
            "/api/v2/order/get_order_detail",
            access_token,
            &[
                ("order_sn_list", list),
                (
                    "response_optional_fields",
                    "order_status,total_amount,item_list,recipient_address,tracking_number".into(),
                ),
            ],
        )
        .await
    }

    /// Buyer↔seller conversations for this shop (Chat API — needs the separate
    /// Chat permission on the partner app).
    pub async fn get_conversation_list(&self, access_token: &str) -> Result<Value> {
        self.get(
            "/api/v2/sellerchat/get_conversation_list",
            access_token,
            &[
                ("direction", "latest".into()),
                ("type", "all".into()),
                ("page_size", "25".into()),
            ],
        )
        .await
    }

    /// Messages of one conversation.
    pub async fn get_message_list(
        &self,
        access_token: &str,
        conversation_id: &str,
    ) -> Result<Value> {
        self.get(
            "/api/v2/sellerchat/get_message",
            access_token,
            &[
                ("conversation_id", conversation_id.to_string()),
                ("page_size", "30".into()),
            ],
        )
        .await
    }

    /// Send a text reply to a buyer. This is the ONLY messaging this app does:
    /// a reply to a customer of *this* shop, gated by the draft-approve queue in
    /// the caller. There is no mass/broadcast messaging endpoint and we add none.
    pub async fn send_message(&self, access_token: &str, to_id: i64, text: &str) -> Result<Value> {
        self.post(
            "/api/v2/sellerchat/send_message",
            access_token,
            json!({
                "to_id": to_id,
                "message_type": "text",
                "content": { "text": text },
            }),
        )
        .await
    }

    // ---- Product API (manage the seller's OWN listings) ----

    /// Paginated list of the shop's items. `status` is one of NORMAL / BANNED /
    /// UNLIST / REVIEWING (Shopee `item_status`).
    pub async fn get_item_list(
        &self,
        access_token: &str,
        offset: i64,
        page_size: i64,
        status: &str,
    ) -> Result<Value> {
        self.get(
            "/api/v2/product/get_item_list",
            access_token,
            &[
                ("offset", offset.to_string()),
                ("page_size", page_size.clamp(1, 100).to_string()),
                ("item_status", status.to_string()),
            ],
        )
        .await
    }

    /// Full info for up to 50 items. `item_ids` are joined into `item_id_list`.
    pub async fn get_item_base_info(&self, access_token: &str, item_ids: &[i64]) -> Result<Value> {
        let list = item_ids
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",");
        self.get(
            "/api/v2/product/get_item_base_info",
            access_token,
            &[("item_id_list", list)],
        )
        .await
    }

    /// Update the stock of one item (single-variant shape: `model_id = 0`).
    /// A write to the seller's own shop — exposed as an explicit action, never
    /// automated by the heartbeat.
    pub async fn update_stock(
        &self,
        access_token: &str,
        item_id: i64,
        stock: i64,
    ) -> Result<Value> {
        self.post(
            "/api/v2/product/update_stock",
            access_token,
            json!({
                "item_id": item_id,
                "stock_list": [ { "model_id": 0, "seller_stock": [ { "stock": stock } ] } ],
            }),
        )
        .await
    }

    /// Update the price of one item (single-variant shape: `model_id = 0`).
    pub async fn update_price(
        &self,
        access_token: &str,
        item_id: i64,
        price: f64,
    ) -> Result<Value> {
        self.post(
            "/api/v2/product/update_price",
            access_token,
            json!({
                "item_id": item_id,
                "price_list": [ { "model_id": 0, "original_price": price } ],
            }),
        )
        .await
    }
}

/// Shopee wraps every response in `{ error, message, request_id, response }`.
/// A non-empty `error` string is a failure.
fn check_api_error(v: &Value) -> Result<Value> {
    let err = v.get("error").and_then(|x| x.as_str()).unwrap_or("");
    if err.is_empty() {
        Ok(v.get("response").cloned().unwrap_or_else(|| v.clone()))
    } else {
        let msg = v.get("message").and_then(|x| x.as_str()).unwrap_or("");
        Err(anyhow!("Shopee API error [{err}]: {msg}"))
    }
}

/// Parsed token-exchange / refresh result.
#[derive(Debug, Clone)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    /// Seconds until the access token expires (Shopee returns `expire_in`).
    pub expire_in: i64,
}

impl TokenResponse {
    fn from_value(v: &Value) -> Result<Self> {
        let err = v.get("error").and_then(|x| x.as_str()).unwrap_or("");
        if !err.is_empty() {
            let msg = v.get("message").and_then(|x| x.as_str()).unwrap_or("");
            return Err(anyhow!("Shopee auth error [{err}]: {msg}"));
        }
        let access_token = v
            .get("access_token")
            .and_then(|x| x.as_str())
            .ok_or_else(|| anyhow!("no access_token in response"))?
            .to_string();
        let refresh_token = v
            .get("refresh_token")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let expire_in = v.get("expire_in").and_then(|x| x.as_i64()).unwrap_or(14400);
        Ok(Self {
            access_token,
            refresh_token,
            expire_in,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        Config {
            host: DEFAULT_HOST.into(),
            partner_id: 200123,
            partner_key: "shpk_test_key".into(),
            shop_id: 55667788,
        }
    }

    #[test]
    fn sign_is_deterministic_and_hex() {
        // Same inputs → same sign (the whole point — a mismatch is the #1 Shopee error).
        let base = "200123/api/v2/shop/get_shop_info1700000000tok55667788";
        let a = hmac_hex(&cfg().partner_key, base);
        let b = hmac_hex(&cfg().partner_key, base);
        assert_eq!(a, b);
        assert_eq!(a.len(), 64); // sha256 hex
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn authorize_link_has_required_params() {
        let link = Client::new(cfg()).authorize_link("https://example.com/cb");
        assert!(link.contains("/api/v2/shop/auth_partner"));
        assert!(link.contains("partner_id=200123"));
        assert!(link.contains("sign="));
        assert!(link.contains("redirect=https://example.com/cb"));
    }

    #[test]
    fn item_base_info_joins_ids() {
        // Regression: item_id_list must be a comma-joined string, no spaces.
        let ids = [111i64, 222, 333];
        let joined = ids
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",");
        assert_eq!(joined, "111,222,333");
    }

    #[test]
    fn order_sn_list_joins() {
        let sns = vec!["2506ABC".to_string(), "2506XYZ".to_string()];
        assert_eq!(sns.join(","), "2506ABC,2506XYZ");
    }

    #[test]
    fn api_error_is_surfaced() {
        let ok = json!({ "error": "", "response": { "shop_name": "X" } });
        assert!(check_api_error(&ok).is_ok());
        let bad = json!({ "error": "error_sign", "message": "wrong sign" });
        assert!(check_api_error(&bad).is_err());
    }
}
