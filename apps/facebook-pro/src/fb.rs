//! Official **Facebook Graph API** client.
//!
//! This is the *only* path this app uses to reach Facebook — the sanctioned
//! Graph API at `graph.facebook.com`, driven by the user's own **Facebook
//! Developer App** (App ID + App Secret). It is NOT the internal web API and it
//! deliberately does not try to defeat any anti-bot: authorized apps don't hit
//! one, only a public rate limit.
//!
//! ## Auth model (three-tier token, the official flow)
//!
//! 1. Register a Developer App at <https://developers.facebook.com/apps> → get
//!    `app_id` + `app_secret`.
//! 2. OAuth (or a Graph API Explorer token) → a **short-lived user token**.
//! 3. Exchange it for a **long-lived user token** (~60 days).
//! 4. `GET /me/accounts` returns the admin's **Pages**, each with a **Page Access
//!    Token** (effectively permanent while the user token is valid). Page tokens
//!    are what post/read on a Page.
//!
//! Every call carries `appsecret_proof = HMAC-SHA256(app_secret, access_token)`
//! (hex) — Facebook's tamper check. `app_secret` and all tokens live only in this
//! app's local SQLite and are only ever sent to `graph.facebook.com`.

use anyhow::{anyhow, Result};
use hmac::{Hmac, Mac};
use serde_json::{json, Value};

type HmacSha256 = Hmac<Sha256>;
use sha2::Sha256;

pub const DEFAULT_VERSION: &str = "v21.0";
pub const GRAPH_HOST: &str = "https://graph.facebook.com";
pub const WWW_HOST: &str = "https://www.facebook.com";

/// The scopes this app requests. Page-scoped + insights + messaging + ads.
pub const SCOPES: &str = "pages_show_list,pages_manage_posts,pages_read_engagement,pages_manage_engagement,pages_read_user_content,pages_messaging,read_insights,ads_read,ads_management";

/// Ad Insights fields shared across levels (spend + the CTR/CPC/CPM metrics the
/// user cares about, plus results/ROAS for the "worth it?" verdict).
const ADS_BASE_FIELDS: &str = "impressions,clicks,spend,ctr,cpc,cpm,reach,frequency,actions,cost_per_action_type,purchase_roas";

/// Static developer-app credentials.
#[derive(Clone, Debug)]
pub struct Config {
    pub app_id: String,
    pub app_secret: String,
    /// Graph API version, e.g. `v21.0`.
    pub version: String,
}

/// `appsecret_proof` — HMAC-SHA256(app_secret, access_token) as lowercase hex.
pub fn appsecret_proof(app_secret: &str, access_token: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(app_secret.as_bytes())
        .expect("HMAC accepts a key of any length");
    mac.update(access_token.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// A Graph API client for one developer app.
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

    fn base(&self) -> String {
        format!("{}/{}", GRAPH_HOST, self.cfg.version)
    }

    // ---- OAuth ----

    /// The Facebook Login dialog URL the admin opens to grant this app access.
    pub fn connect_url(&self, redirect: &str) -> String {
        format!(
            "{host}/{ver}/dialog/oauth?client_id={id}&redirect_uri={redirect}&scope={scope}&response_type=code",
            host = WWW_HOST,
            ver = self.cfg.version,
            id = urlencode(&self.cfg.app_id),
            redirect = urlencode(redirect),
            scope = urlencode(SCOPES),
        )
    }

    /// Exchange an OAuth `code` (from the redirect) for a user access token.
    pub async fn token_by_code(&self, code: &str, redirect: &str) -> Result<String> {
        let url = format!(
            "{}/oauth/access_token?client_id={}&redirect_uri={}&client_secret={}&code={}",
            self.base(),
            urlencode(&self.cfg.app_id),
            urlencode(redirect),
            urlencode(&self.cfg.app_secret),
            urlencode(code),
        );
        let v = self.get_url(&url).await?;
        v.get("access_token")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("no access_token in code exchange"))
    }

    /// Exchange a short-lived user token for a long-lived (~60 day) one. If
    /// Facebook rejects it (e.g. the token is already long-lived), the caller
    /// falls back to using the original token.
    pub async fn exchange_long_lived(&self, short_token: &str) -> Result<String> {
        let url = format!(
            "{}/oauth/access_token?grant_type=fb_exchange_token&client_id={}&client_secret={}&fb_exchange_token={}",
            self.base(),
            urlencode(&self.cfg.app_id),
            urlencode(&self.cfg.app_secret),
            urlencode(short_token),
        );
        let v = self.get_url(&url).await?;
        v.get("access_token")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("no access_token in long-lived exchange"))
    }

    /// The Pages this user administers, each with its Page Access Token.
    pub async fn get_pages(&self, user_token: &str) -> Result<Value> {
        self.get(
            "/me/accounts",
            user_token,
            &[("fields", "id,name,access_token,category,tasks".into())],
        )
        .await
    }

    // ---- Read ----

    /// Recent published posts on a Page.
    pub async fn list_posts(&self, page_id: &str, page_token: &str, limit: i64) -> Result<Value> {
        self.get(
            &format!("/{page_id}/posts"),
            page_token,
            &[
                ("fields", "id,message,created_time,permalink_url,shares,comments.summary(true),reactions.summary(true)".into()),
                ("limit", limit.clamp(1, 50).to_string()),
            ],
        )
        .await
    }

    /// One post with its content + engagement summary.
    pub async fn get_post(&self, post_id: &str, page_token: &str) -> Result<Value> {
        self.get(
            &format!("/{post_id}"),
            page_token,
            &[("fields", "id,message,story,created_time,permalink_url,shares,comments.summary(true),reactions.summary(true)".into())],
        )
        .await
    }

    /// Comments on a post (or replies on a comment).
    pub async fn list_comments(
        &self,
        object_id: &str,
        page_token: &str,
        limit: i64,
    ) -> Result<Value> {
        self.get(
            &format!("/{object_id}/comments"),
            page_token,
            &[
                (
                    "fields",
                    "id,message,from,created_time,like_count,comment_count".into(),
                ),
                ("order", "reverse_chronological".into()),
                ("limit", limit.clamp(1, 100).to_string()),
            ],
        )
        .await
    }

    // ---- Messaging (Page inbox — needs pages_messaging) ----

    /// The Page's Messenger conversations (most recently updated first).
    pub async fn list_conversations(
        &self,
        page_id: &str,
        page_token: &str,
        limit: i64,
    ) -> Result<Value> {
        self.get(
            &format!("/{page_id}/conversations"),
            page_token,
            &[
                ("platform", "messenger".into()),
                (
                    "fields",
                    "id,snippet,updated_time,message_count,unread_count,participants".into(),
                ),
                ("limit", limit.clamp(1, 50).to_string()),
            ],
        )
        .await
    }

    /// Messages inside one conversation (thread), newest last.
    pub async fn conversation_messages(
        &self,
        conversation_id: &str,
        page_token: &str,
        limit: i64,
    ) -> Result<Value> {
        self.get(
            &format!("/{conversation_id}"),
            page_token,
            &[(
                "fields",
                format!(
                    "messages.limit({}){{id,message,from,created_time}}",
                    limit.clamp(1, 50)
                ),
            )],
        )
        .await
    }

    /// Send a text message to a user via the Send API (a RESPONSE to the user's
    /// own message — no broadcasting). `recipient_psid` is the user's page-scoped id.
    pub async fn send_message(
        &self,
        page_id: &str,
        page_token: &str,
        recipient_psid: &str,
        text: &str,
    ) -> Result<Value> {
        self.post(
            &format!("/{page_id}/messages"),
            page_token,
            json!({
                "recipient": { "id": recipient_psid },
                "messaging_type": "RESPONSE",
                "message": { "text": text },
            }),
        )
        .await
    }

    /// Page-level insights. `metrics` is a comma-joined metric list.
    pub async fn page_insights(
        &self,
        page_id: &str,
        page_token: &str,
        metrics: &str,
        period: &str,
    ) -> Result<Value> {
        self.get(
            &format!("/{page_id}/insights"),
            page_token,
            &[
                ("metric", metrics.to_string()),
                ("period", period.to_string()),
            ],
        )
        .await
    }

    /// Post-level insights.
    pub async fn post_insights(
        &self,
        post_id: &str,
        page_token: &str,
        metrics: &str,
    ) -> Result<Value> {
        self.get(
            &format!("/{post_id}/insights"),
            page_token,
            &[("metric", metrics.to_string())],
        )
        .await
    }

    // ---- Marketing API (Ads Insights) — read with the USER token + ads_read ----

    /// The Ad Accounts this user can access. `user_token` must carry `ads_read`.
    pub async fn get_ad_accounts(&self, user_token: &str) -> Result<Value> {
        self.get(
            "/me/adaccounts",
            user_token,
            &[(
                "fields",
                "account_id,id,name,account_status,currency,amount_spent".into(),
            )],
        )
        .await
    }

    /// Campaigns of an ad account (`act_<id>`), with status/objective/budget.
    pub async fn list_campaigns(&self, act_id: &str, user_token: &str) -> Result<Value> {
        self.get(
            &format!("/{act_id}/campaigns"),
            user_token,
            &[
                (
                    "fields",
                    "id,name,status,effective_status,objective,daily_budget,lifetime_budget".into(),
                ),
                ("limit", "50".into()),
            ],
        )
        .await
    }

    /// Ad Insights for an object — an ad account (`act_<id>`), campaign, adset, or
    /// ad id — broken down by `level` (account|campaign|adset|ad). `date_preset`
    /// is e.g. `last_7d`, `last_30d`, `today`, `maximum`. Returns rows with
    /// impressions/clicks/spend/ctr/cpc/cpm/reach + actions + purchase_roas.
    pub async fn ad_insights(
        &self,
        object_id: &str,
        user_token: &str,
        level: &str,
        date_preset: &str,
    ) -> Result<Value> {
        let name_field = match level {
            "campaign" => ",campaign_name",
            "adset" => ",adset_name",
            "ad" => ",ad_name",
            _ => "",
        };
        let fields = format!("{ADS_BASE_FIELDS}{name_field}");
        self.get(
            &format!("/{object_id}/insights"),
            user_token,
            &[
                ("fields", fields),
                ("level", level.to_string()),
                ("date_preset", date_preset.to_string()),
                ("limit", "100".into()),
            ],
        )
        .await
    }

    /// Pause or resume a campaign / adset / ad by setting its `status`
    /// (ACTIVE | PAUSED). Needs `ads_management`. This is the seller's OWN ad
    /// entity — an explicit action, never automated.
    pub async fn set_entity_status(
        &self,
        entity_id: &str,
        user_token: &str,
        status: &str,
    ) -> Result<Value> {
        self.post(
            &format!("/{entity_id}"),
            user_token,
            json!({ "status": status }),
        )
        .await
    }

    // ---- Write (all gated by the caller's draft-approve queue) ----

    /// Publish a text/link post to a Page. Returns `{ "id": "<post-id>" }`.
    pub async fn create_post(
        &self,
        page_id: &str,
        page_token: &str,
        message: &str,
        link: Option<&str>,
    ) -> Result<Value> {
        let mut body = json!({ "message": message });
        if let Some(l) = link {
            if !l.trim().is_empty() {
                body["link"] = json!(l);
            }
        }
        self.post(&format!("/{page_id}/feed"), page_token, body)
            .await
    }

    /// Publish a photo post by image URL. Returns `{ "id", "post_id" }`.
    pub async fn create_photo(
        &self,
        page_id: &str,
        page_token: &str,
        image_url: &str,
        caption: &str,
    ) -> Result<Value> {
        self.post(
            &format!("/{page_id}/photos"),
            page_token,
            json!({ "url": image_url, "caption": caption }),
        )
        .await
    }

    /// Publish a photo post from LOCAL image bytes via multipart `source` (for
    /// files the user uploaded, not a public URL). Returns `{ "id", "post_id" }`.
    pub async fn create_photo_bytes(
        &self,
        page_id: &str,
        page_token: &str,
        bytes: Vec<u8>,
        filename: &str,
        mime: &str,
        caption: &str,
    ) -> Result<Value> {
        let proof = appsecret_proof(&self.cfg.app_secret, page_token);
        let part = reqwest::multipart::Part::bytes(bytes)
            .file_name(filename.to_string())
            .mime_str(mime)
            .unwrap_or_else(|_| reqwest::multipart::Part::text(""));
        let form = reqwest::multipart::Form::new()
            .text("caption", caption.to_string())
            .text("access_token", page_token.to_string())
            .text("appsecret_proof", proof)
            .part("source", part);
        let url = format!("{}/{}/photos", self.base(), page_id);
        let v: Value = self
            .http
            .post(&url)
            .multipart(form)
            .send()
            .await
            .map_err(|e| anyhow!("upload photo failed: {e}"))?
            .json()
            .await
            .map_err(|e| anyhow!("upload photo bad json: {e}"))?;
        check_api_error(v)
    }

    /// Edit a post's message.
    pub async fn edit_post(&self, post_id: &str, page_token: &str, message: &str) -> Result<Value> {
        self.post(
            &format!("/{post_id}"),
            page_token,
            json!({ "message": message }),
        )
        .await
    }

    /// Delete a post.
    pub async fn delete_post(&self, post_id: &str, page_token: &str) -> Result<Value> {
        let proof = appsecret_proof(&self.cfg.app_secret, page_token);
        let url = format!(
            "{}/{}?access_token={}&appsecret_proof={}",
            self.base(),
            post_id,
            urlencode(page_token),
            proof,
        );
        let v: Value = self
            .http
            .delete(&url)
            .send()
            .await
            .map_err(|e| anyhow!("DELETE {post_id} failed: {e}"))?
            .json()
            .await
            .map_err(|e| anyhow!("DELETE {post_id} bad json: {e}"))?;
        check_api_error(v)
    }

    /// Comment on a post, or reply to a comment (same endpoint — `object_id` is a
    /// post id for a comment, a comment id for a reply).
    pub async fn create_comment(
        &self,
        object_id: &str,
        page_token: &str,
        message: &str,
    ) -> Result<Value> {
        self.post(
            &format!("/{object_id}/comments"),
            page_token,
            json!({ "message": message }),
        )
        .await
    }

    /// Like an object (post or comment).
    pub async fn like_object(&self, object_id: &str, page_token: &str) -> Result<Value> {
        self.post(&format!("/{object_id}/likes"), page_token, json!({}))
            .await
    }

    // ---- HTTP core ----

    /// GET a Graph endpoint. `access_token` is added along with appsecret_proof.
    pub async fn get(
        &self,
        path: &str,
        access_token: &str,
        extra: &[(&str, String)],
    ) -> Result<Value> {
        let proof = appsecret_proof(&self.cfg.app_secret, access_token);
        let mut url = format!(
            "{}{}?access_token={}&appsecret_proof={}",
            self.base(),
            path,
            urlencode(access_token),
            proof,
        );
        for (k, v) in extra {
            url.push('&');
            url.push_str(k);
            url.push('=');
            url.push_str(&urlencode(v));
        }
        self.get_url(&url).await
    }

    /// POST a Graph endpoint with a JSON body plus token + proof in the query.
    pub async fn post(&self, path: &str, access_token: &str, mut body: Value) -> Result<Value> {
        let proof = appsecret_proof(&self.cfg.app_secret, access_token);
        if let Value::Object(ref mut m) = body {
            m.insert("access_token".into(), json!(access_token));
            m.insert("appsecret_proof".into(), json!(proof));
        }
        let url = format!("{}{}", self.base(), path);
        let v: Value = self
            .http
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| anyhow!("POST {path} failed: {e}"))?
            .json()
            .await
            .map_err(|e| anyhow!("POST {path} bad json: {e}"))?;
        check_api_error(v)
    }

    async fn get_url(&self, url: &str) -> Result<Value> {
        let v: Value = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| anyhow!("GET failed: {e}"))?
            .json()
            .await
            .map_err(|e| anyhow!("GET bad json: {e}"))?;
        check_api_error(v)
    }
}

/// Graph API surfaces failures as `{ "error": { "message", "type", "code" } }`.
fn check_api_error(v: Value) -> Result<Value> {
    if let Some(err) = v.get("error") {
        let msg = err
            .get("message")
            .and_then(|x| x.as_str())
            .unwrap_or("unknown");
        let code = err.get("code").and_then(|x| x.as_i64()).unwrap_or(0);
        return Err(anyhow!("Facebook API error [{code}]: {msg}"));
    }
    Ok(v)
}

/// Guess an image MIME type from a filename extension (best-effort).
pub fn image_mime(filename: &str) -> &'static str {
    match filename
        .rsplit('.')
        .next()
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        _ => "application/octet-stream",
    }
}

/// Minimal percent-encoding for query values (RFC 3986 unreserved kept as-is).
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        Config {
            app_id: "1234567890".into(),
            app_secret: "s3cr3t".into(),
            version: DEFAULT_VERSION.into(),
        }
    }

    #[test]
    fn appsecret_proof_is_deterministic_hex() {
        let a = appsecret_proof("s3cr3t", "EAAToken");
        let b = appsecret_proof("s3cr3t", "EAAToken");
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        // Different token → different proof.
        assert_ne!(a, appsecret_proof("s3cr3t", "EAAOther"));
    }

    #[test]
    fn connect_url_has_required_params() {
        let url = Client::new(cfg()).connect_url("http://127.0.0.1:4590/api/oauth/callback");
        assert!(url.contains("/dialog/oauth"));
        assert!(url.contains("client_id=1234567890"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("scope=pages_show_list"));
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A4590"));
    }

    #[test]
    fn error_shape_is_surfaced() {
        let ok = json!({ "id": "123_456" });
        assert!(check_api_error(ok).is_ok());
        let bad = json!({ "error": { "message": "Invalid OAuth token", "code": 190 } });
        let e = check_api_error(bad).unwrap_err().to_string();
        assert!(e.contains("190"));
        assert!(e.contains("Invalid OAuth token"));
    }

    #[test]
    fn image_mime_from_extension() {
        assert_eq!(image_mime("a.JPG"), "image/jpeg");
        assert_eq!(image_mime("photo.png"), "image/png");
        assert_eq!(image_mime("x.webp"), "image/webp");
        assert_eq!(image_mime("noext"), "application/octet-stream");
    }

    #[test]
    fn urlencode_escapes_reserved() {
        assert_eq!(urlencode("a b&c"), "a%20b%26c");
        assert_eq!(urlencode("pages_show_list,x"), "pages_show_list%2Cx");
    }
}
