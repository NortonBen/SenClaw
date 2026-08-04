//! Typed client for the Moltbook REST API (`https://www.moltbook.com/api/v1`) —
//! the social network for AI agents. Mirrors the canonical surface documented at
//! <https://www.moltbook.com/skill.md>.
//!
//! Safety: the API key is only ever attached to requests aimed at the configured
//! base URL, which defaults to `https://www.moltbook.com`. Moltbook's own docs
//! warn: "Always use `https://www.moltbook.com` (with `www`)" and never send your
//! API key to any other domain — so [`Moltbook::new`] normalises the base and the
//! app never lets an untrusted host become the base.

use serde_json::{json, Value};
use std::time::Duration;

/// Default (and strongly recommended) Moltbook base URL. Must include `www`.
pub const DEFAULT_BASE: &str = "https://www.moltbook.com";

/// A structured Moltbook API error. `status` is the HTTP status (0 = transport
/// failure). `retry_after` is populated from the `Retry-After` header on 429s so
/// callers can back off instead of hammering.
#[derive(Debug, Clone)]
pub struct MoltError {
    pub status: u16,
    pub message: String,
    pub retry_after: Option<u64>,
}

impl std::fmt::Display for MoltError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.status == 429 {
            if let Some(ra) = self.retry_after {
                return write!(
                    f,
                    "Moltbook rate limit (429) — thử lại sau {ra}s: {}",
                    self.message
                );
            }
        }
        if self.status == 0 {
            write!(f, "Không kết nối được Moltbook: {}", self.message)
        } else {
            write!(f, "Moltbook API {}: {}", self.status, self.message)
        }
    }
}

impl MoltError {
    fn transport(msg: impl Into<String>) -> Self {
        Self {
            status: 0,
            message: msg.into(),
            retry_after: None,
        }
    }
}

pub type MoltResult = Result<Value, MoltError>;

#[derive(Clone)]
pub struct Moltbook {
    base: String,
    api_key: Option<String>,
    http: reqwest::Client,
}

// The client mirrors the full Moltbook REST surface; a few endpoints
// (delete_post, unfollow, upvote_comment, …) are part of the complete API but
// not yet wired to a route/tool.
#[allow(dead_code)]
impl Moltbook {
    /// Build a client. `base` is normalised (trailing slash trimmed); an empty
    /// base falls back to [`DEFAULT_BASE`]. Pass `None`/empty `api_key` for the
    /// unauthenticated calls (register / verify-identity).
    pub fn new(base: Option<&str>, api_key: Option<&str>) -> Self {
        let base = base
            .map(|b| b.trim().trim_end_matches('/'))
            .filter(|b| !b.is_empty())
            .unwrap_or(DEFAULT_BASE)
            .to_string();
        let api_key = api_key
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty());
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_default();
        Self {
            base,
            api_key,
            http,
        }
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    pub fn is_authenticated(&self) -> bool {
        self.api_key.is_some()
    }

    fn url(&self, path: &str) -> String {
        format!("{}/api/v1{}", self.base, path)
    }

    async fn send(&self, req: reqwest::RequestBuilder, auth: bool) -> MoltResult {
        let mut req = req;
        if auth {
            match &self.api_key {
                Some(k) => req = req.bearer_auth(k),
                None => {
                    return Err(MoltError {
                        status: 401,
                        message: "chưa cấu hình API key (đăng ký hoặc kết nối agent trước)".into(),
                        retry_after: None,
                    })
                }
            }
        }
        let resp = req
            .send()
            .await
            .map_err(|e| MoltError::transport(e.to_string()))?;
        let status = resp.status().as_u16();
        let retry_after = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.trim().parse::<u64>().ok());
        let body = resp.text().await.unwrap_or_default();
        let value: Value = serde_json::from_str(&body).unwrap_or_else(|_| json!({ "raw": body }));
        if (200..300).contains(&status) {
            Ok(value)
        } else {
            let message = value
                .get("error")
                .or_else(|| value.get("message"))
                .and_then(|m| m.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| {
                    let raw = value.get("raw").and_then(|r| r.as_str()).unwrap_or("");
                    if raw.is_empty() {
                        format!("HTTP {status}")
                    } else {
                        raw.chars().take(240).collect()
                    }
                });
            Err(MoltError {
                status,
                message,
                retry_after,
            })
        }
    }

    async fn get(&self, path: &str, auth: bool) -> MoltResult {
        self.send(self.http.get(self.url(path)), auth).await
    }
    async fn post(&self, path: &str, body: Value, auth: bool) -> MoltResult {
        self.send(self.http.post(self.url(path)).json(&body), auth)
            .await
    }
    async fn delete(&self, path: &str, auth: bool) -> MoltResult {
        self.send(self.http.delete(self.url(path)), auth).await
    }

    // ---- registration & identity (unauthenticated except where noted) ----

    /// Register a new agent. Returns `{ api_key, claim_url, verification_code, ... }`.
    pub async fn register(&self, name: &str, description: &str) -> MoltResult {
        self.post(
            "/agents/register",
            json!({ "name": name, "description": description }),
            false,
        )
        .await
    }

    /// Your own profile (name, karma, unread notifications, …).
    pub async fn me(&self) -> MoltResult {
        self.get("/agents/me", true).await
    }

    /// Claim / verification status of the agent.
    pub async fn account_status(&self) -> MoltResult {
        self.get("/agents/status", true).await
    }

    /// Another molty's public profile.
    pub async fn profile_of(&self, name: &str) -> MoltResult {
        self.get(&format!("/agents/profile?name={}", urlencode(name)), true)
            .await
    }

    /// Update your own description / metadata.
    pub async fn update_me(&self, patch: Value) -> MoltResult {
        self.send(self.http.patch(self.url("/agents/me")).json(&patch), true)
            .await
    }

    // ---- dashboard & feeds ----

    /// The `/home` dashboard — "gives you everything you need" in one call:
    /// your account, activity on your posts, follows' posts, announcements,
    /// what-to-do-next.
    pub async fn home(&self) -> MoltResult {
        self.get("/home", true).await
    }

    /// Personalised feed. `sort` ∈ hot|new|top; `filter` ∈ all|following.
    pub async fn feed(&self, sort: &str, filter: &str, cursor: Option<&str>) -> MoltResult {
        let mut q = format!(
            "/feed?sort={}&filter={}",
            urlencode(sort),
            urlencode(filter)
        );
        if let Some(c) = cursor {
            q.push_str(&format!("&cursor={}", urlencode(c)));
        }
        self.get(&q, true).await
    }

    /// The global posts feed. `sort` ∈ hot|new|top|rising. When `submolt` is set,
    /// hits the submolt's own feed instead.
    pub async fn posts(
        &self,
        sort: &str,
        submolt: Option<&str>,
        cursor: Option<&str>,
    ) -> MoltResult {
        let mut q = match submolt {
            Some(name) => format!(
                "/submolts/{}/feed?sort={}",
                urlencode(name),
                urlencode(sort)
            ),
            None => format!("/posts?sort={}", urlencode(sort)),
        };
        if let Some(c) = cursor {
            q.push_str(&format!("&cursor={}", urlencode(c)));
        }
        self.get(&q, true).await
    }

    pub async fn get_post(&self, post_id: &str) -> MoltResult {
        self.get(&format!("/posts/{}", urlencode(post_id)), true)
            .await
    }

    /// Semantic search over posts/comments. `kind` ∈ all|posts|comments.
    pub async fn search(&self, q: &str, kind: &str, limit: i64) -> MoltResult {
        self.get(
            &format!(
                "/search?q={}&type={}&limit={}",
                urlencode(q),
                urlencode(kind),
                limit
            ),
            true,
        )
        .await
    }

    pub async fn notifications(&self) -> MoltResult {
        self.get("/notifications", true).await
    }
    pub async fn read_all_notifications(&self) -> MoltResult {
        self.post("/notifications/read-all", json!({}), true).await
    }

    // ---- writes: posts, comments, votes ----

    /// Create a post. Returns `{ post: { id, verification_status, verification? } }`;
    /// when `verification_status == "pending"` the caller must solve the math
    /// challenge and call [`Moltbook::verify`].
    pub async fn create_post(
        &self,
        submolt: &str,
        title: &str,
        content: &str,
        url: Option<&str>,
        kind: &str,
    ) -> MoltResult {
        let mut body = json!({
            "submolt_name": submolt,
            "title": title,
            "content": content,
            "type": if kind.is_empty() { "text" } else { kind },
        });
        if let Some(u) = url.filter(|u| !u.is_empty()) {
            body["url"] = json!(u);
        }
        self.post("/posts", body, true).await
    }

    pub async fn delete_post(&self, post_id: &str) -> MoltResult {
        self.delete(&format!("/posts/{}", urlencode(post_id)), true)
            .await
    }

    pub async fn comments(&self, post_id: &str, sort: &str, cursor: Option<&str>) -> MoltResult {
        let mut q = format!(
            "/posts/{}/comments?sort={}",
            urlencode(post_id),
            urlencode(sort)
        );
        if let Some(c) = cursor {
            q.push_str(&format!("&cursor={}", urlencode(c)));
        }
        self.get(&q, true).await
    }

    /// Add a comment. Pass `parent_id` for a threaded reply.
    pub async fn create_comment(
        &self,
        post_id: &str,
        content: &str,
        parent_id: Option<&str>,
    ) -> MoltResult {
        let mut body = json!({ "content": content });
        if let Some(p) = parent_id.filter(|p| !p.is_empty()) {
            body["parent_id"] = json!(p);
        }
        self.post(
            &format!("/posts/{}/comments", urlencode(post_id)),
            body,
            true,
        )
        .await
    }

    pub async fn upvote_post(&self, post_id: &str) -> MoltResult {
        self.post(
            &format!("/posts/{}/upvote", urlencode(post_id)),
            json!({}),
            true,
        )
        .await
    }
    pub async fn downvote_post(&self, post_id: &str) -> MoltResult {
        self.post(
            &format!("/posts/{}/downvote", urlencode(post_id)),
            json!({}),
            true,
        )
        .await
    }
    pub async fn upvote_comment(&self, comment_id: &str) -> MoltResult {
        self.post(
            &format!("/comments/{}/upvote", urlencode(comment_id)),
            json!({}),
            true,
        )
        .await
    }

    // ---- submolts (communities) ----

    pub async fn submolts(&self, cursor: Option<&str>) -> MoltResult {
        let q = match cursor {
            Some(c) => format!("/submolts?cursor={}", urlencode(c)),
            None => "/submolts".to_string(),
        };
        self.get(&q, true).await
    }
    pub async fn submolt(&self, name: &str) -> MoltResult {
        self.get(&format!("/submolts/{}", urlencode(name)), true)
            .await
    }
    pub async fn create_submolt(
        &self,
        name: &str,
        display_name: &str,
        description: &str,
        allow_crypto: bool,
    ) -> MoltResult {
        self.post(
            "/submolts",
            json!({
                "name": name,
                "display_name": display_name,
                "description": description,
                "allow_crypto": allow_crypto,
            }),
            true,
        )
        .await
    }
    pub async fn subscribe(&self, name: &str) -> MoltResult {
        self.post(
            &format!("/submolts/{}/subscribe", urlencode(name)),
            json!({}),
            true,
        )
        .await
    }
    pub async fn unsubscribe(&self, name: &str) -> MoltResult {
        self.delete(&format!("/submolts/{}/subscribe", urlencode(name)), true)
            .await
    }

    // ---- follow ----

    pub async fn follow(&self, molty_name: &str) -> MoltResult {
        self.post(
            &format!("/agents/{}/follow", urlencode(molty_name)),
            json!({}),
            true,
        )
        .await
    }
    pub async fn unfollow(&self, molty_name: &str) -> MoltResult {
        self.delete(&format!("/agents/{}/follow", urlencode(molty_name)), true)
            .await
    }

    // ---- anti-human verification ----

    /// Submit an answer to a content-verification math challenge. `answer` is a
    /// numeric string with 2 decimal places, e.g. "15.00".
    pub async fn verify(&self, verification_code: &str, answer: &str) -> MoltResult {
        self.post(
            "/verify",
            json!({ "verification_code": verification_code, "answer": answer }),
            true,
        )
        .await
    }
}

/// Extract `(api_key, claim_url, verification_code)` from a Moltbook register
/// (or status/me) response. Tolerant of snake_case / camelCase and common
/// nesting (`agent`/`data`/`result`/`account`), and — as a last resort — scans
/// the whole payload for any claim/verify-looking URL. This is why an odd
/// response shape no longer leaves the user without a claim link.
pub fn extract_register_fields(v: &Value) -> (String, String, String) {
    let api_key = pick_str(v, &["api_key", "apiKey", "apikey", "key", "token"]).unwrap_or_default();
    let claim = pick_str(
        v,
        &[
            "claim_url",
            "claimUrl",
            "claim",
            "claim_link",
            "claimLink",
            "verification_url",
            "verificationUrl",
            "verify_url",
            "verifyUrl",
        ],
    )
    .or_else(|| find_claim_url(v))
    .unwrap_or_default();
    let code = pick_str(
        v,
        &[
            "verification_code",
            "verificationCode",
            "code",
            "verify_code",
            "verifyCode",
        ],
    )
    .unwrap_or_default();
    (api_key, claim, code)
}

/// First non-empty string among `keys`, checked at the top level and inside a
/// few common wrapper objects.
fn pick_str(v: &Value, keys: &[&str]) -> Option<String> {
    let get = |obj: &Value| -> Option<String> {
        keys.iter().find_map(|k| {
            obj.get(k)
                .and_then(|x| x.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
    };
    if let Some(s) = get(v) {
        return Some(s);
    }
    for c in ["agent", "data", "result", "account", "molty"] {
        if let Some(inner) = v.get(c) {
            if let Some(s) = get(inner) {
                return Some(s);
            }
        }
    }
    None
}

/// Walk the whole JSON and return the first http(s) string that looks like a
/// claim/verify link (or, failing that, a non-base moltbook.com URL).
pub fn find_claim_url(v: &Value) -> Option<String> {
    fn walk(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::String(s) => extract_urls_from_str(s, out),
            Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            Value::Object(o) => o.values().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    let mut urls = Vec::new();
    walk(v, &mut urls);
    urls.iter()
        .find(|u| {
            let l = u.to_lowercase();
            l.contains("claim") || l.contains("verify")
        })
        .cloned()
        .or_else(|| {
            urls.into_iter().find(|u| {
                let l = u.to_lowercase();
                l.contains("moltbook.com") && !l.trim_end_matches('/').ends_with("moltbook.com")
            })
        })
}

/// Pull every http(s) URL out of a free-text string (URLs may be embedded
/// mid-sentence, e.g. "Verify at https://… to activate"), trimming trailing
/// sentence punctuation.
fn extract_urls_from_str(s: &str, out: &mut Vec<String>) {
    let mut rest = s;
    while let Some(idx) = rest.find("http") {
        let tail = &rest[idx..];
        let end = tail
            .find(|c: char| {
                c.is_whitespace() || matches!(c, '"' | '\'' | '`' | '<' | '>' | '|' | '\\')
            })
            .unwrap_or(tail.len());
        let url = tail[..end].trim_end_matches(|c: char| {
            matches!(c, '.' | ',' | ')' | ']' | '}' | ';' | ':' | '!' | '?')
        });
        if url.starts_with("http://") || url.starts_with("https://") {
            out.push(url.to_string());
        }
        rest = &tail[end..];
    }
}

/// Percent-encode a path/query component (unreserved set per RFC 3986).
pub fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_is_normalised_and_defaults_to_www() {
        assert_eq!(Moltbook::new(None, None).base(), DEFAULT_BASE);
        assert_eq!(Moltbook::new(Some(""), None).base(), DEFAULT_BASE);
        assert_eq!(
            Moltbook::new(Some("https://www.moltbook.com/"), None).base(),
            "https://www.moltbook.com"
        );
    }

    #[test]
    fn empty_api_key_is_unauthenticated() {
        assert!(!Moltbook::new(None, Some("   ")).is_authenticated());
        assert!(Moltbook::new(None, Some("abc")).is_authenticated());
    }

    #[test]
    fn url_join() {
        let m = Moltbook::new(Some("https://www.moltbook.com"), None);
        assert_eq!(m.url("/posts"), "https://www.moltbook.com/api/v1/posts");
    }

    #[test]
    fn urlencode_escapes_reserved() {
        assert_eq!(urlencode("a b/c?d"), "a%20b%2Fc%3Fd");
        assert_eq!(urlencode("hello-world_1.0~"), "hello-world_1.0~");
    }

    #[test]
    fn rate_limit_error_mentions_retry() {
        let e = MoltError {
            status: 429,
            message: "slow down".into(),
            retry_after: Some(30),
        };
        assert!(e.to_string().contains("30s"));
    }

    #[test]
    fn extract_register_snake_and_camel() {
        let snake = json!({ "api_key": "k1", "claim_url": "https://www.moltbook.com/claim/a", "verification_code": "111" });
        assert_eq!(
            extract_register_fields(&snake),
            (
                "k1".into(),
                "https://www.moltbook.com/claim/a".into(),
                "111".into()
            )
        );
        let camel = json!({ "apiKey": "k2", "claimUrl": "https://www.moltbook.com/claim/b", "verificationCode": "222" });
        assert_eq!(
            extract_register_fields(&camel),
            (
                "k2".into(),
                "https://www.moltbook.com/claim/b".into(),
                "222".into()
            )
        );
    }

    #[test]
    fn extract_register_nested_container() {
        let nested = json!({ "agent": { "api_key": "k3", "claim_url": "https://www.moltbook.com/claim/c" } });
        let (k, claim, _) = extract_register_fields(&nested);
        assert_eq!(k, "k3");
        assert_eq!(claim, "https://www.moltbook.com/claim/c");
    }

    #[test]
    fn extract_register_url_scan_fallback() {
        // Unknown field names, but a claim URL is embedded in a message string.
        let odd = json!({ "token": "k4", "message": "Verify at https://www.moltbook.com/verify/xyz to activate" });
        let (k, claim, _) = extract_register_fields(&odd);
        assert_eq!(k, "k4");
        assert_eq!(claim, "https://www.moltbook.com/verify/xyz");
    }

    #[test]
    fn find_claim_url_ignores_bare_base() {
        // The base URL alone must NOT be mistaken for a claim link.
        assert_eq!(
            find_claim_url(&json!({ "url": "https://www.moltbook.com" })),
            None
        );
        assert_eq!(
            find_claim_url(&json!({ "site": "https://www.moltbook.com/" })),
            None
        );
        assert_eq!(
            find_claim_url(&json!({ "x": "https://www.moltbook.com/m/general" })),
            Some("https://www.moltbook.com/m/general".into())
        );
    }
}
